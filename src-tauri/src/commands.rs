//! The small commands the page uses for the desktop itself: gauges, apps, clipboard, themes.

use crate::settings::SettingsStore;
use crate::{apps, clipboard, files, system, themes};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// The two gauges: processor and memory in use on this computer, in percent.
#[tauri::command]
pub fn system_meters(meters: State<system::SystemMeters>) -> system::Meters {
    meters.read()
}

#[tauri::command]
pub fn apps_list(store: State<SettingsStore>) -> Vec<apps::App> {
    // DESKTOP is always reachable from APPS: the room switch no longer empties this list.
    apps::list(&store.folder(), &apps::start_menu_folders())
}

/// Draws the icons not cached yet; returns how many were drawn.
#[tauri::command]
pub async fn apps_draw_icons(store: State<'_, SettingsStore>) -> Result<usize, String> {
    let folder = store.folder();
    tauri::async_runtime::spawn_blocking(move || apps::draw_missing_icons(&folder, &apps::start_menu_folders()))
        .await
        .map_err(|_| "The icons could not be drawn.".to_string())
}

/// Opens a Start-menu app (only one that is really in the Start menu) and counts the use.
#[tauri::command]
pub fn app_open(path: String, store: State<SettingsStore>) -> Result<(), String> {
    let known = apps::scan(&apps::start_menu_folders())
        .into_iter()
        .find(|(_, shortcut)| shortcut.to_string_lossy() == path);
    let Some((name, shortcut)) = known else {
        return Err(if cfg!(windows) { "That app is not in the Start menu." } else { "That app is not in the Applications folder." }.into());
    };
    apps::open(&shortcut)?;
    let _ = apps::count_use(&store.folder(), &name);
    Ok(())
}

/// What a paste puts into the terminal: copied files' paths, text, or a saved picture's path.
#[tauri::command]
pub async fn clipboard_paste(member: State<'_, crate::chat::MemberPictures>) -> Result<clipboard::Pasted, String> {
    let pasted = tauri::async_runtime::spawn_blocking(clipboard::read)
        .await
        .map_err(|_| "The clipboard could not be read.".to_string())??;
    // A pasted picture, or copied picture files: Chat may show them before they are sent.
    if pasted.kind == "picture" || pasted.kind == "files" {
        let mut allowed = member.0.lock().unwrap();
        for path in crate::transcript::media_paths(&pasted.text) {
            if crate::transcript::media_kind(&path) == Some("picture") {
                allowed.insert(path);
            }
        }
    }
    Ok(pasted)
}

#[tauri::command]
pub fn clipboard_copy(text: String) -> Result<(), String> {
    clipboard::write_text(&text)
}

/// The themes the member dropped into Castle's settings folder, each with its picture or its reason.
/// Reading and encoding every wallpaper takes a moment, so it runs off the page's thread.
#[tauri::command]
pub async fn user_themes(store: State<'_, SettingsStore>) -> Result<Vec<themes::UserTheme>, String> {
    let folder = store.folder();
    tauri::async_runtime::spawn_blocking(move || themes::list(&folder)).await.map_err(|_| "The themes could not be read.".to_string())
}

/// The top bar's minus button: hides Castle to the tray, or minimises it to the taskbar when the
/// member chose that in Settings > Start-up. The WORLD on the other screens goes either way, and
/// comes back when Castle does (the tray, or the window getting focus again from the taskbar).
#[tauri::command]
pub fn hide_castle(app: AppHandle, store: State<SettingsStore>) {
    if let Some(window) = app.get_window("main") {
        let _ = if store.get().minimize_to_taskbar { window.minimize() } else { window.hide() };
    }
    crate::tray::went_hidden(&app);
}

/// Opens a file from the memory folder in its own app, or its folder with the file selected.
/// Only files inside the memory folder may be opened.
#[tauri::command]
pub fn media_open(path: String, folder: bool, store: State<SettingsStore>) -> Result<(), String> {
    let memory = store.get().memory_folder.map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    let file = files::fenced(&memory, &path)?;
    #[cfg(not(windows))]
    return crate::os::open_path(&file, folder);
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("explorer.exe");
        if folder {
            command.arg(format!("/select,{}", file.display()));
        } else {
            command.arg(&file);
        }
        crate::shell::let_it_come_forward();
        command.spawn().map(|_| ()).map_err(|_| "Windows could not open it.".to_string())
    }
}

/// Which MemoryCore features the member's folder holds (rooms and quick actions follow it).
#[tauri::command]
pub fn memory_features(store: State<SettingsStore>) -> crate::memory::Features {
    match store.get().memory_folder {
        Some(folder) => crate::memory::features(std::path::Path::new(&folder)),
        None => crate::memory::Features::default(),
    }
}

/// Opens a picture picker (png, jpg, webp); None when the member closes it.
#[tauri::command]
pub async fn choose_picture(app: AppHandle, title: Option<String>) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title.unwrap_or_else(|| "Choose a picture".into()))
            .add_filter("Pictures", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file()
            .and_then(|file| file.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

const AVATAR_NAMES: [&str; 3] = ["avatar.png", "avatar.jpg", "avatar.webp"];

/// Keeps a copy of the chosen picture as the companion's avatar in Castle's settings folder.
#[tauri::command]
pub fn avatar_set(path: String, store: State<SettingsStore>) -> Result<String, String> {
    let source = PathBuf::from(&path);
    let mime = files::mime(&source).filter(|m| m.starts_with("image")).ok_or("That file is not a picture.")?;
    let name = match mime {
        "image/png" => "avatar.png",
        "image/webp" => "avatar.webp",
        _ => "avatar.jpg",
    };
    let folder = store.folder();
    std::fs::create_dir_all(&folder).map_err(|_| "Castle's settings folder could not be created.".to_string())?;
    for old in AVATAR_NAMES {
        let _ = std::fs::remove_file(folder.join(old));
    }
    std::fs::copy(&source, folder.join(name)).map_err(|_| "The picture could not be copied.".to_string())?;
    files::data_url(&folder.join(name))
}

/// The companion's picture file: the one chosen in Settings, else the standard appearance drawn in
/// APPEARANCE, else None (the default crystal). The header and the room both use it.
pub fn companion_picture(store: &SettingsStore) -> Option<PathBuf> {
    let folder = store.folder();
    let chosen = AVATAR_NAMES.iter().map(|n| folder.join(n)).find(|p| p.is_file());
    let drawn = || {
        let memory = PathBuf::from(store.get().memory_folder?);
        ["portrait.png", "portrait.jpg", "portrait.webp"].iter().map(|n| memory.join("appearance").join(n)).find(|p| p.is_file())
    };
    chosen.or_else(drawn)
}

/// The companion's avatar as a data URL, or None for the default crystal.
#[tauri::command]
pub fn avatar_get(store: State<SettingsStore>) -> Option<String> {
    companion_picture(&store).and_then(|p| files::data_url(&p).ok())
}

/// Puts the default crystal back.
#[tauri::command]
pub fn avatar_clear(store: State<SettingsStore>) {
    let folder = store.folder();
    for old in AVATAR_NAMES {
        let _ = std::fs::remove_file(folder.join(old));
    }
}

/// Opens a web page in the member's browser (only https addresses).
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return Err("Only https addresses can be opened.".into());
    }
    #[cfg(not(windows))]
    return crate::os::open_link(&url);
    // explorer.exe silently fails to hand off a URL with "&" in its query string (opens a plain
    // Explorer window instead); rundll32's FileProtocolHandler is the reliable way to do this.
    #[cfg(windows)]
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|_| "The browser could not be opened.".to_string())
}
