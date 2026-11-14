//! Themes the member drops in: folders under Castle's settings folder, each with a theme.json and
//! its pictures. The core reads them so the screen never needs file access.

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

pub const FOLDER: &str = "themes";
const MAX_PICTURE_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTheme {
    pub folder: String,
    pub theme: Option<Value>,
    pub wallpaper_url: Option<String>,
    /// the theme's usageReveal low picture, read the same way (a missing one is named by the page)
    pub low_url: Option<String>,
    pub error: Option<String>,
}

/// A picture name inside the theme's own folder: a plain file name, never a path.
pub fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != "."
        && name != ".."
        && !name.starts_with('.')
}

fn picture_type(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else {
        None
    }
}

fn read_picture(folder: &Path, name: &str) -> Result<String, String> {
    if !is_plain_file_name(name) {
        return Err("The wallpaper must be a file inside the theme's own folder.".into());
    }
    let kind = picture_type(name).ok_or("The wallpaper must be a PNG, JPEG or WebP picture.")?;
    let path = folder.join(name);
    let size = fs::metadata(&path).map_err(|_| format!("The wallpaper {name} was not found."))?.len();
    if size > MAX_PICTURE_BYTES {
        return Err("The wallpaper is larger than 25 MB.".into());
    }
    let bytes = fs::read(&path).map_err(|_| format!("The wallpaper {name} could not be read."))?;
    Ok(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn read_theme(folder: &Path) -> UserTheme {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let broken = |error: String| UserTheme { folder: name.clone(), theme: None, wallpaper_url: None, low_url: None, error: Some(error) };

    let text = match fs::read_to_string(folder.join("theme.json")) {
        Ok(text) => text,
        Err(_) => return broken("theme.json was not found.".into()),
    };
    let theme: Value = match serde_json::from_str(&text) {
        Ok(theme) => theme,
        Err(_) => return broken("theme.json could not be read.".into()),
    };
    let wallpaper = match theme.get("wallpaper").and_then(Value::as_str) {
        Some(picture) => match read_picture(folder, picture) {
            Ok(url) => Some(url),
            Err(error) => return broken(error),
        },
        None => None,
    };
    let low_url = theme.pointer("/usageReveal/low").and_then(Value::as_str).and_then(|picture| read_picture(folder, picture).ok());
    UserTheme { folder: name, theme: Some(theme), wallpaper_url: wallpaper, low_url, error: None }
}

/// Every folder under `<settings folder>/themes`, in name order. A broken one is listed with its reason.
pub fn list(settings_folder: &Path) -> Vec<UserTheme> {
    let Ok(entries) = fs::read_dir(settings_folder.join(FOLDER)) else {
        return Vec::new();
    };
    let mut folders: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    folders.sort();
    folders.iter().map(|folder| read_theme(folder)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_folder(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("castle-themes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join(FOLDER)).unwrap();
        folder
    }

    #[test]
    fn only_plain_file_names_are_pictures() {
        assert!(is_plain_file_name("wallpaper.png"));
        assert!(!is_plain_file_name("../secret.png"));
        assert!(!is_plain_file_name("C:\\Windows\\a.png"));
        assert!(!is_plain_file_name("sub/a.png"));
        assert!(!is_plain_file_name(""));
    }

    #[test]
    fn a_dropped_in_theme_is_read_with_its_picture() {
        let root = temp_folder("good");
        let folder = root.join(FOLDER).join("rooftop");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("theme.json"), r#"{"id":"rooftop","name":"Rooftop","wallpaper":"art.png"}"#).unwrap();
        fs::write(folder.join("art.png"), b"png").unwrap();
        let themes = list(&root);
        assert_eq!(themes.len(), 1);
        assert!(themes[0].error.is_none());
        assert_eq!(themes[0].wallpaper_url.as_deref(), Some("data:image/png;base64,cG5n"));
    }

    #[test]
    fn broken_themes_are_listed_with_a_reason() {
        let root = temp_folder("broken");
        let bad_json = root.join(FOLDER).join("a-bad-json");
        let escaping = root.join(FOLDER).join("b-escaping");
        fs::create_dir_all(&bad_json).unwrap();
        fs::create_dir_all(&escaping).unwrap();
        fs::write(bad_json.join("theme.json"), "{ nope").unwrap();
        fs::write(escaping.join("theme.json"), r#"{"wallpaper":"../../settings.json"}"#).unwrap();
        let themes = list(&root);
        assert_eq!(themes.len(), 2);
        assert!(themes.iter().all(|t| t.error.is_some() && t.theme.is_none()));
    }

    #[test]
    fn no_themes_folder_means_no_themes() {
        let root = std::env::temp_dir().join(format!("castle-themes-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert!(list(&root).is_empty());
    }
}
