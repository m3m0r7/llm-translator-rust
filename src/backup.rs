use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::build_env;

const META_FILE_NAME: &str = "meta.json";
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct BackupEntry {
    pub(crate) id: String,
    pub(crate) src: String,
    pub(crate) backup: String,
    pub(crate) created_at: u64,
    pub(crate) expires_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct BackupMeta {
    entries: Vec<BackupEntry>,
}

pub(crate) fn backup_dir() -> PathBuf {
    build_env::backup_dir()
}

pub(crate) fn backup_file(src: &Path, ttl_days: u64) -> Result<BackupEntry> {
    let metadata = fs::metadata(src)
        .with_context(|| format!("failed to read file metadata: {}", src.display()))?;
    if !metadata.is_file() {
        return Err(anyhow::anyhow!(
            "backup source is not a file: {}",
            src.display()
        ));
    }

    let ttl_days = if ttl_days == 0 { 30 } else { ttl_days };
    let now = now_unix();
    let expires_at = now.saturating_add(ttl_days.saturating_mul(SECONDS_PER_DAY));

    let dir = backup_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create backup dir: {}", dir.display()))?;

    let mut meta = read_meta()?;
    cleanup_expired(&mut meta, now);

    let file_name = src
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let id_seed = format!("{}:{}", now, src.display());
    let id = format!("{:x}", md5::compute(id_seed.as_bytes()));
    let backup_name = format!("{}_{}", id, sanitize_filename_component(file_name));
    let backup_path = dir.join(backup_name);

    fs::copy(src, &backup_path).with_context(|| {
        format!(
            "failed to copy backup from {} to {}",
            src.display(),
            backup_path.display()
        )
    })?;

    let entry = BackupEntry {
        id,
        src: src.to_string_lossy().to_string(),
        backup: backup_path.to_string_lossy().to_string(),
        created_at: now,
        expires_at,
    };
    meta.entries.push(entry.clone());
    write_meta(&meta)?;
    Ok(entry)
}

fn meta_path() -> PathBuf {
    backup_dir().join(META_FILE_NAME)
}

fn read_meta() -> Result<BackupMeta> {
    let path = meta_path();
    if !path.exists() {
        return Ok(BackupMeta::default());
    }
    let content = fs::read_to_string(&path).with_context(|| "failed to read backup meta")?;
    let meta = serde_json::from_str(&content).with_context(|| "failed to parse backup meta")?;
    Ok(meta)
}

fn write_meta(meta: &BackupMeta) -> Result<()> {
    let path = meta_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create backup meta dir: {}", dir.display()))?;
    }
    let content = serde_json::to_string_pretty(meta)?;
    fs::write(&path, content).with_context(|| "failed to write backup meta")?;
    Ok(())
}

fn cleanup_expired(meta: &mut BackupMeta, now: u64) {
    let mut kept = Vec::new();
    for entry in meta.entries.drain(..) {
        let expired = entry.expires_at <= now;
        let backup_path = Path::new(&entry.backup);
        if expired || !backup_path.exists() {
            if backup_path.exists() {
                let _ = fs::remove_file(backup_path);
            }
            continue;
        }
        kept.push(entry);
    }
    meta.entries = kept;
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sanitize_filename_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "file".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_temp_home;
    use tempfile::tempdir;

    #[test]
    fn backup_file_creates_meta_entry() {
        with_temp_home(|_| {
            let dir = tempdir().expect("tempdir");
            let file_path = dir.path().join("input.txt");
            fs::write(&file_path, "hello").expect("write file");

            let entry = backup_file(&file_path, 1).expect("backup file");
            assert!(Path::new(&entry.backup).exists());
            assert_eq!(entry.src, file_path.to_string_lossy());
            assert!(entry.expires_at >= entry.created_at + SECONDS_PER_DAY - 1);

            let meta_path = backup_dir().join(META_FILE_NAME);
            assert!(meta_path.exists());
            let meta = fs::read_to_string(meta_path).expect("read meta");
            assert!(meta.contains(&entry.id));
        });
    }
}
