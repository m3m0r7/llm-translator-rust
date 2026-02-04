use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::Config;
use crate::settings;

#[derive(Debug, Clone)]
pub struct McpDefaults {
    pub lang: String,
    pub source_lang: String,
    pub formal: String,
    pub slang: bool,
    pub model: Option<String>,
    pub key: Option<String>,
    pub settings_path: Option<String>,
}

pub async fn run_mcp(defaults: McpDefaults) -> Result<()> {
    let settings_path = defaults.settings_path.as_deref().map(std::path::Path::new);
    let settings = settings::load_settings(settings_path)?;

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = stdout;

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match handle_line(&line, &defaults, &settings).await {
            Ok(Some(value)) => Some(value),
            Ok(None) => None,
            Err(err) => Some(jsonrpc_error(None, -32603, &err.to_string())),
        };
        if let Some(value) = response {
            let payload = serde_json::to_vec(&value)?;
            writer.write_all(&payload).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    Ok(())
}

async fn handle_line(
    line: &str,
    defaults: &McpDefaults,
    settings: &settings::Settings,
) -> Result<Option<Value>> {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            return Ok(Some(jsonrpc_error(
                None,
                -32700,
                &format!("parse error: {}", err),
            )));
        }
    };
    let id = value.get("id").cloned();
    let method = match value.get("method").and_then(|method| method.as_str()) {
        Some(method) => method,
        None => return Ok(Some(jsonrpc_error(id, -32600, "invalid request"))),
    };
    let params = value.get("params").cloned().unwrap_or_else(|| json!({}));

    let response = match method {
        "initialize" => Some(jsonrpc_response(id, initialize_result(&params))),
        "tools/list" => Some(jsonrpc_response(id, tools_list_result())),
        "tools/call" => Some(jsonrpc_response(
            id,
            tools_call_result(params, defaults, settings).await,
        )),
        "resources/list" => Some(jsonrpc_response(id, json!({ "resources": [] }))),
        "resources/read" => Some(jsonrpc_error(id, -32601, "resources not supported")),
        "prompts/list" => Some(jsonrpc_response(id, json!({ "prompts": [] }))),
        "prompts/get" => Some(jsonrpc_error(id, -32601, "prompts not supported")),
        "initialized" | "notifications/initialized" => None,
        _ => Some(jsonrpc_error(id, -32601, "method not found")),
    };
    Ok(response)
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    json!({
        "protocolVersion": requested,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": {},
            "prompts": {}
        },
        "serverInfo": {
            "name": "llm-translator-rust",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": "translate",
                "description": "Translate text into the target language.",
                "inputSchema": translate_input_schema()
            },
            {
                "name": "translate_details",
                "description": "Return detailed translations across all formal styles.",
                "inputSchema": translate_input_schema()
            },
            {
                "name": "correction",
                "description": "Proofread input text and point out corrections.",
                "inputSchema": translate_input_schema()
            },
            {
                "name": "pos",
                "description": "Return dictionary-style details for the input term.",
                "inputSchema": translate_input_schema()
            }
        ]
    })
}

fn translate_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text": {
                "type": "string",
                "description": "Text to translate."
            },
            "lang": {
                "type": "string",
                "description": "Target language code (ISO 639-1/2/3)."
            },
            "source_lang": {
                "type": "string",
                "description": "Source language code or 'auto'."
            },
            "formal": {
                "type": "string",
                "description": "Formality/style key from settings [formally]."
            },
            "slang": {
                "type": "boolean",
                "description": "Enable slang mode."
            },
            "model": {
                "type": "string",
                "description": "Override provider/model (e.g. openai:gpt-4o)."
            },
            "key": {
                "type": "string",
                "description": "API key override."
            }
        },
        "required": ["text"]
    })
}

async fn tools_call_result(
    params: Value,
    defaults: &McpDefaults,
    settings: &settings::Settings,
) -> Value {
    let name = params
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let args_value = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let args: TranslateArgs = match serde_json::from_value(args_value) {
        Ok(args) => args,
        Err(err) => {
            return tool_error(&format!("invalid arguments: {}", err));
        }
    };

    let Some(text) = args.text else {
        return tool_error("text is required");
    };
    if text.trim().is_empty() {
        return tool_error("text is empty");
    }

    let mut config = defaults.base_config();
    if let Some(lang) = args.lang {
        config.lang = lang;
    }
    if let Some(source_lang) = args.source_lang {
        config.source_lang = source_lang;
    }
    if let Some(formal) = args.formal {
        config.formal = formal;
    }
    if let Some(slang) = args.slang {
        config.slang = slang;
    }
    if let Some(model) = args.model {
        config.model = Some(model);
    }
    if let Some(key) = args.key {
        config.key = Some(key);
    }

    match name {
        "translate" => {
            config.details = false;
        }
        "translate_details" => {
            config.details = true;
        }
        "correction" => {
            config.correction = true;
        }
        "pos" => {
            config.pos = true;
        }
        _ => return tool_error(&format!("unknown tool: {}", name)),
    };

    match crate::run_with_settings(config, settings.clone(), Some(text)).await {
        Ok(output) => json!({
            "content": [
                {
                    "type": "text",
                    "text": output
                }
            ]
        }),
        Err(err) => tool_error(&err.to_string()),
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TranslateArgs {
    text: Option<String>,
    lang: Option<String>,
    source_lang: Option<String>,
    formal: Option<String>,
    slang: Option<bool>,
    model: Option<String>,
    key: Option<String>,
}

impl McpDefaults {
    fn base_config(&self) -> Config {
        Config {
            lang: self.lang.clone(),
            model: self.model.clone(),
            key: self.key.clone(),
            formal: self.formal.clone(),
            source_lang: self.source_lang.clone(),
            slang: self.slang,
            data: None,
            data_mime: None,
            data_attachment: None,
            directory_translation_threads: None,
            ignore_translation_files: Vec::new(),
            out_path: None,
            overwrite: false,
            force_translation: false,
            settings_path: self.settings_path.clone(),
            show_enabled_languages: false,
            show_enabled_styles: false,
            show_models_list: false,
            show_whisper_models: false,
            pos: false,
            pos_filter: None,
            correction: false,
            details: false,
            report_format: None,
            report_out: None,
            show_histories: false,
            show_trend: false,
            with_using_tokens: false,
            with_using_model: false,
            with_commentout: false,
            debug_ocr: false,
            verbose: false,
            whisper_model: None,
        }
    }
}

fn jsonrpc_response(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn jsonrpc_error(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": message
            }
        ],
        "isError": true
    })
}
