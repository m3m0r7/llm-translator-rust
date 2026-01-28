use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::GenericImageView;
use resvg::render;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tiny_skia::Pixmap;
use ttf_parser::name_id;
use ttf_parser::Face;
use usvg::{fontdb, Options, Tree};

#[derive(Debug, Clone, Serialize)]
pub struct BBoxPx {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize)]
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

pub fn extract_lines(image_bytes: &[u8], ocr_languages: &str) -> Result<OcrResult> {
    let image =
        image::load_from_memory(image_bytes).with_context(|| "failed to decode image for OCR")?;
    let (width, height) = image.dimensions();
    let scale = ocr_scale(width);
    let languages = normalize_ocr_languages(ocr_languages)?;

    let mut lines = Vec::new();
    let variants = preprocess_for_ocr_variants(image, scale);
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
            let hocr = run_tesseract_hocr(tmp.path(), &languages, *psm)?;
            let mut parsed = parse_hocr_lines(&hocr)?;
            if parsed.is_empty() {
                let tsv = run_tesseract_tsv(tmp.path(), &languages, *psm)?;
                parsed = parse_tsv_lines(&tsv)?;
            }
            lines = merge_lines(lines, parsed);
        }
    }
    if scale > 1 {
        lines = scale_lines(lines, scale as f32);
    }
    lines = filter_lines(lines, width, height);
    lines = merge_inline_lines(lines);
    lines = suppress_overlaps(lines);

    Ok(OcrResult {
        width,
        height,
        lines,
    })
}

pub struct OverlayStyle {
    pub text_color: String,
    pub stroke_color: String,
    pub fill_color: String,
    pub font_size: Option<f32>,
    pub font_family: Option<String>,
    pub font_metrics: Option<FontMetrics>,
}

#[derive(Clone)]
pub struct FontMetrics {
    data: Arc<Vec<u8>>,
    units_per_em: u16,
    space_advance: u16,
    family: Option<String>,
    face_index: u32,
}

impl FontMetrics {
    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    pub fn data(&self) -> &[u8] {
        self.data.as_ref()
    }
}

pub fn load_font_metrics(path: &Path) -> Result<FontMetrics> {
    let data =
        std::fs::read(path).with_context(|| format!("failed to read font: {}", path.display()))?;
    load_font_metrics_from_data(&data, None)
        .map_err(|err| anyhow!("failed to parse font: {} ({})", path.display(), err))
}

pub struct ResolvedOverlayFont {
    pub metrics: FontMetrics,
    pub family: String,
}

pub fn resolve_overlay_font(
    font_path: Option<&Path>,
    font_family: Option<&str>,
    fallback: &[&str],
) -> Result<ResolvedOverlayFont> {
    if let Some(path) = font_path {
        let metrics = load_font_metrics(path)?;
        let family = metrics
            .family()
            .map(|name| name.to_string())
            .or_else(|| font_family.map(|name| name.to_string()))
            .unwrap_or_else(|| "sans-serif".to_string());
        return Ok(ResolvedOverlayFont { metrics, family });
    }

    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    if let Some(family) = font_family {
        return load_font_metrics_from_family(&db, family);
    }

    for candidate in fallback {
        if let Ok(resolved) = load_font_metrics_from_family(&db, candidate) {
            return Ok(resolved);
        }
    }

    Err(anyhow!("no fallback fonts found"))
}

pub struct RenderOutcome {
    pub svg: String,
    pub placed: Vec<bool>,
}

pub fn render_svg(
    image_bytes: &[u8],
    image_mime: &str,
    width: u32,
    height: u32,
    lines: &[TranslatedLine],
    style: &OverlayStyle,
    footer_lines: Option<&[String]>,
) -> Result<RenderOutcome> {
    let encoded = BASE64.encode(image_bytes);
    let data_uri = format!("data:{};base64,{}", image_mime, encoded);

    let footer_font_size = style.font_size.unwrap_or(14.0).max(10.0);
    let footer_padding = (footer_font_size * 0.7).clamp(6.0, 16.0);
    let footer_inner_w = (width as f32 - footer_padding * 2.0).max(40.0);
    let mut footer_wrapped: Vec<String> = Vec::new();
    let mut footer_height = 0.0;
    if let Some(lines) = footer_lines {
        for line in lines {
            let (font_size, mut wrapped, line_height) =
                fit_text_to_box(line, footer_font_size, footer_inner_w, 10_000.0, false);
            if wrapped.is_empty() {
                wrapped.push(line.clone());
            }
            let wrap_count = wrapped.len() as f32;
            for wline in wrapped {
                footer_wrapped.push(wline);
            }
            let line_height = if line_height > 0.0 {
                line_height
            } else {
                font_size * 1.1
            };
            footer_height += line_height * wrap_count;
        }
        if !footer_wrapped.is_empty() {
            footer_height += footer_padding * 2.0;
        }
    }

    let canvas_height = height as f32 + footer_height;
    let layout_height = height as f32;

    let mut svg = String::new();
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#,
        w = width,
        h = canvas_height
    ));
    svg.push_str(&format!(
        r#"<image href="{uri}" xlink:href="{uri}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="none"/>"#,
        uri = data_uri,
        w = width,
        h = layout_height
    ));

    let fixed_font_size = style
        .font_size
        .filter(|size| *size > 0.0)
        .map(|size| size.clamp(10.0, layout_height * 0.2))
        .unwrap_or_else(|| choose_fixed_font_size(lines, height));
    let mut placed = Vec::new();
    let avoid = build_avoid_rects(lines);
    let mut placed_flags = Vec::with_capacity(lines.len());

    for (idx, line) in lines.iter().enumerate() {
        let BBoxPx { x, y, w, h } = line.bbox;
        let src_w = (w as f32).max(1.0);
        let src_h = (h as f32).max(1.0);
        let margin = (src_h * 0.2).clamp(3.0, 12.0);
        let max_width = (width as f32 * 0.9)
            .max(src_w * 1.4)
            .min(width as f32 * 0.98);
        let padding = (src_h * 0.22).clamp(4.0, 10.0);
        let inner_w = (max_width - padding * 2.0).max(40.0);
        let inner_h = (layout_height * 0.5 - padding * 2.0).max(20.0);

        let is_label = line
            .text
            .chars()
            .all(|ch| ch.is_ascii_digit());
        let (font_size, mut lines_text, line_height) = if is_label {
            let font_size = fixed_font_size;
            let line_height = font_size * 1.2;
            (font_size, vec![line.text.clone()], line_height)
        } else {
            fit_text_to_box(&line.text, fixed_font_size, inner_w, inner_h, false)
        };
        if lines_text.is_empty() {
            lines_text.push(line.text.trim().to_string());
        }
        let mut block_height = lines_text.len() as f32 * line_height;
        let max_height_cap = (layout_height * 0.5).max(src_h * 2.2);
        if block_height + padding * 2.0 > max_height_cap {
            let max_fit_lines = ((max_height_cap - padding * 2.0) / line_height)
                .floor()
                .max(1.0) as usize;
            if lines_text.len() > max_fit_lines {
                lines_text.truncate(max_fit_lines);
            }
            block_height = lines_text.len() as f32 * line_height;
        }
        let box_h = (block_height + padding * 2.0).min(layout_height);

        let max_line_width = lines_text
            .iter()
            .map(|line| measure_text_width_px(line, font_size, style.font_metrics.as_ref()))
            .fold(1.0, f32::max);
        let mut box_w = max_line_width + padding * 2.0;
        box_w = box_w.clamp(40.0, max_width);

        let max_y = (layout_height - box_h).max(0.0);
        let center_x = x as f32 + src_w * 0.5;
        let mut rect_x = center_x - box_w * 0.5;
        rect_x = rect_x.clamp(0.0, (width as f32 - box_w).max(0.0));
        let anchor_y = (y as f32 + src_h) - (src_h * 0.2);
        let mut rect_y = anchor_y;
        rect_y = rect_y.clamp(0.0, max_y);
        let gap = (margin * 0.4).max(2.0);

        let mut base = PlacedRect {
            x: rect_x,
            y: rect_y,
            w: box_w,
            h: box_h,
        };
        let anchor = PlacedRect {
            x: x as f32,
            y: y as f32,
            w: w as f32,
            h: h as f32,
        };
        let prefer_side = has_avoid_below(&avoid, &anchor, gap);
        if prefer_side {
            let right_x = anchor.x + anchor.w + gap;
            let left_x = anchor.x - box_w - gap;
            if right_x + box_w <= width as f32 {
                base.x = right_x;
            } else if left_x >= 0.0 {
                base.x = left_x;
            }
            base.x = base.x.clamp(0.0, (width as f32 - box_w).max(0.0));
        }
        let resolve_config = ResolveOverlapConfig {
            anchor,
            anchor_overlap_ratio: 0.2,
            prefer_side,
            direction: 1.0,
            bounds_w: width as f32,
            bounds_h: layout_height,
            gap,
        };
        let placed_rect = resolve_overlap(base, &placed, &avoid, &resolve_config);
        let placed_ok = placed_rect.is_some();
        let placed_rect = placed_rect.unwrap_or(base);
        placed.push(placed_rect);
        placed_flags.push(placed_ok);

        let rect_x = placed_rect.x;
        let rect_y = placed_rect.y;
        let text_y = rect_y + padding + font_size;
        let text_color = style.text_color.as_str();
        let font_family = style
            .font_family
            .as_ref()
            .or_else(|| style.font_metrics.as_ref().and_then(|m| m.family.as_ref()));

        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}" stroke="{stroke}" stroke-width="2"/>"##,
            x = rect_x,
            y = rect_y,
            w = box_w,
            h = box_h,
            fill = &style.fill_color,
            stroke = &style.stroke_color
        ));
        let clip_id = format!("clip-{}", idx);
        svg.push_str(&format!(
            r#"<clipPath id="{id}"><rect x="{x}" y="{y}" width="{w}" height="{h}"/></clipPath>"#,
            id = clip_id,
            x = rect_x,
            y = rect_y,
            w = box_w,
            h = box_h
        ));
        let mut text_block = String::new();
        if let Some(family) = font_family {
            text_block.push_str(&format!(
                r#"<text x="{x}" y="{y}" font-size="{size}" fill="{color}" font-family="{family}" clip-path="url(#{clip})">"#,
                x = rect_x + padding,
                y = text_y,
                size = font_size,
                color = text_color,
                family = escape_xml(family),
                clip = clip_id
            ));
        } else {
            text_block.push_str(&format!(
                r#"<text x="{x}" y="{y}" font-size="{size}" fill="{color}" clip-path="url(#{clip})">"#,
                x = rect_x + padding,
                y = text_y,
                size = font_size,
                color = text_color,
                clip = clip_id
            ));
        }
        for (idx, line_text) in lines_text.iter().enumerate() {
            let escaped = escape_xml(line_text);
            if idx == 0 {
                text_block.push_str(&format!(
                    r#"<tspan x="{x}" dy="0">{text}</tspan>"#,
                    x = rect_x + padding,
                    text = escaped
                ));
            } else {
                text_block.push_str(&format!(
                    r#"<tspan x="{x}" dy="{dy}">{text}</tspan>"#,
                    x = rect_x + padding,
                    dy = line_height,
                    text = escaped
                ));
            }
        }
        text_block.push_str("</text>");
        svg.push_str(&text_block);
    }

    if !footer_wrapped.is_empty() {
        let footer_y = layout_height;
        svg.push_str(&format!(
            r#"<rect x="0" y="{y}" width="{w}" height="{h}" fill="{fill}"/>"#,
            y = footer_y,
            w = width,
            h = footer_height,
            fill = &style.fill_color
        ));
        let font_family = style
            .font_family
            .as_ref()
            .or_else(|| style.font_metrics.as_ref().and_then(|m| m.family.as_ref()));
        let mut text_y = footer_y + footer_padding + footer_font_size;
        for line in footer_wrapped {
            let escaped = escape_xml(&line);
            if let Some(family) = font_family {
                svg.push_str(&format!(
                    r#"<text x="{x}" y="{y}" font-size="{size}" fill="{color}" font-family="{family}">{text}</text>"#,
                    x = footer_padding,
                    y = text_y,
                    size = footer_font_size,
                    color = &style.text_color,
                    family = escape_xml(family),
                    text = escaped
                ));
            } else {
                svg.push_str(&format!(
                    r#"<text x="{x}" y="{y}" font-size="{size}" fill="{color}">{text}</text>"#,
                    x = footer_padding,
                    y = text_y,
                    size = footer_font_size,
                    color = &style.text_color,
                    text = escaped
                ));
            }
            text_y += footer_font_size * 1.1;
        }
    }

    svg.push_str("</svg>");
    Ok(RenderOutcome {
        svg,
        placed: placed_flags,
    })
}

pub fn render_bbox_svg(
    image_bytes: &[u8],
    image_mime: &str,
    width: u32,
    height: u32,
    lines: &[OcrLine],
) -> Result<String> {
    let encoded = BASE64.encode(image_bytes);
    let data_uri = format!("data:{};base64,{}", image_mime, encoded);

    let mut svg = String::new();
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#,
        w = width,
        h = height
    ));
    svg.push_str(&format!(
        r#"<image href="{uri}" xlink:href="{uri}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="none"/>"#,
        uri = data_uri,
        w = width,
        h = height
    ));

    for line in lines {
        let BBoxPx { x, y, w, h } = line.bbox;
        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="#00c853" stroke-width="2"/>"##,
            x = x,
            y = y,
            w = w,
            h = h
        ));
    }

    svg.push_str("</svg>");
    Ok(svg)
}

pub fn render_svg_bytes(svg: &str, output_mime: &str, font_data: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    if let Some(data) = font_data {
        db.load_font_data(data.to_vec());
    }
    let options = Options {
        fontdb: Arc::new(db),
        ..Options::default()
    };
    let tree = Tree::from_str(svg, &options).with_context(|| "failed to parse SVG")?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        Pixmap::new(size.width(), size.height()).ok_or_else(|| anyhow!("empty SVG size"))?;
    let mut pixmap_mut = pixmap.as_mut();
    render(&tree, tiny_skia::Transform::identity(), &mut pixmap_mut);
    let image = image::RgbaImage::from_raw(size.width(), size.height(), pixmap.data().to_vec())
        .ok_or_else(|| anyhow!("failed to build image buffer from SVG"))?;
    let format = image_format_from_mime(output_mime)
        .ok_or_else(|| anyhow!("unsupported output image mime '{}'", output_mime))?;
    let mut bytes = Vec::new();
    let mut cursor = Cursor::new(&mut bytes);
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, format)
        .with_context(|| "failed to encode image from SVG")?;
    Ok(bytes)
}

fn parse_tsv_lines(tsv: &str) -> Result<Vec<OcrLine>> {
    let mut word_map: HashMap<(i32, i32, i32, i32), Vec<WordToken>> = HashMap::new();

    for (idx, row) in tsv.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let cols = row.split('\t').collect::<Vec<_>>();
        if cols.len() < 12 {
            continue;
        }
        let level: i32 = cols[0].parse().unwrap_or(0);
        if level != 5 {
            continue;
        }
        let page_num: i32 = cols[1].parse().unwrap_or(0);
        let block_num: i32 = cols[2].parse().unwrap_or(0);
        let par_num: i32 = cols[3].parse().unwrap_or(0);
        let line_num: i32 = cols[4].parse().unwrap_or(0);
        let left: u32 = cols[6].parse().unwrap_or(0);
        let top: u32 = cols[7].parse().unwrap_or(0);
        let width: u32 = cols[8].parse().unwrap_or(0);
        let height: u32 = cols[9].parse().unwrap_or(0);
        let conf: f32 = cols[10].parse().unwrap_or(-1.0);
        let text = cols[11].trim();
        if text.is_empty() || conf < 0.0 {
            continue;
        }

        let word_bbox = BBoxPx {
            x: left,
            y: top,
            w: width,
            h: height,
        };
        let len = text.chars().count().max(1);
        let key = (page_num, block_num, par_num, line_num);
        word_map.entry(key).or_default().push(WordToken {
            text: text.to_string(),
            bbox: word_bbox,
            conf,
            len,
        });
    }

    let mut lines = Vec::new();
    for (_, mut words) in word_map {
        words.sort_by_key(|word| word.bbox.x);
        for segment in split_word_segments(words) {
            if let Some(line) = build_line(&segment) {
                lines.push(line);
            }
        }
    }

    Ok(lines)
}

fn parse_hocr_lines(hocr: &str) -> Result<Vec<OcrLine>> {
    let mut lines = Vec::new();
    let bytes = hocr.as_bytes();
    let mut i = 0usize;
    while let Some(start) = find_subslice(bytes, b"<span", i) {
        let tag_end = match find_byte(bytes, b'>', start) {
            Some(end) => end,
            None => break,
        };
        let tag = &hocr[start..tag_end];
        if !tag.contains("ocr_line") {
            i = tag_end + 1;
            continue;
        }
        let (inner_start, inner_end) = match find_span_inner(bytes, tag_end + 1) {
            Some(value) => value,
            None => break,
        };
        let inner = &hocr[inner_start..inner_end];
        let mut words = parse_hocr_words(inner);
        if words.is_empty() {
            i = inner_end + "</span>".len();
            continue;
        }
        words.sort_by_key(|word| word.bbox.x);
        for segment in split_word_segments(words) {
            if let Some(line) = build_line(&segment) {
                lines.push(line);
            }
        }
        i = inner_end + "</span>".len();
    }
    Ok(lines)
}

fn image_format_from_mime(mime: &str) -> Option<image::ImageFormat> {
    match mime {
        "image/png" => Some(image::ImageFormat::Png),
        "image/jpeg" => Some(image::ImageFormat::Jpeg),
        "image/jpg" => Some(image::ImageFormat::Jpeg),
        "image/gif" => Some(image::ImageFormat::Gif),
        "image/webp" => Some(image::ImageFormat::WebP),
        "image/bmp" => Some(image::ImageFormat::Bmp),
        "image/tiff" => Some(image::ImageFormat::Tiff),
        _ => None,
    }
}

pub fn list_tesseract_languages() -> Result<Vec<String>> {
    let output = Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .with_context(|| "failed to run tesseract --list-langs")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("tesseract --list-langs failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut langs = Vec::new();
    for (idx, line) in stdout.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let value = line.trim();
        if !value.is_empty() {
            langs.push(value.to_string());
        }
    }
    Ok(langs)
}

fn run_tesseract_tsv(path: &std::path::Path, languages: &str, psm: u32) -> Result<String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg(languages)
        .arg("--oem")
        .arg("1")
        .arg("--psm")
        .arg(psm.to_string())
        .arg("--dpi")
        .arg("300")
        .arg("tsv")
        .output()
        .with_context(|| "failed to run tesseract (is it installed?)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("tesseract failed: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_tesseract_hocr(path: &std::path::Path, languages: &str, psm: u32) -> Result<String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg(languages)
        .arg("--oem")
        .arg("1")
        .arg("--psm")
        .arg(psm.to_string())
        .arg("--dpi")
        .arg("300")
        .arg("hocr")
        .output()
        .with_context(|| "failed to run tesseract (is it installed?)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("tesseract failed: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn merge_lines(mut base: Vec<OcrLine>, extra: Vec<OcrLine>) -> Vec<OcrLine> {
    for line in extra {
        if let Some(idx) = base
            .iter()
            .position(|existing| iou(&existing.bbox, &line.bbox) > 0.6)
        {
            let line_len = line.text.chars().count();
            let base_len = base[idx].text.chars().count();
            let line_cjk = cjk_ratio(&line.text);
            let base_cjk = cjk_ratio(&base[idx].text);
            let prefer = (line.conf > base[idx].conf + 5.0)
                || (line_len > base_len && line_cjk + 0.05 >= base_cjk);
            if prefer || (base_len <= 2 && line_len >= 4) {
                base[idx] = line;
            }
        } else {
            base.push(line);
        }
    }
    base
}

fn iou(a: &BBoxPx, b: &BBoxPx) -> f32 {
    let ax2 = a.x + a.w;
    let ay2 = a.y + a.h;
    let bx2 = b.x + b.w;
    let by2 = b.y + b.h;

    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    if ix2 <= ix1 || iy2 <= iy1 {
        return 0.0;
    }
    let inter = (ix2 - ix1) as f32 * (iy2 - iy1) as f32;
    let area_a = (a.w as f32) * (a.h as f32);
    let area_b = (b.w as f32) * (b.h as f32);
    inter / (area_a + area_b - inter).max(1.0)
}

fn scale_lines(lines: Vec<OcrLine>, scale: f32) -> Vec<OcrLine> {
    lines
        .into_iter()
        .map(|line| {
            let bbox = BBoxPx {
                x: ((line.bbox.x as f32) / scale).round() as u32,
                y: ((line.bbox.y as f32) / scale).round() as u32,
                w: ((line.bbox.w as f32) / scale).round() as u32,
                h: ((line.bbox.h as f32) / scale).round() as u32,
            };
            OcrLine {
                text: line.text,
                bbox,
                conf: line.conf,
                font_size: line.font_size / scale,
            }
        })
        .collect()
}

fn preprocess_for_ocr_variants(image: image::DynamicImage, scale: u32) -> Vec<image::DynamicImage> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut luma = image::GrayImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as f32 / 255.0;
        let r = (r as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let g = (g as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let b = (b as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let value = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
        luma.put_pixel(x, y, image::Luma([value]));
    }

    let resized = if scale > 1 {
        image::imageops::resize(
            &luma,
            width.saturating_mul(scale),
            height.saturating_mul(scale),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        luma
    };

    let stretched = contrast_stretch(&resized);
    let threshold = (0.65 * 255.0) as u8;
    let bin = binarize(&stretched, threshold);
    vec![
        image::DynamicImage::ImageLuma8(bin),
        image::DynamicImage::ImageLuma8(stretched),
    ]
}

fn contrast_stretch(image: &image::GrayImage) -> image::GrayImage {
    let mut min = 255u8;
    let mut max = 0u8;
    for pixel in image.pixels() {
        let value = pixel[0];
        min = min.min(value);
        max = max.max(value);
    }

    if max <= min {
        return image.clone();
    }

    let scale = 255.0 / (max as f32 - min as f32);
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        let value = pixel[0];
        let stretched = ((value.saturating_sub(min)) as f32 * scale).round() as u8;
        pixel[0] = stretched;
    }
    output
}

fn binarize(image: &image::GrayImage, threshold: u8) -> image::GrayImage {
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        pixel[0] = if pixel[0] > threshold { 255 } else { 0 };
    }
    output
}

fn ocr_scale(width: u32) -> u32 {
    let max_width = 6000u32;
    let mut scale = 3u32;
    while width.saturating_mul(scale) > max_width && scale > 1 {
        scale -= 1;
    }
    scale.max(1)
}

fn filter_lines(lines: Vec<OcrLine>, width: u32, height: u32) -> Vec<OcrLine> {
    lines
        .into_iter()
        .filter(|line| is_line_valid(line, width, height))
        .collect()
}

fn is_line_valid(line: &OcrLine, width: u32, height: u32) -> bool {
    let text = line.text.trim();
    if text.is_empty() {
        return false;
    }
    if line.bbox.w == 0 || line.bbox.h == 0 {
        return false;
    }
    if line.bbox.h as f32 > height as f32 * 0.25 {
        return false;
    }
    if line.bbox.w as f32 > width as f32 * 0.98 && (line.bbox.h as f32) < 6.0 {
        return false;
    }
    let aspect = line.bbox.w as f32 / line.bbox.h.max(1) as f32;
    if aspect < 0.35 && text.chars().count() > 3 {
        return false;
    }

    let stats = text_stats(text);
    if stats.total == 0 {
        return false;
    }
    let word_ratio = stats.word as f32 / stats.total as f32;
    let digit_ratio = stats.digits as f32 / stats.total as f32;
    let symbol_ratio = stats.symbols as f32 / stats.total as f32;
    if stats.total > 4 && word_ratio < 0.35 {
        return false;
    }
    if stats.total > 3 && digit_ratio > 0.85 {
        return false;
    }
    if stats.total > 3 && symbol_ratio > 0.6 {
        return false;
    }
    if line.conf < 25.0 && stats.total <= 4 {
        return false;
    }
    if line.conf < 70.0 {
        let ascii_ratio = stats.ascii as f32 / stats.total.max(1) as f32;
        if ascii_ratio > 0.4 {
            return false;
        }
    }
    true
}

struct TextStats {
    total: usize,
    word: usize,
    digits: usize,
    symbols: usize,
    ascii: usize,
}

fn text_stats(text: &str) -> TextStats {
    let mut stats = TextStats {
        total: 0,
        word: 0,
        digits: 0,
        symbols: 0,
        ascii: 0,
    };
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        stats.total += 1;
        let code = ch as u32;
        if ch.is_ascii_digit() {
            stats.digits += 1;
            stats.word += 1;
            stats.ascii += 1;
        } else if ch.is_ascii_alphabetic() || ch.is_alphabetic() {
            stats.word += 1;
            if ch.is_ascii() {
                stats.ascii += 1;
            }
        } else if matches!(code, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF) {
            stats.word += 1;
        } else {
            stats.symbols += 1;
        }
    }
    stats
}

fn merge_inline_lines(mut lines: Vec<OcrLine>) -> Vec<OcrLine> {
    lines.sort_by_key(|line| (line.bbox.y, line.bbox.x));
    let mut merged: Vec<OcrLine> = Vec::new();

    for line in lines {
        if let Some(last) = merged.last_mut() {
            let same_line = vertical_overlap_ratio(&last.bbox, &line.bbox) > 0.6;
            if same_line && should_merge_lines(last, &line) {
                last.text = join_inline(&last.text, &line.text);
                last.bbox = union_bbox(&last.bbox, &line.bbox);
                last.conf = merge_conf(last.conf, last.text.len(), line.conf, line.text.len());
                last.font_size = last.font_size.max(line.font_size);
                continue;
            }
        }
        merged.push(line);
    }

    merged
}

fn should_merge_lines(a: &OcrLine, b: &OcrLine) -> bool {
    let gap = b.bbox.y.saturating_sub(a.bbox.y + a.bbox.h);
    let max_gap = (a.bbox.h as f32 * 0.8).max(6.0) as u32;
    let overlap = horizontal_overlap_ratio(&a.bbox, &b.bbox);
    let close_x = (a.bbox.x as i32 - b.bbox.x as i32).abs() < 40;
    gap <= max_gap && (overlap > 0.2 || close_x)
}

fn suppress_overlaps(lines: Vec<OcrLine>) -> Vec<OcrLine> {
    let mut sorted = lines;
    sorted.sort_by(|a, b| {
        b.conf
            .partial_cmp(&a.conf)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<OcrLine> = Vec::new();

    'outer: for line in sorted {
        for existing in &kept {
            if iou(&existing.bbox, &line.bbox) > 0.5 {
                continue 'outer;
            }
            if vertical_overlap_ratio(&existing.bbox, &line.bbox) > 0.8
                && horizontal_overlap_ratio(&existing.bbox, &line.bbox) > 0.8
            {
                continue 'outer;
            }
        }
        kept.push(line);
    }
    kept.sort_by_key(|line| (line.bbox.y, line.bbox.x));
    kept
}

fn horizontal_overlap_ratio(a: &BBoxPx, b: &BBoxPx) -> f32 {
    let ax2 = a.x + a.w;
    let bx2 = b.x + b.w;
    let ix1 = a.x.max(b.x);
    let ix2 = ax2.min(bx2);
    if ix2 <= ix1 {
        return 0.0;
    }
    let inter = (ix2 - ix1) as f32;
    inter / (a.w.min(b.w) as f32).max(1.0)
}

fn vertical_overlap_ratio(a: &BBoxPx, b: &BBoxPx) -> f32 {
    let ay2 = a.y + a.h;
    let by2 = b.y + b.h;
    let iy1 = a.y.max(b.y);
    let iy2 = ay2.min(by2);
    if iy2 <= iy1 {
        return 0.0;
    }
    let inter = (iy2 - iy1) as f32;
    inter / (a.h.min(b.h) as f32).max(1.0)
}

fn union_bbox(a: &BBoxPx, b: &BBoxPx) -> BBoxPx {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.w).max(b.x + b.w);
    let y2 = (a.y + a.h).max(b.y + b.h);
    BBoxPx {
        x: x1,
        y: y1,
        w: x2 - x1,
        h: y2 - y1,
    }
}

struct WordToken {
    text: String,
    bbox: BBoxPx,
    conf: f32,
    len: usize,
}

fn split_word_segments(words: Vec<WordToken>) -> Vec<Vec<WordToken>> {
    if words.is_empty() {
        return Vec::new();
    }
    if words.len() == 1 {
        return vec![words];
    }

    let mut heights = words.iter().map(|word| word.bbox.h).collect::<Vec<_>>();
    heights.sort_unstable();
    let median_h = heights[heights.len() / 2].max(1) as f32;
    let gap_threshold = (median_h * 2.5).clamp(12.0, 120.0);
    let vertical_threshold = (median_h * 0.9).clamp(6.0, 80.0);

    let mut segments: Vec<Vec<WordToken>> = Vec::new();
    let mut current: Vec<WordToken> = Vec::new();
    let mut last_right = 0u32;
    let mut last_center_y = 0f32;
    for word in words {
        if current.is_empty() {
            last_right = word.bbox.x + word.bbox.w;
            last_center_y = word.bbox.y as f32 + word.bbox.h as f32 * 0.5;
            current.push(word);
            continue;
        }
        let gap = word.bbox.x.saturating_sub(last_right);
        let center_y = word.bbox.y as f32 + word.bbox.h as f32 * 0.5;
        let vertical_gap = (center_y - last_center_y).abs();
        if (gap as f32) > gap_threshold || vertical_gap > vertical_threshold {
            segments.push(current);
            current = vec![word];
            last_right = current[0].bbox.x + current[0].bbox.w;
            last_center_y = current[0].bbox.y as f32 + current[0].bbox.h as f32 * 0.5;
        } else {
            last_right = last_right.max(word.bbox.x + word.bbox.w);
            last_center_y = (last_center_y + (word.bbox.y as f32 + word.bbox.h as f32 * 0.5)) * 0.5;
            current.push(word);
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn build_line(words: &[WordToken]) -> Option<OcrLine> {
    if words.is_empty() {
        return None;
    }

    let mut text = String::new();
    let mut last_token = String::new();
    for word in words {
        if !text.is_empty() && needs_space(&last_token, &word.text) {
            text.push(' ');
        }
        text.push_str(&word.text);
        last_token = word.text.clone();
    }
    let final_text = text.trim();
    if final_text.is_empty() {
        return None;
    }

    let mut bbox_opt: Option<BBoxPx> = None;
    let mut conf_sum = 0.0;
    let mut len_sum = 0.0;
    let mut heights: Vec<u32> = Vec::new();
    for word in words {
        bbox_opt = Some(if let Some(bbox) = bbox_opt.take() {
            union_bbox(&bbox, &word.bbox)
        } else {
            word.bbox.clone()
        });
        let weight = word.len.max(1) as f32;
        conf_sum += word.conf * weight;
        len_sum += weight;
        heights.push(word.bbox.h);
    }
    let bbox = bbox_opt?;
    let avg_conf = if len_sum > 0.0 {
        conf_sum / len_sum
    } else {
        0.0
    };
    heights.sort_unstable();
    let median_h = heights[heights.len() / 2].max(1) as f32;
    let font_size = (median_h * 0.9).clamp(8.0, 96.0);

    Some(OcrLine {
        text: final_text.to_string(),
        bbox,
        conf: avg_conf,
        font_size,
    })
}

fn join_inline(left: &str, right: &str) -> String {
    if needs_space(left, right) {
        format!("{} {}", left.trim_end(), right.trim_start())
    } else {
        format!("{}{}", left.trim_end(), right.trim_start())
    }
}

fn needs_space(left: &str, right: &str) -> bool {
    let last = left.chars().rev().find(|ch| !ch.is_whitespace());
    let first = right.chars().find(|ch| !ch.is_whitespace());
    match (last, first) {
        (Some(a), Some(b)) => {
            (a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric())
                || (a.is_alphabetic() && b.is_alphabetic())
        }
        _ => false,
    }
}

fn merge_conf(a: f32, a_len: usize, b: f32, b_len: usize) -> f32 {
    let total = (a_len + b_len).max(1) as f32;
    (a * a_len as f32 + b * b_len as f32) / total
}

fn wrap_text(text: &str, max_units: f32) -> Vec<String> {
    let tokens = tokenize_text(text);
    wrap_tokens(&tokens, max_units)
}

fn wrap_tokens(tokens: &[String], max_units: f32) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut units = 0.0;

    for token in tokens {
        if token == "\n" {
            if !current.trim().is_empty() {
                result.push(current.trim_end().to_string());
            }
            current.clear();
            units = 0.0;
            continue;
        }
        if token.is_empty() {
            continue;
        }
        if token == " " {
            if !current.ends_with(' ') && !current.is_empty() {
                current.push(' ');
                units += 0.3;
            }
            continue;
        }
        let token_units = estimate_text_units(token);
        if units + token_units > max_units && !current.trim().is_empty() {
            result.push(current.trim_end().to_string());
            current.clear();
            units = 0.0;
        }
        current.push_str(token);
        units += token_units;
    }

    if !current.trim().is_empty() {
        result.push(current.trim_end().to_string());
    }

    if result.is_empty() {
        result.push(tokens.join("").trim().to_string());
    }
    result
}

fn fit_text_to_box(
    text: &str,
    font_size_base: f32,
    inner_w: f32,
    inner_h: f32,
    allow_shrink: bool,
) -> (f32, Vec<String>, f32) {
    let min_size = 10.0;
    if !allow_shrink {
        let font_size = font_size_base.max(min_size);
        let line_height = font_size * 1.1;
        let lines_text = wrap_text(text, (inner_w / font_size).max(1.0));
        return (font_size, lines_text, line_height);
    }

    let mut font_size = font_size_base.min(inner_h.max(min_size));
    let mut line_height = font_size * 1.1;
    let mut lines_text = wrap_text(text, (inner_w / font_size).max(1.0));
    let has_cjk = text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF
        )
    });
    let max_lines = if has_cjk { 4 } else { 3 };

    for _ in 0..8 {
        if lines_text.is_empty() {
            lines_text.push(text.trim().to_string());
        }
        line_height = font_size * 1.1;
        let block_height = lines_text.len() as f32 * line_height;
        let fits_height = block_height <= inner_h;
        if (lines_text.len() <= max_lines && fits_height) || font_size <= min_size {
            break;
        }
        let shrink_by_lines = if lines_text.len() > max_lines {
            max_lines as f32 / lines_text.len() as f32
        } else {
            1.0
        };
        let shrink_by_height = if fits_height {
            1.0
        } else {
            inner_h / block_height
        };
        let shrink = shrink_by_lines.min(shrink_by_height).min(0.92);
        font_size = (font_size * shrink).max(min_size);
        lines_text = wrap_text(text, (inner_w / font_size).max(1.0));
    }

    (font_size, lines_text, line_height)
}

fn choose_fixed_font_size(lines: &[TranslatedLine], height: u32) -> f32 {
    let mut sizes: Vec<f32> = lines
        .iter()
        .filter_map(|line| {
            if line.font_size > 0.0 {
                Some(line.font_size)
            } else {
                None
            }
        })
        .collect();
    let base = if sizes.is_empty() {
        height as f32 * 0.028
    } else {
        sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sizes[sizes.len() / 2]
    };
    (base * 1.15).clamp(12.0, 32.0)
}

#[derive(Clone, Copy)]
struct PlacedRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

struct ResolveOverlapConfig {
    anchor: PlacedRect,
    anchor_overlap_ratio: f32,
    prefer_side: bool,
    direction: f32,
    bounds_w: f32,
    bounds_h: f32,
    gap: f32,
}

fn build_avoid_rects(lines: &[TranslatedLine]) -> Vec<PlacedRect> {
    lines
        .iter()
        .map(|line| PlacedRect {
            x: line.bbox.x as f32,
            y: line.bbox.y as f32,
            w: line.bbox.w as f32,
            h: line.bbox.h as f32,
        })
        .collect()
}

fn rects_intersect(a: PlacedRect, b: PlacedRect, gap: f32) -> bool {
    a.x < b.x + b.w + gap && a.x + a.w + gap > b.x && a.y < b.y + b.h + gap && a.y + a.h + gap > b.y
}

fn resolve_overlap(
    rect: PlacedRect,
    placed: &[PlacedRect],
    avoid: &[PlacedRect],
    config: &ResolveOverlapConfig,
) -> Option<PlacedRect> {
    let step = config.gap.max(2.0);
    let max_radius = config.bounds_w.max(config.bounds_h);
    let mut radius = 0.0;

    while radius <= max_radius {
        let offsets = offsets_for_radius(radius, config.direction, config.prefer_side);
        for (dx, dy) in offsets {
            let mut candidate = PlacedRect {
                x: rect.x + dx,
                y: rect.y + dy,
                w: rect.w,
                h: rect.h,
            };
            candidate.x = candidate
                .x
                .clamp(0.0, (config.bounds_w - candidate.w).max(0.0));
            candidate.y = candidate
                .y
                .clamp(0.0, (config.bounds_h - candidate.h).max(0.0));
            if !collides(
                candidate,
                placed,
                avoid,
                &config.anchor,
                config.anchor_overlap_ratio,
                config.gap,
                config.gap,
            ) {
                return Some(candidate);
            }
        }
        radius += step;
    }

    let diag_steps = [50.0, 25.0, 12.0, 6.0, 3.0, 1.0];
    for step in diag_steps {
        for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let mut candidate = PlacedRect {
                x: rect.x + sx * step,
                y: rect.y + sy * step,
                w: rect.w,
                h: rect.h,
            };
            candidate.x = candidate
                .x
                .clamp(0.0, (config.bounds_w - candidate.w).max(0.0));
            candidate.y = candidate
                .y
                .clamp(0.0, (config.bounds_h - candidate.h).max(0.0));
            if !collides(
                candidate,
                placed,
                avoid,
                &config.anchor,
                config.anchor_overlap_ratio,
                config.gap,
                config.gap,
            ) {
                return Some(candidate);
            }
        }
    }

    None
}

fn offsets_for_radius(radius: f32, _direction: f32, prefer_side: bool) -> Vec<(f32, f32)> {
    if radius <= 0.0 {
        return vec![(0.0, 0.0)];
    }
    if prefer_side {
        vec![
            (radius, 0.0),      // right
            (-radius, 0.0),     // left
            (0.0, radius),      // down
            (0.0, -radius),     // up
            (radius, radius),   // down-right
            (-radius, -radius), // up-left
            (-radius, radius),  // down-left
            (radius, -radius),  // up-right
        ]
    } else {
        vec![
            (0.0, radius),      // down
            (radius, 0.0),      // right
            (-radius, 0.0),     // left
            (0.0, -radius),     // up
            (radius, radius),   // down-right
            (-radius, -radius), // up-left
            (-radius, radius),  // down-left
            (radius, -radius),  // up-right
        ]
    }
}

fn collides(
    rect: PlacedRect,
    placed: &[PlacedRect],
    avoid: &[PlacedRect],
    anchor: &PlacedRect,
    anchor_overlap_ratio: f32,
    placed_gap: f32,
    avoid_gap: f32,
) -> bool {
    for existing in placed {
        if rects_intersect(rect, *existing, placed_gap) {
            return true;
        }
    }
    for existing in avoid {
        if rects_intersect(rect, *existing, avoid_gap) {
            if is_anchor_rect(existing, anchor)
                && allowed_anchor_overlap(rect, *existing, anchor_overlap_ratio)
            {
                continue;
            }
            return true;
        }
    }
    false
}

fn is_anchor_rect(a: &PlacedRect, b: &PlacedRect) -> bool {
    (a.x - b.x).abs() < 0.5
        && (a.y - b.y).abs() < 0.5
        && (a.w - b.w).abs() < 0.5
        && (a.h - b.h).abs() < 0.5
}

fn has_avoid_below(avoid: &[PlacedRect], anchor: &PlacedRect, gap: f32) -> bool {
    let anchor_bottom = anchor.y + anchor.h;
    let max_gap = (anchor.h * 2.5).max(48.0) + gap;
    for rect in avoid {
        if is_anchor_rect(rect, anchor) {
            continue;
        }
        if rect.y < anchor_bottom - gap {
            continue;
        }
        if horizontal_overlap_ratio_rect(anchor, rect) < 0.3 {
            continue;
        }
        let gap_y = rect.y - anchor_bottom;
        if gap_y <= max_gap {
            return true;
        }
    }
    false
}

fn horizontal_overlap_ratio_rect(a: &PlacedRect, b: &PlacedRect) -> f32 {
    let ax2 = a.x + a.w;
    let bx2 = b.x + b.w;
    let ix1 = a.x.max(b.x);
    let ix2 = ax2.min(bx2);
    if ix2 <= ix1 {
        return 0.0;
    }
    let inter = ix2 - ix1;
    inter / a.w.min(b.w).max(1.0)
}

fn allowed_anchor_overlap(rect: PlacedRect, anchor: PlacedRect, ratio: f32) -> bool {
    let overlap_y = (rect.y + rect.h).min(anchor.y + anchor.h) - rect.y.max(anchor.y);
    if overlap_y <= 0.0 {
        return true;
    }
    let allowed = anchor.h * ratio.clamp(0.0, 1.0);
    overlap_y <= allowed
}

fn estimate_char_units(ch: char) -> f32 {
    let code = ch as u32;
    if ch.is_ascii() {
        0.6
    } else if matches!(code, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF) {
        1.0
    } else {
        0.9
    }
}

fn estimate_text_units(text: &str) -> f32 {
    text.chars().map(estimate_char_units).sum()
}

fn estimate_char_units_for_width(ch: char) -> f32 {
    if ch.is_whitespace() {
        0.25
    } else if ch.is_ascii_alphanumeric() {
        0.55
    } else if ch.is_ascii() {
        0.35
    } else if matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF
    ) {
        1.0
    } else {
        0.9
    }
}

fn estimate_text_width_units(text: &str) -> f32 {
    text.chars().map(estimate_char_units_for_width).sum()
}

fn measure_text_width_px(text: &str, font_size: f32, font: Option<&FontMetrics>) -> f32 {
    if let Some(font) = font {
        if let Ok(face) = Face::parse(&font.data, font.face_index) {
            let mut advance = 0u32;
            for ch in text.chars() {
                if ch == '\n' {
                    continue;
                }
                if ch == ' ' {
                    advance = advance.saturating_add(font.space_advance as u32);
                    continue;
                }
                if let Some(glyph) = face.glyph_index(ch) {
                    let glyph_advance = face.glyph_hor_advance(glyph).unwrap_or(font.space_advance);
                    advance = advance.saturating_add(glyph_advance as u32);
                } else {
                    advance = advance.saturating_add(font.space_advance as u32);
                }
            }
            let units = font.units_per_em.max(1) as f32;
            return advance as f32 * (font_size / units);
        }
    }
    estimate_text_width_units(text) * font_size
}

fn load_font_metrics_from_data(data: &[u8], preferred_family: Option<&str>) -> Result<FontMetrics> {
    let mut fallback = None;
    let count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    for index in 0..count {
        if let Ok(face) = Face::parse(data, index) {
            let family = extract_family_name(&face);
            let units_per_em = face.units_per_em().max(1);
            let space_advance = face
                .glyph_index(' ')
                .and_then(|id| face.glyph_hor_advance(id))
                .unwrap_or(units_per_em / 2);
            let metrics = FontMetrics {
                data: Arc::new(data.to_vec()),
                units_per_em,
                space_advance,
                family: family.clone(),
                face_index: index,
            };
            if let (Some(preferred), Some(found)) = (preferred_family, &family) {
                if found.eq_ignore_ascii_case(preferred) {
                    return Ok(metrics);
                }
            }
            if fallback.is_none() {
                fallback = Some(metrics);
            }
        }
    }
    if preferred_family.is_some() {
        return Err(anyhow!("font family not found in font file"));
    }
    fallback.ok_or_else(|| anyhow!("failed to parse font data"))
}

fn load_font_metrics_from_family(
    db: &fontdb::Database,
    family: &str,
) -> Result<ResolvedOverlayFont> {
    let is_sans =
        family.eq_ignore_ascii_case("sans-serif") || family.eq_ignore_ascii_case("sens-serif");
    let families = if is_sans {
        vec![fontdb::Family::SansSerif]
    } else {
        vec![fontdb::Family::Name(family)]
    };
    let query = fontdb::Query {
        families: &families,
        ..Default::default()
    };
    let id = db
        .query(&query)
        .ok_or_else(|| anyhow!("font not found: {}", family))?;
    let (data, _face_index) = db
        .with_face_data(id, |data, index| (data.to_vec(), index))
        .ok_or_else(|| anyhow!("failed to load font data: {}", family))?;
    let metrics = load_font_metrics_from_data(&data, Some(family))?;
    let resolved_family = metrics
        .family()
        .map(|name| name.to_string())
        .unwrap_or_else(|| family.to_string());
    Ok(ResolvedOverlayFont {
        metrics,
        family: resolved_family,
    })
}

fn extract_family_name(face: &Face<'_>) -> Option<String> {
    let mut fallback = None;
    for name in face.names() {
        if name.name_id == name_id::TYPOGRAPHIC_FAMILY {
            if let Some(value) = name.to_string() {
                return Some(value);
            }
        } else if name.name_id == name_id::FAMILY && fallback.is_none() {
            fallback = name.to_string();
        }
    }
    fallback
}

fn cjk_ratio(text: &str) -> f32 {
    let mut cjk = 0usize;
    let mut total = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        total += 1;
        if matches!(
            ch as u32,
            0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF
        ) {
            cjk += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        cjk as f32 / total as f32
    }
}

fn tokenize_text(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_word = false;

    for ch in text.chars() {
        if ch == '\n' {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            tokens.push("\n".to_string());
            in_word = false;
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            tokens.push(" ".to_string());
            in_word = false;
            continue;
        }
        let is_cjk = matches!(
            ch as u32,
            0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF
        );
        if is_cjk {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            tokens.push(ch.to_string());
            in_word = false;
            continue;
        }
        if !in_word && !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
        current.push(ch);
        in_word = true;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn find_subslice(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack[from..]
        .windows(needle.len())
        .position(|win| win == needle)
        .map(|pos| from + pos)
}

fn find_byte(haystack: &[u8], needle: u8, from: usize) -> Option<usize> {
    haystack[from..]
        .iter()
        .position(|b| *b == needle)
        .map(|pos| from + pos)
}

fn find_span_inner(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut depth = 1i32;
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if bytes[i..].starts_with(b"<span") {
                depth += 1;
            } else if bytes[i..].starts_with(b"</span") {
                depth -= 1;
                if depth == 0 {
                    return Some((start, i));
                }
            }
        }
        i += 1;
    }
    None
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{}=", name);
    let idx = tag.find(&needle)?;
    let mut rest = &tag[idx + needle.len()..];
    if rest.starts_with('"') || rest.starts_with('\'') {
        let quote = rest.chars().next().unwrap();
        rest = &rest[1..];
        let end = rest.find(quote)?;
        return Some(rest[..end].to_string());
    }
    None
}

fn strip_tags(value: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ => {
                if !in_tag {
                    out.push(ch);
                }
            }
        }
    }
    out
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn parse_hocr_words(inner: &str) -> Vec<WordToken> {
    let bytes = inner.as_bytes();
    let mut words = Vec::new();
    let mut i = 0usize;
    while let Some(start) = find_subslice(bytes, b"<span", i) {
        let tag_end = match find_byte(bytes, b'>', start) {
            Some(end) => end,
            None => break,
        };
        let tag = &inner[start..tag_end];
        if !tag.contains("ocrx_word") {
            i = tag_end + 1;
            continue;
        }
        let (inner_start, inner_end) = match find_span_inner(bytes, tag_end + 1) {
            Some(value) => value,
            None => break,
        };
        let word_text =
            decode_entities(&strip_tags(&inner[inner_start..inner_end])).replace('\u{00a0}', " ");
        let word_text = word_text.trim();
        let bbox = parse_hocr_bbox_from_title(tag);
        let conf = parse_hocr_conf_from_title(tag);
        if let (Some(bbox), Some(conf)) = (bbox, conf) {
            let cleaned = normalize_word_text(word_text);
            if should_keep_hocr_word(&cleaned, conf, &bbox) {
                let len = cleaned.chars().count().max(1);
                words.push(WordToken {
                    text: cleaned,
                    bbox,
                    conf,
                    len,
                });
            }
        }
        i = inner_end + "</span>".len();
    }
    words
}

fn parse_hocr_bbox_from_title(tag: &str) -> Option<BBoxPx> {
    let title = extract_attr(tag, "title")?;
    let bbox_idx = title.find("bbox")?;
    let rest = &title[bbox_idx + 4..];
    let nums = rest
        .split([' ', ';'])
        .filter(|v| !v.is_empty())
        .take(4)
        .filter_map(|v| v.parse::<u32>().ok())
        .collect::<Vec<_>>();
    if nums.len() != 4 {
        return None;
    }
    let (x1, y1, x2, y2) = (nums[0], nums[1], nums[2], nums[3]);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    Some(BBoxPx {
        x: x1,
        y: y1,
        w: x2 - x1,
        h: y2 - y1,
    })
}

fn parse_hocr_conf_from_title(tag: &str) -> Option<f32> {
    let title = extract_attr(tag, "title")?;
    let idx = title.find("x_wconf")?;
    let rest = &title[idx + "x_wconf".len()..];
    let value = rest
        .split([' ', ';'])
        .find(|v| !v.is_empty())?;
    value.parse::<f32>().ok()
}

fn normalize_word_text(value: &str) -> String {
    value.trim().to_string()
}

fn should_keep_hocr_word(text: &str, conf: f32, bbox: &BBoxPx) -> bool {
    if text.is_empty() {
        return false;
    }
    if bbox.w == 0 {
        return false;
    }
    if bbox.h < 8 {
        return conf >= 80.0 && text.chars().count() >= 2;
    }
    if conf < 55.0 && text.chars().count() <= 1 {
        return false;
    }
    if conf < 60.0
        && !text
            .chars()
            .any(|ch| ch.is_alphanumeric() || is_cjk_or_kana(ch))
    {
        return false;
    }
    true
}

fn is_cjk_or_kana(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF
    )
}

fn normalize_ocr_languages(requested: &str) -> Result<String> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("ocr languages is empty"));
    }

    let available = match list_tesseract_languages() {
        Ok(list) => list,
        Err(_) => return Ok(trimmed.to_string()),
    };

    let mut chosen = Vec::new();
    let mut missing = Vec::new();
    for raw in trimmed.split(['+', ',', ' ']) {
        let lang = raw.trim();
        if lang.is_empty() {
            continue;
        }
        if available.iter().any(|value| value == lang) {
            chosen.push(lang.to_string());
        } else {
            missing.push(lang.to_string());
        }
    }

    if chosen.is_empty() {
        return Err(anyhow!(
            "ocr language(s) not available: {} (available: {})",
            missing.join(", "),
            available.join(", ")
        ));
    }
    if !missing.is_empty() {
        eprintln!(
            "warning: ocr language(s) not available: {} (available: {})",
            missing.join(", "),
            available.join(", ")
        );
    }

    Ok(chosen.join("+"))
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
