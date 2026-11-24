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
