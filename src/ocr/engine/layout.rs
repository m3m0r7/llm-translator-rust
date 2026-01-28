use crate::ocr::TranslatedLine;

pub(crate) fn wrap_text(text: &str, max_units: f32) -> Vec<String> {
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

pub(crate) fn fit_text_to_box(
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

pub(crate) fn choose_fixed_font_size(lines: &[TranslatedLine], height: u32) -> f32 {
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
pub(crate) struct PlacedRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

pub(crate) struct ResolveOverlapConfig {
    pub(crate) anchor: PlacedRect,
    pub(crate) anchor_overlap_ratio: f32,
    pub(crate) prefer_side: bool,
    pub(crate) direction: f32,
    pub(crate) bounds_w: f32,
    pub(crate) bounds_h: f32,
    pub(crate) gap: f32,
}

pub(crate) fn build_avoid_rects(lines: &[TranslatedLine]) -> Vec<PlacedRect> {
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

pub(crate) fn resolve_overlap(
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

fn rects_intersect(a: PlacedRect, b: PlacedRect, gap: f32) -> bool {
    a.x < b.x + b.w + gap && a.x + a.w + gap > b.x && a.y < b.y + b.h + gap && a.y + a.h + gap > b.y
}

fn offsets_for_radius(radius: f32, _direction: f32, prefer_side: bool) -> Vec<(f32, f32)> {
    if radius <= 0.0 {
        return vec![(0.0, 0.0)];
    }
    if prefer_side {
        vec![
            (radius, 0.0),
            (-radius, 0.0),
            (0.0, radius),
            (0.0, -radius),
            (radius, radius),
            (-radius, -radius),
            (-radius, radius),
            (radius, -radius),
        ]
    } else {
        vec![
            (0.0, radius),
            (radius, 0.0),
            (-radius, 0.0),
            (0.0, -radius),
            (radius, radius),
            (-radius, -radius),
            (-radius, radius),
            (radius, -radius),
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

pub(crate) fn has_avoid_below(avoid: &[PlacedRect], anchor: &PlacedRect, gap: f32) -> bool {
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
