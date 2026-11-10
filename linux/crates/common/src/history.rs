use crate::{paths, time};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const LIMIT: usize = 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub date: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Gif,
    Video,
}

impl Entry {
    pub fn media(&self) -> Option<MediaKind> {
        media_kind(&self.path)
    }

    pub fn seconds(&self) -> i64 {
        time::parse_iso8601(&self.date).unwrap_or(0)
    }
}

pub fn media_kind(path: &Path) -> Option<MediaKind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "gif" => Some(MediaKind::Gif),
        "mp4" | "mov" | "m4v" | "mkv" | "webm" => Some(MediaKind::Video),
        _ => None,
    }
}

fn file() -> PathBuf {
    paths::data_dir().join("history.json")
}

pub fn modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(file()).and_then(|meta| meta.modified()).ok()
}

pub fn load() -> Vec<Entry> {
    std::fs::read(file()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
}

fn persist(entries: &[Entry]) {
    let file = file();
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_vec(entries) {
        let _ = crate::write_atomically(&file, &data);
    }
}

fn normalized(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn record(path: &Path) {
    let path = normalized(path);
    let mut entries = load();
    entries.retain(|entry| entry.path != path);
    entries.insert(0, Entry { path, date: time::iso8601(time::now()) });
    entries.truncate(LIMIT);
    persist(&entries);
}

pub fn remove(paths: &[PathBuf]) {
    let paths: Vec<PathBuf> = paths.iter().map(|path| normalized(path)).collect();
    let mut entries = load();
    let count = entries.len();
    entries.retain(|entry| !paths.contains(&entry.path));
    if entries.len() != count {
        persist(&entries);
    }
}

pub fn prune_missing() -> Vec<Entry> {
    let mut entries = load();
    let count = entries.len();
    entries.retain(|entry| entry.path.exists());
    if entries.len() != count {
        persist(&entries);
    }
    entries
}
