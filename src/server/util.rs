use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::data;
use crate::settings;
use crate::translation_ignore::TranslationIgnore;

pub(crate) fn write_temp_file(bytes: &[u8], mime: &str, dir: &Path) -> Result<String> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create tmp dir: {}", dir.display()))?;
    let ext = data::extension_from_mime(mime).unwrap_or("bin");
    let suffix = format!(".{}", ext);
    let file = tempfile::Builder::new()
        .prefix("llm-translator-")
        .suffix(&suffix)
        .tempfile_in(dir)?;
    std::fs::write(file.path(), bytes)
        .with_context(|| "failed to write translated temp file")?;
    let temp_path = file.into_temp_path();
    let path = temp_path
        .keep()
        .with_context(|| "failed to persist temp file")?;
    Ok(path.to_string_lossy().to_string())
}

pub(crate) fn resolve_tmp_dir(settings: &settings::Settings) -> Result<PathBuf> {
    if let Some(dir) = settings.server_tmp_dir.as_deref() {
        return Ok(PathBuf::from(dir));
    }
    Ok(std::env::temp_dir().join("llm-translator-rust"))
}

pub(crate) fn decode_text(bytes: &[u8], force_translation: bool) -> Result<String> {
    match std::str::from_utf8(bytes) {
        Ok(value) => Ok(value.to_string()),
        Err(err) if force_translation => Ok(String::from_utf8_lossy(bytes).to_string()),
        Err(err) => Err(anyhow!("failed to decode text as UTF-8: {}", err)),
    }
}

pub(crate) fn is_text_mime(mime: &str) -> bool {
    if mime.starts_with("text/") {
        return true;
    }
    matches!(
        mime,
        data::JSON_MIME
            | data::YAML_MIME
            | data::XML_MIME
            | data::PO_MIME
            | data::MARKDOWN_MIME
            | data::HTML_MIME
            | data::JS_MIME
            | data::TS_MIME
            | data::TSX_MIME
            | data::MERMAID_MIME
    )
}

pub(crate) fn collect_directory_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| "failed to read directory entry")?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| "failed to read file type")?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) fn build_translation_ignore(
    src_dir: &Path,
    ignore_file_name: &str,
    cli_patterns: &[String],
) -> Result<Option<TranslationIgnore>> {
    let mut patterns = Vec::new();
    let name = ignore_file_name.trim();
    if !name.is_empty() {
        let ignore_path = Path::new(name);
        let path = if ignore_path.is_absolute() {
            ignore_path.to_path_buf()
        } else {
            src_dir.join(name)
        };
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read ignore file: {}", path.display()))?;
            for line in content.lines() {
                patterns.push(line.to_string());
            }
        }
    }
    for pattern in cli_patterns {
        patterns.push(pattern.to_string());
    }
    TranslationIgnore::new(src_dir, patterns)
}
