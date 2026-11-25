//! Bringing a member's Chrome along (his word Nov 25 2026: "step by step like opening browser first
//! time and it will import everything"). Each part is the member's own yes:
//!
//! - **History**: Chrome's `History` is copied first (Chrome is never touched or locked) and read:
//!   web pages only - the site, the page's path and its title, how often and when - never the part of
//!   an address after `?` or `#`. Into the member's own `browser_visits`. Chrome's own list of past
//!   search words (`keyword_search_terms`) comes too, into `browser_searches` only, for the address
//!   bar's suggestions; a page's address still never keeps its query.
//! - **Bookmarks**: a copy of Chrome's `Bookmarks` file, web addresses only, with their folders.
//! - **Passwords**: never Chrome's own password file. Only the file the member exports with Chrome's
//!   own Export passwords (their Windows PIN). Castle's code reads it, seals each password into the
//!   vault (vault.rs), and shreds the file at once. Only counts come back: no password ever reaches
//!   the page, a log, or the companion.
//!
//! Nothing leaves the member's computer.

use crate::vault;
use mysql::{prelude::Queryable, Conn};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// At most this many pages of history are brought over (the newest).
pub const HISTORY_MAX: usize = 5000;

/// Where Chrome keeps its profiles on this PC.
pub fn chrome_root() -> Option<PathBuf> {
    // platform: Chrome's own folder; on a Mac it lives under Application Support
    #[cfg(windows)]
    let root = std::env::var("LOCALAPPDATA").ok().map(|d| PathBuf::from(d).join("Google").join("Chrome").join("User Data"));
    #[cfg(not(windows))]
    let root = std::env::var("HOME").ok().map(|d| PathBuf::from(d).join("Library/Application Support/Google/Chrome"));
    root.filter(|r| r.is_dir())
}

/// One Chrome profile: its folder name and the name the member gave it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Profile {
    pub folder: String,
    pub name: String,
}

/// The profile names from Chrome's `Local State` (folder -> name). Pure.
pub fn profile_names(local_state: &str) -> Vec<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(local_state).unwrap_or_default();
    let mut out: Vec<(String, String)> = value["profile"]["info_cache"]
        .as_object()
        .map(|cache| cache.iter().map(|(folder, info)| (folder.clone(), info["name"].as_str().unwrap_or(folder).to_string())).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// The profiles that have a history to bring.
pub fn profiles(root: &Path) -> Vec<Profile> {
    let names = profile_names(&std::fs::read_to_string(root.join("Local State")).unwrap_or_default());
    let mut found: Vec<Profile> = names.into_iter().filter(|(folder, _)| root.join(folder).join("History").is_file()).map(|(folder, name)| Profile { folder, name }).collect();
    if found.is_empty() && root.join("Default").join("History").is_file() {
        found.push(Profile { folder: "Default".into(), name: "Default".into() });
    }
    found
}

/// The profile folder, refused unless it is a plain name inside Chrome's own folder.
fn profile_dir(root: &Path, folder: &str) -> Result<PathBuf, String> {
    if folder.is_empty() || folder.contains(['/', '\\', ':']) || folder.contains("..") {
        return Err("That is not a Chrome profile.".into());
    }
    let dir = root.join(folder);
    dir.is_dir().then_some(dir).ok_or_else(|| "That Chrome profile is not on this PC.".to_string())
}

/// A copy of one of Chrome's files in the temp folder, removed when dropped.
struct Copy(PathBuf);
impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn copy_of(file: &Path) -> Result<Copy, String> {
    let to = std::env::temp_dir().join(format!("castle-chrome-{}-{}", std::process::id(), file.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    std::fs::copy(file, &to).map_err(|_| "Chrome's file could not be copied. Close Chrome and try again if it keeps happening.".to_string())?;
    Ok(Copy(to))
}

/// Chrome's time (microseconds since 1601) as `YYYY-MM-DD HH:MM:SS`. Pure.
pub fn chrome_time(micros: i64) -> Option<String> {
    let unix = micros / 1_000_000 - 11_644_473_600;
    chrono::DateTime::from_timestamp(unix, 0).map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// One page of history as it is kept: host, path, title, visits, last visit.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub host: String,
    pub path: String,
    pub title: String,
    pub visits: u32,
    pub last: String,
}

/// Reads a History file (already a copy): web pages only, newest first, at most HISTORY_MAX.
pub fn read_history(file: &Path) -> Result<Vec<Page>, String> {
    let db = rusqlite::Connection::open_with_flags(file, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| "Chrome's history could not be opened.".to_string())?;
    let mut statement = db
        .prepare("SELECT url, title, visit_count, last_visit_time FROM urls WHERE url LIKE 'http%' ORDER BY last_visit_time DESC LIMIT 20000")
        .map_err(|_| "Chrome's history could not be read.".to_string())?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))
        .map_err(|_| "Chrome's history could not be read.".to_string())?;
    let mut pages: Vec<Page> = Vec::new();
    for (url, title, count, last) in rows.flatten() {
        let (Some((host, path, title)), Some(last)) = (crate::sites::visit_of(&url, &title), chrome_time(last)) else { continue };
        let visits = count.clamp(1, u32::MAX as i64) as u32;
        // two addresses that differ only after ? or # are one page
        match pages.iter_mut().find(|p| p.host == host && p.path == path) {
            Some(page) => page.visits = page.visits.saturating_add(visits),
            None => pages.push(Page { host, path, title, visits, last }),
        }
        if pages.len() >= HISTORY_MAX {
            break;
        }
    }
    Ok(pages)
}

/// Writes the pages into the member's own browser_visits; a page already there adds its visits.
pub fn keep_history(c: &mut Conn, pages: &[Page]) -> Result<u32, String> {
    for p in pages {
        c.exec_drop(
            "INSERT INTO browser_visits (host, path, title, first_at, last_at, count) VALUES (?, ?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE count = count + VALUES(count), last_at = GREATEST(last_at, VALUES(last_at)), first_at = LEAST(first_at, VALUES(first_at))",
            (&p.host, &p.path, &p.title, &p.last, &p.last, p.visits),
        )
        .map_err(|_| "The history could not be kept.".to_string())?;
    }
    Ok(pages.len() as u32)
}

/// At most this many past searches are brought over (the newest).
pub const SEARCHES_MAX: usize = 2000;

/// Chrome's past search words from a History copy: (term, how often, last time), one row per search
/// whatever its case or spacing, newest first.
pub fn read_search_terms(file: &Path) -> Result<Vec<(String, u32, String)>, String> {
    let db = rusqlite::Connection::open_with_flags(file, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| "Chrome's history could not be opened.".to_string())?;
    let mut statement = db
        .prepare("SELECT k.term, u.visit_count, u.last_visit_time FROM keyword_search_terms k JOIN urls u ON u.id = k.url_id ORDER BY u.last_visit_time DESC LIMIT 20000")
        .map_err(|_| "Chrome's searches could not be read.".to_string())?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))
        .map_err(|_| "Chrome's searches could not be read.".to_string())?;
    let mut out: Vec<(String, u32, String)> = Vec::new();
    for (term, count, last) in rows.flatten() {
        let term: String = term.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
        let Some(last) = chrome_time(last) else { continue };
        if term.is_empty() {
            continue;
        }
        let visits = count.clamp(1, u32::MAX as i64) as u32;
        match out.iter_mut().find(|(t, _, _)| t.to_lowercase() == term.to_lowercase()) {
            Some(row) => row.1 = row.1.saturating_add(visits),
            None => out.push((term, visits, last)),
        }
        if out.len() >= SEARCHES_MAX {
            break;
        }
    }
    Ok(out)
}

/// Brings one profile's history over, its past searches with it. The number of pages.
pub fn import_history(c: &mut Conn, profile: &str) -> Result<u32, String> {
    let root = chrome_root().ok_or("Chrome is not on this PC.")?;
    let copy = copy_of(&profile_dir(&root, profile)?.join("History"))?;
    let pages = read_history(&copy.0)?;
    let kept = keep_history(c, &pages)?;
    // an older History without the searches table brings the pages alone
    if let Ok(searches) = read_search_terms(&copy.0) {
        let _ = crate::browser_suggest::keep_searches(c, &searches);
    }
    Ok(kept)
}

/// The bookmarks in Chrome's Bookmarks JSON: (title, url, folder), web addresses only. Pure.
pub fn read_bookmarks(json: &str) -> Vec<(String, String, String)> {
    fn walk(node: &serde_json::Value, folder: &str, out: &mut Vec<(String, String, String)>) {
        let name = node["name"].as_str().unwrap_or("");
        match node["type"].as_str() {
            Some("url") => {
                let url = node["url"].as_str().unwrap_or("");
                if url.starts_with("http://") || url.starts_with("https://") {
                    out.push((name.to_string(), url.to_string(), folder.to_string()));
                }
            }
            Some("folder") => {
                let here = if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
                for child in node["children"].as_array().into_iter().flatten() {
                    walk(child, &here, out);
                }
            }
            _ => {}
        }
    }
    let value: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let mut out = Vec::new();
    if let Some(roots) = value["roots"].as_object() {
        for root in roots.values() {
            walk(root, "", &mut out);
        }
    }
    out
}

/// Brings one profile's bookmarks over. The number kept that were new.
pub fn import_bookmarks(c: &mut Conn, profile: &str) -> Result<u32, String> {
    let root = chrome_root().ok_or("Chrome is not on this PC.")?;
    let copy = copy_of(&profile_dir(&root, profile)?.join("Bookmarks"))?;
    let text = std::fs::read_to_string(&copy.0).map_err(|_| "Chrome's bookmarks could not be read.".to_string())?;
    let mut added = 0;
    for (title, url, folder) in read_bookmarks(&text) {
        if vault::add_bookmark(c, &title, &url, &folder).unwrap_or(false) {
            added += 1;
        }
    }
    Ok(added)
}

/// One CSV line split into fields, with quotes and doubled quotes. Pure.
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().unwrap().push('"');
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(String::new()),
            (c, _) => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

/// The records of a CSV text, a quoted field may hold a line break. Pure.
fn csv_records(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(line);
        if pending.matches('"').count() % 2 == 0 {
            records.push(csv_fields(&pending));
            pending.clear();
        }
    }
    records
}

/// Chrome's password export read: (host, username, password) for each web login, or None when the
/// file is not Chrome's export (its header names url, username and password). Pure.
pub fn read_passwords(text: &str) -> Option<Vec<(String, String, String)>> {
    let records = csv_records(text.trim_start_matches('\u{feff}'));
    let header: Vec<String> = records.first()?.iter().map(|h| h.trim().to_ascii_lowercase()).collect();
    let at = |name: &str| header.iter().position(|h| h == name);
    let (url, user, pass) = (at("url")?, at("username")?, at("password")?);
    Some(
        records[1..]
            .iter()
            .filter_map(|r| {
                let (url, user, pass) = (r.get(url)?, r.get(user)?, r.get(pass)?);
                let (host, _, _) = crate::sites::visit_of(url, "")?;
                (!pass.is_empty()).then(|| (host, user.clone(), pass.clone()))
            })
            .collect(),
    )
}

/// Overwrites a file with zeros and removes it.
pub fn shred(file: &Path) -> Result<(), String> {
    use std::io::Write;
    let len = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(file) {
        let zeros = vec![0u8; 64 * 1024];
        let mut left = len;
        while left > 0 {
            let n = left.min(zeros.len() as u64) as usize;
            if f.write_all(&zeros[..n]).is_err() {
                break;
            }
            left -= n as u64;
        }
        let _ = f.sync_all();
    }
    std::fs::remove_file(file).map_err(|_| "The export file could not be removed. Delete it yourself now.".to_string())
}

/// What a password import did: counts only.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordsDone {
    pub sealed: u32,
    pub skipped: u32,
    pub shredded: bool,
}

/// Seals the passwords of Chrome's export into the vault, then shreds the file. A file that is not
/// Chrome's export is left alone and refused.
pub fn import_passwords(c: &mut Conn, key: &[u8; 32], file: &Path) -> Result<PasswordsDone, String> {
    let text = std::fs::read_to_string(file).map_err(|_| "That file could not be read.".to_string())?;
    let logins = read_passwords(&text).ok_or("That is not Chrome's password export. Nothing was read and the file was left as it is.")?;
    drop(text);
    let mut done = PasswordsDone::default();
    for (host, user, pass) in &logins {
        match vault::save(c, key, host, user, pass) {
            Ok(()) => done.sealed += 1,
            Err(_) => done.skipped += 1,
        }
    }
    done.shredded = shred(file).is_ok();
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_names_come_from_local_state() {
        let state = r#"{"profile":{"info_cache":{"Default":{"name":"Person 1"},"Profile 2":{"name":"Work"}}}}"#;
        assert_eq!(profile_names(state), vec![("Default".to_string(), "Person 1".to_string()), ("Profile 2".to_string(), "Work".to_string())]);
        assert!(profile_names("not json").is_empty());
    }

    #[test]
    fn a_profile_folder_never_climbs_out() {
        let root = std::env::temp_dir();
        assert!(profile_dir(&root, "../x").is_err());
        assert!(profile_dir(&root, "C:\\Windows").is_err());
        assert!(profile_dir(&root, "").is_err());
    }

    #[test]
    fn chrome_time_counts_from_1601() {
        assert_eq!(chrome_time(13_380_000_000_000_000).as_deref(), Some("2024-12-30 02:40:00"));
    }

    #[test]
    fn chromes_past_searches_come_as_one_row_per_search() {
        let dir = std::env::temp_dir().join(format!("castle-chrome-searches-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("History");
        let _ = std::fs::remove_file(&file);
        let db = rusqlite::Connection::open(&file).unwrap();
        db.execute_batch(
            "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER);
             CREATE TABLE keyword_search_terms (keyword_id INTEGER, url_id INTEGER, term TEXT, normalized_term TEXT);
             INSERT INTO urls (id, url, title, visit_count, last_visit_time) VALUES
               (1, 'https://www.google.com/search?q=google+meet', 'google meet', 3, 13380000000000000),
               (2, 'https://www.google.com/search?q=Google+Meet', 'Google Meet', 2, 13370000000000000),
               (3, 'https://www.google.com/search?q=tauri', 'tauri', 1, 13360000000000000);
             INSERT INTO keyword_search_terms (keyword_id, url_id, term, normalized_term) VALUES
               (1, 1, 'google  meet', 'google meet'), (1, 2, 'Google Meet', 'google meet'), (1, 3, 'tauri', 'tauri'), (1, 9, 'orphan', 'orphan');",
        )
        .unwrap();
        drop(db);
        let rows = read_search_terms(&file).unwrap();
        assert_eq!(rows, vec![("google meet".to_string(), 5, "2024-12-30 02:40:00".to_string()), ("tauri".to_string(), 1, chrome_time(13360000000000000).unwrap())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_keeps_web_pages_without_their_queries() {
        let dir = std::env::temp_dir().join(format!("castle-chrome-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("History");
        let _ = std::fs::remove_file(&file);
        let db = rusqlite::Connection::open(&file).unwrap();
        db.execute_batch(
            "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER);
             INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES
               ('https://example.org/a?token=SECRET', 'A', 3, 13380000000000000),
               ('https://example.org/a#part', 'A again', 2, 13370000000000000),
               ('chrome://settings', 'Settings', 9, 13390000000000000),
               ('http://news.example.com/', NULL, 1, 13360000000000000);",
        )
        .unwrap();
        drop(db);
        let pages = read_history(&file).unwrap();
        assert_eq!(pages.len(), 2, "the chrome:// page is not a web page; the two addresses of /a are one page");
        assert_eq!((pages[0].host.as_str(), pages[0].path.as_str(), pages[0].visits), ("example.org", "/a", 5));
        assert!(!format!("{pages:?}").contains("SECRET"));
        assert_eq!(pages[1].title, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bookmarks_keep_their_folders_and_only_web_addresses() {
        let json = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
            {"type":"url","name":"Docs","url":"https://example.org/docs"},
            {"type":"folder","name":"Work","children":[{"type":"url","name":"Board","url":"http://board.example.com/"}]},
            {"type":"url","name":"Script","url":"javascript:alert(1)"}]},
            "other":{"type":"folder","name":"Other bookmarks","children":[]}}}"#;
        let found = read_bookmarks(json);
        assert_eq!(found, vec![
            ("Docs".into(), "https://example.org/docs".into(), "Bookmarks bar".into()),
            ("Board".into(), "http://board.example.com/".into(), "Bookmarks bar/Work".into()),
        ]);
    }

    #[test]
    fn chromes_export_is_read_and_anything_else_refused() {
        let csv = "name,url,username,password,note\nexample.org,https://login.example.org/signin?x=1,ari,\"pa,ss\"\"word\",\n,android://app,bob,x,\nsite,https://other.example.com/,,,\n";
        let logins = read_passwords(csv).unwrap();
        assert_eq!(logins, vec![("login.example.org".to_string(), "ari".to_string(), "pa,ss\"word".to_string())], "an app login and an empty password are skipped");
        assert!(read_passwords("title,body\nhello,world\n").is_none(), "not Chrome's export");
        let multi = "name,url,username,password,note\ns,https://a.example.org/,u,p,\"line one\nline two\"\n";
        assert_eq!(read_passwords(multi).unwrap().len(), 1);
    }

    #[test]
    #[ignore]
    fn live_database_history_adds_its_visits_to_the_members_own() {
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Chrome Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();
        crate::sites::keep_visit(&mut c, "example.org", "/a", "A", false).unwrap();
        let page = Page { host: "example.org".into(), path: "/a".into(), title: "A".into(), visits: 5, last: "2024-12-30 02:40:00".into() };
        assert_eq!(keep_history(&mut c, &[page]).unwrap(), 1);
        let (count, first): (u32, String) = c.query_first("SELECT count, DATE_FORMAT(first_at, '%Y-%m-%d') FROM browser_visits WHERE host = 'example.org'").unwrap().unwrap();
        assert_eq!((count, first.as_str()), (6, "2024-12-30"), "Chrome's visits added, its older first visit kept");
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }

    #[test]
    fn shredding_leaves_no_file() {
        let file = std::env::temp_dir().join(format!("castle-shred-{}.csv", std::process::id()));
        std::fs::write(&file, "name,url,username,password\nx,https://a.example.org,u,secret\n").unwrap();
        shred(&file).unwrap();
        assert!(!file.exists());
    }
}
