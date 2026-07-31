use crate::config_store::resolve_path_with_fallback;
use crate::core::history::{
    HistoryEntry, MAX_HISTORY, cap_entries, entry_line, history_path, parse_history, recall_list,
    to_jsonl,
};
use crate::core::mode::Mode;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn resolve_path() -> PathBuf {
    resolve_path_with_fallback("FARO_HISTORY", "XDG_STATE_HOME", history_path)
}

pub fn load_recall() -> Vec<String> {
    recall_list(&load_entries())
}

pub fn stamped_entry(query: &str, mode: Mode, fast: bool) -> HistoryEntry {
    HistoryEntry {
        query: query.to_string(),
        mode,
        fast,
        at: unix_seconds(),
    }
}

pub fn append(entry: &HistoryEntry) -> std::io::Result<()> {
    let path = resolve_path();
    ensure_parent(&path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", entry_line(entry))
}

pub fn clear() -> std::io::Result<()> {
    match fs::remove_file(resolve_path()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn load_entries() -> Vec<HistoryEntry> {
    let path = resolve_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let entries = parse_history(&text);
    if entries.len() <= MAX_HISTORY {
        return entries;
    }
    let capped = cap_entries(entries);
    let _ = compact(&path, &capped);
    capped
}

fn compact(path: &Path, entries: &[HistoryEntry]) -> std::io::Result<()> {
    ensure_parent(path)?;
    fs::write(path, to_jsonl(entries))
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}
