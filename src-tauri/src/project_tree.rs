//! Sub-projects and tasks (CASTLE-6): a project can sit under one parent, one level deep, and
//! every project keeps a to-do list. The PROJECTS room (project_room.rs) and the companion's
//! door (`castle-memory project ... --parent`, `castle-memory task ...`) both end here.
//!
//! ```text
//!  Castle                      parent: its progress is its own tasks + its sub-projects' tasks
//!   ├ Update chip (CASTLE-3)   sub-project, its own shelf, its own tasks
//!   └ Codex usage (CASTLE-4)
//! ```
//!
//! A sub-project is found by its own name or by "Parent · Child", so a name the companion used
//! before the split ("Castle · Update chip (CASTLE-3)") still finds the project after it.

use crate::door::{move_project, Shelf};
use mysql::prelude::Queryable;
use mysql::Conn;
use serde::Serialize;
use std::collections::HashMap;

/// What joins a parent's name and a sub-project's name ("Castle · Update chip").
pub const JOIN: &str = " · ";

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("{what} did not work.")
}

/// One project as the tree needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u64,
    pub name: String,
    pub shelf: String,
    pub parent: Option<u64>,
    pub status: Option<String>,
}

/// Every project, newest first.
pub fn nodes(conn: &mut Conn) -> Vec<Node> {
    conn.query::<(u64, String, String, Option<u64>, Option<String>), _>("SELECT id, name, shelf, parent_id, status FROM projects ORDER BY updated_at DESC, id")
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name, shelf, parent, status)| Node { id, name, shelf, parent, status })
        .collect()
}

/// (done, all) tasks of each project on its own.
pub fn own_progress(conn: &mut Conn) -> HashMap<u64, (u32, u32)> {
    conn.query::<(u64, u32, u32), _>("SELECT project_id, CAST(SUM(done_at IS NOT NULL) AS UNSIGNED), COUNT(*) FROM project_tasks GROUP BY project_id")
        .unwrap_or_default()
        .into_iter()
        .map(|(id, done, all)| (id, (done, all)))
        .collect()
}

/// (done, all) of each project with its sub-projects' tasks counted in its own.
pub fn rolled_progress(nodes: &[Node], own: &HashMap<u64, (u32, u32)>) -> HashMap<u64, (u32, u32)> {
    let mut out: HashMap<u64, (u32, u32)> = nodes.iter().map(|n| (n.id, own.get(&n.id).copied().unwrap_or((0, 0)))).collect();
    for n in nodes {
        if let (Some(parent), Some((d, a))) = (n.parent, own.get(&n.id)) {
            if let Some(p) = out.get_mut(&parent) {
                p.0 += d;
                p.1 += a;
            }
        }
    }
    out
}

/// The nodes in tree order: each top-level project, then its sub-projects (in the given order).
/// A sub-project whose parent is gone is top-level.
pub fn tree_order(nodes: &[Node]) -> Vec<(&Node, bool)> {
    let ids: Vec<u64> = nodes.iter().map(|n| n.id).collect();
    let is_top = |n: &Node| n.parent.is_none_or(|p| !ids.contains(&p));
    let mut out = Vec::new();
    for top in nodes.iter().filter(|n| is_top(n)) {
        out.push((top, false));
        out.extend(nodes.iter().filter(|n| n.parent == Some(top.id)).map(|n| (n, true)));
    }
    out
}

/// A project's id by its name, or by "Parent · Child" for a sub-project.
pub fn find(conn: &mut Conn, name: &str) -> Result<Option<u64>, String> {
    let exact: Option<u64> = conn.exec_first("SELECT id FROM projects WHERE name = ?", (name,)).map_err(sql("Reading the project"))?;
    if exact.is_some() {
        return Ok(exact);
    }
    let Some((parent, child)) = name.split_once(JOIN) else { return Ok(None) };
    conn.exec_first(
        "SELECT c.id FROM projects c JOIN projects p ON p.id = c.parent_id WHERE p.name = ? AND c.name = ?",
        (parent.trim(), child.trim()),
    )
    .map_err(sql("Reading the project"))
}

pub fn need(conn: &mut Conn, name: &str) -> Result<u64, String> {
    find(conn, name)?.ok_or(format!("No project called {name}."))
}

fn name_of(conn: &mut Conn, id: u64) -> Result<String, String> {
    let name: Option<String> = conn.exec_first("SELECT name FROM projects WHERE id = ?", (id,)).map_err(sql("Reading the project"))?;
    name.ok_or("That project is no longer there.".into())
}

/// Why `parent` cannot take a sub-project (one level deep), if it cannot.
pub fn parent_refusal(conn: &mut Conn, parent: u64, child: Option<u64>) -> Result<Option<String>, String> {
    let row: Option<(String, Option<u64>)> = conn.exec_first("SELECT name, parent_id FROM projects WHERE id = ?", (parent,)).map_err(sql("Reading the project"))?;
    let (name, its_parent) = row.ok_or("That parent is no longer there.")?;
    if Some(parent) == child {
        return Ok(Some(format!("{name} cannot be its own parent.")));
    }
    if its_parent.is_some() {
        return Ok(Some(format!("{name} is itself a sub-project; sub-projects go one level deep.")));
    }
    if let Some(child) = child {
        let kids: Option<u64> = conn.exec_first("SELECT COUNT(*) FROM projects WHERE parent_id = ?", (child,)).map_err(sql("Reading the project"))?;
        if kids.unwrap_or(0) > 0 {
            let child_name = name_of(conn, child)?;
            return Ok(Some(format!("{child_name} has sub-projects of its own, so it cannot go under {name}.")));
        }
    }
    Ok(None)
}

/// Puts `child` under `parent`, or back on its own with None.
pub fn set_parent(conn: &mut Conn, child: u64, parent: Option<u64>) -> Result<(), String> {
    if let Some(parent) = parent {
        if let Some(why) = parent_refusal(conn, parent, Some(child))? {
            return Err(why);
        }
    }
    conn.exec_drop("UPDATE projects SET parent_id = ? WHERE id = ?", (parent, child)).map_err(sql("Moving the project under its parent"))
}

/// The sub-projects that sit on the same shelf as their parent: the ones a move of the parent
/// asks about.
pub fn kids_beside(conn: &mut Conn, parent: u64) -> Result<Vec<(u64, String)>, String> {
    conn.exec("SELECT c.id, c.name FROM projects c JOIN projects p ON p.id = c.parent_id WHERE p.id = ? AND c.shelf = p.shelf ORDER BY c.id", (parent,))
        .map_err(sql("Reading the sub-projects"))
}

/// Moves a project and, when asked, the sub-projects that stood on the same shelf with it.
pub fn move_with_kids(conn: &mut Conn, id: u64, to: Shelf, note: Option<&str>, with_kids: bool) -> Result<String, String> {
    let kids = kids_beside(conn, id)?;
    let mut line = move_project(conn, id, to, note)?;
    if with_kids {
        for (kid, _) in &kids {
            move_project(conn, *kid, to, note)?;
        }
        if !kids.is_empty() {
            line.push_str(&format!(" with {} sub-project(s)", kids.len()));
        }
    } else if !kids.is_empty() {
        line.push_str(&format!(" ({} sub-project(s) stayed where they were)", kids.len()));
    }
    Ok(line)
}

// ---------- tasks ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub done: bool,
    pub due: Option<String>,
}

pub fn tasks(conn: &mut Conn, project: u64) -> Result<Vec<Task>, String> {
    let rows: Vec<(u64, String, bool, Option<String>)> = conn
        .exec("SELECT id, title, done_at IS NOT NULL, DATE_FORMAT(due_date, '%Y-%m-%d') FROM project_tasks WHERE project_id = ? ORDER BY position, id", (project,))
        .map_err(sql("Reading the tasks"))?;
    Ok(rows.into_iter().map(|(id, title, done, due)| Task { id, title, done, due }).collect())
}

/// A task's title as kept: one line, at most 300 characters.
pub fn clean_title(title: &str) -> Result<String, String> {
    let one: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return Err("A task needs a title.".into());
    }
    Ok(one.chars().take(300).collect())
}

/// Adds a task at the end of the list; with `remind` and a due date it also raises a reminder.
pub fn task_add(conn: &mut Conn, project: u64, title: &str, due: Option<&str>, remind: bool) -> Result<u64, String> {
    let title = clean_title(title)?;
    let due = due.map(str::trim).filter(|d| !d.is_empty());
    if let Some(d) = due {
        if !crate::door::is_date(d) {
            return Err(format!("{d} is not a date like 2026-11-10."));
        }
    }
    let name = name_of(conn, project)?;
    let reminder = match (remind, due) {
        (true, Some(d)) => {
            conn.exec_drop("INSERT INTO reminders (title, due_date, detail) VALUES (?, ?, ?)", (&title, d, format!("Task in {name}")))
                .map_err(sql("Raising the reminder"))?;
            Some(conn.last_insert_id())
        }
        (true, None) => return Err("A reminder needs a due date (--due 2026-11-20).".into()),
        _ => None,
    };
    conn.exec_drop(
        "INSERT INTO project_tasks (project_id, title, due_date, reminder_id, position) \
         SELECT ?, ?, ?, ?, COALESCE(MAX(position) + 1, 0) FROM project_tasks WHERE project_id = ?",
        (project, &title, due, reminder, project),
    )
    .map_err(sql("Adding the task"))?;
    let id = conn.last_insert_id();
    conn.exec_drop("UPDATE projects SET last_updated = DATE_FORMAT(NOW(), '%Y-%m-%d') WHERE id = ?", (project,)).map_err(sql("Adding the task"))?;
    Ok(id)
}

/// Ticks a task (or takes the tick off); its reminder closes and opens with it. The task's title.
pub fn task_set_done(conn: &mut Conn, id: u64, done: bool) -> Result<String, String> {
    let row: Option<(String, Option<u64>)> = conn.exec_first("SELECT title, reminder_id FROM project_tasks WHERE id = ?", (id,)).map_err(sql("Reading the task"))?;
    let (title, reminder) = row.ok_or(format!("No task #{id}."))?;
    let stamp = if done { "COALESCE(done_at, NOW())" } else { "NULL" };
    conn.exec_drop(format!("UPDATE project_tasks SET done_at = {stamp} WHERE id = ?"), (id,)).map_err(sql("Ticking the task"))?;
    if let Some(r) = reminder {
        let (state, at) = if done { ("done", "NOW()") } else { ("open", "NULL") };
        conn.exec_drop(format!("UPDATE reminders SET state = ?, done_at = {at} WHERE id = ?"), (state, r)).map_err(sql("Ticking the task"))?;
    }
    Ok(title)
}

pub fn task_remove(conn: &mut Conn, id: u64) -> Result<String, String> {
    let title: Option<String> = conn.exec_first("SELECT title FROM project_tasks WHERE id = ?", (id,)).map_err(sql("Reading the task"))?;
    let title = title.ok_or(format!("No task #{id}."))?;
    conn.exec_drop("DELETE FROM project_tasks WHERE id = ?", (id,)).map_err(sql("Removing the task"))?;
    Ok(title)
}

/// Puts a project's tasks in this order; tasks not named keep their place after them.
pub fn task_order(conn: &mut Conn, project: u64, ids: &[u64]) -> Result<(), String> {
    let mut all: Vec<u64> = tasks(conn, project)?.into_iter().map(|t| t.id).collect();
    let named: Vec<u64> = ids.iter().copied().filter(|i| all.contains(i)).collect();
    all.retain(|i| !named.contains(i));
    for (pos, id) in named.iter().chain(all.iter()).enumerate() {
        conn.exec_drop("UPDATE project_tasks SET position = ? WHERE id = ? AND project_id = ?", (pos as u32, id, project)).map_err(sql("Ordering the tasks"))?;
    }
    Ok(())
}

// ---------- the one-time split of "Parent · Child" names ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SplitItem {
    pub id: u64,
    pub name: String,
    /// the name it would carry under its parent
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    pub parent: String,
    /// true when a project of that name is already there (it becomes the parent)
    pub parent_exists: bool,
    pub items: Vec<SplitItem>,
}

/// "Castle · Update chip (CASTLE-3)" -> ("Castle", "Update chip (CASTLE-3)").
pub fn split_name(name: &str) -> Option<(String, String)> {
    let (a, b) = name.split_once(JOIN)?;
    let (a, b) = (a.trim(), b.trim());
    (!a.is_empty() && !b.is_empty()).then(|| (a.to_string(), b.to_string()))
}

/// The proposal, from the nodes: every top-level project without sub-projects whose name looks
/// grouped, by the parent it names. A parent that is itself a sub-project is left out.
pub fn proposals(nodes: &[Node]) -> Vec<Split> {
    let mut out: Vec<Split> = Vec::new();
    for n in nodes.iter().filter(|n| n.parent.is_none() && n.shelf != "deleted" && !nodes.iter().any(|k| k.parent == Some(n.id))) {
        let Some((parent, rest)) = split_name(&n.name) else { continue };
        let there = nodes.iter().find(|p| p.name == parent);
        if there.is_some_and(|p| p.parent.is_some()) {
            continue;
        }
        // the shorter name must be free (or be this very project)
        let new_name = if nodes.iter().any(|o| o.name == rest && o.id != n.id) { n.name.clone() } else { rest };
        let item = SplitItem { id: n.id, name: n.name.clone(), new_name };
        match out.iter_mut().find(|s| s.parent == parent) {
            Some(s) => s.items.push(item),
            None => out.push(Split { parent, parent_exists: there.is_some(), items: vec![item] }),
        }
    }
    out.sort_by(|a, b| b.items.len().cmp(&a.items.len()).then(a.parent.cmp(&b.parent)));
    out
}

/// Applies the ticked part of the proposal: the parent is made when it is not there yet, each
/// ticked project is renamed to its short name and put under it. A line saying what was done.
pub fn apply_split(conn: &mut Conn, ids: &[u64]) -> Result<String, String> {
    let all = nodes(conn);
    let mut moved = 0;
    let mut parents_made = 0;
    for split in proposals(&all) {
        let ticked: Vec<&SplitItem> = split.items.iter().filter(|i| ids.contains(&i.id)).collect();
        if ticked.is_empty() {
            continue;
        }
        let parent = match find(conn, &split.parent)? {
            Some(id) => id,
            None => {
                conn.exec_drop("INSERT INTO projects (name, shelf, last_updated) VALUES (?, 'active', DATE_FORMAT(NOW(), '%Y-%m-%d'))", (&split.parent,))
                    .map_err(sql("Making the parent project"))?;
                parents_made += 1;
                conn.last_insert_id()
            }
        };
        for item in ticked {
            conn.exec_drop("UPDATE projects SET name = ?, parent_id = ? WHERE id = ?", (&item.new_name, parent, item.id)).map_err(sql("Moving the project under its parent"))?;
            moved += 1;
        }
    }
    Ok(format!("PROJECTS {moved} moved under a parent, {parents_made} parent(s) made"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, name: &str, shelf: &str, parent: Option<u64>) -> Node {
        Node { id, name: name.into(), shelf: shelf.into(), parent, status: None }
    }

    #[test]
    fn the_tree_puts_each_sub_project_under_its_parent() {
        let nodes = vec![node(3, "Update chip", "active", Some(1)), node(2, "Montee", "active", None), node(1, "Castle", "active", None), node(4, "Lost", "paused", Some(99))];
        let order: Vec<(u64, bool)> = tree_order(&nodes).into_iter().map(|(n, kid)| (n.id, kid)).collect();
        assert_eq!(order, vec![(2, false), (1, false), (3, true), (4, false)]);
    }

    #[test]
    fn a_parent_counts_its_sub_projects_tasks() {
        let nodes = vec![node(1, "Castle", "active", None), node(2, "A", "active", Some(1)), node(3, "B", "paused", Some(1)), node(4, "Solo", "active", None)];
        let own = HashMap::from([(1, (1, 1)), (2, (1, 3)), (3, (0, 2))]);
        let rolled = rolled_progress(&nodes, &own);
        assert_eq!(rolled[&1], (2, 6));
        assert_eq!(rolled[&2], (1, 3));
        assert_eq!(rolled[&4], (0, 0));
    }

    #[test]
    fn grouped_names_are_proposed_by_their_parent() {
        let nodes = vec![
            node(1, "Castle · Update chip (CASTLE-3)", "active", None),
            node(2, "Castle · Codex usage (CASTLE-4)", "paused", None),
            node(3, "Montee · Workstation assignment", "active", None),
            node(4, "Workstation assignment", "active", None),
            node(5, "Montee", "active", None),
            node(6, "Castle Mac conversion", "active", None),
            node(7, "Old · Gone", "deleted", None),
        ];
        let found = proposals(&nodes);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].parent.as_str(), found[0].parent_exists, found[0].items.len()), ("Castle", false, 2));
        assert_eq!(found[0].items[0].new_name, "Update chip (CASTLE-3)");
        // the short name is taken, so it keeps its whole name under Montee
        assert_eq!((found[1].parent.as_str(), found[1].parent_exists), ("Montee", true));
        assert_eq!(found[1].items[0].new_name, "Montee · Workstation assignment");
    }

    #[test]
    fn a_project_that_already_has_sub_projects_or_a_parent_is_not_proposed() {
        let nodes = vec![node(1, "A · B", "active", None), node(2, "kid", "active", Some(1)), node(3, "C · D", "active", Some(9)), node(9, "C", "active", None)];
        assert!(proposals(&nodes).is_empty());
        assert_eq!(split_name("Castle · Update chip"), Some(("Castle".into(), "Update chip".into())));
        assert_eq!(split_name("Castle Mac"), None);
        assert_eq!(split_name(" · x"), None);
    }

    #[test]
    fn a_task_title_is_one_trimmed_line() {
        assert_eq!(clean_title("  Fix the\n chip  ").unwrap(), "Fix the chip");
        assert!(clean_title("   ").is_err());
        assert_eq!(clean_title(&"x".repeat(400)).unwrap().len(), 300);
    }

    /// Against a real server, on demand (CASTLE_TEST_DB="<port>:<admin>:<password>"): the
    /// companion's verbs, the room's moves, the split and the export, end to end.
    #[test]
    #[ignore]
    fn live_sub_projects_and_tasks() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &admin, &admin_pw, &format!("Tree Test {}", std::process::id())).unwrap();
        let mut conn = crate::memdb::open(&access).unwrap();
        let folder = std::env::temp_dir().join(format!("castle-tree-live-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut go = |conn: &mut Conn, line: &[&str]| crate::door::run(conn, &crate::door::parse(&line.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(), &folder);

        // a database still at 016 (the app not restarted since the update): the door brings it up first
        conn.query_drop("DROP TABLE project_tasks").unwrap();
        conn.query_drop("ALTER TABLE projects DROP FOREIGN KEY fk_project_parent, DROP COLUMN parent_id").unwrap();
        conn.query_drop("DELETE FROM schema_migrations WHERE name = '017_project_tree'").unwrap();
        assert_eq!(go(&mut conn, &["project", "list"]).unwrap(), "PROJECTS none");

        // a parent and two sub-projects; a missing or nested parent is refused before anything is saved
        go(&mut conn, &["project", "add", "--name", "Castle", "--status", "0.2.23-p"]).unwrap();
        assert_eq!(go(&mut conn, &["project", "add", "--name", "Update chip", "--parent", "Castle"]).unwrap(), "PROJECT saved: Update chip (under Castle)");
        go(&mut conn, &["project", "add", "--name", "Codex usage", "--parent", "Castle", "--status", "built"]).unwrap();
        assert!(go(&mut conn, &["project", "add", "--name", "Ghost", "--parent", "Nowhere"]).is_err());
        assert!(go(&mut conn, &["project", "add", "--name", "Deeper", "--parent", "Update chip"]).unwrap_err().contains("one level deep"));
        assert!(find(&mut conn, "Ghost").unwrap().is_none() && find(&mut conn, "Deeper").unwrap().is_none());
        // a parent cannot go under another
        go(&mut conn, &["project", "add", "--name", "Montee"]).unwrap();
        assert!(go(&mut conn, &["project", "update", "--name", "Castle", "--parent", "Montee"]).unwrap_err().contains("has sub-projects"));

        // "Parent · Child" finds a sub-project, and an update never makes a second row
        assert_eq!(go(&mut conn, &["project", "update", "--name", "Castle · Update chip", "--status", "walkthrough"]).unwrap(), "PROJECT saved: Castle · Update chip");
        let count: Option<u64> = conn.query_first("SELECT COUNT(*) FROM projects").unwrap();
        assert_eq!(count, Some(4));
        assert!(go(&mut conn, &["project", "show", "--name", "Castle · Update chip"]).unwrap().contains("Status: walkthrough\nParent: Castle"));

        // tasks: add, tick, undo, order, remove; one with a reminder that closes with it
        assert_eq!(go(&mut conn, &["task", "add", "--project", "Update chip", "--title", "Move the chip"]).unwrap().split(' ').next(), Some("TASK"));
        go(&mut conn, &["task", "add", "--project", "Update chip", "--title", "Check  Windows\n", "--due", "2026-11-25", "--remind"]).unwrap();
        go(&mut conn, &["task", "add", "--project", "Codex usage", "--title", "Walkthrough"]).unwrap();
        assert!(go(&mut conn, &["task", "add", "--project", "Update chip", "--title", "x", "--remind"]).unwrap_err().contains("due date"));
        assert!(go(&mut conn, &["task", "add", "--project", "Update chip", "--title", "x", "--due", "Friday"]).is_err());
        let chip = need(&mut conn, "Update chip").unwrap();
        let ids: Vec<u64> = tasks(&mut conn, chip).unwrap().iter().map(|t| t.id).collect();
        assert_eq!(tasks(&mut conn, chip).unwrap()[1].title, "Check Windows");
        let reminder: Option<String> = conn.query_first("SELECT state FROM reminders WHERE title = 'Check Windows'").unwrap();
        assert_eq!(reminder.as_deref(), Some("open"));
        go(&mut conn, &["task", "done", "--id", &ids[1].to_string()]).unwrap();
        let reminder: Option<String> = conn.query_first("SELECT state FROM reminders WHERE title = 'Check Windows'").unwrap();
        assert_eq!(reminder.as_deref(), Some("done"));
        go(&mut conn, &["task", "undo", "--id", &ids[1].to_string()]).unwrap();
        // ticked on HOME's TODAY card, the reminder ticks its task; Undo takes both back (F9)
        let rid: u64 = conn.query_first("SELECT id FROM reminders WHERE title = 'Check Windows'").unwrap().unwrap();
        assert_eq!(crate::store::reminder_set_done(&mut conn, rid, true).unwrap(), "Check Windows");
        assert!(tasks(&mut conn, chip).unwrap()[1].done);
        assert!(crate::store::reminder_set_done(&mut conn, rid, true).unwrap_err().contains("No open reminder"));
        crate::store::reminder_set_done(&mut conn, rid, false).unwrap();
        assert!(!tasks(&mut conn, chip).unwrap()[1].done);
        let state: Option<String> = conn.query_first("SELECT state FROM reminders WHERE title = 'Check Windows'").unwrap();
        assert_eq!(state.as_deref(), Some("open"));
        go(&mut conn, &["task", "done", "--id", &format!("#{}", ids[0])]).unwrap();
        task_order(&mut conn, chip, &[ids[1]]).unwrap();
        assert_eq!(go(&mut conn, &["task", "list", "--project", "Update chip"]).unwrap(), format!("#{} [ ] Check Windows (by 2026-11-25)\n#{} [x] Move the chip", ids[1], ids[0]));
        assert!(go(&mut conn, &["task", "done", "--id", "999999"]).is_err());

        // the list: each sub-project under its parent, the parent counting its sub-projects' tasks
        let list = go(&mut conn, &["project", "list"]).unwrap();
        assert!(list.contains("[active] Castle · 0.2.23-p · ") && list.contains(" · 2 sub-project(s) · 1/3 tasks"), "{list}");
        assert!(list.contains("[active]   ↳ Update chip · walkthrough"), "{list}");

        // moving the parent asks about the sub-projects beside it; the CLI moves them only when told
        let castle = need(&mut conn, "Castle").unwrap();
        assert_eq!(kids_beside(&mut conn, castle).unwrap().len(), 2);
        let line = go(&mut conn, &["project", "pause", "--name", "Castle", "--note", "release first"]).unwrap();
        assert!(line.ends_with("(2 sub-project(s) stayed where they were); add --with-subprojects to move them too"), "{line}");
        go(&mut conn, &["project", "resume", "--name", "Castle"]).unwrap();
        assert_eq!(go(&mut conn, &["project", "archive", "--name", "Castle", "--with-subprojects"]).unwrap(), "PROJECT archived: Castle (still readable on the Archived shelf) with 2 sub-project(s)");
        // a sub-project resumed on its own shows its parent on the active shelf, in italics
        go(&mut conn, &["project", "resume", "--name", "Codex usage"]).unwrap();
        let shelves = crate::store::project_shelves(&mut conn);
        let active = shelves.iter().find(|s| s.name == "Active").unwrap();
        // newest change first; the archived parent heads its resumed sub-project
        let titles: Vec<&str> = active.pages.iter().map(|p| p.title.as_str()).collect();
        let at = titles.iter().position(|t| *t == "Castle").unwrap();
        assert_eq!((titles.len(), titles[at + 1]), (3, "Codex usage"), "{titles:?}");
        assert!(titles.contains(&"Montee"));
        assert_eq!(active.pages[at].tree.as_ref().unwrap().elsewhere.as_deref(), Some("archived"));
        assert_eq!(active.pages[at + 1].tree.as_ref().unwrap().parent.as_deref(), Some(format!("db/project/{castle}.md").as_str()));
        assert!(list_has(&go(&mut conn, &["project", "list"]).unwrap(), "[active] Codex usage · built"));
        // HOME: the active projects at the top of the tree (the parent is archived, so Codex usage stands alone)
        let home = crate::store::home(&mut conn, &folder, "2026-11-19");
        let mut names: Vec<&str> = home.projects.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["Codex usage", "Montee"]);
        go(&mut conn, &["project", "restore", "--name", "Castle", "--with-subprojects"]).unwrap();
        let home = crate::store::home(&mut conn, &folder, "2026-11-19");
        let mut names: Vec<&str> = home.projects.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["Castle", "Montee"]);
        assert_eq!(home.projects.iter().find(|p| p.name == "Castle").unwrap().tree.as_ref().map(|t| (t.kids, t.done, t.tasks)), Some((2, 1, 3)));

        // the one-time split: proposed, nothing renamed until applied, then a real tree
        go(&mut conn, &["project", "add", "--name", "TNEX · PKPS"]).unwrap();
        go(&mut conn, &["project", "add", "--name", "TNEX · SD Guthrie"]).unwrap();
        go(&mut conn, &["project", "add", "--name", "Montee · Workstation"]).unwrap();
        let split = go(&mut conn, &["project", "split"]).unwrap();
        assert!(split.contains("TNEX (new parent)\n  ↳ TNEX · PKPS -> PKPS"), "{split}");
        let pkps = need(&mut conn, "TNEX · PKPS").unwrap();
        let guthrie = need(&mut conn, "TNEX · SD Guthrie").unwrap();
        assert_eq!(apply_split(&mut conn, &[pkps, guthrie]).unwrap(), "PROJECTS 2 moved under a parent, 1 parent(s) made");
        assert!(go(&mut conn, &["project", "show", "--name", "TNEX · PKPS"]).unwrap().starts_with("# PKPS\n"));
        assert!(go(&mut conn, &["project", "show", "--name", "TNEX"]).unwrap().contains("Sub-projects:\n"));
        // the unticked one is untouched and still proposed
        assert_eq!(proposals(&nodes(&mut conn)).iter().map(|s| s.parent.as_str()).collect::<Vec<_>>(), vec!["Montee"]);

        // export: the parent and the tasks at the end of the page
        let out = folder.join("export");
        go(&mut conn, &["export", "--to", out.to_str().unwrap()]).unwrap();
        let page = std::fs::read_to_string(out.join("projects/active/update-chip.md")).unwrap();
        assert!(page.contains("**Parent:** Castle\n\n## Tasks\n- [ ] Check Windows (by 2026-11-25)\n- [x] Move the chip\n"), "{page}");

        // a parent removed for good leaves its sub-projects standing; a task goes with its project
        go(&mut conn, &["project", "delete", "--name", "Castle", "--confirmed"]).unwrap();
        conn.query_drop("UPDATE projects SET shelved_at = NOW() - INTERVAL 31 DAY WHERE name = 'Castle'").unwrap();
        crate::door::purge_deleted(&mut conn);
        let free: Option<Option<u64>> = conn.exec_first("SELECT parent_id FROM projects WHERE name = 'Update chip'", ()).unwrap();
        assert_eq!(free, Some(None));
        go(&mut conn, &["task", "add", "--project", "PKPS", "--title", "goes with it"]).unwrap();
        assert!(go(&mut conn, &["project", "delete", "--name", "TNEX", "--confirmed", "--with-subprojects"]).unwrap().ends_with("with 2 sub-project(s)"));
        conn.query_drop("UPDATE projects SET shelved_at = NOW() - INTERVAL 31 DAY WHERE shelf = 'deleted'").unwrap();
        crate::door::purge_deleted(&mut conn);
        assert!(find(&mut conn, "PKPS").unwrap().is_none());
        let left: Option<u64> = conn.query_first("SELECT COUNT(*) FROM project_tasks WHERE title = 'goes with it'").unwrap();
        assert_eq!(left, Some(0));

        // dump and restore carry the tree
        let dump = folder.join("dump.sql");
        crate::door::dump(&mut conn, &dump).unwrap();
        crate::door::restore(&mut conn, &dump).unwrap();
        assert_eq!(tasks(&mut conn, chip).unwrap().len(), 2);

        drop(conn);
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

    fn list_has(list: &str, start: &str) -> bool {
        list.lines().any(|l| l.starts_with(start))
    }
}
