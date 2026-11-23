//! Opt-in tests for `.ics` import and export against real databases (ignored unless asked for).
use super::exchange::*;
use super::ics;
use super::repeat::Repeat;
use super::store::{self, Event};
use chrono::{NaiveDate, NaiveDateTime};
use mysql::{prelude::Queryable, Conn};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
}
fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}
fn event(title: &str, start: &str) -> Event {
    Event { id: 0, title: title.into(), note: String::new(), starts_at: start.into(), ends_at: None, all_day: false, repeat: None, remind_before_min: None, colour: "accent".into(), automation_job_id: None, automation_lead_min: None, state: "active".into(), created_by: "member".into() }
}
fn save(c: &mut Conn, e: Event) -> u64 {
    store::save(c, &store::check(e).unwrap()).unwrap()
}
fn same_tz(t: NaiveDateTime) -> NaiveDateTime {
    t
}

/// What a window shows, as plain lines, to compare two calendars.
fn view(c: &mut Conn) -> Vec<String> {
    let mut lines: Vec<String> = store::items(c, day("2026-11-01"), day("2027-02-28"))
        .unwrap()
        .into_iter()
        .map(|i| format!("{} {} {} end={:?} allday={}", i.date, i.time.unwrap_or_default(), i.title, i.end_time, i.all_day))
        .collect();
    lines.sort();
    lines
}

/// Two isolated databases for one test; `CASTLE_TEST_DB=port:admin:password`, or the installed database's
/// port and stored root key.
struct Pair {
    a: Conn,
    b: Conn,
    port: u16,
    admin: (String, String),
    made: Vec<crate::memdb::Access>,
}

fn two_databases() -> Pair {
    let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
        // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
        let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
        let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
        format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
    });
    let mut parts = config.splitn(3, ':');
    let port: u16 = parts.next().unwrap().parse().unwrap();
    let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
    let a = crate::memdb::provision(port, &user, &pass, &format!("Ics A {}", std::process::id())).unwrap();
    let b = crate::memdb::provision(port, &user, &pass, &format!("Ics B {}", std::process::id())).unwrap();
    let ca = crate::memdb::open(&a).unwrap();
    let cb = crate::memdb::open(&b).unwrap();
    Pair { a: ca, b: cb, port, admin: (user, pass), made: vec![a, b] }
}

impl Pair {
    /// Remove exactly the two databases and two accounts this test made.
    fn remove(self) {
        let Pair { a, b, port, admin, made } = self;
        drop((a, b));
        let mut root = Conn::new(mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(admin.0.as_str())).pass(Some(admin.1.as_str()))).unwrap();
        for access in made {
            root.query_drop(format!("DROP DATABASE IF EXISTS `{}`", access.name)).unwrap();
            for host in ["localhost", "127.0.0.1"] {
                root.query_drop(format!("DROP USER IF EXISTS '{}'@'{host}'", access.user)).unwrap();
            }
        }
    }
}

#[test]
#[ignore]
fn live_database_export_then_import_round_trips_and_a_second_import_adds_nothing() {
    let mut pair = two_databases();
    let (a, b) = (&mut pair.a, &mut pair.b);

    // a calendar with a bit of everything
    let mut dentist = event("Dentist, 3rd floor; bring card", "2026-11-27 15:00");
    dentist.ends_at = Some("2026-11-27 16:00".into());
    dentist.note = "Ask about the crown\nSecond line".into();
    save(a, dentist);
    let mut trip = event("Trip", "2026-12-24 00:00");
    trip.all_day = true;
    trip.ends_at = Some("2026-12-26 00:00".into());
    save(a, trip);
    let weekly = Repeat { every: "week".into(), interval: 1, weekdays: vec![1, 3], until: None, count: Some(8) };
    let mut gym = event("Gym", "2026-11-02 18:30");
    gym.ends_at = Some("2026-11-02 19:30".into());
    gym.repeat = Some(weekly);
    let gym_id = save(a, gym);
    store::change_day(a, gym_id, day("2026-11-16"), None).unwrap();
    store::change_day(a, gym_id, day("2026-11-04"), Some(dt("2026-11-05 07:00"))).unwrap();
    // things that must NOT travel: a suggestion, a turned-down suggestion, and notes about reminders
    let idea = save(a, event("Idea", "2026-12-02 10:00"));
    a.exec_drop("UPDATE calendar_events SET state='proposed' WHERE id=?", (idea,)).unwrap();
    let no = save(a, event("No thanks", "2026-12-03 10:00"));
    a.exec_drop("UPDATE calendar_events SET state='cancelled' WHERE id=?", (no,)).unwrap();
    a.exec_drop("INSERT INTO reminders (title,due_date,detail) VALUES ('Call mum','2026-12-01','')", ()).unwrap();

    let all = view(a);
    assert!(all.iter().any(|l| l.contains("Idea")), "the suggestion shows in the room that has it");
    let before: Vec<String> = all.into_iter().filter(|l| !l.contains("Idea")).collect();
    assert!(before.iter().any(|l| l.contains("Gym")));

    // export, with and without the dated reminders
    let (text, count) = export(a, false, dt("2026-11-22 10:00")).unwrap();
    assert_eq!(count, 3, "dentist, trip and gym; not the suggestion or the turned-down one");
    assert!(!text.contains("Idea") && !text.contains("No thanks") && !text.contains("Call mum"));
    let (with_dated, count2) = export(a, true, dt("2026-11-22 10:00")).unwrap();
    assert_eq!(count2, 4);
    assert!(with_dated.contains("SUMMARY:Call mum") && with_dated.contains("CATEGORIES:Reminder"));

    // import into the other calendar: everything is new, and it looks the same afterwards
    let parsed = ics::parse(&text, &same_tz);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let dup = duplicates(b, &parsed.events).unwrap();
    assert!(dup.iter().all(|d| !*d));
    let first = import(b, &parsed, &dup).unwrap();
    assert_eq!((first.imported, first.duplicates, first.refused), (3, 0, 0));
    assert_eq!(view(b), before, "title, time, end, repeat, a skipped day and a moved day all came across");
    let made_by: Vec<(String, Option<String>)> = b.query("SELECT created_by,ics_uid FROM calendar_events ORDER BY id").unwrap();
    assert!(made_by.iter().all(|(who, uid)| who == "import" && uid.is_some()));
    let reminders: Option<u64> = b.query_first("SELECT COUNT(*) FROM calendar_events WHERE remind_before_min IS NOT NULL OR automation_job_id IS NOT NULL").unwrap();
    assert_eq!(reminders, Some(0), "nothing from a file can ring or start anything");

    // the same file again adds nothing, in the calendar it came from and in the one it went to
    for conn in [&mut *a, &mut *b] {
        let again = ics::parse(&text, &same_tz);
        let dup = duplicates(conn, &again.events).unwrap();
        assert!(dup.iter().all(|d| *d), "every entry is already there");
        let s = import(conn, &again, &dup).unwrap();
        assert_eq!((s.imported, s.duplicates), (0, 3));
    }
    // and re-exporting the imported calendar keeps the same ids, so it can go round again
    let (second, _) = export(b, false, dt("2026-11-22 10:00")).unwrap();
    let uids = |t: &str| -> Vec<String> { let mut u: Vec<String> = t.lines().filter(|l| l.starts_with("UID:")).map(String::from).collect(); u.sort(); u.dedup(); u };
    assert_eq!(uids(&second), uids(&text));

    // Castle's own reminders and tasks coming back are not made into events
    let back = ics::parse(&with_dated, &same_tz);
    assert_eq!(back.own_dated, 1);
    assert_eq!(back.events.len(), 3);

    // a file from somewhere else: a hostile title stays text, and no reminder or link gets in
    let hostile = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:evil-1@example.com\r\nDTSTART:20261201T090000\r\nSUMMARY:<script>alert(1)</script> {{payload.text}}\r\nRRULE:FREQ=DAILY;COUNT=99999\r\nBEGIN:VALARM\r\nTRIGGER:-PT1M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let parsed = ics::parse(hostile, &same_tz);
    let dup = duplicates(b, &parsed.events).unwrap();
    import(b, &parsed, &dup).unwrap();
    let row: Option<(String, Option<u32>, String)> = b.query_first("SELECT title,remind_before_min,CAST(repeat_json AS CHAR) FROM calendar_events WHERE ics_uid='evil-1@example.com'").unwrap();
    let (title, remind, rule) = row.unwrap();
    assert_eq!(title, "<script>alert(1)</script> {{payload.text}}");
    assert_eq!(remind, None);
    assert!(rule.contains("\"count\":1000"), "{rule}");

    pair.remove();
}
