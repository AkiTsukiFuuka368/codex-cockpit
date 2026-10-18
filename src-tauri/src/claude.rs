//! Claude (Anthropic) support: subscription windows and local Claude Code token usage.
//!
//! Quota windows are never fetched with the user's credentials. They come from data
//! Claude Code itself hands to user scripts: the `rate_limits` object passed to a
//! status line command, and `rate_limit_event` messages in `--output-format stream-json`
//! runs. Small scripts written by this app store those values under
//! `<claude config dir>/cockpit/`, and this module only reads them.
//!
//! Token usage comes from Claude Code's local session transcripts
//! (`<claude config dir>/projects/**/*.jsonl`). Only usage numbers, model ids,
//! timestamps and the project folder name are read; message contents are skipped.
use crate::models::{ProviderSnapshot, UsageWindow};
use chrono::{DateTime, Local, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

const COCKPIT_DIR: &str = "cockpit";
const RATE_FILE: &str = "rate-limits.json";
const RAW_STATUSLINE_FILE: &str = "statusline-raw.json";
const MAX_RATE_FILE_BYTES: u64 = 512 * 1024;
const MAX_TRANSCRIPT_FILES: usize = 20_000;
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------

/// Claude Code's config directory: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Some(path);
        }
    }
    dirs::home_dir().map(|home| home.join(".claude"))
}

fn cockpit_dir(base: &Path) -> PathBuf {
    base.join(COCKPIT_DIR)
}

// ---------------------------------------------------------------------------
// Official API price reference (USD per million tokens)
// Source: Anthropic pricing page, checked 2026-10-18. Estimates only.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
    pub output: f64,
}

const fn rates(input: f64, cw5: f64, cw1h: f64, cr: f64, output: f64) -> Rates {
    Rates {
        input,
        cache_write_5m: cw5,
        cache_write_1h: cw1h,
        cache_read: cr,
        output,
    }
}

struct PriceEntry {
    /// Matched against the normalized model id, followed by end, a date or a variant suffix.
    key: &'static str,
    name: &'static str,
    standard: Rates,
    /// Rates for requests whose prompt exceeds `long_threshold` tokens.
    long: Option<(u64, Rates)>,
}

/// Most specific keys first: "opus-5-5" must win over "opus-5".
const PRICES: &[PriceEntry] = &[
    PriceEntry {
        key: "fable-5-1",
        name: "Fable 5.1",
        standard: rates(10.0, 12.5, 20.0, 0.25, 50.0),
        long: None,
    },
    PriceEntry {
        key: "mythos-5-1",
        name: "Mythos 5.1",
        standard: rates(10.0, 12.5, 20.0, 0.25, 50.0),
        long: None,
    },
    PriceEntry {
        key: "fable-5",
        name: "Fable 5",
        standard: rates(10.0, 12.5, 20.0, 1.0, 50.0),
        long: None,
    },
    PriceEntry {
        key: "mythos-5",
        name: "Mythos 5",
        standard: rates(10.0, 12.5, 20.0, 1.0, 50.0),
        long: None,
    },
    PriceEntry {
        key: "opus-5-5",
        name: "Opus 5.5",
        standard: rates(4.0, 5.0, 8.0, 0.20, 20.0),
        long: None,
    },
    PriceEntry {
        key: "opus-5",
        name: "Opus 5",
        standard: rates(5.0, 6.25, 10.0, 0.50, 25.0),
        long: None,
    },
    PriceEntry {
        key: "opus-4-8",
        name: "Opus 4.8",
        standard: rates(5.0, 6.25, 10.0, 0.50, 25.0),
        long: None,
    },
    PriceEntry {
        key: "opus-4-7",
        name: "Opus 4.7",
        standard: rates(5.0, 6.25, 10.0, 0.50, 25.0),
        long: None,
    },
    PriceEntry {
        key: "opus-4-6",
        name: "Opus 4.6",
        standard: rates(5.0, 6.25, 10.0, 0.50, 25.0),
        long: None,
    },
    PriceEntry {
        key: "opus-4-5",
        name: "Opus 4.5",
        standard: rates(5.0, 6.25, 10.0, 0.50, 25.0),
        long: None,
    },
    PriceEntry {
        key: "opus-4-1",
        name: "Opus 4.1",
        standard: rates(15.0, 18.75, 30.0, 1.50, 75.0),
        long: None,
    },
    PriceEntry {
        key: "opus-4",
        name: "Opus 4",
        standard: rates(15.0, 18.75, 30.0, 1.50, 75.0),
        long: None,
    },
    PriceEntry {
        key: "sonnet-5-5",
        name: "Sonnet 5.5",
        standard: rates(2.0, 2.5, 4.0, 0.10, 10.0),
        long: None,
    },
    PriceEntry {
        key: "sonnet-5",
        name: "Sonnet 5",
        standard: rates(2.0, 2.5, 4.0, 0.20, 10.0),
        long: None,
    },
    PriceEntry {
        key: "sonnet-4-6",
        name: "Sonnet 4.6",
        standard: rates(3.0, 3.75, 6.0, 0.30, 15.0),
        long: None,
    },
    PriceEntry {
        key: "sonnet-4-5",
        name: "Sonnet 4.5",
        standard: rates(3.0, 3.75, 6.0, 0.30, 15.0),
        long: None,
    },
    PriceEntry {
        key: "sonnet-4",
        name: "Sonnet 4",
        standard: rates(3.0, 3.75, 6.0, 0.30, 15.0),
        long: None,
    },
    PriceEntry {
        key: "haiku-5-5",
        name: "Haiku 5.5",
        standard: rates(0.10, 0.125, 0.20, 0.01, 0.50),
        long: Some((100_000, rates(0.50, 0.625, 1.0, 0.05, 2.50))),
    },
    PriceEntry {
        key: "haiku-4-5",
        name: "Haiku 4.5",
        standard: rates(1.0, 1.25, 2.0, 0.10, 5.0),
        long: None,
    },
    PriceEntry {
        key: "3-5-haiku",
        name: "Haiku 3.5",
        standard: rates(0.80, 1.0, 1.60, 0.08, 4.0),
        long: None,
    },
    PriceEntry {
        key: "haiku-3-5",
        name: "Haiku 3.5",
        standard: rates(0.80, 1.0, 1.60, 0.08, 4.0),
        long: None,
    },
];

fn normalize_model(id: &str) -> String {
    let lower = id.trim().to_ascii_lowercase();
    let lower = lower.rsplit('/').next().unwrap_or(&lower).to_string();
    lower
        .strip_prefix("anthropic.")
        .map(str::to_string)
        .unwrap_or(lower)
}

fn suffix_ok(rest: &str) -> bool {
    if rest.is_empty() || rest.starts_with('[') || rest.starts_with('@') || rest.starts_with(':') {
        return true;
    }
    // Dated snapshot such as "-20250514", optionally followed by more variant text.
    let Some(digits) = rest.strip_prefix('-') else {
        return false;
    };
    digits.len() >= 8 && digits.as_bytes()[..8].iter().all(u8::is_ascii_digit)
}

fn price_entry(model: &str) -> Option<&'static PriceEntry> {
    let id = normalize_model(model);
    PRICES.iter().find(|entry| {
        id.match_indices(entry.key).any(|(index, matched)| {
            let before_ok = index == 0 || id.as_bytes()[index - 1] == b'-';
            before_ok && suffix_ok(&id[index + matched.len()..])
        })
    })
}

pub fn display_model_name(model: &str) -> String {
    price_entry(model)
        .map(|entry| format!("Claude {}", entry.name))
        .unwrap_or_else(|| model.to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl Tokens {
    fn prompt(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read
    }
    fn total(&self) -> u64 {
        self.prompt() + self.output
    }
}

/// Estimated API-equivalent cost of one request, or `None` for unknown models.
pub fn request_cost(model: &str, tokens: &Tokens) -> Option<f64> {
    let entry = price_entry(model)?;
    let rate = match entry.long {
        Some((threshold, long)) if tokens.prompt() > threshold => long,
        _ => entry.standard,
    };
    let per = |count: u64, price: f64| count as f64 * price / 1_000_000.0;
    Some(
        per(tokens.input, rate.input)
            + per(tokens.cache_write_5m, rate.cache_write_5m)
            + per(tokens.cache_write_1h, rate.cache_write_1h)
            + per(tokens.cache_read, rate.cache_read)
            + per(tokens.output, rate.output),
    )
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    pub name: String,
    pub input: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
    pub output: f64,
    pub note: Option<String>,
}

fn price_rows() -> Vec<PriceRow> {
    let mut seen = Vec::<&str>::new();
    let mut rows = Vec::new();
    for entry in PRICES {
        if seen.contains(&entry.name) {
            continue;
        }
        seen.push(entry.name);
        let row = |rate: Rates, name: String, note: Option<String>| PriceRow {
            name,
            input: rate.input,
            cache_write_5m: rate.cache_write_5m,
            cache_write_1h: rate.cache_write_1h,
            cache_read: rate.cache_read,
            output: rate.output,
            note,
        };
        match entry.long {
            Some((threshold, long)) => {
                rows.push(row(
                    entry.standard,
                    format!("Claude {}", entry.name),
                    Some(format!("prompt ≤ {}K", threshold / 1000)),
                ));
                rows.push(row(
                    long,
                    format!("Claude {}", entry.name),
                    Some(format!("prompt > {}K", threshold / 1000)),
                ));
            }
            None => rows.push(row(entry.standard, format!("Claude {}", entry.name), None)),
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// Subscription windows (5-hour and weekly)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowObservation {
    used_percent: f64,
    /// Unix epoch seconds.
    resets_at: Option<i64>,
    /// Unix epoch seconds.
    observed_at: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RateObservation {
    five_hour: Option<WindowObservation>,
    seven_day: Option<WindowObservation>,
}

fn read_small(path: &Path) -> Option<(String, SystemTime)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_RATE_FILE_BYTES {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path).ok()?.read_to_string(&mut text).ok()?;
    // Windows PowerShell 5.1 writes UTF-8 with a BOM.
    let text = text.trim_start_matches('\u{feff}').to_string();
    Some((text, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
}

fn epoch(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

/// Accept seconds or milliseconds.
fn epoch_seconds(value: &Value) -> Option<i64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    Some(if number > 1e12 {
        (number / 1000.0) as i64
    } else {
        number as i64
    })
}

fn window_from(
    value: Option<&Value>,
    percent_keys: &[&str],
    observed_at: i64,
) -> Option<WindowObservation> {
    let value = value?;
    let used_percent = percent_keys
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))?;
    if !used_percent.is_finite() {
        return None;
    }
    let resets_at = ["resetsAt", "resets_at"]
        .iter()
        .find_map(|key| value.get(*key).and_then(epoch_seconds));
    Some(WindowObservation {
        used_percent: used_percent.clamp(0.0, 100.0),
        resets_at,
        observed_at,
    })
}

/// Our own compact format, written by the status line and wrapper scripts.
fn parse_rate_file(text: &str, modified: i64) -> RateObservation {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return RateObservation::default();
    };
    let observed_at = value
        .get("observedAt")
        .and_then(epoch_seconds)
        .unwrap_or(modified);
    let window = |key: &str| {
        let item = value.get(key)?;
        let observed = item
            .get("observedAt")
            .and_then(epoch_seconds)
            .unwrap_or(observed_at);
        window_from(Some(item), &["usedPercent"], observed)
    };
    RateObservation {
        five_hour: window("fiveHour"),
        seven_day: window("sevenDay"),
    }
}

/// The full JSON Claude Code passes to a status line command.
fn parse_statusline_json(text: &str, modified: i64) -> RateObservation {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return RateObservation::default();
    };
    let limits = value.get("rate_limits");
    RateObservation {
        five_hour: window_from(
            limits.and_then(|v| v.get("five_hour")),
            &["used_percentage"],
            modified,
        ),
        seven_day: window_from(
            limits.and_then(|v| v.get("seven_day")),
            &["used_percentage"],
            modified,
        ),
    }
}

fn newer(a: Option<WindowObservation>, b: Option<WindowObservation>) -> Option<WindowObservation> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.observed_at > a.observed_at { b } else { a }),
        (a, b) => a.or(b),
    }
}

fn read_observation(base: &Path) -> RateObservation {
    let dir = cockpit_dir(base);
    let mut result = RateObservation::default();
    if let Some((text, modified)) = read_small(&dir.join(RATE_FILE)) {
        let parsed = parse_rate_file(&text, epoch(modified));
        result.five_hour = newer(result.five_hour, parsed.five_hour);
        result.seven_day = newer(result.seven_day, parsed.seven_day);
    }
    if let Some((text, modified)) = read_small(&dir.join(RAW_STATUSLINE_FILE)) {
        let parsed = parse_statusline_json(&text, epoch(modified));
        result.five_hour = newer(result.five_hour, parsed.five_hour);
        result.seven_day = newer(result.seven_day, parsed.seven_day);
    }
    result
}

fn rfc3339(seconds: i64) -> Option<String> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.to_rfc3339())
}

/// A window whose reset time has passed starts over: nothing is used until the next request.
fn to_window(observation: WindowObservation, window_seconds: u64, now: i64) -> UsageWindow {
    match observation.resets_at {
        Some(reset) if reset <= now => UsageWindow {
            remaining_percent: 100.0,
            resets_at: None,
            window_seconds,
        },
        reset => UsageWindow {
            remaining_percent: (100.0 - observation.used_percent).clamp(0.0, 100.0),
            resets_at: reset.and_then(rfc3339),
            window_seconds,
        },
    }
}

fn snapshot_from(observation: RateObservation, now: i64) -> Option<ProviderSnapshot> {
    if observation.five_hour.is_none() && observation.seven_day.is_none() {
        return None;
    }
    let observed_at = [observation.five_hour, observation.seven_day]
        .iter()
        .flatten()
        .map(|item| item.observed_at)
        .max()
        .unwrap_or(now);
    let observed_label = Local.timestamp_opt(observed_at, 0).single().map(|time| {
        if time.date_naive() == Local::now().date_naive() {
            time.format("%H:%M").to_string()
        } else {
            time.format("%m-%d %H:%M").to_string()
        }
    });
    Some(ProviderSnapshot {
        provider: "claude".into(),
        display_name: "CLAUDE".into(),
        plan: observed_label,
        short_window: observation
            .five_hour
            .map(|item| to_window(item, 5 * 3600, now)),
        weekly_window: observation
            .seven_day
            .map(|item| to_window(item, 7 * 24 * 3600, now)),
        monthly_window: None,
        reset_credits: None,
        reset_credit_expires_at: Vec::new(),
        balance_remaining: None,
        balance_unit: None,
        rate_limit_snapshot: None,
        updated_at: rfc3339(observed_at).unwrap_or_else(|| Utc::now().to_rfc3339()),
        status: "ok".into(),
        message: None,
    })
}

/// The Claude quota card. Absent until the collector has recorded at least one window.
pub fn fetch_snapshot() -> Option<ProviderSnapshot> {
    let base = config_dir()?;
    snapshot_from(read_observation(&base), Utc::now().timestamp())
}

// ---------------------------------------------------------------------------
// Local token usage from Claude Code transcripts
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Record {
    date: Arc<str>,
    model: Arc<str>,
    project: Arc<str>,
    tokens: Tokens,
}

#[derive(Default)]
struct FileState {
    len: u64,
    modified: Option<SystemTime>,
    offset: u64,
    records: HashMap<String, Record>,
}

#[derive(Default)]
struct Interner(HashMap<String, Arc<str>>);

impl Interner {
    fn get(&mut self, value: &str) -> Arc<str> {
        if let Some(found) = self.0.get(value) {
            return found.clone();
        }
        let arc: Arc<str> = Arc::from(value);
        self.0.insert(value.to_string(), arc.clone());
        arc
    }
}

#[derive(Default)]
struct UsageCache {
    root: Option<PathBuf>,
    files: HashMap<PathBuf, FileState>,
    strings: Interner,
}

static CACHE: Mutex<Option<UsageCache>> = Mutex::new(None);

fn u64_at(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn project_name(line: &Value, fallback: &str) -> String {
    line.get("cwd")
        .and_then(Value::as_str)
        .and_then(|cwd| {
            cwd.trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| fallback.to_string())
}

/// Parse one transcript line into (dedupe key, record).
fn parse_line(
    text: &str,
    fallback_project: &str,
    strings: &mut Interner,
) -> Option<(String, Record)> {
    // Cheap pre-filter: only assistant messages carry usage.
    if !text.contains("\"usage\"") {
        return None;
    }
    let line: Value = serde_json::from_str(text).ok()?;
    if line.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let message = line.get("message")?;
    let usage = message.get("usage")?;
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if model == "<synthetic>" {
        return None;
    }
    let timestamp = line.get("timestamp").and_then(Value::as_str)?;
    let date = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Local)
        .format("%Y-%m-%d")
        .to_string();
    let creation_total = u64_at(usage, "cache_creation_input_tokens");
    let (cw5, cw1h) = match usage.get("cache_creation") {
        Some(split) => {
            let five = u64_at(split, "ephemeral_5m_input_tokens");
            let hour = u64_at(split, "ephemeral_1h_input_tokens");
            if five + hour == 0 {
                (creation_total, 0)
            } else {
                (five, hour)
            }
        }
        None => (creation_total, 0),
    };
    let tokens = Tokens {
        input: u64_at(usage, "input_tokens"),
        cache_write_5m: cw5,
        cache_write_1h: cw1h,
        cache_read: u64_at(usage, "cache_read_input_tokens"),
        output: u64_at(usage, "output_tokens"),
    };
    if tokens.total() == 0 {
        return None;
    }
    let message_id = message.get("id").and_then(Value::as_str).unwrap_or("");
    let request_id = line.get("requestId").and_then(Value::as_str).unwrap_or("");
    let key = if message_id.is_empty() && request_id.is_empty() {
        line.get("uuid").and_then(Value::as_str)?.to_string()
    } else {
        format!("{message_id}:{request_id}")
    };
    Some((
        key,
        Record {
            date: strings.get(&date),
            model: strings.get(model),
            project: strings.get(&project_name(&line, fallback_project)),
            tokens,
        },
    ))
}

/// Streaming writes can repeat one message with growing output counts; keep the largest.
fn keep_larger(map: &mut HashMap<String, Record>, key: String, record: Record) {
    match map.get(&key) {
        Some(existing) if existing.tokens.total() >= record.tokens.total() => {}
        _ => {
            map.insert(key, record);
        }
    }
}

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if out.len() >= MAX_TRANSCRIPT_FILES {
                return;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                out.push(path);
            }
        }
    }
}

fn fallback_project(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|first| first.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn refresh_file(root: &Path, path: &Path, state: &mut FileState, strings: &mut Interner) {
    let Ok(meta) = fs::metadata(path) else { return };
    let len = meta.len();
    let modified = meta.modified().ok();
    if len == state.len && modified == state.modified {
        return;
    }
    if len < state.offset {
        // Rewritten or truncated: start over.
        *state = FileState::default();
    }
    let Ok(mut file) = fs::File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let project = fallback_project(root, path);
    let mut reader = BufReader::new(file);
    let mut offset = state.offset;
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let Ok(read) = reader.read_until(b'\n', &mut buffer) else {
            break;
        };
        if read == 0 || buffer.last() != Some(&b'\n') {
            // EOF or a line still being written: pick it up next time.
            break;
        }
        offset += read as u64;
        if buffer.len() > MAX_LINE_BYTES {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(&buffer) {
            if let Some((key, record)) = parse_line(text.trim_end(), &project, strings) {
                keep_larger(&mut state.records, key, record);
            }
        }
    }
    state.offset = offset;
    state.len = len;
    state.modified = modified;
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    pub id: String,
    pub name: String,
    pub input_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Cost of the rows whose model has a known price.
    pub estimated_cost_usd: f64,
    /// True when some tokens belong to a model without a known price.
    pub cost_incomplete: bool,
}

impl UsageRow {
    fn add(&mut self, record: &Record) {
        let tokens = &record.tokens;
        self.input_tokens += tokens.input;
        self.cache_write_tokens += tokens.cache_write_5m + tokens.cache_write_1h;
        self.cache_read_tokens += tokens.cache_read;
        self.output_tokens += tokens.output;
        self.total_tokens += tokens.total();
        match request_cost(&record.model, tokens) {
            Some(cost) => self.estimated_cost_usd += cost,
            None => self.cost_incomplete = true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    pub date: String,
    pub requests: u64,
    pub models: Vec<UsageRow>,
    pub projects: Vec<UsageRow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatus {
    /// The status line collector is configured in Claude Code settings.
    pub statusline_installed: bool,
    /// Another status line command is configured, so ours was not installed.
    pub statusline_conflict: bool,
    /// The wrapper for `claude -p` runs exists.
    pub wrapper_path: Option<String>,
    /// Last time any window was recorded (RFC 3339).
    pub last_observed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsage {
    /// "ready", "empty" (no transcripts yet) or "unavailable" (no Claude Code folder).
    pub status: String,
    pub directory: Option<String>,
    pub fetched_at: String,
    pub transcript_files: usize,
    pub days: Vec<DayUsage>,
    pub prices: Vec<PriceRow>,
    pub price_source: String,
    pub collector: CollectorStatus,
}

/// Requests, per-model rows and per-project rows for one local date.
type DayAccumulator = (
    u64,
    HashMap<Arc<str>, UsageRow>,
    HashMap<Arc<str>, UsageRow>,
);

fn aggregate(records: &HashMap<&String, &Record>) -> Vec<DayUsage> {
    let mut days: BTreeMap<Arc<str>, DayAccumulator> = BTreeMap::new();
    for record in records.values() {
        let (requests, models, projects) = days.entry(record.date.clone()).or_default();
        *requests += 1;
        models
            .entry(record.model.clone())
            .or_insert_with(|| UsageRow {
                id: record.model.to_string(),
                name: display_model_name(&record.model),
                ..UsageRow::default()
            })
            .add(record);
        projects
            .entry(record.project.clone())
            .or_insert_with(|| UsageRow {
                id: record.project.to_string(),
                name: record.project.to_string(),
                ..UsageRow::default()
            })
            .add(record);
    }
    let sorted = |map: HashMap<Arc<str>, UsageRow>| {
        let mut rows: Vec<UsageRow> = map.into_values().collect();
        rows.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens).then(a.id.cmp(&b.id)));
        rows
    };
    days.into_iter()
        .map(|(date, (requests, models, projects))| DayUsage {
            date: date.to_string(),
            requests,
            models: sorted(models),
            projects: sorted(projects),
        })
        .collect()
}

fn scan(base: &Path) -> (usize, Vec<DayUsage>) {
    let root = base.join("projects");
    let mut guard = match CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let cache = guard.get_or_insert_with(UsageCache::default);
    if cache.root.as_deref() != Some(root.as_path()) {
        *cache = UsageCache {
            root: Some(root.clone()),
            ..UsageCache::default()
        };
    }
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let present: std::collections::HashSet<&PathBuf> = files.iter().collect();
    cache.files.retain(|path, _| present.contains(path));
    let UsageCache {
        files: states,
        strings,
        ..
    } = cache;
    for path in &files {
        let state = states.entry(path.clone()).or_default();
        refresh_file(&root, path, state, strings);
    }
    // The same message can appear in several files (resumed or forked sessions).
    let mut merged: HashMap<&String, &Record> = HashMap::new();
    for state in states.values() {
        for (key, record) in &state.records {
            match merged.get(key) {
                Some(existing) if existing.tokens.total() >= record.tokens.total() => {}
                _ => {
                    merged.insert(key, record);
                }
            }
        }
    }
    (files.len(), aggregate(&merged))
}

fn settings_path(base: &Path) -> PathBuf {
    base.join("settings.json")
}

fn statusline_script_path(base: &Path) -> PathBuf {
    if cfg!(windows) {
        cockpit_dir(base).join("statusline.ps1")
    } else {
        cockpit_dir(base).join("statusline.sh")
    }
}

fn wrapper_path(base: &Path) -> PathBuf {
    cockpit_dir(base).join("claude-run.ps1")
}

fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn statusline_command(base: &Path) -> String {
    let script = forward_slashes(&statusline_script_path(base));
    if cfg!(windows) {
        format!("powershell -NoProfile -ExecutionPolicy Bypass -File \"{script}\"")
    } else {
        format!("sh \"{script}\"")
    }
}

fn is_our_command(command: &str) -> bool {
    command.contains("/cockpit/statusline.ps1") || command.contains("/cockpit/statusline.sh")
}

fn collector_status(base: &Path) -> CollectorStatus {
    let configured = read_small(&settings_path(base))
        .and_then(|(text, _)| serde_json::from_str::<Value>(&text).ok())
        .and_then(|settings| {
            settings
                .pointer("/statusLine/command")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let installed =
        configured.as_deref().is_some_and(is_our_command) && statusline_script_path(base).exists();
    let observation = read_observation(base);
    let last = [observation.five_hour, observation.seven_day]
        .iter()
        .flatten()
        .map(|item| item.observed_at)
        .max()
        .and_then(rfc3339);
    let wrapper = wrapper_path(base);
    CollectorStatus {
        statusline_installed: installed,
        statusline_conflict: configured.is_some() && !installed,
        wrapper_path: (cfg!(windows) && wrapper.exists())
            .then(|| wrapper.to_string_lossy().to_string()),
        last_observed_at: last,
    }
}

fn read_usage() -> ClaudeUsage {
    let fetched_at = Utc::now().to_rfc3339();
    let prices = price_rows();
    let price_source = "Anthropic API pricing page, 2026-10-18".to_string();
    let Some(base) = config_dir().filter(|path| path.is_dir()) else {
        return ClaudeUsage {
            status: "unavailable".into(),
            directory: config_dir().map(|path| path.to_string_lossy().to_string()),
            fetched_at,
            transcript_files: 0,
            days: Vec::new(),
            prices,
            price_source,
            collector: CollectorStatus {
                statusline_installed: false,
                statusline_conflict: false,
                wrapper_path: None,
                last_observed_at: None,
            },
        };
    };
    let (files, days) = scan(&base);
    ClaudeUsage {
        status: if days.is_empty() { "empty" } else { "ready" }.into(),
        directory: Some(base.to_string_lossy().to_string()),
        fetched_at,
        transcript_files: files,
        days,
        prices,
        price_source,
        collector: collector_status(&base),
    }
}

#[tauri::command]
pub async fn get_claude_usage() -> Result<ClaudeUsage, String> {
    tokio::task::spawn_blocking(read_usage)
        .await
        .map_err(|_| "claude_usage_unavailable".to_string())
}

// ---------------------------------------------------------------------------
// Collector installation
// ---------------------------------------------------------------------------

/// Status line script for Windows. Records `rate_limits`, then prints a short line.
const STATUSLINE_PS1: &str = r#"# Written by Codex Cockpit. Records Claude Code's rate_limits for the cockpit's Claude card.
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$raw = [Console]::In.ReadToEnd()
$base = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $HOME '.claude' }
$dir = Join-Path $base 'cockpit'
$file = Join-Path $dir 'rate-limits.json'
$data = $null
try { $data = $raw | ConvertFrom-Json } catch { }
$parts = @()
if ($data -and $data.rate_limits) {
  $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
  $out = @{}
  if (Test-Path -LiteralPath $file) {
    try {
      $old = Get-Content -LiteralPath $file -Raw -Encoding UTF8 | ConvertFrom-Json
      if ($old.fiveHour) { $out.fiveHour = $old.fiveHour }
      if ($old.sevenDay) { $out.sevenDay = $old.sevenDay }
    } catch { }
  }
  $rl = $data.rate_limits
  if ($rl.five_hour -and $null -ne $rl.five_hour.used_percentage) {
    $out.fiveHour = @{ usedPercent = [double]$rl.five_hour.used_percentage; resetsAt = $rl.five_hour.resets_at; observedAt = $now }
    $parts += ('5h ' + [math]::Round([double]$rl.five_hour.used_percentage) + '%')
  }
  if ($rl.seven_day -and $null -ne $rl.seven_day.used_percentage) {
    $out.sevenDay = @{ usedPercent = [double]$rl.seven_day.used_percentage; resetsAt = $rl.seven_day.resets_at; observedAt = $now }
    $parts += ('7d ' + [math]::Round([double]$rl.seven_day.used_percentage) + '%')
  }
  $out.observedAt = $now
  $out.source = 'statusline'
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $tmp = Join-Path $dir ('rate-limits.' + $PID + '.tmp')
  [System.IO.File]::WriteAllText($tmp, ($out | ConvertTo-Json -Compress -Depth 4), (New-Object System.Text.UTF8Encoding $false))
  Move-Item -LiteralPath $tmp -Destination $file -Force
}
$model = 'Claude'
if ($data -and $data.model -and $data.model.display_name) { $model = $data.model.display_name }
if ($parts.Count -gt 0) { Write-Output ("[" + $model + "] " + ($parts -join ' | ')) } else { Write-Output ("[" + $model + "]") }
"#;

/// Status line script for macOS/Linux. Keeps the raw JSON when it carries rate_limits.
const STATUSLINE_SH: &str = r#"#!/bin/sh
# Written by Codex Cockpit. Records Claude Code's rate_limits for the cockpit's Claude card.
dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/cockpit"
input=$(cat)
case "$input" in
  *'"rate_limits"'*)
    mkdir -p "$dir" && printf '%s' "$input" > "$dir/statusline-raw.json.tmp.$$" && mv "$dir/statusline-raw.json.tmp.$$" "$dir/statusline-raw.json"
    ;;
esac
printf 'Claude\n'
"#;

/// Wrapper for non-interactive runs (`claude -p`). It finds claude.exe (PATH, the
/// standalone install, or the copy bundled with the Claude desktop app), loads
/// CLAUDE_CODE_OAUTH_TOKEN from the user environment when the caller started before
/// it was set, runs Claude Code with stream-json output, records rate_limit_event
/// messages and prints only the final result text, like `claude -p`.
const WRAPPER_PS1: &str = r#"# Written by Codex Cockpit.
# Usage:  powershell -NoProfile -ExecutionPolicy Bypass -File claude-run.ps1 [claude options] "prompt"
#    or:  Get-Content task.md -Raw | powershell -NoProfile -ExecutionPolicy Bypass -File claude-run.ps1 --stdin [claude options]
# Standard input is only read with --stdin, so an open but idle pipe can never stall a run.
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

if (-not $env:CLAUDE_CODE_OAUTH_TOKEN) {
  $token = [Environment]::GetEnvironmentVariable('CLAUDE_CODE_OAUTH_TOKEN', 'User')
  if ($token) { $env:CLAUDE_CODE_OAUTH_TOKEN = $token }
}

function Find-Claude {
  $cmd = Get-Command claude.exe -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $cmd) { $cmd = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1 }
  if ($cmd) { return $cmd.Source }
  $local = Join-Path $HOME '.local\bin\claude.exe'
  if (Test-Path -LiteralPath $local) { return $local }
  $roots = @()
  $packages = Join-Path $env:LOCALAPPDATA 'Packages'
  if (Test-Path -LiteralPath $packages) {
    Get-ChildItem -LiteralPath $packages -Directory -Filter 'Claude_*' -ErrorAction SilentlyContinue | ForEach-Object {
      $roots += (Join-Path $_.FullName 'LocalCache\Roaming\Claude\claude-code')
    }
  }
  $roots += (Join-Path $env:APPDATA 'Claude\claude-code')
  foreach ($root in $roots) {
    if (Test-Path -LiteralPath $root) {
      $exe = Get-ChildItem -LiteralPath $root -Recurse -Filter claude.exe -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
      if ($exe) { return $exe.FullName }
    }
  }
  throw 'claude.exe was not found. Install Claude Code or the Claude desktop app.'
}

$base = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $HOME '.claude' }
$dir = Join-Path $base 'cockpit'
$file = Join-Path $dir 'rate-limits.json'

function Save-RateLimit($info) {
  try {
    $kind = [string]$info.rateLimitType
    $key = $null
    if ($kind -eq 'five_hour') { $key = 'fiveHour' } elseif ($kind -eq 'seven_day') { $key = 'sevenDay' } else { return }
    $used = $null
    if ($null -ne $info.utilization) {
      $u = [double]$info.utilization
      if ($u -le 1.0) { $u = $u * 100 }
      $used = $u
    } elseif ([string]$info.status -eq 'rejected') { $used = 100.0 }
    if ($null -eq $used) { return }
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $out = @{}
    if (Test-Path -LiteralPath $file) {
      try {
        $old = Get-Content -LiteralPath $file -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($old.fiveHour) { $out.fiveHour = $old.fiveHour }
        if ($old.sevenDay) { $out.sevenDay = $old.sevenDay }
      } catch { }
    }
    $out[$key] = @{ usedPercent = $used; resetsAt = $info.resetsAt; observedAt = $now }
    $out.observedAt = $now
    $out.source = 'stream'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $tmp = Join-Path $dir ('rate-limits.' + $PID + '.tmp')
    [System.IO.File]::WriteAllText($tmp, ($out | ConvertTo-Json -Compress -Depth 4), $utf8)
    Move-Item -LiteralPath $tmp -Destination $file -Force
  } catch { }
}

$exe = Find-Claude
$useStdin = $false
$passArgs = @()
foreach ($arg in $args) { if ($arg -eq '--stdin') { $useStdin = $true } else { $passArgs += $arg } }
$claudeArgs = @('-p', '--output-format', 'stream-json', '--verbose') + $passArgs
$script:resultText = $null
$script:isError = $false
$handle = {
  $line = [string]$_
  if (-not $line.StartsWith('{')) { return }
  try { $evt = $line | ConvertFrom-Json } catch { return }
  if ($evt.type -eq 'rate_limit_event' -and $evt.rate_limit_info) { Save-RateLimit $evt.rate_limit_info }
  elseif ($evt.type -eq 'result') {
    $script:resultText = $evt.result
    if ($evt.is_error) { $script:isError = $true }
  }
}
$ErrorActionPreference = 'Continue'
if ($useStdin) {
  $stdinText = [Console]::In.ReadToEnd()
  $stdinText | & $exe @claudeArgs | ForEach-Object $handle
} else {
  # An empty pipeline gives claude a closed standard input instead of the caller's.
  @() | & $exe @claudeArgs | ForEach-Object $handle
}
$code = $LASTEXITCODE
if ($null -ne $script:resultText) { Write-Output $script:resultText }
if ($code -ne 0) { exit $code }
if ($script:isError) { exit 1 }
exit 0
"#;

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("invalid_path")?;
    fs::create_dir_all(parent).map_err(|_| "claude_dir_unwritable")?;
    let tmp = path.with_extension("cockpit-tmp");
    fs::write(&tmp, content).map_err(|_| "claude_dir_unwritable")?;
    fs::rename(&tmp, path).map_err(|_| "claude_dir_unwritable".to_string())
}

/// Write the collector scripts and point Claude Code's status line at ours.
/// An existing, different status line is left untouched unless `replace` is set,
/// in which case the old settings file is backed up first.
fn install(base: &Path, replace: bool) -> Result<CollectorStatus, String> {
    let settings_file = settings_path(base);
    let mut settings: Value = if settings_file.exists() {
        // Never fall back to an empty object for a file we could not read: that would drop the user's settings.
        let (text, _) = read_small(&settings_file).ok_or("claude_settings_invalid")?;
        if text.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&text).map_err(|_| "claude_settings_invalid")?
        }
    } else {
        Value::Object(Default::default())
    };
    let Some(object) = settings.as_object_mut() else {
        return Err("claude_settings_invalid".into());
    };
    let existing = object
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if existing
        .as_deref()
        .is_some_and(|command| !is_our_command(command))
        && !replace
    {
        return Err("claude_statusline_conflict".into());
    }

    if cfg!(windows) {
        write_atomic(&statusline_script_path(base), STATUSLINE_PS1)?;
        write_atomic(&wrapper_path(base), WRAPPER_PS1)?;
    } else {
        write_atomic(&statusline_script_path(base), STATUSLINE_SH)?;
    }

    if settings_file.exists() {
        let backup = base.join("settings.json.cockpit-backup");
        if !backup.exists() || replace {
            fs::copy(&settings_file, &backup).map_err(|_| "claude_dir_unwritable")?;
        }
    }
    let mut status_line = serde_json::Map::new();
    status_line.insert("type".into(), Value::String("command".into()));
    status_line.insert("command".into(), Value::String(statusline_command(base)));
    status_line.insert("padding".into(), Value::from(0));
    object.insert("statusLine".into(), Value::Object(status_line));
    let text = serde_json::to_string_pretty(&settings).map_err(|_| "claude_settings_invalid")?;
    write_atomic(&settings_file, &(text + "\n"))?;
    Ok(collector_status(base))
}

#[tauri::command]
pub async fn install_claude_collector(replace: Option<bool>) -> Result<CollectorStatus, String> {
    tokio::task::spawn_blocking(move || {
        let base = config_dir().ok_or("claude_dir_missing")?;
        install(&base, replace.unwrap_or(false))
    })
    .await
    .map_err(|_| "claude_dir_unwritable".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cockpit-claude-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prices_match_most_specific_model() {
        assert_eq!(price_entry("claude-opus-5-5").unwrap().name, "Opus 5.5");
        assert_eq!(price_entry("claude-opus-5").unwrap().name, "Opus 5");
        assert_eq!(
            price_entry("claude-opus-4-1-20250805").unwrap().name,
            "Opus 4.1"
        );
        assert_eq!(
            price_entry("claude-opus-4-20250514").unwrap().name,
            "Opus 4"
        );
        assert_eq!(
            price_entry("claude-sonnet-4-5-20250929").unwrap().name,
            "Sonnet 4.5"
        );
        assert_eq!(
            price_entry("claude-sonnet-4-6[1m]").unwrap().name,
            "Sonnet 4.6"
        );
        assert_eq!(
            price_entry("claude-3-5-haiku-20241022").unwrap().name,
            "Haiku 3.5"
        );
        assert_eq!(
            price_entry("anthropic.claude-haiku-4-5").unwrap().name,
            "Haiku 4.5"
        );
        assert_eq!(price_entry("claude-fable-5-1").unwrap().name, "Fable 5.1");
        // Unknown future versions must not borrow an older price.
        assert!(price_entry("claude-opus-5-6").is_none());
        assert!(price_entry("gpt-5").is_none());
    }

    #[test]
    fn cost_uses_cache_rates_and_haiku_long_prompt_rule() {
        let tokens = Tokens {
            input: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            cache_read: 1_000_000,
            output: 1_000_000,
        };
        let cost = request_cost("claude-opus-5-5", &tokens).unwrap();
        assert!((cost - (4.0 + 5.0 + 8.0 + 0.2 + 20.0)).abs() < 1e-9);
        let short = Tokens {
            input: 50_000,
            output: 1_000_000,
            ..Tokens::default()
        };
        let short_cost = request_cost("claude-haiku-5-5", &short).unwrap();
        assert!((short_cost - (0.005 + 0.5)).abs() < 1e-9);
        let long = Tokens {
            input: 50_000,
            cache_read: 60_000,
            output: 1_000_000,
            ..Tokens::default()
        };
        let long_cost = request_cost("claude-haiku-5-5", &long).unwrap();
        assert!((long_cost - (0.025 + 0.003 + 2.5)).abs() < 1e-9);
        assert!(request_cost("mystery-model", &tokens).is_none());
    }

    #[test]
    fn parses_transcript_lines_and_dedupes_streaming_repeats() {
        let mut strings = Interner::default();
        let line = |out: u64| {
            format!(
                r#"{{"type":"assistant","cwd":"C:\\work\\demo","requestId":"req_1","timestamp":"2026-10-18T01:00:00Z","message":{{"id":"msg_1","model":"claude-opus-5-5","usage":{{"input_tokens":10,"cache_creation_input_tokens":100,"cache_creation":{{"ephemeral_5m_input_tokens":40,"ephemeral_1h_input_tokens":60}},"cache_read_input_tokens":1000,"output_tokens":{out}}}}}}}"#
            )
        };
        let mut map = HashMap::new();
        for out in [5, 50, 20] {
            let (key, record) = parse_line(&line(out), "fallback", &mut strings).unwrap();
            keep_larger(&mut map, key, record);
        }
        assert_eq!(map.len(), 1);
        let record = map.values().next().unwrap();
        assert_eq!(&*record.project, "demo");
        assert_eq!(
            record.tokens,
            Tokens {
                input: 10,
                cache_write_5m: 40,
                cache_write_1h: 60,
                cache_read: 1000,
                output: 50
            }
        );
        assert!(parse_line(
            r#"{"type":"user","message":{"usage":{}}}"#,
            "x",
            &mut strings
        )
        .is_none());
        assert!(parse_line(r#"{"type":"assistant","timestamp":"2026-10-18T01:00:00Z","uuid":"u","message":{"model":"<synthetic>","usage":{"input_tokens":1}}}"#, "x", &mut strings).is_none());
    }

    #[test]
    fn scans_appended_transcripts_incrementally() {
        let base = temp_dir("scan");
        let project = base.join("projects").join("C--work-demo");
        fs::create_dir_all(&project).unwrap();
        let file = project.join("session.jsonl");
        let entry = |id: &str| {
            format!(
            "{{\"type\":\"assistant\",\"requestId\":\"r{id}\",\"timestamp\":\"2026-10-18T03:00:00Z\",\"message\":{{\"id\":\"m{id}\",\"model\":\"claude-sonnet-5-5\",\"usage\":{{\"input_tokens\":1000,\"output_tokens\":100}}}}}}\n"
        )
        };
        fs::write(&file, entry("1")).unwrap();
        let (_, days) = scan(&base);
        assert_eq!(days.iter().map(|d| d.requests).sum::<u64>(), 1);
        let mut text = fs::read_to_string(&file).unwrap();
        text.push_str(&entry("2"));
        text.push_str("{\"partial\":"); // still being written
        fs::write(&file, text).unwrap();
        let (_, days) = scan(&base);
        assert_eq!(days.iter().map(|d| d.requests).sum::<u64>(), 2);
        let model = &days[0].models[0];
        assert_eq!(model.name, "Claude Sonnet 5.5");
        assert_eq!(model.total_tokens, 2200);
        assert!(
            (model.estimated_cost_usd - 2.0 * (1000.0 * 2.0 + 100.0 * 10.0) / 1e6).abs() < 1e-12
        );
        assert_eq!(days[0].projects[0].name, "C--work-demo");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn windows_reset_to_full_after_their_reset_time() {
        let now = 1_800_000_000;
        let observation = RateObservation {
            five_hour: Some(WindowObservation {
                used_percent: 40.0,
                resets_at: Some(now - 10),
                observed_at: now - 100,
            }),
            seven_day: Some(WindowObservation {
                used_percent: 25.0,
                resets_at: Some(now + 3600),
                observed_at: now - 100,
            }),
        };
        let snapshot = snapshot_from(observation, now).unwrap();
        assert_eq!(snapshot.provider, "claude");
        assert_eq!(
            snapshot.short_window.as_ref().unwrap().remaining_percent,
            100.0
        );
        assert!(snapshot.short_window.as_ref().unwrap().resets_at.is_none());
        assert_eq!(
            snapshot.weekly_window.as_ref().unwrap().remaining_percent,
            75.0
        );
        assert!(snapshot_from(RateObservation::default(), now).is_none());
    }

    #[test]
    fn reads_both_collector_formats_and_prefers_newest() {
        let base = temp_dir("rates");
        let dir = cockpit_dir(&base);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(RATE_FILE),
            "\u{feff}{\"observedAt\":1000,\"fiveHour\":{\"usedPercent\":12.5,\"resetsAt\":1700000000000,\"observedAt\":2000},\"sevenDay\":{\"usedPercent\":30,\"resetsAt\":1700000500}}",
        )
        .unwrap();
        fs::write(
            dir.join(RAW_STATUSLINE_FILE),
            "{\"rate_limits\":{\"five_hour\":{\"used_percentage\":50,\"resets_at\":1700000100}}}",
        )
        .unwrap();
        let observation = read_observation(&base);
        let seven = observation.seven_day.unwrap();
        assert_eq!(seven.used_percent, 30.0);
        assert_eq!(seven.observed_at, 1000);
        // The raw file's mtime (now) is newer than observedAt 2000.
        let five = observation.five_hour.unwrap();
        assert_eq!(five.used_percent, 50.0);
        assert_eq!(five.resets_at, Some(1_700_000_100));
        let parsed = parse_rate_file(
            "{\"fiveHour\":{\"usedPercent\":12.5,\"resetsAt\":1700000000000}}",
            5,
        );
        assert_eq!(parsed.five_hour.unwrap().resets_at, Some(1_700_000_000));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn install_preserves_settings_and_refuses_foreign_status_line() {
        let base = temp_dir("install");
        fs::write(
            settings_path(&base),
            "{\"model\":\"opus\",\"permissions\":{\"allow\":[\"Bash\"]}}",
        )
        .unwrap();
        let status = install(&base, false).unwrap();
        assert!(status.statusline_installed);
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(settings_path(&base)).unwrap()).unwrap();
        assert_eq!(settings["model"], "opus");
        assert_eq!(settings["permissions"]["allow"][0], "Bash");
        assert!(is_our_command(
            settings["statusLine"]["command"].as_str().unwrap()
        ));
        assert!(base.join("settings.json.cockpit-backup").exists());
        // Re-installing over our own command is fine.
        install(&base, false).unwrap();

        fs::write(
            settings_path(&base),
            "{\"statusLine\":{\"type\":\"command\",\"command\":\"my-line\"}}",
        )
        .unwrap();
        assert_eq!(
            install(&base, false).unwrap_err(),
            "claude_statusline_conflict"
        );
        assert!(collector_status(&base).statusline_conflict);
        assert!(install(&base, true).unwrap().statusline_installed);

        fs::write(settings_path(&base), "{not json").unwrap();
        assert_eq!(install(&base, true).unwrap_err(), "claude_settings_invalid");
        let _ = fs::remove_dir_all(&base);
    }
}
