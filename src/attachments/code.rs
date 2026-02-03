use anyhow::{Context, Result};

use crate::data;
use crate::providers::Provider;
use crate::{TranslateOptions, Translator};

use super::cache::TranslationCache;
use super::util::should_translate_text;
use super::AttachmentTranslation;

pub(crate) async fn translate_javascript<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    translate_script(bytes, data::JS_MIME, with_commentout, translator, options).await
}

pub(crate) async fn translate_typescript<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    translate_script(bytes, data::TS_MIME, with_commentout, translator, options).await
}

pub(crate) async fn translate_tsx<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let input = std::str::from_utf8(bytes).with_context(|| "failed to decode tsx as UTF-8")?;
    let mut cache = TranslationCache::new();
    let script = translate_script_text(input, with_commentout, &mut cache, translator, options)
        .await?;
    let output = translate_jsx_text(&script, &mut cache, translator, options).await?;
    Ok(cache.finish(data::TSX_MIME.to_string(), output.into_bytes()))
}

pub(crate) async fn translate_mermaid<P: Provider + Clone>(
    bytes: &[u8],
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let input =
        std::str::from_utf8(bytes).with_context(|| "failed to decode mermaid as UTF-8")?;
    let mut cache = TranslationCache::new();
    let mut out_lines = Vec::new();
    for line in input.split('\n') {
        if line.trim_start().starts_with("%%") {
            if with_commentout {
                out_lines.push(
                    translate_mermaid_comment(line, &mut cache, translator, options).await?,
                );
            } else {
                out_lines.push(line.to_string());
            }
        } else {
            out_lines.push(
                translate_mermaid_line(line, &mut cache, translator, options).await?,
            );
        }
    }
    let output = out_lines.join("\n");
    Ok(cache.finish(data::MERMAID_MIME.to_string(), output.into_bytes()))
}

async fn translate_script<P: Provider + Clone>(
    bytes: &[u8],
    mime: &str,
    with_commentout: bool,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    let input = std::str::from_utf8(bytes).with_context(|| "failed to decode script as UTF-8")?;
    let mut cache = TranslationCache::new();
    let output = translate_script_text(input, with_commentout, &mut cache, translator, options)
        .await?;
    Ok(cache.finish(mime.to_string(), output.into_bytes()))
}

async fn translate_script_text<P: Provider + Clone>(
    input: &str,
    with_commentout: bool,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut out = String::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = input[i..].chars().next().unwrap();
        if ch == '/' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'/' {
                let end = find_line_end(bytes, i + 2);
                if with_commentout {
                    let comment = &input[i + 2..end];
                    let translated =
                        translate_line_comment(comment, cache, translator, options).await?;
                    out.push_str("//");
                    out.push_str(&translated);
                } else {
                    out.push_str(&input[i..end]);
                }
                i = end;
                continue;
            }
            if next == b'*' {
                let end = find_block_comment_end(input, i + 2);
                if with_commentout {
                    let content = &input[i + 2..end];
                    let translated =
                        translate_block_comment(content, cache, translator, options).await?;
                    out.push_str("/*");
                    out.push_str(&translated);
                    if end + 2 <= input.len() {
                        out.push_str("*/");
                    }
                } else {
                    let close = if end + 2 <= input.len() { end + 2 } else { input.len() };
                    out.push_str(&input[i..close]);
                }
                i = if end + 2 <= input.len() { end + 2 } else { input.len() };
                continue;
            }
        }

        if ch == '\'' || ch == '"' {
            let quote = ch;
            let (end, _raw, unescaped) = parse_string_literal(input, i, quote);
            if should_translate_text(&unescaped) {
                let translated = cache.translate(&unescaped, translator, options).await?;
                let escaped = escape_js_string(&translated, quote);
                out.push(quote);
                out.push_str(&escaped);
                out.push(quote);
            } else {
                out.push_str(&input[i..end]);
            }
            i = end;
            continue;
        }

        if ch == '`' {
            let (end, _raw, unescaped, has_expr) = parse_template_literal(input, i);
            if has_expr || !should_translate_text(&unescaped) {
                out.push_str(&input[i..end]);
            } else {
                let translated = cache.translate(&unescaped, translator, options).await?;
                let escaped = escape_template_literal(&translated);
                out.push('`');
                out.push_str(&escaped);
                out.push('`');
            }
            i = end;
            continue;
        }

        out.push(ch);
        i += ch.len_utf8();
    }
    Ok(out)
}

async fn translate_jsx_text<P: Provider + Clone>(
    input: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut out = String::new();
    let mut i = 0usize;
    let mut prev_non_ws: Option<char> = None;
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        let next = input[i + ch.len_utf8()..].chars().next();
        if ch == '<' && looks_like_jsx_tag_start(prev_non_ws, next) {
            let end = find_jsx_tag_end(input, i + ch.len_utf8());
            out.push_str(&input[i..end]);
            i = end;
            prev_non_ws = Some('>');

            let text_start = i;
            let mut j = i;
            while j < input.len() {
                let c = input[j..].chars().next().unwrap();
                let next_c = input[j + c.len_utf8()..].chars().next();
                if c == '<' && looks_like_jsx_tag_start(prev_non_ws, next_c) {
                    break;
                }
                j += c.len_utf8();
            }
            let text = &input[text_start..j];
            if text.contains('{') || text.contains('}') || !should_translate_text(text) {
                out.push_str(text);
            } else {
                let translated =
                    cache.translate_preserve_whitespace(text, translator, options).await?;
                out.push_str(&translated);
            }
            prev_non_ws = last_non_whitespace(text).or(prev_non_ws);
            i = j;
            continue;
        }

        out.push(ch);
        if !ch.is_whitespace() {
            prev_non_ws = Some(ch);
        }
        i += ch.len_utf8();
    }
    Ok(out)
}

fn looks_like_jsx_tag_start(prev: Option<char>, next: Option<char>) -> bool {
    let Some(next) = next else { return false };
    let is_tag_start = next.is_ascii_alphabetic() || matches!(next, '/' | '>' | '!');
    if !is_tag_start {
        return false;
    }
    match prev {
        None => true,
        Some(ch) => matches!(ch, '(' | '{' | '[' | '=' | ':' | ',' | '?' | '!' | ';' | '>'),
    }
}

fn last_non_whitespace(text: &str) -> Option<char> {
    text.chars().rev().find(|ch| !ch.is_whitespace())
}

fn find_jsx_tag_end(input: &str, mut i: usize) -> usize {
    let mut in_single = false;
    let mut in_double = false;
    let mut brace_depth = 0usize;
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        if in_single {
            if ch == '\\' {
                i += ch.len_utf8();
                if i < input.len() {
                    i += input[i..].chars().next().unwrap().len_utf8();
                }
                continue;
            }
            if ch == '\'' {
                in_single = false;
            }
            i += ch.len_utf8();
            continue;
        }
        if in_double {
            if ch == '\\' {
                i += ch.len_utf8();
                if i < input.len() {
                    i += input[i..].chars().next().unwrap().len_utf8();
                }
                continue;
            }
            if ch == '"' {
                in_double = false;
            }
            i += ch.len_utf8();
            continue;
        }
        if brace_depth > 0 {
            if ch == '{' {
                brace_depth += 1;
            } else if ch == '}' {
                brace_depth = brace_depth.saturating_sub(1);
            }
            i += ch.len_utf8();
            continue;
        }

        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            '{' => brace_depth = 1,
            '>' => {
                i += ch.len_utf8();
                break;
            }
            _ => {}
        }
        i += ch.len_utf8();
    }
    i
}

fn find_line_end(bytes: &[u8], start: usize) -> usize {
    let mut idx = start;
    while idx < bytes.len() {
        if bytes[idx] == b'\n' {
            break;
        }
        idx += 1;
    }
    idx
}

fn find_block_comment_end(input: &str, start: usize) -> usize {
    if let Some(pos) = input[start..].find("*/") {
        start + pos
    } else {
        input.len()
    }
}

async fn translate_line_comment<P: Provider + Clone>(
    comment: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut idx = 0usize;
    for ch in comment.chars() {
        if ch.is_whitespace() {
            idx += ch.len_utf8();
        } else {
            break;
        }
    }
    let (prefix, body) = comment.split_at(idx);
    if !should_translate_text(body) {
        return Ok(comment.to_string());
    }
    let translated = cache.translate_preserve_whitespace(body, translator, options).await?;
    Ok(format!("{}{}", prefix, translated))
}

async fn translate_block_comment<P: Provider + Clone>(
    comment: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut out_lines = Vec::new();
    for line in comment.split('\n') {
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        let mut prefix = line[..indent_len].to_string();
        let mut body = trimmed;
        if let Some(rest) = trimmed.strip_prefix('*') {
            prefix.push('*');
            body = rest;
            if let Some(rest) = body.strip_prefix(' ') {
                prefix.push(' ');
                body = rest;
            }
        }
        if should_translate_text(body) {
            let translated = cache.translate_preserve_whitespace(body, translator, options).await?;
            out_lines.push(format!("{}{}", prefix, translated));
        } else {
            out_lines.push(line.to_string());
        }
    }
    Ok(out_lines.join("\n"))
}

fn parse_string_literal(input: &str, start: usize, quote: char) -> (usize, String, String) {
    let mut i = start + quote.len_utf8();
    let mut raw = String::new();
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        if ch == '\\' {
            raw.push(ch);
            i += ch.len_utf8();
            if i < input.len() {
                let next = input[i..].chars().next().unwrap();
                raw.push(next);
                i += next.len_utf8();
            }
            continue;
        }
        if ch == quote {
            i += ch.len_utf8();
            break;
        }
        raw.push(ch);
        i += ch.len_utf8();
    }
    let unescaped = unescape_js_string(&raw);
    (i, raw, unescaped)
}

fn parse_template_literal(input: &str, start: usize) -> (usize, String, String, bool) {
    let mut i = start + 1;
    let mut raw = String::new();
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        if ch == '\\' {
            raw.push(ch);
            i += ch.len_utf8();
            if i < input.len() {
                let next = input[i..].chars().next().unwrap();
                raw.push(next);
                i += next.len_utf8();
            }
            continue;
        }
        if ch == '$' {
            let next = input[i + ch.len_utf8()..].chars().next();
            if next == Some('{') {
                let end = find_template_end(input, start + 1);
                return (end, String::new(), String::new(), true);
            }
        }
        if ch == '`' {
            i += ch.len_utf8();
            break;
        }
        raw.push(ch);
        i += ch.len_utf8();
    }
    let unescaped = unescape_js_string(&raw);
    (i, raw, unescaped, false)
}

fn find_template_end(input: &str, start: usize) -> usize {
    let mut i = start;
    let mut escaped = false;
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        if escaped {
            escaped = false;
            i += ch.len_utf8();
            continue;
        }
        if ch == '\\' {
            escaped = true;
            i += ch.len_utf8();
            continue;
        }
        if ch == '`' {
            i += ch.len_utf8();
            return i;
        }
        i += ch.len_utf8();
    }
    input.len()
}

fn unescape_js_string(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        match next {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'v' => out.push('\u{000B}'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            '`' => out.push('`'),
            'x' => {
                let hi = chars.next();
                let lo = chars.next();
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    let hex = format!("{}{}", hi, lo);
                    if let Ok(value) = u8::from_str_radix(&hex, 16) {
                        out.push(value as char);
                    } else {
                        out.push('\\');
                        out.push('x');
                        out.push(hi);
                        out.push(lo);
                    }
                } else {
                    out.push('\\');
                    out.push('x');
                }
            }
            'u' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    let mut hex = String::new();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '}' {
                            break;
                        }
                        hex.push(c);
                    }
                    if let Ok(value) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(value) {
                            out.push(ch);
                        }
                    }
                } else {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(c) = chars.next() {
                            hex.push(c);
                        }
                    }
                    if let Ok(value) = u16::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(value as u32) {
                            out.push(ch);
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn escape_js_string(input: &str, quote: char) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\u{000B}' => out.push_str("\\v"),
            '\0' => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn escape_template_literal(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' if chars.peek() == Some(&'{') => {
                out.push_str("\\$");
            }
            _ => out.push(ch),
        }
    }
    out
}

async fn translate_mermaid_comment<P: Provider + Clone>(
    line: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    let text = trimmed.strip_prefix("%%").unwrap_or(trimmed);
    if !should_translate_text(text) {
        return Ok(line.to_string());
    }
    let translated = cache.translate_preserve_whitespace(text, translator, options).await?;
    Ok(format!("{}%%{}", indent, translated))
}

async fn translate_mermaid_line<P: Provider + Clone>(
    line: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<String> {
    let mut out = String::new();
    let mut i = 0usize;
    while i < line.len() {
        let ch = line[i..].chars().next().unwrap();
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let (end, _raw, unescaped) = parse_string_literal(line, i, quote);
            if should_translate_text(&unescaped) {
                let translated = cache.translate(&unescaped, translator, options).await?;
                let escaped = escape_js_string(&translated, quote);
                out.push(quote);
                out.push_str(&escaped);
                out.push(quote);
            } else {
                out.push_str(&line[i..end]);
            }
            i = end;
            continue;
        }
        if ch == '|' {
            if let Some(end) = line[i + 1..].find('|') {
                let end_idx = i + 1 + end;
                let content = &line[i + 1..end_idx];
                if should_translate_text(content) {
                    let translated = cache.translate_preserve_whitespace(content, translator, options).await?;
                    out.push('|');
                    out.push_str(&translated);
                    out.push('|');
                } else {
                    out.push_str(&line[i..=end_idx]);
                }
                i = end_idx + 1;
                continue;
            }
        }
        if matches!(ch, '[' | '(' | '{') {
            if let Some((end_idx, inner, open_len)) = find_mermaid_bracket(line, i, ch) {
                if let Some(translated) =
                    translate_mermaid_bracket_text(&inner, cache, translator, options).await?
                {
                    out.push_str(&line[i..i + open_len]);
                    out.push_str(&translated);
                    out.push_str(&line[end_idx - open_len..end_idx]);
                } else {
                    out.push_str(&line[i..end_idx]);
                }
                i = end_idx;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    Ok(out)
}

fn find_mermaid_bracket(line: &str, start: usize, open: char) -> Option<(usize, String, usize)> {
    let (open_len, close_seq) = match open {
        '[' => {
            if line[start..].starts_with("[[") {
                (2, "]]")
            } else {
                (1, "]")
            }
        }
        '(' => {
            if line[start..].starts_with("((") {
                (2, "))")
            } else {
                (1, ")")
            }
        }
        '{' => {
            if line[start..].starts_with("{{") {
                (2, "}}")
            } else {
                (1, "}")
            }
        }
        _ => return None,
    };
    let inner_start = start + open_len;
    if let Some(pos) = line[inner_start..].find(close_seq) {
        let end = inner_start + pos + close_seq.len();
        let inner = line[inner_start..inner_start + pos].to_string();
        return Some((end, inner, open_len));
    }
    None
}

async fn translate_mermaid_bracket_text<P: Provider + Clone>(
    inner: &str,
    cache: &mut TranslationCache,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<Option<String>> {
    let trimmed = inner.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next().unwrap();
        let last = trimmed.chars().last().unwrap();
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            let body = &trimmed[first.len_utf8()..trimmed.len() - last.len_utf8()];
            let unescaped = unescape_js_string(body);
            if !should_translate_text(&unescaped) {
                return Ok(None);
            }
            let translated = cache.translate(&unescaped, translator, options).await?;
            let escaped = escape_js_string(&translated, first);
            let rendered = format!("{}{}{}", first, escaped, last);
            return Ok(Some(rendered));
        }
    }

    if !should_translate_text(inner) {
        return Ok(None);
    }
    let translated = cache
        .translate_preserve_whitespace(inner, translator, options)
        .await?;
    Ok(Some(translated))
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
    async fn translate_javascript_string_literal() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let input = r#"const msg = "Hello";"#;
        let output = translate_javascript(input.as_bytes(), false, &translator, &options)
            .await
            .expect("translate js");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert!(out_str.contains("\"T:Hello\""));
    }

    #[tokio::test]
    async fn translate_javascript_comment() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let input = "// Hello";
        let output = translate_javascript(input.as_bytes(), true, &translator, &options)
            .await
            .expect("translate js comment");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert_eq!(out_str, "// T:Hello");
    }

    #[tokio::test]
    async fn translate_javascript_template_literal_with_expr_is_untouched() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let input = "const msg = `Hello ${name}`;";
        let output = translate_javascript(input.as_bytes(), false, &translator, &options)
            .await
            .expect("translate js template");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert_eq!(out_str, input);
    }

    #[tokio::test]
    async fn translate_tsx_jsx_text() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let input = "<div>Hello</div>";
        let output = translate_tsx(input.as_bytes(), false, &translator, &options)
            .await
            .expect("translate tsx");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert!(out_str.contains("T:Hello"));
    }

    #[tokio::test]
    async fn translate_mermaid_quoted_text() {
        let options = TranslateOptions {
            lang: "ja".to_string(),
            formality: "formal".to_string(),
            source_lang: "en".to_string(),
            slang: false,
        };
        let translator = build_translator(&options);
        let input = r#"graph TD; A["Hello"] --> B["World"]"#;
        let output = translate_mermaid(input.as_bytes(), false, &translator, &options)
            .await
            .expect("translate mermaid");
        let out_str = std::str::from_utf8(&output.bytes).expect("utf8");
        assert!(out_str.contains("T:Hello"));
        assert!(out_str.contains("T:World"));
    }
}
