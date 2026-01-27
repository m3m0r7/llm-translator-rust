use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Settings {
    pub formally: HashMap<String, String>,
    pub system_languages: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SettingsFile {
    formally: Option<HashMap<String, String>>,
    system: Option<SystemSettings>,
}

#[derive(Debug, Default, Deserialize)]
struct SystemSettings {
    languages: Option<Vec<String>>,
}

pub fn load_settings(extra_path: Option<&Path>) -> Result<Settings> {
    let mut settings = Settings::default();

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
        }
    }
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
