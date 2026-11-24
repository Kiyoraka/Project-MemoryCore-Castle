//! The Sentinel's look: the read-only script (`resources/sentinel/look.ps1`, carried inside Castle
//! so no loose file can be swapped) plus the listening ports Castle already reads, turned into
//! `Thing`s for the book. Windows for now; elsewhere the look says so.

use super::book::{self, Thing, ALARM, LOOK, NOTE};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub const SCRIPT: &str = include_str!("../../resources/sentinel/look.ps1");
pub const NOT_HERE: &str = "The Sentinel keeps watch on Windows for now.";

/// The kinds the quick look reads (the watch's five-minute look).
pub const QUICK_KINDS: [&str; 5] = ["antivirus", "protection", "signatures", "detection", "firewall"];
/// Everything else the full look reads.
pub const FULL_KINDS: [&str; 8] = ["port", "dbport", "startup", "task", "service", "account", "admin", "rdp"];

/// Signatures older than this many days are worth a look.
pub const OLD_SIGNATURES_DAYS: i64 = 3;
/// The folder of the Sentinel's own scheduled tasks (install.ps1), lowercased.
pub const SENTINEL_TASKS: &str = "\\memorycore castle sentinel\\";

/// One look at the machine.
#[derive(Debug, Clone, Default)]
pub struct Look {
    pub things: Vec<Thing>,
    pub covered: HashSet<String>,
    /// The parts of the look that could not be read.
    pub problems: Vec<String>,
    pub full: bool,
}

/// A listening socket with its program: (address, port, program name).
pub type Socket = (String, u16, String);

/// Looks at the machine. `db_port` is the companion's own database port, when it has one.
#[cfg(windows)]
pub fn look(full: bool, db_port: Option<u16>) -> Result<Look, String> {
    let args = if full { "" } else { "-Quick" };
    let text = run_script(SCRIPT, args, Duration::from_secs(if full { 120 } else { 45 }))?;
    let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}').trim()).map_err(|_| "The look's answer could not be read.".to_string())?;
    let sockets = if full { sockets_with_programs() } else { Vec::new() };
    Ok(things_from(&json, &sockets, db_port, full))
}

#[cfg(not(windows))]
pub fn look(_full: bool, _db_port: Option<u16>) -> Result<Look, String> {
    Err(NOT_HERE.into())
}

#[cfg(windows)]
fn sockets_with_programs() -> Vec<Socket> {
    let procs = crate::servers::processes();
    crate::servers::sockets()
        .into_iter()
        .filter(|(_, _, pid)| *pid != 0 && *pid != 4)
        .map(|(address, port, pid)| {
            let program = procs.get(&pid).map(|p| p.name.clone()).unwrap_or_else(|| "unknown".into());
            (address, port, program)
        })
        .collect()
}

/// `powershell -EncodedCommand` for a script and its arguments: `& { <script> } <args>`.
pub fn encoded(script: &str, args: &str) -> String {
    use base64::Engine;
    let wrapped = format!("& {{\n{script}\n}} {args}");
    let utf16: Vec<u8> = wrapped.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// Runs a script Castle carries in Windows PowerShell, with no window and no profile; stopped
/// after `limit`. Returns what it printed.
#[cfg(windows)]
pub fn run_script(script: &str, args: &str, limit: Duration) -> Result<String, String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    // platform: windows-only - the look reads Windows' own security state
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(encoded(script, args))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000);
    let mut child = command.spawn().map_err(|_| "Windows PowerShell could not be started.".to_string())?;
    let mut stdout = child.stdout.take().ok_or("No output.")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let until = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                return Err("The look took too long and was stopped.".into());
            }
        }
    }
    Ok(reader.join().unwrap_or_default())
}

/// A JSON list, also when PowerShell printed a one-item list as the item itself.
pub fn list(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(one) => vec![one],
    }
}

fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

fn b(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn thing(kind: &str, parts: Vec<String>, hint: &'static str, title: String, detail: String, fix: Option<(&str, Option<String>)>, inventory: bool) -> Thing {
    Thing { kind: kind.into(), parts, hint, title, detail, fix: fix.map(|(m, a)| (m.to_string(), a)), inventory }
}

/// Turns the script's answer and the sockets into things (pure, so it is tested here).
pub fn things_from(json: &Value, sockets: &[Socket], db_port: Option<u16>, full: bool) -> Look {
    let mut look = Look { full, ..Look::default() };
    look.covered = QUICK_KINDS.iter().map(|k| k.to_string()).collect();
    if full {
        look.covered.extend(FULL_KINDS.iter().map(|k| k.to_string()));
    }
    look.problems = list(json.get("problems")).into_iter().filter_map(Value::as_str).map(str::to_string).collect();
    let things = &mut look.things;

    // --- the antivirus ---
    let defender = json.get("defender").cloned().unwrap_or(Value::Null);
    let defender_active = b(&defender, "available") && s(&defender, "mode").eq_ignore_ascii_case("normal");
    let products: Vec<String> = list(json.get("products")).into_iter().map(|p| s(p, "name").to_string()).filter(|n| !n.is_empty()).collect();
    if !defender_active && products.iter().all(|p| p.to_lowercase().contains("defender")) {
        things.push(thing(
            "antivirus",
            vec!["none".into()],
            ALARM,
            "No antivirus is protecting this computer".into(),
            format!("Defender mode: {}. Antivirus products Windows knows: {}.", none_if_empty(s(&defender, "mode")), none_if_empty(&products.join(", "))),
            Some(("protection-on", None)),
            false,
        ));
    }
    if defender_active {
        if !b(&defender, "realtime") || !b(&defender, "antivirus") {
            things.push(thing(
                "protection",
                vec!["realtime".into()],
                ALARM,
                "Antivirus protection is off".into(),
                format!("Real-time protection: {}. Antivirus: {}.", on_off(b(&defender, "realtime")), on_off(b(&defender, "antivirus"))),
                Some(("protection-on", None)),
                false,
            ));
        }
        let age = defender.get("signatureAgeDays").and_then(Value::as_i64).unwrap_or(-1);
        if age > OLD_SIGNATURES_DAYS {
            things.push(thing(
                "signatures",
                vec!["old".into()],
                LOOK,
                format!("The antivirus signatures are {age} days old"),
                "Old signatures miss new threats.".into(),
                Some(("signatures-update", None)),
                false,
            ));
        }
        for d in list(json.get("detections")) {
            let handled = b(d, "handled");
            things.push(thing(
                "detection",
                vec![s(d, "id").into(), s(d, "resources").into()],
                if handled { LOOK } else { ALARM },
                format!("The antivirus found {}", none_if_empty(s(d, "threat"))),
                format!("At {}: {} ({})", s(d, "at"), s(d, "resources"), if handled { "dealt with" } else { "not dealt with yet" }),
                Some(("scan-quick", None)),
                false,
            ));
        }
    }

    // --- the firewall ---
    for f in list(json.get("firewall")) {
        if !b(f, "enabled") {
            let name = s(f, "name");
            things.push(thing("firewall", vec![name.into()], ALARM, format!("The {name} firewall is off"), format!("Firewall profile {name}: off."), Some(("firewall-on", None)), false));
        }
    }

    if !full {
        return look;
    }

    // --- listening ports: one door per (port or dynamic, program), at its widest scope ---
    struct Door {
        port: u16,
        program: String,
        scope: &'static str,
        addresses: Vec<String>,
    }
    let mut doors: HashMap<(String, String), Door> = HashMap::new();
    for (address, port, program) in sockets {
        let number = if *port >= book::DYNAMIC_FROM { "dynamic".to_string() } else { port.to_string() };
        let door = doors.entry((number, program.to_lowercase())).or_insert_with(|| Door { port: *port, program: program.clone(), scope: "local", addresses: Vec::new() });
        door.scope = book::wider(door.scope, book::scope_of(address));
        if !door.addresses.contains(address) {
            door.addresses.push(address.clone());
        }
    }
    let mut doors: Vec<Door> = doors.into_values().collect();
    doors.sort_by(|a, b| a.port.cmp(&b.port).then(a.program.cmp(&b.program)));
    for Door { port, program, scope, addresses } in &doors {
        let (port, scope) = (port, *scope);
        let dynamic = *port >= book::DYNAMIC_FROM;
        let reach = match scope {
            "every" => "every address",
            "one" => "one address",
            _ => "this computer only",
        };
        let what = if dynamic { "A changing port".to_string() } else { format!("Port {port}") };
        things.push(thing(
            "port",
            book::port_parts(*port, scope, program),
            if scope == "local" { NOTE } else { LOOK },
            format!("{what} is open to {reach} ({program})"),
            format!("{program} listens on {}.", addresses.iter().map(|a| format!("{a}:{port}")).collect::<Vec<_>>().join(", ")),
            if dynamic || scope == "local" { None } else { Some(("block-port", Some(port.to_string()))) },
            true,
        ));
    }
    if let Some(db) = db_port {
        let widest = sockets.iter().filter(|(_, p, _)| *p == db).fold("local", |acc, (address, _, _)| book::wider(acc, book::scope_of(address)));
        if widest != "local" {
            things.push(thing(
                "dbport",
                vec![db.to_string()],
                ALARM,
                format!("Your companion's database can be reached from other computers (port {db})"),
                "It should listen on this computer only.".into(),
                Some(("block-port", Some(db.to_string()))),
                false,
            ));
        }
    }

    // --- startup entries ---
    for e in list(json.get("startup")) {
        let (name, command) = (s(e, "name"), s(e, "command"));
        things.push(thing(
            "startup",
            vec![name.into(), command.into()],
            LOOK,
            format!("{} starts with Windows", none_if_empty(name)),
            format!("{command} ({}, {})", s(e, "location"), s(e, "user")),
            Some(("disable-startup", Some(name.to_string()))),
            true,
        ));
    }

    // --- scheduled tasks ---
    for t in list(json.get("tasks")) {
        let full_name = format!("{}{}", s(t, "path"), s(t, "name"));
        let path = s(t, "path").to_lowercase();
        // Windows' own tasks and the Sentinel's own (its Tick, Alarm and Hand) are never offered to
        // switch off: the hand refuses both anyway
        let refused = path.starts_with("\\microsoft\\") || path.starts_with(SENTINEL_TASKS);
        let family = super::families::task_family(&full_name);
        // what the member needs to decide: what the task is, when Castle knows, then what it runs
        let what = family.map(|f| format!("What it is: {}. ", f.words)).unwrap_or_default();
        let actions = super::families::action_words(s(t, "actions"));
        things.push(thing(
            "task",
            vec![full_name.clone(), s(t, "actions").into()],
            if refused || family.is_some_and(|f| f.own) { NOTE } else { LOOK },
            format!("Scheduled task {full_name}"),
            format!("{what}Runs: {}. State: {}. Author: {}.", none_if_empty(&actions), s(t, "state"), none_if_empty(s(t, "author"))),
            if refused { None } else { Some(("disable-task", Some(full_name))) },
            true,
        ));
    }

    // --- services installed lately ---
    for v in list(json.get("services")) {
        let name = s(v, "name");
        things.push(thing(
            "service",
            vec![name.into(), s(v, "path").into()],
            LOOK,
            format!("A service was installed: {}", none_if_empty(name)),
            format!("At {}: {} (runs as {}).", s(v, "at"), s(v, "path"), none_if_empty(s(v, "account"))),
            None,
            true,
        ));
    }

    // --- accounts and administrators ---
    for u in list(json.get("users")) {
        let (name, enabled) = (s(u, "name"), b(u, "enabled"));
        things.push(thing(
            "account",
            vec![name.into(), enabled.to_string()],
            if enabled { ALARM } else { NOTE },
            format!("Account {name} ({})", if enabled { "can sign in" } else { "switched off" }),
            String::new(),
            None,
            true,
        ));
    }
    for a in list(json.get("admins")) {
        let name = s(a, "name");
        things.push(thing("admin", vec![name.into()], ALARM, format!("{name} is an administrator"), String::new(), None, true));
    }

    // --- remote desktop ---
    if json.get("rdp").is_some_and(|r| b(r, "on")) {
        things.push(thing("rdp", vec!["on".into()], LOOK, "Remote desktop is switched on".into(), "Other computers may ask to sign in to this one.".into(), None, false));
    }
    look
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

fn none_if_empty(text: &str) -> &str {
    if text.is_empty() {
        "(none named)"
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kinds(look: &Look) -> Vec<&str> {
        look.things.iter().map(|t| t.kind.as_str()).collect()
    }

    #[test]
    fn a_healthy_quick_look_finds_nothing() {
        let j = json!({"defender": {"available": true, "mode": "Normal", "realtime": true, "antivirus": true, "signatureAgeDays": 0},
                       "products": [{"name": "Windows Defender"}], "detections": [], "firewall": [{"name": "Public", "enabled": true}]});
        let look = things_from(&j, &[], None, false);
        assert!(look.things.is_empty());
        assert!(look.covered.contains("firewall") && !look.covered.contains("port"));
    }

    #[test]
    fn protection_off_a_firewall_off_and_old_signatures_are_found() {
        let j = json!({"defender": {"available": true, "mode": "Normal", "realtime": false, "antivirus": true, "signatureAgeDays": 9},
                       "products": {"name": "Windows Defender"}, "firewall": [{"name": "Domain", "enabled": true}, {"name": "Public", "enabled": false}]});
        let look = things_from(&j, &[], None, false);
        assert_eq!(kinds(&look), vec!["protection", "signatures", "firewall"]);
        assert!(look.things.iter().all(|t| !t.inventory));
        assert_eq!(look.things[0].fix, Some(("protection-on".into(), None)));
        assert_eq!(look.things[2].fix, Some(("firewall-on".into(), None)));
    }

    #[test]
    fn another_antivirus_leaves_defenders_rows_out() {
        let j = json!({"defender": {"available": true, "mode": "Passive Mode", "realtime": false, "antivirus": false, "signatureAgeDays": 90},
                       "products": [{"name": "Windows Defender"}, {"name": "Other Antivirus"}], "firewall": []});
        assert!(things_from(&j, &[], None, false).things.is_empty());
        let none = json!({"defender": {"available": false}, "products": [], "firewall": []});
        assert_eq!(kinds(&things_from(&none, &[], None, false)), vec!["antivirus"]);
    }

    #[test]
    fn a_detection_not_dealt_with_is_an_alarm() {
        let j = json!({"defender": {"available": true, "mode": "Normal", "realtime": true, "antivirus": true, "signatureAgeDays": 0},
                       "detections": {"id": "{A1}", "threat": "Test", "resources": "file:_C:\\x.txt", "handled": false, "at": "2026-11-19 17:00"}});
        let look = things_from(&j, &[], None, false);
        assert_eq!(look.things.len(), 1, "a one-item list printed as the item is still read");
        assert_eq!(look.things[0].hint, ALARM);
    }

    #[test]
    fn ipv4_and_ipv6_of_one_port_are_one_door_at_the_widest_scope() {
        let sockets: Vec<Socket> = vec![
            ("127.0.0.1".into(), 8080, "node.exe".into()),
            ("::".into(), 8080, "node.exe".into()),
            ("127.0.0.1".into(), 3307, "mariadbd.exe".into()),
            ("0.0.0.0".into(), 50001, "app.exe".into()),
        ];
        let look = things_from(&json!({}), &sockets, Some(3307), true);
        let ports: Vec<&Thing> = look.things.iter().filter(|t| t.kind == "port").collect();
        assert_eq!(ports.len(), 3);
        let web = ports.iter().find(|t| t.parts[0] == "8080").unwrap();
        assert_eq!(web.parts, vec!["8080", "every", "node.exe"]);
        assert_eq!(web.fix, Some(("block-port".into(), Some("8080".into()))));
        let db = ports.iter().find(|t| t.parts[0] == "3307").unwrap();
        assert_eq!((db.hint, db.fix.clone()), (NOTE, None));
        let dynamic = ports.iter().find(|t| t.parts[0] == "dynamic").unwrap();
        assert_eq!(dynamic.fix, None, "a changing port is never blocked by number");
        assert!(!kinds(&look).contains(&"dbport"), "the companion's database listens on this computer only");
    }

    #[test]
    fn the_companions_database_open_to_others_is_an_alarm() {
        let sockets: Vec<Socket> = vec![("0.0.0.0".into(), 3306, "mysqld.exe".into())];
        let look = things_from(&json!({}), &sockets, Some(3306), true);
        let db = look.things.iter().find(|t| t.kind == "dbport").unwrap();
        assert_eq!((db.hint, db.inventory), (ALARM, false));
    }

    #[test]
    fn the_full_look_reads_startup_tasks_services_accounts_admins_and_remote_desktop() {
        let j = json!({
            "defender": {"available": true, "mode": "Normal", "realtime": true, "antivirus": true, "signatureAgeDays": 0},
            "startup": {"name": "Updater", "command": "C:\\u.exe", "location": "HKU\\Run", "user": "me"},
            "tasks": [{"path": "\\Microsoft\\Windows\\", "name": "Defrag", "actions": "defrag.exe", "state": "Ready"},
                      {"path": "\\", "name": "Stranger", "actions": "C:\\s.exe -x", "state": "Ready"}],
            "services": [{"name": "Svc", "path": "C:\\svc.exe", "account": "LocalSystem", "at": "2026-11-19 10:00"}],
            "users": [{"name": "me", "enabled": true}, {"name": "Guest", "enabled": false}],
            "admins": {"name": "PC\\me"},
            "rdp": {"on": true}
        });
        let look = things_from(&j, &[], None, true);
        assert_eq!(kinds(&look), vec!["startup", "task", "task", "service", "account", "account", "admin", "rdp"]);
        let microsoft = &look.things[1];
        assert_eq!((microsoft.hint, microsoft.fix.clone()), (NOTE, None), "a Microsoft task is never offered to switch off");
        assert_eq!(look.things[2].fix, Some(("disable-task".into(), Some("\\Stranger".into()))));
        assert!(look.covered.contains("task"));
    }

    #[test]
    fn a_task_card_says_what_it_is_when_castle_knows_and_nothing_invented_when_not() {
        let j = json!({ "tasks": [
            {"path": "\\SoftLanding\\S-1-5-21-1-2-3-1002\\", "name": "SoftLandingDeferralTask-{724494fb-1111-2222-3333-444455556666}", "actions": "{AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}", "state": "Ready"},
            {"path": "\\", "name": "Kettle Brewer", "actions": "C:\\kettle.exe", "state": "Ready"}
        ] });
        let look = things_from(&j, &[], None, true);
        let tasks: Vec<&Thing> = look.things.iter().filter(|t| t.kind == "task").collect();
        assert_eq!(tasks[0].hint, NOTE, "Windows' own tips are a note");
        assert!(tasks[0].detail.starts_with("What it is: Windows' own tips"), "{}", tasks[0].detail);
        assert!(tasks[0].detail.contains("runs a Windows component {AF28889E"));
        assert!(tasks[0].fix.is_some(), "the member may still switch Windows' tips off");
        assert_eq!(tasks[1].hint, LOOK);
        assert!(tasks[1].detail.starts_with("Runs: C:\\kettle.exe"), "{}", tasks[1].detail);
    }

    #[test]
    fn the_sentinels_own_tasks_are_notes_without_a_switch_off() {
        let j = json!({ "tasks": [{"path": "\\MemoryCore Castle Sentinel\\", "name": "Tick", "actions": "castle.exe --sentinel-tick", "state": "Ready"}] });
        let look = things_from(&j, &[], None, true);
        let tick = look.things.iter().find(|t| t.kind == "task").unwrap();
        assert_eq!((tick.hint, tick.fix.clone()), (NOTE, None));
    }

    #[test]
    fn the_encoded_command_is_utf16_base64_of_the_wrapped_script() {
        use base64::Engine;
        let code = encoded("Write-Output 1", "-Quick");
        let bytes = base64::engine::general_purpose::STANDARD.decode(code).unwrap();
        let text: String = char::decode_utf16(bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]]))).map(|c| c.unwrap()).collect();
        assert_eq!(text, "& {\nWrite-Output 1\n} -Quick");
    }

    /// By hand on a Windows PC (read only): `cargo test live_look -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_look_reads_this_machine() {
        let quick = look(false, None).unwrap();
        let full = look(true, Some(3306)).unwrap();
        println!("quick: {} things, problems {:?}", quick.things.len(), quick.problems);
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for t in &full.things {
            *counts.entry(t.kind.as_str()).or_default() += 1;
        }
        println!("full: {} things {counts:?}, problems {:?}", full.things.len(), full.problems);
        assert!(full.things.iter().any(|t| t.kind == "port"));
    }

    #[test]
    fn the_look_script_is_ascii_and_never_writes() {
        assert!(SCRIPT.is_ascii());
        for verb in ["Set-", "Remove-", "New-Item", "New-Net", "New-Scheduled", "Register-", "Stop-", "Disable-", "Enable-", "Start-", "Add-", "Clear-", "Out-File", "Invoke-WebRequest"] {
            assert!(!SCRIPT.contains(verb), "the look must stay read-only: {verb}");
        }
    }
}
