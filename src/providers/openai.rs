use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;

use super::retry::{
    is_rate_limited, retry_after, wait_with_backoff, RATE_LIMIT_BASE_DELAY, RATE_LIMIT_MAX_RETRIES,
};
use super::{
    Message, MessagePart, MessageRole, Provider, ProviderFuture, ProviderResponse, ProviderUsage,
    ToolSpec,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub(crate) const DEFAULT_MODEL: &str = "gpt-4o-mini";

#[derive(Debug, Clone)]
pub struct OpenAI {
    key: String,
    model: String,
    messages: Vec<Message>,
    tools: Vec<ToolSpec>,
}

impl OpenAI {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            model: DEFAULT_MODEL.to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        if !model.trim().is_empty() {
            self.model = model;
        }
        self
    }

    fn find_tool(&self, name: &str) -> Result<&ToolSpec> {
        self.tools
            .iter()
            .find(|tool| tool.name == name)
            .ok_or_else(|| anyhow!("tool '{}' not registered", name))
    }
}

impl Provider for OpenAI {
    fn append_system_input(mut self, input: String) -> Self {
        self.messages.push(Message::system(input));
        self
    }

    fn append_user_input(mut self, input: String) -> Self {
        self.messages.push(Message::user(input));
        self
    }

    fn append_user_data(mut self, data: crate::data::DataAttachment) -> Self {
        self.messages.push(Message::user_data(data));
        self
    }

    fn register_tool(mut self, tool: ToolSpec) -> Self {
        self.tools.push(tool);
        self
    }

    fn call_tool(self, tool_name: &str) -> ProviderFuture {
        let tool_name = tool_name.to_string();
        Box::pin(async move {
            let tool = self.find_tool(&tool_name)?.clone();
            if has_data(&self.messages) {
                call_with_responses(self, tool, &tool_name).await
            } else {
                call_with_chat_completions(self, tool, &tool_name).await
            }
        })
    }
}

fn base_url() -> String {
    std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

fn message_text(message: &Message) -> Result<String> {
    let mut parts = Vec::new();
    for part in &message.parts {
        match part {
            MessagePart::Text(text) => parts.push(text.as_str()),
            MessagePart::Data(_) => {
                return Err(anyhow!("binary data cannot be sent via chat completions"))
            }
        }
    }
    Ok(parts.join("\n\n"))
}

fn has_data(messages: &[Message]) -> bool {
    messages.iter().any(Message::has_data)
}

fn system_text(messages: &[Message]) -> Result<String> {
    let mut parts = Vec::new();
    for message in messages {
        if matches!(message.role, MessageRole::System) {
            parts.push(message_text(message)?);
        }
    }
    Ok(parts.join("\n\n"))
}

async fn call_with_chat_completions(
    provider: OpenAI,
    tool: ToolSpec,
    tool_name: &str,
) -> Result<ProviderResponse> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url());

    let messages = provider
        .messages
        .iter()
        .map(|message| match message.role {
            MessageRole::System => {
                let content = message_text(message)?;
                Ok(json!({"role": "system", "content": content}))
            }
            MessageRole::User => {
                let content = message_text(message)?;
                Ok(json!({"role": "user", "content": content}))
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let body = json!({
        "model": provider.model,
        "messages": messages,
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                }
            }
        ],
        "tool_choice": {"type": "function", "function": {"name": tool.name}}
    });

    let mut attempt = 0usize;
    let mut delay = RATE_LIMIT_BASE_DELAY;
    loop {
        attempt += 1;
        let response = client
            .post(&url)
            .bearer_auth(provider.key.clone())
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let retry_after = retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            return extract_tool_response(&text, tool_name, &provider.model);
        }
        if is_rate_limited(status, &text) && attempt < RATE_LIMIT_MAX_RETRIES {
            delay = wait_with_backoff("OpenAI", attempt, delay, retry_after).await;
            continue;
        }
        return Err(anyhow!(
            "OpenAI API error ({}): {}",
            status,
            extract_openai_error(&text).unwrap_or(text)
        ));
    }
}

async fn call_with_responses(
    provider: OpenAI,
    tool: ToolSpec,
    tool_name: &str,
) -> Result<ProviderResponse> {
    let client = reqwest::Client::new();
    let url = format!("{}/responses", base_url());

    let system = system_text(&provider.messages)?;
    let input = provider
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::User))
        .map(|message| {
            let parts = message
                .parts
                .iter()
                .map(|part| match part {
                    MessagePart::Text(text) => json!({"type": "input_text", "text": text}),
                    MessagePart::Data(data) => {
                        let encoded = BASE64.encode(&data.bytes);
                        if data.mime.starts_with("image/") {
                            let url = format!("data:{};base64,{}", data.mime, encoded);
                            json!({"type": "input_image", "image_url": url})
                        } else {
                            let filename = data
                                .name
                                .clone()
                                .unwrap_or_else(|| "attachment".to_string());
                            json!({"type": "input_file", "filename": filename, "file_data": encoded})
                        }
                    }
                })
                .collect::<Vec<_>>();
            json!({"role": "user", "content": parts})
        })
        .collect::<Vec<_>>();

    let mut body = json!({
        "model": provider.model,
        "input": input,
        "tools": [
            {
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters
            }
        ]
    });

    if !system.trim().is_empty() {
        body["instructions"] = json!(system);
    }

    let mut attempt = 0usize;
    let mut delay = RATE_LIMIT_BASE_DELAY;
    loop {
        attempt += 1;
        let response = client
            .post(&url)
            .bearer_auth(provider.key.clone())
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let retry_after = retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            return extract_response_tool_call(&text, tool_name, &provider.model);
        }
        if is_rate_limited(status, &text) && attempt < RATE_LIMIT_MAX_RETRIES {
            delay = wait_with_backoff("OpenAI", attempt, delay, retry_after).await;
            continue;
        }
        return Err(anyhow!(
            "OpenAI API error ({}): {}",
            status,
            extract_openai_error(&text).unwrap_or(text)
        ));
    }
}

fn extract_tool_response(
    text: &str,
    tool_name: &str,
    fallback_model: &str,
) -> Result<ProviderResponse> {
    let payload: OpenAIResponse =
        serde_json::from_str(text).with_context(|| "failed to parse OpenAI response JSON")?;
    let tool_call = payload
        .choices
        .first()
        .and_then(|choice| choice.message.tool_calls.first())
        .ok_or_else(|| anyhow!("no tool call returned from OpenAI"))?;

    if tool_call.function.name != tool_name {
        return Err(anyhow!(
            "unexpected tool name '{}' from OpenAI",
            tool_call.function.name
        ));
    }

    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
        .with_context(|| "failed to parse OpenAI tool arguments")?;
    let model = payload
        .model
        .filter(|value| !value.trim().is_empty())
        .or_else(|| Some(fallback_model.to_string()));
    let usage = payload.usage.map(|usage| ProviderUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    });
    Ok(ProviderResponse { args, model, usage })
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

fn extract_response_tool_call(
    text: &str,
    tool_name: &str,
    fallback_model: &str,
) -> Result<ProviderResponse> {
    let payload: ResponseApiResponse =
        serde_json::from_str(text).with_context(|| "failed to parse OpenAI response JSON")?;
    let tool_call = payload
        .output
        .iter()
        .find_map(|item| match item {
            ResponseOutputItem::FunctionCall { name, arguments } if name == tool_name => {
                Some(arguments)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow!("no tool call returned from OpenAI"))?;

    let args: serde_json::Value =
        serde_json::from_str(tool_call).with_context(|| "failed to parse OpenAI tool arguments")?;
    let model = payload
        .model
        .filter(|value| !value.trim().is_empty())
        .or_else(|| Some(fallback_model.to_string()));
    let usage = payload.usage.map(|usage| ProviderUsage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
    });
    Ok(ProviderResponse { args, model, usage })
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    model: Option<String>,
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    #[serde(default)]
    tool_calls: Vec<OpenAIToolCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    function: OpenAIFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ResponseApiResponse {
    model: Option<String>,
    #[serde(default)]
    output: Vec<ResponseOutputItem>,
    usage: Option<ResponseApiUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseOutputItem {
    #[serde(rename = "function_call")]
    FunctionCall { name: String, arguments: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct ResponseApiUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::extract_tool_response;
    use insta::assert_json_snapshot;

    #[test]
    fn openai_extract_tool_args_snapshot() {
        let payload = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/openai_tool_response.json"
        ));
        let response =
            extract_tool_response(payload, "deliver_translation", "gpt-4o-mini").unwrap();
        assert_json_snapshot!(response);
    }
}
