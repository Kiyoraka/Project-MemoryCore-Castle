//! The Aegis Network: one castle's sighting protects every castle. A joined Sentinel reports what
//! it caught (an attacker's address, a detection's name - never anything about the member or their
//! files) to the Portfolio, which keeps one shared watch list: an address is listed once two
//! castles reported it within a day. Every joined Sentinel reads the list and closes its own door
//! to a listed address for a day. Off until the member joins in Settings > Sentinel.
//!
//! The doors are the Portfolio's (`/api/room/aegis/**`, its contract page), called with the
//! member's room key the way the chat room is.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a held read may wait at the Portfolio (its own limit), and the request's own timeout.
pub const HELD_WAIT: u64 = 25;
const HELD_TIMEOUT: Duration = Duration::from_secs(HELD_WAIT + 10);
const REPORT_TIMEOUT: Duration = Duration::from_secs(15);
/// At most this many items in one report (the contract's limit).
pub const MAX_ITEMS: usize = 50;
const USER_AGENT: &str = concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION"));

/// One thing reported: an address (with why) or a detection's name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub kind: &'static str,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'static str>,
}

impl Item {
    pub fn burst(address: &str) -> Item {
        Item { kind: "address", value: address.trim().to_string(), why: Some("burst") }
    }
    pub fn blocked(address: &str) -> Item {
        Item { kind: "address", value: address.trim().to_string(), why: Some("blocked") }
    }
    pub fn detection(name: &str) -> Item {
        Item { kind: "detection", value: name.trim().to_string(), why: None }
    }
}

/// A line the Portfolio refused, and why (private, reserved, provider).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Refused {
    pub value: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Reported {
    #[serde(default)]
    pub accepted: u32,
    #[serde(default)]
    pub refused: Vec<Refused>,
}

/// One entry of the shared watch list.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Listed {
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub castles: u32,
    #[serde(default)]
    pub first_seen: String,
    #[serde(default)]
    pub last_seen: String,
    #[serde(default)]
    pub expires: String,
}

/// The whole list at its version. Empty items with the version asked about means nothing new.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct List {
    pub version: i64,
    #[serde(default)]
    pub items: Vec<Listed>,
}

/// Why the Portfolio could not be used this time.
#[derive(Debug, Clone, PartialEq)]
pub enum Away {
    /// no answer (offline, a timeout)
    Offline,
    /// 401 / 403: not a member, or the key is gone
    Locked,
    /// 429: too many reports or reads this hour
    SlowDown,
    /// 400: the report's shape was refused (nothing of it was stored)
    Refused(String),
    /// anything else
    Other(u16),
}

impl Away {
    /// How long the listening loop rests before asking again.
    pub fn rest(&self) -> Duration {
        match self {
            Away::Locked => Duration::from_secs(60),
            Away::SlowDown => Duration::from_secs(300),
            Away::Offline | Away::Refused(_) | Away::Other(_) => Duration::from_secs(10),
        }
    }
}

fn away_for(status: u16, body: &str) -> Away {
    match status {
        401 | 403 => Away::Locked,
        429 => Away::SlowDown,
        400 => Away::Refused(serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v["error"].as_str().map(str::to_string)).unwrap_or_default()),
        other => Away::Other(other),
    }
}

/// The Portfolio's network doors, with the member's room key.
pub struct Door {
    base: String,
    key: String,
}

impl Door {
    pub fn new(base: &str, key: &str) -> Door {
        Door { base: base.to_string(), key: key.to_string() }
    }

    fn client(timeout: Duration) -> Result<reqwest::blocking::Client, Away> {
        reqwest::blocking::Client::builder().user_agent(USER_AGENT).timeout(timeout).build().map_err(|_| Away::Offline)
    }

    /// Reports what was caught (at most MAX_ITEMS; the rest is left for the next catch).
    pub fn report(&self, items: &[Item]) -> Result<Reported, Away> {
        let items = &items[..items.len().min(MAX_ITEMS)];
        let response = Self::client(REPORT_TIMEOUT)?
            .post(crate::chatroom::room_url(&self.base, "/aegis/reports"))
            .bearer_auth(&self.key)
            .json(&serde_json::json!({ "items": items }))
            .send()
            .map_err(|_| Away::Offline)?;
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(away_for(status, &body));
        }
        serde_json::from_str(&body).map_err(|_| Away::Other(status))
    }

    /// The watch list: the whole of it when `since` is None (a catch-up), else held up to `wait`
    /// seconds until it changes.
    pub fn list(&self, since: Option<i64>, wait: u64) -> Result<List, Away> {
        let wait = wait.min(HELD_WAIT);
        let path = match since {
            Some(v) => format!("/aegis/list?since={v}&wait={wait}"),
            None => "/aegis/list".to_string(),
        };
        let response = Self::client(if since.is_some() && wait > 0 { HELD_TIMEOUT } else { REPORT_TIMEOUT })?
            .get(crate::chatroom::room_url(&self.base, &path))
            .bearer_auth(&self.key)
            .send()
            .map_err(|_| Away::Offline)?;
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(away_for(status, &body));
        }
        serde_json::from_str(&body).map_err(|_| Away::Other(status))
    }
}

/// A value reported within this long is not reported again (the Portfolio already holds it).
pub const REPORT_AGAIN_HOURS: u32 = 6;

/// A finding the Sentinel opened that may be reported: its id, kind, title and fix argument.
pub type FindingRow = (u64, String, String, Option<String>);

/// What is reported from what the Sentinel recorded: a failed-sign-in burst's address, a detection's
/// name (never the anti-virus test file's), and an address the hand blocked. Nothing else.
pub fn items_from(findings: &[FindingRow], blocks: &[(u64, String)]) -> Vec<Item> {
    let mut items = Vec::new();
    for (_, kind, title, fix_arg) in findings {
        match kind.as_str() {
            "signin-burst" => {
                if let Some(address) = fix_arg.as_deref().filter(|a| !a.trim().is_empty()) {
                    items.push(Item::burst(address));
                }
            }
            "detection" => {
                let threat = title.strip_prefix(super::look::DETECTION_TITLE).unwrap_or("").trim();
                if !threat.is_empty() && !threat.starts_with('(') && !is_drill_name(threat) {
                    items.push(Item::detection(threat));
                }
            }
            _ => {}
        }
    }
    for (_, address) in blocks {
        if !address.trim().is_empty() {
            items.push(Item::blocked(address));
        }
    }
    let mut seen = std::collections::HashSet::new();
    items.retain(|i| seen.insert(mark(i)));
    items
}

/// The anti-virus test file (EICAR) is a drill: it is never shared.
pub fn is_drill_name(name: &str) -> bool {
    name.to_ascii_lowercase().contains("eicar")
}

/// What an item is known by in `sentinel_network_sent`.
pub fn mark(item: &Item) -> String {
    super::book::print_of("network", &[item.kind, item.value.as_str()])
}

fn state(conn: &mut mysql::Conn, name: &str) -> Option<String> {
    use mysql::prelude::Queryable;
    conn.exec_first("SELECT value FROM sentinel_network_state WHERE name = ?", (name,)).ok().flatten()
}

fn set_state(conn: &mut mysql::Conn, name: &str, value: &str) {
    use mysql::prelude::Queryable;
    let _ = conn.exec_drop("INSERT INTO sentinel_network_state (name, value) VALUES (?, ?) ON DUPLICATE KEY UPDATE value = VALUES(value)", (name, value));
}

fn cursor(conn: &mut mysql::Conn, name: &str) -> Option<u64> {
    state(conn, name).and_then(|v| v.parse().ok())
}

/// Reports what the Sentinel caught since the last report: findings and blocks after the cursors,
/// less what was sent in the last REPORT_AGAIN_HOURS. Joining never sends the past: the first time,
/// the cursors start at what is already there. A failure leaves the cursors for the next minute,
/// except a refused shape, which no retry would mend.
pub fn report_caught(conn: &mut mysql::Conn, door: &Door) -> Result<usize, String> {
    use mysql::prelude::Queryable;
    let unread = |_| "The network's report could not read the Sentinel's book.".to_string();
    let max_finding: u64 = conn.query_first("SELECT COALESCE(MAX(id), 0) FROM sentinel_findings").map_err(unread)?.unwrap_or(0);
    let max_action: u64 = conn.query_first("SELECT COALESCE(MAX(id), 0) FROM sentinel_actions").map_err(unread)?.unwrap_or(0);
    let advance = |conn: &mut mysql::Conn| {
        set_state(conn, "reported_finding", &max_finding.to_string());
        set_state(conn, "reported_action", &max_action.to_string());
    };
    let (Some(from_finding), Some(from_action)) = (cursor(conn, "reported_finding"), cursor(conn, "reported_action")) else {
        advance(conn);
        return Ok(0);
    };
    let findings: Vec<FindingRow> = conn
        .exec("SELECT id, kind, title, fix_arg FROM sentinel_findings WHERE id > ? AND id <= ? AND kind IN ('signin-burst', 'detection') ORDER BY id", (from_finding, max_finding))
        .map_err(unread)?;
    let blocks: Vec<(u64, String)> = conn
        .exec("SELECT id, arg FROM sentinel_actions WHERE id > ? AND id <= ? AND move = 'block-address' AND state = 'done' AND asked_by <> 'network' AND arg IS NOT NULL ORDER BY id", (from_action, max_action))
        .map_err(unread)?;
    let recent: std::collections::HashSet<String> = conn
        .exec("SELECT fingerprint FROM sentinel_network_sent WHERE sent_at > NOW() - INTERVAL ? HOUR", (REPORT_AGAIN_HOURS,))
        .map_err(unread)?
        .into_iter()
        .collect();
    let items: Vec<Item> = items_from(&findings, &blocks).into_iter().filter(|i| !recent.contains(&mark(i))).take(MAX_ITEMS).collect();
    if items.is_empty() {
        advance(conn);
        return Ok(0);
    }
    match door.report(&items) {
        Ok(reported) => {
            for item in &items {
                let _ = conn.exec_drop(
                    "INSERT INTO sentinel_network_sent (fingerprint, kind, value) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE sent_at = NOW()",
                    (mark(item), item.kind, &item.value),
                );
            }
            set_state(conn, "last_refused", &serde_json::to_string(&reported.refused).unwrap_or_default());
            advance(conn);
            Ok(items.len())
        }
        Err(Away::Refused(why)) => {
            advance(conn);
            Err(format!("The Aegis Network refused the report: {why}"))
        }
        Err(away) => Err(format!("The Aegis Network could not be reached ({away:?}); the report waits.")),
    }
}

/// A documentation address (192.0.2/24, 198.51.100/24, 203.0.113/24, 2001:db8::/32): never a real
/// attacker, so the list's copy of one is acted on only by a debug build's drill.
pub fn is_documentation(address: &str) -> bool {
    match address.trim().parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            let o = v4.octets();
            matches!((o[0], o[1], o[2]), (192, 0, 2) | (198, 51, 100) | (203, 0, 113))
        }
        Ok(std::net::IpAddr::V6(v6)) => v6.segments()[0] == 0x2001 && v6.segments()[1] == 0x0db8,
        Err(_) => false,
    }
}

/// An address on the public internet (the Portfolio refuses the rest; this is the castle's own
/// second look before its hand is asked, beside the hand's own refusals).
pub fn is_public(address: &str) -> bool {
    if is_documentation(address) {
        return false;
    }
    match address.trim().parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            let o = v4.octets();
            !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_multicast() || v4.is_broadcast() || v4.is_unspecified() || o[0] == 0 || o[0] >= 240 || (o[0] == 100 && (64..128).contains(&o[1])) || (o[0] == 198 && (o[1] == 18 || o[1] == 19)))
        }
        Ok(std::net::IpAddr::V6(v6)) => {
            let first = v6.segments()[0];
            !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80) && (first & 0xe000) == 0x2000
        }
        Err(_) => false,
    }
}

/// Whether a debug build's drill may act on documentation addresses (CASTLE_AEGIS_DRILL=1). A
/// release never does.
pub fn drill_on() -> bool {
    cfg!(debug_assertions) && std::env::var("CASTLE_AEGIS_DRILL").is_ok_and(|v| v == "1")
}

/// The listed addresses to block now: public (or, in a drill, a documentation address), not yet
/// expired. The castle's own refusals (this PC, its router) are the hand's.
pub fn to_block(list: &List, now_ms: i64, drill: bool) -> Vec<Listed> {
    list.items
        .iter()
        .filter(|i| i.kind == "address")
        .filter(|i| crate::quota::iso_ms(&i.expires).is_none_or(|at| at > now_ms))
        .filter(|i| is_public(&i.value) || (drill && is_documentation(&i.value)))
        .cloned()
        .collect()
}

/// The listed detections: a warning each, never a block.
pub fn warnings(list: &List) -> Vec<Listed> {
    list.items.iter().filter(|i| i.kind == "detection" && !i.value.trim().is_empty() && !is_drill_name(&i.value)).cloned().collect()
}

/// What one reading of the list did.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    pub blocked: u32,
    pub warned: u32,
}

/// Acts on the list: each address the network has not blocked in the last day, and the member has
/// not undone, is blocked for a day through the hand (its own card, settled with Undo); each
/// detection becomes a warning card. The hand still refuses this computer, its router and loopback.
pub fn apply_list(conn: &mut mysql::Conn, list: &List, drill: bool) -> Applied {
    use mysql::prelude::Queryable;
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let mut applied = Applied::default();
    for item in to_block(list, now_ms, drill) {
        let recent: Option<u64> = conn
            .exec_first(
                "SELECT id FROM sentinel_actions WHERE asked_by = 'network' AND move = 'block-address' AND arg = ? AND ((state = 'done' AND done_at > NOW() - INTERVAL 23 HOUR) OR undone_at > NOW() - INTERVAL 1 DAY) LIMIT 1",
                (&item.value,),
            )
            .ok()
            .flatten();
        if recent.is_some() {
            continue;
        }
        let lifts = (chrono::Local::now() + chrono::Duration::hours(24)).format("%a %-I:%M %p").to_string();
        let title = format!("Blocked by the Aegis Network: {}", item.value);
        let detail = format!("{} castles reported this address in the last day. The block lifts {lifts}.", item.castles.max(2));
        let print = super::book::print_of("network-address", &[item.value.as_str()]);
        let opened = conn.exec_drop(
            "INSERT INTO sentinel_findings (fingerprint, kind, hint, title, detail, fix_move, fix_arg) VALUES (?, 'network-address', 'look', ?, ?, 'block-address', ?) ON DUPLICATE KEY UPDATE state = 'open', title = VALUES(title), detail = VALUES(detail), last_seen = NOW(), closed_at = NULL",
            (&print, &title, &detail, &item.value),
        );
        if opened.is_err() {
            continue;
        }
        let Ok(Some(finding)) = conn.exec_first::<u64, _, _>("SELECT id FROM sentinel_findings WHERE fingerprint = ?", (&print,)) else { continue };
        if super::officer::act(conn, super::hand::Asker::Network, "block-address", Some(&item.value), Some(finding)).is_ok_and(|a| a.ok) {
            applied.blocked += 1;
        }
    }
    for item in warnings(list) {
        let print = super::book::print_of("network-detection", &[item.value.as_str()]);
        let title = format!("Other castles saw {} this week", item.value);
        let detail = "Two or more castles' antivirus found this. Nothing was found here; the Sentinel only says so.".to_string();
        let new = conn.exec_drop(
            "INSERT IGNORE INTO sentinel_findings (fingerprint, kind, hint, title, detail) VALUES (?, 'network-detection', 'note', ?, ?)",
            (&print, &title, &detail),
        );
        if new.is_ok() && conn.affected_rows() > 0 {
            applied.warned += 1;
        }
    }
    applied
}

/// How many addresses the network blocked in the last day (the sheet's line).
pub fn blocked_today(conn: &mut mysql::Conn) -> u32 {
    use mysql::prelude::Queryable;
    conn.query_first("SELECT COUNT(*) FROM sentinel_actions WHERE asked_by = 'network' AND state = 'done' AND done_at > NOW() - INTERVAL 1 DAY").ok().flatten().unwrap_or(0)
}

/// Listens to the shared list while the member is joined: one catch-up ask when Castle starts,
/// when the member joins again and after the computer slept, then one held read after another.
/// Off, or signed out, it asks nothing and looks again in a while.
pub fn listen(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let app = app.clone();
    let _ = std::thread::Builder::new().name("aegis-network".into()).spawn(move || {
        let mut version: Option<i64> = None;
        let mut last = std::time::Instant::now();
        loop {
            let settings = app.state::<crate::settings::SettingsStore>().get();
            let (Some(door), Some(memory)) = (door(&settings), settings.memory_folder.clone()) else {
                version = None; // joining again starts with a catch-up
                std::thread::sleep(Duration::from_secs(15));
                continue;
            };
            // a long gap means the computer slept: the news it missed comes with a catch-up
            if last.elapsed() > Duration::from_secs(600) {
                version = None;
            }
            let answer = door.list(version, if version.is_some() { HELD_WAIT } else { 0 });
            last = std::time::Instant::now();
            match answer {
                Ok(list) => {
                    let changed = version != Some(list.version);
                    version = Some(list.version);
                    if changed {
                        if let Ok(mut conn) = super::book::open(std::path::Path::new(&memory)) {
                            let done = apply_list(&mut conn, &list, drill_on());
                            if done.blocked + done.warned > 0 {
                                super::officer::log(&format!("\nthe Aegis Network's list: {} blocked for a day, {} warning(s)\n", done.blocked, done.warned));
                                let _ = app.emit(super::CHANGED, ());
                            }
                        }
                    }
                }
                Err(away) => std::thread::sleep(away.rest()),
            }
        }
    });
}

/// The door for this member, when they joined and are signed in to the Foundation.
pub fn door(settings: &crate::settings::Settings) -> Option<Door> {
    if !settings.sentinel.network {
        return None;
    }
    let key = crate::key::load("member")?;
    Some(Door::new(&crate::signin::portfolio(), &key))
}

/// Whether the member is signed in to the Foundation (joining needs the room key).
pub fn signed_in() -> bool {
    crate::key::load("member").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A pretend Portfolio answering one request with `status` and `body`; the request it saw.
    fn portfolio(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|s| s.trim().parse::<usize>().ok())).unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (base, handle)
    }

    #[test]
    fn only_a_burst_a_detection_and_a_block_are_reported_and_never_the_drill() {
        let findings: Vec<FindingRow> = vec![
            (1, "signin-burst".into(), "Many failed sign-ins from 185.220.101.4".into(), Some("185.220.101.4".into())),
            (2, "detection".into(), "The antivirus found Trojan:Win32/Example!ml".into(), Some(String::new())),
            (3, "detection".into(), "The antivirus found Virus:DOS/EICAR_Test_File".into(), None),
            (4, "detection".into(), "The antivirus found (none named)".into(), None),
            (5, "task".into(), "Scheduled task \\Stranger".into(), Some("\\Stranger".into())),
            (6, "signin-burst".into(), "Many failed sign-ins from 185.220.101.4".into(), Some("185.220.101.4".into())),
        ];
        let items = items_from(&findings, &[(9, "45.155.205.233".into())]);
        assert_eq!(items, vec![Item::burst("185.220.101.4"), Item::detection("Trojan:Win32/Example!ml"), Item::blocked("45.155.205.233")]);
        assert!(is_drill_name("Virus:DOS/EICAR_Test_File"));
        assert_ne!(mark(&Item::burst("1.2.3.4")), mark(&Item::detection("1.2.3.4")));
    }

    #[test]
    fn a_report_carries_only_what_was_caught_with_the_room_key() {
        let (base, seen) = portfolio(200, r#"{"accepted":2,"refused":[{"value":"10.0.0.1","reason":"private"}]}"#);
        let got = Door::new(&base, "mcr_test").report(&[Item::burst("185.220.101.4"), Item::detection("Trojan:Win32/Example!ml"), Item::blocked("10.0.0.1")]).unwrap();
        assert_eq!(got.accepted, 2);
        assert_eq!(got.refused, vec![Refused { value: "10.0.0.1".into(), reason: "private".into() }]);
        let request = seen.join().unwrap();
        assert!(request.starts_with("POST /api/room/aegis/reports "));
        assert!(request.to_lowercase().contains("authorization: bearer mcr_test"));
        let body: serde_json::Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["items"][0], serde_json::json!({"kind":"address","value":"185.220.101.4","why":"burst"}));
        assert_eq!(body["items"][1], serde_json::json!({"kind":"detection","value":"Trojan:Win32/Example!ml"}), "a detection carries no why");
    }

    #[test]
    fn the_list_is_read_whole_at_first_and_held_after() {
        let (base, seen) = portfolio(200, r#"{"version":1764003600000,"items":[{"kind":"address","value":"185.220.101.4","castles":2,"first_seen":"2026-11-24T20:00:00Z","last_seen":"2026-11-24T21:00:00Z","expires":"2026-11-25T20:00:00Z"}]}"#);
        let list = Door::new(&base, "k").list(None, HELD_WAIT).unwrap();
        assert_eq!((list.version, list.items.len(), list.items[0].castles), (1764003600000, 1, 2));
        assert!(seen.join().unwrap().starts_with("GET /api/room/aegis/list "), "a catch-up asks for the whole list");
        let (base, seen) = portfolio(200, r#"{"version":7,"items":[]}"#);
        let list = Door::new(&base, "k").list(Some(7), 90).unwrap();
        assert!(list.items.is_empty(), "nothing new");
        assert!(seen.join().unwrap().starts_with("GET /api/room/aegis/list?since=7&wait=25 "), "the wait is never over the Portfolio's 25 s");
    }

    #[test]
    #[ignore]
    fn live_database_reports_only_the_new_catches_once() {
        use mysql::prelude::Queryable;
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Network Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();
        let finding = |c: &mut mysql::Conn, print: &str, kind: &str, title: &str, arg: Option<&str>| {
            c.exec_drop("INSERT INTO sentinel_findings (fingerprint, kind, hint, title, detail, fix_move, fix_arg) VALUES (?, ?, 'alarm', ?, '', NULL, ?)", (print, kind, title, arg)).unwrap();
        };
        // what was there before joining is never sent
        finding(&mut c, "old", "signin-burst", "Many failed sign-ins from 91.1.1.1", Some("91.1.1.1"));
        let nowhere = Door::new("http://127.0.0.1:9", "k");
        assert_eq!(report_caught(&mut c, &nowhere), Ok(0));
        // a new burst and a detection go in one report
        finding(&mut c, "b1", "signin-burst", "Many failed sign-ins from 185.220.101.4", Some("185.220.101.4"));
        finding(&mut c, "d1", "detection", "The antivirus found Trojan:Win32/Example!ml", None);
        let (base, seen) = portfolio(200, r#"{"accepted":2,"refused":[]}"#);
        assert_eq!(report_caught(&mut c, &Door::new(&base, "k")), Ok(2));
        let body = seen.join().unwrap();
        assert!(body.contains("185.220.101.4") && body.contains("Trojan:Win32/Example!ml") && !body.contains("91.1.1.1"));
        // the same address caught again within six hours is not sent (no request is made)
        finding(&mut c, "b2", "signin-burst", "Many failed sign-ins from 185.220.101.4", Some("185.220.101.4"));
        assert_eq!(report_caught(&mut c, &nowhere), Ok(0));
        // offline: the cursors wait, so the catch is sent on a later minute
        finding(&mut c, "b3", "signin-burst", "Many failed sign-ins from 45.155.205.233", Some("45.155.205.233"));
        assert!(report_caught(&mut c, &nowhere).unwrap_err().contains("waits"));
        let (base, seen) = portfolio(200, r#"{"accepted":1,"refused":[]}"#);
        assert_eq!(report_caught(&mut c, &Door::new(&base, "k")), Ok(1));
        assert!(seen.join().unwrap().contains("45.155.205.233"));
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }

    #[test]
    fn only_a_public_unexpired_address_is_blocked_and_a_drill_only_in_a_drill() {
        let now = crate::quota::iso_ms("2026-11-25T02:00:00Z").unwrap();
        let entry = |kind: &str, value: &str, expires: &str| Listed { kind: kind.into(), value: value.into(), castles: 2, first_seen: String::new(), last_seen: String::new(), expires: expires.into() };
        let list = List {
            version: 1,
            items: vec![
                entry("address", "185.220.101.4", "2026-11-25T20:00:00Z"),
                entry("address", "45.155.205.233", "2026-11-25T01:00:00Z"),
                entry("address", "10.0.0.1", "2026-11-25T20:00:00Z"),
                entry("address", "203.0.113.7", "2026-11-25T20:00:00Z"),
                entry("detection", "Trojan:Win32/Example!ml", "2026-11-25T20:00:00Z"),
                entry("detection", "Virus:DOS/EICAR_Test_File", "2026-11-25T20:00:00Z"),
            ],
        };
        let values = |v: Vec<Listed>| v.into_iter().map(|i| i.value).collect::<Vec<_>>();
        assert_eq!(values(to_block(&list, now, false)), vec!["185.220.101.4"], "expired, private and documentation addresses are left");
        assert_eq!(values(to_block(&list, now, true)), vec!["185.220.101.4", "203.0.113.7"], "a drill acts on the documentation address too");
        assert_eq!(values(warnings(&list)), vec!["Trojan:Win32/Example!ml"], "a detection is a warning, never the test file");
        assert!(is_documentation("203.0.113.7") && is_documentation("2001:db8::1") && !is_documentation("185.220.101.4"));
        for private in ["127.0.0.1", "192.168.1.1", "169.254.0.1", "100.64.0.1", "0.1.2.3", "224.0.0.1", "::1", "fe80::1", "fd00::1"] {
            assert!(!is_public(private), "{private}");
        }
        assert!(is_public("185.220.101.4") && is_public("2a01:4f8::1"));
        assert!(!drill_on() || cfg!(debug_assertions), "a release never drills");
    }

    #[test]
    fn a_refusal_says_why_and_how_long_to_rest() {
        let (base, _) = portfolio(403, r#"{"error":"This room is for MemoryCore Foundation members"}"#);
        assert_eq!(Door::new(&base, "k").list(None, 0), Err(Away::Locked));
        let (base, _) = portfolio(429, r#"{"error":"slow down"}"#);
        assert_eq!(Door::new(&base, "k").report(&[Item::burst("185.220.101.4")]).unwrap_err(), Away::SlowDown);
        let (base, _) = portfolio(400, r#"{"error":"Not an address"}"#);
        assert_eq!(Door::new(&base, "k").report(&[Item::burst("x")]).unwrap_err(), Away::Refused("Not an address".into()));
        assert_eq!(Away::SlowDown.rest(), Duration::from_secs(300));
        assert_eq!(Away::Locked.rest(), Duration::from_secs(60));
        assert_eq!(Away::Offline.rest(), Duration::from_secs(10));
        assert_eq!(Door::new("http://127.0.0.1:9", "k").list(None, 0), Err(Away::Offline));
    }
}
