use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const BUILD_ENV_TOML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/build_env.toml"));

macro_rules! build_env_toml {
    () => {
        BUILD_ENV_TOML
    };
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BuildEnv {
    #[serde(rename = "baseDirectory")]
    base_directory: String,
    #[allow(dead_code)]
    #[serde(rename = "binDirectory")]
    bin_directory: String,
    #[allow(dead_code)]
    #[serde(rename = "installDirectory")]
    install_directory: String,
    #[serde(rename = "settingsFile")]
    settings_file: String,
}

impl Default for BuildEnv {
    fn default() -> Self {
        Self {
            base_directory: "~/.llm-translator-rust".to_string(),
            bin_directory: "target/release".to_string(),
            install_directory: "/usr/local/bin".to_string(),
            settings_file: "~/.llm-translator-rust/settings.toml".to_string(),
        }
    }
}

static BUILD_ENV: OnceLock<BuildEnv> = OnceLock::new();

const BASE_DIR_ENV: &str = "LLM_TRANSLATOR_RUST_DIR";

pub(crate) fn build_env() -> &'static BuildEnv {
    BUILD_ENV
        .get_or_init(|| toml::from_str(build_env_toml!()).unwrap_or_else(|_| BuildEnv::default()))
}

pub(crate) fn base_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir;
    }
    normalize_dir(&build_env().base_directory)
        .unwrap_or_else(|| PathBuf::from(".llm-translator-rust"))
}

pub(crate) fn settings_file() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join("settings.toml");
    }
    let raw = build_env().settings_file.trim();
    if raw.is_empty() {
        return base_dir().join("settings.toml");
    }
    normalize_path(PathBuf::from(expand_home(raw)))
}

pub(crate) fn settings_dir() -> PathBuf {
    let file = settings_file();
    file.parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(base_dir)
}

pub(crate) fn cache_dir() -> PathBuf {
    base_dir().join(".cache")
}

pub(crate) fn history_dest_dir() -> PathBuf {
    cache_dir().join("dest")
}

pub(crate) fn whisper_dir() -> PathBuf {
    cache_dir().join("whisper")
}

pub(crate) fn ocr_dir() -> PathBuf {
    cache_dir().join("ocr")
}

pub(crate) fn backup_dir() -> PathBuf {
    base_dir().join("backup")
}

fn base_dir_override() -> Option<PathBuf> {
    std::env::var(BASE_DIR_ENV)
        .ok()
        .and_then(|value| normalize_dir(&value))
}

fn normalize_dir(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = expand_home(trimmed);
    Some(normalize_path(PathBuf::from(expanded)))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        normalized.push(component.as_os_str());
    }
    normalized
}

fn expand_home(value: &str) -> String {
    if value == "~" {
        return env_home().unwrap_or_else(|| value.to_string());
    }
    if let Some(stripped) = value.strip_prefix("~/") {
        if let Some(home) = env_home() {
            let joined = Path::new(&home).join(stripped);
            return joined.to_string_lossy().to_string();
        }
    }
    value.to_string()
}

fn env_home() -> Option<String> {
    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim();
        if !home.is_empty() {
            return Some(home.to_string());
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        let home = home.trim();
        if !home.is_empty() {
            return Some(home.to_string());
        }
    }
    None
}
