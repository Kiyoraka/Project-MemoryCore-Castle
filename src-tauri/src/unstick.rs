//! A stuck session carries on. Claude Code can stop on "Prompt is too long" or "Context limit reached ·
//! /compact or /clear to continue" and then answers nothing. Every 15 seconds Castle reads the end of
//! each running Claude Code session's screen:
//!
//! - A DEV tab is compacted (`/compact`, then `continue` once the screen says it was compacted, and
//!   compacted again when it is still stuck): a summary of coding work loses nothing that matters.
//! - The companion's home chat is never compacted: there the talk is the memory, so a stuck home chat
//!   starts fresh instead (fresh_start.rs), the talk kept in Chat and carried word for word.
//!
//! At most three compacts in thirty minutes for one tab; past that one line in the tab says so and
//! nothing more is typed until it is unstuck. The screen is only read, never stored. The reading and
//! the steps are the Alice Keep's (its unstick.rs, Phase 180), with the member told when a tab waits.

use crate::dev::DevHosts;
use crate::pty::PtyHost;
use crate::settings::Cli;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

/// How often every session's screen is read.
const EVERY: Duration = Duration::from_secs(15);
/// How much of the end of a screen is read, so the watch stays cheap.
const TAIL_BYTES: usize = 16 * 1024;
/// How many of the last non-empty lines are judged.
const TAIL_LINES: usize = 12;
/// At most this many compacts...
const CAP: usize = 3;
/// ...within this many seconds.
const WINDOW: u64 = 30 * 60;
/// A compact that still shows the stuck line this long after it was typed has finished without help.
const SETTLE: u64 = 20;
/// A compact that shows nothing for this long is given up on and the tab is watched again.
const PATIENCE: u64 = 10 * 60;
/// The event a panel listens to for its one line: `{ slot, line }`, slot null for the home chat, an
/// empty line clearing it.
pub const EVENT: &str = "session-full";
/// The DEV tab's line once three compacts in thirty minutes did not free it.
pub const GAVE_UP_LINE: &str = "This session keeps filling up, so I stopped compacting it. Start a new session in this tab when you are ready.";

/// The screen without its escape sequences and control characters; `\r\n` and `\r` become `\n`. Pure.
pub fn plain(screen: &str) -> String {
    let mut out = String::with_capacity(screen.len());
    let mut chars = screen.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // CSI: parameters and intermediates, then one final byte in @..~.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC (and the other string sequences): up to BEL or ESC \.
                Some(']') | Some('P') | Some('X') | Some('^') | Some('_') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                // A character-set choice takes one more character.
                Some('(') | Some(')') | Some('*') | Some('+') => {
                    chars.next();
                }
                // Any other ESC x is two characters long.
                _ => {}
            },
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' => out.push('\n'),
            // The 8-bit CSI, as some terminals send it.
            '\u{9b}' => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The last `n` non-empty lines, oldest first. Pure.
pub fn tail_lines(text: &str, n: usize) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().rev().filter(|line| !line.trim().is_empty()).take(n).collect();
    lines.reverse();
    lines
}

/// A line trimmed of the glyphs, boxes and quotes around its words, lowercased.
fn words(line: &str) -> String {
    line.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// The line says the session is stuck. "Context limit reached" is known by the `/compact` advice beside
/// it; "Prompt is too long" must begin the line or come in an API error, so a line of code or a diff
/// that merely quotes it is not taken for it.
fn is_stuck_line(line: &str) -> bool {
    let words = words(line);
    (words.contains("context limit reached") && words.contains("/compact"))
        || words.starts_with("prompt is too long")
        || (words.contains("api error") && words.contains("prompt is too long"))
}

fn is_compacting_line(line: &str) -> bool {
    words(line).contains("compacting conversation")
}

fn is_compacted_line(line: &str) -> bool {
    let words = words(line);
    words.contains("conversation compacted") || words.starts_with("compacted")
}

/// Where among the last lines each kind of line was last seen: (stuck, compacting, compacted).
fn marks(screen_plain: &str) -> (Option<usize>, Option<usize>, Option<usize>) {
    let lines = tail_lines(screen_plain, TAIL_LINES);
    let last = |is: fn(&str) -> bool| lines.iter().rposition(|line| is(line));
    (last(is_stuck_line), last(is_compacting_line), last(is_compacted_line))
}

/// True when a stuck line is among the last lines and no compact, running or done, came after it. Pure.
pub fn stuck(screen_plain: &str) -> bool {
    let (stuck, compacting, compacted) = marks(screen_plain);
    stuck.is_some_and(|s| compacting.is_none_or(|c| c < s) && compacted.is_none_or(|d| d < s))
}

/// True when the last lines show a compact running and nothing after it. Pure.
pub fn compacting(screen_plain: &str) -> bool {
    let (stuck, compacting, compacted) = marks(screen_plain);
    compacting.is_some_and(|c| stuck.is_none_or(|s| s < c) && compacted.is_none_or(|d| d < c))
}

/// True when the last lines show a compact done and no stuck line after it. Pure.
pub fn compacted(screen_plain: &str) -> bool {
    let (stuck, _, compacted) = marks(screen_plain);
    compacted.is_some_and(|d| stuck.is_none_or(|s| s < d))
}

/// Where one tab stands.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum State {
    #[default]
    Watching,
    /// `/compact` was typed at `since` (unix seconds).
    Compacting { since: u64 },
    /// The compact finished and `continue` was typed; read like Watching until the compact's line is gone.
    Resumed,
}

/// What Castle does for a tab after reading its screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Nothing,
    /// Type `/compact`.
    Compact,
    /// Type `continue`.
    Continue,
    /// Too many compacts lately: nothing is typed and the tab says so once.
    GiveUp,
    /// The tab that gave up is unstuck: its line is cleared.
    Clear,
}

/// One tab's watch.
#[derive(Debug, Clone, Default)]
pub struct Tab {
    pub state: State,
    /// When each compact was typed (unix seconds); only the last thirty minutes count.
    pub compacts: Vec<u64>,
    /// The member was already told for this stuck stretch.
    pub gave_up: bool,
}

impl Tab {
    /// The tab is stuck: a compact if the cap allows one, else the member is told once.
    fn stuck(&mut self, now: u64) -> Action {
        self.compacts.retain(|&at| now.saturating_sub(at) < WINDOW);
        if self.compacts.len() < CAP {
            self.compacts.push(now);
            self.state = State::Compacting { since: now };
            self.gave_up = false;
            Action::Compact
        } else {
            self.state = State::Watching;
            if self.gave_up {
                Action::Nothing
            } else {
                self.gave_up = true;
                Action::GiveUp
            }
        }
    }
}

/// One reading of a tab's screen at `now` (unix seconds): moves its watch on and says what to do. Pure.
pub fn step(tab: &mut Tab, screen_plain: &str, now: u64) -> Action {
    match tab.state {
        State::Watching | State::Resumed => {
            if stuck(screen_plain) {
                return tab.stuck(now);
            }
            let cleared = std::mem::take(&mut tab.gave_up);
            if tab.state == State::Resumed && !compacted(screen_plain) {
                tab.state = State::Watching;
            }
            if cleared { Action::Clear } else { Action::Nothing }
        }
        State::Compacting { since } => {
            if compacted(screen_plain) && !stuck(screen_plain) {
                tab.state = State::Resumed;
                return Action::Continue;
            }
            let waited = now.saturating_sub(since);
            if stuck(screen_plain) && !compacting(screen_plain) && waited >= SETTLE {
                return tab.stuck(now);
            }
            if waited > PATIENCE {
                tab.state = State::Watching;
            }
            Action::Nothing
        }
    }
}

/// The end of a screen, cut on a character boundary.
fn screen_tail(screen: &str) -> &str {
    let mut start = screen.len().saturating_sub(TAIL_BYTES);
    while !screen.is_char_boundary(start) {
        start += 1;
    }
    &screen[start..]
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The plain end of a running Claude Code session's screen; None for any other session.
pub fn claude_screen(host: &PtyHost) -> Option<String> {
    (host.is_running() && host.provider() == Some(Cli::Claude)).then(|| plain(screen_tail(&host.snapshot())))
}

/// Tells a panel its line (`slot` None for the home chat; an empty line clears it).
pub fn tell(app: &AppHandle, slot: Option<usize>, line: &str) {
    let _ = app.emit(EVENT, serde_json::json!({ "slot": slot, "line": line }));
}

/// The home chat is stuck and this session was not tried yet: start it fresh (once per session, so a
/// fresh start that fails is never repeated in a loop). Pure.
pub fn home_starts_fresh(screen_plain: &str, session: u64, tried: Option<u64>) -> bool {
    stuck(screen_plain) && tried != Some(session)
}

/// Starts the watch: one thread reading every running Claude Code DEV tab's screen, and the home
/// chat's, every 15 seconds. A tab's watch starts over when a new session takes it; a stuck home chat
/// is never compacted, it starts fresh.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut tabs: HashMap<usize, (u64, Tab)> = HashMap::new();
        let mut home_tried: Option<u64> = None;
        loop {
            std::thread::sleep(EVERY);
            let main = &app.state::<crate::session::MainHost>().0;
            if let (Some((id, _)), Some(screen)) = (main.started(), claude_screen(main)) {
                if home_starts_fresh(&screen, id, home_tried) {
                    home_tried = Some(id);
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let _ = crate::fresh_start::start_fresh(&app, true);
                    });
                }
            }
            let hosts = app.state::<DevHosts>();
            for (slot, host) in hosts.0.iter().enumerate() {
                let (Some((id, _)), Some(screen)) = (host.started(), claude_screen(host)) else {
                    tabs.remove(&slot);
                    continue;
                };
                let entry = tabs.entry(slot).or_insert_with(|| (id, Tab::default()));
                if entry.0 != id {
                    *entry = (id, Tab::default());
                    tell(&app, Some(slot), "");
                }
                match step(&mut entry.1, &screen, now_secs()) {
                    Action::Compact => {
                        let _ = host.knock("/compact");
                    }
                    Action::Continue => {
                        let _ = host.knock("continue");
                    }
                    Action::GiveUp => tell(&app, Some(slot), GAVE_UP_LINE),
                    Action::Clear => tell(&app, Some(slot), ""),
                    Action::Nothing => {}
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOO_LONG: &str = "\x1b[2K\x1b[1G  \x1b[38;5;246m⎿\x1b[39m  \x1b[31mPrompt is too long\x1b[0m\r\n";
    const LIMIT: &str = "\x1b[2K\x1b[31mContext limit reached · /compact or /clear to continue · auto-compact is off\x1b[0m\r\n";
    const BOX: &str = "\x1b[2m╭──────────────────────────╮\x1b[0m\r\n\x1b[2m│\x1b[0m > \x1b[7m \x1b[0m                      \x1b[2m│\x1b[0m\r\n\x1b[2m╰──────────────────────────╯\x1b[0m\r\n";
    const RUNNING: &str = "\x1b[38;5;174m✻\x1b[39m Compacting conversation… \x1b[2m(esc to interrupt)\x1b[0m\r\n";
    const DONE: &str = "\x1b[2m✻ Conversation compacted · ctrl+o for history\x1b[0m\r\n";

    fn screen(parts: &[&str]) -> String {
        plain(&format!("\x1b]0;claude\x07● Working on the plan.\r\n{}", parts.concat()))
    }

    #[test]
    fn plain_strips_escapes_and_controls() {
        assert_eq!(plain("\x1b[31mred\x1b[0m\r\nnext\rline\x07"), "red\nnext\nline");
        assert_eq!(plain("\x1b]0;title\x1b\\a\x1b(Bb\x1b=c\x08"), "abc");
        assert_eq!(plain("\x1b[38;2;10;20;30mtrue color\x1b[?25l"), "true color");
    }

    #[test]
    fn tail_lines_keeps_the_last_non_empty_lines() {
        assert_eq!(tail_lines("a\n\nb\n  \nc\n", 2), vec!["b", "c"]);
        assert_eq!(tail_lines("a\n", 5), vec!["a"]);
    }

    #[test]
    fn both_stuck_lines_are_stuck() {
        assert!(stuck(&screen(&[TOO_LONG, BOX])));
        assert!(stuck(&screen(&[LIMIT])));
        assert!(stuck(&screen(&[TOO_LONG, BOX, LIMIT])));
        assert!(!stuck(&screen(&[BOX])));
    }

    #[test]
    fn a_quoted_stuck_line_is_not_stuck() {
        assert!(!stuck("  42 + if line.contains(\"Prompt is too long\") {\n"));
        assert!(!stuck("He said the Context limit reached line is annoying\n"));
    }

    #[test]
    fn a_stuck_line_far_up_the_screen_is_old() {
        let mut parts = vec![TOO_LONG];
        parts.extend(std::iter::repeat_n("● more work\r\n", TAIL_LINES));
        assert!(!stuck(&screen(&parts)));
    }

    #[test]
    fn a_running_compact_is_not_stuck() {
        let s = screen(&[LIMIT, "> /compact\r\n", RUNNING]);
        assert!(!stuck(&s));
        assert!(compacting(&s));
        assert!(!compacted(&s));
    }

    #[test]
    fn a_done_compact_is_compacted() {
        let s = screen(&[LIMIT, RUNNING, DONE, BOX]);
        assert!(!stuck(&s));
        assert!(!compacting(&s));
        assert!(compacted(&s));
        assert!(compacted(&screen(&["  ⎿  Compacted (ctrl+o to see full summary)\r\n"])));
    }

    #[test]
    fn stuck_after_a_compact_is_stuck_again() {
        let s = screen(&[DONE, "> continue\r\n", TOO_LONG, BOX]);
        assert!(stuck(&s));
        assert!(!compacted(&s));
    }

    #[test]
    fn stuck_compact_done_continue() {
        let mut tab = Tab::default();
        assert_eq!(step(&mut tab, &screen(&[BOX]), 0), Action::Nothing);
        assert_eq!(step(&mut tab, &screen(&[LIMIT]), 15), Action::Compact);
        assert_eq!(tab.state, State::Compacting { since: 15 });
        // typed but not shown yet: the stuck line is still last, too soon to call it finished
        assert_eq!(step(&mut tab, &screen(&[LIMIT]), 30), Action::Nothing);
        assert_eq!(step(&mut tab, &screen(&[LIMIT, RUNNING]), 45), Action::Nothing);
        assert_eq!(step(&mut tab, &screen(&[LIMIT, RUNNING, DONE, BOX]), 60), Action::Continue);
        assert_eq!(tab.state, State::Resumed);
        assert_eq!(step(&mut tab, &screen(&[DONE, "> continue\r\n", BOX]), 75), Action::Nothing);
        assert_eq!(step(&mut tab, &screen(&["● Done.\r\n", BOX]), 90), Action::Nothing);
        assert_eq!(tab.state, State::Watching);
    }

    #[test]
    fn still_stuck_after_the_compact_compacts_again() {
        let mut tab = Tab::default();
        assert_eq!(step(&mut tab, &screen(&[TOO_LONG]), 0), Action::Compact);
        let failed = screen(&[TOO_LONG, RUNNING, "  ⎿  Error during compaction\r\n", TOO_LONG, BOX]);
        assert_eq!(step(&mut tab, &failed, 15), Action::Nothing);
        assert_eq!(step(&mut tab, &failed, 30), Action::Compact);
        assert_eq!(tab.compacts, vec![0, 30]);
    }

    #[test]
    fn a_silent_compact_is_given_up_on() {
        let mut tab = Tab::default();
        assert_eq!(step(&mut tab, &screen(&[LIMIT]), 0), Action::Compact);
        assert_eq!(step(&mut tab, &screen(&[LIMIT, RUNNING]), 300), Action::Nothing);
        assert_eq!(step(&mut tab, &screen(&[LIMIT, RUNNING]), 601), Action::Nothing);
        assert_eq!(tab.state, State::Watching);
    }

    #[test]
    fn the_fourth_stuck_in_thirty_minutes_waits_and_says_so_once() {
        let mut tab = Tab::default();
        let stuck_screen = screen(&[DONE, TOO_LONG, BOX]);
        let done_screen = screen(&[TOO_LONG, RUNNING, DONE, BOX]);
        let mut now = 0;
        for _ in 0..CAP {
            assert_eq!(step(&mut tab, &stuck_screen, now), Action::Compact);
            assert_eq!(step(&mut tab, &done_screen, now + 60), Action::Continue);
            now += 120;
        }
        assert_eq!(step(&mut tab, &stuck_screen, now), Action::GiveUp);
        assert_eq!(step(&mut tab, &stuck_screen, now + 15), Action::Nothing);
        assert_eq!(step(&mut tab, &stuck_screen, now + 30), Action::Nothing);
        // thirty minutes after the first compact the cap frees one again
        assert_eq!(step(&mut tab, &stuck_screen, WINDOW + 1), Action::Compact);
    }

    #[test]
    fn a_tab_that_gave_up_clears_its_line_once_unstuck_and_is_told_again_next_time() {
        let mut tab = Tab { compacts: vec![0, 1, 2], ..Tab::default() };
        assert_eq!(step(&mut tab, &screen(&[LIMIT]), 10), Action::GiveUp);
        assert_eq!(step(&mut tab, &screen(&[BOX]), 20), Action::Clear);
        assert_eq!(step(&mut tab, &screen(&[BOX]), 25), Action::Nothing);
        assert_eq!(step(&mut tab, &screen(&[LIMIT]), 30), Action::GiveUp);
    }

    #[test]
    fn a_stuck_home_chat_starts_fresh_once_per_session() {
        assert!(home_starts_fresh(&screen(&[TOO_LONG, BOX]), 7, None));
        assert!(!home_starts_fresh(&screen(&[TOO_LONG, BOX]), 7, Some(7)), "never twice for one session");
        assert!(home_starts_fresh(&screen(&[LIMIT]), 8, Some(7)), "the new session is watched again");
        assert!(!home_starts_fresh(&screen(&[BOX]), 8, None));
    }

    #[test]
    fn the_given_up_line_names_what_to_do() {
        assert!(GAVE_UP_LINE.contains("Start a new session in this tab"));
        assert!(!GAVE_UP_LINE.to_lowercase().contains("alice"));
    }
}
