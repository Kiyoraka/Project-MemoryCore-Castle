//! The page's doors to the Sentinel: its status for the shield, the findings and the member's
//! buttons for the panel, the log, and the set-up lines for Settings.

use super::{book, hand, officer, CHANGED};
use crate::settings::SettingsStore;
use mysql::prelude::Queryable;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// The newest patrol, as the shield shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatrolLine {
    pub status: String,
    pub line: String,
    pub minutes_ago: i64,
}

/// Open findings by hint.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Counts {
    pub alarm: u32,
    pub look: u32,
    pub note: u32,
}

/// Everything the shield needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This computer can be watched (Windows for now).
    pub supported: bool,
    pub on: bool,
    pub installed: bool,
    pub current: bool,
    pub setup_line: String,
    pub patrol: Option<PatrolLine>,
    pub patrolling: bool,
    pub unread: u32,
    pub open: Counts,
    /// Minutes since the watch last ran.
    pub watch_minutes: Option<u64>,
    /// Why the book could not be read, when it could not.
    pub problem: Option<String>,
    /// Joined the Aegis Network.
    pub network: bool,
    /// Addresses the network blocked in the last day.
    pub network_blocked: u32,
}

fn memory(store: &SettingsStore) -> Result<PathBuf, String> {
    store.get().memory_folder.map(PathBuf::from).ok_or_else(|| "Choose your memory folder first.".to_string())
}

fn status_of(store: &SettingsStore) -> Status {
    let gear = store.get().sentinel.bounded();
    let installed = hand::installed();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut status = Status {
        supported: cfg!(windows),
        on: gear.on,
        installed: installed.installed,
        current: installed.current,
        setup_line: installed.line,
        patrol: None,
        patrolling: officer::running(),
        unread: 0,
        open: Counts::default(),
        watch_minutes: super::watch::last_tick(&hand::local_dir()).map(|t| now.saturating_sub(t) / 60),
        problem: None,
        network: gear.network,
        network_blocked: 0,
    };
    let read = memory(store).and_then(|m| book::open(&m)).and_then(|mut conn| {
        let patrol: Option<(Option<String>, Option<String>, i64)> = conn
            .query_first("SELECT status, line, TIMESTAMPDIFF(MINUTE, started_at, NOW()) FROM sentinel_patrols WHERE status IS NOT NULL ORDER BY id DESC LIMIT 1")
            .map_err(|_| "The Sentinel's book could not be read.".to_string())?;
        let unread: Option<u32> = conn.query_first("SELECT COUNT(*) FROM sentinel_alarms WHERE seen_at IS NULL").map_err(|_| "The Sentinel's book could not be read.".to_string())?;
        status.network_blocked = super::network::blocked_today(&mut conn);
        let counts: Vec<(String, u32)> = conn
            .query("SELECT hint, COUNT(*) FROM sentinel_findings WHERE state = 'open' GROUP BY hint")
            .map_err(|_| "The Sentinel's book could not be read.".to_string())?;
        Ok((patrol, unread.unwrap_or(0), counts))
    });
    match read {
        Ok((patrol, unread, counts)) => {
            status.patrol = patrol.map(|(s, l, m)| PatrolLine { status: s.unwrap_or_default(), line: l.unwrap_or_default(), minutes_ago: m });
            status.unread = unread;
            for (hint, n) in counts {
                match hint.as_str() {
                    book::ALARM => status.open.alarm = n,
                    book::LOOK => status.open.look = n,
                    _ => status.open.note = n,
                }
            }
        }
        Err(problem) => status.problem = Some(problem),
    }
    status
}

/// Test my guard: the three checks (the antivirus takes a test file and the alarm writes its row,
/// a practice block and its undo, the five-minute watch alive), on the member's press.
#[tauri::command]
pub async fn sentinel_test(app: AppHandle) -> Result<super::drill::Tested, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let tested = super::drill::test_guard(&memory(&store)?);
        changed(&app);
        tested
    })
    .await
    .map_err(|_| "The test stopped.".to_string())?
}

/// The shield's status.
#[tauri::command]
pub async fn sentinel_status(app: AppHandle) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || status_of(&app.state::<SettingsStore>())).await.map_err(|_| "The Sentinel could not be read.".to_string())
}

/// A finding as the panel shows it: Undo is offered only for a fix made by a move that left a recipe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    #[serde(flatten)]
    pub finding: book::Finding,
    pub undoable: bool,
}

fn undo_request(conn: &mut mysql::Conn, finding: u64) -> Result<Option<(u64, String)>, String> {
    let row: Option<(u64, String)> = conn
        .exec_first(
            "SELECT id, undo_recipe FROM sentinel_actions WHERE finding_id = ? AND state = 'done' AND undo_recipe IS NOT NULL ORDER BY id DESC LIMIT 1",
            (finding,),
        )
        .map_err(|_| "The Sentinel's book could not be read.".to_string())?;
    Ok(row.and_then(|(id, recipe)| {
        let request = serde_json::from_str::<serde_json::Value>(&recipe).ok()?.get("request")?.as_str()?.to_string();
        Some((id, request))
    }))
}

/// The findings in these states (open, settled, accepted, fixed), newest first.
#[tauri::command]
pub async fn sentinel_findings(app: AppHandle, states: Vec<String>) -> Result<Vec<FindingView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let mut conn = book::open(&memory(&store)?)?;
        let wanted: Vec<&str> = states.iter().map(String::as_str).collect();
        let found = book::findings(&mut conn, &wanted, 300)?;
        let mut out = Vec::with_capacity(found.len());
        for finding in found {
            let undoable = finding.state == book::FIXED && undo_request(&mut conn, finding.id)?.is_some();
            out.push(FindingView { finding, undoable });
        }
        Ok(out)
    })
    .await
    .map_err(|_| "The Sentinel could not be read.".to_string())?
}

/// The member pressed Fix, This is normal or Undo: the click is written, then spent at once on that
/// one finding. The companion can never reach this door.
#[tauri::command]
pub async fn sentinel_click(app: AppHandle, id: u64, word: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let mut conn = book::open(&memory(&store)?)?;
        let finding = book::finding(&mut conn, id)?.ok_or("There is no such finding.")?;
        book::click(&mut conn, id, &word)?;
        let said = match word.as_str() {
            "fix" => {
                let mv = finding.fix_move.clone().ok_or("This finding has no move to make.")?;
                if finding.state != book::OPEN {
                    return Err("Only an open finding can be fixed.".into());
                }
                if !book::use_click(&mut conn, id, "fix")? {
                    return Err("The click was already spent.".into());
                }
                let answer = officer::act(&mut conn, hand::Asker::Member, &mv, finding.fix_arg.as_deref(), Some(id))?;
                if !answer.ok {
                    return Err(answer.result);
                }
                answer.result
            }
            "accept" => {
                book::accept(&mut conn, id)?;
                "Kept as normal. It will not be raised again.".to_string()
            }
            "undo" => {
                let (row, request) = undo_request(&mut conn, id)?.ok_or("This fix has nothing to undo.")?;
                if !book::use_click(&mut conn, id, "undo")? {
                    return Err("The click was already spent.".into());
                }
                let answer = officer::act(&mut conn, hand::Asker::Member, "undo", Some(&request), Some(id))?;
                if !answer.ok {
                    return Err(answer.result);
                }
                conn.exec_drop("UPDATE sentinel_actions SET state = 'undone', undone_at = NOW() WHERE id = ?", (row,)).map_err(|_| "The undo could not be written.".to_string())?;
                book::mark_undone(&mut conn, id)?;
                "Undone.".to_string()
            }
            _ => return Err("That is not a button.".into()),
        };
        officer::log(&format!("the member pressed {word} on #{id}: {said}\n"));
        changed(&app);
        Ok(said)
    })
    .await
    .map_err(|_| "The button could not be pressed.".to_string())?
}

/// The newest part of the officer's log.
#[tauri::command]
pub fn sentinel_log() -> String {
    let text = std::fs::read_to_string(officer::room().join("log.txt")).unwrap_or_default();
    let keep = 30 * 1024;
    if text.len() <= keep {
        return text;
    }
    let cut = text.len() - keep;
    let start = text[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
    text[start..].to_string()
}

/// The member opened the Sentinel: its alarms are seen.
#[tauri::command]
pub async fn sentinel_alarms_seen(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let mut conn = book::open(&memory(&store)?)?;
        conn.query_drop("UPDATE sentinel_alarms SET seen_at = NOW() WHERE seen_at IS NULL").map_err(|_| "The alarms could not be marked seen.".to_string())?;
        changed(&app);
        Ok(())
    })
    .await
    .map_err(|_| "The alarms could not be marked seen.".to_string())?
}

/// What Settings > Sentinel shows: the set-up and remove lines, where it stands, and the
/// recommended model for the member's CLI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub supported: bool,
    pub install_line: String,
    pub uninstall_line: String,
    pub installed: hand::Installed,
    pub recommended: Option<String>,
    pub cli_label: String,
    /// signed in to the Foundation, so the Aegis Network can be joined
    pub signed_in: bool,
}

/// Writes the installer and the hand where the member's line points, and gives the lines. The
/// member runs the line themselves, in a PowerShell opened as administrator; Castle never does.
#[tauri::command]
pub async fn sentinel_setup(app: AppHandle) -> Result<Setup, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = app.state::<SettingsStore>().get();
        let recommended = super::recommended_model(settings.cli).map(str::to_string);
        let cli_label = settings.cli.label().to_string();
        if !cfg!(windows) {
            return Ok(Setup { supported: false, install_line: String::new(), uninstall_line: String::new(), installed: hand::installed(), recommended, cli_label, signed_in: super::network::signed_in() });
        }
        let (install, hand_file) = hand::prepare_files(&hand::local_dir())?;
        let castle = std::env::current_exe().map_err(|_| "Castle could not find its own program.".to_string())?;
        let setup = Setup {
            supported: true,
            install_line: hand::install_line(&install, &hand_file, &castle, &hand::account()),
            uninstall_line: hand::uninstall_line(&install),
            installed: hand::installed(),
            recommended,
            cli_label,
            signed_in: super::network::signed_in(),
        };
        changed(&app);
        Ok(setup)
    })
    .await
    .map_err(|_| "The Sentinel's set-up could not be read.".to_string())?
}

/// Tells the page to read the Sentinel again.
pub fn changed(app: &AppHandle) {
    use tauri::Emitter;
    let _ = app.emit(CHANGED, ());
}
