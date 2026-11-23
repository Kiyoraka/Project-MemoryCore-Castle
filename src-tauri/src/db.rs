//! The member's own database engine: a portable MariaDB that Castle downloads once (with the
//! member's OK), keeps in its own app-data folder and starts and stops like XAMPP's buttons.
//! No admin prompt, no Windows service, no window ever shown.
//!
//! ```text
//!  %LOCALAPPDATA%\MemoryCore Castle\mariadb\
//!    11.4.13\bin\mariadbd.exe ...   the official zip, unpacked (its top folder dropped)
//!    data\                          the databases (made once by mariadb-install-db)
//!    my.ini                         Castle's settings: port, 127.0.0.1 only, utf8mb4
//! ```
//!
//! The port rule: 3306 when it is free. When a MySQL / MariaDB already answers there (XAMPP),
//! the member may use that one instead. Otherwise Castle's own engine takes the first free port
//! from 3307 and keeps it for good (saved in settings), so the companion's connection never moves.

use crate::settings::SettingsStore;
use serde::Serialize;
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// The MariaDB release Castle installs: 11.4 is the long-term-support line.
pub const VERSION: &str = "11.4.13";
/// macOS: Homebrew's formula for the same long-term line, so both platforms run one MariaDB
/// (newer lines reserve words the schema uses, such as TO_DATE).
pub const BREW_FORMULA: &str = "mariadb@11.4";
pub const ZIP_URL: &str = "https://archive.mariadb.org/mariadb-11.4.13/winx64-packages/mariadb-11.4.13-winx64.zip";
/// From the release's own sha256sums.txt; a download that differs is thrown away.
pub const ZIP_SHA256: &str = "d62986d433eeebfde218560b276103831604a61e929e87f1a17f5aebd80257e2";
/// For the Install button's "about 95 MB".
pub const ZIP_BYTES: u64 = 95_024_126;
pub const PREFERRED_PORT: u16 = 3306;
/// Castle's own engine looks for a free port from here when 3306 is taken.
pub const FIRST_OWN_PORT: u16 = 3307;
/// The event the page hears while the engine downloads: { done, total }.
pub const INSTALL_EVENT: &str = "castle://db-install";
/// Tests point the engine folder somewhere else.
pub const HOME_VAR: &str = "CASTLE_DB_HOME";

const START_LIMIT: Duration = Duration::from_secs(20);

/// What the database engine is right now, for Settings and the HOME card.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Engine {
    /// Nothing installed yet; `external` is a MySQL already answering on 3306 (XAMPP) if any.
    NotInstalled { external: Option<u16> },
    /// Castle's engine is installed but stopped.
    Stopped { port: u16 },
    /// Castle's engine is running.
    Running { port: u16 },
    /// The member chose a MySQL / MariaDB that was already on this PC.
    External { port: u16, answering: bool },
}

/// Who holds a port.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortUse {
    Free,
    /// A MySQL or MariaDB server greets there.
    MySql,
    /// Something else is listening.
    Other,
}

// ---------- pure parts (tested) ----------

/// A MySQL / MariaDB server speaks first: a packet whose fifth byte is the protocol version 10
/// (or an error packet, 0xff, when it refuses this host - still a MySQL).
pub fn is_mysql_greeting(bytes: &[u8]) -> bool {
    bytes.len() >= 5 && (bytes[4] == 10 || bytes[4] == 0xff)
}

/// The port Castle's own engine takes: 3306 when free, else the first free one from 3307.
/// None when nothing in 3307..=3399 is free.
pub fn choose_port(probe: impl Fn(u16) -> PortUse) -> Option<u16> {
    if probe(PREFERRED_PORT) == PortUse::Free {
        return Some(PREFERRED_PORT);
    }
    (FIRST_OWN_PORT..=3399).find(|p| probe(*p) == PortUse::Free)
}

/// Castle's my.ini: only this PC may connect, and text is stored as full Unicode.
pub fn my_ini(data: &Path, port: u16) -> String {
    let socket = if cfg!(windows) { String::new() } else { format!("socket={}\n", data.with_file_name("mysql.sock").display()) };
    let data = data.display().to_string().replace('\\', "/");
    format!(
        "# Written by MemoryCore Castle - Castle rewrites this file.\n\
         [mysqld]\n\
         datadir={data}\n\
         port={port}\n\
         bind-address=127.0.0.1\n\
         {socket}\
         character-set-server=utf8mb4\n\
         collation-server=utf8mb4_unicode_ci\n\
         \n\
         [client]\n\
         port={port}\n\
         default-character-set=utf8mb4\n"
    )
}

/// The lowercase hex SHA-256 of some bytes (kept here for the memory tables that fingerprint rows).
pub use crate::fetch::sha256_hex;

/// A download is kept only when it is exactly the release's own file.
pub fn checksum_ok(bytes: &[u8]) -> bool {
    crate::fetch::matches(bytes, ZIP_SHA256)
}

/// One line for a port Castle cannot use, saying what to do about it.
pub fn port_busy_line(port: u16) -> String {
    #[cfg(not(windows))]
    return format!(
        "Port {port} is being used by another program, so the database cannot start. \
         See which one with `lsof -nP -iTCP:{port} -sTCP:LISTEN` and close it (or end it with `kill <PID>`), then press Start again."
    );
    #[cfg(windows)]
    format!(
        "Port {port} is being used by another program, so the database cannot start. \
         See which one with `netstat -ano | findstr :{port}` and close it (or end it with `taskkill /PID <number>`), then press Start again."
    )
}

/// What Castle keeps from the release zip: everything a running server and its tools need.
/// Debug symbols, link libraries and C headers (about 150 MB) are for developers of MariaDB.
pub fn keep_in_unpack(relative: &Path) -> bool {
    let first = relative.components().next().map(|c| c.as_os_str().to_string_lossy().to_lowercase());
    if first.as_deref() == Some("include") {
        return false;
    }
    !matches!(relative.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("pdb") | Some("lib"))
}

// ---------- where things live ----------

/// The engine's own folder.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os(HOME_VAR).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "macos")]
    if let Some(user) = std::env::var_os("HOME") {
        return PathBuf::from(user).join("Library/Application Support/MemoryCore Castle/mariadb");
    }
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    local.join("MemoryCore Castle").join("mariadb")
}

/// One of the engine's programs (`mariadbd`, `mariadb-install-db`, `mariadb-admin`). Windows: the
/// unpacked release's `.exe`. macOS: Homebrew's MariaDB, the way MariaDB itself ships it for Macs;
/// Castle still keeps its own data folder and my.ini in `home`.
pub fn program(home: &Path, name: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let _ = home;
        let opt = brew().map(|b| b.parent().and_then(Path::parent).map(|p| p.join("opt").join(BREW_FORMULA)).unwrap_or_default());
        let found = opt
            .into_iter()
            .flat_map(|o| [o.join("bin").join(name), o.join("sbin").join(name)])
            .find(|p| p.is_file());
        found.unwrap_or_else(|| PathBuf::from(name))
    }
    #[cfg(not(target_os = "macos"))]
    bin_dir(home).join(format!("{name}.exe"))
}

/// macOS: Homebrew's `brew`, on PATH or in its usual places (Apple silicon, then Intel).
#[cfg(target_os = "macos")]
pub fn brew() -> Option<PathBuf> {
    crate::env::find_on_path("brew").or_else(|| {
        ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().map(PathBuf::from).find(|p| p.is_file())
    })
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn bin_dir(home: &Path) -> PathBuf {
    home.join(VERSION).join("bin")
}

pub fn data_dir(home: &Path) -> PathBuf {
    home.join("data")
}

pub fn ini_path(home: &Path) -> PathBuf {
    home.join("my.ini")
}

/// Installed means the server program and its data folder are both there.
pub fn installed(home: &Path) -> bool {
    program(home, "mariadbd").is_file() && data_dir(home).join("mysql").is_dir()
}

// ---------- the port ----------

/// Who holds 127.0.0.1:<port>: free, a MySQL that greets, or something else.
pub fn probe_port(port: u16) -> PortUse {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return PortUse::Free;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let mut buf = [0u8; 16];
    match stream.read(&mut buf) {
        Ok(n) if is_mysql_greeting(&buf[..n]) => PortUse::MySql,
        _ => PortUse::Other,
    }
}

/// The engine's state, from the settings Castle keeps and what answers on the port.
pub fn detect(home: &Path, port: Option<u16>, external: bool) -> Engine {
    if external {
        let port = port.unwrap_or(PREFERRED_PORT);
        return Engine::External { port, answering: probe_port(port) == PortUse::MySql };
    }
    if !installed(home) {
        let external = (probe_port(PREFERRED_PORT) == PortUse::MySql).then_some(PREFERRED_PORT);
        return Engine::NotInstalled { external };
    }
    let port = port.unwrap_or(PREFERRED_PORT);
    if probe_port(port) == PortUse::MySql {
        Engine::Running { port }
    } else {
        Engine::Stopped { port }
    }
}

// ---------- programs ----------

fn hidden(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    command
}

/// Runs a program to the end, at most `limit`; true when it finished well.
fn run_to_end(mut command: Command, limit: Duration) -> bool {
    let Ok(mut child) = command.spawn() else { return false };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

// ---------- install ----------

/// Downloads the release (reporting progress), checks it, unpacks it into the engine folder,
/// makes the data folder and writes my.ini. Returns the port the engine will use.
pub fn install(home: &Path, port: u16, progress: impl Fn(u64, u64)) -> Result<u16, String> {
    #[cfg(target_os = "macos")]
    {
        brew_install(progress)?;
        initialise(home, port)?;
        return Ok(port);
    }
    #[cfg(not(target_os = "macos"))]
    install_release(home, port, progress)
}

/// macOS: `brew install mariadb@11.4` (Homebrew needs no admin prompt), unless it is there already.
#[cfg(target_os = "macos")]
fn brew_install(progress: impl Fn(u64, u64)) -> Result<(), String> {
    if program(Path::new(""), "mariadbd").is_file() {
        return Ok(());
    }
    let brew = brew().ok_or("Castle installs the database with Homebrew on a Mac. Install Homebrew from brew.sh, then press Install again.")?;
    progress(0, 1);
    let mut command = hidden(&brew);
    command.args(["install", BREW_FORMULA]).env("HOMEBREW_NO_AUTO_UPDATE", "1");
    if !run_to_end(command, Duration::from_secs(900)) || !program(Path::new(""), "mariadbd").is_file() {
        return Err("Homebrew could not install MariaDB. Run `brew install mariadb@11.4` in Terminal to see why, then press Check again.".into());
    }
    progress(1, 1);
    Ok(())
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn install_release(home: &Path, port: u16, progress: impl Fn(u64, u64)) -> Result<u16, String> {
    // kept on disk while it comes, so a dropped connection or a closed Castle carries on from there
    let part = home.join(format!("mariadb-{VERSION}-winx64.zip.part"));
    let bytes = crate::fetch::download_resumable(ZIP_URL, ZIP_BYTES, "MariaDB", &part, progress)?;
    if !checksum_ok(&bytes) {
        crate::fetch::discard(&part);
        return Err("The download did not match MariaDB's own checksum, so Castle threw it away. Try again later.".into());
    }
    crate::fetch::unzip_dropping_top(&bytes, &home.join(VERSION), keep_in_unpack, "database")?;
    crate::fetch::discard(&part);
    initialise(home, port)?;
    Ok(port)
}

/// Makes the data folder once (root on 127.0.0.1 only) and writes Castle's my.ini.
fn initialise(home: &Path, port: u16) -> Result<(), String> {
    let data = data_dir(home);
    if !data.join("mysql").is_dir() {
        let mut command = hidden(&program(home, "mariadb-install-db"));
        command.arg(format!("--datadir={}", data.display())).arg(format!("--port={port}"));
        // macOS: root signs in with no password over 127.0.0.1, as on Windows (not the unix socket)
        #[cfg(target_os = "macos")]
        {
            let base = program(home, "mariadbd").parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
            command.arg(format!("--basedir={}", base.display())).arg("--auth-root-authentication-method=normal");
        }
        if !run_to_end(command, Duration::from_secs(180)) || !data.join("mysql").is_dir() {
            return Err("MariaDB was downloaded but its data folder could not be made.".into());
        }
    }
    write_ini(home, port)
}

pub fn write_ini(home: &Path, port: u16) -> Result<(), String> {
    std::fs::write(ini_path(home), my_ini(&data_dir(home), port)).map_err(|_| "The database settings could not be written.".to_string())
}

// ---------- start / stop ----------

/// Starts Castle's engine hidden and waits until it greets on its port.
pub fn start(home: &Path, port: u16) -> Result<(), String> {
    if !installed(home) {
        return Err("The database is not installed yet.".into());
    }
    match probe_port(port) {
        PortUse::MySql => return Ok(()),
        PortUse::Other => return Err(port_busy_line(port)),
        PortUse::Free => {}
    }
    write_ini(home, port)?;
    let mut command = hidden(&program(home, "mariadbd"));
    command.arg(format!("--defaults-file={}", ini_path(home).display()));
    command.spawn().map_err(|_| "The database could not be started.".to_string())?;
    let started = Instant::now();
    while started.elapsed() < START_LIMIT {
        if probe_port(port) == PortUse::MySql {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("The database did not answer within 20 seconds.".into())
}

/// Asks Castle's engine to shut down cleanly; waits until the port is quiet.
pub fn stop(home: &Path, port: u16) -> Result<(), String> {
    if probe_port(port) != PortUse::MySql {
        return Ok(());
    }
    let mut command = hidden(&program(home, "mariadb-admin"));
    command.args(["--protocol=tcp", "--host=127.0.0.1", &format!("--port={port}"), "--user=root", "shutdown"]);
    if !run_to_end(command, Duration::from_secs(30)) {
        return Err("The database did not stop when asked.".into());
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        if probe_port(port) == PortUse::Free {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("The database is still stopping.".into())
}

// ---------- what Castle does with it ----------

/// What Settings and the HOME card show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbView {
    pub engine: Engine,
    pub version: &'static str,
    /// The size of the download, for the Install button.
    pub download_bytes: u64,
    /// How Install gets the engine: "download" (the release zip) or "homebrew" (macOS).
    pub source: &'static str,
    pub keep_running: bool,
}

fn view_of(store: &SettingsStore) -> DbView {
    let s = store.get();
    DbView { engine: detect(&home(), s.db_port, s.db_external), version: VERSION, download_bytes: ZIP_BYTES, source: if cfg!(target_os = "macos") { "homebrew" } else { "download" }, keep_running: s.db_keep_running }
}

/// Keeps the engine's port and kind in settings.json (the page never writes these two).
fn remember(store: &SettingsStore, port: Option<u16>, external: bool) -> Result<(), String> {
    let mut settings = store.get();
    settings.db_port = port;
    settings.db_external = external;
    crate::settings::save(&store.path, &settings).map_err(|_| "The settings could not be saved.".to_string())?;
    *store.current.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub async fn db_status(app: AppHandle) -> DbView {
    tauri::async_runtime::spawn_blocking(move || view_of(&app.state::<SettingsStore>())).await.expect("db status")
}

/// Install: download, check, unpack, make the data folder, then start. The port is chosen once.
#[tauri::command]
pub async fn db_install(app: AppHandle) -> Result<DbView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let home = home();
        let port = match store.get().db_port.filter(|_| !store.get().db_external) {
            Some(port) => port,
            None => choose_port(probe_port).ok_or("No free port was found for the database between 3306 and 3399.")?,
        };
        if !installed(&home) {
            let emitter = app.clone();
            install(&home, port, move |done, total| {
                let _ = emitter.emit(INSTALL_EVENT, serde_json::json!({ "done": done, "total": total }));
            })?;
        }
        remember(&store, Some(port), false)?;
        start(&home, port)?;
        Ok(view_of(&store))
    })
    .await
    .map_err(|_| "The install stopped.".to_string())?
}

#[tauri::command]
pub async fn db_start(app: AppHandle) -> Result<DbView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let s = store.get();
        if s.db_external {
            return Err("This database belongs to another program on this PC (XAMPP, for example). Start it there.".into());
        }
        start(&home(), s.db_port.unwrap_or(PREFERRED_PORT))?;
        Ok(view_of(&store))
    })
    .await
    .map_err(|_| "The start stopped.".to_string())?
}

#[tauri::command]
pub async fn db_stop(app: AppHandle) -> Result<DbView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let s = store.get();
        if s.db_external {
            return Err("This database belongs to another program on this PC. Stop it there.".into());
        }
        stop_own(&home(), s.db_port.unwrap_or(PREFERRED_PORT))?;
        Ok(view_of(&store))
    })
    .await
    .map_err(|_| "The stop did not finish.".to_string())?
}

/// The member chooses the MySQL / MariaDB already answering on 3306 instead of installing one.
#[tauri::command]
pub fn db_use_existing(store: State<SettingsStore>) -> Result<DbView, String> {
    if probe_port(PREFERRED_PORT) != PortUse::MySql {
        return Err("No database is answering on port 3306 right now. Start it (XAMPP's MySQL, for example) and try again.".into());
    }
    remember(&store, Some(PREFERRED_PORT), true)?;
    Ok(view_of(&store))
}

/// Creates the companion's database, its own user and its tables, and writes the connection
/// details into the memory folder's .env. Castle's own engine needs nothing from the member;
/// an existing server (XAMPP) is asked for its admin login once, used here and never kept.
#[tauri::command]
pub async fn db_create(app: AppHandle, admin_user: Option<String>, admin_password: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let s = store.get();
        let memory = s.memory_folder.clone().map(PathBuf::from).filter(|f| f.is_dir()).ok_or("Choose your MemoryCore folder first.")?;
        let port = s.db_port.ok_or("Install the database first.")?;
        if probe_port(port) != PortUse::MySql {
            return Err("The database is not running. Press Start first.".into());
        }
        let companion = s.companion_name.clone().unwrap_or_default();
        let access = if s.db_external {
            crate::memdb::provision(port, admin_user.as_deref().unwrap_or("root"), admin_password.as_deref().unwrap_or(""), &companion)?
        } else {
            let root = crate::memdb::secure_root(port, crate::key::db_root())?;
            crate::key::keep_db_root(&root)?;
            crate::memdb::provision(port, "root", &root, &companion)?
        };
        crate::memdb::write_env(&memory, &access)?;
        Ok(access.name)
    })
    .await
    .map_err(|_| "Creating the database stopped.".to_string())?
}

fn memory_folder(store: &SettingsStore) -> Result<PathBuf, String> {
    store.get().memory_folder.map(PathBuf::from).filter(|f| f.is_dir()).ok_or_else(|| "Choose your MemoryCore folder first.".to_string())
}

/// What moving in would take: read only, nothing changed.
#[tauri::command]
pub async fn db_move_preview(app: AppHandle) -> Result<crate::import::Counts, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(crate::import::collect(&memory_folder(&app.state::<SettingsStore>())?).counts()))
        .await
        .map_err(|_| "The check stopped.".to_string())?
}

/// Moves the memory's markdown into the database: every file kept exactly, the rows made and
/// read back, the dump written, the private backup run when there is one - and only after all
/// of that are the files deleted. Then the companion's instructions learn the door.
#[tauri::command]
pub async fn db_move_in(app: AppHandle) -> Result<crate::import::Counts, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let folder = memory_folder(&store)?;
        let mut conn = crate::memdb::open_for(&folder)?;
        crate::memdb::migrate(&mut conn)?;
        let counts = crate::import::move_in(&mut conn, &folder, || {
            // the old files stay in the backup's history; a backup that fails stops the delete
            if crate::backup::status(&folder).ready {
                crate::backup::back_up_now(&folder, "Memory backup before moving into the database").map(|_| ())
            } else {
                Ok(())
            }
        })?;
        let s = store.get();
        if let (Some(companion), Some(member)) = (s.companion_name.as_deref(), s.member_name.as_deref()) {
            let _ = crate::setup::write_blocks(&folder, companion, member);
        }
        Ok(counts)
    })
    .await
    .map_err(|_| "Moving in stopped.".to_string())?
}

/// What Settings shows about this memory's own database.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDb {
    /// its name ("lyra_memorycoredb"), once created
    pub name: Option<String>,
    /// it answers right now
    pub reachable: bool,
    /// the memory lives in it (moved in, or written through the door)
    pub moved_in: bool,
    /// a dump waits in backup/memory.sql and the database is empty: Restore is offered
    pub can_restore: bool,
}

#[tauri::command]
pub async fn db_memory(app: AppHandle) -> MemoryDb {
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(folder) = memory_folder(&app.state::<SettingsStore>()) else { return MemoryDb::default() };
        let name = std::fs::read_to_string(folder.join(".env")).ok().and_then(|t| crate::memdb::access_from_env(&t)).map(|a| a.name);
        let mut view = MemoryDb { name, ..MemoryDb::default() };
        if let Ok(mut conn) = crate::memdb::open_for(&folder) {
            view.reachable = true;
            view.moved_in = crate::memdb::migrate(&mut conn).is_ok() && crate::store::moved_in(&mut conn);
            let _ = crate::mirror::backfill_titles(&mut conn);
            let _ = crate::import::drop_placeholder_entries(&mut conn);
            view.can_restore = !view.moved_in && folder.join(crate::backup::DUMP_FILE).is_file();
        }
        view
    })
    .await
    .unwrap_or_default()
}

/// A new PC, or a database made fresh: the memory comes back from the dump the backup carried.
/// Only into an empty database - a memory already there is never overwritten.
#[tauri::command]
pub async fn db_restore(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let folder = memory_folder(&store)?;
        let mut conn = crate::memdb::open_for(&folder)?;
        crate::memdb::migrate(&mut conn)?;
        if crate::store::moved_in(&mut conn) {
            return Err("This database already holds a memory, so nothing was restored over it.".into());
        }
        let line = crate::door::restore(&mut conn, &folder.join(crate::backup::DUMP_FILE))?;
        crate::memdb::migrate(&mut conn)?;
        let _ = crate::mirror::sync_skills(&mut conn, &folder);
        let s = store.get();
        if let (Some(companion), Some(member)) = (s.companion_name.as_deref(), s.member_name.as_deref()) {
            let _ = crate::setup::write_blocks(&folder, companion, member);
        }
        Ok(line)
    })
    .await
    .map_err(|_| "The restore stopped.".to_string())?
}

/// The way back: the memory becomes files in its folder again and the database is emptied.
/// The companion's instructions go back to reading the files.
#[tauri::command]
pub async fn db_move_out(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let folder = memory_folder(&store)?;
        let mut conn = crate::memdb::open_for(&folder)?;
        let line = crate::import::move_out(&mut conn, &folder)?;
        let s = store.get();
        if let (Some(companion), Some(member)) = (s.companion_name.as_deref(), s.member_name.as_deref()) {
            let _ = crate::setup::write_blocks(&folder, companion, member);
        }
        Ok(line)
    })
    .await
    .map_err(|_| "Moving out stopped.".to_string())?
}

/// Writes the whole memory out as markdown into a folder the member chose.
#[tauri::command]
pub async fn db_export(app: AppHandle, to: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder = memory_folder(&app.state::<SettingsStore>())?;
        let mut conn = crate::memdb::open_for(&folder)?;
        let ask = crate::door::parse(&["export".to_string(), "--to".to_string(), to])?;
        crate::door::run(&mut conn, &ask, &folder)
    })
    .await
    .map_err(|_| "The export stopped.".to_string())?
}

/// The event the page hears while "Set it up for me" runs: { step, state } with state
/// "running" or "done".
pub const SETUP_EVENT: &str = "castle://db-setup";

/// The steps "Set it up for me" still has to run, in order, from where things stand now
/// (pure, tested). It never includes the move: that waits for the member's own tick.
pub fn setup_plan(engine: &Engine, has_database: bool) -> Vec<&'static str> {
    let mut steps = Vec::new();
    match engine {
        Engine::NotInstalled { .. } => steps.extend(["install", "start"]),
        Engine::Stopped { .. } => steps.push("start"),
        Engine::Running { .. } | Engine::External { .. } => {}
    }
    if !has_database {
        steps.push("create");
    }
    steps.push("check");
    steps
}

/// "Set it up for me": installs Castle's own database when there is none, starts it, creates
/// the companion's database and checks what would move - each step said as it runs. It stops
/// there and gives back the counts; nothing is moved or deleted without the member's own tick.
/// A step that fails gives its plain line and leaves the earlier steps done.
#[tauri::command]
pub async fn db_setup_all(app: AppHandle) -> Result<crate::import::Counts, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let folder = memory_folder(&store)?;
        let home = home();
        let s = store.get();
        let engine = detect(&home, s.db_port, s.db_external);
        let said = |step: &str, state: &str| {
            let _ = app.emit(SETUP_EVENT, serde_json::json!({ "step": step, "state": state }));
        };
        for step in setup_plan(&engine, crate::setup::has_database(&folder)) {
            said(step, "running");
            match step {
                "install" => {
                    let port = choose_port(probe_port).ok_or("No free port was found for the database between 3306 and 3399.")?;
                    let emitter = app.clone();
                    install(&home, port, move |done, total| {
                        let _ = emitter.emit(INSTALL_EVENT, serde_json::json!({ "done": done, "total": total }));
                    })?;
                    remember(&store, Some(port), false)?;
                }
                "start" => start(&home, store.get().db_port.unwrap_or(PREFERRED_PORT))?,
                "create" => {
                    let now = store.get();
                    if now.db_external {
                        return Err("This database belongs to another program on this PC, so Castle needs its admin login once. Open Settings > Database to create your memory database.".into());
                    }
                    let port = now.db_port.ok_or("Install the database first.")?;
                    let root = crate::memdb::secure_root(port, crate::key::db_root())?;
                    crate::key::keep_db_root(&root)?;
                    let access = crate::memdb::provision(port, "root", &root, &now.companion_name.clone().unwrap_or_default())?;
                    crate::memdb::write_env(&folder, &access)?;
                }
                _ => {}
            }
            said(step, "done");
        }
        Ok(crate::import::collect(&folder).counts())
    })
    .await
    .map_err(|_| "The setup stopped.".to_string())?
}

/// One skill, for Settings > Database.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRow {
    pub plugin: String,
    pub name: String,
    pub description: Option<String>,
    /// served as a stub: a small SKILL.md on disk, the instructions loaded from the database
    pub stubbed: bool,
    /// the whole SKILL.md and its stub, in bytes: what a session reads either way
    pub full_bytes: usize,
    pub stub_bytes: usize,
    /// the skill's flows (`skill show --for <flow>`)
    pub flows: Vec<String>,
}

fn skill_rows(conn: &mut mysql::Conn) -> Result<Vec<SkillRow>, String> {
    use mysql::prelude::Queryable;
    let rows: Vec<(String, String, Option<String>, bool, Option<String>)> = conn
        .query("SELECT s.plugin, s.name, s.description, s.stubbed, f.body FROM skills s LEFT JOIN skill_files f ON f.plugin = s.plugin AND f.name = s.name AND f.path = 'SKILL.md' ORDER BY s.plugin, s.name")
        .map_err(|_| "The skills could not be read.".to_string())?;
    let mut flows = crate::mirror::flow_names(conn)?;
    Ok(rows
        .into_iter()
        .map(|(plugin, name, description, stubbed, body)| {
            let flows = flows.remove(&(plugin.clone(), name.clone())).unwrap_or_default();
            let body = body.unwrap_or_default();
            let stub_bytes = crate::mirror::stub_of(&plugin, &name, &body, &flows).len();
            SkillRow { full_bytes: body.len(), stub_bytes, flows, plugin, name, description, stubbed }
        })
        .collect())
}

/// The memory's skills (disk and database made the same first, so a skill just installed shows).
#[tauri::command]
pub async fn db_skills(app: AppHandle) -> Result<Vec<SkillRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder = memory_folder(&app.state::<SettingsStore>())?;
        let mut conn = crate::memdb::open_for(&folder)?;
        crate::memdb::migrate(&mut conn)?;
        crate::mirror::sync_skills(&mut conn, &folder)?;
        skill_rows(&mut conn)
    })
    .await
    .map_err(|_| "Reading the skills stopped.".to_string())?
}

/// Switches one skill between a whole SKILL.md and a stub served from the database.
#[tauri::command]
pub async fn db_skill_stub(app: AppHandle, plugin: String, name: String, on: bool) -> Result<Vec<SkillRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder = memory_folder(&app.state::<SettingsStore>())?;
        let mut conn = crate::memdb::open_for(&folder)?;
        crate::mirror::sync_skills(&mut conn, &folder)?;
        crate::mirror::set_stub(&mut conn, &folder, &plugin, &name, on)?;
        skill_rows(&mut conn)
    })
    .await
    .map_err(|_| "Switching the skill stopped.".to_string())?
}

/// Every skill at once: all served from the database, or all whole files again.
#[tauri::command]
pub async fn db_skill_stub_all(app: AppHandle, on: bool) -> Result<Vec<SkillRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use mysql::prelude::Queryable;
        let folder = memory_folder(&app.state::<SettingsStore>())?;
        let mut conn = crate::memdb::open_for(&folder)?;
        crate::mirror::sync_skills(&mut conn, &folder)?;
        let names: Vec<(String, String)> = conn.query("SELECT plugin, name FROM skills ORDER BY plugin, name").map_err(|_| "The skills could not be read.".to_string())?;
        for (plugin, name) in names {
            crate::mirror::set_stub(&mut conn, &folder, &plugin, &name, on)?;
        }
        skill_rows(&mut conn)
    })
    .await
    .map_err(|_| "Switching the skills stopped.".to_string())?
}

/// Stops Castle's own engine: over SQL once root has a password, else with mariadb-admin.
fn stop_own(home: &Path, port: u16) -> Result<(), String> {
    if probe_port(port) != PortUse::MySql {
        return Ok(());
    }
    if let Some(root) = crate::key::db_root() {
        if crate::memdb::shutdown(port, &root).is_ok() {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(15) {
                if probe_port(port) == PortUse::Free {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    stop(home, port)
}

/// When Castle opens: its own installed engine starts by itself, quietly, off the main thread.
pub fn start_with_castle(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let s = app.state::<SettingsStore>().get();
        let home = home();
        if s.db_external {
            settle_skills(&s);
            return;
        }
        if !installed(&home) {
            return;
        }
        if let Err(why) = start(&home, s.db_port.unwrap_or(PREFERRED_PORT)) {
            eprintln!("MemoryCore Castle: the database did not start: {why}");
            return;
        }
        settle_skills(&s);
    });
}

/// The skills on disk and in the database made the same (a file someone deleted comes back,
/// a file edited by hand is taken in). Quiet: a memory with no database is left alone.
fn settle_skills(settings: &crate::settings::Settings) {
    let Some(folder) = settings.memory_folder.as_ref().map(PathBuf::from).filter(|f| crate::setup::has_database(f)) else { return };
    if let Ok(mut conn) = crate::memdb::open_for(&folder) {
        if crate::memdb::migrate(&mut conn).is_ok() {
            let _ = crate::mirror::backfill_titles(&mut conn);
            let _ = crate::import::drop_placeholder_entries(&mut conn);
            let _ = crate::mirror::sync_skills(&mut conn, &folder);
        }
    }
}

/// When Castle ends: its own engine stops with it, unless the member keeps it running.
pub fn stop_with_castle(app: &AppHandle) {
    let s = app.state::<SettingsStore>().get();
    if s.db_external || s.db_keep_running || !installed(&home()) {
        return;
    }
    let _ = stop_own(&home(), s.db_port.unwrap_or(PREFERRED_PORT));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mysql_greeting_is_known_by_its_protocol_byte() {
        // length (3) + sequence (1) + protocol 10 + "11.4.13-MariaDB"...
        assert!(is_mysql_greeting(&[0x5b, 0, 0, 0, 10, b'1', b'1']));
        assert!(is_mysql_greeting(&[0x40, 0, 0, 0, 0xff, 0x6a, 0x04]));
        assert!(!is_mysql_greeting(b"HTTP/1.1 400"));
        assert!(!is_mysql_greeting(&[1, 2, 3]));
    }

    #[test]
    fn the_engine_takes_3306_when_free_else_the_first_free_from_3307() {
        assert_eq!(choose_port(|_| PortUse::Free), Some(3306));
        assert_eq!(choose_port(|p| if p == 3306 { PortUse::MySql } else { PortUse::Free }), Some(3307));
        assert_eq!(choose_port(|p| if p <= 3308 { PortUse::Other } else { PortUse::Free }), Some(3309));
        assert_eq!(choose_port(|_| PortUse::Other), None);
    }

    #[test]
    fn my_ini_keeps_the_engine_on_this_pc_with_unicode_text() {
        let ini = my_ini(Path::new("C:\\Users\\Rin\\AppData\\Local\\MemoryCore Castle\\mariadb\\data"), 3307);
        assert!(ini.contains("datadir=C:/Users/Rin/AppData/Local/MemoryCore Castle/mariadb/data\n"));
        assert!(ini.contains("port=3307\nbind-address=127.0.0.1\n"));
        assert!(ini.contains("character-set-server=utf8mb4"));
        assert_eq!(ini.matches("port=3307").count(), 2);
    }

    #[test]
    fn a_download_that_is_not_the_release_is_refused() {
        assert!(!checksum_ok(b"not the zip"));
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(ZIP_SHA256.len(), 64);
    }

    #[test]
    fn a_busy_port_says_how_to_find_the_program() {
        let line = port_busy_line(3307);
        #[cfg(windows)]
        assert!(line.contains("netstat -ano | findstr :3307") && line.contains("taskkill"));
        #[cfg(not(windows))]
        assert!(line.contains("lsof -nP -iTCP:3307") && line.contains("kill <PID>"));
    }

    #[test]
    fn set_it_up_for_me_runs_only_what_is_still_missing_and_never_the_move() {
        let fresh = Engine::NotInstalled { external: None };
        assert_eq!(setup_plan(&fresh, false), vec!["install", "start", "create", "check"]);
        assert_eq!(setup_plan(&Engine::Stopped { port: 3307 }, false), vec!["start", "create", "check"]);
        assert_eq!(setup_plan(&Engine::Running { port: 3307 }, false), vec!["create", "check"]);
        assert_eq!(setup_plan(&Engine::Running { port: 3307 }, true), vec!["check"]);
        // pressed again after a failed step: the done steps are not repeated
        assert_eq!(setup_plan(&Engine::Stopped { port: 3307 }, true), vec!["start", "check"]);
        assert_eq!(setup_plan(&Engine::External { port: 3306, answering: true }, false), vec!["create", "check"]);
        for engine in [fresh, Engine::Running { port: 3307 }] {
            assert!(!setup_plan(&engine, false).contains(&"move"));
        }
    }

    #[test]
    fn an_empty_engine_folder_is_not_installed_and_nothing_answers_on_a_free_port() {
        let home = std::env::temp_dir().join(format!("castle-db-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        assert!(!installed(&home));
        assert_eq!(bin_dir(&home), home.join(VERSION).join("bin"));
        // a port nobody uses in the test range
        let free = (40000..40100).find(|p| probe_port(*p) == PortUse::Free).unwrap();
        assert_eq!(detect(&home, Some(free), true), Engine::External { port: free, answering: false });
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_listener_that_does_not_greet_is_something_else() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (_s, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(1200));
        });
        assert_eq!(probe_port(port), PortUse::Other);
        handle.join().unwrap();
    }

    #[test]
    fn the_unpack_leaves_out_what_only_mariadb_developers_need() {
        assert!(keep_in_unpack(Path::new("bin/mariadbd.exe")));
        assert!(keep_in_unpack(Path::new("bin/server.dll")));
        assert!(keep_in_unpack(Path::new("share/english/errmsg.sys")));
        assert!(keep_in_unpack(Path::new("lib/plugin/auth_ed25519.dll")));
        assert!(!keep_in_unpack(Path::new("bin/server.pdb")));
        assert!(!keep_in_unpack(Path::new("lib/server.lib")));
        assert!(!keep_in_unpack(Path::new("include/mysql/mysql.h")));
    }

    /// The real thing, on demand: `cargo test --lib db::tests::live -- --ignored` with
    /// CASTLE_DB_LIVE_HOME set to an empty scratch folder. Downloads about 95 MB.
    #[test]
    #[ignore]
    fn live_install_start_stop() {
        let home = PathBuf::from(std::env::var_os("CASTLE_DB_LIVE_HOME").expect("set CASTLE_DB_LIVE_HOME"));
        let port = (3307..=3399).find(|p| probe_port(*p) == PortUse::Free).unwrap();
        if !installed(&home) {
            install(&home, port, |_, _| {}).unwrap();
        }
        assert!(installed(&home));
        assert_eq!(detect(&home, Some(port), false), Engine::Stopped { port });
        start(&home, port).unwrap();
        assert_eq!(detect(&home, Some(port), false), Engine::Running { port });
        stop(&home, port).unwrap();
        assert_eq!(detect(&home, Some(port), false), Engine::Stopped { port });
    }
}
