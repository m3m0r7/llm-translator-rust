use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const BUILD_ENV_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/build/build_env.toml"));

macro_rules! build_env_toml {
    () => {
        BUILD_ENV_TOML
    };
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BuildEnv {
    #[serde(rename = "dataDirectory", alias = "baseDirectory")]
    data_directory: String,
    #[allow(dead_code)]
    #[serde(rename = "binDirectory")]
    bin_directory: String,
    #[allow(dead_code)]
    #[serde(rename = "runtimeDirectory", alias = "installDirectory")]
    runtime_directory: String,
    #[allow(dead_code)]
    #[serde(rename = "configDirectory", default)]
    config_directory: String,
    #[serde(rename = "settingsFile", default)]
    settings_file: String,
}

impl Default for BuildEnv {
    fn default() -> Self {
        Self {
            data_directory: "$XDG_DATA_HOME/llm-translator-rust".to_string(),
            bin_directory: "target/release".to_string(),
            runtime_directory: "$XDG_RUNTIME_DIR".to_string(),
            config_directory: "$XDG_CONFIG_HOME/llm-translator-rust".to_string(),
            settings_file: "".to_string(),
        }
    }
}

static BUILD_ENV: OnceLock<BuildEnv> = OnceLock::new();

const BASE_DIR_ENV: &str = "LLM_TRANSLATOR_RUST_DIR";
const XDG_APP_DIR: &str = "llm-translator-rust";

pub(crate) fn build_env() -> &'static BuildEnv {
    BUILD_ENV
        .get_or_init(|| toml::from_str(build_env_toml!()).unwrap_or_else(|_| BuildEnv::default()))
}

pub(crate) fn base_dir() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir;
    }
    let raw = build_env().data_directory.trim();
    if !raw.is_empty()
        && let Some(dir) = normalize_dir(raw)
    {
        return dir;
    }
    default_data_dir()
}

pub(crate) fn settings_file() -> PathBuf {
    if let Some(dir) = base_dir_override() {
        return dir.join("settings.toml");
    }
    let raw = build_env().settings_file.trim();
    if !raw.is_empty() {
        return normalize_path(PathBuf::from(expand_path(raw)));
    }
    let config_raw = build_env().config_directory.trim();
    if !config_raw.is_empty() {
        let dir = normalize_path(PathBuf::from(expand_path(config_raw)));
        return dir.join("settings.toml");
    }
    if let Some(dir) = xdg_config_home() {
        return dir.join(XDG_APP_DIR).join("settings.toml");
    }
    base_dir().join("settings.toml")
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
    let expanded = expand_path(trimmed);
    Some(normalize_path(PathBuf::from(expanded)))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        normalized.push(component.as_os_str());
    }
    normalized
}

fn expand_path(value: &str) -> String {
    let expanded = expand_env_prefix(value);
    expand_home(&expanded)
}

fn expand_env_prefix(value: &str) -> String {
    let data_home = xdg_data_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_DATA_HOME", data_home) {
        return replaced;
    }
    let config_home = xdg_config_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_CONFIG_HOME", config_home) {
        return replaced;
    }
    let state_home = xdg_state_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_STATE_HOME", state_home) {
        return replaced;
    }
    let cache_home = xdg_cache_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_CACHE_HOME", cache_home) {
        return replaced;
    }
    if let Some(replaced) = expand_named_prefix(
        value,
        "XDG_RUNTIME_DIR",
        xdg_runtime_dir().map(|path| path.to_string_lossy().to_string()),
    ) {
        return replaced;
    }
    let home = env_home();
    if let Some(replaced) = expand_named_prefix(value, "HOME", home.clone()) {
        return replaced;
    }
    if let Some(replaced) = expand_named_prefix(value, "USERPROFILE", home) {
        return replaced;
    }
    value.to_string()
}

fn expand_named_prefix(value: &str, key: &str, fallback: Option<String>) -> Option<String> {
    let env_value = std::env::var(key).ok().and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    let replacement = env_value.or(fallback)?;
    let prefix = format!("${}", key);
    if let Some(rest) = value.strip_prefix(&prefix) {
        return Some(format!("{}{}", replacement, rest));
    }
    let brace_prefix = format!("${{{}}}", key);
    if let Some(rest) = value.strip_prefix(&brace_prefix) {
        return Some(format!("{}{}", replacement, rest));
    }
    None
}

fn expand_home(value: &str) -> String {
    if value == "~" {
        return env_home().unwrap_or_else(|| value.to_string());
    }
    if let Some(stripped) = value.strip_prefix("~/")
        && let Some(home) = env_home()
    {
        let joined = Path::new(&home).join(stripped);
        return joined.to_string_lossy().to_string();
    }
    value.to_string()
}

fn default_data_dir() -> PathBuf {
    xdg_data_home()
        .unwrap_or_else(|| PathBuf::from(".local/share"))
        .join(XDG_APP_DIR)
}

fn env_home() -> Option<String> {
    env_value("HOME").or_else(|| env_value("USERPROFILE"))
}

fn xdg_config_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_CONFIG_HOME") {
        return Some(normalize_path(PathBuf::from(expand_home(&home))));
    }
    env_home().map(|home| PathBuf::from(home).join(".config"))
}

fn xdg_data_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_DATA_HOME") {
        return Some(normalize_path(PathBuf::from(expand_home(&home))));
    }
    env_home().map(|home| PathBuf::from(home).join(".local/share"))
}

fn xdg_cache_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_CACHE_HOME") {
        return Some(normalize_path(PathBuf::from(expand_home(&home))));
    }
    env_home().map(|home| PathBuf::from(home).join(".cache"))
}

fn xdg_state_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_STATE_HOME") {
        return Some(normalize_path(PathBuf::from(expand_home(&home))));
    }
    env_home().map(|home| PathBuf::from(home).join(".local/state"))
}

fn xdg_runtime_dir() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_RUNTIME_DIR") {
        return Some(normalize_path(PathBuf::from(expand_home(&home))));
    }
    None
}

fn env_value(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
