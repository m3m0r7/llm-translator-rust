pub(super) fn join_inline(left: &str, right: &str) -> String {
    if needs_space(left, right) {
        format!("{} {}", left.trim_end(), right.trim_start())
    } else {
        format!("{}{}", left.trim_end(), right.trim_start())
    }
}

pub(super) fn needs_space(left: &str, right: &str) -> bool {
    let last = left.chars().rev().find(|ch| !ch.is_whitespace());
    let first = right.chars().find(|ch| !ch.is_whitespace());
    match (last, first) {
        (Some(a), Some(b)) => {
            (a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric())
                || (a.is_alphabetic() && b.is_alphabetic())
        }
        _ => false,
    }
}

pub(super) fn merge_conf(a: f32, a_len: usize, b: f32, b_len: usize) -> f32 {
    let total = (a_len + b_len).max(1) as f32;
    (a * a_len as f32 + b * b_len as f32) / total
}
