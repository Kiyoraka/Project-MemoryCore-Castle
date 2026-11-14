//! The memory architecture in the database: the memory files the companion boots from, the
//! session memory as sections, and the skills - which also stay on disk as a mirror, because
//! Claude Code and Codex only find a skill that is a file.
//!
//! ```text
//!  memory_files       master-memory.md, main/*.md           only in the database
//!  session_sections   main/current-session*.md, one row per "## " section
//!  skill_files        plugins/<plugin>/skills/<name>/**      database = truth, disk = mirror
//!                     the newer side wins by its clock; the older text goes to history
//! ```

use mysql::prelude::Queryable;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// A skill file larger than this is not text Castle mirrors.
const SKILL_FILE_LIMIT: u64 = 2 * 1024 * 1024;
/// The name of the part of the session memory before its first "## " heading.
pub const TOP: &str = "(top)";

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work.")
}

// ---------- pure ----------

/// The order the companion reads its memory in: master, the consolidated main memory, identity,
/// relationship, then every other main file by name. None = not read at boot.
pub fn boot_order(path: &str) -> Option<u32> {
    match path {
        "master-memory.md" => Some(1),
        "main/main-memory.md" => Some(2),
        "main/identity-core.md" => Some(3),
        "main/relationship-memory.md" => Some(4),
        p if p.starts_with("main/") && p.ends_with(".md") => Some(10),
        _ => None,
    }
}

/// True for the session memory file ("main/current-session.md", "main/current-session-memory.md").
pub fn is_session_file(path: &str) -> bool {
    path.strip_prefix("main/").is_some_and(|name| name.starts_with("current-session") && name.ends_with(".md"))
}

/// The session memory cut at its "## " headings. Each part keeps its own heading line and
/// every byte after it, so the parts joined are the file exactly. A heading used twice gets
/// " (2)", " (3)" after it.
pub fn split_sections(text: &str) -> Vec<(String, String)> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut heading = TOP.to_string();
    let mut body = String::new();
    for line in text.split_inclusive('\n') {
        if let Some(title) = line.strip_prefix("## ") {
            if !body.is_empty() || heading != TOP {
                parts.push((std::mem::take(&mut heading), std::mem::take(&mut body)));
            }
            heading = title.trim().to_string();
        }
        body.push_str(line);
    }
    if !body.is_empty() || heading != TOP {
        parts.push((heading, body));
    }
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    for (heading, _) in parts.iter_mut() {
        let n = seen.entry(heading.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            *heading = format!("{heading} ({n})");
        }
    }
    parts
}

/// A path inside the memory folder the door may write: relative, no "..", markdown.
pub fn memory_path_ok(path: &str) -> bool {
    !path.is_empty()
        && path.ends_with(".md")
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// A plugin or skill name: one folder name, nothing that walks.
pub fn folder_name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && name != "." && name != ".." && !name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// The `description:` of a SKILL.md's front matter.
pub fn skill_description(skill_md: &str) -> Option<String> {
    let mut lines = skill_md.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("description:") {
            let value = value.trim().trim_matches('"').trim_matches('\'').trim();
            return (!value.is_empty()).then(|| value.to_string());
        }
    }
    None
}

// ---------- a skill as a stub ----------

/// The line that marks a SKILL.md as Castle's stub.
pub const STUB_MARK: &str = "<!-- castle:skill-stub";
/// A skill this short is printed whole; a longer one prints its opening and its sections.
pub const WHOLE_LIMIT: usize = 8000;

pub fn is_stub(skill_md: &str) -> bool {
    skill_md.contains(STUB_MARK)
}

/// The front matter block of a SKILL.md, both `---` lines included, exactly as written.
pub fn front_matter(skill_md: &str) -> Option<&str> {
    let rest = skill_md.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let after = end + "\n---".len();
    // the closing line must end there
    matches!(rest[after..].chars().next(), None | Some('\n') | Some('\r')).then(|| &skill_md[..3 + after])
}

/// The stub written to disk for a skill served from the database: the same front matter (so
/// it is listed and triggered exactly as before), the one command that loads the instructions,
/// and the rules that hold even when they cannot be loaded.
pub fn stub_of(plugin: &str, name: &str, master: &str) -> String {
    let front = front_matter(master).map(str::to_string).unwrap_or_else(|| format!("---\nname: {name}\n---"));
    format!(
        "{front}\n\
         {STUB_MARK} - written by MemoryCore Castle. This skill's instructions live in the memory database; change them there, never here. -->\n\
         \n\
         # {name}\n\
         \n\
         The instructions for this skill are kept in the memory database, not in this file.\n\
         \n\
         **Before you do anything for this skill, load them** by running:\n\
         \n\
         ```\n\
         castle-memory skill show --plugin \"{plugin}\" --name \"{name}\"\n\
         ```\n\
         \n\
         (in PowerShell: `& \"$env:CASTLE_TOOLS\\castle-memory.cmd\" skill show --plugin \"{plugin}\" --name \"{name}\"`)\n\
         \n\
         Then follow what it prints exactly as if it were written here. A long skill prints its opening and a list of its sections: load a section with `--section <key>` when you reach it, or everything with `--all`.\n\
         \n\
         Rules that hold even when the instructions cannot be loaded:\n\
         - If the command says the database could not be reached, tell your person kindly that the memory database is stopped and that **Start** in Castle's Settings > Database brings it back. Do not guess the instructions, and do not act on this skill until they load.\n\
         - Never write the instructions back into this file. To change this skill, prepare its folder and run `castle-memory skill save --plugin \"{plugin}\" --name \"{name}\" --dir <that folder>`.\n"
    )
}

/// A section's key from its heading: lowercase letters, digits and dashes ("Step 2: Render" -> "step-2-render").
pub fn section_key(heading: &str) -> String {
    let mut out = String::new();
    for c in heading.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "section".into() } else { out }
}

/// A skill's text as (key, heading, body) parts; the part before the first "## " is "top".
/// Keys are unique: a repeated one gets -2, -3.
pub fn skill_sections(master: &str) -> Vec<(String, String, String)> {
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    split_sections(master)
        .into_iter()
        .map(|(heading, body)| {
            let base = if heading == TOP { "top".to_string() } else { section_key(&heading) };
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            let key = if *n > 1 { format!("{base}-{n}") } else { base };
            (key, heading, body)
        })
        .collect()
}

/// What `skill show` prints: the whole text for a short skill or `all`; one section when asked;
/// else the opening and the list of sections to load as they are needed.
pub fn skill_view(master: &str, section: Option<&str>, all: bool) -> Result<String, String> {
    let parts = skill_sections(master);
    if let Some(wanted) = section {
        return parts.iter().find(|(key, _, _)| key == wanted).map(|(_, _, body)| body.trim_end().to_string()).ok_or_else(|| {
            format!("No section `{wanted}`. Sections: {}", parts.iter().map(|(k, _, _)| k.as_str()).collect::<Vec<_>>().join(", "))
        });
    }
    if all || master.len() <= WHOLE_LIMIT || parts.len() < 2 {
        return Ok(master.trim_end().to_string());
    }
    let mut out = String::new();
    if let Some((_, _, top)) = parts.iter().find(|(key, _, _)| key == "top") {
        out.push_str(top.trim_end());
        out.push_str("\n\n");
    }
    out.push_str("SECTIONS - this skill is long, so only its opening is printed. Load each section when you reach it:\n");
    for (key, heading, body) in parts.iter().filter(|(key, _, _)| key != "top") {
        out.push_str(&format!("- `--section {key}` - {heading} ({} characters)\n", body.len()));
    }
    out.push_str("(`--all` prints everything at once.)");
    Ok(out)
}

/// A plain name for a memory row: its first "# " heading without the decoration in front
/// ("# 💜 Identity Core - lyra" -> "Identity Core - lyra"), else its file name in words
/// ("main/relationship-memory.md" -> "Relationship Memory").
pub fn memory_title(path: &str, body: &str) -> String {
    let heading = body
        .lines()
        .find_map(|l| l.trim().strip_prefix("# "))
        .map(|h| h.trim_start_matches(|c: char| !c.is_alphanumeric()).trim().to_string())
        .filter(|h| !h.is_empty());
    heading.unwrap_or_else(|| {
        let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md");
        stem.split(['-', '_'])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut chars = w.chars();
                chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// Gives a title to every memory row that has none yet (rows moved in before titles existed).
pub fn backfill_titles(conn: &mut impl Queryable) -> Result<usize, String> {
    let rows: Vec<(String, String)> = conn.query("SELECT path, body FROM memory_files WHERE title IS NULL").map_err(sql("Reading the memory"))?;
    for (path, body) in &rows {
        // updated_at is kept: naming a row is not a change to the memory
        conn.exec_drop("UPDATE memory_files SET title = ?, updated_at = updated_at WHERE path = ?", (memory_title(path, body), path)).map_err(sql("Naming the memory"))?;
    }
    Ok(rows.len())
}

// ---------- memory files ----------

/// Saves one memory file; the text it replaces goes to history.
pub fn save_memory(conn: &mut impl Queryable, path: &str, body: &str) -> Result<bool, String> {
    let old: Option<String> = conn.exec_first("SELECT body FROM memory_files WHERE path = ?", (path,)).map_err(sql("Reading the memory"))?;
    if old.as_deref() == Some(body) {
        return Ok(false);
    }
    if let Some(old) = old {
        conn.exec_drop("INSERT INTO memory_file_history (path, body) VALUES (?, ?)", (path, old)).map_err(sql("Keeping the older text"))?;
    }
    conn.exec_drop(
        "INSERT INTO memory_files (path, body, boot_order, title) VALUES (?, ?, ?, ?) ON DUPLICATE KEY UPDATE body = VALUES(body), boot_order = VALUES(boot_order), title = VALUES(title)",
        (path, body, boot_order(path), memory_title(path, body)),
    )
    .map_err(sql("Saving the memory"))?;
    Ok(true)
}

pub fn memory_file(conn: &mut impl Queryable, path: &str) -> Result<Option<String>, String> {
    conn.exec_first("SELECT body FROM memory_files WHERE path = ?", (path,)).map_err(sql("Reading the memory"))
}

/// A memory file's text wherever it lives now: the file while it is on disk, else the database.
pub fn read_memory_text(folder: &Path, path: &str) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(folder.join(path)) {
        return Some(text);
    }
    let mut conn = crate::memdb::open_for(folder).ok()?;
    memory_file(&mut conn, path).ok().flatten()
}

// ---------- session memory ----------

/// Replaces the whole session memory with `text` cut into sections (used when moving in).
pub fn save_session_whole(conn: &mut impl Queryable, file: &str, text: &str) -> Result<usize, String> {
    conn.query_drop("DELETE FROM session_sections").map_err(sql("Clearing the session memory"))?;
    let parts = split_sections(text);
    for (position, (heading, body)) in parts.iter().enumerate() {
        conn.exec_drop("INSERT INTO session_sections (file, position, heading, body) VALUES (?, ?, ?, ?)", (file, position as u32, heading, body))
            .map_err(sql("Saving the session memory"))?;
    }
    Ok(parts.len())
}

/// The session memory as one text, in order.
pub fn session_text(conn: &mut impl Queryable) -> Result<String, String> {
    let parts: Vec<String> = conn.query("SELECT body FROM session_sections ORDER BY position, id").map_err(sql("Reading the session memory"))?;
    Ok(parts.concat())
}

pub fn session_file(conn: &mut impl Queryable) -> Result<Option<String>, String> {
    conn.query_first("SELECT file FROM session_sections ORDER BY position LIMIT 1").map_err(sql("Reading the session memory"))
}

/// Sets one section (its text without the heading line); a new heading goes to the end.
pub fn set_section(conn: &mut impl Queryable, heading: &str, content: &str) -> Result<&'static str, String> {
    let heading = heading.trim().trim_start_matches('#').trim();
    if heading.is_empty() || heading.len() > 250 {
        return Err("A section needs a short heading.".into());
    }
    let body = if heading == TOP { format!("{}\n", content.trim_end()) } else { format!("## {heading}\n{}\n\n", content.trim_end()) };
    let old: Option<String> = conn.exec_first("SELECT body FROM session_sections WHERE heading = ?", (heading,)).map_err(sql("Reading the section"))?;
    match old {
        Some(old) if old == body => Ok("unchanged"),
        Some(old) => {
            conn.exec_drop("INSERT INTO session_section_history (heading, body) VALUES (?, ?)", (heading, old)).map_err(sql("Keeping the older text"))?;
            conn.exec_drop("UPDATE session_sections SET body = ? WHERE heading = ?", (body, heading)).map_err(sql("Saving the section"))?;
            Ok("updated")
        }
        None => {
            let file = session_file(conn)?.unwrap_or_else(|| "main/current-session.md".into());
            conn.exec_drop(
                "INSERT INTO session_sections (file, position, heading, body) SELECT ?, COALESCE(MAX(position), 0) + 1, ?, ? FROM session_sections",
                (file, heading, body),
            )
            .map_err(sql("Saving the section"))?;
            Ok("added")
        }
    }
}

pub fn clear_section(conn: &mut impl Queryable, heading: &str) -> Result<bool, String> {
    let heading = heading.trim().trim_start_matches('#').trim();
    let old: Option<String> = conn.exec_first("SELECT body FROM session_sections WHERE heading = ?", (heading,)).map_err(sql("Reading the section"))?;
    let Some(old) = old else { return Ok(false) };
    conn.exec_drop("INSERT INTO session_section_history (heading, body) VALUES (?, ?)", (heading, old)).map_err(sql("Keeping the older text"))?;
    conn.exec_drop("DELETE FROM session_sections WHERE heading = ?", (heading,)).map_err(sql("Clearing the section"))?;
    Ok(true)
}

// ---------- boot ----------

/// The body both boots share, in reading order: the memory files, then her emotion engine (her
/// active traits), then the session memory. Pure, tested.
pub fn compose_body(files: &[(String, String)], engine: &str, session_file: &str, session: &str) -> String {
    let mut out = String::new();
    for (path, body) in files {
        out.push_str(&format!("
===== {path} =====
{}
", body.trim_end()));
    }
    if !engine.trim().is_empty() {
        out.push_str(&format!("
===== emotion engine (your traits) =====
{}
", engine.trim_end()));
    }
    if !session.is_empty() {
        out.push_str(&format!("
===== {session_file} (session memory) =====
{}
", session.trim_end()));
    }
    out
}

/// The memory body from the database, or None when nothing has moved in. The traits are read so
/// that their own failure (a wake before the traits table exists) never costs the rest.
fn memory_body(conn: &mut impl Queryable) -> Result<Option<String>, String> {
    let files: Vec<(String, String)> = conn
        .query("SELECT path, body FROM memory_files WHERE boot_order IS NOT NULL ORDER BY boot_order, path")
        .map_err(sql("Reading the memory"))?;
    let session = session_text(conn)?;
    if files.is_empty() && session.is_empty() {
        return Ok(None);
    }
    let file = if session.is_empty() { String::new() } else { session_file(conn)?.unwrap_or_default() };
    let engine = crate::traits::engine_for_wake(conn);
    Ok(Some(compose_body(&files, &engine, &file, &session)))
}

/// Everything the companion reads at the start of a session, in order, as one text.
pub fn boot(conn: &mut impl Queryable) -> Result<String, String> {
    let Some(body) = memory_body(conn)? else {
        return Ok("MEMORY BOOT: this memory has not been moved into the database yet. Read `master-memory.md` and follow it.".into());
    };
    Ok(format!(
        "MEMORY BOOT - everything below is your memory, printed whole and in reading order.
         The files it names are not on disk any more: never look for them or recreate them. To read one again use
         `castle-memory memory show --path <name>`; to change one, `castle-memory memory save --path <name> --file <text file>`.
         The session memory is kept by section: `castle-memory session set --section \"<heading>\" --file <text file>`.
{body}"
    ))
}

/// The memory as Castle hands it to the companion when it wakes her: the same body in the same
/// order as `boot`, with a header written for a system prompt. None when nothing has moved in.
pub fn boot_prompt(conn: &mut impl Queryable) -> Result<Option<String>, String> {
    let Some(body) = memory_body(conn)? else { return Ok(None) };
    Ok(Some(format!(
        "# Your memory

         This is your memory. MemoryCore Castle read it from the memory database when it woke you, so it is already complete and already loaded:
         do not run `castle-memory boot`, and never look for the files named below on disk or recreate them - they live in the database.
         It is a picture of the moment you woke. What you save during this session is kept at once and is here again the next time you wake.
         To read a file again: `castle-memory memory show --path <name>`. To change one: `castle-memory memory save --path <name> --file <text file>`.
         The session memory is kept by section: `castle-memory session set --section \"<heading>\" --file <text file>`.
{body}"
    )))
}

// ---------- skills: database = truth, disk = mirror ----------

type SkillKey = (String, String, String);

fn mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

fn files_under(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, base, out);
        } else if let Ok(relative) = path.strip_prefix(base) {
            out.push((relative.display().to_string().replace('\\', "/"), path));
        }
    }
}

/// Every text file of every skill on disk: (plugin, skill, path in the skill) -> (text, changed at).
pub fn skills_on_disk(folder: &Path) -> BTreeMap<SkillKey, (String, u64)> {
    let mut found = BTreeMap::new();
    let Ok(plugins) = std::fs::read_dir(folder.join("plugins")) else { return found };
    for plugin in plugins.flatten().filter(|p| p.path().is_dir()) {
        let Ok(skills) = std::fs::read_dir(plugin.path().join("skills")) else { continue };
        for skill in skills.flatten().filter(|s| s.path().join("SKILL.md").is_file()) {
            let mut files = Vec::new();
            files_under(&skill.path(), &skill.path(), &mut files);
            for (relative, path) in files {
                if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX) > SKILL_FILE_LIMIT {
                    continue;
                }
                // pictures and other non-text files stay on disk only
                let Ok(Ok(body)) = std::fs::read(&path).map(String::from_utf8) else { continue };
                let key = (plugin.file_name().to_string_lossy().to_string(), skill.file_name().to_string_lossy().to_string(), relative);
                found.insert(key, (body, mtime_secs(&path)));
            }
        }
    }
    found
}

/// What one sync did.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSync {
    /// files the database took from disk (new, or edited by hand since)
    pub to_database: usize,
    /// files written to disk from the database (missing, or older than the database)
    pub to_disk: usize,
    pub same: usize,
    pub skills: usize,
}

fn skill_dir(folder: &Path, plugin: &str, name: &str) -> PathBuf {
    folder.join("plugins").join(plugin).join("skills").join(name)
}

fn write_skill_file(folder: &Path, key: &SkillKey, body: &str) -> Result<(), String> {
    if !folder_name_ok(&key.0) || !folder_name_ok(&key.1) || key.2.split('/').any(|part| !folder_name_ok(part)) {
        return Err("A skill in the database has a name that cannot be a file.".into());
    }
    let path = skill_dir(folder, &key.0, &key.1).join(&key.2);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "A skill folder could not be created.".to_string())?;
    }
    std::fs::write(&path, body).map_err(|_| format!("The skill file {} could not be written.", key.2))
}

fn put_skill_file(conn: &mut impl Queryable, key: &SkillKey, body: &str, at: u64) -> Result<(), String> {
    conn.exec_drop(
        "INSERT INTO skill_files (plugin, name, path, body, updated_at) VALUES (?, ?, ?, ?, FROM_UNIXTIME(?)) \
         ON DUPLICATE KEY UPDATE body = VALUES(body), updated_at = VALUES(updated_at)",
        (&key.0, &key.1, &key.2, body, at),
    )
    .map_err(sql("Saving a skill"))
}

/// Makes the skills on disk and in the database the same. A file only on one side is copied to
/// the other; a file that differs is taken from whichever side changed last, and the database's
/// older text goes to history. A skill switched to a stub keeps only the stub as its SKILL.md
/// on disk: a whole SKILL.md found there is taken in as the new instructions, then the stub is
/// written back.
pub fn sync_skills(conn: &mut impl Queryable, folder: &Path) -> Result<SkillSync, String> {
    let disk = skills_on_disk(folder);
    let rows: Vec<(String, String, String, String, u64)> = conn
        .query("SELECT plugin, name, path, body, UNIX_TIMESTAMP(updated_at) FROM skill_files")
        .map_err(sql("Reading the skills"))?;
    let mut database: BTreeMap<SkillKey, (String, u64)> = rows.into_iter().map(|(p, n, f, body, at)| ((p, n, f), (body, at))).collect();
    let stubbed: Vec<(String, String)> = conn.query("SELECT plugin, name FROM skills WHERE stubbed = 1").map_err(sql("Reading the skills"))?;
    let is_stubbed = |key: &SkillKey| key.2 == "SKILL.md" && stubbed.iter().any(|(p, n)| *p == key.0 && *n == key.1);
    let mut report = SkillSync::default();
    for (key, (body, at)) in &disk {
        if is_stubbed(key) {
            // the file on disk is the stub; a whole SKILL.md there is new instructions to take in
            if !is_stub(body) && database.get(key).map(|(kept, _)| kept.as_str()) != Some(body.as_str()) {
                if let Some((kept, _)) = database.get(key) {
                    conn.exec_drop("INSERT INTO skill_file_history (plugin, name, path, body) VALUES (?, ?, ?, ?)", (&key.0, &key.1, &key.2, kept))
                        .map_err(sql("Keeping a skill's older text"))?;
                }
                put_skill_file(conn, key, body, *at)?;
                database.insert(key.clone(), (body.clone(), *at));
                report.to_database += 1;
            }
            let Some((master, _)) = database.get(key) else { continue };
            let stub = stub_of(&key.0, &key.1, master);
            if *body == stub {
                report.same += 1;
            } else {
                write_skill_file(folder, key, &stub)?;
                report.to_disk += 1;
            }
            continue;
        }
        match database.get(key) {
            None => {
                put_skill_file(conn, key, body, *at)?;
                report.to_database += 1;
            }
            Some((kept, _)) if kept == body => report.same += 1,
            Some((kept, kept_at)) if at > kept_at => {
                conn.exec_drop("INSERT INTO skill_file_history (plugin, name, path, body) VALUES (?, ?, ?, ?)", (&key.0, &key.1, &key.2, kept))
                    .map_err(sql("Keeping a skill's older text"))?;
                put_skill_file(conn, key, body, *at)?;
                report.to_database += 1;
            }
            Some((kept, _)) => {
                write_skill_file(folder, key, kept)?;
                report.to_disk += 1;
            }
        }
    }
    for (key, (body, _)) in &database {
        if !disk.contains_key(key) {
            let text = if is_stubbed(key) { stub_of(&key.0, &key.1, body) } else { body.clone() };
            write_skill_file(folder, key, &text)?;
            report.to_disk += 1;
        }
    }

    // the catalog: one row per skill, described by its SKILL.md (a stub carries the same front matter)
    let mut skills: BTreeMap<(String, String), Option<String>> = BTreeMap::new();
    for (key, (body, _)) in database.iter().chain(disk.iter()) {
        let entry = skills.entry((key.0.clone(), key.1.clone())).or_insert(None);
        // the disk's copy comes last in the chain, so the newest reading counts - unless it
        // is a stub, whose front matter is the database's own
        if key.2 == "SKILL.md" && (entry.is_none() || !is_stub(body)) {
            *entry = skill_description(body);
        }
    }
    for ((plugin, name), description) in &skills {
        conn.exec_drop(
            "INSERT INTO skills (plugin, name, description) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE description = VALUES(description)",
            (plugin, name, description),
        )
        .map_err(sql("Saving the skill list"))?;
    }
    report.skills = skills.len();
    Ok(report)
}

/// Switches one skill between a whole SKILL.md on disk and a stub whose instructions are
/// loaded from the database. The instructions themselves are never changed by the switch.
pub fn set_stub(conn: &mut impl Queryable, folder: &Path, plugin: &str, name: &str, on: bool) -> Result<(), String> {
    if !folder_name_ok(plugin) || !folder_name_ok(name) {
        return Err("That is not a skill name.".into());
    }
    let master: Option<String> = conn
        .exec_first("SELECT body FROM skill_files WHERE plugin = ? AND name = ? AND path = 'SKILL.md'", (plugin, name))
        .map_err(sql("Reading the skill"))?;
    let master = master.ok_or_else(|| format!("No skill {plugin}/{name} in the database. Run `castle-memory skill sync` first."))?;
    conn.exec_drop("UPDATE skills SET stubbed = ? WHERE plugin = ? AND name = ?", (on, plugin, name)).map_err(sql("Switching the skill"))?;
    let key = (plugin.to_string(), name.to_string(), "SKILL.md".to_string());
    let text = if on { stub_of(plugin, name, &master) } else { master };
    write_skill_file(folder, &key, &text)
}

/// Removes a skill from the database and its mirror from disk (its text stays in history).
pub fn remove_skill(conn: &mut impl Queryable, folder: &Path, plugin: &str, name: &str) -> Result<bool, String> {
    if !folder_name_ok(plugin) || !folder_name_ok(name) {
        return Err("That is not a skill name.".into());
    }
    conn.exec_drop(
        "INSERT INTO skill_file_history (plugin, name, path, body) SELECT plugin, name, path, body FROM skill_files WHERE plugin = ? AND name = ?",
        (plugin, name),
    )
    .map_err(sql("Keeping the skill's text"))?;
    let files: Option<u64> = conn.exec_first("SELECT COUNT(*) FROM skill_files WHERE plugin = ? AND name = ?", (plugin, name)).map_err(sql("Reading the skill"))?;
    let there = files.unwrap_or(0) > 0;
    conn.exec_drop("DELETE FROM skill_files WHERE plugin = ? AND name = ?", (plugin, name)).map_err(sql("Removing the skill"))?;
    conn.exec_drop("DELETE FROM skills WHERE plugin = ? AND name = ?", (plugin, name)).map_err(sql("Removing the skill"))?;
    let dir = skill_dir(folder, plugin, name);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|_| "The skill's folder could not be removed.".to_string())?;
    }
    Ok(there)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_emotion_engine_sits_between_the_files_and_the_session() {
        let files = vec![("master-memory.md".to_string(), "Master.\n".to_string()), ("main/main-memory.md".to_string(), "Main.".to_string())];
        let body = compose_body(&files, "- Trait 1 - Curious: asks\n", "main/current-session.md", "## Today\nTea.");
        let (master, engine, session) = (body.find("===== master-memory.md").unwrap(), body.find("===== emotion engine").unwrap(), body.find("===== main/current-session.md (session memory)").unwrap());
        assert!(master < engine && engine < session);
        assert!(body.contains("- Trait 1 - Curious: asks\n"));
        // no traits (none active, or no table yet): no engine heading, the rest whole
        let plain = compose_body(&files, "", "main/current-session.md", "## Today\nTea.");
        assert!(!plain.contains("emotion engine") && plain.contains("Master.") && plain.contains("Tea."));
        assert!(!compose_body(&files, "  ", "", "").contains("session memory"));
    }

    #[test]
    fn the_boot_reads_master_then_main_then_the_rest() {
        let mut files = vec!["main/notes.md", "main/relationship-memory.md", "master-memory.md", "main/identity-core.md", "main/main-memory.md"];
        files.sort_by_key(|f| (boot_order(f), f.to_string()));
        assert_eq!(files, vec!["master-memory.md", "main/main-memory.md", "main/identity-core.md", "main/relationship-memory.md", "main/notes.md"]);
        assert_eq!(boot_order("daily-diary/daily-diary-protocol.md"), None);
        assert!(is_session_file("main/current-session.md") && is_session_file("main/current-session-memory.md"));
        assert!(!is_session_file("main/identity-core.md") && !is_session_file("current-session.md"));
    }

    #[test]
    fn a_memory_row_is_named_by_its_heading_or_its_file_name() {
        assert_eq!(memory_title("main/identity-core.md", "# 💜 Identity Core - lyra\n*Who I am*\n"), "Identity Core - lyra");
        assert_eq!(memory_title("master-memory.md", "intro\n# 🧠 Master Memory - Universal AI Architecture\n"), "Master Memory - Universal AI Architecture");
        assert_eq!(memory_title("main/relationship-memory.md", "no heading here\n"), "Relationship Memory");
        assert_eq!(memory_title("main/notes_for_rin.md", "# 💜\n"), "Notes For Rin");
    }

    #[test]
    fn the_session_memory_cut_into_sections_joins_back_exactly() {
        let text = "# Session\r\n*updated*\r\n\r\n## Appearance\r\n- outfit\r\n\r\n## Open threads\n- one\n## Open threads\n- again\n### deeper stays inside\n- x";
        let parts = split_sections(text);
        assert_eq!(parts.iter().map(|(h, _)| h.as_str()).collect::<Vec<_>>(), vec![TOP, "Appearance", "Open threads", "Open threads (2)"]);
        assert_eq!(parts.iter().map(|(_, b)| b.as_str()).collect::<String>(), text);
        assert_eq!(parts[1].1, "## Appearance\r\n- outfit\r\n\r\n");
        assert!(parts[3].1.contains("### deeper stays inside"));
        // no top part when the file opens with a heading; an empty file has no parts
        assert_eq!(split_sections("## Only\nx\n"), vec![("Only".to_string(), "## Only\nx\n".to_string())]);
        assert!(split_sections("").is_empty());
    }

    #[test]
    fn only_plain_paths_and_names_are_written() {
        assert!(memory_path_ok("master-memory.md") && memory_path_ok("main/identity-core.md"));
        for bad in ["", "../x.md", "main/../../x.md", "C:/x.md", "/etc/x.md", "main\\x.md", "main/x.txt", "main//x.md"] {
            assert!(!memory_path_ok(bad), "{bad}");
        }
        assert!(folder_name_ok("rin-skills") && folder_name_ok("save diary"));
        for bad in ["", "..", "a/b", "a\\b", "c:", "x*"] {
            assert!(!folder_name_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_skill_is_described_by_its_front_matter() {
        assert_eq!(skill_description("---\nname: save-diary\ndescription: \"Writes today into the diary\"\n---\n# Save\n").as_deref(), Some("Writes today into the diary"));
        assert_eq!(skill_description("# no front matter\ndescription: x\n"), None);
        assert_eq!(skill_description("---\nname: x\n---\ndescription: later\n"), None);
    }

    #[test]
    fn a_stub_keeps_the_front_matter_and_names_the_one_command() {
        let master = "---\nname: image-generation\ndescription: MUST use when the user wants an image. Triggers on \"render this\".\n---\n\n# Image Generation\n\n## Overview\nRenders.\n";
        assert_eq!(front_matter(master), Some("---\nname: image-generation\ndescription: MUST use when the user wants an image. Triggers on \"render this\".\n---"));
        assert_eq!(front_matter("# no front matter\n"), None);
        assert_eq!(front_matter("---\nname: x\n---more\n"), None);
        let stub = stub_of("lyra-skills", "image-generation", master);
        assert!(stub.starts_with("---\nname: image-generation\ndescription: MUST use when the user wants an image. Triggers on \"render this\".\n---\n<!-- castle:skill-stub"));
        assert!(stub.contains("castle-memory skill show --plugin \"lyra-skills\" --name \"image-generation\""));
        assert!(stub.contains("$env:CASTLE_TOOLS\\castle-memory.cmd"));
        assert!(stub.contains("Do not guess the instructions"));
        assert!(!stub.contains("Renders."));
        assert!(is_stub(&stub) && !is_stub(master));
        // the CLI reads the same name and description from the stub
        assert_eq!(skill_description(&stub), skill_description(master));
        // a skill without front matter still gets a name
        assert!(stub_of("p", "notes", "# Notes\n").starts_with("---\nname: notes\n---\n"));
    }

    #[test]
    fn a_short_skill_prints_whole_and_a_long_one_prints_its_sections() {
        let short = "---\nname: x\n---\n# X\n\n## Steps\n1. do\n";
        assert_eq!(skill_view(short, None, false).unwrap(), short.trim_end());
        let long = format!("---\nname: x\n---\n# X\nintro\n\n## Step 1: Gather\n{}\n## Step 2: Render\nrender it\n## Step 2: Render\nagain\n", "g".repeat(WHOLE_LIMIT));
        let keys: Vec<String> = skill_sections(&long).into_iter().map(|(k, _, _)| k).collect();
        assert_eq!(keys, vec!["top", "step-1-gather", "step-2-render", "step-2-render-2"]);
        let index = skill_view(&long, None, false).unwrap();
        assert!(index.starts_with("---\nname: x\n---\n# X\nintro\n\nSECTIONS"));
        assert!(index.contains("- `--section step-2-render` - Step 2: Render (") && !index.contains("ggggg"));
        assert_eq!(skill_view(&long, Some("step-2-render"), false).unwrap(), "## Step 2: Render\nrender it");
        assert_eq!(skill_view(&long, None, true).unwrap(), long.trim_end());
        assert!(skill_view(&long, Some("nope"), false).unwrap_err().contains("Sections: top, step-1-gather"));
        assert_eq!(section_key("  ## !! "), "section");
    }

    #[test]
    fn skills_on_disk_are_read_with_their_other_files_and_pictures_left_out() {
        let root = std::env::temp_dir().join(format!("castle-mirror-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let skill = root.join("plugins/rin-skills/skills/save-diary");
        std::fs::create_dir_all(skill.join("reference")).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: save-diary\ndescription: Saves\n---\nbody\n").unwrap();
        std::fs::write(skill.join("reference/flows.md"), "flows\n").unwrap();
        std::fs::write(skill.join("icon.png"), [0x89, 0x50, 0xff, 0x00]).unwrap();
        // a folder without SKILL.md is not a skill
        std::fs::create_dir_all(root.join("plugins/rin-skills/skills/notes")).unwrap();
        std::fs::write(root.join("plugins/rin-skills/skills/notes/readme.md"), "x").unwrap();
        let found = skills_on_disk(&root);
        assert_eq!(found.keys().map(|k| k.2.as_str()).collect::<Vec<_>>(), vec!["SKILL.md", "reference/flows.md"]);
        assert_eq!(found.keys().next().unwrap().0, "rin-skills");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
