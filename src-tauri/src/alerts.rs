//! Alerts: the one place that decides whether, how and where to tell the member that something
//! happened. A part of Castle only says "this source has news" (`fire`); the member's rule for that
//! source (Settings > Alerts) decides notification and sound, and the alert stays quiet while the
//! member is already looking at the room it is about. Sources are a table, and a source with no rule
//! yet works from its own defaults, so a new feature or theme adds one line here without touching
//! Settings. CHAT keeps its older fields (roomalert.rs) and is resolved from them.

use crate::settings::{Settings, SettingsStore};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager, State};

/// What an alert source is, and what it does until the member sets it.
pub struct Source {
    pub id: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    /// the room a click opens, and the room whose being on screen silences the alert
    pub room: &'static str,
    pub notify: bool,
    pub sound: bool,
    /// a different default per source, so they can be told apart by ear
    pub sound_name: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source { id: "automation", label: "An automation finished", about: "Scheduled briefings and failed jobs.", room: "home", notify: true, sound: false, sound_name: "koto" },
    Source { id: "chat", label: "Chat room messages", about: "A new line in the Foundation room.", room: "chat", notify: true, sound: false, sound_name: "glass" },
    Source { id: "home_reply", label: "Your companion finished replying", about: "HOME: the reply or the task is complete and waiting for you.", room: "home", notify: true, sound: false, sound_name: "koto" },
    Source { id: "market_round", label: "A MARKET round finished", about: "The companion's automatic trading round has ended. Rounds can be frequent, so this starts off.", room: "market", notify: false, sound: false, sound_name: "woodblock" },
    Source { id: "sentinel", label: "The Sentinel raised an alarm", about: "Something on this computer needs your eyes: protection or the firewall found off, a detection, a new account.", room: "home", notify: true, sound: true, sound_name: "bell" },
    Source { id: "picture_ready", label: "A picture is ready", about: "A drawing from APPEARANCE or STUDIO has finished, or could not be drawn.", room: "studio", notify: true, sound: false, sound_name: "chord" },
];

/// A source's choices as they are now.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub notify: bool,
    pub sound: bool,
    pub sound_name: String,
}

/// The rule for a source: the member's choice where there is one, the source's own default where
/// not. CHAT reads its older fields. A source this build does not know still gets a plain default.
pub fn resolve(settings: &Settings, id: &str) -> Resolved {
    if id == "chat" {
        return Resolved { notify: settings.alert_notify, sound: settings.alert_sound, sound_name: settings.alert_sound_name.clone() };
    }
    let (notify, sound, sound_name) = SOURCES.iter().find(|s| s.id == id).map_or((true, false, crate::roomalert::DEFAULT_SOUND), |s| (s.notify, s.sound, s.sound_name));
    let rule = settings.alerts.get(id);
    Resolved {
        notify: rule.and_then(|r| r.notify).unwrap_or(notify),
        sound: rule.and_then(|r| r.sound).unwrap_or(sound),
        sound_name: rule.and_then(|r| r.sound_name.clone()).unwrap_or_else(|| sound_name.to_string()),
    }
}

/// Which room the member has on screen, told by the page whenever the tab changes.
#[derive(Default)]
pub struct ActiveRoom(Mutex<String>);

#[tauri::command]
pub fn alert_active_room(room: String, active: State<ActiveRoom>) {
    *active.0.lock().unwrap() = room;
}

/// The member is looking at a room when it is the tab on screen in a visible, focused Castle.
fn looking(app: &AppHandle, room: &str) -> bool {
    app.state::<ActiveRoom>().0.lock().unwrap().as_str() == room
        && app.get_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

/// Something happened that this source's member may want to hear about.
pub fn fire(app: &AppHandle, id: &str, title: &str, body: &str) {
    let settings = app.state::<SettingsStore>().get();
    let rule = resolve(&settings, id);
    if !(rule.notify || rule.sound) {
        return;
    }
    let room = SOURCES.iter().find(|s| s.id == id).map_or("home", |s| s.room);
    if looking(app, room) {
        return;
    }
    if rule.sound {
        crate::roomalert::beep(app, &rule.sound_name);
    }
    if rule.notify {
        crate::roomalert::toast(app, title, body, room);
    }
}

/// One row of Settings > Alerts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    pub room: &'static str,
    pub notify: bool,
    pub sound: bool,
    pub sound_name: String,
}

#[tauri::command]
pub fn alert_sources(store: State<SettingsStore>) -> Vec<SourceView> {
    let settings = store.get();
    SOURCES
        .iter()
        .map(|s| {
            let r = resolve(&settings, s.id);
            SourceView { id: s.id, label: s.label, about: s.about, room: s.room, notify: r.notify, sound: r.sound, sound_name: r.sound_name }
        })
        .collect()
}

// ---------- HOME: the companion finished replying ----------

const LOOK_EVERY: Duration = Duration::from_secs(3);
const RECORD_LIMIT: u64 = 64 * 1024 * 1024;

/// Is the main session mid-turn? Some(true) while a turn runs, Some(false) at the end of a turn,
/// None when there is no session to ask. Reads the CLI's own record, like the switch's safe pause;
/// the record is read again only when it has changed since the last look.
fn mid_turn(app: &AppHandle, seen: &mut Option<(SystemTime, bool)>) -> Option<bool> {
    let host = &app.state::<crate::session::MainHost>().0;
    if !host.is_running() {
        return None;
    }
    let (_, since) = host.started()?;
    if crate::providers::nothing_typed_since(host.last_input_time(), since) {
        return Some(false);
    }
    let settings = app.state::<SettingsStore>().get();
    let cli = host.provider().unwrap_or(settings.cli);
    let folder = crate::session::Room::Main.folder(&settings)?;
    let Some((record, _)) = crate::chat::newest_record(cli, &folder, since) else { return Some(true) };
    let meta = record.metadata().ok()?;
    let modified = meta.modified().ok()?;
    if host.last_input_time().is_some_and(|input| modified < input) {
        return Some(true); // typed after the record was last written: the CLI has not answered yet
    }
    if let Some((at, done)) = seen {
        if *at == modified {
            return Some(!*done);
        }
    }
    if meta.len() > RECORD_LIMIT {
        return Some(false);
    }
    let done = std::fs::read_to_string(&record).map(|t| crate::providers::complete_record(&t, cli)).unwrap_or(false);
    *seen = Some((modified, done));
    Some(!done)
}

/// Watches the main session: when a turn that was running reaches its end, HOME's alert fires.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("alerts-home".into()).spawn(move || {
        let mut busy = false;
        let mut seen: Option<(SystemTime, bool)> = None;
        let mut session: Option<u64> = None;
        loop {
            std::thread::sleep(LOOK_EVERY);
            let id = app.state::<crate::session::MainHost>().0.started().map(|(id, _)| id);
            if id != session {
                session = id;
                busy = false;
                seen = None;
            }
            match mid_turn(&app, &mut seen) {
                Some(true) => busy = true,
                Some(false) => {
                    if busy {
                        busy = false;
                        let settings = app.state::<SettingsStore>().get();
                        let who = settings.companion_name.clone().unwrap_or_else(|| "Your companion".into());
                        fire(&app, "home_reply", &format!("{who} replied"), "The reply is ready in HOME.");
                    }
                }
                None => busy = false,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AlertRule;

    #[test]
    fn a_source_with_no_rule_uses_its_own_defaults() {
        let s = Settings::default();
        let home = resolve(&s, "home_reply");
        assert_eq!((home.notify, home.sound, home.sound_name.as_str()), (true, false, "koto"));
        let market = resolve(&s, "market_round");
        assert!(!market.notify, "rounds can be frequent, so they start quiet");
        assert_ne!(home.sound_name, market.sound_name, "each source sounds different by default");
    }

    #[test]
    fn a_members_rule_wins_and_only_for_the_fields_it_sets() {
        let mut s = Settings::default();
        s.alerts.insert("home_reply".into(), AlertRule { sound: Some(true), sound_name: Some("file:Ping.wav".into()), ..Default::default() });
        let home = resolve(&s, "home_reply");
        assert!(home.notify, "notify was not set, so the default stays");
        assert!(home.sound);
        assert_eq!(home.sound_name, "file:Ping.wav");
    }

    #[test]
    fn chat_reads_its_older_fields_so_an_old_file_behaves_the_same() {
        let mut s = Settings::default();
        s.alert_notify = false;
        s.alert_sound = true;
        s.alert_sound_name = "chord".into();
        let chat = resolve(&s, "chat");
        assert_eq!((chat.notify, chat.sound, chat.sound_name.as_str()), (false, true, "chord"));
    }

    #[test]
    fn an_unknown_source_still_gets_a_plain_default() {
        let r = resolve(&Settings::default(), "from_a_theme");
        assert_eq!((r.notify, r.sound), (true, false));
    }

    #[test]
    fn every_source_has_a_real_default_sound_and_a_room() {
        for s in SOURCES {
            assert!(s.id.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert!(!s.room.is_empty() && !s.label.is_empty());
            assert!(crate::roomalert::sound_choices(None).iter().any(|c| c.id == s.sound_name), "{} default sound exists", s.id);
        }
        let mut ids: Vec<_> = SOURCES.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SOURCES.len(), "ids are unique");
    }
}
