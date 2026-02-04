use anyhow::{Context, Result, anyhow};
use quick_xml::events::{BytesText, Event};
use quick_xml::{Reader, Writer};
use std::io::{Cursor, Read, Write};
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::data;
use crate::providers::Provider;
use crate::{TranslateOptions, Translator};

use super::AttachmentTranslation;
use super::cache::TranslationCache;

#[derive(Debug, Clone, Copy)]
pub(crate) enum OfficeKind {
    Docx,
    Pptx,
    Xlsx,
}

pub(crate) async fn translate_office_zip<P: Provider + Clone>(
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
