//! A push wakes its DEV tab (his word Nov 25 2026: "if i push memorycore castle it will hear the
//! moment it landed right ?"). The push-heard hook (pushed.rs) tells the companion at the member's
//! next message; this watch makes that message come by itself. Once a minute, for each running
//! Claude Code DEV tab, the repository's upstream is read from git's own files. When it moved past
//! both the hook's marker (`.git/castle-pushed-<branch>.txt`, heard) and this watch's own
//! (`.git/castle-pushed-woke-<branch>.txt`, woken for), and HEAD holds it (the member's own push,
//! never a fetch of someone else's), the tab is knocked once while it is idle: "My push landed.",
//! and the hook's PUSHED line starts the after-push steps. A first sight only remembers; nothing
//! is ever written into the project itself. Nothing here ties a member's machine to anyone else's.

use crate::dev::DevHosts;
use crate::pushed;
use crate::settings::Cli;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// How often the tabs' repositories are read.
const EVERY: Duration = Duration::from_secs(60);
/// What the knock types into the tab (the member's own voice, as a message they might send).
pub const KNOCK: &str = "My push landed.";

/// What a look at one tab's upstream means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wake {
    Nothing,
    /// remember it, never wake (a first sight, or a move that is not the member's own push)
    Mark,
    /// the member's own push landed: knock the tab, then remember it
    Knock,
}

/// The decision, before any knock. `in_head`: HEAD contains the upstream (asked of git only when
/// the upstream is new). Pure.
pub fn should_wake(upstream: &str, heard: Option<&str>, woke: Option<&str>, in_head: bool) -> Wake {
    let clean = |m: Option<&str>| m.map(str::trim).filter(|m| !m.is_empty()).map(str::to_string);
    let (heard, woke) = (clean(heard), clean(woke));
    let Some(woke) = woke else { return Wake::Mark };
    if woke == upstream || heard.as_deref() == Some(upstream) {
        return Wake::Nothing;
    }
    if in_head { Wake::Knock } else { Wake::Mark }
}

/// The watch's own marker beside the hook's.
pub fn woke_marker(git: &Path, branch: &str) -> PathBuf {
    let heard = pushed::marker(git, branch);
    let name = heard.file_name().and_then(|n| n.to_str()).unwrap_or("castle-pushed-.txt").replacen("castle-pushed-", "castle-pushed-woke-", 1);
    git.join(name)
}

/// One tab's look: what to do and the markers to write. None when the folder has no upstream.
pub struct Look {
    pub wake: Wake,
    pub upstream: String,
    pub heard: PathBuf,
    pub woke: PathBuf,
}

pub fn look(folder: &Path) -> Option<Look> {
    let git = pushed::git_dir(folder)?;
    let branch = pushed::branch(&git)?;
    let common = pushed::common_dir(&git);
    let config = std::fs::read_to_string(common.join("config")).ok()?;
    let (remote, remote_branch) = pushed::upstream(&config, &branch)?;
    let upstream = pushed::remote_commit(&common, &remote, &remote_branch)?;
    let (heard, woke) = (pushed::marker(&git, &branch), woke_marker(&git, &branch));
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let (heard_text, woke_text) = (read(&heard), read(&woke));
    let new = woke_text.as_deref().map(str::trim).is_some_and(|w| !w.is_empty() && w != upstream) && heard_text.as_deref().map(str::trim) != Some(upstream.as_str());
    let in_head = new && pushed::git(folder, &["merge-base", "--is-ancestor", &upstream, "HEAD"]).is_some();
    let wake = should_wake(&upstream, heard_text.as_deref(), woke_text.as_deref(), in_head);
    Some(Look { wake, upstream, heard, woke })
}

/// Writes the watch's marker; on a first sight the hook's too when it has none, so the next push
/// is heard as a move rather than a first sight.
pub fn remember(look: &Look) {
    let _ = std::fs::write(&look.woke, format!("{}\n", look.upstream));
    if !look.heard.exists() {
        let _ = std::fs::write(&look.heard, format!("{}\n", look.upstream));
    }
}

/// Starts the watch over the running Claude Code DEV tabs (Codex sessions have no hook to hear it).
/// A busy tab is tried again the next minute; a knock is never repeated for one push.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(EVERY);
            let tabs = app.state::<crate::settings::SettingsStore>().get().dev_tabs.clone();
            let hosts = app.state::<DevHosts>();
            for (slot, host) in hosts.0.iter().enumerate() {
                let folder = tabs.get(slot).cloned().flatten().map(PathBuf::from);
                let (Some(folder), true) = (folder, host.is_running() && host.provider() == Some(Cli::Claude)) else {
                    continue;
                };
                let Some(look) = look(&folder) else { continue };
                match look.wake {
                    Wake::Nothing => {}
                    Wake::Mark => remember(&look),
                    Wake::Knock => {
                        if !crate::link::should_knock(true, &host.idle(), None) {
                            continue;
                        }
                        let _ = host.knock(KNOCK);
                        remember(&look);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn a_first_sight_only_remembers() {
        assert_eq!(should_wake(A, None, None, true), Wake::Mark);
        assert_eq!(should_wake(A, Some(A), Some("  "), true), Wake::Mark);
    }

    #[test]
    fn nothing_moved_or_already_heard_is_quiet() {
        assert_eq!(should_wake(A, Some(B), Some(A), true), Wake::Nothing);
        assert_eq!(should_wake(B, Some(B), Some(A), true), Wake::Nothing, "the member's message already heard it");
    }

    #[test]
    fn the_members_own_push_knocks_and_a_fetch_only_remembers() {
        assert_eq!(should_wake(B, Some(A), Some(A), true), Wake::Knock);
        assert_eq!(should_wake(B, Some(A), Some(A), false), Wake::Mark, "someone else's commits fetched");
        assert_eq!(should_wake(B, None, Some(A), true), Wake::Knock);
    }

    #[test]
    fn the_watch_marker_sits_beside_the_hooks() {
        let git = Path::new("C:/p/.git");
        assert_eq!(woke_marker(git, "main"), git.join("castle-pushed-woke-main.txt"));
        assert_eq!(woke_marker(git, "feat/x"), git.join("castle-pushed-woke-feat_x.txt"));
    }

    #[test]
    fn a_member_s_push_in_a_real_git_repository_knocks_once() {
        let dir = std::env::temp_dir().join(format!("castle-push-knock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| pushed::git(&dir, args);
        run(&["init", "-q", "-b", "main"]).unwrap();
        run(&["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "one"]).unwrap();
        let one = run(&["rev-parse", "HEAD"]).unwrap();
        let git = dir.join(".git");
        std::fs::write(git.join("config"), format!("{}\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n", std::fs::read_to_string(git.join("config")).unwrap())).unwrap();
        std::fs::create_dir_all(git.join("refs/remotes/origin")).unwrap();
        std::fs::write(git.join("refs/remotes/origin/main"), format!("{one}\n")).unwrap();
        remember(&look(&dir).unwrap());

        // a new commit pushed: the upstream moves to HEAD
        run(&["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "two"]).unwrap();
        let two = run(&["rev-parse", "HEAD"]).unwrap();
        std::fs::write(git.join("refs/remotes/origin/main"), format!("{two}\n")).unwrap();
        let pushed_now = look(&dir).unwrap();
        assert_eq!(pushed_now.wake, Wake::Knock);
        remember(&pushed_now);
        assert_eq!(look(&dir).unwrap().wake, Wake::Nothing, "one knock for one push");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_move_git_cannot_vouch_for_is_only_remembered() {
        let dir = std::env::temp_dir().join(format!("castle-push-wake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/remotes/origin")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(git.join("config"), "[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n").unwrap();
        std::fs::write(git.join("refs/remotes/origin/main"), format!("{A}\n")).unwrap();

        let first = look(&dir).unwrap();
        assert_eq!(first.wake, Wake::Mark);
        remember(&first);
        assert_eq!(std::fs::read_to_string(git.join("castle-pushed-woke-main.txt")).unwrap().trim(), A);
        assert_eq!(std::fs::read_to_string(git.join("castle-pushed-main.txt")).unwrap().trim(), A, "the hook's marker is seeded so the next push is a move");
        assert_eq!(look(&dir).unwrap().wake, Wake::Nothing);

        // the upstream moves; this folder is no real repository, so git cannot vouch for it: remembered, never knocked
        std::fs::write(git.join("refs/remotes/origin/main"), format!("{B}\n")).unwrap();
        let moved = look(&dir).unwrap();
        assert_eq!(moved.wake, Wake::Mark);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
