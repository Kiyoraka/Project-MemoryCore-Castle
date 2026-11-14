//! Castle's settings for this machine: one JSON file in the app config folder.
//! Keys never live here; they are kept by Windows Credential Manager (key.rs).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FILE_NAME: &str = "settings.json";

/// Which AI CLI runs inside Castle. Each signs in with the member's own account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Cli {
    #[default]
    Claude,
    Codex,
}

impl Cli {
    /// The command name looked up on PATH.
    pub fn command(self) -> &'static str {
        match self {
            Cli::Claude => "claude",
            Cli::Codex => "codex",
        }
    }

    /// The name shown on the companion's chip.
    pub fn label(self) -> &'static str {
        match self {
            Cli::Claude => "Claude Code",
            Cli::Codex => "Codex",
        }
    }
}

/// A button the member added to the quick-action row: its label and the words it types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickAction {
    pub label: String,
    pub command: String,
}

/// The rooms the member may switch off in Settings (HOME is always on). Off = not available:
/// the tab is gone and nothing runs for it, but nothing is removed.
pub const ROOMS: [&str; 11] = ["theme", "apps", "diary", "projects", "library", "appearance", "studio", "story", "world", "chat", "dev"];

pub const DEFAULT_WORLD: &str = "3d/floating-isles";
pub const CAMERAS: [&str; 4] = ["director", "follow", "wide", "orbit"];
pub const SHOTS: [&str; 5] = ["follow", "wide", "close", "orbit", "crane"];
pub const GLIDES: [&str; 3] = ["glide", "cut", "mix"];

/// A world id: "3d/<slug>", "2d/<slug>" or "my/<slug>" - lowercase letters, digits and dashes.
pub fn is_world_id(id: &str) -> bool {
    let Some((kind, slug)) = id.split_once('/') else { return false };
    ["3d", "2d", "my"].contains(&kind)
        && (1..=48).contains(&slug.len())
        && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
}

/// One screen's WORLD: which world, how the camera films it, and whether it is on. The screen is
/// found again by its name, then by its size and place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorldScreen {
    pub screen: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub world: String,
    pub camera: String,
    pub on: bool,
    /// Each camera view holds hold_min..hold_max seconds.
    pub hold_min: u32,
    pub hold_max: u32,
    /// "glide", "cut" or "mix".
    pub glide: String,
    /// The views the director may use.
    pub shots: Vec<String>,
    /// 2D: seconds per picture.
    pub seconds: u32,
    pub shuffle: bool,
}

impl Default for WorldScreen {
    fn default() -> Self {
        Self {
            screen: String::new(),
            width: 0,
            height: 0,
            x: 0,
            y: 0,
            world: DEFAULT_WORLD.into(),
            camera: "director".into(),
            on: false,
            hold_min: 8,
            hold_max: 20,
            glide: "mix".into(),
            shots: SHOTS.iter().map(|s| s.to_string()).collect(),
            seconds: 15,
            shuffle: true,
        }
    }
}

impl WorldScreen {
    pub fn normalized(mut self) -> Self {
        if !is_world_id(&self.world) {
            self.world = DEFAULT_WORLD.into();
        }
        if !CAMERAS.contains(&self.camera.as_str()) {
            self.camera = "director".into();
        }
        if !GLIDES.contains(&self.glide.as_str()) {
            self.glide = "mix".into();
        }
        self.hold_min = self.hold_min.clamp(3, 120);
        self.hold_max = self.hold_max.clamp(3, 120);
        if self.hold_min > self.hold_max {
            std::mem::swap(&mut self.hold_min, &mut self.hold_max);
        }
        let mut shots: Vec<String> = SHOTS.iter().filter(|s| self.shots.iter().any(|t| t == *s)).map(|s| s.to_string()).collect();
        if shots.is_empty() {
            shots = SHOTS.iter().map(|s| s.to_string()).collect();
        }
        self.shots = shots;
        self.seconds = self.seconds.clamp(4, 120);
        self
    }
}

/// The WORLD room: each screen's world, and the music.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorldSettings {
    pub music: bool,
    /// 0.0 to 1.0.
    pub volume: f32,
    /// Extra picture folders for the 2D slideshow, beside the memory folder's own pictures.
    pub folders: Vec<String>,
    /// One entry per screen the member has set; a screen with no entry stays off.
    pub screens: Vec<WorldScreen>,
}

impl Default for WorldSettings {
    fn default() -> Self {
        // a fresh install starts quiet: the member turns the music on (a saved file keeps its choice)
        Self { music: false, volume: 0.5, folders: Vec::new(), screens: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The member's MemoryCore folder: the companion's session runs here.
    pub memory_folder: Option<String>,
    pub cli: Cli,
    pub companion_name: Option<String>,
    pub member_name: Option<String>,
    pub relationship_style: Option<String>,
    pub theme: Option<String>,
    /// Off until the member turns it on.
    pub start_with_windows: bool,
    /// Start the companion's session when Castle opens.
    pub start_session_on_launch: bool,
    /// The top bar's minus button minimises Castle to the taskbar. Off (the default), it hides
    /// Castle to the tray icon, as before.
    pub minimize_to_taskbar: bool,
    /// A sound, and a desktop notification, when a new line arrives in the chat room while the
    /// member is not looking at CHAT. The notification is on by default; the sound is off by default
    /// (it plays through Windows' quiet hours); each has its own switch in Settings > Rooms.
    pub alert_sound: bool,
    pub alert_notify: bool,
    /// "all" (every new line, the default) or "named" (only a line that names the member or
    /// their companion, as a whole word): which lines the chat alerts tell about.
    pub alert_when: String,
    /// Optional local-time clock immediately before the top bar's music controls.
    pub display_clock: bool,
    /// "12h" or "24h".
    pub clock_format: String,
    /// Use VoiceStudio, a text-to-speech app the member runs on this same computer, as STUDIO's Voice
    /// power. Off until the member turns it on: Castle then never talks to it.
    pub voicestudio_on: bool,
    /// Whether HOME shows the USAGE card at all. On by default; off hides the whole card and Castle
    /// asks neither CLI.
    pub show_usage_card: bool,
    /// Whether HOME's USAGE card shows Claude Code's limits, and Codex's. On by default; a CLI that
    /// is off is not asked at all.
    pub show_claude_usage: bool,
    pub show_codex_usage: bool,
    /// The id of the companion's room key Castle made on the Portfolio (so it can be removed again).
    pub room_companion_key_id: Option<String>,
    /// The name that key carries in the room (a rename in Settings remakes the key).
    pub room_companion_name: Option<String>,
    pub world: WorldSettings,
    /// The name of the screen Castle itself lives on ("" = wherever it opens, the primary screen).
    pub castle_screen: String,
    /// True once the first-run setup has finished.
    pub setup_done: bool,
    /// The member's own quick-action buttons, after Castle's built-in ones.
    pub quick_actions: Vec<QuickAction>,
    /// Back up the memory to the member's private GitHub repository after every Save diary.
    pub backup_after_diary: bool,
    /// The companion in the Foundation chat room: "off", "named" (answers when its name is
    /// said) or "joins" (joins in now and then).
    pub room_companion_mode: String,
    /// Which rooms are available (every one in ROOMS, on by default).
    pub rooms: std::collections::BTreeMap<String, bool>,
    /// How the STORY stage looks: one of STORY_LOOKS.
    pub story_look: String,
    /// The last Castle version whose "What's new" the member was shown (None before the first launch).
    pub last_seen_version: Option<String>,
    /// The Neural Link: the member's own terminals leave each other notes in <memory>/link/.
    pub link_on: bool,
    /// The room companion may pass the member's own words to the workspace (off until ticked).
    pub room_may_pass: bool,
    /// The port of the member's database, fixed once chosen (None until the engine is installed
    /// or an existing one is chosen). Rust's to keep: the page never writes it.
    pub db_port: Option<u16>,
    /// True when the member uses a MySQL / MariaDB that was already on this PC (XAMPP).
    pub db_external: bool,
    /// Leave Castle's database engine running after Castle closes (off: it stops with Castle).
    pub db_keep_running: bool,
    /// Castle reads the memory from the database when it wakes the companion and starts her
    /// with it already loaded (Claude Code). Off: she loads it herself with `castle-memory boot`.
    pub db_boot_at_wake: bool,
    /// True once the member has answered the one-time offer to move the memory into a database
    /// (either button), or finished a setup that showed it.
    pub db_offer_seen: bool,
    /// The DEV tab's open projects, one folder per tab (at most `dev_tabs::SLOTS`); empty = closed.
    pub dev_tabs: Vec<Option<String>>,
    /// The DEV tab's recent projects, newest first (at most `dev_tabs::RECENT`).
    pub dev_projects: Vec<String>,
    /// Castle's session is only this companion: the member's personal Claude Code setup (its
    /// own CLAUDE.md, hooks and plugins) is left out, and Castle hands the companion her
    /// instructions itself. Off by default; for a PC where that setup is another companion.
    pub only_this_companion: bool,
    /// How the DIARY room is read: one of DIARY_LOOKS.
    pub diary_look: String,
    /// The companion's emotion engine colours how it talks in the chat room (only its traits'
    /// one-line manner, by audience). On by default; the switch is beside its public card.
    pub emotion_room_voice: bool,
    /// How HOME's PROFILE tab shows the emotion engine: one of PROFILE_LOOKS.
    pub profile_look: String,
    /// The DEV tab's projects folder: new projects are cloned or made in it (or in one of its
    /// folders). None until the member chooses one; there is no default.
    pub dev_root: Option<String>,
    /// The top bar's tabs in the member's own order (tab ids, dragged into place). Empty means
    /// Castle's own order; a tab not listed (a room added in a later release) follows the listed ones.
    pub tab_order: Vec<String>,
}

/// The longest tab order kept, and the longest tab id in it.
const TAB_ORDER_MAX: usize = 32;
const TAB_ID_MAX: usize = 32;

pub const DIARY_LOOKS: [&str; 3] = ["list", "calendar", "book"];
pub const PROFILE_LOOKS: [&str; 2] = ["cards", "constellation"];
pub const STORY_LOOKS: [&str; 5] = ["classic", "cinema", "book", "messenger", "jrpg"];

fn all_rooms_on() -> std::collections::BTreeMap<String, bool> {
    ROOMS.iter().map(|r| (r.to_string(), true)).collect()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            memory_folder: None,
            cli: Cli::Claude,
            companion_name: None,
            member_name: None,
            relationship_style: None,
            theme: None,
            start_with_windows: false,
            start_session_on_launch: true,
            minimize_to_taskbar: false,
            alert_sound: false,
            alert_notify: true,
            alert_when: "all".into(),
            display_clock: false,
            clock_format: "24h".into(),
            voicestudio_on: false,
            show_usage_card: true,
            show_claude_usage: true,
            show_codex_usage: true,
            room_companion_key_id: None,
            room_companion_name: None,
            world: WorldSettings::default(),
            castle_screen: String::new(),
            setup_done: false,
            quick_actions: Vec::new(),
            backup_after_diary: false,
            room_companion_mode: "off".into(),
            rooms: all_rooms_on(),
            story_look: "classic".into(),
            last_seen_version: None,
            link_on: true,
            room_may_pass: false,
            db_port: None,
            db_external: false,
            db_keep_running: false,
            db_boot_at_wake: true,
            db_offer_seen: false,
            dev_tabs: Vec::new(),
            dev_projects: Vec::new(),
            only_this_companion: false,
            diary_look: "list".into(),
            emotion_room_voice: true,
            profile_look: "cards".into(),
            dev_root: None,
            tab_order: Vec::new(),
        }
    }
}

impl Settings {
    /// Trims every text field; an empty one means unset. The room address loses its trailing slash.
    pub fn normalized(mut self) -> Self {
        if !["12h", "24h"].contains(&self.clock_format.as_str()) { self.clock_format = "24h".into(); }
        if !["all", "named"].contains(&self.alert_when.as_str()) { self.alert_when = "all".into(); }
        fn clean(value: Option<String>) -> Option<String> {
            value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
        }
        self.memory_folder = clean(self.memory_folder);
        self.companion_name = clean(self.companion_name);
        self.member_name = clean(self.member_name);
        self.relationship_style = clean(self.relationship_style);
        self.theme = clean(self.theme);
        self.room_companion_key_id = clean(self.room_companion_key_id);
        self.room_companion_name = clean(self.room_companion_name);
        self.last_seen_version = clean(self.last_seen_version);
        self.dev_root = clean(self.dev_root);
        // the tab order: plain ids only, each once
        let mut seen = std::collections::BTreeSet::new();
        self.tab_order = std::mem::take(&mut self.tab_order)
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty() && id.len() <= TAB_ID_MAX && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .filter(|id| seen.insert(id.clone()))
            .take(TAB_ORDER_MAX)
            .collect();
        self.world.volume = self.world.volume.clamp(0.0, 1.0);
        self.quick_actions.retain(|a| !a.label.trim().is_empty() && !a.command.trim().is_empty());
        for action in &mut self.quick_actions {
            action.label = action.label.trim().to_string();
            // a quick action types one line: no line breaks may sneak in
            action.command = action.command.replace(['\r', '\n'], " ").trim().to_string();
        }
        if !["off", "named", "joins"].contains(&self.room_companion_mode.as_str()) {
            self.room_companion_mode = "off".into();
        }
        self.world.screens = std::mem::take(&mut self.world.screens).into_iter().map(WorldScreen::normalized).collect();
        if !STORY_LOOKS.contains(&self.story_look.as_str()) {
            self.story_look = "classic".into();
        }
        if !DIARY_LOOKS.contains(&self.diary_look.as_str()) {
            self.diary_look = "list".into();
        }
        if !PROFILE_LOOKS.contains(&self.profile_look.as_str()) {
            self.profile_look = "cards".into();
        }
        // every known room is listed (a missing one is on); unknown names go
        self.rooms = ROOMS.iter().map(|r| (r.to_string(), self.rooms.get(*r).copied().unwrap_or(true))).collect();
        self
    }

    /// HOME is always on; any other room as the member set it.
    pub fn room_on(&self, room: &str) -> bool {
        room == "home" || self.rooms.get(room).copied().unwrap_or(true)
    }

    /// The companion's name for the screen, or a gentle default before setup.
    pub fn companion_label(&self) -> String {
        self.companion_name.clone().unwrap_or_else(|| "Your companion".into())
    }
}

/// Reads the file. A missing file gives the defaults; a broken one gives the defaults and a reason.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => (settings.normalized(), None),
            Err(_) => (
                Settings::default(),
                Some("The settings file could not be read, so the defaults are in use.".into()),
            ),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (Settings::default(), None),
        Err(_) => (
            Settings::default(),
            Some("The settings file could not be opened, so the defaults are in use.".into()),
        ),
    }
}

/// Writes beside the file first and then swaps it in, so a crash never leaves half a file.
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    let partial = path.with_extension("json.partial");
    fs::write(&partial, text)?;
    fs::rename(&partial, path)
}

/// Test builds only: a separate settings folder, so testing never touches the installed Castle's.
pub const TEST_CONFIG_VAR: &str = "CASTLE_CONFIG_DIR";

/// Reads a test-only switch; the installed Castle never does.
pub fn test_override(name: &str) -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
    } else {
        None
    }
}

/// The settings the running Castle holds, and where they are kept.
pub struct SettingsStore {
    pub path: PathBuf,
    pub current: Mutex<Settings>,
    pub warning: Mutex<Option<String>>,
    /// Why Start with Windows is not in effect, when it is not.
    pub startup_note: Mutex<Option<String>>,
}

impl SettingsStore {
    pub fn open(folder: &Path) -> Self {
        let path = folder.join(FILE_NAME);
        let (current, warning) = load(&path);
        Self {
            path,
            current: Mutex::new(current),
            warning: Mutex::new(warning),
            startup_note: Mutex::new(None),
        }
    }

    /// The folder that holds settings.json, notices.jsonl, themes and the avatar.
    pub fn folder(&self) -> PathBuf {
        self.path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_emotion_engine_colours_the_room_by_default() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(old.emotion_room_voice && Settings::default().emotion_room_voice);
        let off: Settings = serde_json::from_str(r#"{"emotionRoomVoice":false}"#).unwrap();
        assert!(!off.emotion_room_voice);
        assert_eq!(old.profile_look, "cards");
        let odd: Settings = serde_json::from_str(r#"{"profileLook":"spiral"}"#).unwrap();
        assert_eq!(odd.normalized().profile_look, "cards");
    }

    #[test]
    fn the_tab_order_keeps_plain_ids_once_each() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(old.tab_order.is_empty());
        let messy: Settings = serde_json::from_str(r#"{"tabOrder":[" dev ","home","dev","","../x","chat"]}"#).unwrap();
        assert_eq!(messy.normalized().tab_order, vec!["dev", "home", "chat"]);
        let long: Settings = serde_json::from_str(&format!(r#"{{"tabOrder":{}}}"#, serde_json::to_string(&(0..40).map(|i| format!("t{i}")).collect::<Vec<_>>()).unwrap())).unwrap();
        assert_eq!(long.normalized().tab_order.len(), TAB_ORDER_MAX);
    }

    #[test]
    fn clock_defaults_and_invalid_formats_are_safe() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(!old.display_clock);
        assert_eq!(old.clock_format, "24h");
        let invalid: Settings = serde_json::from_str(r#"{"displayClock":true,"clockFormat":"23h"}"#).unwrap();
        let fixed = invalid.normalized();
        assert!(fixed.display_clock);
        assert_eq!(fixed.clock_format, "24h");
    }

    fn temp_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("castle-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn local_voicestudio_is_off_until_turned_on() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(!old.voicestudio_on);
        assert!(!Settings::default().voicestudio_on);
        let on: Settings = serde_json::from_str(r#"{"voicestudioOn":true}"#).unwrap();
        assert!(on.voicestudio_on);
    }

    #[test]
    fn the_chat_notification_is_on_and_the_sound_off_for_new_and_older_settings_files() {
        assert!(Settings::default().alert_notify);
        assert!(!Settings::default().alert_sound, "the sound ignores quiet hours, so it starts off");
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(old.alert_notify && !old.alert_sound);
        let chosen: Settings = serde_json::from_str(r#"{"alertSound":true,"alertNotify":false}"#).unwrap();
        assert!(chosen.alert_sound && !chosen.alert_notify);
        assert_eq!(old.alert_when, "all", "an older file alerts for every line, as before");
        let named: Settings = serde_json::from_str::<Settings>(r#"{"alertWhen":"named"}"#).unwrap().normalized();
        assert_eq!(named.alert_when, "named");
        let odd: Settings = serde_json::from_str::<Settings>(r#"{"alertWhen":"loud"}"#).unwrap().normalized();
        assert_eq!(odd.alert_when, "all");
    }

    #[test]
    fn a_missing_file_gives_the_defaults_with_start_up_off() {
        let folder = temp_folder("missing");
        let (settings, warning) = load(&folder.join(FILE_NAME));
        assert_eq!(settings, Settings::default());
        assert!(!settings.start_with_windows);
        assert!(!settings.minimize_to_taskbar, "the minus button hides to the tray unless the member chooses the taskbar");
        assert!(!settings.setup_done);
        assert_eq!(settings.cli, Cli::Claude);
        assert!(warning.is_none());
    }

    #[test]
    fn settings_survive_a_save_and_a_load() {
        let folder = temp_folder("roundtrip");
        let path = folder.join(FILE_NAME);
        let settings = Settings {
            memory_folder: Some("D:\\MemoryCore".into()),
            cli: Cli::Codex,
            companion_name: Some("Lyra".into()),
            member_name: Some("Rin".into()),
            relationship_style: Some("Friendly partner".into()),
            theme: Some("constellation".into()),
            start_with_windows: true,
            start_session_on_launch: false,
            minimize_to_taskbar: true,
            alert_sound: true,
            alert_notify: false,
            alert_when: "named".into(),
            display_clock: true,
            clock_format: "12h".into(),
            voicestudio_on: true,
            show_usage_card: false,
            show_claude_usage: false,
            show_codex_usage: true,
            room_companion_key_id: Some("17".into()),
            room_companion_name: Some("Lyra".into()),
            world: WorldSettings {
                music: false,
                volume: 0.25,
                folders: vec!["D:\\Pictures".into()],
                screens: vec![WorldScreen { screen: "\\\\.\\DISPLAY5".into(), world: "3d/crystal-isle".into(), camera: "orbit".into(), on: true, ..WorldScreen::default() }],
            },
            castle_screen: "\\\\.\\DISPLAY1".into(),
            setup_done: true,
            quick_actions: vec![QuickAction { label: "Plan my day".into(), command: "plan my day".into() }],
            backup_after_diary: true,
            room_companion_mode: "named".into(),
            rooms: { let mut r = all_rooms_on(); r.insert("story".into(), false); r },
            story_look: "jrpg".into(),
            last_seen_version: Some("0.2.0".into()),
            link_on: false,
            room_may_pass: true,
            db_port: Some(3307),
            db_external: false,
            db_keep_running: true,
            db_boot_at_wake: false,
            db_offer_seen: true,
            dev_tabs: vec![Some("D:/Projects/shop".into()), None],
            dev_projects: vec!["D:/Projects/shop".into()],
            only_this_companion: true,
            diary_look: "book".into(),
            emotion_room_voice: false,
            profile_look: "constellation".into(),
            dev_root: Some("D:/Projects".into()),
            tab_order: vec!["dev".into(), "home".into(), "chat".into()],
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), (settings, None));
        assert!(!folder.join("settings.json.partial").exists());
    }

    #[test]
    fn the_cli_is_written_in_lowercase() {
        let text = serde_json::to_string(&Settings { cli: Cli::Codex, ..Settings::default() }).unwrap();
        assert!(text.contains("\"cli\":\"codex\""));
        assert_eq!(Cli::Codex.command(), "codex");
        assert_eq!(Cli::Claude.label(), "Claude Code");
    }

    #[test]
    fn a_broken_file_gives_the_defaults_and_a_reason() {
        let folder = temp_folder("broken");
        let path = folder.join(FILE_NAME);
        fs::write(&path, "{ not json").unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.is_some());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let folder = temp_folder("partial");
        let path = folder.join(FILE_NAME);
        fs::write(&path, r#"{"memoryFolder":"D:\\MemoryCore","somethingNew":1}"#).unwrap();
        let (settings, _) = load(&path);
        assert_eq!(settings.memory_folder.as_deref(), Some("D:\\MemoryCore"));
        assert!(settings.start_session_on_launch);
        assert!(!settings.minimize_to_taskbar, "a settings file from before this option keeps the tray behaviour");
        assert!(settings.show_claude_usage && settings.show_codex_usage, "a settings file from before the toggles shows both");
        assert!(settings.show_usage_card, "a settings file from before the card switch shows the card");
        assert_eq!(settings.world, WorldSettings::default());
    }

    #[test]
    fn text_is_trimmed_and_odd_values_are_brought_back_in_range() {
        let settings = Settings {
            room_companion_key_id: Some("   ".into()),
            companion_name: Some("   ".into()),
            world: WorldSettings { volume: 3.0, ..WorldSettings::default() },
            ..Settings::default()
        }
        .normalized();
        assert_eq!(settings.room_companion_key_id, None);
        assert_eq!(settings.companion_name, None);
        assert_eq!(settings.world.volume, 1.0);
        assert_eq!(settings.companion_label(), "Your companion");
    }

    #[test]
    fn a_screen_with_odd_world_settings_is_brought_back_in_range() {
        let screen = WorldScreen {
            world: "3d/../secrets".into(),
            camera: "drone".into(),
            glide: "teleport".into(),
            hold_min: 500,
            hold_max: 1,
            shots: vec!["selfie".into()],
            seconds: 0,
            ..WorldScreen::default()
        }
        .normalized();
        assert_eq!(screen.world, DEFAULT_WORLD);
        assert_eq!(screen.camera, "director");
        assert_eq!(screen.glide, "mix");
        assert_eq!((screen.hold_min, screen.hold_max), (3, 120));
        assert_eq!(screen.shots.len(), SHOTS.len());
        assert_eq!(screen.seconds, 4);
        let kept = WorldScreen { shots: vec!["crane".into(), "close".into(), "close".into()], ..WorldScreen::default() }.normalized();
        assert_eq!(kept.shots, vec!["close".to_string(), "crane".to_string()]);
    }

    #[test]
    fn world_ids_are_a_kind_and_a_plain_slug() {
        assert!(is_world_id("3d/crystal-isle"));
        assert!(is_world_id("my/star-garden-2"));
        assert!(is_world_id("2d/mine"));
        assert!(!is_world_id("3d/Crystal"));
        assert!(!is_world_id("my/a\\b"));
        assert!(!is_world_id("my/"));
        assert!(!is_world_id("my/-x"));
        assert!(!is_world_id("5d/x"));
        assert!(!is_world_id("crystal-isle"));
    }

    #[test]
    fn every_room_starts_on_home_cannot_be_off_and_unknown_rooms_go() {
        let fresh = Settings::default();
        assert!(ROOMS.iter().all(|r| fresh.room_on(r)));
        let mut rooms = std::collections::BTreeMap::new();
        rooms.insert("story".to_string(), false);
        rooms.insert("home".to_string(), false);
        rooms.insert("spaceship".to_string(), true);
        let s = Settings { rooms, ..Settings::default() }.normalized();
        assert!(!s.room_on("story"));
        assert!(s.room_on("home"));
        assert!(s.room_on("world"), "a room missing from the file is on");
        assert!(!s.rooms.contains_key("spaceship") && !s.rooms.contains_key("home"));
        assert_eq!(s.rooms.len(), ROOMS.len());
    }

    #[test]
    fn the_last_seen_version_is_trimmed_and_starts_unset() {
        assert_eq!(Settings::default().last_seen_version, None);
        assert_eq!(Settings { last_seen_version: Some(" 0.2.0 ".into()), ..Settings::default() }.normalized().last_seen_version.as_deref(), Some("0.2.0"));
        assert_eq!(Settings { last_seen_version: Some("  ".into()), ..Settings::default() }.normalized().last_seen_version, None);
    }

    #[test]
    fn the_diary_look_is_one_of_the_three() {
        assert_eq!(Settings::default().diary_look, "list");
        assert_eq!(Settings { diary_look: "calendar".into(), ..Settings::default() }.normalized().diary_look, "calendar");
        assert_eq!(Settings { diary_look: "scroll".into(), ..Settings::default() }.normalized().diary_look, "list");
    }

    #[test]
    fn the_story_look_is_one_of_the_five() {
        assert_eq!(Settings::default().story_look, "classic");
        assert_eq!(Settings { story_look: "messenger".into(), ..Settings::default() }.normalized().story_look, "messenger");
        assert_eq!(Settings { story_look: "hologram".into(), ..Settings::default() }.normalized().story_look, "classic");
    }

    #[test]
    fn a_new_member_has_every_screen_off() {
        assert!(Settings::default().world.screens.is_empty());
        assert!(!WorldScreen::default().on);
    }

    #[test]
    fn quick_actions_are_one_clean_line_each() {
        let settings = Settings {
            quick_actions: vec![
                QuickAction { label: " Review ".into(), command: "review\r\nmy week ".into() },
                QuickAction { label: "".into(), command: "x".into() },
            ],
            ..Settings::default()
        }
        .normalized();
        assert_eq!(settings.quick_actions, vec![QuickAction { label: "Review".into(), command: "review  my week".into() }]);
    }

}
