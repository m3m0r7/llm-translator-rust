use std::path::{Path, PathBuf};

const BASE_DIR_ENV: &str = "LLM_TRANSLATOR_RUST_DIR";

pub(crate) fn set_base_dir_if_override(value: &str) {
    if base_dir_override().is_some() {
        return;
    }
    let Some(normalized) = normalize_dir(value) else {
        return;
    };
    if let Some(default) = default_base_dir_for_compare() {
        if normalized == default {
            return;
        }
    }
    std::env::set_var(BASE_DIR_ENV, normalized.to_string_lossy().as_ref());
}

pub(crate) fn settings_dir() -> Option<PathBuf> {
    if let Some(dir) = base_dir_override() {
        return Some(dir);
    }
    default_base_dir()
}

pub(crate) fn meta_cache_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join(".cache");
    }
    home_join(".llm-translator/.cache")
        .unwrap_or_else(|| PathBuf::from(".llm-translator/.cache"))
}

pub(crate) fn history_dest_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join(".cache/dest");
    }
    home_join(".llm-translator-rust/.cache/dest")
        .unwrap_or_else(|| PathBuf::from(".llm-translator-rust/.cache/dest"))
}

pub(crate) fn whisper_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join(".cache/whisper");
    }
    home_join(".llm-translator-rust/.cache/whisper")
        .unwrap_or_else(|| PathBuf::from(".llm-translator-rust/.cache/whisper"))
}

pub(crate) fn ocr_debug_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join(".cache/ocr");
    }
    home_join(".llm-translator-rust/.cache/ocr")
        .unwrap_or_else(|| PathBuf::from(".llm-translator-rust/.cache/ocr"))
}

pub(crate) fn backup_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join("backup");
    }
    home_join(".llm-translator-rust/backup")
        .unwrap_or_else(|| PathBuf::from(".llm-translator-rust/backup"))
}

fn base_dir_override() -> Option<PathBuf> {
    std::env::var(BASE_DIR_ENV)
        .ok()
        .and_then(|value| normalize_dir(&value))
}

fn default_base_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().and_then(|home| {
        let home = home.trim();
        if home.is_empty() {
            None
        } else {
            Some(Path::new(home).join(".llm-translator-rust"))
        }
    })
}

fn default_base_dir_for_compare() -> Option<PathBuf> {
    if let Some(path) = default_base_dir() {
        return Some(normalize_path(path));
    }
    Some(normalize_path(PathBuf::from(".llm-translator-rust")))
}

fn home_join(suffix: &str) -> Option<PathBuf> {
    std::env::var("HOME").ok().and_then(|home| {
        let home = home.trim();
        if home.is_empty() {
            None
        } else {
            Some(Path::new(home).join(suffix))
        }
    })
}

fn normalize_dir(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = expand_tilde(trimmed);
    Some(normalize_path(PathBuf::from(expanded)))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        normalized.push(component.as_os_str());
    }
    normalized
}

fn expand_tilde(value: &str) -> String {
    if value == "~" || value.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            let home = home.trim();
            if home.is_empty() {
                return value.to_string();
            }
            if value == "~" {
                return home.to_string();
            }
            return format!("{}{}", home, &value[1..]);
        }
    }
    value.to_string()
}
