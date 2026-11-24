//! Test my guard: the three checks that say the guard truly holds, run on the member's press.
//! 1. The antivirus takes the standard harmless test file (made in a temp folder from its two
//!    halves, so no file of Castle's ever holds it whole), and the Sentinel's alarm writes its row.
//! 2. The hand blocks a documentation address (203.0.113.7, never a real computer) and undoes it.
//! 3. The five-minute watch ran in the last ten minutes.
//! The guard is said to hold only when all three pass, never from the unit tests alone. The test
//! file's detection and the practice block are never shared with the Aegis Network.

use super::{book, hand, officer, watch};
use mysql::prelude::Queryable;
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant};

/// The practice address: a documentation range, refused by the Portfolio and never a real computer.
pub const PRACTICE_ADDRESS: &str = "203.0.113.7";
/// How long the antivirus and the alarm are waited for.
const TAKE_WAIT: Duration = Duration::from_secs(90);
/// The watch counts as alive when it ran this recently.
const WATCH_MINUTES: u64 = 10;

/// The standard anti-virus test file's text, put together only in memory.
pub fn test_text() -> String {
    // two halves, the second kept backwards, so neither this source nor Castle's own program file
    // ever holds the string whole (an antivirus would take Castle itself)
    let first = "X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDA";
    let second: String = "*H+H$!ELIF-TSET-SURIVITNA-DR".chars().rev().collect();
    format!("{first}{second}")
}

/// One check and its reason.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub why: String,
}

/// The three checks, and whether the guard holds (all three passed).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tested {
    pub checks: Vec<Check>,
    pub holds: bool,
}

impl Tested {
    pub fn of(checks: Vec<Check>) -> Tested {
        let holds = !checks.is_empty() && checks.iter().all(|c| c.ok);
        Tested { checks, holds }
    }
}

/// The five-minute watch: alive when its heartbeat is under WATCH_MINUTES old.
pub fn watch_check(last_tick: Option<u64>, now_secs: u64) -> Check {
    match last_tick {
        Some(at) if now_secs.saturating_sub(at) <= WATCH_MINUTES * 60 => Check { name: "The five-minute watch", ok: true, why: format!("It last ran {} minutes ago.", now_secs.saturating_sub(at) / 60) },
        Some(at) => Check { name: "The five-minute watch", ok: false, why: format!("It last ran {} minutes ago; it should run every five.", now_secs.saturating_sub(at) / 60) },
        None => Check { name: "The five-minute watch", ok: false, why: "It has not run yet. Is the Sentinel set up?".into() },
    }
}

/// The antivirus takes the test file, and the Sentinel's alarm writes its row.
fn antivirus_check(conn: &mut mysql::Conn) -> Check {
    let name = "The antivirus takes a test file";
    let before: u64 = conn.query_first("SELECT COALESCE(MAX(id), 0) FROM sentinel_alarms").ok().flatten().unwrap_or(0);
    let folder = std::env::temp_dir().join(format!("castle-guard-test-{}", std::process::id()));
    let file = folder.join("guard-test.txt");
    if std::fs::create_dir_all(&folder).and_then(|_| std::fs::write(&file, test_text())).is_err() {
        // a write refused at once is the antivirus taking it before it landed
        let _ = std::fs::remove_dir_all(&folder);
        return alarm_after(conn, before, name, true);
    }
    let until = Instant::now() + TAKE_WAIT;
    let mut taken = false;
    while Instant::now() < until {
        if std::fs::read_to_string(&file).map_or(true, |t| t != test_text()) {
            taken = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&folder);
    if !taken {
        return Check { name, ok: false, why: "The antivirus left the test file alone for 90 seconds. Is real-time protection on?".into() };
    }
    alarm_after(conn, before, name, false)
}

/// Waits for the Sentinel's alarm row after the take (reading the watch's journal as Castle does).
fn alarm_after(conn: &mut mysql::Conn, before: u64, name: &'static str, at_once: bool) -> Check {
    let until = Instant::now() + TAKE_WAIT;
    while Instant::now() < until {
        let _ = watch::drain(&hand::local_dir(), conn);
        let now: u64 = conn.query_first("SELECT COALESCE(MAX(id), 0) FROM sentinel_alarms").ok().flatten().unwrap_or(0);
        if now > before {
            let how = if at_once { "took it before it was written" } else { "took it" };
            return Check { name, ok: true, why: format!("The antivirus {how}, and the Sentinel's alarm wrote its row.") };
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Check { name, ok: false, why: "The antivirus took it, but the Sentinel's alarm did not answer in 90 seconds. Run the set-up line again.".into() }
}

/// The hand blocks the practice address and undoes it.
fn practice_check(conn: &mut mysql::Conn) -> Check {
    let name = "A practice block";
    let blocked = match officer::act(conn, hand::Asker::Member, "block-address", Some(PRACTICE_ADDRESS), None) {
        Ok(a) if a.ok => a,
        Ok(a) => return Check { name, ok: false, why: format!("The hand did not block it: {}", a.result) },
        Err(why) => return Check { name, ok: false, why },
    };
    match officer::act(conn, hand::Asker::Member, "undo", Some(&blocked.id), None) {
        Ok(a) if a.ok => {
            let _ = conn.exec_drop("UPDATE sentinel_actions SET state = 'undone', undone_at = NOW() WHERE move = 'block-address' AND arg = ? AND state = 'done' AND finding_id IS NULL", (PRACTICE_ADDRESS,));
            Check { name, ok: true, why: format!("The hand blocked {PRACTICE_ADDRESS} and took the block away again.") }
        }
        Ok(a) => Check { name, ok: false, why: format!("The block was made but not undone: {}. Undo it in Windows Firewall.", a.result) },
        Err(why) => Check { name, ok: false, why: format!("The block was made but not undone: {why}") },
    }
}

/// Runs the three checks in turn.
pub fn test_guard(memory: &Path) -> Result<Tested, String> {
    if !cfg!(windows) {
        return Err(super::look::NOT_HERE.into());
    }
    let mut conn = book::open(memory)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let checks = vec![antivirus_check(&mut conn), practice_check(&mut conn), watch_check(watch::last_tick(&hand::local_dir()), now)];
    let tested = Tested::of(checks);
    let lines: Vec<String> = tested.checks.iter().map(|c| format!("{}: {} - {}", c.name, if c.ok { "passed" } else { "not yet" }, c.why)).collect();
    officer::log(&format!("\nTest my guard: {}\n{}\n", if tested.holds { "the guard holds" } else { "not yet" }, lines.join("\n")));
    Ok(tested)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_text_is_the_standard_one_and_is_known_as_a_drill() {
        let text = test_text();
        assert_eq!(text.len(), 68);
        assert!(text.starts_with("X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-") && text.ends_with("!$H+H*"));
        assert!(super::super::network::is_drill_name(&text));
    }

    #[test]
    fn the_watch_is_alive_only_when_it_ran_lately() {
        assert!(watch_check(Some(1_000), 1_000 + 4 * 60).ok);
        let late = watch_check(Some(1_000), 1_000 + 25 * 60);
        assert!(!late.ok && late.why.contains("25 minutes"));
        assert!(!watch_check(None, 1_000).ok);
    }

    #[test]
    fn the_guard_holds_only_when_all_three_pass() {
        let pass = |name| Check { name, ok: true, why: String::new() };
        assert!(Tested::of(vec![pass("a"), pass("b"), pass("c")]).holds);
        assert!(!Tested::of(vec![pass("a"), Check { name: "b", ok: false, why: String::new() }, pass("c")]).holds);
        assert!(!Tested::of(Vec::new()).holds);
        assert!(super::super::network::is_documentation(PRACTICE_ADDRESS), "the practice address is never a real computer");
    }
}
