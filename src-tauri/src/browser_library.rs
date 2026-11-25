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
/// `tab`: a pane's own tab (its toolbar), or None for the tab in front.
fn page_in_front(app: &AppHandle, tab: Option<u32>) -> Result<(u32, String, String), String> {
    let id = {
        let state = app.state::<Browser>();
        let kept = state.0.lock().unwrap();
        tab.filter(|t| kept.list.iter().any(|x| x.id == *t)).or_else(|| kept.current().map(|t| t.id)).ok_or("No page is open.")?
    };
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
pub async fn browser_logins(app: AppHandle, tab: Option<u32>) -> Result<Vec<Login>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Ok((_, _, host)) = page_in_front(&app, tab) else { return Ok(Vec::new()) };
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
pub async fn browser_fill(app: AppHandle, id: u64, tab: Option<u32>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (tab, _, host) = page_in_front(&app, tab)?;
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

/// How long a copied password stays on the clipboard.
pub const CLIPBOARD_SECS: u64 = 30;

/// Whether the clipboard is cleared when the time is up (pure): only while it still holds the
/// password Castle put there, never something the member copied since.
pub fn clears(now_on_clipboard: Option<&str>, copied: &str) -> bool {
    now_on_clipboard == Some(copied)
}

/// The Passwords sheet's Add a login: sealed under the site of the address typed.
#[tauri::command]
pub async fn vault_add(app: AppHandle, url: String, username: String, password: String) -> Result<(), String> {
    if password.is_empty() {
        return Err("Type the password to seal.".into());
    }
    let host = vault::host_of_typed(&url).ok_or("Type the site's address, like example.com.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = conn(&app.state::<SettingsStore>())?;
        vault::save(&mut c, &vault::key()?, &host, username.trim(), &password)
    })
    .await
    .map_err(|_| "The login could not be sealed.".to_string())?
}

/// The Passwords sheet's Copy: the password on the clipboard for 30 seconds, then cleared if it is
/// still the one there. It never reaches a page, a log or the companion.
#[tauri::command]
pub async fn vault_copy(app: AppHandle, id: u64) -> Result<(), String> {
    let password = tauri::async_runtime::spawn_blocking(move || {
        let mut c = conn(&app.state::<SettingsStore>())?;
        vault::secret_of(&mut c, &vault::key()?, id)
    })
    .await
    .map_err(|_| "The password could not be opened.".to_string())??;
    crate::clipboard::write_text(&password)?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(CLIPBOARD_SECS));
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if clears(clipboard.get_text().ok().as_deref(), &password) {
                let _ = clipboard.set_text(String::new());
            }
        }
    });
    Ok(())
}

/// The Passwords sheet's Import from Chrome: the exported file sealed and shredded, the welcome's
/// answers left alone. None when the member closed the picker.
#[tauri::command]
pub async fn vault_import(app: AppHandle) -> Result<Option<crate::chrome_import::PasswordsDone>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app.dialog().file().set_title("Choose Chrome's password export").add_filter("Chrome Passwords", &["csv"]).blocking_pick_file().and_then(|p| p.into_path().ok());
        let Some(path) = picked else { return Ok(None) };
        let mut c = conn(&app.state::<SettingsStore>())?;
        crate::chrome_import::import_passwords(&mut c, &vault::key()?, &path).map(Some)
    })
    .await
    .map_err(|_| "The passwords could not be brought over.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_password_is_cleared_only_while_it_is_still_there() {
        assert!(clears(Some("hunter2"), "hunter2"));
        assert!(!clears(Some("something I copied since"), "hunter2"), "the member's own copy stays");
        assert!(!clears(None, "hunter2"));
        assert_eq!(CLIPBOARD_SECS, 30);
    }

    #[test]
    fn a_login_is_kept_under_the_site_typed() {
        assert_eq!(vault::host_of_typed("https://www.github.com/login?x=1").as_deref(), Some("github.com"));
        assert_eq!(vault::host_of_typed("accounts.example.org").as_deref(), Some("accounts.example.org"));
        assert_eq!(vault::host_of_typed("http://me@shop.example:8080/").as_deref(), Some("shop.example"));
        assert_eq!(vault::host_of_typed("not a site"), None);
        assert_eq!(vault::host_of_typed(""), None);
    }

    #[test]
    fn the_fill_script_carries_the_values_as_strings_never_as_code() {
        let s = fill_script("ari", "p\"); alert(1); (\"");
        assert!(s.contains(r#"const user = "ari";"#));
        assert!(s.contains(r#"set(p, "p\"); alert(1); (\"")"#), "a quote in a password stays inside its string");
        assert!(s.starts_with("(() => {") && s.ends_with("})()"));
        assert!(fill_script("", "x").contains("const user = \"\";"));
    }
}
