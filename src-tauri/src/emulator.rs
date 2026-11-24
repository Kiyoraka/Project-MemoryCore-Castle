//! The docked phone's life (S-1124-5, his pick mobile-emulator-1): Castle starts an Android
//! emulator with no window of its own (the panel draws it), on a free console port and a free gRPC
//! port with a token, waits for it to boot, and stops it again. It only ever stops a phone it
//! started itself - one already running from Android Studio or another project is listed as running
//! elsewhere and left alone. Lifted from the Alice Keep (MIT); the discovery folder is looked for
//! where each platform's emulator writes it, and a stop ends the emulator's whole process tree.

use crate::mobile;
use serde::Serialize;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SLOTS: usize = crate::dev_tabs::SLOTS;
/// How long a phone may take to boot before the panel says so.
const BOOT_TIME: Duration = Duration::from_secs(240);
/// How long the emulator may take to write its discovery file (its port and token).
const DISCOVERY_TIME: Duration = Duration::from_secs(60);

/// Where a phone Castle started stands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneState {
    /// "off", "starting", "booting", "ready", "failed"
    pub state: String,
    pub avd: String,
    /// adb's name for it: `emulator-5556`
    pub serial: String,
    pub note: String,
}

impl PhoneState {
    fn off() -> Self {
        PhoneState { state: "off".into(), avd: String::new(), serial: String::new(), note: String::new() }
    }
}

/// One phone Castle started for a DEV tab.
pub struct Owned {
    pub child: Option<Child>,
    pub console: u16,
    pub wire: Option<crate::emulator_grpc::Wire>,
    pub state: PhoneState,
}

/// Every DEV tab's phone, started by this Castle (managed state).
#[derive(Default, Clone)]
pub struct Phones(pub Arc<Mutex<Vec<Option<Owned>>>>);

impl Phones {
    fn with<T>(&self, slot: usize, f: impl FnOnce(&mut Option<Owned>) -> T) -> T {
        let mut all = self.0.lock().unwrap();
        if all.len() < SLOTS {
            all.resize_with(SLOTS, || None);
        }
        f(&mut all[slot])
    }

    pub fn state(&self, slot: usize) -> PhoneState {
        self.with(slot, |o| o.as_ref().map(|o| o.state.clone()).unwrap_or_else(PhoneState::off))
    }

    pub fn wire(&self, slot: usize) -> Option<crate::emulator_grpc::Wire> {
        self.with(slot, |o| o.as_ref().filter(|o| o.state.state == "ready").and_then(|o| o.wire.clone()))
    }

    pub fn serial(&self, slot: usize) -> Option<String> {
        self.with(slot, |o| o.as_ref().filter(|o| o.state.state == "ready").map(|o| o.state.serial.clone()))
    }

    fn set(&self, slot: usize, change: impl FnOnce(&mut Owned)) {
        self.with(slot, |o| {
            if let Some(o) = o.as_mut() {
                change(o)
            }
        })
    }
}

fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    command
}

/// adb with its words back (stdout), or why not.
pub fn adb(sdk: &Path, args: &[&str]) -> Result<String, String> {
    let out = quiet(Command::new(mobile::adb_program(sdk)).args(args).stdin(Stdio::null())).output().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            "adb is not installed (the SDK's platform-tools).".to_string()
        } else {
            "adb could not run.".to_string()
        }
    })?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The emulators adb sees: `emulator-5554` names whose state is `device` (booted far enough).
pub fn serials_in(devices: &str) -> Vec<String> {
    devices
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let serial = parts.next()?;
            (serial.starts_with("emulator-") && parts.next() == Some("device")).then(|| serial.to_string())
        })
        .collect()
}

/// A free even console port (5554-5584; the emulator also takes the next one for adb), skipping any
/// port a program listens on or adb already names.
pub fn free_console(busy: impl Fn(u16) -> bool, known: &[String]) -> Option<u16> {
    (5554..=5584).step_by(2).find(|p| !busy(*p) && !busy(p + 1) && !known.iter().any(|s| s == &format!("emulator-{p}")))
}

/// The gRPC port for a console port: 8554 + its offset, or the next free one above it.
pub fn free_grpc(console: u16, busy: impl Fn(u16) -> bool) -> Option<u16> {
    (8554 + (console - 5554)..8700).find(|p| !busy(*p))
}

/// The emulator's discovery folders, one `pid_<pid>.ini` per running emulator, where each platform's
/// emulator writes them: `$XDG_RUNTIME_DIR/avd/running` on Linux, `~/Library/Caches/TemporaryItems/
/// avd/running` on macOS, and `<temp>/avd/running` (Windows, and the fallback everywhere).
pub fn discovery_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "linux") {
        if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
            dirs.push(PathBuf::from(run).join("avd").join("running"));
        }
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = crate::env::home_dir() {
            dirs.push(home.join("Library").join("Caches").join("TemporaryItems").join("avd").join("running"));
        }
    }
    dirs.push(std::env::temp_dir().join("avd").join("running"));
    dirs
}

/// The port and token of the emulator on one console port, from whichever discovery folder has it.
pub fn wire_anywhere(console: u16) -> Option<crate::emulator_grpc::Wire> {
    discovery_dirs().iter().find_map(|dir| wire_for(dir, console))
}

/// The port and token of the emulator on one console port: its discovery file is named after its
/// inner qemu process (not the launcher Castle starts), so it is found by its `port.serial`.
pub fn wire_for(dir: &Path, console: u16) -> Option<crate::emulator_grpc::Wire> {
    std::fs::read_dir(dir).ok()?.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("pid_")).find_map(|e| {
        let text = std::fs::read_to_string(e.path()).ok()?;
        (mobile::value_of(&text, "port.serial")? == console.to_string()).then(|| wire_of(&text)).flatten()
    })
}

/// The port and token from a discovery file's text.
pub fn wire_of(text: &str) -> Option<crate::emulator_grpc::Wire> {
    let port = mobile::value_of(text, "grpc.port")?.parse().ok()?;
    let token = mobile::value_of(text, "grpc.token").unwrap_or_default();
    Some(crate::emulator_grpc::Wire { port, token })
}

/// The ports a program listens on right now (read once per start).
fn busy_ports() -> std::collections::HashSet<u16> {
    crate::servers::listeners().into_iter().map(|(port, _)| port).collect()
}

/// Starts a phone for a DEV tab with no window of its own and boots it in the background; the panel
/// reads `state` until it says ready (or why not). A phone already running for the tab is kept.
pub fn start(phones: &Phones, slot: usize, avd: &str) -> Result<PhoneState, String> {
    if slot >= SLOTS {
        return Err("That tab does not exist.".into());
    }
    let current = phones.state(slot);
    if matches!(current.state.as_str(), "starting" | "booting" | "ready") {
        return Ok(current);
    }
    let sdk = mobile::sdk_root().ok_or("The Android SDK is not on this computer.")?;
    let home = mobile::avd_home().ok_or("No folder of phones on this computer.")?;
    if !mobile::phones_in(&home).iter().any(|p| p.name == avd) {
        return Err("That phone is not on this computer.".into());
    }
    let known = adb(&sdk, &["devices"]).map(|d| serials_in(&d)).unwrap_or_default();
    let busy = busy_ports();
    let console = free_console(|p| busy.contains(&p), &known).ok_or("Every emulator port (5554-5584) is taken.")?;
    let grpc = free_grpc(console, |p| busy.contains(&p)).ok_or("No free port for the phone's screen.")?;
    let mut command = Command::new(mobile::emulator_program(&sdk));
    command
        .args(["-avd", avd, "-no-window", "-no-boot-anim", "-port", &console.to_string(), "-grpc", &grpc.to_string(), "-grpc-use-token"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0); // its own group, so a stop ends qemu with it
    }
    let child = quiet(&mut command)
    .spawn()
    .map_err(|e| if e.kind() == ErrorKind::NotFound { "The emulator is not installed.".to_string() } else { "The emulator could not start.".to_string() })?;
    let serial = format!("emulator-{console}");
    let state = PhoneState { state: "starting".into(), avd: avd.into(), serial: serial.clone(), note: "Starting the phone…".into() };
    phones.with(slot, |o| *o = Some(Owned { child: Some(child), console, wire: None, state: state.clone() }));

    let phones = phones.clone();
    std::thread::spawn(move || {
        let fail = |why: &str| phones.set(slot, |o| o.state = PhoneState { state: "failed".into(), note: why.into(), ..o.state.clone() });
        // its port and token, from the file the emulator writes for this run
        let began = Instant::now();
        let wire = loop {
            if let Some(w) = wire_anywhere(console) {
                break w;
            }
            if began.elapsed() > DISCOVERY_TIME || exited(&phones, slot) {
                return fail("The emulator stopped before it was ready - open it once from Android Studio to see why.");
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        phones.set(slot, |o| {
            o.wire = Some(wire);
            o.state.state = "booting".into();
            o.state.note = "Booting…".into();
        });
        loop {
            if adb(&sdk, &["-s", &serial, "shell", "getprop", "sys.boot_completed"]).map(|v| v.trim() == "1").unwrap_or(false) {
                break;
            }
            if began.elapsed() > BOOT_TIME || exited(&phones, slot) {
                return fail("The phone did not finish booting.");
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        phones.set(slot, |o| {
            o.state.state = "ready".into();
            o.state.note = format!("Running on {}", o.state.serial);
        });
    });
    Ok(state)
}

fn exited(phones: &Phones, slot: usize) -> bool {
    phones.with(slot, |o| match o.as_mut().and_then(|o| o.child.as_mut()) {
        Some(child) => !matches!(child.try_wait(), Ok(None)),
        None => true,
    })
}

/// Stops the tab's phone - only one this Castle started: adb's kill first, then its whole process
/// tree (the launcher and the qemu under it), so nothing is left running unseen.
pub fn stop(phones: &Phones, slot: usize) {
    if slot >= SLOTS {
        return;
    }
    let Some(mut owned) = phones.with(slot, Option::take) else { return };
    if let Some(wire) = owned.wire.as_ref() {
        crate::emulator_grpc::forget(wire.port);
    }
    if let Some(sdk) = mobile::sdk_root() {
        let _ = adb(&sdk, &["-s", &format!("emulator-{}", owned.console), "emu", "kill"]);
    }
    if let Some(mut child) = owned.child.take() {
        for _ in 0..50 {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        crate::providers::end_process_tree(&mut child);
    }
}

/// Every phone this Castle started, stopped (Castle is ending: a phone with no window would be left
/// running unseen).
pub fn stop_all(phones: &Phones) {
    for slot in 0..SLOTS {
        stop(phones, slot);
    }
}

/// The phones on this computer, and the emulators running that this Castle did not start.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneList {
    pub phones: Vec<mobile::Phone>,
    pub elsewhere: Vec<String>,
}

pub fn list(phones: &Phones) -> PhoneList {
    let list = mobile::avd_home().map(|h| mobile::phones_in(&h)).unwrap_or_default();
    let mine: Vec<String> = (0..SLOTS).map(|s| phones.state(s).serial).filter(|s| !s.is_empty()).collect();
    let elsewhere = mobile::sdk_root()
        .and_then(|sdk| adb(&sdk, &["devices"]).ok())
        .map(|d| serials_in(&d).into_iter().filter(|s| !mine.contains(s)).collect())
        .unwrap_or_default();
    PhoneList { phones: list, elsewhere }
}

/// Turns the phone a quarter (the emulator's console command, through adb).
pub fn rotate(phones: &Phones, slot: usize) -> Result<(), String> {
    let serial = phones.serial(slot).ok_or("The phone is not running.")?;
    let sdk = mobile::sdk_root().ok_or("The Android SDK is not on this computer.")?;
    adb(&sdk, &["-s", &serial, "emu", "rotate"]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adb_devices_are_read_for_running_emulators_only() {
        let text = "List of devices attached\nemulator-5554\tdevice\nemulator-5556\toffline\nR58M123\tdevice\n\n";
        assert_eq!(serials_in(text), vec!["emulator-5554".to_string()]);
    }

    #[test]
    fn ports_are_chosen_free_and_even() {
        assert_eq!(free_console(|_| false, &[]), Some(5554));
        assert_eq!(free_console(|p| p == 5555, &[]), Some(5556));
        assert_eq!(free_console(|_| false, &["emulator-5554".into()]), Some(5556));
        assert_eq!(free_console(|_| true, &[]), None);
        assert_eq!(free_grpc(5556, |_| false), Some(8556));
        assert_eq!(free_grpc(5554, |p| p == 8554), Some(8555));
    }

    #[test]
    fn the_discovery_file_gives_the_port_and_token() {
        let text = "port.serial=5554\nport.adb=5555\ngrpc.port=8554\ngrpc.token=abc123==\n";
        let w = wire_of(text).unwrap();
        assert_eq!((w.port, w.token.as_str()), (8554, "abc123=="));
        assert!(wire_of("port.serial=5554\n").is_none());

        let dir = std::env::temp_dir().join(format!("castle-avd-running-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pid_31412.ini"), text).unwrap();
        std::fs::write(dir.join("pid_9.ini"), "port.serial=5556\ngrpc.port=8556\ngrpc.token=zz\n").unwrap();
        assert_eq!(wire_for(&dir, 5554).map(|w| w.port), Some(8554));
        assert_eq!(wire_for(&dir, 5556).map(|w| w.token), Some("zz".into()));
        assert!(wire_for(&dir, 5558).is_none());
        let _ = std::fs::remove_dir_all(dir);
        assert!(discovery_dirs().last().unwrap().ends_with("avd/running"), "the temp folder is always looked in");
    }

    /// Against a phone already running on console 5554 (started by hand): one tap and one frame.
    #[test]
    #[ignore]
    fn a_running_phone_streams_its_screen() {
        let wire = wire_anywhere(5554).expect("start a phone on -port 5554 -grpc-use-token first");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            crate::emulator_grpc::touch(wire.clone(), 10, 10, true).await.unwrap();
            crate::emulator_grpc::touch(wire.clone(), 10, 10, false).await.unwrap();
            let (w, h, bytes) = crate::emulator_grpc::one_frame(wire, 360).await.unwrap();
            println!("frame {w}x{h}, {bytes} bytes");
            assert_eq!(w, 360);
            assert_eq!(bytes, (w * h * 4) as usize);
        });
    }
}
