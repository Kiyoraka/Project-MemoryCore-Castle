//! The browser's first-open welcome: one step at a time, each the member's own Yes or Skip - bring
//! the history from Chrome, the bookmarks, the saved passwords, and let the companion check sites.
//! The password step is the member's decision; their own companion may guide them through Chrome's
//! export, and Castle's own code - never the companion - reads the file, seals it and shreds it. The
//! companion hears the outcome as counts only.

use crate::chrome_import::{self, PasswordsDone, Profile};
use crate::session::{self, MainHost, Room};
use crate::settings::SettingsStore;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// The steps, in the order they are offered.
pub const STEPS: [&str; 4] = ["history", "bookmarks", "passwords", "sitecheck"];

/// What the welcome card needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    /// step -> "yes" | "skip"
    pub answers: std::collections::BTreeMap<String, String>,
    pub chrome: bool,
    pub profiles: Vec<Profile>,
    pub companion: String,
}

fn memory_conn(store: &SettingsStore) -> Result<mysql::Conn, String> {
    let memory = store.get().memory_folder.ok_or("Choose your memory folder first.")?;
    crate::store::source(std::path::Path::new(&memory)).ok_or_else(|| "Your memory database is not answering, so nothing was brought over.".to_string())
}

fn save_answer(store: &SettingsStore, step: &str, answer: &str, site_check: Option<bool>) -> Result<(), String> {
    let mut next = store.get();
    next.browser_welcome.insert(step.to_string(), answer.to_string());
    if let Some(on) = site_check {
        next.browser_site_check = on;
    }
    crate::settings::save(&store.path, &next).map_err(|_| "Your answer could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

#[tauri::command]
pub fn browser_welcome(store: State<'_, SettingsStore>) -> Welcome {
    let settings = store.get();
    let root = chrome_import::chrome_root();
    Welcome {
        answers: settings.browser_welcome.clone(),
        chrome: root.is_some(),
        profiles: root.map(|r| chrome_import::profiles(&r)).unwrap_or_default(),
        companion: crate::fresh_start::name_of(settings.companion_name.as_deref()),
    }
}

/// A step answered without bringing anything (Skip, or Yes on the site check).
#[tauri::command]
pub fn browser_welcome_answer(store: State<'_, SettingsStore>, step: String, answer: String) -> Result<(), String> {
    if !STEPS.contains(&step.as_str()) || !["yes", "skip"].contains(&answer.as_str()) {
        return Err("That is not a step of the welcome.".into());
    }
    let site_check = (step == "sitecheck").then_some(answer == "yes");
    save_answer(&store, &step, &answer, site_check)
}

/// Settings' Bring things from Chrome: every skipped step is offered again.
#[tauri::command]
pub fn browser_welcome_reopen(store: State<'_, SettingsStore>) -> Result<(), String> {
    let mut next = store.get();
    next.browser_welcome.retain(|_, answer| answer != "skip");
    crate::settings::save(&store.path, &next).map_err(|_| "The welcome could not be opened again.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

#[tauri::command]
pub async fn browser_import_history(app: AppHandle, profile: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let pages = chrome_import::import_history(&mut memory_conn(&store)?, &profile)?;
        save_answer(&store, "history", "yes", None)?;
        Ok(pages)
    })
    .await
    .map_err(|_| "The history could not be brought over.".to_string())?
}

#[tauri::command]
pub async fn browser_import_bookmarks(app: AppHandle, profile: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let added = chrome_import::import_bookmarks(&mut memory_conn(&store)?, &profile)?;
        save_answer(&store, "bookmarks", "yes", None)?;
        Ok(added)
    })
    .await
    .map_err(|_| "The bookmarks could not be brought over.".to_string())?
}

/// The words the companion is asked to guide with (the member's own message, from Castle). Pure.
pub fn guide_words() -> String {
    "(Castle) I'd like to bring my saved passwords from Chrome into Castle's vault. Please guide me one step at a time: first tell me why a sealed vault on this PC is safer than an export file lying around, then walk me through Chrome's Password Manager > Settings > Export passwords (Chrome will ask for my Windows PIN). Never ask me for a password and never open the file yourself: when I have it, I choose it in the browser's welcome card, and Castle's own code seals it and shreds the file.".into()
}

/// The outcome the companion hears: counts only. Pure.
pub fn outcome_words(done: &PasswordsDone) -> String {
    let n = done.sealed;
    let sealed = format!("{n} password{} {} sealed in my vault", if n == 1 { "" } else { "s" }, if n == 1 { "is" } else { "are" });
    if done.shredded {
        format!("(Castle) {sealed}, and the export file is gone.")
    } else {
        format!("(Castle) {sealed}, but the export file could not be removed: please help me delete it now.")
    }
}

/// Puts a line in the home chat: typed when the chat is awake, the chat woken with it otherwise.
fn tell_companion(app: &AppHandle, text: &str, wake: bool) -> Result<(), String> {
    if app.state::<MainHost>().0.is_running() {
        crate::messages::send_message(text.to_string(), None, app.clone()).map(|_| ())
    } else if wake {
        let store = app.state::<SettingsStore>();
        session::wake_with_prompt(app, &store, &app.state::<MainHost>().0, Room::Main, 100, 30, Some(text))
    } else {
        Ok(())
    }
}

/// Ask <companion> to guide me: one message in the home chat.
#[tauri::command]
pub async fn browser_password_guide(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || tell_companion(&app, &guide_words(), true)).await.map_err(|_| "Your companion could not be asked.".to_string())?
}

/// Choose the exported file: Castle's own picker (the path never passes through a page), then
/// Castle's code seals it and shreds it. `guided`: the companion was asked to guide, so it hears the
/// outcome (counts only). None when the member closed the picker.
#[tauri::command]
pub async fn browser_import_passwords(app: AppHandle, guided: bool) -> Result<Option<PasswordsDone>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let picked: Option<PathBuf> = app.dialog().file().set_title("Choose Chrome's password export").add_filter("Chrome Passwords", &["csv"]).blocking_pick_file().and_then(|p| p.into_path().ok());
        let Some(path) = picked else { return Ok(None) };
        let store = app.state::<SettingsStore>();
        let mut conn = memory_conn(&store)?;
        let key = crate::vault::key()?;
        let done = chrome_import::import_passwords(&mut conn, &key, &path)?;
        save_answer(&store, "passwords", "yes", None)?;
        if guided {
            let _ = tell_companion(&app, &outcome_words(&done), false);
        }
        Ok(Some(done))
    })
    .await
    .map_err(|_| "The passwords could not be brought over.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_never_asks_for_a_password_and_the_outcome_is_counts_only() {
        let words = guide_words();
        assert!(words.contains("Never ask me for a password") && words.contains("Export passwords") && words.contains("Windows PIN"));
        assert_eq!(outcome_words(&PasswordsDone { sealed: 42, skipped: 1, shredded: true }), "(Castle) 42 passwords are sealed in my vault, and the export file is gone.");
        assert_eq!(outcome_words(&PasswordsDone { sealed: 1, skipped: 0, shredded: false }), "(Castle) 1 password is sealed in my vault, but the export file could not be removed: please help me delete it now.");
    }

    #[test]
    fn the_steps_come_in_their_order() {
        assert_eq!(STEPS, ["history", "bookmarks", "passwords", "sitecheck"]);
    }
}
