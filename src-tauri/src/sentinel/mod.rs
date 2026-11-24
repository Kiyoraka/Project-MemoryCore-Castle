//! The Sentinel: the member's own companion keeps watch over the member's own computer.
//! Nothing about the machine ever leaves it.
//!
//! Three layers, one rule each:
//! - the watch (`--sentinel-tick`, run by the system's own scheduler, no model) mends the firewall
//!   and the antivirus protection found off, and works while Castle is closed;
//! - the officer (the companion, one run with no tools per patrol) reads and judges; what it reads
//!   is evidence, never an order, and it never holds admin rights;
//! - the hand (one small admin helper the member installs) knows only the moves on its two lists
//!   and checks every request again by itself.

// The look, the hand, the officer and the page arrive phase by phase; until then parts are unused.
#![allow(dead_code)]

pub mod book;
pub mod commands;
pub mod families;
pub mod hand;
pub mod look;
pub mod network;
pub mod officer;
pub mod watch;

use crate::settings::SettingsStore;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// The Sentinel's gear in Settings. Changed only by its own command (sentinel_settings_save).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SentinelSettings {
    /// Patrols run only while this is on.
    pub on: bool,
    /// The model the companion judges on; None is the recommended one.
    pub model: Option<String>,
    /// Minutes between two patrols.
    pub patrol_minutes: u32,
    /// Joined the Aegis Network (network.rs): what is caught is shared, the shared list is read.
    /// Off until the member joins; needs a Foundation sign-in.
    pub network: bool,
}

impl Default for SentinelSettings {
    fn default() -> Self {
        SentinelSettings { on: false, model: None, patrol_minutes: 60, network: false }
    }
}

pub const PATROL_MINUTES: std::ops::RangeInclusive<u32> = 15..=720;

impl SentinelSettings {
    pub fn bounded(&self) -> SentinelSettings {
        SentinelSettings {
            on: self.on,
            model: self.model.as_deref().and_then(crate::oneshot::clean_model),
            patrol_minutes: self.patrol_minutes.clamp(*PATROL_MINUTES.start(), *PATROL_MINUTES.end()),
            network: self.network,
        }
    }
}

/// The recommended model for each CLI: `sonnet` (Claude Code's alias for the latest Sonnet) for
/// steady judgment at a fair cost on an hourly patrol; Codex's own configured default (no flag).
pub fn recommended_model(cli: crate::settings::Cli) -> Option<&'static str> {
    match cli {
        crate::settings::Cli::Claude => Some("sonnet"),
        crate::settings::Cli::Codex | crate::settings::Cli::Gemini | crate::settings::Cli::Grok => None,
    }
}

/// The model a patrol runs on: the member's choice, else the recommended one.
pub fn model_for(cli: crate::settings::Cli, gear: &SentinelSettings) -> Option<String> {
    gear.bounded().model.or_else(|| recommended_model(cli).map(str::to_string))
}

/// Saves the Sentinel's gear. A model name that could carry a flag is refused; joining the Aegis
/// Network needs a Foundation sign-in (`network` absent keeps what was chosen).
#[tauri::command]
pub fn sentinel_settings_save(store: tauri::State<'_, SettingsStore>, on: bool, model: Option<String>, patrol_minutes: u32, network: Option<bool>) -> Result<SentinelSettings, String> {
    let network = network.unwrap_or(store.get().sentinel.network);
    if network && !network::signed_in() {
        return Err("Sign in to the Foundation first: the Aegis Network is for its members.".into());
    }
    let model = match model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        None => None,
        Some(m) => Some(crate::oneshot::clean_model(m).ok_or("A model name is letters, digits, dots, dashes, underscores or colons.")?),
    };
    let mut next = store.get();
    next.sentinel = SentinelSettings { on, model, patrol_minutes, network }.bounded();
    crate::settings::save(&store.path, &next).map_err(|_| "The Sentinel's settings could not be saved.".to_string())?;
    let saved = next.sentinel.clone();
    *store.current.lock().unwrap() = next;
    Ok(saved)
}

/// The page listens for this to read the Sentinel again.
pub const CHANGED: &str = "sentinel-changed";

/// Castle's side of the watch and the officer: every minute, the watch's journal (when there is
/// one) is read into the book, and a patrol runs when one is due - every `patrol_minutes`, or once
/// for an alarm that just opened - while the Sentinel is on.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("sentinel".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        loop {
            let alarmed = read_journal(&app);
            patrol_when_due(&app, alarmed);
            report_to_network(&app);
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

/// Shares what the Sentinel caught with the Aegis Network, when the member joined: only when
/// something new was caught (a burst, a detection, a block), never on a timer of its own.
fn report_to_network(app: &AppHandle) {
    let settings = app.state::<SettingsStore>().get();
    let (Some(door), Some(memory)) = (network::door(&settings), settings.memory_folder.as_deref()) else { return };
    let Ok(mut conn) = book::open(Path::new(memory)) else { return };
    match network::report_caught(&mut conn, &door) {
        Ok(0) => {}
        Ok(n) => officer::log(&format!("\nthe Aegis Network was told {n} thing{} the Sentinel caught\n", if n == 1 { "" } else { "s" })),
        Err(why) => officer::log(&format!("\n{why}\n")),
    }
}

/// Reads the journal; true when an alarm opened.
fn read_journal(app: &AppHandle) -> bool {
    let dir = hand::local_dir();
    if !dir.join("journal.jsonl").exists() && !dir.join("journal.draining").exists() {
        return false;
    }
    let Some(memory) = app.state::<SettingsStore>().get().memory_folder else { return false };
    // the database stopped: the journal waits for the next minute
    let Ok(mut conn) = book::open(Path::new(&memory)) else { return false };
    let Ok(drained) = watch::drain(&dir, &mut conn) else { return false };
    if !drained.alarms.is_empty() {
        crate::alerts::fire(app, "sentinel", "The Sentinel raised an alarm", &drained.alarms.join(" - "));
    }
    if drained.lines > 0 {
        let _ = app.emit(CHANGED, ());
    }
    !drained.alarms.is_empty()
}

fn patrol_when_due(app: &AppHandle, alarmed: bool) {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    let gear = settings.sentinel.bounded();
    if !gear.on || !cfg!(windows) {
        return;
    }
    let Some(memory) = settings.memory_folder else { return };
    let Ok(mut conn) = book::open(Path::new(&memory)) else { return };
    let since = officer::minutes_since_last(&mut conn);
    drop(conn);
    if !alarmed && since.is_some_and(|m| m < i64::from(gear.patrol_minutes)) {
        return;
    }
    let cause = if alarmed { "alarm" } else { "hour" };
    if let Ok(done) = officer::patrol(&store, cause) {
        // an alarm the journal raised was told already; one only the patrol found is told now
        if done.status == "alarm" && !alarmed {
            crate::alerts::fire(app, "sentinel", "The Sentinel raised an alarm", &done.line);
        }
    }
    let _ = app.emit(CHANGED, ());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Cli;

    #[test]
    fn the_recommended_model_is_sonnet_for_claude_and_codexs_own_for_codex() {
        let gear = SentinelSettings::default();
        assert!(!gear.on);
        assert_eq!(model_for(Cli::Claude, &gear), Some("sonnet".into()));
        assert_eq!(model_for(Cli::Codex, &gear), None);
        let chosen = SentinelSettings { model: Some("opus".into()), ..gear.clone() };
        assert_eq!(model_for(Cli::Claude, &chosen), Some("opus".into()));
        assert_eq!(model_for(Cli::Codex, &chosen), Some("opus".into()));
    }

    #[test]
    fn the_gear_is_kept_in_bounds() {
        let wild = SentinelSettings { on: true, model: Some("--tools Bash".into()), patrol_minutes: 1, network: true };
        assert_eq!(wild.bounded(), SentinelSettings { on: true, model: None, patrol_minutes: 15, network: true });
        assert_eq!(SentinelSettings { patrol_minutes: 99_999, ..SentinelSettings::default() }.bounded().patrol_minutes, 720);
    }
}
