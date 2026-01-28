mod engine;
mod font;
mod render;

pub use engine::{extract_lines, list_tesseract_languages};
pub use font::{load_font_metrics, resolve_overlay_font, FontMetrics, ResolvedOverlayFont};
pub use render::{render_bbox_svg, render_svg, render_svg_bytes, RenderOutcome};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BBoxPx {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OcrLine {
    pub text: String,
    pub bbox: BBoxPx,
    pub conf: f32,
    pub font_size: f32,
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub width: u32,
    pub height: u32,
    pub lines: Vec<OcrLine>,
}

#[derive(Debug, Clone)]
pub struct TranslatedLine {
    pub text: String,
    pub bbox: BBoxPx,
    pub font_size: f32,
}

pub struct OverlayStyle {
    pub text_color: String,
    pub stroke_color: String,
    pub fill_color: String,
    pub font_size: Option<f32>,
    pub font_family: Option<String>,
    pub font_metrics: Option<FontMetrics>,
}
