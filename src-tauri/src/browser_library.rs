//! What came over from Chrome, in use: the new tab's Most visited and Bookmarks, the toolbar's star,
//! the Fill button on a saved site's sign-in page, and Settings' Saved passwords. A password is
//! opened by Castle's own code only for the member's Fill press, only for the page's own site (read
//! from the page itself, never from what the toolbar says), and is never returned, logged or shown.

use crate::browser::{tab_label, Browser};
use crate::settings::SettingsStore;
use crate::vault::{self, Bookmark, Login};
use mysql::{prelude::Queryable, Conn};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

fn conn(store: &SettingsStore) -> Result<Conn, String> {
    let memory = store.get().memory_folder.ok_or("Choose your memory folder first.")?;
    crate::store::source(std::path::Path::new(&memory)).ok_or_else(|| "Your memory database is not answering.".to_string())
}

/// One of the most visited sites.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Visited {
    pub host: String,
    pub url: String,
    pub visits: u64,
}

/// What the new tab shows besides Artifacts and the dev servers.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTab {
    pub most_visited: Vec<Visited>,
    pub bookmarks: Vec<Bookmark>,
}

#[tauri::command]
pub async fn browser_new_tab(app: AppHandle) -> Result<NewTab, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(mut c) = conn(&app.state::<SettingsStore>()) else { return Ok(NewTab::default()) };
        let rows: Vec<(String, u64)> = c.query("SELECT host, SUM(count) FROM browser_visits GROUP BY host ORDER BY SUM(count) DESC LIMIT 8").map_err(|_| "Most visited could not be read.".to_string())?;
        Ok(NewTab {
            most_visited: rows.into_iter().map(|(host, visits)| Visited { url: format!("https://{host}/"), host, visits }).collect(),
            bookmarks: vault::bookmarks(&mut c, 12)?,
        })
    })
    .await
    .map_err(|_| "The new tab could not be read.".to_string())?
}

/// The page in front: (its address, its host), read from the page's own webview.
fn page_in_front(app: &AppHandle) -> Result<(u32, String, String), String> {
    let id = app.state::<Browser>().0.lock().unwrap().current().map(|t| t.id).ok_or("No page is open.")?;
    let view = app.get_webview(&tab_label(id)).ok_or("No page is open.")?;
    let url = view.url().map_err(|_| "The page did not answer.".to_string())?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("This page is not a website.".into());
    }
    let host = crate::site_rules::clean_host(url.host_str().unwrap_or("")).ok_or("This page has no site to fill.")?;
    Ok((id, url.to_string(), host))
}

/// The star: is the page in front bookmarked?
#[tauri::command]
pub async fn browser_is_bookmarked(app: AppHandle, url: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || vault::is_bookmarked(&mut conn(&app.state::<SettingsStore>())?, &url)).await.map_err(|_| "The bookmark could not be read.".to_string())?
}

/// The star pressed: bookmarks the page, or removes its bookmark. True when it is now bookmarked.
#[tauri::command]
pub async fn browser_bookmark_toggle(app: AppHandle, url: String, title: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = conn(&app.state::<SettingsStore>())?;
        if vault::is_bookmarked(&mut c, &url)? {
            vault::remove_bookmark(&mut c, &url)?;
            Ok(false)
        } else {
            vault::add_bookmark(&mut c, &title, &url, "")?;
            Ok(true)
        }
    })
    .await
    .map_err(|_| "The bookmark could not be kept.".to_string())?
}

/// The saved logins for the page in front (usernames only): the Fill button shows when there are.
#[tauri::command]
pub async fn browser_logins(app: AppHandle) -> Result<Vec<Login>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Ok((_, _, host)) = page_in_front(&app) else { return Ok(Vec::new()) };
        let Ok(mut c) = conn(&app.state::<SettingsStore>()) else { return Ok(Vec::new()) };
        vault::logins_for(&mut c, &host)
    })
    .await
    .map_err(|_| "The saved logins could not be read.".to_string())?
}

/// The script that fills the page's visible password box and the username box before it. The values
/// go in as JSON strings, never as code. Pure.
pub fn fill_script(username: &str, password: &str) -> String {
    let (user, pass) = (serde_json::to_string(username).unwrap_or_else(|_| "\"\"".into()), serde_json::to_string(password).unwrap_or_else(|_| "\"\"".into()));
    format!(
        "(() => {{ const shown = (e) => e.offsetParent !== null; const boxes = [...document.querySelectorAll('input[type=password]')]; const p = boxes.find(shown) || boxes[0]; if (!p) return; \
         const set = (el, v) => {{ Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v); el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); }}; \
         const all = [...(p.form || document).querySelectorAll('input')]; const before = all.slice(0, all.indexOf(p)).reverse(); \
         const u = before.find((e) => shown(e) && ['text', 'email', 'tel', ''].includes(e.type)); const user = {user}; if (u && user) set(u, user); set(p, {pass}); }})()"
    )
}

/// Fill pressed: Castle's code opens that login and fills the page in front - only when the page's
/// own site is the login's site or under it.
#[tauri::command]
pub async fn browser_fill(app: AppHandle, id: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (tab, _, host) = page_in_front(&app)?;
        let mut c = conn(&app.state::<SettingsStore>())?;
        let (username, password) = vault::fill_secret(&mut c, &vault::key()?, id, &host)?;
        let view = app.get_webview(&tab_label(tab)).ok_or("The page is gone.")?;
        view.eval(fill_script(&username, &password)).map_err(|_| "The page did not take it.".to_string())
    })
    .await
    .map_err(|_| "The page could not be filled.".to_string())?
}

/// Settings' Saved passwords: each site and username, never a password.
#[tauri::command]
pub async fn vault_list(app: AppHandle) -> Result<Vec<Login>, String> {
    tauri::async_runtime::spawn_blocking(move || vault::list(&mut conn(&app.state::<SettingsStore>())?)).await.map_err(|_| "The saved passwords could not be read.".to_string())?
}

/// Remove one saved login, or every one (`id` None).
#[tauri::command]
pub fn vault_forget(store: State<'_, SettingsStore>, id: Option<u64>) -> Result<(), String> {
    let mut c = conn(&store)?;
    match id {
        Some(id) => vault::forget(&mut c, id),
        None => vault::forget_all(&mut c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fill_script_carries_the_values_as_strings_never_as_code() {
        let s = fill_script("ari", "p\"); alert(1); (\"");
        assert!(s.contains(r#"const user = "ari";"#));
        assert!(s.contains(r#"set(p, "p\"); alert(1); (\"")"#), "a quote in a password stays inside its string");
        assert!(s.starts_with("(() => {") && s.ends_with("})()"));
        assert!(fill_script("", "x").contains("const user = \"\";"));
    }
}
