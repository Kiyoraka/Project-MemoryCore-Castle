//! One run of the member's own CLI, with no tools: the words go in on stdin, the answer comes back
//! as text. The chat room's turns and the emotion engine's proposals both use it, each in its own
//! folder with its own rules file, so neither ever sees the memory folder or the member's own setup.

use crate::env::{self, NEVER_INHERITED};
use crate::settings::Cli;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Writes a file only when its text changed.
pub fn write_if_changed(path: &Path, text: &str) {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        let _ = std::fs::write(path, text);
    }
}

/// Runs the CLI once in `folder` with `rules_file` (a file in that folder) as its whole system
/// prompt, no tools, no MCP servers, no user-level settings; stopped after `limit`.
pub fn run(cli: Cli, folder: &Path, rules_file: &str, words: &str, limit: Duration) -> Result<String, String> {
    let program = env::find_on_path(cli.command()).ok_or_else(|| format!("{} is not installed.", cli.label()))?;
    let script = program.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut command = if script {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(&program);
        c
    } else {
        Command::new(&program)
    };
    let answer_file = folder.join("last-answer.txt");
    let _ = std::fs::remove_file(&answer_file);
    match cli {
        // no tools at all, no MCP servers, no user-level settings or memory - only this folder
        Cli::Claude => command.args(["-p", "--tools", "", "--strict-mcp-config", "--setting-sources", "project", "--output-format", "text", "--system-prompt-file", rules_file]),
        // Codex reads AGENTS.md in this folder as its instructions
        Cli::Codex => command.args(["exec", "--sandbox", "read-only", "--skip-git-repo-check", "--output-last-message"]).arg(&answer_file).arg("-"),
    };
    command.current_dir(folder).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for name in NEVER_INHERITED {
        command.env_remove(name);
    }
    // no tools and no door to the memory database
    for name in ["CASTLE_TOOLS", "CASTLE_STYLES", "CASTLE_IMAGE", "CASTLE_CLI", "CASTLE_EXE", "CASTLE_MEMORY"] {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| format!("{} could not be started.", cli.label()))?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(words.as_bytes());
    }
    let mut stdout = child.stdout.take().ok_or("No output.")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let until = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                return Err("It took too long and was stopped.".into());
            }
        }
    }
    let out = reader.join().unwrap_or_default();
    Ok(std::fs::read_to_string(&answer_file).ok().filter(|a| !a.trim().is_empty()).unwrap_or(out))
}
