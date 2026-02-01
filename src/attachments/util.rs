pub(crate) fn collapse_whitespace(value: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

pub(crate) fn sanitize_ocr_text(value: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    let mut last_punct = false;
    for ch in value.chars() {
        if ch.is_control() {
            continue;
        }
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            last_punct = false;
            continue;
        }
        if is_ignorable_symbol(ch) {
            if !last_punct {
                out.push(ch);
                last_punct = true;
            }
            last_space = false;
            continue;
        }
        out.push(ch);
        last_space = false;
        last_punct = false;
    }
    let mut trimmed = strip_cjk_adjacent_punct(out.trim());
    trimmed = trim_ocr_edges(trimmed.trim());
    let trimmed = trim_ocr_edges(trimmed.trim());
    trim_ascii_edges_for_cjk(&trimmed)
}

pub(crate) fn should_skip_ocr_annotation(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if is_numeric_like(trimmed) {
        return true;
    }
    if is_ascii_alnum_only(trimmed) {
        return true;
    }
    false
}

fn is_ignorable_symbol(ch: char) -> bool {
    matches!(ch, '|' | '¦' | '·' | '•' | '―' | '—' | '–' | '…')
}

fn strip_cjk_adjacent_punct(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, ch) in chars.iter().enumerate() {
        if is_noise_punct(*ch) {
            let prev = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(idx + 1).copied();
            let prev_cjk = prev.map(is_cjk).unwrap_or(false);
            let next_cjk = next.map(is_cjk).unwrap_or(false);
            let next_digit = next.map(|c| c.is_ascii_digit()).unwrap_or(false);
            if prev_cjk || next_cjk || next_digit {
                continue;
            }
        }
        out.push(*ch);
    }
    out
}

fn is_noise_punct(ch: char) -> bool {
    matches!(ch, '!' | '！' | '?' | '？' | '・' | '…')
}

pub(crate) fn should_filter_by_source_lang(source_lang: &str) -> bool {
    let lang = source_lang.trim().to_lowercase();
    if lang.is_empty() || lang == "auto" || lang == "und" || lang == "mul" {
        return false;
    }
    lang == "ja"
        || lang == "jpn"
        || lang == "jp"
        || lang.starts_with("zh")
        || lang.starts_with("ko")
}

pub(crate) fn should_keep_cjk_line(text: &str) -> bool {
    if is_numeric_like(text.trim()) {
        return true;
    }
    let stats = cjk_stats(text);
    if stats.cjk >= 2 {
        let ratio = stats.cjk as f32 / stats.total.max(1) as f32;
        return ratio >= 0.35 || stats.total <= 6;
    }
    false
}

struct CjkStats {
    cjk: usize,
    total: usize,
    digits: usize,
    ascii: usize,
}

fn cjk_stats(text: &str) -> CjkStats {
    let mut stats = CjkStats {
        cjk: 0,
        total: 0,
        digits: 0,
        ascii: 0,
    };
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        stats.total += 1;
        if ch.is_ascii_digit() {
            stats.digits += 1;
        }
        if ch.is_ascii_alphabetic() {
            stats.ascii += 1;
        }
        if matches!(
            ch as u32,
            0x4E00..=0x9FFF
                | 0x3040..=0x30FF
                | 0x31F0..=0x31FF
                | 0x3400..=0x4DBF
                | 0xAC00..=0xD7AF
        ) {
            stats.cjk += 1;
        }
    }
    stats
}

pub(crate) fn is_numeric_like(value: &str) -> bool {
    let mut digits = 0usize;
    let mut letters = 0usize;
    let mut others = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else if ch.is_alphabetic()
            || matches!(ch as u32, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF)
        {
            letters += 1;
        } else if !ch.is_whitespace() {
            others += 1;
        }
    }
    if letters > 0 {
        return false;
    }
    digits > 0 && (digits as f32 / (digits + others).max(1) as f32) >= 0.6
}

pub(crate) fn should_translate_text(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_numeric_like(trimmed) {
        return false;
    }
    !looks_like_code(trimmed)
}

pub(crate) fn looks_like_code(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return true;
    }
    if trimmed.contains("://") {
        return true;
    }
    if trimmed.contains("{{") || trimmed.contains("}}") || trimmed.contains("${") {
        return true;
    }
    if trimmed.contains("=>") || trimmed.contains("->") || trimmed.contains("::") {
        return true;
    }
    if trimmed.contains('<') && trimmed.contains('>') {
        return true;
    }
    if !trimmed.chars().any(|ch| ch.is_whitespace())
        && trimmed.is_ascii()
        && looks_like_identifier(trimmed)
    {
        return true;
    }
    false
}

fn looks_like_identifier(value: &str) -> bool {
    if value.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    let has_special = value
        .chars()
        .any(|ch| matches!(ch, '_' | '-' | '/' | '.' | ':' | '@'));
    let has_digit = value.chars().any(|ch| ch.is_ascii_digit());
    let has_camel = value
        .chars()
        .zip(value.chars().skip(1))
        .any(|(prev, next)| prev.is_ascii_lowercase() && next.is_ascii_uppercase());
    let allowed = value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | ':' | '@' | '$')
    });
    if allowed && (has_special || has_digit || has_camel) {
        return true;
    }
    is_all_uppercase(value)
}

fn is_all_uppercase(value: &str) -> bool {
    let mut has_alpha = false;
    for ch in value.chars() {
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
            if !ch.is_ascii_uppercase() {
                return false;
            }
        }
    }
    has_alpha
}

pub(crate) fn is_numeric_only_like(value: &str) -> bool {
    let mut digits = 0usize;
    let mut letters = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else if ch.is_alphabetic()
            || matches!(ch as u32, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF)
        {
            letters += 1;
        }
    }
    digits > 0 && letters == 0
}

pub(crate) fn is_ascii_alnum_only(value: &str) -> bool {
    let mut has_alnum = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            has_alnum = true;
            continue;
        }
        if is_ascii_punct_ok(ch) {
            continue;
        }
        return false;
    }
    has_alnum
}

pub(crate) fn is_ascii_punct_ok(ch: char) -> bool {
    matches!(
        ch,
        '%' | '.'
            | ','
            | ':'
            | ';'
            | '-'
            | '/'
            | '\\'
            | '+'
            | '#'
            | '&'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '!'
            | '?'
            | '\''
            | '"'
    )
}

fn trim_ocr_edges(value: &str) -> String {
    let mut s = value.trim().to_string();
    if is_numeric_only_like(&s) {
        s = trim_edge_chars(&s, is_edge_noise_numeric);
    } else {
        s = trim_edge_chars(&s, is_edge_noise);
    }
    s = drop_trailing_single_digit(&s);
    s.trim().to_string()
}

fn trim_edge_chars<F>(value: &str, mut predicate: F) -> String
where
    F: FnMut(char) -> bool,
{
    let mut start = 0usize;
    let mut end = value.len();

    for (idx, ch) in value.char_indices() {
        if predicate(ch) {
            start = idx + ch.len_utf8();
        } else {
            break;
        }
    }

    for (idx, ch) in value.char_indices().rev() {
        if idx < start {
            break;
        }
        if predicate(ch) {
            end = idx;
        } else {
            break;
        }
    }

    value[start..end].to_string()
}

fn is_edge_noise(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '「' | '」'
                | '『'
                | '』'
                | '《'
                | '》'
                | '〈'
                | '〉'
                | '【'
                | '】'
                | '（'
                | '）'
                | '・'
                | '、'
                | '。'
                | '，'
                | '．'
                | '※'
        )
}

fn is_edge_noise_numeric(ch: char) -> bool {
    if ch.is_ascii_digit() {
        return false;
    }
    if matches!(ch, '%' | '％' | '+' | '-' | '.' | ',' | '．' | '，') {
        return false;
    }
    is_edge_noise(ch)
}

fn trim_ascii_edges_for_cjk(value: &str) -> String {
    if !value
        .chars()
        .any(|ch| matches!(ch as u32, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF))
    {
        return value.to_string();
    }
    let mut start = 0usize;
    let mut end = value.len();
    let mut leading = true;
    for (idx, ch) in value.char_indices() {
        if leading && ch.is_ascii_alphabetic() {
            start = idx + ch.len_utf8();
        } else {
            leading = false;
        }
    }
    for (idx, ch) in value.char_indices().rev() {
        if idx < start {
            break;
        }
        if ch.is_ascii_alphabetic() {
            end = idx;
        } else {
            break;
        }
    }
    value[start..end].trim().to_string()
}

pub(crate) fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF
    )
}

pub(crate) fn is_hangul(ch: char) -> bool {
    matches!(ch as u32, 0xAC00..=0xD7AF)
}

fn drop_trailing_single_digit(value: &str) -> String {
    let digits: Vec<char> = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() != 1 {
        return value.to_string();
    }
    if value.contains('%') {
        return value.to_string();
    }
    let last = value.chars().last().unwrap_or(' ');
    if last.is_ascii_digit() {
        value
            .trim_end_matches(|ch: char| ch.is_ascii_digit())
            .trim_end()
            .to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn split_text_bounds(text: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut end = None;
    for (idx, ch) in text.char_indices() {
        if !ch.is_whitespace() {
            start = Some(idx);
            break;
        }
    }
    for (idx, ch) in text.char_indices().rev() {
        if !ch.is_whitespace() {
            end = Some(idx + ch.len_utf8());
            break;
        }
    }
    match (start, end) {
        (Some(s), Some(e)) if s < e => Some((s, e)),
        _ => None,
    }
}
