use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::data;
use crate::ocr;
use crate::providers::{Provider, ToolSpec};
use crate::translations;
use crate::{TranslateOptions, Translator};

use crate::attachments::cache::TranslationCache;
use crate::attachments::util::{
    collapse_whitespace, is_cjk, is_hangul, sanitize_ocr_text, should_skip_ocr_annotation,
};

pub(crate) mod debug;
pub(crate) use debug::{OcrDebugConfig, build_ocr_debug_config};

const OCR_NORMALIZE_TOOL: &str = "normalize_ocr";
const OCR_ROMANIZE_TOOL: &str = "romanize_ocr";

#[derive(Debug, Serialize)]
struct OcrNormalizeLineInput {
    id: usize,
    text: String,
    bbox: BBoxNorm,
}

#[derive(Debug, Serialize)]
struct BBoxNorm {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Deserialize)]
struct OcrNormalizeArgs {
    image_kind: String,
    lines: Vec<OcrNormalizeLineOutput>,
}

#[derive(Debug, Deserialize)]
struct OcrNormalizeLineOutput {
    id: usize,
    normalized: String,
    #[serde(default)]
    reading: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OcrRomanizeArgs {
    lines: Vec<OcrRomanizeLineOutput>,
}

#[derive(Debug, Deserialize)]
struct OcrRomanizeLineOutput {
    id: usize,
    romanized: String,
}

pub(crate) struct OcrNormalizeOutcome {
    pub(crate) lines: Vec<ocr::OcrLine>,
    #[allow(dead_code)]
    pub(crate) image_kind: Option<String>,
    pub(crate) readings: HashMap<usize, String>,
}

pub(crate) struct OcrNormalizeRequest<'a> {
    pub(crate) image_bytes: &'a [u8],
    pub(crate) image_mime: &'a str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) lines: &'a [ocr::OcrLine],
}

fn ocr_normalize_tool_spec() -> ToolSpec {
    let spec = serde_json::json!({
        "type": "object",
        "properties": {
            "image_kind": {"type": "string"},
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "normalized": {"type": "string"},
                        "reading": {"type": "string"}
                    },
                    "required": ["id", "normalized", "reading"]
                }
            }
        },
        "required": ["image_kind", "lines"]
    });
    ToolSpec {
        name: OCR_NORMALIZE_TOOL.to_string(),
        description: "Return normalized OCR text per line with image kind.".to_string(),
        parameters: spec,
    }
}

fn ocr_romanize_tool_spec() -> ToolSpec {
    let spec = serde_json::json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "romanized": {"type": "string"}
                    },
                    "required": ["id", "romanized"]
                }
            }
        },
        "required": ["lines"]
    });
    ToolSpec {
        name: OCR_ROMANIZE_TOOL.to_string(),
        description: "Return Latin-script readings for OCR lines.".to_string(),
        parameters: spec,
    }
}

pub(crate) fn is_latin_reading(value: &str) -> bool {
    let mut has_alpha = false;
    for ch in value.chars() {
        if is_cjk(ch) || is_hangul(ch) || (!ch.is_ascii() && ch.is_alphabetic()) {
            return false;
        }
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
        }
    }
    has_alpha
}

pub(crate) fn contains_non_latin_script(value: &str) -> bool {
    value
        .chars()
        .any(|ch| is_cjk(ch) || is_hangul(ch) || (!ch.is_ascii() && ch.is_alphabetic()))
}

pub(crate) async fn romanize_lines_with_llm<P: Provider + Clone>(
    lines: &[(usize, String)],
    translator: &Translator<P>,
    options: &TranslateOptions,
    cache: &mut TranslationCache,
) -> Result<HashMap<usize, String>> {
    if lines.is_empty() {
        return Ok(HashMap::new());
    }
    let payload = serde_json::json!({
        "lines": lines.iter().map(|(id, text)| serde_json::json!({"id": id, "text": text})).collect::<Vec<_>>()
    });
    let user_input = format!("Lines (JSON):\n{}", serde_json::to_string_pretty(&payload)?);
    let system_prompt =
        translations::render_ocr_romanize_prompt(&options.source_lang, OCR_ROMANIZE_TOOL)?;
    let tool = ocr_romanize_tool_spec();
    let response = translator
        .call_tool_with_data(tool, system_prompt, user_input, None)
        .await?;
    cache.record_usage(response.model.clone(), response.usage.clone());

    let args: OcrRomanizeArgs = serde_json::from_value(response.args)?;
    let mut map = HashMap::new();
    for line in args.lines {
        let cleaned = collapse_whitespace(line.romanized.trim());
        if !cleaned.is_empty() {
            map.insert(line.id, cleaned);
        }
    }
    Ok(map)
}

pub(crate) async fn normalize_ocr_lines_with_llm<P: Provider + Clone>(
    request: OcrNormalizeRequest<'_>,
    translator: &Translator<P>,
    options: &TranslateOptions,
    cache: &mut TranslationCache,
) -> Result<OcrNormalizeOutcome> {
    let mut input_lines = Vec::new();
    for (idx, line) in request.lines.iter().enumerate() {
        let cleaned = sanitize_ocr_text(&collapse_whitespace(&line.text));
        if should_skip_ocr_annotation(&cleaned) {
            continue;
        }
        let bbox = BBoxNorm {
            x: (line.bbox.x as f32 / request.width.max(1) as f32).clamp(0.0, 1.0),
            y: (line.bbox.y as f32 / request.height.max(1) as f32).clamp(0.0, 1.0),
            w: (line.bbox.w as f32 / request.width.max(1) as f32).clamp(0.0, 1.0),
            h: (line.bbox.h as f32 / request.height.max(1) as f32).clamp(0.0, 1.0),
        };
        input_lines.push(OcrNormalizeLineInput {
            id: idx,
            text: line.text.clone(),
            bbox,
        });
    }

    if input_lines.is_empty() {
        return Ok(OcrNormalizeOutcome {
            lines: request.lines.to_vec(),
            image_kind: None,
            readings: HashMap::new(),
        });
    }

    let payload = serde_json::json!({
        "image": {
            "mime": request.image_mime,
            "width": request.width,
            "height": request.height
        },
        "lines": input_lines
    });
    let user_input = format!(
        "OCR lines (JSON):\n{}",
        serde_json::to_string_pretty(&payload)?
    );
    let system_prompt =
        translations::render_ocr_normalize_prompt(&options.source_lang, OCR_NORMALIZE_TOOL)?;
    let tool = ocr_normalize_tool_spec();
    let data = data::DataAttachment {
        bytes: request.image_bytes.to_vec(),
        mime: request.image_mime.to_string(),
        name: None,
    };
    let response = translator
        .call_tool_with_data(tool, system_prompt, user_input, Some(data))
        .await?;
    cache.record_usage(response.model.clone(), response.usage.clone());

    let args: OcrNormalizeArgs = serde_json::from_value(response.args)?;
    let mut normalized_map = HashMap::new();
    let mut readings = HashMap::new();
    for line in args.lines {
        let cleaned = collapse_whitespace(line.normalized.trim());
        if !cleaned.is_empty() {
            normalized_map.insert(line.id, cleaned);
        }
        if let Some(reading) = line.reading {
            let reading = collapse_whitespace(reading.trim());
            if !reading.is_empty() {
                readings.insert(line.id, reading);
            }
        }
    }

    let mut normalized_lines = request.lines.to_vec();
    for (idx, line) in normalized_lines.iter_mut().enumerate() {
        if let Some(text) = normalized_map.get(&idx) {
            line.text = text.clone();
        }
    }

    Ok(OcrNormalizeOutcome {
        lines: normalized_lines,
        image_kind: Some(args.image_kind),
        readings,
    })
}
