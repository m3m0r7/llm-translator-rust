mod geom;
mod layout;
mod merge;
mod parse;
mod preprocess;
mod tesseract;
mod text;

use anyhow::{Context, Result};
use image::GenericImageView;
use std::io::Write;

use crate::ocr::OcrResult;

pub use tesseract::list_tesseract_languages;

pub(crate) use layout::{
    build_avoid_rects, choose_fixed_font_size, fit_text_to_box, has_avoid_below, resolve_overlap,
    PlacedRect, ResolveOverlapConfig,
};

pub fn extract_lines(image_bytes: &[u8], ocr_languages: &str) -> Result<OcrResult> {
    let image =
        image::load_from_memory(image_bytes).with_context(|| "failed to decode image for OCR")?;
    let (width, height) = image.dimensions();
    let scale = preprocess::ocr_scale(width);
    let languages = tesseract::normalize_ocr_languages(ocr_languages)?;

    let mut lines = Vec::new();
    let variants = preprocess::preprocess_for_ocr_variants(image, scale);
    for (variant_idx, ocr_image) in variants.into_iter().enumerate() {
        let mut tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .with_context(|| "failed to create temp file for OCR")?;
        ocr_image
            .write_to(&mut tmp, image::ImageFormat::Png)
            .with_context(|| "failed to write temp image for OCR")?;
        tmp.flush().ok();

        let psm_list: &[u32] = if variant_idx == 0 { &[6, 4] } else { &[4] };
        for psm in psm_list {
            let hocr = tesseract::run_tesseract_hocr(tmp.path(), &languages, *psm)?;
            let mut parsed = parse::parse_hocr_lines(&hocr)?;
            if parsed.is_empty() {
                let tsv = tesseract::run_tesseract_tsv(tmp.path(), &languages, *psm)?;
                parsed = parse::parse_tsv_lines(&tsv)?;
            }
            lines = merge::merge_lines(lines, parsed);
        }
    }
    if scale > 1 {
        lines = merge::scale_lines(lines, scale as f32);
    }
    lines = merge::filter_lines(lines, width, height);
    lines = merge::merge_inline_lines(lines);
    lines = merge::suppress_overlaps(lines);

    Ok(OcrResult {
        width,
        height,
        lines,
    })
}
