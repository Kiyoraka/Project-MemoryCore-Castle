//! What the WORLDSMITH page reads and the member's own presses that are not the helper's: the memory
//! of the games (read only), Keep or No on a recipe the companion proposed, and the Screenshot button.

use super::store;
use crate::settings::SettingsStore;
use std::path::PathBuf;
use tauri::State;

fn open(store: &SettingsStore) -> Result<mysql::Conn, String> {
    crate::rooms::need(store, super::ROOM)?;
    let memory = store.get().memory_folder.map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    let mut c = crate::memdb::open_for(&memory)?;
    crate::memdb::migrate(&mut c)?;
    Ok(c)
}

/// The games remembered, and the values and recipes of the one named.
#[tauri::command]
pub async fn worldsmith_memory(game: Option<u64>, store: State<'_, SettingsStore>) -> Result<store::Memory, String> {
    let mut c = open(&store)?;
    store::memory(&mut c, game)
}

/// The id of a game as the helper named it (exe and stamp), for its values and recipes.
#[tauri::command]
pub async fn worldsmith_game(exe: String, title: String, stamp: String, store: State<'_, SettingsStore>) -> Result<u64, String> {
    let mut c = open(&store)?;
    store::game(&mut c, exe.trim(), title.trim(), stamp.trim())
}

/// The member's Keep (true) or No (false) on a proposed recipe.
#[tauri::command]
pub async fn worldsmith_recipe(id: u64, keep: bool, store: State<'_, SettingsStore>) -> Result<(), String> {
    let mut c = open(&store)?;
    store::decide_recipe(&mut c, id, keep)
}

/// The Screenshot button: the attached game's picture, for the chat.
#[tauri::command]
pub async fn worldsmith_snap(app: tauri::AppHandle, store: State<'_, SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, super::ROOM)?;
    tauri::async_runtime::spawn_blocking(move || super::pipe::shot(&app)).await.map_err(|_| "The Worldsmith helper did not answer.".to_string())?
}
