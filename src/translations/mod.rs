use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use tera::{Context as TeraContext, Tera};

use crate::languages::{LanguagePack, LanguageRegistry};
use crate::providers::ToolSpec;
use crate::settings::Settings;

pub const TOOL_NAME: &str = "deliver_translation";

#[derive(Debug, Clone)]
pub struct TranslateOptions {
    pub lang: String,
    pub formality: String,
    pub source_lang: String,
    pub slang: bool,
}

#[derive(Debug, Clone)]
pub struct TranslationResult {
    pub translation: String,
    pub source_language: String,
    pub target_language: String,
}

pub fn tool_spec(tool_name: &str) -> ToolSpec {
    let base = json!({
        "type": "object",
        "properties": {
            "translation": {"type": "string"},
            "source_language": {"type": "string"},
            "target_language": {"type": "string"},
            "style": {"type": "string"},
            "slang": {"type": "boolean"}
        },
        "required": ["translation", "source_language", "target_language", "style", "slang"]
    });

    ToolSpec {
        name: tool_name.to_string(),
        description: "Return the translation with metadata.".to_string(),
        parameters: base,
    }
}

pub fn render_system_prompt(
    options: &TranslateOptions,
    tool_name: &str,
    settings: &Settings,
) -> Result<String> {
    let template = load_prompt_template("system_prompt.tera")?;
    let mut context = TeraContext::new();
    let style = options.formality.trim();
    context.insert("source_lang", options.source_lang.as_str());
    context.insert("target_lang", options.lang.as_str());
    context.insert("style", style);
    let guidance = style_guidance(&options.formality, settings)?;
    context.insert("style_guidance", &guidance);
    context.insert("slang", &options.slang);
    context.insert("tool_name", tool_name);

    Tera::one_off(&template, &context, false).with_context(|| "failed to render system prompt")
}

pub fn parse_tool_args(
    value: Value,
    options: &TranslateOptions,
    registry: &LanguageRegistry,
) -> Result<TranslationResult> {
    let expected = ExpectedMeta::from_options(options);
    let args: ToolArgs = serde_json::from_value(value)?;
    validate_tool_args(&args, &expected, registry)?;

    Ok(TranslationResult {
        translation: args.translation,
        source_language: normalize_lang_code(&args.source_language),
        target_language: normalize_lang_code(&args.target_language),
    })
}

fn load_prompt_template(name: &str) -> Result<String> {
    let path = prompt_path(name)?;
    fs::read_to_string(&path).with_context(|| format!("failed to read prompt: {}", path.display()))
}

fn prompt_path(name: &str) -> Result<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("translations")
        .join("prompts")
        .join(name);
    Ok(base)
}

#[derive(Debug, Deserialize)]
struct ToolArgs {
    translation: String,
    source_language: String,
    target_language: String,
    style: String,
    slang: bool,
}

#[derive(Debug, Clone)]
struct ExpectedMeta {
    source_language: String,
    target_language: String,
    style: String,
    slang: bool,
}

impl ExpectedMeta {
    fn from_options(options: &TranslateOptions) -> Self {
        Self {
            source_language: options.source_lang.clone(),
            target_language: options.lang.clone(),
            style: options.formality.trim().to_string(),
            slang: options.slang,
        }
    }
}

fn validate_tool_args(
    args: &ToolArgs,
    expected: &ExpectedMeta,
    registry: &LanguageRegistry,
) -> Result<()> {
    if args.translation.trim().is_empty() {
        return Err(anyhow!("translation is empty"));
    }
    if args.source_language.trim().is_empty() {
        return Err(anyhow!("source_language is empty"));
    }
    if args.target_language.trim().is_empty() {
        return Err(anyhow!("target_language is empty"));
    }
    if args.style.trim().is_empty() {
        return Err(anyhow!("style is empty"));
    }

    if expected.source_language.trim().eq_ignore_ascii_case("auto") {
        if !is_valid_lang_code(&args.source_language, registry) {
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
    if !eq_insensitive(&args.style, &expected.style) {
        return Err(anyhow!(
            "tool response style mismatch (expected '{}', got '{}')",
            expected.style,
            args.style
        ));
    }
    if args.slang != expected.slang {
        return Err(anyhow!(
            "tool response slang mismatch (expected {}, got {})",
            expected.slang,
            args.slang
        ));
    }
    Ok(())
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

fn style_guidance(formality: &str, settings: &Settings) -> Result<String> {
    let key = formality.trim();
    if key.is_empty() {
        return Err(anyhow!("formality is empty"));
    }
    settings
        .formally
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("missing formality guidance for '{}'", key))
}

pub(crate) fn display_language(
    code: &str,
    registry: &LanguageRegistry,
    pack: Option<&LanguagePack>,
) -> String {
    let code_norm = normalize_lang_code(code);
    if let Some(pack) = pack {
        if let Some(value) = pack.iso_country_lang.get(&code_norm) {
            return value.clone();
        }
    }
    registry.iso_name(&code_norm).unwrap_or(code_norm)
}

fn normalize_lang_code(code: &str) -> String {
    code.trim().to_lowercase()
}

fn eq_insensitive(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}
