//! How much of the member's weekly CLI limit is left - the great crystal shines with it.
//!
//! Claude Code: the endpoint behind Claude Code's `/usage`, asked with the login Claude Code
//! keeps in `~/.claude/.credentials.json`. It is not a documented API and may change. The USAGE
//! card and the crystal share one answer, asked at most every five minutes; when an ask fails the
//! last good numbers stay (dated, so the card can say how old they are), and with none the crystal
//! simply keeps its normal glow. The token is read for each ask, goes out only in the
//! Authorization header, and is never kept, logged or sent to a page.
//!
//! Codex: its session files carry the limits it was last told (`token_count` lines with
//! `rate_limits`); the newest one is read.

use crate::settings::{Cli, SettingsStore};
use crate::transcript::codex;
use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

const CLAUDE_USAGE: &str = "https://api.anthropic.com/api/oauth/usage";
const EVERY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub cli: String,
    /// 0..100 of the weekly limit still left.
    pub left_percent: f64,
    /// Milliseconds since 1970, when known.
    pub resets_at: Option<i64>,
    /// What the number is, in words ("weekly limit").
    pub what: String,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// "2026-11-12T21:00:00.125+00:00" (or Z, or no zone = UTC) -> milliseconds since 1970.
pub fn iso_ms(text: &str) -> Option<i64> {
    let t = text.trim();
    let num = |a: usize, b: usize| t.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut rest = t.get(19..)?;
    if let Some(frac) = rest.strip_prefix('.') {
        rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset_min = match rest {
        "" | "Z" | "z" => 0,
        zone => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let hh = zone.get(1..3)?.parse::<i64>().ok()?;
            let mm = zone.get(zone.len() - 2..)?.parse::<i64>().ok()?;
            sign * (hh * 60 + mm)
        }
    };
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + h * 3600 + mi * 60 + s - offset_min * 60) * 1000)
}

fn quota(cli: &str, used: f64, resets_at: Option<i64>) -> Quota {
    Quota { cli: cli.into(), left_percent: (100.0 - used).clamp(0.0, 100.0), resets_at, what: "weekly limit".into() }
}

/// Claude Code's usage answer -> the tightest weekly limit (the one with the least left).
pub fn parse_claude(answer: &Value) -> Option<Quota> {
    let none = Vec::new();
    let weekly = answer
        .get("limits")
        .and_then(Value::as_array)
        .unwrap_or(&none)
        .iter()
        .filter(|l| l.get("group").and_then(Value::as_str) == Some("weekly"))
        .filter_map(|l| Some((l.get("percent")?.as_f64()?, l.get("resets_at").and_then(Value::as_str).and_then(iso_ms))))
        .max_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((used, resets)) = weekly {
        return Some(quota("claude", used, resets));
    }
    let week = answer.get("seven_day")?;
    Some(quota("claude", week.get("utilization")?.as_f64()?, week.get("resets_at").and_then(Value::as_str).and_then(iso_ms)))
}

/// One Codex session line -> the weekly limit it reports (the secondary window, else the primary).
pub fn parse_codex(line: &str, now: i64) -> Option<Quota> {
    let v: Value = serde_json::from_str(line).ok()?;
    let payload = v.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let limits = payload.get("rate_limits").or_else(|| payload.pointer("/info/rate_limits"))?;
    let window = limits.get("secondary").filter(|w| !w.is_null()).or_else(|| limits.get("primary"))?;
    let used = window.get("used_percent")?.as_f64()?;
    let resets = window
        .get("resets_at")
        .and_then(|r| r.as_i64().map(|s| s * 1000).or_else(|| r.as_str().and_then(iso_ms)))
        .or_else(|| window.get("resets_in_seconds").and_then(Value::as_i64).map(|s| now + s * 1000));
    Some(quota("codex", used, resets))
}

/// The last limits line in a Codex session file.
fn last_codex_quota(path: &Path) -> Option<Quota> {
    let file = std::fs::File::open(path).ok()?;
    let now = now_ms();
    BufReader::new(file).lines().map_while(Result::ok).filter_map(|l| parse_codex(&l, now)).last()
}

/// The newest Codex session files from the last eight days, newest first.
fn codex_files() -> Vec<PathBuf> {
    let Some(root) = codex::sessions_root() else { return Vec::new() };
    let Some(week_ago) = SystemTime::now().checked_sub(Duration::from_secs(8 * 86_400)) else { return Vec::new() };
    let mut files = codex::recent_rollouts(&root, week_ago);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().take(5).map(|(_, path)| path).collect()
}

fn codex_quota() -> Option<Quota> {
    codex_files().iter().find_map(|path| last_codex_quota(path))
}

fn claude_token() -> Option<String> {
    let path: PathBuf = PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?).join(".claude").join(".credentials.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    v.pointer("/claudeAiOauth/accessToken")?.as_str().map(str::to_string).filter(|t| !t.is_empty())
}

/// Claude Code's `/usage` answer, or why there is none.
fn claude_answer() -> Result<Value, &'static str> {
    let token = claude_token().ok_or("signed_out")?;
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|_| "unknown")?;
    let answer = client.get(CLAUDE_USAGE).bearer_auth(token).header("anthropic-beta", "oauth-2025-04-20").send().map_err(|_| "unknown")?;
    if !answer.status().is_success() {
        return Err("unknown");
    }
    answer.json().map_err(|_| "unknown")
}

/// What Castle last heard from Claude Code's usage endpoint. The USAGE card and the crystal both
/// read this one answer, so Anthropic is asked at most once every five minutes for the two.
/// It holds the usage numbers only - never the login.
#[derive(Debug, Clone)]
struct ClaudeCache {
    /// When the endpoint was last asked, answered or not.
    asked: Instant,
    /// Why that ask gave nothing ("signed_out" or "unknown"); None when it answered.
    failed: Option<&'static str>,
    /// The last good answer and when it came, in milliseconds since 1970.
    good: Option<(i64, Value)>,
}

/// An answer younger than five minutes is used again instead of asking.
fn still_fresh(cache: &ClaudeCache, now: Instant) -> bool {
    now.saturating_duration_since(cache.asked) < EVERY
}

/// The cache after one more ask. A network failure or a refused ask keeps the last good numbers
/// (the card shows how old they are); signing out drops them, since they belong to a login that
/// is gone.
fn remember(previous: Option<ClaudeCache>, outcome: Result<Value, &'static str>, asked: Instant, now: i64) -> ClaudeCache {
    match outcome {
        Ok(answer) => ClaudeCache { asked, failed: None, good: Some((now, answer)) },
        Err("signed_out") => ClaudeCache { asked, failed: Some("signed_out"), good: None },
        Err(why) => ClaudeCache { asked, failed: Some(why), good: previous.and_then(|p| p.good) },
    }
}

/// The USAGE card's Claude Code line from the cache: the last good numbers, dated when they
/// came, or why there are none.
fn claude_card(cache: &ClaudeCache) -> CliUsage {
    match &cache.good {
        Some((at, answer)) => parse_claude_usage(answer, *at),
        None => usage(Cli::Claude, cache.failed.unwrap_or("unknown")),
    }
}

/// The crystal's weekly limit from the cache; None = unknown.
fn claude_glow(cache: &ClaudeCache) -> Option<Quota> {
    cache.good.as_ref().and_then(|(_, answer)| parse_claude(answer))
}

static CLAUDE: Mutex<Option<ClaudeCache>> = Mutex::new(None);

/// The shared Claude Code answer, asked again only when it is over five minutes old. The lock is
/// held while asking, so the card and the crystal starting together still make one ask.
fn claude_cached() -> ClaudeCache {
    let mut slot = CLAUDE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();
    if let Some(cache) = slot.as_ref().filter(|c| still_fresh(c, now)) {
        return cache.clone();
    }
    let fresh = remember(slot.take(), claude_answer(), now, now_ms());
    *slot = Some(fresh.clone());
    fresh
}

fn claude_quota() -> Option<Quota> {
    claude_glow(&claude_cached())
}

/// A Codex reading kept for five minutes, so its session files are not read again on every call.
fn cached<T: Clone>(slot: &Mutex<Option<(Instant, T)>>, read: impl FnOnce() -> T) -> T {
    let mut slot = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((when, value)) = slot.as_ref() {
        if when.elapsed() < EVERY {
            return value.clone();
        }
    }
    let value = read();
    *slot = Some((Instant::now(), value.clone()));
    value
}

// ---- HOME's USAGE card: both CLIs, the five-hour window and the week ----------------------------

/// One limit window: how much of it is used, and when it starts over.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// 0..100 of the window already used.
    pub used_percent: f64,
    /// Milliseconds since 1970, when known.
    pub resets_at: Option<i64>,
}

fn window(used: f64, resets_at: Option<i64>) -> Window {
    Window { used_percent: used.clamp(0.0, 100.0), resets_at }
}

/// One CLI's limits as the USAGE card shows them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliUsage {
    pub cli: String,
    pub label: String,
    /// "ok", "signed_out", "not_installed" or "unknown".
    pub status: String,
    pub five_hour: Option<Window>,
    pub weekly: Option<Window>,
    /// The model a tighter weekly limit belongs to ("Opus"), when it is not the overall one.
    pub weekly_model: Option<String>,
    /// When these numbers were last reported, in milliseconds since 1970. Codex only reports
    /// them while it is being used, so its numbers can be hours old.
    pub observed_at: Option<i64>,
}

fn usage(cli: Cli, status: &str) -> CliUsage {
    CliUsage { cli: cli.command().into(), label: cli.label().into(), status: status.into(), five_hour: None, weekly: None, weekly_model: None, observed_at: None }
}

/// Claude Code's usage answer -> its five-hour window and its tightest weekly limit.
pub fn parse_claude_usage(answer: &Value, now: i64) -> CliUsage {
    let mut out = usage(Cli::Claude, "ok");
    out.observed_at = Some(now);
    let resets = |l: &Value| l.get("resets_at").and_then(Value::as_str).and_then(iso_ms);
    if let Some(limits) = answer.get("limits").and_then(Value::as_array) {
        out.five_hour = limits
            .iter()
            .find(|l| l.get("kind").and_then(Value::as_str) == Some("five_hour"))
            .and_then(|l| Some(window(l.get("percent")?.as_f64()?, resets(l))));
        let tightest = limits
            .iter()
            .filter(|l| l.get("group").and_then(Value::as_str) == Some("weekly"))
            .filter_map(|l| Some((l.get("percent")?.as_f64()?, l)))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((used, l)) = tightest {
            out.weekly = Some(window(used, resets(l)));
            out.weekly_model = l.pointer("/scope/model/display_name").and_then(Value::as_str).map(str::to_string);
        }
    }
    let older = |key: &str| answer.get(key).filter(|w| !w.is_null()).and_then(|w| Some(window(w.get("utilization")?.as_f64()?, resets(w))));
    if out.five_hour.is_none() {
        out.five_hour = older("five_hour");
    }
    if out.weekly.is_none() {
        out.weekly = older("seven_day");
    }
    if out.five_hour.is_none() && out.weekly.is_none() {
        out.status = "unknown".into();
    }
    out
}

/// One Codex session line -> both windows: primary is the five hours, secondary the week.
pub fn parse_codex_usage(line: &str, now: i64) -> Option<CliUsage> {
    let v: Value = serde_json::from_str(line).ok()?;
    let payload = v.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let limits = payload.get("rate_limits").or_else(|| payload.pointer("/info/rate_limits"))?;
    let read = |key: &str| -> Option<Window> {
        let w = limits.get(key).filter(|w| !w.is_null())?;
        let resets = w
            .get("resets_at")
            .and_then(|r| r.as_i64().map(|s| s * 1000).or_else(|| r.as_str().and_then(iso_ms)))
            .or_else(|| w.get("resets_in_seconds").and_then(Value::as_i64).map(|s| now + s * 1000));
        Some(window(w.get("used_percent")?.as_f64()?, resets))
    };
    let mut out = usage(Cli::Codex, "ok");
    out.five_hour = read("primary");
    out.weekly = read("secondary");
    out.observed_at = v.get("timestamp").and_then(Value::as_str).and_then(iso_ms).or(Some(now));
    (out.five_hour.is_some() || out.weekly.is_some()).then_some(out)
}

fn claude_usage() -> CliUsage {
    if crate::env::find_on_path(Cli::Claude.command()).is_none() {
        return usage(Cli::Claude, "not_installed");
    }
    claude_card(&claude_cached())
}

fn codex_usage() -> CliUsage {
    if crate::env::find_on_path(Cli::Codex.command()).is_none() {
        return usage(Cli::Codex, "not_installed");
    }
    let now = now_ms();
    codex_files()
        .iter()
        .find_map(|path| {
            let file = std::fs::File::open(path).ok()?;
            BufReader::new(file).lines().map_while(Result::ok).filter_map(|l| parse_codex_usage(&l, now)).last()
        })
        .unwrap_or_else(|| usage(Cli::Codex, "unknown"))
}

/// The CLIs the member wants on the USAGE card, in the card's order. One that is off is not asked.
pub fn shown_clis(claude: bool, codex: bool) -> Vec<Cli> {
    [(Cli::Claude, claude), (Cli::Codex, codex)].into_iter().filter(|(_, on)| *on).map(|(cli, _)| cli).collect()
}

/// Codex's line on the USAGE card and Codex's weekly limit for the crystal, each kept five minutes.
/// Claude Code's two share one cached answer instead (see `claude_cached`).
static CODEX_CARD: Mutex<Option<(Instant, CliUsage)>> = Mutex::new(None);
static CODEX_GLOW: Mutex<Option<(Instant, Option<Quota>)>> = Mutex::new(None);

/// The five-hour and weekly limits of the CLIs switched on in Settings > Companion, asked at most
/// every five minutes. None switched on gives an empty list. Each CLI is looked up on its own, so
/// a changed toggle never shows a CLI that was just switched off.
#[tauri::command]
pub async fn cli_usage(store: State<'_, SettingsStore>) -> Result<Vec<CliUsage>, String> {
    let settings = store.get();
    // the card switched off asks neither CLI
    let wanted = if settings.show_usage_card { shown_clis(settings.show_claude_usage, settings.show_codex_usage) } else { Vec::new() };
    tauri::async_runtime::spawn_blocking(move || {
        wanted.into_iter().map(|cli| if cli == Cli::Claude { claude_usage() } else { cached(&CODEX_CARD, codex_usage) }).collect::<Vec<_>>()
    })
    .await
    .map_err(|_| "Usage check stopped.".to_string())
}

/// The weekly limit left for the member's CLI, asked at most every five minutes; None = unknown.
#[tauri::command]
pub async fn cli_quota(store: State<'_, SettingsStore>) -> Result<Option<Quota>, String> {
    let cli = store.get().cli;
    let answer = tauri::async_runtime::spawn_blocking(move || match cli {
        Cli::Claude => claude_quota(),
        Cli::Codex => cached(&CODEX_GLOW, codex_quota),
    })
    .await
    .unwrap_or(None);
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_gives_the_tightest_weekly_limit() {
        let answer: Value = serde_json::from_str(
            r#"{"limits":[
                {"group":"weekly","kind":"weekly_all","percent":28.0,"resets_at":"2026-11-12T21:00:00+00:00"},
                {"group":"weekly","kind":"weekly_model","percent":61.5,"resets_at":"2026-11-13T05:00:00Z","scope":{"model":{"display_name":"Opus"}}},
                {"group":"session","kind":"five_hour","percent":90.0}
            ]}"#,
        )
        .unwrap();
        let q = parse_claude(&answer).unwrap();
        assert_eq!(q.left_percent, 38.5);
        assert_eq!(q.resets_at, iso_ms("2026-11-13T05:00:00Z"));
        let old: Value = serde_json::from_str(r#"{"seven_day":{"utilization":12.0,"resets_at":"2026-11-12T21:00:00.5+00:00"}}"#).unwrap();
        assert_eq!(parse_claude(&old).unwrap().left_percent, 88.0);
        assert!(parse_claude(&serde_json::json!({})).is_none());
    }

    #[test]
    fn codex_reads_its_weekly_window_from_a_session_line() {
        let line = r#"{"timestamp":"2026-11-09T05:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":40.0,"window_minutes":300,"resets_in_seconds":600},"secondary":{"used_percent":75.0,"window_minutes":10080,"resets_in_seconds":3600}}}}"#;
        let q = parse_codex(line, 1_000_000).unwrap();
        assert_eq!(q.left_percent, 25.0);
        assert_eq!(q.resets_at, Some(1_000_000 + 3_600_000));
        let only_primary = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"resets_at":1762700000}}}}"#;
        assert_eq!(parse_codex(only_primary, 0).unwrap().resets_at, Some(1_762_700_000_000));
        assert!(parse_codex(r#"{"type":"event_msg","payload":{"type":"agent_message","message":"hi"}}"#, 0).is_none());
    }

    #[test]
    fn used_over_a_hundred_never_goes_below_nothing_left() {
        assert_eq!(quota("claude", 130.0, None).left_percent, 0.0);
        assert_eq!(quota("claude", -5.0, None).left_percent, 100.0);
    }

    #[test]
    fn only_the_clis_switched_on_are_asked() {
        assert_eq!(shown_clis(true, true), vec![Cli::Claude, Cli::Codex]);
        assert_eq!(shown_clis(false, true), vec![Cli::Codex]);
        assert_eq!(shown_clis(true, false), vec![Cli::Claude]);
        assert!(shown_clis(false, false).is_empty());
    }

    #[test]
    fn claude_usage_reads_five_hours_and_the_tightest_week() {
        let answer: Value = serde_json::from_str(
            r#"{"limits":[
                {"group":"weekly","kind":"weekly_all","percent":28.0,"resets_at":"2026-11-12T21:00:00+00:00"},
                {"group":"weekly","kind":"weekly_model","percent":61.5,"resets_at":"2026-11-13T05:00:00Z","scope":{"model":{"display_name":"Opus"}}},
                {"group":"session","kind":"five_hour","percent":90.0,"resets_at":"2026-11-11T08:00:00Z"}
            ]}"#,
        )
        .unwrap();
        let u = parse_claude_usage(&answer, 5);
        assert_eq!(u.status, "ok");
        assert_eq!(u.five_hour, Some(Window { used_percent: 90.0, resets_at: iso_ms("2026-11-11T08:00:00Z") }));
        assert_eq!(u.weekly.as_ref().unwrap().used_percent, 61.5);
        assert_eq!(u.weekly_model.as_deref(), Some("Opus"));
        assert_eq!(u.observed_at, Some(5));
    }

    #[test]
    fn claude_usage_reads_the_older_answer_and_says_when_it_has_nothing() {
        let old: Value = serde_json::from_str(r#"{"five_hour":{"utilization":12.0,"resets_at":"2026-11-11T08:00:00Z"},"seven_day":{"utilization":40.0,"resets_at":null},"seven_day_opus":null}"#).unwrap();
        let u = parse_claude_usage(&old, 0);
        assert_eq!(u.five_hour.unwrap().used_percent, 12.0);
        assert_eq!(u.weekly, Some(Window { used_percent: 40.0, resets_at: None }));
        assert_eq!(u.weekly_model, None);
        assert_eq!(parse_claude_usage(&serde_json::json!({}), 0).status, "unknown");
        let only_week: Value = serde_json::from_str(r#"{"limits":[{"group":"weekly","kind":"weekly_all","percent":130.0}]}"#).unwrap();
        let u = parse_claude_usage(&only_week, 0);
        assert_eq!((u.five_hour, u.weekly.unwrap().used_percent), (None, 100.0));
    }

    #[test]
    fn codex_usage_reads_both_windows_and_when_they_were_reported() {
        let line = r#"{"timestamp":"2026-11-10T17:26:51.624Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":6.0,"window_minutes":300,"resets_at":1794340621},"secondary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1794683448}}}}"#;
        let u = parse_codex_usage(line, 0).unwrap();
        assert_eq!(u.five_hour, Some(Window { used_percent: 6.0, resets_at: Some(1_794_340_621_000) }));
        assert_eq!(u.weekly, Some(Window { used_percent: 100.0, resets_at: Some(1_794_683_448_000) }));
        assert_eq!(u.observed_at, iso_ms("2026-11-10T17:26:51.624Z"));
        let no_week = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"resets_in_seconds":60},"secondary":null}}}"#;
        let u = parse_codex_usage(no_week, 1000).unwrap();
        assert_eq!((u.five_hour.unwrap().resets_at, u.weekly, u.observed_at), (Some(61_000), None, Some(1000)));
        assert!(parse_codex_usage(r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{}}}"#, 0).is_none());
    }

    fn claude_answer_with(five_hour: f64, weekly: f64) -> Value {
        serde_json::json!({ "limits": [
            { "group": "session", "kind": "five_hour", "percent": five_hour },
            { "group": "weekly", "kind": "weekly_all", "percent": weekly },
        ]})
    }

    #[test]
    fn the_card_and_the_crystal_read_one_claude_answer() {
        let asked = Instant::now();
        let cache = remember(None, Ok(claude_answer_with(20.0, 30.0)), asked, 1_000);
        let card = claude_card(&cache);
        assert_eq!(card.status, "ok");
        assert_eq!(card.five_hour.unwrap().used_percent, 20.0);
        assert_eq!(card.weekly.unwrap().used_percent, 30.0);
        assert_eq!(card.observed_at, Some(1_000));
        assert_eq!(claude_glow(&cache).unwrap().left_percent, 70.0);
    }

    #[test]
    fn a_claude_answer_is_used_again_for_five_minutes() {
        let asked = Instant::now();
        let cache = remember(None, Ok(claude_answer_with(1.0, 2.0)), asked, 0);
        assert!(still_fresh(&cache, asked));
        assert!(still_fresh(&cache, asked + Duration::from_secs(4 * 60 + 59)));
        assert!(!still_fresh(&cache, asked + EVERY));
        // a failed ask also waits five minutes before asking again
        let failed = remember(None, Err("unknown"), asked, 0);
        assert!(still_fresh(&failed, asked + Duration::from_secs(60)));
    }

    #[test]
    fn a_failed_claude_ask_keeps_the_last_good_numbers_with_their_old_time() {
        let first = Instant::now();
        let good = remember(None, Ok(claude_answer_with(40.0, 50.0)), first, 1_000);
        let later = remember(Some(good), Err("unknown"), first + EVERY, 999_000);
        assert_eq!(later.failed, Some("unknown"));
        let card = claude_card(&later);
        assert_eq!(card.status, "ok");
        assert_eq!(card.weekly.unwrap().used_percent, 50.0);
        // dated when they came, so the card says how old they are
        assert_eq!(card.observed_at, Some(1_000));
        assert_eq!(claude_glow(&later).unwrap().left_percent, 50.0);
        // the next good answer replaces them
        let again = remember(Some(later), Ok(claude_answer_with(5.0, 6.0)), first + EVERY * 2, 2_000_000);
        assert_eq!((claude_card(&again).observed_at, again.failed), (Some(2_000_000), None));
    }

    #[test]
    fn signing_out_drops_the_old_claude_numbers() {
        let first = Instant::now();
        let good = remember(None, Ok(claude_answer_with(40.0, 50.0)), first, 1_000);
        let out = remember(Some(good), Err("signed_out"), first + EVERY, 2_000);
        assert_eq!(claude_card(&out).status, "signed_out");
        assert!(claude_glow(&out).is_none());
    }

    #[test]
    fn with_no_good_claude_answer_yet_the_card_says_why() {
        let cache = remember(None, Err("unknown"), Instant::now(), 0);
        let card = claude_card(&cache);
        assert_eq!((card.status.as_str(), card.five_hour, card.weekly), ("unknown", None, None));
        assert!(claude_glow(&cache).is_none());
    }

    #[test]
    fn a_codex_reading_is_kept_for_five_minutes() {
        let slot: Mutex<Option<(Instant, u32)>> = Mutex::new(None);
        assert_eq!(cached(&slot, || 1), 1);
        assert_eq!(cached(&slot, || 2), 1);
        // (an Instant cannot go back past the moment the computer started)
        if let Some(old) = Instant::now().checked_sub(EVERY + Duration::from_secs(1)) {
            *slot.lock().unwrap() = Some((old, 1));
            assert_eq!(cached(&slot, || 3), 3);
        }
    }

    /// Live, on demand only: `cargo test live_claude_quota -- --ignored --nocapture`. Prints the
    /// percentage left, never the login.
    #[test]
    #[ignore]
    fn live_claude_quota() {
        println!("claude weekly left: {:?}", claude_quota().map(|q| (q.left_percent, q.resets_at)));
    }

    /// Live, on demand only: `cargo test live_cli_usage -- --ignored --nocapture`. Prints the
    /// percentages and status for both CLIs, never the login.
    #[test]
    #[ignore]
    fn live_cli_usage() {
        for u in [claude_usage(), codex_usage()] {
            println!("{} {} 5h {:?} week {:?} model {:?}", u.cli, u.status, u.five_hour.map(|w| w.used_percent), u.weekly.map(|w| w.used_percent), u.weekly_model);
        }
    }

    #[test]
    fn a_time_with_a_zone_becomes_utc_milliseconds() {
        assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_ms("1970-01-01T08:00:00+08:00"), Some(0));
        assert_eq!(iso_ms("2026-11-12T21:00:00.125837+00:00"), iso_ms("2026-11-12T21:00:00Z"));
    }
}
