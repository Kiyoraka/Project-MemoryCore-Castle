//! Grok Build's own files, read only: its sign-in and the record of each session.
//! A session lives at `~/.grok/sessions/<folder key>/<session id>/updates.jsonl`; `GROK_HOME`
//! moves `~/.grok`. The folder key is Grok's own spelling of the working folder, so a session is
//! found by its id under any key, never by guessing the key. Chat's reader joins this module in
//! Phase 6, once the record's lines have been read on a real install (docs/provider-records.md).

use std::path::{Path, PathBuf};

/// Grok's folder: `GROK_HOME` when set, else `~/.grok`.
pub fn home() -> Option<PathBuf> {
    match std::env::var_os("GROK_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => Some(crate::env::home_dir()?.join(".grok")),
    }
}

/// The file Grok keeps once the member has signed in through the browser.
pub fn auth_file() -> Option<PathBuf> {
    Some(home()?.join("auth.json"))
}

/// Signed in: the auth file is there, or an xAI key is set in the member's own environment.
pub fn signed_in() -> bool {
    signed_in_with(auth_file().as_deref(), std::env::var_os("XAI_API_KEY").is_some_and(|v| !v.is_empty()))
}

fn signed_in_with(auth_file: Option<&Path>, has_key: bool) -> bool {
    has_key || auth_file.is_some_and(Path::is_file)
}

/// The record of the session with this id, under whichever folder key it was written.
pub fn record_with_id(id: &str) -> Option<PathBuf> {
    record_in(&home()?.join("sessions"), id)
}

fn record_in(sessions: &Path, id: &str) -> Option<PathBuf> {
    if !crate::resume::is_id(id) {
        return None;
    }
    std::fs::read_dir(sessions)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(id).join("updates.jsonl"))
        .find(|path| path.is_file())
}

/// The session id a record carries: the name of the folder it sits in.
pub fn id_of_record(record: &Path) -> Option<String> {
    let id = record.parent()?.file_name()?.to_str()?;
    crate::resume::is_id(id).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "6fa92052-0000-4000-8000-000000000001";

    #[test]
    fn a_session_is_found_by_its_id_under_any_folder_key() {
        let root = std::env::temp_dir().join(format!("castle-grok-{}", std::process::id()));
        let record = root.join("D--work-memory").join(ID).join("updates.jsonl");
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, "{}\n").unwrap();
        std::fs::create_dir_all(root.join("other-folder")).unwrap();
        assert_eq!(record_in(&root, ID), Some(record.clone()));
        assert_eq!(record_in(&root, "6fa92052-0000-4000-8000-000000000002"), None);
        assert_eq!(record_in(&root, "../escape"), None, "only an id is looked up");
        assert_eq!(id_of_record(&record).as_deref(), Some(ID));
        assert_eq!(id_of_record(Path::new("C:/g/sessions/key/notes/updates.jsonl")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn signed_in_by_the_auth_file_or_the_key() {
        let root = std::env::temp_dir().join(format!("castle-grok-auth-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("auth.json");
        assert!(!signed_in_with(Some(&file), false));
        assert!(signed_in_with(Some(&file), true), "XAI_API_KEY alone is enough");
        std::fs::write(&file, "{}").unwrap();
        assert!(signed_in_with(Some(&file), false));
        assert!(!signed_in_with(None, false));
        let _ = std::fs::remove_dir_all(&root);
    }
}
