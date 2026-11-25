//! The push is heard: a member's own push, said once to their companion at their next message.
//! Claude Code runs `memorycore-castle --pushed` as a hook from Castle's own session settings, in
//! the session's folder. It reads git's own files - nothing is started on a quiet message - and
//! only when the branch's upstream moved does it ask git whether the move is our own commits going
//! forward. Then, once: `PUSHED: N commits to <remote>/<branch> (<old7>..<new7>)`.
//!
//! The last upstream commit seen is kept in `.git/castle-pushed-<branch>.txt`, inside `.git`, so
//! the project never gets a new tracked file. A first sight is silent; a fetch of someone else's
//! commits is silent; a push from another computer is heard once a fetch brings it here. Any
//! trouble says nothing: the hook is never in the member's way.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The command-line word: `memorycore-castle --pushed`.
pub const FLAG: &str = "--pushed";
const GIT_LIMIT: Duration = Duration::from_secs(5);

/// The repository's git folder: `.git` itself, or the folder a `.git` file points to (a worktree).
pub fn git_dir(folder: &Path) -> Option<PathBuf> {
    let dot = folder.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let path = PathBuf::from(target);
    let path = if path.is_absolute() { path } else { folder.join(path) };
    path.is_dir().then_some(path)
}

/// The branch HEAD names; None for a detached HEAD.
pub fn branch(git: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    head.trim().strip_prefix("ref: refs/heads/").map(str::to_string)
}

/// The common git folder (a worktree keeps its config and refs there).
pub(crate) fn common_dir(git: &Path) -> PathBuf {
    std::fs::read_to_string(git.join("commondir"))
        .ok()
        .map(|c| {
            let p = PathBuf::from(c.trim());
            if p.is_absolute() { p } else { git.join(p) }
        })
        .unwrap_or_else(|| git.to_path_buf())
}

/// The branch's upstream from `config`: (remote, the remote's branch).
pub fn upstream(config: &str, branch: &str) -> Option<(String, String)> {
    let header = format!("[branch \"{branch}\"]");
    let mut inside = false;
    let (mut remote, mut merge) = (None, None);
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "remote" => remote = Some(value.trim().to_string()),
                "merge" => merge = value.trim().strip_prefix("refs/heads/").map(str::to_string),
                _ => {}
            }
        }
    }
    let remote = remote.filter(|r| r != ".")?;
    Some((remote, merge?))
}

/// The commit a remote-tracking branch points at: its ref file, else its line in `packed-refs`.
pub fn remote_commit(common: &Path, remote: &str, branch: &str) -> Option<String> {
    let name = format!("refs/remotes/{remote}/{branch}");
    if let Ok(text) = std::fs::read_to_string(common.join(&name)) {
        let id = text.trim().to_string();
        if is_commit_id(&id) {
            return Some(id);
        }
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| {
        let (id, r) = l.split_once(' ')?;
        (r.trim() == name && is_commit_id(id)).then(|| id.to_string())
    })
}

fn is_commit_id(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// The marker's file for a branch (slashes and odd characters made plain).
pub fn marker(git: &Path, branch: &str) -> PathBuf {
    let safe: String = branch.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    git.join(format!("castle-pushed-{safe}.txt"))
}

/// What a look at the upstream means, before git is asked anything.
#[derive(Debug, PartialEq)]
pub enum Seen {
    /// first sight: remember it, say nothing
    First,
    /// nothing moved
    Same,
    /// it moved from this commit
    Moved(String),
}

pub fn compare(kept: Option<&str>, now: &str) -> Seen {
    match kept.map(str::trim).filter(|k| is_commit_id(k)) {
        None => Seen::First,
        Some(k) if k == now => Seen::Same,
        Some(k) => Seen::Moved(k.to_string()),
    }
}

/// Runs git in the folder with no window and a time limit; its trimmed output when it succeeded.
pub(crate) fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command.current_dir(folder).args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let until = Instant::now() + GIT_LIMIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                use std::io::Read;
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                return status.success().then(|| out.trim().to_string());
            }
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// The `PUSHED:` line for the session's folder, or nothing.
pub fn run(folder: &Path) -> Option<String> {
    let git_folder = git_dir(folder)?;
    let branch = branch(&git_folder)?;
    let common = common_dir(&git_folder);
    let config = std::fs::read_to_string(common.join("config")).ok()?;
    let (remote, remote_branch) = upstream(&config, &branch)?;
    let now = remote_commit(&common, &remote, &remote_branch)?;
    let mark = marker(&git_folder, &branch);
    let seen = compare(std::fs::read_to_string(&mark).ok().as_deref(), &now);
    if seen == Seen::Same {
        return None;
    }
    // the marker always moves to what is there now, whatever the move was
    let _ = std::fs::write(&mark, format!("{now}\n"));
    let Seen::Moved(old) = seen else { return None };
    let ours = git(folder, &["merge-base", "--is-ancestor", &now, "HEAD"]).is_some();
    let forward = git(folder, &["merge-base", "--is-ancestor", &old, &now]).is_some();
    if !(ours && forward) {
        return None;
    }
    let count: u64 = git(folder, &["rev-list", "--count", &format!("{old}..{now}")])?.parse().ok()?;
    if count == 0 {
        return None;
    }
    Some(format!("PUSHED: {count} commit{} to {remote}/{remote_branch} ({}..{})", if count == 1 { "" } else { "s" }, &old[..7], &now[..7]))
}

/// `memorycore-castle --pushed`: prints the line for the current folder when there is one. Always
/// ends well, so the hook never stands in the member's way.
pub fn main() -> i32 {
    if let Some(line) = std::env::current_dir().ok().and_then(|d| run(&d)) {
        println!("{line}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    fn repo(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("castle-pushed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git/refs/remotes/origin")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = x\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n[branch \"other\"]\n\tremote = .\n\tmerge = refs/heads/main\n").unwrap();
        root
    }

    #[test]
    fn git_files_are_read_without_git() {
        let root = repo("read");
        let git = git_dir(&root).unwrap();
        assert_eq!(branch(&git).as_deref(), Some("main"));
        let config = std::fs::read_to_string(git.join("config")).unwrap();
        assert_eq!(upstream(&config, "main"), Some(("origin".into(), "main".into())));
        assert_eq!(upstream(&config, "other"), None, "a branch tracking a local one has no remote");
        assert_eq!(upstream(&config, "missing"), None);
        assert_eq!(remote_commit(&git, "origin", "main"), None);
        std::fs::write(git.join("packed-refs"), format!("# pack-refs\n{A} refs/remotes/origin/main\n")).unwrap();
        assert_eq!(remote_commit(&git, "origin", "main").as_deref(), Some(A), "packed-refs");
        std::fs::write(git.join("refs/remotes/origin/main"), format!("{B}\n")).unwrap();
        assert_eq!(remote_commit(&git, "origin", "main").as_deref(), Some(B), "the loose ref wins");
        std::fs::write(git.join("HEAD"), format!("{A}\n")).unwrap();
        assert_eq!(branch(&git), None, "a detached HEAD hears nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_worktree_points_to_its_git_folder() {
        let root = repo("tree");
        let tree = root.join("tree");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join(".git"), format!("gitdir: {}\n", root.join(".git").display())).unwrap();
        assert_eq!(git_dir(&tree), Some(root.join(".git")));
        assert_eq!(git_dir(&std::env::temp_dir().join("castle-no-repo-here")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn first_sight_is_silent_and_the_same_commit_starts_no_git() {
        assert_eq!(compare(None, A), Seen::First);
        assert_eq!(compare(Some("not an id"), A), Seen::First);
        assert_eq!(compare(Some(&format!("{A}\n")), A), Seen::Same);
        assert_eq!(compare(Some(A), B), Seen::Moved(A.into()));
        let root = repo("first");
        std::fs::write(root.join(".git/refs/remotes/origin/main"), format!("{A}\n")).unwrap();
        assert_eq!(run(&root), None, "first sight");
        assert_eq!(std::fs::read_to_string(marker(&root.join(".git"), "main")).unwrap().trim(), A);
        assert_eq!(run(&root), None, "nothing moved");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_branch_with_slashes_has_a_plain_marker() {
        assert!(marker(Path::new("C:/r/.git"), "feature/new thing").ends_with("castle-pushed-feature_new_thing.txt"));
    }

    #[test]
    #[ignore]
    fn live_git_a_push_is_heard_once_and_a_fetch_or_a_rewrite_is_not() {
        let root = std::env::temp_dir().join(format!("castle-pushed-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (bare, mine, theirs) = (root.join("remote.git"), root.join("mine"), root.join("theirs"));
        std::fs::create_dir_all(&root).unwrap();
        let g = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git").current_dir(dir).args(args).output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        g(&root, &["init", "--bare", "-b", "main", "remote.git"]);
        g(&root, &["clone", bare.to_str().unwrap(), "mine"]);
        for dir in [&mine] {
            g(dir, &["config", "user.email", "t@t"]);
            g(dir, &["config", "user.name", "t"]);
        }
        let commit = |dir: &Path, n: &str| {
            std::fs::write(dir.join(format!("{n}.txt")), n).unwrap();
            g(dir, &["add", "."]);
            g(dir, &["commit", "-m", n]);
        };
        commit(&mine, "one");
        g(&mine, &["push", "-u", "origin", "main"]);
        assert_eq!(run(&mine), None, "first sight");
        commit(&mine, "two");
        commit(&mine, "three");
        g(&mine, &["push"]);
        let heard = run(&mine).unwrap();
        assert!(heard.starts_with("PUSHED: 2 commits to origin/main ("), "{heard}");
        assert_eq!(run(&mine), None, "said once");
        assert!(!std::process::Command::new("git").current_dir(&mine).args(["status", "--porcelain"]).output().unwrap().stdout.iter().any(|_| true), "no new tracked file");
        // someone else pushes; our fetch brings it: silent
        g(&root, &["clone", bare.to_str().unwrap(), "theirs"]);
        g(&theirs, &["config", "user.email", "o@o"]);
        g(&theirs, &["config", "user.name", "o"]);
        commit(&theirs, "theirs");
        g(&theirs, &["push"]);
        g(&mine, &["fetch"]);
        assert_eq!(run(&mine), None, "someone else's commits");
        // a rewrite that is not forward: silent
        g(&mine, &["reset", "--hard", "origin/main~2"]);
        commit(&mine, "rewritten");
        g(&mine, &["push", "--force"]);
        assert_eq!(run(&mine), None, "not forward");
        let _ = std::fs::remove_dir_all(&root);
    }
}
