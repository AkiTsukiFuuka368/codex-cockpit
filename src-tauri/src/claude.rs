//! Claude (Anthropic) support: subscription windows and local Claude Code token usage.
//!
//! Quota windows are never fetched with the user's credentials. They come from data
//! Claude Code itself hands to user scripts: the `rate_limits` object passed to a
//! status line command, and `rate_limit_event` messages in `--output-format stream-json`
//! runs. Small scripts written by this app store those values under
//! `<claude config dir>/cockpit/`, and this module only reads them.
//!
//! Token usage comes from Claude Code's local session transcripts
//! (`<claude config dir>/projects/**/*.jsonl`). Usage numbers, model ids, timestamps
//! and the project folder name are read. Session titles (or, when a session has
//! none, the first line of its first prompt) are shown locally and never stored.
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
    if let Some(custom) = crate::custom_prices::lookup(model) {
        let per = |count: u64, price: f64| count as f64 * price / 1_000_000.0;
        let (write_5m, write_1h) = match custom.cache_write {
            Some(write) => (write, write),
            None => (custom.input * 1.25, custom.input * 2.0),
        };
        return Some(
            per(tokens.input, custom.input)
                + per(tokens.cache_write_5m, write_5m)
                + per(tokens.cache_write_1h, write_1h)
                + per(tokens.cache_read, custom.cache_read)
                + per(tokens.output, custom.output),
        );
    }
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
    /// Request time, epoch seconds.
    ts: i64,
    model: Arc<str>,
    project: Arc<str>,
    /// Task (session or subagent) this request belongs to.
    task: Arc<str>,
    tokens: Tokens,
}

/// What a transcript file tells us about the task it records.
#[derive(Clone, Debug, Default)]
struct TaskMeta {
    id: String,
    parent: Option<String>,
    custom_title: Option<String>,
    summary: Option<String>,
    first_prompt: Option<String>,
    project: Option<String>,
}

#[derive(Default)]
struct FileState {
    len: u64,
    modified: Option<SystemTime>,
    offset: u64,
    records: HashMap<String, Record>,
    meta: TaskMeta,
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
    roots: Vec<PathBuf>,
    files: HashMap<PathBuf, FileState>,
    strings: Interner,
}

static CACHE: Mutex<Option<UsageCache>> = Mutex::new(None);

fn u64_at(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn folder_name(path: &str) -> Option<String> {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn project_name(line: &Value, fallback: &str) -> String {
    line.get("cwd")
        .and_then(Value::as_str)
        .and_then(folder_name)
        .unwrap_or_else(|| fallback.to_string())
}

const TITLE_CHARS: usize = 60;

fn short_title(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut title: String = line.chars().take(TITLE_CHARS).collect();
    if line.chars().count() > TITLE_CHARS {
        title.push('…');
    }
    Some(title)
}

/// The first thing the user typed, used as a title only when the session has none.
/// Command wrappers, tool results and injected reminders are skipped.
fn prompt_text(line: &Value) -> Option<String> {
    if line.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let content = line.get("message")?.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .find(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)?
            .to_string(),
        _ => return None,
    };
    let trimmed = text.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('<') || trimmed.starts_with("Caveat:") {
        return None;
    }
    short_title(trimmed)
}

/// Record titles and the first prompt; cheap checks first so usage lines stay fast.
fn observe_meta(text: &str, meta: &mut TaskMeta) {
    let wants_prompt = meta.first_prompt.is_none() && text.contains("\"type\":\"user\"");
    let wants_title = text.contains("\"type\":\"summary\"")
        || text.contains("\"type\":\"custom-title\"")
        || text.contains("\"type\":\"ai-title\"");
    let wants_project = meta.project.is_none() && text.contains("\"cwd\"");
    if !(wants_prompt || wants_title || wants_project) {
        return;
    }
    let Ok(line) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if wants_project {
        meta.project = line
            .get("cwd")
            .and_then(Value::as_str)
            .and_then(folder_name);
    }
    match line.get("type").and_then(Value::as_str) {
        Some("user") if wants_prompt => meta.first_prompt = prompt_text(&line),
        Some("custom-title") => {
            meta.custom_title = line
                .get("customTitle")
                .or_else(|| line.get("title"))
                .and_then(Value::as_str)
                .and_then(short_title)
        }
        Some("summary") | Some("ai-title") => {
            if let Some(title) = ["summary", "aiTitle", "title"]
                .iter()
                .find_map(|key| line.get(*key).and_then(Value::as_str))
                .and_then(short_title)
            {
                meta.summary = Some(title);
            }
        }
        _ => {}
    }
}

/// Parse one transcript line into (dedupe key, record).
fn parse_line(
    text: &str,
    fallback_project: &str,
    task: &str,
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
    let parsed = DateTime::parse_from_rfc3339(timestamp).ok()?;
    let ts = parsed.timestamp();
    let date = parsed.with_timezone(&Local).format("%Y-%m-%d").to_string();
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
            ts,
            model: strings.get(model),
            project: strings.get(&project_name(&line, fallback_project)),
            task: strings.get(task),
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

/// `<project>/<session>.jsonl` is a session; `<project>/<session>/subagents/<agent>.jsonl`
/// is a subagent run that belongs to that session.
fn task_identity(file: &Path) -> (String, Option<String>) {
    let stem = file
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());
    let parent_dir = file.parent();
    let is_subagent = parent_dir
        .and_then(Path::file_name)
        .is_some_and(|name| name == "subagents");
    if is_subagent {
        if let Some(session) = parent_dir
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().to_string())
        {
            return (format!("{session}/{stem}"), Some(session));
        }
    }
    (stem, None)
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
    if state.meta.id.is_empty() {
        let (id, parent) = task_identity(path);
        state.meta.id = id;
        state.meta.parent = parent;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let project = fallback_project(root, path);
    let task = state.meta.id.clone();
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
            let text = text.trim_end();
            if let Some((key, record)) = parse_line(text, &project, &task, strings) {
                keep_larger(&mut state.records, key, record);
            } else {
                observe_meta(text, &mut state.meta);
            }
        }
    }
    state.offset = offset;
    state.len = len;
    state.modified = modified;
}

// ----- Aggregation into the same shapes the Codex usage page reads -----

/// Token and cost totals for one model on one day.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ModelTotals {
    tokens: Tokens,
    requests: u64,
    cost: f64,
    /// Some requests used a model without a known price.
    unpriced: bool,
}

impl ModelTotals {
    fn add(&mut self, record: &Record) {
        self.tokens.input += record.tokens.input;
        self.tokens.cache_write_5m += record.tokens.cache_write_5m;
        self.tokens.cache_write_1h += record.tokens.cache_write_1h;
        self.tokens.cache_read += record.tokens.cache_read;
        self.tokens.output += record.tokens.output;
        self.requests += 1;
        match request_cost(&record.model, &record.tokens) {
            Some(cost) => self.cost += cost,
            None => self.unpriced = true,
        }
    }
}

/// date → model → totals
type DailyLedger = BTreeMap<Arc<str>, BTreeMap<Arc<str>, ModelTotals>>;

fn add_to(ledger: &mut DailyLedger, record: &Record) {
    ledger
        .entry(record.date.clone())
        .or_default()
        .entry(record.model.clone())
        .or_default()
        .add(record);
}

/// Cache writes are billed as (more expensive) input, so they count as uncached input here.
fn metrics_json(totals: &ModelTotals) -> serde_json::Map<String, Value> {
    let tokens = &totals.tokens;
    let mut map = serde_json::Map::new();
    map.insert(
        "inputTokens".into(),
        Value::from(tokens.input + tokens.cache_write_5m + tokens.cache_write_1h),
    );
    map.insert("cachedInputTokens".into(), Value::from(tokens.cache_read));
    map.insert("outputTokens".into(), Value::from(tokens.output));
    map.insert("reasoningTokens".into(), Value::from(0));
    map.insert("totalTokens".into(), Value::from(tokens.total()));
    map.insert(
        "estimatedCostUsd".into(),
        if totals.unpriced {
            Value::Null
        } else {
            Value::from(totals.cost)
        },
    );
    map
}

/// `Record<date, DailyModelUsage>` as used by the Codex usage page.
fn daily_json(ledger: &DailyLedger) -> Value {
    let mut days = serde_json::Map::new();
    for (date, models) in ledger {
        let mut total = ModelTotals::default();
        let mut rows = Vec::new();
        for (model, totals) in models {
            total.tokens.input += totals.tokens.input;
            total.tokens.cache_write_5m += totals.tokens.cache_write_5m;
            total.tokens.cache_write_1h += totals.tokens.cache_write_1h;
            total.tokens.cache_read += totals.tokens.cache_read;
            total.tokens.output += totals.tokens.output;
            total.requests += totals.requests;
            total.cost += totals.cost;
            total.unpriced |= totals.unpriced;
            let mut row = metrics_json(totals);
            row.insert("id".into(), Value::from(model.to_string()));
            row.insert("name".into(), Value::from(display_model_name(model)));
            rows.push(Value::Object(row));
        }
        let mut day = metrics_json(&total);
        day.insert("models".into(), Value::Array(rows));
        days.insert(date.to_string(), Value::Object(day));
    }
    Value::Object(days)
}

struct ScanResult {
    files: usize,
    records: Vec<Record>,
    tasks: Vec<TaskMeta>,
    sources: Vec<SourceInfo>,
}

/// One folder of Claude Code transcripts (`<config>/projects`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptRoot {
    pub path: PathBuf,
    /// `cli` (Claude Code's own folder), `desktop` (Claude desktop app) or `cowork`.
    pub kind: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub kind: String,
    pub files: usize,
    pub records: usize,
}

const MAX_DISCOVERY_DEPTH: usize = 7;
const MAX_EXTRA_ROOTS: usize = 200;
/// Folders inside the desktop app's data that never hold transcripts.
const SKIP_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "ShaderCache",
    "IndexedDB",
    "Local Storage",
    "Session Storage",
    "blob_storage",
    "Service Worker",
    "Crashpad",
    "logs",
    "node_modules",
    "vm_bundles",
    "claude-code",
    "Partitions",
    "WebStorage",
    "shared_proto_db",
    "VideoDecodeStats",
];

/// Folders the Claude desktop app keeps its own data in (Code tab, Cowork).
fn desktop_data_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(config) = dirs::config_dir() {
        out.push(config.join("Claude"));
    }
    if let Some(local) = dirs::data_local_dir() {
        // Microsoft Store (MSIX) installs keep AppData\Roaming inside the package.
        if let Ok(entries) = fs::read_dir(local.join("Packages")) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("Claude_") {
                    out.push(
                        entry
                            .path()
                            .join("LocalCache")
                            .join("Roaming")
                            .join("Claude"),
                    );
                }
            }
        }
    }
    out
}

/// `.claude/projects` folders under `base`, a few levels deep.
fn find_project_dirs(base: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![(base.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if out.len() >= MAX_EXTRA_ROOTS {
                return;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if name == "projects" && dir.file_name().is_some_and(|parent| parent == ".claude") {
                out.push(path);
                continue;
            }
            if depth < MAX_DISCOVERY_DEPTH && !SKIP_DIRS.contains(&name.as_str()) {
                stack.push((path, depth + 1));
            }
        }
    }
}

fn root_kind(path: &Path) -> &'static str {
    let text = path.to_string_lossy().to_lowercase();
    if text.contains("local-agent-mode") || text.contains("cowork") {
        "cowork"
    } else {
        "desktop"
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Every transcript folder on this computer: Claude Code's own first, then any the
/// desktop app keeps (its Code tab and Cowork sessions can use their own config folder).
fn roots_from(cli_base: Option<&Path>, desktop_dirs: &[PathBuf]) -> Vec<TranscriptRoot> {
    let mut roots: Vec<TranscriptRoot> = Vec::new();
    if let Some(base) = cli_base {
        roots.push(TranscriptRoot {
            path: base.join("projects"),
            kind: "cli",
        });
    }
    let mut found = Vec::new();
    for dir in desktop_dirs {
        find_project_dirs(dir, &mut found);
    }
    found.sort();
    for path in found {
        if roots.iter().any(|root| same_dir(&root.path, &path)) {
            continue;
        }
        let kind = root_kind(&path);
        roots.push(TranscriptRoot { path, kind });
    }
    roots
}

/// Looking through the desktop app's folders is slower than reading transcripts,
/// so the folder list is reused for a few minutes.
static ROOTS: Mutex<Option<(std::time::Instant, Vec<TranscriptRoot>)>> = Mutex::new(None);
const ROOTS_TTL: std::time::Duration = std::time::Duration::from_secs(300);

pub fn transcript_roots() -> Vec<TranscriptRoot> {
    let mut guard = match ROOTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some((at, roots)) = guard.as_ref() {
        if at.elapsed() < ROOTS_TTL {
            return roots.clone();
        }
    }
    let roots = roots_from(claude_base().as_deref(), &desktop_data_dirs());
    *guard = Some((std::time::Instant::now(), roots.clone()));
    roots
}

fn scan(roots: &[TranscriptRoot]) -> ScanResult {
    let mut guard = match CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let cache = guard.get_or_insert_with(UsageCache::default);
    let root_paths: Vec<PathBuf> = roots.iter().map(|root| root.path.clone()).collect();
    if cache.roots != root_paths {
        *cache = UsageCache {
            roots: root_paths,
            ..UsageCache::default()
        };
    }
    let mut per_root: Vec<Vec<PathBuf>> = Vec::new();
    let mut total_files = 0usize;
    for root in roots {
        let mut files = Vec::new();
        collect_files(&root.path, &mut files);
        total_files += files.len();
        per_root.push(files);
    }
    let present: std::collections::HashSet<&PathBuf> = per_root.iter().flatten().collect();
    cache.files.retain(|path, _| present.contains(path));
    let UsageCache {
        files: states,
        strings,
        ..
    } = cache;
    for (root, files) in roots.iter().zip(&per_root) {
        for path in files {
            let state = states.entry(path.clone()).or_default();
            refresh_file(&root.path, path, state, strings);
        }
    }
    // The same message can appear in several files (resumed or forked sessions, or the
    // same session seen from two folders): count it once, in the oldest file.
    let mut ordered: Vec<(&PathBuf, &FileState)> = states.iter().collect();
    ordered.sort_by(|a, b| a.1.modified.cmp(&b.1.modified).then(a.0.cmp(b.0)));
    let mut merged: HashMap<&String, (&Record, &PathBuf)> = HashMap::new();
    for (path, state) in &ordered {
        for (key, record) in &state.records {
            match merged.get(key) {
                Some((existing, _)) if existing.tokens.total() >= record.tokens.total() => {}
                _ => {
                    merged.insert(key, (record, path));
                }
            }
        }
    }
    let sources = roots
        .iter()
        .zip(&per_root)
        .map(|(root, files)| SourceInfo {
            path: root.path.to_string_lossy().to_string(),
            kind: root.kind.to_string(),
            files: files.len(),
            records: merged
                .values()
                .filter(|(_, path)| path.starts_with(&root.path))
                .count(),
        })
        .collect();
    ScanResult {
        files: total_files,
        records: merged
            .into_values()
            .map(|(record, _)| record.clone())
            .collect(),
        tasks: ordered
            .iter()
            .map(|(_, state)| state.meta.clone())
            .collect(),
        sources,
    }
}

fn device_id() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "local".into())
}

pub const CLAUDE_GROUP_ID: &str = "claude-local";
const PRICING_UPDATED_AT: &str = "2026-10-18";

fn task_name(meta: &TaskMeta, zh_fallback: bool) -> String {
    meta.custom_title
        .clone()
        .or_else(|| meta.summary.clone())
        .or_else(|| meta.first_prompt.clone())
        .unwrap_or_else(|| {
            let short: String = meta
                .id
                .rsplit('/')
                .next()
                .unwrap_or(&meta.id)
                .chars()
                .take(8)
                .collect();
            if zh_fallback {
                format!("会话 {short}")
            } else {
                format!("Session {short}")
            }
        })
}

/// Both payloads the Codex usage page reads, built from Claude Code transcripts.
fn ledgers(roots: &[TranscriptRoot]) -> (Value, Value) {
    let now = Utc::now().to_rfc3339();
    let device = device_id();
    if roots.is_empty() {
        let usage = serde_json::json!({
            "groups": [{ "id": CLAUDE_GROUP_ID, "name": device, "deviceIds": [device] }],
            "defaultGroupId": CLAUDE_GROUP_ID, "localGroupId": CLAUDE_GROUP_ID,
            "fetchedAt": now, "status": "unavailable", "devices": [],
            "warnings": ["claude_directory_missing"], "projectBreakdownAvailable": true,
        });
        let projects = serde_json::json!({
            "deviceId": device, "updatedAt": now, "status": "unavailable", "coverage": "local",
            "scannedFiles": 0, "pricingSource": "bundled-catalog", "pricingUpdatedAt": PRICING_UPDATED_AT,
            "taskMetadataCoverage": "complete", "projects": [], "tasks": [], "peerTasks": [],
            "warnings": ["claude_directory_missing"],
        });
        return (usage, projects);
    }
    let result = scan(roots);
    let mut device_ledger = DailyLedger::new();
    let mut project_ledgers: BTreeMap<Arc<str>, DailyLedger> = BTreeMap::new();
    let mut task_ledgers: HashMap<Arc<str>, DailyLedger> = HashMap::new();
    let mut task_projects: HashMap<Arc<str>, Arc<str>> = HashMap::new();
    let mut unpriced = false;
    for record in &result.records {
        unpriced |= request_cost(&record.model, &record.tokens).is_none();
        add_to(&mut device_ledger, record);
        add_to(
            project_ledgers.entry(record.project.clone()).or_default(),
            record,
        );
        add_to(task_ledgers.entry(record.task.clone()).or_default(), record);
        task_projects
            .entry(record.task.clone())
            .or_insert_with(|| record.project.clone());
    }
    let mut warnings = Vec::new();
    if result.records.is_empty() {
        warnings.push("no_usage_records");
    }
    if unpriced {
        warnings.push("missing_model_price");
    }
    let known: std::collections::HashSet<&str> =
        result.tasks.iter().map(|meta| meta.id.as_str()).collect();
    let tasks: Vec<Value> = result
        .tasks
        .iter()
        .filter_map(|meta| {
            let ledger = task_ledgers.get(meta.id.as_str())?;
            let project = task_projects
                .get(meta.id.as_str())
                .map(|value| value.to_string())
                .or_else(|| meta.project.clone());
            let parent = meta.parent.as_deref().filter(|id| known.contains(id));
            let mut task = serde_json::json!({
                "id": meta.id,
                "name": task_name(meta, true),
                "relation": if parent.is_some() { "subagent" } else { "root" },
                "daily": daily_json(ledger),
            });
            if let Some(project) = project {
                task["projectName"] = Value::from(project);
            }
            if let Some(parent) = parent {
                task["parentId"] = Value::from(parent);
                task["rootId"] = Value::from(parent);
                task["agentRole"] = Value::from("subagent");
            } else {
                task["rootId"] = Value::from(meta.id.clone());
            }
            Some(task)
        })
        .collect();
    let projects: Vec<Value> = project_ledgers
        .iter()
        .map(|(name, ledger)| {
            serde_json::json!({ "id": name.to_string(), "name": name.to_string(), "daily": daily_json(ledger) })
        })
        .collect();
    let status = "ready";
    let usage = serde_json::json!({
        "groups": [{ "id": CLAUDE_GROUP_ID, "name": device, "deviceIds": [device] }],
        "defaultGroupId": CLAUDE_GROUP_ID, "localGroupId": CLAUDE_GROUP_ID,
        "fetchedAt": now, "status": status,
        "devices": if device_ledger.is_empty() { Vec::new() } else { vec![serde_json::json!({
            "id": device, "updatedAt": now, "stale": false, "daily": daily_json(&device_ledger), "ranges": {},
        })] },
        "warnings": warnings, "projectBreakdownAvailable": true,
    });
    let projects = serde_json::json!({
        "deviceId": device, "updatedAt": now, "status": status, "coverage": "local",
        "scannedFiles": result.files, "pricingSource": "bundled-catalog", "pricingUpdatedAt": PRICING_UPDATED_AT,
        "taskMetadataCoverage": "complete", "projects": projects, "tasks": tasks, "peerTasks": [],
        "warnings": warnings, "sources": result.sources,
    });
    (usage, projects)
}

fn claude_base() -> Option<PathBuf> {
    config_dir().filter(|path| path.is_dir())
}

/// Token ledger for the Claude usage page (same shape as `get_tokei_usage`).
#[tauri::command]
pub async fn get_claude_tokei_usage(app: tauri::AppHandle) -> Result<Value, String> {
    if let Some(dir) = crate::custom_prices::data_dir(&app) {
        crate::custom_prices::refresh(&dir);
    }
    tokio::task::spawn_blocking(|| ledgers(&transcript_roots()).0)
        .await
        .map_err(|_| "claude_usage_unavailable".to_string())
}

/// Projects and tasks for the Claude usage page (same shape as `get_codex_project_usage`).
#[tauri::command]
pub async fn get_claude_project_usage(app: tauri::AppHandle) -> Result<Value, String> {
    if let Some(dir) = crate::custom_prices::data_dir(&app) {
        crate::custom_prices::refresh(&dir);
    }
    tokio::task::spawn_blocking(|| ledgers(&transcript_roots()).1)
        .await
        .map_err(|_| "claude_usage_unavailable".to_string())
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

/// Setup state and the price table for the Claude page's extras.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSetup {
    /// False when there is no Claude Code folder on this computer.
    pub available: bool,
    pub directory: Option<String>,
    pub prices: Vec<PriceRow>,
    pub price_source: String,
    pub collector: CollectorStatus,
    /// Transcript folders found on this computer.
    pub sources: Vec<SourceInfo>,
    /// Last quota readings next to what this computer recorded in the same windows.
    pub windows: Vec<WindowCheck>,
}

/// One quota window: the last reading, and local usage since the window began.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowCheck {
    /// `fiveHour` or `sevenDay`.
    pub window: String,
    pub used_percent: f64,
    pub observed_at: String,
    pub window_start: String,
    pub resets_at: String,
    /// Local requests between the window start and the reading.
    pub local_tokens: u64,
    pub local_cost_usd: Option<f64>,
    /// Local requests after the reading (not yet reflected in the percentage).
    pub tokens_after_reading: u64,
    pub cost_after_reading: Option<f64>,
}

#[derive(Default)]
struct Tally {
    tokens: u64,
    cost: f64,
    unpriced: bool,
}

impl Tally {
    fn add(&mut self, record: &Record) {
        self.tokens += record.tokens.total();
        match request_cost(&record.model, &record.tokens) {
            Some(cost) => self.cost += cost,
            None => self.unpriced = true,
        }
    }
    fn cost(&self) -> Option<f64> {
        (!self.unpriced).then_some(self.cost)
    }
}

fn window_check(
    key: &str,
    observation: Option<WindowObservation>,
    window_seconds: i64,
    records: &[Record],
    now: i64,
) -> Option<WindowCheck> {
    let observation = observation?;
    let resets_at = observation.resets_at?;
    if resets_at <= now {
        return None;
    }
    let start = resets_at - window_seconds;
    let mut before = Tally::default();
    let mut after = Tally::default();
    for record in records.iter().filter(|record| record.ts >= start) {
        if record.ts <= observation.observed_at {
            before.add(record);
        } else {
            after.add(record);
        }
    }
    Some(WindowCheck {
        window: key.to_string(),
        used_percent: observation.used_percent,
        observed_at: rfc3339(observation.observed_at)?,
        window_start: rfc3339(start)?,
        resets_at: rfc3339(resets_at)?,
        local_tokens: before.tokens,
        local_cost_usd: before.cost(),
        tokens_after_reading: after.tokens,
        cost_after_reading: after.cost(),
    })
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

fn read_setup() -> ClaudeSetup {
    let base = claude_base();
    let roots = transcript_roots();
    let result = scan(&roots);
    let now = Utc::now().timestamp();
    let observation = base.as_deref().map(read_observation).unwrap_or_default();
    let windows = [
        window_check(
            "fiveHour",
            observation.five_hour,
            5 * 3600,
            &result.records,
            now,
        ),
        window_check(
            "sevenDay",
            observation.seven_day,
            7 * 24 * 3600,
            &result.records,
            now,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    ClaudeSetup {
        sources: result.sources,
        windows,
        available: base.is_some(),
        directory: config_dir().map(|path| path.to_string_lossy().to_string()),
        prices: price_rows(),
        price_source: format!("Anthropic API pricing page, {PRICING_UPDATED_AT}"),
        collector: match base {
            Some(base) => collector_status(&base),
            None => CollectorStatus {
                statusline_installed: false,
                statusline_conflict: false,
                wrapper_path: None,
                last_observed_at: None,
            },
        },
    }
}

#[tauri::command]
pub async fn get_claude_setup(app: tauri::AppHandle) -> Result<ClaudeSetup, String> {
    if let Some(dir) = crate::custom_prices::data_dir(&app) {
        crate::custom_prices::refresh(&dir);
    }
    tokio::task::spawn_blocking(read_setup)
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
const WRAPPER_PS1: &str = include_str!("../../scripts/claude-run.ps1");

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
            let (key, record) = parse_line(&line(out), "fallback", "task", &mut strings).unwrap();
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
            "task",
            &mut strings
        )
        .is_none());
        assert!(parse_line(r#"{"type":"assistant","timestamp":"2026-10-18T01:00:00Z","uuid":"u","message":{"model":"<synthetic>","usage":{"input_tokens":1}}}"#, "x", "task", &mut strings).is_none());
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
        let roots = roots_from(Some(&base), &[]);
        assert_eq!(scan(&roots).records.len(), 1);
        let mut text = fs::read_to_string(&file).unwrap();
        text.push_str(&entry("2"));
        text.push_str("{\"partial\":"); // still being written
        fs::write(&file, text).unwrap();
        let result = scan(&roots);
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.files, 1);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ledgers_match_the_codex_usage_page_shapes() {
        let base = temp_dir("ledgers");
        let project = base.join("projects").join("C--work-demo");
        let sub = project.join("sess1").join("subagents");
        fs::create_dir_all(&sub).unwrap();
        let usage = |id: &str, model: &str, cwd: &str| {
            format!(
            "{{\"type\":\"assistant\",\"cwd\":\"{cwd}\",\"requestId\":\"r{id}\",\"timestamp\":\"2026-10-18T03:00:00Z\",\"message\":{{\"id\":\"m{id}\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":1000,\"cache_creation_input_tokens\":200,\"cache_read_input_tokens\":5000,\"output_tokens\":100}}}}}}\n"
        )
        };
        let main = format!(
            "{}{}{}{}",
            "{\"type\":\"user\",\"cwd\":\"C:\\\\work\\\\demo\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/clear</command-name>\"}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"修复登录页面的样式问题\\n细节...\"}]}}\n",
            usage("1", "claude-opus-5-5", "C:\\\\work\\\\demo"),
            usage("2", "mystery-model", "C:\\\\work\\\\demo"),
        );
        fs::write(project.join("sess1.jsonl"), main).unwrap();
        fs::write(
            sub.join("agent-a1.jsonl"),
            usage("3", "claude-haiku-5-5", "C:\\\\work\\\\demo"),
        )
        .unwrap();
        // A resumed session repeats message 1; it must count once.
        fs::write(
            project.join("sess2.jsonl"),
            format!(
                "{{\"type\":\"summary\",\"summary\":\"续写会话\"}}\n{}",
                usage("1", "claude-opus-5-5", "C:\\\\work\\\\demo")
            ),
        )
        .unwrap();
        let (usage_json, projects_json) = ledgers(&roots_from(Some(&base), &[]));
        let device = &usage_json["devices"][0];
        let day = &device["daily"]["2026-10-18"];
        assert!(day.is_object(), "{usage_json}");
        assert_eq!(day["totalTokens"], 3 * 6300);
        assert_eq!(day["inputTokens"], 3 * 1200);
        assert_eq!(day["cachedInputTokens"], 3 * 5000);
        assert!(
            day["estimatedCostUsd"].is_null(),
            "unknown model makes the day partial"
        );
        let models = day["models"].as_array().unwrap();
        let opus = models
            .iter()
            .find(|m| m["id"] == "claude-opus-5-5")
            .unwrap();
        assert_eq!(opus["name"], "Claude Opus 5.5");
        let expected = (1000.0 * 4.0 + 200.0 * 5.0 + 5000.0 * 0.2 + 100.0 * 20.0) / 1e6;
        assert!((opus["estimatedCostUsd"].as_f64().unwrap() - expected).abs() < 1e-12);
        assert!(models
            .iter()
            .any(|m| m["id"] == "mystery-model" && m["estimatedCostUsd"].is_null()));
        assert_eq!(usage_json["groups"][0]["deviceIds"][0], device["id"]);
        assert_eq!(projects_json["deviceId"], device["id"]);
        assert_eq!(projects_json["projects"][0]["name"], "demo");
        assert!(projects_json["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "missing_model_price"));
        let tasks = projects_json["tasks"].as_array().unwrap();
        let root = tasks.iter().find(|t| t["id"] == "sess1").unwrap();
        assert_eq!(root["name"], "修复登录页面的样式问题");
        assert_eq!(root["relation"], "root");
        assert_eq!(root["projectName"], "demo");
        let child = tasks.iter().find(|t| t["id"] == "sess1/agent-a1").unwrap();
        assert_eq!(child["relation"], "subagent");
        assert_eq!(child["parentId"], "sess1");
        // sess2 only repeated an already-counted message, so it has no usage of its own.
        assert!(tasks.iter().all(|t| t["id"] != "sess2"));
        let _ = fs::remove_dir_all(&base);

        let (missing, _) = ledgers(&[]);
        assert_eq!(missing["status"], "unavailable");
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

    #[test]
    fn finds_desktop_transcripts_and_counts_shared_messages_once() {
        let base = temp_dir("roots-cli");
        let app = temp_dir("roots-app");
        let cli_project = base.join("projects").join("C--work");
        let cowork_project = app
            .join("local-agent-mode-sessions")
            .join("acct")
            .join("local_1")
            .join(".claude")
            .join("projects")
            .join("-sessions-x");
        let ignored = app.join("Cache").join(".claude").join("projects").join("p");
        for dir in [&cli_project, &cowork_project, &ignored] {
            fs::create_dir_all(dir).unwrap();
        }
        let entry = |id: &str| {
            format!(
            "{{\"type\":\"assistant\",\"requestId\":\"r{id}\",\"timestamp\":\"2026-10-18T03:00:00Z\",\"message\":{{\"id\":\"m{id}\",\"model\":\"claude-opus-5-5\",\"usage\":{{\"input_tokens\":1000,\"output_tokens\":100}}}}}}\n"
        )
        };
        fs::write(cli_project.join("a.jsonl"), entry("1")).unwrap();
        fs::write(
            cowork_project.join("b.jsonl"),
            format!("{}{}", entry("1"), entry("2")),
        )
        .unwrap();
        fs::write(ignored.join("c.jsonl"), entry("3")).unwrap();
        let roots = roots_from(Some(&base), &[app.clone()]);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].kind, "cli");
        assert_eq!(roots[1].kind, "cowork");
        let result = scan(&roots);
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.sources.len(), 2);
        assert_eq!(result.sources.iter().map(|s| s.records).sum::<usize>(), 2);
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn window_check_splits_local_usage_at_the_reading() {
        let mut strings = Interner::default();
        let mut record = |ts: i64| Record {
            date: strings.get("2026-10-20"),
            ts,
            model: strings.get("claude-opus-5-5"),
            project: strings.get("p"),
            task: strings.get("t"),
            tokens: Tokens {
                input: 1_000_000,
                ..Tokens::default()
            },
        };
        let now = 1_000_000;
        let reset = now + 3600;
        let start = reset - 7 * 24 * 3600;
        let records = vec![
            record(start - 10),
            record(start + 10),
            record(now - 100),
            record(now - 10),
        ];
        let observation = WindowObservation {
            used_percent: 40.0,
            resets_at: Some(reset),
            observed_at: now - 50,
        };
        let check =
            window_check("sevenDay", Some(observation), 7 * 24 * 3600, &records, now).unwrap();
        assert_eq!(check.local_tokens, 2_000_000);
        assert_eq!(check.tokens_after_reading, 1_000_000);
        assert!((check.local_cost_usd.unwrap() - 8.0).abs() < 1e-9);
        let expired = WindowObservation {
            resets_at: Some(now - 1),
            ..observation
        };
        assert!(window_check("sevenDay", Some(expired), 7 * 24 * 3600, &records, now).is_none());
    }
}
