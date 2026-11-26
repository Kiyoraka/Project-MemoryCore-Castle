//! The Worldsmith helper on the member's computer: where it lives, whether it is the release, and
//! fetching it on the member's own press (the `node_runtime.rs` way: a pinned SHA-256, a resumable
//! download, a mismatch thrown away). Nothing is fetched or started before that press.

use crate::fetch;
use crate::node_runtime::Release;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// The helper's file name, the same in Castle, the Keep and the release.
pub const FILE: &str = "worldsmith-helper.exe";
/// The helper release Castle takes (its own tag, `worldsmith-helper-v<VERSION>`).
pub const VERSION: &str = "1.0.0";
/// The pinned release: None until the first `worldsmith-helper-v1.0.0` release is published, then its
/// SHA-256 and size go here. Until then Get it says so and fetches nothing.
pub const RELEASE: Option<Release> = None;
/// A local build of the helper, for development before the release exists (debug builds only).
pub const OVERRIDE_VAR: &str = "CASTLE_WORLDSMITH_HELPER";
/// What the room's session reads to find the helper (the Keep's room reads the same name).
pub const HELPER_VAR: &str = "WORLDSMITH_HELPER";
/// The event the page hears while the helper downloads: { done, total }.
pub const INSTALL_EVENT: &str = "castle://worldsmith-install";

pub const NOT_RELEASED: &str = "The Worldsmith helper isn't released yet, so nothing was downloaded.";
pub const NOT_GOT: &str = "Get the Worldsmith helper first.";
pub const WINDOWS_ONLY: &str = "WORLDSMITH works on Windows only.";

fn url_of(release: &Release) -> String {
    format!("https://github.com/Kiyoraka/Project-MemoryCore-Castle/releases/download/worldsmith-helper-v{VERSION}/{}", release.file)
}

/// Castle's own Worldsmith folder for this helper version.
pub fn home() -> PathBuf {
    // platform: windows-only - WORLDSMITH runs on Windows only: elsewhere this names a folder in
    // the temp folder that nothing installs to (install and start refuse with WINDOWS_ONLY)
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    local.join("MemoryCore Castle").join("worldsmith").join(VERSION)
}

/// The local build a developer named, in a debug build only. Pure on its input.
pub fn local_build(named: Option<String>, debug: bool) -> Option<PathBuf> {
    named.filter(|v| debug && !v.trim().is_empty()).map(|v| PathBuf::from(v.trim()))
}

fn local() -> Option<PathBuf> {
    local_build(std::env::var(OVERRIDE_VAR).ok(), cfg!(debug_assertions))
}

/// Where the helper is: the local build when one is named, else Castle's own folder.
pub fn path() -> PathBuf {
    local().unwrap_or_else(|| home().join(FILE))
}

/// Whether `file` is the pinned release (or the named local build). Checked before every start, so a
/// file an antivirus swapped or a half-written one is never run.
pub fn trusted_at(file: &Path, release: Option<&Release>, is_local: bool) -> bool {
    if !file.is_file() {
        return false;
    }
    if is_local {
        return true;
    }
    match release {
        Some(r) => std::fs::read(file).map(|bytes| fetch::matches(&bytes, r.sha256)).unwrap_or(false),
        None => false,
    }
}

/// The helper ready to run, or why not (one plain line).
pub fn ready() -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err(WINDOWS_ONLY.into());
    }
    let file = path();
    if trusted_at(&file, RELEASE.as_ref(), local().is_some()) {
        Ok(file)
    } else {
        Err(NOT_GOT.into())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperState {
    pub windows: bool,
    pub released: bool,
    pub local: bool,
    pub installed: bool,
    pub running: bool,
}

pub fn state(running: bool) -> HelperState {
    HelperState { windows: cfg!(windows), released: RELEASE.is_some(), local: local().is_some(), installed: ready().is_ok(), running }
}

/// The member pressed Get it: the release downloaded, kept only when it is exactly the pinned file.
pub fn get(app: &AppHandle) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(WINDOWS_ONLY.into());
    }
    if local().is_some() {
        return Ok(());
    }
    let Some(release) = RELEASE else { return Err(NOT_RELEASED.into()) };
    let dir = home();
    std::fs::create_dir_all(&dir).map_err(|_| "A folder for the Worldsmith helper could not be made.".to_string())?;
    let part = dir.join(format!("{FILE}.part"));
    let events = app.clone();
    let bytes = fetch::download_resumable(&url_of(&release), release.bytes, "The Worldsmith helper", &part, move |done, total| {
        let _ = events.emit(INSTALL_EVENT, serde_json::json!({ "done": done, "total": total }));
    })?;
    if !fetch::matches(&bytes, release.sha256) {
        fetch::discard(&part);
        return Err("The download did not match the Worldsmith helper's checksum, so Castle threw it away.".into());
    }
    let temp = dir.join(format!("{FILE}.partial"));
    std::fs::write(&temp, &bytes).map_err(|_| "The Worldsmith helper could not be saved.".to_string())?;
    std::fs::rename(&temp, dir.join(FILE)).map_err(|_| "The Worldsmith helper could not be saved.".to_string())?;
    fetch::discard(&part);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_build_counts_only_in_a_debug_build_and_only_when_named() {
        assert_eq!(local_build(Some("D:\\build\\worldsmith-helper.exe".into()), true), Some(PathBuf::from("D:\\build\\worldsmith-helper.exe")));
        assert_eq!(local_build(Some("D:\\build\\worldsmith-helper.exe".into()), false), None, "a release build never runs a named file");
        assert_eq!(local_build(Some("   ".into()), true), None);
        assert_eq!(local_build(None, true), None);
    }

    #[test]
    fn only_the_pinned_file_is_trusted() {
        let dir = std::env::temp_dir().join(format!("castle-worldsmith-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(FILE);
        std::fs::write(&file, b"the helper").unwrap();
        let right = Release { file: FILE, sha256: "ignored", bytes: 10 };
        let sha = fetch::sha256_hex(b"the helper");
        let pinned = Release { sha256: Box::leak(sha.into_boxed_str()), ..right };
        assert!(trusted_at(&file, Some(&pinned), false));
        assert!(!trusted_at(&file, Some(&right), false), "a file that is not the release is never run");
        assert!(!trusted_at(&file, None, false), "nothing is trusted before a release is pinned");
        assert!(trusted_at(&file, None, true), "a developer's named local build is");
        assert!(!trusted_at(&dir.join("missing.exe"), Some(&pinned), false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_release_comes_from_the_helpers_own_tag() {
        let r = Release { file: FILE, sha256: "x", bytes: 1 };
        assert_eq!(url_of(&r), "https://github.com/Kiyoraka/Project-MemoryCore-Castle/releases/download/worldsmith-helper-v1.0.0/worldsmith-helper.exe");
    }
}
