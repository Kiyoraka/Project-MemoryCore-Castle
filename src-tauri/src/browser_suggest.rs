//! The address bar's suggestions, from the member's own past: what they searched for, the pages
//! they visited (Chrome's history too, once brought over) and their bookmarks. Search words are kept
//! only in `browser_searches`, only from the member's own typing in Castle's address bar (never while
//! Remember sign-ins is off) and from Chrome's past searches on the welcome's history step. Pages
//! still never keep anything after `?`. Nothing leaves the computer.

use crate::settings::SettingsStore;
use chrono::NaiveDateTime;
use mysql::{prelude::Queryable, Conn};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

/// At most this many rows under the address bar.
pub const MAX_ROWS: usize = 8;

/// One kept search.
#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub term: String,
    pub count: u32,
    /// "YYYY-MM-DD HH:MM:SS"
    pub last: String,
}

/// One visited page (browser_visits).
#[derive(Debug, Clone, PartialEq)]
pub struct PageRow {
    pub host: String,
    pub path: String,
    pub title: String,
    pub count: u32,
    pub last: String,
}

/// One row under the address bar.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// search, page or bookmark
    pub kind: &'static str,
    /// what the row says
    pub text: String,
    /// where it goes
    pub url: String,
    /// the small line under it (the site), empty for a search
    pub sub: String,
}

/// The key a search is kept under: the same words in any case and spacing are one search.
pub fn term_key(term: &str) -> String {
    let clean = term.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    Sha256::digest(clean.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// What the address bar should keep from what was typed: the search words when it became a search
/// and Remember sign-ins is on; nothing for an address, and nothing at all while browsing privately.
pub fn search_to_keep(input: &str, remember: bool) -> Option<String> {
    remember.then(|| crate::browser_tabs::search_term(input)).flatten()
}

fn age_days(last: &str, now: NaiveDateTime) -> f64 {
    NaiveDateTime::parse_from_str(last, "%Y-%m-%d %H:%M:%S").map(|t| (now - t).num_seconds().max(0) as f64 / 86_400.0).unwrap_or(365.0)
}

/// How much a row weighs: its count, halved every thirty days since it was last used.
fn weight(count: u32, last: &str, now: NaiveDateTime) -> f64 {
    count.max(1) as f64 * 0.5f64.powf(age_days(last, now) / 30.0)
}

/// 0 when the text starts with what was typed, 1 when one of its words does, None otherwise.
fn tier(text: &str, typed: &str) -> Option<u8> {
    let text = text.to_lowercase();
    if text.starts_with(typed) {
        Some(0)
    } else if text.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(typed)) {
        Some(1)
    } else {
        None
    }
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("").to_string()
}

/// The suggestions for what is typed (pure), at most MAX_ROWS: matches at the start before matches
/// at a word, then by weight; one row per address. An empty field shows the newest searches.
pub fn rank(typed: &str, searches: &[Search], pages: &[PageRow], bookmarks: &[crate::vault::Bookmark], now: NaiveDateTime) -> Vec<Suggestion> {
    let typed = typed.trim().to_lowercase();
    let search_row = |s: &Search| crate::browser_tabs::address(&s.term).map(|url| Suggestion { kind: "search", text: s.term.clone(), url, sub: String::new() });
    if typed.is_empty() {
        let mut newest: Vec<&Search> = searches.iter().collect();
        newest.sort_by(|a, b| b.last.cmp(&a.last));
        return newest.into_iter().filter_map(search_row).take(MAX_ROWS).collect();
    }
    let mut scored: Vec<(u8, f64, Suggestion)> = Vec::new();
    for s in searches {
        if let (Some(t), Some(row)) = (tier(&s.term, &typed), search_row(s)) {
            scored.push((t, weight(s.count, &s.last, now), row));
        }
    }
    for p in pages {
        let bare = p.host.trim_start_matches("www.");
        let t = [tier(bare, &typed), tier(&p.host, &typed), tier(&p.title, &typed)].into_iter().flatten().min();
        if let Some(t) = t {
            let url = format!("https://{}{}", p.host, p.path);
            let text = if p.title.trim().is_empty() { p.host.clone() } else { p.title.clone() };
            scored.push((t, weight(p.count, &p.last, now), Suggestion { kind: "page", text, url, sub: format!("{}{}", p.host, if p.path == "/" { "" } else { &p.path }) }));
        }
    }
    for b in bookmarks {
        let host = host_of(&b.url);
        let t = [tier(&b.title, &typed), tier(host.trim_start_matches("www."), &typed)].into_iter().flatten().min();
        if let Some(t) = t {
            let text = if b.title.trim().is_empty() { b.url.clone() } else { b.title.clone() };
            // a bookmark is something the member chose to keep: it weighs like a few visits
            scored.push((t, 3.0, Suggestion { kind: "bookmark", text, url: b.url.clone(), sub: host }));
        }
    }
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)));
    let mut out: Vec<Suggestion> = Vec::new();
    for (_, _, row) in scored {
        let same = |u: &str| u.trim_end_matches('/').eq_ignore_ascii_case(row.url.trim_end_matches('/'));
        if !out.iter().any(|o| same(&o.url)) {
            out.push(row);
        }
        if out.len() >= MAX_ROWS {
            break;
        }
    }
    out
}

/// Keeps one search the member made (a second time adds to its count).
pub fn remember_search(c: &mut Conn, term: &str) -> Result<(), String> {
    let term: String = term.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    if term.is_empty() {
        return Ok(());
    }
    c.exec_drop(
        "INSERT INTO browser_searches (term, term_key) VALUES (?, ?) ON DUPLICATE KEY UPDATE count = count + 1, last_at = NOW(), term = VALUES(term)",
        (&term, term_key(&term)),
    )
    .map_err(|_| "The search could not be kept.".to_string())
}

/// Searches brought from Chrome: (term, count, last); a search already kept keeps the larger count,
/// the earlier first time and the later last time.
pub fn keep_searches(c: &mut Conn, rows: &[(String, u32, String)]) -> Result<u32, String> {
    for (term, count, last) in rows {
        c.exec_drop(
            "INSERT INTO browser_searches (term, term_key, count, first_at, last_at) VALUES (?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE count = GREATEST(count, VALUES(count)), first_at = LEAST(first_at, VALUES(first_at)), last_at = GREATEST(last_at, VALUES(last_at))",
            (term, term_key(term), count, last, last),
        )
        .map_err(|_| "The searches could not be kept.".to_string())?;
    }
    Ok(rows.len() as u32)
}

/// The month's searches (`YYYY-MM`, by when last made), most used first, at most 30: for MY
/// PROFILE's reading only.
pub fn searches_of_month(c: &mut Conn, month: &str) -> Vec<(String, u32)> {
    c.exec("SELECT term, count FROM browser_searches WHERE DATE_FORMAT(last_at, '%Y-%m') = ? ORDER BY count DESC, last_at DESC LIMIT 30", (month,)).unwrap_or_default()
}

/// What a LIKE pattern needs escaped from what was typed.
fn like_escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn conn(app: &AppHandle) -> Option<Conn> {
    let memory = app.state::<SettingsStore>().get().memory_folder?;
    crate::store::source(std::path::Path::new(&memory))
}

/// The rows a suggestion is ranked from, with bounded queries. A missing table (an older database)
/// answers nothing, quietly.
fn read_rows(c: &mut Conn, typed: &str) -> (Vec<Search>, Vec<PageRow>, Vec<crate::vault::Bookmark>) {
    let fmt = "DATE_FORMAT(last_at, '%Y-%m-%d %H:%i:%s')";
    let typed = typed.trim();
    if typed.is_empty() {
        let searches: Vec<(String, u32, String)> = c.query(format!("SELECT term, count, {fmt} FROM browser_searches ORDER BY last_at DESC LIMIT {MAX_ROWS}")).unwrap_or_default();
        return (searches.into_iter().map(|(term, count, last)| Search { term, count, last }).collect(), Vec::new(), Vec::new());
    }
    let like = like_escaped(typed);
    let (start, word, www) = (format!("{like}%"), format!("% {like}%"), format!("www.{like}%"));
    let searches: Vec<(String, u32, String)> = c
        .exec(format!("SELECT term, count, {fmt} FROM browser_searches WHERE term LIKE ? OR term LIKE ? ORDER BY count DESC LIMIT 50"), (&start, &word))
        .unwrap_or_default();
    let pages: Vec<(String, String, String, u32, String)> = c
        .exec(
            format!("SELECT host, path, title, count, {fmt} FROM browser_visits WHERE host LIKE ? OR host LIKE ? OR title LIKE ? OR title LIKE ? ORDER BY count DESC LIMIT 50"),
            (&start, &www, &start, &word),
        )
        .unwrap_or_default();
    let marks: Vec<(u64, String, String, String)> = c
        .exec("SELECT id, title, url, folder FROM browser_bookmarks WHERE title LIKE ? OR title LIKE ? OR url LIKE ? OR url LIKE ? ORDER BY id DESC LIMIT 50", (&start, &word, format!("%://{like}%"), format!("%://www.{like}%")))
        .unwrap_or_default();
    (
        searches.into_iter().map(|(term, count, last)| Search { term, count, last }).collect(),
        pages.into_iter().map(|(host, path, title, count, last)| PageRow { host, path, title, count, last }).collect(),
        marks.into_iter().map(|(id, title, url, folder)| crate::vault::Bookmark { id, title, url, folder }).collect(),
    )
}

/// The suggestions for what is in the address bar now.
#[tauri::command]
pub async fn browser_suggest(app: AppHandle, input: String) -> Result<Vec<Suggestion>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(mut c) = conn(&app) else { return Vec::new() };
        let (searches, pages, marks) = read_rows(&mut c, &input);
        rank(&input, &searches, &pages, &marks, chrono::Local::now().naive_local())
    })
    .await
    .map_err(|_| "The suggestions could not be read.".to_string())
}

/// The (x) on a past search: it is forgotten.
#[tauri::command]
pub async fn browser_forget_search(app: AppHandle, term: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = conn(&app).ok_or("Your memory database is not answering.")?;
        c.exec_drop("DELETE FROM browser_searches WHERE term_key = ?", (term_key(&term),)).map_err(|_| "The search could not be forgotten.".to_string())
    })
    .await
    .map_err(|_| "The search could not be forgotten.".to_string())?
}

/// Keeps a search typed in the address bar, off the window's thread; nothing while browsing privately.
pub fn remember_typed(app: &AppHandle, input: &str, private: bool) {
    let Some(term) = search_to_keep(input, crate::browser_tabs::may_keep(app.state::<SettingsStore>().get().browser_remember, private)) else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(mut c) = conn(&app) {
            let _ = remember_search(&mut c, &term);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-11-25 16:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn search(term: &str, count: u32, last: &str) -> Search {
        Search { term: term.into(), count, last: last.into() }
    }

    fn page(host: &str, path: &str, title: &str, count: u32) -> PageRow {
        PageRow { host: host.into(), path: path.into(), title: title.into(), count, last: "2026-11-24 10:00:00".into() }
    }

    #[test]
    fn a_search_at_the_start_comes_before_one_at_a_word() {
        let searches = [search("my google meet link", 50, "2026-11-25 09:00:00"), search("google meet", 2, "2026-11-25 09:00:00")];
        let rows = rank("goo", &searches, &[], &[], now());
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["google meet", "my google meet link"]);
        assert_eq!(rows[0].kind, "search");
        assert_eq!(rows[0].url, "https://duckduckgo.com/?q=google+meet");
    }

    #[test]
    fn pages_match_by_site_without_www_and_by_title() {
        let pages = [page("www.linkedin.com", "/feed/", "Feed | LinkedIn", 40), page("example.org", "/", "", 1)];
        let rows = rank("link", &[], &pages, &[], now());
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].kind, rows[0].url.as_str(), rows[0].sub.as_str()), ("page", "https://www.linkedin.com/feed/", "www.linkedin.com/feed/"));
        assert_eq!(rank("exa", &[], &pages, &[], now())[0].text, "example.org", "a page with no title shows its site");
    }

    #[test]
    fn recent_use_weighs_more_and_one_address_shows_once() {
        let searches = [search("rust old", 10, "2026-05-01 10:00:00"), search("rust new", 3, "2026-11-25 10:00:00")];
        assert_eq!(rank("rust", &searches, &[], &[], now())[0].text, "rust new", "three this week beat ten in May");
        let pages = [page("github.com", "/", "GitHub", 9)];
        let marks = [crate::vault::Bookmark { id: 1, title: "GitHub".into(), url: "https://github.com/".into(), folder: String::new() }];
        let rows = rank("git", &[], &pages, &marks, now());
        assert_eq!(rows.len(), 1, "the page and the bookmark go to one address");
    }

    #[test]
    fn an_empty_field_shows_the_newest_searches_and_never_more_than_eight() {
        let searches: Vec<Search> = (0..12).map(|i| search(&format!("term {i}"), 1, &format!("2026-11-{:02} 10:00:00", 10 + i))).collect();
        let rows = rank("  ", &searches, &[page("a.example", "/", "A", 99)], &[], now());
        assert_eq!(rows.len(), MAX_ROWS);
        assert_eq!(rows[0].text, "term 11");
        assert!(rows.iter().all(|r| r.kind == "search"));
    }

    #[test]
    fn only_a_search_is_kept_and_never_while_browsing_privately() {
        assert_eq!(search_to_keep("  google   meet ", true).as_deref(), Some("google meet"));
        assert_eq!(search_to_keep("google meet", false), None, "Remember sign-ins off keeps nothing");
        assert_eq!(search_to_keep("github.com", true), None, "an address is not a search");
        assert_eq!(search_to_keep("https://x.example/?q=secret", true), None);
        assert_eq!(term_key("Google  Meet"), term_key("google meet"));
    }

    #[test]
    #[ignore]
    fn live_database_searches_are_kept_ranked_and_forgotten() {
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Search Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();

        remember_search(&mut c, "google meet").unwrap();
        remember_search(&mut c, "Google   Meet").unwrap();
        keep_searches(&mut c, &[("google meet".into(), 9, "2026-01-01 10:00:00".into()), ("50% off".into(), 1, "2026-01-02 10:00:00".into())]).unwrap();
        let (count, first): (u32, String) = c.query_first("SELECT count, DATE_FORMAT(first_at, '%Y-%m-%d') FROM browser_searches WHERE term_key = SHA2('google meet', 256)").unwrap().unwrap();
        assert_eq!((count, first.as_str()), (9, "2026-01-01"), "one row whatever the case; the larger count and the earlier first time");
        let (searches, _, _) = read_rows(&mut c, "goo");
        assert_eq!(searches.len(), 1);
        let (searches, _, _) = read_rows(&mut c, "%");
        assert!(searches.is_empty(), "a typed % is a letter, not a wildcard");
        assert_eq!(read_rows(&mut c, "").0.len(), 2, "an empty field reads the newest");
        c.exec_drop("DELETE FROM browser_searches WHERE term_key = ?", (term_key("GOOGLE MEET"),)).unwrap();
        assert_eq!(read_rows(&mut c, "").0.len(), 1);
        let this_month: String = c.query_first("SELECT DATE_FORMAT(NOW(), '%Y-%m')").unwrap().unwrap();
        remember_search(&mut c, "tauri windows").unwrap();
        let month_rows = searches_of_month(&mut c, &this_month);
        assert_eq!(month_rows, vec![("tauri windows".to_string(), 1)], "only the searches made this month");
        assert!(searches_of_month(&mut c, "2001-01").is_empty());
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }

    #[test]
    fn typed_wildcards_are_only_letters_to_like() {
        assert_eq!(like_escaped("50%_off\\"), "50\\%\\_off\\\\");
    }
}
