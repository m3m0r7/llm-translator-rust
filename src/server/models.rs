use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ServerRequest {
    pub(crate) text: Option<String>,
    pub(crate) data: Option<String>,
    pub(crate) data_mime: Option<String>,
    pub(crate) data_base64: Option<String>,
    pub(crate) data_name: Option<String>,
    pub(crate) lang: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) key: Option<String>,
    pub(crate) formal: Option<String>,
    pub(crate) source_lang: Option<String>,
    pub(crate) slang: Option<bool>,
    pub(crate) with_commentout: Option<bool>,
    pub(crate) debug_ocr: Option<bool>,
    pub(crate) force_translation: Option<bool>,
    pub(crate) directory_translation_threads: Option<usize>,
    pub(crate) ignore_translation_files: Option<Vec<String>>,
    pub(crate) whisper_model: Option<String>,
    pub(crate) correction: Option<bool>,
    pub(crate) response_format: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerResponse {
    pub(crate) contents: Vec<ServerContent>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerContent {
    pub(crate) mime: String,
    pub(crate) format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original: Option<String>,
    pub(crate) translated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) correction: Option<CorrectionPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CorrectionPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) markers: Option<String>,
    pub(crate) reasons: Vec<String>,
    pub(crate) source_language: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorResponse {
    pub(crate) error: String,
}
