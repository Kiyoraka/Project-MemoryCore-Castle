//! Opt-in tests for the automation link against a real database (ignored unless asked for).
use super::link::*;
use super::repeat::Repeat;
use super::store::{self, Event};
use chrono::{NaiveDate, NaiveDateTime};
use mysql::{prelude::Queryable, Conn};

fn t(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn event(title: &str, start: &str, job: Option<u64>, lead: u32, repeat: Option<Repeat>) -> Event {
    Event {
        id: 0,
        title: title.into(),
        note: String::new(),
        starts_at: start.into(),
        ends_at: None,
        all_day: false,
        repeat,
        remind_before_min: None,
        colour: "accent".into(),
        automation_job_id: job,
        automation_lead_min: Some(lead),
        state: "active".into(),
        created_by: "member".into(),
    }
}

/// Opt-in isolated database: the generated test database and user are created and dropped here.
/// `CASTLE_TEST_DB=port:admin:password`, or the installed database's port and stored root key.
#[test]
#[ignore]
fn live_database_links_start_once_skip_when_late_and_unlink() {
    let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
        // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
        let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
        let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
        format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
    });
    let mut parts = config.splitn(3, ':');
    let port = parts.next().unwrap().parse().unwrap();
    let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
    let access = crate::memdb::provision(port, &user, &pass, &format!("Links Test {}", std::process::id())).unwrap();
    let mut c = crate::memdb::open(&access).unwrap();
    crate::memdb::migrate(&mut c).unwrap();
    let seen: Option<String> = c.query_first("SELECT name FROM schema_migrations WHERE name='023_calendar_automation'").unwrap();
    assert!(seen.is_some());

    let job = |c: &mut Conn, name: &str, ev: &str| -> u64 {
        c.exec_drop(
            "INSERT INTO automation_jobs (name,note,enabled,trigger_kind,trigger_json,action_kind,action_json,delivery_json,policy_json) VALUES (?,'',1,'event',?,'companion','{}','[]','{}')",
            (name, format!("{{\"event\":\"{ev}\"}}")),
        )
        .unwrap();
        c.last_insert_id()
    };
    let briefing = job(&mut c, "Briefing", "calendar_event");
    let other = job(&mut c, "After backup", "backup_done");
    let names: Vec<u64> = eligible(&mut c).unwrap().into_iter().map(|j| j.id).collect();
    assert_eq!(names, vec![briefing], "only a job that waits for a calendar event can be linked");

    let make = |c: &mut Conn, title: &str, start: &str, job: Option<u64>, lead: u32, repeat: Option<Repeat>| -> u64 {
        let ok = store::check(event(title, start, job, lead, repeat)).unwrap();
        check_link(c, &ok.event).unwrap();
        store::save(c, &ok).unwrap()
    };
    let bad = store::check(event("x", "2026-11-27 15:00", Some(other), 0, None)).unwrap();
    assert!(check_link(&mut c, &bad.event).is_err(), "a job with another trigger is refused");
    let missing = store::check(event("x", "2026-11-27 15:00", Some(99999), 0, None)).unwrap();
    assert!(check_link(&mut c, &missing.event).is_err());

    // one meeting, a job 30 minutes before: nothing early, one start on time, never a second
    let meeting = make(&mut c, "Planning", "2026-11-27 09:00", Some(briefing), 30, None);
    assert!(collect(&mut c, t("2026-11-27 08:29:59")).unwrap().is_empty());
    let first = collect(&mut c, t("2026-11-27 08:30:05")).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].job, first[0].due.event_id), (briefing, meeting));
    assert_eq!(payload_text(&first[0].due), "Planning - in 30 minutes (09:00)");
    c.exec_drop("UPDATE calendar_triggers SET status='started',run_id=7 WHERE id=?", (first[0].row,)).unwrap();
    assert!(collect(&mut c, t("2026-11-27 08:30:20")).unwrap().is_empty());
    drop(c);
    let mut c = crate::memdb::open(&access).unwrap();
    assert!(collect(&mut c, t("2026-11-27 08:45:00")).unwrap().is_empty(), "a new connection and a later time start nothing again");
    let status: Option<(String, Option<u64>)> = c.exec_first("SELECT status,run_id FROM calendar_triggers WHERE event_id=?", (meeting,)).unwrap();
    assert_eq!(status, Some(("started".to_string(), Some(7))));

    // Castle was closed: a start more than five minutes late is recorded as skipped, never run late
    let away = make(&mut c, "Missed", "2026-12-01 09:00", Some(briefing), 0, None);
    assert!(collect(&mut c, t("2026-12-01 10:00:00")).unwrap().is_empty());
    let skipped: Option<String> = c.exec_first("SELECT status FROM calendar_triggers WHERE event_id=?", (away,)).unwrap();
    assert_eq!(skipped.as_deref(), Some("skipped"));
    assert!(collect(&mut c, t("2026-12-01 10:00:15")).unwrap().is_empty());

    // saving an event whose start has already passed does not run it at once
    let past = make(&mut c, "Already", "2026-12-02 09:00", Some(briefing), 30, None);
    settle(&mut c, past, t("2026-12-02 08:50:00")).unwrap();
    assert!(collect(&mut c, t("2026-12-02 08:50:10")).unwrap().is_empty());
    let note: Option<String> = c.exec_first("SELECT note FROM calendar_triggers WHERE event_id=?", (past,)).unwrap();
    assert_eq!(note.as_deref(), Some("Already past when the event was saved."));

    // a repeating event starts the job each time; a skipped day and a moved day follow the calendar
    let weekly = Repeat { every: "week".into(), interval: 1, weekdays: vec![], until: None, count: None };
    let gym = make(&mut c, "Gym", "2026-11-02 18:30", Some(briefing), 0, Some(weekly));
    store::change_day(&mut c, gym, NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(), None).unwrap();
    assert_eq!(collect(&mut c, t("2026-11-02 18:30:02")).unwrap().len(), 1);
    assert!(collect(&mut c, t("2026-11-16 18:30:02")).unwrap().is_empty(), "a skipped Monday starts nothing");
    assert_eq!(collect(&mut c, t("2026-11-23 18:30:02")).unwrap().len(), 1);
    store::change_day(&mut c, gym, NaiveDate::from_ymd_opt(2026, 11, 30).unwrap(), Some(t("2026-12-01 07:00:00"))).unwrap();
    assert!(collect(&mut c, t("2026-11-30 18:30:02")).unwrap().is_empty());
    assert_eq!(collect(&mut c, t("2026-12-01 07:00:02")).unwrap().len(), 1, "a moved day starts the job at its new time");

    // a suggestion never starts anything
    let idea = make(&mut c, "Idea", "2026-12-05 10:00", Some(briefing), 0, None);
    c.exec_drop("UPDATE calendar_events SET state='proposed' WHERE id=?", (idea,)).unwrap();
    assert!(collect(&mut c, t("2026-12-05 10:00:02")).unwrap().is_empty());

    // the day panel says what the event starts
    let day = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap();
    let items = store::items(&mut c, day, day).unwrap();
    assert_eq!(items[0].detail, "Runs \"Briefing\" 30 minutes before");

    // archiving the job frees the events, deleting an event takes its start records with it
    unlink_job(&mut c, briefing);
    let linked: Option<u64> = c.query_first("SELECT COUNT(*) FROM calendar_events WHERE automation_job_id IS NOT NULL").unwrap();
    assert_eq!(linked, Some(0));
    let kept = store::get(&mut c, meeting).unwrap();
    assert_eq!((kept.automation_job_id, kept.automation_lead_min), (None, None));
    let before: Option<u64> = c.exec_first("SELECT COUNT(*) FROM calendar_triggers WHERE event_id=?", (meeting,)).unwrap();
    assert_eq!(before, Some(1));
    store::delete(&mut c, meeting).unwrap();
    let after: Option<u64> = c.exec_first("SELECT COUNT(*) FROM calendar_triggers WHERE event_id=?", (meeting,)).unwrap();
    assert_eq!(after, Some(0));

    c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    let mut root = Conn::new(mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(user.as_str())).pass(Some(pass.as_str()))).unwrap();
    for host in ["localhost", "127.0.0.1"] {
        root.query_drop(format!("DROP USER IF EXISTS '{}'@'{host}'", access.user)).unwrap();
    }
}
