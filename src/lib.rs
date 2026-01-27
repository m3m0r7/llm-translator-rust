use anyhow::{anyhow, Context, Result};
use std::path::Path;

pub mod languages;
mod model_registry;
mod providers;
pub mod settings;
pub mod translations;
mod translator;

pub use providers::{Claude, Gemini, OpenAI, Provider, ProviderKind, ProviderUsage};
pub use translations::TranslateOptions;
pub use translator::{ExecutionOutput, Translator};

#[derive(Debug, Clone)]
pub struct Config {
    pub lang: String,
    pub model: Option<String>,
    pub key: Option<String>,
    pub formal: String,
    pub source_lang: String,
    pub slang: bool,
    pub settings_path: Option<String>,
    pub show_enabled_languages: bool,
    pub show_enabled_styles: bool,
    pub show_models_list: bool,
    pub with_using_tokens: bool,
    pub with_using_model: bool,
}

pub async fn run(config: Config, input: Option<String>) -> Result<String> {
    let settings_path = config.settings_path.as_deref().map(Path::new);
    let settings = settings::load_settings(settings_path)?;
    let registry = languages::LanguageRegistry::load()?;
    let packs = languages::load_language_packs(&settings.system_languages)?;

    if config.show_enabled_languages || config.show_enabled_styles {
        return Ok(format_show_output(&config, &settings, &registry, &packs));
    }
    if config.show_models_list {
        return show_models_list(&config).await;
    }

    let input = input.unwrap_or_default();
    let input = input.trim();
    if input.is_empty() {
        return Err(anyhow!("stdin is empty"));
    }
    let formality = config.formal.trim().to_string();
    if formality.is_empty() {
        return Err(anyhow!("formality is empty"));
    }
    let with_using_model = config.with_using_model;
    let with_using_tokens = config.with_using_tokens;

    let selection =
        providers::resolve_provider_selection(config.model.as_deref(), config.key.as_deref())?;
    let key = providers::resolve_key(selection.provider, config.key.as_deref())
        .with_context(|| "no API key found for selected provider")?;

    let model = resolve_model(
        selection.provider,
        selection.requested_model.as_deref(),
        &key,
    )
    .await
    .with_context(|| "failed to resolve model")?;

    validate_lang_codes(&config, &registry)?;

    let provider = providers::build_provider(selection.provider, key, model);
    let translator = Translator::new(provider, settings, registry);

    let options = TranslateOptions {
        lang: config.lang,
        formality,
        source_lang: config.source_lang,
        slang: config.slang,
    };

    let execution = translator.exec(input, options).await?;

    Ok(format_execution_output(
        &execution,
        with_using_model,
        with_using_tokens,
    ))
}

fn format_execution_output(
    execution: &ExecutionOutput,
    with_using_model: bool,
    with_using_tokens: bool,
) -> String {
    let mut output = execution.text.clone();
    let mut meta_lines = Vec::new();

    if with_using_model {
        let model = execution.model.as_deref().unwrap_or("unavailable");
        meta_lines.push(format!("model: {}", model));
    }

    if with_using_tokens {
        let tokens = format_usage(execution.usage.as_ref());
        meta_lines.push(tokens);
    }

    if !meta_lines.is_empty() {
        output.push('\n');
        output.push_str(&meta_lines.join("\n"));
    }

    output
}

fn format_usage(usage: Option<&ProviderUsage>) -> String {
    let Some(usage) = usage else {
        return "tokens: unavailable".to_string();
    };
    let total = usage.total_tokens.or_else(|| {
        usage
            .prompt_tokens
            .zip(usage.completion_tokens)
            .map(|(prompt, completion)| prompt + completion)
    });

    let mut parts = Vec::new();
    if let Some(prompt) = usage.prompt_tokens {
        parts.push(format!("prompt={}", prompt));
    }
    if let Some(completion) = usage.completion_tokens {
        parts.push(format!("completion={}", completion));
    }
    if let Some(total) = total {
        parts.push(format!("total={}", total));
    }

    if parts.is_empty() {
        "tokens: unavailable".to_string()
    } else {
        format!("tokens: {}", parts.join(", "))
    }
}

fn format_show_output(
    config: &Config,
    settings: &settings::Settings,
    registry: &languages::LanguageRegistry,
    packs: &languages::LanguagePacks,
) -> String {
    let mut sections = Vec::new();

    if config.show_enabled_languages {
        let mut lines = Vec::new();
        let pack = packs.primary_pack();
        for code in &settings.system_languages {
            let display = translations::display_language(code, registry, pack);
            lines.push(format!("{}\t{}", code, display));
        }
        sections.push(lines.join("\n"));
    }

    if config.show_enabled_styles {
        let mut keys = settings.formally.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        let mut lines = Vec::new();
        for key in keys {
            if let Some(value) = settings.formally.get(&key) {
                lines.push(format!("{}\t{}", key, value));
            }
        }
        sections.push(lines.join("\n"));
    }

    sections.join("\n")
}

async fn show_models_list(config: &Config) -> Result<String> {
    if config.key.is_some() && config.model.is_none() {
        return Err(anyhow!(
            "--key requires --model when using --show-models-list"
        ));
    }

    if let Some(model_arg) = config.model.as_deref() {
        let selection =
            providers::resolve_provider_selection(Some(model_arg), config.key.as_deref())?;
        let key = providers::resolve_key(selection.provider, config.key.as_deref())?;
        let models = model_registry::get_models(selection.provider, &key).await?;
        return Ok(format_models_for_provider(selection.provider, &models));
    }

    let mut sections = Vec::new();
    for provider in [
        ProviderKind::OpenAI,
        ProviderKind::Gemini,
        ProviderKind::Claude,
    ] {
        let Ok(key) = providers::resolve_key(provider, None) else {
            continue;
        };
        let models = model_registry::get_models(provider, &key).await?;
        if !models.is_empty() {
            sections.push(format_models_for_provider(provider, &models));
        }
    }

    if sections.is_empty() {
        return Err(anyhow!(
            "no API keys found (checked OPENAI_API_KEY, GEMINI_API_KEY/GOOGLE_API_KEY, ANTHROPIC_API_KEY)"
        ));
    }

    Ok(sections.join("\n"))
}

fn format_models_for_provider(provider: ProviderKind, models: &[String]) -> String {
    let prefix = provider.as_str();
    models
        .iter()
        .map(|model| format!("{}:{}", prefix, model))
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_lang_codes(config: &Config, registry: &languages::LanguageRegistry) -> Result<()> {
    if config.source_lang.trim().eq_ignore_ascii_case("auto") {
        // ok
    } else if !is_valid_lang_code(&config.source_lang, registry) {
        return Err(anyhow!(
            "invalid source language code '{}' (expected ISO 639-1/2/3 code, zho-hans/zho-hant, or auto)",
            config.source_lang
        ));
    }

    if !is_valid_lang_code(&config.lang, registry) {
        return Err(anyhow!(
            "invalid target language code '{}' (expected ISO 639-1/2/3 code or zho-hans/zho-hant)",
            config.lang
        ));
    }
    Ok(())
}

fn is_valid_lang_code(code: &str, registry: &languages::LanguageRegistry) -> bool {
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

async fn resolve_model(
    provider: ProviderKind,
    requested_model: Option<&str>,
    key: &str,
) -> Result<String> {
    let models = model_registry::get_models(provider, key).await?;
    if models.is_empty() {
        return Err(anyhow!(
            "model list is empty for provider {}",
            provider.as_str()
        ));
    }
    let compatible = filter_compatible_models(provider, &models);
    let suggestion_pool = if compatible.is_empty() {
        &models
    } else {
        &compatible
    };

    if let Some(requested) = requested_model {
        if models.iter().any(|model| model == requested) {
            if !is_model_compatible(provider, requested) {
                return Err(anyhow!(
                    "model '{}' is not compatible with chat/completions for provider {}",
                    requested,
                    provider.as_str()
                ));
            }
            return Ok(requested.to_string());
        }
        let suggestions = suggest_models(requested, suggestion_pool, 8);
        let hint = if suggestions.is_empty() {
            "no close matches found".to_string()
        } else {
            format!("did you mean: {}", suggestions.join(", "))
        };
        return Err(anyhow!(
            "model '{}' not found for provider {} ({})",
            requested,
            provider.as_str(),
            hint
        ));
    }

    if let Some(preferred) = preferred_default_model(provider) {
        if is_model_compatible(provider, preferred) && models.iter().any(|model| model == preferred)
        {
            return Ok(preferred.to_string());
        }
    }

    let candidates = if compatible.is_empty() {
        models
    } else {
        compatible
    };
    Ok(candidates[0].to_string())
}

fn suggest_models(requested: &str, models: &[String], limit: usize) -> Vec<String> {
    let requested_lower = requested.to_lowercase();
    let mut candidates = models
        .iter()
        .filter(|model| model.to_lowercase().contains(&requested_lower))
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates.extend(models.iter().take(limit).cloned());
    }
    candidates.truncate(limit);
    candidates
}

fn filter_compatible_models(provider: ProviderKind, models: &[String]) -> Vec<String> {
    models
        .iter()
        .filter(|model| is_model_compatible(provider, model))
        .cloned()
        .collect()
}

fn is_model_compatible(provider: ProviderKind, model: &str) -> bool {
    let lower = model.to_lowercase();
    match provider {
        ProviderKind::OpenAI => {
            lower.starts_with("gpt-") || lower.starts_with("o1") || lower.starts_with("o3")
        }
        ProviderKind::Gemini => lower.contains("gemini"),
        ProviderKind::Claude => true,
    }
}

fn preferred_default_model(provider: ProviderKind) -> Option<&'static str> {
    match provider {
        ProviderKind::OpenAI => Some("gpt-5.1"),
        ProviderKind::Gemini => Some("gemini-2.5-flash"),
        ProviderKind::Claude => Some("claude-sonnet-4-5-20250929"),
    }
}
