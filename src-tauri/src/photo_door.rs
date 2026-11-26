//! PHOTO and DEV, wired. Two ways a project's picture reaches PHOTO and goes back:
//! - the member's hand: "→photo" on a picture in a DEV tab's FILES opens it in PHOTO, which then
//!   offers "Save back to <file>".
//! - a project's terminal, through Castle's single-instance door: a second launch with
//!   `--photo-edit "<file>" --say "<words>" --ask <id>`. The running Castle opens the file in PHOTO,
//!   the companion edits it with the native tools only (never the image provider), it is saved
//!   back, and the answer is written to `photo-door/<id>.json` in Castle's settings folder
//!   (`{ id, file, state: working | edited | refused, say, reason }`), which the terminal reads.
//! Only a png, jpg or webp inside a project folder open in a DEV tab is ever touched, and the first
//! save back keeps the old file beside it as `<name>.bak.<ext>`. A request waits here until PHOTO
//! takes it (PHOTO may not be open yet when it arrives), and the page handles each one once.

use crate::settings::SettingsStore;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::photo::ROOM;

/// The event the page hears when a request arrives while PHOTO is open.
pub const REQUEST_EVENT: &str = "photo-request";
/// The folder in Castle's settings folder the terminal's answers are written to.
pub const ANSWERS: &str = "photo-door";

/// The request waiting for PHOTO to take it: `{ kind: open | door, path, saveBack, id?, say?, at }`.
static WAITING: Mutex<Option<Value>> = Mutex::new(None);
static STAMP: AtomicU64 = AtomicU64::new(0);

fn picture_ext(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()).then_some(ext)
}

/// Whether the file sits inside one of these folders (both sides compared the way DEV compares
/// project folders: one slash style, and without case on Windows). Pure.
pub fn inside_any(file: &Path, folders: &[PathBuf]) -> bool {
    let file = crate::dev_tabs::same_folder_key(&file.display().to_string());
    folders.iter().map(|f| crate::dev_tabs::same_folder_key(&f.display().to_string())).filter(|f| !f.is_empty()).any(|f| file.starts_with(&format!("{f}/")))
}

/// Where the old picture is kept: `<stem>.bak.<ext>` beside it. Pure.
pub fn bak_path(file: &Path) -> Option<PathBuf> {
    let stem = file.file_stem()?.to_str()?;
    let ext = file.extension()?.to_str()?;
    Some(file.with_file_name(format!("{stem}.bak.{ext}")))
}

/// Why this file may not be edited from here, or None (pure apart from the file's existence).
pub fn refusal(file: &Path, projects: &[PathBuf]) -> Option<&'static str> {
    if picture_ext(file).is_none() {
        return Some("Only a png, jpg or webp picture can be edited in PHOTO.");
    }
    if file.display().to_string().contains("..") || !inside_any(file, projects) {
        return Some("That picture is not inside a project open in a DEV tab.");
    }
    if !file.is_file() {
        return Some("That picture could not be found.");
    }
    None
}

fn open_projects(store: &SettingsStore) -> Vec<PathBuf> {
    store.get().dev_tabs.into_iter().flatten().map(PathBuf::from).collect()
}

/// A door ask's id: letters, digits and `-`, at most 40 (it names the answer file). Pure.
pub fn clean_ask(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// The door word read from a launch's arguments: `--photo-edit <file> --say <words> --ask <id>`.
/// None when the launch carries no `--photo-edit`. Pure.
pub fn door_word(args: &[String]) -> Option<(String, String, String)> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(|v| v.trim().trim_matches('"').to_string());
    let file = value("--photo-edit")?;
    Some((file, value("--say").unwrap_or_default(), value("--ask").unwrap_or_default()))
}

/// The answer the terminal reads. Pure.
pub fn answer(id: &str, file: &str, state: &str, say: Option<&str>, reason: Option<&str>) -> Value {
    json!({
        "id": id,
        "file": file,
        "state": state,
        "say": say.map(|s| s.chars().take(500).collect::<String>()),
        "reason": reason.map(|s| s.chars().take(500).collect::<String>()),
    })
}

fn write_answer(folder: &Path, id: &str, value: &Value) -> Result<(), String> {
    let id = clean_ask(id).ok_or("That ask's id is not plain.")?;
    let dir = folder.join(ANSWERS);
    std::fs::create_dir_all(&dir).map_err(|_| "The answer folder could not be made.".to_string())?;
    let text = serde_json::to_vec_pretty(value).map_err(|_| "The answer could not be written.".to_string())?;
    std::fs::write(dir.join(format!("{id}.json")), text).map_err(|_| "The answer could not be written.".to_string())
}

fn next_stamp() -> u64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    // always rising, even for two requests in the same millisecond
    let mut last = STAMP.load(Ordering::SeqCst);
    loop {
        let next = now.max(last + 1);
        match STAMP.compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return next,
            Err(seen) => last = seen,
        }
    }
}

/// A request handed to PHOTO: kept until the page takes it, heard at once if PHOTO is open, and
/// PHOTO brought to the front.
fn hand_over(app: &AppHandle, mut request: Value) {
    request["at"] = json!(next_stamp());
    *WAITING.lock().unwrap() = Some(request.clone());
    let _ = app.emit(REQUEST_EVENT, request);
    crate::roomalert::open_room(app, ROOM);
}

/// The single-instance door: a second launch carrying `--photo-edit` is checked, answered
/// "working" (or "refused") and handed to PHOTO; the page answers the rest through `photo_door_done`.
pub fn from_args(app: &AppHandle, args: &[String]) {
    let Some((file, say, ask)) = door_word(args) else { return };
    let Some(id) = clean_ask(&ask) else { return };
    let store = app.state::<SettingsStore>();
    let folder = store.folder();
    let path = PathBuf::from(file.trim());
    let why = if crate::rooms::need(&store, ROOM).is_err() {
        Some("PHOTO is switched off in Settings > Rooms.")
    } else if say.trim().is_empty() {
        Some("Say what to change with --say.")
    } else {
        refusal(&path, &open_projects(&store))
    };
    let shown = path.display().to_string();
    if let Some(why) = why {
        let _ = write_answer(&folder, &id, &answer(&id, &shown, "refused", None, Some(why)));
        return;
    }
    let _ = write_answer(&folder, &id, &answer(&id, &shown, "working", None, None));
    hand_over(app, json!({ "kind": "door", "path": shown, "saveBack": true, "id": id, "say": say.trim() }));
}

/// "→photo" in DEV's FILES: the picture opens in PHOTO with "Save back to <file>".
#[tauri::command]
pub fn photo_open_from_dev(app: AppHandle, path: String, store: State<'_, SettingsStore>) -> Result<(), String> {
    crate::rooms::need(&store, ROOM)?;
    let file = PathBuf::from(path.trim());
    if let Some(why) = refusal(&file, &open_projects(&store)) {
        return Err(why.into());
    }
    hand_over(&app, json!({ "kind": "open", "path": file.display().to_string(), "saveBack": true }));
    Ok(())
}

/// The request waiting for PHOTO, taken once (None when there is none).
#[tauri::command]
pub fn photo_request_take(store: State<'_, SettingsStore>) -> Result<Option<Value>, String> {
    crate::rooms::need(&store, ROOM)?;
    Ok(WAITING.lock().unwrap().take())
}

/// The page finished a door edit: the answer the terminal reads.
#[tauri::command]
pub fn photo_door_done(id: String, file: String, ok: bool, say: Option<String>, reason: Option<String>, store: State<'_, SettingsStore>) -> Result<(), String> {
    let value = if ok {
        answer(&id, &file, "edited", Some(say.as_deref().unwrap_or("Edited and saved back.")), None)
    } else {
        answer(&id, &file, "refused", None, Some(reason.as_deref().unwrap_or("The edit could not be made.")))
    };
    write_answer(&store.folder(), &id, &value)
}

/// Save back (the member's press, or a door edit): the new picture written over the project's file,
/// the old one kept once as `<name>.bak.<ext>`. The bytes must be the file's own kind (the page
/// encodes it). -> the file's path.
#[tauri::command]
pub fn photo_save_back(path: String, image: String, store: State<'_, SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    let file = PathBuf::from(path.trim());
    if let Some(why) = refusal(&file, &open_projects(&store)) {
        return Err(why.into());
    }
    save_back_in(&file, &image)
}

/// The save back itself, once the file is known to be fine.
pub fn save_back_in(file: &Path, image: &str) -> Result<String, String> {
    let ext = picture_ext(file).ok_or("Only a png, jpg or webp picture can be saved back.")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(image.trim()).map_err(|_| "The picture could not be read.".to_string())?;
    if !crate::photo::bytes_match(&ext, &bytes) {
        return Err(format!("The picture did not come as a {ext}."));
    }
    if let Some(bak) = bak_path(file) {
        if !bak.exists() {
            std::fs::copy(file, &bak).map_err(|_| "The old picture could not be kept as a .bak, so nothing was changed.".to_string())?;
        }
    }
    std::fs::write(file, bytes).map_err(|_| "The picture could not be saved back.".to_string())?;
    Ok(file.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_counts_only_inside_an_open_project() {
        let projects = vec![PathBuf::from("D:\\Work\\Kopi Saigon\\"), PathBuf::from("D:/Work/Baek")];
        assert!(inside_any(Path::new("D:\\Work\\Kopi Saigon\\public\\logo.png"), &projects));
        assert!(inside_any(Path::new("D:/Work/Baek/a.jpg"), &projects));
        assert!(!inside_any(Path::new("D:\\Work\\Kopi Saigon Two\\logo.png"), &projects), "a folder sharing a prefix is not inside");
        assert!(!inside_any(Path::new("C:\\Windows\\x.png"), &projects));
        assert!(!inside_any(Path::new("D:\\Work\\Baek"), &projects), "the folder itself is not a file inside it");
    }

    #[test]
    fn only_pictures_inside_a_project_are_edited() {
        let projects = vec![PathBuf::from("D:\\Work\\Kopi")];
        assert!(refusal(Path::new("D:\\Work\\Kopi\\notes.txt"), &projects).unwrap().contains("png, jpg or webp"));
        assert!(refusal(Path::new("D:\\Elsewhere\\a.png"), &projects).unwrap().contains("not inside"));
        assert!(refusal(Path::new("D:\\Work\\Kopi\\..\\..\\Windows\\a.png"), &projects).unwrap().contains("not inside"));
        assert!(refusal(Path::new("D:\\Work\\Kopi\\missing-for-sure-123.png"), &projects).unwrap().contains("could not be found"));
    }

    #[test]
    fn the_door_word_is_read_from_the_launch() {
        let args: Vec<String> = ["castle.exe", "--photo-edit", "\"D:\\W\\logo.png\"", "--say", "warmer please", "--ask", "a1-b2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(door_word(&args), Some(("D:\\W\\logo.png".into(), "warmer please".into(), "a1-b2".into())));
        assert_eq!(door_word(&["castle.exe".to_string(), "memorycore-castle://chat".to_string()]), None);
        assert_eq!(clean_ask("a1-b2").as_deref(), Some("a1-b2"));
        assert!(clean_ask("../x").is_none() && clean_ask("").is_none() && clean_ask(&"a".repeat(41)).is_none());
        let a = answer("a1", "D:\\W\\logo.png", "refused", None, Some("no"));
        assert_eq!((a["state"].as_str(), a["reason"].as_str(), a["say"].clone()), (Some("refused"), Some("no"), Value::Null));
    }

    #[test]
    fn the_old_picture_is_kept_once_and_bytes_must_match() {
        assert_eq!(bak_path(Path::new("D:\\W\\logo.png")).unwrap(), PathBuf::from("D:\\W\\logo.bak.png"));
        let root = std::env::temp_dir().join(format!("castle-photo-back-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("logo.png");
        std::fs::write(&file, b"\x89PNGold").unwrap();
        let b = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        assert!(save_back_in(&file, &b(&[0xFF, 0xD8, 0])).is_err(), "a jpeg is not written over a png");
        assert!(!root.join("logo.bak.png").exists());
        save_back_in(&file, &b(b"\x89PNGnew1")).unwrap();
        save_back_in(&file, &b(b"\x89PNGnew2")).unwrap();
        assert_eq!(std::fs::read(root.join("logo.bak.png")).unwrap(), b"\x89PNGold", "the first save keeps the original, later ones never replace it");
        assert_eq!(std::fs::read(&file).unwrap(), b"\x89PNGnew2");
        write_answer(&root, "a1", &answer("a1", "x", "edited", Some("done"), None)).unwrap();
        assert!(root.join(ANSWERS).join("a1.json").is_file());
        assert!(write_answer(&root, "../a", &json!({})).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
