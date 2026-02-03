use anyhow::{Context, Result};
use serde::Deserialize;

use crate::data;
use crate::providers::Provider;
use crate::translations::{self, MIME_TOOL_NAME};
use crate::Translator;

#[derive(Debug, Clone)]
pub struct MimeDetection {
    pub mime: String,
    pub confident: bool,
    pub model: Option<String>,
    pub usage: Option<crate::providers::ProviderUsage>,
}

#[derive(Debug, Deserialize)]
struct MimeToolArgs {
    mime: String,
    confident: bool,
}

pub async fn detect_mime_with_llm<P: Provider + Clone>(
    data: &data::DataAttachment,
    translator: &Translator<P>,
) -> Result<MimeDetection> {
    let tool = translations::mime_tool_spec(MIME_TOOL_NAME);
    let prompt =
        translations::render_mime_prompt(MIME_TOOL_NAME, data.name.as_deref(), &mime_list())?;
    let response = translator
        .call_tool_with_data(
            tool,
            prompt,
            "Detect the MIME type of the attached file.".to_string(),
            Some(data.clone()),
        )
        .await?;
    let args: MimeToolArgs =
        serde_json::from_value(response.args).with_context(|| "failed to parse mime tool args")?;
    let mime = args
        .mime
        .split(';')
        .next()
        .unwrap_or(args.mime.as_str())
        .trim()
        .to_lowercase();
    let mime = if mime.is_empty() {
        data::OCTET_STREAM_MIME.to_string()
    } else {
        mime
    };
    Ok(MimeDetection {
        mime,
        confident: args.confident,
        model: response.model,
        usage: response.usage,
    })
}

fn mime_list() -> Vec<&'static str> {
    vec![
        "image/*",
        "audio/*",
        data::PDF_MIME,
        data::DOC_MIME,
        data::DOCX_MIME,
        data::PPTX_MIME,
        data::XLSX_MIME,
        data::TEXT_MIME,
        data::MARKDOWN_MIME,
        data::HTML_MIME,
        data::JSON_MIME,
        data::YAML_MIME,
        data::PO_MIME,
        data::XML_MIME,
        data::JS_MIME,
        data::TS_MIME,
        data::TSX_MIME,
        data::MERMAID_MIME,
    ]
}
