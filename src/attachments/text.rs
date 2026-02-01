use crate::data;
use crate::providers::Provider;
use crate::{TranslateOptions, Translator};
use anyhow::{anyhow, Context, Result};
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use quick_xml::events::{BytesCData, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::io::Cursor;

use super::cache::TranslationCache;
use super::util::{is_numeric_like, looks_like_code, should_translate_text, split_text_bounds};
use super::AttachmentTranslation;
pub(crate) async fn translate_html<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    use kuchiki::traits::*;

    let html = std::str::from_utf8(bytes).with_context(|| "failed to decode html as UTF-8")?;
    let document = kuchiki::parse_html().one(html);
    let mut cache = TranslationCache::new();

    translate_html_document(&document, with_commentout, translator, options, &mut cache).await?;

    let output = document.to_string();
    Ok(cache.finish(data::HTML_MIME.to_string(), output.into_bytes()))
}

pub(crate) async fn translate_markdown<P: Provider + Clone>(
    bytes: &[u8],
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let markdown =
        std::str::from_utf8(bytes).with_context(|| "failed to decode markdown as UTF-8")?;
    let mut cache = TranslationCache::new();
    let output = translate_markdown_text(markdown, &mut cache, translator, options).await?;
    Ok(cache.finish(data::MARKDOWN_MIME.to_string(), output.into_bytes()))
}

pub(crate) async fn translate_xml<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let mut reader = Reader::from_reader(Cursor::new(bytes));
    reader.trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut cache = TranslationCache::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Text(text)) => {
                let decoded = text
                    .unescape()
                    .with_context(|| "failed to decode xml text")?;
                if should_translate_text(decoded.as_ref()) {
                    let translated = cache
                        .translate_preserve_whitespace(decoded.as_ref(), translator, options)
                        .await?;
                    writer
                        .write_event(Event::Text(BytesText::new(&translated)))
                        .with_context(|| "failed to write xml text")?;
                } else {
                    writer
                        .write_event(Event::Text(text.into_owned()))
                        .with_context(|| "failed to write xml text")?;
                }
            }
            Ok(Event::CData(cdata)) => {
                let mut wrote = false;
                if let Ok(text) = std::str::from_utf8(cdata.as_ref()) {
                    if should_translate_text(text) {
                        let translated = cache
                            .translate_preserve_whitespace(text, translator, options)
                            .await?;
                        writer
                            .write_event(Event::CData(BytesCData::new(&translated)))
                            .with_context(|| "failed to write xml cdata")?;
                        wrote = true;
                    }
                }
                if !wrote {
                    writer
                        .write_event(Event::CData(cdata.into_owned()))
                        .with_context(|| "failed to write xml cdata")?;
                }
            }
            Ok(Event::Comment(comment)) => {
                if with_commentout {
                    let decoded = comment
                        .unescape()
                        .with_context(|| "failed to decode xml comment")?;
                    if should_translate_text(decoded.as_ref()) {
                        let translated = cache
                            .translate_preserve_whitespace(decoded.as_ref(), translator, options)
                            .await?;
                        writer
                            .write_event(Event::Comment(BytesText::new(&translated)))
                            .with_context(|| "failed to write xml comment")?;
                        buf.clear();
                        continue;
                    }
                }
                writer
                    .write_event(Event::Comment(comment.into_owned()))
                    .with_context(|| "failed to write xml comment")?;
            }
            Ok(event) => {
                writer
                    .write_event(event.into_owned())
                    .with_context(|| "failed to write xml event")?;
            }
            Err(err) => return Err(anyhow!("failed to parse xml: {}", err)),
        }
        buf.clear();
    }

    Ok(cache.finish(data::XML_MIME.to_string(), writer.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::LanguageRegistry;
    use crate::providers::{Provider, ProviderFuture, ProviderResponse, ToolSpec};
    use crate::settings::Settings;
    use serde_json::json;

    #[derive(Clone)]
    struct TestProvider {
        last_user_input: Option<String>,
        source_lang: String,
        target_lang: String,
        style: String,
        slang: bool,
        prefix: String,
    }

    impl TestProvider {
        fn new(options: &TranslateOptions, prefix: &str) -> Self {
            Self {
                last_user_input: None,
                source_lang: options.source_lang.clone(),
                target_lang: options.lang.clone(),
                style: options.formality.clone(),
                slang: options.slang,
                prefix: prefix.to_string(),
            }
        }
    }

    impl Provider for TestProvider {
        fn append_system_input(self, _input: String) -> Self {
            self
        }

        fn append_user_input(mut self, input: String) -> Self {
            self.last_user_input = Some(input);
            self
        }

        fn append_user_data(self, _data: data::DataAttachment) -> Self {
            self
        }

        fn register_tool(self, _tool: ToolSpec) -> Self {
            self
        }

        fn call_tool(self, _tool_name: &str) -> ProviderFuture {
            let translation = self.last_user_input.unwrap_or_default();
            let args = json!({
                "translation": format!("{}{}", self.prefix, translation),
                "source_language": self.source_lang,
                "target_language": self.target_lang,
                "style": self.style,
                "slang": self.slang
            });
            let response = ProviderResponse {
                args,
                model: Some("test".to_string()),
                usage: None,
            };
            Box::pin(async move { Ok(response) })
        }
    }

    fn build_translator(options: &TranslateOptions) -> Translator<TestProvider> {
        let provider = TestProvider::new(options, "T:");
        let registry = LanguageRegistry::load().expect("registry");
        let mut settings = Settings::default();
        settings
            .formally
            .insert("formal".to_string(), "Use formal style.".to_string());
        Translator::new(provider, settings, registry)
    }

    #[tokio::test]
    async fn translate_xml_text_and_comment() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let xml = br#"<root>Hello<!--Note--></root>"#;
        let output = translate_xml(xml, true, &translator, &options)
            .await
            .expect("translate xml");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert!(out_str.contains("T:Hello"));
        assert!(out_str.contains("T:Note"));
        assert_eq!(output.mime, data::XML_MIME);
    }

    #[tokio::test]
    async fn translate_xml_skips_numeric_text() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let xml = br#"<root>12345</root>"#;
        let output = translate_xml(xml, false, &translator, &options)
            .await
            .expect("translate xml");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert!(out_str.contains("12345"));
        assert!(!out_str.contains("T:12345"));
    }
}

async fn translate_markdown_text<P: Provider + Clone>(
    markdown: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};

    let parser = Parser::new_ext(markdown, Options::all());
    let mut events = Vec::new();
    let mut code_block_depth = 0usize;

    for event in parser {
        match event {
            event @ Event::Start(Tag::CodeBlock(_)) => {
                code_block_depth = code_block_depth.saturating_add(1);
                events.push(event);
            }
            event @ Event::End(Tag::CodeBlock(_)) => {
                code_block_depth = code_block_depth.saturating_sub(1);
                events.push(event);
            }
            Event::Text(text) => {
                if code_block_depth == 0 && should_translate_text(text.as_ref()) {
                    let translated = cache
                        .translate_preserve_whitespace(text.as_ref(), translator, options)
                        .await?;
                    events.push(Event::Text(CowStr::from(translated)));
                } else {
                    events.push(Event::Text(text));
                }
            }
            event => events.push(event),
        }
    }

    let mut output = String::new();
    pulldown_cmark_to_cmark::cmark(events.into_iter(), &mut output)
        .with_context(|| "failed to render markdown")?;
    Ok(output)
}

async fn translate_html_document<P: Provider + Clone>(
    document: &kuchiki::NodeRef,
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
    cache: &mut TranslationCache,
) -> Result<()> {
    for node in document.descendants() {
        if should_skip_html_node(&node) {
            continue;
        }
        if let Some(text) = node.as_text() {
            let original = text.borrow().to_string();
            if should_translate_text(&original) {
                let translated = cache
                    .translate_preserve_whitespace(&original, translator, options)
                    .await?;
                if translated != original {
                    *text.borrow_mut() = translated;
                }
            }
        }
        if with_commentout {
            if let Some(comment) = node.as_comment() {
                let original = comment.borrow().to_string();
                if should_translate_text(&original) {
                    let translated = cache
                        .translate_preserve_whitespace(&original, translator, options)
                        .await?;
                    if translated != original {
                        *comment.borrow_mut() = translated;
                    }
                }
            }
        }
        if let Some(element) = node.as_element() {
            let name = element.name.local.as_ref();
            if is_html_skip_element(name) {
                continue;
            }

            let mut updates = Vec::new();
            {
                let attrs = element.attributes.borrow();
                for attr in html_translatable_attrs() {
                    if let Some(value) = attrs.get(*attr) {
                        if should_translate_text(value) {
                            updates.push((attr.to_string(), value.to_string()));
                        }
                    }
                }
            }

            for (attr, value) in updates {
                let translated = cache
                    .translate_preserve_whitespace(&value, translator, options)
                    .await?;
                if translated != value {
                    element.attributes.borrow_mut().insert(attr, translated);
                }
            }
        }
    }
    Ok(())
}

fn html_translatable_attrs() -> &'static [&'static str] {
    &[
        "title",
        "alt",
        "placeholder",
        "aria-label",
        "aria-description",
    ]
}

fn should_skip_html_node(node: &kuchiki::NodeRef) -> bool {
    node.ancestors().any(|ancestor| {
        if let Some(element) = ancestor.as_element() {
            return is_html_skip_element(element.name.local.as_ref());
        }
        false
    })
}

fn is_html_skip_element(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "noscript" | "code" | "pre" | "kbd" | "samp"
    )
}

pub(crate) async fn translate_json<P: Provider + Clone>(
    bytes: &[u8],
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).with_context(|| "failed to parse json")?;
    let mut cache = TranslationCache::new();
    let translated = translate_json_value(value, &mut cache, translator, options).await?;
    let output =
        serde_json::to_string_pretty(&translated).with_context(|| "failed to write json")?;
    Ok(cache.finish(data::JSON_MIME.to_string(), output.into_bytes()))
}

fn translate_json_value<'a, P: Provider + Clone + 'a>(
    value: serde_json::Value,
    cache: &'a mut TranslationCache,
    translator: &'a Translator<P>,
    options: &'a TranslateOptions,
) -> BoxFuture<'a, Result<serde_json::Value>> {
    async move {
        match value {
            serde_json::Value::String(text) => {
                if should_translate_text(&text) {
                    let translated = cache
                        .translate_preserve_whitespace(&text, translator, options)
                        .await?;
                    Ok(serde_json::Value::String(translated))
                } else {
                    Ok(serde_json::Value::String(text))
                }
            }
            serde_json::Value::Array(values) => {
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    out.push(translate_json_value(value, cache, translator, options).await?);
                }
                Ok(serde_json::Value::Array(out))
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (key, value) in map {
                    out.insert(
                        key,
                        translate_json_value(value, cache, translator, options).await?,
                    );
                }
                Ok(serde_json::Value::Object(out))
            }
            other => Ok(other),
        }
    }
    .boxed()
}

pub(crate) async fn translate_yaml<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let input = std::str::from_utf8(bytes).with_context(|| "failed to decode yaml as UTF-8")?;
    let lines: Vec<&str> = input.lines().collect();
    let mut cache = TranslationCache::new();
    let mut out_lines = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx];
        let (prefix, comment) = split_yaml_comment(line);
        if let Some(block) = detect_yaml_block_start(prefix.trim_end()) {
            let translated_line = translate_yaml_line(
                &prefix,
                comment.as_deref(),
                with_commentout,
                &mut cache,
                translator,
                options,
            )
            .await?;
            out_lines.push(translated_line);

            let mut block_indent = block.indent + 1;
            let mut lookahead = idx + 1;
            while lookahead < lines.len() {
                let next = lines[lookahead];
                if next.trim().is_empty() {
                    lookahead += 1;
                    continue;
                }
                let indent = leading_whitespace_len(next);
                if indent > block.indent {
                    block_indent = indent;
                }
                break;
            }

            let mut block_lines = Vec::new();
            let mut block_end = idx + 1;
            while block_end < lines.len() {
                let next = lines[block_end];
                if next.trim().is_empty() {
                    block_lines.push(String::new());
                    block_end += 1;
                    continue;
                }
                let indent = leading_whitespace_len(next);
                if indent < block_indent {
                    break;
                }
                let content = next.get(block_indent..).unwrap_or("").to_string();
                block_lines.push(content);
                block_end += 1;
            }

            if !block_lines.is_empty() {
                let block_text = block_lines.join("\n");
                let translated =
                    translate_markdown_text(&block_text, &mut cache, translator, options).await?;
                for line in translated.split('\n') {
                    out_lines.push(format!("{:indent$}{}", "", line, indent = block_indent));
                }
            }

            idx = block_end;
            continue;
        }

        let translated = translate_yaml_line(
            &prefix,
            comment.as_deref(),
            with_commentout,
            &mut cache,
            translator,
            options,
        )
        .await?;
        out_lines.push(translated);
        idx += 1;
    }

    let output = out_lines.join("\n");
    Ok(cache.finish(data::YAML_MIME.to_string(), output.into_bytes()))
}

async fn translate_yaml_line<P: Provider + Clone>(
    prefix: &str,
    comment: Option<&str>,
    with_commentout: bool,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut translated_prefix = prefix.to_string();
    if !prefix.trim().is_empty() {
        if let Some(idx) = find_yaml_key_sep(prefix) {
            let (head, value) = prefix.split_at(idx + 1);
            let translated = translate_yaml_value(value, cache, translator, options).await?;
            translated_prefix = format!("{}{}", head, translated);
        } else {
            let trimmed = prefix.trim_start();
            if trimmed.starts_with('-') {
                let dash_len = prefix.len() - trimmed.len();
                let mut offset = dash_len + 1;
                let rest = &prefix[offset..];
                let spaces = rest.chars().take_while(|ch| ch.is_whitespace()).count();
                offset += spaces;
                let value = &prefix[offset..];
                if !value.trim().is_empty() {
                    let translated =
                        translate_yaml_value(value, cache, translator, options).await?;
                    translated_prefix = format!("{}{}", &prefix[..offset], translated);
                }
            }
        }
    }

    if let Some(comment) = comment {
        let suffix = if with_commentout {
            translate_yaml_comment(comment, cache, translator, options).await?
        } else {
            comment.to_string()
        };
        translated_prefix.push_str(&suffix);
    }

    Ok(translated_prefix)
}

async fn translate_yaml_comment<P: Provider + Clone>(
    comment: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut idx = 0usize;
    let bytes = comment.as_bytes();
    if bytes.first() != Some(&b'#') {
        return Ok(comment.to_string());
    }
    idx += 1;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let prefix = &comment[..idx];
    let text = &comment[idx..];
    if !should_translate_text(text) {
        return Ok(comment.to_string());
    }
    let translated = cache
        .translate_preserve_whitespace(text, translator, options)
        .await?;
    Ok(format!("{}{}", prefix, translated))
}

async fn translate_yaml_value<P: Provider + Clone>(
    value: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let Some((start, end)) = split_text_bounds(value) else {
        return Ok(value.to_string());
    };
    let leading = &value[..start];
    let core = &value[start..end];
    let trailing = &value[end..];
    let translated = translate_yaml_scalar(core, cache, translator, options).await?;
    Ok(format!("{}{}{}", leading, translated, trailing))
}

async fn translate_yaml_scalar<P: Provider + Clone>(
    value: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(value.to_string());
    }
    if is_yaml_literal(trimmed) {
        return Ok(value.to_string());
    }
    if trimmed.starts_with(['|', '>', '&', '*', '!', '@']) {
        return Ok(value.to_string());
    }
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        return Ok(value.to_string());
    }
    if looks_like_code(trimmed) {
        return Ok(value.to_string());
    }

    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        if !should_translate_text(inner) {
            return Ok(value.to_string());
        }
        let unescaped = unescape_yaml_double(inner);
        let translated = cache.translate(&unescaped, translator, options).await?;
        let escaped = escape_yaml_double(&translated);
        return Ok(format!("\"{}\"", escaped));
    }
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        if !should_translate_text(inner) {
            return Ok(value.to_string());
        }
        let unescaped = inner.replace("''", "'");
        let translated = cache.translate(&unescaped, translator, options).await?;
        let escaped = translated.replace('\'', "''");
        return Ok(format!("'{}'", escaped));
    }

    if !should_translate_text(trimmed) {
        return Ok(value.to_string());
    }
    let mut translated = cache.translate(trimmed, translator, options).await?;
    if needs_yaml_quotes(&translated) {
        translated = format!("\"{}\"", escape_yaml_double(&translated));
    }
    Ok(translated)
}

fn split_yaml_comment(line: &str) -> (String, Option<String>) {
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for (idx, ch) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_double && ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if ch == '#' && !in_single && !in_double {
            return (line[..idx].to_string(), Some(line[idx..].to_string()));
        }
    }
    (line.to_string(), None)
}

fn detect_yaml_block_start(line: &str) -> Option<YamlBlockStart> {
    let trimmed = line.trim_end();
    let indent = leading_whitespace_len(trimmed);
    let content = trimmed.trim_start();
    if content.is_empty() {
        return None;
    }
    let key_sep = find_yaml_key_sep(content);
    let value_start = if let Some(idx) = key_sep {
        idx + 1
    } else if content.starts_with("- ") {
        1
    } else {
        return None;
    };
    let value = content.get(value_start..).unwrap_or("").trim();
    if value.starts_with('|') || value.starts_with('>') {
        return Some(YamlBlockStart { indent });
    }
    None
}

struct YamlBlockStart {
    indent: usize,
}

fn find_yaml_key_sep(line: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for (idx, ch) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_double && ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if ch == ':' && !in_single && !in_double {
            let next = line[idx + 1..].chars().next();
            if next.map(|value| value.is_whitespace()).unwrap_or(true) {
                return Some(idx);
            }
        }
    }
    None
}

fn is_yaml_literal(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if is_numeric_like(trimmed) {
        return true;
    }
    matches!(
        trimmed.to_lowercase().as_str(),
        "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
    )
}

fn needs_yaml_quotes(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.len() != value.len() {
        return true;
    }
    if trimmed.starts_with(['-', '?', ':', '!', '@', '&', '*', '#']) {
        return true;
    }
    if trimmed.contains(['#', ':', '\n', '\r', '\t']) {
        return true;
    }
    matches!(
        trimmed.to_lowercase().as_str(),
        "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
    )
}

fn escape_yaml_double(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_yaml_double(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            } else {
                out.push('\\');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn leading_whitespace_len(line: &str) -> usize {
    line.bytes()
        .take_while(|ch| ch.is_ascii_whitespace())
        .count()
}

pub(crate) async fn translate_po<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let input = std::str::from_utf8(bytes).with_context(|| "failed to decode po as UTF-8")?;
    let mut cache = TranslationCache::new();
    let mut out_lines = Vec::new();
    let mut entry = Vec::new();

    for line in input.lines() {
        if line.trim().is_empty() {
            if !entry.is_empty() {
                let translated =
                    translate_po_entry(&entry, with_commentout, &mut cache, translator, options)
                        .await?;
                out_lines.extend(translated);
                entry.clear();
            }
            out_lines.push(String::new());
        } else {
            entry.push(line.to_string());
        }
    }
    if !entry.is_empty() {
        let translated =
            translate_po_entry(&entry, with_commentout, &mut cache, translator, options).await?;
        out_lines.extend(translated);
    }

    let output = out_lines.join("\n");
    Ok(cache.finish(data::PO_MIME.to_string(), output.into_bytes()))
}

async fn translate_po_entry<P: Provider + Clone>(
    lines: &[String],
    with_commentout: bool,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Vec<String>> {
    let meta = parse_po_entry_meta(lines);
    if meta.msgid.trim().is_empty() {
        return translate_po_comments_only(lines, with_commentout, cache, translator, options)
            .await;
    }

    let mut output = Vec::new();
    let mut field = PoField::None;
    let mut insert_index = None;

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            if with_commentout {
                output.push(translate_po_comment_line(line, cache, translator, options).await?);
            } else {
                output.push(line.clone());
            }
            field = PoField::None;
            continue;
        }
        if trimmed.starts_with("msgid_plural") {
            output.push(line.clone());
            field = PoField::MsgIdPlural;
            insert_index = Some(output.len());
            continue;
        }
        if trimmed.starts_with("msgid") {
            output.push(line.clone());
            field = PoField::MsgId;
            insert_index = Some(output.len());
            continue;
        }
        if trimmed.starts_with("msgstr") {
            field = PoField::MsgStr;
            continue;
        }
        if trimmed.starts_with('"') {
            match field {
                PoField::MsgId | PoField::MsgIdPlural => {
                    output.push(line.clone());
                    insert_index = Some(output.len());
                }
                PoField::MsgStr => {}
                PoField::None => output.push(line.clone()),
            }
            continue;
        }
        field = PoField::None;
        output.push(line.clone());
    }

    let insert_at = insert_index.unwrap_or(output.len());
    let mut msgstr_lines = Vec::new();
    if let Some(plural) = meta.msgid_plural {
        let singular = if should_translate_text(&meta.msgid) {
            cache.translate(&meta.msgid, translator, options).await?
        } else {
            meta.msgid
        };
        let plural = if should_translate_text(&plural) {
            cache.translate(&plural, translator, options).await?
        } else {
            plural
        };
        msgstr_lines.push(format!("msgstr[0] \"{}\"", escape_po_string(&singular)));
        msgstr_lines.push(format!("msgstr[1] \"{}\"", escape_po_string(&plural)));
    } else {
        let translated = if should_translate_text(&meta.msgid) {
            cache.translate(&meta.msgid, translator, options).await?
        } else {
            meta.msgid
        };
        msgstr_lines.push(format!("msgstr \"{}\"", escape_po_string(&translated)));
    }
    output.splice(insert_at..insert_at, msgstr_lines);
    Ok(output)
}

async fn translate_po_comments_only<P: Provider + Clone>(
    lines: &[String],
    with_commentout: bool,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Vec<String>> {
    if !with_commentout {
        return Ok(lines.to_vec());
    }
    let mut output = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            output.push(translate_po_comment_line(line, cache, translator, options).await?);
        } else {
            output.push(line.clone());
        }
    }
    Ok(output)
}

async fn translate_po_comment_line<P: Provider + Clone>(
    line: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    if trimmed.starts_with("#:") || trimmed.starts_with("#,") || trimmed.starts_with("#|") {
        return Ok(line.to_string());
    }
    let (prefix, text) = split_po_comment(trimmed);
    if !should_translate_text(&text) {
        return Ok(line.to_string());
    }
    let translated = cache.translate(&text, translator, options).await?;
    Ok(format!("{}{}{}", indent, prefix, translated))
}

fn split_po_comment(comment: &str) -> (String, String) {
    let mut chars = comment.chars();
    let mut prefix = String::new();
    if let Some(first) = chars.next() {
        prefix.push(first);
    }
    if let Some(next) = chars.next() {
        if matches!(next, '.' | '~') {
            prefix.push(next);
        } else if next.is_whitespace() {
            prefix.push(' ');
            return (prefix, chars.collect());
        } else {
            let mut rest = String::new();
            rest.push(next);
            rest.push_str(&chars.collect::<String>());
            return (prefix, rest);
        }
    }
    let rest = chars.collect::<String>();
    if rest.starts_with(' ') {
        let mut chars = rest.chars();
        chars.next();
        prefix.push(' ');
        return (prefix, chars.collect());
    }
    (prefix, rest)
}

fn parse_po_entry_meta(lines: &[String]) -> PoEntryMeta {
    let mut msgid = String::new();
    let mut msgid_plural = None;
    let mut field = PoField::None;
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("msgid_plural") {
            field = PoField::MsgIdPlural;
            let value = parse_po_string_line(trimmed);
            msgid_plural = Some(value);
            continue;
        }
        if trimmed.starts_with("msgid") {
            field = PoField::MsgId;
            msgid = parse_po_string_line(trimmed);
            continue;
        }
        if trimmed.starts_with("msgstr") {
            field = PoField::MsgStr;
            continue;
        }
        if trimmed.starts_with('"') {
            let value = parse_po_quoted(trimmed);
            match field {
                PoField::MsgId => msgid.push_str(&value),
                PoField::MsgIdPlural => {
                    let current = msgid_plural.take().unwrap_or_default();
                    msgid_plural = Some(format!("{}{}", current, value));
                }
                _ => {}
            }
        } else {
            field = PoField::None;
        }
    }
    PoEntryMeta {
        msgid,
        msgid_plural,
    }
}

#[derive(Default)]
struct PoEntryMeta {
    msgid: String,
    msgid_plural: Option<String>,
}

#[derive(Clone, Copy)]
enum PoField {
    None,
    MsgId,
    MsgIdPlural,
    MsgStr,
}

fn parse_po_string_line(line: &str) -> String {
    if let Some(idx) = line.find('"') {
        parse_po_quoted(&line[idx..])
    } else {
        String::new()
    }
}

fn parse_po_quoted(line: &str) -> String {
    let mut chars = line.chars();
    if chars.next() != Some('"') {
        return String::new();
    }
    let mut out = String::new();
    let mut escape = false;
    for ch in chars {
        if escape {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    out
}

fn escape_po_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}
