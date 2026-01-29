use anyhow::{anyhow, Context, Result};
use quick_xml::events::{BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use tracing::info;
use whisper_rs::{get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::data;
use crate::ocr;
use crate::providers::{Provider, ProviderUsage, ToolSpec};
use crate::{TranslateOptions, Translator};

pub struct AttachmentTranslation {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub model: Option<String>,
    pub usage: Option<ProviderUsage>,
}

const OCR_NORMALIZE_TOOL: &str = "normalize_ocr";
const OCR_ROMANIZE_TOOL: &str = "romanize_ocr";

pub async fn translate_attachment<P: Provider + Clone>(
    data: &data::DataAttachment,
    ocr_languages: &str,
    translator: &Translator<P>,
    options: &TranslateOptions,
    debug_ocr: bool,
    debug_src: Option<&Path>,
) -> Result<Option<AttachmentTranslation>> {
    match data.mime.as_str() {
        mime if mime.starts_with("image/") => {
            info!("attachment: image (mime={})", mime);
            let mut cache = TranslationCache::new();
            let input_mime = data::sniff_mime(&data.bytes).unwrap_or_else(|| data.mime.clone());
            let debug = if debug_ocr {
                Some(build_ocr_debug_config(debug_src, data.name.as_deref())?)
            } else {
                None
            };
            let output = translate_image_with_cache(
                ImageTranslateRequest {
                    image_bytes: &data.bytes,
                    image_mime: &input_mime,
                    output_mime: &data.mime,
                    ocr_languages,
                    allow_empty: false,
                    debug,
                },
                &mut cache,
                translator,
                options,
            )
            .await?;
            return Ok(Some(cache.finish(data.mime.clone(), output)));
        }
        data::DOCX_MIME => {
            info!("attachment: docx");
            let output =
                translate_office_zip(&data.bytes, OfficeKind::Docx, translator, options).await?;
            return Ok(Some(output));
        }
        data::PPTX_MIME => {
            info!("attachment: pptx");
            let output =
                translate_office_zip(&data.bytes, OfficeKind::Pptx, translator, options).await?;
            return Ok(Some(output));
        }
        data::XLSX_MIME => {
            info!("attachment: xlsx");
            let output =
                translate_office_zip(&data.bytes, OfficeKind::Xlsx, translator, options).await?;
            return Ok(Some(output));
        }
        data::PDF_MIME => {
            info!("attachment: pdf");
            let debug = if debug_ocr {
                Some(build_ocr_debug_config(debug_src, data.name.as_deref())?)
            } else {
                None
            };
            let output =
                translate_pdf(&data.bytes, ocr_languages, translator, options, debug).await?;
            return Ok(Some(output));
        }
        mime if mime.starts_with("audio/") => {
            info!("attachment: audio ({})", mime);
            let output = translate_audio(data, translator, options).await?;
            return Ok(Some(output));
        }
        data::TEXT_MIME => {
            info!("attachment: text");
            let text = std::str::from_utf8(&data.bytes)
                .with_context(|| "failed to decode text file as UTF-8")?;
            let exec = translator.exec(text, options.clone()).await?;
            let bytes = exec.text.into_bytes();
            let output = AttachmentTranslation {
                bytes,
                mime: data::TEXT_MIME.to_string(),
                model: exec.model,
                usage: exec.usage,
            };
            return Ok(Some(output));
        }
        _ => {}
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy)]
enum OfficeKind {
    Docx,
    Pptx,
    Xlsx,
}

async fn translate_office_zip<P: Provider + Clone>(
    bytes: &[u8],
    kind: OfficeKind,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).with_context(|| "failed to read zip archive")?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut cache = TranslationCache::new();

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .with_context(|| "failed to read zip entry")?;
        let name = file.name().to_string();
        let file_options = FileOptions::default().compression_method(file.compression());
        if file.is_dir() {
            writer
                .add_directory(name, file_options)
                .with_context(|| "failed to write zip directory")?;
            continue;
        }

        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .with_context(|| "failed to read zip entry content")?;
        drop(file);

        let output = if should_translate_office_entry(kind, &name) {
            match kind {
                OfficeKind::Docx => {
                    translate_docx_xml(&data, &mut cache, translator, options).await?
                }
                OfficeKind::Pptx => {
                    translate_pptx_xml(&data, &mut cache, translator, options).await?
                }
                OfficeKind::Xlsx => {
                    translate_xlsx_xml(&data, &mut cache, translator, options).await?
                }
            }
        } else {
            data
        };

        writer
            .start_file(name, file_options)
            .with_context(|| "failed to write zip entry")?;
        writer
            .write_all(&output)
            .with_context(|| "failed to write zip content")?;
    }

    let bytes = writer
        .finish()
        .with_context(|| "failed to finalize zip output")?
        .into_inner();
    Ok(cache.finish(kind.mime().to_string(), bytes))
}

fn should_translate_office_entry(kind: OfficeKind, name: &str) -> bool {
    if !name.ends_with(".xml") {
        return false;
    }
    match kind {
        OfficeKind::Docx => name.starts_with("word/"),
        OfficeKind::Pptx => name.starts_with("ppt/"),
        OfficeKind::Xlsx => name.starts_with("xl/"),
    }
}

impl OfficeKind {
    fn mime(&self) -> &'static str {
        match self {
            OfficeKind::Docx => data::DOCX_MIME,
            OfficeKind::Pptx => data::PPTX_MIME,
            OfficeKind::Xlsx => data::XLSX_MIME,
        }
    }
}

async fn translate_docx_xml<P: Provider + Clone>(
    xml: &[u8],
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Vec<u8>> {
    translate_xml_simple(xml, cache, translator, options, b"w:t").await
}

async fn translate_pptx_xml<P: Provider + Clone>(
    xml: &[u8],
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Vec<u8>> {
    translate_xml_simple(xml, cache, translator, options, b"a:t").await
}

async fn translate_xlsx_xml<P: Provider + Clone>(
    xml: &[u8],
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();
    let mut in_text = false;
    let mut in_si = 0usize;
    let mut in_is = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"si" {
                    in_si += 1;
                } else if e.name().as_ref() == b"is" {
                    in_is += 1;
                } else if e.name().as_ref() == b"t" && (in_si > 0 || in_is > 0) {
                    in_text = true;
                }
                writer.write_event(Event::Start(e.to_owned()))?;
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"t" {
                    in_text = false;
                } else if e.name().as_ref() == b"si" {
                    in_si = in_si.saturating_sub(1);
                } else if e.name().as_ref() == b"is" {
                    in_is = in_is.saturating_sub(1);
                }
                writer.write_event(Event::End(e.to_owned()))?;
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    let text = e.unescape()?.into_owned();
                    let translated = cache
                        .translate_preserve_whitespace(&text, translator, options)
                        .await?;
                    let output = BytesText::new(&translated);
                    writer.write_event(Event::Text(output))?;
                } else {
                    writer.write_event(Event::Text(e))?;
                }
            }
            Ok(Event::CData(e)) => {
                if in_text {
                    let raw = e.into_inner();
                    let text = String::from_utf8_lossy(raw.as_ref()).into_owned();
                    let translated = cache
                        .translate_preserve_whitespace(&text, translator, options)
                        .await?;
                    let output = BytesText::new(&translated);
                    writer.write_event(Event::Text(output))?;
                } else {
                    writer.write_event(Event::CData(e))?;
                }
            }
            Ok(Event::Eof) => break,
            Ok(event) => {
                writer.write_event(event)?;
            }
            Err(err) => {
                return Err(anyhow!("failed to parse xlsx xml: {}", err));
            }
        }
        buf.clear();
    }
    Ok(writer.into_inner())
}

async fn translate_xml_simple<P: Provider + Clone>(
    xml: &[u8],
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
    tag_name: &[u8],
) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == tag_name {
                    in_text = true;
                }
                writer.write_event(Event::Start(e.to_owned()))?;
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == tag_name {
                    in_text = false;
                }
                writer.write_event(Event::End(e.to_owned()))?;
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    let text = e.unescape()?.into_owned();
                    let translated = cache
                        .translate_preserve_whitespace(&text, translator, options)
                        .await?;
                    let output = BytesText::new(&translated);
                    writer.write_event(Event::Text(output))?;
                } else {
                    writer.write_event(Event::Text(e))?;
                }
            }
            Ok(Event::CData(e)) => {
                if in_text {
                    let raw = e.into_inner();
                    let text = String::from_utf8_lossy(raw.as_ref()).into_owned();
                    let translated = cache
                        .translate_preserve_whitespace(&text, translator, options)
                        .await?;
                    let output = BytesText::new(&translated);
                    writer.write_event(Event::Text(output))?;
                } else {
                    writer.write_event(Event::CData(e))?;
                }
            }
            Ok(Event::Eof) => break,
            Ok(event) => {
                writer.write_event(event)?;
            }
            Err(err) => {
                return Err(anyhow!("failed to parse xml: {}", err));
            }
        }
        buf.clear();
    }
    Ok(writer.into_inner())
}

struct ImageTranslateRequest<'a> {
    image_bytes: &'a [u8],
    image_mime: &'a str,
    output_mime: &'a str,
    ocr_languages: &'a str,
    allow_empty: bool,
    debug: Option<OcrDebugConfig>,
}

#[derive(Debug, Clone)]
struct AnnotationEntry {
    id: usize,
    original: String,
    reading: Option<String>,
    translated: String,
}

async fn translate_image_with_cache<P: Provider + Clone>(
    request: ImageTranslateRequest<'_>,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Vec<u8>> {
    let mut ocr_result = ocr::extract_lines(request.image_bytes, request.ocr_languages)?;
    let mut reading_map: HashMap<usize, String> = HashMap::new();
    if should_filter_by_source_lang(&options.source_lang) {
        ocr_result
            .lines
            .retain(|line| should_keep_cjk_line(&line.text));
    }
    if ocr_result.lines.is_empty() {
        if request.allow_empty {
            return Ok(request.image_bytes.to_vec());
        }
        return Err(anyhow!("no text found in image"));
    }

    if let Some(debug) = request.debug.as_ref() {
        let label = debug.page_label(None);
        let svg = ocr::render_bbox_svg(
            request.image_bytes,
            request.image_mime,
            ocr_result.width,
            ocr_result.height,
            &ocr_result.lines,
        )?;
        let bytes = ocr::render_svg_bytes(&svg, "image/png", None)?;
        let output_path = debug.output_path(&label);
        std::fs::write(&output_path, bytes).with_context(|| {
            format!("failed to write ocr debug image: {}", output_path.display())
        })?;
        let json_path = debug.json_path(&label);
        let json = serde_json::to_vec_pretty(&ocr_result.lines)?;
        std::fs::write(&json_path, json)
            .with_context(|| format!("failed to write ocr debug json: {}", json_path.display()))?;
        eprintln!("debug: wrote ocr bbox {}", output_path.display());
    }

    if translator.settings().ocr_normalize {
        let normalize_request = OcrNormalizeRequest {
            image_bytes: request.image_bytes,
            image_mime: request.image_mime,
            width: ocr_result.width,
            height: ocr_result.height,
            lines: &ocr_result.lines,
        };
        match normalize_ocr_lines_with_llm(normalize_request, translator, options, cache).await {
            Ok(outcome) => {
                ocr_result.lines = outcome.lines;
                reading_map = outcome.readings;
            }
            Err(err) => {
                eprintln!("warning: failed to normalize ocr lines: {}", err);
            }
        }
    }

    let mut translated_lines = Vec::new();
    let mut entries = Vec::new();
    let mut number_map: HashMap<String, usize> = HashMap::new();
    let mut lines_sorted: Vec<(usize, ocr::OcrLine)> =
        ocr_result.lines.iter().cloned().enumerate().collect();
    lines_sorted.sort_by_key(|(_, line)| (line.bbox.y, line.bbox.x));
    if !reading_map.is_empty() {
        reading_map.retain(|_, reading| is_latin_reading(reading));
    }
    let mut romanize_targets = Vec::new();
    for (idx, line) in &lines_sorted {
        let cleaned = sanitize_ocr_text(&collapse_whitespace(&line.text));
        if should_skip_ocr_annotation(&cleaned) {
            continue;
        }
        if reading_map.contains_key(idx) {
            continue;
        }
        if contains_non_latin_script(&cleaned) {
            romanize_targets.push((*idx, cleaned.clone()));
        }
    }
    if !romanize_targets.is_empty() {
        match romanize_lines_with_llm(&romanize_targets, translator, options, cache).await {
            Ok(map) => {
                for (id, value) in map {
                    reading_map.entry(id).or_insert(value);
                }
            }
            Err(err) => {
                eprintln!("warning: failed to romanize readings: {}", err);
            }
        }
    }
    for (idx, line) in &lines_sorted {
        let cleaned = sanitize_ocr_text(&collapse_whitespace(&line.text));
        if should_skip_ocr_annotation(&cleaned) {
            continue;
        }
        let translated = cache
            .translate_ocr_line(&line.text, translator, options)
            .await?;
        let key = translated.trim().to_string();
        let id = if let Some(&existing) = number_map.get(&key) {
            existing
        } else {
            let next_id = number_map.len() + 1;
            number_map.insert(key.clone(), next_id);
            next_id
        };
        entries.push(AnnotationEntry {
            id,
            original: cleaned.trim().to_string(),
            reading: reading_map.get(idx).cloned(),
            translated: key,
        });
        translated_lines.push(ocr::TranslatedLine {
            text: format!("({})", id),
            bbox: line.bbox.clone(),
            font_size: line.font_size,
        });
    }

    let font_path = translator
        .settings()
        .overlay_font_path
        .as_deref()
        .map(Path::new);
    let font_family = translator.settings().overlay_font_family.as_deref();
    let fallback_fonts = overlay_fallback_fonts();
    let resolved_font = ocr::resolve_overlay_font(font_path, font_family, fallback_fonts)?;
    let overlay = ocr::OverlayStyle {
        text_color: translator.settings().overlay_text_color.clone(),
        stroke_color: translator.settings().overlay_stroke_color.clone(),
        fill_color: translator.settings().overlay_fill_color.clone(),
        font_size: translator.settings().overlay_font_size,
        font_family: Some(resolved_font.family.clone()),
        font_metrics: Some(resolved_font.metrics.clone()),
    };
    let footer_lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            if let Some(reading) = entry.reading.as_ref().filter(|value| !value.is_empty()) {
                format!(
                    "({}) {} ({}) : {}",
                    entry.id, entry.original, reading, entry.translated
                )
            } else {
                format!("({}) {}: {}", entry.id, entry.original, entry.translated)
            }
        })
        .collect();
    let outcome = ocr::render_svg(
        request.image_bytes,
        request.image_mime,
        ocr_result.width,
        ocr_result.height,
        &translated_lines,
        &overlay,
        Some(&footer_lines),
    )?;
    let bytes = ocr::render_svg_bytes(
        &outcome.svg,
        request.output_mime,
        overlay.font_metrics.as_ref().map(|metrics| metrics.data()),
    )?;
    Ok(bytes)
}

async fn translate_pdf<P: Provider + Clone>(
    pdf_bytes: &[u8],
    ocr_languages: &str,
    translator: &Translator<P>,
    options: &TranslateOptions,
    debug: Option<OcrDebugConfig>,
) -> Result<AttachmentTranslation> {
    let pages = render_pdf_pages(pdf_bytes)?;
    if pages.is_empty() {
        return Err(anyhow!("no pages found in pdf"));
    }

    let mut cache = TranslationCache::new();
    let mut translated_images = Vec::new();
    for (index, page) in pages.into_iter().enumerate() {
        let debug_page = debug.as_ref().map(|config| OcrDebugConfig {
            output_dir: config.output_dir.clone(),
            base_name: config.page_label(Some(index)),
        });
        let output = translate_image_with_cache(
            ImageTranslateRequest {
                image_bytes: &page,
                image_mime: "image/png",
                output_mime: "image/png",
                ocr_languages,
                allow_empty: true,
                debug: debug_page,
            },
            &mut cache,
            translator,
            options,
        )
        .await?;
        translated_images.push(output);
    }

    let pdf = images_to_pdf(&translated_images)?;
    Ok(cache.finish(data::PDF_MIME.to_string(), pdf))
}

fn render_pdf_pages(pdf_bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let dir = tempdir().with_context(|| "failed to create temp dir for pdf")?;
    let input_path = dir.path().join("input.pdf");
    fs::write(&input_path, pdf_bytes).with_context(|| "failed to write temp pdf")?;

    if command_exists("mutool") {
        let output = Command::new("mutool")
            .arg("draw")
            .arg("-r")
            .arg("200")
            .arg("-o")
            .arg(dir.path().join("page-%03d.png"))
            .arg(&input_path)
            .output()
            .with_context(|| "failed to run mutool draw")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("mutool draw failed: {}", stderr.trim()));
        }
        return read_sorted_pngs(dir.path());
    }

    if command_exists("pdftoppm") {
        let output = Command::new("pdftoppm")
            .arg("-r")
            .arg("200")
            .arg("-png")
            .arg(&input_path)
            .arg(dir.path().join("page"))
            .output()
            .with_context(|| "failed to run pdftoppm")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("pdftoppm failed: {}", stderr.trim()));
        }
        return read_sorted_pngs(dir.path());
    }

    Err(anyhow!(
        "pdf rendering requires mutool or pdftoppm (install mupdf or poppler)"
    ))
}

fn read_sorted_pngs(dir: &std::path::Path) -> Result<Vec<Vec<u8>>> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| "failed to read temp pdf directory")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(page_index_from_name)
            .unwrap_or(u32::MAX)
    });

    let mut pages = Vec::new();
    for entry in entries {
        let bytes = fs::read(entry.path()).with_context(|| "failed to read rendered pdf page")?;
        pages.push(bytes);
    }
    Ok(pages)
}

fn command_exists(cmd: &str) -> bool {
    match Command::new(cmd).arg("-h").output() {
        Ok(_) => true,
        Err(err) => err.kind() != std::io::ErrorKind::NotFound,
    }
}

fn page_index_from_name(name: &str) -> Option<u32> {
    let mut digits = String::new();
    for ch in name.chars().rev() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    let value: String = digits.chars().rev().collect();
    value.parse::<u32>().ok()
}

fn images_to_pdf(pages: &[Vec<u8>]) -> Result<Vec<u8>> {
    use printpdf::{Image, ImageTransform, Mm, PdfDocument};

    let mut doc = None;
    let mut layers = Vec::new();

    for (idx, bytes) in pages.iter().enumerate() {
        let image = printpdf::image_crate::load_from_memory(bytes)
            .with_context(|| "failed to decode rendered pdf page")?;
        let width = image.width();
        let height = image.height();
        let width_mm = px_to_mm(width);
        let height_mm = px_to_mm(height);

        if idx == 0 {
            let (doc_handle, page, layer) =
                PdfDocument::new("translated", Mm(width_mm), Mm(height_mm), "Layer 1");
            doc = Some(doc_handle);
            layers.push((page, layer, image));
        } else if let Some(doc_handle) = doc.as_mut() {
            let (page, layer) =
                doc_handle.add_page(Mm(width_mm), Mm(height_mm), format!("Layer {}", idx + 1));
            layers.push((page, layer, image));
        }
    }

    let doc = doc.ok_or_else(|| anyhow!("no pages to render"))?;
    for (page, layer, image) in layers.into_iter() {
        let current_layer = doc.get_page(page).get_layer(layer);
        let pdf_image = Image::from_dynamic_image(&image);
        let transform = ImageTransform {
            translate_x: Some(Mm(0.0)),
            translate_y: Some(Mm(0.0)),
            rotate: None,
            scale_x: Some(1.0),
            scale_y: Some(1.0),
            dpi: Some(72.0),
        };
        pdf_image.add_to_layer(current_layer, transform);
    }

    let mut buffer = Vec::new();
    {
        let mut writer = std::io::BufWriter::new(&mut buffer);
        doc.save(&mut writer)
            .with_context(|| "failed to write pdf")?;
    }
    Ok(buffer)
}

fn px_to_mm(px: u32) -> f32 {
    let inches = px as f32 / 72.0;
    inches * 25.4
}

async fn translate_audio<P: Provider + Clone>(
    data: &data::DataAttachment,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    ensure_command("ffmpeg", "audio translation requires ffmpeg")?;
    info!("audio: decoding with ffmpeg");

    let dir = tempdir().with_context(|| "failed to create temp dir for audio")?;
    let input_ext = data::extension_from_mime(&data.mime).unwrap_or("bin");
    let input_path = dir.path().join(format!("input.{}", input_ext));
    fs::write(&input_path, &data.bytes).with_context(|| "failed to write audio input")?;

    let wav_path = dir.path().join("input.wav");
    run_ffmpeg(&[
        "-y",
        "-i",
        input_path.to_string_lossy().as_ref(),
        "-ar",
        "16000",
        "-ac",
        "1",
        wav_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to decode audio with ffmpeg")?;

    let transcript = transcribe_audio(&wav_path, &options.source_lang).await?;
    let transcript = transcript.trim();
    if transcript.is_empty() {
        return Err(anyhow!("no speech detected in audio"));
    }

    info!("audio: transcribed {} chars", transcript.chars().count());
    let exec = translator.exec(transcript, options.clone()).await?;
    let translated = exec.text.trim();
    if translated.is_empty() {
        return Err(anyhow!("translation returned empty text"));
    }

    let tts_wav = dir.path().join("tts.wav");
    info!("audio: synthesizing speech");
    synthesize_speech(translated, &options.lang, &tts_wav)?;

    let out_ext = data::extension_from_mime(&data.mime).unwrap_or("mp3");
    let output_path = dir.path().join(format!("output.{}", out_ext));
    run_ffmpeg(&[
        "-y",
        "-i",
        tts_wav.to_string_lossy().as_ref(),
        output_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to encode translated audio")?;

    let bytes = fs::read(&output_path).with_context(|| "failed to read translated audio")?;

    Ok(AttachmentTranslation {
        bytes,
        mime: data.mime.clone(),
        model: exec.model,
        usage: exec.usage,
    })
}

async fn transcribe_audio(wav_path: &Path, source_lang: &str) -> Result<String> {
    let forced_lang = resolve_forced_lang(source_lang);
    let outcome = transcribe_audio_with_params(wav_path, forced_lang.as_deref(), false).await?;
    if !outcome.text.trim().is_empty() {
        return Ok(outcome.text);
    }
    if forced_lang.is_none() {
        if let Some(detected) = outcome.detected_lang.as_deref() {
            let retry = transcribe_audio_with_params(wav_path, Some(detected), true).await?;
            if !retry.text.trim().is_empty() {
                return Ok(retry.text);
            }
        }
    }

    info!("audio: no speech detected, retrying with normalization");
    let dir = wav_path
        .parent()
        .ok_or_else(|| anyhow!("invalid wav path"))?;
    let normalized_path = dir.join("input_norm.wav");
    run_ffmpeg(&[
        "-y",
        "-i",
        wav_path.to_string_lossy().as_ref(),
        "-af",
        "dynaudnorm",
        normalized_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to normalize audio")?;

    let outcome = transcribe_audio_with_params(&normalized_path, forced_lang.as_deref(), true).await?;
    if !outcome.text.trim().is_empty() {
        return Ok(outcome.text);
    }
    if forced_lang.is_none() {
        if let Some(detected) = outcome.detected_lang.as_deref() {
            let retry = transcribe_audio_with_params(&normalized_path, Some(detected), true).await?;
            if !retry.text.trim().is_empty() {
                return Ok(retry.text);
            }
        }
    }

    info!("audio: still empty, retrying with normalization + gain");
    let boosted_path = dir.join("input_boost.wav");
    run_ffmpeg(&[
        "-y",
        "-i",
        wav_path.to_string_lossy().as_ref(),
        "-af",
        "dynaudnorm,volume=6",
        boosted_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to normalize audio with gain")?;

    let outcome = transcribe_audio_with_params(&boosted_path, forced_lang.as_deref(), true).await?;
    if !outcome.text.trim().is_empty() {
        return Ok(outcome.text);
    }
    if forced_lang.is_none() {
        if let Some(detected) = outcome.detected_lang.as_deref() {
            let retry = transcribe_audio_with_params(&boosted_path, Some(detected), true).await?;
            if !retry.text.trim().is_empty() {
                return Ok(retry.text);
            }
        }
    }

    Ok(outcome.text)
}

struct TranscribeOutcome {
    text: String,
    detected_lang: Option<String>,
}

async fn transcribe_audio_with_params(
    wav_path: &Path,
    forced_lang: Option<&str>,
    relaxed: bool,
) -> Result<TranscribeOutcome> {
    let model = whisper_model_path().await?;
    let audio = read_wav_mono_f32(wav_path)?;

    let model_path = model.to_string_lossy();
    let ctx =
        WhisperContext::new_with_params(model_path.as_ref(), WhisperContextParameters::default())
            .with_context(|| "failed to load whisper model")?;
    let mut state = ctx
        .create_state()
        .with_context(|| "failed to init whisper state")?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(num_cpus::get() as i32);
    params.set_translate(false);
    if relaxed {
        params.set_suppress_blank(false);
        params.set_suppress_non_speech_tokens(false);
        params.set_no_speech_thold(1.0);
        params.set_logprob_thold(-5.0);
        params.set_temperature(0.4);
        params.set_temperature_inc(0.2);
        params.set_no_timestamps(true);
        params.set_single_segment(true);
    }
    if let Some(lang) = forced_lang {
        params.set_language(Some(lang));
    } else {
        params.set_detect_language(true);
    }

    state
        .full(params, &audio[..])
        .with_context(|| "whisper transcription failed")?;

    let detected_lang = state
        .full_lang_id_from_state()
        .ok()
        .and_then(|id| get_lang_str(id))
        .map(|value: &str| value.to_string());
    let num_segments = state
        .full_n_segments()
        .with_context(|| "failed to read segments")?;
    let mut parts = Vec::new();
    for idx in 0..num_segments {
        let text = state
            .full_get_segment_text(idx)
            .with_context(|| "failed to read segment text")?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    Ok(TranscribeOutcome {
        text: parts.join(" "),
        detected_lang,
    })
}

fn resolve_forced_lang(source_lang: &str) -> Option<String> {
    if source_lang.trim().is_empty() || source_lang.eq_ignore_ascii_case("auto") {
        return None;
    }
    map_lang_for_whisper(source_lang).map(|value| value.to_string())
}

fn synthesize_speech(text: &str, target_lang: &str, out_wav: &Path) -> Result<()> {
    let text = text.replace('\n', " ");
    if command_exists("say") {
        let aiff_path = out_wav.with_extension("aiff");
        let status = Command::new("say")
            .arg("-o")
            .arg(&aiff_path)
            .arg(&text)
            .status()
            .with_context(|| "failed to run say")?;
        if !status.success() {
            return Err(anyhow!("say failed to synthesize audio"));
        }
        run_ffmpeg(&[
            "-y",
            "-i",
            aiff_path.to_string_lossy().as_ref(),
            out_wav.to_string_lossy().as_ref(),
        ])
        .with_context(|| "failed to convert say output")?;
        return Ok(());
    }

    if command_exists("espeak") {
        let voice = map_lang_for_espeak(target_lang).unwrap_or("en");
        let status = Command::new("espeak")
            .arg("-v")
            .arg(voice)
            .arg("-w")
            .arg(out_wav)
            .arg(&text)
            .status()
            .with_context(|| "failed to run espeak")?;
        if !status.success() {
            return Err(anyhow!("espeak failed to synthesize audio"));
        }
        return Ok(());
    }

    Err(anyhow!(
        "no TTS engine found (install macOS 'say' or Linux 'espeak')"
    ))
}

fn ensure_command(cmd: &str, message: &str) -> Result<()> {
    if command_exists(cmd) {
        Ok(())
    } else {
        Err(anyhow!("{}", message))
    }
}

fn run_ffmpeg(args: &[&str]) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .with_context(|| "failed to run ffmpeg")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed: {}", stderr.trim()));
    }
    Ok(())
}

fn map_lang_for_whisper(lang: &str) -> Option<&'static str> {
    match lang.trim().to_lowercase().as_str() {
        "ja" | "jpn" => Some("ja"),
        "en" | "eng" => Some("en"),
        "zh" | "zho" | "zho-hans" | "zho-hant" => Some("zh"),
        "ko" | "kor" => Some("ko"),
        "fr" | "fra" => Some("fr"),
        "es" | "spa" => Some("es"),
        "de" | "deu" => Some("de"),
        "it" | "ita" => Some("it"),
        "pt" | "por" => Some("pt"),
        "ru" | "rus" => Some("ru"),
        "nl" | "nld" => Some("nl"),
        "sv" | "swe" => Some("sv"),
        "no" | "nor" => Some("no"),
        "da" | "dan" => Some("da"),
        "fi" | "fin" => Some("fi"),
        "pl" | "pol" => Some("pl"),
        "cs" | "ces" => Some("cs"),
        "el" | "ell" => Some("el"),
        "tr" | "tur" => Some("tr"),
        "ar" | "ara" => Some("ar"),
        "hi" | "hin" => Some("hi"),
        "id" | "ind" => Some("id"),
        "vi" | "vie" => Some("vi"),
        "th" | "tha" => Some("th"),
        _ => None,
    }
}

const WHISPER_MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

async fn whisper_model_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("LLM_TRANSLATOR_WHISPER_MODEL") {
        let path = path.trim();
        if !path.is_empty() {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
            if let Some(model) = normalize_model_name(path.to_string_lossy().as_ref()) {
                return ensure_whisper_model(&model).await;
            }
        }
    }
    if let Ok(path) = std::env::var("WHISPER_CPP_MODEL") {
        let path = path.trim();
        if !path.is_empty() {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
            if let Some(model) = normalize_model_name(path.to_string_lossy().as_ref()) {
                return ensure_whisper_model(&model).await;
            }
        }
    }

    ensure_whisper_model("base").await
}

async fn ensure_whisper_model(model: &str) -> Result<PathBuf> {
    let normalized = normalize_model_name(model).unwrap_or_else(|| "base".to_string());
    let dest = default_model_path(&normalized)?;
    if dest.exists() {
        return Ok(dest);
    }

    let url = whisper_model_url(&normalized)?;
    info!("whisper model not found; downloading {} ...", normalized);
    download_whisper_model(&url, &dest).await?;
    Ok(dest)
}

fn default_model_path(model: &str) -> Result<PathBuf> {
    let file = format!("ggml-{}.bin", model);
    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim();
        if !home.is_empty() {
            return Ok(Path::new(home)
                .join(".llm-translator-rust/.cache/whisper")
                .join(file));
        }
    }
    Ok(Path::new(".llm-translator-rust/.cache/whisper").join(file))
}

fn whisper_model_url(model: &str) -> Result<String> {
    let file = format!("ggml-{}.bin", model);
    Ok(format!("{}/{}", WHISPER_MODEL_BASE_URL, file))
}

fn normalize_model_name(input: &str) -> Option<String> {
    let raw = input.trim().to_lowercase();
    if raw.is_empty() {
        return None;
    }
    let trimmed = raw
        .strip_prefix("ggml-")
        .unwrap_or(raw.as_str())
        .strip_suffix(".bin")
        .unwrap_or(raw.as_str());

    let allowed = [
        "tiny",
        "base",
        "small",
        "medium",
        "large",
        "large-v2",
        "large-v3",
        "tiny.en",
        "base.en",
        "small.en",
        "medium.en",
    ];
    if allowed.contains(&trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

async fn download_whisper_model(url: &str, dest: &Path) -> Result<()> {
    let dir = dest.parent().ok_or_else(|| anyhow!("invalid model path"))?;
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create model dir: {}", dir.display()))?;

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("failed to download whisper model: {}", url))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "failed to download whisper model: {} (status {})",
            url,
            response.status()
        ));
    }

    let tmp = dest.with_extension("bin.part");
    let mut file = fs::File::create(&tmp)
        .with_context(|| format!("failed to write model: {}", tmp.display()))?;
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| "failed to read model bytes")?;
        std::io::Write::write_all(&mut file, &chunk)?;
    }
    fs::rename(&tmp, dest)
        .with_context(|| format!("failed to finalize model: {}", dest.display()))?;
    Ok(())
}

fn read_wav_mono_f32(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open wav: {}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    if channels == 0 {
        return Err(anyhow!("wav has no channels"));
    }

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max = (1i64 << (bits - 1)) as f32;
            if bits <= 16 {
                reader
                    .samples::<i16>()
                    .map(|s| s.unwrap_or(0) as f32 / max)
                    .collect()
            } else {
                reader
                    .samples::<i32>()
                    .map(|s| s.unwrap_or(0) as f32 / max)
                    .collect()
            }
        }
    };

    if channels == 1 {
        return Ok(samples);
    }

    let mut mono = Vec::with_capacity(samples.len() / channels);
    for chunk in samples.chunks(channels) {
        let sum: f32 = chunk.iter().sum();
        mono.push(sum / channels as f32);
    }
    Ok(mono)
}

fn map_lang_for_espeak(lang: &str) -> Option<&'static str> {
    match lang.trim().to_lowercase().as_str() {
        "ja" | "jpn" => Some("ja"),
        "en" | "eng" => Some("en"),
        "zh" | "zho" | "zho-hans" | "zho-hant" => Some("zh"),
        "ko" | "kor" => Some("ko"),
        "fr" | "fra" => Some("fr"),
        "es" | "spa" => Some("es"),
        "de" | "deu" => Some("de"),
        "it" | "ita" => Some("it"),
        "pt" | "por" => Some("pt"),
        "ru" | "rus" => Some("ru"),
        "nl" | "nld" => Some("nl"),
        "sv" | "swe" => Some("sv"),
        "no" | "nor" => Some("no"),
        "da" | "dan" => Some("da"),
        "fi" | "fin" => Some("fi"),
        "pl" | "pol" => Some("pl"),
        "cs" | "ces" => Some("cs"),
        "el" | "ell" => Some("el"),
        "tr" | "tur" => Some("tr"),
        "ar" | "ara" => Some("ar"),
        "hi" | "hin" => Some("hi"),
        "id" | "ind" => Some("id"),
        "vi" | "vie" => Some("vi"),
        "th" | "tha" => Some("th"),
        _ => None,
    }
}

struct TranslationCache {
    map: HashMap<String, String>,
    model: Option<String>,
    usage: ProviderUsage,
    used: bool,
}

#[derive(Debug, Clone)]
struct OcrDebugConfig {
    output_dir: PathBuf,
    base_name: String,
}

impl OcrDebugConfig {
    fn page_label(&self, page: Option<usize>) -> String {
        if let Some(index) = page {
            format!("{}_page{:02}", self.base_name, index + 1)
        } else {
            self.base_name.clone()
        }
    }

    fn output_path(&self, label: &str) -> PathBuf {
        self.output_dir.join(format!("{}_ocr_bbox.png", label))
    }

    fn json_path(&self, label: &str) -> PathBuf {
        self.output_dir.join(format!("{}_ocr.json", label))
    }
}

fn build_ocr_debug_config(src_path: Option<&Path>, name: Option<&str>) -> Result<OcrDebugConfig> {
    let (dir, base) = if let Some(path) = src_path {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let base = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("input");
        (dir.to_path_buf(), base.to_string())
    } else if let Some(name) = name {
        let base = Path::new(name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("input")
            .to_string();
        (default_debug_dir()?, base)
    } else {
        (default_debug_dir()?, "stdin".to_string())
    };

    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create debug dir: {}", dir.display()))?;
    Ok(OcrDebugConfig {
        output_dir: dir,
        base_name: sanitize_filename_component(&base),
    })
}

fn default_debug_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Ok(Path::new(&home).join(".llm-translator-rust/.cache/ocr"));
        }
    }
    Ok(Path::new(".llm-translator-rust/.cache/ocr").to_path_buf())
}

fn sanitize_filename_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        "input".to_string()
    } else {
        out
    }
}

#[cfg(target_os = "macos")]
fn overlay_fallback_fonts() -> &'static [&'static str] {
    &["NotoSans", "Hiragino Sans", "sans-serif"]
}

#[cfg(target_os = "windows")]
fn overlay_fallback_fonts() -> &'static [&'static str] {
    &["NotoSans", "Arial Unicode", "sans-serif"]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn overlay_fallback_fonts() -> &'static [&'static str] {
    &["NotoSans", "sans-serif"]
}

impl TranslationCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            model: None,
            usage: ProviderUsage {
                prompt_tokens: Some(0),
                completion_tokens: Some(0),
                total_tokens: Some(0),
            },
            used: false,
        }
    }

    fn record_usage(&mut self, model: Option<String>, usage: Option<ProviderUsage>) {
        if self.model.is_none() {
            self.model = model;
        }
        if let Some(usage) = usage {
            self.usage = merge_usage(self.usage.clone(), Some(usage));
            self.used = true;
        }
    }

    async fn translate_preserve_whitespace<P: Provider + Clone>(
        &mut self,
        text: &str,
        translator: &Translator<P>,
        options: &TranslateOptions,
    ) -> Result<String> {
        let Some((start, end)) = split_text_bounds(text) else {
            return Ok(text.to_string());
        };
        let leading = &text[..start];
        let core = &text[start..end];
        let trailing = &text[end..];
        let translated = self.translate(core, translator, options).await?;
        Ok(format!("{}{}{}", leading, translated, trailing))
    }

    async fn translate_ocr_line<P: Provider + Clone>(
        &mut self,
        text: &str,
        translator: &Translator<P>,
        options: &TranslateOptions,
    ) -> Result<String> {
        let cleaned = collapse_whitespace(text);
        let cleaned = sanitize_ocr_text(&cleaned);
        if cleaned.trim().is_empty() {
            return Ok(text.to_string());
        }
        if is_numeric_like(cleaned.trim()) {
            return Ok(cleaned.trim().to_string());
        }
        if cleaned.trim().chars().count() <= 1 {
            return Ok(cleaned.trim().to_string());
        }
        self.translate(cleaned.trim(), translator, options).await
    }

    async fn translate<P: Provider + Clone>(
        &mut self,
        text: &str,
        translator: &Translator<P>,
        options: &TranslateOptions,
    ) -> Result<String> {
        if let Some(existing) = self.map.get(text) {
            return Ok(existing.clone());
        }
        let exec = translator.exec(text, options.clone()).await?;
        if self.model.is_none() {
            self.model = exec.model.clone();
        }
        self.usage = merge_usage(self.usage.clone(), exec.usage);
        self.used = true;
        self.map.insert(text.to_string(), exec.text.clone());
        Ok(exec.text)
    }

    fn finish(self, mime: String, bytes: Vec<u8>) -> AttachmentTranslation {
        AttachmentTranslation {
            bytes,
            mime,
            model: self.model,
            usage: if self.used { Some(self.usage) } else { None },
        }
    }
}

#[derive(Debug, Serialize)]
struct OcrNormalizeLineInput {
    id: usize,
    text: String,
    bbox: BBoxNorm,
}

#[derive(Debug, Serialize)]
struct BBoxNorm {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Deserialize)]
struct OcrNormalizeArgs {
    image_kind: String,
    lines: Vec<OcrNormalizeLineOutput>,
}

#[derive(Debug, Deserialize)]
struct OcrNormalizeLineOutput {
    id: usize,
    normalized: String,
    #[serde(default)]
    reading: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OcrRomanizeArgs {
    lines: Vec<OcrRomanizeLineOutput>,
}

#[derive(Debug, Deserialize)]
struct OcrRomanizeLineOutput {
    id: usize,
    romanized: String,
}

struct OcrNormalizeOutcome {
    lines: Vec<ocr::OcrLine>,
    #[allow(dead_code)]
    image_kind: Option<String>,
    readings: HashMap<usize, String>,
}

struct OcrNormalizeRequest<'a> {
    image_bytes: &'a [u8],
    image_mime: &'a str,
    width: u32,
    height: u32,
    lines: &'a [ocr::OcrLine],
}

fn ocr_normalize_tool_spec() -> ToolSpec {
    let spec = serde_json::json!({
        "type": "object",
        "properties": {
            "image_kind": {"type": "string"},
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "normalized": {"type": "string"},
                        "reading": {"type": "string"}
                    },
                    "required": ["id", "normalized", "reading"]
                }
            }
        },
        "required": ["image_kind", "lines"]
    });
    ToolSpec {
        name: OCR_NORMALIZE_TOOL.to_string(),
        description: "Return normalized OCR text per line with image kind.".to_string(),
        parameters: spec,
    }
}

fn ocr_romanize_tool_spec() -> ToolSpec {
    let spec = serde_json::json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "romanized": {"type": "string"}
                    },
                    "required": ["id", "romanized"]
                }
            }
        },
        "required": ["lines"]
    });
    ToolSpec {
        name: OCR_ROMANIZE_TOOL.to_string(),
        description: "Return Latin-script readings for OCR lines.".to_string(),
        parameters: spec,
    }
}

fn render_ocr_normalize_prompt(source_lang: &str) -> String {
    format!(
        r#"You are an OCR normalization engine.
First determine the image kind. Choose one short label from:
webpage_screenshot, document_scan, presentation_slide, spreadsheet, infographic, form, menu, chart, photo_with_text, ui_screen, other.
Then normalize each OCR line:
- Fix spacing between characters (e.g., "マッ チン グ" -> "マッチング").
- Merge split words, fix obvious OCR noise or stray symbols.
- Use the attached image to correct OCR mistakes.
- DO NOT translate. Keep the original language.
- If unsure, return the original text for that line.
Also provide a pronunciation reading in Latin script for the normalized text when the source language is non-Latin:
- Japanese: Hepburn romaji with macrons (e.g., shō, ryō).
- Chinese: Hanyu Pinyin with tone marks (e.g., Dàjiā hǎo).
- Korean: Revised Romanization.
- For other non-Latin scripts: best-effort transliteration in Latin script.
- If the source text is already Latin script, return an empty string for reading.
Source language hint (may be auto): {source_lang}
Return ONLY the tool arguments JSON.
Tool name: {tool_name}"#,
        source_lang = source_lang,
        tool_name = OCR_NORMALIZE_TOOL
    )
}

fn render_ocr_romanize_prompt(source_lang: &str) -> String {
    format!(
        r#"You are a transliteration engine.
Return a Latin-script reading for each line based on the source language.
- Japanese: Hepburn romaji with macrons.
- Chinese: Hanyu Pinyin with tone marks.
- Korean: Revised Romanization.
- For other non-Latin scripts: best-effort Latin transliteration.
- If the input is already Latin script, return an empty string.
DO NOT translate.
Source language hint (may be auto): {source_lang}
Return ONLY the tool arguments JSON.
Tool name: {tool_name}"#,
        source_lang = source_lang,
        tool_name = OCR_ROMANIZE_TOOL
    )
}

fn is_latin_reading(value: &str) -> bool {
    let mut has_alpha = false;
    for ch in value.chars() {
        if is_cjk(ch) || is_hiragana(ch) || is_katakana(ch) || is_hangul(ch) {
            return false;
        }
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
        }
    }
    has_alpha
}

fn contains_non_latin_script(value: &str) -> bool {
    value.chars().any(|ch| {
        is_cjk(ch)
            || is_hiragana(ch)
            || is_katakana(ch)
            || is_hangul(ch)
            || (!ch.is_ascii() && ch.is_alphabetic())
    })
}

async fn romanize_lines_with_llm<P: Provider + Clone>(
    lines: &[(usize, String)],
    translator: &Translator<P>,
    options: &TranslateOptions,
    cache: &mut TranslationCache,
) -> Result<HashMap<usize, String>> {
    if lines.is_empty() {
        return Ok(HashMap::new());
    }
    let payload = serde_json::json!({
        "lines": lines.iter().map(|(id, text)| serde_json::json!({"id": id, "text": text})).collect::<Vec<_>>()
    });
    let user_input = format!("Lines (JSON):\n{}", serde_json::to_string_pretty(&payload)?);
    let system_prompt = render_ocr_romanize_prompt(&options.source_lang);
    let tool = ocr_romanize_tool_spec();
    let response = translator
        .call_tool_with_data(tool, system_prompt, user_input, None)
        .await?;
    cache.record_usage(response.model.clone(), response.usage.clone());

    let args: OcrRomanizeArgs = serde_json::from_value(response.args)?;
    let mut map = HashMap::new();
    for line in args.lines {
        let cleaned = collapse_whitespace(line.romanized.trim());
        if !cleaned.is_empty() {
            map.insert(line.id, cleaned);
        }
    }
    Ok(map)
}

async fn normalize_ocr_lines_with_llm<P: Provider + Clone>(
    request: OcrNormalizeRequest<'_>,
    translator: &Translator<P>,
    options: &TranslateOptions,
    cache: &mut TranslationCache,
) -> Result<OcrNormalizeOutcome> {
    let mut input_lines = Vec::new();
    for (idx, line) in request.lines.iter().enumerate() {
        let cleaned = sanitize_ocr_text(&collapse_whitespace(&line.text));
        if should_skip_ocr_annotation(&cleaned) {
            continue;
        }
        let bbox = BBoxNorm {
            x: (line.bbox.x as f32 / request.width.max(1) as f32).clamp(0.0, 1.0),
            y: (line.bbox.y as f32 / request.height.max(1) as f32).clamp(0.0, 1.0),
            w: (line.bbox.w as f32 / request.width.max(1) as f32).clamp(0.0, 1.0),
            h: (line.bbox.h as f32 / request.height.max(1) as f32).clamp(0.0, 1.0),
        };
        input_lines.push(OcrNormalizeLineInput {
            id: idx,
            text: line.text.clone(),
            bbox,
        });
    }

    if input_lines.is_empty() {
        return Ok(OcrNormalizeOutcome {
            lines: request.lines.to_vec(),
            image_kind: None,
            readings: HashMap::new(),
        });
    }

    let payload = serde_json::json!({
        "image": {
            "mime": request.image_mime,
            "width": request.width,
            "height": request.height
        },
        "lines": input_lines
    });
    let user_input = format!(
        "OCR lines (JSON):\n{}",
        serde_json::to_string_pretty(&payload)?
    );
    let system_prompt = render_ocr_normalize_prompt(&options.source_lang);
    let tool = ocr_normalize_tool_spec();
    let data = data::DataAttachment {
        bytes: request.image_bytes.to_vec(),
        mime: request.image_mime.to_string(),
        name: None,
    };
    let response = translator
        .call_tool_with_data(tool, system_prompt, user_input, Some(data))
        .await?;
    cache.record_usage(response.model.clone(), response.usage.clone());

    let args: OcrNormalizeArgs = serde_json::from_value(response.args)?;
    let mut normalized_map = HashMap::new();
    let mut readings = HashMap::new();
    for line in args.lines {
        let cleaned = collapse_whitespace(line.normalized.trim());
        if !cleaned.is_empty() {
            normalized_map.insert(line.id, cleaned);
        }
        if let Some(reading) = line.reading {
            let reading = collapse_whitespace(reading.trim());
            if !reading.is_empty() {
                readings.insert(line.id, reading);
            }
        }
    }

    let mut normalized_lines = request.lines.to_vec();
    for (idx, line) in normalized_lines.iter_mut().enumerate() {
        if let Some(text) = normalized_map.get(&idx) {
            line.text = text.clone();
        }
    }

    Ok(OcrNormalizeOutcome {
        lines: normalized_lines,
        image_kind: Some(args.image_kind),
        readings,
    })
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

fn sanitize_ocr_text(value: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    let mut last_punct = false;
    for ch in value.chars() {
        if ch.is_control() {
            continue;
        }
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            last_punct = false;
            continue;
        }
        if is_ignorable_symbol(ch) {
            if !last_punct {
                out.push(ch);
                last_punct = true;
            }
            last_space = false;
            continue;
        }
        out.push(ch);
        last_space = false;
        last_punct = false;
    }
    let mut trimmed = strip_cjk_adjacent_punct(out.trim());
    trimmed = trim_ocr_edges(trimmed.trim());
    let trimmed = trim_ocr_edges(trimmed.trim());
    trim_ascii_edges_for_cjk(&trimmed)
}

fn should_skip_ocr_annotation(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if is_numeric_like(trimmed) {
        return true;
    }
    if is_ascii_alnum_only(trimmed) {
        return true;
    }
    if is_short_kana_only(trimmed) {
        return true;
    }
    false
}

fn is_ignorable_symbol(ch: char) -> bool {
    matches!(ch, '|' | '¦' | '·' | '•' | '―' | '—' | '–' | '…')
}

fn strip_cjk_adjacent_punct(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, ch) in chars.iter().enumerate() {
        if is_noise_punct(*ch) {
            let prev = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(idx + 1).copied();
            let prev_cjk = prev.map(is_cjk).unwrap_or(false);
            let next_cjk = next.map(is_cjk).unwrap_or(false);
            let next_digit = next.map(|c| c.is_ascii_digit()).unwrap_or(false);
            if prev_cjk || next_cjk || next_digit {
                continue;
            }
        }
        out.push(*ch);
    }
    out
}

fn is_noise_punct(ch: char) -> bool {
    matches!(ch, '!' | '！' | '?' | '？' | '・' | '…')
}

fn should_filter_by_source_lang(source_lang: &str) -> bool {
    let lang = source_lang.trim().to_lowercase();
    if lang.is_empty() || lang == "auto" || lang == "und" || lang == "mul" {
        return false;
    }
    lang == "ja"
        || lang == "jpn"
        || lang == "jp"
        || lang.starts_with("zh")
        || lang.starts_with("ko")
}

fn should_keep_cjk_line(text: &str) -> bool {
    if is_numeric_like(text.trim()) {
        return true;
    }
    let stats = cjk_stats(text);
    if stats.cjk >= 2 {
        let ratio = stats.cjk as f32 / stats.total.max(1) as f32;
        return ratio >= 0.35 || stats.total <= 6;
    }
    false
}

struct CjkStats {
    cjk: usize,
    total: usize,
    digits: usize,
    ascii: usize,
}

fn cjk_stats(text: &str) -> CjkStats {
    let mut stats = CjkStats {
        cjk: 0,
        total: 0,
        digits: 0,
        ascii: 0,
    };
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        stats.total += 1;
        if ch.is_ascii_digit() {
            stats.digits += 1;
        }
        if ch.is_ascii_alphabetic() {
            stats.ascii += 1;
        }
        if matches!(
            ch as u32,
            0x4E00..=0x9FFF
                | 0x3040..=0x30FF
                | 0x31F0..=0x31FF
                | 0x3400..=0x4DBF
                | 0xAC00..=0xD7AF
        ) {
            stats.cjk += 1;
        }
    }
    stats
}

fn is_numeric_like(value: &str) -> bool {
    let mut digits = 0usize;
    let mut letters = 0usize;
    let mut others = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else if ch.is_alphabetic()
            || matches!(ch as u32, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF)
        {
            letters += 1;
        } else if !ch.is_whitespace() {
            others += 1;
        }
    }
    if letters > 0 {
        return false;
    }
    digits > 0 && (digits as f32 / (digits + others).max(1) as f32) >= 0.6
}

fn is_numeric_only_like(value: &str) -> bool {
    let mut digits = 0usize;
    let mut letters = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else if ch.is_alphabetic()
            || matches!(ch as u32, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF)
        {
            letters += 1;
        }
    }
    digits > 0 && letters == 0
}

fn is_ascii_alnum_only(value: &str) -> bool {
    let mut has_alnum = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            has_alnum = true;
            continue;
        }
        if is_ascii_punct_ok(ch) {
            continue;
        }
        return false;
    }
    has_alnum
}

fn is_ascii_punct_ok(ch: char) -> bool {
    matches!(
        ch,
        '%' | '.'
            | ','
            | ':'
            | ';'
            | '-'
            | '/'
            | '\\'
            | '+'
            | '#'
            | '&'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '!'
            | '?'
            | '\''
            | '"'
    )
}

fn is_short_kana_only(value: &str) -> bool {
    let trimmed = value
        .trim_matches(|ch: char| matches!(ch, '。' | '、' | '.' | ',' | '!' | '?' | '！' | '？'));
    let len = trimmed.chars().count();
    if len == 0 || len > 3 {
        return false;
    }
    trimmed.chars().all(is_kana)
}

fn is_kana(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF | 0x31F0..=0x31FF
    )
}

fn trim_ocr_edges(value: &str) -> String {
    let mut s = value.trim().to_string();
    if is_numeric_only_like(&s) {
        s = trim_edge_chars(&s, is_edge_noise_numeric);
    } else {
        s = trim_edge_chars(&s, is_edge_noise);
    }
    s = drop_leading_particle(&s);
    s = drop_trailing_single_digit(&s);
    s.trim().to_string()
}

fn trim_edge_chars<F>(value: &str, mut predicate: F) -> String
where
    F: FnMut(char) -> bool,
{
    let mut start = 0usize;
    let mut end = value.len();

    for (idx, ch) in value.char_indices() {
        if predicate(ch) {
            start = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    for (idx, ch) in value.char_indices().rev() {
        if idx < start {
            break;
        }
        if predicate(ch) {
            end = idx;
        } else {
            break;
        }
    }

    value[start..end].to_string()
}

fn is_edge_noise(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '「' | '」'
                | '『'
                | '』'
                | '《'
                | '》'
                | '〈'
                | '〉'
                | '【'
                | '】'
                | '（'
                | '）'
                | '・'
                | '、'
                | '。'
                | '，'
                | '．'
                | '※'
        )
}

fn is_edge_noise_numeric(ch: char) -> bool {
    if ch.is_ascii_digit() {
        return false;
    }
    if matches!(ch, '%' | '％' | '+' | '-' | '.' | ',' | '．' | '，') {
        return false;
    }
    is_edge_noise(ch)
}

fn trim_ascii_edges_for_cjk(value: &str) -> String {
    if !value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF
        )
    }) {
        return value.to_string();
    }
    let mut start = 0usize;
    let mut end = value.len();
    let mut leading = true;
    for (idx, ch) in value.char_indices() {
        if leading && ch.is_ascii_alphabetic() {
            start = idx + ch.len_utf8();
        } else {
            leading = false;
        }
    }
    for (idx, ch) in value.char_indices().rev() {
        if idx < start {
            break;
        }
        if ch.is_ascii_alphabetic() {
            end = idx;
        } else {
            break;
        }
    }
    value[start..end].trim().to_string()
}

fn drop_leading_particle(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return value.to_string();
    };
    let remainder: String = chars.collect();
    if !is_particle(first) {
        return value.to_string();
    }
    if value.chars().count() <= 3 {
        return value.to_string();
    }
    let second = value.chars().nth(1);
    let drop = second
        .map(|ch| is_cjk(ch) || ch.is_ascii_alphanumeric())
        .unwrap_or(false);
    if drop && !remainder.trim().is_empty() {
        remainder.trim_start().to_string()
    } else {
        value.to_string()
    }
}

fn is_particle(ch: char) -> bool {
    matches!(
        ch,
        'は' | 'が' | 'を' | 'に' | 'へ' | 'と' | 'で' | 'も' | 'や' | 'の'
    )
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF
    )
}

fn is_hiragana(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x309F)
}

fn is_katakana(ch: char) -> bool {
    matches!(ch as u32, 0x30A0..=0x30FF | 0x31F0..=0x31FF)
}

fn is_hangul(ch: char) -> bool {
    matches!(ch as u32, 0xAC00..=0xD7AF)
}

fn drop_trailing_single_digit(value: &str) -> String {
    let digits: Vec<char> = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() != 1 {
        return value.to_string();
    }
    if value.contains('%') {
        return value.to_string();
    }
    let last = value.chars().last().unwrap_or(' ');
    if last.is_ascii_digit() {
        value
            .trim_end_matches(|ch: char| ch.is_ascii_digit())
            .trim_end()
            .to_string()
    } else {
        value.to_string()
    }
}

fn split_text_bounds(text: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut end = None;
    for (idx, ch) in text.char_indices() {
        if !ch.is_whitespace() {
            start = Some(idx);
            break;
        }
    }
    for (idx, ch) in text.char_indices().rev() {
        if !ch.is_whitespace() {
            end = Some(idx + ch.len_utf8());
            break;
        }
    }
    match (start, end) {
        (Some(s), Some(e)) if s < e => Some((s, e)),
        _ => None,
    }
}

fn merge_usage(total: ProviderUsage, next: Option<ProviderUsage>) -> ProviderUsage {
    let Some(next) = next else {
        return total;
    };
    ProviderUsage {
        prompt_tokens: Some(total.prompt_tokens.unwrap_or(0) + next.prompt_tokens.unwrap_or(0)),
        completion_tokens: Some(
            total.completion_tokens.unwrap_or(0) + next.completion_tokens.unwrap_or(0),
        ),
        total_tokens: Some(total.total_tokens.unwrap_or(0) + next.total_tokens.unwrap_or(0)),
    }
}
