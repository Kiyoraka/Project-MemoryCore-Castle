//! Google Safe Browsing for the BROWSER window, the private way (the Update API v4).
//!
//! Castle keeps Google's lists of dangerous sites on this computer as short hash prefixes (four
//! bytes of a SHA-256), refreshed no more often than Google asks. Every address a page goes to is
//! turned into its few host-and-path expressions and hashed here; only when a hash starts with a
//! listed prefix is Google asked, and then only for that short prefix, never the address. The key is
//! the member's own (Settings > Keys, "Google Safe Browsing"); without one this is quietly off and
//! the other guards still run.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

/// The member's key in Castle's keys (key.rs).
pub const KEY: &str = "google-safe-browsing";
const BASE: &str = "https://safebrowsing.googleapis.com/v4";
/// The kinds of danger Castle asks for.
const THREATS: [&str; 4] = ["SOCIAL_ENGINEERING", "MALWARE", "UNWANTED_SOFTWARE", "POTENTIALLY_HARMFUL_APPLICATION"];

/// What Google found on an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threat {
    Phishing,
    Malware,
    Unwanted,
    Harmful,
}

impl Threat {
    /// The word the DANGER screen shows.
    pub fn word(self) -> &'static str {
        match self {
            Threat::Phishing => "phishing",
            Threat::Malware => "malware",
            Threat::Unwanted => "unwanted",
            Threat::Harmful => "harmful",
        }
    }
}

pub fn threat_of(name: &str) -> Option<Threat> {
    match name {
        "SOCIAL_ENGINEERING" => Some(Threat::Phishing),
        "MALWARE" => Some(Threat::Malware),
        "UNWANTED_SOFTWARE" => Some(Threat::Unwanted),
        "POTENTIALLY_HARMFUL_APPLICATION" => Some(Threat::Harmful),
        _ => None,
    }
}

// ---------- the address, canonicalised by the spec (pure) ----------

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-unescapes until nothing changes.
fn unescape_all(bytes: &[u8]) -> Vec<u8> {
    let mut now = bytes.to_vec();
    loop {
        let mut out = Vec::with_capacity(now.len());
        let mut i = 0;
        while i < now.len() {
            if now[i] == b'%' && i + 2 < now.len() {
                if let (Some(h), Some(l)) = (hex(now[i + 1]), hex(now[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
            }
            out.push(now[i]);
            i += 1;
        }
        if out == now {
            return out;
        }
        now = out;
    }
}

/// Escapes the bytes the spec names: control and space, non-ASCII, `#` and `%`, in capital hex.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b <= 0x20 || b >= 0x7f || b == b'#' || b == b'%' {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// One part of an IPv4 address the way inet_aton reads it: decimal, 0x hex or leading-0 octal.
fn ip_part(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let (digits, radix) = if let Some(h) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        (h, 16)
    } else if part.len() > 1 && part.starts_with('0') {
        (&part[1..], 8)
    } else {
        (part, 10)
    };
    if digits.is_empty() {
        return if radix == 16 { None } else { Some(0) };
    }
    u64::from_str_radix(digits, radix).ok()
}

/// A host that is an IPv4 address in any of its spellings, as four dotted numbers.
fn ipv4(host: &str) -> Option<String> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let values: Vec<u64> = parts.iter().map(|p| ip_part(p)).collect::<Option<_>>()?;
    let (head, last) = values.split_at(values.len() - 1);
    if head.iter().any(|v| *v > 255) {
        return None;
    }
    let room = 4 - head.len();
    if last[0] >= 1u64 << (8 * room) {
        return None;
    }
    let mut bytes: Vec<u64> = head.to_vec();
    for i in (0..room).rev() {
        bytes.push((last[0] >> (8 * i)) & 0xff);
    }
    Some(bytes.iter().map(u64::to_string).collect::<Vec<_>>().join("."))
}

/// The path with `.` and `..` resolved and runs of slashes made one; a trailing slash stays.
fn clean_path(path: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    let segments: Vec<&[u8]> = path.split(|b| *b == b'/').collect();
    let mut trailing = path.ends_with(b"/");
    for (i, seg) in segments.iter().enumerate() {
        let last = i == segments.len() - 1;
        match *seg {
            b"" => {}
            b"." => trailing |= last,
            b".." => {
                parts.pop();
                trailing |= last;
            }
            s => parts.push(s),
        }
    }
    let mut out = b"/".to_vec();
    out.extend(parts.join(&b'/'));
    if trailing && !parts.is_empty() {
        out.push(b'/');
    }
    out
}

/// An address as the spec canonicalises it: (host, path, query). The scheme is not part of what is
/// hashed, the port and the fragment go, the host is lower-cased with its dots tidied and an IP
/// written as four numbers, the path is resolved, and the rest is escaped.
pub fn canonical(url: &str) -> Option<(String, String, Option<String>)> {
    let cleaned: String = url.chars().filter(|c| !matches!(c, '\t' | '\r' | '\n')).collect();
    let cleaned = cleaned.trim_matches(|c: char| c == ' ' || c.is_ascii_control());
    let no_fragment = cleaned.split('#').next().unwrap_or("");
    let rest = match no_fragment.find("://") {
        Some(at) if no_fragment[..at].chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) => &no_fragment[at + 3..],
        _ => no_fragment,
    };
    let (before_query, query) = match rest.find('?') {
        Some(at) => (&rest[..at], Some(&rest[at + 1..])),
        None => (rest, None),
    };
    let (authority, path) = match before_query.find('/') {
        Some(at) => (&before_query[..at], &before_query[at..]),
        None => (before_query, "/"),
    };
    // no user info, no port
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host_raw = match authority.rfind(':') {
        Some(at) if authority[at + 1..].chars().all(|c| c.is_ascii_digit()) => &authority[..at],
        _ => authority,
    };
    let host_bytes = unescape_all(host_raw.as_bytes());
    let mut host = String::from_utf8_lossy(&host_bytes).to_ascii_lowercase();
    host = host.trim_matches('.').to_string();
    while host.contains("..") {
        host = host.replace("..", ".");
    }
    if host.is_empty() {
        return None;
    }
    let host = ipv4(&host).unwrap_or(host);
    let host = escape(host.as_bytes());
    let path = escape(&clean_path(&unescape_all(path.as_bytes())));
    let query = query.map(|q| escape(&unescape_all(q.as_bytes())));
    Some((host, path, query))
}

/// The host-and-path expressions Google lists, at most 5 hosts x 6 paths: the exact host and up to
/// four of its parents (never the top-level domain alone, never for an IP), each with the exact path
/// with and without its query and up to four of the path's leading folders.
pub fn expressions(url: &str) -> Vec<String> {
    let Some((host, path, query)) = canonical(url) else { return Vec::new() };
    let mut hosts = vec![host.clone()];
    if ipv4(&host).as_deref() != Some(host.as_str()) {
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() > 2 {
            for start in (parts.len().saturating_sub(5)).max(1)..parts.len() - 1 {
                hosts.push(parts[start..].join("."));
            }
        }
    }
    let mut paths: Vec<String> = Vec::new();
    if let Some(q) = &query {
        paths.push(format!("{path}?{q}"));
    }
    paths.push(path.clone());
    let folders: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let mut prefix = String::from("/");
    for (i, folder) in std::iter::once("").chain(folders.iter().take(folders.len().saturating_sub(1)).copied()).enumerate() {
        if i >= 4 {
            break;
        }
        if !folder.is_empty() {
            prefix.push_str(folder);
            prefix.push('/');
        }
        if !paths.contains(&prefix) {
            paths.push(prefix.clone());
        }
    }
    let mut out = Vec::new();
    for h in &hosts {
        for p in &paths {
            let e = format!("{h}{p}");
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

/// The SHA-256 of each expression.
pub fn full_hashes(url: &str) -> Vec<[u8; 32]> {
    expressions(url).iter().map(|e| Sha256::digest(e.as_bytes()).into()).collect()
}

// ---------- one list of prefixes (pure) ----------

/// One of Google's lists as prefixes, sorted the way Google numbers them for removals: four-byte
/// prefixes packed tightly (almost all of them), longer ones apart.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PrefixList {
    four: Vec<[u8; 4]>,
    long: Vec<Vec<u8>>,
}

impl PrefixList {
    pub fn len(&self) -> usize {
        self.four.len() + self.long.len()
    }

    /// Every prefix in Google's order (byte order), the two kinds merged.
    fn merged(&self) -> Vec<&[u8]> {
        let (mut a, mut b) = (0, 0);
        let mut out = Vec::with_capacity(self.len());
        while a < self.four.len() || b < self.long.len() {
            let take_four = b >= self.long.len() || (a < self.four.len() && self.four[a].as_slice() <= self.long[b].as_slice());
            if take_four {
                out.push(self.four[a].as_slice());
                a += 1;
            } else {
                out.push(self.long[b].as_slice());
                b += 1;
            }
        }
        out
    }

    /// The listed prefix a full hash starts with, if any.
    pub fn prefix_of(&self, hash: &[u8; 32]) -> Option<Vec<u8>> {
        let four: [u8; 4] = hash[..4].try_into().unwrap();
        if self.four.binary_search(&four).is_ok() {
            return Some(four.to_vec());
        }
        self.long.iter().find(|p| hash.starts_with(p)).cloned()
    }

    /// One update: the removals (indices into the list as it was), then the additions.
    pub fn apply(&mut self, removals: &[usize], additions: Vec<Vec<u8>>) -> Result<(), String> {
        let gone: std::collections::HashSet<usize> = removals.iter().copied().collect();
        if gone.iter().any(|i| *i >= self.len()) {
            return Err("a removal past the end of the list".into());
        }
        let kept: Vec<Vec<u8>> = self.merged().into_iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, p)| p.to_vec()).collect();
        let (mut four, mut long) = (Vec::new(), Vec::new());
        for p in kept.into_iter().chain(additions) {
            match p.len() {
                4 => four.push(<[u8; 4]>::try_from(p.as_slice()).unwrap()),
                5..=32 => long.push(p),
                _ => return Err("a prefix of the wrong size".into()),
            }
        }
        four.sort_unstable();
        four.dedup();
        long.sort_unstable();
        long.dedup();
        self.four = four;
        self.long = long;
        Ok(())
    }

    /// Google's checksum of the list: the SHA-256 of every prefix in order.
    pub fn checksum(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        for p in self.merged() {
            h.update(p);
        }
        h.finalize().into()
    }

    /// The list as a file: the four-byte prefixes, then each longer one after its length.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = (self.four.len() as u32).to_le_bytes().to_vec();
        for p in &self.four {
            out.extend_from_slice(p);
        }
        for p in &self.long {
            out.push(p.len() as u8);
            out.extend_from_slice(p);
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PrefixList> {
        let n = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let body = bytes.get(4..4 + n * 4)?;
        let four = body.chunks_exact(4).map(|c| c.try_into().unwrap()).collect();
        let mut long = Vec::new();
        let mut at = 4 + n * 4;
        while at < bytes.len() {
            let len = bytes[at] as usize;
            long.push(bytes.get(at + 1..at + 1 + len)?.to_vec());
            at += 1 + len;
        }
        Some(PrefixList { four, long })
    }
}

/// Google's raw additions: one long run of prefixes of the same size.
pub fn split_raw(size: usize, raw: &[u8]) -> Vec<Vec<u8>> {
    if !(4..=32).contains(&size) {
        return Vec::new();
    }
    raw.chunks_exact(size).map(<[u8]>::to_vec).collect()
}

/// "593.44s" as seconds.
pub fn seconds(duration: Option<&Value>) -> f64 {
    duration.and_then(Value::as_str).and_then(|d| d.trim_end_matches('s').parse::<f64>().ok()).unwrap_or(0.0)
}

/// What fullHashes:find answered: each listed full hash with its threat and how long it may be kept,
/// and how long the asked prefixes may be kept as clean.
pub fn read_matches(answer: &Value) -> (Vec<([u8; 32], Threat, f64)>, f64) {
    let found = answer["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let threat = threat_of(m["threatType"].as_str()?)?;
            let hash: [u8; 32] = B64.decode(m["threat"]["hash"].as_str()?).ok()?.try_into().ok()?;
            Some((hash, threat, seconds(m.get("cacheDuration"))))
        })
        .collect();
    (found, seconds(answer.get("negativeCacheDuration")))
}

// ---------- the lists on this computer, and the questions to Google ----------

struct List {
    threat: String,
    platform: String,
    state: String,
    prefixes: PrefixList,
}

#[derive(Default)]
struct Safe {
    lists: Vec<List>,
    /// unix seconds before which no update is asked
    next_update: i64,
    /// unix seconds before which no full-hash question is asked
    next_find: i64,
    /// full hash -> (threat, kept until)
    listed: HashMap<[u8; 32], (Threat, i64)>,
    /// prefix -> clean until
    clean: HashMap<Vec<u8>, i64>,
}

fn safe() -> &'static RwLock<Safe> {
    static SAFE: OnceLock<RwLock<Safe>> = OnceLock::new();
    SAFE.get_or_init(Default::default)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The address's full hashes that start with a listed prefix, each with that prefix: on this
/// computer, no network. Empty when the lists are empty (no key, or not fetched yet).
pub fn local_hits(url: &str) -> Vec<([u8; 32], Vec<u8>)> {
    let safe = safe().read().unwrap();
    if safe.lists.is_empty() {
        return Vec::new();
    }
    full_hashes(url).into_iter().filter_map(|h| safe.lists.iter().find_map(|l| l.prefixes.prefix_of(&h)).map(|p| (h, p))).collect()
}

/// The body of a fullHashes:find question: only the prefixes, never the address.
pub fn find_body(prefixes: &[Vec<u8>], states: &[String], platforms: &[String]) -> Value {
    json!({
        "client": { "clientId": "memorycore-castle", "clientVersion": env!("CARGO_PKG_VERSION") },
        "clientStates": states,
        "threatInfo": {
            "threatTypes": THREATS,
            "platformTypes": platforms,
            "threatEntryTypes": ["URL"],
            "threatEntries": prefixes.iter().map(|p| json!({ "hash": B64.encode(p) })).collect::<Vec<_>>(),
        }
    })
}

fn client() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .ok()
}

/// Whether an address with local hits is really on Google's lists. The caches answer first; then one
/// question to Google with the short prefixes only. None: not listed, no key, or Google not reached
/// (the page then loads; the other guards still stand).
pub fn confirm(url: &str, key: Option<&str>) -> Option<Threat> {
    let key = key.filter(|k| !k.trim().is_empty())?;
    let hits = local_hits(url);
    if hits.is_empty() {
        return None;
    }
    let t = now();
    let (ask, states, platforms) = {
        let safe = safe().read().unwrap();
        if let Some(threat) = hits.iter().find_map(|(h, _)| safe.listed.get(h).filter(|(_, until)| *until > t).map(|(threat, _)| *threat)) {
            return Some(threat);
        }
        let ask: Vec<Vec<u8>> = hits.iter().map(|(_, p)| p.clone()).filter(|p| safe.clean.get(p).is_none_or(|until| *until <= t)).collect();
        if ask.is_empty() || t < safe.next_find {
            return None;
        }
        let mut platforms: Vec<String> = safe.lists.iter().map(|l| l.platform.clone()).collect();
        platforms.dedup();
        (ask, safe.lists.iter().map(|l| l.state.clone()).collect::<Vec<_>>(), platforms)
    };
    let answer: Value = client()?.post(format!("{BASE}/fullHashes:find?key={key}")).json(&find_body(&ask, &states, &platforms)).send().ok()?.json().ok()?;
    let (found, clean_for) = read_matches(&answer);
    let mut safe = safe().write().unwrap();
    safe.next_find = t + seconds(answer.get("minimumWaitDuration")) as i64;
    for p in &ask {
        safe.clean.insert(p.clone(), t + clean_for as i64);
    }
    for (hash, threat, keep) in &found {
        safe.listed.insert(*hash, (*threat, t + *keep as i64));
    }
    hits.iter().find_map(|(h, _)| found.iter().find(|(f, _, _)| f == h).map(|(_, threat, _)| *threat))
}

/// Which of Google's lists to keep: for each threat, the all-platforms list, else this computer's.
pub fn pick_lists(available: &Value) -> Vec<(String, String)> {
    let os = if cfg!(target_os = "macos") { "OSX" } else { "WINDOWS" };
    let all: Vec<(&str, &str)> = available["threatLists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["threatEntryType"] == "URL")
        .filter_map(|l| Some((l["threatType"].as_str()?, l["platformType"].as_str()?)))
        .collect();
    THREATS
        .iter()
        .filter_map(|threat| {
            ["ANY_PLATFORM", os, "ALL_PLATFORMS"].iter().find(|p| all.contains(&(threat, p))).map(|p| (threat.to_string(), p.to_string()))
        })
        .collect()
}

/// Applies one list's update answer; false when its checksum did not match (the list then starts
/// again from nothing at the next update, as the spec asks).
fn apply_update(list: &mut List, answer: &Value) -> bool {
    if answer["responseType"] == "FULL_UPDATE" {
        list.prefixes = PrefixList::default();
    }
    let removals: Vec<usize> = answer["removals"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|r| r["rawIndices"]["indices"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|i| i as usize))
        .collect();
    let additions: Vec<Vec<u8>> = answer["additions"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|a| {
            let size = a["rawHashes"]["prefixSize"].as_u64().unwrap_or(0) as usize;
            let raw = a["rawHashes"]["rawHashes"].as_str().and_then(|r| B64.decode(r).ok()).unwrap_or_default();
            split_raw(size, &raw)
        })
        .collect();
    let want = answer["checksum"]["sha256"].as_str().and_then(|c| B64.decode(c).ok());
    let ok = list.prefixes.apply(&removals, additions).is_ok() && want.is_some_and(|w| w == list.prefixes.checksum());
    if ok {
        list.state = answer["newClientState"].as_str().unwrap_or("").to_string();
    } else {
        list.prefixes = PrefixList::default();
        list.state.clear();
    }
    ok
}

fn file_of(folder: &Path, threat: &str, platform: &str) -> PathBuf {
    folder.join(format!("{}-{}.bin", threat.to_ascii_lowercase(), platform.to_ascii_lowercase()))
}

/// The lists kept on disk, read back at start.
fn load(folder: &Path) {
    let Ok(text) = std::fs::read_to_string(folder.join("state.json")) else { return };
    let Ok(saved) = serde_json::from_str::<Value>(&text) else { return };
    let lists = saved["lists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| {
            let (threat, platform) = (l["threatType"].as_str()?.to_string(), l["platformType"].as_str()?.to_string());
            let prefixes = std::fs::read(file_of(folder, &threat, &platform)).ok().and_then(|b| PrefixList::from_bytes(&b)).unwrap_or_default();
            let state = if prefixes.len() > 0 { l["state"].as_str().unwrap_or("").to_string() } else { String::new() };
            Some(List { threat, platform, state, prefixes })
        })
        .collect();
    let mut safe = safe().write().unwrap();
    safe.lists = lists;
    safe.next_update = saved["nextUpdate"].as_i64().unwrap_or(0);
}

fn save(folder: &Path) {
    let safe = safe().read().unwrap();
    let _ = std::fs::create_dir_all(folder);
    for l in &safe.lists {
        let _ = std::fs::write(file_of(folder, &l.threat, &l.platform), l.prefixes.to_bytes());
    }
    let lists: Vec<Value> = safe.lists.iter().map(|l| json!({ "threatType": l.threat, "platformType": l.platform, "state": l.state })).collect();
    let _ = std::fs::write(folder.join("state.json"), json!({ "lists": lists, "nextUpdate": safe.next_update }).to_string());
}

/// One round with Google: which lists exist (first time), then every list's update. The seconds to
/// wait before the next round.
fn update_once(key: &str, folder: &Path) -> Result<f64, String> {
    let client = client().ok_or("no client")?;
    if safe().read().unwrap().lists.is_empty() {
        let available: Value = client.get(format!("{BASE}/threatLists?key={key}")).send().map_err(|_| "unreachable")?.json().map_err(|_| "bad answer")?;
        let picked = pick_lists(&available);
        if picked.is_empty() {
            return Err("no lists".into());
        }
        safe().write().unwrap().lists = picked.into_iter().map(|(threat, platform)| List { threat, platform, state: String::new(), prefixes: PrefixList::default() }).collect();
    }
    let requests: Vec<Value> = safe()
        .read()
        .unwrap()
        .lists
        .iter()
        .map(|l| {
            json!({
                "threatType": l.threat, "platformType": l.platform, "threatEntryType": "URL", "state": l.state,
                "constraints": { "supportedCompressions": ["RAW"] }
            })
        })
        .collect();
    let body = json!({
        "client": { "clientId": "memorycore-castle", "clientVersion": env!("CARGO_PKG_VERSION") },
        "listUpdateRequests": requests,
    });
    let response = client.post(format!("{BASE}/threatListUpdates:fetch?key={key}")).json(&body).send().map_err(|_| "unreachable")?;
    if !response.status().is_success() {
        return Err(format!("answered {}", response.status().as_u16()));
    }
    let answer: Value = response.json().map_err(|_| "bad answer")?;
    {
        let mut safe = safe().write().unwrap();
        for update in answer["listUpdateResponses"].as_array().into_iter().flatten() {
            if let Some(list) = safe.lists.iter_mut().find(|l| update["threatType"] == l.threat.as_str() && update["platformType"] == l.platform.as_str()) {
                apply_update(list, update);
            }
        }
    }
    let wait = seconds(answer.get("minimumWaitDuration")).max(30.0 * 60.0);
    safe().write().unwrap().next_update = now() + wait as i64;
    save(folder);
    Ok(wait)
}

/// Keeps the lists fresh while Castle runs, once started (the browser window's first open). Without
/// a key it only looks again now and then; Google is never asked more often than it says.
pub fn start(folder: PathBuf) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        load(&folder);
        let mut backoff: f64 = 15.0 * 60.0;
        loop {
            let Some(key) = crate::key::load(KEY).filter(|k| !k.trim().is_empty()) else {
                std::thread::sleep(std::time::Duration::from_secs(600));
                continue;
            };
            let due = safe().read().unwrap().next_update;
            if now() < due {
                std::thread::sleep(std::time::Duration::from_secs((due - now()).clamp(60, 3600) as u64));
                continue;
            }
            match update_once(&key, &folder) {
                Ok(_) => backoff = 15.0 * 60.0,
                Err(_) => {
                    safe().write().unwrap().next_update = now() + backoff as i64;
                    backoff = (backoff * 2.0).min(8.0 * 3600.0);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_of(url: &str) -> String {
        let (h, p, q) = canonical(url).unwrap();
        format!("http://{h}{p}{}", q.map(|q| format!("?{q}")).unwrap_or_default())
    }

    #[test]
    fn addresses_are_canonicalised_as_the_spec_shows() {
        for (given, want) in [
            ("http://host/%25%32%35", "http://host/%25"),
            ("http://host/%25%32%35%25%32%35", "http://host/%25%25"),
            ("http://host/%2525252525252525", "http://host/%25"),
            ("http://host/asdf%25%32%35asd", "http://host/asdf%25asd"),
            ("http://host/%%%25%32%35asd%%", "http://host/%25%25%25asd%25%25"),
            ("http://www.google.com/", "http://www.google.com/"),
            ("http://%31%36%38%2e%31%38%38%2e%39%39%2e%32%36/%2E%73%65%63%75%72%65/%77%77%77%2E%65%62%61%79%2E%63%6F%6D/", "http://168.188.99.26/.secure/www.ebay.com/"),
            ("http://195.127.0.11/uploads/%20%20%20%20/.verify/.eBaysecure=updateuserdataxplimnbqmn-xplmvalidateinfoswqpcmlx=hgplmcx/", "http://195.127.0.11/uploads/%20%20%20%20/.verify/.eBaysecure=updateuserdataxplimnbqmn-xplmvalidateinfoswqpcmlx=hgplmcx/"),
            ("http://host%23.com/%257Ea%2521b%2540c%2523d%2524e%25f%255E00%252611%252A22%252833%252944_55%252B", "http://host%23.com/~a!b@c%23d$e%25f^00&11*22(33)44_55+"),
            ("http://3279880203/blah", "http://195.127.0.11/blah"),
            ("http://www.google.com/blah/..", "http://www.google.com/"),
            ("www.google.com/", "http://www.google.com/"),
            ("www.google.com", "http://www.google.com/"),
            ("http://www.evil.com/blah#frag", "http://www.evil.com/blah"),
            ("http://www.GOOgle.com/", "http://www.google.com/"),
            ("http://www.google.com.../", "http://www.google.com/"),
            ("http://www.google.com/foo\tbar\rbaz\n2", "http://www.google.com/foobarbaz2"),
            ("http://www.google.com/q?", "http://www.google.com/q?"),
            ("http://www.google.com/q?r?", "http://www.google.com/q?r?"),
            ("http://www.google.com/q?r?s", "http://www.google.com/q?r?s"),
            ("http://evil.com/foo#bar#baz", "http://evil.com/foo"),
            ("http://evil.com/foo;", "http://evil.com/foo;"),
            ("http://evil.com/foo?bar;", "http://evil.com/foo?bar;"),
            ("http://notrailingslash.com", "http://notrailingslash.com/"),
            ("http://www.gotaport.com:1234/", "http://www.gotaport.com/"),
            ("  http://www.google.com/  ", "http://www.google.com/"),
            ("http:// leadingspace.com/", "http://%20leadingspace.com/"),
            ("http://%20leadingspace.com/", "http://%20leadingspace.com/"),
            ("%20leadingspace.com/", "http://%20leadingspace.com/"),
            ("https://www.securesite.com/", "http://www.securesite.com/"),
            ("http://host.com/ab%23cd", "http://host.com/ab%23cd"),
            ("http://host.com//twoslashes?more//slashes", "http://host.com/twoslashes?more//slashes"),
        ] {
            assert_eq!(url_of(given), want, "{given}");
        }
    }

    #[test]
    fn expressions_are_the_hosts_and_paths_the_spec_lists() {
        let mut got = expressions("http://a.b.c/1/2.html?param=1");
        got.sort();
        let mut want = vec!["a.b.c/1/2.html?param=1", "a.b.c/1/2.html", "a.b.c/", "a.b.c/1/", "b.c/1/2.html?param=1", "b.c/1/2.html", "b.c/", "b.c/1/"];
        want.sort();
        assert_eq!(got, want);
        let mut got = expressions("http://a.b.c.d.e.f.g/1.html");
        got.sort();
        let mut want = vec!["a.b.c.d.e.f.g/1.html", "a.b.c.d.e.f.g/", "c.d.e.f.g/1.html", "c.d.e.f.g/", "d.e.f.g/1.html", "d.e.f.g/", "e.f.g/1.html", "e.f.g/", "f.g/1.html", "f.g/"];
        want.sort();
        assert_eq!(got, want);
        let mut got = expressions("http://1.2.3.4/1/");
        got.sort();
        assert_eq!(got, vec!["1.2.3.4/", "1.2.3.4/1/"], "an IP has no parent hosts");
        assert!(expressions("http://a.b.c.d.e.f.g/1/2/3/4/5/6.html?q").len() <= 30);
    }

    #[test]
    fn a_full_hash_matches_the_prefix_it_starts_with() {
        let hash = full_hashes("http://evil.example/login")[0];
        let mut list = PrefixList::default();
        list.apply(&[], vec![hash[..4].to_vec(), vec![0, 0, 0, 1], vec![0xff; 8]]).unwrap();
        assert_eq!(list.prefix_of(&hash), Some(hash[..4].to_vec()));
        assert_eq!(list.prefix_of(&[7u8; 32]), None);
        let mut long = [0xffu8; 32];
        long[31] = 0;
        assert_eq!(list.prefix_of(&long), Some(vec![0xff; 8]), "a longer prefix matches too");
    }

    #[test]
    fn removals_count_in_googles_order_and_the_checksum_follows_it() {
        let mut list = PrefixList::default();
        list.apply(&[], vec![vec![3, 0, 0, 0], vec![1, 0, 0, 0], vec![2, 0, 0, 0, 9], vec![2, 0, 0, 0]]).unwrap();
        // order: 01000000, 02000000, 0200000009, 03000000
        list.apply(&[1, 2], vec![vec![4, 0, 0, 0]]).unwrap();
        assert_eq!(list.merged(), vec![&[1, 0, 0, 0][..], &[3, 0, 0, 0][..], &[4, 0, 0, 0][..]]);
        let mut h = Sha256::new();
        h.update([1, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0]);
        assert_eq!(list.checksum(), <[u8; 32]>::from(h.finalize()));
        assert!(list.apply(&[9], vec![]).is_err(), "a removal past the end");
        assert_eq!(PrefixList::from_bytes(&list.to_bytes()), Some(list.clone()), "the file reads back the same");
    }

    #[test]
    fn an_update_with_a_wrong_checksum_starts_the_list_again() {
        let mut list = List { threat: "MALWARE".into(), platform: "ANY_PLATFORM".into(), state: "old".into(), prefixes: PrefixList::default() };
        let raw = B64.encode([1, 0, 0, 0, 2, 0, 0, 0]);
        let mut good = PrefixList::default();
        good.apply(&[], vec![vec![1, 0, 0, 0], vec![2, 0, 0, 0]]).unwrap();
        let answer = json!({ "responseType": "FULL_UPDATE", "additions": [{ "rawHashes": { "prefixSize": 4, "rawHashes": raw } }], "newClientState": "new", "checksum": { "sha256": B64.encode(good.checksum()) } });
        assert!(apply_update(&mut list, &answer));
        assert_eq!((list.state.as_str(), list.prefixes.len()), ("new", 2));
        let bad = json!({ "responseType": "PARTIAL_UPDATE", "removals": [{ "rawIndices": { "indices": [0] } }], "newClientState": "newer", "checksum": { "sha256": B64.encode([0u8; 32]) } });
        assert!(!apply_update(&mut list, &bad));
        assert_eq!((list.state.as_str(), list.prefixes.len()), ("", 0), "a full update is asked next time");
    }

    #[test]
    fn a_full_hash_answer_is_read_into_threats() {
        let hash = [5u8; 32];
        let answer = json!({
            "matches": [
                { "threatType": "SOCIAL_ENGINEERING", "threat": { "hash": B64.encode(hash) }, "cacheDuration": "300s" },
                { "threatType": "SOMETHING_NEW", "threat": { "hash": B64.encode(hash) } },
                { "threatType": "MALWARE", "threat": { "hash": "not base64!" } }
            ],
            "negativeCacheDuration": "600.5s"
        });
        let (found, clean) = read_matches(&answer);
        assert_eq!(found, vec![(hash, Threat::Phishing, 300.0)]);
        assert_eq!(clean, 600.5);
    }

    #[test]
    fn the_question_to_google_carries_prefixes_never_the_address() {
        let body = find_body(&[vec![0xde, 0xad, 0xbe, 0xef]], &["s".into()], &["ANY_PLATFORM".into()]).to_string();
        assert!(body.contains(&B64.encode([0xde, 0xad, 0xbe, 0xef])));
        assert!(!body.contains("evil") && !body.contains("http"), "{body}");
    }

    #[test]
    fn no_key_means_no_question() {
        assert_eq!(confirm("http://testsafebrowsing.appspot.com/s/phishing.html", None), None);
        assert_eq!(confirm("http://testsafebrowsing.appspot.com/s/phishing.html", Some("  ")), None);
    }

    #[test]
    fn each_threat_keeps_one_list_all_platforms_first() {
        let os = if cfg!(target_os = "macos") { "OSX" } else { "WINDOWS" };
        let available = json!({ "threatLists": [
            { "threatType": "MALWARE", "platformType": os, "threatEntryType": "URL" },
            { "threatType": "MALWARE", "platformType": "ANY_PLATFORM", "threatEntryType": "URL" },
            { "threatType": "SOCIAL_ENGINEERING", "platformType": "ANY_PLATFORM", "threatEntryType": "URL" },
            { "threatType": "UNWANTED_SOFTWARE", "platformType": os, "threatEntryType": "URL" },
            { "threatType": "MALWARE", "platformType": "ANY_PLATFORM", "threatEntryType": "EXECUTABLE" },
            { "threatType": "SOCIAL_ENGINEERING", "platformType": "LINUX", "threatEntryType": "URL" }
        ]});
        assert_eq!(
            pick_lists(&available),
            vec![("SOCIAL_ENGINEERING".to_string(), "ANY_PLATFORM".to_string()), ("MALWARE".into(), "ANY_PLATFORM".into()), ("UNWANTED_SOFTWARE".into(), os.into())]
        );
    }
}
