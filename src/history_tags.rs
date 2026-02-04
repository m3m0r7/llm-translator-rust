use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::providers::{Provider, ToolSpec};
use crate::Translator;

const TOOL_NAME: &str = "generate_history_tags";
const MAX_TEXT_LEN: usize = 600;
const MAX_TAGS: usize = 8;

#[derive(Debug, Default, Deserialize)]
struct TagResponse {
    #[serde(default)]
    tags: Vec<String>,
}

pub async fn generate_history_tags<P: Provider + Clone>(
    translator: &Translator<P>,
    text: &str,
    source_lang: &str,
    target_lang: &str,
) -> Result<Vec<String>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let prompt = render_prompt()?;
    let tool = tool_spec();
    let input_json = serde_json::to_string_pretty(&json!({
        "text": truncate_text(trimmed, MAX_TEXT_LEN),
        "source_language": source_lang,
        "target_language": target_lang
    }))?;

    let response = translator
        .call_tool_with_data(tool, prompt, input_json, None)
        .await?;
    let parsed: TagResponse = serde_json::from_value(response.args)
        .with_context(|| "failed to parse history tag response")?;

    Ok(normalize_tags(parsed.tags))
}

fn tool_spec() -> ToolSpec {
    ToolSpec {
        name: TOOL_NAME.to_string(),
        description: "Generate short topic tags for translation history.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["tags"]
        }),
    }
}

fn render_prompt() -> Result<String> {
    let template = load_prompt_template("history_tags_prompt.tera")?;
    let mut context = tera::Context::new();
    context.insert("tool_name", TOOL_NAME);
    tera::Tera::one_off(&template, &context, false)
        .with_context(|| "failed to render history tags prompt")
}

fn load_prompt_template(name: &str) -> Result<String> {
    let path = prompt_path(name)?;
    std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read prompt: {}", path.display()))
}

fn prompt_path(name: &str) -> Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("translations")
        .join("prompts")
        .join(name))
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    text.chars().take(max_len).collect()
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
        if out.len() >= MAX_TAGS {
            break;
        }
    }
    out
}
