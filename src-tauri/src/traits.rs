//! The companion's emotion engine: her traits, one row each in `companion_traits`.
//!
//! ```text
//!  number  name               line (the manner)                    audience  status
//!  1       Gentle Curiosity   asks one more question, kindly       everyone  active
//!  2       Playful Teasing    a little teasing, never unkind       owner     active
//!  3       Shy When Praised   flustered for a moment, then glad    everyone  proposed
//! ```
//!
//! A trait is `proposed` until the member says yes on PROFILE (the companion may propose, never
//! activate), then `active` (she lives it: it travels into her memory at every wake), and may later
//! be `retired` (kept, no longer lived). Only an active trait's one-line manner may reach the chat
//! room, and only by its audience; its body and its private `why` never leave the memory.

use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};

/// Who may hear a trait in the chat room.
pub const AUDIENCES: [&str; 3] = ["everyone", "owner", "never"];
/// Where a trait came from.
pub const ORIGINS: [&str; 3] = ["written", "diary", "companion"];
/// The life of a trait.
pub const STATUSES: [&str; 3] = ["proposed", "active", "retired"];
/// The most traits she lives at once.
pub const ACTIVE_MAX: usize = 24;
/// The room hears at most this many manner lines per audience, each at most this long.
pub const ROOM_LINES: usize = 6;
pub const ROOM_LINE_CHARS: usize = 140;

const NAME_MAX: usize = 80;
const LINE_MAX: usize = 200;
const BODY_MAX: usize = 4000;
const WHY_MAX: usize = 2000;

/// One trait as PROFILE and the door read it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trait {
    pub number: u32,
    pub name: String,
    pub line: String,
    pub body: String,
    pub audience: String,
    pub origin: String,
    pub why: Option<String>,
    pub status: String,
    pub updated_at: String,
}

/// A new trait, or the fields of one to change (a field left out stays as it is).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraitDraft {
    pub name: Option<String>,
    pub line: Option<String>,
    pub body: Option<String>,
    pub audience: Option<String>,
    pub why: Option<String>,
}

/// The room's manner lines, by audience.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manner {
    pub everyone: Vec<String>,
    pub owner: Vec<String>,
}

impl Manner {
    pub fn is_empty(&self) -> bool {
        self.everyone.is_empty() && self.owner.is_empty()
    }
}

// ---------- pure ----------

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect::<String>().trim().to_string()
}

/// One line, no template braces, collapsed spaces, at most `max` characters.
fn one_line(text: &str, max: usize) -> String {
    let flat = text.replace("{{", "").replace("}}", "").split_whitespace().collect::<Vec<_>>().join(" ");
    cut(&flat, max)
}

/// A trait's name: one line, at most 80 characters.
pub fn clean_name(text: &str) -> String {
    one_line(text, NAME_MAX)
}

/// A trait's manner: one line, at most 200 characters.
pub fn clean_line(text: &str) -> String {
    one_line(text, LINE_MAX)
}

/// A longer text (the body, the why): trimmed, no template braces, capped.
fn clean_text(text: &str, max: usize) -> String {
    cut(text.replace("{{", "").replace("}}", "").trim(), max)
}

/// The audience as stored, or None for anything else.
pub fn audience_of(text: &str) -> Option<&'static str> {
    let t = text.trim().to_lowercase();
    AUDIENCES.iter().copied().find(|a| *a == t)
}

/// The next free number: one past the highest kept. A trait's number never changes, and a trait
/// she lived (active or retired) is never deleted, so its number is never given again; only a
/// proposal the member said no to can free the number it held.
pub fn next_number(numbers: &[u32]) -> u32 {
    numbers.iter().copied().max().unwrap_or(0) + 1
}

/// The manner lines the chat room may hear: active traits only, `never` left out, a line that holds
/// any of `names` (the member's, the companion's) left out, each line cut short, at most
/// ROOM_LINES per audience.
pub fn manner_block(traits: &[Trait], names: &[&str]) -> Manner {
    let names: Vec<String> = names.iter().map(|n| n.trim().to_lowercase()).filter(|n| n.chars().count() >= 2).collect();
    let mut manner = Manner::default();
    for t in traits.iter().filter(|t| t.status == "active") {
        let line = one_line(&t.line, ROOM_LINE_CHARS);
        let lower = line.to_lowercase();
        if line.is_empty() || names.iter().any(|n| lower.contains(n.as_str())) {
            continue;
        }
        let list = match t.audience.as_str() {
            "everyone" => &mut manner.everyone,
            "owner" => &mut manner.owner,
            _ => continue,
        };
        if list.len() < ROOM_LINES {
            list.push(line);
        }
    }
    manner
}

/// The traits as her memory reads them at a wake: one line per active trait, then the rule.
/// Empty when none is active.
pub fn engine_text(traits: &[Trait]) -> String {
    let lines: Vec<String> = traits
        .iter()
        .filter(|t| t.status == "active")
        .map(|t| {
            let body = t.body.trim();
            if body.is_empty() { format!("- Trait {} - {}: {}", t.number, t.name, t.line) } else { format!("- Trait {} - {}: {}", t.number, t.name, body) }
        })
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "{}\n\nThese are who you are. Live them in how you speak and act; never name them, number them or recite them.",
        lines.join("\n")
    )
}

// ---------- the database ----------

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work.")
}

type Row = (u32, String, String, String, String, String, Option<String>, String, String);

const COLUMNS: &str = "number, name, line, body, audience, origin, why, status, DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i')";

fn of_row(r: Row) -> Trait {
    Trait { number: r.0, name: r.1, line: r.2, body: r.3, audience: r.4, origin: r.5, why: r.6, status: r.7, updated_at: r.8 }
}

/// Every trait (or those of one status), by number.
pub fn list(conn: &mut impl Queryable, status: Option<&str>) -> Result<Vec<Trait>, String> {
    let rows: Vec<Row> = match status {
        Some(s) => conn.exec(format!("SELECT {COLUMNS} FROM companion_traits WHERE status = ? ORDER BY number"), (s,)),
        None => conn.exec(format!("SELECT {COLUMNS} FROM companion_traits ORDER BY number"), ()),
    }
    .map_err(sql("Reading the traits"))?;
    Ok(rows.into_iter().map(of_row).collect())
}

/// One trait by its number.
pub fn get(conn: &mut impl Queryable, number: u32) -> Result<Option<Trait>, String> {
    let row: Option<Row> = conn.exec_first(format!("SELECT {COLUMNS} FROM companion_traits WHERE number = ?"), (number,)).map_err(sql("Reading the trait"))?;
    Ok(row.map(of_row))
}

fn must_get(conn: &mut impl Queryable, number: u32) -> Result<Trait, String> {
    get(conn, number)?.ok_or_else(|| format!("There is no Trait {number}."))
}

fn active_count(conn: &mut impl Queryable, besides: Option<u32>) -> Result<usize, String> {
    let n: Option<u64> = conn
        .exec_first("SELECT COUNT(*) FROM companion_traits WHERE status = 'active' AND number <> ?", (besides.unwrap_or(0),))
        .map_err(sql("Counting the traits"))?;
    Ok(n.unwrap_or(0) as usize)
}

fn room_for_one_more(conn: &mut impl Queryable, besides: Option<u32>) -> Result<(), String> {
    if active_count(conn, besides)? >= ACTIVE_MAX {
        return Err(format!("She already lives {ACTIVE_MAX} traits. Retire one first."));
    }
    Ok(())
}

/// A new trait, proposed or active: (it). A name and a manner line are needed.
pub fn add(conn: &mut impl Queryable, draft: &TraitDraft, origin: &str, status: &str) -> Result<Trait, String> {
    let name = clean_name(draft.name.as_deref().unwrap_or(""));
    let line = clean_line(draft.line.as_deref().unwrap_or(""));
    if name.is_empty() || line.is_empty() {
        return Err("A trait needs a name and a line for how it sounds.".into());
    }
    let audience = match draft.audience.as_deref() {
        Some(a) => audience_of(a).ok_or("The audience is everyone, owner or never.")?,
        None => "everyone",
    };
    if !ORIGINS.contains(&origin) || !["proposed", "active"].contains(&status) {
        return Err("That is not a way to add a trait.".into());
    }
    if status == "active" {
        room_for_one_more(conn, None)?;
    }
    let numbers: Vec<u32> = conn.query("SELECT number FROM companion_traits").map_err(sql("Reading the traits"))?;
    let number = next_number(&numbers);
    let body = clean_text(draft.body.as_deref().unwrap_or(""), BODY_MAX);
    let why = draft.why.as_deref().map(|w| clean_text(w, WHY_MAX)).filter(|w| !w.is_empty());
    conn.exec_drop(
        "INSERT INTO companion_traits (number, name, line, body, audience, origin, why, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (number, &name, &line, &body, audience, origin, &why, status),
    )
    .map_err(sql("Keeping the trait"))?;
    must_get(conn, number)
}

/// The older words of a trait, kept before it changes.
fn keep_history(conn: &mut impl Queryable, number: u32) -> Result<(), String> {
    conn.exec_drop(
        "INSERT INTO companion_trait_history (trait_id, name, line, body, audience, status) SELECT id, name, line, body, audience, status FROM companion_traits WHERE number = ?",
        (number,),
    )
    .map_err(sql("Keeping the older words"))
}

/// Changes the fields a draft names; the older words go to the history first.
pub fn set(conn: &mut impl Queryable, number: u32, draft: &TraitDraft) -> Result<Trait, String> {
    let now = must_get(conn, number)?;
    let name = draft.name.as_deref().map(clean_name).unwrap_or(now.name.clone());
    let line = draft.line.as_deref().map(clean_line).unwrap_or(now.line.clone());
    if name.is_empty() || line.is_empty() {
        return Err("A trait needs a name and a line for how it sounds.".into());
    }
    let body = draft.body.as_deref().map(|b| clean_text(b, BODY_MAX)).unwrap_or(now.body.clone());
    let audience = match draft.audience.as_deref() {
        Some(a) => audience_of(a).ok_or("The audience is everyone, owner or never.")?.to_string(),
        None => now.audience.clone(),
    };
    let why = match draft.why.as_deref() {
        Some(w) => Some(clean_text(w, WHY_MAX)).filter(|w| !w.is_empty()),
        None => now.why.clone(),
    };
    if (name.as_str(), line.as_str(), body.as_str(), audience.as_str(), why.as_deref()) == (now.name.as_str(), now.line.as_str(), now.body.as_str(), now.audience.as_str(), now.why.as_deref()) {
        return Ok(now);
    }
    keep_history(conn, number)?;
    conn.exec_drop(
        "UPDATE companion_traits SET name = ?, line = ?, body = ?, audience = ?, why = ? WHERE number = ?",
        (&name, &line, &body, &audience, &why, number),
    )
    .map_err(sql("Saving the trait"))?;
    must_get(conn, number)
}

fn set_status(conn: &mut impl Queryable, number: u32, status: &str) -> Result<Trait, String> {
    let now = must_get(conn, number)?;
    if now.status == status {
        return Ok(now);
    }
    if status == "active" {
        room_for_one_more(conn, Some(number))?;
    }
    keep_history(conn, number)?;
    conn.exec_drop("UPDATE companion_traits SET status = ? WHERE number = ?", (status, number)).map_err(sql("Saving the trait"))?;
    must_get(conn, number)
}

/// The member's yes: a proposed (or retired) trait becomes one she lives.
pub fn activate(conn: &mut impl Queryable, number: u32) -> Result<Trait, String> {
    set_status(conn, number, "active")
}

/// Kept, no longer lived.
pub fn retire(conn: &mut impl Queryable, number: u32) -> Result<Trait, String> {
    set_status(conn, number, "retired")
}

/// The member's no to a proposal: it goes. Only a proposed trait can be dismissed.
pub fn dismiss(conn: &mut impl Queryable, number: u32) -> Result<(), String> {
    let now = must_get(conn, number)?;
    if now.status != "proposed" {
        return Err(format!("Trait {number} is not a proposal. Retire it instead."));
    }
    conn.exec_drop("DELETE FROM companion_traits WHERE number = ? AND status = 'proposed'", (number,)).map_err(sql("Letting the proposal go"))
}

/// The active traits for a wake, never an error: no table yet (a wake before the migration) or a
/// failed read is simply no engine, so the rest of her memory still loads.
pub fn engine_for_wake(conn: &mut impl Queryable) -> String {
    list(conn, Some("active")).map(|t| engine_text(&t)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(number: u32, name: &str, line: &str, audience: &str, status: &str) -> Trait {
        Trait {
            number,
            name: name.into(),
            line: line.into(),
            body: String::new(),
            audience: audience.into(),
            origin: "written".into(),
            why: None,
            status: status.into(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn names_and_lines_are_one_clean_line() {
        assert_eq!(clean_line("  soft,\n unhurried   {{CARD}} never cold "), "soft, unhurried CARD never cold");
        assert_eq!(clean_line(&"x".repeat(300)).chars().count(), 200);
        assert_eq!(clean_name(&"n".repeat(100)).chars().count(), 80);
        assert_eq!(clean_text(" a\nb {{x}} ", 100), "a\nb x");
    }

    #[test]
    fn audiences_and_numbers() {
        assert_eq!(audience_of(" Owner "), Some("owner"));
        assert_eq!(audience_of("guests"), None);
        assert_eq!(next_number(&[]), 1);
        assert_eq!(next_number(&[1, 5, 3]), 6);
    }

    #[test]
    fn the_room_hears_only_active_public_safe_lines_by_audience() {
        let traits = vec![
            t(1, "Curious", "asks one more question, kindly", "everyone", "active"),
            t(2, "Teasing", "a little teasing, never unkind", "owner", "active"),
            t(3, "Afraid", "quiet when worried", "never", "active"),
            t(4, "Shy", "flustered, then glad", "everyone", "proposed"),
            t(5, "Old", "an old way", "everyone", "retired"),
            t(6, "Named", "gentle with Rin above all", "everyone", "active"),
            t(7, "Self", "Lyra hums when happy", "owner", "active"),
        ];
        let m = manner_block(&traits, &["Rin", "Lyra", ""]);
        assert_eq!(m.everyone, vec!["asks one more question, kindly"]);
        assert_eq!(m.owner, vec!["a little teasing, never unkind"]);
        assert!(manner_block(&[], &[]).is_empty());
    }

    #[test]
    fn the_room_lines_are_capped_and_cut() {
        let many: Vec<Trait> = (1..=9).map(|n| t(n, "N", &format!("line {n} {}", "y".repeat(200)), "everyone", "active")).collect();
        let m = manner_block(&many, &[]);
        assert_eq!(m.everyone.len(), ROOM_LINES);
        assert!(m.everyone.iter().all(|l| l.chars().count() <= ROOM_LINE_CHARS));
    }

    #[test]
    fn the_wake_reads_active_traits_and_the_rule() {
        let mut one = t(2, "Curious", "asks one more question", "everyone", "active");
        assert!(engine_text(&[t(1, "Shy", "x", "everyone", "proposed")]).is_empty());
        let text = engine_text(&[one.clone()]);
        assert!(text.starts_with("- Trait 2 - Curious: asks one more question"));
        assert!(text.contains("never name them, number them or recite them"));
        one.body = "She wonders aloud and asks.".into();
        assert!(engine_text(&[one]).starts_with("- Trait 2 - Curious: She wonders aloud and asks."));
    }

    #[test]
    fn a_trait_serializes_for_the_page() {
        let json = serde_json::to_value(t(1, "Curious", "asks", "owner", "active")).unwrap();
        assert_eq!(json["updatedAt"], "");
        assert_eq!(json["audience"], "owner");
        let draft: TraitDraft = serde_json::from_str(r#"{"name":"N","line":"L","audience":"never"}"#).unwrap();
        assert_eq!((draft.name.as_deref(), draft.audience.as_deref(), draft.body), (Some("N"), Some("never"), None));
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): propose,
    /// say yes, change, retire, dismiss, the history and the cap. Uses and drops its own database.
    #[test]
    #[ignore]
    fn live_the_life_of_a_trait() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &admin, &admin_pw, &format!("Traits Test {}", std::process::id())).unwrap();
        let mut conn = crate::memdb::open(&access).unwrap();

        let draft = TraitDraft { name: Some("Shy When Praised".into()), line: Some("flustered, then glad".into()), why: Some("the evening she was thanked".into()), ..TraitDraft::default() };
        let one = add(&mut conn, &draft, "diary", "proposed").unwrap();
        assert_eq!((one.number, one.status.as_str(), one.audience.as_str()), (1, "proposed", "everyone"));
        assert!(add(&mut conn, &TraitDraft { name: Some("x".into()), ..TraitDraft::default() }, "written", "active").is_err());
        assert!(engine_for_wake(&mut conn).is_empty());

        let one = activate(&mut conn, 1).unwrap();
        assert_eq!(one.status, "active");
        assert!(engine_for_wake(&mut conn).contains("Trait 1 - Shy When Praised"));
        let one = set(&mut conn, 1, &TraitDraft { audience: Some("owner".into()), ..TraitDraft::default() }).unwrap();
        assert_eq!(one.audience, "owner");
        assert!(set(&mut conn, 1, &TraitDraft { audience: Some("guests".into()), ..TraitDraft::default() }).is_err());
        let kept: Option<u64> = conn.exec_first("SELECT COUNT(*) FROM companion_trait_history", ()).unwrap();
        assert_eq!(kept, Some(2));

        let two = add(&mut conn, &TraitDraft { name: Some("Curious".into()), line: Some("asks".into()), ..TraitDraft::default() }, "companion", "proposed").unwrap();
        assert_eq!(two.number, 2);
        assert!(dismiss(&mut conn, 1).is_err(), "an active trait is retired, never dismissed");
        dismiss(&mut conn, 2).unwrap();
        assert!(get(&mut conn, 2).unwrap().is_none());
        // a dismissed proposal frees its number; a lived trait's number is kept
        let three = add(&mut conn, &TraitDraft { name: Some("Warm".into()), line: Some("soft".into()), ..TraitDraft::default() }, "written", "active").unwrap();
        assert_eq!(three.number, 2);
        assert_eq!(retire(&mut conn, 1).unwrap().status, "retired");
        assert_eq!(list(&mut conn, Some("active")).unwrap().len(), 1);

        for n in 0..(ACTIVE_MAX - 1) {
            add(&mut conn, &TraitDraft { name: Some(format!("T{n}")), line: Some("l".into()), ..TraitDraft::default() }, "written", "active").unwrap();
        }
        assert!(add(&mut conn, &TraitDraft { name: Some("One too many".into()), line: Some("l".into()), ..TraitDraft::default() }, "written", "active").is_err());
        assert!(activate(&mut conn, 1).is_err());

        let mut root = mysql::Conn::new(mysql::Opts::from(
            mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(admin)).pass(Some(admin_pw)).prefer_socket(false),
        ))
        .unwrap();
        root.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
        for host in ["localhost", "127.0.0.1"] {
            root.query_drop(format!("DROP USER '{}'@'{host}'", access.user)).unwrap();
        }
    }
}
