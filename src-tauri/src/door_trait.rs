//! The door's trait verbs (`castle-memory trait list | show | propose | set | retire`), kept apart
//! from door.rs: the companion reads her emotion engine and may propose a trait, which waits on
//! PROFILE for the member's yes. She never activates one herself.

use crate::door::Ask;
use crate::door_plan::number;
use crate::traits::{self, Trait, TraitDraft};
use mysql::Conn;

/// The trait verbs may run before Castle's background migration has made the table: make sure.
fn ready(conn: &mut Conn) -> Result<(), String> {
    crate::memdb::migrate(conn).map(|_| ())
}

fn trait_number(ask: &Ask) -> Result<u32, String> {
    number::<u32>(ask, "number")?.ok_or("--number is needed (the trait's number, like 3).".into())
}

fn draft(ask: &Ask) -> Result<TraitDraft, String> {
    Ok(TraitDraft {
        name: ask.get("name").map(str::to_string),
        line: ask.get("line").map(str::to_string),
        body: ask.text()?,
        audience: ask.get("audience").map(str::to_string),
        why: ask.get("why").map(str::to_string),
    })
}

/// "TRAIT 2 [active, everyone] Gentle Curiosity - asks one more question"
fn row_line(t: &Trait) -> String {
    format!("TRAIT {} [{}, {}] {} - {}", t.number, t.status, t.audience, t.name, t.line)
}

pub(crate) fn trait_list(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    ready(conn)?;
    let status = ask.get("status").map(str::trim).filter(|s| !s.is_empty());
    if let Some(s) = status {
        if !traits::STATUSES.contains(&s) {
            return Err("--status is proposed, active or retired.".into());
        }
    }
    let rows = traits::list(conn, status)?;
    if rows.is_empty() {
        return Ok("TRAITS none yet".into());
    }
    Ok(rows.iter().map(row_line).collect::<Vec<_>>().join("\n"))
}

pub(crate) fn trait_show(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    ready(conn)?;
    let n = trait_number(ask)?;
    let t = traits::get(conn, n)?.ok_or_else(|| format!("There is no Trait {n}."))?;
    let why = t.why.as_deref().map(|w| format!("\nWhy (private): {w}")).unwrap_or_default();
    Ok(format!("{}\nFrom: {} | changed {}\n\n{}{why}", row_line(&t), t.origin, t.updated_at, if t.body.is_empty() { "(no longer words yet)" } else { t.body.as_str() }))
}

/// The companion's own proposal: kept as `proposed`, never active.
pub(crate) fn trait_propose(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    ready(conn)?;
    let t = traits::add(conn, &draft(ask)?, "companion", "proposed")?;
    Ok(format!("TRAIT {} proposed: {} - it waits on PROFILE for your person's yes. Do not live it before then.", t.number, t.name))
}

pub(crate) fn trait_set(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    ready(conn)?;
    let t = traits::set(conn, trait_number(ask)?, &draft(ask)?)?;
    Ok(format!("{} (saved)", row_line(&t)))
}

pub(crate) fn trait_retire(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    ready(conn)?;
    let t = traits::retire(conn, trait_number(ask)?)?;
    Ok(format!("{} - kept, no longer lived", row_line(&t)))
}
