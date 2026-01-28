use anyhow::Result;
use std::collections::HashMap;

use crate::ocr::{BBoxPx, OcrLine};

use super::geom::union_bbox;
use super::text::needs_space;

#[derive(Clone)]
struct WordToken {
    text: String,
    bbox: BBoxPx,
    conf: f32,
    len: usize,
}

pub(super) fn parse_tsv_lines(tsv: &str) -> Result<Vec<OcrLine>> {
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

pub(super) fn parse_hocr_lines(hocr: &str) -> Result<Vec<OcrLine>> {
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
    let value = rest.split([' ', ';']).find(|v| !v.is_empty())?;
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
