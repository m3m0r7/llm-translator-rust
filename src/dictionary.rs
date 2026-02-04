use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use tera::{Context as TeraContext, Tera};

use crate::languages::{self, LanguageRegistry};
use crate::providers::ToolSpec;
use crate::settings::Settings;
use crate::translations::TranslateOptions;
use crate::translator::{ExecutionOutput, Translator};

pub const TOOL_NAME: &str = "deliver_dictionary_entry";
const READING_TOOL_NAME: &str = "deliver_readings";

#[derive(Debug, Clone)]
pub struct DictionaryResult {
    pub translation: String,
    pub translation_reading: String,
    pub part_of_speech: String,
    pub attributes: Vec<String>,
    pub inflections: Inflections,
    pub usage: String,
    pub examples: Vec<Example>,
    pub alternatives: Vec<AltTranslation>,
    pub labels: Labels,
    pub source_language: String,
    pub target_language: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Example {
    pub target: String,
    pub source: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AltTranslation {
    pub text: String,
    pub reading: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Inflections {
    pub plural: String,
    pub third_person_singular: String,
    pub past_tense: String,
    pub present_participle: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Labels {
    pub translation: Option<String>,
    pub reading: Option<String>,
    pub part_of_speech: Option<String>,
    pub attributes: Option<String>,
    pub alternatives: Option<String>,
    pub plural: Option<String>,
    pub third_person_singular: Option<String>,
    pub past_tense: Option<String>,
    pub present_participle: Option<String>,
    pub usage: Option<String>,
    pub usage_examples: Option<String>,
}

pub fn tool_spec(tool_name: &str) -> ToolSpec {
    let base = json!({
        "type": "object",
        "properties": {
            "translation": {"type": "string"},
            "translation_reading": {"type": "string"},
            "part_of_speech": {"type": "string"},
            "attributes": {"type": "array", "items": {"type": "string"}},
            "alternatives": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"},
                        "reading": {"type": "string"}
                    },
                    "required": ["text", "reading"]
                }
            },
            "inflections": {
                "type": "object",
                "properties": {
                    "plural": {"type": "string"},
                    "third_person_singular": {"type": "string"},
                    "past_tense": {"type": "string"},
                    "present_participle": {"type": "string"}
                },
                "required": ["plural", "third_person_singular", "past_tense", "present_participle"]
            },
            "usage": {"type": "string"},
            "examples": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "target": {"type": "string"},
                        "source": {"type": "string"}
                    },
                    "required": ["target", "source"]
                }
            },
            "labels": {
                "type": "object",
                "properties": {
                    "translation": {"type": "string"},
                    "reading": {"type": "string"},
                    "part_of_speech": {"type": "string"},
                    "attributes": {"type": "string"},
                    "alternatives": {"type": "string"},
                    "plural": {"type": "string"},
                    "third_person_singular": {"type": "string"},
                    "past_tense": {"type": "string"},
                    "present_participle": {"type": "string"},
                    "usage": {"type": "string"},
                    "usage_examples": {"type": "string"}
                },
                "required": [
                    "translation",
                    "reading",
                    "part_of_speech",
                    "attributes",
                    "alternatives",
                    "plural",
                    "third_person_singular",
                    "past_tense",
                    "present_participle",
                    "usage",
                    "usage_examples"
                ]
            },
            "source_language": {"type": "string"},
            "target_language": {"type": "string"}
        },
        "required": [
            "translation",
            "translation_reading",
            "part_of_speech",
            "inflections",
            "usage",
            "examples",
            "alternatives",
            "labels",
            "source_language",
            "target_language"
        ]
    });

    ToolSpec {
        name: tool_name.to_string(),
        description: "Return dictionary metadata for the input term.".to_string(),
        parameters: base,
    }
}

pub async fn exec_pos<P: crate::providers::Provider + Clone>(
    translator: &Translator<P>,
    input: &str,
    options: &TranslateOptions,
    pos_filter: Option<&[String]>,
) -> Result<ExecutionOutput> {
    let tool = tool_spec(TOOL_NAME);
    let prompt_filter = resolve_pos_filter(pos_filter, &options.source_lang);
    let system_prompt =
        render_system_prompt(options, translator.settings(), prompt_filter.as_deref())?;
    let response = translator
        .call_tool_with_data(tool, system_prompt, input.to_string(), None)
        .await?;
    let mut parsed = parse_tool_args(response.args, options, translator.registry())?;
    let resolved_filter = resolve_pos_filter(pos_filter, &parsed.source_language);
    let mut filtered = false;
    if let Some(filter) = resolved_filter.as_ref()
        && !filter.is_empty()
    {
        let original_pos = parsed.part_of_speech.clone();
        let matches = filter_part_of_speech(&mut parsed.part_of_speech, filter);
        if !matches {
            return Ok(ExecutionOutput {
                text: format!(
                    "No matching part of speech found (allowed: {}, got: {})",
                    pos_filter
                        .map(|items| items.join(", "))
                        .unwrap_or_else(|| "all".to_string()),
                    display_value(&original_pos)
                ),
                model: response.model,
                usage: response.usage,
            });
        }
        filtered = true;
    }
    if should_discard_labels(&parsed) {
        parsed.labels = Labels::default();
    }
    if should_fix_attributes(&parsed)
        && let Some(fixed) = translate_attributes_to_source(translator, &parsed, options).await?
    {
        parsed.attributes = fixed;
    }
    if should_fix_usage(&parsed)
        && let Some(fixed) = translate_usage_to_source(translator, &parsed, options).await?
    {
        parsed.usage = fixed;
    }
    if should_fix_example_sources(&parsed) {
        translate_example_sources(translator, &mut parsed, options).await?;
    }
    fill_missing_readings(translator, &mut parsed).await?;
    if filtered {
        parsed.attributes.retain(|value| !value.trim().is_empty());
    }
    let text = format_pos_output(&parsed);
    Ok(ExecutionOutput {
        text,
        model: response.model,
        usage: response.usage,
    })
}

pub fn render_system_prompt(
    options: &TranslateOptions,
    settings: &Settings,
    pos_filter: Option<&[String]>,
) -> Result<String> {
    let template = load_prompt_template("pos_prompt.tera")?;
    let mut context = TeraContext::new();
    context.insert("source_lang", options.source_lang.as_str());
    context.insert("target_lang", options.lang.as_str());
    context.insert("tool_name", TOOL_NAME);
    context.insert("style_guidance", &settings.formally);
    if let Some(filter) = pos_filter {
        context.insert("allowed_pos", filter);
    }

    Tera::one_off(&template, &context, false).with_context(|| "failed to render pos prompt")
}

pub fn parse_tool_args(
    value: Value,
    options: &TranslateOptions,
    registry: &LanguageRegistry,
) -> Result<DictionaryResult> {
    let expected = ExpectedMeta::from_options(options);
    let args: ToolArgs = serde_json::from_value(value)?;
    validate_tool_args(&args, &expected, registry)?;

    Ok(DictionaryResult {
        translation: args.translation,
        translation_reading: args.translation_reading,
        part_of_speech: args.part_of_speech,
        attributes: args.attributes.unwrap_or_default(),
        inflections: args.inflections,
        usage: args.usage,
        examples: args.examples.unwrap_or_default(),
        alternatives: args.alternatives.unwrap_or_default(),
        labels: args.labels.unwrap_or_default(),
        source_language: normalize_lang_code(&args.source_language),
        target_language: normalize_lang_code(&args.target_language),
    })
}

#[derive(Debug, Deserialize)]
struct ToolArgs {
    translation: String,
    translation_reading: String,
    part_of_speech: String,
    #[serde(default)]
    attributes: Option<Vec<String>>,
    #[serde(default)]
    alternatives: Option<Vec<AltTranslation>>,
    inflections: Inflections,
    usage: String,
    #[serde(default)]
    examples: Option<Vec<Example>>,
    labels: Option<Labels>,
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
    if args.translation.trim().is_empty() {
        return Err(anyhow!("translation is empty"));
    }
    if args.part_of_speech.trim().is_empty() {
        return Err(anyhow!("part_of_speech is empty"));
    }
    if let Some(alternatives) = args.alternatives.as_ref() {
        for alt in alternatives {
            if alt.text.trim().is_empty() {
                return Err(anyhow!("alternative text is empty"));
            }
        }
    }
    if args.usage.trim().is_empty() {
        return Err(anyhow!("usage is empty"));
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

pub fn format_pos_output(result: &DictionaryResult) -> String {
    let labels = &result.labels;
    let translation_label = label_or(labels.translation.as_deref(), "Translation");
    let reading_label = label_or(labels.reading.as_deref(), "Reading");
    let pos_label = label_or(labels.part_of_speech.as_deref(), "Part of speech");
    let attr_label = label_or(labels.attributes.as_deref(), "Attributes");
    let alt_label = label_or(labels.alternatives.as_deref(), "Alternatives");
    let plural_label = label_or(labels.plural.as_deref(), "Plural");
    let third_label = label_or(
        labels.third_person_singular.as_deref(),
        "3rd person singular",
    );
    let past_label = label_or(labels.past_tense.as_deref(), "Past tense");
    let present_label = label_or(labels.present_participle.as_deref(), "Present participle");
    let usage_label = label_or(labels.usage.as_deref(), "Usage");
    let examples_label = label_or(labels.usage_examples.as_deref(), "Usage examples");

    let attributes = if result.attributes.is_empty() {
        "-".to_string()
    } else {
        result.attributes.join(", ")
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "{}: {}",
        translation_label,
        display_value(&result.translation)
    ));
    lines.push(format!(
        "{}: {}",
        reading_label,
        display_value(&result.translation_reading)
    ));
    lines.push(format!(
        "{}: {}",
        pos_label,
        display_value(&result.part_of_speech)
    ));
    lines.push(format!("{}: {}", attr_label, display_value(&attributes)));
    lines.push(format!(
        "{}: {}",
        alt_label,
        display_value(&format_alternatives(result))
    ));
    lines.push(String::new());
    lines.push(format!(
        "{}: {}",
        plural_label,
        display_value(&result.inflections.plural)
    ));
    lines.push(format!(
        "{}: {}",
        third_label,
        display_value(&result.inflections.third_person_singular)
    ));
    lines.push(format!(
        "{}: {}",
        past_label,
        display_value(&result.inflections.past_tense)
    ));
    lines.push(format!(
        "{}: {}",
        present_label,
        display_value(&result.inflections.present_participle)
    ));
    lines.push(String::new());
    lines.push(format!("{}: {}", usage_label, display_value(&result.usage)));
    lines.push(format!("{}:", examples_label));

    if result.examples.is_empty() {
        lines.push("-".to_string());
    } else {
        for example in &result.examples {
            let mut target = example.target.trim().to_string();
            let source = example.source.trim();
            if !example_matches_translation(result, &target) && !target.is_empty() {
                target = format!("{} ({})", target, result.translation);
            }
            if target.is_empty() && source.is_empty() {
                continue;
            }
            if target.is_empty() {
                lines.push(format!("- ({})", source));
            } else if source.is_empty() {
                lines.push(format!("- {}", target));
            } else {
                lines.push(format!("- {} ({})", target, source));
            }
        }
    }

    lines.join("\n")
}

pub fn parse_pos_filter(value: &str) -> Option<Vec<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
        return None;
    }
    let mut items = Vec::new();
    for part in split_pos_tokens(trimmed) {
        let normalized = normalize_pos_token(&part);
        if !normalized.is_empty() && normalized != "all" {
            items.push(normalized);
        }
    }
    items.sort();
    items.dedup();
    if items.is_empty() { None } else { Some(items) }
}

fn resolve_pos_filter(pos_filter: Option<&[String]>, source_lang: &str) -> Option<Vec<String>> {
    let filter = pos_filter?;
    if filter.is_empty() {
        return None;
    }
    let (localized_map, reverse_map) = load_pos_maps(source_lang);
    let mut expanded = Vec::new();
    for token in filter {
        let normalized = normalize_pos_token(token);
        if normalized.is_empty() {
            continue;
        }
        expanded.push(normalized.clone());
        if let Some(canonical) = canonical_pos_from_token(&normalized) {
            expanded.push(canonical.to_string());
            if let Some(localized) = localized_map.get(canonical) {
                expanded.push(normalize_pos_token(localized));
            }
        } else if let Some(canonical) = reverse_map.get(&normalized) {
            expanded.push(canonical.to_string());
            if let Some(localized) = localized_map.get(canonical.as_str()) {
                expanded.push(normalize_pos_token(localized));
            }
        }
    }
    expanded.sort();
    expanded.dedup();
    if expanded.is_empty() {
        None
    } else {
        Some(expanded)
    }
}

fn filter_part_of_speech(current: &mut String, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }
    let tokens = split_pos_tokens(current);
    if tokens.is_empty() {
        return false;
    }
    let mut matched = Vec::new();
    for token in tokens {
        let normalized = normalize_pos_token(&token);
        if allowed.iter().any(|item| item == &normalized) {
            matched.push(token);
        }
    }
    if matched.is_empty() {
        let normalized = normalize_pos_token(current);
        if allowed.iter().any(|item| item == &normalized) {
            return true;
        }
        return false;
    }
    *current = matched.join(", ");
    true
}

fn split_pos_tokens(value: &str) -> Vec<String> {
    value
        .split([',', '/', ';', '|', '・'])
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect()
}

fn normalize_pos_token(value: &str) -> String {
    let trimmed = value.trim();
    let trimmed = trimmed.split(['(', '（']).next().unwrap_or(trimmed).trim();
    trimmed
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn load_pos_maps(
    source_lang: &str,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, String>,
) {
    let mut localized = std::collections::HashMap::new();
    let mut reverse = std::collections::HashMap::new();
    let code = source_lang.trim().to_lowercase();
    if code.is_empty() || code == "auto" || code == "und" {
        return (localized, reverse);
    }
    let packs = languages::load_language_packs(std::slice::from_ref(&code));
    let Ok(packs) = packs else {
        return (localized, reverse);
    };
    let Some(pack) = packs.packs.get(&code) else {
        return (localized, reverse);
    };
    for (key, label) in &pack.parts_of_speech {
        if let Some(canonical) = canonical_from_pack_key(key) {
            localized.insert(canonical.to_string(), label.clone());
            let normalized_label = normalize_pos_token(label);
            if !normalized_label.is_empty() {
                reverse.insert(normalized_label, canonical.to_string());
            }
        }
    }
    (localized, reverse)
}

fn canonical_from_pack_key(key: &str) -> Option<&'static str> {
    match key.trim().to_lowercase().as_str() {
        "noun" => Some("noun"),
        "verb" => Some("verb"),
        "adjective" => Some("adjective"),
        "adverb" => Some("adverb"),
        "pronoun" => Some("pronoun"),
        "particle" => Some("particle"),
        "auxiliary" => Some("auxiliary"),
        "conjunction" => Some("conjunction"),
        "interjection" => Some("interjection"),
        "preposition" => Some("preposition"),
        "determiner" => Some("determiner"),
        "subject" => Some("subject"),
        "object" => Some("object"),
        _ => None,
    }
}

fn canonical_pos_from_token(token: &str) -> Option<&'static str> {
    match token {
        "noun" | "n" => Some("noun"),
        "verb" | "v" => Some("verb"),
        "adjective" | "adj" => Some("adjective"),
        "adverb" | "adv" => Some("adverb"),
        "pronoun" | "pron" => Some("pronoun"),
        "preposition" | "prep" | "postposition" => Some("preposition"),
        "conjunction" | "conj" => Some("conjunction"),
        "interjection" | "interj" => Some("interjection"),
        "determiner" | "det" | "article" => Some("determiner"),
        "particle" | "part" => Some("particle"),
        "auxiliary" | "aux" | "auxiliary verb" => Some("auxiliary"),
        "subject" | "subj" => Some("subject"),
        "object" | "obj" => Some("object"),
        _ => None,
    }
}

fn label_or(value: Option<&str>, fallback: &str) -> String {
    let trimmed = value.unwrap_or("").trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn display_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "-".to_string()
    } else {
        trimmed.to_string()
    }
}

fn should_fix_usage(result: &DictionaryResult) -> bool {
    let source = normalize_lang_code(&result.source_language);
    matches!(source.as_str(), "en" | "eng") && contains_non_latin(&result.usage)
}

async fn translate_usage_to_source<P: crate::providers::Provider + Clone>(
    translator: &Translator<P>,
    result: &DictionaryResult,
    options: &TranslateOptions,
) -> Result<Option<String>> {
    let mut translate_options = options.clone();
    translate_options.lang = result.source_language.clone();
    translate_options.source_lang = result.target_language.clone();
    let output = translator.exec(&result.usage, translate_options).await?;
    let trimmed = output.text.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

fn should_fix_example_sources(result: &DictionaryResult) -> bool {
    let source = normalize_lang_code(&result.source_language);
    if !matches!(source.as_str(), "en" | "eng") {
        return false;
    }
    result
        .examples
        .iter()
        .any(|example| contains_non_latin(example.source.trim()))
}

async fn translate_example_sources<P: crate::providers::Provider + Clone>(
    translator: &Translator<P>,
    result: &mut DictionaryResult,
    options: &TranslateOptions,
) -> Result<()> {
    let mut translate_options = options.clone();
    translate_options.lang = result.source_language.clone();
    translate_options.source_lang = result.target_language.clone();

    for example in &mut result.examples {
        if example.source.trim().is_empty() || !contains_non_latin(example.source.trim()) {
            continue;
        }
        let output = translator
            .exec(&example.source, translate_options.clone())
            .await?;
        let fixed = output.text.trim();
        if !fixed.is_empty() {
            example.source = fixed.to_string();
        }
    }
    Ok(())
}

async fn fill_missing_readings<P: crate::providers::Provider + Clone>(
    translator: &Translator<P>,
    result: &mut DictionaryResult,
) -> Result<()> {
    let mut requests: Vec<(u32, String)> = Vec::new();
    if needs_reading(&result.translation, &result.source_language)
        && normalize_reading(&result.translation_reading, &result.source_language).is_none()
    {
        requests.push((0, result.translation.clone()));
    }
    for (idx, alt) in result.alternatives.iter().enumerate() {
        if needs_reading(&alt.text, &result.source_language)
            && normalize_reading(&alt.reading, &result.source_language).is_none()
        {
            requests.push(((idx + 1) as u32, alt.text.clone()));
        }
    }
    if requests.is_empty() {
        return Ok(());
    }

    let mut readings = fetch_readings(translator, &result.source_language, &requests).await?;
    apply_readings(result, &readings);

    let mut missing: Vec<(u32, String)> = Vec::new();
    if needs_reading(&result.translation, &result.source_language)
        && normalize_reading(&result.translation_reading, &result.source_language).is_none()
    {
        missing.push((0, result.translation.clone()));
    }
    for (idx, alt) in result.alternatives.iter().enumerate() {
        if needs_reading(&alt.text, &result.source_language)
            && normalize_reading(&alt.reading, &result.source_language).is_none()
        {
            missing.push(((idx + 1) as u32, alt.text.clone()));
        }
    }
    if !missing.is_empty() {
        let retry = fetch_readings(translator, &result.source_language, &missing).await?;
        readings.extend(retry);
        apply_readings(result, &readings);
    }

    if needs_reading(&result.translation, &result.source_language)
        && normalize_reading(&result.translation_reading, &result.source_language).is_none()
    {
        result.translation_reading = "-".to_string();
    }
    for alt in result.alternatives.iter_mut() {
        if needs_reading(&alt.text, &result.source_language)
            && normalize_reading(&alt.reading, &result.source_language).is_none()
        {
            alt.reading = "-".to_string();
        }
    }
    Ok(())
}

fn apply_readings(
    result: &mut DictionaryResult,
    readings: &std::collections::HashMap<u32, String>,
) {
    if let Some(reading) = readings
        .get(&0)
        .and_then(|value| normalize_reading(value, &result.source_language))
    {
        result.translation_reading = reading;
    }
    for (idx, alt) in result.alternatives.iter_mut().enumerate() {
        let key = (idx + 1) as u32;
        if let Some(reading) = readings
            .get(&key)
            .and_then(|value| normalize_reading(value, &result.source_language))
        {
            alt.reading = reading;
        }
    }
}

fn reading_tool_spec() -> ToolSpec {
    let base = json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "reading": {"type": "string"}
                    },
                    "required": ["id", "reading"]
                }
            }
        },
        "required": ["items"]
    });
    ToolSpec {
        name: READING_TOOL_NAME.to_string(),
        description: "Return pronunciation readings in the source language's usual writing system."
            .to_string(),
        parameters: base,
    }
}

fn reading_prompt(lang: &str) -> String {
    format!(
        "You generate pronunciation readings for the SOURCE language. \
Input language hint: {lang}. Do not translate the meaning. \
Rules: \
- If source language is Japanese, return katakana. \
- If source language is Chinese, return pinyin with tone marks. \
- If source language is Korean, return hangul. \
- Otherwise, return a readable pronunciation in the source language's usual writing system, \
  including accent/diacritics when customary. \
Always call the tool \"{tool}\" with JSON.",
        lang = lang,
        tool = READING_TOOL_NAME
    )
}

#[derive(Debug, Deserialize)]
struct ReadingToolArgs {
    items: Vec<ReadingToolItem>,
}

#[derive(Debug, Deserialize)]
struct ReadingToolItem {
    id: u32,
    reading: String,
}

async fn fetch_readings<P: crate::providers::Provider + Clone>(
    translator: &Translator<P>,
    source_lang: &str,
    items: &[(u32, String)],
) -> Result<std::collections::HashMap<u32, String>> {
    let payload = json!({
        "language": source_lang,
        "items": items
            .iter()
            .map(|(id, text)| json!({"id": id, "text": text}))
            .collect::<Vec<_>>()
    });
    let user_input = format!("Items (JSON):\n{}", serde_json::to_string_pretty(&payload)?);
    let response = translator
        .call_tool_with_data(
            reading_tool_spec(),
            reading_prompt(source_lang),
            user_input,
            None,
        )
        .await?;
    let args: ReadingToolArgs = serde_json::from_value(response.args)?;
    let mut map = std::collections::HashMap::new();
    for item in args.items {
        map.insert(item.id, item.reading);
    }
    Ok(map)
}

fn normalize_reading(reading: &str, source_lang: &str) -> Option<String> {
    let mut trimmed = reading.trim().to_string();
    if trimmed.is_empty() || trimmed == "-" {
        return None;
    }
    let lang = normalize_lang_code(source_lang);
    if matches!(lang.as_str(), "ja" | "jpn") {
        trimmed = hiragana_to_katakana(&trimmed);
    }
    if !is_reading_valid_for_source(&trimmed, source_lang) {
        return None;
    }
    Some(trimmed)
}

fn is_reading_valid_for_source(reading: &str, source_lang: &str) -> bool {
    let lang = normalize_lang_code(source_lang);
    if matches!(lang.as_str(), "ja" | "jpn") {
        return contains_katakana(reading);
    }
    if matches!(lang.as_str(), "zh" | "zho" | "zho-hans" | "zho-hant") {
        return !contains_cjk(reading) && !reading.trim().is_empty();
    }
    if matches!(lang.as_str(), "ko" | "kor") {
        return contains_hangul(reading);
    }
    !reading.trim().is_empty()
}

fn contains_katakana(value: &str) -> bool {
    value
        .chars()
        .any(|ch| matches!(ch as u32, 0x30A0..=0x30FF | 0x31F0..=0x31FF))
}

fn hiragana_to_katakana(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            let code = ch as u32;
            if (0x3041..=0x3096).contains(&code) {
                std::char::from_u32(code + 0x60).unwrap_or(ch)
            } else {
                ch
            }
        })
        .collect()
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|ch| matches!(ch as u32, 0x4E00..=0x9FFF))
}

fn contains_hangul(value: &str) -> bool {
    value.chars().any(|ch| matches!(ch as u32, 0xAC00..=0xD7AF))
}

fn format_alternatives(result: &DictionaryResult) -> String {
    if result.alternatives.is_empty() {
        return "-".to_string();
    }
    let mut parts = Vec::new();
    for alt in &result.alternatives {
        let text = alt.text.trim();
        if text.is_empty() {
            continue;
        }
        let reading = alt.reading.trim();
        if reading.is_empty() {
            parts.push(text.to_string());
        } else {
            parts.push(format!("{} ({})", text, reading));
        }
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(", ")
    }
}

fn example_matches_translation(result: &DictionaryResult, target: &str) -> bool {
    if target.contains(&result.translation) {
        return true;
    }
    result
        .alternatives
        .iter()
        .any(|alt| !alt.text.trim().is_empty() && target.contains(alt.text.trim()))
}

fn needs_reading(text: &str, source_lang: &str) -> bool {
    let lang = normalize_lang_code(source_lang);
    if matches!(
        lang.as_str(),
        "ja" | "jpn" | "zh" | "zho" | "zho-hans" | "zho-hant" | "ko" | "kor"
    ) {
        return true;
    }
    contains_non_latin(text)
}

fn should_fix_attributes(result: &DictionaryResult) -> bool {
    let source = normalize_lang_code(&result.source_language);
    if !matches!(source.as_str(), "en" | "eng") {
        return false;
    }
    result
        .attributes
        .iter()
        .any(|value| contains_non_latin(value))
}

async fn translate_attributes_to_source<P: crate::providers::Provider + Clone>(
    translator: &Translator<P>,
    result: &DictionaryResult,
    options: &TranslateOptions,
) -> Result<Option<Vec<String>>> {
    if result.attributes.is_empty() {
        return Ok(None);
    }
    let joined = result.attributes.join(", ");
    let mut translate_options = options.clone();
    translate_options.lang = result.source_language.clone();
    translate_options.source_lang = result.target_language.clone();

    let output = translator.exec(&joined, translate_options).await?;
    let values = split_attributes(&output.text);
    if values.is_empty() {
        Ok(None)
    } else {
        Ok(Some(values))
    }
}

fn split_attributes(value: &str) -> Vec<String> {
    value
        .split([',', '、', ';', '\n'])
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

fn should_discard_labels(result: &DictionaryResult) -> bool {
    let source = normalize_lang_code(&result.source_language);
    if !matches!(source.as_str(), "en" | "eng") {
        return false;
    }
    labels_have_non_latin(&result.labels)
}

fn labels_have_non_latin(labels: &Labels) -> bool {
    let mut values: Vec<&str> = Vec::new();
    if let Some(value) = labels.translation.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.reading.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.part_of_speech.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.attributes.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.alternatives.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.plural.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.third_person_singular.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.past_tense.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.present_participle.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.usage.as_deref() {
        values.push(value);
    }
    if let Some(value) = labels.usage_examples.as_deref() {
        values.push(value);
    }

    values.into_iter().any(contains_non_latin)
}

fn contains_non_latin(value: &str) -> bool {
    value.chars().any(|ch| {
        let code = ch as u32;
        matches!(
            code,
            0x4E00..=0x9FFF
                | 0x3040..=0x30FF
                | 0x31F0..=0x31FF
                | 0xAC00..=0xD7AF
                | 0x0400..=0x04FF
                | 0x0370..=0x03FF
        )
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
