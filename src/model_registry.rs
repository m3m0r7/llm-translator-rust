use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::providers::ProviderKind;

const TTL_SECONDS: u64 = 60 * 60 * 24;

#[derive(Debug, Serialize, Deserialize, Default)]
struct MetaCache {
    #[serde(rename = "lastUsingModel")]
    last_using_model: Option<String>,
    #[serde(rename = "lastFetchedModelDateTime", alias = "lastUpdatedTime")]
    last_fetched_model_datetime: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    histories: Vec<HistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum HistoryType {
    Text,
    Attachment,
}

impl HistoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HistoryType::Text => "text",
            HistoryType::Attachment => "attachment",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    pub datetime: String,
    pub model: String,
    pub mime: String,
    #[serde(rename = "type")]
    pub kind: HistoryType,
    pub src: String,
    pub dest: String,
}

pub async fn get_models(provider: ProviderKind, key: &str) -> Result<Vec<String>> {
    let mut meta = read_meta()?;
    let prefix = provider_prefix(provider);
    let cached = models_for_provider(&meta.models, &prefix);
    if !cached.is_empty() && !is_expired(&meta) {
        return Ok(cached);
    }

    let models = fetch_models(provider, key).await?;
    update_provider_models(&mut meta.models, &prefix, &models);
    meta.last_fetched_model_datetime = Some(now_unix().to_string());
    write_meta(&meta)?;
    Ok(models)
}

fn base_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Path::new(&home).join(".llm-translator/.cache");
        }
    }
    Path::new(".llm-translator/.cache").to_path_buf()
}

fn history_dest_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Path::new(&home).join(".llm-translator-rust/.cache/dest");
        }
    }
    Path::new(".llm-translator-rust/.cache/dest").to_path_buf()
}

pub fn get_last_using_model() -> Result<Option<String>> {
    let meta = read_meta()?;
    Ok(meta.last_using_model)
}

pub fn set_last_using_model(provider: ProviderKind, model: &str) -> Result<()> {
    let mut meta = read_meta()?;
    meta.last_using_model = Some(format!("{}:{}", provider.as_str(), model));
    write_meta(&meta)?;
    Ok(())
}

pub fn get_histories() -> Result<Vec<HistoryEntry>> {
    let meta = read_meta()?;
    Ok(meta.histories)
}

pub fn record_history(entry: HistoryEntry, limit: usize) -> Result<()> {
    let mut meta = read_meta()?;
    meta.histories.insert(0, entry);
    if limit > 0 && meta.histories.len() > limit {
        let removed = meta.histories.split_off(limit);
        for item in removed {
            if matches!(item.kind, HistoryType::Attachment) {
                let path = PathBuf::from(item.dest);
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    write_meta(&meta)?;
    Ok(())
}

pub fn write_history_dest(content: &str, salt: &str) -> Result<String> {
    let hash_input = format!("{}:{}", salt, content);
    let digest = format!("{:x}", md5::compute(hash_input.as_bytes()));
    let dir = history_dest_dir();
    fs::create_dir_all(&dir).with_context(|| "failed to create history dest directory")?;
    let path = dir.join(&digest);
    fs::write(&path, content).with_context(|| "failed to write history dest")?;
    Ok(path.to_string_lossy().to_string())
}

pub fn write_history_dest_bytes(bytes: &[u8], salt: &str) -> Result<String> {
    let mut hash_input = salt.as_bytes().to_vec();
    hash_input.extend_from_slice(bytes);
    let digest = format!("{:x}", md5::compute(&hash_input));
    let dir = history_dest_dir();
    fs::create_dir_all(&dir).with_context(|| "failed to create history dest directory")?;
    let path = dir.join(&digest);
    fs::write(&path, bytes).with_context(|| "failed to write history dest")?;
    Ok(path.to_string_lossy().to_string())
}

fn meta_path() -> PathBuf {
    base_cache_dir().join("meta.json")
}

fn read_meta() -> Result<MetaCache> {
    let path = meta_path();
    if !path.exists() {
        return Ok(MetaCache::default());
    }

    let content = fs::read_to_string(path).with_context(|| "failed to read meta cache")?;
    let entry: MetaCache =
        serde_json::from_str(&content).with_context(|| "failed to parse meta cache JSON")?;
    Ok(entry)
}

fn write_meta(meta: &MetaCache) -> Result<()> {
    let path = meta_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| "failed to create cache directory")?;
    }

    let content = serde_json::to_string_pretty(meta)?;
    fs::write(path, content).with_context(|| "failed to write meta cache")?;
    Ok(())
}

fn is_expired(meta: &MetaCache) -> bool {
    let Some(ts) = meta
        .last_fetched_model_datetime
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return true;
    };
    now_unix().saturating_sub(ts) > TTL_SECONDS
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

fn provider_prefix(provider: ProviderKind) -> String {
    format!("{}:", provider.as_str())
}

fn models_for_provider(models: &[String], prefix: &str) -> Vec<String> {
    models
        .iter()
        .filter_map(|model| model.strip_prefix(prefix).map(|value| value.to_string()))
        .collect()
}

fn update_provider_models(meta_models: &mut Vec<String>, prefix: &str, models: &[String]) {
    meta_models.retain(|model| !model.starts_with(prefix));
    meta_models.extend(models.iter().map(|model| format!("{}{}", prefix, model)));
    meta_models.sort();
    meta_models.dedup();
}

async fn fetch_models(provider: ProviderKind, key: &str) -> Result<Vec<String>> {
    match provider {
        ProviderKind::OpenAI => fetch_openai_models(key).await,
        ProviderKind::Gemini => fetch_gemini_models(key).await,
        ProviderKind::Claude => fetch_claude_models(key).await,
    }
}

async fn fetch_openai_models(key: &str) -> Result<Vec<String>> {
    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let url = format!("{}/models", base_url.trim_end_matches('/'));

    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(key)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "OpenAI models API error ({}): {}",
            status,
            extract_openai_error(&body).unwrap_or(body)
        ));
    }

    let payload: OpenAIModelsResponse =
        serde_json::from_str(&body).with_context(|| "failed to parse OpenAI models list")?;
    let mut models = payload
        .data
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    models.sort();
    Ok(models)
}

async fn fetch_gemini_models(key: &str) -> Result<Vec<String>> {
    let url = "https://generativelanguage.googleapis.com/v1beta/models";
    let response = reqwest::Client::new()
        .get(url)
        .header("x-goog-api-key", key)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Gemini models API error ({}): {}",
            status,
            extract_gemini_error(&body).unwrap_or(body)
        ));
    }

    let payload: GeminiModelsResponse =
        serde_json::from_str(&body).with_context(|| "failed to parse Gemini models list")?;
    let mut models = payload
        .models
        .into_iter()
        .filter_map(|item| item.name)
        .map(|name| name.strip_prefix("models/").unwrap_or(&name).to_string())
        .collect::<Vec<_>>();
    models.sort();
    Ok(models)
}

async fn fetch_claude_models(key: &str) -> Result<Vec<String>> {
    let url = "https://api.anthropic.com/v1/models";
    let response = reqwest::Client::new()
        .get(url)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Claude models API error ({}): {}",
            status,
            extract_claude_error(&body).unwrap_or(body)
        ));
    }

    let payload: ClaudeModelsResponse =
        serde_json::from_str(&body).with_context(|| "failed to parse Claude models list")?;
    let mut models = payload
        .data
        .into_iter()
        .filter_map(|item| item.id)
        .collect::<Vec<_>>();
    models.sort();
    Ok(models)
}

fn extract_openai_error(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<OpenAIError>,
    }

    #[derive(Deserialize)]
    struct OpenAIError {
        message: Option<String>,
        #[serde(rename = "type")]
        kind: Option<String>,
        code: Option<String>,
    }

    let parsed: ErrorBody = serde_json::from_str(body).ok()?;
    let error = parsed.error?;
    Some(format_error_parts(error.message, error.kind, error.code))
}

fn extract_gemini_error(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<GeminiError>,
    }

    #[derive(Deserialize)]
    struct GeminiError {
        message: Option<String>,
        status: Option<String>,
        code: Option<i32>,
    }

    let parsed: ErrorBody = serde_json::from_str(body).ok()?;
    let error = parsed.error?;
    Some(format_error_parts(
        error.message,
        error.status,
        error.code.map(|value| value.to_string()),
    ))
}

fn extract_claude_error(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<ClaudeError>,
    }

    #[derive(Deserialize)]
    struct ClaudeError {
        #[serde(rename = "type")]
        kind: Option<String>,
        message: Option<String>,
    }

    let parsed: ErrorBody = serde_json::from_str(body).ok()?;
    let error = parsed.error?;
    Some(format_error_parts(error.message, error.kind, None))
}

fn format_error_parts(
    message: Option<String>,
    kind: Option<String>,
    code: Option<String>,
) -> String {
    let mut parts = Vec::new();
    if let Some(message) = message {
        if !message.trim().is_empty() {
            parts.push(message);
        }
    }
    if let Some(kind) = kind {
        if !kind.trim().is_empty() {
            parts.push(format!("type: {}", kind));
        }
    }
    if let Some(code) = code {
        if !code.trim().is_empty() {
            parts.push(format!("code: {}", code));
        }
    }
    if parts.is_empty() {
        "unknown error".to_string()
    } else {
        parts.join(" | ")
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIModelsResponse {
    data: Vec<OpenAIModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModel {
    id: String,
}

#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModel>,
}

#[derive(Debug, Deserialize)]
struct GeminiModel {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeModelsResponse {
    data: Vec<ClaudeModel>,
}

#[derive(Debug, Deserialize)]
struct ClaudeModel {
    id: Option<String>,
}
