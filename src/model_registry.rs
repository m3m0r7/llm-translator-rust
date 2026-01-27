use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::providers::ProviderKind;

const TTL_SECONDS: u64 = 60 * 60 * 24;

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    fetched_at: u64,
    models: Vec<String>,
}

pub async fn get_models(provider: ProviderKind, key: &str) -> Result<Vec<String>> {
    let cache_path = cache_file(provider);
    if let Some(entry) = read_cache(&cache_path)? {
        if !is_expired(entry.fetched_at) {
            return Ok(entry.models);
        }
    }

    let models = fetch_models(provider, key).await?;
    write_cache(&cache_path, &models)?;
    Ok(models)
}

fn cache_file(provider: ProviderKind) -> PathBuf {
    let filename = format!("models_{}.json", provider.as_str());
    base_cache_dir().join(filename)
}

fn base_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Path::new(&home).join(".cache/llm-translator-rust");
        }
    }
    Path::new(".cache/llm-translator-rust").to_path_buf()
}

fn read_cache(path: &Path) -> Result<Option<CacheEntry>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path).with_context(|| "failed to read model cache")?;
    let entry: CacheEntry =
        serde_json::from_str(&content).with_context(|| "failed to parse model cache JSON")?;
    Ok(Some(entry))
}

fn write_cache(path: &Path, models: &[String]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| "failed to create cache directory")?;
    }

    let entry = CacheEntry {
        fetched_at: now_unix(),
        models: models.to_vec(),
    };
    let content = serde_json::to_string_pretty(&entry)?;
    fs::write(path, content).with_context(|| "failed to write model cache")?;
    Ok(())
}

fn is_expired(fetched_at: u64) -> bool {
    now_unix().saturating_sub(fetched_at) > TTL_SECONDS
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
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
