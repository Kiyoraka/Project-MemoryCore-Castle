//! The update door. An official build (made by the release workflow or scripts/release.ps1 with
//! CASTLE_OFFICIAL set at build time) updates itself from the signed latest.json on the project's
//! GitHub release. A member's own build never updates itself, so it can never overwrite their
//! changes - it is only told that a newer release is out (the GitHub release list, read at most
//! every six hours). An update replaces the app only - the memory folder, settings, themes and
//! keys are never touched. The page saves the diary first (it asks the companion), then calls
//! update_install.

use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::AppHandle;
#[cfg(feature = "official")]
use tauri_plugin_updater::UpdaterExt;

/// True for an official release build.
pub fn official() -> bool {
    option_env!("CASTLE_OFFICIAL").is_some_and(|v| v == "1")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub official: bool,
    pub version: String,
    /// the newer version, when there is one
    pub available: Option<String>,
    pub notes: Option<String>,
    pub message: String,
    /// true when Castle can install the newer version itself (official builds only)
    pub can_install: bool,
    /// the release's page, for "read more"
    pub release_url: Option<String>,
}

/// Every release, newest first, on the project's GitHub page.
pub const RELEASES_PAGE: &str = "https://github.com/Kiyoraka/Project-MemoryCore-Castle/releases";
const LATEST_RELEASE: &str = "https://api.github.com/repos/Kiyoraka/Project-MemoryCore-Castle/releases/latest";

/// "0.2.0" or "v0.2.0" -> (0, 2, 0).
fn parts(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    let out = (it.next()??, it.next()??, it.next()??);
    if it.next().is_some() {
        return None;
    }
    Some(out)
}

/// True when `a` is a newer version than `b`.
pub fn newer(a: &str, b: &str) -> bool {
    matches!((parts(a), parts(b)), (Some(x), Some(y)) if x > y)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: Option<String>,
    pub url: Option<String>,
}

/// GitHub's answer for the latest release; drafts and pre-releases never count.
pub fn parse_release(answer: &Value) -> Option<Release> {
    if answer.get("draft").and_then(Value::as_bool) == Some(true) || answer.get("prerelease").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let tag = answer.get("tag_name")?.as_str()?;
    parts(tag)?;
    Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: answer.get("body").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()).map(String::from),
        url: answer.get("html_url").and_then(Value::as_str).filter(|u| u.starts_with("https://")).map(String::from),
    })
}

static SOURCE_CHECK: Mutex<Option<(Instant, Option<Release>)>> = Mutex::new(None);

/// The latest release for a member's own build, read at most every six hours (a failed read is
/// tried again after ten minutes).
fn latest_release(version: &str) -> Option<Release> {
    if let Some((at, release)) = SOURCE_CHECK.lock().unwrap().as_ref() {
        let keep = if release.is_some() { Duration::from_secs(6 * 3600) } else { Duration::from_secs(600) };
        if at.elapsed() < keep {
            return release.clone();
        }
    }
    let read = || -> Option<Release> {
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build().ok()?;
        let answer = client
            .get(LATEST_RELEASE)
            .header("User-Agent", format!("MemoryCoreCastle/{version}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .ok()?;
        if !answer.status().is_success() {
            return None;
        }
        parse_release(&answer.json::<Value>().ok()?)
    };
    let release = read();
    *SOURCE_CHECK.lock().unwrap() = Some((Instant::now(), release.clone()));
    release
}

/// What a member's own build says about the latest release.
pub fn source_status(version: &str, release: Option<Release>) -> UpdateStatus {
    let own = |message: &str| UpdateStatus { official: false, version: version.to_string(), available: None, notes: None, message: message.into(), can_install: false, release_url: None };
    match release {
        Some(r) if newer(&r.version, version) => UpdateStatus {
            official: false,
            version: version.to_string(),
            message: format!("Castle {} is out - your own build updates with git pull (see MAKE-IT-YOURS.md).", r.version),
            available: Some(r.version),
            notes: r.notes,
            can_install: false,
            release_url: r.url,
        },
        Some(_) => own("Your own build has the newest release. Pull new code with git (see MAKE-IT-YOURS.md)."),
        None => own("Updates: your own build - pull new code with git (see MAKE-IT-YOURS.md)."),
    }
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateStatus {
    let version = env!("CARGO_PKG_VERSION").to_string();
    if !official() {
        let v = version.clone();
        let release = tauri::async_runtime::spawn_blocking(move || latest_release(&v)).await.ok().flatten();
        return source_status(&version, release);
    }
    check(&app, version).await
}

#[cfg(feature = "official")]
async fn check(app: &AppHandle, version: String) -> UpdateStatus {
    let status = |available: Option<String>, notes: Option<String>, message: String| UpdateStatus {
        official: true,
        version: version.clone(),
        can_install: available.is_some(),
        release_url: available.as_ref().map(|v| format!("{RELEASES_PAGE}/tag/v{v}")),
        available,
        notes,
        message,
    };
    let Ok(updater) = app.updater() else { return status(None, None, "Updates could not be checked.".into()) };
    match updater.check().await {
        Ok(Some(update)) => status(Some(update.version.clone()), update.body.clone(), format!("Update ready: {}", update.version)),
        Ok(None) => status(None, None, "Castle is up to date.".into()),
        Err(_) => status(None, None, "Updates could not be checked right now.".into()),
    }
}

#[cfg(not(feature = "official"))]
async fn check(_app: &AppHandle, version: String) -> UpdateStatus {
    UpdateStatus { official: true, version, available: None, notes: None, message: "This build was made without the updater.".into(), can_install: false, release_url: None }
}

/// Downloads, checks the signature, installs and restarts. The page has saved the diary first.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    if !official() {
        return Err("Your own build does not update itself.".into());
    }
    install(&app).await
}

#[cfg(feature = "official")]
async fn install(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|_| "Updates could not be started.".to_string())?;
    let update = updater.check().await.map_err(|_| "Updates could not be checked.".to_string())?.ok_or("Castle is already up to date.")?;
    // the installer may end Castle before any exit event: the tabs are kept as they stand now
    crate::browser_session::leave();
    if update.download_and_install(|_, _| {}, || {}).await.is_err() {
        crate::browser_session::stay();
        return Err("The update could not be installed. Castle stays as it is.".to_string());
    }
    app.restart();
}

#[cfg(not(feature = "official"))]
async fn install(_app: &AppHandle) -> Result<(), String> {
    Err("This build was made without the updater.".into())
}

/// The public releases page on the MemoryCore Foundation's site, from its latest-release answer
/// (an absolute https pageUrl), or None.
pub fn page_from(answer: &Value) -> Option<String> {
    answer.get("pageUrl")?.as_str().filter(|u| u.starts_with("https://") && !u.contains(char::is_whitespace)).map(String::from)
}

static RELEASES_PAGE_FOUND: Mutex<Option<String>> = Mutex::new(None);

/// Where "All releases" opens: the Foundation's releases page once it answers, else GitHub's.
/// Asked once per launch.
#[tauri::command]
pub async fn releases_page() -> String {
    if let Some(found) = RELEASES_PAGE_FOUND.lock().unwrap().clone() {
        return found;
    }
    let found = tauri::async_runtime::spawn_blocking(|| -> Option<String> {
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(8)).build().ok()?;
        let answer = client
            .get(format!("{}/api/castle/releases/latest", crate::signin::PORTFOLIO))
            .header("User-Agent", format!("MemoryCoreCastle/{}", env!("CARGO_PKG_VERSION")))
            .send()
            .ok()?;
        if !answer.status().is_success() {
            return None;
        }
        page_from(&answer.json::<Value>().ok()?)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| RELEASES_PAGE.to_string());
    *RELEASES_PAGE_FOUND.lock().unwrap() = Some(found.clone());
    found
}

/// Remembers the version whose "What's new" the member has seen; gives back the one before.
#[tauri::command]
pub fn version_seen(version: String, store: tauri::State<crate::settings::SettingsStore>) -> Result<Option<String>, String> {
    let mut settings = store.get();
    let before = settings.last_seen_version.clone();
    // never written backwards: an older Castle opening a newer one's settings leaves the newer version
    if before.as_deref().is_some_and(|seen| crate::settings::version_newer(seen, &version)) {
        return Ok(before);
    }
    settings.last_seen_version = Some(version);
    let settings = settings.normalized();
    crate::settings::save(&store.path, &settings).map_err(|_| "The settings could not be saved.".to_string())?;
    *store.current.lock().unwrap() = settings;
    Ok(before)
}

/// The newest change time of the diary (seconds since 1970), so the page can wait for Save diary.
#[tauri::command]
pub fn diary_stamp(store: tauri::State<crate::settings::SettingsStore>) -> u64 {
    let Some(folder) = store.get().memory_folder else { return 0 };
    // a memory in the database: the newest change among the diary's rows
    if let Some(mut conn) = crate::store::source(std::path::Path::new(&folder)) {
        use mysql::prelude::Queryable;
        return conn.query_first::<Option<u64>, _>("SELECT UNIX_TIMESTAMP(MAX(updated_at)) FROM diary_entries").ok().flatten().flatten().unwrap_or(0);
    }
    crate::memory::diary_files(std::path::Path::new(&folder))
        .iter()
        .filter_map(|p| p.metadata().and_then(|m| m.modified()).ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_test_build_is_never_official() {
        assert!(!super::official());
    }

    #[test]
    fn versions_are_compared_by_number() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(!newer("0.2", "0.1.0"));
        assert!(!newer("0.2.0.1", "0.1.0"));
        assert!(!newer("latest", "0.1.0"));
    }

    #[test]
    fn the_latest_release_is_read_from_github() {
        let answer = json!({ "tag_name": "v0.2.0", "body": " - the room\n", "html_url": "https://github.com/x/y/releases/tag/v0.2.0", "draft": false, "prerelease": false });
        assert_eq!(parse_release(&answer), Some(Release { version: "0.2.0".into(), notes: Some("- the room".into()), url: Some("https://github.com/x/y/releases/tag/v0.2.0".into()) }));
        assert_eq!(parse_release(&json!({ "tag_name": "v0.3.0", "prerelease": true })), None);
        assert_eq!(parse_release(&json!({ "tag_name": "nightly" })), None);
        assert_eq!(parse_release(&json!({ "message": "Not Found" })), None);
        let bare = parse_release(&json!({ "tag_name": "0.2.1", "body": "", "html_url": "javascript:x" })).unwrap();
        assert_eq!((bare.notes, bare.url), (None, None));
    }

    #[test]
    fn the_foundation_page_is_used_only_when_it_is_a_safe_address() {
        assert_eq!(page_from(&json!({ "version": "0.2.0", "pageUrl": "https://example.org/castle" })).as_deref(), Some("https://example.org/castle"));
        assert_eq!(page_from(&json!({ "pageUrl": "http://example.org/castle" })), None);
        assert_eq!(page_from(&json!({ "pageUrl": "https://x.org/a b" })), None);
        assert_eq!(page_from(&json!({ "version": "0.2.0" })), None);
    }

    #[test]
    fn a_members_own_build_is_told_but_never_installs() {
        let release = Release { version: "0.2.0".into(), notes: Some("- new".into()), url: None };
        let told = source_status("0.1.0", Some(release.clone()));
        assert_eq!(told.available.as_deref(), Some("0.2.0"));
        assert!(!told.can_install && !told.official);
        assert!(told.message.contains("0.2.0 is out"));
        assert_eq!(source_status("0.2.0", Some(release)).available, None);
        assert_eq!(source_status("0.1.0", None).available, None);
    }
}
