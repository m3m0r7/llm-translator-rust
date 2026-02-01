use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_SETTINGS_TOML: &str = include_str!("../settings.toml");

#[derive(Debug, Clone)]
pub struct Settings {
    pub formally: HashMap<String, String>,
    pub system_languages: Vec<String>,
    pub history_limit: usize,
    pub translated_suffix: String,
    pub backup_ttl_days: u64,
    pub directory_translation_threads: usize,
    pub translation_ignore_file: String,
    pub overlay_text_color: String,
    pub overlay_stroke_color: String,
    pub overlay_fill_color: String,
    pub overlay_font_size: Option<f32>,
    pub overlay_font_family: Option<String>,
    pub overlay_font_path: Option<String>,
    pub ocr_normalize: bool,
    pub whisper_model: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            formally: HashMap::new(),
            system_languages: Vec::new(),
            history_limit: 10,
            translated_suffix: "_translated".to_string(),
            backup_ttl_days: 30,
            directory_translation_threads: 3,
            translation_ignore_file: ".llm-translation-rust-ignore".to_string(),
            overlay_text_color: "#c40000".to_string(),
            overlay_stroke_color: "#c40000".to_string(),
            overlay_fill_color: "#ffffff".to_string(),
            overlay_font_size: None,
            overlay_font_family: None,
            overlay_font_path: None,
            ocr_normalize: true,
            whisper_model: None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct SettingsFile {
    formally: Option<HashMap<String, String>>,
    system: Option<SystemSettings>,
    ocr: Option<OcrSettings>,
    whisper: Option<WhisperSettings>,
}

#[derive(Debug, Default, Deserialize)]
struct SystemSettings {
    languages: Option<Vec<String>>,
    histories: Option<usize>,
    translated_suffix: Option<String>,
    backup_ttl_days: Option<u64>,
    directory_translation_threads: Option<usize>,
    translation_ignore_file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OcrSettings {
    text_color: Option<String>,
    stroke_color: Option<String>,
    fill_color: Option<String>,
    font_size: Option<f32>,
    font_family: Option<String>,
    font_path: Option<String>,
    normalize: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct WhisperSettings {
    model: Option<String>,
}

pub fn load_settings(extra_path: Option<&Path>) -> Result<Settings> {
    let mut settings = Settings::default();
    ensure_home_settings_file()?;

    let mut ordered_paths = Vec::new();
    ordered_paths.push(PathBuf::from("settings.toml"));
    ordered_paths.push(PathBuf::from("settings.local.toml"));

    if let Some(home) = home_dir() {
        ordered_paths.push(home.join("settings.toml"));
        ordered_paths.push(home.join("settings.local.toml"));
    }

    if let Some(extra) = extra_path {
        if !extra.exists() {
            return Err(anyhow!("settings file not found: {}", extra.display()));
        }
        ordered_paths.push(extra.to_path_buf());
    }

    for path in ordered_paths {
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read settings: {}", path.display()))?;
            let parsed: SettingsFile = toml::from_str(&content)
                .with_context(|| format!("failed to parse settings: {}", path.display()))?;
            settings.merge(parsed);
        }
    }

    Ok(settings)
}

impl Settings {
    fn merge(&mut self, incoming: SettingsFile) {
        if let Some(map) = incoming.formally {
            for (key, value) in map {
                self.formally.insert(key, value);
            }
        }
        if let Some(system) = incoming.system {
            if let Some(languages) = system.languages {
                self.system_languages = languages;
            }
            if let Some(limit) = system.histories {
                if limit > 0 {
                    self.history_limit = limit;
                }
            }
            if let Some(suffix) = system.translated_suffix {
                self.translated_suffix = suffix;
            }
            if let Some(ttl) = system.backup_ttl_days {
                if ttl > 0 {
                    self.backup_ttl_days = ttl;
                }
            }
            if let Some(threads) = system.directory_translation_threads {
                if threads > 0 {
                    self.directory_translation_threads = threads;
                }
            }
            if let Some(ignore_file) = system.translation_ignore_file {
                if !ignore_file.trim().is_empty() {
                    self.translation_ignore_file = ignore_file;
                }
            }
        }
        if let Some(ocr) = incoming.ocr {
            if let Some(color) = ocr.text_color {
                if !color.trim().is_empty() {
                    self.overlay_text_color = color;
                }
            }
            if let Some(color) = ocr.stroke_color {
                if !color.trim().is_empty() {
                    self.overlay_stroke_color = color;
                }
            }
            if let Some(color) = ocr.fill_color {
                if !color.trim().is_empty() {
                    self.overlay_fill_color = color;
                }
            }
            if let Some(size) = ocr.font_size {
                if size > 0.0 {
                    self.overlay_font_size = Some(size);
                }
            }
            if let Some(family) = ocr.font_family {
                if !family.trim().is_empty() {
                    self.overlay_font_family = Some(family);
                }
            }
            if let Some(path) = ocr.font_path {
                if !path.trim().is_empty() {
                    self.overlay_font_path = Some(path);
                }
            }
            if let Some(normalize) = ocr.normalize {
                self.ocr_normalize = normalize;
            }
        }
        if let Some(whisper) = incoming.whisper {
            if let Some(model) = whisper.model {
                if !model.trim().is_empty() {
                    self.whisper_model = Some(model);
                }
            }
        }
    }
}

fn ensure_home_settings_file() -> Result<()> {
    let Some(home) = home_dir() else {
        return Ok(());
    };
    fs::create_dir_all(&home)
        .with_context(|| format!("failed to create settings directory: {}", home.display()))?;
    let path = home.join("settings.toml");
    if !path.exists() {
        fs::write(&path, DEFAULT_SETTINGS_TOML)
            .with_context(|| format!("failed to write settings: {}", path.display()))?;
    }
    Ok(())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().and_then(|home| {
        let home = home.trim();
        if home.is_empty() {
            None
        } else {
            Some(Path::new(home).join(".llm-translator-rust"))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_temp_home;

    #[test]
    fn settings_override_translated_suffix_and_backup_ttl() {
        with_temp_home(|home| {
            let custom_path = home.join("override.toml");
            let content = r#"
[system]
translated_suffix = "_xlat"
backup_ttl_days = 7
histories = 42
"#;
            fs::write(&custom_path, content).expect("write settings");

            let settings = load_settings(Some(&custom_path)).expect("load settings");
            assert_eq!(settings.translated_suffix, "_xlat");
            assert_eq!(settings.backup_ttl_days, 7);
            assert_eq!(settings.history_limit, 42);
        });
    }
}
