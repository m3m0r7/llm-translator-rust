use crate::ocr::{BBoxPx, OcrLine};

use super::geom::{horizontal_overlap_ratio, iou, union_bbox, vertical_overlap_ratio};
use super::text::{join_inline, merge_conf};

pub(super) fn merge_lines(mut base: Vec<OcrLine>, extra: Vec<OcrLine>) -> Vec<OcrLine> {
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

pub(super) fn scale_lines(lines: Vec<OcrLine>, scale: f32) -> Vec<OcrLine> {
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

pub(super) fn filter_lines(lines: Vec<OcrLine>, width: u32, height: u32) -> Vec<OcrLine> {
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

pub(super) fn merge_inline_lines(mut lines: Vec<OcrLine>) -> Vec<OcrLine> {
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

pub(super) fn suppress_overlaps(lines: Vec<OcrLine>) -> Vec<OcrLine> {
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
