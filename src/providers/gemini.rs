use anyhow::anyhow;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value, json};

use super::retry::{
    RATE_LIMIT_BASE_DELAY, RATE_LIMIT_MAX_RETRIES, is_rate_limited, retry_after, wait_with_backoff,
};
use super::{
    Message, MessagePart, MessageRole, Provider, ProviderFuture, ProviderResponse, ProviderUsage,
    ToolSpec,
};

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
pub(crate) const DEFAULT_MODEL: &str = "gemini-1.5-flash";

#[derive(Debug, Clone)]
pub struct Gemini {
    key: String,
    model: String,
    messages: Vec<Message>,
    tools: Vec<ToolSpec>,
}

impl Gemini {
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

impl Provider for Gemini {
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
            let url = format!("{}/{}:generateContent", BASE_URL, self.model);

            let (system_inputs, user_inputs): (Vec<Message>, Vec<Message>) = self
                .messages
                .into_iter()
                .partition(|message| matches!(message.role, MessageRole::System));

            let system_instruction = system_inputs
                .into_iter()
                .flat_map(|message| message.parts)
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text),
                    MessagePart::Data(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            let contents = user_inputs
                .into_iter()
                .map(|message| {
                    let parts = message
                        .parts
                        .into_iter()
                        .map(|part| match part {
                            MessagePart::Text(text) => json!({"text": text}),
                            MessagePart::Data(data) => {
                                let encoded = BASE64.encode(&data.bytes);
                                json!({
                                    "inline_data": {
                                        "mime_type": data.mime,
                                        "data": encoded
                                    }
                                })
                            }
                        })
                        .collect::<Vec<_>>();
                    json!({
                        "role": "user",
                        "parts": parts
                    })
                })
                .collect::<Vec<_>>();

            let body = json!({
                "contents": contents,
                "systemInstruction": if system_instruction.trim().is_empty() { Value::Null } else { json!({"parts": [{"text": system_instruction}]}) },
                "tools": [
                    {
                        "function_declarations": [
                            {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters
                            }
                        ]
                    }
                ],
                "tool_config": {
                    "function_calling_config": {
                        "mode": "ANY",
                        "allowed_function_names": [tool.name]
                    }
                }
            });

            let mut attempt = 0usize;
            let mut delay = RATE_LIMIT_BASE_DELAY;
            loop {
                attempt += 1;
                let response = client
                    .post(&url)
                    .header("x-goog-api-key", self.key.clone())
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
                    delay = wait_with_backoff("Gemini", attempt, delay, retry_after).await;
                    continue;
                }
                return Err(anyhow!(
                    "Gemini API error ({}): {}",
                    status,
                    extract_gemini_error(&text).unwrap_or(text)
                ));
            }
        })
    }
}

fn extract_tool_response(
    text: &str,
    tool_name: &str,
    fallback_model: &str,
) -> Result<ProviderResponse, anyhow::Error> {
    let payload: GeminiResponse = serde_json::from_str(text)
        .map_err(|err| anyhow!("failed to parse Gemini response JSON: {}", err))?;
    let candidate = payload
        .candidates
        .first()
        .and_then(|candidate| candidate.content.as_ref())
        .ok_or_else(|| anyhow!("no candidate returned from Gemini"))?;

    for part in &candidate.parts {
        if let Some(function_call) = &part.function_call
            && function_call.name == tool_name
        {
            let model = payload
                .model_version
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(fallback_model.to_string()));
            let usage = payload.usage_metadata.map(|usage| ProviderUsage {
                prompt_tokens: usage.prompt_token_count,
                completion_tokens: usage.candidates_token_count,
                total_tokens: usage.total_token_count,
            });
            return Ok(ProviderResponse {
                args: function_call.args.clone(),
                model,
                usage,
            });
        }
    }

    Err(anyhow!("no tool call returned from Gemini"))
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
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
    #[serde(rename = "modelVersion")]
    model_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: Value,
}

#[cfg(test)]
mod tests {
    use super::extract_tool_response;
    use insta::assert_json_snapshot;

    #[test]
    fn gemini_extract_tool_args_snapshot() {
        let payload = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/gemini_tool_response.json"
        ));
        let response =
            extract_tool_response(payload, "deliver_translation", "gemini-1.5-flash").unwrap();
        assert_json_snapshot!(response);
    }
}
