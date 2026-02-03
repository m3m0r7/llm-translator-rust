mod cache;
mod code;
mod media;
mod mime_detect;
mod office;
mod text;
mod util;

use anyhow::{anyhow, Result};
use std::path::Path;
use tracing::info;

use crate::data;
use crate::providers::{Provider, ProviderUsage};
use crate::{TranslateOptions, Translator};

use cache::TranslationCache;
use code::{translate_javascript, translate_mermaid, translate_tsx, translate_typescript};
use media::{
    build_ocr_debug_config, translate_audio, translate_image_with_cache, translate_pdf,
    ImageTranslateRequest,
};
pub use mime_detect::{detect_mime_with_llm, MimeDetection};
use office::{translate_office_zip, OfficeKind};
use text::{
    translate_html, translate_json, translate_markdown, translate_po, translate_xml, translate_yaml,
};

pub struct AttachmentTranslation {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub model: Option<String>,
    pub usage: Option<ProviderUsage>,
}

#[allow(clippy::too_many_arguments)]
pub async fn translate_attachment<P: Provider + Clone>(
    data: &data::DataAttachment,
    ocr_languages: &str,
    translator: &Translator<P>,
    options: &TranslateOptions,
    with_commentout: bool,
    debug_ocr: bool,
    force_translation: bool,
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
        data::MARKDOWN_MIME => {
            info!("attachment: markdown");
            let output = translate_markdown(&data.bytes, translator, options).await?;
            return Ok(Some(output));
        }
        data::HTML_MIME => {
            info!("attachment: html");
            let output = translate_html(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::JSON_MIME => {
            info!("attachment: json");
            let output = translate_json(&data.bytes, translator, options).await?;
            return Ok(Some(output));
        }
        data::YAML_MIME => {
            info!("attachment: yaml");
            let output = translate_yaml(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::PO_MIME => {
            info!("attachment: po");
            let output = translate_po(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::XML_MIME => {
            info!("attachment: xml");
            let output = translate_xml(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::JS_MIME => {
            info!("attachment: javascript");
            let output =
                translate_javascript(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::TS_MIME => {
            info!("attachment: typescript");
            let output =
                translate_typescript(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::TSX_MIME => {
            info!("attachment: tsx");
            let output = translate_tsx(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        data::MERMAID_MIME => {
            info!("attachment: mermaid");
            let output =
                translate_mermaid(&data.bytes, with_commentout, translator, options).await?;
            return Ok(Some(output));
        }
        mime if mime.starts_with("audio/") => {
            info!("attachment: audio ({})", mime);
            let output = translate_audio(data, translator, options).await?;
            return Ok(Some(output));
        }
        data::TEXT_MIME => {
            info!("attachment: text");
            let text = match std::str::from_utf8(&data.bytes) {
                Ok(value) => value.to_string(),
                Err(_) if force_translation => String::from_utf8_lossy(&data.bytes).to_string(),
                Err(err) => return Err(anyhow!("failed to decode text file as UTF-8: {}", err)),
            };
            let exec = translator.exec(text.as_str(), options.clone()).await?;
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
