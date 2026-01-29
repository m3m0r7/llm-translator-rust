use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

pub mod attachments;
pub mod data;
pub mod dictionary;
pub mod languages;
pub mod logging;
mod model_registry;
pub mod ocr;
mod providers;
pub mod settings;
pub mod translations;
mod translator;

pub use providers::{Claude, Gemini, OpenAI, Provider, ProviderKind, ProviderUsage};
pub use translations::TranslateOptions;
pub use translator::{ExecutionOutput, TranslationInput, Translator};

#[derive(Debug, Clone)]
pub struct Config {
    pub lang: String,
    pub model: Option<String>,
    pub key: Option<String>,
    pub formal: String,
    pub source_lang: String,
    pub slang: bool,
    pub data: Option<String>,
    pub data_mime: Option<String>,
    pub data_attachment: Option<data::DataAttachment>,
    pub settings_path: Option<String>,
    pub show_enabled_languages: bool,
    pub show_enabled_styles: bool,
    pub show_models_list: bool,
    pub show_whisper_models: bool,
    pub pos: bool,
    pub show_histories: bool,
    pub with_using_tokens: bool,
    pub with_using_model: bool,
    pub debug_ocr: bool,
    pub verbose: bool,
    pub whisper_model: Option<String>,
}

pub async fn run(config: Config, input: Option<String>) -> Result<String> {
    let mut config = config;
    let settings_path = config.settings_path.as_deref().map(Path::new);
    let mut settings = settings::load_settings(settings_path)?;
    if let Some(model) = config.whisper_model.as_deref() {
        if !model.trim().is_empty() {
            settings.whisper_model = Some(model.to_string());
        }
    }
    let registry = languages::LanguageRegistry::load()?;
    let packs = languages::load_language_packs(&settings.system_languages)?;
    let ocr_languages = resolve_ocr_languages(&settings, &config.source_lang, &config.lang)?;
    info!(
        "settings loaded (history_limit={}, ocr_languages={})",
        settings.history_limit, ocr_languages
    );

    if config.show_enabled_languages || config.show_enabled_styles {
        return Ok(format_show_output(&config, &settings, &registry, &packs));
    }
    if config.show_models_list {
        return show_models_list(&config).await;
    }
    if config.show_whisper_models {
        return Ok(show_whisper_models());
    }
    if config.show_histories {
        return show_histories();
    }

    if config.data.is_none() && config.data_attachment.is_none() && config.data_mime.is_some() {
        return Err(anyhow!("--data-mime requires --data or stdin"));
    }

    let data_attachment = if let Some(attachment) = config.data_attachment.take() {
        Some(attachment)
    } else if let Some(path) = config.data.as_deref() {
        info!("loading attachment: {}", path);
        Some(data::load_attachment(
            Path::new(path),
            config.data_mime.as_deref(),
        )?)
    } else {
        None
    };
    let attachment_mime = data_attachment.as_ref().map(|data| data.mime.clone());
    let history_src = config.data.clone();

    let input = input.unwrap_or_default();
    let input = input.trim();
    if input.is_empty() && data_attachment.is_none() {
        return Err(anyhow!("stdin is empty"));
    }
    let formality = config.formal.trim().to_string();
    if formality.is_empty() {
        return Err(anyhow!("formality is empty"));
    }
    let with_using_model = config.with_using_model;
    let with_using_tokens = config.with_using_tokens;
    let input_text = input.to_string();
    let history_limit = settings.history_limit;

    let selection = if let Some(model_arg) = config.model.as_deref() {
        info!("model requested: {}", model_arg);
        providers::resolve_provider_selection(Some(model_arg), config.key.as_deref())?
    } else {
        match model_registry::get_last_using_model()? {
            Some(last) => providers::resolve_provider_selection(Some(&last), config.key.as_deref())
                .or_else(|_| providers::resolve_provider_selection(None, config.key.as_deref()))?,
            None => providers::resolve_provider_selection(None, config.key.as_deref())?,
        }
    };
    let key = providers::resolve_key(selection.provider, config.key.as_deref())
        .with_context(|| "no API key found for selected provider")?;

    let model = resolve_model(
        selection.provider,
        selection.requested_model.as_deref(),
        &key,
    )
    .await
    .with_context(|| "failed to resolve model")?;
    let history_model = model.clone();
    info!(
        "provider selected: {} (model={})",
        selection.provider.as_str(),
        history_model
    );

    validate_lang_codes(&config, &registry)?;

    model_registry::set_last_using_model(selection.provider, &model)?;
    let provider = providers::build_provider(selection.provider, key, model);
    let translator = Translator::new(provider, settings, registry);

    let options = TranslateOptions {
        lang: config.lang,
        formality,
        source_lang: config.source_lang,
        slang: config.slang,
    };
    let user_text = if data_attachment.is_some() {
        if input.is_empty() {
            format!("Translate the attached file into {}.", options.lang)
        } else {
            format!(
                "Translate the attached file into {}.\n\nAdditional instructions:\n{}",
                options.lang, input
            )
        }
    } else {
        input.to_string()
    };

    if config.pos {
        if data_attachment.is_some() {
            return Err(anyhow!("--pos only supports text input"));
        }
        info!("running dictionary mode");
        let execution = dictionary::exec_pos(&translator, input, &options).await?;
        return Ok(format_execution_output(
            &execution,
            with_using_model,
            with_using_tokens,
        ));
    }

    if let Some(data) = data_attachment.as_ref() {
        info!("translating attachment: {}", data.mime);
        if let Some(output) = attachments::translate_attachment(
            data,
            &ocr_languages,
            &translator,
            &options,
            config.debug_ocr,
            history_src.as_deref().map(Path::new),
        )
        .await?
        {
            let datetime = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string();
            let dest_path = model_registry::write_history_dest_bytes(&output.bytes, &datetime)?;
            let output_path = if let Some(src_path) = history_src.as_deref() {
                let translated = translated_output_path(Path::new(src_path), &output.mime)?;
                fs::write(&translated, &output.bytes)
                    .with_context(|| "failed to write translated file")?;
                translated
            } else {
                PathBuf::from(&dest_path)
            };
            let output_path = std::fs::canonicalize(&output_path).unwrap_or(output_path);
            let output_text = if output.mime.starts_with("image/") {
                let size_kb = output.bytes.len().div_ceil(1024);
                format!("Created image {} ({}KB) !", output_path.display(), size_kb)
            } else if output.mime.starts_with("audio/") {
                let size_kb = output.bytes.len().div_ceil(1024);
                format!("Created audio {} ({}KB) !", output_path.display(), size_kb)
            } else {
                output_path.to_string_lossy().to_string()
            };

            let entry = model_registry::HistoryEntry {
                datetime,
                model: format!("{}:{}", selection.provider.as_str(), history_model),
                mime: output.mime.clone(),
                kind: model_registry::HistoryType::Attachment,
                src: history_src.clone().unwrap_or_else(|| "stdin".to_string()),
                dest: dest_path.clone(),
            };
            if let Err(err) = model_registry::record_history(entry, history_limit) {
                warn!("failed to record history: {}", err);
            }

            let execution = ExecutionOutput {
                text: output_text,
                model: output.model,
                usage: output.usage,
            };

            return Ok(format_execution_output(
                &execution,
                with_using_model,
                with_using_tokens,
            ));
        }
    }

    let execution = translator
        .exec_with_data(
            TranslationInput {
                text: user_text,
                data: data_attachment,
            },
            options,
        )
        .await?;

    let output = format_execution_output(&execution, with_using_model, with_using_tokens);

    if let Err(err) = record_history(
        selection.provider,
        &history_model,
        history_src.as_deref(),
        history_limit,
        &input_text,
        attachment_mime.as_deref(),
        &execution.text,
    ) {
        eprintln!("warning: failed to record history: {}", err);
    }

    Ok(output)
}

fn translated_output_path(src: &Path, mime: &str) -> Result<PathBuf> {
    let ext = data::extension_from_mime(mime)
        .ok_or_else(|| anyhow!("unsupported output mime '{}'", mime))?;
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let stem = src
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("translated");
    let filename = format!("{}_translated.{}", stem, ext);
    Ok(parent.join(filename))
}

fn show_histories() -> Result<String> {
    let histories = model_registry::get_histories()?;
    if histories.is_empty() {
        return Ok("histories: 0".to_string());
    }
    let mut lines = Vec::new();
    lines.push(format!("histories: {}", histories.len()));
    for (idx, entry) in histories.iter().enumerate() {
        lines.push(format!("[{}]", idx + 1));
        lines.push(format!("  datetime: {}", entry.datetime));
        lines.push(format!("  type: {}", entry.kind.as_str()));
        lines.push(format!("  model: {}", entry.model));
        lines.push(format!("  mime: {}", entry.mime));
        lines.push(format!("  src: {}", summarize_history_value(&entry.src)));
        lines.push(format!("  dest: {}", summarize_history_value(&entry.dest)));
    }
    Ok(lines.join("\n"))
}

fn show_whisper_models() -> String {
    let models = [
        "tiny",
        "base",
        "small",
        "medium",
        "large",
        "large-v2",
        "large-v3",
        "tiny.en",
        "base.en",
        "small.en",
        "medium.en",
    ];
    let mut lines = Vec::new();
    lines.push("whisper models (ggml/gguf):".to_string());
    for model in models {
        lines.push(format!("- {}", model));
    }
    lines.push("note: *.en models are English-only".to_string());
    lines.join("\n")
}

fn summarize_history_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }
    let normalized = trimmed.replace('\n', "\\n").replace('\r', "\\r");
    let max_len = 160usize;
    if normalized.chars().count() > max_len {
        let preview: String = normalized.chars().take(max_len).collect();
        format!("{}...", preview)
    } else {
        normalized
    }
}

fn record_history(
    provider: ProviderKind,
    model: &str,
    src_path: Option<&str>,
    history_limit: usize,
    input_text: &str,
    attachment_mime: Option<&str>,
    output_text: &str,
) -> Result<()> {
    let datetime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let full_model = format!("{}:{}", provider.as_str(), model);
    let (kind, mime, src, dest) = if let Some(mime) = attachment_mime {
        let src = src_path
            .map(|value| value.to_string())
            .unwrap_or_else(|| "stdin".to_string());
        let dest = model_registry::write_history_dest(output_text, &datetime)?;
        (
            model_registry::HistoryType::Attachment,
            mime.to_string(),
            src,
            dest,
        )
    } else {
        (
            model_registry::HistoryType::Text,
            data::TEXT_MIME.to_string(),
            input_text.to_string(),
            output_text.to_string(),
        )
    };

    let entry = model_registry::HistoryEntry {
        datetime,
        model: full_model,
        mime,
        kind,
        src,
        dest,
    };
    model_registry::record_history(entry, history_limit)
}

fn resolve_ocr_languages(
    _settings: &settings::Settings,
    source_lang: &str,
    target_lang: &str,
) -> Result<String> {
    let mut langs = Vec::new();
    let source_trimmed = source_lang.trim();
    let mut required: Option<String> = None;
    if !source_trimmed.eq_ignore_ascii_case("auto") {
        if let Some(mapped) = map_lang_to_tesseract(source_trimmed) {
            required = Some(mapped.to_string());
            langs.push(mapped.to_string());
        } else if !source_trimmed.is_empty() {
            required = Some(source_trimmed.to_string());
            langs.push(source_trimmed.to_string());
        }
    }
    let target_trimmed = target_lang.trim();
    if !target_trimmed.is_empty() {
        if let Some(mapped) = map_lang_to_tesseract(target_trimmed) {
            langs.push(mapped.to_string());
        } else {
            langs.push(target_trimmed.to_string());
        }
    }

    langs.sort();
    langs.dedup();
    if !langs.is_empty() {
        if let Ok(available) = ocr::list_tesseract_languages() {
            if let Some(req) = required.as_deref() {
                if !available.iter().any(|value| value == req) {
                    return Err(anyhow!(
                        "tesseract language '{}' is not installed (available: {}). Install the language pack or change --source-lang.",
                        req,
                        available.join(", ")
                    ));
                }
            }
            let mut chosen = Vec::new();
            let mut missing = Vec::new();
            for lang in &langs {
                if available.iter().any(|value| value == lang) {
                    chosen.push(lang.clone());
                } else {
                    missing.push(lang.clone());
                }
            }
            if !missing.is_empty() {
                eprintln!(
                    "warning: tesseract language(s) not available: {} (available: {})",
                    missing.join(", "),
                    available.join(", ")
                );
            }
            if !chosen.is_empty() {
                return Ok(chosen.join("+"));
            }
        }
        return Ok(langs.join("+"));
    }

    Ok(guess_default_ocr_languages())
}

fn guess_default_ocr_languages() -> String {
    if let Ok(langs) = ocr::list_tesseract_languages() {
        if langs.iter().any(|lang| lang == "jpn") {
            return "jpn+eng".to_string();
        }
        if langs.iter().any(|lang| lang == "eng") {
            return "eng".to_string();
        }
        if let Some(first) = langs.first() {
            return first.clone();
        }
    }
    "eng".to_string()
}

fn map_lang_to_tesseract(code: &str) -> Option<&'static str> {
    let lower = code.trim().to_lowercase();
    match lower.as_str() {
        "ja" | "jpn" => Some("jpn"),
        "en" | "eng" => Some("eng"),
        "zh" | "zho" | "zh-cn" | "zh-hans" => Some("chi_sim"),
        "zh-hant" | "zh-tw" => Some("chi_tra"),
        "ko" | "kor" => Some("kor"),
        "fr" | "fra" => Some("fra"),
        "es" | "spa" => Some("spa"),
        "de" | "deu" => Some("deu"),
        "it" | "ita" => Some("ita"),
        "pt" | "por" => Some("por"),
        "ru" | "rus" => Some("rus"),
        _ => None,
    }
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
