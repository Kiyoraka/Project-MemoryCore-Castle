//! Castle's settings for this machine: one JSON file in the app config folder.
//! Keys never live here; they are kept by Windows Credential Manager (key.rs).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
pub const ROOMS: [&str; 17] = ["widgets", "theme", "apps", "myprofile", "diary", "projects", "library", "appearance", "studio", "story", "world", "chat", "dev", "market", "messaging", "automations", "calendar"];
/// Rooms that wait for the member's own switch in Settings > Rooms, for a new Castle and for one
/// that never stored a choice for them.
const ROOMS_OFF_AT_FIRST: [&str; 5] = ["library", "market", "messaging", "automations", "calendar"];

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

/// The keys that start and stop Chat's mic, set on Settings > Chat > Mic: the key by its
/// KeyboardEvent code (KeyA..KeyZ, Digit0..Digit9), with ⌘ on a Mac (Ctrl elsewhere) always held,
/// plus Shift and/or Alt. None (the default) is ⌘⇧M / Ctrl+Shift+M.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MicShortcut {
    pub code: String,
    pub shift: bool,
    pub alt: bool,
}

impl MicShortcut {
    /// A letter or a digit key; anything else is no choice.
    fn known(&self) -> bool {
        let code = self.code.as_str();
        let one = |rest: &str, ok: fn(char) -> bool| rest.len() == 1 && rest.chars().all(ok);
        code.strip_prefix("Key").is_some_and(|r| one(r, |c| c.is_ascii_uppercase()))
            || code.strip_prefix("Digit").is_some_and(|r| one(r, |c| c.is_ascii_digit()))
    }
}

/// One alert source's choices, set on Settings > Alerts. A field left out (None) keeps the source's
/// own default, so a source added later by a theme or a new feature works before it is ever set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertRule {
    pub notify: Option<bool>,
    pub sound: Option<bool>,
    pub sound_name: Option<String>,
}

/// Someone the member approved to message the Castle's WhatsApp number besides themselves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WhatsAppGuest {
    pub chat_id: String,
    pub name: String,
    /// the phone digits behind the chat, when WhatsApp gave them
    pub digits: Option<String>,
    /// seconds since 1970
    pub approved_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The member's MemoryCore folder: the companion's session runs here.
    pub memory_folder: Option<String>,
    pub cli: Cli,
    pub automations_paused: bool,
    pub automations_commands: Vec<String>,
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
    /// which sound: "windows" (Windows' own notification sound) or one of Castle's own (see roomalert::SOUNDS)
    pub alert_sound_name: String,
    pub alert_notify: bool,
    /// "all" (every new line, the default) or "named" (only a line that names the member or
    /// their companion, as a whole word): which lines the chat alerts tell about.
    pub alert_when: String,
    /// Also tell the member when someone reacts to one of their lines (off by default; needs a
    /// room that offers reactions).
    pub alert_reactions: bool,
    /// Every other alert source's rule, by the source's id (see alerts.rs). "chat" keeps the four
    /// fields above, so an older settings file reads exactly as it did.
    pub alerts: BTreeMap<String, AlertRule>,
    /// The companion may draw a picture in the chat room when someone asks (off by default), at
    /// most this many a day (0..5, under the room's own cap), at this quality when the drawing
    /// spends the member's ChatGPT key ("low" or "medium"; Codex uses the subscription).
    pub room_pictures_on: bool,
    pub room_pictures_per_day: u32,
    pub room_picture_quality: String,
    /// The companion may speak a short line aloud in the chat room with the member's ElevenLabs
    /// key (off by default), at most this many a day (0..5, under the room's own cap), in this
    /// ElevenLabs voice (none = the default voice STUDIO uses).
    pub room_voices_on: bool,
    pub room_voices_per_day: u32,
    pub room_voice_id: Option<String>,
    /// The companion brings its life into the chat room: a shelf of notes it chose, in private,
    /// from its diary and the rest of its memory (off by default; roomshelf.rs).
    pub room_shelf_on: bool,
    /// The companion wakes in a new mood every day, written by the member's own CLI (mood.rs).
    pub daily_mood_on: bool,
    /// In the Marketplace town, the shopkeeper explains items and answers questions through one short
    /// run of the member's own CLI (keeper.rs). Off: the item's own description shows instead.
    pub keeper_speaks: bool,
    /// Optional local-time clock immediately before the top bar's music controls.
    pub display_clock: bool,
    /// "12h" or "24h".
    pub clock_format: String,
    /// Use VoiceStudio, a text-to-speech app the member runs on this same computer, as STUDIO's Voice
    /// power. Off until the member turns it on: Castle then never talks to it.
    pub voicestudio_on: bool,
    /// MemoryCore in your pocket: let the member text their own companion's real session from
    /// Telegram. Off until switched on; the bot token lives in Windows Credential Manager.
    pub telegram_bridge_on: bool,
    /// The one chat id (as text) the bridge answers, set by telegram_bridge_approve once the
    /// member approves whoever first messaged the bot. None: nothing is answered yet.
    pub telegram_bridge_chat_id: Option<String>,
    /// MemoryCore in your pocket, Discord: same idea as the Telegram switch, but a Gateway
    /// WebSocket DM instead of long-polling.
    pub discord_bridge_on: bool,
    /// The one DM channel id the bridge answers, set by discord_bridge_approve.
    pub discord_bridge_channel_id: Option<String>,
    /// MemoryCore in your pocket, WhatsApp: no token here - the "credential" is the QR-paired
    /// session itself, kept in the settings folder's whatsapp-auth. Unofficial, ban risk.
    pub whatsapp_bridge_on: bool,
    /// The chat id (a WhatsApp JID) of the member's own number - "Me": the one chat whose words
    /// reach the companion's real session. Set by whatsapp_bridge_approve / _make_me.
    pub whatsapp_bridge_chat_id: Option<String>,
    /// The name WhatsApp gave the member's own chat, for the Approved numbers list.
    pub whatsapp_bridge_me_name: Option<String>,
    /// The phone digits behind the member's own chat, when WhatsApp gave them: the same person is
    /// known by them even when the chat id changes.
    pub whatsapp_bridge_me_digits: Option<String>,
    /// People the member approved to message the Castle's WhatsApp number besides themselves.
    /// They are answered by a guest-only run (bridges/guests.rs), never by the companion's real
    /// session. Re-pairing the number keeps them; only Remove lets one go.
    pub whatsapp_guests: Vec<WhatsAppGuest>,
    /// Whether HOME shows the USAGE card at all. On by default; off hides the whole card and Castle
    /// asks neither CLI.
    pub show_usage_card: bool,
    /// A theme's living wallpaper on HOME (its `motionVideo` loop, or its drawn `motion`): on by
    /// default since Nov 20 2026; off, HOME shows the still picture and nothing is loaded. A new
    /// name, so the old `wallpaperMotion: false` every settings file carries never keeps it off.
    pub living_wallpaper: bool,
    /// A theme's looping sound on HOME (its `motionSound`: the shores' waves, the river): on by
    /// default since Nov 20 2026, at a share of the music slider's level; off, nothing is fetched.
    pub wallpaper_sound: bool,
    /// The id of the companion's room key Castle made on the Portfolio (so it can be removed again).
    pub room_companion_key_id: Option<String>,
    /// The name that key carries in the room (a rename in Settings remakes the key).
    pub room_companion_name: Option<String>,
    pub world: WorldSettings,
    /// The name of the screen Castle itself lives on ("" = wherever it opens, the primary screen).
    pub castle_screen: String,
    /// How much of that screen Castle takes: "full" (or "" from older files), "left", "right"
    /// (half the screen) or "free" (the member drags the edges; `window_bounds` keeps the size).
    pub window_layout: String,
    /// The free layout's last place and size: x, y, width, height in physical pixels.
    pub window_bounds: Option<[i32; 4]>,
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
    /// Each DEV tab's number within its project ("shop · 2"), by slot; 0 or missing = not kept yet.
    pub dev_tab_numbers: Vec<u32>,
    /// The DEV tab's recent projects, newest first (at most `dev_tabs::RECENT`).
    pub dev_projects: Vec<String>,
    /// Each DEV project's own database logins, by `dev_tabs::same_folder_key` (the Database panel's
    /// form). Never a password: those are kept by Windows Credential Manager / the Keychain.
    pub project_databases: BTreeMap<String, ProjectDb>,
    /// Castle's session is only this companion: the member's personal Claude Code setup (its
    /// own CLAUDE.md, hooks and plugins) is left out, and Castle hands the companion her
    /// instructions itself. Off by default; for a PC where that setup is another companion.
    pub only_this_companion: bool,
    /// How the DIARY room is read: one of DIARY_LOOKS.
    pub diary_look: String,
    /// The companion's emotion engine colours how it talks in the chat room (only its traits'
    /// one-line manner, by audience). On by default; the switch is beside its public card.
    pub emotion_room_voice: bool,
    /// The companion's emotion engine colours how it talks at the MARKET desk - its own switch,
    /// apart from the room's, in MARKET > Settings. On by default.
    pub emotion_market_voice: bool,
    /// How HOME's PROFILE tab shows the emotion engine: one of PROFILE_LOOKS.
    pub profile_look: String,
    /// The keys for Chat's mic (None = ⌘⇧M / Ctrl+Shift+M).
    pub mic_shortcut: Option<MicShortcut>,
    /// The DEV tab's projects folder: new projects are cloned or made in it (or in one of its
    /// folders). None until the member chooses one; there is no default.
    pub dev_root: Option<String>,
    /// The top bar's tabs in the member's own order (tab ids, dragged into place). Empty means
    /// Castle's own order; a tab not listed (a room added in a later release) follows the listed ones.
    pub tab_order: Vec<String>,
    /// The rooms pinned to the top bar beside HOME and APPS (the room launcher). Every other room opens from
    /// APPS. A settings file without this key gets Castle's first pins; an empty list means none.
    pub pinned_rooms: Vec<String>,
    /// HOME's widgets the member switched off in WIDGETS. USAGE is not listed here: its switch is
    /// `show_usage_card`, which also stops Castle asking the CLIs.
    pub widgets_off: Vec<String>,
    /// HOME's widgets that stay hidden until the member switches them on (WIDGET_OPT_IN), so one that
    /// asks for something, like the weather's location, never appears by itself after an update.
    pub widgets_on: Vec<String>,
    /// Each widget's size on HOME: one of WIDGET_SIZES (a widget not listed is "m").
    pub widget_sizes: std::collections::BTreeMap<String, String>,
    /// Where the member put HOME's widgets, per theme id: the widget's top-left corner as
    /// percentages of HOME. A widget not listed keeps the theme's own place.
    pub widget_places: std::collections::BTreeMap<String, std::collections::BTreeMap<String, WidgetPlace>>,
    /// The gauges' gear: watch CPU and RAM, and the value each trips at. Changed only by its own
    /// command (meter_watch_save), never by the Settings page.
    pub meter_watch: crate::meter_watch::MeterWatch,
    /// The graphics-card gauges beside CPU and RAM: shown or not. Changed only by its own command
    /// (gpu_gauges_save), never by the Settings page.
    pub gpu_gauges: crate::gpu::GpuGauges,
    /// The weather widget's place and units. Changed only by its own command (weather_save), never
    /// by the Settings page.
    pub weather: crate::weather::WeatherSettings,
    /// Sweep's gear: the look chosen by hand and whether the Recycle Bin starts ticked. Changed only
    /// by its own command (sweep_settings_save), never by the Settings page.
    pub sweep: crate::sweep::SweepSettings,
    /// The Sentinel's gear: on, the model, how often it patrols. Changed only by its own command
    /// (sentinel_settings_save), never by the Settings page.
    pub sentinel: crate::sentinel::SentinelSettings,
    /// HOME's credit widgets: the OpenAI and Seedance figures, the page readers' last word, Suno's
    /// whole and the Seedance budget. Changed only by the credits commands.
    pub credits: crate::credits::CreditsSaved,
}

/// HOME's widgets, by id (the theme's `widgets` keys).
pub const WIDGETS: [&str; 19] = [
    "clock", "gauges", "weather", "disks", "sentinel", "automations", "calendar", "sweep", "usage", "today", "projects", "diary", "growth", "backup", "database",
    // HOME's credit widgets, each shown once its key is saved
    "credit-openai", "credit-seedance", "credit-suno", "credit-elevenlabs",
];
/// Widgets that are off until the member switches them on.
pub const WIDGET_OPT_IN: [&str; 1] = ["weather"];
pub const WIDGET_SIZES: [&str; 3] = ["s", "m", "l"];
/// The most themes whose widget places are kept.
const WIDGET_THEMES_MAX: usize = 64;

/// A widget's top-left corner on HOME, in percent of HOME's width and height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WidgetPlace {
    pub left: f64,
    pub top: f64,
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= TAB_ID_MAX && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The longest tab order kept, and the longest tab id in it.
const TAB_ORDER_MAX: usize = 32;

/// The rooms pinned to the top bar until the member chooses their own.
pub const DEFAULT_PINS: [&str; 3] = ["chat", "projects", "dev"];
fn default_pinned_rooms() -> Vec<String> {
    DEFAULT_PINS.iter().map(|r| r.to_string()).collect()
}
/// HOME and the launcher are always in the top bar and are never "pinned"; `widgets` is not a tab.
const ALWAYS_SHOWN: [&str; 3] = ["home", "launcher", "widgets"];
const TAB_ID_MAX: usize = 32;

pub const DIARY_LOOKS: [&str; 3] = ["list", "calendar", "book"];
pub const PROFILE_LOOKS: [&str; 2] = ["cards", "constellation"];
pub const STORY_LOOKS: [&str; 5] = ["classic", "cinema", "book", "messenger", "jrpg"];

/// Every room on for a new Castle, except LIBRARY and MARKET: each is the member's switch to turn
/// on (Settings > Rooms). A choice already stored is kept as it is.
fn all_rooms_on() -> std::collections::BTreeMap<String, bool> {
    ROOMS.iter().map(|r| (r.to_string(), !ROOMS_OFF_AT_FIRST.contains(r))).collect()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            memory_folder: None,
            automations_paused: false,
            automations_commands: Vec::new(),
            cli: Cli::Claude,
            companion_name: None,
            member_name: None,
            relationship_style: None,
            theme: None,
            start_with_windows: false,
            start_session_on_launch: true,
            minimize_to_taskbar: false,
            alert_sound: false,
            alert_sound_name: "glass".into(),
            alert_notify: true,
            alert_when: "all".into(),
            alert_reactions: false,
            alerts: BTreeMap::new(),
            room_pictures_on: false,
            room_pictures_per_day: 1,
            room_picture_quality: "low".into(),
            room_voices_on: false,
            room_voices_per_day: 2,
            room_voice_id: None,
            room_shelf_on: false,
            daily_mood_on: true,
            keeper_speaks: true,
            display_clock: false,
            clock_format: "24h".into(),
            voicestudio_on: false,
            telegram_bridge_on: false,
            telegram_bridge_chat_id: None,
            discord_bridge_on: false,
            discord_bridge_channel_id: None,
            whatsapp_bridge_on: false,
            whatsapp_bridge_chat_id: None,
            whatsapp_bridge_me_name: None,
            whatsapp_bridge_me_digits: None,
            whatsapp_guests: Vec::new(),
            show_usage_card: true,
            living_wallpaper: true,
            wallpaper_sound: true,
            room_companion_key_id: None,
            room_companion_name: None,
            world: WorldSettings::default(),
            castle_screen: String::new(),
            window_layout: String::new(),
            window_bounds: None,
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
            dev_tab_numbers: Vec::new(),
            dev_projects: Vec::new(),
            project_databases: BTreeMap::new(),
            only_this_companion: false,
            diary_look: "list".into(),
            emotion_room_voice: true,
            emotion_market_voice: true,
            profile_look: "cards".into(),
            mic_shortcut: None,
            dev_root: None,
            tab_order: Vec::new(),
            pinned_rooms: default_pinned_rooms(),
            widgets_off: Vec::new(),
            widgets_on: Vec::new(),
            widget_sizes: std::collections::BTreeMap::new(),
            widget_places: std::collections::BTreeMap::new(),
            meter_watch: crate::meter_watch::MeterWatch::default(),
            gpu_gauges: crate::gpu::GpuGauges::default(),
            weather: crate::weather::WeatherSettings::default(),
            sweep: crate::sweep::SweepSettings::default(),
            sentinel: crate::sentinel::SentinelSettings::default(),
            credits: crate::credits::CreditsSaved::default(),
        }
    }
}

impl Settings {
    /// Trims every text field; an empty one means unset. The room address loses its trailing slash.
    pub fn normalized(mut self) -> Self {
        if !["12h", "24h"].contains(&self.clock_format.as_str()) { self.clock_format = "24h".into(); }
        if !["all", "named"].contains(&self.alert_when.as_str()) { self.alert_when = "all".into(); }
        // any id is kept (a member's own sound file is chosen by name); only an empty or absurd one resets
        self.alerts.retain(|id, _| !id.is_empty() && id.len() <= 40);
        while self.alerts.len() > 40 {
            let Some(last) = self.alerts.keys().next_back().cloned() else { break };
            self.alerts.remove(&last);
        }
        for rule in self.alerts.values_mut() {
            rule.sound_name = rule.sound_name.take().map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && n.len() <= 110);
        }
        self.alert_sound_name = self.alert_sound_name.trim().to_string();
        if self.alert_sound_name.is_empty() || self.alert_sound_name.len() > 110 { self.alert_sound_name = crate::roomalert::DEFAULT_SOUND.into(); }
        self.room_pictures_per_day = self.room_pictures_per_day.min(5);
        if !["low", "medium"].contains(&self.room_picture_quality.as_str()) { self.room_picture_quality = "low".into(); }
        self.room_voices_per_day = self.room_voices_per_day.min(5);
        // an ElevenLabs voice id is letters and digits; anything else is no choice (the default voice)
        self.room_voice_id = self.room_voice_id.take().map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && v.len() <= 64 && v.chars().all(|c| c.is_ascii_alphanumeric()));
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
        self.telegram_bridge_chat_id = clean(self.telegram_bridge_chat_id);
        self.discord_bridge_channel_id = clean(self.discord_bridge_channel_id);
        self.whatsapp_bridge_chat_id = clean(self.whatsapp_bridge_chat_id);
        self.whatsapp_bridge_me_name = clean(self.whatsapp_bridge_me_name);
        self.whatsapp_bridge_me_digits = clean(self.whatsapp_bridge_me_digits);
        self.whatsapp_guests.retain(|g| !g.chat_id.trim().is_empty());
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
        // the pinned rooms: only real rooms, each once, in the member's order; HOME and the launcher need no pin
        let mut pinned_seen = std::collections::BTreeSet::new();
        self.pinned_rooms = std::mem::take(&mut self.pinned_rooms)
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| ROOMS.contains(&id.as_str()) && !ALWAYS_SHOWN.contains(&id.as_str()))
            .filter(|id| pinned_seen.insert(id.clone()))
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
        self.mic_shortcut = self.mic_shortcut.take().filter(MicShortcut::known);
        // HOME's widgets: known ids only, USAGE's switch is show_usage_card; places kept on HOME
        let mut off: Vec<String> = std::mem::take(&mut self.widgets_off).into_iter().filter(|w| WIDGETS.contains(&w.as_str()) && w != "usage").collect();
        off.sort();
        off.dedup();
        self.widgets_off = off;
        let mut on: Vec<String> = std::mem::take(&mut self.widgets_on).into_iter().filter(|w| WIDGET_OPT_IN.contains(&w.as_str())).collect();
        on.sort();
        on.dedup();
        self.widgets_on = on;
        self.widget_sizes.retain(|w, size| WIDGETS.contains(&w.as_str()) && WIDGET_SIZES.contains(&size.as_str()) && size != "m");
        self.widget_places = std::mem::take(&mut self.widget_places)
            .into_iter()
            .filter(|(theme, _)| plain_id(theme))
            .take(WIDGET_THEMES_MAX)
            .map(|(theme, places)| {
                let kept = places
                    .into_iter()
                    .filter(|(w, p)| WIDGETS.contains(&w.as_str()) && p.left.is_finite() && p.top.is_finite())
                    .map(|(w, p)| (w, WidgetPlace { left: p.left.clamp(0.0, 100.0), top: p.top.clamp(0.0, 100.0) }))
                    .collect::<std::collections::BTreeMap<_, _>>();
                (theme, kept)
            })
            .filter(|(_, places)| !places.is_empty())
            .collect();
        // every known room is listed (a missing one is on, but a room that waits for the member stays
        // off); unknown names go
        self.rooms = ROOMS.iter().map(|r| (r.to_string(), self.rooms.get(*r).copied().unwrap_or(!ROOMS_OFF_AT_FIRST.contains(r)))).collect();
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

/// One DEV project's database logins, as the member filled them in (Local may stay empty: then the
/// project's own `.env` is read).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectDb {
    pub local: Option<DbLogin>,
    pub production: Option<DbLogin>,
}

/// Where one side's database is: never its password.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DbLogin {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// The copy of the CA certificate Castle keeps (`<settings folder>/certs/<id>.crt`), if any.
    pub ca: Option<String>,
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
    fn an_older_settings_file_has_no_project_databases() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(old.project_databases.is_empty());
        let one: Settings = serde_json::from_str(r#"{"projectDatabases":{"d:/shop":{"production":{"host":"db.example","port":25060,"database":"shop","user":"app"}}}}"#).unwrap();
        let shop = &one.project_databases["d:/shop"];
        assert_eq!(shop.local, None);
        assert_eq!(shop.production.as_ref().map(|p| (p.port, p.ca.clone())), Some((25060, None)));
    }

    #[test]
    fn an_older_settings_file_keeps_watch_at_ninety() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert_eq!(old.meter_watch, crate::meter_watch::MeterWatch { on: true, cpu: 90, memory: 90 });
        let off: Settings = serde_json::from_str(r#"{"meterWatch":{"on":false,"cpu":70}}"#).unwrap();
        assert_eq!((off.meter_watch.on, off.meter_watch.cpu, off.meter_watch.memory), (false, 70, 90));
    }

    #[test]
    fn the_emotion_engine_colours_the_room_by_default() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert!(old.emotion_room_voice && Settings::default().emotion_room_voice);
        let off: Settings = serde_json::from_str(r#"{"emotionRoomVoice":false}"#).unwrap();
        assert!(!off.emotion_room_voice);
        assert!(old.emotion_market_voice && Settings::default().emotion_market_voice);
        let desk_off: Settings = serde_json::from_str(r#"{"emotionMarketVoice":false}"#).unwrap();
        assert!(!desk_off.emotion_market_voice && desk_off.emotion_room_voice, "the desk's switch is its own");
        assert_eq!(old.profile_look, "cards");
        let odd: Settings = serde_json::from_str(r#"{"profileLook":"spiral"}"#).unwrap();
        assert_eq!(odd.normalized().profile_look, "cards");
    }

    #[test]
    fn the_mic_shortcut_keeps_only_a_letter_or_a_digit() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert_eq!(old.mic_shortcut, None, "an older file keeps ⌘⇧M");
        let k: Settings = serde_json::from_str(r#"{"micShortcut":{"code":"KeyK","shift":true}}"#).unwrap();
        assert_eq!(k.normalized().mic_shortcut, Some(MicShortcut { code: "KeyK".into(), shift: true, alt: false }));
        let seven: Settings = serde_json::from_str(r#"{"micShortcut":{"code":"Digit7","alt":true}}"#).unwrap();
        assert_eq!(seven.normalized().mic_shortcut.map(|m| m.code), Some("Digit7".into()));
        for odd in ["Escape", "Keyk", "KeyAB", "F5", "", "Digit10"] {
            let s: Settings = serde_json::from_str(&format!(r#"{{"micShortcut":{{"code":"{odd}"}}}}"#)).unwrap();
            assert_eq!(s.normalized().mic_shortcut, None, "{odd} is no choice");
        }
    }

    #[test]
    fn the_widgets_keep_known_ids_sizes_and_places_on_home() {
        let old: Settings = serde_json::from_str(r#"{"cli":"claude"}"#).unwrap();
        assert!(old.widgets_off.is_empty() && old.widget_sizes.is_empty() && old.widget_places.is_empty(), "a settings file from before WIDGETS shows every widget where its theme puts it");
        let odd: Settings = serde_json::from_str(
            r#"{"widgetsOff":["diary","usage","nonsense","diary"],
                "widgetSizes":{"clock":"l","gauges":"m","today":"xl","ghost":"s"},
                "widgetPlaces":{"constellation":{"clock":{"left":140,"top":-5},"ghost":{"left":1,"top":1}},"bad id!":{"clock":{"left":1,"top":1}},"seed":{"ghost":{"left":1,"top":1}}}}"#,
        )
        .unwrap();
        let s = odd.normalized();
        assert_eq!(s.widgets_off, ["diary"], "USAGE's switch is show_usage_card; unknown ids go");
        let credit: Settings = serde_json::from_str(r#"{"widgetsOff":["credit-suno"]}"#).unwrap();
        assert_eq!(credit.normalized().widgets_off, ["credit-suno"], "a credit widget switched off stays off");
        let later: Settings = serde_json::from_str(r#"{"widgetsOff":["sentinel","disks","sweep"],"widgetSizes":{"sentinel":"s"}}"#).unwrap();
        let later = later.normalized();
        assert_eq!(later.widgets_off, ["disks", "sentinel", "sweep"], "the cards added after WIDGETS was first written stay off too");
        assert_eq!(later.widget_sizes.get("sentinel").map(String::as_str), Some("s"));
        assert_eq!(s.widget_sizes.into_iter().collect::<Vec<_>>(), [("clock".to_string(), "l".to_string())]);
        assert_eq!(s.widget_places.keys().collect::<Vec<_>>(), ["constellation"]);
        assert_eq!(s.widget_places["constellation"]["clock"], WidgetPlace { left: 100.0, top: 0.0 });
        assert!(!s.widget_places["constellation"].contains_key("ghost"));
    }

    #[test]
    fn an_old_settings_file_gets_the_first_pins_and_an_empty_list_stays_empty() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true}"#).unwrap();
        assert_eq!(old.pinned_rooms, vec!["chat", "projects", "dev"], "a file written before pins existed");
        let none: Settings = serde_json::from_str(r#"{"pinnedRooms":[]}"#).unwrap();
        assert!(none.normalized().pinned_rooms.is_empty(), "the member may pin nothing");
        assert_eq!(Settings::default().pinned_rooms, vec!["chat", "projects", "dev"]);
    }

    #[test]
    fn pinned_rooms_keep_only_real_rooms_once_each_and_never_home_or_the_launcher() {
        let messy: Settings = serde_json::from_str(r#"{"pinnedRooms":[" dev ","home","launcher","widgets","dev","../x","","nothing","calendar","apps","chat"]}"#).unwrap();
        assert_eq!(messy.clone().normalized().pinned_rooms, vec!["dev", "calendar", "apps", "chat"]);
        // saved and read back unchanged
        let back: Settings = serde_json::from_str(&serde_json::to_string(&messy.normalized()).unwrap()).unwrap();
        assert_eq!(back.pinned_rooms, vec!["dev", "calendar", "apps", "chat"]);
    }

    #[test]
    fn the_first_pins_are_real_rooms() {
        assert!(DEFAULT_PINS.iter().all(|r| ROOMS.contains(r) && !ALWAYS_SHOWN.contains(r)));
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
    fn the_weather_widget_stays_off_until_it_is_switched_on() {
        let old: Settings = serde_json::from_str(r#"{"setupDone":true,"widgetsOff":["diary"]}"#).unwrap();
        assert!(old.normalized().widgets_on.is_empty(), "a settings file from before the weather widget has it off");
        let on: Settings = serde_json::from_str(r#"{"widgetsOn":["weather","weather","diary","nonsense"]}"#).unwrap();
        assert_eq!(on.normalized().widgets_on, ["weather"], "only opt-in widgets are kept, once");
        assert!(WIDGETS.contains(&"weather"), "its size and place are kept like every widget's");
    }

    #[test]
    fn the_one_arrangement_keeps_every_widgets_place_including_automations_and_calendar() {
        let s: Settings = serde_json::from_str(r#"{"widgetPlaces":{"all":{"automations":{"left":61.5,"top":30},"calendar":{"left":62,"top":55},"ghost":{"left":1,"top":1}}},"widgetsOff":["calendar"]}"#).unwrap();
        let s = s.normalized();
        assert_eq!(s.widget_places["all"]["automations"], WidgetPlace { left: 61.5, top: 30.0 }, "a restart must not forget where AUTOMATIONS was put");
        assert_eq!(s.widget_places["all"]["calendar"], WidgetPlace { left: 62.0, top: 55.0 });
        assert!(!s.widget_places["all"].contains_key("ghost"));
        assert_eq!(s.widgets_off, ["calendar"], "the off switch of the new cards is kept too");
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
    fn room_pictures_start_off_one_a_day_at_low_and_stay_in_bounds() {
        let old: Settings = serde_json::from_str::<Settings>(r#"{"setupDone":true}"#).unwrap().normalized();
        assert!(!old.room_pictures_on, "an older file never starts drawing by itself");
        assert_eq!((old.room_pictures_per_day, old.room_picture_quality.as_str()), (1, "low"));
        let wild: Settings = serde_json::from_str::<Settings>(r#"{"roomPicturesPerDay":40,"roomPictureQuality":"high"}"#).unwrap().normalized();
        assert_eq!((wild.room_pictures_per_day, wild.room_picture_quality.as_str()), (5, "low"), "never more than 5 a day, never the costly quality");
    }

    #[test]
    fn room_voices_start_off_two_a_day_with_the_default_voice_and_stay_in_bounds() {
        let old: Settings = serde_json::from_str::<Settings>(r#"{"setupDone":true}"#).unwrap().normalized();
        assert!(!old.room_voices_on, "an older file never starts speaking by itself");
        assert_eq!((old.room_voices_per_day, old.room_voice_id.as_deref()), (2, None));
        let wild: Settings = serde_json::from_str::<Settings>(r#"{"roomVoicesPerDay":40,"roomVoiceId":" abc/../x "}"#).unwrap().normalized();
        assert_eq!((wild.room_voices_per_day, wild.room_voice_id.as_deref()), (5, None), "never more than 5 a day; an id that is not letters and digits is no choice");
        let chosen: Settings = serde_json::from_str::<Settings>(r#"{"roomVoiceId":" 21m00Tcm4TlvDq8ikWAM "}"#).unwrap().normalized();
        assert_eq!(chosen.room_voice_id.as_deref(), Some("21m00Tcm4TlvDq8ikWAM"));
    }

    #[test]
    fn the_life_shelf_starts_off_and_an_older_file_reads_off() {
        assert!(!Settings::default().room_shelf_on);
        let old: Settings = serde_json::from_str::<Settings>(r#"{"setupDone":true}"#).unwrap().normalized();
        assert!(!old.room_shelf_on, "an older file never starts sharing by itself");
        let on: Settings = serde_json::from_str::<Settings>(r#"{"roomShelfOn":true}"#).unwrap();
        assert!(on.room_shelf_on);
    }

    #[test]
    fn a_new_mood_every_day_starts_on_and_an_older_file_reads_on() {
        assert!(Settings::default().daily_mood_on);
        assert!(Settings::default().keeper_speaks, "shopkeepers speak until the member turns them off");
        let old: Settings = serde_json::from_str::<Settings>(r#"{"setupDone":true}"#).unwrap().normalized();
        assert!(old.daily_mood_on);
        let off: Settings = serde_json::from_str::<Settings>(r#"{"dailyMoodOn":false}"#).unwrap();
        assert!(!off.daily_mood_on);
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
            automations_paused: false,
            automations_commands: Vec::new(),
            companion_name: Some("Lyra".into()),
            member_name: Some("Rin".into()),
            relationship_style: Some("Friendly partner".into()),
            theme: Some("constellation".into()),
            start_with_windows: true,
            start_session_on_launch: false,
            minimize_to_taskbar: true,
            alert_sound: true,
            alert_sound_name: "koto".into(),
            alert_notify: false,
            alert_when: "named".into(),
            alert_reactions: false,
            alerts: BTreeMap::new(),
            room_pictures_on: true,
            room_pictures_per_day: 3,
            room_picture_quality: "medium".into(),
            room_voices_on: true,
            room_voices_per_day: 4,
            room_voice_id: Some("21m00Tcm4TlvDq8ikWAM".into()),
            room_shelf_on: true,
            daily_mood_on: false,
            keeper_speaks: false,
            display_clock: true,
            clock_format: "12h".into(),
            voicestudio_on: true,
            telegram_bridge_on: true,
            telegram_bridge_chat_id: Some("123456789".into()),
            discord_bridge_on: true,
            discord_bridge_channel_id: Some("987654321".into()),
            whatsapp_bridge_on: true,
            whatsapp_bridge_chat_id: Some("6591234567@s.whatsapp.net".into()),
            whatsapp_bridge_me_name: Some("Rin".into()),
            whatsapp_bridge_me_digits: Some("6591234567".into()),
            whatsapp_guests: vec![WhatsAppGuest { chat_id: "60123456789@s.whatsapp.net".into(), name: "Aina".into(), digits: Some("60123456789".into()), approved_at: 1 }],
            show_usage_card: false,
            living_wallpaper: false,
            wallpaper_sound: false,
            room_companion_key_id: Some("17".into()),
            room_companion_name: Some("Lyra".into()),
            world: WorldSettings {
                music: false,
                volume: 0.25,
                folders: vec!["D:\\Pictures".into()],
                screens: vec![WorldScreen { screen: "\\\\.\\DISPLAY5".into(), world: "3d/crystal-isle".into(), camera: "orbit".into(), on: true, ..WorldScreen::default() }],
            },
            castle_screen: "\\\\.\\DISPLAY1".into(),
            window_layout: "left".into(),
            window_bounds: Some([10, 20, 1600, 900]),
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
            dev_tab_numbers: vec![1],
            dev_projects: vec!["D:/Projects/shop".into()],
            project_databases: BTreeMap::from([(
                "d:/projects/shop".to_string(),
                ProjectDb { local: None, production: Some(DbLogin { host: "db.example".into(), port: 25060, database: "shop".into(), user: "app".into(), ca: None }) },
            )]),
            only_this_companion: true,
            diary_look: "book".into(),
            emotion_room_voice: false,
            emotion_market_voice: false,
            profile_look: "constellation".into(),
            mic_shortcut: Some(MicShortcut { code: "KeyK".into(), shift: true, alt: false }),
            dev_root: Some("D:/Projects".into()),
            tab_order: vec!["dev".into(), "home".into(), "chat".into()],
            pinned_rooms: vec!["dev".into(), "calendar".into()],
            widgets_off: vec!["growth".into()],
            widgets_on: vec!["weather".into()],
            widget_sizes: [("clock".to_string(), "l".to_string())].into_iter().collect(),
            widget_places: [("seed".to_string(), [("today".to_string(), WidgetPlace { left: 12.5, top: 40.0 })].into_iter().collect())].into_iter().collect(),
            meter_watch: crate::meter_watch::MeterWatch { on: false, cpu: 75, memory: 85 },
            gpu_gauges: crate::gpu::GpuGauges { on: false },
            weather: crate::weather::WeatherSettings { place: Some(crate::weather::Place { name: "Ipoh".into(), region: "Perak, Malaysia".into(), lat: 4.6, lon: 101.08 }), units: "imperial".into() },
            sweep: crate::sweep::SweepSettings { look: Some("broom".into()), recycle_bin: true },
            sentinel: crate::sentinel::SentinelSettings { on: true, model: Some("opus".into()), patrol_minutes: 30 },
            credits: crate::credits::CreditsSaved { seedance_budget: Some(20.0), suno_peak: Some(800.0), ..Default::default() },
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
        assert!(settings.show_usage_card, "a settings file from before the card switch shows the card");
        assert!(settings.living_wallpaper, "a settings file from before living wallpapers starts with them on, whatever its old wallpaperMotion said");
        let old: Settings = serde_json::from_str(r#"{"wallpaperMotion":false}"#).unwrap();
        assert!(old.living_wallpaper, "the old switch's saved false never keeps the new one off");
        let off: Settings = serde_json::from_str(r#"{"livingWallpaper":false}"#).unwrap();
        assert!(!off.living_wallpaper, "a member's own off stays off");
        assert!(settings.wallpaper_sound, "a settings file from before wallpaper sounds starts with them on");
        let quiet: Settings = serde_json::from_str(r#"{"wallpaperSound":false}"#).unwrap();
        assert!(!quiet.wallpaper_sound, "a member's own silence stays");
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
    fn every_room_starts_on_but_the_library_home_cannot_be_off_and_unknown_rooms_go() {
        let fresh = Settings::default();
        assert!(ROOMS.iter().filter(|r| !ROOMS_OFF_AT_FIRST.contains(r)).all(|r| fresh.room_on(r)));
        assert!(!fresh.room_on("library"), "the Library waits for the member's switch");
        assert!(!fresh.room_on("market"), "the Market waits for the member's switch");
        let mut rooms = std::collections::BTreeMap::new();
        rooms.insert("story".to_string(), false);
        rooms.insert("home".to_string(), false);
        rooms.insert("spaceship".to_string(), true);
        let s = Settings { rooms, ..Settings::default() }.normalized();
        assert!(!s.room_on("story"));
        assert!(s.room_on("home"));
        assert!(s.room_on("world"), "a room missing from the file is on");
        assert!(!s.room_on("market"), "the Market stays off for a Castle that never chose it");
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
