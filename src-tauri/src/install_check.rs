//! Two copies of Castle on one PC (Ari and Mei's report, Nov 25 2026). An installer run from inside a
//! packaged (MSIX) app has its writes redirected into that app's private folder
//! (`%LOCALAPPDATA%\Packages\<app>\LocalCache\...`), so a leftover Castle can sit there beside the
//! real one, with Windows' start-up and the Start menu pointing at it. Castle tells the two apart,
//! keeps Windows' start-up pointing at the real copy, and never repairs anything toward a leftover.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The name Windows' start-up entry and the per-user uninstall entry carry.
pub const APP_NAME: &str = "MemoryCore Castle";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// A path as Windows compares it: backslashes, no quotes, no trailing slash, any case.
fn plain(path: &str) -> String {
    path.trim().trim_matches('"').replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// The program a start-up value or an uninstall string starts (the first word, or the quoted path).
pub fn program_of(command: &str) -> String {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().unwrap_or("").to_string();
    }
    // an unquoted path ends at its first option
    c.split(" --").next().unwrap_or(c).split(" /").next().unwrap_or(c).trim().to_string()
}

/// Whether Windows' start-up value starts this very program.
pub fn run_value_points_here(value: &str, exe: &Path) -> bool {
    let program = program_of(value);
    !program.is_empty() && plain(&program) == plain(&exe.display().to_string())
}

/// A copy inside a packaged app's private folder: the leftover of an installer run from inside
/// another app.
pub fn is_leftover(exe: &Path) -> bool {
    let p = plain(&exe.display().to_string());
    p.contains("\\packages\\") && p.contains("\\localcache\\")
}

/// The installed copy's folder from the uninstall entry: its `InstallLocation`, else the folder of
/// its `UninstallString`.
pub fn install_folder_of(install_location: Option<&str>, uninstall_string: Option<&str>) -> Option<PathBuf> {
    if let Some(location) = install_location.map(|l| l.trim().trim_matches('"')).filter(|l| !l.is_empty()) {
        return Some(PathBuf::from(location.trim_end_matches(['\\', '/'])));
    }
    let program = program_of(uninstall_string?);
    Path::new(&program).parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty())
}

/// Whether `exe` lives in `folder`.
pub fn is_in(exe: &Path, folder: &Path) -> bool {
    exe.parent().is_some_and(|p| plain(&p.display().to_string()) == plain(&folder.display().to_string()))
}

/// What Castle knows about its copies, for HOME, Settings > About and the update card.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copies {
    /// this program
    pub this: String,
    /// the installed copy's folder, when Windows names one
    pub installed: Option<String>,
    /// this program is a leftover inside another app's private folder
    pub leftover: bool,
    /// this program is not the installed copy
    pub two: bool,
    /// these settings were last opened by a newer Castle (its version)
    pub newer_settings: Option<String>,
    /// the memory database was last updated by a newer Castle
    pub newer_database: bool,
}

pub fn copies_of(exe: &Path, installed: Option<PathBuf>) -> Copies {
    let two = installed.as_deref().is_some_and(|folder| !is_in(exe, folder));
    Copies { this: exe.display().to_string(), installed: installed.map(|p| p.display().to_string()), leftover: is_leftover(exe), two, ..Copies::default() }
}

/// This program's copies, read from Windows (a test build is never "two copies": it lives in target).
pub fn check() -> Copies {
    let Ok(exe) = std::env::current_exe() else { return Copies::default() };
    if cfg!(debug_assertions) {
        return Copies { this: exe.display().to_string(), ..Copies::default() };
    }
    copies_of(&exe, installed_folder())
}

#[cfg(windows)]
fn user_string(subkey: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (key, name) = (wide(subkey), wide(value));
    let mut size: u32 = 0;
    // SAFETY: both strings end in a zero; the first call only asks for the size
    let first = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) };
    if first != 0 || size < 2 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: the buffer holds `size` bytes, as Windows asked
    let read = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
    if read != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(not(windows))]
fn user_string(_subkey: &str, _value: &str) -> Option<String> {
    None
}

/// Windows' start-up value for Castle, when there is one.
pub fn run_value() -> Option<String> {
    user_string(RUN_KEY, APP_NAME)
}

/// The installed copy's folder, from the per-user uninstall entry.
pub fn installed_folder() -> Option<PathBuf> {
    let key = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{APP_NAME}");
    install_folder_of(user_string(&key, "InstallLocation").as_deref(), user_string(&key, "UninstallString").as_deref())
}

/// The Start menu shortcut the installer makes for this member.
pub fn start_menu_shortcut() -> Option<PathBuf> {
    let roaming = std::env::var_os("APPDATA").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(roaming).join("Microsoft\\Windows\\Start Menu\\Programs").join(format!("{APP_NAME}.lnk")))
}

/// Runs a short hidden PowerShell script with its paths given as environment variables (never put
/// into the script's text); its output.
#[cfg(windows)]
fn shell(script: &str, vars: &[(&str, &str)]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]).creation_flags(0x0800_0000);
    for (name, value) in vars {
        command.env(name, value);
    }
    let out = command.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Points the Start menu shortcut at this program when it points at another copy. A missing
/// shortcut is the installer's to make; a leftover copy never repairs anything toward itself.
/// The note when it was repaired.
#[cfg(windows)]
pub fn repair_start_menu(exe: &Path) -> Option<String> {
    if is_leftover(exe) {
        return None;
    }
    let link = start_menu_shortcut().filter(|l| l.is_file())?;
    let link_text = link.display().to_string();
    let target = shell("(New-Object -ComObject WScript.Shell).CreateShortcut($env:CASTLE_LINK).TargetPath", &[("CASTLE_LINK", &link_text)])?;
    if target.is_empty() || plain(&target) == plain(&exe.display().to_string()) {
        return None;
    }
    let folder = exe.parent()?.display().to_string();
    let exe_text = exe.display().to_string();
    shell(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:CASTLE_LINK); $s.TargetPath = $env:CASTLE_EXE; $s.WorkingDirectory = $env:CASTLE_DIR; $s.Save()",
        &[("CASTLE_LINK", &link_text), ("CASTLE_EXE", &exe_text), ("CASTLE_DIR", &folder)],
    )?;
    Some("The Start menu's Castle pointed at another copy; it opens this one now.".into())
}

#[cfg(not(windows))]
pub fn repair_start_menu(_exe: &Path) -> Option<String> {
    None
}

/// What the page reads about Castle's copies, and whether a newer Castle used these settings or
/// the memory database.
#[tauri::command]
pub fn install_copies(store: tauri::State<crate::settings::SettingsStore>) -> Copies {
    Copies { newer_settings: crate::settings::used_by_newer(&store.get()), newer_database: crate::memdb::used_by_newer(), ..check() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "C:\\Users\\Ari\\AppData\\Local\\MemoryCore Castle\\memorycore-castle.exe";
    const LEFT: &str = "C:\\Users\\Ari\\AppData\\Local\\Packages\\Claude_pzs8sxrjxfjjc\\LocalCache\\Local\\MemoryCore Castle\\memorycore-castle.exe";

    #[test]
    fn the_start_up_value_is_read_as_the_program_it_starts() {
        let here = Path::new(REAL);
        assert!(run_value_points_here(&format!("\"{REAL}\" --autostart"), here));
        assert!(run_value_points_here(&format!("\"{}\" --autostart", REAL.to_uppercase()), here), "any case");
        assert!(run_value_points_here(&format!("{REAL} --autostart"), here), "unquoted");
        assert!(!run_value_points_here(&format!("\"{LEFT}\" --autostart"), here), "the leftover");
        assert!(!run_value_points_here("", here));
        assert_eq!(program_of("\"C:\\A B\\x.exe\" /S"), "C:\\A B\\x.exe");
    }

    #[test]
    fn a_packaged_apps_private_copy_is_a_leftover() {
        assert!(is_leftover(Path::new(LEFT)));
        assert!(!is_leftover(Path::new(REAL)));
        assert!(!is_leftover(Path::new("C:\\Packages\\castle.exe")), "both parts are needed");
    }

    #[test]
    // platform: windows-only - an uninstall entry and its C:\ paths exist only on Windows (a Mac splits no backslash)
    #[cfg(windows)]
    fn the_installed_folder_comes_from_the_uninstall_entry() {
        let folder = "C:\\Users\\Ari\\AppData\\Local\\MemoryCore Castle";
        assert_eq!(install_folder_of(Some(&format!("{folder}\\")), None), Some(PathBuf::from(folder)));
        assert_eq!(install_folder_of(Some(""), Some(&format!("\"{folder}\\uninstall.exe\""))), Some(PathBuf::from(folder)));
        assert_eq!(install_folder_of(None, Some(&format!("{folder}\\uninstall.exe /S"))), Some(PathBuf::from(folder)));
        assert_eq!(install_folder_of(None, None), None);
    }

    #[test]
    // platform: windows-only - the installed copy is read from Windows' uninstall entry, with C:\ paths
    #[cfg(windows)]
    fn two_copies_are_named_and_the_real_one_is_one() {
        let installed = Some(PathBuf::from("C:\\Users\\Ari\\AppData\\Local\\MemoryCore Castle"));
        let left = copies_of(Path::new(LEFT), installed.clone());
        assert!(left.leftover && left.two);
        let real = copies_of(Path::new(REAL), installed);
        assert!(!real.leftover && !real.two);
        assert!(!copies_of(Path::new(REAL), None).two, "with no uninstall entry nothing is said");
    }
}
