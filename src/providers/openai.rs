use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;

use super::{
    Message, MessageRole, Provider, ProviderFuture, ProviderResponse, ProviderUsage, ToolSpec,
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

    fn register_tool(mut self, tool: ToolSpec) -> Self {
        self.tools.push(tool);
        self
    }

    fn call_tool(self, tool_name: &str) -> ProviderFuture {
        let tool_name = tool_name.to_string();
        Box::pin(async move {
            let tool = self.find_tool(&tool_name)?;
            let client = reqwest::Client::new();
            let url = format!("{}/chat/completions", base_url());

            let messages = self
                .messages
                .iter()
                .map(|message| match message.role {
                    MessageRole::System => json!({"role": "system", "content": message.content}),
                    MessageRole::User => json!({"role": "user", "content": message.content}),
                })
                .collect::<Vec<_>>();

            let body = json!({
                "model": self.model,
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

            let response = client
                .post(url)
                .bearer_auth(self.key)
                .json(&body)
                .send()
                .await?;

            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(anyhow!(
                    "OpenAI API error ({}): {}",
                    status,
                    extract_openai_error(&text).unwrap_or(text)
                ));
            }

            extract_tool_response(&text, &tool_name, &self.model)
        })
    }
}

fn base_url() -> String {
    std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
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
