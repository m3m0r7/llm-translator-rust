use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

mod tools;

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
    let path = language_pack_path(code)?;
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read language pack: {}", path.display()))?;
    let parsed: LanguagePackFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse language pack: {}", path.display()))?;

    let mut iso_country_lang = HashMap::new();
    let mut parts_of_speech = HashMap::new();
    let mut report_labels = HashMap::new();
    if let Some(translate) = parsed.translate {
        if let Some(map) = translate.iso_country_lang {
            if let Some(entries) = map.get(&code.to_lowercase()) {
                iso_country_lang.extend(entries.iter().map(|(k, v)| (k.to_lowercase(), v.clone())));
            }
        }
        if let Some(map) = translate.parts_of_speech {
            if let Some(entries) = map.get(&code.to_lowercase()) {
                parts_of_speech.extend(entries.iter().map(|(k, v)| (k.to_string(), v.clone())));
            }
        }
        if let Some(map) = translate.report_labels {
            if let Some(entries) = map.get(&code.to_lowercase()) {
                report_labels.extend(entries.iter().map(|(k, v)| (k.to_string(), v.clone())));
            }
        }
    }

    Ok(LanguagePack {
        iso_country_lang,
        parts_of_speech,
        report_labels,
    })
}

fn language_pack_path(code: &str) -> Result<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("languages")
        .join(format!("{}.toml", code.to_lowercase()));
    if !base.exists() {
        return Err(anyhow!("language pack not found: {}", base.display()));
    }
    Ok(base)
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
}
