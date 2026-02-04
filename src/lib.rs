use anyhow::{anyhow, Context, Result};
use futures_util::stream::{self, StreamExt};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn};

pub mod attachments;
mod backup;
mod build_env;
pub mod correction;
pub mod data;
pub mod details;
pub mod dictionary;
pub mod ext;
mod history_tags;
pub mod languages;
pub mod logging;
pub mod mcp;
mod model_registry;
pub mod ocr;
mod providers;
pub mod report;
pub mod server;
pub mod settings;
mod translation_ignore;
pub mod translations;
mod translator;

pub use providers::{Claude, Gemini, OpenAI, Provider, ProviderKind, ProviderUsage};
use translation_ignore::TranslationIgnore;
pub use translations::TranslateOptions;
pub use translator::{ExecutionOutput, TranslationInput, Translator};

#[cfg(test)]
mod test_util;

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
    pub directory_translation_threads: Option<usize>,
    pub ignore_translation_files: Vec<String>,
    pub out_path: Option<String>,
    pub overwrite: bool,
    pub force_translation: bool,
    pub settings_path: Option<String>,
    pub show_enabled_languages: bool,
    pub show_enabled_styles: bool,
    pub show_models_list: bool,
    pub show_whisper_models: bool,
    pub pos: bool,
    pub correction: bool,
    pub details: bool,
    pub report_format: Option<ReportFormat>,
    pub report_out: Option<String>,
    pub show_histories: bool,
    pub with_using_tokens: bool,
    pub with_using_model: bool,
    pub with_commentout: bool,
    pub debug_ocr: bool,
    pub verbose: bool,
    pub whisper_model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Html,
    Xml,
    Json,
}

impl ReportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ReportFormat::Html => "html",
            ReportFormat::Xml => "xml",
            ReportFormat::Json => "json",
        }
    }
}

pub async fn run(config: Config, input: Option<String>) -> Result<String> {
    let settings_path = config.settings_path.as_deref().map(Path::new);
    let settings = settings::load_settings(settings_path)?;
    run_with_loaded_settings(config, settings, input).await
}

pub async fn run_with_settings(
    config: Config,
    settings: settings::Settings,
    input: Option<String>,
) -> Result<String> {
    run_with_loaded_settings(config, settings, input).await
}

async fn run_with_loaded_settings(
    mut config: Config,
    mut settings: settings::Settings,
    input: Option<String>,
) -> Result<String> {
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

    if config.report_out.is_some() && config.report_format.is_none() {
        return Err(anyhow!("--report-out requires --report"));
    }
    let report_requested = config.report_format.is_some();
    if report_requested {
        if config.pos || config.correction || config.details {
            return Err(anyhow!(
                "--report cannot be used with --pos/--correction/--details"
            ));
        }
        if config.data.is_some()
            || config.data_attachment.is_some()
            || config.data_mime.is_some()
            || config.overwrite
            || config.out_path.is_some()
        {
            return Err(anyhow!(
                "--report cannot be used with --data/--data-mime/--overwrite/--out"
            ));
        }
    }

    if !report_requested
        && config.data.is_none()
        && config.data_attachment.is_none()
        && config.data_mime.is_some()
    {
        return Err(anyhow!("--data-mime requires --data or stdin"));
    }

    let data_path = config.data.as_deref().map(Path::new);
    let data_is_dir = if let Some(path) = data_path {
        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to read --data path: {}", path.display()))?;
        metadata.is_dir()
    } else {
        false
    };
    if config.overwrite && config.data.is_none() {
        return Err(anyhow!("--overwrite requires --data path"));
    }
    if config.overwrite && config.out_path.is_some() {
        return Err(anyhow!("--out cannot be used with --overwrite"));
    }

    let mut needs_mime_detection = false;
    let mut data_attachment = if data_is_dir {
        None
    } else if let Some(attachment) = config.data_attachment.take() {
        Some(attachment)
    } else if let Some(path) = data_path {
        info!("loading attachment: {}", path.display());
        match data::load_attachment(path, config.data_mime.as_deref()) {
            Ok(attachment) => Some(attachment),
            Err(err) => {
                let mime_hint = config.data_mime.as_deref().unwrap_or("auto");
                if mime_hint.eq_ignore_ascii_case("auto") {
                    let bytes = fs::read(path)
                        .with_context(|| format!("failed to read data file: {}", path.display()))?;
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .map(|value| value.to_string());
                    needs_mime_detection = true;
                    Some(data::DataAttachment {
                        bytes,
                        mime: data::OCTET_STREAM_MIME.to_string(),
                        name,
                    })
                } else {
                    return Err(err);
                }
            }
        }
    } else {
        None
    };
    let history_src = config.data.clone();

    if config.out_path.is_some() && !data_is_dir && data_attachment.is_none() {
        return Err(anyhow!("--out requires --data or stdin attachment"));
    }

    let input = input.unwrap_or_default();
    let input = input.trim();
    if !report_requested && input.is_empty() && data_attachment.is_none() && !data_is_dir {
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
    let translated_suffix = settings.translated_suffix.clone();
    let backup_ttl_days = settings.backup_ttl_days;
    let translation_ignore_file = settings.translation_ignore_file.clone();
    let directory_threads = config
        .directory_translation_threads
        .unwrap_or(settings.directory_translation_threads)
        .max(1);
    let out_path = config.out_path.as_ref().map(PathBuf::from);

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

    let report_lang_hint = config.source_lang.clone();
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

    if let Some(report_format) = config.report_format {
        let histories = model_registry::get_histories()?;
        let report = report::build_report(&translator, &histories, Some(&report_lang_hint)).await?;
        let rendered = report::render_report(&report, report_format)?;
        let output_path = report::resolve_report_out(report_format, config.report_out.as_deref());
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create report directory: {}", parent.display())
                })?;
            }
        }
        fs::write(&output_path, rendered)
            .with_context(|| format!("failed to write report: {}", output_path.display()))?;
        let resolved = std::fs::canonicalize(&output_path).unwrap_or(output_path);
        return Ok(resolved.to_string_lossy().to_string());
    }

    if config.correction {
        if config.pos {
            return Err(anyhow!("--correction cannot be used with --pos"));
        }
        if config.details {
            return Err(anyhow!("--correction cannot be used with --details"));
        }
        if data_attachment.is_some() || data_is_dir {
            return Err(anyhow!("--correction only supports text input"));
        }
        info!("running correction mode");
        let output = correction::exec_correction(&translator, input, &options).await?;
        let formatted = correction::format_correction_output(&output.result);
        let execution = ExecutionOutput {
            text: formatted,
            model: output.model,
            usage: output.usage,
        };
        return Ok(format_execution_output(
            &execution,
            with_using_model,
            with_using_tokens,
        ));
    }

    if config.pos {
        if config.details {
            return Err(anyhow!("--pos cannot be used with --details"));
        }
        if data_attachment.is_some() || data_is_dir {
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

    if config.details {
        if data_attachment.is_some() || data_is_dir {
            return Err(anyhow!("--details only supports text input"));
        }
        info!("running details mode");
        let output = details::exec_details(&translator, input, &options).await?;
        let execution = ExecutionOutput {
            text: output.result.details,
            model: output.model,
            usage: output.usage,
        };
        return Ok(format_execution_output(
            &execution,
            with_using_model,
            with_using_tokens,
        ));
    }

    if data_is_dir {
        let src_dir = data_path.ok_or_else(|| anyhow!("--data directory not found"))?;
        if let Some(out) = out_path.as_ref() {
            if out.exists() && out.is_file() {
                return Err(anyhow!(
                    "--out must be a directory when --data is a directory"
                ));
            }
            if let (Ok(src_abs), Ok(out_abs)) =
                (std::fs::canonicalize(src_dir), std::fs::canonicalize(out))
            {
                if src_abs == out_abs {
                    return Err(anyhow!(
                        "--out must be different from the source directory (use --overwrite to write in place)"
                    ));
                }
            }
        }
        let ignore = build_translation_ignore(
            src_dir,
            translation_ignore_file.as_str(),
            &config.ignore_translation_files,
        )?;
        let dir_config = DirTranslateConfig {
            mime_hint: config.data_mime.clone(),
            ocr_languages: ocr_languages.clone(),
            options: options.clone(),
            with_commentout: config.with_commentout,
            debug_ocr: config.debug_ocr,
            overwrite: config.overwrite,
            force_translation: config.force_translation,
            translated_suffix,
            backup_ttl_days,
            provider: selection.provider,
            history_model,
            history_limit,
            directory_threads,
            ignore,
            output_dir: out_path.clone(),
        };
        let output = translate_data_dir(src_dir, &translator, dir_config).await?;
        return Ok(output);
    }

    if needs_mime_detection {
        if let Some(attachment) = data_attachment.as_mut() {
            let detection = attachments::detect_mime_with_llm(attachment, &translator).await?;
            let normalized = data::normalize_mime_hint(&detection.mime);
            if detection.confident {
                if let Some(mime) = normalized {
                    attachment.mime = mime;
                } else if config.force_translation {
                    attachment.mime = data::TEXT_MIME.to_string();
                } else {
                    return Err(anyhow!(
                        "unable to determine supported mime for '{}' (detected '{}'); use --force to treat as text",
                        attachment
                            .name
                            .as_deref()
                            .unwrap_or("attachment"),
                        detection.mime
                    ));
                }
            } else if config.force_translation {
                attachment.mime = data::TEXT_MIME.to_string();
            } else {
                return Err(anyhow!(
                    "unable to determine mime for '{}' (low confidence); use --force to treat as text",
                    attachment
                        .name
                        .as_deref()
                        .unwrap_or("attachment")
                ));
            }
        }
    }

    if let Some(data) = data_attachment.as_ref() {
        info!("translating attachment: {}", data.mime);
        if let Some(output) = attachments::translate_attachment(
            data,
            &ocr_languages,
            &translator,
            &options,
            config.with_commentout,
            config.debug_ocr,
            config.force_translation,
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
            let output_path = if let Some(out) = out_path.as_ref() {
                let src_path = history_src.as_deref().map(Path::new);
                let data_name = data.name.as_deref();
                let output_path = resolve_out_path(out, src_path, data_name, &output.mime)?;
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create output dir: {}", parent.display())
                    })?;
                }
                fs::write(&output_path, &output.bytes)
                    .with_context(|| "failed to write translated file")?;
                output_path
            } else if let Some(src_path) = history_src.as_deref() {
                let src_path = Path::new(src_path);
                let translated = if config.overwrite {
                    backup::backup_file(src_path, backup_ttl_days)?;
                    src_path.to_path_buf()
                } else {
                    translated_output_path(src_path, &output.mime, &translated_suffix)?
                };
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
                formal: Some(options.formality.clone()),
                mime: output.mime.clone(),
                kind: model_registry::HistoryType::Attachment,
                source_language: normalize_lang_for_history(&options.source_lang),
                target_language: normalize_lang_for_history(&options.lang),
                tags: None,
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

    let attachment_mime = data_attachment.as_ref().map(|data| data.mime.clone());

    let execution = translator
        .exec_with_data(
            TranslationInput {
                text: user_text,
                data: data_attachment,
            },
            options.clone(),
        )
        .await?;

    let output = format_execution_output(&execution, with_using_model, with_using_tokens);

    let tags = if attachment_mime.is_none() {
        match history_tags::generate_history_tags(
            &translator,
            &input_text,
            &options.source_lang,
            &options.lang,
        )
        .await
        {
            Ok(tags) if !tags.is_empty() => Some(tags),
            Ok(_) => None,
            Err(err) => {
                warn!("failed to generate history tags: {}", err);
                None
            }
        }
    } else {
        None
    };

    if let Err(err) = record_history(
        selection.provider,
        &history_model,
        history_src.as_deref(),
        history_limit,
        &input_text,
        attachment_mime.as_deref(),
        &execution.text,
        &options.source_lang,
        &options.lang,
        &options.formality,
        tags,
    ) {
        eprintln!("warning: failed to record history: {}", err);
    }

    Ok(output)
}

fn translated_output_path(src: &Path, mime: &str, suffix: &str) -> Result<PathBuf> {
    let ext = data::extension_from_mime(mime)
        .ok_or_else(|| anyhow!("unsupported output mime '{}'", mime))?;
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let stem = src
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("translated");
    let filename = if suffix.is_empty() {
        format!("{}.{}", stem, ext)
    } else {
        format!("{}{}.{}", stem, suffix, ext)
    };
    Ok(parent.join(filename))
}

fn translated_output_dir(src: &Path, suffix: &str) -> PathBuf {
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let base = src
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("translated");
    if suffix.is_empty() {
        parent.join(base)
    } else {
        parent.join(format!("{}{}", base, suffix))
    }
}

fn translated_output_path_in_dir(
    src_root: &Path,
    dest_root: &Path,
    src_file: &Path,
    input_mime: &str,
    output_mime: &str,
) -> Result<PathBuf> {
    let rel = src_file
        .strip_prefix(src_root)
        .with_context(|| "failed to resolve relative path")?;
    let mut dest = dest_root.join(rel);
    if output_mime != input_mime {
        if let Some(ext) = data::extension_from_mime(output_mime) {
            dest.set_extension(ext);
        }
    }
    Ok(dest)
}

fn copy_output_path_in_dir(src_root: &Path, dest_root: &Path, src_file: &Path) -> Result<PathBuf> {
    let rel = src_file
        .strip_prefix(src_root)
        .with_context(|| "failed to resolve relative path")?;
    Ok(dest_root.join(rel))
}

fn resolve_out_path(
    out: &Path,
    src_path: Option<&Path>,
    data_name: Option<&str>,
    output_mime: &str,
) -> Result<PathBuf> {
    if out.exists() && out.is_dir() {
        let base = src_path
            .and_then(|value| value.file_stem().and_then(|stem| stem.to_str()))
            .or_else(|| data_name.and_then(|value| Path::new(value).file_stem()?.to_str()))
            .unwrap_or("translated");
        let ext = data::extension_from_mime(output_mime)
            .ok_or_else(|| anyhow!("unsupported output mime '{}'", output_mime))?;
        return Ok(out.join(format!("{}.{}", base, ext)));
    }
    Ok(out.to_path_buf())
}

fn collect_directory_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| "failed to read directory entry")?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| "failed to read file type")?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn build_translation_ignore(
    src_dir: &Path,
    ignore_file_name: &str,
    cli_patterns: &[String],
) -> Result<Option<TranslationIgnore>> {
    let mut patterns = Vec::new();
    let name = ignore_file_name.trim();
    if !name.is_empty() {
        let ignore_path = Path::new(name);
        let path = if ignore_path.is_absolute() {
            ignore_path.to_path_buf()
        } else {
            src_dir.join(name)
        };
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read ignore file: {}", path.display()))?;
            for line in content.lines() {
                patterns.push(line.to_string());
            }
        }
    }
    patterns.extend(cli_patterns.iter().cloned());

    TranslationIgnore::new(src_dir, patterns)
}

struct DirTranslateConfig {
    mime_hint: Option<String>,
    ocr_languages: String,
    options: TranslateOptions,
    with_commentout: bool,
    debug_ocr: bool,
    overwrite: bool,
    force_translation: bool,
    translated_suffix: String,
    backup_ttl_days: u64,
    provider: ProviderKind,
    history_model: String,
    history_limit: usize,
    directory_threads: usize,
    ignore: Option<TranslationIgnore>,
    output_dir: Option<PathBuf>,
}

#[derive(Clone)]
struct DirTranslateShared {
    src_dir: PathBuf,
    output_dir: Option<PathBuf>,
    mime_hint: Option<String>,
    ocr_languages: String,
    options: TranslateOptions,
    with_commentout: bool,
    debug_ocr: bool,
    overwrite: bool,
    force_translation: bool,
    backup_ttl_days: u64,
    provider: ProviderKind,
    history_model: String,
    history_limit: usize,
    ignore: Option<TranslationIgnore>,
    meta_lock: Arc<Mutex<()>>,
}

enum DirItemStatus {
    Translated,
    Copied,
    Skipped,
    Failed,
}

struct DirItemResult {
    status: DirItemStatus,
    message: Option<String>,
}

async fn translate_data_dir<P: Provider + Clone>(
    src_dir: &Path,
    translator: &Translator<P>,
    config: DirTranslateConfig,
) -> Result<String> {
    let output_dir = if config.overwrite {
        None
    } else if let Some(out) = config.output_dir.as_ref() {
        Some(out.clone())
    } else {
        Some(translated_output_dir(
            src_dir,
            config.translated_suffix.as_str(),
        ))
    };
    if let Some(dir) = output_dir.as_ref() {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create output dir: {}", dir.display()))?;
    }

    let files = collect_directory_files(src_dir)?;
    if files.is_empty() {
        let message = if let Some(dir) = output_dir.as_ref() {
            let output_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
            format!(
                "No files found in {} (output dir: {})",
                src_dir.display(),
                output_dir.display()
            )
        } else {
            format!("No files found in {}", src_dir.display())
        };
        return Ok(message);
    }

    let shared = Arc::new(DirTranslateShared {
        src_dir: src_dir.to_path_buf(),
        output_dir,
        mime_hint: config.mime_hint,
        ocr_languages: config.ocr_languages,
        options: config.options,
        with_commentout: config.with_commentout,
        debug_ocr: config.debug_ocr,
        overwrite: config.overwrite,
        force_translation: config.force_translation,
        backup_ttl_days: config.backup_ttl_days,
        provider: config.provider,
        history_model: config.history_model,
        history_limit: config.history_limit,
        ignore: config.ignore,
        meta_lock: Arc::new(Mutex::new(())),
    });

    let concurrency = config.directory_threads.max(1);
    let results: Vec<DirItemResult> = stream::iter(files)
        .map(|path| {
            let translator = translator.clone();
            let shared = shared.clone();
            async move { process_dir_file(path, translator, shared).await }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let mut translated = 0usize;
    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for result in results {
        match result.status {
            DirItemStatus::Translated => translated += 1,
            DirItemStatus::Copied => copied += 1,
            DirItemStatus::Skipped => skipped += 1,
            DirItemStatus::Failed => failed += 1,
        }
        if let Some(message) = result.message {
            failures.push(message);
        }
    }

    let mut lines = Vec::new();
    if let Some(dir) = shared.output_dir.as_ref() {
        let output_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        lines.push(format!("Translated directory: {}", output_dir.display()));
    } else {
        lines.push(format!("Overwrote directory: {}", src_dir.display()));
        let backup_dir = backup::backup_dir();
        lines.push(format!("backup dir: {}", backup_dir.display()));
    }
    lines.push(format!(
        "files: {} translated, {} copied, {} skipped, {} failed (total {})",
        translated,
        copied,
        skipped,
        failed,
        translated + copied + skipped + failed
    ));
    if !failures.is_empty() {
        lines.push("failures:".to_string());
        let limit = 20usize;
        for message in failures.iter().take(limit) {
            lines.push(format!("- {}", message));
        }
        if failures.len() > limit {
            lines.push(format!("... and {} more", failures.len() - limit));
        }
    }

    Ok(lines.join("\n"))
}

async fn process_dir_file<P: Provider + Clone>(
    path: PathBuf,
    translator: Translator<P>,
    shared: Arc<DirTranslateShared>,
) -> DirItemResult {
    if let Some(ignore) = &shared.ignore {
        if ignore.is_ignored(&path) {
            return copy_or_skip(&path, &shared, None);
        }
    }

    let attachment = match data::load_attachment(&path, shared.mime_hint.as_deref()) {
        Ok(value) => value,
        Err(err) => {
            let mime_hint = shared.mime_hint.as_deref().unwrap_or("auto");
            if mime_hint.eq_ignore_ascii_case("auto") {
                let bytes = match fs::read(&path) {
                    Ok(value) => value,
                    Err(read_err) => return copy_or_skip(&path, &shared, Some(read_err.into())),
                };
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_string());
                let mut attachment = data::DataAttachment {
                    bytes,
                    mime: data::OCTET_STREAM_MIME.to_string(),
                    name,
                };
                let detection =
                    match attachments::detect_mime_with_llm(&attachment, &translator).await {
                        Ok(value) => value,
                        Err(detect_err) => return copy_or_skip(&path, &shared, Some(detect_err)),
                    };
                let normalized = data::normalize_mime_hint(&detection.mime);
                if detection.confident {
                    if let Some(mime) = normalized {
                        attachment.mime = mime;
                    } else if shared.force_translation {
                        attachment.mime = data::TEXT_MIME.to_string();
                    } else {
                        return copy_or_skip(
                            &path,
                            &shared,
                            Some(anyhow!(
                                "unable to determine supported mime (detected '{}')",
                                detection.mime
                            )),
                        );
                    }
                } else if shared.force_translation {
                    attachment.mime = data::TEXT_MIME.to_string();
                } else {
                    return copy_or_skip(
                        &path,
                        &shared,
                        Some(anyhow!("unable to determine mime (low confidence)")),
                    );
                }
                attachment
            } else {
                return copy_or_skip(&path, &shared, Some(err));
            }
        }
    };

    let debug_src = if shared.debug_ocr {
        Some(path.as_path())
    } else {
        None
    };
    let output = attachments::translate_attachment(
        &attachment,
        &shared.ocr_languages,
        &translator,
        &shared.options,
        shared.with_commentout,
        shared.debug_ocr,
        shared.force_translation,
        debug_src,
    )
    .await;
    let output = match output {
        Ok(value) => value,
        Err(err) => return copy_or_skip(&path, &shared, Some(err)),
    };
    let output = match output {
        Some(value) => value,
        None => return copy_or_skip(&path, &shared, None),
    };

    let output_path = if shared.overwrite {
        let guard = shared.meta_lock.lock().await;
        if let Err(err) = backup::backup_file(&path, shared.backup_ttl_days) {
            drop(guard);
            return copy_or_skip(&path, &shared, Some(anyhow!("backup failed: {}", err)));
        }
        drop(guard);
        path.clone()
    } else {
        let Some(dest_root) = shared.output_dir.as_ref() else {
            return DirItemResult {
                status: DirItemStatus::Failed,
                message: Some(format!("{}: output dir missing", path.display())),
            };
        };
        match translated_output_path_in_dir(
            &shared.src_dir,
            dest_root,
            &path,
            &attachment.mime,
            &output.mime,
        ) {
            Ok(value) => value,
            Err(err) => {
                return DirItemResult {
                    status: DirItemStatus::Failed,
                    message: Some(format!("{}: {}", path.display(), err)),
                }
            }
        }
    };
    if let Some(parent) = output_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            return DirItemResult {
                status: DirItemStatus::Failed,
                message: Some(format!(
                    "{}: failed to create output dir: {}",
                    path.display(),
                    err
                )),
            };
        }
    }
    if let Err(err) = fs::write(&output_path, &output.bytes) {
        return DirItemResult {
            status: DirItemStatus::Failed,
            message: Some(format!(
                "{}: failed to write output: {}",
                path.display(),
                err
            )),
        };
    }

    let datetime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    match model_registry::write_history_dest_bytes(&output.bytes, &datetime) {
        Ok(dest_path) => {
            let entry = model_registry::HistoryEntry {
                datetime,
                model: format!("{}:{}", shared.provider.as_str(), shared.history_model),
                formal: Some(shared.options.formality.clone()),
                mime: output.mime.clone(),
                kind: model_registry::HistoryType::Attachment,
                source_language: normalize_lang_for_history(&shared.options.source_lang),
                target_language: normalize_lang_for_history(&shared.options.lang),
                tags: None,
                src: path.to_string_lossy().to_string(),
                dest: dest_path,
            };
            let guard = shared.meta_lock.lock().await;
            if let Err(err) = model_registry::record_history(entry, shared.history_limit) {
                warn!("failed to record history: {}", err);
            }
            drop(guard);
        }
        Err(err) => {
            warn!("failed to write history output: {}", err);
        }
    }

    DirItemResult {
        status: DirItemStatus::Translated,
        message: None,
    }
}

fn copy_or_skip(
    path: &Path,
    shared: &DirTranslateShared,
    err: Option<anyhow::Error>,
) -> DirItemResult {
    if let Some(dest_root) = shared.output_dir.as_ref() {
        match copy_output_path_in_dir(&shared.src_dir, dest_root, path) {
            Ok(dest) => {
                if let Some(parent) = dest.parent() {
                    if let Err(copy_err) = fs::create_dir_all(parent) {
                        return DirItemResult {
                            status: DirItemStatus::Failed,
                            message: Some(format!(
                                "{}: failed to create output dir: {}",
                                path.display(),
                                copy_err
                            )),
                        };
                    }
                }
                if let Err(copy_err) = fs::copy(path, &dest) {
                    return DirItemResult {
                        status: DirItemStatus::Failed,
                        message: Some(format!(
                            "{}: failed to copy original: {}",
                            path.display(),
                            copy_err
                        )),
                    };
                }
                return DirItemResult {
                    status: if err.is_some() {
                        DirItemStatus::Failed
                    } else {
                        DirItemStatus::Copied
                    },
                    message: err.map(|value| format!("{}: {}", path.display(), value)),
                };
            }
            Err(copy_err) => {
                return DirItemResult {
                    status: DirItemStatus::Failed,
                    message: Some(format!("{}: {}", path.display(), copy_err)),
                };
            }
        }
    }

    DirItemResult {
        status: if err.is_some() {
            DirItemStatus::Failed
        } else {
            DirItemStatus::Skipped
        },
        message: err.map(|value| format!("{}: {}", path.display(), value)),
    }
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
        if let Some(formal) = entry.formal.as_deref() {
            lines.push(format!("  formal: {}", formal));
        }
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
    source_lang: &str,
    target_lang: &str,
    formal: &str,
    tags: Option<Vec<String>>,
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
        formal: normalize_formality_for_history(formal),
        mime,
        kind,
        source_language: normalize_lang_for_history(source_lang),
        target_language: normalize_lang_for_history(target_lang),
        tags,
        src,
        dest,
    };
    model_registry::record_history(entry, history_limit)
}

fn normalize_lang_for_history(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_lowercase())
    }
}

fn normalize_formality_for_history(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn resolve_ocr_languages(
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

pub(crate) fn validate_lang_codes(
    config: &Config,
    registry: &languages::LanguageRegistry,
) -> Result<()> {
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

pub(crate) async fn resolve_model(
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
        ProviderKind::OpenAI => Some("gpt-5.2"),
        ProviderKind::Gemini => Some("gemini-2.5-flash"),
        ProviderKind::Claude => Some("claude-sonnet-4-5-20250929"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_path_uses_suffix_and_extension() {
        let src = Path::new("/tmp/sample.txt");
        let path = translated_output_path(src, data::XML_MIME, "_x").expect("path");
        assert!(path.to_string_lossy().ends_with("sample_x.xml"));

        let path = translated_output_path(src, data::XML_MIME, "").expect("path");
        assert!(path.to_string_lossy().ends_with("sample.xml"));
    }

    #[test]
    fn output_dir_uses_suffix() {
        let dir = Path::new("/tmp/data");
        let path = translated_output_dir(dir, "_translated");
        assert!(path.to_string_lossy().ends_with("data_translated"));

        let path = translated_output_dir(dir, "");
        assert!(path.to_string_lossy().ends_with("data"));
    }

    #[test]
    fn output_path_in_dir_changes_extension_when_mime_changes() {
        let src_root = Path::new("/tmp/src");
        let dest_root = Path::new("/tmp/out");
        let src_file = Path::new("/tmp/src/sub/file.txt");
        let dest = translated_output_path_in_dir(
            src_root,
            dest_root,
            src_file,
            data::TEXT_MIME,
            data::MARKDOWN_MIME,
        )
        .expect("dest");
        assert!(dest.to_string_lossy().ends_with("/tmp/out/sub/file.md"));
    }
}
