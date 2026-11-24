//! The watch: `memorycore-castle.exe --sentinel-tick [--cause alarm]`, run by the system's own
//! scheduler every five minutes (and on an antivirus or firewall alarm event), with no model and
//! no window. It works while Castle is closed, so it never needs the database: what it sees goes
//! into a small journal that Castle reads into the book when it opens and every minute after.
//!
//! It mends two things at once with no judgment - the firewall found off and the antivirus
//! protection found off - through the hand, at most once every fifteen minutes each.

use super::book::{self, Thing, ALARM, LOOK, NOTE};
use super::hand::{self, Asker};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const TICK_FLAG: &str = "--sentinel-tick";
/// A reflex move is tried at most this often.
pub const REFLEX_GAP: u64 = 15 * 60;
/// The same thing is written to the journal at most this often.
pub const JOURNAL_GAP: u64 = 30 * 60;
/// The journal is cut to its newer half past this size.
pub const JOURNAL_CAP: u64 = 512 * 1024;
/// Failed sign-ins from one address within this window ...
pub const BURST_WINDOW: i64 = 15 * 60;
/// ... this many times, is a burst.
pub const BURST_COUNT: usize = 10;

/// A thing as the journal carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Carried {
    pub kind: String,
    pub parts: Vec<String>,
    pub hint: String,
    pub title: String,
    pub detail: String,
    pub fix_move: Option<String>,
    pub fix_arg: Option<String>,
}

impl Carried {
    pub fn of(thing: &Thing) -> Carried {
        Carried {
            kind: thing.kind.clone(),
            parts: thing.parts.clone(),
            hint: thing.hint.into(),
            title: thing.title.clone(),
            detail: thing.detail.clone(),
            fix_move: thing.fix.as_ref().map(|f| f.0.clone()),
            fix_arg: thing.fix.as_ref().and_then(|f| f.1.clone()),
        }
    }

    pub fn thing(&self) -> Thing {
        let hint = match self.hint.as_str() {
            ALARM => ALARM,
            LOOK => LOOK,
            _ => NOTE,
        };
        Thing {
            kind: self.kind.clone(),
            parts: self.parts.clone(),
            hint,
            title: self.title.clone(),
            detail: self.detail.clone(),
            fix: self.fix_move.clone().map(|m| (m, self.fix_arg.clone())),
            inventory: false,
        }
    }
}

/// One line of the journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entry {
    /// A state thing the watch saw (journaled when new to it, or again after half an hour).
    Thing { at: u64, cause: String, thing: Carried },
    /// A reflex move and how it went.
    Action {
        at: u64,
        #[serde(rename = "move")]
        mv: String,
        ok: bool,
        result: String,
        print: Option<String>,
    },
    /// A part of the watch that could not be done.
    Problem { at: u64, line: String },
}

/// What the watch remembers between its runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// move -> when it was last tried
    #[serde(default)]
    pub reflex: HashMap<String, u64>,
    /// fingerprint -> when it was last journaled
    #[serde(default)]
    pub journaled: HashMap<String, u64>,
}

/// The move that mends a thing with no judgment, if any.
pub fn reflex_for(thing: &Thing) -> Option<&'static str> {
    match thing.kind.as_str() {
        "protection" | "antivirus" => Some("protection-on"),
        "firewall" => Some("firewall-on"),
        _ => None,
    }
}

/// Decides what one run writes and which reflex moves it makes (pure). `state` is updated.
pub fn decide(things: &[Thing], state: &mut State, now: u64, cause: &str) -> (Vec<Entry>, Vec<(&'static str, String)>) {
    let mut entries = Vec::new();
    let mut moves: Vec<(&'static str, String)> = Vec::new();
    for thing in things {
        if thing.hint == NOTE {
            continue;
        }
        let print = thing.fingerprint();
        let last = state.journaled.get(&print).copied().unwrap_or(0);
        if now.saturating_sub(last) >= JOURNAL_GAP || last == 0 {
            entries.push(Entry::Thing { at: now, cause: cause.into(), thing: Carried::of(thing) });
            state.journaled.insert(print.clone(), now);
        }
        if let Some(mv) = reflex_for(thing) {
            let tried = state.reflex.get(mv).copied().unwrap_or(0);
            if (now.saturating_sub(tried) >= REFLEX_GAP || tried == 0) && !moves.iter().any(|(m, _)| *m == mv) {
                moves.push((mv, print));
                state.reflex.insert(mv.to_string(), now);
            }
        }
    }
    // a thing not seen for a day is forgotten, so its next appearance is journaled at once
    state.journaled.retain(|_, at| now.saturating_sub(*at) < 24 * 3600);
    (entries, moves)
}

/// Seconds since 1970 for "yyyy-MM-dd HH:mm:ss" (local time is fine: only differences are used).
pub fn seconds_of(text: &str) -> Option<i64> {
    let (date, time) = text.trim().split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|p| p.parse::<i64>());
    let (h, mi) = (t.next()?.ok()?, t.next()?.ok()?);
    let s = t.next().and_then(Result::ok).unwrap_or(0);
    // days from civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s)
}

fn field<'a>(event: &'a Value, key: &str) -> &'a str {
    event.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

/// The things the sign-in log shows (pure): a burst of failed sign-ins from one address, an
/// account made, an account added to the administrators, a remote desktop sign-in.
pub fn signin_things(events: &[Value]) -> Vec<Thing> {
    let mut out = Vec::new();
    let mut failed: HashMap<String, Vec<i64>> = HashMap::new();
    for e in events {
        let id = e.get("id").and_then(Value::as_i64).unwrap_or(0);
        let at = field(e, "at");
        match id {
            4625 => {
                let address = field(e, "address");
                if !address.is_empty() && address != "-" {
                    if let Some(s) = seconds_of(at) {
                        failed.entry(address.to_string()).or_default().push(s);
                    }
                }
            }
            4720 => out.push(Thing {
                kind: "account-made".into(),
                parts: vec![field(e, "user").into(), at.into()],
                hint: ALARM,
                title: format!("A new account was made: {}", field(e, "user")),
                detail: format!("At {at}, by {}.", field(e, "by")),
                fix: None,
                inventory: false,
            }),
            4732 if field(e, "group") == "S-1-5-32-544" => out.push(Thing {
                kind: "admin-join".into(),
                parts: vec![field(e, "member").into(), at.into()],
                hint: ALARM,
                title: "An account was made an administrator".into(),
                detail: format!("At {at}: {} (by {}).", field(e, "member"), field(e, "by")),
                fix: None,
                inventory: false,
            }),
            4624 => out.push(Thing {
                kind: "remote-signin".into(),
                parts: vec![field(e, "user").into(), field(e, "address").into(), at.into()],
                hint: LOOK,
                title: format!("{} signed in through remote desktop", field(e, "user")),
                detail: format!("At {at}, from {}.", field(e, "address")),
                fix: None,
                inventory: false,
            }),
            _ => {}
        }
    }
    let mut addresses: Vec<_> = failed.into_iter().collect();
    addresses.sort();
    for (address, mut times) in addresses {
        times.sort_unstable();
        let burst = times.windows(BURST_COUNT).any(|w| w[BURST_COUNT - 1] - w[0] <= BURST_WINDOW);
        if burst {
            out.push(Thing {
                kind: "signin-burst".into(),
                parts: vec![address.clone()],
                hint: ALARM,
                title: format!("Many failed sign-ins from {address}"),
                detail: format!("{} failed sign-ins from {address} in the last day.", times.len()),
                fix: Some(("block-address".into(), Some(address))),
                inventory: false,
            });
        }
    }
    out
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn journal(dir: &Path) -> PathBuf {
    dir.join("journal.jsonl")
}

/// Appends lines to the journal, cutting it to its newer half when it grows past the cap.
pub fn append(dir: &Path, entries: &[Entry]) {
    if entries.is_empty() {
        return;
    }
    let path = journal(dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > JOURNAL_CAP) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = text.lines().collect();
            let keep = lines[lines.len() / 2..].join("\n");
            let _ = std::fs::write(&path, keep + "\n");
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        for entry in entries {
            if let Ok(line) = serde_json::to_string(entry) {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

/// One run of the watch. Exit 0 always (a problem is journaled); the scheduler needs no more.
pub fn tick_cli(args: &[String]) -> i32 {
    let cause = if args.windows(2).any(|w| w[0] == "--cause" && w[1] == "alarm") { "alarm" } else { "tick" };
    let dir = hand::local_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return 0;
    }
    let at = now();
    let state_path = dir.join("watch-state.json");
    let mut state: State = std::fs::read_to_string(&state_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut entries = Vec::new();
    let mut things = Vec::new();
    match super::look::look(false, None) {
        Ok(look) => things = look.things,
        Err(line) => entries.push(Entry::Problem { at, line }),
    }
    if hand::data_dir().join("queue").is_dir() {
        match hand::ask(Asker::Reflex, "signins", "") {
            Ok(answer) if answer.ok => {
                let events = answer.events.and_then(|e| e.as_array().cloned()).unwrap_or_default();
                things.extend(signin_things(&events));
            }
            Ok(answer) => entries.push(Entry::Problem { at, line: answer.result }),
            Err(line) => entries.push(Entry::Problem { at, line }),
        }
    }
    let (seen, moves) = decide(&things, &mut state, at, cause);
    entries.extend(seen);
    for (mv, print) in moves {
        let (ok, result) = match hand::ask(Asker::Reflex, mv, "") {
            Ok(answer) => (answer.ok, answer.result),
            Err(line) => (false, line),
        };
        entries.push(Entry::Action { at: now(), mv: mv.into(), ok, result, print: Some(print) });
    }
    append(&dir, &entries);
    if let Ok(text) = serde_json::to_string(&state) {
        let _ = std::fs::write(&state_path, text);
    }
    let _ = std::fs::write(dir.join("heartbeat.txt"), format!("{at} {cause}\n"));
    0
}

/// When the watch last ran (seconds since 1970), from its heartbeat.
pub fn last_tick(dir: &Path) -> Option<u64> {
    std::fs::read_to_string(dir.join("heartbeat.txt")).ok()?.split_whitespace().next()?.parse().ok()
}

/// What Castle read from the journal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drained {
    /// The titles of the alarms that opened.
    pub alarms: Vec<String>,
    pub lines: u32,
}

/// Reads the watch's journal into the book. The journal is moved aside first, so the watch can
/// keep writing; a journal moved aside by a run that stopped halfway is read first.
pub fn drain(dir: &Path, conn: &mut mysql::Conn) -> Result<Drained, String> {
    use mysql::prelude::Queryable;
    let path = journal(dir);
    let aside = dir.join("journal.draining");
    if !aside.exists() {
        if !path.exists() {
            return Ok(Drained::default());
        }
        std::fs::rename(&path, &aside).map_err(|_| "The watch's journal could not be read.".to_string())?;
    }
    let text = std::fs::read_to_string(&aside).unwrap_or_default();
    let mut out = Drained::default();
    let none = HashSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<Entry>(line) else { continue };
        out.lines += 1;
        match entry {
            Entry::Thing { cause, thing, .. } => {
                let thing = thing.thing();
                let opened = book::collect(conn, std::slice::from_ref(&thing), &none, false)?;
                if thing.hint == ALARM {
                    for id in opened.opened {
                        conn.exec_drop(
                            "INSERT INTO sentinel_alarms (source, detail, finding_id) VALUES (?, ?, ?)",
                            (if cause == "alarm" { "alarm-event" } else { "watch" }, thing.title.chars().take(400).collect::<String>(), id),
                        )
                        .map_err(|_| "The Sentinel's book: writing an alarm did not work.".to_string())?;
                        out.alarms.push(thing.title.clone());
                    }
                }
            }
            Entry::Action { mv, ok, result, print, .. } => {
                let finding: Option<u64> = match &print {
                    Some(p) => conn.exec_first("SELECT id FROM sentinel_findings WHERE fingerprint = ?", (p,)).ok().flatten(),
                    None => None,
                };
                conn.exec_drop(
                    "INSERT INTO sentinel_actions (finding_id, move, asked_by, state, result, done_at) VALUES (?, ?, 'reflex', ?, ?, NOW())",
                    (finding, &mv, if ok { "done" } else { "failed" }, result.chars().take(400).collect::<String>()),
                )
                .map_err(|_| "The Sentinel's book: writing a move did not work.".to_string())?;
                if let (true, Some(id)) = (ok, finding) {
                    book::mark_fixed(conn, id, "The watch switched it back on.")?;
                }
            }
            Entry::Problem { .. } => {}
        }
    }
    let _ = std::fs::remove_file(&aside);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state_thing(kind: &str, part: &str, hint: &'static str) -> Thing {
        Thing { kind: kind.into(), parts: vec![part.into()], hint, title: format!("{kind} {part}"), detail: String::new(), fix: None, inventory: false }
    }

    #[test]
    fn a_firewall_off_is_journaled_and_mended_once_in_fifteen_minutes() {
        let things = [state_thing("firewall", "Public", ALARM), state_thing("firewall", "Private", ALARM)];
        let mut state = State::default();
        let (entries, moves) = decide(&things, &mut state, 1_000, "tick");
        assert_eq!(entries.len(), 2);
        assert_eq!(moves.len(), 1, "one firewall-on mends every profile");
        assert_eq!(moves[0].0, "firewall-on");
        let (entries, moves) = decide(&things, &mut state, 1_000 + 300, "tick");
        assert!(entries.is_empty() && moves.is_empty(), "five minutes later: nothing again");
        let (entries, moves) = decide(&things, &mut state, 1_000 + REFLEX_GAP, "tick");
        assert!(entries.is_empty());
        assert_eq!(moves.len(), 1, "fifteen minutes later the reflex tries again");
        let (entries, _) = decide(&things, &mut state, 1_000 + JOURNAL_GAP, "tick");
        assert_eq!(entries.len(), 2, "half an hour later the journal hears it again");
    }

    #[test]
    fn protection_off_is_mended_and_a_note_is_never_journaled() {
        let things = [state_thing("protection", "realtime", ALARM), state_thing("port", "x", NOTE)];
        let mut state = State::default();
        let (entries, moves) = decide(&things, &mut state, 50, "alarm");
        assert_eq!(entries.len(), 1);
        assert_eq!(moves[0].0, "protection-on");
        assert!(matches!(&entries[0], Entry::Thing { cause, .. } if cause == "alarm"));
    }

    #[test]
    fn a_detection_is_journaled_but_never_mended_by_reflex() {
        let things = [state_thing("detection", "{A}", ALARM)];
        let (entries, moves) = decide(&things, &mut State::default(), 9, "alarm");
        assert_eq!((entries.len(), moves.len()), (1, 0));
    }

    #[test]
    fn ten_failed_sign_ins_in_fifteen_minutes_from_one_address_are_a_burst() {
        let mut events: Vec<Value> = (0..10).map(|i| json!({"id": 4625, "at": format!("2026-11-19 10:{:02}:00", i), "address": "198.51.100.9", "user": "admin"})).collect();
        events.extend((0..9).map(|i| json!({"id": 4625, "at": format!("2026-11-19 11:{:02}:00", i * 5), "address": "198.51.100.10"})));
        events.push(json!({"id": 4625, "at": "2026-11-19 10:00:00", "address": "-"}));
        let things = signin_things(&events);
        assert_eq!(things.len(), 1);
        assert_eq!(things[0].kind, "signin-burst");
        assert_eq!(things[0].fix, Some(("block-address".into(), Some("198.51.100.9".into()))));
    }

    #[test]
    fn a_burst_across_midnight_still_counts_and_a_slow_trickle_does_not() {
        let near: Vec<Value> = (0..10).map(|i| json!({"id": 4625, "at": if i < 5 { format!("2026-11-19 23:5{}:00", i + 5) } else { format!("2026-11-20 00:0{}:00", i - 5) }, "address": "203.0.113.5"})).collect();
        assert_eq!(signin_things(&near).len(), 1);
        let slow: Vec<Value> = (0..10).map(|i| json!({"id": 4625, "at": format!("2026-11-19 {:02}:00:00", i), "address": "203.0.113.6"})).collect();
        assert!(signin_things(&slow).is_empty());
    }

    #[test]
    fn an_account_made_and_an_administrator_added_are_alarms_and_other_groups_are_not() {
        let events = vec![
            json!({"id": 4720, "at": "2026-11-19 09:00:00", "user": "helper", "by": "me"}),
            json!({"id": 4732, "at": "2026-11-19 09:01:00", "member": "S-1-5-21-1", "group": "S-1-5-32-544", "by": "me"}),
            json!({"id": 4732, "at": "2026-11-19 09:02:00", "member": "S-1-5-21-1", "group": "S-1-5-32-555", "by": "me"}),
            json!({"id": 4624, "at": "2026-11-19 09:03:00", "user": "me", "address": "192.0.2.4"}),
        ];
        let kinds: Vec<String> = signin_things(&events).into_iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec!["account-made", "admin-join", "remote-signin"]);
    }

    #[test]
    fn a_journal_line_round_trips_and_carries_its_thing() {
        let mut thing = state_thing("firewall", "Public", ALARM);
        thing.fix = Some(("firewall-on".into(), None));
        let line = serde_json::to_string(&Entry::Thing { at: 1, cause: "tick".into(), thing: Carried::of(&thing) }).unwrap();
        assert!(line.contains("\"kind\":\"thing\""));
        let back: Entry = serde_json::from_str(&line).unwrap();
        let Entry::Thing { thing: carried, .. } = back else { panic!() };
        assert_eq!(carried.thing(), thing);
        assert_eq!(carried.thing().fingerprint(), thing.fingerprint());
        let action = serde_json::to_string(&Entry::Action { at: 2, mv: "firewall-on".into(), ok: true, result: "on".into(), print: None }).unwrap();
        assert!(action.contains("\"move\":\"firewall-on\""));
    }

    #[test]
    fn the_journal_is_cut_to_its_newer_half_past_the_cap() {
        let dir = std::env::temp_dir().join(format!("castle-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = "x".repeat(200);
        let lines: Vec<String> = (0..3000).map(|i| format!("{{\"kind\":\"problem\",\"at\":{i},\"line\":\"{big}\"}}")).collect();
        std::fs::write(journal(&dir), lines.join("\n") + "\n").unwrap();
        append(&dir, &[Entry::Problem { at: 9999, line: "new".into() }]);
        let text = std::fs::read_to_string(journal(&dir)).unwrap();
        assert_eq!(text.lines().count(), 1501, "the newer half and the new line");
        assert!(text.starts_with("{\"kind\":\"problem\",\"at\":1500,"));
        assert!(text.trim_end().ends_with("\"line\":\"new\"}"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): the book's
    /// starting picture, settling, the member's click (fresh ten minutes, spent once), accepting,
    /// and the watch's journal read in once.
    #[test]
    #[ignore]
    fn live_the_book_the_click_and_the_journal() {
        use mysql::prelude::Queryable;
        let spec = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &admin, &admin_pw, &format!("Sentinel Test {}", std::process::id())).unwrap();
        let mut conn = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut conn).unwrap();
        let covered: HashSet<String> = ["port", "task", "firewall"].iter().map(|k| k.to_string()).collect();

        // the starting picture, and a state thing that opens anyway
        let port80 = Thing { kind: "port".into(), parts: vec!["80".into(), "every".into(), "web.exe".into()], hint: LOOK, title: "Port 80".into(), detail: String::new(), fix: Some(("block-port".into(), Some("80".into()))), inventory: true };
        let mut firewall = state_thing("firewall", "Public", ALARM);
        firewall.fix = Some(("firewall-on".into(), None));
        let first = book::collect(&mut conn, &[port80.clone(), firewall.clone()], &covered, true).unwrap();
        assert_eq!((first.baseline, first.opened.len()), (1, 1));
        let alarm_id = first.opened[0];
        // a new task after the picture opens
        let task = Thing { kind: "task".into(), parts: vec!["\\Stranger".into(), "s.exe".into()], hint: LOOK, title: "Task".into(), detail: String::new(), fix: Some(("disable-task".into(), Some("\\Stranger".into()))), inventory: true };
        let second = book::collect(&mut conn, &[port80.clone(), firewall.clone(), task.clone()], &covered, true).unwrap();
        assert_eq!(second.opened.len(), 1);
        let task_id = second.opened[0];

        // settling: a reason, never an alarm
        assert!(book::settle(&mut conn, alarm_id, "fine").is_err());
        book::settle(&mut conn, task_id, "the member's own updater").unwrap();
        // settled outlives going away and coming back
        book::collect(&mut conn, &[port80.clone(), firewall.clone()], &covered, true).unwrap();
        book::collect(&mut conn, &[port80.clone(), firewall.clone(), task.clone()], &covered, true).unwrap();
        assert_eq!(book::finding(&mut conn, task_id).unwrap().unwrap().state, book::SETTLED);

        // the click: none, fresh, spent once, stale
        assert!(!book::use_click(&mut conn, alarm_id, "accept").unwrap());
        assert!(book::accept(&mut conn, alarm_id).is_err(), "accepting needs the member's click");
        book::click(&mut conn, alarm_id, "fix").unwrap();
        assert!(book::use_click(&mut conn, alarm_id, "fix").unwrap());
        assert!(!book::use_click(&mut conn, alarm_id, "fix").unwrap(), "spent once");
        conn.exec_drop("INSERT INTO sentinel_clicks (finding_id, word, clicked_at) VALUES (?, 'fix', NOW() - INTERVAL 11 MINUTE)", (alarm_id,)).unwrap();
        assert!(!book::use_click(&mut conn, alarm_id, "fix").unwrap(), "older than ten minutes");
        assert!(book::click(&mut conn, alarm_id, "format").is_err());

        // the journal: the reflex fixed the firewall while Castle was closed; read in once
        let dir = std::env::temp_dir().join(format!("castle-sentinel-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let domain = {
            let mut t = state_thing("firewall", "Domain", ALARM);
            t.fix = Some(("firewall-on".into(), None));
            t
        };
        append(&dir, &[
            Entry::Thing { at: 1, cause: "tick".into(), thing: Carried::of(&domain) },
            Entry::Action { at: 2, mv: "firewall-on".into(), ok: true, result: "Every firewall profile is on.".into(), print: Some(domain.fingerprint()) },
        ]);
        let drained = drain(&dir, &mut conn).unwrap();
        assert_eq!((drained.lines, drained.alarms.len()), (2, 1));
        let fixed: Option<String> = conn.exec_first("SELECT state FROM sentinel_findings WHERE fingerprint = ?", (domain.fingerprint(),)).unwrap();
        assert_eq!(fixed.as_deref(), Some(book::FIXED));
        assert_eq!(drain(&dir, &mut conn).unwrap(), Drained::default(), "read once");
        let reflex: Option<u64> = conn.query_first("SELECT COUNT(*) FROM sentinel_actions WHERE asked_by = 'reflex' AND state = 'done'").unwrap();
        assert_eq!(reflex, Some(1));

        // accepting with a click makes it known: a later full look never opens it again
        book::click(&mut conn, task_id, "accept").unwrap();
        book::accept(&mut conn, task_id).unwrap();
        let known: Option<String> = conn.exec_first("SELECT accepted_by FROM sentinel_known WHERE fingerprint = ?", (task.fingerprint(),)).unwrap();
        assert_eq!(known.as_deref(), Some("member"));
        let again = book::collect(&mut conn, &[port80, firewall, task], &covered, true).unwrap();
        assert!(again.opened.is_empty());

        // a SoftLanding task the member kept under its old, raw mark keeps its word under the steady mark
        let renewed = book::Thing {
            kind: "task".into(),
            parts: vec!["\\SoftLanding\\S-1-5-21-1-2-3-1002\\SoftLandingDeferralTask-128000000001627409-{724494fb-1111-2222-3333-444455556666}".into(), "{AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}".into()],
            hint: book::LOOK,
            title: "Scheduled task SoftLandingDeferralTask".into(),
            detail: String::new(),
            fix: None,
            inventory: true,
        };
        let old = renewed.legacy_print().unwrap();
        conn.exec_drop("INSERT INTO sentinel_known (fingerprint, kind, title, accepted_by) VALUES (?, 'task', 'old mark', 'member')", (old,)).unwrap();
        let after = book::collect(&mut conn, std::slice::from_ref(&renewed), &covered, true).unwrap();
        assert!(after.opened.is_empty(), "nothing raised on the update");
        let carried: Option<String> = conn.exec_first("SELECT accepted_by FROM sentinel_known WHERE fingerprint = ?", (renewed.fingerprint(),)).unwrap();
        assert_eq!(carried.as_deref(), Some("member"));

        let _ = std::fs::remove_dir_all(&dir);
        conn.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
        drop(conn);
        let opts = mysql::OptsBuilder::new().ip_or_hostname(Some(crate::memdb::HOST)).tcp_port(port).user(Some(admin.as_str())).pass(Some(admin_pw.as_str()));
        let mut root = mysql::Conn::new(opts).unwrap();
        root.query_drop(format!("DROP USER IF EXISTS '{0}'@'localhost', '{0}'@'127.0.0.1', '{0}'@'%'", access.user)).unwrap();
    }

    #[test]
    fn a_time_reads_as_seconds_and_differences_hold() {
        assert_eq!(seconds_of("1970-01-01 00:00:00"), Some(0));
        assert_eq!(seconds_of("2026-11-20 00:00:00").unwrap() - seconds_of("2026-11-19 23:59:00").unwrap(), 60);
        assert_eq!(seconds_of("2024-03-01 00:00:00").unwrap() - seconds_of("2024-02-28 00:00:00").unwrap(), 2 * 86_400, "a leap year");
        assert_eq!(seconds_of("nonsense"), None);
    }
}
