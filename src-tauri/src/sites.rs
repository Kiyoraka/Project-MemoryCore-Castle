//! The browser's record in the member's own database (030_browser_sites.sql): the visits (never
//! for private browsing, never the part of an address after `?` or `#`), the Sentinel's marks on
//! hosts, and the patterns she learned. A mark keeps the site and her verdict, never when or whether
//! the member visited it; a wrong pattern is switched off, never deleted, so its why stays.

use crate::site_rules::{clean_host, Pattern, Verdict, Warning};
use mysql::{prelude::Queryable, Conn};
use serde::Serialize;

/// A web page's visit as it is kept: (host, path, title). None for anything that is not a web page.
pub fn visit_of(url: &str, title: &str) -> Option<(String, String, String)> {
    let parsed = tauri::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = clean_host(parsed.host_str()?)?;
    let path: String = parsed.path().chars().take(512).collect();
    let title: String = title.trim().chars().take(120).collect();
    Some((host, if path.is_empty() { "/".into() } else { path }, title))
}

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("The browser's record: {what} did not work.")
}

/// Keeps one visit: a new page is added, a known one counted again; a password box is never forgotten.
pub fn keep_visit(c: &mut Conn, host: &str, path: &str, title: &str, has_login: bool) -> Result<(), String> {
    c.exec_drop(
        "INSERT INTO browser_visits (host, path, title, has_login, mark_id) VALUES (?, ?, ?, ?, (SELECT id FROM sentinel_site_marks WHERE host = ?)) \
         ON DUPLICATE KEY UPDATE title = VALUES(title), last_at = NOW(), count = count + 1, has_login = GREATEST(has_login, VALUES(has_login))",
        (host, path, title, has_login, host),
    )
    .map_err(sql("keeping a visit"))
}

/// Hosts visited that the Sentinel has not marked yet, newest first, with whether any page asked for
/// a password.
pub fn unmarked(c: &mut Conn, limit: u32) -> Result<Vec<(String, bool)>, String> {
    c.exec(
        "SELECT host, MAX(has_login) FROM browser_visits WHERE mark_id IS NULL GROUP BY host ORDER BY MAX(last_at) DESC LIMIT ?",
        (limit,),
    )
    .map_err(sql("reading the hosts to judge"))
}

/// Writes a mark for a host (by the Sentinel, or the member's "safe") and points its visits at it.
/// A member's mark is never overwritten by the Sentinel.
pub fn mark(c: &mut Conn, host: &str, verdict: Verdict, reason: &str, by: &str, pattern_id: Option<u64>) -> Result<(), String> {
    let host = clean_host(host).ok_or("That is not a host.")?;
    let reason: String = reason.trim().chars().take(300).collect();
    if by == "sentinel" {
        let kept: Option<String> = c.exec_first("SELECT marked_by FROM sentinel_site_marks WHERE host = ?", (&host,)).map_err(sql("reading a mark"))?;
        if kept.as_deref() == Some("member") {
            return Ok(());
        }
    }
    c.exec_drop(
        "INSERT INTO sentinel_site_marks (host, verdict, reason, marked_by, pattern_id) VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE verdict = VALUES(verdict), reason = VALUES(reason), marked_by = VALUES(marked_by), pattern_id = VALUES(pattern_id), marked_at = NOW()",
        (&host, verdict.word(), &reason, by, pattern_id),
    )
    .map_err(sql("writing a mark"))?;
    c.exec_drop("UPDATE browser_visits SET mark_id = (SELECT id FROM sentinel_site_marks WHERE host = ?) WHERE host = ?", (&host, &host)).map_err(sql("pointing the visits at the mark"))
}

/// A host's mark: what it says, and whether the member set it.
pub fn mark_of(c: &mut Conn, host: &str) -> Result<Option<(Warning, bool)>, String> {
    let row: Option<(String, String, String)> = c.exec_first("SELECT verdict, reason, marked_by FROM sentinel_site_marks WHERE host = ?", (host,)).map_err(sql("reading a mark"))?;
    Ok(row.and_then(|(v, reason, by)| Some((Warning { verdict: Verdict::from_word(&v)?, reason }, by == "member"))))
}

/// Writes a pattern the Sentinel learned (one per kind and value; a second time refreshes its why
/// and turns it back on). Its id.
pub fn add_pattern(c: &mut Conn, kind: &str, value: &str, verdict: Verdict, why: &str, from: &str) -> Result<u64, String> {
    if !crate::site_rules::PATTERN_KINDS.contains(&kind) || verdict == Verdict::Safe {
        return Err("That is not a pattern.".into());
    }
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() || value.len() > 253 || !value.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '-') {
        return Err("A pattern's value is letters, digits, dots and dashes.".into());
    }
    c.exec_drop(
        "INSERT INTO sentinel_site_patterns (kind, value, verdict, why, learned_from) VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE verdict = VALUES(verdict), why = VALUES(why), learned_from = VALUES(learned_from), active = 1",
        (kind, &value, verdict.word(), why.trim().chars().take(300).collect::<String>(), from),
    )
    .map_err(sql("keeping a pattern"))?;
    let id: Option<u64> = c.exec_first("SELECT id FROM sentinel_site_patterns WHERE kind = ? AND value = ?", (kind, &value)).map_err(sql("reading a pattern"))?;
    id.ok_or_else(|| "The pattern could not be read back.".to_string())
}

/// What the browser's guard keeps in memory, so a page can be held back before it loads without
/// asking the database: the hosts marked dangerous (with why), and the hosts the member marked safe.
pub fn guard_marks(c: &mut Conn) -> Result<(Vec<(String, String)>, Vec<String>), String> {
    let rows: Vec<(String, String, String, String)> = c
        .query("SELECT host, verdict, reason, marked_by FROM sentinel_site_marks WHERE verdict = 'dangerous' OR (marked_by = 'member' AND verdict = 'safe') LIMIT 5000")
        .map_err(sql("reading the marks for the guard"))?;
    let (mut dangerous, mut member_safe) = (Vec::new(), Vec::new());
    for (host, verdict, reason, by) in rows {
        if by == "member" && verdict == "safe" {
            member_safe.push(host);
        } else {
            dangerous.push((host, reason));
        }
    }
    Ok((dangerous, member_safe))
}

/// The patterns that still count.
pub fn active_patterns(c: &mut Conn) -> Result<Vec<Pattern>, String> {
    let rows: Vec<(String, String, String, String)> = c.query("SELECT kind, value, verdict, why FROM sentinel_site_patterns WHERE active = 1").map_err(sql("reading the patterns"))?;
    Ok(rows.into_iter().filter_map(|(kind, value, verdict, why)| Some(Pattern { kind, value, verdict: Verdict::from_word(&verdict)?, why })).collect())
}

/// Switches a pattern off (never deleted, so its why stays).
pub fn switch_off(c: &mut Conn, id: u64) -> Result<(), String> {
    c.exec_drop("UPDATE sentinel_site_patterns SET active = 0 WHERE id = ?", (id,)).map_err(sql("switching a pattern off"))
}

/// One mark as the Sentinel sheet lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkRow {
    pub host: String,
    pub verdict: String,
    pub reason: String,
    pub by: String,
    pub at: String,
}

/// One pattern as the Sentinel sheet lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternRow {
    pub id: u64,
    pub kind: String,
    pub value: String,
    pub verdict: String,
    pub why: String,
    pub learned_from: String,
    pub active: bool,
}

/// The latest marks that are not safe, and every pattern (switched-off ones too, marked so).
pub fn listing(c: &mut Conn) -> Result<(Vec<MarkRow>, Vec<PatternRow>), String> {
    let marks: Vec<(String, String, String, String, String)> = c
        .query("SELECT host, verdict, reason, marked_by, DATE_FORMAT(marked_at, '%Y-%m-%d %H:%i') FROM sentinel_site_marks WHERE verdict <> 'safe' OR marked_by = 'member' ORDER BY marked_at DESC LIMIT 50")
        .map_err(sql("reading the marks"))?;
    let patterns: Vec<(u64, String, String, String, String, String, bool)> = c
        .query("SELECT id, kind, value, verdict, why, learned_from, active = 1 FROM sentinel_site_patterns ORDER BY active DESC, created_at DESC LIMIT 100")
        .map_err(sql("reading the patterns"))?;
    Ok((
        marks.into_iter().map(|(host, verdict, reason, by, at)| MarkRow { host, verdict, reason, by, at }).collect(),
        patterns.into_iter().map(|(id, kind, value, verdict, why, learned_from, active)| PatternRow { id, kind, value, verdict, why, learned_from, active }).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visit_keeps_no_query_and_only_web_pages() {
        assert_eq!(visit_of("https://Shop.Example.com/cart/pay?token=SECRET#step2", " Pay now "), Some(("shop.example.com".into(), "/cart/pay".into(), "Pay now".into())));
        assert_eq!(visit_of("https://example.com", ""), Some(("example.com".into(), "/".into(), "".into())));
        assert_eq!(visit_of("file:///C:/secret.txt", "x"), None);
        assert_eq!(visit_of("about:blank", "x"), None);
        assert_eq!(visit_of("http://localhost:5173/", "x"), None, "a host without a dot is not a site to judge");
    }

    #[test]
    #[ignore]
    fn live_database_visits_marks_and_patterns() {
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Sites Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();

        let (host, path, title) = visit_of("https://paypal-login-secure.top/signin?session=abc", "Sign in").unwrap();
        keep_visit(&mut c, &host, &path, &title, true).unwrap();
        keep_visit(&mut c, &host, &path, &title, false).unwrap();
        let (count, login, stored_path): (u32, bool, String) = c.query_first("SELECT count, has_login = 1, path FROM browser_visits").unwrap().unwrap();
        assert_eq!((count, login, stored_path.as_str()), (2, true, "/signin"), "counted, the password box kept, no query");
        assert_eq!(unmarked(&mut c, 20).unwrap(), vec![(host.clone(), true)]);

        let pid = add_pattern(&mut c, "suffix", "-secure.top", Verdict::Dangerous, "fake sign-in pages end like this", &host).unwrap();
        mark(&mut c, &host, Verdict::Dangerous, "a PayPal sign-in on a strange host", "sentinel", Some(pid)).unwrap();
        assert!(unmarked(&mut c, 20).unwrap().is_empty(), "the visits point at the mark");
        assert_eq!(mark_of(&mut c, &host).unwrap().unwrap().0.verdict, Verdict::Dangerous);
        assert_eq!(active_patterns(&mut c).unwrap().len(), 1);

        // the member's safe stands; the Sentinel cannot overwrite it
        mark(&mut c, &host, Verdict::Safe, "mine", "member", None).unwrap();
        mark(&mut c, &host, Verdict::Dangerous, "again", "sentinel", None).unwrap();
        let (w, by_member) = mark_of(&mut c, &host).unwrap().unwrap();
        assert!(by_member && w.verdict == Verdict::Safe);

        switch_off(&mut c, pid).unwrap();
        assert!(active_patterns(&mut c).unwrap().is_empty());
        let (marks, patterns) = listing(&mut c).unwrap();
        assert_eq!((marks.len(), patterns.len(), patterns[0].active), (1, 1, false), "switched off, never deleted");
        assert!(add_pattern(&mut c, "regex", ".*", Verdict::Dangerous, "x", &host).is_err());
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }
}
