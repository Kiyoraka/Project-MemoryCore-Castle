//! Rooms the member switched off in Settings: not available, never removed. A room that is off
//! has no tab, its commands answer that it is off, and what Castle ran for it pauses (the story
//! session ends, the world windows close). Turning it back on brings everything back as it was.

use crate::session::StoryHost;
use crate::settings::{self, SettingsStore};
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, State};

/// Ok when the room is on; else a plain line for the page.
pub fn need(store: &SettingsStore, room: &str) -> Result<(), String> {
    if store.get().room_on(room) {
        Ok(())
    } else {
        Err(off_line(room))
    }
}

pub fn off_line(room: &str) -> String {
    format!("The {} room is turned off in Settings.", room.to_uppercase())
}

/// Saves the switches, then pauses what runs for a room just turned off (and brings back what a
/// room just turned on needs at once).
#[tauri::command]
pub async fn rooms_apply(app: AppHandle, rooms: BTreeMap<String, bool>, store: State<'_, SettingsStore>) -> Result<BTreeMap<String, bool>, String> {
    let before = store.get();
    let mut next = before.clone();
    next.rooms = rooms;
    let next = next.normalized();
    settings::save(&store.path, &next).map_err(|_| "The rooms could not be saved.".to_string())?;
    let saved = next.rooms.clone();
    let story = (before.room_on("story"), next.room_on("story"));
    let world = (before.room_on("world"), next.room_on("world"));
    let worldsmith = (before.room_on(crate::worldsmith::ROOM), next.room_on(crate::worldsmith::ROOM));
    *store.current.lock().unwrap() = next;
    if worldsmith == (true, false) {
        // the game let go, every frozen value stopped, the helper ended
        crate::worldsmith::pipe::stop(&app);
    }
    if story == (true, false) {
        app.state::<StoryHost>().0.kill();
    }
    match world {
        (true, false) => crate::world::close_all(&app),
        (false, true) => crate::world::restore(&app),
        _ => {}
    }
    Ok(saved)
}
