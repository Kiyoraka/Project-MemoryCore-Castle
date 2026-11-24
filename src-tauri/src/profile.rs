//! MY PROFILE: the member's own personality, read by the member's own CLI from the member's own
//! diary, one reading per finished month, kept in `profile_readings` / `profile_scores` /
//! `profile_notes` (schema 009). The month in progress is never read. Nothing here ever reaches
//! the chat room: no profile text goes into the room's rules, card, manner or a wake.
//!
//! ```text
//!  Scan my diary -> the finished months not read yet -> one oneshot run each (the month's pages)
//!                -> <reading>{json}</reading> -> check_reading -> save (one transaction a month)
//! ```

use crate::settings::SettingsStore;
use mysql::prelude::Queryable;
use mysql::{Conn, TxOpts};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const RULES: &str = include_str!("../resources/profile-rules.md");
/// A whole month of pages takes longer than a handful.
const READ_LIMIT: Duration = Duration::from_secs(240);
/// A month with fewer pages is too thin to read.
pub const MIN_ENTRIES: u32 = 5;
const ENTRY_EACH: usize = 3000;
const MONTH_TOTAL: usize = 60000;
pub const PROGRESS_EVENT: &str = "profile-progress";
pub const MARKDOWN_DIR: &str = "profile";
/// Export and Move back to files carry the readings here.
pub const EXPORT_FILE: &str = "profile/readings.json";

/// The words a reading may use. The three tiles are fixed so every month compares like with like;
/// a card may score fewer than five, never a word that is not here.
pub const HEADLINE: [&str; 3] = ["Growth", "Focus", "Wellbeing"];
pub const THINK: [&str; 5] = ["Analytical", "Strategic", "Systems", "Creative", "Practical"];
pub const VALUES: [&str; 5] = ["Integrity", "Knowledge", "Independence", "Impact", "Kindness"];
pub const SHADOW: [&str; 5] = ["Overthinking", "Self-criticism", "Control", "Isolation", "Impatience"];
pub const SCORE_GROUPS: [&str; 4] = ["headline", "think", "values", "shadow"];
pub const NOTE_GROUPS: [&str; 5] = ["think", "values", "shadow", "patterns", "changes"];
const CARD_MAX: usize = 5;
const NOTES_MAX: usize = 3;
pub const LINE_MAX: usize = 240;
const HEADLINE_MAX: usize = 600;

static SCANNING: AtomicBool = AtomicBool::new(false);

fn labels(grp: &str) -> &'static [&'static str] {
    match grp {
        "headline" => &HEADLINE,
        "think" => &THINK,
        "values" => &VALUES,
        "shadow" => &SHADOW,
        _ => &[],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub grp: String,
    pub label: String,
    pub score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub grp: String,
    pub line: String,
}

/// One month on the Changes timeline: its type and the three tiles.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonthPoint {
    pub month: String,
    pub mbti: Option<String>,
    pub headline: Vec<Score>,
}

/// One month in full. Also the shape of `profile/readings.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub month: String,
    pub mbti: Option<String>,
    pub enneagram: Option<String>,
    pub tritype: Option<String>,
    pub headline: Option<String>,
    pub entries_read: Option<u32>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    #[serde(default)]
    pub scanned_at: Option<String>,
    #[serde(default)]
    pub scores: Vec<Score>,
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// The room's whole read: every month for the timeline, the newest in full, and how many finished
/// months are waiting to be read.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MyProfile {
    pub ready: bool,
    pub timeline: Vec<MonthPoint>,
    pub current: Option<Reading>,
    pub waiting: Vec<String>,
}

/// What one scan did.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub read: u32,
    pub skipped: Vec<String>,
}

// ---------- pure ----------

/// A month as the room names it: `YYYY-MM`.
pub fn is_month(month: &str) -> bool {
    let b = month.as_bytes();
    b.len() == 7 && b[4] == b'-' && b[..4].iter().chain(&b[5..]).all(u8::is_ascii_digit) && matches!(month[5..].parse::<u8>(), Ok(1..=12))
}

/// The months to read: the finished ones with enough pages that have no reading yet, oldest first.
pub fn missing_months(finished: &[String], have: &[String]) -> Vec<String> {
    let have: BTreeSet<&String> = have.iter().collect();
    finished.iter().filter(|m| !have.contains(m)).cloned().collect()
}

/// The JSON in the answer's last `<reading>…</reading>` block.
pub fn read_answer(answer: &str) -> Option<Value> {
    let start = answer.rfind("<reading>")?;
    let rest = &answer[start + "<reading>".len()..];
    serde_json::from_str(rest.split("</reading>").next()?.trim()).ok()
}

fn one_line(text: &str, limit: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= limit {
        return line;
    }
    let cut: String = line.chars().take(limit - 1).collect();
    format!("{}…", cut.trim_end())
}

fn short(body: &Value, key: &str, limit: usize) -> Option<String> {
    let text = body.get(key)?.as_str()?.trim();
    (!text.is_empty() && text.chars().count() <= limit).then(|| text.to_string())
}

/// `INTJ` or `INTJ-A` (any case), else nothing.
fn mbti_of(body: &Value) -> Option<String> {
    let code = body.get("mbti")?.as_str()?.trim().to_uppercase();
    let b = code.as_bytes();
    let four = b.len() >= 4 && b"EI".contains(&b[0]) && b"NS".contains(&b[1]) && b"TF".contains(&b[2]) && b"JP".contains(&b[3]);
    let tail = b.len() == 4 || (b.len() == 6 && b[4] == b'-' && b"AT".contains(&b[5]));
    (four && tail).then_some(code)
}

/// The reading as it will be kept, or the one sentence that refuses it. Strict on the words and
/// the numbers (a word not on the list is left out; the three tiles must all be there; a score is
/// a whole number 0-100); forgiving on size (a card keeps its first five bars in the list's order,
/// a group its first three lines, a long line is cut). The pages, the dates and the count are
/// Castle's own, never the answer's. Pure, tested.
pub fn check_reading(month: &str, body: &Value, entries: u32, from_date: &str, to_date: &str) -> Result<Reading, String> {
    if !is_month(month) {
        return Err("That is not a month.".into());
    }
    if !body.is_object() {
        return Err("The reading was not an object.".into());
    }
    let mut scores = Vec::new();
    let empty = serde_json::Map::new();
    let given = body.get("scores").and_then(Value::as_object).unwrap_or(&empty);
    for grp in SCORE_GROUPS {
        let mut seen: HashMap<&str, u8> = HashMap::new();
        for row in given.get(grp).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let label = row.get("label").and_then(Value::as_str).map(str::trim).unwrap_or("");
            let Some(word) = labels(grp).iter().find(|w| w.eq_ignore_ascii_case(label)) else { continue };
            let score = row.get("score").and_then(Value::as_u64).filter(|s| *s <= 100).ok_or_else(|| format!("{grp} · {word}: a score is a whole number from 0 to 100."))?;
            seen.entry(word).or_insert(score as u8);
        }
        if grp == "headline" && seen.len() != HEADLINE.len() {
            return Err(format!("The reading must score all three tiles: {}.", HEADLINE.join(", ")));
        }
        let ordered: Vec<&str> = labels(grp).iter().copied().filter(|w| seen.contains_key(w)).take(CARD_MAX).collect();
        scores.extend(ordered.into_iter().map(|w| Score { grp: grp.into(), label: w.into(), score: seen[w] }));
    }
    let mut notes = Vec::new();
    let given = body.get("notes").and_then(Value::as_object).unwrap_or(&empty);
    for grp in NOTE_GROUPS {
        let lines = given.get(grp).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        let kept = lines.iter().filter_map(Value::as_str).map(|l| one_line(l, LINE_MAX)).filter(|l| !l.is_empty()).take(NOTES_MAX);
        notes.extend(kept.map(|line| Note { grp: grp.into(), line }));
    }
    let headline = body.get("headline").and_then(Value::as_str).map(|h| one_line(h, HEADLINE_MAX)).filter(|h| !h.is_empty());
    Ok(Reading {
        month: month.into(),
        mbti: mbti_of(body),
        enneagram: short(body, "enneagram", 8),
        tritype: short(body, "tritype", 8),
        headline,
        entries_read: Some(entries),
        from_date: Some(from_date.into()),
        to_date: Some(to_date.into()),
        scanned_at: None,
        scores,
        notes,
    })
}

/// "2026-03" -> "March 2026".
pub fn month_name(month: &str) -> String {
    const NAMES: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    match month.get(5..7).and_then(|m| m.parse::<usize>().ok()).filter(|m| (1..=12).contains(m)) {
        Some(m) => format!("{} {}", NAMES[m - 1], &month[..4]),
        None => month.to_string(),
    }
}

/// The words handed to the CLI: the month, the words it may score, then the pages as data. Pure, tested.
pub fn reading_words(month: &str, diary: &[(String, String)]) -> String {
    let mut words = format!("Read the diary of {} ({} entries) and answer one reading.\n\n", month_name(month), diary.len());
    words.push_str(&format!("Words you may score:\n- headline (all three): {}\n", HEADLINE.join(", ")));
    for (grp, list) in [("think", &THINK), ("values", &VALUES), ("shadow", &SHADOW)] {
        words.push_str(&format!("- {grp} (up to five): {}\n", list.join(", ")));
    }
    words.push_str("\nThe diary pages:\n");
    for (title, body) in diary {
        words.push_str(&format!("<page title=\"{}\">\n{}\n</page>\n", title.replace('"', "'"), body.replace("</page>", "")));
    }
    words.push_str("\nAnswer only with <reading>{...}</reading>.");
    words
}

/// The rows into the timeline: months in order, each with its tiles in the list's order. Pure, tested.
pub fn assemble_timeline(rows: Vec<(u64, String, Option<String>)>, headline: Vec<(u64, String, u8)>) -> Vec<MonthPoint> {
    let mut by_reading: HashMap<u64, Vec<Score>> = HashMap::new();
    for (id, label, score) in headline {
        by_reading.entry(id).or_default().push(Score { grp: "headline".into(), label, score: score.min(100) });
    }
    let mut points: Vec<MonthPoint> = rows
        .into_iter()
        .map(|(id, month, mbti)| {
            let mut tiles = by_reading.remove(&id).unwrap_or_default();
            tiles.sort_by_key(|s| HEADLINE.iter().position(|w| *w == s.label).unwrap_or(HEADLINE.len()));
            MonthPoint { month, mbti, headline: tiles }
        })
        .collect();
    points.sort_by(|a, b| a.month.cmp(&b.month));
    points
}

// ---------- the database ----------

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work.")
}

/// Every month read so far, oldest first.
pub fn months_read(conn: &mut impl Queryable) -> Result<Vec<String>, String> {
    conn.query("SELECT DATE_FORMAT(month, '%Y-%m') FROM profile_readings ORDER BY month").map_err(sql("Reading your profile"))
}

/// A month's reading, kept whole in one transaction (any older reading of that month is replaced).
pub fn save(conn: &mut Conn, r: &Reading) -> Result<(), String> {
    let mut tx = conn.start_transaction(TxOpts::default()).map_err(sql("Opening the database"))?;
    save_in(&mut tx, r)?;
    tx.commit().map_err(sql("Keeping the reading"))
}

/// The rows of one reading, inside a transaction the caller holds (moving in keeps every reading
/// in its own transaction with the rest of the memory).
pub fn save_in(tx: &mut impl Queryable, r: &Reading) -> Result<(), String> {
    if !is_month(&r.month) {
        return Err("That is not a month.".into());
    }
    let first = format!("{}-01", r.month);
    tx.exec_drop("DELETE FROM profile_readings WHERE month = ?", (&first,)).map_err(sql("Replacing the month"))?;
    tx.exec_drop(
        "INSERT INTO profile_readings (month, mbti, enneagram, tritype, headline, entries_read, from_date, to_date, scanned_at)          VALUES (?, ?, ?, ?, ?, ?, ?, ?, COALESCE(?, NOW()))",
        (&first, &r.mbti, &r.enneagram, &r.tritype, &r.headline, r.entries_read, &r.from_date, &r.to_date, &r.scanned_at),
    )
    .map_err(sql("Keeping the reading"))?;
    let id: u64 = tx.query_first("SELECT LAST_INSERT_ID()").map_err(sql("Keeping the reading"))?.ok_or("Keeping the reading did not work.")?;
    for grp in SCORE_GROUPS {
        for (sort, s) in r.scores.iter().filter(|s| s.grp == grp).enumerate() {
            tx.exec_drop("INSERT INTO profile_scores (reading_id, grp, label, score, sort) VALUES (?, ?, ?, ?, ?)", (id, &s.grp, &s.label, s.score.min(100), sort as u32))
                .map_err(sql("Keeping the scores"))?;
        }
    }
    for grp in NOTE_GROUPS {
        for (sort, n) in r.notes.iter().filter(|n| n.grp == grp).enumerate() {
            let line: String = n.line.chars().take(LINE_MAX).collect();
            tx.exec_drop("INSERT INTO profile_notes (reading_id, grp, line, sort) VALUES (?, ?, ?, ?)", (id, &n.grp, &line, sort as u32)).map_err(sql("Keeping the notes"))?;
        }
    }
    Ok(())
}

/// `profile/readings.json`: every reading, oldest first.
pub fn to_json(readings: &[Reading]) -> String {
    serde_json::to_string_pretty(readings).unwrap_or_else(|_| "[]".into())
}

/// The readings in an exported file; a file that is not readings is no readings.
pub fn from_json(text: &str) -> Vec<Reading> {
    serde_json::from_str::<Vec<Reading>>(text).unwrap_or_default().into_iter().filter(|r| is_month(&r.month)).collect()
}

type ReadingRow = (u64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<u32>, Option<String>, Option<String>, Option<String>);

/// One month in full, or nothing when it was never read.
pub fn month(conn: &mut impl Queryable, month: &str) -> Result<Option<Reading>, String> {
    if !is_month(month) {
        return Err("That is not a month.".into());
    }
    let row: Option<ReadingRow> = conn
        .exec_first(
            "SELECT id, DATE_FORMAT(month, '%Y-%m'), mbti, enneagram, tritype, headline, entries_read, \
             DATE_FORMAT(from_date, '%Y-%m-%d'), DATE_FORMAT(to_date, '%Y-%m-%d'), DATE_FORMAT(scanned_at, '%Y-%m-%d %H:%i') \
             FROM profile_readings WHERE month = ?",
            (format!("{month}-01"),),
        )
        .map_err(sql("Reading your profile"))?;
    let Some((id, month, mbti, enneagram, tritype, headline, entries_read, from_date, to_date, scanned_at)) = row else {
        return Ok(None);
    };
    let scores: Vec<(String, String, u8)> = conn
        .exec("SELECT grp, label, score FROM profile_scores WHERE reading_id = ? ORDER BY FIELD(grp, 'headline', 'think', 'values', 'shadow'), sort, id", (id,))
        .map_err(sql("Reading your profile"))?;
    let notes: Vec<(String, String)> = conn
        .exec("SELECT grp, line FROM profile_notes WHERE reading_id = ? ORDER BY FIELD(grp, 'think', 'values', 'shadow', 'patterns', 'changes'), sort, id", (id,))
        .map_err(sql("Reading your profile"))?;
    Ok(Some(Reading {
        month,
        mbti,
        enneagram,
        tritype,
        headline,
        entries_read,
        from_date,
        to_date,
        scanned_at,
        scores: scores.into_iter().map(|(grp, label, score)| Score { grp, label, score: score.min(100) }).collect(),
        notes: notes.into_iter().map(|(grp, line)| Note { grp, line }).collect(),
    }))
}

/// Every month for the timeline and the newest in full.
pub fn profile(conn: &mut impl Queryable) -> Result<MyProfile, String> {
    let rows: Vec<(u64, String, Option<String>)> = conn.query("SELECT id, DATE_FORMAT(month, '%Y-%m'), mbti FROM profile_readings ORDER BY month").map_err(sql("Reading your profile"))?;
    let headline: Vec<(u64, String, u8)> = conn.query("SELECT reading_id, label, score FROM profile_scores WHERE grp = 'headline' ORDER BY reading_id, sort, id").map_err(sql("Reading your profile"))?;
    let timeline = assemble_timeline(rows, headline);
    let current = match timeline.last() {
        Some(last) => month(conn, &last.month)?,
        None => None,
    };
    let finished: Vec<String> = crate::memory::diary_months(conn, MIN_ENTRIES)?.into_iter().map(|(m, _)| m).collect();
    let have: Vec<String> = timeline.iter().map(|p| p.month.clone()).collect();
    Ok(MyProfile { ready: !timeline.is_empty(), waiting: missing_months(&finished, &have), timeline, current })
}

/// Every reading in full, oldest first: what Export carries.
pub fn all(conn: &mut impl Queryable) -> Result<Vec<Reading>, String> {
    let mut out = Vec::new();
    for m in months_read(conn)? {
        if let Some(r) = month(conn, &m)? {
            out.push(r);
        }
    }
    Ok(out)
}

// ---------- the Tauri side ----------

fn open(store: &SettingsStore) -> Result<Conn, String> {
    crate::rooms::need(store, "myprofile")?;
    let folder = store.get().memory_folder.map(PathBuf::from).filter(|f| f.is_dir()).ok_or("Choose your MemoryCore folder first.")?;
    let mut conn = crate::store::source(&folder).ok_or("MY PROFILE lives in the memory database. Move your memory in first (Settings > Database).")?;
    crate::memdb::migrate(&mut conn)?;
    Ok(conn)
}

/// The profile folder: the rules for both CLIs, written fresh each time.
fn prepare_folder(store: &SettingsStore) -> Result<PathBuf, String> {
    let settings = store.get();
    let folder = store.folder().join(MARKDOWN_DIR);
    std::fs::create_dir_all(&folder).map_err(|_| "The profile folder could not be made.".to_string())?;
    let member = settings.member_name.clone().unwrap_or_else(|| "your person".into());
    let rules = RULES.replace("{{AI_NAME}}", &settings.companion_label()).replace("{{USER_NAME}}", &member);
    for name in std::iter::once("rules.md").chain(crate::setup::INSTRUCTION_FILES) {
        crate::oneshot::write_if_changed(&folder.join(name), &rules);
    }
    Ok(folder)
}

#[tauri::command]
pub async fn profile_read(app: AppHandle) -> Result<MyProfile, String> {
    tauri::async_runtime::spawn_blocking(move || profile(&mut open(&app.state::<SettingsStore>())?)).await.map_err(|_| "Reading your profile stopped.".to_string())?
}

#[tauri::command]
pub async fn profile_month(app: AppHandle, month: String) -> Result<Option<Reading>, String> {
    tauri::async_runtime::spawn_blocking(move || self::month(&mut open(&app.state::<SettingsStore>())?, &month)).await.map_err(|_| "Reading your profile stopped.".to_string())?
}

fn progress(app: &AppHandle, month: &str, done: usize, total: usize, state: &str, problem: Option<&str>) {
    let _ = app.emit(PROGRESS_EVENT, serde_json::json!({ "month": month, "done": done, "total": total, "state": state, "problem": problem }));
}

/// Reads every finished month that has no reading yet, one after another. A month that comes back
/// wrong is said and passed over; the next scan tries it again.
#[tauri::command]
pub async fn profile_scan(app: AppHandle) -> Result<ScanResult, String> {
    if SCANNING.swap(true, Ordering::SeqCst) {
        return Err("Your diary is already being read.".into());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let mut conn = open(&store)?;
        let finished: Vec<String> = crate::memory::diary_months(&mut conn, MIN_ENTRIES)?.into_iter().map(|(m, _)| m).collect();
        if finished.is_empty() {
            return Err(format!("There is no finished month with {MIN_ENTRIES} diary entries yet. A month is read once it is over."));
        }
        let todo = missing_months(&finished, &months_read(&mut conn)?);
        let place = prepare_folder(&store)?;
        let cli = store.get().cli;
        let mut result = ScanResult { read: 0, skipped: Vec::new() };
        for (i, m) in todo.iter().enumerate() {
            progress(&app, m, i, todo.len(), "reading", None);
            let outcome = (|| {
                let diary = crate::memory::diary_for_month(&mut conn, m, ENTRY_EACH, MONTH_TOTAL)?;
                let (Some(first), Some(last)) = (diary.first(), diary.last()) else { return Err("That month has no pages.".to_string()) };
                let (from, to) = (first.0[..10].to_string(), last.0[..10].to_string());
                let answer = crate::oneshot::run(cli, &place, "rules.md", &reading_words(m, &diary), READ_LIMIT)?;
                let body = read_answer(&answer).ok_or("No reading came back for that month.")?;
                let reading = check_reading(m, &body, diary.len() as u32, &from, &to)?;
                save(&mut conn, &reading)
            })();
            match outcome {
                Ok(()) => {
                    result.read += 1;
                    progress(&app, m, i + 1, todo.len(), "read", None);
                }
                Err(why) => {
                    result.skipped.push(m.clone());
                    progress(&app, m, i + 1, todo.len(), "skipped", Some(&why));
                }
            }
        }
        Ok(result)
    })
    .await
    .map_err(|_| "Reading your diary stopped.".to_string());
    SCANNING.store(false, Ordering::SeqCst);
    result?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({
            "mbti": "intj-a", "enneagram": "5w4", "tritype": "513", "headline": "  A quiet builder\n who plans. ",
            "scores": {
                "headline": [{"label": "Focus", "score": 62}, {"label": "growth", "score": 78}, {"label": "Wellbeing", "score": 71}],
                "think": [{"label": "Practical", "score": 58}, {"label": "Analytical", "score": 92}, {"label": "Made up", "score": 99}],
                "values": [], "shadow": [{"label": "Control", "score": 69}, {"label": "Control", "score": 10}]
            },
            "notes": {"think": ["one", "two", "three", "four"], "patterns": ["x".repeat(300)], "nonsense": ["no"]}
        })
    }

    #[test]
    fn a_good_reading_is_kept_in_the_lists_order_and_cut_to_size() {
        let r = check_reading("2026-03", &good(), 12, "2026-03-01", "2026-03-30").unwrap();
        assert_eq!(r.mbti.as_deref(), Some("INTJ-A"));
        assert_eq!(r.headline.as_deref(), Some("A quiet builder who plans."));
        assert_eq!((r.entries_read, r.from_date.as_deref(), r.to_date.as_deref()), (Some(12), Some("2026-03-01"), Some("2026-03-30")));
        let labels: Vec<_> = r.scores.iter().map(|s| (s.grp.as_str(), s.label.as_str(), s.score)).collect();
        assert_eq!(labels, [("headline", "Growth", 78), ("headline", "Focus", 62), ("headline", "Wellbeing", 71), ("think", "Analytical", 92), ("think", "Practical", 58), ("shadow", "Control", 69)]);
        assert_eq!(r.notes.iter().filter(|n| n.grp == "think").count(), 3);
        let long = r.notes.iter().find(|n| n.grp == "patterns").unwrap();
        assert_eq!(long.line.chars().count(), LINE_MAX);
        assert!(long.line.ends_with('…'));
        assert!(!r.notes.iter().any(|n| n.grp == "nonsense"));
    }

    #[test]
    fn a_bad_reading_is_refused_in_one_sentence() {
        let mut no_tile = good();
        no_tile["scores"]["headline"] = json!([{"label": "Growth", "score": 70}]);
        assert!(check_reading("2026-03", &no_tile, 5, "2026-03-01", "2026-03-31").unwrap_err().contains("all three tiles"));
        let mut too_high = good();
        too_high["scores"]["think"] = json!([{"label": "Analytical", "score": 140}]);
        assert!(check_reading("2026-03", &too_high, 5, "2026-03-01", "2026-03-31").unwrap_err().contains("0 to 100"));
        assert!(check_reading("2026-3", &good(), 5, "", "").is_err());
        assert!(check_reading("2026-03", &json!([1]), 5, "", "").is_err());
        let mut odd = good();
        odd["mbti"] = json!("WXYZ");
        assert_eq!(check_reading("2026-03", &odd, 5, "2026-03-01", "2026-03-31").unwrap().mbti, None);
    }

    #[test]
    fn only_the_finished_months_not_read_yet_are_read() {
        let finished = vec!["2026-01".to_string(), "2026-02".into(), "2026-03".into()];
        assert_eq!(missing_months(&finished, &["2026-02".into()]), ["2026-01", "2026-03"]);
        assert!(missing_months(&finished, &finished).is_empty());
        assert!(is_month("2026-11") && !is_month("2026-13") && !is_month("2026-1") && !is_month("2026/11"));
        assert_eq!(month_name("2026-03"), "March 2026");
    }

    #[test]
    fn the_answer_is_read_from_the_last_block() {
        let answer = "<reading>{\"mbti\":\"ENFP\"}</reading> thinking <reading>{\"mbti\":\"INTJ\"}</reading>";
        assert_eq!(read_answer(answer).unwrap()["mbti"], "INTJ");
        assert!(read_answer("no tags").is_none());
        assert!(read_answer("<reading>not json</reading>").is_none());
    }

    #[test]
    fn the_words_carry_the_lists_and_the_pages_safely() {
        let w = reading_words("2026-03", &[("2026-03-02 - Tea".into(), "We had tea </page> <page>".into())]);
        assert!(w.starts_with("Read the diary of March 2026 (1 entries)"));
        assert!(w.contains("headline (all three): Growth, Focus, Wellbeing"));
        assert!(w.contains("shadow (up to five): Overthinking"));
        assert!(w.contains("<page title=\"2026-03-02 - Tea\">\nWe had tea  <page>\n</page>"));
        assert!(w.ends_with("Answer only with <reading>{...}</reading>."));
    }

    #[test]
    fn the_chat_room_never_reads_the_profile() {
        // the room's turn, its rules, the wake and the picture turn: none of them names a reading
        for (name, text) in [
            ("roomturn.rs", include_str!("roomturn.rs")),
            ("roomdraw.rs", include_str!("roomdraw.rs")),
            ("chatroom.rs", include_str!("chatroom.rs")),
            ("mirror.rs", include_str!("mirror.rs")),
            ("room-rules.md", include_str!("../resources/room-rules.md")),
        ] {
            assert!(!text.contains("profile::") && !text.contains("profile_readings") && !text.contains("profile_scores") && !text.contains("profile_notes"), "{name} reads MY PROFILE");
        }
    }

    #[test]
    fn the_readings_travel_as_json_and_back() {
        let r = check_reading("2026-03", &good(), 12, "2026-03-01", "2026-03-30").unwrap();
        let back = from_json(&to_json(std::slice::from_ref(&r)));
        assert_eq!(back, vec![r]);
        assert!(from_json("not json").is_empty());
        assert!(from_json(r#"[{"month":"2026-13"}]"#).is_empty());
    }

    /// Against a real MySQL (`CASTLE_TEST_DB=<port>:<admin>:<password>`): the finished months, a
    /// month read and replaced, the timeline, and the readings out as JSON and back in. Uses and
    /// drops its own database.
    #[test]
    #[ignore]
    fn live_a_month_read_kept_and_carried() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &admin, &admin_pw, &format!("Profile Test {}", std::process::id())).unwrap();
        let mut conn = crate::memdb::open(&access).unwrap();

        // six pages in a finished month, three in an older thin one, one in the month in progress
        for day in 1..=6 {
            conn.exec_drop("INSERT INTO diary_entries (entry_date, session_no, title, body) VALUES (?, 1, ?, ?)", (format!("2025-01-0{day}"), format!("Day {day}"), format!("Page {day}"))).unwrap();
        }
        for day in 1..=3 {
            conn.exec_drop("INSERT INTO diary_entries (entry_date, session_no, title, body) VALUES (?, 1, 'Thin', 'thin')", (format!("2024-12-0{day}"),)).unwrap();
        }
        conn.query_drop("INSERT INTO diary_entries (entry_date, session_no, title, body) VALUES (CURDATE(), 1, 'Today', 'now')").unwrap();
        assert_eq!(crate::memory::diary_months(&mut conn, MIN_ENTRIES).unwrap(), vec![("2025-01".to_string(), 6)]);
        let pages = crate::memory::diary_for_month(&mut conn, "2025-01", ENTRY_EACH, MONTH_TOTAL).unwrap();
        assert_eq!(pages.len(), 6);
        assert_eq!(pages[0].0, "2025-01-01 - Day 1");

        let empty = profile(&mut conn).unwrap();
        assert!(!empty.ready && empty.current.is_none());
        assert_eq!(empty.waiting, ["2025-01"]);

        let r = check_reading("2025-01", &good(), 6, "2025-01-01", "2025-01-06").unwrap();
        save(&mut conn, &r).unwrap();
        save(&mut conn, &r).unwrap();
        let count: Option<u64> = conn.query_first("SELECT COUNT(*) FROM profile_readings").unwrap();
        assert_eq!(count, Some(1), "a month read again replaces its reading");
        let p = profile(&mut conn).unwrap();
        assert!(p.ready && p.waiting.is_empty());
        assert_eq!(p.timeline[0].headline.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), HEADLINE);
        let current = p.current.unwrap();
        assert_eq!((current.scores.clone(), current.notes.clone()), (r.scores.clone(), r.notes.clone()));
        assert!(month(&mut conn, "2025-02").unwrap().is_none());

        // out as profile/readings.json and back in
        let json = to_json(&all(&mut conn).unwrap());
        conn.query_drop("DELETE FROM profile_readings").unwrap();
        let notes: Option<u64> = conn.query_first("SELECT COUNT(*) FROM profile_notes").unwrap();
        assert_eq!(notes, Some(0), "the notes go with their reading");
        let mut tx = conn.start_transaction(TxOpts::default()).unwrap();
        for reading in from_json(&json) {
            save_in(&mut tx, &reading).unwrap();
        }
        tx.commit().unwrap();
        let again = month(&mut conn, "2025-01").unwrap().unwrap();
        assert_eq!((again.mbti, again.scores, again.notes), (r.mbti, r.scores, r.notes));

        let mut root = mysql::Conn::new(mysql::Opts::from(
            mysql::OptsBuilder::new().ip_or_hostname(Some("127.0.0.1")).tcp_port(port).user(Some(admin)).pass(Some(admin_pw)).prefer_socket(false),
        ))
        .unwrap();
        root.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
        for host in ["localhost", "127.0.0.1"] {
            root.query_drop(format!("DROP USER '{}'@'{host}'", access.user)).unwrap();
        }
    }

    #[test]
    fn the_timeline_is_in_month_order_with_its_tiles_in_order() {
        let rows = vec![(2, "2026-03".to_string(), Some("INTJ-A".to_string())), (1, "2026-01".to_string(), None)];
        let tiles = vec![(2, "Wellbeing".to_string(), 71), (2, "Growth".to_string(), 140), (2, "Focus".to_string(), 62)];
        let t = assemble_timeline(rows, tiles);
        assert_eq!(t.iter().map(|p| p.month.as_str()).collect::<Vec<_>>(), ["2026-01", "2026-03"]);
        assert!(t[0].headline.is_empty());
        assert_eq!(t[1].headline.iter().map(|s| (s.label.as_str(), s.score)).collect::<Vec<_>>(), [("Growth", 100), ("Focus", 62), ("Wellbeing", 71)]);
    }
}
