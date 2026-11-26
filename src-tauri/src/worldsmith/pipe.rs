//! The helper's life and the page's own calls. `serve` starts when the WORLDSMITH room opens (never at
//! start-up) and ends when the room closes, its switch goes off or Castle ends (`--parent` ends it with
//! Castle too). The page asks through the helper's client verbs, the same contract the Alice Keep calls:
//! `worldsmith-helper <verb> <args>` prints one JSON line with `ok`, and `error` when it said no.

use super::helper;
use crate::settings::SettingsStore;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

/// The verbs the page itself may send; the room's session calls the helper on its own.
pub const VERBS: [&str; 9] = ["games", "attach", "detach", "state", "freeze", "unfreeze", "shot", "caps", "thumb"];
const TYPES: [&str; 4] = ["i32", "i64", "f32", "f64"];

/// The helper's `serve` while the room is open, and whether F9 and F10 are held (only while a game is
/// attached).
#[derive(Default)]
pub struct Serve {
    child: Mutex<Option<Child>>,
    pub f9_held: AtomicBool,
    pub f10_held: AtomicBool,
}

fn is_addr(s: &str) -> bool {
    s.len() > 2 && s.len() <= 18 && (s.starts_with("0x") || s.starts_with("0X")) && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.parse::<f64>().is_ok_and(f64::is_finite)
}

/// The helper's arguments for one call from the page, or why not. Pure.
pub fn call_args(verb: &str, args: &[String]) -> Result<Vec<String>, String> {
    if !VERBS.contains(&verb) {
        return Err(format!("Worldsmith does not know \"{verb}\"."));
    }
    let a: Vec<&str> = args.iter().map(|s| s.trim()).collect();
    let ok = match (verb, a.as_slice()) {
        ("games" | "detach" | "state" | "shot" | "caps" | "thumb", []) => true,
        ("attach", [pid]) => !pid.is_empty() && pid.len() <= 10 && pid.chars().all(|c| c.is_ascii_digit()),
        ("freeze", [addr, kind, value]) => is_addr(addr) && TYPES.contains(kind) && is_number(value),
        ("unfreeze", [addr]) => is_addr(addr),
        _ => false,
    };
    if !ok {
        return Err(format!("Worldsmith's {verb} was given the wrong things."));
    }
    Ok(std::iter::once(verb.to_string()).chain(a.iter().map(|s| s.to_string())).collect())
}

/// The last JSON object the helper printed (one line per answer). Pure.
pub fn last_json(stdout: &str) -> Option<Value> {
    stdout.lines().rev().map(str::trim).filter(|l| l.starts_with('{')).find_map(|l| serde_json::from_str::<Value>(l).ok())
}

/// The answer when it worked, else its one line. Pure.
pub fn answer_of(stdout: &str) -> Result<Value, String> {
    let answer = last_json(stdout).ok_or("The Worldsmith helper gave no answer.")?;
    if answer.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok(answer)
    } else {
        let why = answer.get("error").and_then(Value::as_str).unwrap_or("The Worldsmith helper said no.");
        Err(why.chars().take(300).collect())
    }
}

fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn run(helper: &Path, args: &[String]) -> Result<Value, String> {
    let out = quiet(Command::new(helper).args(args).stdin(Stdio::null())).output().map_err(|_| "The Worldsmith helper could not be started.".to_string())?;
    answer_of(&String::from_utf8_lossy(&out.stdout))
}

fn need(app: &AppHandle) -> Result<(), String> {
    crate::rooms::need(&app.state::<SettingsStore>(), super::ROOM)
}

/// Whether the helper's `serve` is running now.
pub fn running(serve: &Serve) -> bool {
    serve.child.lock().unwrap().as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
}

/// The room closed, its switch went off, or Castle ends: the game is let go (every frozen value stops),
/// F9 and F10 are released, Translate's Auto stops and its overlay goes, and `serve` ends.
pub fn stop(app: &AppHandle) {
    super::translate::stop(app);
    let serve = app.state::<Serve>();
    let child = serve.child.lock().unwrap().take();
    if let Some(mut child) = child {
        if let Ok(file) = helper::ready() {
            let _ = run(&file, &["detach".to_string()]);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    super::keys::release_f9(app);
}

#[tauri::command]
pub fn worldsmith_status(serve: State<'_, Serve>) -> helper::HelperState {
    helper::state(running(&serve))
}

/// The member pressed Get it.
#[tauri::command]
pub async fn worldsmith_get_helper(app: AppHandle) -> Result<helper::HelperState, String> {
    need(&app)?;
    let fetching = app.clone();
    tauri::async_runtime::spawn_blocking(move || helper::get(&fetching)).await.map_err(|_| "The Worldsmith helper could not be downloaded.".to_string())??;
    Ok(helper::state(running(&app.state::<Serve>())))
}

/// The room opened: `serve` starts (once), tied to Castle so it ends with it.
#[tauri::command]
pub fn worldsmith_start(app: AppHandle, serve: State<'_, Serve>) -> Result<(), String> {
    need(&app)?;
    let file = helper::ready()?;
    let mut slot = serve.child.lock().unwrap();
    if slot.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None))) {
        return Ok(());
    }
    let child = quiet(Command::new(&file).args(["serve", "--parent", &std::process::id().to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()))
        .spawn()
        .map_err(|_| "The Worldsmith helper could not be started.".to_string())?;
    *slot = Some(child);
    Ok(())
}

#[tauri::command]
pub fn worldsmith_stop(app: AppHandle) {
    stop(&app);
}

/// One call from the page (the games, an attach, the state poll, a lock). An attach that worked takes
/// F9; a detach releases it.
#[tauri::command]
pub async fn worldsmith_call(app: AppHandle, verb: String, args: Vec<String>) -> Result<Value, String> {
    need(&app)?;
    let args = call_args(&verb, &args)?;
    let file = helper::ready()?;
    let mut answer = tauri::async_runtime::spawn_blocking(move || run(&file, &args)).await.map_err(|_| "The Worldsmith helper did not answer.".to_string())??;
    match verb.as_str() {
        "attach" => {
            if let Err(note) = super::keys::hold_f9(&app) {
                answer["note"] = Value::String(note);
            }
        }
        "detach" => {
            super::keys::release_f9(&app);
            super::translate::stop(&app);
        }
        _ => {}
    }
    Ok(answer)
}

/// Where the helper saves the game's pictures (the contract: `%TEMP%\worldsmith-shots`).
pub fn shots_folder() -> std::path::PathBuf {
    std::env::temp_dir().join("worldsmith-shots")
}

/// Whether a picture the helper named sits in its own shots folder (nothing else is ever shown). Pure on its inputs.
pub fn in_shots(path: &Path, folder: &Path) -> bool {
    let key = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    let (path, folder) = (key(path), key(folder));
    path.starts_with(&format!("{}\\", folder.trim_end_matches('\\'))) && path.ends_with(".png") && !path.contains("..")
}

/// One call the helper takes with no arguments (`state`, `thumb`, `caps`), from Castle itself.
pub fn ask(verb: &str) -> Result<Value, String> {
    let args = call_args(verb, &[])?;
    run(&helper::ready()?, &args)
}

/// Where the game's window sits on the screen, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// The `rect` of the helper's answer, when it gave one that makes sense. Pure.
pub fn rect_of(answer: &Value) -> Option<Rect> {
    let r = answer.get("rect")?;
    let n = |k: &str| r.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).map(f64::round);
    let (x, y, w, h) = (n("x")?, n("y")?, n("w")?, n("h")?);
    let fits = |v: f64| v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX);
    if !fits(x) || !fits(y) || !(1.0..=65_535.0).contains(&w) || !(1.0..=65_535.0).contains(&h) {
        return None;
    }
    Some(Rect { x: x as i32, y: y as i32, w: w as u32, h: h as u32 })
}

/// The game's picture as the helper saved it: its path (always in the shots folder), how it was taken
/// and where the window sits (None from an older helper that does not say).
pub struct Placed {
    pub path: String,
    pub via: String,
    pub rect: Option<Rect>,
}

/// The helper takes the game's picture; only one in its own shots folder is ever taken further.
pub fn shot_placed() -> Result<Placed, String> {
    let file = helper::ready()?;
    let answer = run(&file, &["shot".to_string()])?;
    let path = answer.get("path").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "The Worldsmith helper gave no picture.".to_string())?;
    if !in_shots(Path::new(&path), &shots_folder()) {
        return Err("The Worldsmith helper saved its picture somewhere else, so it was not shown.".into());
    }
    let via = answer.get("via").and_then(Value::as_str).unwrap_or("").chars().take(40).collect();
    Ok(Placed { path, via, rect: rect_of(&answer) })
}

/// The game's picture now (F9 or the page's Screenshot button): its path, for the canvas and the chat.
/// The page may show it (the asset scope allows that one file).
pub fn shot(app: &AppHandle) -> Result<String, String> {
    let path = shot_placed()?.path;
    let _ = app.asset_protocol_scope().allow_file(&path);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn only_the_pages_own_verbs_with_the_right_things_reach_the_helper() {
        assert_eq!(call_args("games", &[]).unwrap(), s(&["games"]));
        assert_eq!(call_args("attach", &s(&["4242"])).unwrap(), s(&["attach", "4242"]));
        assert_eq!(call_args("freeze", &s(&["0x1A2F40", "i32", "9999"])).unwrap(), s(&["freeze", "0x1A2F40", "i32", "9999"]));
        assert!(call_args("write", &s(&["0x1", "i32", "1"])).is_err(), "writes belong to the session, through its own words");
        assert!(call_args("scan", &s(&["--label", "Gold"])).is_err());
        assert!(call_args("attach", &s(&["12; del"])).is_err());
        assert!(call_args("freeze", &s(&["1A2F40", "i32", "1"])).is_err(), "an address is written 0x...");
        assert!(call_args("freeze", &s(&["0x1A2F40", "u8", "1"])).is_err());
        assert!(call_args("freeze", &s(&["0x1A2F40", "f32", "NaN"])).is_err());
        assert!(call_args("games", &s(&["extra"])).is_err());
        assert_eq!(call_args("caps", &[]).unwrap(), s(&["caps"]), "what the helper can capture, no game needed");
        assert_eq!(call_args("thumb", &[]).unwrap(), s(&["thumb"]));
        assert!(call_args("thumb", &s(&["64"])).is_err() && call_args("caps", &s(&["x"])).is_err());
    }

    #[test]
    fn the_games_place_is_read_only_when_it_makes_sense() {
        let rect = |v: Value| rect_of(&serde_json::json!({ "ok": true, "rect": v }));
        assert_eq!(rect(serde_json::json!({"x": -8, "y": 30, "w": 1920, "h": 1080})), Some(Rect { x: -8, y: 30, w: 1920, h: 1080 }));
        assert_eq!(rect(serde_json::json!({"x": 10.4, "y": 0, "w": 800.6, "h": 600})), Some(Rect { x: 10, y: 0, w: 801, h: 600 }));
        assert_eq!(rect(serde_json::json!({"x": 0, "y": 0, "w": 0, "h": 600})), None, "a window with no width is nowhere");
        assert_eq!(rect(serde_json::json!({"x": 0, "y": 0, "w": 800})), None);
        assert_eq!(rect(serde_json::json!({"x": "0", "y": 0, "w": 800, "h": 600})), None);
        assert_eq!(rect_of(&serde_json::json!({ "ok": true, "path": "a.png" })), None, "an older helper does not say");
    }

    #[test]
    fn only_a_png_in_the_shots_folder_is_shown() {
        let folder = Path::new(r"C:\Users\m\AppData\Local\Temp\worldsmith-shots");
        assert!(in_shots(Path::new(r"C:\Users\m\AppData\Local\Temp\worldsmith-shots\shot-1.png"), folder));
        assert!(in_shots(Path::new("c:/users/m/appdata/local/temp/worldsmith-shots/shot-2.PNG"), folder));
        assert!(!in_shots(Path::new(r"C:\Users\m\Documents\secret.png"), folder));
        assert!(!in_shots(Path::new(r"C:\Users\m\AppData\Local\Temp\worldsmith-shots\..\x.png"), folder));
        assert!(!in_shots(Path::new(r"C:\Users\m\AppData\Local\Temp\worldsmith-shots-evil\a.png"), folder));
        assert!(!in_shots(Path::new(r"C:\Users\m\AppData\Local\Temp\worldsmith-shots\a.exe"), folder));
    }

    #[test]
    fn the_helpers_last_line_is_the_answer_and_a_no_is_its_own_line() {
        assert_eq!(answer_of("starting\n{\"ok\":true,\"games\":[]}\n").unwrap()["games"], serde_json::json!([]));
        assert_eq!(answer_of("{\"ok\":false,\"error\":\"Worldsmith only works with offline single-player games - this one runs anti-cheat (EasyAntiCheat).\"}").unwrap_err(), "Worldsmith only works with offline single-player games - this one runs anti-cheat (EasyAntiCheat).");
        assert!(answer_of("").is_err());
        assert!(answer_of("{\"games\":[]}").is_err(), "an answer that does not say ok is not a yes");
    }
}
