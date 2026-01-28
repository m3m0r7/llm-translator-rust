use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::sync::Arc;
use ttf_parser::name_id;
use ttf_parser::Face;
use usvg::fontdb;

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

pub(crate) fn measure_text_width_px(text: &str, font_size: f32, font: Option<&FontMetrics>) -> f32 {
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
