//! Member-approved native programs. Paths and arguments are never built from a payload.
use super::actions::{self, ProcessOutput};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub kind: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "no_input")]
    pub stdin: String,
}
fn no_input() -> String {
    "none".into()
}
pub fn canonical_program(value: &str) -> Result<PathBuf, String> {
    if value.len() > 2048 || value.contains(['\0', '\n', '\r']) || !Path::new(value).is_absolute() {
        return Err("Choose the program's full path.".into());
    }
    #[cfg(windows)]
    {
        let plain = value.strip_prefix(r"\\?\").unwrap_or(value);
        if plain.starts_with(r"\\")
            || plain.to_ascii_uppercase().starts_with("UNC\\")
            || plain
                .split(['\\', '/'])
                .any(|s| s != "." && s != ".." && (s.ends_with(' ') || s.ends_with('.')))
        {
            return Err("Choose an unambiguous local program path.".into());
        }
    }
    let path =
        std::fs::canonicalize(value).map_err(|_| "That program could not be found.".to_string())?;
    #[cfg(windows)]
    if path
        .to_string_lossy()
        .to_ascii_uppercase()
        .starts_with(r"\\?\UNC\")
    {
        return Err("Choose a local executable program.".into());
    }
    if !path.is_file() {
        return Err("Choose a program, rather than a folder.".into());
    }
    #[cfg(windows)]
    if !path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("exe") || s.eq_ignore_ascii_case("com"))
    {
        return Err(
            "Choose an executable program. Run scripts through their approved interpreter.".into(),
        );
    }
    Ok(path)
}
pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}
fn same(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}
pub fn approved(program: &str, allow: &[String]) -> Result<PathBuf, String> {
    let actual = canonical_program(program)?;
    if !allow
        .iter()
        .filter_map(|s| canonical_program(s).ok())
        .any(|p| same(&actual, &p))
    {
        return Err("This program is not approved. Add it in Settings > Automations.".into());
    }
    Ok(actual)
}
pub fn spec(action: &Value) -> Result<Spec, String> {
    serde_json::from_value(action.clone()).map_err(|_| {
        "Check the program, arguments, working folder, environment and input fields.".into()
    })
}
pub fn validate(s: &Spec, allow: &[String]) -> Result<PathBuf, String> {
    if s.kind != "command" {
        return Err("Choose a command action.".into());
    }
    let program = approved(&s.program, allow)?;
    if !Path::new(&s.cwd).is_absolute() || !Path::new(&s.cwd).is_dir() {
        return Err("Choose an existing working folder using its full path.".into());
    }
    if s.args.len() > 128
        || s.args.iter().any(|a| a.len() > 8192 || a.contains('\0'))
        || s.args.iter().map(String::len).sum::<usize>() > 24000
    {
        return Err("Keep arguments within 128 items and 24,000 bytes.".into());
    }
    if !matches!(s.stdin.as_str(), "none" | "payload") {
        return Err("Choose no input or payload input.".into());
    }
    if s.env.len() > 64
        || s.env.iter().any(|(k, v)| {
            k.is_empty()
                || k.len() > 128
                || !k.chars().enumerate().all(|(i, c)| {
                    c.is_ascii_alphabetic() || c == '_' || (i > 0 && c.is_ascii_digit())
                })
                || v.contains('\0')
                || v.len() > 8192
                || k.eq_ignore_ascii_case("AUTOMATION_PAYLOAD")
                || k.to_ascii_uppercase().starts_with("CASTLE_")
        })
    {
        return Err(
            "Use valid environment names; Castle and payload variables are reserved.".into(),
        );
    }
    if s.env.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() > 24000 {
        return Err("Keep environment values under 24,000 bytes.".into());
    }
    // Shell interpreters are supported only with a literal script file. No inline code/elevation.
    let stem = program
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if matches!(stem.as_str(), "cmd" | "command" | "sudo" | "doas" | "runas") {
        return Err(
            "This launcher is not supported. Choose a native program or a script interpreter."
                .into(),
        );
    }
    if matches!(stem.as_str(), "powershell" | "pwsh") {
        let allowed = ["-noprofile", "-noninteractive", "-nologo"];
        let file=s.args.iter().position(|a|a.eq_ignore_ascii_case("-file")).ok_or("Use -File followed by an existing script; inline shell commands are not supported.")?;
        if s.args[..file]
            .iter()
            .any(|a| !allowed.contains(&a.to_ascii_lowercase().as_str()))
        {
            return Err("Only profile-free options may precede -File.".into());
        }
        script_file(s.args.get(file + 1), &s.cwd)?;
    } else if matches!(
        stem.as_str(),
        "bash"
            | "sh"
            | "zsh"
            | "fish"
            | "python"
            | "python3"
            | "pythonw"
            | "node"
            | "nodejs"
            | "ruby"
            | "perl"
    ) || stem.starts_with("python3")
    {
        script_file(s.args.first(), &s.cwd)?;
    }
    if s.env.keys().any(|k| {
        ["KEY", "TOKEN", "PASSWORD", "SECRET"]
            .iter()
            .any(|v| k.to_ascii_uppercase().contains(v))
    }) {
        return Err(
            "Keep secrets outside job definitions; use your script's protected configuration."
                .into(),
        );
    }
    Ok(program)
}
fn script_file(value: Option<&String>, cwd: &str) -> Result<(), String> {
    let file = value
        .filter(|s| !s.starts_with('-'))
        .ok_or("Choose a script file as the interpreter's first argument.")?;
    let path = Path::new(file);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(cwd).join(path)
    };
    if !full.is_file() {
        return Err("The script file could not be found.".into());
    }
    Ok(())
}
pub fn run(
    s: &Spec,
    allow: &[String],
    payload: &Value,
    limit: Duration,
) -> Result<ProcessOutput, String> {
    let program = validate(s, allow)?;
    let mut cmd = Command::new(program);
    cmd.args(&s.args)
        .current_dir(&s.cwd)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATH",
        "PATHEXT",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOME",
        "LOCALAPPDATA",
        "APPDATA",
        "LANG",
        "LC_ALL",
    ] {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    cmd.envs(&s.env)
        .env("AUTOMATION_PAYLOAD", payload.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if payload.to_string().len() > 65536 {
        return Err("The job payload is larger than 64 KiB.".into());
    }
    let input = if s.stdin == "payload" {
        payload.to_string()
    } else {
        String::new()
    };
    if input.len() > 65536 {
        return Err("The job input is larger than 64 KiB.".into());
    }
    actions::capture(cmd, &input, limit)
}
pub fn format(result: &ProcessOutput) -> String {
    format!(
        "Exit code: {}\n{}\n\nStandard error:\n{}",
        result
            .exit_code
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unavailable".into()),
        result.stdout,
        result.stderr
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        folder: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let folder = std::env::temp_dir().join(format!(
                "castle-command-test-{}-{}",
                std::process::id(),
                COUNT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&folder).unwrap();
            Self { folder }
        }
        fn file(&self, name: &str, text: &str) -> String {
            let p = self.folder.join(name);
            std::fs::write(&p, text).unwrap();
            p.to_string_lossy().into_owned()
        }
        fn shell(&self, text: &str) -> Spec {
            let script = self.file("fixture script.ps1", text);
            Spec {
                kind: "command".into(),
                // platform: windows-only - the module is cfg(all(test, windows))
                program: Path::new(&std::env::var("SystemRoot").unwrap())
                    .join("System32/WindowsPowerShell/v1.0/powershell.exe")
                    .to_string_lossy()
                    .into_owned(),
                args: vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-File".into(),
                    script,
                ],
                cwd: self.folder.to_string_lossy().into_owned(),
                env: BTreeMap::new(),
                stdin: "none".into(),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for name in ["fixture script.ps1", "allowed.exe", "blocked.exe"] {
                let _ = std::fs::remove_file(self.folder.join(name));
            }
            let _ = std::fs::remove_dir(self.folder.join("bin"));
            let _ = std::fs::remove_dir(&self.folder);
        }
    }
    fn allow(s: &Spec) -> Vec<String> {
        vec![s.program.clone()]
    }
    #[test]
    fn canonical_aliases_cannot_select_another_program() {
        let f = Fixture::new();
        let a = f.file("allowed.exe", "");
        let b = f.file("blocked.exe", "");
        std::fs::create_dir(f.folder.join("bin")).unwrap();
        let list = vec![a.clone()];
        assert!(approved(&a, &list).is_ok());
        assert!(approved(&a.to_ascii_uppercase(), &list).is_ok());
        assert!(approved(
            &f.folder.join("bin/../allowed.exe").to_string_lossy(),
            &list
        )
        .is_ok());
        assert!(approved(
            &f.folder.join("bin/../blocked.exe").to_string_lossy(),
            &list
        )
        .is_err());
        assert!(approved(&b, &list).is_err());
        for bad in [format!("{a}."), format!("{a} "), "allowed.exe".into()] {
            assert!(approved(&bad, &list).is_err());
        }
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
        }
        let short = |path: &str| {
            let input: Vec<u16> = std::ffi::OsStr::new(path)
                .encode_wide()
                .chain(Some(0))
                .collect();
            let mut out = vec![0u16; 32768];
            let n =
                unsafe { GetShortPathNameW(input.as_ptr(), out.as_mut_ptr(), out.len() as u32) };
            if n > 0 && n < (out.len() as u32) {
                String::from_utf16_lossy(&out[..n as usize])
            } else {
                path.into()
            }
        };
        assert!(approved(&short(&a), &list).is_ok());
        assert!(approved(&short(&b), &list).is_err());
    }
    #[test]
    fn revoked_or_missing_program_is_refused_at_execution() {
        let f = Fixture::new();
        let s = f.shell("Write-Output 'not run'");
        assert!(validate(&s, &allow(&s)).is_ok());
        assert!(run(&s, &[], &serde_json::json!({}), Duration::from_secs(5)).is_err());
    }
    #[test]
    fn inline_shell_and_missing_script_are_refused() {
        let f = Fixture::new();
        let mut s = f.shell("Write-Output 'ok'");
        s.args = vec!["-Command".into(), "Write-Output inline".into()];
        assert!(validate(&s, &allow(&s)).is_err());
        s.args = vec!["-EncodedCommand".into(), "AAAA".into()];
        assert!(validate(&s, &allow(&s)).is_err());
        s.args = vec!["-File".into(), "absent.ps1".into()];
        assert!(validate(&s, &allow(&s)).is_err());
    }
    #[test]
    fn payload_is_data_and_arguments_stay_literal() {
        let f = Fixture::new();
        let mut s=f.shell(r#"@{ arguments=@($args); payload=$env:AUTOMATION_PAYLOAD; input=[Console]::In.ReadToEnd(); mode=$env:REPORT_MODE } | ConvertTo-Json -Compress"#);
        s.args.extend(
            [
                "; calc",
                "a&calc.pdf",
                "{{payload.text}}",
                "$(Write-Output injected)",
            ]
            .map(str::to_string),
        );
        s.stdin = "payload".into();
        s.env.insert("REPORT_MODE".into(), "daily".into());
        let payload = serde_json::json!({"text":"; calc","file":"a&calc.pdf"});
        let original = s.args.clone();
        let result = run(&s, &allow(&s), &payload, Duration::from_secs(120)).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(s.args, original);
        let data: Value = serde_json::from_str(result.stdout.trim()).unwrap();
        assert_eq!(
            data["arguments"],
            serde_json::json!([
                "; calc",
                "a&calc.pdf",
                "{{payload.text}}",
                "$(Write-Output injected)"
            ])
        );
        assert_eq!(
            serde_json::from_str::<Value>(data["payload"].as_str().unwrap()).unwrap(),
            payload
        );
        assert_eq!(
            serde_json::from_str::<Value>(data["input"].as_str().unwrap()).unwrap(),
            payload
        );
        assert_eq!(data["mode"], "daily");
    }
    #[test]
    fn exit_codes_empty_success_and_failed_output_are_kept() {
        let f = Fixture::new();
        let s =
            f.shell("[Console]::Write('before failure');[Console]::Error.Write('problem');exit 7");
        let r = run(
            &s,
            &allow(&s),
            &serde_json::json!({}),
            Duration::from_secs(120),
        )
        .unwrap();
        assert_eq!(r.exit_code, Some(7));
        assert!(format(&r).contains("Exit code: 7"));
        assert_eq!(r.stdout, "before failure");
        assert_eq!(r.stderr, "problem");
        let s = f.shell("exit 0");
        let r = run(
            &s,
            &allow(&s),
            &serde_json::json!({}),
            Duration::from_secs(120),
        )
        .unwrap();
        assert_eq!(r.exit_code, Some(0));
        assert!(r.stdout.is_empty());
    }
    #[test]
    fn timeout_kills_descendant_and_keeps_partial_output() {
        // the release runner can take more than 20 s to start PowerShell while the suite runs in parallel;
        // the child's id must be printed before the limit stops the script
        let f = Fixture::new();
        let s=f.shell(r#"$p=Start-Process -WindowStyle Hidden -FilePath "$env:SystemRoot\System32\ping.exe" -ArgumentList '127.0.0.1','-n','600' -PassThru;[Console]::WriteLine($p.Id);Start-Sleep -Seconds 600"#);
        let r = run(
            &s,
            &allow(&s),
            &serde_json::json!({}),
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(r.timed_out);
        let pid: u32 = r.stdout.trim().parse().unwrap();
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        };
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | 0x00100000, 0, pid);
            if !process.is_null() {
                let wait = WaitForSingleObject(process, 3000);
                CloseHandle(process);
                assert_eq!(wait, 0, "fixture child survived timeout");
            }
        }
    }
    #[test]
    fn environment_cannot_override_payload_or_smuggle_secrets() {
        let f = Fixture::new();
        let mut s = f.shell("exit 0");
        for key in [
            "AUTOMATION_PAYLOAD",
            "automation_payload",
            "CASTLE_TOOLS",
            "API_TOKEN",
            "BAD=NAME",
        ] {
            s.env.clear();
            s.env.insert(key.into(), "x".into());
            assert!(validate(&s, &allow(&s)).is_err(), "{key}");
        }
    }
}

#[cfg(all(test, windows))]
mod hidden_tests {
    use super::*;
    #[test]
    fn commands_have_no_console_window() {
        let folder =
            std::env::temp_dir().join(format!("castle-hidden-command-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("hidden.ps1");
        std::fs::write(&file,r#"Add-Type -Namespace Fixture -Name Console -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();';[Console]::Write([Fixture.Console]::GetConsoleWindow().ToInt64())"#).unwrap();
        // platform: windows-only - the module is cfg(all(test, windows))
        let program = Path::new(&std::env::var("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe")
            .to_string_lossy()
            .to_string();
        let spec = Spec {
            kind: "command".into(),
            program: program.clone(),
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-File".into(),
                file.to_string_lossy().into_owned(),
            ],
            cwd: folder.to_string_lossy().into_owned(),
            env: BTreeMap::new(),
            stdin: "none".into(),
        };
        let result = run(
            &spec,
            &[program],
            &serde_json::json!({}),
            Duration::from_secs(120),
        );
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&folder);
        let result = result.unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
        assert_eq!(result.stdout.trim(), "0");
    }
}
