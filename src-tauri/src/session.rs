//! Castle's two sessions: the companion's main one in the MemoryCore folder, and the STORY
//! room's in <memory>\adventures. Each is a PtyHost; the page talks to them by room name.

use crate::env;
use crate::key;
use crate::pty::{self, PtyHost};
use crate::settings::{Cli, Settings, SettingsStore};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

pub const TERMINAL_OUTPUT: &str = "terminal-output";
pub const TERMINAL_EXIT: &str = "terminal-exit";
pub const STORY_OUTPUT: &str = "story-output";
pub const STORY_EXIT: &str = "story-exit";
/// The STORY room's folder inside the memory folder (MemoryCore's Interactive Story uses it).
pub const STORY_FOLDER: &str = "adventures";

#[derive(Default)]
pub struct MainHost(pub PtyHost);
#[derive(Default)]
pub struct StoryHost(pub PtyHost);

/// Which session a command is for: the main one (the default) or the STORY room's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Room {
    Main,
    Story,
}

impl Room {
    pub fn named(room: Option<&str>) -> Room {
        if room == Some("story") {
            Room::Story
        } else {
            Room::Main
        }
    }

    fn events(self) -> (&'static str, &'static str) {
        match self {
            Room::Main => (TERMINAL_OUTPUT, TERMINAL_EXIT),
            Room::Story => (STORY_OUTPUT, STORY_EXIT),
        }
    }

    /// The folder the room's session starts in.
    pub fn folder(self, settings: &Settings) -> Option<PathBuf> {
        let memory = PathBuf::from(settings.memory_folder.as_ref()?);
        Some(match self {
            Room::Main => memory,
            Room::Story => memory.join(STORY_FOLDER),
        })
    }
}

/// Whether a room's session can start, and if not, why - in words for the screen.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub ready: bool,
    pub message: String,
    /// What the screen can offer to fix it: "setup", "settings" or "install".
    pub fix: Option<&'static str>,
}

pub fn status(settings: &Settings, room: Room, program: Option<&std::path::Path>) -> SessionStatus {
    let not_ready = |message: String, fix: &'static str| SessionStatus { ready: false, message, fix: Some(fix) };
    let Some(folder) = room.folder(settings) else {
        return not_ready("No MemoryCore folder is chosen yet.".into(), "setup");
    };
    if room == Room::Main && !folder.is_dir() {
        return not_ready("Your MemoryCore folder was not found. Choose it again in Settings.".into(), "settings");
    }
    if room == Room::Story && !folder.is_dir() {
        return not_ready("The story room is not ready yet. Open STORY once to prepare it.".into(), "setup");
    }
    if program.is_none() {
        let cli = settings.cli.label();
        return not_ready(format!("{cli} was not found on this computer. Install it, then try again."), "install");
    }
    let message = match room {
        Room::Main => format!("Ready to wake {}.", settings.companion_label()),
        Room::Story => "Ready to open the story.".to_string(),
    };
    SessionStatus { ready: true, message, fix: None }
}

fn host<'a>(room: Room, main: &'a MainHost, story: &'a StoryHost) -> &'a PtyHost {
    match room {
        Room::Main => &main.0,
        Room::Story => &story.0,
    }
}

/// Where Castle's helper scripts live (bundled resources).
pub fn tools_folder(app: &AppHandle) -> PathBuf {
    app.path().resource_dir().map(|dir| dir.join("tools")).unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    provider: Option<Cli>,
    running: bool,
    status: SessionStatus,
    snapshot: String,
    /// The resting screen offers "Start fresh" beside Wake (the main session, unless every start
    /// is fresh already).
    offers_fresh_start: bool,
    /// The session started on another model because its own weekly share is spent: one line for
    /// the member (the screen writes it in their words and time).
    notice: Option<ModelNotice>,
}

/// What the member is told when a session starts on the fallback.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelNotice {
    pub share: String,
    pub model: String,
    pub until_ms: i64,
}

impl From<crate::model_pick::Fallback> for ModelNotice {
    fn from(f: crate::model_pick::Fallback) -> Self {
        ModelNotice { share: f.share, model: f.model.to_string(), until_ms: f.until_ms }
    }
}

/// The fallback for a session about to start in `folder` with Claude Code, from the record it
/// picks up (or the folder's newest). Other CLIs have no weekly model shares.
pub fn fallback_for(settings: &Settings, folder: &Path, record: Option<&Path>) -> Option<crate::model_pick::Fallback> {
    match settings.cli {
        Cli::Claude => {
            if !settings.share_fallback {
                crate::model_pick::forget_main();
                return None;
            }
            let records = crate::transcript::claude::projects_root().map(|root| root.join(crate::transcript::claude::project_folder_name(&folder.display().to_string())));
            crate::model_pick::for_session(true, record, records.as_deref())
        }
        Cli::Codex | Cli::Gemini | Cli::Grok => None,
    }
}

/// The CLI's words for starting on the fallback.
pub fn fallback_args(fallback: Option<&crate::model_pick::Fallback>) -> Vec<String> {
    fallback.map(|f| vec!["--model".to_string(), f.model.to_string()]).unwrap_or_default()
}

/// Whether the resting screen offers a one-off fresh start: only the main session picks a talk
/// up, and a member whose every start is fresh needs no second button.
pub fn offers_fresh_start(room: Room, main_start: &str) -> bool {
    room == Room::Main && main_start != "fresh"
}

fn view(store: &SettingsStore, host: &PtyHost, room: Room) -> TerminalView {
    let mut settings = store.get();
    settings.cli = host.provider().unwrap_or(settings.cli);
    let program = env::find_on_path(settings.cli.command());
    TerminalView {
        provider: host.provider(),
        running: host.is_running(),
        status: status(&settings, room, program.as_deref()),
        snapshot: host.snapshot(),
        offers_fresh_start: offers_fresh_start(room, &settings.main_start),
        notice: host.fallback().map(ModelNotice::from),
    }
}

/// Where Castle keeps the memory it hands to the companion at wake: its own settings folder,
/// never the memory folder (so no backup carries it). Removed when the session ends.
pub fn boot_file(store: &SettingsStore) -> PathBuf {
    store.folder().join("boot").join("main.md")
}

pub fn forget_boot_file(store: &SettingsStore) {
    let _ = std::fs::remove_file(boot_file(store));
}

/// What Castle hands the CLI as it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Boot {
    /// the system prompt file
    pub file: PathBuf,
    /// the memory is in it (read from the database)
    pub memory: bool,
    /// the session is only this companion: the personal Claude Code setup is left out
    pub only_this: bool,
}

/// The CLI's own arguments at wake: Claude Code is given the boot file as a system prompt. The
/// others have no such option, so they get none and load the memory themselves. Keeping the
/// personal setup out is `isolation_args`, which does not depend on a boot file.
pub fn wake_args(cli: Cli, boot: Option<&Boot>) -> Vec<String> {
    let (Some(flag), Some(boot)) = (cli.boot_flag(), boot) else { return Vec::new() };
    vec![flag.to_string(), boot.file.display().to_string()]
}

/// Castle's own list of MCP servers for a session that is only this companion: empty, so no server
/// from the member's personal setup (`~/.claude.json`, read whatever the setting sources say) joins.
pub fn mcp_config_file(store: &SettingsStore) -> PathBuf {
    store.folder().join("mcp.json")
}

/// Writes the empty list once; None if it cannot be written (the session then is not started
/// half-isolated - see `isolation`).
pub fn ensure_mcp_config(store: &SettingsStore) -> Option<PathBuf> {
    let file = mcp_config_file(store);
    if !file.is_file() {
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, "{\"mcpServers\":{}}").ok()?;
    }
    Some(file)
}

/// What keeps the member's personal Claude Code setup out of a Castle session, in every room
/// (main, STORY, a DEV tab): only the project's and the folder's own settings are read (so the
/// personal settings, hooks, skills and `~/.claude/CLAUDE.md` are left out), and only Castle's own
/// MCP list. Codex has no such options.
#[cfg(test)]
pub fn isolation_args(cli: Cli, mcp_config: &Path) -> Vec<String> {
    isolation_args_with(cli, mcp_config, None)
}

/// The same, with Castle's own session settings (its status line) given by `--settings` when there
/// is a file for it.
pub fn isolation_args_with(cli: Cli, mcp_config: &Path, session_settings: Option<&Path>) -> Vec<String> {
    if !cli.isolates() {
        return Vec::new();
    }
    let mut args = vec!["--setting-sources".to_string(), "project,local".to_string()];
    if let Some(file) = session_settings {
        args.push("--settings".to_string());
        args.push(file.display().to_string());
    }
    // `--mcp-config` takes several files, so it must never be last: a wake greeting after it would be
    // read as one more config file and the CLI would refuse to start. `--strict-mcp-config` closes it.
    args.push("--mcp-config".to_string());
    args.push(mcp_config.display().to_string());
    args.push("--strict-mcp-config".to_string());
    args
}

/// Castle's status line script among its bundled tools.
pub const STATUS_LINE_SCRIPT: &str = "castle-statusline.mjs";

/// Castle's own settings for a session that is only this companion: kept in its settings folder,
/// never the memory folder.
pub fn session_settings_file(store: &SettingsStore) -> PathBuf {
    store.folder().join("session-settings.json")
}

/// The command Claude Code runs for the status line: Castle's script, which prints
/// `<model> | NN% | 123.4k tokens` so Chat can show how full the context is (the personal status
/// line is left out with the rest of the personal setup).
pub fn status_line_command(tools: &Path) -> String {
    format!("\"{}\" \"{}\"", crate::env::node().display(), tools.join(STATUS_LINE_SCRIPT).display())
}

/// The settings file's text: the status line, and the member's own voice setting when they have one.
pub fn session_settings_text(tools: &Path, voice: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut settings = voice.clone();
    settings.insert("statusLine".to_string(), serde_json::json!({ "type": "command", "command": status_line_command(tools) }));
    serde_json::Value::Object(settings).to_string()
}

/// The member's own Claude Code voice setting (`/voice`), from their personal settings file. A
/// session that is only this companion reads no personal settings (`--setting-sources
/// project,local`), and then holding Space in the Log or Chat's mic records nothing - tested live
/// (Nov 19 2026): `/voice` inside such a session says it turned on, yet Space only types spaces.
/// Castle's own `--settings` file is read whatever the setting sources say, so the voice travels
/// there: only these two keys, never the rest of the personal setup.
pub fn personal_voice() -> serde_json::Map<String, serde_json::Value> {
    let file = env::home_dir().map(|home| home.join(".claude").join("settings.json"));
    let text = file.and_then(|file| std::fs::read_to_string(file).ok()).unwrap_or_default();
    voice_of(&text)
}

/// `voiceEnabled` and `voice` from a Claude Code settings file's text (nothing when it has neither
/// or cannot be read).
pub fn voice_of(text: &str) -> serde_json::Map<String, serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    ["voiceEnabled", "voice"]
        .iter()
        .filter_map(|key| parsed.get(*key).map(|value| (key.to_string(), value.clone())))
        .collect()
}

/// What Chat's mic needs to know: whether Claude Code's voice is on, and its mode.
#[derive(Debug, Serialize, PartialEq)]
pub struct VoiceView {
    pub enabled: bool,
    /// "hold" (Space held records) or "tap" (one tap starts, another sends)
    pub mode: String,
}

/// `voice.enabled` wins over the older `voiceEnabled`; hold is Claude Code's own default mode.
pub fn voice_view(voice: &serde_json::Map<String, serde_json::Value>) -> VoiceView {
    let nested = voice.get("voice");
    let enabled = nested
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| voice.get("voiceEnabled").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let mode = nested.and_then(|v| v.get("mode")).and_then(|v| v.as_str()).unwrap_or("hold").to_string();
    VoiceView { enabled, mode }
}

/// Chat's mic button shows only while the member's Claude Code voice is on (read again often, so
/// `/voice` in the Log shows or hides it).
#[tauri::command]
pub fn claude_voice() -> VoiceView {
    voice_view(&personal_voice())
}

/// Writes Castle's session settings (again each wake, so a moved install points at its own tools).
/// None when the script is not there or the file cannot be written: the session then simply has no
/// status line, which never stops it from starting.
pub fn ensure_session_settings(store: &SettingsStore, tools: &Path) -> Option<PathBuf> {
    if !tools.join(STATUS_LINE_SCRIPT).is_file() {
        return None;
    }
    let file = session_settings_file(store);
    std::fs::create_dir_all(file.parent()?).ok()?;
    std::fs::write(&file, session_settings_text(tools, &personal_voice())).ok()?;
    Some(file)
}

/// The variables that go with it, whatever the room: no CLAUDE.md file is read and no auto memory.
/// Tested live (Nov 14 2026): `--setting-sources project,local` still lets the personal
/// `~/.claude/CLAUDE.md` in, and that file can make the session someone else - so every CLAUDE.md
/// is turned off, and the room's own instructions (Castle's block, the story rules, the project's
/// CLAUDE.md) travel in the boot file instead (`folder_instructions`).
pub fn isolation_vars(cli: Cli) -> Vec<(String, String)> {
    if !cli.isolates() {
        return Vec::new();
    }
    vec![
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(), "1".to_string()),
        ("CLAUDE_CODE_DISABLE_CLAUDE_MDS".to_string(), "1".to_string()),
    ]
}

/// A folder's own instructions - its `CLAUDE.md`, `.claude/CLAUDE.md` and `CLAUDE.local.md` - for a
/// boot file, since an isolated session reads no CLAUDE.md by itself. Only the folder itself, never
/// the folders above it (where someone's personal files may live). None when it has none.
pub fn folder_instructions(folder: &Path) -> Option<String> {
    let parts: Vec<String> = ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"]
        .iter()
        .filter_map(|name| std::fs::read_to_string(folder.join(name)).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    (!parts.is_empty()).then(|| format!("# This folder's own instructions (its CLAUDE.md)\n\n{}", parts.join("\n\n")))
}

/// The boot file of a room that has no other boot: its folder's own instructions, so an isolated
/// session still hears them (the main room without names, the STORY room's rules).
fn room_instructions_boot(store: &SettingsStore, room: Room, folder: &Path) -> Option<Boot> {
    let text = folder_instructions(folder)?;
    let name = if room == Room::Story { "story.md" } else { "main.md" };
    let file = store.folder().join("boot").join(name);
    std::fs::create_dir_all(file.parent()?).ok()?;
    std::fs::write(&file, text).ok()?;
    Some(Boot { file, memory: false, only_this: true })
}

/// Both together for a session, or an error when the switch is on and the MCP list cannot be
/// written: a session that should be only this companion is never started half-isolated.
#[cfg(test)]
pub fn isolation(store: &SettingsStore, cli: Cli, on: bool) -> Result<(Vec<String>, Vec<(String, String)>), String> {
    isolation_inner(store, cli, on, None)
}

/// `isolation`, plus Castle's status line (from the script in `tools`), so the session's context
/// shows in Chat.
pub fn isolation_with_status(store: &SettingsStore, cli: Cli, on: bool, tools: &Path) -> Result<(Vec<String>, Vec<(String, String)>), String> {
    isolation_inner(store, cli, on, Some(tools))
}

fn isolation_inner(store: &SettingsStore, cli: Cli, on: bool, tools: Option<&Path>) -> Result<(Vec<String>, Vec<(String, String)>), String> {
    if !on || !cli.isolates() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mcp = ensure_mcp_config(store).ok_or("Castle could not write its settings folder, so the session was not started.")?;
    let settings = tools.and_then(|tools| ensure_session_settings(store, tools));
    Ok((isolation_args_with(cli, &mcp, settings.as_deref()), isolation_vars(cli)))
}

/// The variables a wake adds for the boot: a session that is only this companion reads no
/// CLAUDE.md at all (the personal one would make it someone else), so Castle's own block
/// travels in the boot file instead.
pub fn wake_vars(boot: Option<&Boot>) -> Vec<(String, String)> {
    match boot {
        Some(boot) if boot.only_this => vec![("CLAUDE_CODE_DISABLE_CLAUDE_MDS".to_string(), "1".to_string())],
        _ => Vec::new(),
    }
}

/// Writes the boot file when this wake carries one: the main room and Claude Code, with the
/// memory read from the database (the switch on, the database answering and holding it) and,
/// for a session that is only this companion, Castle's own block in front. Any miss is None -
/// the companion then loads everything herself, exactly as before.
fn prepare_boot(store: &SettingsStore, settings: &Settings, room: Room, folder: &Path) -> Option<Boot> {
    forget_boot_file(store);
    if room != Room::Main || settings.cli.boot_flag().is_none() {
        return None;
    }
    let memory = if settings.db_boot_at_wake {
        crate::store::source(folder).and_then(|mut conn| crate::mirror::boot_prompt(&mut conn).ok().flatten())
    } else {
        None
    };
    let block = if settings.only_this_companion {
        let (companion, member) = (settings.companion_name.as_deref()?, settings.member_name.as_deref()?);
        Some(crate::setup::block_text(folder, companion, member, memory.is_some()).ok()?)
    } else {
        None
    };
    let text = match (&block, &memory) {
        (None, None) => return None,
        (Some(block), Some(memory)) => format!("{block}\n\n{memory}"),
        (Some(block), None) => block.clone(),
        (None, Some(memory)) => memory.clone(),
    };
    let file = boot_file(store);
    std::fs::create_dir_all(file.parent()?).ok()?;
    std::fs::write(&file, text).ok()?;
    Some(Boot { file, memory: memory.is_some(), only_this: block.is_some() })
}

/// Starts the room's session unless it is already running. The screen hears it through that
/// room's output and exit events.
pub(crate) fn wake(app: &AppHandle, store: &SettingsStore, host: &PtyHost, room: Room, cols: u16, rows: u16) -> Result<(), String> {
    wake_with_prompt(app, store, host, room, cols, rows, None)
}

pub(crate) fn wake_with_prompt(app: &AppHandle, store: &SettingsStore, host: &PtyHost, room: Room, cols: u16, rows: u16, prompt: Option<&str>) -> Result<(), String> {
    wake_as(app, store, host, room, cols, rows, prompt, false)
}

/// `fresh`: the member pressed "Start fresh" - the main session starts a new talk whatever the
/// start setting says.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wake_as(app: &AppHandle, store: &SettingsStore, host: &PtyHost, room: Room, cols: u16, rows: u16, prompt: Option<&str>, fresh: bool) -> Result<(), String> {
    let settings = store.get();
    let program = env::find_on_path(settings.cli.command());
    let ready = status(&settings, room, program.as_deref());
    let (Some(folder), Some(program), true) = (room.folder(&settings), program, ready.ready) else {
        return Err(ready.message);
    };
    let mut vars = env::session_env(settings.cli, &|name| key::load(name), &tools_folder(app), env::NodePlace::First);
    // the drawing styles (standard appearance, turnarounds, try-on) ship beside the tools
    if let Ok(dir) = app.path().resource_dir() {
        vars.push(("CASTLE_STYLES".into(), dir.join("styles").display().to_string()));
    }
    // Castle's block in CLAUDE.md / AGENTS.md follows the Castle version (the Neural Link's lines
    // reach a member who set up before them); the member's own words outside it are untouched
    // the memory, read from the database now and handed to the CLI as it starts
    let mut boot = prepare_boot(store, &settings, room, &folder);
    if boot.is_none() && settings.only_this_companion && settings.cli.isolates() {
        boot = room_instructions_boot(store, room, &folder);
    }
    vars.extend(wake_vars(boot.as_ref()));
    // the personal setup stays out whenever the switch is on - in the STORY room too, and even when
    // the names are missing and no boot file was written
    let (isolation_args, isolation_vars) = isolation_with_status(store, settings.cli, settings.only_this_companion, &tools_folder(app))?;
    vars.extend(isolation_vars);
    if room == Room::Main {
        if let (Some(companion), Some(member)) = (settings.companion_name.as_deref(), settings.member_name.as_deref()) {
            let _ = crate::setup::write_blocks_for(&folder, companion, member, boot.as_ref().is_some_and(|b| b.memory));
        }
    }
    let (output_event, exit_event) = room.events();
    let output = app.clone();
    let ending = app.clone();
    // The main session picks its own earlier conversation up again (never a provider switch's
    // handoff, which is a fresh session by design); the others always start fresh.
    let fresh_handoff = prompt.is_some_and(|p| p.starts_with(crate::providers::HANDOFF_PROMPT));
    let plan = (room == Room::Main).then(|| crate::resume::plan(store, settings.cli, &folder, !(fresh_handoff || fresh), &settings.main_start));
    let resumed = plan.as_ref().is_some_and(|p| p.resumed);
    // a model whose weekly share is spent starts on the fallback (Claude Code only)
    let fallback = fallback_for(&settings, &folder, plan.as_ref().and_then(|p| p.record.as_deref()));
    let plan_args: Vec<String> = plan.iter().flat_map(|p| p.args.clone()).collect();
    let mut args = Vec::new();
    match settings.cli {
        // Codex's `resume <id>` is a subcommand, so it comes before any option
        Cli::Codex => {
            args.extend(plan_args);
            args.extend(wake_args(settings.cli, boot.as_ref()));
        }
        Cli::Claude | Cli::Gemini | Cli::Grok => {
            args.extend(wake_args(settings.cli, boot.as_ref()));
            args.extend(plan_args);
        }
    }
    args.extend(fallback_args(fallback.as_ref()));
    args.extend(isolation_args);
    // a conversation picked up again is not greeted a second time
    let prompt = prompt.filter(|p| !(resumed && *p == WAKE_PROMPT));
    if let Some(prompt) = prompt { args.push(prompt.to_owned()); }
    let started = host.start(
        pty::launch_command(&program, &folder, &vars, &args),
        cols.max(20),
        rows.max(5),
        move |text| {
            let _ = output.emit(output_event, text);
        },
        move |id, code| {
            match room {
                Room::Main => {
                    forget_boot_file(&ending.state::<SettingsStore>());
                    ending.state::<MainHost>().0.finished(id)
                }
                Room::Story => ending.state::<StoryHost>().0.finished(id),
            }
            let _ = ending.emit(exit_event, code);
        },
    )?;
    if started {
        host.bind_provider(settings.cli);
        host.set_fallback(fallback);
    }
    // a CLI that names its own sessions is remembered once its record appears
    if started && room == Room::Main && !resumed {
        match settings.cli {
            Cli::Codex => crate::resume::remember_codex_when_it_appears(app, folder.clone()),
            Cli::Claude | Cli::Grok | Cli::Gemini => {}
        }
    }
    // A handoff prompt is a turn the new CLI is now working on: a later switch waits for it.
    if started && prompt.is_some() { host.mark_input(); }
    Ok(())
}

pub fn any_running(app: &AppHandle) -> bool {
    app.state::<MainHost>().0.is_running() || app.state::<StoryHost>().0.is_running()
}

pub fn end_all(app: &AppHandle) {
    app.state::<MainHost>().0.kill();
    app.state::<StoryHost>().0.kill();
}

#[tauri::command]
pub fn terminal_state(room: Option<String>, store: State<SettingsStore>, main: State<MainHost>, story: State<StoryHost>) -> TerminalView {
    let room = Room::named(room.as_deref());
    view(&store, host(room, &main, &story), room)
}

/// The opening line Castle gives a session when the member presses Wake: neither CLI does anything
/// until its first message, so without it a fresh session sits blank instead of greeting. The
/// companion's own instructions (CLAUDE.md / AGENTS.md) say what to do on a first turn; this only
/// starts one. It lands in the conversation record like something the member said, so the provider
/// handoff leaves it out.
pub const WAKE_PROMPT: &str = "Hi, I just woke you. Please greet me.";

/// What to start the CLI with: only the Wake button asks for the greeting, and only for the main
/// session - not a wake Castle does itself at launch, and not the STORY room.
pub fn opening_prompt(room: Room, greet: Option<bool>) -> Option<&'static str> {
    (room == Room::Main && greet == Some(true)).then_some(WAKE_PROMPT)
}

#[tauri::command]
pub fn terminal_start(
    app: AppHandle,
    cols: u16,
    rows: u16,
    room: Option<String>,
    greet: Option<bool>,
    fresh: Option<bool>,
    store: State<SettingsStore>,
    main: State<MainHost>,
    story: State<StoryHost>,
) -> Result<TerminalView, String> {
    let room = Room::named(room.as_deref());
    if room == Room::Story {
        crate::rooms::need(&store, "story")?;
    }
    let host = host(room, &main, &story);
    wake_as(&app, &store, host, room, cols, rows, opening_prompt(room, greet), fresh == Some(true))?;
    Ok(view(&store, host, room))
}

/// The terminal talking to the console on its own, not the member typing: answers to the
/// console's questions and the focus reports the CLI asked for. Windows' console waits for the
/// cursor answer before it starts the CLI, so these are never held back, and they never count as
/// the member's last input - otherwise a fresh session would look typed into (Codex asks for the
/// screen's colours the moment it starts) and a provider switch would wait for a turn that never
/// began. What it knows, with the bytes Castle's screen really sends back:
///   cursor position `ESC [ 1;1 R`, device attributes `ESC [ ? 1;2 c` and `ESC [ > 0;276;0 c`,
///   status `ESC [ 0 n`, keyboard mode `ESC [ ? 0 u`, mode reports `ESC [ ? 2026;2 $ y`,
///   colours `ESC ] 10;rgb:ee/f3/ff ESC \`, and focus `ESC [ I` / `ESC [ O`.
/// A key press is never one of these (arrows end in A-D, F-keys in ~, a kitty key in u without `?`).
pub fn is_terminal_reply(data: &str) -> bool {
    if data.len() > 128 {
        return false;
    }
    if data == "\x1b[I" || data == "\x1b[O" {
        return true;
    }
    // OSC (colours) and DCS (version) replies: ESC ] or ESC P, then text, then ST (ESC \) or BEL
    if let Some(rest) = data.strip_prefix("\x1b]").or_else(|| data.strip_prefix("\x1bP")) {
        let text = rest.strip_suffix("\x1b\\").or_else(|| rest.strip_suffix('\x07'));
        return text.is_some_and(|t| !t.contains('\x1b') && t.starts_with(|c: char| c.is_ascii_digit() || c == '>'));
    }
    let Some(body) = data.strip_prefix("\x1b[") else { return false };
    let Some(last) = body.chars().last().filter(char::is_ascii) else { return false };
    let params = &body[..body.len() - 1];
    if !params.chars().all(|c| c.is_ascii_digit() || matches!(c, ';' | '?' | '>' | '$')) {
        return false;
    }
    match last {
        'R' | 'c' | 'n' => !params.contains('$'),
        'u' => params.starts_with('?') && !params.contains('$'),
        'y' => params.starts_with('?') && params.ends_with('$'),
        _ => false,
    }
}

#[tauri::command]
pub fn terminal_write(data: String, room: Option<String>, main: State<MainHost>, story: State<StoryHost>, images: State<crate::providers::Images>, control: State<crate::providers::Control>) -> Result<(), String> {
    let room = Room::named(room.as_deref());
    if is_terminal_reply(&data) {
        return host(room, &main, &story).reply(data.as_bytes());
    }
    // A provider switch and a drawing only concern the main session: the STORY room never waits on them.
    if room == Room::Main {
        let switching = control.0.try_lock().map_err(|_| "Provider is changing. Your message has not been sent.")?;
        if switching.phase == "switching" {
            return Err("Provider is switching. Your message has not been sent.".into());
        }
        drop(switching);
        let drawing = images.0.lock().unwrap().iter().any(|j| j.holds_session());
        if drawing && holds_back_while_drawing(&data) {
            return Err("A picture is being drawn in this memory. Your message has not been sent; send it when the drawing finishes, or press Stop on the drawing.".into());
        }
    }
    host(room, &main, &story).write(data.as_bytes())
}

/// While Codex draws in the memory folder, only a message being sent (Enter) waits: typing, Ctrl+C,
/// Esc and the arrows still reach the session, so it is never locked.
fn holds_back_while_drawing(data: &str) -> bool {
    data.contains('\r')
}

#[tauri::command]
pub fn terminal_resize(cols: u16, rows: u16, room: Option<String>, main: State<MainHost>, story: State<StoryHost>) -> Result<(), String> {
    host(Room::named(room.as_deref()), &main, &story).resize(cols.max(20), rows.max(5))
}

/// Ends the STORY room's session ("Leave the story"); the main one is never ended this way.
#[tauri::command]
pub fn story_close(story: State<StoryHost>) {
    story.0.kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("castle-session-{name}-{}", std::process::id()));
        // an old run with the same process id may have left it behind
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn status_names_what_is_missing_and_how_to_fix_it() {
        let program = Path::new("C:\\bin\\claude.exe");
        let unset = Settings::default();
        assert_eq!(status(&unset, Room::Main, Some(program)).fix, Some("setup"));

        let gone = Settings { memory_folder: Some(folder("x").join("nope").display().to_string()), ..Settings::default() };
        assert_eq!(status(&gone, Room::Main, Some(program)).fix, Some("settings"));

        let memory = folder("ok");
        let set = Settings { memory_folder: Some(memory.display().to_string()), companion_name: Some("Lyra".into()), ..Settings::default() };
        let missing_cli = status(&set, Room::Main, None);
        assert_eq!((missing_cli.ready, missing_cli.fix), (false, Some("install")));
        assert!(missing_cli.message.contains("Claude Code"));

        let ready = status(&set, Room::Main, Some(program));
        assert!(ready.ready);
        assert_eq!(ready.message, "Ready to wake Lyra.");

        // the story room needs its own folder
        assert!(!status(&set, Room::Story, Some(program)).ready);
        std::fs::create_dir_all(memory.join(STORY_FOLDER)).unwrap();
        assert!(status(&set, Room::Story, Some(program)).ready);
    }

    #[test]
    fn rooms_are_named_by_the_page() {
        assert_eq!(Room::named(Some("story")), Room::Story);
        assert_eq!(Room::named(None), Room::Main);
        assert_eq!(Room::named(Some("anything")), Room::Main);
    }

    #[test]
    fn claude_code_is_given_the_boot_file_and_codex_loads_it_itself() {
        let boot = Boot { file: PathBuf::from("C:/castle/boot/main.md"), memory: true, only_this: false };
        assert_eq!(wake_args(Cli::Claude, Some(&boot)), vec!["--append-system-prompt-file".to_string(), boot.file.display().to_string()]);
        assert!(wake_args(Cli::Codex, Some(&boot)).is_empty());
        assert!(wake_args(Cli::Claude, None).is_empty());
        assert!(wake_vars(Some(&boot)).is_empty() && wake_vars(None).is_empty());
        // Castle's block in the boot file: no CLAUDE.md is read (the block travels in the boot file)
        let only = Boot { only_this: true, ..boot.clone() };
        assert_eq!(wake_args(Cli::Claude, Some(&only)).len(), 2);
        assert_eq!(wake_vars(Some(&only)), vec![("CLAUDE_CODE_DISABLE_CLAUDE_MDS".to_string(), "1".to_string())]);
        assert!(wake_args(Cli::Codex, Some(&only)).is_empty());
    }

    #[test]
    fn a_spent_share_adds_the_fallback_model_and_only_for_claude_code() {
        let fallback = crate::model_pick::Fallback { model: "opus", share: "Fable".into(), until_ms: 1 };
        assert_eq!(fallback_args(Some(&fallback)), vec!["--model", "opus"]);
        assert!(fallback_args(None).is_empty());
        let folder = std::env::temp_dir();
        for cli in [Cli::Codex, Cli::Gemini, Cli::Grok] {
            assert_eq!(fallback_for(&Settings { cli, ..Settings::default() }, &folder, None), None);
        }
        assert_eq!(fallback_for(&Settings { share_fallback: false, ..Settings::default() }, &folder, None), None, "the switch off");
        let notice = ModelNotice::from(fallback);
        assert_eq!((notice.share.as_str(), notice.model.as_str(), notice.until_ms), ("Fable", "opus", 1));
    }

    #[test]
    fn a_folders_own_instructions_travel_in_the_boot_file_and_never_the_folders_above() {
        let root = folder("instructions");
        let project = root.join("shop");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "someone above").unwrap();
        assert_eq!(folder_instructions(&project), None);
        std::fs::write(project.join("CLAUDE.md"), "Build with Vite.").unwrap();
        std::fs::write(project.join(".claude").join("CLAUDE.md"), "Tests with vitest.").unwrap();
        let text = folder_instructions(&project).unwrap();
        assert!(text.contains("Build with Vite.") && text.contains("Tests with vitest."));
        assert!(!text.contains("someone above"));
    }

    #[test]
    fn only_this_companion_keeps_the_personal_setup_out_with_or_without_a_boot_file() {
        let mcp = PathBuf::from("C:/castle/mcp.json");
        let args = isolation_args(Cli::Claude, &mcp);
        assert_eq!(args[..2], ["--setting-sources".to_string(), "project,local".to_string()]);
        // the file follows --mcp-config, and a flag (not the file) comes last, so a prompt after the
        // isolation can never be taken for a second config file
        let at = args.iter().position(|a| a == "--mcp-config").unwrap();
        assert_eq!(args[at + 1], mcp.display().to_string());
        assert_eq!(args.last(), Some(&"--strict-mcp-config".to_string()));
        // every CLAUDE.md is off (the personal one gets past --setting-sources), and the auto memory
        let vars = isolation_vars(Cli::Claude);
        assert!(vars.contains(&("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(), "1".to_string())));
        assert!(vars.contains(&("CLAUDE_CODE_DISABLE_CLAUDE_MDS".to_string(), "1".to_string())));
        assert!(isolation_args(Cli::Codex, &mcp).is_empty() && isolation_vars(Cli::Codex).is_empty());

        // the switch on, whatever the names: isolated, and Castle's empty MCP list is written outside the memory
        let settings_folder = folder("isolation");
        let store = SettingsStore::open(&settings_folder);
        let (on_args, on_vars) = isolation(&store, Cli::Claude, true).unwrap();
        assert!(on_args.contains(&"--strict-mcp-config".to_string()) && !on_vars.is_empty());
        assert_eq!(std::fs::read_to_string(mcp_config_file(&store)).unwrap(), "{\"mcpServers\":{}}");
        let (off_args, off_vars) = isolation(&store, Cli::Claude, false).unwrap();
        assert!(off_args.is_empty() && off_vars.is_empty());
    }

    #[test]
    fn castles_status_line_travels_in_its_own_settings_file_before_the_mcp_list() {
        let tools = folder("status-tools");
        let store = SettingsStore::open(&folder("status-config"));
        // no script among the tools: no settings file and no --settings, the session still isolated
        let (args, _) = isolation_with_status(&store, Cli::Claude, true, &tools).unwrap();
        assert!(!args.contains(&"--settings".to_string()));
        assert_eq!(args.last(), Some(&"--strict-mcp-config".to_string()));

        std::fs::write(tools.join(STATUS_LINE_SCRIPT), "# script").unwrap();
        let (args, vars) = isolation_with_status(&store, Cli::Claude, true, &tools).unwrap();
        let file = session_settings_file(&store);
        assert!(file.starts_with(store.folder()) && file.ends_with("session-settings.json"));
        let at = args.iter().position(|a| a == "--settings").unwrap();
        assert_eq!(args[at + 1], file.display().to_string());
        // after the setting sources, before the MCP list, and a flag still comes last
        assert!(at > args.iter().position(|a| a == "--setting-sources").unwrap());
        assert!(at < args.iter().position(|a| a == "--mcp-config").unwrap());
        assert_eq!(args.last(), Some(&"--strict-mcp-config".to_string()));
        assert!(!vars.is_empty());

        // the file holds only the status line, pointing at the script among the tools
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let script = tools.join(STATUS_LINE_SCRIPT).display().to_string();
        assert_eq!(written["statusLine"]["type"], "command");
        assert_eq!(written["statusLine"]["command"], format!("\"{}\" \"{script}\"", crate::env::node().display()));
        // beside it only the member's own voice setting, if they have one on this computer
        assert!(written.as_object().unwrap().keys().all(|k| ["statusLine", "voiceEnabled", "voice"].contains(&k.as_str())));

        // the plain isolation (and the switch off, or Codex) has no status line of Castle's
        let (plain, _) = isolation(&store, Cli::Claude, true).unwrap();
        assert!(!plain.contains(&"--settings".to_string()));
        assert!(isolation_with_status(&store, Cli::Claude, false, &tools).unwrap().0.is_empty());
        assert!(isolation_with_status(&store, Cli::Codex, true, &tools).unwrap().0.is_empty());
    }

    #[test]
    fn only_the_members_voice_setting_travels_into_castles_session_settings() {
        let personal = r#"{"voiceEnabled":true,"voice":{"enabled":true,"mode":"hold"},"hooks":{"Stop":[]},"statusLine":{"type":"command","command":"mine"}}"#;
        let voice = voice_of(personal);
        assert_eq!(voice.len(), 2);
        assert!(voice.contains_key("voiceEnabled") && voice.contains_key("voice"));
        // none at all, or a file that cannot be read: nothing travels
        assert!(voice_of("{}").is_empty() && voice_of("not json").is_empty() && voice_of("").is_empty());

        let tools = Path::new("C:/castle/tools");
        let written: serde_json::Value = serde_json::from_str(&session_settings_text(tools, &voice)).unwrap();
        assert_eq!(written["voice"]["mode"], "hold");
        assert_eq!(written["voiceEnabled"], true);
        // Castle's status line, never the member's own
        assert_eq!(written["statusLine"]["command"], status_line_command(tools));
        assert!(written.get("hooks").is_none());
        let plain: serde_json::Value = serde_json::from_str(&session_settings_text(tools, &voice_of(""))).unwrap();
        assert_eq!(plain.as_object().unwrap().len(), 1);
    }

    #[test]
    fn chats_mic_reads_whether_voice_is_on_and_its_mode() {
        let on = |text: &str| voice_view(&voice_of(text));
        assert_eq!(on(r#"{"voiceEnabled":true,"voice":{"enabled":true,"mode":"hold"}}"#), VoiceView { enabled: true, mode: "hold".into() });
        assert_eq!(on(r#"{"voice":{"enabled":true,"mode":"tap"}}"#), VoiceView { enabled: true, mode: "tap".into() });
        // the older single switch, and Claude Code's default mode
        assert_eq!(on(r#"{"voiceEnabled":true}"#), VoiceView { enabled: true, mode: "hold".into() });
        // the nested switch wins when both are written
        assert!(!on(r#"{"voiceEnabled":true,"voice":{"enabled":false}}"#).enabled);
        assert!(!on("{}").enabled && !on("").enabled);
    }

    /// Runs the real script with a session's JSON on stdin (through Node, as Claude Code does).
    fn status_line(json: &serde_json::Value) -> Option<String> {
        use std::io::Write;
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("tools").join(STATUS_LINE_SCRIPT);
        let mut child = std::process::Command::new(crate::env::node())
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(json.to_string().as_bytes()).ok()?;
        let output = child.wait_with_output().ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[test]
    fn the_status_line_says_the_model_the_percent_and_the_tokens() {
        let place = folder("status-run");
        let transcript = place.join("t.jsonl");
        let lines = [
            r#"{"type":"user","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_creation_input_tokens":2000,"cache_read_input_tokens":40000,"output_tokens":50}}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":999999}}}"#,
            r#"{"type":"user","message":{"content":"next"}}"#,
        ];
        std::fs::write(&transcript, lines.join("\n")).unwrap();
        let path = transcript.display().to_string();
        // no PowerShell (not Windows): nothing to check
        let Some(line) = status_line(&serde_json::json!({ "model": { "id": "claude-x", "display_name": "Opus" }, "transcript_path": path })) else { return };
        // the newest main-thread answer: 42,010 of 200k (a side agent's answer is its own context)
        assert_eq!(line, "Opus | 21% | 42k tokens");
        let wide = status_line(&serde_json::json!({ "model": { "id": "claude-x[1m]", "display_name": "Opus" }, "transcript_path": path })).unwrap();
        assert_eq!(wide, "Opus | 4% | 42k tokens");
        // Claude Code's own numbers win when it sends them
        let told = status_line(&serde_json::json!({ "model": { "display_name": "Sonnet" }, "context_window": { "used_percentage": 42.4, "context_window_size": 200000 } })).unwrap();
        assert_eq!(told, "Sonnet | 42% | 84.8k tokens");
        assert_eq!(status_line(&serde_json::json!({ "model": { "display_name": "Sonnet" } })).unwrap(), "Sonnet | 0% | 0 tokens");
    }

    #[test]
    fn a_session_that_is_only_this_companion_carries_castles_block_in_the_boot_file() {
        let config = folder("only-config");
        let memory = folder("only-memory");
        std::fs::write(memory.join("master-memory.md"), "**I am Lyra**\n").unwrap();
        let store = SettingsStore::open(&config);
        let mut settings = Settings {
            memory_folder: Some(memory.display().to_string()),
            companion_name: Some("Lyra".into()),
            member_name: Some("Rin".into()),
            ..Settings::default()
        };
        // off (the default): a memory kept in files hands nothing over
        assert!(!settings.only_this_companion);
        assert_eq!(prepare_boot(&store, &settings, Room::Main, &memory), None);
        settings.only_this_companion = true;
        let boot = prepare_boot(&store, &settings, Room::Main, &memory).unwrap();
        assert_eq!((boot.memory, boot.only_this), (false, true));
        let text = std::fs::read_to_string(&boot.file).unwrap();
        assert!(text.contains("You are **Lyra**, the MemoryCore companion of **Rin**") && text.contains("read `master-memory.md`"));
        assert!(text.contains("In this folder you are Lyra and only Lyra"));
        // never for the STORY room or Codex, and never without both names
        assert_eq!(prepare_boot(&store, &settings, Room::Story, &memory), None);
        assert_eq!(prepare_boot(&store, &Settings { cli: Cli::Codex, ..settings.clone() }, Room::Main, &memory), None);
        assert_eq!(prepare_boot(&store, &Settings { member_name: None, ..settings.clone() }, Room::Main, &memory), None);
    }

    #[test]
    fn the_boot_file_lives_with_castles_settings_never_in_the_memory_folder() {
        let config = folder("boot-config");
        let memory = folder("boot-memory");
        let store = SettingsStore::open(&config);
        let file = boot_file(&store);
        assert!(file.starts_with(&config) && !file.starts_with(&memory));
        assert!(file.ends_with("boot/main.md") || file.ends_with("boot\\main.md"));
        // a memory kept in files, or a database that does not answer: nothing is written
        let mut settings = Settings { memory_folder: Some(memory.display().to_string()), ..Settings::default() };
        assert!(settings.db_boot_at_wake);
        assert_eq!(prepare_boot(&store, &settings, Room::Main, &memory), None);
        std::fs::write(memory.join(".env"), "MEMORY_DB_HOST=127.0.0.1
MEMORY_DB_PORT=1
MEMORY_DB_NAME=x_memorycoredb
MEMORY_DB_USER=castle_x
MEMORY_DB_PASSWORD=x
").unwrap();
        assert_eq!(prepare_boot(&store, &settings, Room::Main, &memory), None);
        settings.cli = Cli::Codex;
        assert_eq!(prepare_boot(&store, &settings, Room::Main, &memory), None);
        // a file left by a run that ended badly is removed
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "old").unwrap();
        forget_boot_file(&store);
        assert!(!file.exists());
    }

    #[test]
    fn only_the_main_session_offers_a_fresh_start_and_not_when_every_start_is_fresh() {
        assert!(offers_fresh_start(Room::Main, "rested"));
        assert!(offers_fresh_start(Room::Main, "continue"));
        assert!(!offers_fresh_start(Room::Main, "fresh"));
        assert!(!offers_fresh_start(Room::Story, "continue"));
    }

    #[test]
    fn only_the_wake_button_greets_and_only_in_the_main_session() {
        assert_eq!(opening_prompt(Room::Main, Some(true)), Some(WAKE_PROMPT));
        assert_eq!(opening_prompt(Room::Main, Some(false)), None, "a wake Castle does itself");
        assert_eq!(opening_prompt(Room::Main, None), None, "no answer means no greeting");
        assert_eq!(opening_prompt(Room::Story, Some(true)), None, "the STORY room has its own opening");
    }

    #[test]
    fn the_terminal_answering_the_console_is_told_apart_from_typing() {
        // the bytes Castle's screen (xterm.js) sent back to real Codex and Claude Code queries, plus the same
        // shapes ending in BEL and as a DCS version reply (not captured, but the same kind of reply)
        for reply in [
            "\x1b[1;1R", "\x1b[24;80R", "\x1b[?1;2c", "\x1b[>0;276;0c", "\x1b[0n", "\x1b[I", "\x1b[O",
            "\x1b[?0u", "\x1b[?2026;2$y", "\x1b[?1;2$y",
            "\x1b]10;rgb:eeee/f3f3/ffff\x1b\\", "\x1b]11;rgb:0000/0000/0000\x1b\\", "\x1b]4;1;rgb:cccc/0000/0000\x1b\\",
            "\x1b]11;rgb:0000/0000/0000\x07", "\x1bP>|xterm.js(5.5.0)\x1b\\",
        ] {
            assert!(is_terminal_reply(reply), "{reply:?}");
        }
        for typed in [
            "hello", "\r", "\x7f", "\t", "\x1b", "\x1b[A", "\x1b[1;5C", "\x1b[3~", "\x1b[Z", "\x1bOP", "\x1b[200~paste\x1b[201~",
            "\x1b[1;1Rmore", "\x1b[", "R", "\x1b[1;1H", "\x1b[97;5u", "\x1b[<0;10;5M", "\x1b]\x1b\\", "\x1b]11;rgb:0/0/0\x1b\\x",
            "\x1b[?2026;2$u", "\x1bb", "\x1b]hello\x07",
        ] {
            assert!(!is_terminal_reply(typed), "{typed:?}");
        }
    }
}
