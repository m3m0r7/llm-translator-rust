use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::Path;

use crate::providers::Provider;
use crate::{TranslateOptions, Translator};

use crate::attachments::cache::TranslationCache;
use crate::attachments::util::{
    collapse_whitespace, sanitize_ocr_text, should_filter_by_source_lang, should_keep_cjk_line,
    should_skip_ocr_annotation,
};
use crate::ocr;

use super::ocr::{
    OcrDebugConfig, OcrNormalizeRequest, contains_non_latin_script, is_latin_reading,
    normalize_ocr_lines_with_llm, romanize_lines_with_llm,
};

pub(crate) struct ImageTranslateRequest<'a> {
    pub(crate) image_bytes: &'a [u8],
    pub(crate) image_mime: &'a str,
    pub(crate) output_mime: &'a str,
    pub(crate) ocr_languages: &'a str,
    pub(crate) allow_empty: bool,
    pub(crate) debug: Option<OcrDebugConfig>,
}

#[derive(Debug, Clone)]
struct AnnotationEntry {
    id: usize,
    original: String,
    reading: Option<String>,
    translated: String,
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

pub(crate) async fn translate_image_with_cache<P: Provider + Clone>(
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
