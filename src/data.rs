use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub const DOCX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const PPTX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";
pub const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
pub const PDF_MIME: &str = "application/pdf";
pub const DOC_MIME: &str = "application/msword";
pub const TEXT_MIME: &str = "text/plain";
pub const HTML_MIME: &str = "text/html";
pub const JSON_MIME: &str = "application/json";
pub const YAML_MIME: &str = "text/yaml";
pub const PO_MIME: &str = "text/x-po";
pub const MP3_MIME: &str = "audio/mpeg";
pub const WAV_MIME: &str = "audio/wav";
pub const M4A_MIME: &str = "audio/mp4";
pub const FLAC_MIME: &str = "audio/flac";
pub const OGG_MIME: &str = "audio/ogg";

#[derive(Debug, Clone)]
pub struct DataAttachment {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DataInfo {
    pub mime: String,
    pub name: Option<String>,
}

impl DataAttachment {
    pub fn info(&self) -> DataInfo {
        DataInfo {
            mime: self.mime.clone(),
            name: self.name.clone(),
        }
    }
}

pub fn load_attachment(path: &Path, mime_hint: Option<&str>) -> Result<DataAttachment> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read data file: {}", path.display()))?;
    let mime = resolve_mime(mime_hint.unwrap_or("auto"), &bytes, Some(path))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string());
    Ok(DataAttachment { bytes, mime, name })
}

pub fn load_attachment_from_bytes(
    bytes: Vec<u8>,
    mime_hint: Option<&str>,
    name: Option<&str>,
) -> Result<DataAttachment> {
    let path = name.map(PathBuf::from);
    let mime = resolve_mime(mime_hint.unwrap_or("auto"), &bytes, path.as_deref())?;
    Ok(DataAttachment {
        bytes,
        mime,
        name: name.map(|value| value.to_string()),
    })
}

pub fn sniff_mime(bytes: &[u8]) -> Option<String> {
    sniff_mime_bytes(bytes).map(|value| value.to_string())
}

fn resolve_mime(input: &str, bytes: &[u8], path: Option<&Path>) -> Result<String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(anyhow!("data-mime is empty"));
    }
    let lower = raw.to_lowercase();

    match lower.as_str() {
        "auto" => return detect_mime(bytes, path, false),
        "image" | "image/*" => return detect_mime(bytes, path, true),
        "pdf" => return Ok(PDF_MIME.to_string()),
        "doc" => return Ok(DOC_MIME.to_string()),
        "docs" => return Ok(DOCX_MIME.to_string()),
        "docx" => return Ok(DOCX_MIME.to_string()),
        "pptx" => return Ok(PPTX_MIME.to_string()),
        "xlsx" => return Ok(XLSX_MIME.to_string()),
        "txt" => return Ok(TEXT_MIME.to_string()),
        "text" => return Ok(TEXT_MIME.to_string()),
        "html" | "htm" => return Ok(HTML_MIME.to_string()),
        "json" => return Ok(JSON_MIME.to_string()),
        "yaml" | "yml" => return Ok(YAML_MIME.to_string()),
        "po" => return Ok(PO_MIME.to_string()),
        "mp3" => return Ok(MP3_MIME.to_string()),
        "wav" => return Ok(WAV_MIME.to_string()),
        "m4a" => return Ok(M4A_MIME.to_string()),
        "flac" => return Ok(FLAC_MIME.to_string()),
        "ogg" => return Ok(OGG_MIME.to_string()),
        "png" => return Ok("image/png".to_string()),
        "jpg" | "jpeg" => return Ok("image/jpeg".to_string()),
        "gif" => return Ok("image/gif".to_string()),
        "webp" => return Ok("image/webp".to_string()),
        "bmp" => return Ok("image/bmp".to_string()),
        "tiff" | "tif" => return Ok("image/tiff".to_string()),
        "heic" => return Ok("image/heic".to_string()),
        _ => {}
    }

    if lower == DOCX_MIME
        || lower == PPTX_MIME
        || lower == XLSX_MIME
        || lower == PDF_MIME
        || lower == DOC_MIME
        || lower == TEXT_MIME
        || lower == HTML_MIME
        || lower == JSON_MIME
        || lower == YAML_MIME
        || lower == PO_MIME
        || lower == "application/x-yaml"
        || lower == "application/yaml"
        || lower == "text/x-yaml"
        || lower == "text/x-gettext-translation"
        || lower == "application/x-gettext-translation"
        || lower == MP3_MIME
        || lower == WAV_MIME
        || lower == M4A_MIME
        || lower == FLAC_MIME
        || lower == OGG_MIME
    {
        return Ok(match lower.as_str() {
            "application/x-yaml" | "application/yaml" | "text/x-yaml" => YAML_MIME.to_string(),
            "text/x-gettext-translation" | "application/x-gettext-translation" => {
                PO_MIME.to_string()
            }
            _ => lower,
        });
    }
    if lower.starts_with("image/") {
        return Ok(lower);
    }
    if lower.starts_with("audio/") {
        return Ok(lower);
    }

    Err(anyhow!(
        "unsupported --data-mime '{}' (expected auto, image/*, pdf, doc, docx, docs, pptx, xlsx, txt, html, json, yaml, po, mp3, wav, m4a, flac, ogg)",
        raw
    ))
}

fn detect_mime(bytes: &[u8], path: Option<&Path>, require_image: bool) -> Result<String> {
    if let Some(detected) = sniff_mime_bytes(bytes) {
        if require_image && !detected.starts_with("image/") {
            return Err(anyhow!(
                "data-mime image/* requires image data (detected '{}')",
                detected
            ));
        }
        return Ok(detected.to_string());
    }

    if let Some(ext) = extension_lower(path) {
        if let Some(mime) = mime_from_extension(&ext) {
            if require_image && !mime.starts_with("image/") {
                return Err(anyhow!(
                    "data-mime image/* requires image data (detected '{}')",
                    mime
                ));
            }
            return Ok(mime.to_string());
        }
    }

    Err(anyhow!(
        "unable to detect supported mime for file '{}'",
        path.map(|value| value.display().to_string())
            .unwrap_or_else(|| "stdin".to_string())
    ))
}

fn sniff_mime_bytes(bytes: &[u8]) -> Option<&'static str> {
    let kind = infer::get(bytes)?;
    let detected = kind.mime_type();
    if detected.starts_with("image/") {
        return Some(detected);
    }
    if detected.starts_with("audio/") {
        return Some(detected);
    }
    match detected {
        HTML_MIME => Some(HTML_MIME),
        JSON_MIME => Some(JSON_MIME),
        YAML_MIME | "application/x-yaml" | "text/x-yaml" => Some(YAML_MIME),
        PO_MIME | "text/x-gettext-translation" | "application/x-gettext-translation" => {
            Some(PO_MIME)
        }
        PDF_MIME => Some(PDF_MIME),
        DOC_MIME => Some(DOC_MIME),
        TEXT_MIME => Some(TEXT_MIME),
        "application/zip" => detect_office_in_zip(bytes),
        _ => None,
    }
}

fn detect_office_in_zip(bytes: &[u8]) -> Option<&'static str> {
    if contains_bytes(bytes, b"word/") {
        return Some(DOCX_MIME);
    }
    if contains_bytes(bytes, b"ppt/") {
        return Some(PPTX_MIME);
    }
    if contains_bytes(bytes, b"xl/") {
        return Some(XLSX_MIME);
    }
    None
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn extension_lower(path: Option<&Path>) -> Option<String> {
    path.and_then(|path| path.extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
}

fn mime_from_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some(PDF_MIME),
        "doc" => Some(DOC_MIME),
        "docs" => Some(DOCX_MIME),
        "docx" => Some(DOCX_MIME),
        "pptx" => Some(PPTX_MIME),
        "xlsx" => Some(XLSX_MIME),
        "txt" => Some(TEXT_MIME),
        "html" | "htm" => Some(HTML_MIME),
        "json" => Some(JSON_MIME),
        "yaml" | "yml" => Some(YAML_MIME),
        "po" => Some(PO_MIME),
        "mp3" => Some(MP3_MIME),
        "wav" => Some(WAV_MIME),
        "m4a" => Some(M4A_MIME),
        "flac" => Some(FLAC_MIME),
        "ogg" => Some(OGG_MIME),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tiff" | "tif" => Some("image/tiff"),
        "heic" => Some("image/heic"),
        _ => None,
    }
}

pub fn extension_from_mime(mime: &str) -> Option<&'static str> {
    match mime {
        PDF_MIME => Some("pdf"),
        DOC_MIME => Some("doc"),
        DOCX_MIME => Some("docx"),
        PPTX_MIME => Some("pptx"),
        XLSX_MIME => Some("xlsx"),
        TEXT_MIME => Some("txt"),
        HTML_MIME => Some("html"),
        JSON_MIME => Some("json"),
        YAML_MIME => Some("yaml"),
        PO_MIME => Some("po"),
        MP3_MIME => Some("mp3"),
        WAV_MIME => Some("wav"),
        M4A_MIME => Some("m4a"),
        FLAC_MIME => Some("flac"),
        OGG_MIME => Some("ogg"),
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        "image/heic" => Some("heic"),
        _ => None,
    }
}
