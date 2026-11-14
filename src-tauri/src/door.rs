//! The one door: how the companion reads and writes the memory database from its own terminal.
//!
//! ```text
//!  castle-memory diary add --date 2026-11-10 --title "The garden" --file entry.md
//!  castle-memory diary list [--month 2026-11] [--limit 20]      diary show --date 2026-11-10
//!  castle-memory diary rewrite --date 2026-11-10 --session 1 [--title "..."] --file entry.md
//!  castle-memory reminder add --title "Call Rin" [--due 2026-11-12] [--detail "..."]
//!  castle-memory reminder list | reminder done --id 3
//!  castle-memory project add|update --name "Portfolio site" [--status ..] [--summary ..] [--file notes.md]
//!  castle-memory project list | project show --name "Portfolio site"
//!  castle-memory link send --from main --to story --about "..." [--word "..."] [--no-wake]
//!  castle-memory link inbox --to main | link read --to main --note <file name> | link here --name main
//!  castle-memory boot                         the memory to read at the start of a session
//!  castle-memory memory list | memory show --path master-memory.md | memory save --path .. --file ..
//!  castle-memory session show [--section "Open threads"] | session set --section ".." --file .. | session clear --section ".."
//!  castle-memory skill list | skill show --plugin p --name n [--section key | --all] | skill sync
//!  castle-memory skill save --plugin p --name n --dir <folder> | skill remove --plugin p --name n
//!  castle-memory skill stub --plugin p --name n      a small SKILL.md on disk, the instructions loaded from here
//!  castle-memory skill full --plugin p --name n      back to the whole SKILL.md on disk
//!  castle-memory plan copy [--plan <file.md>] [--folder <dir>]   an approved plan into the project's rows
//!  castle-memory plan status | plan next | plan show [--id N]    the project folder's open plans
//!  castle-memory plan mark [--next | --item N] [--state done|carried|flagged] [--note ".."] [--id N]
//!  castle-memory trait list [--status active] | trait show --number 3   her emotion engine
//!  castle-memory trait propose --name ".." --line ".." [--audience everyone|owner|never] [--why ".."] [--file ..]
//!  castle-memory trait set --number 3 [--name ..] [--line ..] [--audience ..] [--file ..] | trait retire --number 3
//!  castle-memory export --to <folder>        the memory back out as markdown
//!  castle-memory dump --to backup/memory.sql | restore --from backup/memory.sql
//! ```
//!
//! `castle-memory` is Castle's own program started with `--memory` (the tools folder holds the
//! small launcher). It finds the database through the memory folder's .env, and the memory
//! folder by `--memory-folder <dir>`, else the CASTLE_MEMORY environment variable, else the
//! nearest folder upward from where it was started.
//!
//! The `plan` verbs work for a project folder: `--folder <dir>`, else the folder the door was
//! started in (so a door started in a project needs CASTLE_MEMORY or `--memory-folder`).
//! `plan copy` without `--plan` takes the newest `*.md` in `%USERPROFILE%\.claude\plans`.

use crate::memdb;
use crate::mirror;
use crate::door_plan::{next_line, plan_copy, plan_mark, plan_show, plan_status, project_folder};
use crate::door_trait::{trait_list, trait_propose, trait_retire, trait_set, trait_show};
use mysql::prelude::Queryable;
use mysql::{Conn, Row, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The argument that turns Castle's program into the door.
pub const FLAG: &str = "--memory";

/// The environment variable that names the memory folder, for a door started somewhere else
/// (a project folder).
pub const MEMORY_ENV: &str = "CASTLE_MEMORY";

/// The words after `castle-memory`: plain words in order, and `--name value` pairs
/// (`--no-wake` style flags carry "true").
#[derive(Debug, Default, PartialEq)]
pub struct Ask {
    pub words: Vec<String>,
    pub flags: BTreeMap<String, String>,
}

const BARE_FLAGS: [&str; 3] = ["no-wake", "all", "next"];

pub fn parse(args: &[String]) -> Result<Ask, String> {
    let mut ask = Ask::default();
    let mut i = 0;
    while i < args.len() {
        if let Some(name) = args[i].strip_prefix("--") {
            if BARE_FLAGS.contains(&name) {
                ask.flags.insert(name.to_string(), "true".into());
            } else {
                let value = args.get(i + 1).ok_or(format!("--{name} needs a value."))?;
                ask.flags.insert(name.to_string(), value.clone());
                i += 1;
            }
        } else {
            ask.words.push(args[i].clone());
        }
        i += 1;
    }
    Ok(ask)
}

impl Ask {
    fn need(&self, name: &str) -> Result<&str, String> {
        self.flags.get(name).map(String::as_str).filter(|v| !v.trim().is_empty()).ok_or(format!("--{name} is needed."))
    }
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }
    /// The long text of a verb: `--file <path>` (read as UTF-8) or `--body "..."`.
    pub(crate) fn text(&self) -> Result<Option<String>, String> {
        if let Some(file) = self.get("file") {
            let text = std::fs::read_to_string(file).map_err(|_| format!("The file {file} could not be read."))?;
            return Ok(Some(text.trim_start_matches('\u{feff}').to_string()));
        }
        Ok(self.get("body").map(str::to_string))
    }
}

/// "2026-11-10": four digits, dash, two, dash, two.
pub fn is_date(value: &str) -> bool {
    value.len() == 10 && value.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
}

fn date(value: &str) -> Result<&str, String> {
    if is_date(value) { Ok(value) } else { Err(format!("{value} is not a date like 2026-11-10.")) }
}

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work.")
}

/// The memory folder: `--memory-folder`, else CASTLE_MEMORY, else the nearest folder upward
/// from here whose .env names a memory database.
pub fn find_memory(ask: &Ask, from: &Path) -> Result<PathBuf, String> {
    find_memory_with(ask, std::env::var_os(MEMORY_ENV), from)
}

fn find_memory_with(ask: &Ask, named: Option<OsString>, from: &Path) -> Result<PathBuf, String> {
    if let Some(folder) = ask.get("memory-folder") {
        return Ok(PathBuf::from(folder));
    }
    if let Some(folder) = named.filter(|f| !f.is_empty()) {
        return Ok(PathBuf::from(folder));
    }
    let mut at = Some(from);
    while let Some(folder) = at {
        if std::fs::read_to_string(folder.join(".env")).is_ok_and(|t| memdb::access_from_env(&t).is_some()) {
            return Ok(folder.to_path_buf());
        }
        at = folder.parent();
    }
    Err(format!("No memory database was found from this folder. Run this inside your MemoryCore folder, or name it with --memory-folder <dir> or {MEMORY_ENV}."))
}

/// Runs one ask against an open database; the text to print. `memory` is the memory folder
/// (the skills' mirror lives there).
pub fn run(conn: &mut Conn, ask: &Ask, memory: &Path) -> Result<String, String> {
    let words: Vec<&str> = ask.words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["diary", "add"] => diary_add(conn, ask),
        ["diary", "rewrite"] => diary_rewrite(conn, ask),
        ["diary", "unseed"] => {
            conn.query_drop("DELETE FROM diary_entries WHERE source_file LIKE 'seed/%'").map_err(sql("Removing the seed entries"))?;
            Ok(format!("DIARY {} seed entries removed", conn.affected_rows()))
        }
        ["diary", "list"] => diary_list(conn, ask),
        ["diary", "show"] => diary_show(conn, ask),
        ["reminder", "add"] => reminder_add(conn, ask),
        ["reminder", "list"] => reminder_list(conn, ask),
        ["reminder", "done"] => reminder_done(conn, ask),
        ["project", "add"] | ["project", "update"] => project_save(conn, ask),
        ["project", "list"] => project_list(conn),
        ["project", "show"] => project_show(conn, ask),
        ["link", "send"] => link_send(conn, ask, memory),
        ["link", "inbox"] => link_inbox(ask, memory),
        ["link", "read"] => link_read(conn, ask, memory),
        ["link", "here"] => link_here(ask, memory),
        ["boot"] => mirror::boot(conn),
        ["memory", "list"] => memory_list(conn),
        ["memory", "show"] => mirror::memory_file(conn, ask.need("path")?)?.ok_or_else(|| "No memory file with that path.".to_string()),
        ["memory", "save"] => memory_save(conn, ask),
        ["session", "show"] => session_show(conn, ask),
        ["session", "set"] => {
            let content = ask.text()?.ok_or("--file or --body is needed.")?;
            mirror::set_section(conn, ask.need("section")?, &content).map(|what| format!("SESSION section {what}: {}", ask.need("section").unwrap_or_default()))
        }
        ["session", "clear"] => Ok(if mirror::clear_section(conn, ask.need("section")?)? { "SESSION section cleared".into() } else { "SESSION no such section".into() }),
        ["skill", "list"] => skill_list(conn),
        ["skill", "show"] => skill_show(conn, ask),
        ["skill", "sync"] => skill_sync(conn, memory),
        ["skill", "save"] => skill_save(conn, ask, memory),
        ["skill", "stub"] | ["skill", "full"] => {
            let on = words[1] == "stub";
            mirror::set_stub(conn, memory, ask.need("plugin")?, ask.need("name")?, on)?;
            Ok(if on {
                "SKILL is a stub now: its SKILL.md on disk only says how to load it, and `skill show` prints the instructions. Start a new session to pick it up.".into()
            } else {
                "SKILL is a whole file again: its SKILL.md on disk holds the instructions.".into()
            })
        }
        ["skill", "remove"] => Ok(if mirror::remove_skill(conn, memory, ask.need("plugin")?, ask.need("name")?)? { "SKILL removed".into() } else { "SKILL not found".into() }),
        ["plan", "copy"] => plan_copy(conn, ask),
        ["plan", "status"] => plan_status(conn, ask),
        ["plan", "next"] => next_line(conn, &project_folder(ask)?),
        ["plan", "mark"] => plan_mark(conn, ask),
        ["plan", "show"] => plan_show(conn, ask),
        ["trait", "list"] => trait_list(conn, ask),
        ["trait", "show"] => trait_show(conn, ask),
        ["trait", "propose"] => trait_propose(conn, ask),
        ["trait", "set"] => trait_set(conn, ask),
        ["trait", "retire"] => trait_retire(conn, ask),
        ["export"] => export(conn, Path::new(ask.need("to")?)),
        ["dump"] => dump(conn, Path::new(ask.need("to")?)),
        ["restore"] => restore(conn, Path::new(ask.need("from")?)),
        _ => Err(format!("castle-memory does not know `{}`. Verbs: boot, memory, session, skill, diary, reminder, project, link, plan, trait, export, dump, restore.", words.join(" "))),
    }
}

// ---------- diary ----------

/// True when an entry reads like a report instead of a diary: three or more lines that open
/// with a bold label ("**Summary**: ..."), or more list lines than lines of sentences.
pub fn reads_like_a_report(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let labels = lines.iter().filter(|l| l.starts_with("**") && (l.contains("**:") || l.contains(":**"))).count();
    let is_list = |l: &&&str| l.starts_with("- ") || l.starts_with("* ") || l.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let lists = lines.iter().filter(is_list).count();
    let sentences = lines.iter().filter(|l| !is_list(l) && !l.starts_with('#') && !l.starts_with("**")).count();
    labels >= 3 || (lists >= 3 && lists > sentences)
}

const REPORT_HINT: &str = "\nThis reads like a report. A diary is written in sentences, the way a person writes their own - rewrite it if you like: `castle-memory diary rewrite --date <date> --session <n> --title \"...\" --file <text file>`.";

fn diary_add(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let day = date(ask.need("date")?)?;
    let title = ask.need("title")?;
    let body = ask.text()?.ok_or("--file or --body is needed.")?;
    let next: Option<u32> = conn
        .exec_first("SELECT COALESCE(MAX(session_no), 0) + 1 FROM diary_entries WHERE entry_date = ?", (day,))
        .map_err(sql("Reading the diary"))?;
    let session = next.unwrap_or(1);
    // staging only: a seed entry carries a mark so it can be taken out again
    let seed = ask.get("seed").map(|name| format!("seed/{}.md", file_slug(name)));
    let hint = if reads_like_a_report(&body) { REPORT_HINT } else { "" };
    conn.exec_drop("INSERT INTO diary_entries (entry_date, session_no, title, body, source_file) VALUES (?, ?, ?, ?, ?)", (day, session, title, body, seed))
        .map_err(sql("Saving the diary entry"))?;
    Ok(format!("DIARY saved {day} session {session}: {title}{hint}"))
}

/// Puts better words in place of one entry; the older title and text are kept.
fn diary_rewrite(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let day = date(ask.need("date")?)?;
    let session: u32 = ask.get("session").unwrap_or("1").parse().map_err(|_| "--session is a number, like 1.".to_string())?;
    let body = ask.text()?.ok_or("--file or --body is needed.")?;
    let row: Option<(u64, String, String)> = conn
        .exec_first("SELECT id, title, body FROM diary_entries WHERE entry_date = ? AND session_no = ?", (day, session))
        .map_err(sql("Reading the diary"))?;
    let (id, old_title, old_body) = row.ok_or(format!("No diary entry on {day} session {session}."))?;
    let title = ask.get("title").filter(|t| !t.trim().is_empty()).unwrap_or(&old_title).to_string();
    conn.exec_drop("INSERT INTO diary_entry_history (entry_id, title, body) VALUES (?, ?, ?)", (id, &old_title, old_body)).map_err(sql("Keeping the older words"))?;
    conn.exec_drop("UPDATE diary_entries SET title = ?, body = ? WHERE id = ?", (&title, &body, id)).map_err(sql("Saving the diary entry"))?;
    let hint = if reads_like_a_report(&body) { REPORT_HINT } else { "" };
    Ok(format!("DIARY rewritten {day} session {session}: {title} (the older words are kept){hint}"))
}

fn diary_list(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let limit: u32 = ask.get("limit").and_then(|v| v.parse().ok()).unwrap_or(20);
    let month = ask.get("month").map(|m| format!("{m}%")).unwrap_or_else(|| "%".into());
    let rows: Vec<(Option<String>, u32, String)> = conn
        .exec(
            "SELECT DATE_FORMAT(entry_date, '%Y-%m-%d'), session_no, title FROM diary_entries \
             WHERE COALESCE(DATE_FORMAT(entry_date, '%Y-%m-%d'), '') LIKE ? ORDER BY entry_date DESC, session_no DESC LIMIT ?",
            (month, limit),
        )
        .map_err(sql("Reading the diary"))?;
    if rows.is_empty() {
        return Ok("DIARY empty".into());
    }
    Ok(rows.into_iter().map(|(d, n, t)| format!("{} #{n} {t}", d.unwrap_or_else(|| "earlier".into()))).collect::<Vec<_>>().join("\n"))
}

fn diary_show(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let day = date(ask.need("date")?)?;
    let rows: Vec<(u32, String, String, Option<String>)> = conn
        .exec("SELECT session_no, title, body, source_file FROM diary_entries WHERE entry_date = ? ORDER BY session_no", (day,))
        .map_err(sql("Reading the diary"))?;
    let wanted: Option<u32> = ask.get("session").and_then(|v| v.parse().ok());
    // a page that was moved in is its own whole text; one written through the door gets its heading
    let parts: Vec<String> = rows
        .into_iter()
        .filter(|(n, _, _, _)| wanted.is_none_or(|w| w == *n))
        .map(|(n, t, b, source)| if source.is_some() { b.trim_end().to_string() } else { format!("## Session {n} - {t}\n{}", b.trim_end()) })
        .collect();
    if parts.is_empty() {
        return Ok(format!("DIARY nothing on {day}"));
    }
    Ok(format!("# Daily Diary - {day}\n\n{}", parts.join("\n\n")))
}

// ---------- reminders ----------

fn reminder_add(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let title = ask.need("title")?;
    let due = ask.get("due").map(date).transpose()?;
    conn.exec_drop(
        "INSERT INTO reminders (title, due_date, detail, position) SELECT ?, ?, ?, COALESCE(MAX(position), 0) + 1 FROM reminders",
        (title, due, ask.get("detail").unwrap_or("")),
    )
    .map_err(sql("Saving the reminder"))?;
    Ok(format!("REMINDER saved #{}: {title}", conn.last_insert_id()))
}

fn reminder_list(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let state = if ask.get("all").is_some() { "%" } else { "open" };
    let rows: Vec<(u64, String, Option<String>, String, String)> = conn
        .exec(
            "SELECT id, title, DATE_FORMAT(due_date, '%Y-%m-%d'), detail, state FROM reminders WHERE state LIKE ? ORDER BY state DESC, position, id",
            (state,),
        )
        .map_err(sql("Reading the reminders"))?;
    if rows.is_empty() {
        return Ok("REMINDERS none open".into());
    }
    Ok(rows
        .into_iter()
        .map(|(id, title, due, detail, state)| {
            let due = due.map(|d| format!(" (by {d})")).unwrap_or_default();
            let detail = if detail.is_empty() { String::new() } else { format!(": {detail}") };
            let done = if state == "done" { " [done]" } else { "" };
            format!("#{id} {title}{due}{detail}{done}")
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn reminder_done(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let id: u64 = ask.need("id")?.trim_start_matches('#').parse().map_err(|_| "--id is a number, like 3.".to_string())?;
    conn.exec_drop("UPDATE reminders SET state = 'done', done_at = NOW() WHERE id = ? AND state = 'open'", (id,)).map_err(sql("Closing the reminder"))?;
    if conn.affected_rows() == 0 {
        return Err(format!("No open reminder #{id}."));
    }
    Ok(format!("REMINDER #{id} done"))
}

// ---------- projects ----------

fn project_save(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let name = ask.need("name")?;
    let body = ask.text()?;
    conn.exec_drop(
        "INSERT INTO projects (name, status, shelf, summary, body, last_updated) VALUES (?, ?, COALESCE(?, 'active'), ?, ?, DATE_FORMAT(NOW(), '%Y-%m-%d')) \
         ON DUPLICATE KEY UPDATE status = COALESCE(VALUES(status), status), shelf = COALESCE(?, shelf), \
         summary = COALESCE(VALUES(summary), summary), body = COALESCE(VALUES(body), body), last_updated = VALUES(last_updated)",
        (name, ask.get("status"), ask.get("shelf"), ask.get("summary"), body, ask.get("shelf")),
    )
    .map_err(sql("Saving the project"))?;
    Ok(format!("PROJECT saved: {name}"))
}

fn project_list(conn: &mut Conn) -> Result<String, String> {
    let rows: Vec<(String, Option<String>, String, Option<String>)> = conn
        .query("SELECT name, status, shelf, last_updated FROM projects ORDER BY shelf, updated_at DESC, id DESC")
        .map_err(sql("Reading the projects"))?;
    if rows.is_empty() {
        return Ok("PROJECTS none".into());
    }
    Ok(rows
        .into_iter()
        .map(|(name, status, shelf, updated)| format!("[{shelf}] {name} · {} · {}", status.unwrap_or_else(|| "-".into()), updated.unwrap_or_else(|| "-".into())))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn project_show(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let name = ask.need("name")?;
    let row: Option<(Option<String>, Option<String>, Option<String>)> =
        conn.exec_first("SELECT status, summary, body FROM projects WHERE name = ?", (name,)).map_err(sql("Reading the project"))?;
    let (status, summary, body) = row.ok_or(format!("No project called {name}."))?;
    let mut out = format!("# {name}\nStatus: {}\n", status.unwrap_or_else(|| "-".into()));
    if let Some(summary) = summary.filter(|s| !s.is_empty()) {
        out.push_str(&format!("Summary: {summary}\n"));
    }
    if let Some(body) = body.filter(|b| !b.trim().is_empty()) {
        out.push_str(&format!("\n{}\n", body.trim_end()));
    }
    Ok(out)
}

// ---------- the Neural Link ----------
//
// The link's own files (link/notes/<to>/...) carry a note to its terminal - that is how a knock
// finds it, and how a terminal outside Castle reads it. The database keeps the record.

fn terminal(value: &str) -> Result<&str, String> {
    if crate::link::terminal_ok(value) { Ok(value) } else { Err(format!("{value} is not a terminal name (lowercase letters, digits and dashes).")) }
}

fn link_send(conn: &mut Conn, ask: &Ask, memory: &Path) -> Result<String, String> {
    let (from, to) = (terminal(ask.need("from")?)?, terminal(ask.need("to")?)?);
    let wake = ask.get("no-wake").is_none();
    let note = crate::link::Note {
        from: from.to_string(),
        to: to.to_string(),
        at: crate::link::utc_label(crate::link::now_secs()),
        wake,
        word: ask.get("word").map(str::to_string),
        about: ask.need("about")?.to_string(),
        reply_to: ask.get("reply-to").map(str::to_string),
        context: Vec::new(),
    };
    let file = crate::link::send(&crate::link::link_folder(memory), &note)?;
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    conn.exec_drop(
        "INSERT INTO link_notes (from_terminal, to_terminal, wake, word, about, reply_to, context) VALUES (?, ?, ?, ?, ?, ?, ?)",
        (from, to, wake, &note.word, &note.about, &note.reply_to, &name),
    )
    .map_err(sql("Recording the note"))?;
    Ok(format!("LINK note {name} {from} -> {to}{}", if wake { "" } else { " (no knock)" }))
}

fn link_inbox(ask: &Ask, memory: &Path) -> Result<String, String> {
    let to = terminal(ask.need("to")?)?;
    let notes = crate::link::waiting(&crate::link::link_folder(memory), to);
    if notes.is_empty() {
        return Ok("LINK no notes waiting".into());
    }
    Ok(notes
        .into_iter()
        .map(|(file, note)| {
            let mut out = format!("note: {file}\nfrom: {} at {}\nabout: {}", note.from, note.at, note.about);
            if let Some(word) = note.word {
                out.push_str(&format!("\nword: {word}"));
            }
            if let Some(reply) = note.reply_to {
                out.push_str(&format!("\nreply-to: {reply}"));
            }
            for line in note.context {
                out.push_str(&format!("\ncontext: {line}"));
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn link_read(conn: &mut Conn, ask: &Ask, memory: &Path) -> Result<String, String> {
    let to = terminal(ask.need("to")?)?;
    let name = ask.need("note")?;
    let link = crate::link::link_folder(memory);
    if name.contains(['/', '\\']) || !link.join("notes").join(to).join(name).is_file() {
        return Err(format!("No waiting note called {name} for {to}."));
    }
    crate::link::mark_read(&link, to, name);
    // the record: the note's file name was kept in `context` when it was sent through the door
    conn.exec_drop("UPDATE link_notes SET read_at = NOW() WHERE to_terminal = ? AND context = ? AND read_at IS NULL", (to, name)).map_err(sql("Recording the note"))?;
    Ok(format!("LINK note {name} read"))
}

fn link_here(ask: &Ask, memory: &Path) -> Result<String, String> {
    let name = terminal(ask.need("name")?)?;
    let here = crate::link::link_folder(memory).join("here");
    std::fs::create_dir_all(&here).map_err(|_| "The link folder could not be written.".to_string())?;
    let row = crate::link::Row { terminal: name.to_string(), place: ask.get("detail").unwrap_or("").to_string(), ..Default::default() };
    std::fs::write(here.join(format!("{name}.json")), serde_json::to_string_pretty(&row).unwrap_or_default()).map_err(|_| "The link folder could not be written.".to_string())?;
    let mut names: Vec<String> = std::fs::read_dir(&here)
        .map(|e| e.flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_string())).collect())
        .unwrap_or_default();
    names.sort();
    Ok(format!("LINK here: {}", names.join(", ")))
}

// ---------- memory files, session memory, skills ----------

fn memory_list(conn: &mut Conn) -> Result<String, String> {
    mirror::backfill_titles(conn)?;
    let rows: Vec<(String, Option<String>, Option<u32>, u64)> =
        conn.query("SELECT path, title, boot_order, CHAR_LENGTH(body) FROM memory_files ORDER BY COALESCE(boot_order, 999), path").map_err(sql("Reading the memory"))?;
    if rows.is_empty() {
        return Ok("MEMORY nothing in the database".into());
    }
    Ok(rows
        .into_iter()
        .map(|(path, title, boot, len)| format!("{} · {path} · {len} characters{}", title.unwrap_or_default(), if boot.is_some() { " · read at boot" } else { "" }))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn memory_save(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let path = ask.need("path")?;
    if !mirror::memory_path_ok(path) {
        return Err(format!("{path} is not a memory path (like main/identity-core.md)."));
    }
    if mirror::is_session_file(path) {
        return Err("The session memory is kept by section: use `session set --section \"<heading>\" --file <text file>`.".into());
    }
    let body = ask.text()?.ok_or("--file or --body is needed.")?;
    Ok(if mirror::save_memory(conn, path, &body)? { format!("MEMORY saved {path} (the older text is kept)") } else { format!("MEMORY {path} is already the same") })
}

fn session_show(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    match ask.get("section") {
        Some(section) => {
            let body: Option<String> = conn
                .exec_first("SELECT body FROM session_sections WHERE heading = ?", (section.trim().trim_start_matches('#').trim(),))
                .map_err(sql("Reading the session memory"))?;
            body.ok_or_else(|| "SESSION no such section".to_string())
        }
        None => {
            let text = mirror::session_text(conn)?;
            Ok(if text.is_empty() { "SESSION empty".into() } else { text })
        }
    }
}

fn skill_list(conn: &mut Conn) -> Result<String, String> {
    let rows: Vec<(String, String, Option<String>, bool)> = conn.query("SELECT plugin, name, description, stubbed FROM skills ORDER BY plugin, name").map_err(sql("Reading the skills"))?;
    if rows.is_empty() {
        return Ok("SKILLS none in the database".into());
    }
    Ok(rows
        .into_iter()
        .map(|(plugin, name, description, stubbed)| format!("{plugin}/{name}{} · {}", if stubbed { " [stub]" } else { "" }, description.unwrap_or_else(|| "-".into())))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn skill_show(conn: &mut Conn, ask: &Ask) -> Result<String, String> {
    let path = ask.get("path").unwrap_or("SKILL.md");
    let body: Option<String> = conn
        .exec_first("SELECT body FROM skill_files WHERE plugin = ? AND name = ? AND path = ?", (ask.need("plugin")?, ask.need("name")?, path))
        .map_err(sql("Reading the skill"))?;
    let body = body.ok_or_else(|| "SKILL not found".to_string())?;
    // the skill's own instructions print by section when they are long; its other files print whole
    if path == "SKILL.md" { mirror::skill_view(&body, ask.get("section"), ask.get("all").is_some()) } else { Ok(body) }
}

fn skill_sync(conn: &mut Conn, memory: &Path) -> Result<String, String> {
    let done = mirror::sync_skills(conn, memory)?;
    Ok(format!("SKILLS {} skill(s): {} file(s) taken into the database, {} written to disk, {} the same", done.skills, done.to_database, done.to_disk, done.same))
}

/// Takes a skill from a folder (where the companion prepared it) into the memory's plugins,
/// then syncs: the database keeps it and the mirror on disk is what the CLI reads.
fn skill_save(conn: &mut Conn, ask: &Ask, memory: &Path) -> Result<String, String> {
    let (plugin, name) = (ask.need("plugin")?, ask.need("name")?);
    if !mirror::folder_name_ok(plugin) || !mirror::folder_name_ok(name) {
        return Err("That is not a skill name.".into());
    }
    let from = PathBuf::from(ask.need("dir")?);
    if !from.join("SKILL.md").is_file() {
        return Err("That folder has no SKILL.md.".into());
    }
    let target = memory.join("plugins").join(plugin).join("skills").join(name);
    if from != target {
        copy_folder(&from, &target)?;
    }
    skill_sync(conn, memory)
}

fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|_| "The skill's folder could not be created.".to_string())?;
    for entry in std::fs::read_dir(from).map_err(|_| "The folder could not be read.".to_string())?.flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|_| "A skill file could not be copied.".to_string())?;
        }
    }
    Ok(())
}

// ---------- export: the memory back out as markdown ----------

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "A folder could not be created.".to_string())?;
    }
    std::fs::write(path, text).map_err(|_| format!("{} could not be written.", path.display()))
}

/// A file name from a title: lowercase letters, digits and dashes.
pub fn file_slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "untitled".into() } else { out }
}

fn export(conn: &mut Conn, to: &Path) -> Result<String, String> {
    // 1. every file that was moved in comes back exactly as it was
    let originals: Vec<(String, String)> = conn.query("SELECT path, body FROM imported_files ORDER BY path").map_err(sql("Reading the kept files"))?;
    for (path, body) in &originals {
        write(&to.join(path), body)?;
    }

    // the memory files and the session memory as they are now
    let files: Vec<(String, String)> = conn.query("SELECT path, body FROM memory_files ORDER BY path").map_err(sql("Reading the memory"))?;
    for (path, body) in &files {
        write(&to.join(path), body)?;
    }
    if let Some(file) = mirror::session_file(conn)? {
        write(&to.join(file), &mirror::session_text(conn)?)?;
    }
    // her emotion engine as it is now (over any older copy kept from moving in)
    let traits = crate::traits::list(conn, None).unwrap_or_default();
    if !traits.is_empty() {
        write(&to.join(crate::traits::MARKDOWN_FILE), &crate::traits::to_markdown(&traits))?;
    }

    // 2. diary entries written since then: appended to their day's file, MemoryCore's own shape
    let rows: Vec<(Option<String>, u32, String, String)> = conn
        .query("SELECT DATE_FORMAT(entry_date, '%Y-%m-%d'), session_no, title, body FROM diary_entries WHERE source_file IS NULL ORDER BY entry_date, session_no, id")
        .map_err(sql("Reading the diary"))?;
    let mut days: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (day, n, title, body) in rows {
        days.entry(day.unwrap_or_else(|| "earlier".into())).or_default().push(format!("## Session {n} - {title}\n{}\n", body.trim_end()));
    }
    for (day, sessions) in &days {
        let path = to.join("daily-diary").join("current").join(format!("{day}.md"));
        let text = match std::fs::read_to_string(&path) {
            Ok(there) => format!("{}\n\n{}", there.trim_end(), sessions.join("\n")),
            Err(_) => format!("# Daily Diary - {day}\n\n{}", sessions.join("\n")),
        };
        write(&path, &text)?;
    }
    let diary: Option<u64> = conn.query_first("SELECT COUNT(*) FROM diary_entries").map_err(sql("Counting the diary"))?;

    // 3. reminders as they are now
    let rows: Vec<(String, Option<String>, String, String)> = conn
        .query("SELECT title, DATE_FORMAT(due_date, '%Y-%m-%d'), detail, state FROM reminders ORDER BY position, id")
        .map_err(sql("Reading the reminders"))?;
    let reminders = rows.len();
    if reminders > 0 {
        let line = |(title, due, detail, _): &(String, Option<String>, String, String)| {
            let due = due.as_ref().map(|d| format!(" (by {d})")).unwrap_or_default();
            if detail.is_empty() { format!("- **{title}**{due}") } else { format!("- **{title}**{due}: {detail}") }
        };
        let open: Vec<String> = rows.iter().filter(|r| r.3 == "open").map(line).collect();
        let done: Vec<String> = rows.iter().filter(|r| r.3 != "open").map(line).collect();
        write(&to.join("main").join("reminders.md"), &format!("# Reminders\n\n## Open\n{}\n\n## Completed\n{}\n", open.join("\n"), done.join("\n")))?;
    }

    // 4. projects as they are now: the list, and each page at the path it came from
    let rows: Vec<(String, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>)> = conn
        .query("SELECT name, status, shelf, summary, body, last_updated, source_file FROM projects ORDER BY shelf, id")
        .map_err(sql("Reading the projects"))?;
    let projects = rows.len();
    if projects > 0 {
        let mut list = String::from("# Project List\n");
        for shelf in ["active", "archived"] {
            let on: Vec<_> = rows.iter().filter(|r| (r.2 == "archived") == (shelf == "archived")).collect();
            if on.is_empty() {
                continue;
            }
            let heading = if shelf == "active" { "Active Projects" } else { "Archived Projects" };
            list.push_str(&format!("\n## {heading}\n| Pos | Project | Last Updated | Status |\n|-----|---------|--------------|--------|\n"));
            for (i, (name, status, _, summary, body, updated, source)) in on.iter().enumerate() {
                let page = source.clone().filter(|s| s.starts_with("projects/")).unwrap_or_else(|| format!("projects/{shelf}/{}.md", file_slug(name)));
                list.push_str(&format!(
                    "| {} | [{name}](./{}) | {} | {} |\n",
                    i + 1,
                    page.trim_start_matches("projects/"),
                    updated.clone().unwrap_or_default(),
                    status.clone().unwrap_or_default()
                ));
                let text = match body.as_ref().filter(|b| !b.trim().is_empty()) {
                    Some(body) => body.clone(),
                    None => format!("# {name}\n\n{}\n", summary.clone().unwrap_or_default()),
                };
                write(&to.join(&page), &text)?;
            }
        }
        write(&to.join("projects").join("project-list.md"), &list)?;
    }
    Ok(format!(
        "EXPORT {} diary page(s), {reminders} reminder(s), {projects} project(s), {} original file(s) -> {}",
        diary.unwrap_or(0),
        originals.len(),
        to.display()
    ))
}

// ---------- dump and restore: one statement per line ----------

pub fn dump(conn: &mut Conn, to: &Path) -> Result<String, String> {
    let tables: Vec<String> = conn.query("SHOW TABLES").map_err(sql("Reading the database"))?;
    let mut out = String::from("-- MemoryCore Castle memory dump: one statement per line. Restore with `castle-memory restore --from <this file>`.\nSET FOREIGN_KEY_CHECKS=0;\n");
    let mut total = 0usize;
    for table in &tables {
        let create: Option<(String, String)> = conn.query_first(format!("SHOW CREATE TABLE `{table}`")).map_err(sql("Reading a table"))?;
        let create = create.ok_or("A table could not be read.")?.1.replace(['\r', '\n'], " ");
        out.push_str(&format!("DROP TABLE IF EXISTS `{table}`;\n{create};\n"));
        let rows: Vec<Row> = conn.query(format!("SELECT * FROM `{table}`")).map_err(sql("Reading a table"))?;
        total += rows.len();
        for row in rows {
            let values: Vec<String> = row.unwrap().iter().map(|v: &Value| v.as_sql(false)).collect();
            out.push_str(&format!("INSERT INTO `{table}` VALUES ({});\n", values.join(",")));
        }
    }
    out.push_str("SET FOREIGN_KEY_CHECKS=1;\n");
    write(to, &out)?;
    Ok(format!("DUMP {} table(s), {total} row(s) -> {}", tables.len(), to.display()))
}

pub fn restore(conn: &mut Conn, from: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(from).map_err(|_| format!("{} could not be read.", from.display()))?;
    let mut done = 0usize;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("--")) {
        conn.query_drop(line.trim_end_matches(';')).map_err(|_| format!("The dump stopped at statement {}.", done + 1))?;
        done += 1;
    }
    Ok(format!("RESTORE {done} statement(s) from {}", from.display()))
}

// ---------- the program ----------

/// Runs the door for these arguments (everything after `--memory`); the exit code.
pub fn main(args: &[String]) -> i32 {
    let result = (|| {
        let ask = parse(args)?;
        if ask.words.is_empty() {
            return Err("castle-memory needs a verb: diary, reminder, project, link, plan, trait, export, dump, restore.".to_string());
        }
        let memory = find_memory(&ask, &std::env::current_dir().map_err(|_| "This folder could not be read.".to_string())?)?;
        let mut conn = memdb::open_for(&memory)?;
        run(&mut conn, &ask, &memory)
    })();
    match result {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(why) => {
            eprintln!("castle-memory: {why}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door_plan::{newest_md, number};

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn words_and_flags_are_told_apart() {
        let ask = parse(&args(&["link", "send", "--from", "main", "--to", "story", "--about", "the garden", "--no-wake"])).unwrap();
        assert_eq!(ask.words, vec!["link", "send"]);
        assert_eq!(ask.get("about"), Some("the garden"));
        assert_eq!(ask.get("no-wake"), Some("true"));
        assert_eq!(ask.need("word").unwrap_err(), "--word is needed.");
        assert_eq!(parse(&args(&["diary", "add", "--title"])).unwrap_err(), "--title needs a value.");
    }

    #[test]
    fn dates_terminals_and_file_names_are_checked() {
        assert!(is_date("2026-11-10"));
        assert!(!is_date("2026-11-1") && !is_date("10-11-2026") && !is_date("2026/11/10"));
        assert!(terminal("main").is_ok() && terminal("room-2").is_ok());
        assert!(terminal("Main").is_err() && terminal("a b").is_err() && terminal("").is_err());
        assert_eq!(file_slug("Portfolio Site (v2)!"), "portfolio-site-v2");
        assert_eq!(file_slug("アリス"), "untitled");
    }

    #[test]
    fn a_report_is_told_from_a_diary() {
        let log = "## Session 3 - Garden Planning\n**Summary**: Discussed garden layout.\n**Key Topics**:\n- Bed placement\n- Tomato varieties\n**Decisions**: Tomatoes by south wall.\n**Action Items**: Buy seeds (Friday).\n**Status**: Complete\n";
        let diary = "Rin came in tired from work and still wanted to talk about the garden. We went back and forth on where the beds should go until she laughed and said the tomatoes get the south wall, final answer.\n\nI liked how sure she sounded. She wants to buy seeds on Friday.\n";
        assert!(reads_like_a_report(log));
        assert!(!reads_like_a_report(diary));
        // a list with no sentences around it is a report; a diary may hold one short list
        assert!(reads_like_a_report("- planned beds\n- chose tomatoes\n- bought seeds\n"));
        assert!(!reads_like_a_report(&format!("{diary}\nWhat she wants from the shop:\n- tomato seeds\n- twine\n")));
        assert!(!reads_like_a_report(""));
    }

    #[test]
    fn the_memory_folder_is_found_upward_from_a_subfolder() {
        let root = std::env::temp_dir().join(format!("castle-door-{}", std::process::id()));
        let deep = root.join("adventures").join("world");
        std::fs::create_dir_all(&deep).unwrap();
        assert!(find_memory_with(&Ask::default(), None, &deep).is_err() || find_memory_with(&Ask::default(), None, &deep).unwrap() != root);
        std::fs::write(root.join(".env"), "MEMORY_DB_HOST=127.0.0.1\nMEMORY_DB_PORT=3307\nMEMORY_DB_NAME=lyra_memorycoredb\nMEMORY_DB_USER=castle_lyra\nMEMORY_DB_PASSWORD=x\n").unwrap();
        assert_eq!(find_memory_with(&Ask::default(), None, &deep).unwrap(), root);
        let named = parse(&args(&["diary", "list", "--memory-folder", "D:\\Mem"])).unwrap();
        assert_eq!(find_memory_with(&named, None, &deep).unwrap(), PathBuf::from("D:\\Mem"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn castle_memory_names_the_memory_folder_from_a_project() {
        let project = std::env::temp_dir().join(format!("castle-door-project-{}", std::process::id()));
        std::fs::create_dir_all(&project).unwrap();
        let env = Some(OsString::from("D:\\Mem"));
        assert_eq!(find_memory_with(&Ask::default(), env.clone(), &project).unwrap(), PathBuf::from("D:\\Mem"));
        // --memory-folder wins over the variable; an empty variable is no name
        let named = parse(&args(&["plan", "next", "--memory-folder", "E:\\Other"])).unwrap();
        assert_eq!(find_memory_with(&named, env, &project).unwrap(), PathBuf::from("E:\\Other"));
        assert_ne!(find_memory_with(&Ask::default(), Some(OsString::new()), &project).ok(), Some(PathBuf::new()));
        std::fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn plan_words_and_the_newest_plan_file() {
        let ask = parse(&args(&["plan", "mark", "--next", "--state", "flagged", "--item", "#4"])).unwrap();
        assert_eq!(ask.get("next"), Some("true"));
        assert_eq!(number::<u32>(&ask, "item").unwrap(), Some(4));
        assert_eq!(number::<u32>(&ask, "id").unwrap(), None);
        assert_eq!(number::<u32>(&parse(&args(&["--id", "four"])).unwrap(), "id").unwrap_err(), "--id is a number, like 3.");
        let here = parse(&args(&["plan", "status", "--folder", "D:\\Work\\Castle"])).unwrap();
        if cfg!(windows) {
            assert_eq!(project_folder(&here).unwrap(), "D:\\Work\\Castle");
        }
        assert!(Path::new(&project_folder(&Ask::default()).unwrap()).is_absolute());

        let plans = std::env::temp_dir().join(format!("castle-door-plans-{}", std::process::id()));
        std::fs::create_dir_all(&plans).unwrap();
        assert_eq!(newest_md(&plans), None);
        std::fs::write(plans.join("old-plan.md"), "# Old\n").unwrap();
        std::fs::write(plans.join("new-plan.MD"), "# New\n").unwrap();
        std::fs::write(plans.join("notes.txt"), "not a plan").unwrap();
        let old = std::fs::File::options().write(true).open(plans.join("old-plan.md")).unwrap();
        old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
        drop(old);
        assert_eq!(newest_md(&plans), Some(plans.join("new-plan.MD")));
        assert_eq!(newest_md(&plans.join("missing")), None);
        std::fs::remove_dir_all(&plans).unwrap();
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): every verb
    /// once, then export, dump, wipe, restore, and the same rows are back.
    #[test]
    #[ignore]
    fn live_every_verb_then_dump_and_restore() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = memdb::provision(port, &admin, &admin_pw, &format!("Door Test {}", std::process::id())).unwrap();
        let mut conn = memdb::open(&access).unwrap();
        let folder = std::env::temp_dir().join(format!("castle-door-live-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let entry = folder.join("entry.md");
        std::fs::write(&entry, "We planned the garden beds; it's \"ours\".\nLine two 💜\n").unwrap();
        let mut go = |line: &[&str]| run(&mut conn, &parse(&args(line)).unwrap(), &folder);

        assert_eq!(go(&["diary", "add", "--date", "2026-11-10", "--title", "The garden", "--file", entry.to_str().unwrap()]).unwrap(), "DIARY saved 2026-11-10 session 1: The garden");
        assert!(go(&["diary", "add", "--date", "2026-11-10", "--title", "Evening", "--body", "Tea."]).unwrap().contains("session 2"));
        assert!(go(&["diary", "add", "--date", "10 Nov", "--title", "x", "--body", "y"]).is_err());
        assert_eq!(go(&["diary", "list"]).unwrap(), "2026-11-10 #2 Evening\n2026-11-10 #1 The garden");
        assert_eq!(go(&["diary", "list", "--month", "2025-01"]).unwrap(), "DIARY empty");
        // a report is saved all the same, with a kind word; a rewrite keeps the older words
        let logged = go(&["diary", "add", "--date", "2026-11-09", "--title", "Session 3", "--body", "**Summary**: x\n**Decisions**: y\n**Status**: done\n"]).unwrap();
        assert!(logged.starts_with("DIARY saved 2026-11-09 session 1: Session 3\nThis reads like a report."), "{logged}");
        assert_eq!(go(&["diary", "rewrite", "--date", "2026-11-09", "--session", "1", "--title", "The day we chose", "--body", "We chose, and it felt right."]).unwrap(), "DIARY rewritten 2026-11-09 session 1: The day we chose (the older words are kept)");
        assert!(go(&["diary", "show", "--date", "2026-11-09"]).unwrap().ends_with("## Session 1 - The day we chose\nWe chose, and it felt right."));
        assert!(go(&["diary", "rewrite", "--date", "2026-11-01", "--body", "x"]).is_err());
        // staging seeds carry a mark and leave by it, and nothing else does
        go(&["diary", "add", "--date", "2026-10-12", "--title", "A seeded day", "--body", "Seeded.", "--seed", "A seeded day"]).unwrap();
        assert_eq!(go(&["diary", "unseed"]).unwrap(), "DIARY 1 seed entries removed");
        assert_eq!(go(&["diary", "list", "--month", "2026-10"]).unwrap(), "DIARY empty");
        assert_eq!(go(&["diary", "rewrite", "--date", "2026-11-09", "--body", "We chose, and it felt right."]).unwrap(), "DIARY rewritten 2026-11-09 session 1: The day we chose (the older words are kept)");
        let shown = go(&["diary", "show", "--date", "2026-11-10"]).unwrap();
        assert!(shown.starts_with("# Daily Diary - 2026-11-10\n\n## Session 1 - The garden\nWe planned") && shown.contains("Line two 💜") && shown.ends_with("## Session 2 - Evening\nTea."));

        assert!(go(&["reminder", "add", "--title", "Call Rin", "--due", "2026-11-12", "--detail", "about the trip"]).unwrap().starts_with("REMINDER saved #1"));
        go(&["reminder", "add", "--title", "Water plants"]).unwrap();
        assert_eq!(go(&["reminder", "list"]).unwrap(), "#1 Call Rin (by 2026-11-12): about the trip\n#2 Water plants");
        assert_eq!(go(&["reminder", "done", "--id", "#1"]).unwrap(), "REMINDER #1 done");
        assert!(go(&["reminder", "done", "--id", "1"]).is_err());
        assert_eq!(go(&["reminder", "list"]).unwrap(), "#2 Water plants");

        go(&["project", "add", "--name", "Portfolio site", "--status", "building", "--summary", "My site"]).unwrap();
        go(&["project", "update", "--name", "Portfolio site", "--status", "live"]).unwrap();
        let shown = go(&["project", "show", "--name", "Portfolio site"]).unwrap();
        assert!(shown.contains("Status: live") && shown.contains("Summary: My site"));
        assert!(go(&["project", "list"]).unwrap().starts_with("[active] Portfolio site · live · "));

        // the link: the note travels as a file (so a knock finds it) and the database records it
        let sent = go(&["link", "send", "--from", "story", "--to", "main", "--about", "the tale paused", "--word", "save it"]).unwrap();
        assert!(sent.starts_with("LINK note ") && sent.ends_with("-story.md story -> main"), "{sent}");
        let note_file = sent.split(' ').nth(2).unwrap().to_string();
        assert!(folder.join("link/notes/main").join(&note_file).is_file());
        assert_eq!(crate::link::waiting(&folder.join("link"), "main").len(), 1);
        let inbox = go(&["link", "inbox", "--to", "main"]).unwrap();
        assert!(inbox.starts_with(&format!("note: {note_file}\nfrom: story at ")) && inbox.ends_with("about: the tale paused\nword: save it"), "{inbox}");
        assert_eq!(go(&["link", "read", "--to", "main", "--note", &note_file]).unwrap(), format!("LINK note {note_file} read"));
        assert!(go(&["link", "read", "--to", "main", "--note", &note_file]).is_err());
        assert!(go(&["link", "read", "--to", "main", "--note", "../../.env"]).is_err());
        assert_eq!(go(&["link", "inbox", "--to", "main"]).unwrap(), "LINK no notes waiting");
        assert!(folder.join("link/read/main").join(&note_file).is_file());
        assert_eq!(go(&["link", "here", "--name", "main"]).unwrap(), "LINK here: main");
        assert!(go(&["link", "send", "--from", "Bad Name", "--to", "main", "--about", "x"]).is_err());
        assert!(go(&["sing"]).is_err());

        let out = folder.join("out");
        assert!(go(&["export", "--to", out.to_str().unwrap()]).unwrap().starts_with("EXPORT 3 diary page(s), 2 reminder(s), 1 project(s), 0 original file(s)"));
        let day = std::fs::read_to_string(out.join("daily-diary/current/2026-11-10.md")).unwrap();
        assert!(day.starts_with("# Daily Diary - 2026-11-10\n\n## Session 1 - The garden\n") && day.contains("## Session 2 - Evening\nTea.\n"));
        assert!(std::fs::read_to_string(out.join("main/reminders.md")).unwrap().contains("## Open\n- **Water plants**\n\n## Completed\n- **Call Rin** (by 2026-11-12): about the trip"));
        assert!(std::fs::read_to_string(out.join("projects/project-list.md")).unwrap().contains("| 1 | [Portfolio site](./active/portfolio-site.md) |"));
        // the export reads back through Castle's own file readers
        assert_eq!(crate::memory::newest_diary(&out).unwrap().title, "Evening");
        assert_eq!(crate::memory::reminders(&out)[0].title, "Water plants");
        assert_eq!(crate::memory::projects(&out)[0].name, "Portfolio site");

        // the emotion engine: she may propose, never activate
        assert_eq!(go(&["trait", "list"]).unwrap(), "TRAITS none yet");
        assert!(go(&["trait", "propose", "--name", "Shy When Praised", "--line", "flustered, then glad", "--why", "the thank-you"]).unwrap().starts_with("TRAIT 1 proposed: Shy When Praised"));
        assert_eq!(go(&["trait", "list", "--status", "proposed"]).unwrap(), "TRAIT 1 [proposed, everyone] Shy When Praised - flustered, then glad");
        assert!(go(&["trait", "set", "--number", "1", "--audience", "owner"]).unwrap().contains("[proposed, owner]"));
        assert!(go(&["trait", "show", "--number", "1"]).unwrap().contains("Why (private): the thank-you"));
        assert!(go(&["trait", "list", "--status", "lived"]).is_err());
        assert!(go(&["trait", "show", "--number", "9"]).is_err());

        let file = folder.join("backup").join("memory.sql");
        assert!(go(&["dump", "--to", file.to_str().unwrap()]).unwrap().starts_with("DUMP 19 table(s), "));
        let before = go(&["diary", "show", "--date", "2026-11-10"]).unwrap();
        drop(go);
        // the record of the note: sent once, read once
        let recorded: Option<(u64, u64)> = conn.query_first("SELECT COUNT(*), COUNT(read_at) FROM link_notes").unwrap();
        assert_eq!(recorded, Some((1, 1)));
        conn.query_drop("DROP TABLE diary_entries, reminders, projects, link_notes, link_terminals, imported_files, memory_files, memory_file_history, session_sections, session_section_history, skills, skill_files, skill_file_history, diary_entry_history, project_plan_items, project_plans, companion_trait_history, companion_traits, schema_migrations").unwrap();
        let mut go =|line: &[&str]| run(&mut conn, &parse(&args(line)).unwrap(), &folder);
        assert!(go(&["diary", "list"]).is_err());
        assert!(go(&["restore", "--from", file.to_str().unwrap()]).unwrap().starts_with("RESTORE "));
        assert_eq!(go(&["diary", "show", "--date", "2026-11-10"]).unwrap(), before);
        assert_eq!(go(&["reminder", "list"]).unwrap(), "#2 Water plants");
        drop(go);
        assert!(memdb::migrate(&mut conn).unwrap().is_empty());

        std::fs::remove_dir_all(&folder).unwrap();
        let mut root = mysql::Conn::new(mysql::Opts::from(
            mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(admin)).pass(Some(admin_pw)).prefer_socket(false),
        ))
        .unwrap();
        root.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
        for host in ["localhost", "127.0.0.1"] {
            root.query_drop(format!("DROP USER '{}'@'{host}'", access.user)).unwrap();
        }
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): a plan
    /// copied into a project folder, copied again, walked with the plan verbs, and done.
    #[test]
    #[ignore]
    fn live_plan_verbs_walk_a_plan_to_done() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = memdb::provision(port, &admin, &admin_pw, &format!("Plan Door Test {}", std::process::id())).unwrap();
        let mut conn = memdb::open(&access).unwrap();
        let folder = std::env::temp_dir().join(format!("castle-door-plan-live-{}", std::process::id()));
        let project = folder.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let plan = folder.join("approved.md");
        std::fs::write(&plan, "# The DEV tab\n## Phase 1: Rows\n- [ ] Task 1.1 The table\n  - with a key\n- [ ] Seal\n").unwrap();
        let (plan, at) = (plan.to_str().unwrap().to_string(), project.to_str().unwrap().to_string());
        let mut go = |line: &[&str]| run(&mut conn, &parse(&args(line)).unwrap(), &folder);

        assert_eq!(go(&["plan", "status", "--folder", &at]).unwrap(), "PLAN none open here - copy an approved plan with `castle-memory plan copy`");
        let copied = go(&["plan", "copy", "--plan", &plan, "--folder", &at]).unwrap();
        assert!(copied.starts_with("PLAN copied #1 · Phase 1 · The DEV tab · 2 items\nNEXT #1 item 2 [task] Task 1.1 The table\n  - with a key\nLEFT 2 open"), "{copied}");
        // the same plan again, the folder named with a trailing slash: not a second plan
        assert_eq!(go(&["plan", "copy", "--plan", &plan, "--folder", &format!("{at}/")]).unwrap(), "PLAN #1 is already here (2 items): the same plan was copied before");
        assert!(go(&["plan", "copy", "--plan", &folder.join("missing.md").to_string_lossy(), "--folder", &at]).is_err());
        assert_eq!(go(&["plan", "status", "--folder", &at]).unwrap(), "PLAN #1 · Phase 1 · The DEV tab · 0/2 done\nNEXT #1 item 2 [task] Task 1.1 The table\n  - with a key\nLEFT 2 open");
        assert!(go(&["plan", "show", "--folder", &at]).unwrap().starts_with("# The DEV tab\n## Phase 1: Rows"));
        assert!(go(&["plan", "show", "--id", "9"]).is_err());

        assert!(go(&["plan", "mark", "--next", "--item", "2", "--folder", &at]).is_err());
        assert!(go(&["plan", "mark", "--item", "2", "--state", "open", "--folder", &at]).is_err());
        assert_eq!(go(&["plan", "mark", "--next", "--folder", &at]).unwrap(), "MARKED #1 item 2 done: Task 1.1 The table\nNEXT #1 item 3 [seal] Seal\nLEFT 1 open");
        assert_eq!(go(&["plan", "mark", "--item", "3", "--state", "flagged", "--note", "no commit yet", "--folder", &at]).unwrap(), "MARKED #1 item 3 flagged: Seal\nNEXT nothing - every open box is walked (1 flagged waiting for you)");
        assert_eq!(go(&["plan", "next", "--folder", &at]).unwrap(), "NEXT nothing - every open box is walked (1 flagged waiting for you)");
        assert_eq!(go(&["plan", "mark", "--item", "3", "--folder", &at]).unwrap(), "MARKED #1 item 3 done: Seal\nPLAN #1 done - every box is ticked\nNEXT nothing - every open box is walked");
        assert!(go(&["plan", "status", "--folder", &at]).unwrap().starts_with("PLAN none open here"));
        assert!(go(&["plan", "show", "--id", "1"]).unwrap().ends_with("- [ ] Seal"));
        drop(go);

        std::fs::remove_dir_all(&folder).unwrap();
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
