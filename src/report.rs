use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use time::{format_description, OffsetDateTime};

use crate::languages::{self, LanguagePack, LanguageRegistry};
use crate::model_registry::{HistoryEntry, HistoryType};
use crate::providers::{Provider, ToolSpec};
use crate::translations;
use crate::{ReportFormat, Translator};

const ANALYSIS_TOOL_NAME: &str = "generate_report_analysis";
const MAX_ANALYSIS_ITEMS: usize = 200;
const MAX_REPORT_TEXTS: usize = 100;
const MAX_TEXT_LEN: usize = 320;

#[derive(Debug, Clone, Serialize)]
pub struct ReportData {
    pub generated_at: String,
    pub totals: Totals,
    pub languages: Vec<CountItem>,
    pub types: Vec<CountItem>,
    pub models: Vec<CountItem>,
    pub daily: Vec<CountItem>,
    pub translated_histories: Vec<HistoryRow>,
    pub tags: Vec<CountItem>,
    pub clusters: Vec<Cluster>,
    pub keywords: Vec<KeywordEntry>,
    pub daily_bars: Vec<DailyBar>,
    #[serde(skip_serializing)]
    pub labels: ReportLabels,
}

#[derive(Debug, Clone, Serialize)]
pub struct Totals {
    pub total: usize,
    pub text: usize,
    pub attachment: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CountItem {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextCount {
    pub text: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryRow {
    pub datetime: String,
    pub kind: String,
    pub source_lang: String,
    pub target_lang: String,
    pub source_text: String,
    pub target_text: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DailyBar {
    pub date: String,
    pub count: usize,
    pub height: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub label: String,
    pub items: Vec<TextCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordEntry {
    pub keyword: String,
    pub count: usize,
    pub translation: String,
    pub pos: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ReportLabels {
    pub report_title: String,
    pub generated_at: String,
    pub totals: String,
    pub total: String,
    pub text: String,
    pub attachment: String,
    pub language_pairs: String,
    pub content_types: String,
    pub models: String,
    pub daily_volume: String,
    pub translated_histories: String,
    pub tags: String,
    pub clusters: String,
    pub keywords: String,
    pub history_date: String,
    pub history_type: String,
    pub history_source_lang: String,
    pub history_target_lang: String,
    pub history_source_text: String,
    pub history_target_text: String,
    pub history_tags: String,
    pub unknown: String,
    pub auto: String,
    pub type_text: String,
    pub type_audio: String,
    pub type_image: String,
    pub type_video: String,
    pub type_document: String,
    pub type_other: String,
}

pub async fn build_report<P: Provider + Clone>(
    translator: &Translator<P>,
    histories: &[HistoryEntry],
    display_lang_hint: Option<&str>,
) -> Result<ReportData> {
    let generated_at = OffsetDateTime::now_utc()
        .format(&format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    let (text_count, attachment_count) =
        histories
            .iter()
            .fold((0, 0), |acc, entry| match entry.kind {
                HistoryType::Text => (acc.0 + 1, acc.1),
                HistoryType::Attachment => (acc.0, acc.1 + 1),
            });

    let totals = Totals {
        total: histories.len(),
        text: text_count,
        attachment: attachment_count,
    };

    let display_lang = choose_report_language(histories, translator.settings(), display_lang_hint);
    let report_packs = load_report_packs(display_lang.as_deref());
    let labels = resolve_report_labels(&report_packs);
    let languages = count_languages(
        histories,
        translator.registry(),
        report_packs.display_pack(),
        &labels,
    );
    let types = count_types(histories, &labels);
    let models = count_models(histories);
    let daily = count_daily(histories);
    let daily_bars = build_daily_bars(&daily);
    let translated_histories = collect_histories(
        histories,
        translator.registry(),
        report_packs.display_pack(),
        &labels,
    );
    let tags = count_tags(histories);
    let analysis_texts = collect_analysis_texts(histories);

    let analysis = if analysis_texts.is_empty() {
        ReportAnalysis::default()
    } else {
        let target_lang = most_common_target_lang(histories).unwrap_or_else(|| "en".to_string());
        analyze_texts(translator, &analysis_texts, &target_lang).await?
    };

    Ok(ReportData {
        generated_at,
        totals,
        languages,
        types,
        models,
        daily,
        translated_histories,
        tags,
        clusters: analysis.clusters,
        keywords: analysis.keywords,
        daily_bars,
        labels,
    })
}

pub fn render_report(report: &ReportData, format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Json => Ok(serde_json::to_string_pretty(report)?),
        ReportFormat::Xml => render_xml(report),
        ReportFormat::Html => render_html(report),
    }
}

pub fn resolve_report_out(format: ReportFormat, path: Option<&str>) -> PathBuf {
    if let Some(path) = path {
        return PathBuf::from(path);
    }
    PathBuf::from(format!("report.{}", format.extension()))
}

struct ReportPacks {
    packs: Option<languages::LanguagePacks>,
    display_code: Option<String>,
}

impl ReportPacks {
    fn display_pack(&self) -> Option<&LanguagePack> {
        let packs = self.packs.as_ref()?;
        if let Some(code) = self.display_code.as_deref() {
            if let Some(pack) = packs.packs.get(code) {
                return Some(pack);
            }
        }
        packs.packs.get("eng")
    }

    fn english_pack(&self) -> Option<&LanguagePack> {
        self.packs.as_ref().and_then(|packs| packs.packs.get("eng"))
    }
}

fn load_report_packs(display_lang: Option<&str>) -> ReportPacks {
    let display_code = display_lang.and_then(normalize_pack_code);
    let mut codes = Vec::new();
    if let Some(code) = display_code.as_deref() {
        if code != "eng" {
            codes.push(code.to_string());
        }
    }
    codes.push("eng".to_string());

    let packs = languages::load_language_packs(&codes)
        .or_else(|_| languages::load_language_packs(&["eng".to_string()]))
        .ok();

    ReportPacks {
        packs,
        display_code,
    }
}

fn resolve_report_labels(report_packs: &ReportPacks) -> ReportLabels {
    let display_pack = report_packs.display_pack();
    let english_pack = report_packs.english_pack();
    let fallback = |key: &str, default: &str| {
        display_pack
            .and_then(|pack| pack.report_labels.get(key))
            .or_else(|| english_pack.and_then(|pack| pack.report_labels.get(key)))
            .cloned()
            .unwrap_or_else(|| default.to_string())
    };

    ReportLabels {
        report_title: fallback("report_title", "Translation Report"),
        generated_at: fallback("generated_at", "Generated at"),
        totals: fallback("totals", "Totals"),
        total: fallback("total", "Total"),
        text: fallback("text", "Text"),
        attachment: fallback("attachment", "Attachment"),
        language_pairs: fallback("language_pairs", "Language pairs"),
        content_types: fallback("content_types", "Content types"),
        models: fallback("models", "Models"),
        daily_volume: fallback("daily_volume", "Daily volume"),
        translated_histories: fallback("translated_histories", "Translated histories"),
        tags: fallback("tags", "Tags"),
        clusters: fallback("clusters", "Clusters"),
        keywords: fallback("keywords", "Keywords"),
        history_date: fallback("history_date", "Date"),
        history_type: fallback("history_type", "Type"),
        history_source_lang: fallback("history_source_lang", "Source"),
        history_target_lang: fallback("history_target_lang", "Target"),
        history_source_text: fallback("history_source_text", "Source text"),
        history_target_text: fallback("history_target_text", "Translated text"),
        history_tags: fallback("history_tags", "Tags"),
        unknown: fallback("unknown", "Unknown"),
        auto: fallback("auto", "Auto"),
        type_text: fallback("type_text", "Text"),
        type_audio: fallback("type_audio", "Audio"),
        type_image: fallback("type_image", "Image"),
        type_video: fallback("type_video", "Video"),
        type_document: fallback("type_document", "Document"),
        type_other: fallback("type_other", "Other"),
    }
}

fn count_languages(
    histories: &[HistoryEntry],
    registry: &LanguageRegistry,
    pack: Option<&LanguagePack>,
    labels: &ReportLabels,
) -> Vec<CountItem> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        let from = resolve_entry_lang(entry, true);
        let to = resolve_entry_lang(entry, false);
        let from_label = display_lang_label(from.as_deref(), registry, pack, labels);
        let to_label = display_lang_label(to.as_deref(), registry, pack, labels);
        let key = format!("{} -> {}", from_label, to_label);
        *map.entry(key).or_insert(0) += 1;
    }
    counts_from_map(map)
}

fn count_types(histories: &[HistoryEntry], labels: &ReportLabels) -> Vec<CountItem> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        let label = classify_type(entry, labels);
        *map.entry(label).or_insert(0) += 1;
    }
    counts_from_map(map)
}

fn count_models(histories: &[HistoryEntry]) -> Vec<CountItem> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        let key = entry.model.trim();
        if key.is_empty() {
            continue;
        }
        *map.entry(key.to_string()).or_insert(0) += 1;
    }
    counts_from_map(map)
}

fn count_daily(histories: &[HistoryEntry]) -> Vec<CountItem> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        let date = unix_to_date(entry.datetime.trim());
        *map.entry(date).or_insert(0) += 1;
    }
    let mut items = map
        .into_iter()
        .map(|(label, count)| CountItem { label, count })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn build_daily_bars(daily: &[CountItem]) -> Vec<DailyBar> {
    let max = daily.iter().map(|item| item.count).max().unwrap_or(0);
    daily
        .iter()
        .map(|item| DailyBar {
            date: item.label.clone(),
            count: item.count,
            height: if max == 0 {
                0
            } else {
                let ratio = item.count as f64 / max as f64;
                let percent = (ratio * 100.0).round();
                percent.clamp(6.0, 100.0) as u8
            },
        })
        .collect()
}

fn count_tags(histories: &[HistoryEntry]) -> Vec<CountItem> {
    let mut map: HashMap<String, (String, usize)> = HashMap::new();
    for entry in histories {
        let Some(tags) = entry.tags.as_ref() else {
            continue;
        };
        for tag in tags {
            let trimmed = tag.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            let slot = map.entry(key).or_insert_with(|| (trimmed.to_string(), 0));
            slot.1 += 1;
        }
    }
    let mut items = map
        .into_iter()
        .map(|(_, (label, count))| CountItem { label, count })
        .collect::<Vec<_>>();
    sort_desc(&mut items, |item| item.count);
    items
}

fn collect_histories(
    histories: &[HistoryEntry],
    registry: &LanguageRegistry,
    pack: Option<&LanguagePack>,
    labels: &ReportLabels,
) -> Vec<HistoryRow> {
    histories
        .iter()
        .map(|entry| {
            let source_lang = display_lang_label(
                resolve_entry_lang(entry, true).as_deref(),
                registry,
                pack,
                labels,
            );
            let target_lang = display_lang_label(
                resolve_entry_lang(entry, false).as_deref(),
                registry,
                pack,
                labels,
            );
            HistoryRow {
                datetime: format_history_datetime(entry.datetime.trim()),
                kind: history_kind_label(entry, labels),
                source_lang,
                target_lang,
                source_text: entry.src.clone(),
                target_text: entry.dest.clone(),
                tags: entry.tags.clone().unwrap_or_default(),
            }
        })
        .collect()
}

fn history_kind_label(entry: &HistoryEntry, labels: &ReportLabels) -> String {
    classify_type(entry, labels)
}

fn format_history_datetime(value: &str) -> String {
    let Ok(secs) = value.parse::<i64>() else {
        return "unknown".to_string();
    };
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) else {
        return "unknown".to_string();
    };
    let format = format_description::parse("[year]-[month]-[day] [hour]:[minute]");
    if let Ok(format) = format {
        if let Ok(rendered) = dt.format(&format) {
            return rendered;
        }
    }
    dt.date().to_string()
}

fn choose_report_language(
    histories: &[HistoryEntry],
    settings: &crate::settings::Settings,
    hint: Option<&str>,
) -> Option<String> {
    if let Some(code) = normalize_report_lang_hint(hint) {
        return Some(code);
    }
    if let Some(code) = most_common_source_lang(histories) {
        return Some(code);
    }
    settings.system_languages.first().cloned()
}

fn most_common_source_lang(histories: &[HistoryEntry]) -> Option<String> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        if let Some(code) = resolve_entry_lang(entry, true) {
            if is_unknown_lang(&code) || code == "auto" {
                continue;
            }
            *map.entry(code).or_insert(0) += 1;
        }
    }
    map.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(lang, _)| lang)
}

fn collect_analysis_texts(histories: &[HistoryEntry]) -> Vec<TextCount> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        if !matches!(entry.kind, HistoryType::Text) {
            continue;
        }
        let text = entry.src.trim();
        if text.is_empty() {
            continue;
        }
        let normalized = truncate_text(&normalize_text(text), MAX_TEXT_LEN);
        *map.entry(normalized).or_insert(0) += 1;
    }
    let mut items = map
        .into_iter()
        .map(|(text, count)| TextCount { text, count })
        .collect::<Vec<_>>();
    sort_desc(&mut items, |item| item.count);
    items.truncate(MAX_REPORT_TEXTS);
    items
}

async fn analyze_texts<P: Provider + Clone>(
    translator: &Translator<P>,
    texts: &[TextCount],
    target_lang: &str,
) -> Result<ReportAnalysis> {
    let input_items = texts
        .iter()
        .take(MAX_ANALYSIS_ITEMS)
        .map(|item| TextCount {
            text: truncate_text(&item.text, MAX_TEXT_LEN),
            count: item.count,
        })
        .collect::<Vec<_>>();

    if input_items.is_empty() {
        return Ok(ReportAnalysis::default());
    }

    let prompt = render_report_prompt(target_lang)?;
    let tool = analysis_tool_spec();
    let input_json = serde_json::to_string_pretty(&json!({ "items": input_items }))?;

    let response = translator
        .call_tool_with_data(tool, prompt, input_json, None)
        .await?;
    let analysis = parse_analysis(response.args, texts)?;
    Ok(analysis)
}

fn analysis_tool_spec() -> ToolSpec {
    let base = json!({
        "type": "object",
        "properties": {
            "clusters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "text": { "type": "string" },
                                    "count": { "type": "number" }
                                },
                                "required": ["text", "count"]
                            }
                        }
                    },
                    "required": ["label", "items"]
                }
            },
            "keywords": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "keyword": { "type": "string" },
                        "count": { "type": "number" },
                        "translation": { "type": "string" },
                        "pos": { "type": "string" }
                    },
                    "required": ["keyword", "count", "translation", "pos"]
                }
            }
        },
        "required": ["clusters", "keywords"]
    });

    ToolSpec {
        name: ANALYSIS_TOOL_NAME.to_string(),
        description: "Return clusters and keyword analysis for translation history.".to_string(),
        parameters: base,
    }
}

fn render_report_prompt(target_lang: &str) -> Result<String> {
    let template = load_prompt_template("report_prompt.tera")?;
    let mut context = tera::Context::new();
    context.insert("keyword_target_lang", target_lang);
    context.insert("tool_name", ANALYSIS_TOOL_NAME);
    tera::Tera::one_off(&template, &context, false)
        .with_context(|| "failed to render report prompt")
}

#[derive(Debug, Default, Deserialize)]
struct ReportAnalysis {
    #[serde(default)]
    clusters: Vec<Cluster>,
    #[serde(default)]
    keywords: Vec<KeywordEntry>,
}

fn parse_analysis(value: serde_json::Value, texts: &[TextCount]) -> Result<ReportAnalysis> {
    let mut analysis: ReportAnalysis = serde_json::from_value(value)?;
    let mut lookup: HashMap<String, usize> = HashMap::new();
    for item in texts {
        lookup.insert(item.text.clone(), item.count);
    }

    for cluster in &mut analysis.clusters {
        let mut items = Vec::new();
        for item in &cluster.items {
            if let Some(count) = lookup.get(&item.text) {
                items.push(TextCount {
                    text: item.text.clone(),
                    count: *count,
                });
            }
        }
        sort_desc(&mut items, |item| item.count);
        cluster.items = items;
    }

    analysis.clusters.retain(|c| !c.items.is_empty());
    analysis
        .clusters
        .sort_by_key(|cluster| std::cmp::Reverse(cluster_total(cluster)));

    analysis.keywords.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.keyword.cmp(&b.keyword))
    });
    Ok(analysis)
}

fn cluster_total(cluster: &Cluster) -> usize {
    cluster.items.iter().map(|item| item.count).sum()
}

fn load_prompt_template(name: &str) -> Result<String> {
    let path = prompt_path(name)?;
    std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read prompt: {}", path.display()))
}

fn prompt_path(name: &str) -> Result<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("translations")
        .join("prompts")
        .join(name);
    Ok(base)
}

fn counts_from_map(map: HashMap<String, usize>) -> Vec<CountItem> {
    let mut items = map
        .into_iter()
        .map(|(label, count)| CountItem { label, count })
        .collect::<Vec<_>>();
    sort_desc(&mut items, |item| item.count);
    items
}

fn sort_desc<T, F: Fn(&T) -> usize>(items: &mut [T], key: F) {
    items.sort_by_key(|item| std::cmp::Reverse(key(item)));
}

fn normalize_text(text: &str) -> String {
    text.trim().to_string()
}

fn truncate_text(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_len).collect()
}

fn most_common_target_lang(histories: &[HistoryEntry]) -> Option<String> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for entry in histories {
        let Some(lang) = resolve_entry_lang(entry, false) else {
            continue;
        };
        if is_unknown_lang(&lang) || lang == "auto" {
            continue;
        }
        *map.entry(lang).or_insert(0) += 1;
    }
    map.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(lang, _)| lang)
}

fn resolve_entry_lang(entry: &HistoryEntry, is_source: bool) -> Option<String> {
    let value = if is_source {
        entry.source_language.as_deref()
    } else {
        entry.target_language.as_deref()
    };
    if let Some(code) = value.and_then(normalize_lang_code) {
        return Some(code);
    }
    if !matches!(entry.kind, HistoryType::Text) {
        return None;
    }
    let text = if is_source {
        entry.src.as_str()
    } else {
        entry.dest.as_str()
    };
    infer_lang_from_text(text)
}

fn display_lang_label(
    code: Option<&str>,
    registry: &LanguageRegistry,
    pack: Option<&LanguagePack>,
    labels: &ReportLabels,
) -> String {
    let Some(code) = code.and_then(normalize_lang_code) else {
        return labels.unknown.clone();
    };
    if code == "auto" {
        return labels.auto.clone();
    }
    if is_unknown_lang(&code) {
        return labels.unknown.clone();
    }
    translations::display_language(&code, registry, pack)
}

fn normalize_report_lang_hint(value: Option<&str>) -> Option<String> {
    let value = value?;
    let normalized = normalize_lang_code(value)?;
    if normalized == "auto" || is_unknown_lang(&normalized) {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_lang_code(value: &str) -> Option<String> {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed)
}

fn is_unknown_lang(code: &str) -> bool {
    matches!(code, "unknown" | "und" | "unk" | "mul" | "zxx")
}

fn infer_lang_from_text(text: &str) -> Option<String> {
    let mut has_hiragana = false;
    let mut has_katakana = false;
    let mut has_hangul = false;
    let mut has_cjk = false;
    let mut has_cyrillic = false;
    let mut has_arabic = false;
    let mut has_devanagari = false;
    let mut has_ascii_alpha = false;
    let mut has_non_ascii_alpha = false;

    for ch in text.chars() {
        let code = ch as u32;
        if (0x3040..=0x309F).contains(&code) {
            has_hiragana = true;
        } else if (0x30A0..=0x30FF).contains(&code) {
            has_katakana = true;
        } else if (0xAC00..=0xD7AF).contains(&code) {
            has_hangul = true;
        } else if (0x4E00..=0x9FFF).contains(&code) {
            has_cjk = true;
        } else if (0x0400..=0x04FF).contains(&code) {
            has_cyrillic = true;
        } else if (0x0600..=0x06FF).contains(&code) {
            has_arabic = true;
        } else if (0x0900..=0x097F).contains(&code) {
            has_devanagari = true;
        }
        if ch.is_ascii_alphabetic() {
            has_ascii_alpha = true;
        } else if ch.is_alphabetic() {
            has_non_ascii_alpha = true;
        }
    }

    if has_hiragana || has_katakana {
        return Some("jpn".to_string());
    }
    if has_hangul {
        return Some("kor".to_string());
    }
    if has_cjk {
        return Some("zho".to_string());
    }
    if has_cyrillic {
        return Some("rus".to_string());
    }
    if has_arabic {
        return Some("ara".to_string());
    }
    if has_devanagari {
        return Some("hin".to_string());
    }
    if has_ascii_alpha && !has_non_ascii_alpha {
        return Some("eng".to_string());
    }
    None
}

fn normalize_pack_code(code: &str) -> Option<String> {
    let code = code.trim().to_lowercase();
    if code.is_empty() {
        return None;
    }
    if code.len() == 3 {
        return Some(code);
    }
    let mapped = match code.as_str() {
        "ja" => "jpn",
        "en" => "eng",
        "zh" => "zho",
        "ko" => "kor",
        "fr" => "fra",
        "de" => "deu",
        "es" => "spa",
        "it" => "ita",
        "pt" => "por",
        "ru" => "rus",
        "nl" => "nld",
        "sv" => "swe",
        "no" => "nor",
        "da" => "dan",
        "fi" => "fin",
        "el" => "ell",
        "he" => "heb",
        "tr" => "tur",
        "uk" => "ukr",
        "pl" => "pol",
        "cs" => "ces",
        "hu" => "hun",
        "ro" => "ron",
        "bg" => "bul",
        "sr" => "srp",
        "hr" => "hrv",
        "sk" => "slk",
        "sl" => "slv",
        "ca" => "cat",
        "eu" => "eus",
        "gl" => "glg",
        "is" => "isl",
        "ga" => "gle",
        "cy" => "cym",
        "af" => "afr",
        "am" => "amh",
        "bn" => "ben",
        "ta" => "tam",
        "te" => "tel",
        "mr" => "mar",
        "ml" => "mal",
        "gu" => "guj",
        "kn" => "kan",
        "pa" => "pan",
        "yo" => "yor",
        "vi" => "vie",
        "id" => "ind",
        "th" => "tha",
        "ar" => "ara",
        "hi" => "hin",
        _ => return None,
    };
    Some(mapped.to_string())
}

fn unix_to_date(value: &str) -> String {
    let Ok(secs) = value.parse::<i64>() else {
        return "unknown".to_string();
    };
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) else {
        return "unknown".to_string();
    };
    let format = format_description::parse("[year]-[month]-[day]");
    if let Ok(format) = format {
        if let Ok(rendered) = dt.format(&format) {
            return rendered;
        }
    }
    dt.date().to_string()
}

fn classify_type(entry: &HistoryEntry, labels: &ReportLabels) -> String {
    if matches!(entry.kind, HistoryType::Text) {
        return labels.type_text.clone();
    }
    let mime = entry.mime.trim().to_lowercase();
    if mime.starts_with("audio/") {
        return labels.type_audio.clone();
    }
    if mime.starts_with("image/") {
        return labels.type_image.clone();
    }
    if mime.starts_with("video/") {
        return labels.type_video.clone();
    }
    if mime.starts_with("text/") {
        return labels.type_text.clone();
    }
    if mime.contains("pdf")
        || mime.contains("word")
        || mime.contains("excel")
        || mime.contains("powerpoint")
        || mime.contains("officedocument")
        || mime.contains("presentation")
        || mime.contains("spreadsheet")
    {
        return labels.type_document.clone();
    }
    labels.type_other.clone()
}

fn render_xml(report: &ReportData) -> Result<String> {
    render_template("report.xml.tera", report, false)
}

fn render_html(report: &ReportData) -> Result<String> {
    render_template("report.html.tera", report, true)
}

fn render_template(name: &str, report: &ReportData, autoescape: bool) -> Result<String> {
    let template = load_report_template(name)?;
    let mut context = tera::Context::new();
    context.insert("report", report);
    context.insert("labels", &report.labels);
    tera::Tera::one_off(&template, &context, autoescape)
        .with_context(|| format!("failed to render report template: {}", name))
}

fn load_report_template(name: &str) -> Result<String> {
    let path = report_template_path(name)?;
    std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read report template: {}", path.display()))
}

fn report_template_path(name: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("translations")
        .join("templates")
        .join(name))
}
