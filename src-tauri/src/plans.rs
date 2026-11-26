//! Approved plans for the companion's project folders: a plan's markdown read into rows, and the
//! rows walked one box at a time. The door's `plan` verbs and the DEV tab both read these.
//!
//! ```text
//!  # Sign in through the Library          the plan's title (the first `# ` heading)
//!  ## Phase 45: Sign in                   a `phase` row, so the panel can group the boxes
//!  - [ ] Task 45.1 The button             a `task` row; the indented lines under it stay with it
//!    - the small print                    (part of Task 45.1's text)
//!  - [ ] Seal                             a `seal` row
//!  - [x] Task 45.2 The link               a box already ticked: `done`
//! ```
//!
//! A box under a heading that starts with "Carried" is `carried`, and an open box whose text holds
//! "(FLAGGED" or "(BLOCKED" is `flagged`: both wait for the member, and `next` passes them by.
//! Boxes inside a ``` code block are examples, not steps.

use mysql::prelude::Queryable;
use mysql::{Conn, TxOpts};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The states `mark` may give a box.
pub const MARK_STATES: [&str; 3] = ["done", "carried", "flagged"];

/// One open plan of a folder as the DEV tab reads it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub id: u64,
    pub title: String,
    pub phase: Option<String>,
    pub status: String,
    pub items: Vec<PlanItem>,
}

/// One row of a plan: a box (`task`, `seal`) or a phase heading (`phase`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub ordinal: u32,
    pub kind: String,
    pub text: String,
    pub state: String,
}

// ---------- pure: reading a plan ----------

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedItem {
    pub kind: &'static str,
    pub text: String,
    pub state: &'static str,
}

#[derive(Debug, Default, PartialEq)]
pub struct ParsedPlan {
    pub title: String,
    /// The phase numbers its headings name, in order ("45", "46").
    pub phases: Vec<String>,
    pub items: Vec<ParsedItem>,
}

impl ParsedPlan {
    /// "Phase 45", "Phase 45-47", or nothing when no heading names a phase.
    pub fn phase(&self) -> Option<String> {
        let (first, last) = (self.phases.first()?, self.phases.last()?);
        Some(if first == last { format!("Phase {first}") } else { format!("Phase {first}-{last}") })
    }

    /// How many boxes it holds (every row but the phase headings).
    pub fn boxes(&self) -> usize {
        self.items.iter().filter(|i| i.kind != "phase").count()
    }
}

/// `# Title` -> (1, "Title"): the heading's level and its words.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    let words = line[level..].strip_prefix(' ')?;
    if (1..=6).contains(&level) { Some((level, words.trim())) } else { None }
}

/// `- [ ] words` or `- [x] words` (also `* `) at the start of a line: (ticked, words).
fn checkbox(line: &str) -> Option<(bool, &str)> {
    let rest = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))?;
    let ticked = match rest.get(..3)? {
        "[ ]" => false,
        "[x]" | "[X]" => true,
        _ => return None,
    };
    let words = rest[3..].trim();
    if words.is_empty() { None } else { Some((ticked, words)) }
}

/// The phase a heading names: "Phase 45: Sign in" -> Some("45"); Some("") for a phase heading
/// with no number; None for any other heading.
fn phase_of(words: &str) -> Option<String> {
    let rest = words.get(..5).filter(|w| w.eq_ignore_ascii_case("phase")).map(|_| &words[5..])?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let number: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '.').collect();
    Some(number.trim_end_matches('.').to_string())
}

/// `seal` when the box is just the Seal ("Seal", "Seal (abc1234)", "Seal - nothing changed");
/// every other box is a `task` ("Sealed envelope art" too).
fn kind_of(words: &str) -> &'static str {
    let low = words.to_lowercase();
    match low.strip_prefix("seal") {
        Some(rest) if !rest.starts_with(char::is_alphabetic) => "seal",
        _ => "task",
    }
}

fn flagged(words: &str) -> bool {
    let head: String = words.chars().take(160).collect::<String>().to_uppercase();
    head.contains("(FLAGGED") || head.contains("(BLOCKED")
}

/// Reads a plan's markdown into its title, its phases and its rows, in the order written.
pub fn parse_plan(markdown: &str) -> ParsedPlan {
    let text = markdown.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    walk(&text).0
}

/// The walk `parse_plan` makes, with the line (of `text.lines()`) each box came from; a phase
/// row has no line of its own to tick.
fn walk(text: &str) -> (ParsedPlan, Vec<Option<usize>>) {
    let mut plan = ParsedPlan::default();
    let mut lines_of: Vec<Option<usize>> = Vec::new();
    let mut fenced = false;
    // the level of the "Carried ..." heading the lines are under, while they are
    let mut carried_at: Option<usize> = None;
    // the box that indented lines below it belong to
    let mut last: Option<usize> = None;
    for (at_line, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            last = None;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some((level, words)) = heading(line) {
            last = None;
            if level == 1 && plan.title.is_empty() {
                plan.title = words.to_string();
                continue;
            }
            if carried_at.is_some_and(|at| level <= at) {
                carried_at = None;
            }
            if words.to_lowercase().starts_with("carried") {
                carried_at = Some(level);
            }
            if level >= 2 {
                if let Some(number) = phase_of(words) {
                    if !number.is_empty() {
                        plan.phases.push(number);
                    }
                    plan.items.push(ParsedItem { kind: "phase", text: words.to_string(), state: "open" });
                    lines_of.push(None);
                }
            }
            continue;
        }
        if let Some((ticked, words)) = checkbox(line) {
            let state = if ticked {
                "done"
            } else if carried_at.is_some() {
                "carried"
            } else if flagged(words) {
                "flagged"
            } else {
                "open"
            };
            plan.items.push(ParsedItem { kind: kind_of(words), text: words.to_string(), state });
            lines_of.push(Some(at_line));
            last = Some(plan.items.len() - 1);
            continue;
        }
        let indented = line.starts_with([' ', '\t']) && !trimmed.is_empty();
        match last {
            Some(at) if indented => {
                let item = &mut plan.items[at];
                item.text.push('\n');
                item.text.push_str(line.trim_end());
            }
            _ => last = None,
        }
    }
    (plan, lines_of)
}

/// The plan's text with every box ticked from its row, by ordinal: `[x]` when the row is done,
/// `[ ]` otherwise. A box already as its row says keeps its own mark (`[X]` too); every other
/// byte is left as it was, line ends included. Boxes in a ``` block are never touched.
pub fn render_plan(body: &str, items: &[PlanItem]) -> String {
    let text = body.trim_start_matches('\u{feff}');
    let bom = &body[..body.len() - text.len()];
    // text.lines() drops a "\r" before each "\n", so the walk reads it as parse_plan does
    let (_, lines_of) = walk(text);
    let mut tick: BTreeMap<usize, bool> = BTreeMap::new();
    for (n, line) in lines_of.iter().enumerate() {
        let row = items.iter().find(|i| i.ordinal as usize == n + 1);
        if let (Some(line), Some(row)) = (line, row) {
            tick.insert(*line, row.state == "done");
        }
    }
    let mut out = String::with_capacity(body.len());
    out.push_str(bom);
    for (n, line) in text.split_inclusive('\n').enumerate() {
        match (tick.get(&n), checkbox(line.trim_end_matches(['\n', '\r']))) {
            (Some(&done), Some((ticked, _))) if done != ticked => {
                // "- " or "* ", then the three bytes of the mark
                out.push_str(&line[..2]);
                out.push_str(if done { "[x]" } else { "[ ]" });
                out.push_str(&line[5..]);
            }
            _ => out.push_str(line),
        }
    }
    out
}

// ---------- pure: a plan as a file ----------

/// The first line of a plan written out as a file: `<!-- castle-plan {json} -->`.
const HEADER_OPEN: &str = "<!-- castle-plan ";
const HEADER_CLOSE: &str = " -->";

/// One box's state as the database held it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoxState {
    pub state: String,
    pub note: Option<String>,
    /// done_at, "YYYY-MM-DD HH:MM:SS"
    pub done: Option<String>,
}

/// What a plan file carries besides its text, so the plan comes back as it was.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanMeta {
    pub folder: String,
    pub status: String,
    /// approved_at, "YYYY-MM-DD HH:MM:SS"
    pub approved: String,
    pub source: Option<String>,
    /// every box (phase headings left out) by ordinal
    pub states: BTreeMap<u32, BoxState>,
    /// the boxes ticked in the text as it was approved, so that text comes back exactly
    #[serde(default)]
    pub ticked: Vec<u32>,
}

/// The header line (no line end).
pub fn plan_header(meta: &PlanMeta) -> String {
    let json = serde_json::to_string(meta).unwrap_or_else(|_| "{}".into());
    // "-->" can only be inside a JSON string, where its ">" may be written escaped
    format!("{HEADER_OPEN}{}{HEADER_CLOSE}", json.replace("-->", "--\\u003e"))
}

/// The header of a plan file and the text under it; None when the first line is not one.
pub fn read_plan_header(text: &str) -> Option<(PlanMeta, &str)> {
    let text = text.trim_start_matches('\u{feff}');
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let json = first.trim_end_matches('\r').strip_prefix(HEADER_OPEN)?.strip_suffix(HEADER_CLOSE)?;
    serde_json::from_str(json).ok().map(|meta| (meta, rest))
}

/// The text a plan was approved with, from its file's text: every box ticked as `meta.ticked`
/// says, the way the plan was before it was walked.
pub fn approved_text(rendered: &str, meta: &PlanMeta) -> String {
    let rows: Vec<PlanItem> = parse_plan(rendered)
        .items
        .iter()
        .enumerate()
        .map(|(n, i)| {
            let ordinal = n as u32 + 1;
            let state = if meta.ticked.contains(&ordinal) { "done" } else { "open" };
            PlanItem { ordinal, kind: i.kind.to_string(), text: String::new(), state: state.into() }
        })
        .collect();
    render_plan(rendered, &rows)
}

/// The states to put back: the header's, except where the file's own tick says otherwise (a
/// box ticked or unticked by hand while the plan was a file wins).
pub fn states_from_file(rendered: &str, meta: &PlanMeta) -> BTreeMap<u32, BoxState> {
    let mut states = meta.states.clone();
    for (n, item) in parse_plan(rendered).items.iter().enumerate() {
        if item.kind == "phase" {
            continue;
        }
        let ticked = item.state == "done";
        let entry = states.entry(n as u32 + 1).or_insert_with(|| BoxState { state: item.state.to_string(), note: None, done: None });
        if ticked != (entry.state == "done") {
            entry.state = if ticked { "done".into() } else { item.state.to_string() };
            entry.done = None;
        }
    }
    states
}

/// "YYYY-MM-DD HH:MM:SS" when the text is one, else None.
fn datetime(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok().map(|_| text.to_string())
}

/// A project folder made comparable: no trailing slash, and on Windows one kind of slash and
/// one case ("D:/Work/Castle/" and "d:\work\castle" are the same folder there).
pub fn folder_key(folder: &str) -> String {
    let folder = folder.trim();
    let folder = if cfg!(windows) { folder.trim_start_matches(r"\\?\") } else { folder };
    let trimmed = folder.trim_end_matches(['/', '\\']);
    let kept = if trimmed.is_empty() { folder } else { trimmed };
    if cfg!(windows) { kept.replace('/', "\\").to_lowercase() } else { kept.to_string() }
}

/// The first line of a box's text (the indented lines under it left out).
pub fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// `folder_key` for a folder given as text or as a path.
fn key_of(folder: &Path) -> String {
    folder_key(&folder.to_string_lossy())
}

fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

// ---------- the rows ----------

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work.")
}

/// Keeps an approved plan for a folder: (plan id, boxes, duplicate). The same text copied again
/// into the same folder is not kept twice; its first copy's id comes back with `duplicate` true.
pub fn copy_plan(conn: &mut Conn, folder: impl AsRef<Path>, markdown: &str, source_file: Option<&str>) -> Result<(u64, usize, bool), String> {
    let mut tx = conn.start_transaction(TxOpts::default()).map_err(sql("Opening the database"))?;
    let kept = copy_plan_in(&mut tx, folder, markdown, source_file)?;
    tx.commit().map_err(sql("Keeping the plan"))?;
    Ok(kept)
}

/// `copy_plan` inside a transaction the caller holds (moving in keeps every plan in its own).
pub fn copy_plan_in(tx: &mut impl Queryable, folder: impl AsRef<Path>, markdown: &str, source_file: Option<&str>) -> Result<(u64, usize, bool), String> {
    let body = markdown.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let parsed = parse_plan(&body);
    if parsed.boxes() == 0 {
        return Err("The plan has no `- [ ]` lines to walk. Write its steps as `- [ ] ...` and copy it again.".into());
    }
    let folder = folder.as_ref().to_string_lossy();
    let shown = folder.trim().trim_end_matches(['/', '\\']);
    let shown = if shown.is_empty() { folder.trim() } else { shown };
    let key = folder_key(&folder);
    let plan_key = crate::db::sha256_hex(format!("{key}\n{}", body.trim()).as_bytes());
    let there: Option<u64> = tx.exec_first("SELECT id FROM project_plans WHERE plan_key = ?", (&plan_key,)).map_err(sql("Reading the plans"))?;
    if let Some(id) = there {
        return Ok((id, parsed.boxes(), true));
    }

    let title = match parsed.title.as_str() {
        "" => source_file
            .and_then(|f| std::path::Path::new(f).file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled plan".into()),
        title => title.to_string(),
    };
    // a plan copied with every box already ticked is done from the start
    let status = if parsed.items.iter().all(|i| i.kind == "phase" || i.state == "done") { "done" } else { "open" };
    tx.exec_drop(
        "INSERT INTO project_plans (folder, folder_key, title, phase, status, body, plan_key, source_file) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (clip(shown, 500), clip(&key, 500), clip(&title, 500), parsed.phase().map(|p| clip(&p, 100)), status, &body, &plan_key, source_file.map(|f| clip(f, 500))),
    )
    .map_err(sql("Keeping the plan"))?;
    let id: u64 = tx.query_first("SELECT LAST_INSERT_ID()").map_err(sql("Keeping the plan"))?.filter(|id| *id > 0).ok_or("Keeping the plan did not work.")?;
    for (n, item) in parsed.items.iter().enumerate() {
        tx.exec_drop(
            "INSERT INTO project_plan_items (plan_id, ordinal, kind, text, state, done_at) VALUES (?, ?, ?, ?, ?, IF(? = 'done', NOW(), NULL))",
            (id, n as u32 + 1, item.kind, &item.text, item.state, item.state),
        )
        .map_err(sql("Keeping the plan's boxes"))?;
    }
    Ok((id, parsed.boxes(), false))
}

/// Puts a plan's states back after it was copied in again: each box's state, note and done
/// time by ordinal, then the plan's status and approval time. A state, status or time that is
/// not one Castle writes is passed by, and the copy's own stays.
pub fn restore_states(tx: &mut impl Queryable, plan_id: u64, states: &BTreeMap<u32, BoxState>, status: &str, approved: &str) -> Result<(), String> {
    for (ordinal, kept) in states {
        if !["open", "done", "carried", "flagged"].contains(&kept.state.as_str()) {
            continue;
        }
        tx.exec_drop(
            "UPDATE project_plan_items SET state = ?, note = ?, done_at = IF(? = 'done', COALESCE(?, done_at, NOW()), NULL) \
             WHERE plan_id = ? AND ordinal = ? AND kind <> 'phase'",
            (&kept.state, kept.note.as_deref().map(|n| clip(n, 500)), &kept.state, datetime(kept.done.as_deref()), plan_id, ordinal),
        )
        .map_err(sql("Putting the plan's boxes back"))?;
    }
    let status = ["open", "done", "archived"].contains(&status).then_some(status);
    tx.exec_drop(
        "UPDATE project_plans SET status = COALESCE(?, status), approved_at = COALESCE(?, approved_at) WHERE id = ?",
        (status, datetime(Some(approved)), plan_id),
    )
    .map_err(sql("Putting the plan back"))?;
    Ok(())
}

/// A plan's file: the header, then its text with every box ticked from its row. `meta.ticked`
/// is filled here from the text as it was approved.
pub fn plan_file(mut meta: PlanMeta, body: &str) -> String {
    meta.ticked = parse_plan(body).items.iter().enumerate().filter(|(_, i)| i.state == "done").map(|(n, _)| n as u32 + 1).collect();
    let rows: Vec<PlanItem> = meta
        .states
        .iter()
        .map(|(ordinal, kept)| PlanItem { ordinal: *ordinal, kind: "task".into(), text: String::new(), state: kept.state.clone() })
        .collect();
    format!("{}\n{}", plan_header(&meta), render_plan(body, &rows))
}

/// Every plan the database keeps, whatever its status, as (id, title, file text). None at all
/// when the database has no plans table yet.
pub fn plan_files(conn: &mut Conn) -> Result<Vec<(u64, String, String)>, String> {
    let table: Option<u64> = conn
        .query_first("SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'project_plans'")
        .map_err(sql("Reading the plans"))?;
    if table.unwrap_or(0) == 0 {
        return Ok(Vec::new());
    }
    let plans: Vec<(u64, String, String, String, Option<String>, String, Option<String>)> = conn
        .query("SELECT id, folder, title, status, DATE_FORMAT(approved_at, '%Y-%m-%d %H:%i:%s'), body, source_file FROM project_plans ORDER BY id")
        .map_err(sql("Reading the plans"))?;
    let mut out = Vec::with_capacity(plans.len());
    for (id, folder, title, status, approved, body, source) in plans {
        let rows: Vec<(u32, String, String, Option<String>, Option<String>)> = conn
            .exec(
                "SELECT ordinal, kind, state, note, DATE_FORMAT(done_at, '%Y-%m-%d %H:%i:%s') FROM project_plan_items WHERE plan_id = ? ORDER BY ordinal",
                (id,),
            )
            .map_err(sql("Reading the plan's boxes"))?;
        let states = rows
            .into_iter()
            .filter(|(_, kind, ..)| kind != "phase")
            .map(|(ordinal, _, state, note, done)| (ordinal, BoxState { state, note, done }))
            .collect();
        let meta = PlanMeta { folder, status, approved: approved.unwrap_or_default(), source, states, ticked: Vec::new() };
        out.push((id, title, plan_file(meta, &body)));
    }
    Ok(out)
}

fn items_of(conn: &mut Conn, plan_id: u64) -> Result<Vec<PlanItem>, String> {
    conn.exec_map(
        "SELECT ordinal, kind, text, state FROM project_plan_items WHERE plan_id = ? ORDER BY ordinal",
        (plan_id,),
        |(ordinal, kind, text, state)| PlanItem { ordinal, kind, text, state },
    )
    .map_err(sql("Reading the plan's boxes"))
}

/// The open plans of a folder, newest first, each with its rows.
pub fn open_plans(conn: &mut Conn, folder: impl AsRef<Path>) -> Result<Vec<PlanView>, String> {
    let rows: Vec<(u64, String, Option<String>, String)> = conn
        .exec("SELECT id, title, phase, status FROM project_plans WHERE folder_key = ? AND status = 'open' ORDER BY approved_at DESC, id DESC", (key_of(folder.as_ref()),))
        .map_err(sql("Reading the plans"))?;
    rows.into_iter().map(|(id, title, phase, status)| Ok(PlanView { id, title, phase, status, items: items_of(conn, id)? })).collect()
}

/// A plan's whole text, as it was approved.
pub fn plan_body(conn: &mut Conn, id: u64) -> Result<String, String> {
    let body: Option<String> = conn.exec_first("SELECT body FROM project_plans WHERE id = ?", (id,)).map_err(sql("Reading the plan"))?;
    body.ok_or(format!("No plan #{id}."))
}

/// The plan being walked in a folder: its oldest open plan.
pub fn current_plan(conn: &mut Conn, folder: impl AsRef<Path>) -> Result<Option<u64>, String> {
    conn.exec_first("SELECT id FROM project_plans WHERE folder_key = ? AND status = 'open' ORDER BY approved_at, id LIMIT 1", (key_of(folder.as_ref()),))
        .map_err(sql("Reading the plans"))
}

/// The box in turn and the plan that holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Next {
    pub plan_id: u64,
    pub item: PlanItem,
}

/// The first open box of a folder's open plans, oldest plan first (or of one plan, by id).
fn first_open(conn: &mut Conn, folder: &Path, plan: Option<u64>) -> Result<Option<Next>, String> {
    let row: Option<(u64, u32, String, String, String)> = conn
        .exec_first(
            "SELECT i.plan_id, i.ordinal, i.kind, i.text, i.state FROM project_plan_items i JOIN project_plans p ON p.id = i.plan_id \
             WHERE i.state = 'open' AND i.kind <> 'phase' AND (? IS NULL AND p.folder_key = ? AND p.status = 'open' OR p.id = ?) \
             ORDER BY p.approved_at, p.id, i.ordinal LIMIT 1",
            (plan, key_of(folder), plan),
        )
        .map_err(sql("Reading the plans"))?;
    Ok(row.map(|(plan_id, ordinal, kind, text, state)| Next { plan_id, item: PlanItem { ordinal, kind, text, state } }))
}

/// The next box to do in a folder: the first open one, oldest open plan first. Carried and
/// flagged boxes are passed by.
pub fn next_item(conn: &mut Conn, folder: impl AsRef<Path>) -> Result<Option<Next>, String> {
    first_open(conn, folder.as_ref(), None)
}

/// The boxes of a folder's open plans by state.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Left {
    pub open: u32,
    pub done: u32,
    pub carried: u32,
    pub flagged: u32,
}

pub fn left(conn: &mut Conn, folder: impl AsRef<Path>) -> Result<Left, String> {
    let rows: Vec<(String, u32)> = conn
        .exec(
            "SELECT i.state, COUNT(*) FROM project_plan_items i JOIN project_plans p ON p.id = i.plan_id \
             WHERE p.folder_key = ? AND p.status = 'open' AND i.kind <> 'phase' GROUP BY i.state",
            (key_of(folder.as_ref()),),
        )
        .map_err(sql("Reading the plans"))?;
    let mut left = Left::default();
    for (state, count) in rows {
        match state.as_str() {
            "open" => left.open = count,
            "done" => left.done = count,
            "carried" => left.carried = count,
            "flagged" => left.flagged = count,
            _ => {}
        }
    }
    Ok(left)
}

/// Which box to mark: the one in turn, or one by its number in the plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Which {
    Next,
    Ordinal(u32),
}

/// What a mark did.
#[derive(Debug, Clone, PartialEq)]
pub struct Marked {
    pub plan_id: u64,
    pub ordinal: u32,
    pub text: String,
    pub state: String,
    /// Every box of the plan is done now, so the plan is too.
    pub plan_done: bool,
}

/// Gives one box a state (done, carried or flagged), with an optional note. `plan` names the
/// plan by id; without it `Next` is the folder's box in turn and `Ordinal` counts in the plan
/// being walked. When every box of the plan is done, the plan's status becomes done.
pub fn mark(conn: &mut Conn, folder: impl AsRef<Path>, plan: Option<u64>, which: Which, state: &str, note: Option<&str>) -> Result<Marked, String> {
    if !MARK_STATES.contains(&state) {
        return Err(format!("{state} is not a state to mark: done, carried or flagged."));
    }
    let folder = folder.as_ref();
    let (plan_id, ordinal) = match which {
        Which::Next => first_open(conn, folder, plan)?.map(|n| (n.plan_id, n.item.ordinal)).ok_or("No open box to mark here.")?,
        Which::Ordinal(n) => match plan {
            Some(id) => (id, n),
            None => (current_plan(conn, folder)?.ok_or("No open plan here.")?, n),
        },
    };
    let row: Option<(String, String, String)> = conn
        .exec_first("SELECT kind, text, state FROM project_plan_items WHERE plan_id = ? AND ordinal = ?", (plan_id, ordinal))
        .map_err(sql("Reading the plan"))?;
    let (kind, text, was) = row.ok_or(format!("Plan #{plan_id} has no item {ordinal}."))?;
    if kind == "phase" {
        return Err(format!("Item {ordinal} is a phase heading; mark its boxes."));
    }
    if was == state {
        return Err(format!("Item {ordinal} is already {state}."));
    }
    conn.exec_drop(
        "UPDATE project_plan_items SET state = ?, done_at = IF(? = 'done', NOW(), NULL), note = COALESCE(?, note) WHERE plan_id = ? AND ordinal = ?",
        (state, state, note.map(|n| clip(n, 500)), plan_id, ordinal),
    )
    .map_err(sql("Marking the box"))?;
    let plan_done = settle(conn, plan_id)?;
    Ok(Marked { plan_id, ordinal, text, state: state.to_string(), plan_done })
}

/// The plan's status from its boxes: done when every box is done, open again when one is not.
/// An archived plan stays archived.
fn settle(conn: &mut Conn, plan_id: u64) -> Result<bool, String> {
    let open: Option<u64> = conn
        .exec_first("SELECT COUNT(*) FROM project_plan_items WHERE plan_id = ? AND kind <> 'phase' AND state <> 'done'", (plan_id,))
        .map_err(sql("Reading the plan"))?;
    let done = open.unwrap_or(0) == 0;
    conn.exec_drop("UPDATE project_plans SET status = ? WHERE id = ? AND status <> 'archived'", (if done { "done" } else { "open" }, plan_id))
        .map_err(sql("Closing the plan"))?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "\u{feff}# Sign In Through the Library\r\n\
        \r\n\
        Context: the button moves.\r\n\
        \r\n\
        ## Phase 45: Sign in\r\n\
        - [ ] Task 45.1 The button\r\n\
        \x20 - lives in the header\r\n\
        \x20 - [ ] a nested box is part of the task\r\n\
        - [ ] Seal\r\n\
        - [x] Task 45.2 The link\r\n\
        - [x] Seal (abc1234)\r\n\
        \r\n\
        ### Phase 46 - Connect\r\n\
        - [ ] Task 46.1 (FLAGGED - needs the key) Connect Castle\r\n\
        - [ ] Sealed envelope art\r\n\
        \r\n\
        ```md\r\n\
        - [ ] an example inside a code block\r\n\
        ```\r\n\
        ## Carried open items\r\n\
        ### From the old plan\r\n\
        - [ ] Task 12.3 old work\r\n\
        ## Verification\r\n\
        - [ ] cargo test passes\r\n";

    fn kinds_and_states(plan: &ParsedPlan) -> Vec<(&str, &str)> {
        plan.items.iter().map(|i| (i.kind, i.state)).collect()
    }

    #[test]
    fn a_plan_is_read_into_its_title_phases_and_boxes() {
        let plan = parse_plan(PLAN);
        assert_eq!(plan.title, "Sign In Through the Library");
        assert_eq!(plan.phases, vec!["45", "46"]);
        assert_eq!(plan.phase().as_deref(), Some("Phase 45-46"));
        assert_eq!(
            kinds_and_states(&plan),
            vec![
                ("phase", "open"),
                ("task", "open"),
                ("seal", "open"),
                ("task", "done"),
                ("seal", "done"),
                ("phase", "open"),
                ("task", "flagged"),
                ("task", "open"),
                ("task", "carried"),
                ("task", "open"),
            ]
        );
        assert_eq!(plan.boxes(), 8);
        assert_eq!(plan.items[0].text, "Phase 45: Sign in");
        assert_eq!(plan.items[5].text, "Phase 46 - Connect");
        // the indented lines under a box are part of its text, a nested box included
        assert_eq!(plan.items[1].text, "Task 45.1 The button\n  - lives in the header\n  - [ ] a nested box is part of the task");
        assert_eq!(first_line(&plan.items[1].text), "Task 45.1 The button");
        // a box in a code block is an example; "Verification" ends the carried section
        assert!(plan.items.iter().all(|i| !i.text.contains("example")));
        assert_eq!(plan.items[9].text, "cargo test passes");
    }

    #[test]
    fn a_seal_is_only_the_seal() {
        assert_eq!(kind_of("Seal"), "seal");
        assert_eq!(kind_of("seal (abc1234)"), "seal");
        assert_eq!(kind_of("Seal - nothing changed"), "seal");
        assert_eq!(kind_of("Sealed envelope art"), "task");
        assert_eq!(kind_of("Task 1 Seal the deal"), "task");
    }

    #[test]
    fn headings_and_boxes_are_told_from_other_lines() {
        assert_eq!(heading("## Phase 1"), Some((2, "Phase 1")));
        assert_eq!(heading("#hashtag"), None);
        assert_eq!(heading("####### seven"), None);
        assert_eq!(checkbox("- [ ] one"), Some((false, "one")));
        assert_eq!(checkbox("* [X] two"), Some((true, "two")));
        assert_eq!(checkbox("- [ ]"), None);
        assert_eq!(checkbox("- [-] maybe"), None);
        assert_eq!(checkbox("  - [ ] indented"), None);
        assert_eq!(checkbox("- アリス"), None);
        assert_eq!(phase_of("Phase 3 — Polish"), Some("3".into()));
        assert_eq!(phase_of("phase 4.1: tidy"), Some("4.1".into()));
        assert_eq!(phase_of("Phase: later"), None);
        assert_eq!(phase_of("Phases 1-3"), None);
        assert_eq!(phase_of("Verification"), None);
        assert_eq!(phase_of("Phase one"), Some("one".into()));
    }

    #[test]
    fn a_plan_with_no_title_or_phase_still_reads() {
        let plan = parse_plan("Just do these:\n- [ ] one\n- [x] two\n");
        assert_eq!(plan.title, "");
        assert_eq!(plan.phase(), None);
        assert_eq!(kinds_and_states(&plan), vec![("task", "open"), ("task", "done")]);
        assert_eq!(parse_plan("# Only words\n\nNo boxes here.\n").boxes(), 0);
        // a single phase is named alone
        assert_eq!(parse_plan("## Phase 7\n- [ ] x\n").phase().as_deref(), Some("Phase 7"));
    }

    #[test]
    fn folders_compare_without_their_trailing_slash() {
        assert_eq!(folder_key("  /home/rin/castle/ "), if cfg!(windows) { "\\home\\rin\\castle" } else { "/home/rin/castle" });
        assert_eq!(folder_key("/"), if cfg!(windows) { "\\" } else { "/" });
        if cfg!(windows) {
            assert_eq!(folder_key("D:/Work/Castle/"), "d:\\work\\castle");
            assert_eq!(folder_key("d:\\work\\castle"), folder_key("D:\\Work\\Castle\\"));
            assert_eq!(folder_key(r"\\?\D:\Work\Castle"), "d:\\work\\castle");
        } else {
            assert_ne!(folder_key("/home/Rin"), folder_key("/home/rin"));
        }
    }

    #[test]
    fn the_screen_reads_camel_case() {
        let view = PlanView {
            id: 3,
            title: "T".into(),
            phase: None,
            status: "open".into(),
            items: vec![PlanItem { ordinal: 1, kind: "task".into(), text: "x".into(), state: "open".into() }],
        };
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"id":3,"title":"T","phase":null,"status":"open","items":[{"ordinal":1,"kind":"task","text":"x","state":"open"}]}"#
        );
    }

    fn rows_of(plan: &ParsedPlan) -> Vec<PlanItem> {
        plan.items
            .iter()
            .enumerate()
            .map(|(n, i)| PlanItem { ordinal: n as u32 + 1, kind: i.kind.into(), text: i.text.clone(), state: i.state.into() })
            .collect()
    }

    #[test]
    fn a_plan_is_written_back_with_its_boxes_ticked_from_the_rows() {
        let parsed = parse_plan(PLAN);
        // rows as they were: not one byte changes, line ends and the BOM included
        assert_eq!(render_plan(PLAN, &rows_of(&parsed)), PLAN);

        let mut rows = rows_of(&parsed);
        rows[1].state = "done".into(); // Task 45.1
        rows[3].state = "carried".into(); // Task 45.2, ticked when approved
        rows[9].state = "done".into(); // cargo test passes
        let rendered = render_plan(PLAN, &rows);
        let again = parse_plan(&rendered);
        let words = |p: &ParsedPlan| p.items.iter().map(|i| (i.kind, i.text.clone())).collect::<Vec<_>>();
        assert_eq!(words(&again), words(&parsed));
        let done: Vec<usize> = again.items.iter().enumerate().filter(|(_, i)| i.state == "done").map(|(n, _)| n + 1).collect();
        assert_eq!(done, vec![2, 5, 10]);
        assert!(rendered.contains("- [x] Task 45.1 The button\r\n") && rendered.contains("- [ ] Task 45.2 The link\r\n"));
        // the example in the code block and the nested box stay as written
        assert!(rendered.contains("```md\r\n- [ ] an example inside a code block\r\n```"));
        assert!(rendered.contains("  - [ ] a nested box is part of the task\r\n"));
        assert_eq!(rendered.len(), PLAN.len());
        // a box already as its row says keeps its own mark
        assert_eq!(render_plan("- [X] one\n- [ ] two\n", &[PlanItem { ordinal: 1, kind: "task".into(), text: String::new(), state: "done".into() }]), "- [X] one\n- [ ] two\n");
    }

    fn meta() -> PlanMeta {
        let mut states = BTreeMap::new();
        states.insert(2, BoxState { state: "done".into(), note: Some("went well --> next 💜\nline two".into()), done: Some("2026-11-20 09:15:00".into()) });
        states.insert(3, BoxState { state: "flagged".into(), note: None, done: None });
        PlanMeta { folder: "D:/Work/Castle".into(), status: "archived".into(), approved: "2026-11-19 22:01:05".into(), source: None, states, ticked: vec![4] }
    }

    #[test]
    fn a_plan_header_is_one_line_and_reads_back() {
        let header = plan_header(&meta());
        assert!(header.starts_with("<!-- castle-plan {") && header.ends_with("} -->") && !header.contains('\n'));
        assert_eq!(header.matches("-->").count(), 1, "{header}");
        let file = format!("{header}\r\n# Plan\n- [ ] x\n");
        let (read, rest) = read_plan_header(&file).unwrap();
        assert_eq!(read, meta());
        assert_eq!(rest, "# Plan\n- [ ] x\n");
        let (_, empty) = read_plan_header(&header).unwrap();
        assert_eq!(empty, "");
        // the source travels as null when there is none
        assert!(header.contains("\"source\":null"));
    }

    #[test]
    fn a_file_without_the_header_is_not_a_plan_file() {
        assert!(read_plan_header("# Plan\n- [ ] x\n").is_none());
        assert!(read_plan_header("").is_none());
        assert!(read_plan_header("<!-- castle-plan not json -->\n- [ ] x\n").is_none());
        assert!(read_plan_header("\n<!-- castle-plan {} -->\n").is_none(), "only the first line");
    }

    #[test]
    fn a_plan_file_gives_back_the_approved_text_and_the_states() {
        let body = "# Small\n## Phase 1: Start\n- [ ] Task 1.1 One\n- [x] Task 1.2 Two\n- [ ] Task 1.3 Three\n```\n- [ ] example\n```\n";
        let mut states = BTreeMap::new();
        states.insert(2, BoxState { state: "done".into(), note: Some("ok".into()), done: Some("2026-11-20 09:15:00".into()) });
        states.insert(3, BoxState { state: "carried".into(), note: None, done: None });
        states.insert(4, BoxState { state: "flagged".into(), note: Some("the key".into()), done: None });
        let meta = PlanMeta { folder: "D:/Work".into(), status: "open".into(), approved: "2026-11-19 22:01:05".into(), source: Some("C:/p.md".into()), states, ticked: Vec::new() };
        let file = plan_file(meta.clone(), body);
        let (read, rendered) = read_plan_header(&file).unwrap();
        assert_eq!(read.ticked, vec![3], "Task 1.2 was ticked when approved");
        assert_eq!(rendered, "# Small\n## Phase 1: Start\n- [x] Task 1.1 One\n- [ ] Task 1.2 Two\n- [ ] Task 1.3 Three\n```\n- [ ] example\n```\n");
        assert_eq!(approved_text(rendered, &read), body);
        assert_eq!(states_from_file(rendered, &read), meta.states);
        // a box ticked by hand while the plan was a file wins over the header
        let by_hand = rendered.replace("- [ ] Task 1.3", "- [x] Task 1.3");
        let states = states_from_file(&by_hand, &read);
        assert_eq!(states[&4], BoxState { state: "done".into(), note: Some("the key".into()), done: None });
        assert_eq!(states[&2], meta.states[&2]);
        assert!(datetime(Some("2026-11-20 09:15:00")).is_some() && datetime(Some("yesterday")).is_none() && datetime(None).is_none());
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): a plan copied,
    /// copied again, walked to the end, and done. Uses and drops its own database.
    #[test]
    #[ignore]
    fn live_copy_walk_and_finish() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &admin, &admin_pw, &format!("Plans Test {}", std::process::id())).unwrap();
        let mut conn = crate::memdb::open(&access).unwrap();
        let folder = "D:/Work/Castle/";
        let plan = "# Small plan\n## Phase 1: Start\n- [ ] Task 1.1 One\n  - detail\n- [ ] Seal\n- [x] Task 1.2 Two\n- [ ] Task 1.3 (BLOCKED - the key) Three\n";

        let (id, boxes, duplicate) = copy_plan(&mut conn, folder, plan, Some("C:/plans/small.md")).unwrap();
        assert_eq!((boxes, duplicate), (4, false));
        // the same plan again, written with other line ends, is the same plan
        assert_eq!(copy_plan(&mut conn, folder, &plan.replace('\n', "\r\n"), None).unwrap(), (id, 4, true));
        assert!(copy_plan(&mut conn, folder, "# Words only\n", None).is_err());
        assert_eq!(plan_body(&mut conn, id).unwrap(), plan);
        assert!(plan_body(&mut conn, id + 100).is_err());

        let open = open_plans(&mut conn, folder).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!((open[0].title.as_str(), open[0].phase.as_deref(), open[0].status.as_str()), ("Small plan", Some("Phase 1"), "open"));
        let rows: Vec<(u32, &str, &str)> = open[0].items.iter().map(|i| (i.ordinal, i.kind.as_str(), i.state.as_str())).collect();
        assert_eq!(rows, vec![(1, "phase", "open"), (2, "task", "open"), (3, "seal", "open"), (4, "task", "done"), (5, "task", "flagged")]);
        assert_eq!(open[0].items[1].text, "Task 1.1 One\n  - detail");
        if cfg!(windows) {
            assert_eq!(open_plans(&mut conn, "d:\\work\\castle").unwrap().len(), 1);
        }
        assert!(open_plans(&mut conn, "D:/Elsewhere").unwrap().is_empty());
        assert_eq!(current_plan(&mut conn, folder).unwrap(), Some(id));

        // walk: the task, its Seal, then the flagged box is passed by
        assert_eq!(next_item(&mut conn, folder).unwrap().unwrap().item.ordinal, 2);
        assert_eq!(left(&mut conn, folder).unwrap(), Left { open: 2, done: 1, carried: 0, flagged: 1 });
        let marked = mark(&mut conn, folder, None, Which::Next, "done", Some("went well")).unwrap();
        assert_eq!((marked.plan_id, marked.ordinal, marked.plan_done), (id, 2, false));
        assert_eq!(next_item(&mut conn, folder).unwrap().unwrap().item.kind, "seal");
        mark(&mut conn, folder, None, Which::Next, "done", None).unwrap();
        assert_eq!(next_item(&mut conn, folder).unwrap(), None);
        assert!(mark(&mut conn, folder, None, Which::Next, "done", None).is_err());
        assert!(mark(&mut conn, folder, None, Which::Ordinal(1), "done", None).is_err());
        assert!(mark(&mut conn, folder, None, Which::Ordinal(2), "done", None).is_err());
        assert!(mark(&mut conn, folder, None, Which::Ordinal(9), "done", None).is_err());
        assert!(mark(&mut conn, folder, None, Which::Ordinal(5), "open", None).is_err());
        let note: Option<Option<String>> = conn.exec_first("SELECT note FROM project_plan_items WHERE plan_id = ? AND ordinal = 2", (id,)).unwrap();
        assert_eq!(note, Some(Some("went well".into())));

        // the last box done: the plan is done and leaves the open list
        let last = mark(&mut conn, folder, None, Which::Ordinal(5), "done", None).unwrap();
        assert!(last.plan_done);
        assert!(open_plans(&mut conn, folder).unwrap().is_empty());
        let status: Option<String> = conn.exec_first("SELECT status FROM project_plans WHERE id = ?", (id,)).unwrap();
        assert_eq!(status.as_deref(), Some("done"));
        // opened again by id when a box is carried after all
        assert!(!mark(&mut conn, folder, Some(id), Which::Ordinal(5), "carried", None).unwrap().plan_done);
        assert_eq!(open_plans(&mut conn, folder).unwrap().len(), 1);
        let done_at: Option<Option<String>> = conn.exec_first("SELECT DATE_FORMAT(done_at, '%Y') FROM project_plan_items WHERE plan_id = ? AND ordinal = 5", (id,)).unwrap();
        assert_eq!(done_at, Some(None));

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
