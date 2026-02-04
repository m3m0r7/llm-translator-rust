use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use resvg::render;
use std::io::Cursor;
use std::sync::Arc;
use tiny_skia::Pixmap;
use usvg::{Options, Tree, fontdb};

use super::{BBoxPx, OcrLine, OverlayStyle, TranslatedLine};
use crate::ocr::engine::{
    ResolveOverlapConfig, build_avoid_rects, choose_fixed_font_size, fit_text_to_box,
    has_avoid_below, resolve_overlap,
};
use crate::ocr::font::measure_text_width_px;

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

        let is_label = line.text.chars().all(|ch| ch.is_ascii_digit());
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

        let mut base = super::engine::PlacedRect {
            x: rect_x,
            y: rect_y,
            w: box_w,
            h: box_h,
        };
        let anchor = super::engine::PlacedRect {
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
            .as_deref()
            .or_else(|| style.font_metrics.as_ref().and_then(|m| m.family()));

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
                text_block.push_str(&escaped);
            } else {
                let dy = line_height;
                text_block.push_str(&format!(
                    r#"<tspan x="{x}" dy="{dy}">{text}</tspan>"#,
                    x = rect_x + padding,
                    dy = dy,
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
            .as_deref()
            .or_else(|| style.font_metrics.as_ref().and_then(|m| m.family()));
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

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
