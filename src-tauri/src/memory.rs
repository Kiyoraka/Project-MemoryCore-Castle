//! Reading the member's MemoryCore folder: which features are installed. MemoryCore features
//! create their own folders or skills when installed, so the files themselves are the answer
//! (the repo's Feature/ folders say nothing - installs often delete them).

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The MemoryCore features Castle knows how to show, as found in one folder.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Features {
    /// daily-diary/ (MemoryCore's diary; always there in a fresh MemoryCore)
    pub diary: bool,
    /// projects/ (LRU Projects)
    pub projects: bool,
    /// library/ (Library)
    pub library: bool,
    /// a save-memory skill from the Skill Plugin System
    pub save_memory: bool,
    /// main/reminders.md (Reminders)
    pub reminders: bool,
    /// adventures/ (Interactive Story; Castle's STORY room creates it when missing)
    pub adventures: bool,
    /// music/ (Song Creation)
    pub music: bool,
    /// media-generation/ (Image / Video Generation)
    pub media: bool,
    /// appearance/ (Castle's APPEARANCE room)
    pub appearance: bool,
}

/// Whether any plugin in `plugins/` carries a skill folder named `skill` with a SKILL.md.
fn has_skill(folder: &Path, skill: &str) -> bool {
    let Ok(plugins) = std::fs::read_dir(folder.join("plugins")) else {
        return false;
    };
    plugins.flatten().any(|plugin| plugin.path().join("skills").join(skill).join("SKILL.md").is_file())
}

pub fn features(folder: &Path) -> Features {
    let dir = |name: &str| folder.join(name).is_dir();
    Features {
        diary: dir("daily-diary"),
        projects: dir("projects"),
        library: dir("library"),
        save_memory: has_skill(folder, "save-memory"),
        reminders: folder.join("main").join("reminders.md").is_file(),
        adventures: dir("adventures"),
        music: dir("music"),
        media: dir("media-generation"),
        appearance: dir("appearance"),
    }
}

// ---------- What the HOME widgets read ----------

/// One open reminder from main/reminders.md: "- **Title** (by 2026-11-10): what".
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub title: String,
    pub due: Option<String>,
    pub detail: String,
}

/// The open reminders, in the file's order (MemoryCore's Reminders feature).
pub fn reminders(folder: &Path) -> Vec<Reminder> {
    let Ok(text) = std::fs::read_to_string(folder.join("main").join("reminders.md")) else {
        return Vec::new();
    };
    let mut open = false;
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            open = heading.trim().eq_ignore_ascii_case("open");
            continue;
        }
        if !open {
            continue;
        }
        let Some(item) = trimmed.strip_prefix("- ") else { continue };
        let item = item.trim();
        let (head, detail) = item.split_once(':').map(|(h, d)| (h.trim(), d.trim())).unwrap_or((item, ""));
        let due = head.split_once("(by ").and_then(|(_, rest)| rest.split(')').next()).map(|d| d.trim().to_string());
        let title = head.split(" (by ").next().unwrap_or(head).trim().trim_matches('*').trim().to_string();
        if !title.is_empty() {
            found.push(Reminder { title, due, detail: detail.to_string() });
        }
    }
    found
}

/// One project row from projects/project-list.md (LRU Projects).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    /// the project file, relative to the memory folder, when the row links one
    pub file: Option<String>,
    pub updated: Option<String>,
    pub status: Option<String>,
}

/// The active projects, most recent first (the list's own order: | Pos | Project | Last Updated | Status |).
pub fn projects(folder: &Path) -> Vec<Project> {
    let Ok(text) = std::fs::read_to_string(folder.join("projects").join("project-list.md")) else {
        return Vec::new();
    };
    let mut in_active = true;
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            in_active = heading.to_lowercase().contains("active");
            continue;
        }
        if !in_active || !trimmed.starts_with('|') || trimmed.contains("---") {
            continue;
        }
        let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 2 || cells[0].eq_ignore_ascii_case("pos") || cells[1].eq_ignore_ascii_case("project") {
            continue;
        }
        let cell = cells[1];
        // "[Portfolio site](./active/portfolio-site.md)" or plain text
        let (name, file) = match (cell.find('['), cell.find("]("), cell.rfind(')')) {
            (Some(a), Some(b), Some(c)) if a < b && b < c => {
                let link = cell[b + 2..c].trim_start_matches("./");
                (cell[a + 1..b].to_string(), Some(format!("projects/{link}")))
            }
            _ => (cell.to_string(), None),
        };
        found.push(Project {
            name,
            file,
            updated: cells.get(2).filter(|c| !c.is_empty()).map(|c| c.to_string()),
            status: cells.get(3).filter(|c| !c.is_empty()).map(|c| c.to_string()),
        });
    }
    found
}

/// The newest diary entry, in either diary format.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiaryEntry {
    pub title: String,
    /// "2026-11-08" for the current format; None for the older numbered files
    pub date: Option<String>,
    /// the file, relative to the memory folder
    pub file: String,
}

/// Diary files: daily-diary/current/YYYY-MM-DD.md (Save Diary) and, for memories that never
/// installed it, the older daily-diary/Daily-Diary-NNN.md. Sorted oldest first.
pub fn diary_files(folder: &Path) -> Vec<PathBuf> {
    let diary = folder.join("daily-diary");
    // a day is one file (2026-11-08.md) or a folder of pages (2026-11-10/diary-1.md, diary-2.md)
    let mut dated: Vec<PathBuf> = Vec::new();
    let mut days: Vec<PathBuf> = std::fs::read_dir(diary.join("current")).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    days.sort();
    for day in days {
        if is_dated_md(&day) {
            dated.push(day);
        } else if day.is_dir() && day.file_name().and_then(|n| n.to_str()).is_some_and(crate::door::is_date) {
            let mut pages = md_files(&day);
            pages.sort_by_key(|p| crate::import::natural_key(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
            dated.extend(pages);
        }
    }
    let mut numbered: Vec<PathBuf> = std::fs::read_dir(&diary)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("Daily-Diary-") && n.ends_with(".md")))
                .collect()
        })
        .unwrap_or_default();
    numbered.sort();
    numbered.extend(dated);
    numbered
}

/// The day a diary page belongs to: its own name (2026-11-08.md) or its folder's (2026-11-10/diary-1.md).
fn date_of(path: &Path) -> Option<String> {
    if is_dated_md(path) {
        return path.file_stem().and_then(|s| s.to_str()).map(str::to_string);
    }
    path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).filter(|n| crate::door::is_date(n)).map(str::to_string)
}

fn is_dated_md(path: &Path) -> bool {
    let Some(stem) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".md")) else {
        return false;
    };
    stem.len() == 10 && stem.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
}

/// The entry's title: its last "## " heading (the newest session), else its "# " heading.
pub fn diary_title(text: &str) -> Option<String> {
    let last_session = text.lines().filter_map(|l| l.trim().strip_prefix("## ")).last();
    let title = last_session.or_else(|| text.lines().find_map(|l| l.trim().strip_prefix("# ")))?.trim();
    // "Session 1 - The day we planned the garden" -> "The day we planned the garden"
    let title = title
        .split_once(" - ")
        .filter(|(head, _)| head.to_lowercase().starts_with("session"))
        .map(|(_, rest)| rest)
        .unwrap_or(title);
    // "🎯 **Auto-Archive Protocol**" -> "Auto-Archive Protocol": no emphasis marks, no decoration in front
    let title = title.replace("**", "").replace("__", "");
    let title = title.trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_ascii_punctuation()).trim();
    (!title.is_empty()).then(|| title.to_string())
}

pub fn newest_diary(folder: &Path) -> Option<DiaryEntry> {
    let path = diary_files(folder).pop()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let name = path.file_name()?.to_str()?.to_string();
    let date = date_of(&path);
    let file = path.strip_prefix(folder).ok()?.display().to_string().replace('\\', "/");
    Some(DiaryEntry { title: diary_title(&text).unwrap_or_else(|| name.clone()), date, file })
}

/// How the memory has grown: memory files, days since the first diary, skills.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Growth {
    pub memories: usize,
    pub days: Option<u64>,
    pub skills: usize,
    pub diary_entries: usize,
}

/// Markdown files that are the companion's memory - not MemoryCore's feature installers, not the
/// git folder, not node_modules, not the plugins.
fn count_memory_files(folder: &Path, depth: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    entries
        .flatten()
        .map(|e| e.path())
        .map(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if p.is_dir() {
                if depth >= 6 || name.starts_with('.') || ["Feature", "node_modules", "plugins"].contains(&name.as_str()) {
                    0
                } else {
                    count_memory_files(&p, depth + 1)
                }
            } else if name.ends_with(".md") && !["README.md", "LICENSE.md", "SECURITY.md", "CLAUDE.md", "AGENTS.md"].contains(&name.as_str()) {
                1
            } else {
                0
            }
        })
        .sum()
}

fn skills(folder: &Path) -> usize {
    std::fs::read_dir(folder.join("plugins"))
        .map(|plugins| {
            plugins
                .flatten()
                .flat_map(|p| std::fs::read_dir(p.path().join("skills")).into_iter().flatten().flatten())
                .filter(|s| s.path().join("SKILL.md").is_file())
                .count()
        })
        .unwrap_or(0)
}

/// Days from `first` (YYYY-MM-DD) to `today` (YYYY-MM-DD), counting the first day as day 1.
pub fn days_between(first: &str, today: &str) -> Option<u64> {
    fn day_number(date: &str) -> Option<i64> {
        let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
        let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
        // days since 1970-01-01 (civil calendar)
        let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * m + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(era * 146_097 + doe - 719_468)
    }
    let span = day_number(today)? - day_number(first)?;
    (span >= 0).then(|| span as u64 + 1)
}

pub fn growth(folder: &Path, today: &str) -> Growth {
    let diary = diary_files(folder);
    let first_dated = diary.iter().find_map(|p| date_of(p));
    Growth {
        memories: count_memory_files(folder, 0),
        days: first_dated.and_then(|first| days_between(&first, today)),
        skills: skills(folder),
        diary_entries: diary.len(),
    }
}

/// Everything HOME shows, read in one go.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HomeData {
    pub features: Features,
    pub reminders: Vec<Reminder>,
    pub projects: Vec<Project>,
    pub diary: Option<DiaryEntry>,
    pub growth: Growth,
}

pub fn home(folder: &Path, today: &str) -> HomeData {
    // a memory that lives in the database is read from its rows
    if let Some(mut conn) = crate::store::source(folder) {
        return crate::store::home(&mut conn, folder, today);
    }
    HomeData {
        features: features(folder),
        reminders: reminders(folder),
        projects: projects(folder),
        diary: newest_diary(folder),
        growth: growth(folder, today),
    }
}

/// HOME's widgets. `today` is the page's local date (YYYY-MM-DD), so days count in the member's time.
#[tauri::command]
pub fn memory_home(today: String, store: tauri::State<crate::settings::SettingsStore>) -> Option<HomeData> {
    store.get().memory_folder.map(|folder| home(Path::new(&folder), &today))
}

// ---------- The DIARY, PROJECTS and LIBRARY rooms ----------

/// One readable page in a room's list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub title: String,
    /// relative to the memory folder, forward slashes
    pub file: String,
    pub date: Option<String>,
}

/// A group of pages: a diary month, "Active" / "Archived" projects, a library section.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub name: String,
    pub pages: Vec<Page>,
}

fn relative(folder: &Path, path: &Path) -> String {
    path.strip_prefix(folder).map(|p| p.display().to_string().replace('\\', "/")).unwrap_or_default()
}

/// A page's title: its first "# " heading, else the file name in words.
fn page_title(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("# ").map(|h| h.trim().to_string())))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().replace(['-', '_'], " ")).unwrap_or_default())
}

fn md_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| e.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("md")).collect())
        .unwrap_or_default();
    files.sort();
    files
}

/// The diary, newest first, grouped by month ("2026-11"); the older numbered files under "Earlier".
/// Archived months (daily-diary/archived/YYYY-MM/) are read too.
pub fn diary_shelves(folder: &Path) -> Vec<Shelf> {
    let mut pages: Vec<(String, Page)> = Vec::new();
    for path in diary_files(folder) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let date = date_of(&path);
        let month = date.as_deref().map(|d| d[..7].to_string()).unwrap_or_else(|| "Earlier".into());
        pages.push((month, Page { title: diary_title(&text).unwrap_or_else(|| page_title(&path)), file: relative(folder, &path), date }));
    }
    if let Ok(months) = std::fs::read_dir(folder.join("daily-diary").join("archived")) {
        for month in months.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            let name = month.file_name().unwrap_or_default().to_string_lossy().to_string();
            for path in md_files(&month) {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let date = is_dated_md(&path).then(|| path.file_stem().unwrap_or_default().to_string_lossy().to_string());
                pages.push((name.clone(), Page { title: diary_title(&text).unwrap_or_else(|| page_title(&path)), file: relative(folder, &path), date }));
            }
        }
    }
    // newest first: dated pages by date, "Earlier" last
    pages.sort_by(|a, b| b.1.date.cmp(&a.1.date).then(b.1.file.cmp(&a.1.file)));
    let mut shelves: Vec<Shelf> = Vec::new();
    for (month, page) in pages {
        match shelves.iter_mut().find(|s| s.name == month) {
            Some(shelf) => shelf.pages.push(page),
            None => shelves.push(Shelf { name: month, pages: vec![page] }),
        }
    }
    shelves
}

/// Active and archived projects (LRU Projects keeps them in projects/active and projects/archived).
pub fn project_shelves(folder: &Path) -> Vec<Shelf> {
    [("Active", "active"), ("Archived", "archived")]
        .iter()
        .map(|(name, dir)| Shelf {
            name: name.to_string(),
            pages: md_files(&folder.join("projects").join(dir))
                .iter()
                .map(|p| Page { title: page_title(p), file: relative(folder, p), date: None })
                .collect(),
        })
        .filter(|s| !s.pages.is_empty())
        .collect()
}

/// Where the Library lives: a backticked folder named on a master-memory line that mentions the
/// library (the member may have chosen another path at install), else library/, else MemoryCore's
/// own library-items/.
pub fn library_folder(folder: &Path) -> Option<PathBuf> {
    let master = crate::mirror::read_memory_text(folder, "master-memory.md").unwrap_or_default();
    // lines that mention the library, and every line of a section whose heading does
    let mut in_section = false;
    let lines: Vec<&str> = master
        .lines()
        .filter(|l| {
            if l.trim_start().starts_with('#') {
                in_section = l.to_lowercase().contains("library");
            }
            in_section || l.to_lowercase().contains("library")
        })
        .collect();
    for line in lines {
        let mut parts = line.split('`');
        while let (Some(_), Some(inside)) = (parts.next(), parts.next()) {
            let candidate = folder.join(inside.trim().trim_end_matches(['/', '\\']));
            if !inside.trim().is_empty() && candidate.is_dir() && candidate.starts_with(folder) {
                return Some(candidate);
            }
        }
    }
    ["library", "library-items"].iter().map(|n| folder.join(n)).find(|p| p.is_dir())
}

/// The library's sections (its sub-folders) and their pages.
pub fn library_shelves(folder: &Path) -> Vec<Shelf> {
    let Some(library) = library_folder(folder) else { return Vec::new() };
    let mut sections: Vec<PathBuf> = std::fs::read_dir(&library)
        .map(|e| e.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    sections.sort();
    let mut shelves: Vec<Shelf> = sections
        .iter()
        .map(|dir| Shelf {
            name: dir.file_name().unwrap_or_default().to_string_lossy().replace(['-', '_'], " "),
            pages: md_files(dir).iter().map(|p| Page { title: page_title(p), file: relative(folder, p), date: None }).collect(),
        })
        .filter(|s| !s.pages.is_empty())
        .collect();
    let loose = md_files(&library);
    if !loose.is_empty() {
        shelves.insert(0, Shelf { name: "General".into(), pages: loose.iter().map(|p| Page { title: page_title(p), file: relative(folder, p), date: None }).collect() });
    }
    shelves
}

/// A room's shelves: "diary", "projects" or "library".
#[tauri::command]
pub fn memory_shelves(room: String, store: tauri::State<crate::settings::SettingsStore>) -> Vec<Shelf> {
    if !store.get().room_on(&room) {
        return Vec::new();
    }
    let Some(folder) = store.get().memory_folder.map(PathBuf::from) else { return Vec::new() };
    shelves_of(&folder, &room)
}

/// A room's shelves, from the database when the memory lives there, else from its files.
pub fn shelves_of(folder: &Path, room: &str) -> Vec<Shelf> {
    if matches!(room, "diary" | "projects") {
        if let Some(mut conn) = crate::store::source(folder) {
            return if room == "diary" { crate::store::diary_shelves(&mut conn) } else { crate::store::project_shelves(&mut conn) };
        }
    }
    match room {
        "diary" => diary_shelves(folder),
        "projects" => project_shelves(folder),
        "library" => library_shelves(folder),
        _ => Vec::new(),
    }
}

/// One page for the reader: a row of the database (`db/...`), or a .md file inside the memory
/// folder, at most 2 MB.
pub fn read_page(folder: &Path, file: &str) -> Result<String, String> {
    if file.starts_with(crate::store::PAGE_PREFIX) {
        let mut conn = crate::memdb::open_for(folder)?;
        return crate::store::read(&mut conn, file);
    }
    let path = crate::files::fenced_in(folder, file)?;
    if path.extension().and_then(|x| x.to_str()) != Some("md") {
        return Err("Only memory pages (.md) open here.".into());
    }
    if path.metadata().map(|m| m.len()).unwrap_or(0) > 2 * 1024 * 1024 {
        return Err("This page is too large to show here.".into());
    }
    std::fs::read_to_string(&path).map_err(|_| "The page could not be read.".to_string())
}

/// One markdown page of the memory, for the room's reader - only a .md file inside the memory
/// folder, at most 2 MB.
#[tauri::command]
pub fn memory_read(file: String, store: tauri::State<crate::settings::SettingsStore>) -> Result<String, String> {
    // the reader of DIARY, PROJECTS, LIBRARY and STORY: refused only when all four are off
    let s = store.get();
    if !["diary", "projects", "library", "story"].iter().any(|r| s.room_on(r)) {
        return Err(crate::rooms::off_line("diary"));
    }
    let folder = s.memory_folder.map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    read_page(&folder, &file)
}

// ---------- the recent diary (the emotion engine's Scan my diary) ----------

/// Newest-first entries cut to size: each at most `each` characters, the whole at most `total`
/// (an entry that no longer fits is left out). Pure, tested.
pub fn cap_entries(entries: Vec<(String, String)>, each: usize, total: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut used = 0;
    for (title, body) in entries {
        let body: String = body.trim().chars().take(each).collect();
        let size = title.chars().count() + body.chars().count();
        if body.is_empty() || used + size > total {
            continue;
        }
        used += size;
        out.push((title, body));
    }
    out
}

/// The newest `n` diary entries with their text, newest first, cut to size: from the database
/// when the memory lives there, else from the diary files.
pub fn recent_diary(folder: &Path, n: usize, each: usize, total: usize) -> Vec<(String, String)> {
    let entries: Vec<(String, String)> = match crate::store::source(folder) {
        Some(mut conn) => {
            use mysql::prelude::Queryable;
            conn.exec::<(Option<String>, String, String), _, _>(
                "SELECT DATE_FORMAT(entry_date, '%Y-%m-%d'), title, body FROM diary_entries ORDER BY entry_date IS NULL, entry_date DESC, session_no DESC, id DESC LIMIT ?",
                (n as u64,),
            )
            .unwrap_or_default()
            .into_iter()
            .map(|(date, title, body)| (date.map(|d| format!("{d} - {title}")).unwrap_or(title), body))
            .collect()
        }
        None => diary_files(folder)
            .into_iter()
            .rev()
            .take(n)
            .filter_map(|p| {
                let body = std::fs::read_to_string(&p).ok()?;
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                Some((name, body))
            })
            .collect(),
    };
    cap_entries(entries, each, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recent_diary_is_cut_to_size() {
        let entries = vec![("Today".to_string(), format!("  {}  ", "a".repeat(50))), ("Empty".into(), "   ".into()), ("Long ago".into(), "b".repeat(50))];
        let cut = cap_entries(entries.clone(), 10, 1000);
        assert_eq!(cut, vec![("Today".to_string(), "a".repeat(10)), ("Long ago".to_string(), "b".repeat(10))]);
        // the whole is capped: the newest is kept, the rest left out once it no longer fits
        assert_eq!(cap_entries(entries, 50, 60).len(), 1);
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
    }

    #[test]
    fn the_existing_memory_fills_every_widget() {
        let home = home(&fixture("memory-existing"), "2026-11-09");
        assert_eq!(home.reminders.len(), 2, "completed reminders are left out");
        assert_eq!(home.reminders[0].title, "Finish portfolio homepage");
        assert_eq!(home.reminders[0].due.as_deref(), Some("2026-11-10"));
        assert_eq!(home.reminders[1].due, None);
        assert_eq!(home.projects.len(), 2);
        assert_eq!(home.projects[0].name, "Portfolio site");
        assert_eq!(home.projects[0].file.as_deref(), Some("projects/active/portfolio-site.md"));
        let diary = home.diary.unwrap();
        assert_eq!(diary.title, "The day we planned the garden");
        assert_eq!(diary.date.as_deref(), Some("2026-11-08"));
        assert_eq!(home.growth.days, Some(2));
        assert!(home.growth.memories >= 6);
        assert!(home.features.projects && home.features.reminders);
    }

    #[test]
    fn the_fresh_memory_shows_empty_widgets_without_errors() {
        let home = home(&fixture("memory-fresh"), "2026-11-09");
        assert!(home.reminders.is_empty() && home.projects.is_empty() && home.diary.is_none());
        assert_eq!(home.growth.days, None);
    }

    #[test]
    fn a_day_kept_as_a_folder_of_pages_is_read_in_page_order() {
        let root = std::env::temp_dir().join(format!("castle-day-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let day = root.join("daily-diary").join("current").join("2026-11-10");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(root.join("daily-diary").join("current").join("2026-11-08.md"), "## Session 1 - Before\nx\n").unwrap();
        std::fs::write(day.join("diary-1.md"), "## The clinic moved\nx\n").unwrap();
        std::fs::write(day.join("diary-2.md"), "## The board breathes\nx\n").unwrap();
        std::fs::write(day.join("diary-10.md"), "## The tenth page\nx\n").unwrap();
        // a folder that is not a date is not a day
        std::fs::create_dir_all(root.join("daily-diary").join("current").join("notes")).unwrap();
        std::fs::write(root.join("daily-diary").join("current").join("notes").join("x.md"), "## Not a day\n").unwrap();
        let names: Vec<String> = diary_files(&root).iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["2026-11-08.md", "diary-1.md", "diary-2.md", "diary-10.md"]);
        let newest = newest_diary(&root).unwrap();
        assert_eq!((newest.title.as_str(), newest.date.as_deref(), newest.file.as_str()), ("The tenth page", Some("2026-11-10"), "daily-diary/current/2026-11-10/diary-10.md"));
        let shelves = diary_shelves(&root);
        assert_eq!(shelves[0].name, "2026-11");
        assert_eq!(shelves[0].pages.len(), 4);
        assert_eq!(growth(&root, "2026-11-12").days, Some(5));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_diary_title_loses_its_emphasis_marks_and_decoration() {
        assert_eq!(diary_title("# \u{1F3AF} **Auto-Archive Protocol**\n").as_deref(), Some("Auto-Archive Protocol"));
        assert_eq!(diary_title("## Session 2 - **The** garden\n").as_deref(), Some("The garden"));
        assert_eq!(diary_title("## \"Quoted\" stays\n").as_deref(), Some("\"Quoted\" stays"));
        assert_eq!(diary_title("## \u{1F49C}\n"), None);
    }

    #[test]
    fn the_older_numbered_diary_format_is_read_too() {
        let root = std::env::temp_dir().join(format!("castle-old-diary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("daily-diary")).unwrap();
        std::fs::write(root.join("daily-diary").join("Daily-Diary-001.md"), "# Daily Diary - First Day\nhello\n").unwrap();
        let entry = newest_diary(&root).unwrap();
        assert_eq!(entry.title, "Daily Diary - First Day");
        assert_eq!(entry.date, None);
        assert_eq!(entry.file, "daily-diary/Daily-Diary-001.md");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_rooms_list_the_existing_memory() {
        let root = fixture("memory-existing");
        let diary = diary_shelves(&root);
        assert_eq!(diary[0].name, "2026-11");
        assert_eq!(diary[0].pages[0].title, "The day we planned the garden");
        let projects = project_shelves(&root);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "Active");
        assert_eq!(projects[0].pages[0].file, "projects/active/portfolio-site.md");
        assert!(library_shelves(&root).is_empty());
        assert!(diary_shelves(&fixture("memory-fresh")).is_empty());
    }

    #[test]
    fn a_library_moved_to_another_folder_is_found() {
        let root = std::env::temp_dir().join(format!("castle-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("knowledge").join("security")).unwrap();
        std::fs::write(root.join("master-memory.md"), "### Library\n- **Location**: `knowledge/`\n").unwrap();
        std::fs::write(root.join("knowledge").join("security").join("headers.md"), "# Security headers\n").unwrap();
        assert_eq!(library_folder(&root), Some(root.join("knowledge")));
        let shelves = library_shelves(&root);
        assert_eq!(shelves[0].name, "security");
        assert_eq!(shelves[0].pages[0].title, "Security headers");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn days_are_counted_across_months_and_years() {
        assert_eq!(days_between("2026-11-08", "2026-11-09"), Some(2));
        assert_eq!(days_between("2025-12-31", "2026-01-01"), Some(2));
        assert_eq!(days_between("2024-02-28", "2024-03-01"), Some(3), "a leap day counts");
        assert_eq!(days_between("2026-11-10", "2026-11-09"), None);
    }

    #[test]
    fn features_are_found_by_what_they_create() {
        let root = std::env::temp_dir().join(format!("castle-features-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("daily-diary")).unwrap();
        assert_eq!(features(&root), Features { diary: true, ..Features::default() });

        std::fs::create_dir_all(root.join("projects").join("active")).unwrap();
        std::fs::create_dir_all(root.join("main")).unwrap();
        std::fs::write(root.join("main").join("reminders.md"), "## Open\n").unwrap();
        let skill = root.join("plugins").join("rin-skills").join("skills").join("save-memory");
        std::fs::create_dir_all(&skill).unwrap();
        // a skill folder without its SKILL.md is not installed
        assert!(!features(&root).save_memory);
        std::fs::write(skill.join("SKILL.md"), "---\nname: save-memory\n---\n").unwrap();
        let found = features(&root);
        assert!(found.projects && found.reminders && found.save_memory);
        assert!(!found.library && !found.music);
        let _ = std::fs::remove_dir_all(root);
    }
}
