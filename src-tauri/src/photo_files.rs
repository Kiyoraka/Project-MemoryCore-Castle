//! PHOTO's files beyond one picture: Export As in many files into one folder the member picks, a
//! batch over a folder's pictures written into its own `PHOTO out` subfolder (the originals are
//! never touched), the actions the member records kept as `photo-actions.json` in Castle's settings
//! folder, and a picture's raw bytes for the page to read (a PSD is parsed in the page).

use crate::settings::SettingsStore;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

use crate::photo::ROOM;

/// The subfolder a batch writes into; the pictures it reads are never changed.
pub const BATCH_OUT: &str = "PHOTO out";
/// The actions file in Castle's settings folder.
const ACTIONS_FILE: &str = "photo-actions.json";
const PICTURES: [&str; 5] = ["png", "jpg", "jpeg", "webp", "psd"];
const FILE_CAP: usize = 200;
/// A PSD can be large; its bytes are read for the page up to this.
const BYTES_CAP: u64 = 300 * 1024 * 1024;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// A file name as written: letters, digits, spaces, `-`, `_` and `.`, no folder parts, at most 80,
/// with the format's own extension. Pure.
pub fn file_name(name: &str, format: &str) -> Option<String> {
    let ext = match format {
        "png" => "png",
        "jpeg" | "jpg" => "jpg",
        "webp" => "webp",
        _ => return None,
    };
    let stem: String = name.rsplit(['\\', '/']).next().unwrap_or("").chars().filter(|c| c.is_alphanumeric() || " -_.".contains(*c)).collect();
    let stem = stem.trim().trim_end_matches(&format!(".{ext}")).trim_matches('.').trim();
    let stem: String = stem.chars().take(80).collect();
    (!stem.is_empty() && !stem.contains("..")).then(|| format!("{stem}.{ext}"))
}

fn bytes_for(format: &str, image: &str) -> Result<Vec<u8>, String> {
    let bytes = b64().decode(image.trim()).map_err(|_| "A picture could not be read.".to_string())?;
    if crate::photo::bytes_match(format, &bytes) {
        Ok(bytes)
    } else {
        Err(format!("A picture did not come as a {format}."))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutFile {
    pub name: String,
    pub image: String,
    pub format: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutMany {
    #[serde(default)]
    pub folder: Option<String>,
    pub files: Vec<OutFile>,
}

/// Writes every file into the folder: plain names, never outside it.
pub fn write_many(folder: &Path, files: &[OutFile]) -> Result<usize, String> {
    if files.is_empty() || files.len() > FILE_CAP {
        return Err("There is nothing to export, or too much at once.".into());
    }
    // every file is checked before the first one is written
    let mut ready = Vec::new();
    for f in files {
        let name = file_name(&f.name, &f.format).ok_or("A file name is not plain.")?;
        ready.push((name, bytes_for(&f.format, &f.image)?));
    }
    std::fs::create_dir_all(folder).map_err(|_| "The export folder could not be made.".to_string())?;
    for (name, bytes) in ready {
        std::fs::write(folder.join(name), bytes).map_err(|_| "A file could not be written.".to_string())?;
    }
    Ok(files.len())
}

fn pick_folder(app: &AppHandle, title: &str) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog().file().set_title(title).blocking_pick_folder().and_then(|p| p.into_path().ok())
}

/// Export As (the member's press or the companion's op): one folder asked once, then every size or
/// artboard written into it. None when the member cancelled the folder pick.
#[tauri::command]
pub async fn photo_export_many(app: AppHandle, out: OutMany, store: State<'_, SettingsStore>) -> Result<Option<String>, String> {
    crate::rooms::need(&store, ROOM)?;
    tauri::async_runtime::spawn_blocking(move || -> Result<Option<String>, String> {
        let folder = match out.folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => PathBuf::from(f),
            None => match pick_folder(&app, "Export As - choose a folder") {
                Some(f) => f,
                None => return Ok(None),
            },
        };
        write_many(&folder, &out.files)?;
        Ok(Some(folder.display().to_string()))
    })
    .await
    .map_err(|_| "The export failed.".to_string())?
}

#[tauri::command]
pub async fn photo_pick_folder(app: AppHandle, store: State<'_, SettingsStore>) -> Result<Option<String>, String> {
    crate::rooms::need(&store, ROOM)?;
    tauri::async_runtime::spawn_blocking(move || pick_folder(&app, "Choose a folder of pictures").map(|p| p.display().to_string()))
        .await
        .map_err(|_| "The picker could not open.".to_string())
}

/// The pictures in a folder: its own files only (png, jpg, webp), sorted, at most 500.
pub fn pictures_in(folder: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(folder)
        .map(|r| {
            r.flatten()
                .filter(|e| e.path().is_file())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| PICTURES[..4].contains(&x.to_ascii_lowercase().as_str())))
                .map(|e| e.path().display().to_string())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found.truncate(500);
    found
}

#[tauri::command]
pub fn photo_batch_list(folder: String, store: State<'_, SettingsStore>) -> Result<Vec<String>, String> {
    crate::rooms::need(&store, ROOM)?;
    let folder = PathBuf::from(folder.trim());
    if !folder.is_dir() {
        return Err("That folder could not be found.".into());
    }
    Ok(pictures_in(&folder))
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchItem {
    pub folder: String,
    pub name: String,
    pub image: String,
    pub format: String,
}

/// One batch result, written into `<folder>/PHOTO out/` (never over a picture it read).
pub fn batch_write_in(item: &BatchItem) -> Result<PathBuf, String> {
    let folder = PathBuf::from(item.folder.trim());
    if !folder.is_dir() {
        return Err("That folder could not be found.".into());
    }
    let name = file_name(&item.name, &item.format).ok_or("A file name is not plain.")?;
    let bytes = bytes_for(&item.format, &item.image)?;
    let out = folder.join(BATCH_OUT);
    std::fs::create_dir_all(&out).map_err(|_| "The PHOTO out folder could not be made.".to_string())?;
    let path = out.join(name);
    std::fs::write(&path, bytes).map_err(|_| "A batch picture could not be written.".to_string())?;
    Ok(path)
}

#[tauri::command]
pub fn photo_batch_write(item: BatchItem, store: State<'_, SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    batch_write_in(&item).map(|p| p.display().to_string())
}

/// The actions as kept: a list of `{ name, steps }`, names plain and unique, at most 200 actions of
/// 500 steps. Pure.
pub fn clean_actions(actions: &Value) -> Value {
    let mut seen = std::collections::HashSet::new();
    let list: Vec<Value> = actions
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|act| {
                    let name: String = act["name"].as_str()?.chars().filter(|c| !c.is_control()).take(60).collect::<String>().trim().to_string();
                    let steps = act["steps"].as_array()?;
                    (!name.is_empty() && seen.insert(name.to_lowercase())).then(|| json!({ "name": name, "steps": steps.iter().take(500).cloned().collect::<Vec<_>>() }))
                })
                .take(200)
                .collect()
        })
        .unwrap_or_default();
    Value::Array(list)
}

pub fn actions_load_in(folder: &Path) -> Value {
    std::fs::read(folder.join(ACTIONS_FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| clean_actions(&v)).unwrap_or_else(|| json!([]))
}

pub fn actions_save_in(folder: &Path, actions: &Value) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(&clean_actions(actions)).map_err(|_| "The actions could not be written.".to_string())?;
    if text.len() > 8 * 1024 * 1024 {
        return Err("The actions are too big to keep.".into());
    }
    std::fs::create_dir_all(folder).map_err(|_| "The settings folder could not be made.".to_string())?;
    std::fs::write(folder.join(ACTIONS_FILE), text).map_err(|_| "The actions could not be saved.".to_string())
}

#[tauri::command]
pub fn photo_actions_load(store: State<'_, SettingsStore>) -> Result<Value, String> {
    crate::rooms::need(&store, ROOM)?;
    Ok(actions_load_in(&store.folder()))
}

#[tauri::command]
pub fn photo_actions_save(actions: Value, store: State<'_, SettingsStore>) -> Result<(), String> {
    crate::rooms::need(&store, ROOM)?;
    actions_save_in(&store.folder(), &actions)
}

/// A picture's raw bytes as base64 (a PSD the page parses itself): png, jpg, webp or psd, at most 300 MB.
#[tauri::command]
pub fn photo_read_bytes(path: String, store: State<'_, SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    let file = PathBuf::from(path.trim());
    let ext = file.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if !PICTURES.contains(&ext.as_str()) {
        return Err("That is not a picture PHOTO opens.".into());
    }
    let size = std::fs::metadata(&file).map_err(|_| "That file could not be found.".to_string())?.len();
    if size > BYTES_CAP {
        return Err("That file is too big (over 300 MB).".into());
    }
    std::fs::read(&file).map(|b| b64().encode(b)).map_err(|_| "That file could not be read.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> String {
        b64().encode(b"\x89PNG\r\n\x1a\nxx")
    }

    #[test]
    fn file_names_stay_plain_and_inside() {
        assert_eq!(file_name("icon-1024", "png").as_deref(), Some("icon-1024.png"));
        assert_eq!(file_name("..\\..\\evil", "png").as_deref(), Some("evil.png"));
        assert_eq!(file_name("banner.jpg", "jpeg").as_deref(), Some("banner.jpg"));
        assert_eq!(file_name("a:b*c?", "webp").as_deref(), Some("abc.webp"));
        assert!(file_name("...", "png").is_none());
        assert!(file_name("x", "gif").is_none());
    }

    #[test]
    fn many_files_go_into_one_folder_and_a_batch_never_touches_the_originals() {
        let root = std::env::temp_dir().join(format!("castle-photo-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let files = vec![OutFile { name: "icon-512".into(), image: png(), format: "png".into() }, OutFile { name: "icon-192".into(), image: png(), format: "png".into() }];
        assert_eq!(write_many(&root.join("icons"), &files).unwrap(), 2);
        assert!(root.join("icons").join("icon-192.png").is_file());
        assert!(write_many(&root, &[OutFile { name: "x".into(), image: png(), format: "jpeg".into() }]).is_err(), "png bytes are not a jpeg");
        assert!(!root.join("x.jpg").exists(), "nothing is written when a file is refused");
        std::fs::write(root.join("a.png"), b"\x89PNGorig").unwrap();
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        std::fs::write(root.join("deep.psd"), b"8BPS").unwrap();
        assert_eq!(pictures_in(&root).len(), 1, "only its own pictures, never a psd or a note");
        let out = batch_write_in(&BatchItem { folder: root.display().to_string(), name: "a".into(), image: png(), format: "png".into() }).unwrap();
        assert_eq!(out, root.join(BATCH_OUT).join("a.png"));
        assert_eq!(std::fs::read(root.join("a.png")).unwrap(), b"\x89PNGorig", "the original is untouched");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn actions_are_kept_plain_and_unique_in_the_settings_folder() {
        let v = json!([{ "name": "App icon set", "steps": [{ "op": "export_as" }] }, { "name": "app icon set", "steps": [] }, { "name": "", "steps": [] }, { "steps": [] }]);
        let kept = clean_actions(&v);
        assert_eq!(kept.as_array().unwrap().len(), 1);
        assert_eq!(kept[0]["name"], "App icon set");
        assert_eq!(clean_actions(&json!("nope")), json!([]));
        let folder = std::env::temp_dir().join(format!("castle-photo-actions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(actions_load_in(&folder), json!([]));
        actions_save_in(&folder, &v).unwrap();
        assert_eq!(actions_load_in(&folder), kept);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
