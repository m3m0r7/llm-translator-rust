use anyhow::Result;
use std::collections::HashMap;

use crate::providers::{Provider, ProviderUsage};
use crate::{TranslateOptions, Translator};

use super::AttachmentTranslation;
use super::util::{collapse_whitespace, is_numeric_like, sanitize_ocr_text, split_text_bounds};

pub(crate) struct TranslationCache {
    map: HashMap<String, String>,
    model: Option<String>,
    usage: ProviderUsage,
    used: bool,
}

impl TranslationCache {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
            model: None,
            usage: ProviderUsage {
                prompt_tokens: Some(0),
                completion_tokens: Some(0),
                total_tokens: Some(0),
            },
            used: false,
        }
    }

    pub(crate) fn record_usage(&mut self, model: Option<String>, usage: Option<ProviderUsage>) {
        if self.model.is_none() {
            self.model = model;
        }
        if let Some(usage) = usage {
            self.usage = merge_usage(self.usage.clone(), Some(usage));
            self.used = true;
        }
    }

    pub(crate) async fn translate_preserve_whitespace<P: Provider + Clone>(
        &mut self,
        text: &str,
        translator: &Translator<P>,
        options: &TranslateOptions,
    ) -> Result<String> {
        let Some((start, end)) = split_text_bounds(text) else {
            return Ok(text.to_string());
        };
        let leading = &text[..start];
        let core = &text[start..end];
        let trailing = &text[end..];
        let translated = self.translate(core, translator, options).await?;
        Ok(format!("{}{}{}", leading, translated, trailing))
    }

    pub(crate) async fn translate_ocr_line<P: Provider + Clone>(
        &mut self,
        text: &str,
        translator: &Translator<P>,
        options: &TranslateOptions,
    ) -> Result<String> {
        let cleaned = collapse_whitespace(text);
        let cleaned = sanitize_ocr_text(&cleaned);
        if cleaned.trim().is_empty() {
            return Ok(text.to_string());
        }
        if is_numeric_like(cleaned.trim()) {
            return Ok(cleaned.trim().to_string());
        }
        if cleaned.trim().chars().count() <= 1 {
            return Ok(cleaned.trim().to_string());
        }
        self.translate(cleaned.trim(), translator, options).await
    }

    pub(crate) async fn translate<P: Provider + Clone>(
        &mut self,
        text: &str,
        translator: &Translator<P>,
        options: &TranslateOptions,
    ) -> Result<String> {
        if let Some(existing) = self.map.get(text) {
            return Ok(existing.clone());
        }
        let exec = translator.exec(text, options.clone()).await?;
        if self.model.is_none() {
            self.model = exec.model.clone();
        }
        self.usage = merge_usage(self.usage.clone(), exec.usage);
        self.used = true;
        self.map.insert(text.to_string(), exec.text.clone());
        Ok(exec.text)
    }

    pub(crate) fn finish(self, mime: String, bytes: Vec<u8>) -> AttachmentTranslation {
        AttachmentTranslation {
            bytes,
            mime,
            model: self.model,
            usage: if self.used { Some(self.usage) } else { None },
        }
    }
}

pub(crate) fn merge_usage(total: ProviderUsage, next: Option<ProviderUsage>) -> ProviderUsage {
    let Some(next) = next else {
        return total;
    };
    ProviderUsage {
        prompt_tokens: Some(total.prompt_tokens.unwrap_or(0) + next.prompt_tokens.unwrap_or(0)),
        completion_tokens: Some(
            total.completion_tokens.unwrap_or(0) + next.completion_tokens.unwrap_or(0),
        ),
        total_tokens: Some(total.total_tokens.unwrap_or(0) + next.total_tokens.unwrap_or(0)),
    }
}
