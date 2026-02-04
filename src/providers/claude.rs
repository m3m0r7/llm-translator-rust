use anyhow::anyhow;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::json;

use super::retry::{
    RATE_LIMIT_BASE_DELAY, RATE_LIMIT_MAX_RETRIES, is_rate_limited, retry_after, wait_with_backoff,
};
use super::{
    Message, MessagePart, MessageRole, Provider, ProviderFuture, ProviderResponse, ProviderUsage,
    ToolSpec,
};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1/messages";
pub(crate) const DEFAULT_MODEL: &str = "claude-3-5-sonnet-latest";

#[derive(Debug, Clone)]
pub struct Claude {
    key: String,
    model: String,
    messages: Vec<Message>,
    tools: Vec<ToolSpec>,
}

impl Claude {
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

    fn find_tool(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.iter().find(|tool| tool.name == name)
    }
}

impl Provider for Claude {
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
            let tool = self
                .find_tool(&tool_name)
                .cloned()
                .ok_or_else(|| anyhow!("tool '{}' not registered", tool_name))?;
            let client = reqwest::Client::new();
            let url = base_url();

            let (system_inputs, user_inputs): (Vec<Message>, Vec<Message>) = self
                .messages
                .into_iter()
                .partition(|message| matches!(message.role, MessageRole::System));

            let system = system_inputs
                .into_iter()
                .flat_map(|message| message.parts)
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text),
                    MessagePart::Data(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            let messages = user_inputs
                .into_iter()
                .map(|message| {
                    let content = message
                        .parts
                        .into_iter()
                        .map(|part| match part {
                            MessagePart::Text(text) => json!({"type": "text", "text": text}),
                            MessagePart::Data(data) => {
                                let encoded = BASE64.encode(&data.bytes);
                                if data.mime.starts_with("image/") {
                                    json!({
                                        "type": "image",
                                        "source": {
                                            "type": "base64",
                                            "media_type": data.mime,
                                            "data": encoded
                                        }
                                    })
                                } else {
                                    json!({
                                        "type": "document",
                                        "source": {
                                            "type": "base64",
                                            "media_type": data.mime,
                                            "data": encoded
                                        }
                                    })
                                }
                            }
                        })
                        .collect::<Vec<_>>();
                    json!({
                        "role": "user",
                        "content": content
                    })
                })
                .collect::<Vec<_>>();

            let system_value = if system.trim().is_empty() {
                json!(null)
            } else {
                json!(system)
            };

            let body = json!({
                "model": self.model,
                "max_tokens": 1024,
                "messages": messages,
                "system": system_value,
                "tools": [
                    {
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters
                    }
                ],
                "tool_choice": {"type": "tool", "name": tool.name}
            });

            let mut attempt = 0usize;
            let mut delay = RATE_LIMIT_BASE_DELAY;
            loop {
                attempt += 1;
                let response = client
                    .post(&url)
                    .header("x-api-key", self.key.clone())
                    .header("anthropic-version", "2023-06-01")
                    .json(&body)
                    .send()
                    .await?;

                let status = response.status();
                let retry_after = retry_after(response.headers());
                let text = response.text().await.unwrap_or_default();
                if status.is_success() {
                    return extract_tool_response(&text, &tool_name, &self.model);
                }
                if is_rate_limited(status, &text) && attempt < RATE_LIMIT_MAX_RETRIES {
                    delay = wait_with_backoff("Claude", attempt, delay, retry_after).await;
                    continue;
                }
                return Err(anyhow!(
                    "Claude API error ({}): {}",
                    status,
                    extract_claude_error(&text).unwrap_or(text)
                ));
            }
        })
    }
}

fn base_url() -> String {
    std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

fn extract_tool_response(
    text: &str,
    tool_name: &str,
    fallback_model: &str,
) -> Result<ProviderResponse, anyhow::Error> {
    let payload: ClaudeResponse = serde_json::from_str(text)
        .map_err(|err| anyhow!("failed to parse Claude response JSON: {}", err))?;
    for block in &payload.content {
        if block.kind == "tool_use" && block.name.as_deref() == Some(tool_name) {
            let input = block
                .input
                .clone()
                .ok_or_else(|| anyhow!("Claude tool_use missing input"))?;
            let model = payload
                .model
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(fallback_model.to_string()));
            let usage = payload.usage.map(|usage| ProviderUsage {
                prompt_tokens: usage.input_tokens,
                completion_tokens: usage.output_tokens,
                total_tokens: usage
                    .input_tokens
                    .zip(usage.output_tokens)
                    .map(|(input, output)| input + output),
            });
            return Ok(ProviderResponse {
                args: input,
                model,
                usage,
            });
        }
    }

    Err(anyhow!("no tool call returned from Claude"))
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
    if let Some(message) = message
        && !message.trim().is_empty()
    {
        parts.push(message);
    }
    if let Some(kind) = kind
        && !kind.trim().is_empty()
    {
        parts.push(format!("type: {}", kind));
    }
    if let Some(code) = code
        && !code.trim().is_empty()
    {
        parts.push(format!("code: {}", code));
    }
    if parts.is_empty() {
        "unknown error".to_string()
    } else {
        parts.join(" | ")
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ClaudeContent {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::extract_tool_response;
    use insta::assert_json_snapshot;

    #[test]
    fn claude_extract_tool_args_snapshot() {
        let payload = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/claude_tool_response.json"
        ));
        let response =
            extract_tool_response(payload, "deliver_translation", "claude-3-5-sonnet-latest")
                .unwrap();
        assert_json_snapshot!(response);
    }
}
