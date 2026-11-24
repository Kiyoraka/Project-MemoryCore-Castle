//! The DEV tab: the companion at work in the member's project folders, up to `dev_tabs::SLOTS`
//! sessions at once (a project may run several), each its own tab shown as a Chat with a Log
//! beside it. The project list is kept apart from the sessions: ending a session keeps its
//! project, and removing a project ends its sessions first. She wakes there as the Castle's
//! companion with her memory (from the database when it is there) and a short work note; the
//! member's personal Claude Code setup is always left out, so on a PC whose own setup is another
//! companion a DEV tab is never someone else. Plans she copies live in the member's database and
//! show in the Plan panel.

use crate::chat::{ChatPoll, ChatState};
use crate::dev_tabs::{self, SLOTS};
use crate::env;
use crate::key;
use crate::pty::{self, PtyHost};
use crate::session::{self, SessionStatus};
use crate::settings::{self, Settings, SettingsStore};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
pub struct DevHosts(pub [PtyHost; SLOTS]);
#[derive(Default)]
pub struct DevChats(pub [ChatState; SLOTS]);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevTab {
    slot: usize,
    folder: String,
    name: String,
    /// its number within its project, kept for the session's life ("shop · 2")
    number: u32,
    running: bool,
    status: SessionStatus,
    /// the tab started on another model because its own weekly share is spent
    notice: Option<session::ModelNotice>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevProject {
    folder: String,
    name: String,
    /// the folder is no longer on this computer (it stays listed until the member removes it)
    missing: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevState {
    tabs: Vec<DevTab>,
    projects: Vec<DevProject>,
    memory_folder: Option<String>,
}

fn host(hosts: &DevHosts, slot: usize) -> Result<&PtyHost, String> {
    hosts.0.get(slot).ok_or_else(|| "That DEV tab does not exist.".to_string())
}

pub(crate) fn folder_of(settings: &Settings, slot: usize) -> Result<PathBuf, String> {
    settings
        .dev_tabs
        .get(slot)
        .cloned()
        .flatten()
        .map(PathBuf::from)
        .ok_or_else(|| "That DEV tab has no project open.".to_string())
}

fn tab_status(settings: &Settings, folder: &Path, program: Option<&Path>) -> SessionStatus {
    let not_ready = |message: String, fix: &'static str| SessionStatus { ready: false, message, fix: Some(fix) };
    if !folder.is_dir() {
        return not_ready("This project folder was not found. Take it off the list and open it again.".into(), "settings");
    }
    if program.is_none() {
        return not_ready(format!("{} was not found on this computer. Install it, then try again.", settings.cli.label()), "install");
    }
    SessionStatus { ready: true, message: format!("Ready to wake {} here.", settings.companion_label()), fix: None }
}

fn state(store: &SettingsStore, hosts: &DevHosts) -> DevState {
    let settings = store.get();
    let program = env::find_on_path(settings.cli.command());
    let numbers = dev_tabs::numbers(&settings.dev_tabs, &settings.dev_tab_numbers);
    let tabs = settings
        .dev_tabs
        .iter()
        .enumerate()
        .filter_map(|(slot, folder)| {
            let folder = folder.as_ref()?;
            Some(DevTab {
                slot,
                folder: folder.clone(),
                name: dev_tabs::name_of(folder),
                number: numbers.get(slot).copied().unwrap_or(1),
                running: hosts.0.get(slot).is_some_and(PtyHost::is_running),
                status: tab_status(&settings, Path::new(folder), program.as_deref()),
                notice: hosts.0.get(slot).and_then(PtyHost::fallback).map(session::ModelNotice::from),
            })
        })
        .collect();
    let projects = settings
        .dev_projects
        .iter()
        .map(|f| DevProject { folder: f.clone(), name: dev_tabs::name_of(f), missing: !Path::new(f).is_dir() })
        .collect();
    DevState { tabs, projects, memory_folder: settings.memory_folder.clone() }
}

fn save(store: &SettingsStore, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let mut next = store.get();
    change(&mut next);
    settings::save(&store.path, &next).map_err(|_| "The DEV tabs could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

/// The boot file for a tab: the work note, then the memory when the database holds it. None for
/// Codex (it has no such option) or when it cannot be written.
fn prepare_boot(store: &SettingsStore, settings: &Settings, slot: usize, project: &Path) -> Option<(PathBuf, bool)> {
    let file = dev_tabs::boot_file(&store.folder(), slot);
    let _ = std::fs::remove_file(&file);
    if settings.cli.boot_flag().is_none() {
        return None;
    }
    let memory_folder = settings.memory_folder.as_deref().map(PathBuf::from);
    let memory = match (&memory_folder, settings.db_boot_at_wake) {
        (Some(folder), true) => crate::store::source(folder).and_then(|mut conn| crate::mirror::boot_prompt(&mut conn).ok().flatten()),
        _ => None,
    };
    let block = dev_tabs::dev_block(
        &settings.companion_label(),
        settings.member_name.as_deref().unwrap_or("the member"),
        &dev_tabs::name_of(&project.display().to_string()),
        memory.is_some(),
    );
    // an isolated session reads no CLAUDE.md by itself, so the project's own travels here
    let mut text = block;
    if let Some(instructions) = session::folder_instructions(project) {
        text = format!("{text}\n\n{instructions}");
    }
    if let Some(memory) = &memory {
        text = format!("{text}\n\n{memory}");
    }
    std::fs::create_dir_all(file.parent()?).ok()?;
    std::fs::write(&file, text).ok()?;
    Some((file, memory.is_some()))
}

fn wake(app: &AppHandle, store: &SettingsStore, hosts: &DevHosts, slot: usize, cols: u16, rows: u16) -> Result<(), String> {
    let settings = store.get();
    let project = folder_of(&settings, slot)?;
    let program = env::find_on_path(settings.cli.command());
    let status = tab_status(&settings, &project, program.as_deref());
    let (true, Some(program)) = (status.ready, program) else {
        return Err(status.message);
    };
    let host = host(hosts, slot)?;
    let mut vars = env::session_env(settings.cli, &|name| key::load(name), &session::tools_folder(app), env::NodePlace::Last);
    if let Ok(dir) = app.path().resource_dir() {
        vars.push(("CASTLE_STYLES".into(), dir.join("styles").display().to_string()));
    }
    // the door (`castle-memory`) finds the companion's memory from any project folder by this
    if let Some(memory) = &settings.memory_folder {
        vars.push(("CASTLE_MEMORY".into(), memory.clone()));
    }
    let mut args = Vec::new();
    if let Some((boot, _)) = prepare_boot(store, &settings, slot, &project) {
        args.extend(["--append-system-prompt-file".to_string(), boot.display().to_string()]);
    }
    // A DEV tab is always only this companion: the personal setup never reaches a project session.
    // The project's own CLAUDE.md is still read (it describes the project).
    // with Castle's status line, so the tab can show how full her context is
    // a model whose weekly share is spent starts on the fallback (the project's own records say which)
    let fallback = session::fallback_for(&settings, &project, None);
    args.extend(session::fallback_args(fallback.as_ref()));
    let (isolation_args, isolation_vars) = session::isolation_with_status(store, settings.cli, true, &session::tools_folder(app))?;
    args.extend(isolation_args);
    vars.extend(isolation_vars);
    let output_event = format!("dev-output-{slot}");
    let exit_event = format!("dev-exit-{slot}");
    let output = app.clone();
    let ending = app.clone();
    let started = host.start(
        pty::launch_command(&program, &project, &vars, &args),
        cols.max(20),
        rows.max(5),
        move |text| {
            let _ = output.emit(&output_event, text);
        },
        move |id, code| {
            let store = ending.state::<SettingsStore>();
            let _ = std::fs::remove_file(dev_tabs::boot_file(&store.folder(), slot));
            if let Some(host) = ending.state::<DevHosts>().0.get(slot) {
                host.finished(id);
            }
            let _ = ending.emit(&exit_event, code);
        },
    )?;
    if started {
        host.bind_provider(settings.cli);
        host.set_fallback(fallback);
    }
    Ok(())
}

pub fn any_running(app: &AppHandle) -> bool {
    app.state::<DevHosts>().0.iter().any(PtyHost::is_running)
}

pub fn end_all(app: &AppHandle) {
    for host in app.state::<DevHosts>().0.iter() {
        host.kill();
    }
}

#[tauri::command]
pub fn dev_state(store: State<SettingsStore>, hosts: State<DevHosts>) -> DevState {
    state(&store, &hosts)
}

#[tauri::command]
pub fn dev_open(folder: String, store: State<SettingsStore>, hosts: State<DevHosts>) -> Result<DevState, String> {
    crate::rooms::need(&store, "dev")?;
    let settings = store.get();
    let memory = settings.memory_folder.as_deref().map(PathBuf::from);
    if let Some(why) = dev_tabs::refusal(Path::new(&folder), memory.as_deref()) {
        return Err(why);
    }
    let slot = dev_tabs::slot_for(&settings.dev_tabs, &folder)?;
    save(&store, |s| {
        if s.dev_tabs.get(slot).cloned().flatten().is_none() {
            place(s, slot, &folder);
        }
        s.dev_projects = dev_tabs::remembered(&s.dev_projects, &folder);
    })?;
    Ok(state(&store, &hosts))
}

/// A new session in `folder` in the tab `slot`, with the lowest number its project has free.
fn place(s: &mut Settings, slot: usize, folder: &str) {
    let number = dev_tabs::number_for(&s.dev_tabs, &s.dev_tab_numbers, folder);
    s.dev_tab_numbers = dev_tabs::numbers(&s.dev_tabs, &s.dev_tab_numbers);
    s.dev_tabs = dev_tabs::with_tab(&s.dev_tabs, slot, Some(folder.to_string()));
    s.dev_tab_numbers = dev_tabs::with_number(&s.dev_tab_numbers, s.dev_tabs.len(), slot, number);
}

/// One more session in a project, even when it already has one: the first free tab (8 in all).
#[tauri::command]
pub fn dev_new_session(folder: String, store: State<SettingsStore>, hosts: State<DevHosts>) -> Result<DevState, String> {
    crate::rooms::need(&store, "dev")?;
    let settings = store.get();
    let memory = settings.memory_folder.as_deref().map(PathBuf::from);
    if let Some(why) = dev_tabs::refusal(Path::new(&folder), memory.as_deref()) {
        return Err(why);
    }
    let slot = dev_tabs::free_slot(&settings.dev_tabs)?;
    save(&store, |s| {
        place(s, slot, &folder);
        s.dev_projects = dev_tabs::remembered(&s.dev_projects, &folder);
    })?;
    Ok(state(&store, &hosts))
}

/// The live servers: every listening developer port, and which DEV tab owns it. Read only:
/// nothing here ever ends a process.
#[tauri::command]
pub async fn dev_servers(store: State<'_, SettingsStore>, hosts: State<'_, DevHosts>) -> Result<Vec<crate::servers::Server>, String> {
    let settings = store.get();
    let sessions: Vec<(u8, u32)> = hosts.0.iter().enumerate().filter_map(|(slot, host)| host.pid().map(|pid| (slot as u8, pid))).collect();
    let projects: Vec<(u8, PathBuf)> =
        settings.dev_tabs.iter().enumerate().filter_map(|(slot, folder)| folder.as_ref().map(|f| (slot as u8, PathBuf::from(f)))).collect();
    let companion = settings.companion_label();
    tauri::async_runtime::spawn_blocking(move || crate::servers::servers(&sessions, &projects, &companion))
        .await
        .map_err(|_| "The servers could not be read.".to_string())
}

/// Ends the session in `slot` and frees its tab: its project stays on the list.
fn end_session(s: &mut Settings, slot: usize, hosts: &DevHosts, chats: &DevChats, phones: &crate::emulator::Phones, runs: &crate::phone_run::PhoneRuns) {
    if let Some(host) = hosts.0.get(slot) {
        host.kill();
    }
    if let Some(run) = runs.hosts.get(slot) {
        run.kill(); // the tab's app run (flutter run, gradlew) ends with its tab
    }
    // the tab's phone ends with it (only one Castle started); off the settings lock, it may take seconds
    let phones = phones.clone();
    std::thread::spawn(move || crate::emulator::stop(&phones, slot));
    if let Some(chat) = chats.0.get(slot) {
        chat.clear();
    }
    s.dev_tab_numbers = dev_tabs::numbers(&s.dev_tabs, &s.dev_tab_numbers);
    s.dev_tabs = dev_tabs::with_tab(&s.dev_tabs, slot, None);
    s.dev_tab_numbers = dev_tabs::with_number(&s.dev_tab_numbers, s.dev_tabs.len(), slot, 0);
}

#[tauri::command]
pub fn dev_close_tab(slot: usize, store: State<SettingsStore>, hosts: State<DevHosts>, chats: State<DevChats>, phones: State<crate::emulator::Phones>, runs: State<crate::phone_run::PhoneRuns>) -> Result<DevState, String> {
    host(&hosts, slot)?;
    save(&store, |s| end_session(s, slot, &hosts, &chats, &phones, &runs))?;
    Ok(state(&store, &hosts))
}

/// Takes a project off the list, after the member confirmed: its sessions end first, so no tab
/// is ever left without its project. The folder and its files are not touched.
#[tauri::command]
pub fn dev_remove_project(folder: String, store: State<SettingsStore>, hosts: State<DevHosts>, chats: State<DevChats>, phones: State<crate::emulator::Phones>, runs: State<crate::phone_run::PhoneRuns>) -> Result<DevState, String> {
    save(&store, |s| {
        for slot in dev_tabs::slots_of(&s.dev_tabs, &folder) {
            end_session(s, slot, &hosts, &chats, &phones, &runs);
        }
        s.dev_projects = dev_tabs::forgotten(&s.dev_projects, &folder);
    })?;
    Ok(state(&store, &hosts))
}

#[tauri::command]
pub fn dev_wake(app: AppHandle, slot: usize, cols: u16, rows: u16, store: State<SettingsStore>, hosts: State<DevHosts>) -> Result<DevState, String> {
    crate::rooms::need(&store, "dev")?;
    wake(&app, &store, &hosts, slot, cols, rows)?;
    Ok(state(&store, &hosts))
}

#[tauri::command]
pub fn dev_write(slot: usize, data: String, hosts: State<DevHosts>) -> Result<(), String> {
    let host = host(&hosts, slot)?;
    if session::is_terminal_reply(&data) {
        return host.reply(data.as_bytes());
    }
    host.write(data.as_bytes())
}

#[tauri::command]
pub fn dev_resize(slot: usize, cols: u16, rows: u16, hosts: State<DevHosts>) -> Result<(), String> {
    host(&hosts, slot)?.resize(cols.max(20), rows.max(5))
}

#[tauri::command]
pub fn dev_snapshot(slot: usize, hosts: State<DevHosts>) -> Result<String, String> {
    Ok(host(&hosts, slot)?.snapshot())
}

#[tauri::command]
pub fn dev_chat_poll(slot: usize, store: State<SettingsStore>, hosts: State<DevHosts>, chats: State<DevChats>) -> Result<ChatPoll, String> {
    let settings = store.get();
    let folder = folder_of(&settings, slot)?;
    let chat = chats.0.get(slot).ok_or("That DEV tab does not exist.")?;
    Ok(crate::chat::follow_folder(chat, host(&hosts, slot)?, settings.cli, &folder))
}

/// Whether the project has changes git has not committed (Close session asks first then). A
/// folder that is not a git repository has none.
#[tauri::command]
pub async fn dev_git_dirty(slot: usize, store: State<'_, SettingsStore>) -> Result<bool, String> {
    let folder = folder_of(&store.get(), slot)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = std::process::Command::new("git");
        command.arg("-C").arg(&folder).args(["status", "--porcelain"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        Ok(command
            .output()
            .ok()
            .filter(|o| o.status.success())
            .is_some_and(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty()))
    })
    .await
    .map_err(|_| "Castle could not ask git.".to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevPlans {
    available: bool,
    message: Option<String>,
    plans: Vec<crate::plans::PlanView>,
}

fn plans_for(settings: &Settings, project: &Path) -> DevPlans {
    let none = |message: &str| DevPlans { available: false, message: Some(message.to_string()), plans: Vec::new() };
    let Some(memory) = settings.memory_folder.as_deref().map(PathBuf::from) else {
        return none("Choose your companion's memory folder first.");
    };
    if !crate::setup::has_database(&memory) {
        return none("Plans are kept in your memory's database. Set it up in Settings > Database.");
    }
    let Ok(mut conn) = crate::memdb::open_for(&memory) else {
        return none("Your memory's database is not answering. Start it in Settings > Database.");
    };
    match crate::plans::open_plans(&mut conn, project) {
        Ok(plans) => DevPlans { available: true, message: None, plans },
        Err(why) => none(&String::from(why)),
    }
}

#[tauri::command]
pub async fn dev_plan_items(slot: usize, store: State<'_, SettingsStore>) -> Result<DevPlans, String> {
    let settings = store.get();
    let project = folder_of(&settings, slot)?;
    tauri::async_runtime::spawn_blocking(move || plans_for(&settings, &project))
        .await
        .map_err(|_| "The plans could not be read.".to_string())
}

#[tauri::command]
pub async fn dev_plan_body(id: u64, store: State<'_, SettingsStore>) -> Result<String, String> {
    let memory = store.get().memory_folder.map(PathBuf::from).ok_or("Choose your companion's memory folder first.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = crate::memdb::open_for(&memory)?;
        crate::plans::plan_body(&mut conn, id)
    })
    .await
    .map_err(|_| "The plan could not be read.".to_string())?
}
