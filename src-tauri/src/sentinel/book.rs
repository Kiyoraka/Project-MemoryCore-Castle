//! The Sentinel's book: what the look found, as findings with a state, and the member's clicks.
//!
//! A thing is marked by what it is - a port, its scope and its program - never by a process id
//! or a time, so the same thing seen again is the same finding. The first full look takes what
//! the machine already has as the starting picture; after that only what is new opens. Some
//! things open whatever the first look saw (protection off, a firewall off, a detection ...):
//! those are "state" things, never part of the starting picture.
//!
//! The decisions are pure (`plan`) and tested without a database; `collect` applies them.

use mysql::prelude::Queryable;
use mysql::Conn;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

pub const NOTE: &str = "note";
pub const LOOK: &str = "look";
pub const ALARM: &str = "alarm";

pub const OPEN: &str = "open";
pub const SETTLED: &str = "settled";
pub const ACCEPTED: &str = "accepted";
pub const FIXED: &str = "fixed";
pub const GONE: &str = "gone";

/// A fix of these that has not held after this long opens its finding again.
pub const HOLD_SECONDS: i64 = 600;
/// The fixes whose holding is checked: the thing they mend is a state the look reads again.
pub const HELD_FIXES: [&str; 3] = ["firewall-on", "protection-on", "signatures-update"];
/// A member's click is fresh for this long, and spent once.
pub const CLICK_MINUTES: u32 = 10;

/// One thing the look found.
#[derive(Debug, Clone, PartialEq)]
pub struct Thing {
    pub kind: String,
    /// What the thing is; the fingerprint is made of these and the kind.
    pub parts: Vec<String>,
    pub hint: &'static str,
    pub title: String,
    pub detail: String,
    /// The move that would mend it, and its argument.
    pub fix: Option<(String, Option<String>)>,
    /// Part of the machine's inventory (taken into the starting picture), or a state that
    /// opens whatever the first look saw.
    pub inventory: bool,
}

impl Thing {
    pub fn fingerprint(&self) -> String {
        print_of(&self.kind, &self.print_parts())
    }

    /// The parts the mark is made of: a scheduled task's name without what Windows renews (its
    /// action stays whole), everything else as it is.
    fn print_parts(&self) -> Vec<String> {
        let mut parts = self.parts.clone();
        if self.kind == "task" {
            if let Some(name) = parts.first_mut() {
                *name = stable_task(name);
            }
        }
        parts
    }

    /// The mark this thing had before the steady task mark (Castle 0.5.x and older), when it
    /// differs: a task known or open under it moves to the new mark instead of being raised again.
    pub fn legacy_print(&self) -> Option<String> {
        let old = print_of(&self.kind, &self.parts);
        (old != self.fingerprint()).then_some(old)
    }
}

/// A scheduled task's path and name with the parts Windows renews taken out: a `-{GUID}` and any
/// run of twelve or more digits. Windows' own tips tasks (`\SoftLanding\<SID>\...`) come back
/// every day with both renewed; an account SID's parts are ten digits, so they stay.
pub fn stable_task(name: &str) -> String {
    static GUID: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let guid = GUID.get_or_init(|| regex::Regex::new(r"(?i)-?\{[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\}").unwrap());
    let number = NUMBER.get_or_init(|| regex::Regex::new(r"\d{12,}").unwrap());
    number.replace_all(&guid.replace_all(name, ""), "#").into_owned()
}

/// The fingerprint of a thing: sha256 of `kind|part|part`, each lowercased and trimmed, cut to 40.
pub fn print_of<S: AsRef<str>>(kind: &str, parts: &[S]) -> String {
    let mut text = kind.trim().to_lowercase();
    for part in parts {
        text.push('|');
        text.push_str(&part.as_ref().trim().to_lowercase());
    }
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect::<String>()[..40].to_string()
}

/// Ports from here up are handed out afresh at every start, so they are known by program alone.
pub const DYNAMIC_FROM: u16 = 49152;

/// Who can reach a listening port: every address, only this machine, or one address.
pub fn scope_of(address: &str) -> &'static str {
    match address.trim() {
        "0.0.0.0" | "::" | "[::]" | "*" => "every",
        a if a.starts_with("127.") || a == "::1" || a == "[::1]" || a.eq_ignore_ascii_case("localhost") => "local",
        _ => "one",
    }
}

/// The wider of two scopes (an IPv4 and an IPv6 socket of one port are one door).
pub fn wider(a: &'static str, b: &'static str) -> &'static str {
    let rank = |s: &str| match s {
        "every" => 2,
        "one" => 1,
        _ => 0,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

/// What a listening port is: its number (or "dynamic"), its scope and its program.
pub fn port_parts(port: u16, scope: &str, program: &str) -> Vec<String> {
    let number = if port >= DYNAMIC_FROM { "dynamic".to_string() } else { port.to_string() };
    vec![number, scope.to_string(), program.to_string()]
}

/// A finding's row as the plan needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub id: u64,
    pub kind: String,
    pub state: String,
    pub fix_move: Option<String>,
    /// Seconds since the finding was closed (fixed), when it is.
    pub closed_for: Option<i64>,
}

/// What `collect` does with the database.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Taken into the starting picture.
    Baseline { print: String, kind: String, title: String },
    /// A new finding.
    Open { print: String, thing: Thing },
    /// Seen again: last_seen, and its hint, title and detail refreshed.
    Seen { id: u64, thing: Thing },
    /// Open again, with why.
    Reopen { id: u64, thing: Thing, why: String },
    /// No longer on the machine.
    Gone { id: u64 },
    /// Known under its old mark: known under the new one too, as it was (baseline or member).
    Rekey { old: String, new: String },
    /// A finding stored under its old mark moves to the new one.
    Remark { id: u64, print: String },
}

/// Decides what a look means against what the book holds.
/// - `covered`: the kinds this look read (a quick look reads only a few); a finding of another
///   kind is never called gone by it.
/// - `baseline_taken`: the starting picture exists; `full` is true only for a full look, the only
///   one that may take the starting picture.
pub fn plan(found: &[Thing], covered: &HashSet<String>, known: &HashSet<String>, stored: &HashMap<String, Stored>, baseline_taken: bool, full: bool) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut present = HashSet::new();
    let taking_baseline = !baseline_taken && full;
    for thing in found {
        let print = thing.fingerprint();
        if !present.insert(print.clone()) {
            continue;
        }
        let legacy = thing.legacy_print();
        let mut row = stored.get(&print);
        if let Some(old) = &legacy {
            present.insert(old.clone());
            if row.is_none() {
                if let Some(old_row) = stored.get(old) {
                    changes.push(Change::Remark { id: old_row.id, print: print.clone() });
                    row = Some(old_row);
                }
            }
            if !known.contains(&print) && known.contains(old) {
                changes.push(Change::Rekey { old: old.clone(), new: print.clone() });
                if thing.inventory {
                    continue;
                }
            }
        }
        if thing.inventory && known.contains(&print) {
            continue;
        }
        if thing.inventory && taking_baseline && row.is_none() {
            changes.push(Change::Baseline { print, kind: thing.kind.clone(), title: thing.title.clone() });
            continue;
        }
        if thing.inventory && !baseline_taken {
            // a quick look before the starting picture: wait for the full look
            continue;
        }
        match row {
            None => changes.push(Change::Open { print, thing: thing.clone() }),
            Some(row) if row.state == GONE => changes.push(Change::Reopen { id: row.id, thing: thing.clone(), why: "It came back.".into() }),
            Some(row) if row.state == FIXED && did_not_hold(row) => {
                changes.push(Change::Reopen { id: row.id, thing: thing.clone(), why: "The fix did not hold.".into() })
            }
            Some(row) => changes.push(Change::Seen { id: row.id, thing: thing.clone() }),
        }
    }
    for (print, row) in stored {
        if present.contains(print) || !(row.state == OPEN || row.state == FIXED) {
            continue;
        }
        if covered.contains(&row.kind) {
            changes.push(Change::Gone { id: row.id });
        }
    }
    changes.sort_by_key(|c| match c {
        Change::Gone { id } => *id,
        _ => 0,
    });
    changes
}

fn did_not_hold(row: &Stored) -> bool {
    row.fix_move.as_deref().is_some_and(|m| HELD_FIXES.contains(&m)) && row.closed_for.is_some_and(|s| s >= HOLD_SECONDS)
}

/// Settling needs a reason, and is refused for an alarm: an alarm is the member's to see.
pub fn settle_problem(hint: &str, why: &str) -> Option<&'static str> {
    if why.trim().is_empty() {
        return Some("A settled finding needs its reason.");
    }
    if hint == ALARM {
        return Some("An alarm is never settled by the companion; the member sees it.");
    }
    None
}

// ---------- the database ----------

static MIGRATED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The companion's database, with the Sentinel's tables in place (migrated once per run).
pub fn open(memory: &Path) -> Result<Conn, String> {
    let mut conn = crate::memdb::open_for(memory).map_err(|_| "The Sentinel keeps its book in your memory database. Set it up in Settings > Database.".to_string())?;
    let key = memory.to_string_lossy().into_owned();
    let done = MIGRATED.lock().unwrap().as_ref().is_some_and(|set| set.contains(&key));
    if !done {
        crate::memdb::migrate(&mut conn)?;
        MIGRATED.lock().unwrap().get_or_insert_with(HashSet::new).insert(key);
    }
    Ok(conn)
}

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("The Sentinel's book: {what} did not work.")
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// What one look changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Collected {
    pub seen: u32,
    pub changed: u32,
    /// Findings that opened (new or again) in this look.
    pub opened: Vec<u64>,
    pub baseline: u32,
}

/// Applies one look to the book.
pub fn collect(conn: &mut Conn, found: &[Thing], covered: &HashSet<String>, full: bool) -> Result<Collected, String> {
    let baseline_taken = conn
        .query_first::<u64, _>("SELECT COUNT(*) FROM sentinel_known WHERE accepted_by = 'baseline'")
        .map_err(sql("reading the starting picture"))?
        .unwrap_or(0)
        > 0;
    let known: HashSet<String> = conn.query("SELECT fingerprint FROM sentinel_known").map_err(sql("reading what is known"))?.into_iter().collect();
    let rows: Vec<(u64, String, String, String, Option<String>, Option<i64>)> = conn
        .query("SELECT id, fingerprint, kind, state, fix_move, TIMESTAMPDIFF(SECOND, closed_at, NOW()) FROM sentinel_findings")
        .map_err(sql("reading the findings"))?;
    let mut stored = HashMap::new();
    for (id, print, kind, state, fix_move, closed_for) in rows {
        stored.insert(print, Stored { id, kind, state, fix_move, closed_for });
    }
    let mut out = Collected { seen: found.len() as u32, ..Collected::default() };
    for change in plan(found, covered, &known, &stored, baseline_taken, full) {
        match change {
            Change::Baseline { print, kind, title } => {
                conn.exec_drop(
                    "INSERT IGNORE INTO sentinel_known (fingerprint, kind, title, accepted_by) VALUES (?, ?, ?, 'baseline')",
                    (print, kind, cut(&title, 200)),
                )
                .map_err(sql("taking the starting picture"))?;
                out.baseline += 1;
            }
            Change::Open { print, thing } => {
                let (fix_move, fix_arg) = split_fix(&thing);
                conn.exec_drop(
                    "INSERT INTO sentinel_findings (fingerprint, kind, hint, title, detail, fix_move, fix_arg) VALUES (?, ?, ?, ?, ?, ?, ?)",
                    (print, &thing.kind, thing.hint, cut(&thing.title, 200), cut(&thing.detail, 1000), fix_move, fix_arg),
                )
                .map_err(sql("writing a finding"))?;
                out.opened.push(conn.last_insert_id());
                out.changed += 1;
            }
            Change::Seen { id, thing } => {
                let (fix_move, fix_arg) = split_fix(&thing);
                conn.exec_drop(
                    "UPDATE sentinel_findings SET last_seen = NOW(), hint = ?, title = ?, detail = ?, fix_move = ?, fix_arg = ? WHERE id = ?",
                    (thing.hint, cut(&thing.title, 200), cut(&thing.detail, 1000), fix_move, fix_arg, id),
                )
                .map_err(sql("refreshing a finding"))?;
            }
            Change::Reopen { id, thing, why } => {
                // the fix carries today's real argument (a renewed task's path)
                let (fix_move, fix_arg) = split_fix(&thing);
                conn.exec_drop(
                    "UPDATE sentinel_findings SET state = 'open', state_why = ?, hint = ?, detail = ?, fix_move = ?, fix_arg = ?, last_seen = NOW(), closed_at = NULL WHERE id = ?",
                    (why, thing.hint, cut(&thing.detail, 1000), fix_move, fix_arg, id),
                )
                .map_err(sql("opening a finding again"))?;
                out.opened.push(id);
                out.changed += 1;
            }
            Change::Gone { id } => {
                conn.exec_drop("UPDATE sentinel_findings SET state = 'gone', closed_at = NOW() WHERE id = ?", (id,)).map_err(sql("closing a finding"))?;
                out.changed += 1;
            }
            Change::Rekey { old, new } => {
                conn.exec_drop(
                    "INSERT IGNORE INTO sentinel_known (fingerprint, kind, title, accepted_by) SELECT ?, kind, title, accepted_by FROM sentinel_known WHERE fingerprint = ?",
                    (new, old),
                )
                .map_err(sql("carrying a known task to its steady mark"))?;
            }
            Change::Remark { id, print } => {
                conn.exec_drop("UPDATE sentinel_findings SET fingerprint = ? WHERE id = ?", (print, id)).map_err(sql("moving a finding to its steady mark"))?;
            }
        }
    }
    Ok(out)
}

fn split_fix(thing: &Thing) -> (Option<String>, Option<String>) {
    match &thing.fix {
        Some((mv, arg)) => (Some(mv.clone()), arg.clone()),
        None => (None, None),
    }
}

/// A finding as the page and the officer see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: u64,
    pub kind: String,
    pub hint: String,
    pub title: String,
    pub detail: String,
    pub fix_move: Option<String>,
    pub fix_arg: Option<String>,
    pub state: String,
    pub state_why: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
}

const FINDING: &str = "id, kind, hint, title, detail, fix_move, fix_arg, state, state_why, \
    DATE_FORMAT(first_seen, '%Y-%m-%d %H:%i'), DATE_FORMAT(last_seen, '%Y-%m-%d %H:%i')";

type FindingRow = (u64, String, String, String, String, Option<String>, Option<String>, String, Option<String>, String, String);

fn finding_of(row: FindingRow) -> Finding {
    let (id, kind, hint, title, detail, fix_move, fix_arg, state, state_why, first_seen, last_seen) = row;
    Finding { id, kind, hint, title, detail, fix_move, fix_arg, state, state_why, first_seen, last_seen }
}

/// The findings in these states, newest first (at most `limit`).
pub fn findings(conn: &mut Conn, states: &[&str], limit: u32) -> Result<Vec<Finding>, String> {
    let wanted: Vec<&str> = states.iter().copied().filter(|s| [OPEN, SETTLED, ACCEPTED, FIXED, GONE].contains(s)).collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let list = wanted.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ");
    let rows: Vec<FindingRow> = conn
        .query(format!("SELECT {FINDING} FROM sentinel_findings WHERE state IN ({list}) ORDER BY id DESC LIMIT {}", limit.min(500)))
        .map_err(sql("reading the findings"))?;
    Ok(rows.into_iter().map(finding_of).collect())
}

pub fn finding(conn: &mut Conn, id: u64) -> Result<Option<Finding>, String> {
    let row: Option<FindingRow> = conn.exec_first(format!("SELECT {FINDING} FROM sentinel_findings WHERE id = ?"), (id,)).map_err(sql("reading a finding"))?;
    Ok(row.map(finding_of))
}

/// The companion's judgment that an open finding is harmless, with its reason.
pub fn settle(conn: &mut Conn, id: u64, why: &str) -> Result<(), String> {
    let found = finding(conn, id)?.ok_or("There is no such finding.")?;
    if let Some(problem) = settle_problem(&found.hint, why) {
        return Err(problem.into());
    }
    if found.state != OPEN {
        return Err("Only an open finding can be settled.".into());
    }
    conn.exec_drop("UPDATE sentinel_findings SET state = 'settled', state_why = ? WHERE id = ? AND state = 'open'", (cut(why.trim(), 400), id))
        .map_err(sql("settling a finding"))
}

/// The member pressed a button: a click row, fresh for ten minutes, spent once.
pub fn click(conn: &mut Conn, id: u64, word: &str) -> Result<(), String> {
    if !["fix", "accept", "undo"].contains(&word) {
        return Err("That is not a button.".into());
    }
    conn.exec_drop("INSERT INTO sentinel_clicks (finding_id, word) VALUES (?, ?)", (id, word)).map_err(sql("writing the click"))
}

/// Spends the newest fresh click for this finding and word; false when there is none.
pub fn use_click(conn: &mut Conn, id: u64, word: &str) -> Result<bool, String> {
    let newest: Option<u64> = conn
        .exec_first(
            format!(
                "SELECT id FROM sentinel_clicks WHERE finding_id = ? AND word = ? AND used_at IS NULL \
                 AND clicked_at >= NOW() - INTERVAL {CLICK_MINUTES} MINUTE ORDER BY id DESC LIMIT 1"
            ),
            (id, word),
        )
        .map_err(sql("reading the click"))?;
    let Some(click) = newest else { return Ok(false) };
    // spent once: only one caller can turn used_at from NULL
    conn.exec_drop("UPDATE sentinel_clicks SET used_at = NOW() WHERE id = ? AND used_at IS NULL", (click,)).map_err(sql("spending the click"))?;
    Ok(conn.affected_rows() == 1)
}

/// The member's word that a finding is normal: needs a spent click, and the thing becomes known.
pub fn accept(conn: &mut Conn, id: u64) -> Result<(), String> {
    let found = finding(conn, id)?.ok_or("There is no such finding.")?;
    if !use_click(conn, id, "accept")? {
        return Err("This needs your own click in the Sentinel.".into());
    }
    let print: Option<String> = conn.exec_first("SELECT fingerprint FROM sentinel_findings WHERE id = ?", (id,)).map_err(sql("reading a finding"))?;
    let print = print.ok_or("There is no such finding.")?;
    conn.exec_drop(
        "INSERT INTO sentinel_known (fingerprint, kind, title, accepted_by) VALUES (?, ?, ?, 'member') \
         ON DUPLICATE KEY UPDATE accepted_by = 'member', accepted_at = NOW()",
        (print, &found.kind, cut(&found.title, 200)),
    )
    .map_err(sql("writing what is known"))?;
    conn.exec_drop("UPDATE sentinel_findings SET state = 'accepted', state_why = 'You said this is normal.' WHERE id = ?", (id,))
        .map_err(sql("accepting a finding"))
}

/// A move mended it.
pub fn mark_fixed(conn: &mut Conn, id: u64, how: &str) -> Result<(), String> {
    conn.exec_drop(
        "UPDATE sentinel_findings SET state = 'fixed', state_why = ?, closed_at = NOW() WHERE id = ? AND state = 'open'",
        (cut(how, 400), id),
    )
    .map_err(sql("marking a finding fixed"))
}

/// An undo puts a fixed finding back to open.
pub fn mark_undone(conn: &mut Conn, id: u64) -> Result<(), String> {
    conn.exec_drop(
        "UPDATE sentinel_findings SET state = 'open', state_why = 'The fix was undone.', closed_at = NULL WHERE id = ? AND state = 'fixed'",
        (id,),
    )
    .map_err(sql("undoing a fix"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thing(kind: &str, part: &str, inventory: bool) -> Thing {
        Thing {
            kind: kind.into(),
            parts: vec![part.into()],
            hint: if inventory { LOOK } else { ALARM },
            title: format!("{kind} {part}"),
            detail: String::new(),
            fix: None,
            inventory,
        }
    }

    fn covered(kinds: &[&str]) -> HashSet<String> {
        kinds.iter().map(|k| k.to_string()).collect()
    }

    fn row(id: u64, kind: &str, state: &str) -> Stored {
        Stored { id, kind: kind.into(), state: state.into(), fix_move: None, closed_for: None }
    }

    const SOFT: &str = "\\SoftLanding\\S-1-5-21-1234567890-1234567890-1234567890-1002\\";

    fn task(name: &str, action: &str) -> Thing {
        Thing { kind: "task".into(), parts: vec![format!("{SOFT}{name}"), action.into()], hint: LOOK, title: format!("Scheduled task {SOFT}{name}"), detail: String::new(), fix: Some(("disable-task".into(), Some(format!("{SOFT}{name}")))), inventory: true }
    }

    #[test]
    fn a_renewed_softlanding_task_keeps_its_mark_and_its_action_stays_whole() {
        let today = task("ActionCenterTriggerTask-128000000001627409-render-{8e2c3a5f-1111-2222-3333-444455556666}", "{AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}");
        let tomorrow = task("ActionCenterTriggerTask-128000000009999999-render-{43963bad-aaaa-bbbb-cccc-ddddeeeeffff}", "{AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}");
        assert_eq!(today.fingerprint(), tomorrow.fingerprint());
        assert_eq!(stable_task(&today.parts[0]), format!("{SOFT}ActionCenterTriggerTask-#-render"));
        assert!(stable_task(SOFT).contains("1234567890-1002"), "an account SID's ten-digit parts stay");
        let other_action = task("ActionCenterTriggerTask-128000000001627409-render-{8e2c3a5f-1111-2222-3333-444455556666}", "{00000000-9F8A-4C20-A1B5-0E3C4F3C9A11}");
        assert_ne!(today.fingerprint(), other_action.fingerprint(), "the action is never stripped");
        assert!(tomorrow.fix.as_ref().unwrap().1.as_deref().unwrap().ends_with("{43963bad-aaaa-bbbb-cccc-ddddeeeeffff}"), "the fix keeps today's real path");
        assert!(today.legacy_print().is_some());
        assert_eq!(task("Kettle Brewer", "kettle.exe").legacy_print(), None, "a plain name keeps its old mark");
        assert_eq!(thing("port", "80", true).legacy_print(), None);
    }

    #[test]
    fn a_task_known_or_open_under_its_old_mark_moves_to_the_steady_one() {
        let t = task("SoftLandingDeferralTask-{724494fb-1111-2222-3333-444455556666}", "{AF28889E-9F8A-4C20-A1B5-0E3C4F3C9A11}");
        let (old, new) = (t.legacy_print().unwrap(), t.fingerprint());
        // known (the starting picture took it): carried over, nothing raised
        let known = HashSet::from([old.clone()]);
        let changes = plan(std::slice::from_ref(&t), &covered(&["task"]), &known, &HashMap::new(), true, true);
        assert_eq!(changes, vec![Change::Rekey { old: old.clone(), new: new.clone() }]);
        // open under the old mark: moved, then seen; never gone, never opened twice
        let stored = HashMap::from([(old.clone(), row(9, "task", OPEN))]);
        let changes = plan(std::slice::from_ref(&t), &covered(&["task"]), &HashSet::new(), &stored, true, true);
        assert_eq!(changes, vec![Change::Remark { id: 9, print: new.clone() }, Change::Seen { id: 9, thing: t.clone() }]);
        // already on the new mark: nothing to carry
        let known = HashSet::from([old, new]);
        assert!(plan(std::slice::from_ref(&t), &covered(&["task"]), &known, &HashMap::new(), true, true).is_empty());
    }

    #[test]
    fn a_fingerprint_is_forty_hex_and_ignores_case_and_spaces() {
        let a = print_of("port", &["3306", "every", "mysqld.exe"]);
        assert_eq!(a.len(), 40);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, print_of("PORT", &[" 3306", "Every ", "MYSQLD.EXE"]));
        assert_ne!(a, print_of("port", &["3307", "every", "mysqld.exe"]));
        // the parts are separated, so "ab|c" and "a|bc" differ
        assert_ne!(print_of("k", &["ab", "c"]), print_of("k", &["a", "bc"]));
    }

    #[test]
    fn the_first_full_look_is_the_starting_picture_and_then_only_new_things_open() {
        let first = [thing("port", "80", true), thing("task", "backup", true)];
        let changes = plan(&first, &covered(&["port", "task"]), &HashSet::new(), &HashMap::new(), false, true);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| matches!(c, Change::Baseline { .. })));

        let known: HashSet<String> = first.iter().map(Thing::fingerprint).collect();
        let second = [thing("port", "80", true), thing("task", "backup", true), thing("task", "stranger", true)];
        let changes = plan(&second, &covered(&["port", "task"]), &known, &HashMap::new(), true, true);
        assert_eq!(changes.len(), 1);
        assert!(matches!(&changes[0], Change::Open { thing, .. } if thing.parts == vec!["stranger".to_string()]));
    }

    #[test]
    fn a_quick_look_never_takes_the_starting_picture() {
        let found = [thing("port", "80", true)];
        assert!(plan(&found, &covered(&["port"]), &HashSet::new(), &HashMap::new(), false, false).is_empty());
    }

    #[test]
    fn a_state_thing_opens_even_at_the_first_look() {
        let found = [thing("firewall", "public", false), thing("port", "80", true)];
        let changes = plan(&found, &covered(&["firewall", "port"]), &HashSet::new(), &HashMap::new(), false, true);
        assert!(changes.iter().any(|c| matches!(c, Change::Open { thing, .. } if thing.kind == "firewall")));
        assert!(changes.iter().any(|c| matches!(c, Change::Baseline { kind, .. } if kind == "port")));
    }

    #[test]
    fn an_open_thing_missing_from_a_look_that_read_its_kind_is_gone() {
        let print = thing("firewall", "public", false).fingerprint();
        let stored = HashMap::from([(print, row(7, "firewall", OPEN))]);
        let changes = plan(&[], &covered(&["firewall"]), &HashSet::new(), &stored, true, false);
        assert_eq!(changes, vec![Change::Gone { id: 7 }]);
        // a quick look that did not read firewalls never calls it gone
        assert!(plan(&[], &covered(&["protection"]), &HashSet::new(), &stored, true, false).is_empty());
    }

    #[test]
    fn settled_and_accepted_outlive_the_thing_going_away_and_coming_back() {
        let t = thing("task", "nightly", true);
        let stored = HashMap::from([(t.fingerprint(), row(3, "task", SETTLED))]);
        assert!(plan(&[], &covered(&["task"]), &HashSet::new(), &stored, true, true).is_empty());
        let changes = plan(std::slice::from_ref(&t), &covered(&["task"]), &HashSet::new(), &stored, true, true);
        assert!(matches!(&changes[0], Change::Seen { id: 3, .. }));
    }

    #[test]
    fn a_gone_thing_that_comes_back_opens_again() {
        let t = thing("startup", "updater", true);
        let stored = HashMap::from([(t.fingerprint(), row(4, "startup", GONE))]);
        let changes = plan(std::slice::from_ref(&t), &covered(&["startup"]), &HashSet::new(), &stored, true, true);
        assert!(matches!(&changes[0], Change::Reopen { id: 4, why, .. } if why == "It came back."));
    }

    #[test]
    fn a_fix_that_did_not_hold_opens_again_after_ten_minutes() {
        let t = thing("firewall", "domain", false);
        let mut fixed = row(5, "firewall", FIXED);
        fixed.fix_move = Some("firewall-on".into());
        fixed.closed_for = Some(120);
        let mut stored = HashMap::from([(t.fingerprint(), fixed.clone())]);
        let changes = plan(std::slice::from_ref(&t), &covered(&["firewall"]), &HashSet::new(), &stored, true, false);
        assert!(matches!(&changes[0], Change::Seen { id: 5, .. }), "too soon to say");
        fixed.closed_for = Some(HOLD_SECONDS);
        stored.insert(t.fingerprint(), fixed);
        let changes = plan(std::slice::from_ref(&t), &covered(&["firewall"]), &HashSet::new(), &stored, true, false);
        assert!(matches!(&changes[0], Change::Reopen { id: 5, why, .. } if why == "The fix did not hold."));
    }

    #[test]
    fn a_member_fix_that_is_not_a_state_is_never_reopened_by_time() {
        let t = thing("port", "8080", true);
        let mut fixed = row(6, "port", FIXED);
        fixed.fix_move = Some("block-port".into());
        fixed.closed_for = Some(HOLD_SECONDS * 10);
        let stored = HashMap::from([(t.fingerprint(), fixed)]);
        let changes = plan(std::slice::from_ref(&t), &covered(&["port"]), &HashSet::new(), &stored, true, true);
        assert!(matches!(&changes[0], Change::Seen { id: 6, .. }));
    }

    #[test]
    fn the_same_thing_twice_in_one_look_counts_once() {
        let found = [thing("task", "x", false), thing("task", "x", false)];
        assert_eq!(plan(&found, &covered(&["task"]), &HashSet::new(), &HashMap::new(), true, true).len(), 1);
    }

    #[test]
    fn a_port_is_known_by_number_scope_and_program_and_a_dynamic_one_by_program() {
        assert_eq!(port_parts(3306, "local", "mariadbd.exe"), vec!["3306", "local", "mariadbd.exe"]);
        let a = print_of("port", &port_parts(49200, "every", "app.exe"));
        let b = print_of("port", &port_parts(61000, "every", "app.exe"));
        assert_eq!(a, b, "a restart that moves a dynamic port is the same door");
        assert_eq!(scope_of("0.0.0.0"), "every");
        assert_eq!(scope_of("::"), "every");
        assert_eq!(scope_of("127.0.0.1"), "local");
        assert_eq!(scope_of("::1"), "local");
        assert_eq!(scope_of("192.168.1.5"), "one");
        assert_eq!(wider("local", "every"), "every");
        assert_eq!(wider("one", "local"), "one");
    }

    #[test]
    fn settling_needs_a_reason_and_never_an_alarm() {
        assert!(settle_problem(LOOK, "  ").is_some());
        assert!(settle_problem(ALARM, "it is mine").is_some());
        assert!(settle_problem(LOOK, "the backup tool the member installed").is_none());
    }
}
