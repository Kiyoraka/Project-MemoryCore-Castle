//! FINANCE: the receipts the member's companion reads, kept in the member's own memory database.
//! The companion proposes (`castle-memory finance propose`); only the member keeps or dismisses,
//! here. Nothing of it leaves this computer: no Portfolio, no Foundation server, no room.

pub mod companion;
pub mod store;

use crate::settings::SettingsStore;
use mysql::{prelude::Queryable, Conn};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// The migration FINANCE needs before it can read or write.
const MIGRATION: &str = "028_finance";

/// A connection for a FINANCE command: the room must be on and its table must exist.
fn connect(app: &AppHandle) -> Result<Conn, String> {
    let settings = app.state::<SettingsStore>();
    crate::rooms::need(&settings, "finance")?;
    let mut c = crate::automations::room_connection(&settings)?;
    let ready: Option<String> = c
        .exec_first("SELECT name FROM schema_migrations WHERE name=?", (MIGRATION,))
        .map_err(|_| "The memory database is preparing. Try again in a moment.".to_string())?;
    ready.ok_or("FINANCE is being set up. Restart Castle and try again.".to_string())?;
    Ok(c)
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|_| "FINANCE could not be reached.".to_string())?
}

/// A month's spending (this month when none is named), the kept receipts and everything waiting.
#[tauri::command]
pub async fn finance_month(app: AppHandle, month: Option<String>) -> Result<store::Month, String> {
    let month = month.unwrap_or_else(store::this_month);
    blocking(move || store::month(&mut connect(&app)?, &month)).await
}

/// The member keeps a waiting receipt (with the fields they corrected) or dismisses it.
#[tauri::command]
pub async fn finance_decide(app: AppHandle, id: u64, keep: bool, edits: Option<store::Draft>) -> Result<(), String> {
    let today = chrono::Local::now().date_naive();
    let edited = match edits {
        Some(d) if keep => Some(store::check(&d, today)?),
        _ => None,
    };
    blocking(move || store::decide(&mut connect(&app)?, id, keep, edited.as_ref())).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: String,
    pub count: usize,
}

/// The kept receipts of a month (or all of them) as CSV, where the member chooses to save it.
#[tauri::command]
pub async fn finance_export(app: AppHandle, month: Option<String>) -> Result<Option<Exported>, String> {
    blocking(move || {
        let mut c = connect(&app)?;
        let name = format!("castle-receipts-{}.csv", month.as_deref().unwrap_or("all"));
        let Some(path) = app.dialog().file().set_title("Save the receipts").set_file_name(&name).add_filter("CSV", &["csv"]).blocking_save_file().and_then(|p| p.into_path().ok()) else {
            return Ok(None);
        };
        let rows = store::kept_for_export(&mut c, month.as_deref())?;
        std::fs::write(&path, store::csv(&rows)).map_err(|_| "The file could not be saved there.".to_string())?;
        Ok(Some(Exported { path: path.to_string_lossy().to_string(), count: rows.len() }))
    })
    .await
}

#[cfg(test)]
mod store_live;
