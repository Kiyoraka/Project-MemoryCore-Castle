//! The companion's session: one Windows ConPTY that Castle owns, running the member's CLI in their
//! MemoryCore folder. Castle only carries keystrokes in and screen bytes out.

use crate::env::NEVER_INHERITED;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// How much of the session's output is kept so a reloaded screen can be redrawn.
const SCROLLBACK_LIMIT: usize = 512 * 1024;

/// How many leading bytes are complete UTF-8. An unfinished character at the end waits for the
/// next read; anything truly invalid is passed on and shown as a replacement mark.
pub fn complete_utf8_len(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Err(_) => bytes.len(),
    }
}

/// The command that starts the CLI in `folder`. A .cmd or .bat shim (an npm install) runs
/// through cmd.exe; a real program runs directly. `args` go to the CLI itself.
pub fn launch_command(program: &Path, folder: &Path, vars: &[(String, String)], args: &[String]) -> CommandBuilder {
    let is_script = program
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    // platform: windows-only - a .cmd / .bat launcher exists only on Windows
    let mut command = if is_script {
        let mut command = CommandBuilder::new("cmd.exe");
        command.arg("/c");
        command.arg(program.as_os_str());
        command
    } else {
        CommandBuilder::new(program.as_os_str())
    };
    for arg in args {
        command.arg(arg);
    }
    command.cwd(folder);
    for name in NEVER_INHERITED {
        command.env_remove(name);
    }
    command.env("TERM", "xterm-256color");
    // macOS: an app opened from the Finder has no locale, and the CLI's shells would mangle
    // anything that is not plain English text
    #[cfg(not(windows))]
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        command.env("LANG", "en_US.UTF-8");
    }
    command.env("COLORTERM", "truecolor");
    for (name, value) in vars {
        command.env(name, value);
    }
    command
}

/// One running console. Dropping it closes the console, which ends the reader.
pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// The program's process id (DEV's live servers are traced back to it).
    pid: Option<u32>,
}

impl Session {
    /// Starts `command` in a new console. `on_output` gets each complete piece of text;
    /// `on_exit` gets the exit code once the program ends.
    pub fn spawn(
        command: CommandBuilder,
        cols: u16,
        rows: u16,
        on_output: impl Fn(String) + Send + 'static,
        on_exit: impl FnOnce(Option<u32>) + Send + 'static,
    ) -> Result<Session, String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|_| "A console could not be opened.".to_string())?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|_| "Your companion's CLI could not be started.".to_string())?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| "The console could not be read.".to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| "The console could not be written to.".to_string())?;
        let killer = child.clone_killer();
        let pid = child.process_id();

        std::thread::spawn(move || {
            let mut pending = Vec::new();
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        pending.extend_from_slice(&buffer[..read]);
                        let complete = complete_utf8_len(&pending);
                        if complete > 0 {
                            let piece: Vec<u8> = pending.drain(..complete).collect();
                            on_output(String::from_utf8_lossy(&piece).into_owned());
                        }
                    }
                }
            }
        });

        std::thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            on_exit(code);
        });

        Ok(Session { master: pair.master, writer, killer, pid })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(data)
            .and_then(|_| self.writer.flush())
            .map_err(|_| "The keys could not reach the console.".to_string())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|_| "The console could not be resized.".to_string())
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }
}

/// One session host and what it has shown so far. Castle has two: the companion's main session
/// and the STORY room's.
#[derive(Default)]
pub struct PtyHost {
    provider: Mutex<Option<crate::settings::Cli>>,
    input_time: Mutex<Option<std::time::SystemTime>>,
    session: Mutex<Option<(u64, Session)>>,
    next_id: Mutex<u64>,
    scrollback: Arc<Mutex<Vec<u8>>>,
    /// When the current session started, and its id, so Chat can find its record.
    started: Mutex<Option<(u64, std::time::SystemTime)>>,
    /// When the session last printed anything (the Neural Link knocks only into a quiet one).
    last_output: Arc<Mutex<Option<std::time::Instant>>>,
    /// When the member last typed into it.
    last_input: Mutex<Option<std::time::Instant>>,
    /// When the session's recent turns were sent (Enter), newest last: a record the CLI starts
    /// right after one of them is this session's own (a /clear goes on in a new record).
    enters: Mutex<std::collections::VecDeque<std::time::SystemTime>>,
    /// The session started on another model because its own weekly share was spent.
    fallback: Mutex<Option<crate::model_pick::Fallback>>,
}

/// How many sent turns the host remembers for finding a record a /clear started.
const ENTERS_KEPT: usize = 32;

impl PtyHost {
    pub fn last_input_time(&self) -> Option<std::time::SystemTime> { *self.input_time.lock().unwrap() }

    /// When this session's recent turns were sent, oldest first.
    pub fn recent_enters(&self) -> Vec<std::time::SystemTime> {
        self.enters.lock().unwrap().iter().copied().collect()
    }

    fn note_enter(&self) {
        let mut enters = self.enters.lock().unwrap();
        enters.push_back(std::time::SystemTime::now());
        while enters.len() > ENTERS_KEPT {
            enters.pop_front();
        }
    }
    pub fn provider(&self) -> Option<crate::settings::Cli> {
        if self.is_running() { *self.provider.lock().unwrap() } else { None }
    }

    /// What this session started on instead of the member's model, when it did; kept while it runs.
    pub fn fallback(&self) -> Option<crate::model_pick::Fallback> {
        if self.is_running() { self.fallback.lock().unwrap().clone() } else { None }
    }

    pub fn set_fallback(&self, fallback: Option<crate::model_pick::Fallback>) {
        *self.fallback.lock().unwrap() = fallback;
    }

    pub fn bind_provider(&self, provider: crate::settings::Cli) {
        *self.provider.lock().unwrap() = Some(provider);
    }
    pub fn is_running(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }

    /// The running session's process id (DEV's live servers are traced back to it).
    pub fn pid(&self) -> Option<u32> {
        self.session.lock().unwrap().as_ref().and_then(|(_, session)| session.pid)
    }

    /// The running session's id and start time.
    pub fn started(&self) -> Option<(u64, std::time::SystemTime)> {
        if self.is_running() {
            *self.started.lock().unwrap()
        } else {
            None
        }
    }

    /// Starts the session unless one is already running. Returns false when one was.
    pub fn start(
        &self,
        command: CommandBuilder,
        cols: u16,
        rows: u16,
        on_output: impl Fn(String) + Send + 'static,
        on_exit: impl FnOnce(u64, Option<u32>) + Send + 'static,
    ) -> Result<bool, String> {
        let mut slot = self.session.lock().unwrap();
        if slot.is_some() {
            return Ok(false);
        }
        let id = {
            let mut next = self.next_id.lock().unwrap();
            *next += 1;
            *next
        };
        self.scrollback.lock().unwrap().clear();
        self.enters.lock().unwrap().clear();
        let scrollback = Arc::clone(&self.scrollback);
        let last_output = Arc::clone(&self.last_output);
        *last_output.lock().unwrap() = Some(std::time::Instant::now());
        let session = Session::spawn(
            command,
            cols,
            rows,
            move |text| {
                {
                    let mut kept = scrollback.lock().unwrap();
                    kept.extend_from_slice(text.as_bytes());
                    if kept.len() > SCROLLBACK_LIMIT {
                        let excess = kept.len() - SCROLLBACK_LIMIT;
                        kept.drain(..excess);
                    }
                }
                *last_output.lock().unwrap() = Some(std::time::Instant::now());
                on_output(text);
            },
            move |code| on_exit(id, code),
        )?;
        *slot = Some((id, session));
        *self.started.lock().unwrap() = Some((id, std::time::SystemTime::now()));
        Ok(true)
    }

    /// Called when a session's program ended; an older session's ending never clears a newer one.
    pub fn finished(&self, id: u64) {
        let mut slot = self.session.lock().unwrap();
        if matches!(slot.as_ref(), Some((current, _)) if *current == id) {
            *slot = None;
        }
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        *self.input_time.lock().unwrap() = Some(std::time::SystemTime::now());
        *self.last_input.lock().unwrap() = Some(std::time::Instant::now());
        if data.contains(&b'\r') {
            self.note_enter();
        }
        self.write_quietly(data)
    }

    /// Something was handed to the session as if typed (a provider handoff prompt).
    pub fn mark_input(&self) {
        *self.input_time.lock().unwrap() = Some(std::time::SystemTime::now());
    }

    /// The terminal's answer to the console's own question: not the member's input, so it does not
    /// count as the last thing they typed.
    pub fn reply(&self, data: &[u8]) -> Result<(), String> {
        self.write_quietly(data)
    }

    fn write_quietly(&self, data: &[u8]) -> Result<(), String> {
        match self.session.lock().unwrap().as_mut() {
            Some((_, session)) => session.write(data),
            None => Err("Your companion's session is not running.".into()),
        }
    }

    /// How the session looks to the Neural Link: running, how long quiet, how long since typing.
    pub fn idle(&self) -> crate::link::Idle {
        let since = |at: Option<std::time::Instant>| at.map(|t| t.elapsed()).unwrap_or(std::time::Duration::from_secs(3600));
        crate::link::Idle {
            running: self.is_running(),
            quiet: since(*self.last_output.lock().unwrap()),
            typed: since(*self.last_input.lock().unwrap()),
        }
    }

    /// Types one word and Enter, the way a quick action does - Castle's knock, which never counts
    /// as the member typing.
    pub fn knock(&self, word: &str) -> Result<(), String> {
        // the knock starts a turn: a provider switch must wait for it to end, even on a session
        // nobody typed into (the Neural Link's idle reading, `last_input`, is left alone)
        self.mark_input();
        self.write_quietly(word.as_bytes())?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        self.note_enter();
        self.write_quietly(b"\r")
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        match self.session.lock().unwrap().as_ref() {
            Some((_, session)) => session.resize(cols, rows),
            None => Ok(()),
        }
    }

    /// Everything the session has shown, for redrawing a reloaded screen.
    pub fn snapshot(&self) -> String {
        String::from_utf8_lossy(&self.scrollback.lock().unwrap()).into_owned()
    }

    pub fn kill(&self) {
        if let Some((_, session)) = self.session.lock().unwrap().as_mut() {
            session.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn complete_text_passes_and_an_unfinished_character_waits() {
        assert_eq!(complete_utf8_len(b"hello"), 5);
        let heart = "\u{1F49C}".as_bytes(); // four bytes
        let mut split = b"ok ".to_vec();
        split.extend_from_slice(&heart[..2]);
        assert_eq!(complete_utf8_len(&split), 3);
        assert_eq!(complete_utf8_len(&[0xff, b'a']), 2);
    }

    #[test]
    fn inherited_keys_and_markers_are_dropped_and_chosen_ones_are_set() {
        let vars = vec![("SUNOAPI_ORG_KEY".to_string(), "chosen".to_string())];
        let command = launch_command(Path::new("C:\\npm\\codex.cmd"), Path::new("C:\\"), &vars, &[]);
        for name in ["CLAUDECODE", "OPENAI_API_KEY", "ARK_API_KEY"] {
            assert!(command.get_env(name).is_none(), "{name} leaked");
        }
        assert_eq!(command.get_env("SUNOAPI_ORG_KEY").and_then(|v| v.to_str()), Some("chosen"));
        assert_eq!(command.get_env("TERM").and_then(|v| v.to_str()), Some("xterm-256color"));
    }

    #[test]
    fn a_cmd_shim_runs_through_cmd_and_a_program_runs_directly() {
        let shim = launch_command(Path::new("C:\\npm\\codex.cmd"), Path::new("C:\\"), &[], &[]);
        assert_eq!(shim.get_argv()[0].to_str(), Some("cmd.exe"));
        assert_eq!(shim.get_argv()[2].to_str(), Some("C:\\npm\\codex.cmd"));
        let program = launch_command(Path::new("C:\\bin\\claude.exe"), Path::new("C:\\"), &[], &[]);
        assert_eq!(program.get_argv()[0].to_str(), Some("C:\\bin\\claude.exe"));
    }

    #[test]
    fn a_real_console_carries_text_out_and_reports_the_end() {
        let host = PtyHost::default();
        #[cfg(windows)]
        let mut command = {
            let mut c = CommandBuilder::new("cmd.exe");
            c.args(["/c", "echo castle-console-test-\u{65E5}\u{672C}"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut c = CommandBuilder::new("/bin/sh");
            c.args(["-c", "echo castle-console-test-\u{65E5}\u{672C}"]);
            c
        };
        command.cwd(std::env::temp_dir());

        let (text_tx, text_rx) = mpsc::channel::<String>();
        let (exit_tx, exit_rx) = mpsc::channel::<(u64, Option<u32>)>();
        let started = host
            .start(command, 80, 24, move |t| { let _ = text_tx.send(t); }, move |id, code| {
                let _ = exit_tx.send((id, code));
            })
            .unwrap();
        assert!(started);
        assert!(host.is_running());

        // Windows' console asks where the cursor is before it runs anything; a terminal answers.
        let mut seen = String::new();
        let mut answered = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let (id, code) = loop {
            if let Ok(ending) = exit_rx.try_recv() {
                break ending;
            }
            assert!(std::time::Instant::now() < deadline, "the program never ended; output was: {seen:?}");
            if let Ok(piece) = text_rx.recv_timeout(Duration::from_millis(200)) {
                seen.push_str(&piece);
                if !answered && seen.contains("\x1b[6n") {
                    host.write(b"\x1b[1;1R").unwrap();
                    answered = true;
                }
            }
        };
        assert_eq!(code, Some(0));
        host.finished(id);
        assert!(!host.is_running());

        while let Ok(piece) = text_rx.recv_timeout(Duration::from_secs(2)) {
            seen.push_str(&piece);
        }
        assert!(seen.contains("castle-console-test-\u{65E5}\u{672C}"), "output was: {seen:?}");
        assert!(host.snapshot().contains("castle-console-test-"));
    }

    #[test]
    fn the_clis_own_arguments_follow_the_program_through_a_shim_or_directly() {
        let args = vec!["--append-system-prompt-file".to_string(), "C:\\boot\\main.md".to_string()];
        let shim = launch_command(Path::new("C:\\npm\\claude.cmd"), Path::new("C:\\"), &[], &args);
        let words: Vec<String> = shim.get_argv().iter().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(words, vec!["cmd.exe", "/c", "C:\\npm\\claude.cmd", "--append-system-prompt-file", "C:\\boot\\main.md"]);
        let program = launch_command(Path::new("C:\\bin\\claude.exe"), Path::new("C:\\"), &[], &args);
        let words: Vec<String> = program.get_argv().iter().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(words, vec!["C:\\bin\\claude.exe", "--append-system-prompt-file", "C:\\boot\\main.md"]);
    }
}
