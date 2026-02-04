use anyhow::{Context, Result};
use futures_util::stream::{self, StreamExt};
use std::path::{Path, PathBuf};

use crate::attachments;
use crate::correction;
use crate::data;
use crate::model_registry;
use crate::providers;
use crate::settings;
use crate::translations::TranslateOptions;
use crate::{resolve_model, resolve_ocr_languages, validate_lang_codes, Config, Translator};

use super::models::{CorrectionPayload, ServerContent, ServerRequest, ServerResponse};
use super::state::ServerState;
use super::util::{
    build_translation_ignore, collect_directory_files, decode_text, is_text_mime, resolve_tmp_dir,
    write_temp_file,
};

#[derive(Debug)]
pub(crate) struct ServerError {
    pub(crate) status: axum::http::StatusCode,
    pub(crate) message: String,
}

impl ServerError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ServerError {
    fn from(err: anyhow::Error) -> Self {
        ServerError::internal(err.to_string())
    }
}

pub(crate) async fn translate_request(
    state: &ServerState,
    request: ServerRequest,
) -> Result<ServerResponse, ServerError> {
    if request.text.is_some() && request.data.is_some() {
        return Err(ServerError::bad_request(
            "text and data cannot be provided together",
        ));
    }

    let config = config_from_request(&request);
    let registry = state.registry.clone();
    let mut settings = state.settings.clone();
    if let Some(model) = config.whisper_model.as_deref() {
        if !model.trim().is_empty() {
            settings.whisper_model = Some(model.to_string());
        }
    }

    validate_lang_codes(&config, &registry)
        .map_err(|err| ServerError::bad_request(err.to_string()))?;
    let ocr_languages = resolve_ocr_languages(&settings, &config.source_lang, &config.lang)
        .map_err(|err| ServerError::bad_request(err.to_string()))?;

    let selection = if let Some(model_arg) = config.model.as_deref() {
        providers::resolve_provider_selection(Some(model_arg), config.key.as_deref())
            .map_err(|err| ServerError::bad_request(err.to_string()))?
    } else {
        match model_registry::get_last_using_model().map_err(ServerError::from)? {
            Some(last) => providers::resolve_provider_selection(Some(&last), config.key.as_deref())
                .or_else(|_| providers::resolve_provider_selection(None, config.key.as_deref()))
                .map_err(|err| ServerError::bad_request(err.to_string()))?,
            None => providers::resolve_provider_selection(None, config.key.as_deref())
                .map_err(|err| ServerError::bad_request(err.to_string()))?,
        }
    };
    let key = providers::resolve_key(selection.provider, config.key.as_deref())
        .map_err(|err| ServerError::bad_request(err.to_string()))?;

    let model = resolve_model(
        selection.provider,
        selection.requested_model.as_deref(),
        &key,
    )
    .await
    .map_err(|err| ServerError::bad_request(err.to_string()))?;
    let provider = providers::build_provider(selection.provider, key, model.clone());
    let translator = Translator::new(provider, settings.clone(), registry);
    let options = TranslateOptions {
        lang: config.lang.clone(),
        formality: config.formal.clone(),
        source_lang: config.source_lang.clone(),
        slang: config.slang,
    };

    if config.correction {
        if request.data.is_some() {
            return Err(ServerError::bad_request(
                "correction only supports text input",
            ));
        }
        let Some(text) = request.text else {
            return Err(ServerError::bad_request("text is required for correction"));
        };
        if text.trim().is_empty() {
            return Err(ServerError::bad_request("text is empty"));
        }
        return translate_correction(&translator, &options, text).await;
    }

    if let Some(path) = config.data.as_deref() {
        let path = Path::new(path);
        let meta = std::fs::metadata(path)
            .with_context(|| format!("failed to read data path: {}", path.display()))
            .map_err(ServerError::from)?;
        if meta.is_dir() {
            let contents = translate_directory(
                path,
                &translator,
                &options,
                &settings,
                &ocr_languages,
                &config,
            )
            .await?;
            return Ok(ServerResponse { contents });
        }

        let content = translate_file(
            path,
            &translator,
            &options,
            &settings,
            &ocr_languages,
            &config,
        )
        .await?;
        return Ok(ServerResponse {
            contents: vec![content],
        });
    }

    let Some(text) = request.text else {
        return Err(ServerError::bad_request("text or data is required"));
    };
    if text.trim().is_empty() {
        return Err(ServerError::bad_request("text is empty"));
    }
    let exec = translator
        .exec(text.as_str(), options)
        .await
        .map_err(ServerError::from)?;
    Ok(ServerResponse {
        contents: vec![ServerContent {
            mime: data::TEXT_MIME.to_string(),
            format: "raw".to_string(),
            original: Some(text),
            translated: exec.text,
            correction: None,
        }],
    })
}

fn config_from_request(request: &ServerRequest) -> Config {
    Config {
        lang: request.lang.clone().unwrap_or_else(|| "en".to_string()),
        model: request.model.clone(),
        key: request.key.clone(),
        formal: request
            .formal
            .clone()
            .unwrap_or_else(|| "formal".to_string()),
        source_lang: request
            .source_lang
            .clone()
            .unwrap_or_else(|| "auto".to_string()),
        slang: request.slang.unwrap_or(false),
        data: request.data.clone(),
        data_mime: request.data_mime.clone(),
        data_attachment: None,
        directory_translation_threads: request.directory_translation_threads,
        ignore_translation_files: request.ignore_translation_files.clone().unwrap_or_default(),
        out_path: None,
        overwrite: false,
        force_translation: request.force_translation.unwrap_or(false),
        settings_path: None,
        show_enabled_languages: false,
        show_enabled_styles: false,
        show_models_list: false,
        show_whisper_models: false,
        pos: false,
        correction: request.correction.unwrap_or(false),
        details: false,
        show_histories: false,
        with_using_tokens: false,
        with_using_model: false,
        with_commentout: request.with_commentout.unwrap_or(false),
        debug_ocr: request.debug_ocr.unwrap_or(false),
        verbose: false,
        whisper_model: request.whisper_model.clone(),
    }
}

async fn translate_file<P: providers::Provider + Clone>(
    path: &Path,
    translator: &Translator<P>,
    options: &TranslateOptions,
    settings: &settings::Settings,
    ocr_languages: &str,
    config: &Config,
) -> Result<ServerContent, ServerError> {
    let attachment = load_attachment_with_detection(
        path,
        config.data_mime.as_deref(),
        config.force_translation,
        translator,
    )
    .await?;

    let debug_src = if config.debug_ocr { Some(path) } else { None };
    let output = attachments::translate_attachment(
        &attachment,
        ocr_languages,
        translator,
        options,
        config.with_commentout,
        config.debug_ocr,
        config.force_translation,
        debug_src,
    )
    .await
    .map_err(ServerError::from)?
    .ok_or_else(|| ServerError::bad_request("unsupported attachment mime"))?;

    content_from_attachment(
        &attachment,
        &output,
        config.force_translation,
        resolve_tmp_dir(settings)?,
    )
}

async fn translate_directory<P: providers::Provider + Clone>(
    path: &Path,
    translator: &Translator<P>,
    options: &TranslateOptions,
    settings: &settings::Settings,
    ocr_languages: &str,
    config: &Config,
) -> Result<Vec<ServerContent>, ServerError> {
    let files = collect_directory_files(path).map_err(ServerError::from)?;
    let ignore = build_translation_ignore(
        path,
        settings.translation_ignore_file.as_str(),
        &config.ignore_translation_files,
    )
    .map_err(ServerError::from)?;
    let concurrency = config
        .directory_translation_threads
        .unwrap_or(settings.directory_translation_threads)
        .max(1);
    let tmp_dir = resolve_tmp_dir(settings)?;

    let results: Vec<Result<Option<ServerContent>, ServerError>> = stream::iter(files)
        .map(|file| {
            let translator = translator.clone();
            let options = options.clone();
            let ignore = ignore.clone();
            let tmp_dir = tmp_dir.clone();
            async move {
                if let Some(ignore) = ignore {
                    if ignore.is_ignored(&file) {
                        return Ok(None);
                    }
                }
                let attachment = load_attachment_with_detection(
                    &file,
                    config.data_mime.as_deref(),
                    config.force_translation,
                    &translator,
                )
                .await?;
                let debug_src = if config.debug_ocr {
                    Some(file.as_path())
                } else {
                    None
                };
                let output = attachments::translate_attachment(
                    &attachment,
                    ocr_languages,
                    &translator,
                    &options,
                    config.with_commentout,
                    config.debug_ocr,
                    config.force_translation,
                    debug_src,
                )
                .await
                .map_err(ServerError::from)?;
                let output = match output {
                    Some(value) => value,
                    None => return Ok(None),
                };
                let content = content_from_attachment(
                    &attachment,
                    &output,
                    config.force_translation,
                    tmp_dir,
                )?;
                Ok(Some(content))
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let mut contents = Vec::new();
    for result in results {
        match result {
            Ok(Some(content)) => contents.push(content),
            Ok(None) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(contents)
}

async fn load_attachment_with_detection<P: providers::Provider + Clone>(
    path: &Path,
    mime_hint: Option<&str>,
    force_translation: bool,
    translator: &Translator<P>,
) -> Result<data::DataAttachment, ServerError> {
    match data::load_attachment(path, mime_hint) {
        Ok(attachment) => Ok(attachment),
        Err(err) => {
            let hint = mime_hint.unwrap_or("auto");
            if !hint.eq_ignore_ascii_case("auto") {
                return Err(ServerError::bad_request(err.to_string()));
            }
            let bytes = std::fs::read(path)
                .with_context(|| format!("failed to read data file: {}", path.display()))
                .map_err(ServerError::from)?;
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string());
            let mut attachment = data::DataAttachment {
                bytes,
                mime: data::OCTET_STREAM_MIME.to_string(),
                name,
            };
            let detection = attachments::detect_mime_with_llm(&attachment, translator)
                .await
                .map_err(ServerError::from)?;
            let normalized = data::normalize_mime_hint(&detection.mime);
            if detection.confident {
                if let Some(mime) = normalized {
                    attachment.mime = mime;
                } else if force_translation {
                    attachment.mime = data::TEXT_MIME.to_string();
                } else {
                    return Err(ServerError::bad_request(format!(
                        "unable to determine supported mime (detected '{}')",
                        detection.mime
                    )));
                }
            } else if force_translation {
                attachment.mime = data::TEXT_MIME.to_string();
            } else {
                return Err(ServerError::bad_request(
                    "unable to determine mime (low confidence)",
                ));
            }
            Ok(attachment)
        }
    }
}

fn content_from_attachment(
    attachment: &data::DataAttachment,
    output: &attachments::AttachmentTranslation,
    force_translation: bool,
    tmp_dir: PathBuf,
) -> Result<ServerContent, ServerError> {
    if is_text_mime(&output.mime) {
        let original = decode_text(&attachment.bytes, force_translation)
            .map_err(|err| ServerError::bad_request(err.to_string()))?;
        let translated = decode_text(&output.bytes, force_translation)
            .map_err(|err| ServerError::bad_request(err.to_string()))?;
        return Ok(ServerContent {
            mime: output.mime.clone(),
            format: "raw".to_string(),
            original: Some(original),
            translated,
            correction: None,
        });
    }

    let translated_path =
        write_temp_file(&output.bytes, &output.mime, &tmp_dir).map_err(ServerError::from)?;
    Ok(ServerContent {
        mime: output.mime.clone(),
        format: "path".to_string(),
        original: None,
        translated: translated_path,
        correction: None,
    })
}

async fn translate_correction<P: providers::Provider + Clone>(
    translator: &Translator<P>,
    options: &TranslateOptions,
    text: String,
) -> Result<ServerResponse, ServerError> {
    let output = correction::exec_correction(translator, text.as_str(), options)
        .await
        .map_err(ServerError::from)?;
    let correction::CorrectionResult {
        corrected,
        markers,
        reasons,
        source_language,
    } = output.result;
    let content = ServerContent {
        mime: data::TEXT_MIME.to_string(),
        format: "raw".to_string(),
        original: Some(text),
        translated: corrected,
        correction: Some(CorrectionPayload {
            markers,
            reasons,
            source_language,
        }),
    };
    Ok(ServerResponse {
        contents: vec![content],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::LanguageRegistry;
    use crate::providers::{Provider, ProviderFuture, ProviderResponse, ToolSpec};
    use crate::settings::Settings;
    use serde_json::json;

    #[derive(Clone)]
    struct TestProvider {
        response: serde_json::Value,
    }

    impl Provider for TestProvider {
        fn append_system_input(self, _input: String) -> Self {
            self
        }

        fn append_user_input(self, _input: String) -> Self {
            self
        }

        fn append_user_data(self, _data: data::DataAttachment) -> Self {
            self
        }

        fn register_tool(self, _tool: ToolSpec) -> Self {
            self
        }

        fn call_tool(self, _tool_name: &str) -> ProviderFuture {
            let args = self.response;
            Box::pin(async move {
                Ok(ProviderResponse {
                    args,
                    model: Some("test".to_string()),
                    usage: None,
                })
            })
        }
    }

    fn build_translator(response: serde_json::Value) -> Translator<TestProvider> {
        let provider = TestProvider { response };
        let registry = LanguageRegistry::load().expect("registry");
        let mut settings = Settings::default();
        settings
            .formally
            .insert("formal".to_string(), "Use formal style.".to_string());
        Translator::new(provider, settings, registry)
    }

    #[tokio::test]
    async fn correction_response_contains_structured_data() {
        let response = json!({
            "corrected": "This is a pen",
            "markers": "        -",
            "reasons": ["English requires a/an before a countable noun"],
            "source_language": "en"
        });
        let translator = build_translator(response);
        let options = TranslateOptions {
            lang: "en".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let output = translate_correction(&translator, &options, "This is pen".to_string())
            .await
            .expect("correction response");
        assert_eq!(output.contents.len(), 1);
        let content = &output.contents[0];
        assert_eq!(content.mime, data::TEXT_MIME);
        assert_eq!(content.format, "raw");
        assert_eq!(content.original.as_deref(), Some("This is pen"));
        assert_eq!(content.translated, "This is a pen");
        let correction = content.correction.as_ref().expect("correction");
        assert_eq!(correction.markers.as_deref(), Some("        -"));
        assert_eq!(
            correction.reasons,
            vec!["English requires a/an before a countable noun"]
        );
        assert_eq!(correction.source_language, "en");

        let value = serde_json::to_value(&output).expect("serialize");
        assert!(value["contents"][0]["correction"].is_object());
    }
}
