//! What the companion's session is started with: which program, and which environment.
//! Keys reach the CLI only here, as environment variables for that one session.

use crate::settings::Cli;
use std::path::{Path, PathBuf};

/// Finds a command on PATH. A real program (.exe / .com) anywhere on PATH is taken before a script
/// (.cmd / .bat) that comes earlier: a script in front of a CLI is often someone's own wrapper that
/// adds words of its own (a model, a first message), and Castle must start the CLI itself, exactly
/// as it means to. With no program on PATH (an npm install is only a .cmd), the script is used.
pub fn find_on_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        find_program_first(command, std::env::split_paths(&path).collect::<Vec<_>>(), &exts)
    }
    #[cfg(not(windows))]
    find_executable(command, std::env::split_paths(&path))
}

/// Node, which runs Castle's helpers (castle-image.mjs and the rest): the member's own when it is
/// Node 20 or later, else the one Castle brought (node_runtime.rs).
pub fn node_program() -> Option<PathBuf> {
    crate::node_runtime::choice().map(|c| c.path().to_path_buf())
}

/// Where Castle's own Node folder goes on a session's PATH, when Castle's own is the one in use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodePlace {
    /// The companion's session: first, so its `node` is the one the helpers were made for.
    First,
    /// A DEV tab, the member's own coding terminal: last, so a Node their project uses is never shadowed.
    Last,
}

/// The session's PATH with the tools folder (macOS and Linux, where `castle-memory` is a script
/// among them) and Castle's own Node folder put in, or None when nothing needs to change.
fn session_path(base: &std::ffi::OsStr, tools: Option<&Path>, node: Option<(&Path, NodePlace)>) -> Option<String> {
    if tools.is_none() && node.is_none() {
        return None;
    }
    let mut parts: Vec<PathBuf> = std::env::split_paths(base).filter(|p| !p.as_os_str().is_empty()).collect();
    if let Some(tools) = tools {
        parts.insert(0, tools.to_path_buf());
    }
    match node {
        Some((folder, NodePlace::First)) => parts.insert(0, folder.to_path_buf()),
        Some((folder, NodePlace::Last)) => parts.push(folder.to_path_buf()),
        None => {}
    }
    let mut seen = std::collections::HashSet::new();
    parts.retain(|p| seen.insert(if cfg!(windows) { p.display().to_string().to_lowercase() } else { p.display().to_string() }));
    std::env::join_paths(parts).ok().map(|p| p.to_string_lossy().into_owned())
}

/// Node, or the bare name so the error says what is missing.
pub fn node() -> PathBuf {
    node_program().unwrap_or_else(|| PathBuf::from("node"))
}

/// The member's home folder: %USERPROFILE% on Windows, $HOME elsewhere.
pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Where per-user app data lives (Castle's spend log and notices, the helpers write the same place):
/// %APPDATA% on Windows, ~/Library/Application Support on macOS, $XDG_CONFIG_HOME or ~/.config on Linux.
pub fn app_data() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map(PathBuf::from);
    }
    if cfg!(target_os = "macos") {
        return home_dir().map(|h| h.join("Library").join("Application Support"));
    }
    std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| home_dir().map(|h| h.join(".config")))
}

/// macOS and Linux: the first file named exactly `command` on PATH that may be run.
#[cfg(not(windows))]
pub fn find_executable(command: &str, folders: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    folders
        .into_iter()
        .map(|folder| folder.join(command))
        .find(|candidate| std::fs::metadata(candidate).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// PATH with each folder once, in its first place. A shell profile that adds a folder every time it
/// runs can grow PATH past what macOS lets a program start with, so Castle keeps only one of each.
#[cfg(not(windows))]
pub fn unique_path(path: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    path.split(':').filter(|p| !p.is_empty() && seen.insert(*p)).collect::<Vec<_>>().join(":")
}

/// macOS: an app opened from the Finder or the Dock starts with only the system folders on PATH,
/// so a CLI installed with npm or Homebrew (and the node it runs on) would not be found. Castle takes
/// PATH from the member's login shell once, at start, the way their Terminal sees it.
#[cfg(target_os = "macos")]
pub fn adopt_login_path() {
    use std::io::Read;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let Ok(mut child) = std::process::Command::new(shell)
        .args(["-ilc", "printf '%s' \"$PATH\""])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return;
    };
    // a slow or stuck shell profile must never hold Castle's window back: five seconds, then the
    // PATH Castle already has
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < std::time::Duration::from_secs(5) => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return;
            }
        }
    }
    let mut stdout = Vec::new();
    if child.stdout.take().is_some_and(|mut out| out.read_to_end(&mut stdout).is_ok()) {
        let path = String::from_utf8_lossy(&stdout);
        // an interactive shell may print a banner first; PATH is the last line
        if let Some(path) = path.lines().last().map(str::trim).filter(|p| p.contains('/')) {
            std::env::set_var("PATH", unique_path(path));
        }
    }
}

/// `find_in`, but a program anywhere on PATH before a script anywhere on PATH.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn find_program_first(command: &str, folders: Vec<PathBuf>, pathext: &str) -> Option<PathBuf> {
    let is_program = |e: &&str| matches!(e.to_ascii_lowercase().as_str(), ".exe" | ".com");
    let programs: String = pathext.split(';').filter(is_program).collect::<Vec<_>>().join(";");
    find_in(command, folders.clone(), &programs).or_else(|| find_in(command, folders, pathext))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn find_in(command: &str, folders: impl IntoIterator<Item = PathBuf>, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<String> = pathext.split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect();
    for folder in folders {
        for ext in &exts {
            let candidate = folder.join(format!("{command}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The image path the companion should take: Codex draws by itself, the others need an OpenAI key.
pub fn image_mode(cli: Cli, has_openai_key: bool) -> &'static str {
    match (cli.draws_itself(), has_openai_key) {
        (true, _) => "codex",
        (false, true) => "openai",
        (false, false) => "none",
    }
}

/// How an APPEARANCE or STUDIO picture is drawn: "codex" (a separate Codex job on the member's
/// ChatGPT plan), "key" (Castle runs castle-look.mjs / castle-image.mjs itself with the OpenAI key:
/// no AI writes the prompt; the key never reaches Codex) or "none" (the prompt is saved).
/// Whoever the main companion is, Codex signed in with ChatGPT draws first, on the plan the member
/// already pays for; the key is the fallback (issue #39).
pub fn picture_way(codex_chatgpt: impl FnOnce() -> bool, has_openai_key: bool) -> &'static str {
    if codex_chatgpt() {
        "codex"
    } else if has_openai_key {
        "key"
    } else {
        "none"
    }
}

/// How a queued STUDIO or APPEARANCE picture is drawn: the member's own ComfyUI first when they
/// turned it on and it can draw this picture (a plain text one, no reference and no look), else
/// `picture_way`. Reading a picture (describe.rs) and the chat room's own drawing (roomdraw.rs)
/// keep using `picture_way`: ComfyUI draws, it does not read and it does not answer the room.
pub fn drawing_way(comfy_on: bool, comfy_can_draw_it: bool, codex_chatgpt: impl FnOnce() -> bool, has_openai_key: bool) -> &'static str {
    if comfy_on && comfy_can_draw_it {
        "comfy"
    } else {
        picture_way(codex_chatgpt, has_openai_key)
    }
}

/// Every variable Castle adds to the session. `key` reads a kept key by its account name.
/// OPENAI_API_KEY is never given to Codex: Codex would switch from the member's ChatGPT login
/// to paid API billing. `place` says where Castle's own Node folder goes on PATH (only when
/// Castle's own Node is the one in use; a member's good Node leaves PATH alone).
pub fn session_env(cli: Cli, key: &dyn Fn(&str) -> Option<String>, tools: &Path, place: NodePlace) -> Vec<(String, String)> {
    let own_node = crate::node_runtime::own_folder_in_use();
    session_env_using(cli, key, tools, own_node.as_deref().map(|folder| (folder, place)))
}

/// `session_env` with Castle's own Node folder given (None when the member's own Node is used).
fn session_env_using(cli: Cli, key: &dyn Fn(&str) -> Option<String>, tools: &Path, own_node: Option<(&Path, NodePlace)>) -> Vec<(String, String)> {
    let mut vars = vec![
        ("CASTLE_CLI".to_string(), cli.command().to_string()),
        ("CASTLE_TOOLS".to_string(), tools.display().to_string()),
    ];
    // castle-memory (in the tools folder) starts this same program with --memory
    if let Ok(exe) = std::env::current_exe() {
        vars.push(("CASTLE_EXE".into(), exe.display().to_string()));
    }
    // macOS and Linux: `castle-memory` is a script among the tools, so the tools go first on PATH
    // (and the script must be runnable; an installer may not keep that bit)
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let door = tools.join("castle-memory");
        if let Ok(meta) = std::fs::metadata(&door) {
            if meta.permissions().mode() & 0o111 == 0 {
                let _ = std::fs::set_permissions(&door, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    let tools_on_path = if cfg!(windows) { None } else { Some(tools) };
    if let Some(path) = session_path(&std::env::var_os("PATH").unwrap_or_default(), tools_on_path, own_node) {
        vars.push(("PATH".into(), path));
    }
    let openai = key("openai");
    vars.push(("CASTLE_IMAGE".into(), image_mode(cli, openai.is_some()).into()));
    match cli {
        // never to Codex: it would switch from the member's ChatGPT login to paid API billing
        Cli::Codex => {}
        Cli::Claude | Cli::Gemini | Cli::Grok => {
            if let Some(value) = openai {
                vars.push(("OPENAI_API_KEY".into(), value));
            }
        }
    }
    for (account, variable) in [("seedance", "ARK_API_KEY"), ("suno", "SUNOAPI_ORG_KEY"), ("elevenlabs", "ELEVENLABS_API_KEY")] {
        if let Some(value) = key(account) {
            vars.push((variable.into(), value));
        }
    }
    vars
}

/// Variables the session must not inherit from wherever Castle itself was started: a parent
/// Claude Code session's markers, and any API keys the member did not choose in Castle.
pub const NEVER_INHERITED: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SSE_PORT",
    "OPENAI_API_KEY",
    "ARK_API_KEY",
    "SUNOAPI_ORG_KEY",
    "ELEVENLABS_API_KEY",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn keys(set: &[&str]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = set.iter().map(|k| (k.to_string(), format!("value-of-{k}"))).collect();
        move |name| map.get(name).cloned()
    }

    fn get<'a>(vars: &'a [(String, String)], name: &str) -> Option<&'a str> {
        vars.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn joined(parts: &[&str]) -> std::ffi::OsString {
        std::env::join_paths(parts.iter().map(PathBuf::from)).unwrap()
    }

    fn split(path: &str) -> Vec<PathBuf> {
        std::env::split_paths(path).collect()
    }

    #[test]
    fn castles_own_node_goes_first_for_the_companion_and_last_for_a_dev_tab() {
        let base = joined(&["/usr/bin", "/projects/nvm/node18"]);
        let node = Path::new("/castle/node/24.22.2");
        let first = session_path(&base, None, Some((node, NodePlace::First))).unwrap();
        assert_eq!(split(&first), [PathBuf::from("/castle/node/24.22.2"), "/usr/bin".into(), "/projects/nvm/node18".into()]);
        let last = session_path(&base, None, Some((node, NodePlace::Last))).unwrap();
        assert_eq!(split(&last), [PathBuf::from("/usr/bin"), "/projects/nvm/node18".into(), "/castle/node/24.22.2".into()], "a DEV tab's own Node is never shadowed");
    }

    #[test]
    fn a_members_own_good_node_leaves_path_alone() {
        let base = joined(&["/usr/bin"]);
        assert_eq!(session_path(&base, None, None), None, "nothing to add: PATH is not even set");
        let vars = session_env_using(Cli::Claude, &keys(&[]), Path::new("/tools"), None);
        assert_eq!(get(&vars, "PATH").is_some(), cfg!(not(windows)), "only macOS and Linux put the tools on PATH");
    }

    #[test]
    fn the_session_path_carries_castles_node_when_it_is_in_use() {
        let vars = session_env_using(Cli::Claude, &keys(&[]), Path::new("/tools"), Some((Path::new("/castle/node/24.22.2"), NodePlace::First)));
        let path = split(get(&vars, "PATH").unwrap());
        assert_eq!(path[0], PathBuf::from("/castle/node/24.22.2"));
        assert_eq!(path.iter().filter(|p| **p == PathBuf::from("/castle/node/24.22.2")).count(), 1);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_repeated_folder_stays_on_path_once_in_its_first_place() {
        let grown = format!("{}/usr/bin:/a/tools::/usr/bin", "/u/.npm-global/bin:".repeat(4776));
        assert_eq!(unique_path(&grown), "/u/.npm-global/bin:/usr/bin:/a/tools");
    }

    #[cfg(not(windows))]
    #[test]
    fn on_macos_the_cli_is_the_runnable_file_with_its_bare_name() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("castle-find-{}", std::process::id()));
        let (plain, runnable) = (root.join("plain"), root.join("runnable"));
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::create_dir_all(&runnable).unwrap();
        // a file that may not be run is passed over, even earlier on PATH
        std::fs::write(plain.join("claude"), "").unwrap();
        std::fs::write(runnable.join("claude"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(runnable.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_executable("claude", [plain.clone(), runnable.clone()]), Some(runnable.join("claude")));
        assert_eq!(find_executable("codex", [plain, runnable]), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_signed_in_codex_draws_first_whoever_the_companion_is() {
        assert_eq!(picture_way(|| true, true), "codex");
        assert_eq!(picture_way(|| true, false), "codex");
        assert_eq!(picture_way(|| false, true), "key");
        assert_eq!(picture_way(|| false, false), "none");
    }

    #[test]
    fn the_members_comfyui_draws_first_when_it_is_on_and_can_draw_the_picture() {
        // on and able: ComfyUI, whatever else is signed in, and nothing else is even asked
        assert_eq!(drawing_way(true, true, || panic!("Codex must not be asked"), true), "comfy");
        // on but the picture has a reference or is an APPEARANCE look: the other ways, as before
        assert_eq!(drawing_way(true, false, || true, true), "codex");
        assert_eq!(drawing_way(true, false, || false, true), "key");
        assert_eq!(drawing_way(true, false, || false, false), "none");
        // off: exactly picture_way
        assert_eq!(drawing_way(false, true, || true, false), "codex");
        assert_eq!(drawing_way(false, true, || false, false), "none");
    }

    #[test]
    fn codex_never_receives_the_openai_key() {
        let all = keys(&["openai", "seedance", "suno", "elevenlabs"]);
        let vars = session_env_using(Cli::Codex, &all, Path::new("C:\\tools"), None);
        assert_eq!(get(&vars, "OPENAI_API_KEY"), None);
        assert_eq!(get(&vars, "CASTLE_IMAGE"), Some("codex"));
        assert_eq!(get(&vars, "ARK_API_KEY"), Some("value-of-seedance"));
        assert_eq!(get(&vars, "SUNOAPI_ORG_KEY"), Some("value-of-suno"));
        assert_eq!(get(&vars, "ELEVENLABS_API_KEY"), Some("value-of-elevenlabs"));
        assert_eq!(get(&vars, "CASTLE_CLI"), Some("codex"));
    }

    #[test]
    fn claude_code_gets_the_openai_key_when_one_is_kept() {
        let vars = session_env_using(Cli::Claude, &keys(&["openai"]), Path::new("C:\\tools"), None);
        assert_eq!(get(&vars, "OPENAI_API_KEY"), Some("value-of-openai"));
        assert_eq!(get(&vars, "CASTLE_IMAGE"), Some("openai"));
        assert_eq!(get(&vars, "ARK_API_KEY"), None);
        assert_eq!(get(&vars, "CASTLE_TOOLS"), Some("C:\\tools"));
    }

    #[test]
    fn gemini_and_grok_draw_with_the_key_like_claude_code() {
        for cli in [Cli::Gemini, Cli::Grok] {
            let vars = session_env_using(cli, &keys(&["openai"]), Path::new("C:\\tools"), None);
            assert_eq!(get(&vars, "OPENAI_API_KEY"), Some("value-of-openai"));
            assert_eq!(get(&vars, "CASTLE_IMAGE"), Some("openai"));
            assert_eq!(get(&vars, "CASTLE_CLI"), Some(cli.command()));
            assert_eq!(image_mode(cli, false), "none");
        }
    }

    #[test]
    fn claude_code_without_a_key_draws_nothing_and_says_so() {
        let vars = session_env_using(Cli::Claude, &keys(&[]), Path::new("C:\\tools"), None);
        assert_eq!(get(&vars, "CASTLE_IMAGE"), Some("none"));
        assert_eq!(get(&vars, "OPENAI_API_KEY"), None);
    }

    #[test]
    fn a_command_is_found_by_path_order_then_pathext_order() {
        let root = std::env::temp_dir().join(format!("castle-path-{}", std::process::id()));
        let (first, second) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("tool.cmd"), "").unwrap();
        std::fs::write(second.join("tool.exe"), "").unwrap();
        // the first folder wins even though its file is a .cmd
        assert_eq!(find_in("tool", [first.clone(), second.clone()], ".EXE;.CMD"), Some(first.join("tool.cmd")));
        std::fs::write(first.join("tool.exe"), "").unwrap();
        // inside one folder .EXE comes before .CMD
        assert_eq!(find_in("tool", [first.clone(), second], ".EXE;.CMD"), Some(first.join("tool.exe")));
        assert_eq!(find_in("missing", [first], ".EXE"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_cli_itself_is_started_not_a_wrapper_script_in_front_of_it() {
        let root = std::env::temp_dir().join(format!("castle-path-wrapper-{}", std::process::id()));
        let (wrappers, real) = (root.join("bin"), root.join("local"));
        std::fs::create_dir_all(&wrappers).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        // someone's own launcher earlier on PATH, adding words of its own; the real program later
        std::fs::write(wrappers.join("claude.cmd"), "@echo off\r\nclaude.exe \"alice\" %*\r\n").unwrap();
        std::fs::write(real.join("claude.exe"), "").unwrap();
        let folders = vec![wrappers.clone(), real.clone()];
        assert_eq!(find_program_first("claude", folders.clone(), ".COM;.EXE;.BAT;.CMD"), Some(real.join("claude.exe")));
        // only a script installed (npm): the script is used
        std::fs::remove_file(real.join("claude.exe")).unwrap();
        assert_eq!(find_program_first("claude", folders, ".COM;.EXE;.BAT;.CMD"), Some(wrappers.join("claude.cmd")));
        let _ = std::fs::remove_dir_all(root);
    }
}
