//! Run, Hot reload and Restart on the docked phone (S-1124-5). Each DEV tab gets its own run console,
//! apart from the companion's session, so the chat never sees the build. Flutter runs
//! `flutter run -d <phone>` and takes `r` (hot reload), `R` (restart) and `q` (quit) on its console; a
//! Kotlin app is built and installed with `gradlew installDebug` and launched with adb (no hot reload
//! from the command line). The iPhone preview runs the Flutter app as a local web page instead.
//! Lifted from the Alice Keep (MIT): `cmd /c` only on Windows, `sh gradlew` elsewhere, and the keys
//! Castle never hands to a child are left out of the build's environment.

use crate::emulator::{Phones, SLOTS};
use crate::mobile::{self, Kind};
use crate::pty::PtyHost;
use portable_pty::CommandBuilder;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const OUTPUT: [&str; SLOTS] = ["phone-output-0", "phone-output-1", "phone-output-2", "phone-output-3", "phone-output-4", "phone-output-5", "phone-output-6", "phone-output-7"];
pub const EXIT: [&str; SLOTS] = ["phone-exit-0", "phone-exit-1", "phone-exit-2", "phone-exit-3", "phone-exit-4", "phone-exit-5", "phone-exit-6", "phone-exit-7"];

/// What a tab's run is.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub running: bool,
    /// "flutter" or "android"
    pub kind: String,
    /// "android" (the phone), "ios" (the iPhone preview) or "simulator" (a Mac's real iPhone simulator)
    pub target: String,
    /// the iPhone preview's local address, once it runs
    pub preview: String,
    /// the run's text so far (its tail), for a panel opened again
    pub tail: String,
}

/// Every DEV tab's run console and what it runs (managed state).
#[derive(Default)]
pub struct PhoneRuns {
    pub hosts: [PtyHost; SLOTS],
    pub info: Mutex<[RunInfo; SLOTS]>,
}

/// The arguments for `flutter run`: on the phone, or as a local web page for the iPhone preview.
pub fn flutter_args(target: &Target) -> Vec<String> {
    match target {
        Target::Phone(serial) => vec!["run".into(), "-d".into(), serial.clone()],
        Target::Preview(port) => vec!["run".into(), "-d".into(), "web-server".into(), "--web-hostname".into(), "127.0.0.1".into(), "--web-port".into(), port.to_string()],
    }
}

/// Where a run goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// an emulator by adb's name
    Phone(String),
    /// the iPhone preview's local port
    Preview(u16),
}

/// The adb arguments that open an installed app on the phone (its launcher activity).
pub fn launch_args(serial: &str, app_id: &str) -> Vec<String> {
    ["-s", serial, "shell", "monkey", "-p", app_id, "-c", "android.intent.category.LAUNCHER", "1"].iter().map(|s| s.to_string()).collect()
}

/// A free port for the iPhone preview, from 7357 up.
pub fn free_preview_port(busy: impl Fn(u16) -> bool) -> Option<u16> {
    (7357..7457).find(|p| !busy(*p))
}

/// The program a run console starts and its arguments: on Windows through `cmd /c` (Flutter and
/// gradlew are .bat files there); elsewhere the program itself, and a `gradlew` script through `sh`
/// (a fresh clone may have lost its execute bit).
pub fn console_parts(program: &Path, args: &[String]) -> (String, Vec<String>) {
    let program_text = program.to_string_lossy().to_string();
    if cfg!(windows) {
        let mut all = vec!["/c".to_string(), program_text];
        all.extend(args.iter().cloned());
        ("cmd.exe".into(), all)
    } else if program.file_name().is_some_and(|n| n == "gradlew") {
        let mut all = vec![program_text];
        all.extend(args.iter().cloned());
        ("sh".into(), all)
    } else {
        (program_text, args.to_vec())
    }
}

fn console(program: &Path, args: &[String], folder: &Path) -> CommandBuilder {
    let (first, rest) = console_parts(program, args);
    let mut command = CommandBuilder::new(first);
    for a in rest {
        command.arg(a);
    }
    command.cwd(folder);
    command.env("TERM", "xterm-256color");
    for key in crate::env::NEVER_INHERITED {
        command.env_remove(key);
    }
    if std::env::var_os("JAVA_HOME").is_none() {
        if let Some(java) = mobile::java_home() {
            command.env("JAVA_HOME", java);
        }
    }
    if std::env::var_os("ANDROID_HOME").is_none() {
        if let Some(sdk) = mobile::sdk_root() {
            command.env("ANDROID_HOME", sdk);
        }
    }
    command
}

fn set_info(app: &AppHandle, slot: usize, change: impl FnOnce(&mut RunInfo)) {
    let runs = app.state::<PhoneRuns>();
    let mut all = runs.info.lock().unwrap();
    change(&mut all[slot]);
}

/// Starts the tab's run: on its phone (it must be running) or, for Flutter, as the iPhone preview.
pub fn run(app: &AppHandle, slot: usize, folder: &Path, ios: bool) -> Result<RunInfo, String> {
    let runs = app.state::<PhoneRuns>();
    if runs.hosts[slot].is_running() {
        return Ok(runs.info.lock().unwrap()[slot].clone());
    }
    let kind = mobile::kind_of(folder).ok_or("This project is neither Flutter nor an Android app.")?;
    let (program, args, target, preview): (PathBuf, Vec<String>, &str, String) = match (kind, ios) {
        (Kind::Android, true) => return Err("Kotlin builds Android apps only.".into()),
        (Kind::Flutter, true) if cfg!(target_os = "macos") => {
            let udid = boot_simulator()?;
            let flutter = mobile::flutter_program().ok_or("Flutter is not installed.")?;
            (flutter, flutter_args(&Target::Phone(udid)), "simulator", String::new())
        }
        (Kind::Flutter, true) => {
            let busy: std::collections::HashSet<u16> = crate::servers::listeners().into_iter().map(|(p, _)| p).collect();
            let port = free_preview_port(|p| busy.contains(&p)).ok_or("No free port for the iPhone preview.")?;
            let flutter = mobile::flutter_program().ok_or("Flutter is not installed.")?;
            (flutter, flutter_args(&Target::Preview(port)), "ios", format!("http://127.0.0.1:{port}"))
        }
        (Kind::Flutter, false) => {
            let serial = app.state::<Phones>().serial(slot).ok_or("Start the phone first.")?;
            let flutter = mobile::flutter_program().ok_or("Flutter is not installed.")?;
            (flutter, flutter_args(&Target::Phone(serial)), "android", String::new())
        }
        (Kind::Android, false) => {
            app.state::<Phones>().serial(slot).ok_or("Start the phone first.")?;
            let gradlew = folder.join(mobile::script("gradlew"));
            if !gradlew.is_file() {
                return Err(format!("This Android project has no {}.", mobile::script("gradlew")));
            }
            (gradlew, vec!["installDebug".into()], "android", String::new())
        }
    };
    let first = format!("> {} {}\r\n", program.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), args.join(" "));
    let _ = app.emit(OUTPUT[slot], first.clone());
    let out_app = app.clone();
    let exit_app = app.clone();
    let folder_owned = folder.to_path_buf();
    runs.hosts[slot].start(
        console(&program, &args, folder),
        120,
        30,
        move |text| {
            let _ = out_app.emit(OUTPUT[slot], text);
        },
        move |id, code| {
            exit_app.state::<PhoneRuns>().hosts[slot].finished(id);
            // a Kotlin app built and installed: open it on the phone
            if kind == Kind::Android && code == Some(0) {
                let said = launch(&exit_app, slot, &folder_owned);
                let _ = exit_app.emit(OUTPUT[slot], format!("\r\n{said}\r\n"));
            }
            set_info(&exit_app, slot, |i| i.running = false);
            let _ = exit_app.emit(EXIT[slot], code);
        },
    )?;
    let info = RunInfo { running: true, kind: if kind == Kind::Flutter { "flutter" } else { "android" }.into(), target: target.into(), preview, tail: String::new() };
    set_info(app, slot, |i| *i = info.clone());
    Ok(info)
}

/// A Mac's iPhone simulator, booted and shown in its own Simulator window: a booted one is used as it
/// is, else the newest iPhone is booted. Its udid is what `flutter run -d` takes.
fn boot_simulator() -> Result<String, String> {
    let (xcode, simulators) = mobile::ios_found();
    if !xcode.as_deref().is_some_and(|p| p.contains(".app/")) {
        return Err("The iPhone simulator needs Xcode.".into());
    }
    let chosen = mobile::simulator_choice(&simulators).ok_or("There is no iPhone simulator yet.")?.clone();
    if !chosen.booted {
        let booted = std::process::Command::new("xcrun").args(["simctl", "boot", &chosen.udid]).status().map_err(|_| "xcrun could not run.".to_string())?;
        if !booted.success() {
            return Err(format!("{} could not be started.", chosen.name));
        }
    }
    let _ = std::process::Command::new("open").args(["-a", "Simulator"]).status();
    Ok(chosen.udid)
}

fn launch(app: &AppHandle, slot: usize, folder: &Path) -> String {
    let Some(id) = mobile::application_id(folder) else { return "Installed - its applicationId was not found, so open it on the phone yourself.".into() };
    let Some(serial) = app.state::<Phones>().serial(slot) else { return "Installed - the phone stopped before it could open.".into() };
    let Some(sdk) = mobile::sdk_root() else { return "Installed.".into() };
    let args = launch_args(&serial, &id);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match crate::emulator::adb(&sdk, &refs) {
        Ok(_) => format!("Opened {id} on the phone."),
        Err(why) => format!("Installed - it could not be opened: {why}"),
    }
}

/// Hot reload: `r` to a Flutter run.
pub fn reload(app: &AppHandle, slot: usize) -> Result<(), String> {
    let runs = app.state::<PhoneRuns>();
    if runs.info.lock().unwrap()[slot].kind != "flutter" {
        return Err("Kotlin has no hot reload from the command line - Restart reinstalls.".into());
    }
    runs.hosts[slot].write(b"r")
}

/// Restart: `R` to a Flutter run; a Kotlin app is built, installed and opened again.
pub fn restart(app: &AppHandle, slot: usize, folder: &Path) -> Result<RunInfo, String> {
    let runs = app.state::<PhoneRuns>();
    let info = runs.info.lock().unwrap()[slot].clone();
    if info.kind == "flutter" && runs.hosts[slot].is_running() {
        runs.hosts[slot].write(b"R")?;
        return Ok(info);
    }
    run(app, slot, folder, info.target == "ios" || info.target == "simulator")
}

/// Stops the tab's run: `q` to Flutter first, the console ended a moment later either way.
pub fn stop(app: &AppHandle, slot: usize) {
    let runs = app.state::<PhoneRuns>();
    if !runs.hosts[slot].is_running() {
        return;
    }
    if runs.info.lock().unwrap()[slot].kind == "flutter" {
        let _ = runs.hosts[slot].write(b"q");
        for _ in 0..30 {
            if !runs.hosts[slot].is_running() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    runs.hosts[slot].kill();
}

/// What the tab's run is, with its text so far.
pub fn info(app: &AppHandle, slot: usize) -> RunInfo {
    let runs = app.state::<PhoneRuns>();
    let mut info = runs.info.lock().unwrap()[slot].clone();
    info.running = runs.hosts[slot].is_running();
    let all = runs.hosts[slot].snapshot();
    info.tail = all.chars().rev().take(6000).collect::<Vec<_>>().into_iter().rev().collect();
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flutter_runs_on_the_phone_or_as_the_preview() {
        assert_eq!(flutter_args(&Target::Phone("emulator-5556".into())), vec!["run", "-d", "emulator-5556"]);
        let web = flutter_args(&Target::Preview(7357));
        assert_eq!(web[..3], ["run", "-d", "web-server"]);
        assert!(web.windows(2).any(|w| w == ["--web-hostname", "127.0.0.1"]));
        assert!(web.windows(2).any(|w| w == ["--web-port", "7357"]));
    }

    #[test]
    fn a_kotlin_app_opens_by_its_launcher_and_the_preview_finds_a_port() {
        let args = launch_args("emulator-5554", "com.kiyo.demo");
        assert_eq!(args[..2], ["-s", "emulator-5554"]);
        assert!(args.contains(&"com.kiyo.demo".to_string()));
        assert!(args.contains(&"android.intent.category.LAUNCHER".to_string()));
        assert_eq!(free_preview_port(|_| false), Some(7357));
        assert_eq!(free_preview_port(|p| p == 7357), Some(7358));
    }

    /// The iPhone preview is the Flutter app served on 127.0.0.1 in an iframe: the window's CSP must
    /// allow exactly that, and nothing wider (no other host can be framed).
    #[test]
    fn the_preview_may_be_framed_from_this_computer_only() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        let frame = csp.split(';').map(str::trim).find(|d| d.starts_with("frame-src")).expect("frame-src is missing: the iPhone preview cannot show");
        let sources: Vec<&str> = frame.split_whitespace().skip(1).collect();
        assert_eq!(sources, vec!["http://127.0.0.1:*"], "frame-src stays narrow: {frame}");
        let port = free_preview_port(|_| false).unwrap();
        assert!(flutter_args(&Target::Preview(port)).windows(2).any(|w| w == ["--web-hostname", "127.0.0.1"]), "the preview is served where the CSP lets it be framed");
    }

    #[test]
    fn the_console_starts_the_program_the_way_this_platform_needs() {
        let args = vec!["installDebug".to_string()];
        let (first, rest) = console_parts(Path::new("proj/gradlew"), &args);
        if cfg!(windows) {
            assert_eq!(first, "cmd.exe");
            assert_eq!(rest[0], "/c");
        } else {
            assert_eq!((first.as_str(), rest.len()), ("sh", 2));
        }
        let (first, rest) = console_parts(Path::new("bin/flutter"), &["run".to_string()]);
        if cfg!(windows) {
            assert_eq!((first.as_str(), rest.last().map(String::as_str)), ("cmd.exe", Some("run")));
        } else {
            assert_eq!((first.as_str(), rest), ("bin/flutter", vec!["run".to_string()]));
        }
    }
}
