use anyhow::{Result, anyhow};
use globset::{GlobBuilder, GlobMatcher};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct TranslationIgnore {
    root: PathBuf,
    patterns: Vec<IgnorePattern>,
}

#[derive(Clone)]
struct IgnorePattern {
    matcher: GlobMatcher,
    negated: bool,
    dir_only: bool,
    match_basename: bool,
}

impl TranslationIgnore {
    pub(crate) fn new(root: &Path, patterns: Vec<String>) -> Result<Option<Self>> {
        let mut compiled = Vec::new();
        for raw in patterns {
            if let Some(pattern) = parse_pattern(&raw)? {
                compiled.push(pattern);
            }
        }
        if compiled.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            root: root.to_path_buf(),
            patterns: compiled,
        }))
    }

    pub(crate) fn is_ignored(&self, path: &Path) -> bool {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        let rel_str = normalize_path(rel);
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let ancestors = ancestor_dirs(rel);

        let mut ignored = false;
        for pattern in &self.patterns {
            if pattern.matches(&rel_str, file_name, &ancestors) {
                ignored = !pattern.negated;
            }
        }
        ignored
    }
}

impl IgnorePattern {
    fn matches(&self, rel: &str, file_name: &str, ancestors: &[String]) -> bool {
        if self.dir_only {
            if self.match_basename {
                for dir in ancestors {
                    let base = dir.rsplit('/').next().unwrap_or(dir);
                    if self.matcher.is_match(base) {
                        return true;
                    }
                }
            } else {
                for dir in ancestors {
                    if self.matcher.is_match(dir) {
                        return true;
                    }
                }
            }
            return false;
        }

        if self.match_basename {
            self.matcher.is_match(file_name)
        } else {
            self.matcher.is_match(rel)
        }
    }
}

fn parse_pattern(raw: &str) -> Result<Option<IgnorePattern>> {
    let line = raw.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let mut pattern = line;
    if pattern.starts_with("\\#") || pattern.starts_with("\\!") {
        pattern = &pattern[1..];
    } else if pattern.starts_with('#') {
        return Ok(None);
    }

    let mut negated = false;
    if let Some(stripped) = pattern.strip_prefix('!') {
        negated = true;
        pattern = stripped;
    }
    if pattern.is_empty() {
        return Ok(None);
    }

    let mut dir_only = false;
    if pattern.ends_with('/') {
        dir_only = true;
        pattern = pattern.trim_end_matches('/');
        if pattern.is_empty() {
            return Ok(None);
        }
    }

    let anchored = pattern.starts_with('/');
    if anchored {
        pattern = pattern.trim_start_matches('/');
    }

    let has_slash = pattern.contains('/');
    let match_basename = !anchored && !has_slash;
    let glob = if match_basename || anchored {
        pattern.to_string()
    } else {
        format!("**/{}", pattern)
    };

    let matcher = GlobBuilder::new(&glob)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
        .map_err(|err| anyhow!("invalid ignore pattern '{}': {}", raw, err))?
        .compile_matcher();

    Ok(Some(IgnorePattern {
        matcher,
        negated,
        dir_only,
        match_basename,
    }))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn ancestor_dirs(rel: &Path) -> Vec<String> {
    let Some(parent) = rel.parent() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut current = String::new();
    for component in parent.components() {
        let part = component.as_os_str().to_string_lossy();
        if part.is_empty() {
            continue;
        }
        if current.is_empty() {
            current = part.to_string();
        } else {
            current.push('/');
            current.push_str(&part);
        }
        out.push(current.clone());
    }
    out
}
