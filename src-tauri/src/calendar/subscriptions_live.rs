//! Opt-in tests for calendar subscriptions: a real database, and one real fetch of the Malaysia feed
//! (both ignored unless asked for).
use super::subscriptions::*;
use chrono::{NaiveDate, NaiveDateTime};
use mysql::{prelude::Queryable, Conn};

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}
fn at(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}
fn row(uid: &str, d: &str, title: &str, regions: &str, tentative: bool) -> Row {
    Row { uid: uid.into(), day: day(d), end_day: None, title: title.into(), regions: regions.into(), tentative }
}
fn same(t: NaiveDateTime) -> NaiveDateTime {
    t
}

#[test]
#[ignore]
fn live_database_subscriptions_show_by_state_replace_keep_on_failure_and_cascade() {
    let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
        // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
        let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
        let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
        format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
    });
    let mut parts = config.splitn(3, ':');
    let port = parts.next().unwrap().parse().unwrap();
    let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
    let access = crate::memdb::provision(port, &user, &pass, &format!("Subs Test {}", std::process::id())).unwrap();
    let mut c: Conn = crate::memdb::open(&access).unwrap();
    crate::memdb::migrate(&mut c).unwrap();
    let seen: Option<String> = c.query_first("SELECT name FROM schema_migrations WHERE name='025_calendar_subscriptions'").unwrap();
    assert!(seen.is_some());

    // adding: the preset, a duplicate, a bad address, and the limit
    let id = add(&mut c, "Holidays in Malaysia", MALAYSIA_URL, "malaysia").unwrap();
    assert!(add(&mut c, "Again", MALAYSIA_URL, "").unwrap_err().contains("already"));
    assert!(add(&mut c, "Local", "https://127.0.0.1/a.ics", "").is_err());
    assert!(add(&mut c, "Plain", "http://example.com/a.ics", "").is_err());
    assert!(add(&mut c, "  ", "https://example.com/a.ics", "").unwrap_err().contains("name"));
    for n in 1..MAX_SUBS {
        add(&mut c, &format!("Extra {n}"), &format!("https://example.com/{n}.ics"), "").unwrap();
    }
    assert!(add(&mut c, "One too many", "https://example.com/many.ics", "").unwrap_err().contains("At most"));

    // a fresh copy: nationwide, regional, tentative
    let rows = vec![
        row("a", "2026-12-25", "Christmas Day", "", false),
        row("b", "2026-11-25", "Diwali (regional holiday)", "|Selangor|Sabah|", false),
        row("c", "2026-11-26", "Kelantan day", "|Kelantan|", false),
        row("d", "2026-11-27", "Hari Raya", "", true),
        Row { end_day: Some(day("2026-12-02")), ..row("e", "2026-12-01", "Long break", "", false) },
    ];
    store_rows(&mut c, id, &rows, at("2026-11-22 09:00:00")).unwrap();
    let titles = |c: &mut Conn| -> Vec<String> { items(c, day("2026-11-01"), day("2026-12-31")).unwrap().into_iter().map(|i| format!("{} {}", i.date, i.title)).collect() };
    // no state chosen: nationwide days only
    let none = titles(&mut c);
    assert_eq!(none, vec!["2026-11-27 Hari Raya", "2026-12-01 Long break", "2026-12-02 Long break", "2026-12-25 Christmas Day"]);
    // a state chosen: its own days appear too
    set(&mut c, id, true, "Selangor").unwrap();
    assert!(titles(&mut c).contains(&"2026-11-25 Diwali (regional holiday)".to_string()));
    assert!(!titles(&mut c).iter().any(|t| t.contains("Kelantan day")), "another state's holiday stays hidden");
    assert!(set(&mut c, id, true, "Atlantis").is_err(), "only the listed states");
    let tentative = items(&mut c, day("2026-11-27"), day("2026-11-27")).unwrap();
    assert!(tentative[0].detail.contains("date may change") && tentative[0].detail.contains("nationwide"), "{}", tentative[0].detail);
    let regional = items(&mut c, day("2026-11-25"), day("2026-11-25")).unwrap();
    assert!(regional[0].detail.contains("Selangor, Sabah"));
    assert!(regional[0].all_day && regional[0].kind == "holiday");
    let long = items(&mut c, day("2026-12-02"), day("2026-12-02")).unwrap();
    assert_eq!((long.len(), long[0].first), (1, false), "the second day of a two-day holiday is not the first piece");

    // a refresh replaces the copy: what the file dropped is gone
    store_rows(&mut c, id, &[row("a", "2026-12-25", "Christmas Day", "", false), row("z", "2026-12-26", "Boxing Day", "", false)], at("2026-11-23 09:00:00")).unwrap();
    assert_eq!(titles(&mut c), vec!["2026-12-25 Christmas Day", "2026-12-26 Boxing Day"]);

    // a failed refresh keeps the last copy and says so quietly
    record_failure(&mut c, id, "The calendar could not be reached. Castle will try again later.", at("2026-11-24 09:00:00"));
    assert_eq!(titles(&mut c).len(), 2, "the old copy is still shown");
    let sub = list(&mut c).unwrap().into_iter().find(|s| s.id == id).unwrap();
    assert!(sub.last_error.contains("could not be reached"));
    assert_eq!(sub.entries, 2);
    assert_eq!(sub.last_ok_at.as_deref(), Some("2026-11-23 09:00"));

    // when a refresh is due: never tried, a day after success, six hours after a failure
    let due_ids = |c: &mut Conn, now: &str| due(c, at(now)).unwrap();
    assert!(due_ids(&mut c, "2026-11-24 10:00:00").iter().filter(|i| **i == id).count() == 0, "an hour after a failed try: wait");
    assert!(due_ids(&mut c, "2026-11-24 15:30:00").contains(&id), "six hours after a failed try: again");
    store_rows(&mut c, id, &rows, at("2026-11-25 09:00:00")).unwrap();
    assert!(!due_ids(&mut c, "2026-11-26 08:00:00").contains(&id), "a day has not passed since the last good copy");
    assert!(due_ids(&mut c, "2026-11-26 09:01:00").contains(&id));
    set(&mut c, id, false, "").unwrap();
    assert!(!due_ids(&mut c, "2026-12-30 09:00:00").contains(&id), "a switched-off calendar is never fetched");
    assert!(titles(&mut c).is_empty(), "and shows nothing");
    set(&mut c, id, true, "").unwrap();

    // removing a calendar takes its copy with it
    let before: Option<u64> = c.exec_first("SELECT COUNT(*) FROM calendar_holidays WHERE sub_id=?", (id,)).unwrap();
    assert!(before.unwrap() > 0);
    remove(&mut c, id).unwrap();
    let after: Option<u64> = c.exec_first("SELECT COUNT(*) FROM calendar_holidays WHERE sub_id=?", (id,)).unwrap();
    assert_eq!(after, Some(0));
    assert!(remove(&mut c, id).is_err());

    c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    let mut root = Conn::new(mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(user.as_str())).pass(Some(pass.as_str()))).unwrap();
    for host in ["localhost", "127.0.0.1"] {
        root.query_drop(format!("DROP USER IF EXISTS '{}'@'{host}'", access.user)).unwrap();
    }
}

/// The one test that uses the internet: it fetches the real Malaysia calendar, reads it, and checks what
/// came back is sensible. Run with `cargo test fetch_the_real_malaysia -- --ignored`.
#[test]
#[ignore]
fn fetch_the_real_malaysia_calendar_and_read_it() {
    let text = fetch(MALAYSIA_URL).expect("the Malaysia calendar is reachable");
    assert!(text.contains("BEGIN:VCALENDAR") && text.len() < 2 * 1024 * 1024);
    let today = chrono::Local::now().date_naive();
    let (rows, warnings) = rows_from(&text, today, &same);
    assert!(rows.len() > 20, "{} entries", rows.len());
    assert!(warnings.iter().all(|w| !w.contains("left out")), "{warnings:?}");
    let nationwide = rows.iter().filter(|r| r.regions.is_empty()).count();
    let regional = rows.iter().filter(|r| !r.regions.is_empty()).count();
    assert!(nationwide > 5 && regional > 5, "nationwide {nationwide}, regional {regional}");
    assert!(rows.iter().any(|r| r.title.contains("Malaysia Day")), "the national day is in there");
    assert!(rows.iter().any(|r| r.tentative), "some dates are marked as tentative");
    eprintln!("fetched {} entries ({nationwide} nationwide, {regional} regional)", rows.len());
}
