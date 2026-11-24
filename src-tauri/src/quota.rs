//! How much of the member's weekly CLI limit is left - the great crystal shines with it.
//!
//! Claude Code: the endpoint behind Claude Code's `/usage`, asked with the login Claude Code
//! keeps in `~/.claude/.credentials.json`. It is not a documented API and may change. The USAGE
//! card and the crystal share one answer, asked at most every five minutes; when an ask fails the
//! last good numbers stay (dated, so the card can say how old they are), and with none the crystal
//! simply keeps its normal glow. The token is read for each ask, goes out only in the
//! Authorization header, and is never kept, logged or sent to a page.
//!
//! Codex signed in with ChatGPT: the endpoint behind Codex's own `/status` limits
//! (`GET https://chatgpt.com/backend-api/wham/usage`, see openai/codex
//! `codex-rs/backend-client/src/client/rate_limit_resets.rs`), asked with the login Codex keeps in
//! `$CODEX_HOME/auth.json` (default `~/.codex`, `%USERPROFILE%\.codex` on Windows). Like Claude's,
//! it is not a documented API; the token is read for each ask, goes out only in the Authorization
//! header, and is never kept, logged, put in an error or sent to a page. When the ask fails, the
//! session files are read instead: they carry the limits Codex was last told (`token_count` lines
//! with `rate_limits`), though Codex Desktop writes `null` there. Codex with an API key has no
//! weekly limit: it is billed per use, and the card says so.

use crate::settings::{Cli, SettingsStore};
use crate::transcript::codex;
use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

const CLAUDE_USAGE: &str = "https://api.anthropic.com/api/oauth/usage";
const CODEX_USAGE: &str = "https://chatgpt.com/backend-api/wham/usage";
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

/// The newest Codex session files from the last eight days, newest first.
fn codex_files() -> Vec<PathBuf> {
    let Some(root) = codex::sessions_root() else { return Vec::new() };
    let Some(week_ago) = SystemTime::now().checked_sub(Duration::from_secs(8 * 86_400)) else { return Vec::new() };
    let mut files = codex::recent_rollouts(&root, week_ago);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().take(5).map(|(_, path)| path).collect()
}

/// How Codex is signed in, from its `auth.json`. The token is wrapped so it can never be printed.
#[derive(Debug, PartialEq)]
enum CodexAuth {
    ChatGpt { token: Secret, account_id: Option<String> },
    ApiKey,
    None,
}

/// A login that prints as `<hidden>`, so a `{:?}` anywhere can never leak it.
#[derive(PartialEq)]
struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<hidden>")
    }
}

/// Codex's `auth.json` text -> how it is signed in. `auth_mode` says it when present; older files
/// only have `tokens` (ChatGPT) or `OPENAI_API_KEY` (an API key).
fn codex_auth_in(text: &str) -> CodexAuth {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return CodexAuth::None };
    let chatgpt = || {
        let tokens = v.get("tokens")?;
        let token = tokens.get("access_token")?.as_str().filter(|t| !t.is_empty())?.to_string();
        let account_id = tokens.get("account_id").and_then(Value::as_str).filter(|a| !a.is_empty()).map(str::to_string);
        Some(CodexAuth::ChatGpt { token: Secret(token), account_id })
    };
    let api_key = || v.get("OPENAI_API_KEY").and_then(Value::as_str).filter(|k| !k.is_empty()).map(|_| CodexAuth::ApiKey);
    let mode = v.get("auth_mode").and_then(Value::as_str).map(|m| m.to_ascii_lowercase().replace(['_', '-'], ""));
    match mode.as_deref() {
        Some("apikey") => CodexAuth::ApiKey,
        Some("chatgpt") => chatgpt().unwrap_or(CodexAuth::None),
        _ => chatgpt().or_else(api_key).unwrap_or(CodexAuth::None),
    }
}

fn codex_auth() -> CodexAuth {
    let Some(path) = codex::codex_home().map(|home| home.join("auth.json")) else { return CodexAuth::None };
    std::fs::read_to_string(path).map(|text| codex_auth_in(&text)).unwrap_or(CodexAuth::None)
}

/// Codex's usage answer, asked the way Codex asks it. Errors are fixed words, never the token.
fn codex_answer(token: &Secret, account_id: Option<&str>) -> Result<Value, &'static str> {
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|_| "unknown")?;
    let mut ask = client.get(CODEX_USAGE).bearer_auth(&token.0).header(reqwest::header::USER_AGENT, "codex-cli");
    if let Some(account) = account_id {
        ask = ask.header("chatgpt-account-id", account);
    }
    let answer = ask.send().map_err(|_| "unknown")?;
    if !answer.status().is_success() {
        return Err("unknown");
    }
    answer.json().map_err(|_| "unknown")
}

/// Codex's usage answer -> its windows. Each window says how long it is, so a plan with only a
/// weekly limit (it comes as the primary window) still lands on the Week bar.
pub fn parse_codex_answer(answer: &Value, now: i64) -> Option<CliUsage> {
    let limits = answer.get("rate_limit").filter(|l| !l.is_null())?;
    let mut out = usage(Cli::Codex, "ok");
    out.observed_at = Some(now);
    for (key, fallback_weekly) in [("primary_window", false), ("secondary_window", true)] {
        let Some(w) = limits.get(key).filter(|w| !w.is_null()) else { continue };
        let Some(used) = w.get("used_percent").and_then(Value::as_f64) else { continue };
        let resets = w
            .get("reset_at")
            .and_then(Value::as_i64)
            .filter(|at| *at > 0)
            .map(|at| at * 1000)
            .or_else(|| w.get("reset_after_seconds").and_then(Value::as_i64).map(|s| now + s * 1000));
        let weekly = match w.get("limit_window_seconds").and_then(Value::as_i64).filter(|s| *s > 0) {
            Some(seconds) => seconds >= 2 * 86_400,
            None => fallback_weekly,
        };
        let slot = if weekly { &mut out.weekly } else { &mut out.five_hour };
        if slot.is_none() {
            *slot = Some(window(used, resets));
        }
    }
    (out.five_hour.is_some() || out.weekly.is_some()).then_some(out)
}

/// The newest limits Codex wrote into its session files, if any.
fn codex_from_sessions() -> Option<CliUsage> {
    let now = now_ms();
    codex_files().iter().find_map(|path| {
        let file = std::fs::File::open(path).ok()?;
        BufReader::new(file).lines().map_while(Result::ok).filter_map(|l| parse_codex_usage(&l, now)).last()
    })
}

/// Codex's line from its login: ChatGPT asks the usage endpoint, an API key is billed per use, and
/// whatever cannot be asked falls back to the session files.
fn codex_reading_from(
    auth: CodexAuth,
    ask: impl FnOnce(&Secret, Option<&str>) -> Result<Value, &'static str>,
    sessions: impl FnOnce() -> Option<CliUsage>,
    now: i64,
) -> CliUsage {
    match auth {
        CodexAuth::ApiKey => usage(Cli::Codex, "api_key"),
        CodexAuth::ChatGpt { token, account_id } => ask(&token, account_id.as_deref())
            .ok()
            .and_then(|answer| parse_codex_answer(&answer, now))
            .or_else(sessions)
            .unwrap_or_else(|| usage(Cli::Codex, "unknown")),
        CodexAuth::None => sessions().unwrap_or_else(|| usage(Cli::Codex, "unknown")),
    }
}

fn codex_reading() -> CliUsage {
    codex_reading_from(codex_auth(), codex_answer, codex_from_sessions, now_ms())
}

/// The crystal's weekly limit from Codex's line (the five hours when no week is known).
fn codex_glow(reading: &CliUsage) -> Option<Quota> {
    if reading.status != "ok" {
        return None;
    }
    let w = reading.weekly.as_ref().or(reading.five_hour.as_ref())?;
    Some(quota("codex", w.used_percent, w.resets_at))
}

fn codex_quota() -> Option<Quota> {
    codex_glow(&cached(&CODEX, codex_reading))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The usable token in Claude Code's credentials text, with when it stops working (ms since 1970,
/// i64::MAX when the text does not say). An expired token is no token.
fn token_in(text: &str, now: i64) -> Option<(String, i64)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let token = v.pointer("/claudeAiOauth/accessToken")?.as_str().filter(|t| !t.is_empty())?.to_string();
    // the credentials file writes milliseconds; the macOS Keychain entry writes seconds
    let until = match v.pointer("/claudeAiOauth/expiresAt").and_then(Value::as_i64) {
        Some(at) if at < 100_000_000_000 => at * 1000,
        Some(at) => at,
        None => i64::MAX,
    };
    (until > now).then_some((token, until))
}

fn claude_token() -> Option<String> {
    #[cfg(target_os = "macos")]
    if let Some(token) = keychain_token() {
        return Some(token);
    }
    let path: PathBuf = crate::env::home_dir()?.join(".claude").join(".credentials.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    v.pointer("/claudeAiOauth/accessToken")?.as_str().map(str::to_string).filter(|t| !t.is_empty())
}

/// macOS: Claude Code keeps its live sign-in in the Keychain (a credentials file there may be an
/// old, expired copy). macOS asks the member once before Castle may read it, so the token is kept
/// for this run and read again only when it has expired.
#[cfg(target_os = "macos")]
fn keychain_token() -> Option<String> {
    static KEPT: std::sync::Mutex<Option<(String, i64)>> = std::sync::Mutex::new(None);
    let now = now_ms();
    let mut kept = KEPT.lock().unwrap();
    if let Some((token, _)) = kept.as_ref().filter(|(_, until)| *until > now) {
        return Some(token.clone());
    }
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let found = token_in(&text, now)?;
    *kept = Some(found.clone());
    Some(found.0)
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
    /// "ok", "signed_out", "not_installed", "api_key" (Codex billed per use), "not_measured" (a
    /// provider whose limits Castle cannot read: Gemini, Grok) or "unknown".
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
    CliUsage { cli: cli.id().into(), label: cli.label().into(), status: status.into(), five_hour: None, weekly: None, weekly_model: None, observed_at: None }
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
    cached(&CODEX, codex_reading)
}

/// Codex's one reading, kept five minutes: the USAGE card and the crystal both read it, so the
/// usage endpoint is asked at most once every five minutes for the two.
static CODEX: Mutex<Option<(Instant, CliUsage)>> = Mutex::new(None);

/// Whether Castle can read a provider's limits: Claude Code and Codex report them, Gemini and Grok do not.
fn measured(cli: Cli) -> bool {
    match cli {
        Cli::Claude | Cli::Codex => true,
        Cli::Gemini | Cli::Grok => false,
    }
}

/// The card's rows: every provider Castle can measure, and the chosen one when it cannot (said so,
/// never left out silently).
fn usage_rows(chosen: Cli) -> Vec<Cli> {
    Cli::ALL.into_iter().filter(|&cli| measured(cli) || cli == chosen).collect()
}

/// The five-hour and weekly limits of Claude Code and Codex, in the card's order, asked at most
/// every five minutes. The card itself is switched on or off in WIDGETS; off, neither CLI is asked.
#[tauri::command]
pub async fn cli_usage(store: State<'_, SettingsStore>) -> Result<Vec<CliUsage>, String> {
    let settings = store.get();
    // the card switched off asks neither CLI
    let wanted = if settings.show_usage_card { usage_rows(settings.cli) } else { Vec::new() };
    tauri::async_runtime::spawn_blocking(move || {
        wanted
            .into_iter()
            .map(|cli| match cli {
                Cli::Claude => claude_usage(),
                Cli::Codex => codex_usage(),
                Cli::Gemini | Cli::Grok => usage(cli, "not_measured"),
            })
            .collect::<Vec<_>>()
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
        Cli::Codex => codex_quota(),
        Cli::Gemini | Cli::Grok => None,
    })
    .await
    .unwrap_or(None);
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expired_claude_token_is_no_token() {
        let text = |until: i64| format!(r#"{{"claudeAiOauth":{{"accessToken":"tok","refreshToken":"r","expiresAt":{until}}}}}"#);
        assert_eq!(token_in(&text(1_800_000_000_000), 1_790_000_000_000), Some(("tok".into(), 1_800_000_000_000)));
        assert_eq!(token_in(&text(1_760_000_000_000), 1_790_000_000_000), None, "the old copy a Mac leaves in the file");
        assert_eq!(token_in(r#"{"claudeAiOauth":{"accessToken":"tok"}}"#, 5), Some(("tok".into(), i64::MAX)));
        assert_eq!(token_in(r#"{"claudeAiOauth":{"accessToken":""}}"#, 5), None);
        // the Keychain's seconds: 1_800_000_000 s is 2027, still valid at a 2026 now in ms
        assert_eq!(token_in(&text(1_800_000_000), 1_790_000_000_000), Some(("tok".into(), 1_800_000_000_000)));
        assert_eq!(token_in(&text(1_700_000_000), 1_790_000_000_000), None);
    }

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
        let q = codex_glow(&parse_codex_usage(line, 1_000_000).unwrap()).unwrap();
        assert_eq!(q.left_percent, 25.0);
        assert_eq!(q.resets_at, Some(1_000_000 + 3_600_000));
        let only_primary = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"resets_at":1762700000}}}}"#;
        assert_eq!(codex_glow(&parse_codex_usage(only_primary, 0).unwrap()).unwrap().resets_at, Some(1_762_700_000_000));
        assert!(parse_codex_usage(r#"{"type":"event_msg","payload":{"type":"agent_message","message":"hi"}}"#, 0).is_none());
        // Codex Desktop writes no limits at all
        assert!(parse_codex_usage(r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#, 0).is_none());
    }

    /// The answer Codex itself reads (openai/codex `RateLimitStatusPayload`).
    fn codex_answer_with(primary: Value, secondary: Value) -> Value {
        serde_json::json!({
            "plan_type": "plus",
            "rate_limit": { "allowed": true, "limit_reached": false, "primary_window": primary, "secondary_window": secondary },
            "credits": null,
        })
    }

    #[test]
    fn codex_answer_gives_five_hours_and_the_week() {
        let answer = codex_answer_with(
            serde_json::json!({ "used_percent": 12, "limit_window_seconds": 18_000, "reset_after_seconds": 3_000, "reset_at": 1_794_340_621 }),
            serde_json::json!({ "used_percent": 47, "limit_window_seconds": 604_800, "reset_after_seconds": 400_000, "reset_at": 1_794_683_448 }),
        );
        let u = parse_codex_answer(&answer, 7).unwrap();
        assert_eq!(u.status, "ok");
        assert_eq!(u.five_hour, Some(Window { used_percent: 12.0, resets_at: Some(1_794_340_621_000) }));
        assert_eq!(u.weekly, Some(Window { used_percent: 47.0, resets_at: Some(1_794_683_448_000) }));
        assert_eq!((u.observed_at, u.weekly_model.as_deref()), (Some(7), None));
        assert_eq!(codex_glow(&u).unwrap().left_percent, 53.0);
    }

    #[test]
    fn codex_answer_with_only_a_weekly_window_lands_on_the_week() {
        // a plan with no five-hour limit sends its week as the primary window
        let answer = codex_answer_with(serde_json::json!({ "used_percent": 100, "limit_window_seconds": 604_800, "reset_after_seconds": 60, "reset_at": 0 }), Value::Null);
        let u = parse_codex_answer(&answer, 1_000).unwrap();
        assert_eq!(u.five_hour, None);
        assert_eq!(u.weekly, Some(Window { used_percent: 100.0, resets_at: Some(61_000) }));
        // windows without a length: primary is the five hours, secondary the week
        let bare = codex_answer_with(serde_json::json!({ "used_percent": 5 }), serde_json::json!({ "used_percent": 9 }));
        let u = parse_codex_answer(&bare, 0).unwrap();
        assert_eq!((u.five_hour.unwrap().used_percent, u.weekly.unwrap().used_percent), (5.0, 9.0));
        // no limits in the answer is no answer
        assert!(parse_codex_answer(&serde_json::json!({ "plan_type": "plus", "rate_limit": null }), 0).is_none());
        assert!(parse_codex_answer(&codex_answer_with(Value::Null, Value::Null), 0).is_none());
        assert!(parse_codex_answer(&serde_json::json!({ "detail": "Unauthorized" }), 0).is_none());
    }

    #[test]
    fn codex_auth_json_says_how_codex_is_signed_in() {
        let chatgpt = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"x","access_token":"tok","refresh_token":"r","account_id":"acc"}}"#;
        assert_eq!(codex_auth_in(chatgpt), CodexAuth::ChatGpt { token: Secret("tok".into()), account_id: Some("acc".into()) });
        assert_eq!(codex_auth_in(r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#), CodexAuth::ApiKey);
        assert_eq!(codex_auth_in(r#"{"auth_mode":"api_key","OPENAI_API_KEY":"sk-x"}"#), CodexAuth::ApiKey);
        // an older file without auth_mode
        assert_eq!(codex_auth_in(r#"{"OPENAI_API_KEY":"sk-x","tokens":null}"#), CodexAuth::ApiKey);
        assert!(matches!(codex_auth_in(r#"{"tokens":{"access_token":"tok"}}"#), CodexAuth::ChatGpt { account_id: None, .. }));
        assert_eq!(codex_auth_in(r#"{"auth_mode":"chatgpt","tokens":{"access_token":""}}"#), CodexAuth::None);
        assert_eq!(codex_auth_in(r#"{"OPENAI_API_KEY":null}"#), CodexAuth::None);
        assert_eq!(codex_auth_in("not json"), CodexAuth::None);
    }

    #[test]
    fn a_codex_login_never_prints() {
        let auth = codex_auth_in(r#"{"tokens":{"access_token":"very-secret","account_id":"acc"}}"#);
        let shown = format!("{auth:?}");
        assert!(!shown.contains("very-secret"), "{shown}");
        assert!(shown.contains("<hidden>"));
    }

    fn a_session_reading() -> Option<CliUsage> {
        parse_codex_usage(r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":3.0},"secondary":{"used_percent":4.0}}}}"#, 0)
    }

    #[test]
    fn codex_with_an_api_key_is_billed_per_use() {
        let u = codex_reading_from(CodexAuth::ApiKey, |_, _| panic!("an API key is never asked"), || panic!("nor its sessions"), 0);
        assert_eq!((u.status.as_str(), &u.five_hour, &u.weekly), ("api_key", &None, &None));
        assert!(codex_glow(&u).is_none());
    }

    #[test]
    fn codex_signed_in_with_chatgpt_asks_the_usage_endpoint() {
        let auth = codex_auth_in(r#"{"auth_mode":"chatgpt","tokens":{"access_token":"tok","account_id":"acc"}}"#);
        let answer = codex_answer_with(serde_json::json!({ "used_percent": 20, "limit_window_seconds": 18_000, "reset_at": 100 }), serde_json::json!({ "used_percent": 30, "limit_window_seconds": 604_800, "reset_at": 200 }));
        let u = codex_reading_from(
            auth,
            |token, account| {
                assert_eq!((token.0.as_str(), account), ("tok", Some("acc")));
                Ok(answer)
            },
            || panic!("the sessions are not needed"),
            5,
        );
        assert_eq!((u.status.as_str(), u.five_hour.unwrap().used_percent, u.weekly.unwrap().used_percent, u.observed_at), ("ok", 20.0, 30.0, Some(5)));
    }

    #[test]
    fn a_failed_codex_ask_falls_back_to_the_session_files() {
        let chatgpt = || codex_auth_in(r#"{"tokens":{"access_token":"tok"}}"#);
        let u = codex_reading_from(chatgpt(), |_, _| Err("unknown"), a_session_reading, 0);
        assert_eq!((u.status.as_str(), u.weekly.unwrap().used_percent), ("ok", 4.0));
        // an answer with no limits in it also falls back
        let u = codex_reading_from(chatgpt(), |_, _| Ok(serde_json::json!({ "rate_limit": null })), a_session_reading, 0);
        assert_eq!(u.five_hour.unwrap().used_percent, 3.0);
        // and with nothing in the sessions either, the card says it has nothing yet
        let u = codex_reading_from(chatgpt(), |_, _| Err("unknown"), || None, 0);
        assert_eq!((u.status.as_str(), u.five_hour, u.weekly), ("unknown", None, None));
        // no auth.json (or a login kept elsewhere): the sessions only
        assert_eq!(codex_reading_from(CodexAuth::None, |_, _| panic!("no login, no ask"), a_session_reading, 0).status, "ok");
        assert_eq!(codex_reading_from(CodexAuth::None, |_, _| panic!("no login, no ask"), || None, 0).status, "unknown");
    }

    #[test]
    fn used_over_a_hundred_never_goes_below_nothing_left() {
        assert_eq!(quota("claude", 130.0, None).left_percent, 0.0);
        assert_eq!(quota("claude", -5.0, None).left_percent, 100.0);
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

    /// Live, on demand only: `cargo test live_codex_usage -- --ignored --nocapture`. Asks Codex's
    /// usage endpoint once (a failed ask falls back to the session files, like the card); prints
    /// how Codex is signed in, never the login, and the percentages.
    #[test]
    #[ignore]
    fn live_codex_usage() {
        let how = match codex_auth() {
            CodexAuth::ChatGpt { .. } => "chatgpt",
            CodexAuth::ApiKey => "api key",
            CodexAuth::None => "none",
        };
        let u = codex_reading();
        println!("codex auth {how}; card {} 5h {:?} week {:?}", u.status, u.five_hour.map(|w| (w.used_percent, w.resets_at)), u.weekly.map(|w| (w.used_percent, w.resets_at)));
    }

    #[test]
    fn a_time_with_a_zone_becomes_utc_milliseconds() {
        assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_ms("1970-01-01T08:00:00+08:00"), Some(0));
        assert_eq!(iso_ms("2026-11-12T21:00:00.125837+00:00"), iso_ms("2026-11-12T21:00:00Z"));
    }
}
