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
    run_as(cli, folder, rules_file, words, limit, None)
}

/// A model name the CLI may be given: letters, digits, `.`, `-`, `_` and `:`, never starting with
/// `-`, at most 64 characters - so it can never carry another flag.
pub fn clean_model(name: &str) -> Option<String> {
    let name = name.trim();
    let fine = !name.is_empty() && name.len() <= 64 && !name.starts_with('-') && name.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:".contains(c));
    fine.then(|| name.to_string())
}

/// The model flag for this CLI, when a model is chosen.
pub fn model_args(cli: Cli, model: Option<&str>) -> Vec<String> {
    match model.and_then(clean_model) {
        Some(m) => match cli {
            Cli::Claude => vec!["--model".into(), m],
            Cli::Codex => vec!["-m".into(), m],
            // no background runs for them yet (see `run_as`), so no model flag either
            Cli::Gemini | Cli::Grok => Vec::new(),
        },
        None => Vec::new(),
    }
}

/// Whether this CLI may run a background job. A background run reads other people's words (room
/// guests, the market), so it runs with no tools (Claude Code) or read-only (Codex). Grok's way to
/// switch its tools off and agy's headless mode are not known yet (docs/provider-records.md), so
/// they are refused in words, never run with their tools on.
pub fn runs_in_background(cli: Cli) -> Result<(), String> {
    match cli {
        Cli::Claude | Cli::Codex => Ok(()),
        Cli::Gemini | Cli::Grok => Err(format!("This runs on Claude Code or Codex; {} cannot yet be run safely in the background.", cli.label())),
    }
}

/// `run`, on a chosen model (None: the CLI's own default).
pub fn run_as(cli: Cli, folder: &Path, rules_file: &str, words: &str, limit: Duration, model: Option<&str>) -> Result<String, String> {
    runs_in_background(cli)?;
    let program = env::find_on_path(cli.command()).ok_or_else(|| format!("{} is not installed.", cli.label()))?;
    let script = program.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    // platform: windows-only - a .cmd / .bat launcher exists only on Windows
    let mut command = if script {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(&program);
        c
    } else {
        Command::new(&program)
    };
    let answer_file = folder.join("last-answer.txt");
    let _ = std::fs::remove_file(&answer_file);
    let model = model_args(cli, model);
    match cli {
        // no tools at all, no MCP servers, no user-level settings or memory - only this folder
        Cli::Claude => command.args(["-p", "--tools", "", "--strict-mcp-config", "--setting-sources", "project", "--output-format", "text", "--system-prompt-file", rules_file]).args(&model),
        // Codex reads AGENTS.md in this folder as its instructions
        Cli::Codex => command.arg("exec").args(&model).args(["--sandbox", "read-only", "--skip-git-repo-check", "--output-last-message"]).arg(&answer_file).arg("-"),
        Cli::Gemini | Cli::Grok => unreachable!("refused by runs_in_background above"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_name_can_never_carry_another_flag() {
        assert_eq!(clean_model(" sonnet "), Some("sonnet".into()));
        assert_eq!(clean_model("gpt-5.1-codex"), Some("gpt-5.1-codex".into()));
        assert_eq!(clean_model("claude-opus-4:latest"), Some("claude-opus-4:latest".into()));
        let long = "x".repeat(65);
        for bad in ["", "-p", "--tools x", "sonnet --tools Bash", "a;b", "a\"b", long.as_str()] {
            assert_eq!(clean_model(bad), None, "{bad}");
        }
    }

    #[test]
    fn each_cli_gets_its_own_model_flag() {
        assert_eq!(model_args(Cli::Claude, Some("sonnet")), vec!["--model", "sonnet"]);
        assert_eq!(model_args(Cli::Codex, Some("gpt-5")), vec!["-m", "gpt-5"]);
        assert!(model_args(Cli::Claude, None).is_empty());
        assert!(model_args(Cli::Codex, Some("--bad")).is_empty());
    }

    #[test]
    fn gemini_and_grok_are_refused_in_words_before_anything_runs() {
        assert!(runs_in_background(Cli::Claude).is_ok() && runs_in_background(Cli::Codex).is_ok());
        for cli in [Cli::Gemini, Cli::Grok] {
            let refusal = run_as(cli, Path::new("C:/nowhere"), "rules.md", "hi", Duration::from_secs(1), None).unwrap_err();
            assert!(refusal.contains("Claude Code or Codex") && refusal.contains(cli.label()), "{refusal}");
        }
    }
}
