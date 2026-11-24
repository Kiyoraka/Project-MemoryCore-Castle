use super::*;
use mysql::prelude::Queryable;
use std::time::Duration;
pub fn capped(s: &str, cap: usize) -> String {
    let mut end = s.len().min(cap);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
pub fn scrub(mut text: String, secrets: &[String]) -> String {
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        text = text.replace(secret, "[redacted]");
    }
    text
}
pub fn execute(app: &AppHandle, j: &Value, run: u64, payload: &Value) -> Result<String, String> {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    let mut c = connection(&store)?;
    let template = j["action"]["prompt"].as_str().unwrap_or("").to_string();
    let mut contexts = std::collections::BTreeMap::new();
    // Explicit read-only context; never read retired files or give the model a DB connection.
    for token in ["work_log", "reminders", "projects", "today", "payload.text"] {
        if !template.contains(&format!("{{{{{token}}}}}")) {
            continue;
        }
        let context=match token {
   "work_log"=>c.exec_first::<String,_,_>("SELECT COALESCE(body,'') FROM projects WHERE name=?",("Shared work log",)).map_err(|_|"Work log could not be read.")?.unwrap_or_default(),
   "reminders"=>c.query::<String,_>("SELECT CONCAT(title,' ',COALESCE(CAST(due_date AS CHAR),'')) FROM reminders WHERE state='open' ORDER BY due_date LIMIT 100").map_err(|_|"Reminders could not be read.")?.join("\n"),
   "projects"=>c.query::<String,_>("SELECT CONCAT(name,': ',COALESCE(summary,'')) FROM projects WHERE shelf='active' LIMIT 100").map_err(|_|"Projects could not be read.")?.join("\n"),
   "payload.text"=>payload["text"].as_str().unwrap_or("").to_string(),
   "today"=>chrono::Local::now().format("%A %Y-%m-%d %H:%M").to_string(), _=>String::new()
  };
        contexts.insert(token.to_string(), capped(&context, 24000));
    }
    let prompt = render(&template, &contexts);
    let folder = store
        .folder()
        .join("automations")
        .join(format!("run-{run}"));
    std::fs::create_dir_all(&folder).map_err(|_| "The isolated job folder could not be made.")?;
    let rules = include_str!("../../resources/automation-rules.md");
    crate::oneshot::write_if_changed(&folder.join("AGENTS.md"), rules);
    crate::oneshot::write_if_changed(&folder.join("rules.md"), rules);
    let cli = j["action"]["cli"].as_str().and_then(crate::settings::Cli::from_id).unwrap_or(settings.cli);
    let timeout = Duration::from_secs(j["policy"]["timeout_s"].as_u64().unwrap_or(120));
    let result = run_isolated(
        cli,
        &folder,
        &prompt,
        timeout,
        j["action"]["model"].as_str(),
    );
    // These exact files are owned by this run; never recursively remove an inferred path.
    for name in ["AGENTS.md", "rules.md", "last-answer.txt"] {
        let _ = std::fs::remove_file(folder.join(name));
    }
    let _ = std::fs::remove_dir(&folder);
    let mut secrets: Vec<String> = crate::key::KEYS
        .iter()
        .filter_map(|(k, _)| crate::key::load(k))
        .collect();
    if let Some(memory) = settings.memory_folder {
        if let Ok(env) = std::fs::read_to_string(std::path::Path::new(&memory).join(".env")) {
            for line in env.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    if ["PASSWORD", "SECRET", "TOKEN", "KEY"]
                        .iter()
                        .any(|s| key.to_uppercase().contains(s))
                    {
                        let v = value.trim().trim_matches('"');
                        if !v.is_empty() {
                            secrets.push(v.to_string());
                        }
                    }
                }
            }
        }
    }
    match result {
        Ok(text) => Ok(capped(
            &scrub(text, &secrets),
            j["policy"]["max_output_kb"].as_u64().unwrap_or(64) as usize * 1024,
        )),
        Err(e) => Err(scrub(e, &secrets)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_is_text_bounded_and_scrubbed() {
        assert_eq!(capped("a\u{e9}z", 2), "a");
        assert_eq!(
            scrub("key SECRET and SECRET".into(), &["SECRET".into()]),
            "key [redacted] and [redacted]"
        );
    }
}

/// Use a fresh CLI process, no user config/MCP/apps/shell, and bounded, drained pipes. None for a
/// provider whose isolated run Castle does not know yet (Gemini, Grok: Task 8).
fn cli_args(cli: crate::settings::Cli, model: Option<&str>) -> Option<Vec<String>> {
    let mut args: Vec<String> = match cli {
        crate::settings::Cli::Claude => [
            "-p",
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "project",
            "--output-format",
            "text",
            "--system-prompt-file",
            "rules.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        crate::settings::Cli::Codex => [
            "exec",
            "--ignore-user-config",
            "--ignore-rules",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--color",
            "never",
            "-c",
            "web_search=\"disabled\"",
            "-c",
            "features.shell_tool=false",
            "-c",
            "features.unified_exec=false",
            "-c",
            "features.apps=false",
            "-c",
            "features.browser_use=false",
            "-c",
            "features.computer_use=false",
            "-c",
            "features.multi_agent=false",
            "-c",
            "features.multi_agent_v2=false",
            "-c",
            "features.image_generation=false",
            "-c",
            "features.plugins=false",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        crate::settings::Cli::Gemini | crate::settings::Cli::Grok => return None,
    };
    args.extend(crate::oneshot::model_args(cli, model));
    match cli {
        // Codex reads the prompt from stdin when told "-"
        crate::settings::Cli::Codex => args.push("-".into()),
        crate::settings::Cli::Claude | crate::settings::Cli::Gemini | crate::settings::Cli::Grok => {}
    }
    Some(args)
}
fn drain(mut pipe: impl std::io::Read, cap: usize) -> String {
    let mut kept = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let left = cap.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(left)]);
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn run_isolated(
    cli: crate::settings::Cli,
    folder: &std::path::Path,
    prompt: &str,
    limit: Duration,
    model: Option<&str>,
) -> Result<String, String> {
    use std::process::{Command, Stdio};
    let args = cli_args(cli, model).ok_or_else(|| format!("Briefings need Claude Code or Codex; {} cannot run one yet.", cli.label()))?;
    let program = crate::env::find_on_path(cli.command())
        .ok_or("The selected companion provider is not installed.")?;
    let mut cmd = if program
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
    {
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/c"]).arg(&program);
        c
    } else {
        Command::new(program)
    };
    cmd.args(args)
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in crate::env::NEVER_INHERITED.iter().copied().chain([
        "CASTLE_TOOLS",
        "CASTLE_EXE",
        "CASTLE_MEMORY",
        "CASTLE_IMAGE",
        "CASTLE_STYLES",
        "CASTLE_CLI",
    ]) {
        cmd.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    run_command(cmd, prompt, limit)
}
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}
fn run_command(
    cmd: std::process::Command,
    prompt: &str,
    limit: Duration,
) -> Result<String, String> {
    let result = capture(cmd, prompt, limit)?;
    if result.timed_out {
        return Err("It took too long and was stopped.".into());
    }
    if result.exit_code == Some(0) && !result.stdout.trim().is_empty() {
        Ok(result.stdout)
    } else {
        Err(format!("Provider failed: {}", capped(&result.stderr, 2000)))
    }
}
pub fn capture(
    mut cmd: std::process::Command,
    input: &str,
    limit: Duration,
) -> Result<ProcessOutput, String> {
    use std::io::Write;
    use std::time::Instant;
    let mut child = cmd.spawn().map_err(|_| "The program could not start.")?;
    #[cfg(windows)]
    let tree = ProcessTree::attach(&mut child)?;
    let mut pipe = child.stdin.take().ok_or("No input stream.")?;
    let words = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = pipe.write_all(words.as_bytes());
    });
    let stdout = child.stdout.take().ok_or("No output stream.")?;
    let stderr = child.stderr.take().ok_or("No error stream.")?;
    let out = std::thread::spawn(move || drain(stdout, 65536));
    let err = std::thread::spawn(move || drain(stderr, 65536));
    let until = Instant::now() + limit;
    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                #[cfg(windows)]
                tree.stop();
                kill_tree(&mut child);
                break (None, true);
            }
        }
    };
    #[cfg(windows)]
    tree.stop();
    let _ = writer.join();
    Ok(ProcessOutput {
        exit_code,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
        timed_out,
    })
}
pub struct Report {
    pub status: &'static str,
    pub output: String,
    pub error: Option<String>,
}
pub fn perform(app: &AppHandle, j: &Value, run: u64, payload: &Value) -> Report {
    if j["action"]["kind"] == "http" {
        let result=super::http::spec(&j["action"]).and_then(|s| {
            if payload["cause"]=="test" { return Ok("HTTP action valid. No request or delivery was made.".into()); }
            super::http::run(&s,payload,Duration::from_secs(j["policy"]["timeout_s"].as_u64().unwrap_or(120)),j["policy"]["max_output_kb"].as_u64().unwrap_or(64) as usize*1024,if s.key.is_empty(){None}else{crate::key::load(&s.key)})
        });
        let secrets=known_secrets(&app.state::<SettingsStore>());
        return match result { Ok(output)=>Report{status:"ok",output:capped(&scrub(output,&secrets),j["policy"]["max_output_kb"].as_u64().unwrap_or(64) as usize*1024),error:None},Err(error)=>Report{status:if error.contains("too long"){"timed_out"}else{"failed"},output:String::new(),error:Some(scrub(error,&secrets))} };
    }
    if j["action"]["kind"] == "castle" {
        let started = std::time::Instant::now();
        let result = super::castle_actions::execute(app, j, run, payload);
        let secrets = known_secrets(&app.state::<SettingsStore>());
        return match result {
            Ok(output) => Report {
                status: if started.elapsed()
                    > Duration::from_secs(j["policy"]["timeout_s"].as_u64().unwrap_or(120))
                {
                    "timed_out"
                } else {
                    "ok"
                },
                output: capped(
                    &scrub(output, &secrets),
                    j["policy"]["max_output_kb"].as_u64().unwrap_or(64) as usize * 1024,
                ),
                error: (started.elapsed()>Duration::from_secs(j["policy"]["timeout_s"].as_u64().unwrap_or(120))).then(||"The Castle action completed after its time limit. Its result was kept; it was not repeated.".into()),
            },
            Err(error) => Report {
                status: if started.elapsed()>=Duration::from_secs(j["policy"]["timeout_s"].as_u64().unwrap_or(120)){"timed_out"}else{"failed"},
                output: String::new(),
                error: Some(scrub(error, &secrets)),
            },
        };
    }
    if j["action"]["kind"] == "command" {
        let result = super::commands::spec(&j["action"]).and_then(|spec| {
            super::commands::run(
                &spec,
                &app.state::<SettingsStore>().get().automations_commands,
                payload,
                Duration::from_secs(j["policy"]["timeout_s"].as_u64().unwrap_or(120)),
            )
        });
        let secrets = known_secrets(&app.state::<SettingsStore>());
        match result {
            Ok(result) => {
                let status = if result.timed_out {
                    "timed_out"
                } else if result.exit_code == Some(0) {
                    "ok"
                } else {
                    "failed"
                };
                let error = if result.timed_out {
                    Some("The command took too long; its process group was stopped.".into())
                } else if result.exit_code != Some(0) {
                    Some(format!(
                        "Command exited with code {}.",
                        result
                            .exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "unavailable".into())
                    ))
                } else {
                    None
                };
                Report {
                    status,
                    output: capped(
                        &scrub(super::commands::format(&result), &secrets),
                        j["policy"]["max_output_kb"].as_u64().unwrap_or(64) as usize * 1024,
                    ),
                    error,
                }
            }
            Err(error) => Report {
                status: "failed",
                output: String::new(),
                error: Some(scrub(error, &secrets)),
            },
        }
    } else {
        match execute(app, j, run, payload) {
            Ok(output) => Report {
                status: "ok",
                output,
                error: None,
            },
            Err(error) => Report {
                status: if error.contains("too long") {
                    "timed_out"
                } else {
                    "failed"
                },
                output: String::new(),
                error: Some(error),
            },
        }
    }
}
pub fn known_secrets(store: &SettingsStore) -> Vec<String> {
    secrets_for(store.get().memory_folder.as_deref())
}
pub(super) fn secrets_for(memory: Option<&str>) -> Vec<String> {
    let mut secrets: Vec<String> = crate::key::KEYS
        .iter()
        .filter_map(|(k, _)| crate::key::load(k))
        .collect();
    if let Some(folder) = memory {
        if let Ok(text) = std::fs::read_to_string(std::path::Path::new(&folder).join(".env")) {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    if ["PASSWORD", "SECRET", "TOKEN", "KEY"]
                        .iter()
                        .any(|s| key.to_ascii_uppercase().contains(s))
                    {
                        let value = value.trim().trim_matches('"');
                        if !value.is_empty() {
                            secrets.push(value.to_string());
                        }
                    }
                }
            }
        }
    }
    secrets
}
#[cfg(test)]
mod isolation_tests {
    use super::*;
    #[test]
    fn cli_blocks_tools_and_user_setup() {
        let args = cli_args(crate::settings::Cli::Codex, None).unwrap();
        for flag in [
            "--ignore-user-config",
            "--ephemeral",
            "features.shell_tool=false",
            "features.apps=false",
            "features.browser_use=false",
        ] {
            assert!(args.contains(&flag.to_string()));
        }
        let args = cli_args(crate::settings::Cli::Claude, None).unwrap();
        assert!(args.windows(2).any(|w| w == ["--tools", ""]));
        assert!(!args.contains(&"-".to_string()), "Claude Code gets its prompt otherwise");
        assert!(cli_args(crate::settings::Cli::Gemini, None).is_none());
        assert!(cli_args(crate::settings::Cli::Grok, None).is_none());
    }
    #[test]
    fn pipe_is_drained_but_bounded() {
        assert_eq!(
            drain(std::io::Cursor::new(vec![b'x'; 100000]), 123).len(),
            123
        );
    }
}

#[cfg(all(test, windows))]
mod process_tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    fn fixture(script: &str) -> Command {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000);
        c
    }
    #[test]
    fn provider_exit_failure_is_not_success() {
        let e = run_command(
            fixture("[Console]::Error.Write('fixture failure'); exit 7"),
            "",
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(e.contains("fixture failure"));
    }
    #[test]
    fn timeout_stops_process() {
        let start = std::time::Instant::now();
        let e = run_command(
            fixture("Start-Sleep -Seconds 60"),
            "",
            Duration::from_millis(500),
        )
        .unwrap_err();
        assert!(e.contains("too long"));
        assert!(start.elapsed() < Duration::from_secs(10));
    }
    #[test]
    fn output_and_stderr_are_drained() {
        let text = run_command(
            fixture("[Console]::Error.Write(('e'*100000));[Console]::Write(('x'*100000))"),
            "",
            Duration::from_secs(15),
        )
        .unwrap();
        assert_eq!(text.len(), 65536);
    }
}

#[cfg(test)]
mod provider_smoke {
    use super::*;
    #[test]
    #[ignore]
    fn actual_codex_briefing() {
        let folder =
            std::env::temp_dir().join(format!("castle-automation-provider-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let rules = include_str!("../../resources/automation-rules.md");
        std::fs::write(folder.join("AGENTS.md"), rules).unwrap();
        let result=run_isolated(crate::settings::Cli::Codex,&folder,"This is an isolated automation integration check. Reply with exactly AUTOMATION_BRIEFING_OK. Do not call any tools.",Duration::from_secs(60),None);
        let _ = std::fs::remove_file(folder.join("AGENTS.md"));
        let _ = std::fs::remove_dir(&folder);
        assert_eq!(result.unwrap().trim(), "AUTOMATION_BRIEFING_OK");
    }
}

#[cfg(windows)]
struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &mut std::process::Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                kill_tree(child);
                return Err("The isolated process limit could not be created.".into());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
            {
                CloseHandle(job);
                kill_tree(child);
                return Err("The provider could not be isolated in a process group.".into());
            }
            Ok(Self(job))
        }
    }
    fn stop(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Substitute only tokens the member actually wrote. Context cannot introduce more tokens.
pub(super) fn render(template: &str, contexts: &std::collections::BTreeMap<String, String>) -> String {
    let mut rest = template;
    let mut out = String::new();
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        if let Some(end) = tail.find("}}") {
            let token = &tail[..end];
            if let Some(value) = contexts.get(token) {
                out.push_str(value);
            } else {
                out.push_str(&rest[start..start + end + 4]);
            }
            rest = &tail[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}
#[cfg(test)]
mod template_tests {
    use super::*;
    #[test]
    fn context_is_never_a_recursive_template() {
        let map = std::collections::BTreeMap::from([
            ("work_log".into(), "Data {{projects}} ; calc".into()),
            ("projects".into(), "private context".into()),
        ]);
        assert_eq!(render("{{work_log}}", &map), "Data {{projects}} ; calc");
    }
}
