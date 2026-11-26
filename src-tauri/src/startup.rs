//! Starting with Windows: the sign-in entry, and knowing when this launch came from it.
//! Off by default; the member turns it on in setup or Settings.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// The argument the sign-in entry passes, so Castle knows Windows started it.
pub const SIGN_IN_ARG: &str = "--autostart";

pub fn is_sign_in_launch<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == SIGN_IN_ARG)
}

/// The companion's session starts by itself only when Windows started Castle and the switch is on.
pub fn should_auto_wake(at_sign_in: bool, start_session_on_launch: bool) -> bool {
    at_sign_in && start_session_on_launch
}

/// Makes the Windows sign-in entry match the setting. Returns a plain note when it was not applied.
pub fn sync(app: &AppHandle, wanted: bool) -> Option<String> {
    // A test build lives in target\debug; pointing Windows at it would leave a broken entry behind.
    if cfg!(debug_assertions) {
        return Some(if cfg!(windows) { "Start with Windows takes effect in the installed Castle, not in this test build." } else { "Open at login takes effect in the installed Castle, not in this test build." }.into());
    }
    // a leftover copy inside another app's private folder never takes Windows' start-up back from
    // the real Castle (install_check.rs)
    let exe = std::env::current_exe().ok();
    if let Some(left) = exe.as_deref().filter(|e| crate::install_check::is_leftover(e)) {
        // the page says it on the top bar and in Settings > About (install_check::install_copies)
        eprintln!("MemoryCore Castle: running from a leftover copy inside another app's private folder ({}); Start with Windows is left alone", left.display());
        return None;
    }
    let launcher = app.autolaunch();
    if launcher.is_enabled().ok() == Some(wanted) {
        // "on" only says an entry exists: it may still start another copy of Castle
        if let (true, Some(exe), Some(value)) = (wanted, exe.as_deref(), crate::install_check::run_value()) {
            if !crate::install_check::run_value_points_here(&value, exe) {
                eprintln!("MemoryCore Castle: Start with Windows pointed at {value}; pointing it at {}", exe.display());
                let _ = launcher.disable();
                return match launcher.enable() {
                    Ok(()) => Some(if cfg!(windows) { "Start with Windows pointed at another copy of Castle; it points at this one now." } else { "Open at login pointed at another copy of Castle; it points at this one now." }.into()),
                    Err(_) => Some("Start with Windows points at another copy of Castle, and Windows did not take the change.".into()),
                };
            }
        }
        return None;
    }
    let result = if wanted { launcher.enable() } else { launcher.disable() };
    result.err().map(|_| if cfg!(windows) { "Windows did not accept the change to start-up." } else { "macOS did not accept the change to the login items." }.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_in_argument_is_recognised() {
        assert!(is_sign_in_launch(["memorycore-castle.exe", "--autostart"]));
        assert!(!is_sign_in_launch(["memorycore-castle.exe"]));
        assert!(!is_sign_in_launch(["memorycore-castle.exe", "--autostart-later"]));
    }

    #[test]
    fn the_session_starts_by_itself_only_at_sign_in_with_the_switch_on() {
        assert!(should_auto_wake(true, true));
        assert!(!should_auto_wake(true, false));
        assert!(!should_auto_wake(false, true));
    }

    // Issue #99: the start-up value is compared with this program, whatever its quotes, options,
    // case or slashes (the entry auto-launch writes is `<exe> --autostart`, unquoted)
    #[test]
    fn a_start_up_value_for_this_program_is_recognised_however_it_is_written() {
        use crate::install_check::run_value_points_here;
        use std::path::Path;
        let here = Path::new(r"C:\Users\Ari\AppData\Local\MemoryCore Castle\memorycore-castle.exe");
        for value in [
            r"C:\Users\Ari\AppData\Local\MemoryCore Castle\memorycore-castle.exe --autostart",
            r#""C:\Users\Ari\AppData\Local\MemoryCore Castle\memorycore-castle.exe" --autostart"#,
            r#""C:\Users\Ari\AppData\Local\MemoryCore Castle\memorycore-castle.exe""#,
            r"c:\users\ari\appdata\local\memorycore castle\MEMORYCORE-CASTLE.EXE --autostart",
            "C:/Users/Ari/AppData/Local/MemoryCore Castle/memorycore-castle.exe --autostart",
            r#"  "C:/Users/Ari/AppData/Local/MemoryCore Castle/memorycore-castle.exe"  --autostart  "#,
        ] {
            assert!(run_value_points_here(value, here), "{value}");
        }
        for value in [
            r"C:\Users\Ari\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\MemoryCore Castle\memorycore-castle.exe --autostart",
            r#""C:\Program Files\MemoryCore Castle\memorycore-castle.exe" --autostart"#,
            r"C:\Users\Ari\AppData\Local\MemoryCore Castle\other.exe --autostart",
            "",
            "--autostart",
        ] {
            assert!(!run_value_points_here(value, here), "{value}");
        }
    }

    #[test]
    fn a_copy_in_a_packaged_apps_private_folder_is_told_apart() {
        use crate::install_check::is_leftover;
        use std::path::Path;
        assert!(is_leftover(Path::new(r"C:\Users\Ari\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\MemoryCore Castle\memorycore-castle.exe")));
        assert!(is_leftover(Path::new("c:/users/ari/appdata/local/packages/claude_x/localcache/local/memorycore castle/memorycore-castle.exe")), "slashes and case");
        assert!(!is_leftover(Path::new(r"C:\Users\Ari\AppData\Local\MemoryCore Castle\memorycore-castle.exe")));
        assert!(!is_leftover(Path::new(r"C:\Users\Ari\AppData\Local\Packages\memorycore-castle.exe")), "both parts are needed");
        assert!(!is_leftover(Path::new(r"D:\Games\LocalCache\memorycore-castle.exe")), "both parts are needed");
    }
}
