use anyhow::{anyhow, Result};
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;

use crate::data::DataAttachment;

mod claude;
mod gemini;
mod openai;

pub use claude::Claude;
pub use gemini::Gemini;
pub use openai::OpenAI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAI,
    Gemini,
    Claude,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenAI => "openai",
            ProviderKind::Gemini => "gemini",
            ProviderKind::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSelection {
    pub provider: ProviderKind,
    pub requested_model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResponse {
    pub args: serde_json::Value,
    pub model: Option<String>,
    pub usage: Option<ProviderUsage>,
}

#[derive(Debug, Clone, Copy)]
pub enum MessageRole {
    System,
    User,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: MessageRole,
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Clone)]
pub enum MessagePart {
    Text(String),
    Data(DataAttachment),
}

impl Message {
    pub fn system(content: String) -> Self {
        Self {
            role: MessageRole::System,
            parts: vec![MessagePart::Text(content)],
        }
    }

    pub fn user(content: String) -> Self {
        Self {
            role: MessageRole::User,
            parts: vec![MessagePart::Text(content)],
        }
    }

    pub fn user_data(data: DataAttachment) -> Self {
        Self {
            role: MessageRole::User,
            parts: vec![MessagePart::Data(data)],
        }
    }

    pub fn has_data(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, MessagePart::Data(_)))
    }
}

pub type ProviderFuture = Pin<Box<dyn Future<Output = Result<ProviderResponse>> + Send>>;

pub trait Provider: Clone + Send + Sync {
    fn append_system_input(self, input: String) -> Self;
    fn append_user_input(self, input: String) -> Self;
    fn append_user_data(self, data: DataAttachment) -> Self;
    fn register_tool(self, tool: ToolSpec) -> Self;
    fn call_tool(self, tool_name: &str) -> ProviderFuture;
}

#[derive(Debug, Clone)]
pub enum ProviderImpl {
    OpenAI(OpenAI),
    Gemini(Gemini),
    Claude(Claude),
}

impl Provider for ProviderImpl {
    fn append_system_input(self, input: String) -> Self {
        match self {
            ProviderImpl::OpenAI(provider) => {
                ProviderImpl::OpenAI(provider.append_system_input(input))
            }
            ProviderImpl::Gemini(provider) => {
                ProviderImpl::Gemini(provider.append_system_input(input))
            }
            ProviderImpl::Claude(provider) => {
                ProviderImpl::Claude(provider.append_system_input(input))
            }
        }
    }

    fn append_user_input(self, input: String) -> Self {
        match self {
            ProviderImpl::OpenAI(provider) => {
                ProviderImpl::OpenAI(provider.append_user_input(input))
            }
            ProviderImpl::Gemini(provider) => {
                ProviderImpl::Gemini(provider.append_user_input(input))
            }
            ProviderImpl::Claude(provider) => {
                ProviderImpl::Claude(provider.append_user_input(input))
            }
        }
    }

    fn append_user_data(self, data: DataAttachment) -> Self {
        match self {
            ProviderImpl::OpenAI(provider) => ProviderImpl::OpenAI(provider.append_user_data(data)),
            ProviderImpl::Gemini(provider) => ProviderImpl::Gemini(provider.append_user_data(data)),
            ProviderImpl::Claude(provider) => ProviderImpl::Claude(provider.append_user_data(data)),
        }
    }

    fn register_tool(self, tool: ToolSpec) -> Self {
        match self {
            ProviderImpl::OpenAI(provider) => ProviderImpl::OpenAI(provider.register_tool(tool)),
            ProviderImpl::Gemini(provider) => ProviderImpl::Gemini(provider.register_tool(tool)),
            ProviderImpl::Claude(provider) => ProviderImpl::Claude(provider.register_tool(tool)),
        }
    }

    fn call_tool(self, tool_name: &str) -> ProviderFuture {
        match self {
            ProviderImpl::OpenAI(provider) => provider.call_tool(tool_name),
            ProviderImpl::Gemini(provider) => provider.call_tool(tool_name),
            ProviderImpl::Claude(provider) => provider.call_tool(tool_name),
        }
    }
}

pub fn build_provider(provider: ProviderKind, key: String, model: String) -> ProviderImpl {
    match provider {
        ProviderKind::OpenAI => ProviderImpl::OpenAI(OpenAI::new(key).with_model(model)),
        ProviderKind::Gemini => ProviderImpl::Gemini(Gemini::new(key).with_model(model)),
        ProviderKind::Claude => ProviderImpl::Claude(Claude::new(key).with_model(model)),
    }
}

pub fn resolve_provider_selection(
    model_arg: Option<&str>,
    override_key: Option<&str>,
) -> Result<ProviderSelection> {
    match model_arg {
        Some(model) => parse_model_arg(model),
        None => default_provider_selection(override_key),
    }
}

pub fn resolve_key(provider: ProviderKind, override_key: Option<&str>) -> Result<String> {
    if let Some(key) = override_key {
        return Ok(key.to_string());
    }

    match provider {
        ProviderKind::OpenAI => get_env("OPENAI_API_KEY"),
        ProviderKind::Gemini => get_env("GEMINI_API_KEY").or_else(|| get_env("GOOGLE_API_KEY")),
        ProviderKind::Claude => get_env("ANTHROPIC_API_KEY"),
    }
    .ok_or_else(|| anyhow!("API key not found for provider"))
}

fn default_provider_selection(override_key: Option<&str>) -> Result<ProviderSelection> {
    if get_env("OPENAI_API_KEY").is_some() {
        return Ok(ProviderSelection {
            provider: ProviderKind::OpenAI,
            requested_model: None,
        });
    }

    if get_env("GEMINI_API_KEY").is_some() || get_env("GOOGLE_API_KEY").is_some() {
        return Ok(ProviderSelection {
            provider: ProviderKind::Gemini,
            requested_model: None,
        });
    }

    if get_env("ANTHROPIC_API_KEY").is_some() {
        return Ok(ProviderSelection {
            provider: ProviderKind::Claude,
            requested_model: None,
        });
    }

    if override_key.is_some() {
        return Ok(ProviderSelection {
            provider: ProviderKind::OpenAI,
            requested_model: None,
        });
    }

    Err(anyhow!(
        "no API keys found (checked OPENAI_API_KEY, GEMINI_API_KEY/GOOGLE_API_KEY, ANTHROPIC_API_KEY)"
    ))
}

fn parse_model_arg(model_arg: &str) -> Result<ProviderSelection> {
    let raw = model_arg.trim();
    if raw.is_empty() {
        return Err(anyhow!("model argument is empty"));
    }

    let lower = raw.to_lowercase();
    if let Some(provider) = provider_from_name(&lower) {
        return Ok(ProviderSelection {
            provider,
            requested_model: None,
        });
    }

    if let Some((provider, model)) = parse_provider_model_pair(raw) {
        return Ok(ProviderSelection {
            provider,
            requested_model: model,
        });
    }

    Err(anyhow!(
        "unable to infer provider from model '{}'. Use provider:model (openai:, gemini:, claude:)",
        raw
    ))
}

fn parse_provider_model_pair(input: &str) -> Option<(ProviderKind, Option<String>)> {
    let (provider_part, model_part) = input.split_once(':')?;
    let provider = provider_from_name(&provider_part.to_lowercase())?;
    let model = if model_part.trim().is_empty() {
        None
    } else {
        Some(model_part.trim().to_string())
    };
    Some((provider, model))
}

fn provider_from_name(name: &str) -> Option<ProviderKind> {
    match name {
        "openai" => Some(ProviderKind::OpenAI),
        "gemini" | "google" => Some(ProviderKind::Gemini),
        "claude" | "anthropic" => Some(ProviderKind::Claude),
        _ => None,
    }
}

fn get_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
