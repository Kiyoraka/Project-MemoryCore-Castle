//! STUDIO: what the companion made - pictures, videos, songs, voice lines - and what was saved
//! instead when a key was missing. Plus the notices the helpers leave, turned into toasts.

use crate::key;
use crate::settings::{Cli, SettingsStore};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

pub const NOTICE_EVENT: &str = "castle-notice";
const LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Made {
    /// absolute path (the page shows it through the asset protocol, scoped to the memory folder)
    pub path: String,
    pub name: String,
    /// a group: an album for songs, a world for story scenes
    pub group: Option<String>,
    /// seconds since 1970
    pub at: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub images: Vec<Made>,
    pub videos: Vec<Made>,
    pub songs: Vec<Made>,
    pub voices: Vec<Made>,
    /// prompts, lyrics and lines saved instead of a render (no key)
    pub saved: Vec<Made>,
    /// the pictures Castle kept before replacing one (`<name>.<job>-previous.png`,
    /// `<name>-previous-<n>.png`), on their own shelf so they never crowd Pictures
    pub backups: Vec<Made>,
}

/// The folders the companion's made things live in, and the only ones STUDIO may delete from. The
/// WORLD screens and the music read all of them.
pub const SHELVES: [&str; 4] = ["media-generation", "music", "adventures", "appearance"];
/// What STUDIO's own gallery shows: what was made there. Outfit and hair sheets stay in APPEARANCE,
/// adventure scenes in their world.
pub const MADE_HERE: [&str; 2] = ["media-generation", "music"];
/// Where a picture the companion makes is kept, inside the memory folder, so STUDIO's gallery shows it.
pub const PICTURE_SHELF: &str = "media-generation/image-generation";

/// A copy Castle kept before replacing a picture.
pub fn is_backup(name: &str) -> bool {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name).to_lowercase();
    stem.contains("-previous")
}

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            if depth < 4 {
                walk(&path, depth + 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn made(path: &Path, group: Option<String>) -> Made {
    let at = path.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Made { path: path.display().to_string(), name: path.file_name().unwrap_or_default().to_string_lossy().to_string(), group, at }
}

/// The things made on the named shelves (SHELVES: everything in the memory folder -
/// media-generation/, music/, adventures/*/scenes and music, appearance/), sorted into pictures, videos, songs, voices, saved prompts
/// and backups.
pub fn library_of(folder: &Path, shelves: &[&str]) -> Library {
    let mut files = Vec::new();
    for dir in shelves {
        walk(&folder.join(dir), 0, &mut files);
    }
    let mut lib = Library::default();
    for path in files {
        let rel = path.strip_prefix(folder).map(|p| p.display().to_string().replace('\\', "/")).unwrap_or_default();
        let parts: Vec<&str> = rel.split('/').collect();
        let group = match parts.first() {
            Some(&"music") if parts.len() > 2 => Some(parts[1].to_string()),
            Some(&"adventures") if parts.len() > 2 => Some(parts[1].to_string()),
            _ => None,
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        // a PAINT painting shows on the shelf as its flat picture, never its layers one by one
        if rel.starts_with("media-generation/paint/") && name != "flat.png" {
            continue;
        }
        // a PHOTO project shows as its thumbnail; its layers and the redraw work files never do
        if rel.starts_with("media-generation/photo/") && (rel.starts_with("media-generation/photo/.work/") || name != "thumb.png") {
            continue;
        }
        if name.ends_with(".prompt.md") || name.ends_with(".lyrics.md") || name.ends_with(".line.txt") {
            lib.saved.push(made(&path, group));
            continue;
        }
        match ext(&path).as_str() {
            "png" | "jpg" | "jpeg" | "webp" if is_backup(&name) => lib.backups.push(made(&path, group)),
            "png" | "jpg" | "jpeg" | "webp" if !name.starts_with("cover") || parts.first() != Some(&"music") => lib.images.push(made(&path, group)),
            "mp4" | "webm" => lib.videos.push(made(&path, group)),
            "mp3" | "wav" | "m4a" | "ogg" => {
                if parts.first() == Some(&"music") || rel.contains("/music/") {
                    lib.songs.push(made(&path, group));
                } else {
                    lib.voices.push(made(&path, group));
                }
            }
            _ => {}
        }
    }
    for list in [&mut lib.images, &mut lib.videos, &mut lib.songs, &mut lib.voices, &mut lib.saved, &mut lib.backups] {
        list.sort_by(|a, b| b.at.cmp(&a.at));
        list.truncate(LIMIT);
    }
    lib
}

/// Each STUDIO card's state: "ready", "builtin" (Codex draws by itself) or "prompt" (no key).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Powers {
    /// How a picture with a reference or an APPEARANCE look would be drawn: Codex or the key.
    pub image: &'static str,
    pub video: &'static str,
    pub song: &'static str,
    pub voice: &'static str,
    /// The member's own ComfyUI is on: it draws a plain text picture first. Kept apart from
    /// `image` because ComfyUI cannot draw a reference or an APPEARANCE look.
    pub comfy: bool,
}

pub fn powers(cli: Cli, has: &dyn Fn(&str) -> bool) -> Powers {
    let state = |name: &str| if has(name) { "ready" } else { "prompt" };
    Powers {
        image: if cli.draws_itself() { "builtin" } else { state("openai") },
        video: state("seedance"),
        song: state("suno"),
        voice: state("elevenlabs"),
        comfy: false,
    }
}

/// A file STUDIO may delete: inside one of the memory's shelves, never the standard look itself
/// (the panel's avatar and every drawing's reference).
pub fn deletable(memory: &Path, path: &str) -> Result<PathBuf, String> {
    let file = crate::files::fenced(memory, path)?;
    let root = memory.canonicalize().map_err(|_| "The memory folder is gone.".to_string())?;
    let inside = file.strip_prefix(&root).map_err(|_| "That file is outside your memory.".to_string())?;
    let shelf = inside.components().next().and_then(|c| c.as_os_str().to_str()).unwrap_or("");
    if !SHELVES.contains(&shelf) {
        return Err("Only STUDIO's own pictures, videos, songs and voice lines can be deleted here.".into());
    }
    let is_portrait = inside.parent() == Some(Path::new("appearance")) && inside.file_stem().and_then(|s| s.to_str()) == Some("portrait");
    if is_portrait {
        return Err("The standard look is kept; replace it in APPEARANCE instead.".into());
    }
    Ok(file)
}

/// Sends one made thing to the Recycle Bin, where it can be restored.
#[tauri::command]
pub async fn studio_delete(path: String, store: State<'_, SettingsStore>) -> Result<(), String> {
    let memory = store.get().memory_folder.map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let file = deletable(&memory, &path)?;
        trash::delete(&file).map_err(|_| "It could not be moved to the Recycle Bin. Close any app holding it and try again.".to_string())
    })
    .await
    .map_err(|_| "The delete could not finish.".to_string())?
}

/// Reading every shelf walks the memory folder and looks at each file, so it runs off the page's
/// thread: a command that is not async would hold the whole window still while it reads.
/// `made_here`: only what STUDIO itself shows (its gallery); otherwise every shelf (WORLD, music).
#[tauri::command]
pub async fn studio_library(made_here: Option<bool>, store: State<'_, SettingsStore>) -> Result<Library, String> {
    let Some(folder) = store.get().memory_folder else { return Ok(Library::default()) };
    let shelves: &'static [&'static str] = if made_here == Some(true) { &MADE_HERE } else { &SHELVES };
    tauri::async_runtime::spawn_blocking(move || library_of(Path::new(&folder), shelves)).await.map_err(|_| "The library could not be read.".to_string())
}

/// The Pictures card's state for a way to draw (`env::picture_way`): the plan is "builtin", the
/// key is "ready", nothing is "prompt". Pure.
pub fn picture_state(way: &str) -> &'static str {
    match way {
        "codex" => "builtin",
        "key" => "ready",
        _ => "prompt",
    }
}

/// How the next APPEARANCE or STUDIO picture would be drawn. Codex is asked whether it is signed in
/// with ChatGPT whoever the companion is (a process, so off the main thread); an installed Codex
/// alone proves nothing.
async fn current_picture_way() -> &'static str {
    let has_key = key::load("openai").is_some();
    tauri::async_runtime::spawn_blocking(move || crate::env::picture_way(crate::providers::codex_chatgpt_cached, has_key))
        .await
        .unwrap_or("none")
}

#[tauri::command]
pub async fn picture_way() -> Result<&'static str, String> {
    Ok(current_picture_way().await)
}

#[tauri::command]
pub async fn studio_powers(store: State<'_, SettingsStore>) -> Result<Powers, String> {
    let settings = store.get();
    let mut available = powers(settings.cli, &|name| key::load(name).is_some());
    available.image = picture_state(current_picture_way().await);
    // the member's own ComfyUI, once turned on, draws a plain picture first (env::drawing_way);
    // `image` stays the honest state of the ways that draw the rest
    available.comfy = settings.comfy.on;
    Ok(available)
}

/// Lets the page show files from the memory folder (and only there) through the asset protocol.
pub fn allow_memory_folder(app: &AppHandle) {
    if let Some(folder) = app.state::<SettingsStore>().get().memory_folder {
        let _ = app.asset_protocol_scope().allow_directory(Path::new(&folder), true);
    }
}

/// Where the helpers leave their notices (castle-common.mjs writes here).
pub fn notices_file() -> Option<PathBuf> {
    crate::env::app_data().map(|a| a.join("MemoryCore Castle").join("notices.jsonl"))
}

/// The room a notice belongs to: a world prepare is WORLD's, a drawing for the companion's look
/// (a file under appearance/) is APPEARANCE's, every other picture, video, song or voice STUDIO's.
pub fn notice_room(notice: &serde_json::Value) -> &'static str {
    if notice.get("kind").and_then(|k| k.as_str()) == Some("world") {
        return "world";
    }
    let file = notice.get("file").and_then(|f| f.as_str()).unwrap_or("").replace('\\', "/").to_lowercase();
    if file.contains("/appearance/") { "appearance" } else { "studio" }
}

/// Watches notices.jsonl and sends each new line to the page as a toast. Lines written before
/// Castle started are not repeated.
pub fn start_notice_watcher(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(path) = notices_file() else { return };
        let mut offset = path.metadata().map(|m| m.len()).unwrap_or(0);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let Ok(len) = path.metadata().map(|m| m.len()) else { continue };
            if len < offset {
                offset = 0;
            }
            if len == offset {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&path) {
                let fresh = String::from_utf8_lossy(&bytes[offset as usize..]).to_string();
                let settings = app.state::<SettingsStore>().get();
                for line in fresh.lines().filter(|l| !l.trim().is_empty()) {
                    if let Ok(notice) = serde_json::from_str::<serde_json::Value>(line) {
                        // a room switched off in Settings holds its notices back (the line stays in the file)
                        if settings.room_on(notice_room(&notice)) {
                            let _ = app.emit(NOTICE_EVENT, notice);
                        }
                    }
                }
                offset = len;
            }
            let _ = SystemTime::now();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_belongs_to_the_room_that_made_it() {
        let n = |v: &str| serde_json::from_str::<serde_json::Value>(v).unwrap();
        assert_eq!(notice_room(&n(r#"{"kind":"image","file":"D:\\Memory\\appearance\\wardrobe\\coat\\turnaround.prompt.md"}"#)), "appearance");
        assert_eq!(notice_room(&n(r#"{"kind":"song","file":"D:\\Memory\\music\\a\\x.lyrics.md"}"#)), "studio");
        assert_eq!(notice_room(&n(r#"{"kind":"world","file":"D:\\Memory\\worlds\\x\\hd"}"#)), "world");
    }

    #[test]
    fn made_things_are_sorted_into_their_shelves() {
        let root = std::env::temp_dir().join(format!("castle-studio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, file) in [
            ("media-generation/image-generation", "lake.png"),
            ("media-generation/image-generation", "lake.prompt.md"),
            ("media-generation/video-generation", "waves.mp4"),
            ("media-generation/voice", "hello.mp3"),
            ("music/sky-songs/audio", "01-first-light.mp3"),
            ("music/sky-songs", "night.lyrics.md"),
            ("adventures/celestial/scenes", "01-gate.png"),
            ("media-generation/paint/Night sky", "layer-1.png"),
            ("media-generation/paint/Night sky", "flat.png"),
            ("media-generation/photo/Beach", "layer-1.png"),
            ("media-generation/photo/Beach", "thumb.png"),
            ("media-generation/photo/.work", "1-in.png"),
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(file), b"x").unwrap();
        }
        let lib = library_of(&root, &SHELVES);
        assert_eq!(lib.images.len(), 4, "the lake, the scene, the painting's flat picture and the photo's thumbnail, never their layers");
        assert!(lib.images.iter().any(|m| m.name == "flat.png") && lib.images.iter().any(|m| m.name == "thumb.png"));
        assert!(!lib.images.iter().any(|m| m.name == "layer-1.png" || m.name == "1-in.png"), "no layer or redraw work file");
        assert_eq!(lib.videos.len(), 1);
        assert_eq!(lib.voices.len(), 1);
        assert_eq!(lib.songs.len(), 1);
        assert_eq!(lib.songs[0].group.as_deref(), Some("sky-songs"));
        assert_eq!(lib.saved.len(), 2);
        assert!(lib.images.iter().any(|m| m.group.as_deref() == Some("celestial")));
        let studio = library_of(&root, &MADE_HERE);
        assert_eq!(studio.images.len(), 3, "STUDIO's own gallery leaves the adventure's scene in its world");
        assert_eq!((studio.songs.len(), studio.videos.len(), studio.voices.len()), (1, 1, 1));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_kept_copy_goes_to_backups_and_only_shelf_files_can_be_deleted() {
        let root = std::env::temp_dir().join(format!("castle-studio-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, file) in [
            ("appearance", "portrait.png"),
            ("appearance", "portrait.1794731540694128500-previous.png"),
            ("appearance", "portrait-previous-1.png"),
            ("media-generation/image-generation", "lake.png"),
            ("main", "memory.md"),
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(file), b"x").unwrap();
        }
        let lib = library_of(&root, &SHELVES);
        assert_eq!(lib.backups.len(), 2);
        assert_eq!(lib.images.len(), 2, "the portrait and the lake, not the copies");
        let studio = library_of(&root, &MADE_HERE);
        assert_eq!((studio.images.len(), studio.backups.len()), (1, 0), "outfit and portrait pictures stay in APPEARANCE");
        let p = |rel: &str| root.join(rel).display().to_string();
        assert!(deletable(&root, &p("appearance/portrait.1794731540694128500-previous.png")).is_ok());
        assert!(deletable(&root, &p("media-generation/image-generation/lake.png")).is_ok());
        assert!(deletable(&root, &p("appearance/portrait.png")).is_err(), "the standard look is kept");
        assert!(deletable(&root, &p("main/memory.md")).is_err(), "not a shelf");
        assert!(deletable(&root, &p("media-generation/../main/memory.md")).is_err(), "no climbing out");
        assert!(deletable(&root, r"C:\Windows\win.ini").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn powers_follow_the_keys_and_the_cli() {
        let only_suno = |n: &str| n == "suno";
        let p = powers(Cli::Claude, &only_suno);
        assert_eq!((p.image, p.video, p.song, p.voice), ("prompt", "prompt", "ready", "prompt"));
        assert_eq!(powers(Cli::Codex, &only_suno).image, "builtin");
        // ComfyUI is its own switch, set from the settings; the keys never turn it on
        assert!(!p.comfy);
    }

    #[test]
    fn the_pictures_card_keeps_its_three_states() {
        assert_eq!(picture_state("codex"), "builtin");
        assert_eq!(picture_state("key"), "ready");
        assert_eq!(picture_state("none"), "prompt");
    }
}
