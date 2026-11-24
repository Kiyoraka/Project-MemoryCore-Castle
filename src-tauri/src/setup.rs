//! First run: finding or creating the member's MemoryCore folder, checking their CLI, filling in
//! MemoryCore's name placeholders, and writing Castle's block into CLAUDE.md and AGENTS.md so
//! Claude Code and Codex both load the memory by themselves.

use crate::env;
use crate::settings::Cli;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const MEMORYCORE_REPO: &str = "https://github.com/Kiyoraka/Poject-AI-Memory-Architecture";
const BLOCK_TEMPLATE: &str = include_str!("../resources/castle-block.md");
const DB_BLOCK_TEMPLATE: &str = include_str!("../resources/castle-block-db.md");
const BLOCK_BEGIN: &str = "<!-- castle:begin";
const BLOCK_END: &str = "<!-- castle:end -->";
const PLACEHOLDERS: [&str; 3] = ["[AI_NAME]", "[YOUR_NAME]", "[RELATIONSHIP_STYLE]"];

/// What a chosen folder is, for the setup screen.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderState {
    pub exists: bool,
    /// A MemoryCore folder: master-memory.md (or main/main-memory.md after Memory Consolidation).
    pub is_memorycore: bool,
    /// MemoryCore's placeholders are still there: the names have not been chosen yet.
    pub needs_names: bool,
    /// The file the companion reads first.
    pub entry_file: Option<String>,
    /// The names already in the memory (from MemoryCore's own wizard or an earlier setup).
    pub companion_name: Option<String>,
    pub member_name: Option<String>,
    pub relationship_style: Option<String>,
}

/// The value after `label` on the first line that carries it, cut at " - " (MemoryCore writes
/// "- **My Name**: Lyra - Chosen by you"). Placeholders still in brackets are not names.
fn value_after(text: &str, label: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains(label))?;
    let mut rest = line.split_once(label)?.1.trim();
    // a label that opens bold ("You are **") ends where the bold closes
    if label.ends_with("**") {
        rest = rest.split("**").next()?;
    }
    let value = rest.split(" - ").next()?.trim().trim_matches('*').trim();
    let value = value.strip_suffix(" AND learning companion").unwrap_or(value).trim();
    (!value.is_empty() && !value.starts_with('[') && value.chars().count() <= 60).then(|| value.to_string())
}

/// The names a memory already holds: identity-core.md (or main-memory.md after Memory
/// Consolidation) first, master-memory.md's "I am ..." line next, Castle's own block last.
pub fn names_in(folder: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    let identity = read(folder.join("main").join("identity-core.md")) + "
" + &read(folder.join("main").join("main-memory.md"));
    let master = read(folder.join("master-memory.md"));
    let block = read(folder.join("CLAUDE.md"));
    let companion = value_after(&identity, "**My Name**:")
        .or_else(|| value_after(&master, "**I am ").map(|v| v.trim_end_matches("**").to_string()))
        .or_else(|| value_after(&block, "You are **"));
    let member = value_after(&identity, "**Your Name**:").or_else(|| value_after(&block, "companion of **"));
    let style = value_after(&identity, "**My Role**:");
    (companion, member, style)
}

fn entry_file(folder: &Path) -> Option<&'static str> {
    if folder.join("main").join("main-memory.md").is_file() {
        Some("main/main-memory.md")
    } else if folder.join("master-memory.md").is_file() {
        Some("master-memory.md")
    } else {
        None
    }
}

/// The memory files that may hold MemoryCore's placeholders: master-memory.md and main/*.md.
fn memory_files(folder: &Path) -> Vec<PathBuf> {
    let mut files = vec![folder.join("master-memory.md")];
    if let Ok(entries) = std::fs::read_dir(folder.join("main")) {
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md")));
    }
    files.retain(|p| p.is_file());
    files
}

pub fn inspect(folder: &Path) -> FolderState {
    let entry = entry_file(folder);
    let needs_names = memory_files(folder)
        .iter()
        .any(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("[AI_NAME]")));
    // placeholders still in brackets are skipped by value_after, so this is safe on a fresh memory
    let (companion_name, member_name, relationship_style) = names_in(folder);
    FolderState {
        exists: folder.is_dir(),
        // a memory moved into the database has no master-memory.md on disk and is still a MemoryCore
        is_memorycore: entry.is_some() || lives_in_database(folder),
        needs_names,
        entry_file: entry.map(str::to_string),
        companion_name,
        member_name,
        relationship_style,
    }
}

/// Replaces MemoryCore's placeholders with the member's choices in every memory file.
/// Returns how many placeholders were filled.
pub fn fill_names(folder: &Path, companion: &str, member: &str, style: &str) -> Result<usize, String> {
    // "A friendly partner" on a button reads "designed to be a friendly partner" in a sentence
    let style = sentence_case(style.trim());
    let values = [companion.trim(), member.trim(), style.as_str()];
    if values[0].is_empty() || values[1].is_empty() {
        return Err("Both names are needed.".into());
    }
    let mut filled = 0;
    for file in memory_files(folder) {
        let text = std::fs::read_to_string(&file).map_err(|_| format!("{} could not be read.", file.display()))?;
        let mut changed = text.clone();
        for (placeholder, value) in PLACEHOLDERS.iter().zip(values) {
            if value.is_empty() {
                continue;
            }
            filled += changed.matches(placeholder).count();
            changed = changed.replace(placeholder, value);
        }
        if changed != text {
            std::fs::write(&file, changed).map_err(|_| format!("{} could not be written.", file.display()))?;
        }
    }
    Ok(filled)
}

/// "A calm mentor" -> "a calm mentor": a style is written in the middle of a sentence.
/// Words in capitals throughout ("AI", "NASA") are left alone.
pub fn sentence_case(style: &str) -> String {
    let mut chars = style.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if first.is_uppercase() && !second.is_uppercase() => first.to_lowercase().chain(style.chars().skip(1)).collect(),
        _ => style.to_string(),
    }
}

/// What the block says when the memory files live in the database: one command prints them.
#[cfg(windows)]
const BOOT_FROM_DATABASE: &str =
    "run `castle-memory boot` (in PowerShell: `& \"$env:CASTLE_TOOLS\\castle-memory.cmd\" boot`) and follow what it prints to load your memory";
/// macOS: `castle-memory` is on the session's PATH (Castle puts its tools first).
#[cfg(not(windows))]
const BOOT_FROM_DATABASE: &str = "run `castle-memory boot` and follow what it prints to load your memory";

/// What the block says when Castle itself loaded the memory at wake (it is in the session's
/// system prompt before the companion's first word).
const ALREADY_LOADED: &str =
    "your memory is already loaded - MemoryCore Castle read it from the memory database when it woke you, and it is in this session under \"Your memory\". Do not load it again. (Only if you find no memory in this session: run `castle-memory boot` and follow what it prints.)";

/// Castle's block for a session Castle woke with the memory already loaded.
pub fn castle_block_loaded(companion: &str, member: &str) -> String {
    BLOCK_TEMPLATE
        .replace("At the start of every session, before anything else, read `{{ENTRY_FILE}}` and follow it to load your memory.", &format!("At the start of this session {ALREADY_LOADED}"))
        .replace("{{AI_NAME}}", companion.trim())
        .replace("{{USER_NAME}}", member.trim())
}

/// Castle's block with the member's names in it. `entry` is the file the companion reads
/// first; None when the memory files have moved into the database.
pub fn castle_block_for(companion: &str, member: &str, entry: Option<&str>) -> String {
    match entry {
        Some(entry) => castle_block(companion, member, entry),
        None => BLOCK_TEMPLATE
            .replace("read `{{ENTRY_FILE}}` and follow it to load your memory", BOOT_FROM_DATABASE)
            .replace("{{AI_NAME}}", companion.trim())
            .replace("{{USER_NAME}}", member.trim()),
    }
}

/// Castle's block with the member's names in it.
pub fn castle_block(companion: &str, member: &str, entry: &str) -> String {
    BLOCK_TEMPLATE
        .replace("{{AI_NAME}}", companion.trim())
        .replace("{{USER_NAME}}", member.trim())
        .replace("{{ENTRY_FILE}}", entry)
}

/// `text` with Castle's block put in place: the old block replaced, or the block added at the
/// end. Everything the member wrote outside the markers is kept as it was.
pub fn with_block(text: &str, block: &str) -> String {
    let block = block.trim_end();
    if let (Some(start), Some(end)) = (text.find(BLOCK_BEGIN), text.find(BLOCK_END)) {
        if end > start {
            let after = end + BLOCK_END.len();
            return format!("{}{}{}", &text[..start], block, &text[after..]);
        }
    }
    if text.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{block}\n", text.trim_end())
    }
}

/// Castle's block plus the memory-database section, placed just before the closing marker.
pub fn with_db_section(block: &str, member: &str) -> String {
    let section = DB_BLOCK_TEMPLATE.replace("{{USER_NAME}}", member.trim());
    match block.rfind(BLOCK_END) {
        Some(end) => format!("{}\n{}\n{}", block[..end].trim_end(), section.trim_end(), &block[end..]),
        None => block.to_string(),
    }
}

/// True when the memory folder's .env names a memory database (Castle wrote it there).
pub fn has_database(folder: &Path) -> bool {
    std::fs::read_to_string(folder.join(".env")).is_ok_and(|text| crate::memdb::access_from_env(&text).is_some())
}

/// A memory folder whose memory is in a database: its .env names one, or - on a new PC, where
/// the .env never travels with the backup - the dump the backup carried is there to restore.
pub fn lives_in_database(folder: &Path) -> bool {
    has_database(folder) || folder.join(crate::backup::DUMP_FILE).is_file()
}

/// The instructions files the CLIs read in a folder: CLAUDE.md (Claude Code) and AGENTS.md (Codex
/// and Grok). Castle writes its block and its rooms' rules into each.
pub const INSTRUCTION_FILES: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];

/// Writes Castle's block into each of INSTRUCTION_FILES.
pub fn write_blocks(folder: &Path, companion: &str, member: &str) -> Result<(), String> {
    write_blocks_for(folder, companion, member, false)
}

/// Writes Castle's block. `loaded` = Castle is waking the companion with the memory already in
/// the session, so the block says so instead of telling her to load it.
pub fn write_blocks_for(folder: &Path, companion: &str, member: &str, loaded: bool) -> Result<(), String> {
    let block = block_text(folder, companion, member, loaded)?;
    for name in INSTRUCTION_FILES {
        let path = folder.join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let next = with_block(&text, &block);
        if next != text {
            std::fs::write(&path, next).map_err(|_| format!("{name} could not be written."))?;
        }
    }
    Ok(())
}

/// Castle's whole block for this folder, as text: the opening that fits (read the entry file,
/// boot from the database, or already loaded) and the database section when there is one.
pub fn block_text(folder: &Path, companion: &str, member: &str, loaded: bool) -> Result<String, String> {
    let entry = entry_file(folder);
    if entry.is_none() && !lives_in_database(folder) {
        return Err("This folder has no master-memory.md, so it is not a MemoryCore folder.".into());
    }
    let mut block = if loaded { castle_block_loaded(companion, member) } else { castle_block_for(companion, member, entry) };
    if has_database(folder) {
        block = with_db_section(&block, member);
    }
    Ok(block)
}

/// Runs a program with no console window and a time limit; its first output line.
fn first_line(program: &Path, args: &[&str], limit: Duration) -> Option<String> {
    let is_script = program.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    // platform: windows-only - a .cmd / .bat launcher exists only on Windows
    let mut command = if is_script {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(program);
        c
    } else {
        Command::new(program)
    };
    command.args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    let mut child = command.spawn().ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&output.stdout).lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// Whether the member's CLI is installed and signed in, with the words to fix it when not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCheck {
    pub cli: Cli,
    pub label: &'static str,
    pub installed: bool,
    pub version: Option<String>,
    /// None when Castle cannot tell.
    pub signed_in: Option<bool>,
    pub install_command: &'static str,
    pub sign_in_hint: &'static str,
}

/// macOS: Claude Code keeps its sign-in in the Keychain, not in a file. Only whether the entry
/// exists is asked (no password is read, so macOS shows no prompt).
fn in_keychain(cli: Cli) -> bool {
    #[cfg(target_os = "macos")]
    match cli {
        Cli::Claude => {
            return Command::new("security")
                .args(["find-generic-password", "-s", "Claude Code-credentials"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        }
        Cli::Codex | Cli::Gemini | Cli::Grok => {}
    }
    let _ = cli;
    false
}

/// The file each CLI keeps once the member has signed in.
fn credentials_file(cli: Cli) -> Option<PathBuf> {
    let home = crate::env::home_dir()?;
    Some(match cli {
        Cli::Claude => home.join(".claude").join(".credentials.json"),
        Cli::Codex => std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".codex")).join("auth.json"),
        // their sign-ins are read in Task 8 (Grok's file, agy's status command): until then unknown
        Cli::Gemini | Cli::Grok => return None,
    })
}

pub fn check_cli(cli: Cli) -> CliCheck {
    let program = env::find_on_path(cli.command());
    let version = program.as_deref().and_then(|p| first_line(p, &["--version"], Duration::from_secs(15)));
    let (install_command, sign_in_hint) = match cli {
        Cli::Claude => ("npm install -g @anthropic-ai/claude-code", "Open the Log, type /login and follow the browser."),
        Cli::Codex => ("npm install -g @openai/codex", "Run `codex login` once (the Log can do it) and sign in with your ChatGPT account."),
        Cli::Gemini => ("Install the Antigravity CLI from antigravity.google/docs/install (the CLI tab).", "Run `agy` once and sign in with your Google account."),
        Cli::Grok if cfg!(windows) => ("irm https://x.ai/cli/install.ps1 | iex", "Run `grok login` once and sign in in the browser."),
        Cli::Grok => ("curl -fsSL https://x.ai/cli/install.sh | bash", "Run `grok login` once and sign in in the browser."),
    };
    CliCheck {
        cli,
        label: cli.label(),
        installed: program.is_some(),
        version,
        signed_in: credentials_file(cli).map(|f| f.is_file() || in_keychain(cli)),
        install_command,
        sign_in_hint,
    }
}

/// Creates a fresh MemoryCore folder at `target` from the public MemoryCore repository.
/// Uses git when it is installed; otherwise downloads the repository's zip and unpacks it.
pub fn create(target: &Path) -> Result<FolderState, String> {
    if target.exists() && std::fs::read_dir(target).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err("That folder already has files in it. Choose an empty or new folder.".into());
    }
    if let Some(git) = env::find_on_path("git") {
        let status = Command::new(git)
            .args(["clone", "--depth", "1", MEMORYCORE_REPO])
            .arg(target)
            .status()
            .map_err(|_| "git could not be started.".to_string())?;
        if status.success() {
            // A fresh memory belongs to the member: it starts its own history, not MemoryCore's.
            let _ = std::fs::remove_dir_all(target.join(".git"));
        } else if cfg!(windows) {
            return Err("MemoryCore could not be downloaded with git. Check the internet connection and try again.".into());
        } else {
            // macOS: /usr/bin/git is only a stand-in until Apple's developer tools are installed;
            // the zip download needs none of them
            let _ = std::fs::remove_dir_all(target);
            download_zip(target)?;
        }
    } else {
        download_zip(target)?;
    }
    let state = inspect(target);
    if !state.is_memorycore {
        return Err("The download finished but no master-memory.md was found in it.".into());
    }
    Ok(state)
}

fn download_zip(target: &Path) -> Result<(), String> {
    let url = format!("{MEMORYCORE_REPO}/archive/refs/heads/main.zip");
    let bytes = reqwest::blocking::Client::builder()
        .user_agent(concat!("MemoryCore-Castle/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()
        .and_then(|c| c.get(&url).send())
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.bytes())
        .map_err(|_| "MemoryCore could not be downloaded. Check the internet connection and try again.".to_string())?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "The download was not a readable zip.".to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|_| "The zip could not be read.".to_string())?;
        let Some(inside) = file.enclosed_name() else { continue };
        // the zip holds one top folder (<repo>-main/); its contents become the memory folder
        let relative: PathBuf = inside.components().skip(1).collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let out = target.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|_| "A folder could not be created.".to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|_| "A folder could not be created.".to_string())?;
            }
            let mut writer = std::fs::File::create(&out).map_err(|_| "A file could not be written.".to_string())?;
            std::io::copy(&mut file, &mut writer).map_err(|_| "A file could not be written.".to_string())?;
        }
    }
    Ok(())
}

/// The keys MemoryCore's own media scripts read from a .env, and Castle's account for each.
pub const ENV_KEYS: [(&str, &str); 4] = [
    ("OPENAI_API_KEY", "openai"),
    ("ARK_API_KEY", "seedance"),
    ("SUNOAPI_ORG_KEY", "suno"),
    ("ELEVENLABS_API_KEY", "elevenlabs"),
];

/// A key found in the member's .env, shown masked; the value stays in Rust.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundKey {
    pub name: String,
    pub variable: String,
    pub masked: String,
}

/// The real key values in a .env text: quotes trimmed, comments, empty values and
/// placeholders (`<your-key>`, `your_key_here`) left out.
pub fn env_values(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else { continue };
        let Some((_, account)) = ENV_KEYS.iter().find(|(v, _)| *v == name.trim()) else { continue };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        let lower = value.to_lowercase();
        if value.len() < 8 || value.starts_with('<') || lower.contains("your") || lower.contains("placeholder") {
            continue;
        }
        found.push((account.to_string(), value.to_string()));
    }
    found
}

fn env_file_keys(folder: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(folder.join(".env")).map(|t| env_values(&t)).unwrap_or_default()
}

/// Keys the member already keeps in the memory folder's .env, for the Keys step's offer.
pub fn found_keys(folder: &Path) -> Vec<FoundKey> {
    env_file_keys(folder)
        .into_iter()
        .map(|(name, value)| FoundKey {
            variable: ENV_KEYS.iter().find(|(_, a)| *a == name).map(|(v, _)| v.to_string()).unwrap_or_default(),
            masked: crate::key::mask(&value),
            name,
        })
        .collect()
}

#[tauri::command]
pub fn setup_found_keys(folder: String) -> Vec<FoundKey> {
    found_keys(Path::new(&folder))
}

/// Keeps the chosen .env keys encrypted in Castle. The .env itself is left exactly as it was.
#[tauri::command]
pub fn setup_import_keys(folder: String, names: Vec<String>) -> Result<Vec<crate::key::KeyStatus>, String> {
    for (name, value) in env_file_keys(Path::new(&folder)) {
        if names.contains(&name) {
            crate::key::save(&name, &value)?;
        }
    }
    Ok(crate::key::statuses())
}

// ---------- Commands for the setup screens ----------

#[tauri::command]
pub fn setup_inspect(folder: String) -> FolderState {
    inspect(Path::new(&folder))
}

/// Downloads a fresh MemoryCore into `folder` (new or empty).
#[tauri::command]
pub async fn setup_create(folder: String) -> Result<FolderState, String> {
    tauri::async_runtime::spawn_blocking(move || create(Path::new(&folder)))
        .await
        .map_err(|_| "The download stopped unexpectedly.".to_string())?
}

#[tauri::command]
pub async fn setup_check_cli(cli: Cli) -> Result<CliCheck, String> {
    tauri::async_runtime::spawn_blocking(move || check_cli(cli))
        .await
        .map_err(|_| "The check stopped unexpectedly.".to_string())
}

/// Writes the names into the memory (when its placeholders are still there) and Castle's block
/// into CLAUDE.md and AGENTS.md.
#[tauri::command]
pub fn setup_apply(folder: String, companion: String, member: String, style: String) -> Result<FolderState, String> {
    let path = Path::new(&folder);
    if inspect(path).needs_names {
        fill_names(path, &companion, &member, &style)?;
    }
    write_blocks(path, &companion, &member)?;
    Ok(inspect(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("castle-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("main")).unwrap();
        std::fs::write(root.join("master-memory.md"), "**I am [AI_NAME]** - type \"[AI_NAME]\"\n").unwrap();
        std::fs::write(root.join("main").join("identity-core.md"), "# [AI_NAME]\nbe [RELATIONSHIP_STYLE] for [YOUR_NAME]\n").unwrap();
        std::fs::write(root.join("main").join("notes.txt"), "[AI_NAME] stays in non-md files").unwrap();
        root
    }

    #[test]
    fn a_fresh_memorycore_needs_names_and_filling_them_clears_the_need() {
        let root = memory("fill");
        let state = inspect(&root);
        assert!(state.exists && state.is_memorycore && state.needs_names);
        assert_eq!(state.entry_file.as_deref(), Some("master-memory.md"));

        assert_eq!(fill_names(&root, "Lyra", "Rin", "a friendly partner").unwrap(), 5);
        assert!(!inspect(&root).needs_names);
        let identity = std::fs::read_to_string(root.join("main").join("identity-core.md")).unwrap();
        assert_eq!(identity, "# Lyra\nbe a friendly partner for Rin\n");
        assert!(std::fs::read_to_string(root.join("main").join("notes.txt")).unwrap().contains("[AI_NAME]"));
        assert!(fill_names(&root, " ", "Rin", "").is_err());
        assert_eq!(sentence_case("A friendly partner"), "a friendly partner");
        assert_eq!(sentence_case("AI study buddy"), "AI study buddy");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_block_is_added_once_and_replaced_without_touching_the_members_words() {
        let block_v1 = castle_block("Lyra", "Rin", "master-memory.md");
        assert!(block_v1.contains("You are **Lyra**") && block_v1.contains("read `master-memory.md`"));
        let mine = "# My notes\n\nKeep answers short.\n";
        let once = with_block(mine, &block_v1);
        assert!(once.starts_with(mine.trim_end()));
        assert_eq!(once.matches(BLOCK_BEGIN).count(), 1);

        let with_tail = format!("{once}\nMore of my own words.\n");
        let block_v2 = castle_block("Nova", "Rin", "main/main-memory.md");
        let twice = with_block(&with_tail, &block_v2);
        assert_eq!(twice.matches(BLOCK_BEGIN).count(), 1);
        assert!(twice.contains("You are **Nova**") && !twice.contains("You are **Lyra**"));
        assert!(twice.contains("Keep answers short.") && twice.contains("More of my own words."));
        assert_eq!(with_block("", &block_v1), format!("{}\n", block_v1.trim_end()));
    }

    #[test]
    fn blocks_go_to_both_cli_files_and_the_name_reads_back() {
        let root = memory("blocks");
        write_blocks(&root, "Lyra", "Rin").unwrap();
        for name in ["CLAUDE.md", "AGENTS.md"] {
            assert!(std::fs::read_to_string(root.join(name)).unwrap().contains("You are **Lyra**"));
        }
        assert_eq!(inspect(&root).companion_name.as_deref(), Some("Lyra"));
        // after Memory Consolidation the entry file moves
        std::fs::write(root.join("main").join("main-memory.md"), "x").unwrap();
        assert_eq!(inspect(&root).entry_file.as_deref(), Some("main/main-memory.md"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_memory_with_a_database_is_told_to_use_the_door() {
        let root = memory("blocks-db");
        write_blocks(&root, "Lyra", "Rin").unwrap();
        let plain = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(!plain.contains("castle-memory"));
        // every member's companion is told to write the diary like a person, database or not
        assert!(plain.contains("the way a person writes their own diary") && plain.contains("Never a report"));
        std::fs::write(root.join(".env"), "MEMORY_DB_HOST=127.0.0.1\nMEMORY_DB_PORT=3307\nMEMORY_DB_NAME=lyra_memorycoredb\nMEMORY_DB_USER=castle_lyra\nMEMORY_DB_PASSWORD=x\n").unwrap();
        assert!(has_database(&root));
        write_blocks(&root, "Lyra", "Rin").unwrap();
        for name in ["CLAUDE.md", "AGENTS.md"] {
            let text = std::fs::read_to_string(root.join(name)).unwrap();
            assert!(text.contains("### The memory database") && text.contains("castle-memory diary add"));
            assert_eq!(text.matches("the way a person writes their own diary").count(), 2);
            assert!(text.contains("The diary never needs archiving") && text.contains("castle-memory diary rewrite"));
            assert!(text.contains("on Rin's own computer") && !text.contains("{{"));
            // the section sits inside the block, once, and the block still closes
            assert_eq!(text.matches("### The memory database").count(), 1);
            assert!(text.trim_end().ends_with(BLOCK_END));
            assert!(text.find("### The memory database").unwrap() < text.find(BLOCK_END).unwrap());
        }
        // the memory files moved into the database: the block boots through the door, and the
        // folder is still a MemoryCore
        std::fs::remove_file(root.join("master-memory.md")).unwrap();
        let _ = std::fs::remove_dir_all(root.join("main"));
        assert!(inspect(&root).is_memorycore && inspect(&root).entry_file.is_none());
        write_blocks(&root, "Lyra", "Rin").unwrap();
        let booted = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(booted.contains("run `castle-memory boot`") && !booted.contains("{{ENTRY_FILE}}") && !booted.contains("read `master-memory.md`"));
        assert_eq!(inspect(&root).companion_name.as_deref(), Some("Lyra"));
        // woken by Castle with the memory already loaded: the block says so and names the fallback
        write_blocks_for(&root, "Lyra", "Rin", true).unwrap();
        let loaded = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(loaded.contains("your memory is already loaded") && loaded.contains("Do not load it again."));
        assert!(loaded.contains("Then greet Rin by name.") && !loaded.contains("{{") && !loaded.contains("before anything else, read"));
        assert!(loaded.contains("### The memory database"));
        assert_eq!(inspect(&root).companion_name.as_deref(), Some("Lyra"));
        std::fs::write(root.join("master-memory.md"), "**I am Lyra**\n").unwrap();
        write_blocks(&root, "Lyra", "Rin").unwrap();
        // the database gone again (the .env line removed): the section leaves with it
        std::fs::write(root.join(".env"), "OPENAI_API_KEY=sk-1\n").unwrap();
        write_blocks(&root, "Lyra", "Rin").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(), plain);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn names_written_by_memorycores_own_wizard_are_read_back() {
        let root = memory("names");
        std::fs::write(root.join("master-memory.md"), "**I am Nova** - Your personal AI companion
").unwrap();
        std::fs::write(
            root.join("main").join("identity-core.md"),
            "# Identity Core - Nova
- **My Name**: Nova - Chosen by you, carried with pride
- **My Role**: a calm mentor AND learning companion
- **Your Name**: Aiman - My human partner and focus
",
        )
        .unwrap();
        let state = inspect(&root);
        assert!(!state.needs_names);
        assert_eq!(state.companion_name.as_deref(), Some("Nova"));
        assert_eq!(state.member_name.as_deref(), Some("Aiman"));
        assert_eq!(state.relationship_style.as_deref(), Some("a calm mentor"));
        // only master-memory.md filled in: the "I am" line still names the companion
        std::fs::remove_file(root.join("main").join("identity-core.md")).unwrap();
        assert_eq!(inspect(&root).companion_name.as_deref(), Some("Nova"));
        let _ = std::fs::remove_dir_all(root);
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
    }

    #[test]
    fn the_fresh_and_the_existing_memory_are_told_apart() {
        let fresh = inspect(&fixture("memory-fresh"));
        assert!(fresh.is_memorycore && fresh.needs_names);
        assert_eq!((fresh.companion_name, fresh.member_name), (None, None));
        let existing = inspect(&fixture("memory-existing"));
        assert!(existing.is_memorycore && !existing.needs_names);
        assert_eq!(existing.companion_name.as_deref(), Some("Nova"));
        assert_eq!(existing.member_name.as_deref(), Some("Aiman"));
        assert_eq!(existing.relationship_style.as_deref(), Some("a calm mentor"));
    }

    #[test]
    fn keys_in_a_members_env_are_found_and_placeholders_are_not() {
        let found = found_keys(&fixture("memory-existing"));
        let names: Vec<_> = found.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["openai", "seedance"]);
        assert_eq!(found[1].variable, "ARK_API_KEY");
        assert!(!found[0].masked.contains("not-a-real"), "the page only sees the ends");
        assert!(found_keys(&fixture("memory-fresh")).is_empty());
        assert_eq!(env_values("# OPENAI_API_KEY=sk-commented-out-0000
SUNOAPI_ORG_KEY='suno-real-ish-123'
"), vec![("suno".to_string(), "suno-real-ish-123".to_string())]);
    }

    #[test]
    fn a_folder_with_files_is_never_cloned_into() {
        let root = memory("busy");
        assert!(create(&root).unwrap_err().contains("already has files"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_cli_says_how_to_install_it() {
        let check = check_cli(Cli::Codex);
        assert!(check.install_command.contains("@openai/codex"));
        assert_eq!(check.label, "Codex");
    }
}
