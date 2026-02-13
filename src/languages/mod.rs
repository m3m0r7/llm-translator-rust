use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;

mod tools;
mod generated {
    include!(concat!(env!("OUT_DIR"), "/embedded_language_packs.rs"));
}

pub use tools::{map_lang_for_espeak, map_lang_for_whisper};

#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    codes: HashMap<String, String>,
}

impl LanguageRegistry {
    pub fn load() -> Result<Self> {
        let raw = include_str!("iso_639.json");
        let parsed: IsoData =
            serde_json::from_str(raw).with_context(|| "failed to parse ISO 639 language data")?;
        Ok(LanguageRegistry {
            codes: parsed.codes,
        })
    }

    pub fn is_valid_code(&self, code: &str) -> bool {
        let code = normalize_code(code);
        matches!(code.len(), 2 | 3) && self.codes.contains_key(&code)
    }

    pub fn iso_name(&self, code: &str) -> Option<String> {
        let code = normalize_code(code);
        self.codes.get(&code).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct LanguagePack {
    pub iso_country_lang: HashMap<String, String>,
    pub parts_of_speech: HashMap<String, String>,
    pub report_labels: HashMap<String, String>,
    pub client_labels: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct LanguagePacks {
    pub primary: Option<String>,
    pub packs: HashMap<String, LanguagePack>,
}

impl LanguagePacks {
    pub fn primary_pack(&self) -> Option<&LanguagePack> {
        self.primary.as_ref().and_then(|code| self.packs.get(code))
    }
}

pub fn load_language_packs(codes: &[String]) -> Result<LanguagePacks> {
    let mut packs = HashMap::new();
    for code in codes {
        let pack = load_language_pack(code)?;
        packs.insert(code.to_lowercase(), pack);
    }

    let primary = codes.first().map(|code| code.to_lowercase());

    Ok(LanguagePacks { primary, packs })
}

fn load_language_pack(code: &str) -> Result<LanguagePack> {
    let code_norm = code.to_lowercase();
    let content = generated::embedded_language_pack(&code_norm)
        .ok_or_else(|| anyhow!("language pack not found: {}.toml", code_norm))?;
    let parsed: LanguagePackFile = toml::from_str(content)
        .with_context(|| format!("failed to parse language pack: {}.toml", code_norm))?;

    let mut iso_country_lang = HashMap::new();
    let mut parts_of_speech = HashMap::new();
    let mut report_labels = HashMap::new();
    let mut client_labels = HashMap::new();
    if let Some(translate) = parsed.translate {
        if let Some(map) = translate.iso_country_lang
            && let Some(entries) = map.get(&code_norm)
        {
            iso_country_lang.extend(entries.iter().map(|(k, v)| (k.to_lowercase(), v.clone())));
        }
        if let Some(map) = translate.parts_of_speech
            && let Some(entries) = map.get(&code_norm)
        {
            parts_of_speech.extend(entries.iter().map(|(k, v)| (k.to_string(), v.clone())));
        }
        if let Some(map) = translate.report_labels
            && let Some(entries) = map.get(&code_norm)
        {
            report_labels.extend(entries.iter().map(|(k, v)| (k.to_string(), v.clone())));
        }
        if let Some(map) = translate.client_labels
            && let Some(entries) = map.get(&code_norm)
        {
            client_labels.extend(entries.iter().map(|(k, v)| (k.to_string(), v.clone())));
        }
    }

    Ok(LanguagePack {
        iso_country_lang,
        parts_of_speech,
        report_labels,
        client_labels,
    })
}

pub fn load_client_labels(code: &str) -> HashMap<String, String> {
    let mut labels = HashMap::new();
    if let Ok(english) = load_language_pack("eng") {
        labels.extend(english.client_labels);
    }
    if let Some(pack) = normalize_pack_code(code)
        && let Ok(selected) = load_language_pack(&pack)
    {
        labels.extend(selected.client_labels);
    }
    labels
}

pub fn language_autonym(code: &str) -> Option<String> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let pack_code = normalize_pack_code(&lower).unwrap_or_else(|| lower.clone());
    let pack = load_language_pack(&pack_code).ok()?;
    if let Some(label) = pack.iso_country_lang.get(&lower) {
        return Some(label.clone());
    }
    if let Some(label) = pack.iso_country_lang.get(&pack_code) {
        return Some(label.clone());
    }
    None
}

pub fn normalize_pack_code(code: &str) -> Option<String> {
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

fn normalize_code(code: &str) -> String {
    code.trim().to_lowercase()
}

#[derive(Debug, Deserialize)]
struct IsoData {
    codes: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct LanguagePackFile {
    translate: Option<TranslateSection>,
}

#[derive(Debug, Deserialize, Clone)]
struct TranslateSection {
    iso_country_lang: Option<HashMap<String, HashMap<String, String>>>,
    parts_of_speech: Option<HashMap<String, HashMap<String, String>>>,
    report_labels: Option<HashMap<String, HashMap<String, String>>>,
    client_labels: Option<HashMap<String, HashMap<String, String>>>,
}
