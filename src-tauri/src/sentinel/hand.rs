//! The member's side of the Sentinel's hand: the install line Castle shows, the check that it is
//! in place and current, and asking the hand for one move (a request file in its queue, the Hand
//! task started, its answer read back).
//!
//! Who may ask for which move is decided here, before a request is written: the companion and the
//! watch only from the companion-alone list (the watch only firewall-on and protection-on), the
//! member's spent click for the rest. The hand itself checks every request again.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const HAND_SCRIPT: &str = include_str!("../../resources/sentinel/hand.ps1");
pub const INSTALL_SCRIPT: &str = include_str!("../../resources/sentinel/install.ps1");

pub const NAME: &str = "MemoryCore Castle Sentinel";
pub const TASKS: [&str; 3] = ["Tick", "Alarm", "Hand"];

/// The companion may ask for these alone.
pub const ALONE: [&str; 5] = ["firewall-on", "protection-on", "scan-quick", "signatures-update", "block-address"];
/// Only on the member's click.
pub const CLICK: [&str; 4] = ["block-port", "disable-task", "disable-startup", "stop-process"];
/// The watch mends these two with no judgment.
pub const REFLEX: [&str; 2] = ["firewall-on", "protection-on"];

/// How long Castle waits for the hand's answer.
pub const WAIT: Duration = Duration::from_secs(45);

/// Who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    /// The companion, from a patrol.
    Officer,
    /// The watch, with no judgment.
    Reflex,
    /// The member, with a spent click.
    Member,
    /// The Aegis Network's shared list (network.rs): an address only, for a day.
    Network,
}

impl Asker {
    pub fn word(self) -> &'static str {
        match self {
            Asker::Officer => "officer",
            Asker::Reflex => "reflex",
            Asker::Member => "member",
            Asker::Network => "network",
        }
    }
}

/// True when this asker may ask for this move at all.
pub fn allowed(asker: Asker, mv: &str) -> bool {
    match asker {
        // the watch also reads the sign-in log, which needs the hand's rights
        Asker::Reflex => REFLEX.contains(&mv) || mv == "signins",
        Asker::Officer => ALONE.contains(&mv),
        Asker::Member => ALONE.contains(&mv) || CLICK.contains(&mv) || mv == "undo",
        // the shared list may only close the door to an address, never anything else
        Asker::Network => mv == "block-address",
    }
}

/// Castle's local folder for the Sentinel: the watch's journal and heartbeat, the officer's room,
/// and the installer files the member's line points at. Found the same way with or without the
/// window (the watch runs without it).
pub fn local_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from)
    } else {
        crate::env::app_data()
    };
    base.unwrap_or_else(std::env::temp_dir).join("MemoryCore Castle").join("sentinel")
}

/// Where the installed hand lives (only an administrator can change it).
pub fn program_dir() -> PathBuf {
    // platform: windows-only - the hand is installed only on Windows
    let base = std::env::var_os("ProgramFiles").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    base.join(NAME)
}

/// The hand's queue and answers.
pub fn data_dir() -> PathBuf {
    // platform: windows-only - the hand is installed only on Windows
    let base = std::env::var_os("ProgramData").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join(NAME)
}

/// Writes the installer and the hand where the member's line points (only when they changed).
pub fn prepare_files(dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|_| "The Sentinel's folder could not be made.".to_string())?;
    let install = dir.join("install.ps1");
    let hand = dir.join("hand.ps1");
    crate::oneshot::write_if_changed(&install, INSTALL_SCRIPT);
    crate::oneshot::write_if_changed(&hand, HAND_SCRIPT);
    if std::fs::read_to_string(&hand).ok().as_deref() != Some(HAND_SCRIPT) {
        return Err("The hand could not be written.".into());
    }
    Ok((install, hand))
}

/// The member's account as Windows names it (DOMAIN\name).
pub fn account() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    }
}

/// The one line the member runs in a PowerShell opened as administrator.
pub fn install_line(install: &Path, hand: &Path, castle: &Path, user: &str) -> String {
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" -Castle \"{}\" -Hand \"{}\" -User \"{}\"",
        install.display(),
        castle.display(),
        hand.display(),
        user.replace('"', "")
    )
}

/// The line that removes it all.
pub fn uninstall_line(install: &Path) -> String {
    format!("powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" -Uninstall", install.display())
}

/// Where the Sentinel stands on this computer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    /// The hand and its three tasks are in place.
    pub installed: bool,
    /// The installed hand is the one this Castle ships (false after an update changed it).
    pub current: bool,
    pub line: String,
}

/// The sha256 of the hand this Castle ships.
pub fn shipped_sha() -> String {
    crate::db::sha256_hex(HAND_SCRIPT.as_bytes())
}

/// Checks the hand and its tasks.
#[cfg(windows)]
pub fn installed() -> Installed {
    let hand = std::fs::read(program_dir().join("hand.ps1")).ok();
    let tasks = TASKS.iter().all(|task| task_exists(task));
    let installed = hand.is_some() && tasks;
    let current = hand.is_some_and(|bytes| crate::db::sha256_hex(&bytes) == shipped_sha());
    let line = match (installed, current) {
        (false, _) => "The Sentinel is not set up on this computer yet.",
        (true, false) => "The Sentinel needs a refresh after an update: run the line again.",
        (true, true) => "The Sentinel is set up.",
    };
    Installed { installed, current, line: line.into() }
}

#[cfg(not(windows))]
pub fn installed() -> Installed {
    Installed { installed: false, current: false, line: super::look::NOT_HERE.into() }
}

#[cfg(windows)]
fn schtasks(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    // platform: windows-only - the Task Scheduler's own command
    std::process::Command::new("schtasks.exe")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
fn task_exists(task: &str) -> bool {
    schtasks(&["/Query", "/TN", &format!("\\{NAME}\\{task}")])
}

/// The hand's answer to one request.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Answer {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "move")]
    pub mv: String,
    #[serde(default)]
    pub arg: String,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub undo: Option<Value>,
    #[serde(default)]
    pub events: Option<Value>,
    #[serde(default)]
    pub dry: bool,
}

/// A request number the hand accepts: nine digits, never a starting zero.
pub fn request_id() -> String {
    let mut buf = [0u8; 4];
    let seed = getrandom::fill(&mut buf).map(|_| u32::from_le_bytes(buf)).unwrap_or_else(|_| SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(7));
    (100_000_000 + seed % 800_000_000).to_string()
}

/// The request file's text.
pub fn request_text(id: &str, mv: &str, arg: &str) -> String {
    serde_json::json!({ "id": id, "move": mv, "arg": arg }).to_string()
}

/// An answer counts only when it was written after the request (five seconds of slack).
pub fn fresh(answer_written: SystemTime, asked: SystemTime) -> bool {
    answer_written + Duration::from_secs(5) >= asked
}

/// Asks the hand for one move and waits for its answer. Refused here when this asker may not ask
/// for it (`allowed`); the hand checks everything again.
#[cfg(windows)]
pub fn ask(asker: Asker, mv: &str, arg: &str) -> Result<Answer, String> {
    if !allowed(asker, mv) {
        return Err(format!("{} may not ask for {mv}.", asker.word()));
    }
    let data = data_dir();
    let (queue, done) = (data.join("queue"), data.join("done"));
    if !queue.is_dir() {
        return Err("The Sentinel's hand is not set up on this computer.".into());
    }
    let id = request_id();
    let asked = SystemTime::now();
    let partial = queue.join(format!("{id}.json.partial"));
    let request = queue.join(format!("{id}.json"));
    std::fs::write(&partial, request_text(&id, mv, arg)).map_err(|_| "The request could not be written.".to_string())?;
    std::fs::rename(&partial, &request).map_err(|_| "The request could not be written.".to_string())?;
    if !schtasks(&["/Run", "/TN", &format!("\\{NAME}\\Hand")]) {
        let _ = std::fs::remove_file(&request);
        return Err("The Sentinel's hand could not be started. Run the set-up line again.".into());
    }
    let answer_path = done.join(format!("{id}.json"));
    let until = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < until {
        if let Ok(meta) = std::fs::metadata(&answer_path) {
            if meta.modified().is_ok_and(|m| fresh(m, asked)) {
                if let Some(answer) = std::fs::read_to_string(&answer_path).ok().and_then(|t| serde_json::from_str::<Answer>(t.trim_start_matches('\u{feff}')).ok()) {
                    if answer.id == id {
                        return Ok(answer);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err("The hand did not answer in time.".into())
}

#[cfg(not(windows))]
pub fn ask(_asker: Asker, _mv: &str, _arg: &str) -> Result<Answer, String> {
    Err(super::look::NOT_HERE.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_companion_asks_only_from_its_own_list_and_the_watch_only_its_two() {
        for mv in CLICK {
            assert!(!allowed(Asker::Officer, mv), "{mv} needs the member's click");
            assert!(allowed(Asker::Member, mv));
        }
        assert!(allowed(Asker::Officer, "block-address"));
        assert!(!allowed(Asker::Officer, "undo"));
        assert!(allowed(Asker::Reflex, "firewall-on") && allowed(Asker::Reflex, "protection-on"));
        assert!(!allowed(Asker::Reflex, "block-address") && !allowed(Asker::Reflex, "scan-quick"));
        assert!(allowed(Asker::Reflex, "signins") && !allowed(Asker::Officer, "signins"));
        assert!(!allowed(Asker::Member, "format-disk") && !allowed(Asker::Member, "signins"));
        // the Aegis Network's list may only block an address
        assert!(allowed(Asker::Network, "block-address"));
        for mv in ["firewall-on", "scan-quick", "block-port", "disable-task", "stop-process", "undo", "signins"] {
            assert!(!allowed(Asker::Network, mv), "the network may not ask for {mv}");
        }
    }

    #[test]
    fn the_lists_match_the_hands_own_lists() {
        assert!(HAND_SCRIPT.contains("$Alone = @('firewall-on', 'protection-on', 'scan-quick', 'signatures-update', 'block-address')"));
        assert!(HAND_SCRIPT.contains("$Click = @('block-port', 'disable-task', 'disable-startup', 'stop-process')"));
    }

    #[test]
    fn the_install_line_names_the_installer_castle_the_hand_and_the_member() {
        let line = install_line(Path::new(r"C:\Users\Rin\AppData\Local\MemoryCore Castle\sentinel\install.ps1"), Path::new(r"C:\x\hand.ps1"), Path::new(r"C:\Apps\Castle\memorycore-castle.exe"), "PC\\Rin");
        assert_eq!(
            line,
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"C:\\Users\\Rin\\AppData\\Local\\MemoryCore Castle\\sentinel\\install.ps1\" -Castle \"C:\\Apps\\Castle\\memorycore-castle.exe\" -Hand \"C:\\x\\hand.ps1\" -User \"PC\\Rin\""
        );
        assert!(install_line(Path::new("i"), Path::new("h"), Path::new("c"), "a\"b").ends_with("-User \"ab\""));
        assert!(uninstall_line(Path::new("i.ps1")).ends_with("-File \"i.ps1\" -Uninstall"));
    }

    #[test]
    fn a_request_number_is_nine_digits_and_its_text_is_plain_json() {
        for _ in 0..50 {
            let id = request_id();
            assert_eq!(id.len(), 9);
            assert!(id.chars().all(|c| c.is_ascii_digit()) && !id.starts_with('0'));
        }
        let text = request_text("123456789", "block-address", "203.0.113.7");
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["move"], "block-address");
    }

    #[test]
    fn an_answer_older_than_the_request_is_not_the_answer() {
        let asked = SystemTime::now();
        assert!(fresh(asked, asked));
        assert!(fresh(asked - Duration::from_secs(4), asked));
        assert!(!fresh(asked - Duration::from_secs(30), asked));
    }

    #[test]
    fn an_answer_reads_with_or_without_its_parts() {
        let a: Answer = serde_json::from_str(r#"{"id":"1","move":"block-port","arg":"8080","ok":true,"result":"done","undo":{"kind":"remove-rule","name":"MCSentinel-port-8080"},"events":null,"at":"x","dry":false}"#).unwrap();
        assert!(a.ok && a.undo.is_some() && a.mv == "block-port");
        let b: Answer = serde_json::from_str(r#"{"id":"2"}"#).unwrap();
        assert!(!b.ok);
    }

    #[cfg(windows)]
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("castle-sentinel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(windows)]
    fn powershell(args: &[&str]) -> String {
        let out = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass"]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[cfg(windows)]
    #[test]
    fn every_script_parses_in_windows_powershell() {
        let dir = temp_dir("parse");
        for (name, text) in [("look.ps1", super::super::look::SCRIPT), ("hand.ps1", HAND_SCRIPT), ("install.ps1", INSTALL_SCRIPT)] {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            let check = format!(
                "$e = $null; [void][System.Management.Automation.Language.Parser]::ParseFile('{}', [ref]$null, [ref]$e); if ($e.Count) {{ $e | ForEach-Object {{ $_.Message }} }} else {{ 'parses' }}",
                path.display()
            );
            assert_eq!(powershell(&["-Command", &check]).trim(), "parses", "{name}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The hand in a dry run (nothing on the machine changes): every refusal holds.
    #[cfg(windows)]
    #[test]
    fn the_hand_in_a_dry_run_refuses_what_must_never_be_done() {
        let dir = temp_dir("hand");
        let (queue, done) = (dir.join("q"), dir.join("d"));
        std::fs::create_dir_all(&queue).unwrap();
        std::fs::create_dir_all(&done).unwrap();
        let script = dir.join("hand.ps1");
        std::fs::write(&script, HAND_SCRIPT).unwrap();
        let lsass = crate::servers::processes().into_values().find(|p| p.name.eq_ignore_ascii_case("lsass.exe")).map(|p| p.pid.to_string()).unwrap_or_else(|| "8".into());
        // this computer's own address, as the route to an outside address would leave from (no packet is sent)
        let own = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr())).map(|a| a.ip().to_string()).ok();
        let mut requests = vec![
            ("1", "block-address", "127.0.0.1", false),
            ("2", "disable-task", "\\Microsoft\\Windows\\Defrag\\ScheduledDefrag", false),
            ("3", "stop-process", lsass.as_str(), false),
            ("4", "format-disk", "C", false),
            ("5", "block-port", "99999", false),
            ("6", "firewall-on", "x; Remove-Item C:\\", false),
            ("7", "block-address", "224.0.0.1", false),
            ("8", "disable-task", "\\MemoryCore Castle Sentinel\\Hand", false),
            ("9", "block-address", "203.0.113.7", true),
            ("10", "block-port", "8080", true),
            ("11", "firewall-on", "", true),
        ];
        if let Some(own) = own.as_deref().filter(|a| !a.starts_with("169.254.") && a != &"0.0.0.0") {
            requests.push(("12", "block-address", own, false));
        }
        for (id, mv, arg, _) in &requests {
            std::fs::write(queue.join(format!("{id}.json")), request_text(id, mv, arg)).unwrap();
        }
        std::fs::write(queue.join("bad.json"), r#"{"id":"abc","move":"firewall-on","arg":""}"#).unwrap();
        powershell(&["-File", &script.display().to_string(), "-DryRun", "-Queue", &queue.display().to_string(), "-Done", &done.display().to_string()]);
        for (id, mv, arg, ok) in &requests {
            let text = std::fs::read_to_string(done.join(format!("{id}.json"))).unwrap_or_else(|_| panic!("no answer to {id} {mv}"));
            let answer: Answer = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
            assert_eq!(answer.ok, *ok, "{mv} {arg}: {}", answer.result);
            assert!(answer.dry);
        }
        assert!(!done.join("abc.json").exists(), "a request with a bad number gets no answer");
        assert_eq!(std::fs::read_dir(&queue).unwrap().count(), 0, "the queue is emptied");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_shipped_scripts_are_ascii() {
        assert!(HAND_SCRIPT.is_ascii() && INSTALL_SCRIPT.is_ascii());
        assert_eq!(shipped_sha().len(), 64);
    }
}
