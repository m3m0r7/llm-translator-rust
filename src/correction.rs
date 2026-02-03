use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tera::{Context as TeraContext, Tera};

use crate::languages::LanguageRegistry;
use crate::providers::{Provider, ProviderUsage, ToolSpec};
use crate::settings::Settings;
use crate::translations::TranslateOptions;
use crate::Translator;

const TOOL_NAME: &str = "correct_text";

#[derive(Debug, Clone, Serialize)]
pub struct CorrectionResult {
    pub corrected: String,
    pub markers: Option<String>,
    pub reasons: Vec<String>,
    pub source_language: String,
}

#[derive(Debug, Clone)]
pub struct CorrectionOutput {
    pub result: CorrectionResult,
    pub model: Option<String>,
    pub usage: Option<ProviderUsage>,
}

pub async fn exec_correction<P: Provider + Clone>(
    translator: &Translator<P>,
    input: &str,
    options: &TranslateOptions,
) -> Result<CorrectionOutput> {
    let tool = tool_spec(TOOL_NAME);
    let system_prompt = render_system_prompt(options, translator.settings())?;
    let response = translator
        .call_tool_with_data(tool, system_prompt, input.to_string(), None)
        .await?;
    let result = parse_tool_args(response.args, options, translator.registry())?;
    Ok(CorrectionOutput {
        result,
        model: response.model,
        usage: response.usage,
    })
}

pub fn format_correction_output(result: &CorrectionResult) -> String {
    let mut output = String::new();
    output.push_str(result.corrected.as_str());
    output.push('\n');
    if let Some(markers) = result.markers.as_deref() {
        if !markers.trim().is_empty() {
            output.push_str(markers);
            output.push('\n');
        }
    }
    if !result.reasons.is_empty() {
        output.push('\n');
        output.push_str("Correction reasons:\n");
        for reason in &result.reasons {
            output.push_str("- ");
            output.push_str(reason);
            output.push('\n');
        }
    }
    output.trim_end_matches('\n').to_string()
}

fn tool_spec(tool_name: &str) -> ToolSpec {
    let base = json!({
        "type": "object",
        "properties": {
            "corrected": {
                "type": "string",
                "description": "Corrected text in the same language as the input."
            },
            "markers": {
                "type": ["string", "null"],
                "description": "Optional marker line aligned to the corrected text. Use '-' to mark corrected spans."
            },
            "reasons": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Short reasons for each correction."
            },
            "source_language": {
                "type": "string",
                "description": "ISO 639-1/2/3 code for the detected or provided source language."
            }
        },
        "required": ["corrected", "reasons", "source_language"]
    });

    ToolSpec {
        name: tool_name.to_string(),
        description: "Return corrections for the input text.".to_string(),
        parameters: base,
    }
}

fn render_system_prompt(options: &TranslateOptions, _settings: &Settings) -> Result<String> {
    let template = load_prompt_template("correction_prompt.tera")?;
    let mut context = TeraContext::new();
    context.insert("source_lang", options.source_lang.as_str());
    context.insert("tool_name", TOOL_NAME);
    Tera::one_off(&template, &context, false)
        .with_context(|| "failed to render correction prompt")
}

fn parse_tool_args(
    value: serde_json::Value,
    options: &TranslateOptions,
    registry: &LanguageRegistry,
) -> Result<CorrectionResult> {
    let expected = ExpectedMeta::from_options(options);
    let args: ToolArgs = serde_json::from_value(value)?;
    validate_tool_args(&args, &expected, registry)?;

    Ok(CorrectionResult {
        corrected: args.corrected,
        markers: args.markers,
        reasons: args.reasons.unwrap_or_default(),
        source_language: normalize_lang_code(&args.source_language),
    })
}

#[derive(Debug, Deserialize)]
struct ToolArgs {
    corrected: String,
    #[serde(default)]
    markers: Option<String>,
    #[serde(default)]
    reasons: Option<Vec<String>>,
    source_language: String,
}

#[derive(Debug, Clone)]
struct ExpectedMeta {
    source_language: String,
}

impl ExpectedMeta {
    fn from_options(options: &TranslateOptions) -> Self {
        Self {
            source_language: options.source_lang.clone(),
        }
    }
}

fn validate_tool_args(
    args: &ToolArgs,
    expected: &ExpectedMeta,
    registry: &LanguageRegistry,
) -> Result<()> {
    if args.corrected.trim().is_empty() {
        return Err(anyhow!("corrected text is empty"));
    }
    if args.source_language.trim().is_empty() {
        return Err(anyhow!("source_language is empty"));
    }

    if expected.source_language.trim().eq_ignore_ascii_case("auto") {
        let source = args.source_language.trim();
        if !is_auto_source_placeholder(source) && !is_valid_lang_code(source, registry) {
            return Err(anyhow!(
                "source_language must be ISO 639 code (or zho-hans/zho-hant) when auto-detected (got '{}')",
                args.source_language
            ));
        }
    } else if !eq_insensitive(&args.source_language, &expected.source_language) {
        return Err(anyhow!(
            "tool response source_language mismatch (expected '{}', got '{}')",
            expected.source_language,
            args.source_language
        ));
    }
    Ok(())
}

fn load_prompt_template(name: &str) -> Result<String> {
    let path = prompt_path(name)?;
    std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read prompt: {}", path.display()))
}

fn prompt_path(name: &str) -> Result<std::path::PathBuf> {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("translations")
        .join("prompts")
        .join(name);
    Ok(base)
}

fn normalize_lang_code(code: &str) -> String {
    code.trim().to_lowercase()
}

fn eq_insensitive(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn is_auto_source_placeholder(value: &str) -> bool {
    let lower = value.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "auto" | "und" | "unknown" | "unk" | "mul" | "zxx"
    )
}

fn is_valid_lang_code(code: &str, registry: &LanguageRegistry) -> bool {
    if registry.is_valid_code(code) {
        return true;
    }
    let Some((base, suffix)) = split_lang_suffix(code) else {
        return false;
    };
    if !registry.is_valid_code(&base) {
        return false;
    }
    matches!(suffix.as_str(), "hans" | "hant")
}

fn split_lang_suffix(code: &str) -> Option<(String, String)> {
    let mut parts = code.trim().splitn(2, '-');
    let base = parts.next()?.trim();
    let suffix = parts.next()?.trim();
    if base.is_empty() || suffix.is_empty() {
        return None;
    }
    Some((base.to_lowercase(), suffix.to_lowercase()))
}
