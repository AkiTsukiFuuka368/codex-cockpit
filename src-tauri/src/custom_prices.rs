//! Prices the user enters in Settings → custom model prices.
//!
//! Stored in `<app config dir>/custom-prices.json`. They take precedence over the
//! built-in, shared and Tokei catalogs, for Codex and for Claude, and are never
//! exported to a shared catalog. A model id ending in `*` matches by prefix.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
    time::SystemTime,
};
use tauri::Manager;

pub const FILE: &str = "custom-prices.json";
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 500;

/// USD per million tokens.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomPrice {
    pub model: String,
    pub input: f64,
    pub cache_read: f64,
    pub output: f64,
    /// Cache write price (Claude). When absent, Claude's own ratio is used:
    /// 1.25× input for 5-minute writes, 2× input for 1-hour writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

#[derive(Default, Deserialize, Serialize)]
struct FileShape {
    #[serde(default)]
    models: Vec<CustomPrice>,
}

fn valid_price(value: f64) -> bool {
    value.is_finite() && (0.0..=100_000.0).contains(&value)
}

/// Lower case, without an `openai/` or `anthropic/` namespace.
pub fn normalize(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();
    for prefix in ["openai/", "anthropic/"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    lower
}

fn clean(entries: Vec<CustomPrice>) -> Vec<CustomPrice> {
    let mut out: Vec<CustomPrice> = Vec::new();
    for mut entry in entries.into_iter().take(MAX_ENTRIES) {
        entry.model = normalize(&entry.model);
        let id_ok = !entry.model.is_empty()
            && entry.model.len() <= 200
            && entry
                .model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/[]@*".contains(c))
            && !entry.model[..entry.model.len() - 1].contains('*');
        if !id_ok
            || !valid_price(entry.input)
            || !valid_price(entry.cache_read)
            || !valid_price(entry.output)
            || entry.cache_write.is_some_and(|value| !valid_price(value))
        {
            continue;
        }
        out.retain(|existing| existing.model != entry.model);
        out.push(entry);
    }
    out
}

pub fn read(path: &Path) -> Vec<CustomPrice> {
    let Ok(meta) = fs::metadata(path) else {
        return Vec::new();
    };
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Vec::new();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            serde_json::from_str::<FileShape>(text.trim_start_matches('\u{feff}')).ok()
        })
        .map(|shape| clean(shape.models))
        .unwrap_or_default()
}

/// The entry for `model`: an exact match first, then the longest `prefix*` match.
pub fn find<'a>(entries: &'a [CustomPrice], model: &str) -> Option<&'a CustomPrice> {
    let id = normalize(model);
    entries.iter().find(|entry| entry.model == id).or_else(|| {
        entries
            .iter()
            .filter_map(|entry| {
                let prefix = entry.model.strip_suffix('*')?;
                id.starts_with(prefix).then_some((prefix.len(), entry))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, entry)| entry)
    })
}

/// Length and modification time, so caches notice an edit.
pub fn file_key(path: &Path) -> (Option<u64>, Option<SystemTime>) {
    let meta = fs::metadata(path).ok();
    (
        meta.as_ref().map(fs::Metadata::len),
        meta.and_then(|m| m.modified().ok()),
    )
}

// ----- Process-wide copy for Claude, refreshed by its commands -----

struct Loaded {
    path: PathBuf,
    key: (Option<u64>, Option<SystemTime>),
    entries: Vec<CustomPrice>,
}

static LOADED: RwLock<Option<Loaded>> = RwLock::new(None);

/// Re-read the file when it changed since the last call.
pub fn refresh(data_dir: &Path) {
    let path = data_dir.join(FILE);
    let key = file_key(&path);
    if let Ok(guard) = LOADED.read() {
        if guard
            .as_ref()
            .is_some_and(|loaded| loaded.path == path && loaded.key == key)
        {
            return;
        }
    }
    let entries = read(&path);
    if let Ok(mut guard) = LOADED.write() {
        *guard = Some(Loaded { path, key, entries });
    }
}

pub fn lookup(model: &str) -> Option<CustomPrice> {
    let guard = LOADED.read().ok()?;
    find(&guard.as_ref()?.entries, model).cloned()
}

pub fn data_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok()
}

// ----- Commands -----

#[tauri::command]
pub async fn get_custom_prices(app: tauri::AppHandle) -> Result<Vec<CustomPrice>, String> {
    let dir = data_dir(&app).ok_or("settings_unavailable")?;
    Ok(read(&dir.join(FILE)))
}

#[tauri::command]
pub async fn save_custom_prices(
    app: tauri::AppHandle,
    entries: Vec<CustomPrice>,
) -> Result<Vec<CustomPrice>, String> {
    let dir = data_dir(&app).ok_or("settings_unavailable")?;
    let entries = clean(entries);
    fs::create_dir_all(&dir).map_err(|_| "custom_prices_write_failed")?;
    let path = dir.join(FILE);
    let tmp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(&FileShape {
        models: entries.clone(),
    })
    .map_err(|_| "custom_prices_write_failed")?;
    fs::write(&tmp, text).map_err(|_| "custom_prices_write_failed")?;
    fs::rename(&tmp, &path).map_err(|_| {
        let _ = fs::remove_file(&tmp);
        "custom_prices_write_failed"
    })?;
    refresh(&dir);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(model: &str, input: f64) -> CustomPrice {
        CustomPrice {
            model: model.into(),
            input,
            cache_read: 0.1,
            output: 2.0,
            cache_write: None,
        }
    }

    #[test]
    fn cleans_and_matches_exact_before_prefix() {
        let entries = clean(vec![
            price(" OpenAI/GPT-7-Terra ", 1.0),
            price("gpt-7*", 3.0),
            price("gpt-*", 5.0),
            price("bad*id*", 1.0),
            price("neg", -1.0),
            price("gpt-7-terra", 2.0), // later duplicate wins
        ]);
        assert_eq!(entries.len(), 3);
        assert_eq!(find(&entries, "openai/gpt-7-terra").unwrap().input, 2.0);
        assert_eq!(find(&entries, "gpt-7-sol").unwrap().input, 3.0);
        assert_eq!(find(&entries, "gpt-8").unwrap().input, 5.0);
        assert!(find(&entries, "claude-opus-6").is_none());
    }

    #[test]
    fn reads_what_was_written() {
        let dir = std::env::temp_dir().join(format!("cockpit-custom-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        fs::write(
            &path,
            r#"{"models":[{"model":"claude-opus-6","input":5,"cacheRead":0.5,"output":25,"cacheWrite":6.25}]}"#,
        )
        .unwrap();
        let entries = read(&path);
        assert_eq!(entries[0].cache_write, Some(6.25));
        refresh(&dir);
        assert_eq!(lookup("anthropic/claude-opus-6").unwrap().output, 25.0);
        let _ = fs::remove_dir_all(&dir);
    }
}
