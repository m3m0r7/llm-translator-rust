use anyhow::{Context, Result, anyhow};
use std::process::Command;

pub fn list_tesseract_languages() -> Result<Vec<String>> {
    let output = Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .with_context(|| "failed to run tesseract --list-langs")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("tesseract --list-langs failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut langs = Vec::new();
    for (idx, line) in stdout.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let value = line.trim();
        if !value.is_empty() {
            langs.push(value.to_string());
        }
    }
    Ok(langs)
}

pub(super) fn normalize_ocr_languages(requested: &str) -> Result<String> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("ocr languages is empty"));
    }

    let available = match list_tesseract_languages() {
        Ok(list) => list,
        Err(_) => return Ok(trimmed.to_string()),
    };

    let mut chosen = Vec::new();
    let mut missing = Vec::new();
    for raw in trimmed.split(['+', ',', ' ']) {
        let lang = raw.trim();
        if lang.is_empty() {
            continue;
        }
        if available.iter().any(|value| value == lang) {
            chosen.push(lang.to_string());
        } else {
            missing.push(lang.to_string());
        }
    }

    if chosen.is_empty() {
        return Err(anyhow!(
            "ocr language(s) not available: {} (available: {})",
            missing.join(", "),
            available.join(", ")
        ));
    }
    if !missing.is_empty() {
        eprintln!(
            "warning: ocr language(s) not available: {} (available: {})",
            missing.join(", "),
            available.join(", ")
        );
    }

    Ok(chosen.join("+"))
}

pub(super) fn run_tesseract_tsv(
    path: &std::path::Path,
    languages: &str,
    psm: u32,
) -> Result<String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg(languages)
        .arg("--oem")
        .arg("1")
        .arg("--psm")
        .arg(psm.to_string())
        .arg("--dpi")
        .arg("300")
        .arg("tsv")
        .output()
        .with_context(|| "failed to run tesseract (is it installed?)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("tesseract failed: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(super) fn run_tesseract_hocr(
    path: &std::path::Path,
    languages: &str,
    psm: u32,
) -> Result<String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg(languages)
        .arg("--oem")
        .arg("1")
        .arg("--psm")
        .arg(psm.to_string())
        .arg("--dpi")
        .arg("300")
        .arg("hocr")
        .output()
        .with_context(|| "failed to run tesseract (is it installed?)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("tesseract failed: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
