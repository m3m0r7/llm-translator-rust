use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tera::{Context as TeraContext, Tera};

use crate::languages::LanguageRegistry;
use crate::providers::{Provider, ProviderUsage, ToolSpec};
use crate::settings::Settings;
use crate::translations::TranslateOptions;
use crate::Translator;

const TOOL_NAME: &str = "deliver_translation_details";

#[derive(Debug, Clone, Serialize)]
pub struct DetailsResult {
    pub details: String,
    pub source_language: String,
    pub target_language: String,
}

#[derive(Debug, Clone)]
pub struct DetailsOutput {
    pub result: DetailsResult,
    pub model: Option<String>,
    pub usage: Option<ProviderUsage>,
}

#[derive(Debug, Clone, Serialize)]
struct StyleEntry {
    key: String,
    guidance: String,
}

pub async fn exec_details<P: Provider + Clone>(
    translator: &Translator<P>,
    input: &str,
    options: &TranslateOptions,
) -> Result<DetailsOutput> {
    let styles = collect_styles(translator.settings(), &options.formality)?;
    let tool = tool_spec(TOOL_NAME);
    let system_prompt = render_system_prompt(options, translator.settings(), &styles)?;
    let response = translator
        .call_tool_with_data(tool, system_prompt, input.to_string(), None)
        .await?;
    let result = parse_tool_args(response.args, options, translator.registry())?;
    Ok(DetailsOutput {
        result,
        model: response.model,
        usage: response.usage,
    })
}

fn tool_spec(tool_name: &str) -> ToolSpec {
    let base = json!({
        "type": "object",
        "properties": {
            "details": {
                "type": "string",
                "description": "Detailed translation report with all styles."
            },
            "source_language": {
                "type": "string",
                "description": "ISO 639-1/2/3 code for the detected or provided source language."
            },
            "target_language": {
                "type": "string",
                "description": "ISO 639-1/2/3 code for the target language."
            }
        },
        "required": ["details", "source_language", "target_language"]
    });

    ToolSpec {
        name: tool_name.to_string(),
        description: "Return detailed translations for each style.".to_string(),
        parameters: base,
    }
}

fn render_system_prompt(
    options: &TranslateOptions,
    _settings: &Settings,
    styles: &[StyleEntry],
) -> Result<String> {
    let template = load_prompt_template("details_prompt.tera")?;
    let mut context = TeraContext::new();
    context.insert("source_lang", options.source_lang.as_str());
    context.insert("target_lang", options.lang.as_str());
    context.insert("slang", &options.slang);
    context.insert("styles", styles);
    context.insert("tool_name", TOOL_NAME);
    Tera::one_off(&template, &context, false).with_context(|| "failed to render details prompt")
}

fn parse_tool_args(
    value: serde_json::Value,
    options: &TranslateOptions,
    registry: &LanguageRegistry,
) -> Result<DetailsResult> {
    let expected = ExpectedMeta::from_options(options);
    let args: ToolArgs = serde_json::from_value(value)?;
    validate_tool_args(&args, &expected, registry)?;

    Ok(DetailsResult {
        details: args.details,
        source_language: normalize_lang_code(&args.source_language),
        target_language: normalize_lang_code(&args.target_language),
    })
}

#[derive(Debug, Deserialize)]
struct ToolArgs {
    details: String,
    source_language: String,
    target_language: String,
}

#[derive(Debug, Clone)]
struct ExpectedMeta {
    source_language: String,
    target_language: String,
}

impl ExpectedMeta {
    fn from_options(options: &TranslateOptions) -> Self {
        Self {
            source_language: options.source_lang.clone(),
            target_language: options.lang.clone(),
        }
    }
}

fn validate_tool_args(
    args: &ToolArgs,
    expected: &ExpectedMeta,
    registry: &LanguageRegistry,
) -> Result<()> {
    if args.details.trim().is_empty() {
        return Err(anyhow!("details output is empty"));
    }
    if args.source_language.trim().is_empty() {
        return Err(anyhow!("source_language is empty"));
    }
    if args.target_language.trim().is_empty() {
        return Err(anyhow!("target_language is empty"));
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

    if !is_valid_lang_code(&args.target_language, registry) {
        return Err(anyhow!(
            "target_language must be ISO 639 code (or zho-hans/zho-hant) (got '{}')",
            args.target_language
        ));
    }
    if !eq_insensitive(&args.target_language, &expected.target_language) {
        return Err(anyhow!(
            "tool response target_language mismatch (expected '{}', got '{}')",
            expected.target_language,
            args.target_language
        ));
    }

    Ok(())
}

fn collect_styles(settings: &Settings, preferred: &str) -> Result<Vec<StyleEntry>> {
    if settings.formally.is_empty() {
        return Err(anyhow!("settings.formally is empty"));
    }
    let mut entries: Vec<StyleEntry> = settings
        .formally
        .iter()
        .map(|(key, guidance)| StyleEntry {
            key: key.clone(),
            guidance: guidance.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));

    let preferred = preferred.trim();
    if !preferred.is_empty() {
        if let Some(idx) = entries
            .iter()
            .position(|entry| entry.key.eq_ignore_ascii_case(preferred))
        {
            let entry = entries.remove(idx);
            entries.insert(0, entry);
        }
    }

    Ok(entries)
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
