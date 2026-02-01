use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct OcrDebugConfig {
    output_dir: PathBuf,
    base_name: String,
}

impl OcrDebugConfig {
    pub(crate) fn page_label(&self, page: Option<usize>) -> String {
        if let Some(index) = page {
            format!("{}_page{:02}", self.base_name, index + 1)
        } else {
            self.base_name.clone()
        }
    }

    pub(crate) fn for_page(&self, index: usize) -> OcrDebugConfig {
        OcrDebugConfig {
            output_dir: self.output_dir.clone(),
            base_name: self.page_label(Some(index)),
        }
    }

    pub(crate) fn output_path(&self, label: &str) -> PathBuf {
        self.output_dir.join(format!("{}_ocr_bbox.png", label))
    }

    pub(crate) fn json_path(&self, label: &str) -> PathBuf {
        self.output_dir.join(format!("{}_ocr.json", label))
    }
}

pub(crate) fn build_ocr_debug_config(
    src_path: Option<&Path>,
    name: Option<&str>,
) -> Result<OcrDebugConfig> {
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
