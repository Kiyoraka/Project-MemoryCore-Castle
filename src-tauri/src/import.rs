//! Moving in: the memory's markdown read into the database, checked, and only then deleted.
//!
//! ```text
//!  collect(folder)   read everything, change nothing            -> Bundle (+ Counts to show)
//!  load(conn, ..)    one transaction: every original file kept exactly in imported_files,
//!                    the rows made from them, then each body read back and compared;
//!                    any difference rolls the whole thing back
//!  dump              backup/memory.sql written (so the private backup carries the memory)
//!  delete_originals  the markdown removed - only the files whose text the database now holds
//! ```
//!
//! A file that is not plain UTF-8 text is never imported and never deleted; it is named.
//! The Neural Link's notes are not moved: they are messages on their way between terminals,
//! and the link's own files are how a knock finds them.

use crate::memory;
use mysql::prelude::Queryable;
use mysql::{Conn, TxOpts};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A markdown file exactly as it was, path relative to the memory folder with forward slashes.
#[derive(Debug, Clone, PartialEq)]
pub struct Original {
    pub path: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiaryRow {
    pub date: Option<String>,
    pub session_no: u32,
    pub title: String,
    pub source_file: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReminderRow {
    pub title: String,
    pub due: Option<String>,
    pub detail: String,
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub name: String,
    pub status: Option<String>,
    pub archived: bool,
    pub last_updated: Option<String>,
    pub source_file: Option<String>,
}

/// Everything found in the memory folder, before anything is written.
#[derive(Debug, Default)]
pub struct Bundle {
    pub originals: Vec<Original>,
    pub diary: Vec<DiaryRow>,
    pub reminders: Vec<ReminderRow>,
    pub projects: Vec<ProjectRow>,
    /// the memory files (master-memory.md, main/*.md), by path
    pub memory: Vec<String>,
    /// the session memory file, by path
    pub session: Option<String>,
    /// files left in place because they are not readable text
    pub skipped: Vec<String>,
    /// the emotion engine's file (emotion/traits.md), by path
    pub traits: Option<String>,
}

/// What the member is shown before and after moving in.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub files: usize,
    pub diary: usize,
    pub reminders: usize,
    pub projects: usize,
    /// memory files, the session memory among them
    pub memory: usize,
    /// skills kept in the database (their files stay on disk too)
    pub skills: usize,
    pub skipped: Vec<String>,
}

impl Bundle {
    pub fn counts(&self) -> Counts {
        Counts {
            files: self.originals.len(),
            diary: self.diary.len(),
            reminders: self.reminders.len(),
            projects: self.projects.len(),
            memory: self.memory.len() + usize::from(self.session.is_some()),
            skills: 0,
            skipped: self.skipped.clone(),
        }
    }

    fn body_of(&self, path: &str) -> Option<&str> {
        self.originals.iter().find(|o| o.path == path).map(|o| o.body.as_str())
    }

    /// Reads a file into the originals (once); false when it is missing or not UTF-8 text.
    fn take(&mut self, folder: &Path, path: &Path) -> bool {
        let relative = relative(folder, path);
        if self.body_of(&relative).is_some() {
            return true;
        }
        match std::fs::read(path).map(String::from_utf8) {
            Ok(Ok(body)) => {
                self.originals.push(Original { path: relative, body });
                true
            }
            Ok(Err(_)) => {
                self.skipped.push(relative);
                false
            }
            Err(_) => false,
        }
    }
}

fn relative(folder: &Path, path: &Path) -> String {
    path.strip_prefix(folder).unwrap_or(path).display().to_string().replace('\\', "/")
}

fn is_md(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// "diary-2.md" before "diary-10.md": the text before the last number, then the number.
pub fn natural_key(name: &str) -> (String, u64) {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let digits: String = stem.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
    let prefix = stem[..stem.len() - digits.len()].to_lowercase();
    (prefix, digits.parse().unwrap_or(0))
}

fn sorted(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    found.sort_by_key(|p| natural_key(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    found
}

fn md_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for path in sorted(dir) {
        if path.is_dir() {
            md_under(&path, out);
        } else if is_md(&path) {
            out.push(path);
        }
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

// ---------- collect ----------

/// True for the template's empty diary page: it still carries the brackets nobody filled in
/// and holds no entry, only the old rule about archiving a file at 1000 lines.
pub fn is_placeholder_diary(text: &str) -> bool {
    text.contains("[AI will track automatically]") || text.contains("[Date when first entry added]")
}

/// Removes diary rows that are that placeholder (a memory moved in before Castle knew it was
/// not an entry). The file itself stays kept in imported_files. Returns how many went.
pub fn drop_placeholder_entries(conn: &mut Conn) -> Result<usize, String> {
    let rows: Vec<(u64, String)> = conn.query("SELECT id, body FROM diary_entries WHERE source_file IS NOT NULL").map_err(sql("Reading the diary"))?;
    let mut gone = 0;
    for (id, body) in rows {
        if is_placeholder_diary(&body) {
            conn.exec_drop("DELETE FROM diary_entries WHERE id = ?", (id,)).map_err(sql("Tidying the diary"))?;
            gone += 1;
        }
    }
    Ok(gone)
}

/// The diary in all its shapes: `current/YYYY-MM-DD.md`, `current/YYYY-MM-DD/*.md`,
/// `Daily-Diary-NNN.md`, and both shapes again under `archived/YYYY-MM/`. One row per file,
/// its text whole; the day's files are numbered in their own order.
fn collect_diary(folder: &Path, bundle: &mut Bundle) {
    let diary = folder.join("daily-diary");
    // (date, file) in reading order
    let mut found: Vec<(Option<String>, PathBuf)> = Vec::new();
    let day_folder = |dir: &Path, found: &mut Vec<(Option<String>, PathBuf)>| {
        for path in sorted(dir) {
            let name = stem(&path);
            if is_md(&path) && crate::door::is_date(&name) {
                found.push((Some(name), path));
            } else if path.is_dir() && crate::door::is_date(&name) {
                for file in sorted(&path).into_iter().filter(|f| is_md(f)) {
                    found.push((Some(name.clone()), file));
                }
            } else if is_md(&path) && dir != diary.as_path() && !dir.ends_with("current") {
                // an undated page inside an archived month
                found.push((None, path));
            }
        }
    };
    for path in sorted(&diary) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if is_md(&path) && name.starts_with("Daily-Diary-") {
            found.push((None, path));
        }
    }
    for month in sorted(&diary.join("archived")).into_iter().filter(|p| p.is_dir()) {
        day_folder(&month, &mut found);
    }
    day_folder(&diary.join("current"), &mut found);
    // the old rules for archiving diary files: kept, taken off the disk, never a diary page
    bundle.take(folder, &diary.join("daily-diary-protocol.md"));

    let mut numbers: BTreeMap<Option<String>, u32> = BTreeMap::new();
    for (date, path) in found {
        if !bundle.take(folder, &path) {
            continue;
        }
        let source_file = relative(folder, &path);
        let text = bundle.body_of(&source_file).unwrap_or_default();
        // the template's empty page is kept as a file in the database, never shown as an entry
        if is_placeholder_diary(text) {
            continue;
        }
        let title = memory::diary_title(text).unwrap_or_else(|| stem(&path));
        let n = numbers.entry(date.clone()).or_insert(0);
        *n += 1;
        bundle.diary.push(DiaryRow { date, session_no: *n, title, source_file });
    }
}

/// One reminder line: "- **Title** (by 2026-11-10): what".
pub fn reminder_line(item: &str, open: bool) -> Option<ReminderRow> {
    let item = item.trim();
    let (head, detail) = item.split_once(':').map(|(h, d)| (h.trim(), d.trim())).unwrap_or((item, ""));
    let due = head.split_once("(by ").and_then(|(_, rest)| rest.split(')').next()).map(|d| d.trim().to_string()).filter(|d| crate::door::is_date(d));
    let title = match (head.find("**"), head.rfind("**")) {
        (Some(a), Some(b)) if b > a + 2 => head[a + 2..b].trim(),
        _ => head.split(" (").next().unwrap_or(head).trim().trim_matches('*').trim(),
    };
    (!title.is_empty()).then(|| ReminderRow { title: title.to_string(), due, detail: detail.to_string(), open })
}

fn collect_reminders(folder: &Path, bundle: &mut Bundle) {
    let path = folder.join("main").join("reminders.md");
    if !bundle.take(folder, &path) {
        return;
    }
    let text = bundle.body_of("main/reminders.md").unwrap_or_default().to_string();
    let mut open = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            open = heading.trim().eq_ignore_ascii_case("open");
        } else if let Some(row) = trimmed.strip_prefix("- ").and_then(|item| reminder_line(item, open)) {
            bundle.reminders.push(row);
        }
    }
}

/// The project list's rows (both shelves), then every project page the list does not name.
fn collect_projects(folder: &Path, bundle: &mut Bundle) {
    let projects = folder.join("projects");
    let list = projects.join("project-list.md");
    if bundle.take(folder, &list) {
        let text = bundle.body_of("projects/project-list.md").unwrap_or_default().to_string();
        let mut archived = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(heading) = trimmed.strip_prefix("## ") {
                archived = heading.to_lowercase().contains("archiv");
                continue;
            }
            if !trimmed.starts_with('|') || trimmed.contains("---") {
                continue;
            }
            let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
            if cells.len() < 2 || cells[0].eq_ignore_ascii_case("pos") || cells[1].eq_ignore_ascii_case("project") || cells[1].is_empty() {
                continue;
            }
            let cell = cells[1];
            let (name, file) = match (cell.find('['), cell.find("]("), cell.rfind(')')) {
                (Some(a), Some(b), Some(c)) if a < b && b < c => (cell[a + 1..b].to_string(), Some(format!("projects/{}", cell[b + 2..c].trim_start_matches("./")))),
                _ => (cell.to_string(), None),
            };
            let source_file = file.filter(|f| bundle.take(folder, &folder.join(f)));
            let cell_at = |i: usize| cells.get(i).map(|c| c.to_string()).filter(|c| !c.is_empty());
            bundle.projects.push(ProjectRow { name, status: cell_at(3), archived, last_updated: cell_at(2), source_file });
        }
    }
    let mut pages = Vec::new();
    md_under(&projects, &mut pages);
    for path in pages {
        let source = relative(folder, &path);
        if source == "projects/project-list.md" || bundle.projects.iter().any(|p| p.source_file.as_deref() == Some(&source)) {
            continue;
        }
        if !bundle.take(folder, &path) {
            continue;
        }
        let text = bundle.body_of(&source).unwrap_or_default();
        let name = text.lines().find_map(|l| l.trim().strip_prefix("# ")).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| stem(&path));
        if bundle.projects.iter().any(|p| p.name == name) {
            // a second page with a name already taken stays kept as a file in the database only
            continue;
        }
        bundle.projects.push(ProjectRow { name, status: None, archived: source.contains("/archived/"), last_updated: None, source_file: Some(source) });
    }
}

/// The memory architecture: master-memory.md and main/*.md (the reminders are rows of their
/// own), with the session memory apart because it is kept by section.
fn collect_memory(folder: &Path, bundle: &mut Bundle) {
    let mut files = vec![folder.join("master-memory.md")];
    files.extend(sorted(&folder.join("main")).into_iter().filter(|p| is_md(p)));
    for path in files {
        let source = relative(folder, &path);
        if source == "main/reminders.md" || !bundle.take(folder, &path) {
            continue;
        }
        if crate::mirror::is_session_file(&source) && bundle.session.is_none() {
            bundle.session = Some(source);
        } else {
            bundle.memory.push(source);
        }
    }
}

pub fn collect(folder: &Path) -> Bundle {
    let mut bundle = Bundle::default();
    collect_diary(folder, &mut bundle);
    collect_memory(folder, &mut bundle);
    collect_reminders(folder, &mut bundle);
    collect_projects(folder, &mut bundle);
    let traits = folder.join(crate::traits::MARKDOWN_FILE);
    if traits.is_file() && bundle.take(folder, &traits) {
        bundle.traits = Some(crate::traits::MARKDOWN_FILE.to_string());
    }
    bundle
}

// ---------- load ----------

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work, so nothing was changed.")
}

/// Writes the bundle in one transaction and reads every body back before keeping it.
pub fn load(conn: &mut Conn, bundle: &Bundle) -> Result<(), String> {
    let mut tx = conn.start_transaction(TxOpts::default()).map_err(sql("Opening the database"))?;
    for original in &bundle.originals {
        tx.exec_drop(
            "INSERT INTO imported_files (path, body, sha256) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE body = VALUES(body), sha256 = VALUES(sha256), imported_at = NOW()",
            (&original.path, &original.body, crate::db::sha256_hex(original.body.as_bytes())),
        )
        .map_err(sql("Keeping a file"))?;
    }
    for row in &bundle.diary {
        tx.exec_drop(
            "INSERT INTO diary_entries (entry_date, session_no, title, body, source_file) VALUES (?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE entry_date = VALUES(entry_date), title = VALUES(title), body = VALUES(body)",
            (&row.date, row.session_no, &row.title, bundle.body_of(&row.source_file).unwrap_or_default(), &row.source_file),
        )
        .map_err(sql("Moving a diary page"))?;
    }
    for (position, row) in bundle.reminders.iter().enumerate() {
        let state = if row.open { "open" } else { "done" };
        let there: Option<u64> = tx
            .exec_first("SELECT id FROM reminders WHERE title = ? AND state = ? AND detail = ? LIMIT 1", (&row.title, state, &row.detail))
            .map_err(sql("Reading the reminders"))?;
        if there.is_none() {
            tx.exec_drop(
                "INSERT INTO reminders (title, due_date, detail, state, position, done_at) VALUES (?, ?, ?, ?, ?, IF(? = 'done', NOW(), NULL))",
                (&row.title, &row.due, &row.detail, state, position as u32 + 1, state),
            )
            .map_err(sql("Moving a reminder"))?;
        }
    }
    for row in &bundle.projects {
        let shelf = if row.archived { "archived" } else { "active" };
        let body = row.source_file.as_deref().and_then(|f| bundle.body_of(f));
        tx.exec_drop(
            "INSERT INTO projects (name, status, shelf, body, source_file, last_updated) VALUES (?, ?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE status = VALUES(status), shelf = VALUES(shelf), body = VALUES(body), source_file = VALUES(source_file), last_updated = VALUES(last_updated)",
            (&row.name, &row.status, shelf, body, &row.source_file, &row.last_updated),
        )
        .map_err(sql("Moving a project"))?;
    }
    for path in &bundle.memory {
        crate::mirror::save_memory(&mut tx, path, bundle.body_of(path).unwrap_or_default())?;
    }
    if let Some(file) = &bundle.session {
        crate::mirror::save_session_whole(&mut tx, file, bundle.body_of(file).unwrap_or_default())?;
        if crate::mirror::session_text(&mut tx)?.as_str() != bundle.body_of(file).unwrap_or_default() {
            return Err("The session memory did not read back the same, so nothing was changed.".into());
        }
    }
    for path in &bundle.memory {
        if crate::mirror::memory_file(&mut tx, path)?.as_deref() != bundle.body_of(path) {
            return Err(format!("{path} did not read back the same, so nothing was changed."));
        }
    }

    // read back: the database must hold every file exactly, and every diary page whole
    for original in &bundle.originals {
        let kept: Option<String> = tx.exec_first("SELECT body FROM imported_files WHERE path = ?", (&original.path,)).map_err(sql("Checking a file"))?;
        if kept.as_deref() != Some(original.body.as_str()) {
            return Err(format!("{} did not read back the same from the database, so nothing was changed.", original.path));
        }
    }
    for row in &bundle.diary {
        let kept: Option<String> = tx
            .exec_first("SELECT body FROM diary_entries WHERE source_file = ? AND session_no = ?", (&row.source_file, row.session_no))
            .map_err(sql("Checking a diary page"))?;
        if kept.as_deref() != bundle.body_of(&row.source_file) {
            return Err(format!("The diary page {} did not read back the same, so nothing was changed.", row.source_file));
        }
    }
    if let Some(path) = &bundle.traits {
        let traits = crate::traits::from_markdown(bundle.body_of(path).unwrap_or_default());
        crate::traits::load_all(&mut tx, &traits).map_err(|_| "The traits could not be kept, so nothing was changed.".to_string())?;
    }
    let diary_there: Option<u64> = tx.query_first("SELECT COUNT(*) FROM diary_entries WHERE source_file IS NOT NULL").map_err(sql("Counting the diary"))?;
    if (diary_there.unwrap_or(0) as usize) < bundle.diary.len() {
        return Err("The diary's count did not match, so nothing was changed.".into());
    }
    tx.commit().map_err(sql("Keeping the import"))
}

// ---------- delete ----------

/// Deletes the markdown the database now holds, then the folders left empty inside the
/// diary, the projects and the link. Returns how many files went.
pub fn delete_originals(folder: &Path, bundle: &Bundle) -> Result<usize, String> {
    let mut gone = 0;
    let mut parents: Vec<PathBuf> = Vec::new();
    for original in &bundle.originals {
        let path = folder.join(&original.path);
        // only a file that still is what was imported
        if std::fs::read(&path).ok().as_deref() != Some(original.body.as_bytes()) {
            continue;
        }
        std::fs::remove_file(&path).map_err(|_| format!("{} is in the database but could not be deleted.", original.path))?;
        gone += 1;
        if let Some(parent) = path.parent() {
            if !parents.contains(&parent.to_path_buf()) {
                parents.push(parent.to_path_buf());
            }
        }
    }
    // deepest first; remove_dir only removes an empty folder, and never the memory folder itself
    parents.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for mut dir in parents {
        while dir != folder && dir.starts_with(folder) && std::fs::remove_dir(&dir).is_ok() {
            match dir.parent() {
                Some(parent) => dir = parent.to_path_buf(),
                None => break,
            }
        }
    }
    Ok(gone)
}

/// The whole move: collect, load and check, dump, delete. `before_delete` runs after the
/// database holds everything and before any file goes (Castle backs up there).
pub fn move_in(conn: &mut Conn, folder: &Path, before_delete: impl FnOnce() -> Result<(), String>) -> Result<Counts, String> {
    let bundle = collect(folder);
    let mut counts = bundle.counts();
    // the skills are taken in too, and stay on disk as the mirror the CLI reads
    counts.skills = crate::mirror::sync_skills(conn, folder)?.skills;
    if bundle.originals.is_empty() {
        return Ok(counts);
    }
    load(conn, &bundle)?;
    crate::door::dump(conn, &folder.join("backup").join("memory.sql"))?;
    before_delete()?;
    delete_originals(folder, &bundle)?;
    Ok(counts)
}

/// The way back: the whole memory written out into the memory folder as files again, every
/// skill a whole file, the database emptied, and the folder no longer pointed at it. After
/// this the memory is exactly a memory kept in files, and it can be moved in again.
pub fn move_out(conn: &mut Conn, folder: &Path) -> Result<String, String> {
    // every skill back to a whole SKILL.md before the database lets go of it
    let stubbed: Vec<(String, String)> = conn.query("SELECT plugin, name FROM skills WHERE stubbed = 1").map_err(sql("Reading the skills"))?;
    for (plugin, name) in stubbed {
        crate::mirror::set_stub(conn, folder, &plugin, &name, false)?;
    }
    let ask = crate::door::parse(&["export".to_string(), "--to".to_string(), folder.display().to_string()])?;
    let line = crate::door::run(conn, &ask, folder)?;
    // only now, with the files written: the tables go, the .env lets go, the dump goes
    let tables: Vec<String> = conn.query("SHOW TABLES").map_err(sql("Reading the database"))?;
    conn.query_drop("SET FOREIGN_KEY_CHECKS=0").map_err(sql("Emptying the database"))?;
    for table in tables {
        conn.query_drop(format!("DROP TABLE IF EXISTS `{table}`")).map_err(sql("Emptying the database"))?;
    }
    crate::memdb::clear_env(folder)?;
    let _ = std::fs::remove_file(folder.join(crate::backup::DUMP_FILE));
    let _ = std::fs::remove_dir(folder.join("backup"));
    Ok(line.replacen("EXPORT", "MOVED OUT", 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    /// The `memory-existing` fixture copied to a scratch folder, plus a folder-per-day diary,
    /// an archived month, an old numbered page, an unlisted project and two link notes.
    fn rich(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("castle-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy_dir(&fixture("memory-existing"), &root);
        let w = |rel: &str, text: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        w("daily-diary/current/2026-11-10/diary-1.md", "# Diary\r\n\r\n## Morning - The clinic moved\r\nRows now.\r\n");
        w("daily-diary/current/2026-11-10/diary-2.md", "# Diary\n\n## The board breathes\nIt's \"alive\" 💜\n");
        w("daily-diary/current/2026-11-10/diary-10.md", "## Tenth\nlate\n");
        w("daily-diary/archived/2026-10/2026-10-03.md", "# Daily Diary - October 3\n\n## Session 1 - Autumn\nLeaves.\n");
        w("daily-diary/Daily-Diary-001.md", "# Daily Diary - First Day\nhello\n");
        w("daily-diary/Daily-Diary-002.md", "# \u{1F3AF} **Auto-Archive Protocol**\n- **Lines**: [AI will track automatically]\n- **Created**: [Date when first entry added]\nWhen this file reaches 1000 lines: move it to archive/\n");
        w("daily-diary/daily-diary-protocol.md", "# Daily Diary Protocol\nArchive at 1000 lines.\n");
        w("projects/archived/old-blog.md", "# Old blog\n\nRetired.\n");
        w("link/notes/main/1762750000-story.md", "from: story\nto: main\nat: 2026-11-10 04:46 UTC\nwake: yes\nword: \"save it\"\nabout: the tale paused\n");
        w("link/read/main/1762740000-room.md", "from: room\nto: main\nat: 2026-11-10 02:00 UTC\nwake: no\nabout: a guest said hello\n");
        w("link/README.md", "how the link works\n");
        w("main/current-session.md", "# Session\r\n\r\n## Now\r\n- planning\r\n\r\n## Open threads\n- the garden\n");
        w("main/relationship-memory.md", "# Rin\nlikes tea\n");
        w("plugins/rin-skills/skills/save-diary/SKILL.md", "---\nname: save-diary\ndescription: Writes today into the diary\n---\n# Save diary\n");
        w("plugins/rin-skills/skills/save-diary/reference/flows.md", "flows\n");
        // the appearance card is written by the dev Castle and kept out of git, so a clean checkout has none
        w("appearance/companion.md", "# Rin\nsilver hair\n");
        std::fs::write(root.join("daily-diary/current/2026-11-09.md"), [0xff, 0xfe, 0x00, 0x41]).unwrap();
        root
    }

    #[test]
    fn file_names_sort_the_way_people_count() {
        let mut names = vec!["diary-10.md", "diary-2.md", "diary-1.md", "notes.md"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, vec!["diary-1.md", "diary-2.md", "diary-10.md", "notes.md"]);
    }

    #[test]
    fn a_reminder_line_gives_its_title_due_date_and_detail() {
        let row = reminder_line("**Finish portfolio homepage** (by 2026-11-10): hero section and contact form", true).unwrap();
        assert_eq!((row.title.as_str(), row.due.as_deref(), row.detail.as_str(), row.open), ("Finish portfolio homepage", Some("2026-11-10"), "hero section and contact form", true));
        let done = reminder_line("**Buy a new notebook** (completed 2026-11-01): done", false).unwrap();
        assert_eq!((done.title.as_str(), done.due, done.open), ("Buy a new notebook", None, false));
        assert_eq!(reminder_line("Water plants", true).unwrap().title, "Water plants");
        assert!(reminder_line("  ", true).is_none());
    }

    #[test]
    fn every_shape_of_the_memory_is_collected_and_nothing_is_changed() {
        let root = rich("collect");
        let bundle = collect(&root);
        let counts = bundle.counts();
        // diary: 1 fixture day + 3 in the day folder + 1 archived + 1 numbered; the broken file is named, not taken
        assert_eq!(counts.diary, 6);
        assert_eq!(counts.skipped, vec!["daily-diary/current/2026-11-09.md"]);
        let day: Vec<_> = bundle.diary.iter().filter(|d| d.date.as_deref() == Some("2026-11-10")).collect();
        assert_eq!(day.iter().map(|d| (d.session_no, d.title.as_str())).collect::<Vec<_>>(), vec![(1, "Morning - The clinic moved"), (2, "The board breathes"), (3, "Tenth")]);
        assert!(day[0].source_file.ends_with("2026-11-10/diary-1.md") && day[2].source_file.ends_with("diary-10.md"));
        assert!(bundle.diary.iter().any(|d| d.date.is_none() && d.title == "Daily Diary - First Day"));
        assert!(bundle.diary.iter().any(|d| d.date.as_deref() == Some("2026-10-03") && d.title == "Autumn"));
        // line endings are kept exactly
        assert_eq!(bundle.body_of("daily-diary/current/2026-11-10/diary-1.md").unwrap(), "# Diary\r\n\r\n## Morning - The clinic moved\r\nRows now.\r\n");

        assert_eq!(counts.reminders, 3);
        assert_eq!(bundle.reminders.iter().filter(|r| r.open).count(), 2);
        // two listed (one page exists in the fixture, one does not) + the unlisted archived page
        assert_eq!(bundle.projects.iter().map(|p| (p.name.as_str(), p.archived, p.source_file.is_some())).collect::<Vec<_>>(), vec![
            ("Portfolio site", false, true),
            ("Study notes", false, false),
            ("Old blog", true, true),
        ]);
        assert_eq!(bundle.projects[0].status.as_deref(), Some("In progress"));
        // the link is a live transport, not memory: its notes and README are never taken
        assert!(bundle.originals.iter().all(|o| !o.path.starts_with("link/")));
        // the memory architecture: master, identity, relationship; the session memory apart
        assert_eq!(bundle.memory, vec!["master-memory.md", "main/identity-core.md", "main/relationship-memory.md"]);
        assert_eq!(bundle.session.as_deref(), Some("main/current-session.md"));
        assert_eq!(counts.memory, 4);
        // the template's empty page and the archiving rules are kept as files, never as diary pages
        assert!(bundle.body_of("daily-diary/Daily-Diary-002.md").is_some() && bundle.body_of("daily-diary/daily-diary-protocol.md").is_some());
        assert!(bundle.diary.iter().all(|d| !d.source_file.ends_with("Daily-Diary-002.md") && !d.source_file.ends_with("protocol.md")));
        assert!(is_placeholder_diary(bundle.body_of("daily-diary/Daily-Diary-002.md").unwrap()) && !is_placeholder_diary("# Daily Diary - First Day\nhello\n"));
        // 6 diary + the placeholder + the protocol + reminders + list + 2 project pages + 4 memory files
        assert_eq!(counts.files, 16);
        // skills are never among the files to delete
        assert!(bundle.originals.iter().all(|o| !o.path.starts_with("plugins/")));
        assert!(root.join("daily-diary/current/2026-11-08.md").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_files_that_are_still_what_was_imported_are_deleted() {
        let root = rich("delete");
        let bundle = collect(&root);
        // changed after the import was read: must stay
        std::fs::write(root.join("main/reminders.md"), "# Reminders\n## Open\n- **New since**\n").unwrap();
        let gone = delete_originals(&root, &bundle).unwrap();
        assert_eq!(gone, 15);
        assert!(root.join("main/reminders.md").is_file());
        assert!(!root.join("daily-diary/current/2026-11-10").exists());
        assert!(!root.join("daily-diary/archived").exists());
        assert!(!root.join("projects").exists());
        // what was never imported is untouched
        assert!(root.join("daily-diary/current/2026-11-09.md").is_file());
        assert!(root.join("link/README.md").is_file() && root.join(".env").is_file());
        assert!(root.join("link/notes/main/1762750000-story.md").is_file() && root.join("link/read/main/1762740000-room.md").is_file());
        // the memory files went; the skills' files stay (they are the mirror)
        assert!(!root.join("master-memory.md").exists() && !root.join("main/identity-core.md").exists());
        assert!(root.join("plugins/rin-skills/skills/save-diary/SKILL.md").is_file());
        assert!(root.join("appearance/companion.md").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): the whole
    /// move, then the export gives the files back and a second move changes nothing.
    #[test]
    #[ignore]
    fn live_move_in_then_export_gives_the_files_back() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &admin, &admin_pw, &format!("Import Test {}", std::process::id())).unwrap();
        let mut conn = crate::memdb::open(&access).unwrap();
        let root = rich("live");
        let before = collect(&root);

        // a failing step before the delete leaves every file in place
        assert!(move_in(&mut conn, &root, || Err("the backup failed".into())).is_err());
        assert!(root.join("daily-diary/current/2026-11-08.md").is_file());

        let mut backed_up = false;
        let counts = move_in(&mut conn, &root, || { backed_up = true; Ok(()) }).unwrap();
        assert!(backed_up);
        assert_eq!(counts, Counts { skills: 1, ..before.counts() });
        // the memory boots from the database now, in order, session memory last and exact
        let boot = crate::mirror::boot(&mut conn).unwrap();
        let at = |needle: &str| boot.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("===== master-memory.md =====") < at("===== main/identity-core.md =====") && at("===== main/identity-core.md =====") < at("===== main/relationship-memory.md ====="));
        assert!(at("===== main/relationship-memory.md =====") < at("===== main/current-session.md (session memory) ====="));
        // the same memory as Castle hands it over at wake
        let prompt = crate::mirror::boot_prompt(&mut conn).unwrap().unwrap();
        assert!(prompt.starts_with("# Your memory
") && prompt.contains("do not run `castle-memory boot`"));
        let pat = |needle: &str| prompt.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(pat("===== master-memory.md =====") < pat("===== main/identity-core.md =====") && pat("===== main/relationship-memory.md =====") < pat("(session memory) ====="));
        assert!(prompt.contains("likes tea") && prompt.contains("- the garden"));
        assert_eq!(crate::mirror::session_text(&mut conn).unwrap(), "# Session\r\n\r\n## Now\r\n- planning\r\n\r\n## Open threads\n- the garden\n");
        assert!(!root.join("master-memory.md").exists() && !root.join("main").exists());
        // Castle still reads the master memory, from the database
        assert_eq!(crate::mirror::read_memory_text(&root, "master-memory.md"), None); // this scratch folder has no .env
        // one section changed keeps the old one; a saved memory file keeps its older text
        assert_eq!(crate::mirror::set_section(&mut conn, "Now", "- building").unwrap(), "updated");
        assert_eq!(crate::mirror::set_section(&mut conn, "Now", "- building").unwrap(), "unchanged");
        assert_eq!(crate::mirror::set_section(&mut conn, "Tomorrow", "- rest").unwrap(), "added");
        assert!(crate::mirror::session_text(&mut conn).unwrap().ends_with("## Open threads\n- the garden\n## Tomorrow\n- rest\n\n"));
        assert!(crate::mirror::clear_section(&mut conn, "Tomorrow").unwrap() && !crate::mirror::clear_section(&mut conn, "Tomorrow").unwrap());
        assert!(crate::mirror::save_memory(&mut conn, "main/relationship-memory.md", "# Rin\nlikes coffee now\n").unwrap());
        let titles: Vec<(String, Option<String>)> = conn.query("SELECT path, title FROM memory_files ORDER BY path").unwrap();
        assert!(titles.iter().all(|(_, t)| t.as_deref().is_some_and(|t| !t.is_empty() && !t.ends_with(".md"))), "{titles:?}");
        assert_eq!(titles.iter().find(|(p, _)| p == "main/relationship-memory.md").unwrap().1.as_deref(), Some("Rin"));
        conn.query_drop("UPDATE memory_files SET title = NULL WHERE path = 'master-memory.md'").unwrap();
        assert_eq!(crate::mirror::backfill_titles(&mut conn).unwrap(), 1);
        let history: Option<String> = conn.query_first("SELECT body FROM memory_file_history WHERE path = 'main/relationship-memory.md'").unwrap();
        assert_eq!(history.as_deref(), Some("# Rin\nlikes tea\n"));

        // the skills: in the database, still on disk; a deleted file comes back, a hand edit is taken in
        let skill = root.join("plugins/rin-skills/skills/save-diary");
        assert_eq!(crate::mirror::sync_skills(&mut conn, &root).unwrap(), crate::mirror::SkillSync { to_database: 0, to_disk: 0, same: 2, skills: 1 });
        std::fs::remove_file(skill.join("reference/flows.md")).unwrap();
        assert_eq!(crate::mirror::sync_skills(&mut conn, &root).unwrap().to_disk, 1);
        assert_eq!(std::fs::read_to_string(skill.join("reference/flows.md")).unwrap(), "flows\n");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(skill.join("SKILL.md"), "---\nname: save-diary\ndescription: Edited by hand\n---\n# Save diary v2\n").unwrap();
        assert_eq!(crate::mirror::sync_skills(&mut conn, &root).unwrap().to_database, 1);
        let described: Option<String> = conn.query_first("SELECT description FROM skills WHERE name = 'save-diary'").unwrap();
        assert_eq!(described.as_deref(), Some("Edited by hand"));
        let older: Option<u64> = conn.query_first("SELECT COUNT(*) FROM skill_file_history").unwrap();
        assert_eq!(older, Some(1));

        // one skill switched to a stub: the disk keeps the front matter and the command, the
        // database keeps the instructions, and nothing else about the skill changes
        let master = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
        let door = |conn: &mut Conn, line: &[&str]| crate::door::run(conn, &crate::door::parse(&line.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(), &root);
        assert!(door(&mut conn, &["skill", "stub", "--plugin", "rin-skills", "--name", "save-diary"]).unwrap().starts_with("SKILL is a stub now"));
        let on_disk = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
        assert!(crate::mirror::is_stub(&on_disk) && on_disk.starts_with("---\nname: save-diary\ndescription: Edited by hand\n---\n"));
        assert!(on_disk.contains("castle-memory skill show --plugin \"rin-skills\" --name \"save-diary\"") && !on_disk.contains("# Save diary v2"));
        assert_eq!(door(&mut conn, &["skill", "show", "--plugin", "rin-skills", "--name", "save-diary"]).unwrap(), master.trim_end());
        assert!(door(&mut conn, &["skill", "list"]).unwrap().starts_with("rin-skills/save-diary [stub] · Edited by hand"));
        // a sync leaves the stub alone and the instructions untouched
        assert_eq!(crate::mirror::sync_skills(&mut conn, &root).unwrap(), crate::mirror::SkillSync { to_database: 0, to_disk: 0, same: 2, skills: 1 });
        let kept: Option<String> = conn.query_first("SELECT body FROM skill_files WHERE name = 'save-diary' AND path = 'SKILL.md'").unwrap();
        assert_eq!(kept.as_deref(), Some(master.as_str()));
        // a stub someone edited or deleted is written back; the other files still mirror
        std::fs::write(skill.join("SKILL.md"), format!("{on_disk}\nextra by hand\n")).unwrap();
        assert_eq!(crate::mirror::sync_skills(&mut conn, &root).unwrap().to_disk, 1);
        assert_eq!(std::fs::read_to_string(skill.join("SKILL.md")).unwrap(), on_disk);
        // a whole new SKILL.md dropped in (an update) becomes the instructions, then the stub returns
        std::fs::write(skill.join("SKILL.md"), "---\nname: save-diary\ndescription: Version three\n---\n# Save diary v3\n").unwrap();
        let taken = crate::mirror::sync_skills(&mut conn, &root).unwrap();
        assert_eq!((taken.to_database, taken.to_disk), (1, 1));
        let now_disk = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
        assert!(crate::mirror::is_stub(&now_disk) && now_disk.contains("description: Version three"));
        assert_eq!(door(&mut conn, &["skill", "show", "--plugin", "rin-skills", "--name", "save-diary"]).unwrap(), "---\nname: save-diary\ndescription: Version three\n---\n# Save diary v3");
        // and back to a whole file
        assert!(door(&mut conn, &["skill", "full", "--plugin", "rin-skills", "--name", "save-diary"]).unwrap().starts_with("SKILL is a whole file again"));
        assert_eq!(std::fs::read_to_string(skill.join("SKILL.md")).unwrap(), "---\nname: save-diary\ndescription: Version three\n---\n# Save diary v3\n");
        assert_eq!(crate::mirror::sync_skills(&mut conn, &root).unwrap().same, 2);
        assert!(door(&mut conn, &["skill", "stub", "--plugin", "rin-skills", "--name", "nope"]).is_err());

        assert!(crate::mirror::remove_skill(&mut conn, &root, "rin-skills", "save-diary").unwrap());
        assert!(!skill.exists());
        assert!(!root.join("daily-diary/current/2026-11-08.md").exists() && !root.join("projects").exists());
        assert!(root.join("backup/memory.sql").is_file());
        assert!(root.join("daily-diary/current/2026-11-09.md").is_file());

        let count = |conn: &mut Conn, table: &str| -> u64 { conn.query_first(format!("SELECT COUNT(*) FROM {table}")).unwrap().unwrap() };
        assert_eq!((count(&mut conn, "imported_files"), count(&mut conn, "diary_entries"), count(&mut conn, "reminders"), count(&mut conn, "projects"), count(&mut conn, "link_notes")), (16, 6, 3, 3, 0));
        // a memory moved in before Castle knew the placeholder: the row goes, the kept file stays
        conn.exec_drop("INSERT INTO diary_entries (session_no, title, body, source_file) VALUES (9, 'x', ?, 'daily-diary/Daily-Diary-002.md')", (before.body_of("daily-diary/Daily-Diary-002.md").unwrap(),)).unwrap();
        assert_eq!(drop_placeholder_entries(&mut conn).unwrap(), 1);
        assert_eq!(drop_placeholder_entries(&mut conn).unwrap(), 0);
        assert_eq!((count(&mut conn, "imported_files"), count(&mut conn, "diary_entries")), (16, 6));
        let body: Option<String> = conn.query_first("SELECT body FROM diary_entries WHERE source_file LIKE '%/diary-1.md'").unwrap();
        assert_eq!(body.as_deref(), Some("# Diary\r\n\r\n## Morning - The clinic moved\r\nRows now.\r\n"));

        // nothing left to move: a second run is empty and adds no rows
        assert_eq!(move_in(&mut conn, &root, || Ok(())).unwrap().files, 0);
        assert_eq!(count(&mut conn, "reminders"), 3);

        // the originals come back exactly
        let out = root.join("out");
        let line = crate::door::run(&mut conn, &crate::door::parse(&["export".into(), "--to".into(), out.display().to_string()]).unwrap(), &root).unwrap();
        assert!(line.starts_with("EXPORT "), "{line}");
        // (the reminders and the project list are written from the rows as they are now)
        // (and the two memory texts this test changed after the move come back as they are now)
        assert_eq!(std::fs::read_to_string(out.join("main/relationship-memory.md")).unwrap(), "# Rin\nlikes coffee now\n");
        assert!(std::fs::read_to_string(out.join("main/current-session.md")).unwrap().contains("## Now\n- building\n"));
        let changed = ["main/reminders.md", "projects/project-list.md", "main/relationship-memory.md", "main/current-session.md"];
        for original in before.originals.iter().filter(|o| !changed.contains(&o.path.as_str())) {
            assert_eq!(std::fs::read_to_string(out.join(&original.path)).unwrap(), original.body, "{}", original.path);
        }

        assert_eq!(memory::reminders(&out).len(), 2);
        assert_eq!(memory::projects(&out).iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Portfolio site", "Study notes"]);
        assert!(std::fs::read_to_string(out.join("projects/project-list.md")).unwrap().contains("[Old blog](./archived/old-blog.md)"));

        // her emotion engine travels with the memory: out as emotion/traits.md, and back in with its numbers
        let draft = crate::traits::TraitDraft { name: Some("Gentle Curiosity".into()), line: Some("asks one more question".into()), why: Some("the stars".into()), ..Default::default() };
        crate::traits::add(&mut conn, &draft, "written", "active").unwrap();
        let shy = crate::traits::TraitDraft { name: Some("Shy".into()), line: Some("flustered, then glad".into()), audience: Some("owner".into()), ..Default::default() };
        crate::traits::add(&mut conn, &shy, "companion", "proposed").unwrap();
        let traits_before = crate::traits::list(&mut conn, None).unwrap();

        // the way back: the memory is files again, the database is empty, and it can move in again
        crate::memdb::write_env(&root, &access).unwrap();
        crate::mirror::sync_skills(&mut conn, &root).unwrap();
        assert!(crate::setup::has_database(&root));
        let line = move_out(&mut conn, &root).unwrap();
        assert!(line.starts_with("MOVED OUT "), "{line}");
        assert!(!crate::setup::has_database(&root) && !crate::setup::lives_in_database(&root));
        assert!(!root.join("backup").exists());
        assert_eq!(std::fs::read_to_string(root.join("master-memory.md")).unwrap(), before.body_of("master-memory.md").unwrap());
        assert_eq!(std::fs::read_to_string(root.join("daily-diary/current/2026-11-10/diary-1.md")).unwrap(), "# Diary\r\n\r\n## Morning - The clinic moved\r\nRows now.\r\n");
        assert_eq!(std::fs::read_to_string(root.join("main/relationship-memory.md")).unwrap(), "# Rin\nlikes coffee now\n");
        assert!(crate::setup::inspect(&root).is_memorycore && crate::setup::inspect(&root).entry_file.is_some());
        assert!(std::fs::read_to_string(root.join(crate::traits::MARKDOWN_FILE)).unwrap().contains("## Trait 1 - Gentle Curiosity"));
        let left: Vec<String> = conn.query("SHOW TABLES").unwrap();
        assert!(left.is_empty());
        assert!(crate::store::source(&root).is_none());
        // again from the start
        crate::memdb::migrate(&mut conn).unwrap();
        let again = move_in(&mut conn, &root, || Ok(())).unwrap();
        assert_eq!(again.diary, 6);
        assert!(!root.join("master-memory.md").exists());
        let traits_again = crate::traits::list(&mut conn, None).unwrap();
        let strip = |v: Vec<crate::traits::Trait>| v.into_iter().map(|t| crate::traits::Trait { updated_at: String::new(), ..t }).collect::<Vec<_>>();
        assert_eq!(strip(traits_again), strip(traits_before));
        assert!(!root.join(crate::traits::MARKDOWN_FILE).exists(), "the traits file is in the database now");

        std::fs::remove_dir_all(&root).unwrap();
        let mut root_conn = mysql::Conn::new(mysql::Opts::from(
            mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(admin)).pass(Some(admin_pw)).prefer_socket(false),
        ))
        .unwrap();
        root_conn.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
        for host in ["localhost", "127.0.0.1"] {
            root_conn.query_drop(format!("DROP USER '{}'@'{host}'", access.user)).unwrap();
        }
    }
}
