//! Private browsing remembered for the member's own companion (his word Nov 25 2026: "they should
//! have switch and it their own database, i wont check it"). With the switch on, a page opened while
//! browsing privately (Remember sign-ins off) is kept in the member's own memory database for 30
//! days: the site, the page's path (never the part after `?` or `#`) and its title. It is never
//! shown in any history. Only MY PROFILE's month reading reads it, with the month's normal visits,
//! as evidence about the member - never the companion's own traits, never anything that leaves the
//! computer.

use mysql::{prelude::Queryable, Conn};

/// How long a private visit is kept.
pub const KEEP_DAYS: u32 = 30;
/// At most this many sites in one month's reading.
pub const MONTH_SITES: usize = 100;

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("Private browsing: {what} did not work.")
}

/// Keeps one private visit and lets go of every one older than 30 days.
pub fn remember(c: &mut Conn, host: &str, path: &str, title: &str) -> Result<(), String> {
    c.exec_drop("INSERT INTO browser_private_visits (host, path, title) VALUES (?, ?, ?)", (host, path, title)).map_err(sql("keeping a visit"))?;
    c.exec_drop("DELETE FROM browser_private_visits WHERE at < NOW() - INTERVAL ? DAY", (KEEP_DAYS,)).map_err(sql("letting old visits go"))
}

/// One site in a month's browsing: its name, how many visits, and up to three page titles.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub host: String,
    pub visits: u32,
    pub titles: Vec<String>,
}

/// Merges rows of (host, visits, title) into sites, the most visited first. Pure.
pub fn merge(rows: Vec<(String, u32, String)>) -> Vec<Site> {
    let mut sites: Vec<Site> = Vec::new();
    for (host, visits, title) in rows {
        let title = title.trim().to_string();
        match sites.iter_mut().find(|s| s.host == host) {
            Some(site) => {
                site.visits += visits;
                if !title.is_empty() && site.titles.len() < 3 && !site.titles.contains(&title) {
                    site.titles.push(title);
                }
            }
            None => sites.push(Site { host, visits, titles: if title.is_empty() { Vec::new() } else { vec![title] } }),
        }
    }
    sites.sort_by(|a, b| b.visits.cmp(&a.visits).then_with(|| a.host.cmp(&b.host)));
    sites.truncate(MONTH_SITES);
    sites
}

/// The month's browsing (`YYYY-MM`): normal visits last seen in it and private visits made in it.
pub fn month(c: &mut Conn, month: &str) -> Result<Vec<Site>, String> {
    let mut rows: Vec<(String, u32, String)> = c
        .exec("SELECT host, count, title FROM browser_visits WHERE DATE_FORMAT(last_at, '%Y-%m') = ?", (month,))
        .map_err(sql("reading the month's visits"))?;
    let private: Vec<(String, u32, String)> = c
        .exec("SELECT host, 1, title FROM browser_private_visits WHERE DATE_FORMAT(at, '%Y-%m') = ?", (month,))
        .map_err(sql("reading the month's private visits"))?;
    rows.extend(private);
    Ok(merge(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_merge_most_visited_first_with_three_titles_at_most() {
        let rows = vec![
            ("a.example".into(), 1, "One".into()),
            ("b.example".into(), 5, "".into()),
            ("a.example".into(), 2, "Two".into()),
            ("a.example".into(), 1, "Two".into()),
            ("a.example".into(), 1, "Three".into()),
            ("a.example".into(), 1, "Four".into()),
        ];
        let sites = merge(rows);
        assert_eq!(sites[0], Site { host: "a.example".into(), visits: 6, titles: vec!["One".into(), "Two".into(), "Three".into()] });
        assert_eq!(sites[1], Site { host: "b.example".into(), visits: 5, titles: vec![] });
    }

    #[test]
    fn a_month_holds_at_most_a_hundred_sites() {
        let rows = (0..150).map(|n| (format!("s{n}.example"), 1, String::new())).collect();
        assert_eq!(merge(rows).len(), MONTH_SITES);
    }

    #[test]
    #[ignore]
    fn live_database_private_visits_are_kept_thirty_days_and_join_the_month() {
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Private Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();

        let (host, path, title) = crate::sites::visit_of("https://research.example.org/paper/42?token=SECRET#p3", "A paper").unwrap();
        remember(&mut c, &host, &path, &title).unwrap();
        c.query_drop("INSERT INTO browser_private_visits (host, path, title, at) VALUES ('old.example', '/', 'Old', NOW() - INTERVAL 31 DAY)").unwrap();
        remember(&mut c, &host, &path, &title).unwrap();
        let rows: Vec<(String, String)> = c.query("SELECT host, path FROM browser_private_visits ORDER BY id").unwrap();
        assert_eq!(rows, vec![(host.clone(), "/paper/42".to_string()), (host.clone(), "/paper/42".to_string())], "no query kept, the 31-day-old visit let go");

        crate::sites::keep_visit(&mut c, &host, "/", "Home", false).unwrap();
        let this_month: String = c.query_first("SELECT DATE_FORMAT(NOW(), '%Y-%m')").unwrap().unwrap();
        let sites = month(&mut c, &this_month).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!((sites[0].host.as_str(), sites[0].visits), (host.as_str(), 3), "two private visits and one normal");
        assert!(month(&mut c, "2001-01").unwrap().is_empty());
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }
}
