//! SWEEP's Developer section: build output and package caches that can be rebuilt, found only in
//! the DEV room's project folders and their git worktrees, with a Scan of its own. Sweep's other
//! groups never look at project folders; this section never looks anywhere else (but the package
//! caches, cleaned through their own tools).
//!
//! On at the start: a project's Rust `target/debug`, its `target/release` but `release/bundle`
//! (every installer stays), the npm cache (`npm cache clean --force`), the pnpm store's unused
//! packages (`pnpm store prune`) and `~/.cargo/registry/cache`.
//! Off at the start, picked per item: a project's node_modules, .next and bin/obj (beside a .csproj);
//! a worktree's build output; a whole worktree, only when merged, clean and not in use, through
//! `git worktree remove` without --force (the branch stays).
//! Never: Docker data, databases and their snapshots or backups, anything git tracks, a dirty or
//! unpushed-unmerged worktree as a whole, anything a running program uses (shown with the reason).
//! Links are never followed. Every check runs again when Clean is pressed, and each item writes
//! one line to Sweep's log.

use crate::sweep::{self, Tally};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const GROUP_DEFAULT: &str = "default";
pub const GROUP_WORKTREES: &str = "worktrees";
pub const GROUP_PROJECTS: &str = "projects";

/// Folders the finder never enters: other people's data, not build output.
const NEVER_DIRS: &[&str] = &["docker", "volumes", "pgdata", "postgres", "mysql", "mongo", "redis", "backup", "backups", "snapshot", "snapshots", "dumps", "db"];
/// Files the cleaner never deletes, wherever they sit: databases and their dumps.
const NEVER_FILES: &[&str] = &["db", "sqlite", "sqlite3", "dump", "mdf", "ldf"];
/// How deep the finder looks inside a project for build folders (src-tauri/target is depth 2).
const MAX_DEPTH: usize = 6;

// ---------- what the sheet shows ----------

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevItem {
    /// `<kind>:<path>`, the same from one scan to the next
    pub id: String,
    /// default | worktrees | projects
    pub group: String,
    /// target-debug | target-release | build | worktree | npm | pnpm | cargo
    pub kind: String,
    pub name: String,
    /// what it is and how it goes ("keeps release/bundle: 4 files, 2.4 GB")
    pub detail: String,
    pub path: String,
    /// what a tick frees (a worktree: its build output)
    pub bytes: u64,
    /// false when the size is only known after (pnpm prunes what no project uses)
    pub bytes_known: bool,
    /// a worktree as a whole
    pub whole_bytes: u64,
    /// ticked at the start
    pub on: bool,
    /// a worktree's choices besides Leave: "build", "whole"
    pub choices: Vec<String>,
    /// why it cannot be cleaned now ("in use: node on :3000"), shown disabled
    pub blocked: Option<String>,
    /// uncommitted changes (a worktree)
    pub dirty: bool,
    // a worktree's git facts, for the sheet's line
    pub repo: String,
    pub branch: Option<String>,
    pub merged_into: Option<String>,
    pub pushed: bool,
    pub last_commit: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevScan {
    pub items: Vec<DevItem>,
    /// unix seconds
    pub scanned: u64,
    pub projects: usize,
    pub worktrees: usize,
}

/// One pick from the sheet: `how` is "clean" (a tick), "build" or "whole" (a worktree).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Pick {
    pub id: String,
    pub how: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevCleaned {
    pub id: String,
    pub name: String,
    pub path: String,
    pub how: String,
    pub freed: u64,
    pub ok: bool,
    /// why it stayed, or ''
    pub note: String,
}

// ---------- running a tool ----------

pub struct Ran {
    pub ok: bool,
    pub out: String,
    pub err: String,
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = send.send(bytes);
    });
    receive
}

/// Runs a program, waits at most `timeout`; None when it could not start or took too long.
fn run(program: &Path, args: &[&str], cwd: Option<&Path>, timeout: Duration) -> Option<Ran> {
    let mut command = Command::new(program);
    command.args(args).env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    let mut child = command.spawn().ok()?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => return None,
        }
    };
    let grace = Duration::from_secs(5);
    Some(Ran {
        ok: status.success(),
        out: String::from_utf8_lossy(&out.recv_timeout(grace).unwrap_or_default()).into_owned(),
        err: String::from_utf8_lossy(&err.recv_timeout(grace).unwrap_or_default()).into_owned(),
    })
}

fn git(dir: &Path, args: &[&str]) -> Option<Ran> {
    if !dir.is_dir() {
        return None;
    }
    run(Path::new("git"), args, Some(dir), Duration::from_secs(30))
}

// ---------- git worktrees (pure parser, tested) ----------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

/// Reads `git worktree list --porcelain`: blocks of `worktree <path>`, `HEAD`, `branch refs/heads/x`
/// or `detached`, and `bare`, `locked`, `prunable`. The first block is the main checkout.
pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut out: Vec<Worktree> = Vec::new();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            out.push(Worktree { path: PathBuf::from(path), ..Default::default() });
            continue;
        }
        let Some(last) = out.last_mut() else { continue };
        if let Some(branch) = line.strip_prefix("branch ") {
            last.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).to_string());
        } else if line == "bare" {
            last.bare = true;
        } else if line == "locked" || line.starts_with("locked ") {
            last.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            last.prunable = true;
        }
    }
    out
}

/// What git says about one worktree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitFacts {
    pub branch: Option<String>,
    pub merged_into: Option<String>,
    pub last_commit: Option<u64>,
    pub dirty: bool,
    pub pushed: bool,
}

/// Merged: its HEAD is in main or master (local or origin's). Dirty: `git status --porcelain` shows
/// anything (an unreadable status counts as dirty). Pushed: it has an upstream with nothing ahead.
pub fn git_facts(dir: &Path) -> GitFacts {
    let mut facts = GitFacts { dirty: true, ..Default::default() };
    if let Some(r) = git(dir, &["status", "--porcelain"]) {
        if r.ok {
            facts.dirty = !r.out.trim().is_empty();
        }
    }
    facts.branch = git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).filter(|r| r.ok).map(|r| r.out.trim().to_string()).filter(|b| !b.is_empty());
    facts.last_commit = git(dir, &["log", "-1", "--format=%ct"]).filter(|r| r.ok).and_then(|r| r.out.trim().parse().ok());
    for (reference, name) in [("refs/heads/main", "main"), ("refs/heads/master", "master"), ("refs/remotes/origin/main", "origin/main"), ("refs/remotes/origin/master", "origin/master")] {
        if facts.branch.as_deref() == Some(name) {
            continue; // main is never "merged into" itself
        }
        let exists = git(dir, &["rev-parse", "--verify", "--quiet", reference]).is_some_and(|r| r.ok);
        if exists && git(dir, &["merge-base", "--is-ancestor", "HEAD", reference]).is_some_and(|r| r.ok) {
            facts.merged_into = Some(name.to_string());
            break;
        }
    }
    let has_upstream = git(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).is_some_and(|r| r.ok);
    facts.pushed = has_upstream && git(dir, &["rev-list", "--count", "@{u}..HEAD"]).is_some_and(|r| r.ok && r.out.trim() == "0");
    facts
}

/// Whether git tracks anything inside `dir` (such a folder is never offered).
fn tracked(repo: &Path, dir: &Path) -> bool {
    let Ok(rel) = dir.strip_prefix(repo) else { return false };
    let rel = rel.to_string_lossy().replace('\\', "/");
    match git(repo, &["ls-files", "--", &rel]) {
        Some(r) if r.ok => !r.out.trim().is_empty(),
        Some(_) => false, // not a git folder
        None => repo.join(".git").exists(), // git did not answer: careful in a repo
    }
}

// ---------- what is in use (pure parser, tested) ----------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Opened {
    pub pid: u32,
    pub command: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct InUse {
    /// working folders and open files of running programs
    pub files: Vec<Opened>,
    /// listening ports by program
    pub ports: HashMap<u32, Vec<u16>>,
    /// running program names, lowercase
    pub names: Vec<String>,
}

/// Reads `lsof -n -P -F pcnT`: `p<pid>`, `c<command>`, then per file `n<name>` and for a socket
/// `TST=LISTEN`. Paths (names starting with `/`) are kept; a listening socket gives its port.
pub fn parse_lsof(text: &str) -> InUse {
    let mut seen = InUse::default();
    let (mut pid, mut command) = (0u32, String::new());
    let mut last_name = String::new();
    for line in text.lines() {
        let Some(tag) = line.chars().next() else { continue };
        let rest = &line[tag.len_utf8()..];
        match tag {
            'p' => {
                pid = rest.parse().unwrap_or(0);
                command.clear();
            }
            'c' => {
                command = rest.to_string();
                let lower = rest.to_lowercase();
                if !seen.names.contains(&lower) {
                    seen.names.push(lower);
                }
            }
            'n' => {
                last_name = rest.to_string();
                if rest.starts_with('/') {
                    // "/path (deleted)" and similar notes after the path are dropped
                    let path = rest.split(" (").next().unwrap_or(rest);
                    seen.files.push(Opened { pid, command: command.clone(), path: PathBuf::from(path) });
                }
            }
            'T' if rest == "ST=LISTEN" => {
                if let Some(port) = last_name.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) {
                    let ports = seen.ports.entry(pid).or_default();
                    if !ports.contains(&port) {
                        ports.push(port);
                    }
                }
            }
            _ => {}
        }
    }
    seen
}

impl InUse {
    /// "in use: node on :3000" when a running program works in or holds a file under `path`.
    pub fn user_of(&self, path: &Path) -> Option<String> {
        let hit = self.files.iter().find(|o| o.path.starts_with(path))?;
        let ports = self.ports.get(&hit.pid).map(|p| p.iter().map(|n| format!(":{n}")).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let who = if hit.command.is_empty() { "a running program".to_string() } else { hit.command.clone() };
        Some(if ports.is_empty() { format!("in use: {who}") } else { format!("in use: {who} on {ports}") })
    }

    pub fn running(&self, names: &[&str]) -> Option<String> {
        self.names.iter().find(|n| names.contains(&n.trim_end_matches(".exe"))).map(|n| n.trim_end_matches(".exe").to_string())
    }
}

/// One pass over this computer: lsof (macOS, Linux) and every program's working folder.
fn read_in_use() -> InUse {
    let mut seen = InUse::default();
    #[cfg(not(windows))]
    if let Some(r) = run(Path::new("lsof"), &["-n", "-P", "-w", "-F", "pcnT"], None, Duration::from_secs(60)) {
        // lsof exits 1 when some files could not be read; what it printed still counts
        seen = parse_lsof(&r.out);
    }
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    if let Ok(mut system) = System::new() {
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always));
        for (pid, process) in system.processes() {
            let name = process.name().to_string_lossy().to_string();
            if !seen.names.contains(&name.to_lowercase()) {
                seen.names.push(name.to_lowercase());
            }
            if let Some(cwd) = process.cwd().filter(|c| c.components().count() > 1) {
                seen.files.push(Opened { pid: pid.as_u32(), command: name, path: cwd.to_path_buf() });
            }
        }
    }
    seen
}

// ---------- finding build output ----------

#[derive(Debug, Clone, PartialEq)]
pub struct BuildDir {
    pub path: PathBuf,
    /// target | node_modules | .next | bin | obj
    pub kind: &'static str,
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).map(|m| sweep::is_link(&m)).unwrap_or(true)
}

fn has_csproj(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("csproj")))
}

/// The build folders inside a project: `target` beside a Cargo.toml (or marked by cargo's
/// CACHEDIR.TAG), node_modules and .next beside a package.json, bin and obj beside a .csproj. Never
/// enters a link, .git, another repo or worktree inside it, a hidden folder or a data folder.
pub fn find_build_dirs(root: &Path) -> Vec<BuildDir> {
    let mut out = Vec::new();
    if !is_link(root) && root.is_dir() {
        find_in(root, root, 0, &mut out);
    }
    out
}

fn find_in(root: &Path, dir: &Path, depth: usize, out: &mut Vec<BuildDir>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut subs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    subs.sort();
    let csproj = has_csproj(dir);
    for path in subs {
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if sweep::is_link(&meta) || !meta.is_dir() {
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let lower = name.to_lowercase();
        let kind: Option<&'static str> = match lower.as_str() {
            "target" if dir.join("Cargo.toml").is_file() || path.join("CACHEDIR.TAG").is_file() => Some("target"),
            "node_modules" if dir.join("package.json").is_file() => Some("node_modules"),
            ".next" if dir.join("package.json").is_file() => Some(".next"),
            "bin" if csproj => Some("bin"),
            "obj" if csproj => Some("obj"),
            _ => None,
        };
        if let Some(kind) = kind {
            out.push(BuildDir { path, kind });
            continue;
        }
        if lower == "node_modules" || lower.starts_with('.') || NEVER_DIRS.contains(&lower.as_str()) {
            continue;
        }
        // another repo or a worktree inside this one is scanned on its own, or not at all
        if path != root && path.join(".git").exists() {
            continue;
        }
        find_in(root, &path, depth + 1, out);
    }
}

// ---------- the walk that sizes and deletes ----------

fn never_file(path: &Path) -> bool {
    path.extension().map(|x| x.to_string_lossy().to_lowercase()).is_some_and(|x| NEVER_FILES.contains(&x.as_str()))
}

/// Sizes (dry) or deletes what is under `root`, but what `keep` says stays (and what is under it),
/// links (never followed, never deleted) and database files. The root itself stays.
pub fn wipe(root: &Path, keep: &dyn Fn(&Path) -> bool, dry: bool, progress: &mut dyn FnMut(u64)) -> Tally {
    let mut tally = Tally::default();
    if is_link(root) || !root.is_dir() {
        return tally;
    }
    wipe_in(root, root, keep, dry, &mut tally, progress);
    tally
}

fn wipe_in(root: &Path, dir: &Path, keep: &dyn Fn(&Path) -> bool, dry: bool, tally: &mut Tally, progress: &mut dyn FnMut(u64)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.starts_with(root) || keep(&path) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if sweep::is_link(&meta) {
            continue;
        }
        if meta.is_dir() {
            wipe_in(root, &path, keep, dry, tally, progress);
            if !dry {
                let _ = std::fs::remove_dir(&path);
            }
            continue;
        }
        if never_file(&path) {
            continue;
        }
        if dry {
            tally.bytes += meta.len();
            tally.files += 1;
        } else if std::fs::remove_file(&path).is_ok() {
            tally.bytes += meta.len();
            tally.files += 1;
            progress(tally.bytes);
        } else {
            tally.skipped += 1;
        }
    }
}

fn keep_nothing(_: &Path) -> bool {
    false
}

/// The size of everything under a folder (nothing deleted, links not followed).
fn size_of(path: &Path) -> u64 {
    wipe(path, &keep_nothing, true, &mut |_| {}).bytes
}

/// A build folder's keep rule: `target/release/bundle` (the installers) always stays.
fn keep_for(dir: &BuildDir) -> impl Fn(&Path) -> bool + '_ {
    let bundle = dir.path.join("release").join("bundle");
    let is_target = dir.kind == "target";
    move |p: &Path| is_target && p == bundle
}

// ---------- the scan ----------

/// The computer as the scan sees it: what is in use and where the package tools are. Tests pass
/// their own.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub in_use: InUse,
    pub npm: Option<PathBuf>,
    pub pnpm: Option<PathBuf>,
    pub cargo_cache: Option<PathBuf>,
    /// measure sizes (the re-check at Clean skips them: the walk counts what it frees)
    pub sizes: bool,
}

fn live_env(sizes: bool) -> Env {
    let cargo_home = std::env::var_os("CARGO_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| crate::env::home_dir().map(|h| h.join(".cargo")));
    Env {
        in_use: read_in_use(),
        npm: crate::env::find_on_path("npm"),
        pnpm: crate::env::find_on_path("pnpm"),
        cargo_cache: cargo_home.map(|c| c.join("registry").join("cache")).filter(|p| p.is_dir()),
        sizes,
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string())
}

fn rel_label(project: &Path, path: &Path) -> String {
    path.strip_prefix(project).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string())
}

fn item(group: &str, kind: &str, path: &Path) -> DevItem {
    DevItem { id: format!("{kind}:{}", path.display()), group: group.into(), kind: kind.into(), path: path.display().to_string(), bytes_known: true, ..Default::default() }
}

/// The repo a folder belongs to and its worktrees (the first is the main checkout); a folder
/// without git is its own single "worktree".
fn worktrees_of(folder: &Path) -> Vec<Worktree> {
    match git(folder, &["worktree", "list", "--porcelain"]) {
        Some(r) if r.ok => {
            let list = parse_worktrees(&r.out);
            if list.is_empty() {
                vec![Worktree { path: folder.to_path_buf(), ..Default::default() }]
            } else {
                list
            }
        }
        _ => vec![Worktree { path: folder.to_path_buf(), ..Default::default() }],
    }
}

fn same(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// The items of an active project (a DEV project or a repo's main checkout).
fn project_items(project: &Path, repo: &Path, env: &Env, out: &mut Vec<DevItem>) {
    let pname = name_of(project);
    for dir in find_build_dirs(project) {
        let tracked_here = tracked(repo, &dir.path);
        if dir.kind == "target" {
            for (sub, kind) in [("debug", "target-debug"), ("release", "target-release")] {
                let path = dir.path.join(sub);
                if is_link(&path) || !path.is_dir() {
                    continue;
                }
                let mut it = item(GROUP_DEFAULT, kind, &path);
                it.name = format!("{pname} {}", rel_label(project, &path));
                it.repo = repo.display().to_string();
                it.on = true;
                let bundle = path.join("bundle");
                if kind == "target-release" && bundle.is_dir() && !is_link(&bundle) {
                    let (bytes, files) = if env.sizes { let t = wipe(&bundle, &keep_nothing, true, &mut |_| {}); (t.bytes, t.files) } else { (0, 0) };
                    it.detail = format!("Rust build output · keeps release/bundle ({files} files, {})", human(bytes));
                } else {
                    it.detail = "Rust build output".into();
                }
                if env.sizes {
                    let keep = keep_for(&dir);
                    it.bytes = wipe(&path, &keep, true, &mut |_| {}).bytes;
                }
                it.blocked = if tracked_here { Some("git tracks files in it".into()) } else { env.in_use.user_of(&path) };
                out.push(it);
            }
        } else {
            let mut it = item(GROUP_PROJECTS, "build", &dir.path);
            it.name = format!("{pname} {}", rel_label(project, &dir.path));
            it.repo = repo.display().to_string();
            it.detail = match dir.kind {
                "node_modules" => "packages: npm install brings them back",
                ".next" => "Next.js build output",
                _ => ".NET build output",
            }
            .into();
            if env.sizes {
                it.bytes = size_of(&dir.path);
            }
            it.blocked = if tracked_here { Some("git tracks files in it".into()) } else { env.in_use.user_of(&dir.path) };
            out.push(it);
        }
    }
}

fn worktree_item(wt: &Worktree, repo: &Path, env: &Env) -> DevItem {
    let path = &wt.path;
    let mut it = item(GROUP_WORKTREES, "worktree", path);
    it.name = format!("{} · {}", name_of(repo), name_of(path));
    it.repo = repo.display().to_string();
    let facts = git_facts(path);
    it.branch = facts.branch.clone().or_else(|| wt.branch.clone());
    it.merged_into = facts.merged_into.clone();
    it.pushed = facts.pushed;
    it.last_commit = facts.last_commit;
    it.dirty = facts.dirty;
    let dirs: Vec<BuildDir> = find_build_dirs(path).into_iter().filter(|d| !tracked(path, &d.path)).collect();
    if env.sizes {
        it.bytes = dirs.iter().map(|d| wipe(&d.path, &keep_for(d), true, &mut |_| {}).bytes).sum();
        it.whole_bytes = size_of(path);
    }
    it.blocked = env.in_use.user_of(path);
    if it.blocked.is_none() && facts.merged_into.is_none() && !facts.pushed {
        it.blocked = Some("not merged and not pushed".into());
    }
    if it.blocked.is_none() {
        if !dirs.is_empty() {
            it.choices.push("build".into());
        }
        if facts.merged_into.is_some() && !facts.dirty && !wt.locked {
            it.choices.push("whole".into());
        }
    }
    it.detail = if facts.dirty {
        "uncommitted changes · build output only".into()
    } else if wt.locked {
        "locked · build output only".into()
    } else if facts.merged_into.is_none() && facts.pushed {
        "pushed, not merged · build output only".into()
    } else {
        String::new()
    };
    it
}

fn cache_items(env: &Env, out: &mut Vec<DevItem>) {
    let building = env.in_use.running(&["cargo", "rustc"]);
    if let Some(cache) = &env.cargo_cache {
        let mut it = item(GROUP_DEFAULT, "cargo", cache);
        it.name = "cargo registry cache".into();
        it.detail = "downloaded crates: cargo fetches them again".into();
        it.on = true;
        if env.sizes {
            it.bytes = size_of(cache);
        }
        it.blocked = building.map(|n| format!("in use: {n} is running"));
        out.push(it);
    }
    if let Some(npm) = &env.npm {
        if let Some(dir) = npm_cache_dir(npm) {
            let mut it = item(GROUP_DEFAULT, "npm", &dir);
            it.name = "npm cache".into();
            it.detail = "npm cache clean --force".into();
            it.on = true;
            if env.sizes {
                it.bytes = size_of(&dir.join("_cacache"));
            }
            it.blocked = env.in_use.running(&["npm"]).map(|n| format!("in use: {n} is running"));
            out.push(it);
        }
    }
    if let Some(pnpm) = &env.pnpm {
        if let Some(dir) = pnpm_store_dir(pnpm) {
            let mut it = item(GROUP_DEFAULT, "pnpm", &dir);
            it.name = "pnpm store (unused packages)".into();
            it.on = true;
            it.bytes_known = false;
            it.detail = if env.sizes { format!("pnpm store prune · the store holds {}", human(size_of(&dir))) } else { "pnpm store prune".into() };
            it.blocked = env.in_use.running(&["pnpm"]).map(|n| format!("in use: {n} is running"));
            out.push(it);
        }
    }
}

fn npm_cache_dir(npm: &Path) -> Option<PathBuf> {
    let r = run(npm, &["config", "get", "cache"], None, Duration::from_secs(30)).filter(|r| r.ok)?;
    Some(PathBuf::from(r.out.trim())).filter(|p| p.is_dir())
}

fn pnpm_store_dir(pnpm: &Path) -> Option<PathBuf> {
    let r = run(pnpm, &["store", "path"], None, Duration::from_secs(30)).filter(|r| r.ok)?;
    Some(PathBuf::from(r.out.trim())).filter(|p| p.is_dir())
}

/// "2.4 GB", for the details Rust writes (the sheet formats its own sizes).
fn human(bytes: u64) -> String {
    let gb = bytes as f64 / 1024f64.powi(3);
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", (bytes as f64 / 1024f64.powi(2)).round() as u64)
    }
}

/// Everything the Developer section offers for these DEV projects. Nothing is deleted.
pub fn scan_with(projects: &[PathBuf], env: &Env) -> DevScan {
    let mut items = Vec::new();
    let mut actives: Vec<PathBuf> = Vec::new();
    let mut linked: Vec<(Worktree, PathBuf)> = Vec::new();
    for folder in projects.iter().filter(|p| p.is_dir()) {
        let list = worktrees_of(folder);
        let main = list.first().map(|w| w.path.clone()).unwrap_or_else(|| folder.clone());
        for (i, wt) in list.into_iter().enumerate() {
            if wt.bare || wt.prunable || !wt.path.is_dir() {
                continue;
            }
            let active = i == 0 || projects.iter().any(|p| same(p, &wt.path));
            if active {
                if !actives.iter().any(|a| same(a, &wt.path)) {
                    actives.push(wt.path.clone());
                }
            } else if !linked.iter().any(|(w, _)| same(&w.path, &wt.path)) {
                linked.push((wt, main.clone()));
            }
        }
    }
    for project in &actives {
        let repo = worktrees_of(project).first().map(|w| w.path.clone()).unwrap_or_else(|| project.clone());
        project_items(project, &repo, env, &mut items);
    }
    cache_items(env, &mut items);
    for (wt, repo) in &linked {
        items.push(worktree_item(wt, repo, env));
    }
    DevScan { items, scanned: unix_now(), projects: actives.len(), worktrees: linked.len() }
}

// ---------- the clean ----------

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One line per item in Sweep's log.
fn log(folder: &Path, c: &DevCleaned) {
    use std::io::Write;
    let line = serde_json::json!({ "unix": unix_now(), "group": "developer", "path": c.path, "bytes": c.freed, "how": c.how, "ok": c.ok, "note": c.note });
    let _ = std::fs::create_dir_all(folder);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(folder.join(sweep::LOG_FILE)) {
        let _ = writeln!(f, "{line}");
    }
}

/// Cleans the picks after scanning again (nothing is trusted from the sheet but the ids): an item
/// that is gone, now in use, or no longer allows what was picked stays, with the reason.
/// `progress(item index, freed so far in all)`.
pub fn clean_with(projects: &[PathBuf], picks: &[Pick], env: &Env, log_folder: &Path, progress: &mut dyn FnMut(usize, u64)) -> Vec<DevCleaned> {
    let fresh = scan_with(projects, env);
    let mut done = Vec::new();
    let mut total = 0u64;
    for (index, pick) in picks.iter().enumerate() {
        let found = fresh.items.iter().find(|i| i.id == pick.id);
        let mut result = DevCleaned { id: pick.id.clone(), name: found.map(|i| i.name.clone()).unwrap_or_default(), path: found.map(|i| i.path.clone()).unwrap_or_default(), how: String::new(), freed: 0, ok: false, note: String::new() };
        match found {
            None => result.note = "no longer there".into(),
            Some(it) if it.blocked.is_some() => result.note = it.blocked.clone().unwrap_or_default(),
            Some(it) => {
                let allowed = if it.kind == "worktree" { it.choices.contains(&pick.how) } else { pick.how == "clean" };
                if !allowed {
                    result.note = "no longer offered".into();
                } else {
                    let base = total;
                    clean_one(it, &pick.how, &mut result, &mut |b| progress(index, base + b));
                }
            }
        }
        total += result.freed;
        progress(index, total);
        if !result.path.is_empty() {
            log(log_folder, &result);
        }
        done.push(result);
    }
    done
}

fn clean_one(it: &DevItem, how: &str, result: &mut DevCleaned, progress: &mut dyn FnMut(u64)) {
    let path = PathBuf::from(&it.path);
    match (it.kind.as_str(), how) {
        ("target-debug" | "build" | "cargo", _) => {
            let t = wipe(&path, &keep_nothing, false, progress);
            result.how = "walk".into();
            result.freed = t.bytes;
            result.ok = true;
            if t.skipped > 0 {
                result.note = format!("{} files skipped (in use)", t.skipped);
            }
        }
        ("target-release", _) => {
            let bundle = path.join("bundle");
            let t = wipe(&path, &|p: &Path| p == bundle, false, progress);
            result.how = "walk, kept bundle".into();
            result.freed = t.bytes;
            result.ok = true;
        }
        ("npm" | "pnpm", _) => {
            let (tool, args, how): (&str, &[&str], &str) = if it.kind == "npm" { ("npm", &["cache", "clean", "--force"], "npm cache clean --force") } else { ("pnpm", &["store", "prune"], "pnpm store prune") };
            let measured = if it.kind == "npm" { path.join("_cacache") } else { path.clone() };
            let before = size_of(&measured);
            result.how = how.into();
            match crate::env::find_on_path(tool).and_then(|p| run(&p, args, None, Duration::from_secs(600))) {
                Some(r) if r.ok => {
                    result.ok = true;
                    result.freed = before.saturating_sub(size_of(&measured));
                }
                Some(r) => result.note = r.err.trim().lines().last().unwrap_or("it did not finish").to_string(),
                None => result.note = format!("{tool} could not run"),
            }
            progress(result.freed);
        }
        ("worktree", "build") => {
            result.how = "walk, build output".into();
            for dir in find_build_dirs(&path).into_iter().filter(|d| !tracked(&path, &d.path)) {
                let base = result.freed;
                let t = wipe(&dir.path, &keep_for(&dir), false, &mut |b| progress(base + b));
                result.freed += t.bytes;
            }
            result.ok = true;
        }
        ("worktree", "whole") => {
            let before = size_of(&path);
            result.how = "git worktree remove".into();
            // never --force: git itself refuses a worktree with changes or a lock
            match git(Path::new(&it.repo), &["worktree", "remove", &it.path]) {
                Some(r) if r.ok => {
                    result.ok = true;
                    result.freed = before;
                }
                Some(r) => result.note = r.err.trim().lines().next().unwrap_or("git refused").to_string(),
                None => result.note = "git could not run".into(),
            }
            progress(result.freed);
        }
        _ => result.note = "no longer offered".into(),
    }
}

// ---------- the commands ----------

fn dev_projects(store: &crate::settings::SettingsStore) -> Vec<PathBuf> {
    store.get().dev_projects.iter().map(PathBuf::from).collect()
}

/// What the Developer section can free now (off the page's thread; nothing deleted).
#[tauri::command]
pub async fn sweep_dev_scan(store: tauri::State<'_, crate::settings::SettingsStore>) -> Result<DevScan, String> {
    let projects = dev_projects(&store);
    tauri::async_runtime::spawn_blocking(move || scan_with(&projects, &live_env(true))).await.map_err(|_| "Sweep could not scan.".to_string())
}

/// Cleans the picks; the page hears `castle://sweep-dev` { index, freed } as they go.
#[tauri::command]
pub async fn sweep_dev_clean(app: tauri::AppHandle, picks: Vec<Pick>) -> Result<Vec<DevCleaned>, String> {
    use tauri::{Emitter, Manager};
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(_one) = sweep::RUNNING.try_lock() else { return Err("A sweep is already running.".to_string()) };
        let store = app.state::<crate::settings::SettingsStore>();
        let (projects, folder) = (dev_projects(&store), store.folder());
        let mut last = Instant::now() - Duration::from_secs(1);
        Ok(clean_with(&projects, &picks, &live_env(false), &folder, &mut |index, freed| {
            if last.elapsed() > Duration::from_millis(150) {
                let _ = app.emit("castle://sweep-dev", serde_json::json!({ "index": index, "freed": freed }));
                last = Instant::now();
            }
        }))
    })
    .await
    .map_err(|_| "The sweep stopped.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("castle-sweepdev-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::canonicalize(&base).unwrap()
    }

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    fn sh(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(["-c", "user.name=Test", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn env() -> Env {
        Env { sizes: true, ..Default::default() }
    }

    #[test]
    fn reads_the_worktree_list() {
        let text = "worktree /code/app\nHEAD abc\nbranch refs/heads/main\n\nworktree /code/app/worktrees/x\nHEAD def\nbranch refs/heads/feat/x\nlocked\n\nworktree /code/app/worktrees/y\nHEAD 123\ndetached\nprunable gitdir file points to non-existent location\n\n";
        let list = parse_worktrees(text);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].branch.as_deref(), Some("feat/x"));
        assert!(list[1].locked && !list[1].prunable);
        assert_eq!(list[2].branch, None);
        assert!(list[2].prunable);
    }

    #[test]
    fn reads_lsof_and_names_who_uses_a_folder_and_its_ports() {
        let text = "p812\ncnode\nfcwd\nn/code/shop/worktrees/audit\nftxt\nn/usr/local/bin/node\nf22\nn*:3200\nTST=LISTEN\nf23\nn127.0.0.1:51000->127.0.0.1:5432\nTST=ESTABLISHED\np90\ncrustc\nfcwd\nn/code/castle/src-tauri\nf3\nn/code/castle/src-tauri/target/debug/deps/x.rlib (deleted)\n";
        let seen = parse_lsof(text);
        assert_eq!(seen.ports.get(&812), Some(&vec![3200]));
        assert!(seen.ports.get(&90).is_none());
        assert_eq!(seen.user_of(Path::new("/code/shop/worktrees/audit")).as_deref(), Some("in use: node on :3200"));
        assert_eq!(seen.user_of(Path::new("/code/castle/src-tauri/target/debug")).as_deref(), Some("in use: rustc"));
        assert_eq!(seen.user_of(Path::new("/code/shop/worktrees/aud")), None, "a prefix of a name is not the folder");
        assert_eq!(seen.user_of(Path::new("/code/other")), None);
        assert_eq!(seen.running(&["cargo", "rustc"]).as_deref(), Some("rustc"));
    }

    #[cfg(unix)]
    #[test]
    fn finds_build_folders_and_never_follows_a_link() {
        let base = tree("find");
        write(&base.join("Cargo.toml"), 1);
        write(&base.join("target/debug/app"), 10);
        write(&base.join("web/package.json"), 1);
        write(&base.join("web/node_modules/a/index.js"), 10);
        write(&base.join("web/.next/cache/x"), 10);
        write(&base.join("api/Api.csproj"), 1);
        write(&base.join("api/bin/Debug/api.dll"), 10);
        write(&base.join("api/obj/x.json"), 10);
        write(&base.join("scripts/bin/run.sh"), 1); // no .csproj beside it: a source folder
        write(&base.join("nested/.git"), 1); // another worktree inside: not this project's
        write(&base.join("nested/Cargo.toml"), 1);
        write(&base.join("nested/target/debug/x"), 1);
        write(&base.join("docker/pg/package.json"), 1);
        write(&base.join("docker/pg/node_modules/x"), 1);
        let outside = base.join("outside");
        write(&outside.join("Cargo.toml"), 1);
        write(&outside.join("target/debug/precious"), 99);
        write(&base.join("linked/Cargo.toml"), 1);
        std::os::unix::fs::symlink(outside.join("target"), base.join("linked/target")).unwrap();
        let found: Vec<String> = find_build_dirs(&base).iter().map(|d| rel_label(&base, &d.path)).collect();
        assert!(found.contains(&"target".to_string()));
        assert!(found.contains(&"web/node_modules".to_string()) && found.contains(&"web/.next".to_string()));
        assert!(found.contains(&"api/bin".to_string()) && found.contains(&"api/obj".to_string()));
        assert!(!found.iter().any(|f| f.starts_with("scripts") || f.starts_with("nested") || f.starts_with("docker")), "{found:?}");
        assert!(!found.contains(&"linked/target".to_string()), "a linked target is never followed");
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn the_walk_keeps_the_bundle_databases_and_whatever_a_link_points_at() {
        let base = tree("wipe");
        let target = base.join("target");
        write(&target.join("release/app"), 100);
        write(&target.join("release/deps/lib.rlib"), 50);
        write(&target.join("release/bundle/dmg/Castle_0.7.0.dmg"), 300);
        write(&target.join("release/test.sqlite"), 7);
        let outside = base.join("outside");
        write(&outside.join("precious.txt"), 99);
        std::os::unix::fs::symlink(&outside, target.join("release/link")).unwrap();
        let dir = BuildDir { path: target.clone(), kind: "target" };
        let keep = keep_for(&dir);
        assert_eq!(wipe(&target.join("release"), &keep, true, &mut |_| {}).bytes, 150, "the dry walk counts what would go");
        let done = wipe(&target, &keep, false, &mut |_| {});
        assert_eq!(done.bytes, 150);
        assert!(target.join("release/bundle/dmg/Castle_0.7.0.dmg").exists(), "every dmg stays");
        assert!(target.join("release/test.sqlite").exists(), "a database file stays");
        assert!(outside.join("precious.txt").exists() && target.join("release/link").exists(), "a link is never followed or removed");
        assert!(!target.join("release/deps").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    /// A repo on main with a remote, and four worktrees: merged and clean, pushed but not merged,
    /// neither merged nor pushed, and merged but dirty. Each has a target with 10 bytes.
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let base = tree(name);
        let remote = base.join("remote.git");
        let repo = base.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        sh(&base, &["init", "--bare", "-q", "-b", "main", remote.to_str().unwrap()]);
        sh(&repo, &["init", "-q", "-b", "main"]);
        write(&repo.join("Cargo.toml"), 1);
        write(&repo.join(".gitignore"), 0);
        std::fs::write(repo.join(".gitignore"), "target/\nworktrees/\n").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-q", "-m", "one"]);
        sh(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        sh(&repo, &["push", "-q", "-u", "origin", "main"]);
        write(&repo.join("target/debug/app"), 40);
        write(&repo.join("target/release/app"), 30);
        write(&repo.join("target/release/bundle/dmg/a.dmg"), 500);
        for wt in ["merged", "pushed", "local", "dirty"] {
            sh(&repo, &["worktree", "add", "-q", "-b", &format!("feat/{wt}"), &format!("worktrees/{wt}")]);
            let dir = repo.join("worktrees").join(wt);
            write(&dir.join("target/debug/x"), 10);
            if wt == "pushed" || wt == "local" {
                write(&dir.join(format!("{wt}.txt")), 1);
                sh(&dir, &["add", "."]);
                sh(&dir, &["commit", "-q", "-m", wt]);
            }
            if wt == "pushed" {
                sh(&dir, &["push", "-q", "-u", "origin", "feat/pushed"]);
            }
            if wt == "dirty" {
                write(&dir.join("notes.txt"), 1);
            }
        }
        (base, repo)
    }

    fn by_name<'a>(scan: &'a DevScan, suffix: &str) -> &'a DevItem {
        scan.items.iter().find(|i| i.name.ends_with(suffix)).unwrap_or_else(|| panic!("no {suffix} in {:?}", scan.items.iter().map(|i| &i.name).collect::<Vec<_>>()))
    }

    #[test]
    fn a_scan_sorts_worktrees_by_what_is_safe() {
        let (base, repo) = fixture("scan");
        let scan = scan_with(std::slice::from_ref(&repo), &env());
        assert_eq!((scan.projects, scan.worktrees), (1, 4));
        let debug = by_name(&scan, "app target/debug");
        assert!(debug.on && debug.group == GROUP_DEFAULT && debug.bytes == 40);
        let release = by_name(&scan, "app target/release");
        assert_eq!(release.bytes, 30, "the bundle is not counted");
        assert!(release.detail.contains("keeps release/bundle"));
        let merged = by_name(&scan, "· merged");
        assert_eq!(merged.choices, ["build", "whole"]);
        assert_eq!(merged.merged_into.as_deref(), Some("main"));
        assert!(!merged.on && merged.blocked.is_none() && merged.bytes == 10);
        let pushed = by_name(&scan, "· pushed");
        assert_eq!(pushed.choices, ["build"], "pushed but not merged: build output only");
        assert!(pushed.pushed && pushed.merged_into.is_none());
        let local = by_name(&scan, "· local");
        assert_eq!(local.blocked.as_deref(), Some("not merged and not pushed"));
        assert!(local.choices.is_empty());
        let dirty = by_name(&scan, "· dirty");
        assert!(dirty.dirty);
        assert_eq!(dirty.choices, ["build"], "a dirty worktree is never offered whole");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn something_in_use_is_shown_with_the_reason_and_never_cleaned() {
        let (base, repo) = fixture("inuse");
        let wt = repo.join("worktrees/merged");
        let mut e = env();
        e.in_use = parse_lsof(&format!("p7\ncdotnet\nfcwd\nn{}\nf9\nn*:8010\nTST=LISTEN\n", wt.display()));
        let scan = scan_with(std::slice::from_ref(&repo), &e);
        let item = by_name(&scan, "· merged");
        assert_eq!(item.blocked.as_deref(), Some("in use: dotnet on :8010"));
        assert!(item.choices.is_empty());
        let log = base.join("log");
        let done = clean_with(std::slice::from_ref(&repo), &[Pick { id: item.id.clone(), how: "whole".into() }], &e, &log, &mut |_, _| {});
        assert!(!done[0].ok && wt.is_dir());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_clean_checks_again_removes_only_what_is_safe_and_logs_each_item() {
        let (base, repo) = fixture("clean");
        let e = env();
        let scan = scan_with(std::slice::from_ref(&repo), &e);
        let id = |s: &str| by_name(&scan, s).id.clone();
        let log = base.join("log");
        let picks = vec![
            Pick { id: id("app target/debug"), how: "clean".into() },
            Pick { id: id("app target/release"), how: "clean".into() },
            Pick { id: id("· merged"), how: "whole".into() },
            Pick { id: id("· dirty"), how: "whole".into() }, // the sheet never offers it; a stale pick is refused
            Pick { id: id("· pushed"), how: "build".into() },
            Pick { id: "build:/nowhere".into(), how: "clean".into() },
        ];
        let done = clean_with(std::slice::from_ref(&repo), &picks, &e, &log, &mut |_, _| {});
        assert_eq!(done.iter().map(|d| d.ok).collect::<Vec<_>>(), [true, true, true, false, true, false]);
        assert_eq!((done[0].freed, done[1].freed, done[4].freed), (40, 30, 10));
        assert!(repo.join("target/release/bundle/dmg/a.dmg").exists(), "the bundle stays");
        assert!(!repo.join("worktrees/merged").exists(), "the merged worktree went through git");
        assert_eq!(done[2].how, "git worktree remove");
        let branches = Command::new("git").args(["branch", "--list", "feat/merged"]).current_dir(&repo).output().unwrap();
        assert!(String::from_utf8_lossy(&branches.stdout).contains("feat/merged"), "the branch is kept");
        assert!(repo.join("worktrees/dirty/notes.txt").exists(), "the dirty worktree stays");
        assert!(repo.join("worktrees/pushed/pushed.txt").exists() && !repo.join("worktrees/pushed/target/debug/x").exists());
        let text = std::fs::read_to_string(log.join(sweep::LOG_FILE)).unwrap();
        assert_eq!(text.lines().count(), 5, "one line per item found (the missing one has no path)");
        assert!(text.contains("\"how\":\"git worktree remove\"") && text.contains("\"group\":\"developer\""));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_cargo_cache_waits_while_cargo_runs() {
        let base = tree("cargo");
        write(&base.join("index.crates.io/serde-1.0.crate"), 20);
        let mut e = env();
        e.cargo_cache = Some(base.clone());
        let mut items = Vec::new();
        cache_items(&e, &mut items);
        assert_eq!((items[0].bytes, items[0].on, items[0].blocked.clone()), (20, true, None));
        e.in_use.names = vec!["cargo".into()];
        let mut items = Vec::new();
        cache_items(&e, &mut items);
        assert_eq!(items[0].blocked.as_deref(), Some("in use: cargo is running"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    #[ignore = "scans this computer's DEV projects given in CASTLE_DEV_PROJECTS (nothing is deleted)"]
    fn real_scan() {
        let projects: Vec<PathBuf> = std::env::var("CASTLE_DEV_PROJECTS").unwrap_or_default().split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
        for i in scan_with(&projects, &live_env(true)).items {
            println!("{:<10} {:<50} {:>14} {:?} {:?}", i.group, i.name, i.bytes, i.choices, i.blocked);
        }
    }
}
