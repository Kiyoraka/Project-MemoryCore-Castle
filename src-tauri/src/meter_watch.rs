//! The gauges keep watch. A loop reads the processor and the memory every five seconds. A meter
//! trips after three readings in a row at or above its value (about fifteen seconds, so a short
//! spike passes), then rests ten minutes.
//!
//! On a memory trip Castle asks Windows to trim every app's working set. Nothing is ended, and part
//! of the memory comes back as apps use it again. On either trip Castle names the five programs
//! using the most, each with who it belongs to. Only an ordinary app may be closed, by the member's
//! click, asked once and checked again here.
//!
//! Never closable: Castle and everything it started (the companion's sessions, the DEV tabs'
//! servers), the memory database, programs inside the member's projects, developer servers, the
//! member's editor, and Windows itself.

use crate::servers::{owner_of, Owner, Proc};
use crate::settings::{Cli, SettingsStore};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// Readings in a row at or above the value before a meter trips (5 s apart: about 15 s).
pub const NEEDED: u8 = 3;
/// Seconds a meter rests after a trip before it may trip again.
pub const COOLDOWN_SECS: u64 = 600;
/// The value's bounds in the gear.
pub const MIN_LIMIT: u8 = 50;
pub const MAX_LIMIT: u8 = 99;
const TICK: Duration = Duration::from_secs(5);
const KEPT_TRIPS: usize = 10;
const TOP: usize = 5;
/// The event a trip sends to the page.
pub const TRIP_EVENT: &str = "castle://meter-trip";

/// The gear's settings, kept in Castle's settings file. On from the start at a gentle 90%.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MeterWatch {
    pub on: bool,
    pub cpu: u8,
    pub memory: u8,
}

impl Default for MeterWatch {
    fn default() -> Self {
        Self { on: true, cpu: 90, memory: 90 }
    }
}

impl MeterWatch {
    /// The values held to the gear's bounds.
    pub fn bounded(self) -> Self {
        Self { on: self.on, cpu: self.cpu.clamp(MIN_LIMIT, MAX_LIMIT), memory: self.memory.clamp(MIN_LIMIT, MAX_LIMIT) }
    }
}

/// One meter's streak and rest.
#[derive(Debug, Default, Clone, Copy)]
pub struct Trip {
    streak: u8,
    rest_until: u64,
}

impl Trip {
    /// One reading at `now` (seconds). True when this reading trips the meter.
    pub fn step(&mut self, value: u8, limit: u8, now: u64) -> bool {
        if value < limit {
            self.streak = 0;
            return false;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak >= NEEDED && now >= self.rest_until {
            self.streak = 0;
            self.rest_until = now + COOLDOWN_SECS;
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.streak = 0;
    }
}

/// One program in a trip's top five.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Eater {
    pub pid: u32,
    pub name: String,
    /// Percent of the whole processor.
    pub cpu: f32,
    pub mem_mb: u64,
    /// Who it belongs to when it is protected: "castle", "companion", "database", "project",
    /// "server", "editor", "system", or "" for an ordinary app.
    pub owner: String,
    pub closable: bool,
}

/// What a trip found.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TripReport {
    /// "cpu" or "memory".
    pub meter: String,
    pub value: u8,
    pub limit: u8,
    /// Milliseconds since 1970.
    pub at: u64,
    /// Memory trips on Windows only: the MB the trim gave back.
    pub freed_mb: Option<u64>,
    pub top: Vec<Eater>,
}

/// The last trips, newest first, for the gear.
#[derive(Default)]
pub struct Watch(Mutex<VecDeque<TripReport>>);

// ---------- who may be closed ----------

/// The memory database's engines, wherever they were started from.
const DATABASES: &[&str] = &["mariadbd", "mysqld"];
/// The member's editor.
const EDITORS: &[&str] = &["code", "code - insiders", "cursor"];
/// macOS: an editor runs as many helper programs ("Code Helper (Renderer)", "Cursor Helper (GPU)").
const EDITOR_HELPERS: &[&str] = &["code helper", "code - insiders helper", "cursor helper", "electron"];
/// The companion's programs, also when the member runs one outside Castle.
const COMPANIONS: &[&str] = &[Cli::Claude.command(), Cli::Codex.command(), Cli::Gemini.command(), Cli::Grok.command()];
/// Programs that are part of Windows or of Castle and are never closed from the card.
const PROTECTED: &[&str] = &[
    "memorycore-castle", "msedgewebview2", "explorer", "dwm", "csrss", "winlogon", "wininit", "services", "lsass",
    "svchost", "smss", "system", "registry", "fontdrvhost", "searchhost", "startmenuexperiencehost",
    "shellexperiencehost", "taskmgr", "conhost", "openconsole", "windowsterminal",
];

fn key(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// macOS: a program that is part of the system (or WebKit, which draws Castle's own window), which
/// Close must never offer: closing WindowServer logs the member out.
#[cfg_attr(windows, allow(dead_code))]
fn in_macos(exe: Option<&Path>, pid: u32) -> bool {
    if pid == 1 {
        return true;
    }
    exe.map(|p| {
        let text = p.to_string_lossy();
        ["/System/", "/usr/", "/bin/", "/sbin/", "/Library/Apple/"].iter().any(|root| text.starts_with(root))
            || text.contains("com.apple.WebKit")
            || text.contains("MemoryCore Castle.app/")
    })
    .unwrap_or(false)
}

// platform: windows-only - the Windows folder; macOS has in_macos
/// True when the program lives in the Windows folder.
fn in_windows(exe: Option<&Path>) -> bool {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
    let root = format!("{}\\", root.trim_end_matches('\\'));
    exe.map(|p| {
        let text = p.to_string_lossy().to_lowercase();
        text.strip_prefix(r"\\?\").unwrap_or(&text).starts_with(&root)
    })
    .unwrap_or(false)
}

/// True when `pid` is Castle or anything Castle started (its webview, the companion's sessions,
/// the DEV tabs' servers).
fn is_castle(pid: u32, castle: u32, procs: &HashMap<u32, Proc>) -> bool {
    let mut at = Some(pid);
    for _ in 0..64 {
        match at {
            Some(p) if p == castle => return true,
            Some(p) => at = procs.get(&p).and_then(|x| x.parent).filter(|&parent| parent != p),
            None => return false,
        }
    }
    false
}

/// The owner tag, and whether the card may offer Close.
pub fn judge(pid: u32, name: &str, exe: Option<&Path>, owner: Owner, castle: u32, procs: &HashMap<u32, Proc>) -> (String, bool) {
    let k = key(name);
    let tag = |t: &str| (t.to_string(), false);
    if pid == 0 || pid == 4 || k.is_empty() {
        return tag("system");
    }
    if DATABASES.contains(&k.as_str()) {
        return tag("database");
    }
    if COMPANIONS.contains(&k.as_str()) {
        return tag("companion");
    }
    if EDITORS.contains(&k.as_str()) || EDITOR_HELPERS.iter().any(|h| k.starts_with(h)) {
        return tag("editor");
    }
    if is_castle(pid, castle, procs) {
        return tag("castle");
    }
    match owner {
        Owner::Tab(_) | Owner::Folder(_) => return tag("project"),
        Owner::Other => return tag("server"),
        Owner::Hidden => {}
    }
    if in_windows(exe) || (!cfg!(windows) && in_macos(exe, pid)) || PROTECTED.contains(&k.as_str()) {
        return tag("system");
    }
    (String::new(), true)
}

/// The top `n` by the tripped meter: CPU by percent, memory by MB.
pub fn top_by(mut eaters: Vec<Eater>, meter: &str, n: usize) -> Vec<Eater> {
    if meter == "cpu" {
        eaters.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal).then(b.mem_mb.cmp(&a.mem_mb)));
    } else {
        eaters.sort_by(|a, b| b.mem_mb.cmp(&a.mem_mb).then(a.pid.cmp(&b.pid)));
    }
    eaters.truncate(n);
    eaters
}

// ---------- the machine ----------

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// (slot, session pid) per open DEV tab and (slot, project folder) per DEV tab, as `dev_servers` reads them.
fn dev_owners(app: &AppHandle) -> (Vec<(u8, u32)>, Vec<(u8, PathBuf)>) {
    let settings = app.state::<SettingsStore>().get();
    let hosts = app.state::<crate::dev::DevHosts>();
    let sessions = hosts.0.iter().enumerate().filter_map(|(slot, host)| host.pid().map(|pid| (slot as u8, pid))).collect();
    let projects =
        settings.dev_tabs.iter().enumerate().filter_map(|(slot, folder)| folder.as_ref().map(|f| (slot as u8, PathBuf::from(f)))).collect();
    (sessions, projects)
}

/// Every process with its CPU (two samples a second apart), memory, owner and whether it may be closed.
fn eaters(app: &AppHandle) -> Vec<Eater> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let Ok(mut system) = System::new() else { return Vec::new() };
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_exe(UpdateKind::OnlyIfNotSet).with_cwd(UpdateKind::OnlyIfNotSet);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    std::thread::sleep(Duration::from_millis(1000));
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
    let procs: HashMap<u32, Proc> = system
        .processes()
        .iter()
        .map(|(pid, p)| {
            let pid = pid.as_u32();
            let proc = Proc {
                pid,
                parent: p.parent().map(|x| x.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                folder: p.cwd().filter(|x| !x.as_os_str().is_empty()).map(Path::to_path_buf),
            };
            (pid, proc)
        })
        .collect();
    let (sessions, projects) = dev_owners(app);
    let castle = std::process::id();
    system
        .processes()
        .iter()
        .map(|(pid, p)| {
            let pid = pid.as_u32();
            let name = p.name().to_string_lossy().into_owned();
            let owner = owner_of(pid, &procs, &sessions, &projects);
            let (tag, closable) = judge(pid, &name, p.exe(), owner, castle, &procs);
            Eater {
                pid,
                name: name.strip_suffix(".exe").unwrap_or(&name).to_string(),
                cpu: ((p.cpu_usage() / cores) * 10.0).round() / 10.0,
                mem_mb: p.memory() / (1024 * 1024),
                owner: tag,
                closable,
            }
        })
        .collect()
}

fn used_mb() -> u64 {
    let Ok(mut system) = sysinfo::System::new() else { return 0 };
    system.refresh_memory();
    system.used_memory() / (1024 * 1024)
}

/// Asks Windows to trim every app's working set it may touch. Returns the MB given back, or None
/// where the trim does not exist (other systems).
pub fn trim() -> Option<u64> {
    if !cfg!(windows) {
        return None;
    }
    let before = used_mb();
    trim_all();
    std::thread::sleep(Duration::from_millis(800));
    Some(before.saturating_sub(used_mb()))
}

#[cfg(windows)]
fn trim_all() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA};
    let Ok(mut system) = sysinfo::System::new() else { return };
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for pid in system.processes().keys().map(|p| p.as_u32()).filter(|&p| p > 4) {
        // SAFETY: a plain handle opened and closed here. A process Windows refuses is skipped.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA, 0, pid);
            if handle.is_null() {
                continue;
            }
            K32EmptyWorkingSet(handle);
            CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn trim_all() {}

fn report(app: &AppHandle, meter: &str, value: u8, limit: u8) -> TripReport {
    let freed_mb = if meter == "memory" { trim() } else { None };
    TripReport { meter: meter.into(), value, limit, at: now_ms(), freed_mb, top: top_by(eaters(app), meter, TOP) }
}

fn keep_trip(app: &AppHandle, trip: TripReport) {
    {
        let watch = app.state::<Watch>();
        let mut trips = watch.0.lock().unwrap();
        trips.push_front(trip.clone());
        trips.truncate(KEPT_TRIPS);
    }
    let _ = app.emit(TRIP_EVENT, trip);
}

/// The processor reading, by the same way the gauges read it: the kernel's own times on Windows
/// (sysinfo's figure reads 0% on some computers), sysinfo elsewhere.
struct CpuReader {
    system: sysinfo::System,
    last: Option<crate::system::CpuTimes>,
    value: u8,
}

impl CpuReader {
    fn read(&mut self) -> u8 {
        match crate::system::cpu_times() {
            Some(now) => {
                if let Some(busy) = self.last.and_then(|before| crate::system::busy_percent(before, now)) {
                    self.value = busy;
                }
                self.last = Some(now);
                self.value
            }
            None => {
                self.system.refresh_cpu_usage();
                crate::system::percent(self.system.global_cpu_usage() as f64, 100.0)
            }
        }
    }
}

/// The loop: started once at launch, reading every five seconds while the gear is on.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(system) = sysinfo::System::new() else { return };
        let mut cpu_reader = CpuReader { system, last: crate::system::cpu_times(), value: 0 };
        let Ok(mut memory_system) = sysinfo::System::new() else { return };
        let (mut cpu_trip, mut mem_trip) = (Trip::default(), Trip::default());
        loop {
            std::thread::sleep(TICK);
            let watch = app.state::<SettingsStore>().get().meter_watch.bounded();
            let cpu = cpu_reader.read();
            if !watch.on {
                cpu_trip.reset();
                mem_trip.reset();
                continue;
            }
            memory_system.refresh_memory();
            let memory = crate::system::percent(memory_system.used_memory() as f64, memory_system.total_memory() as f64);
            let now = now_ms() / 1000;
            if mem_trip.step(memory, watch.memory, now) {
                let trip = report(&app, "memory", memory, watch.memory);
                keep_trip(&app, trip);
            }
            if cpu_trip.step(cpu, watch.cpu, now) {
                let trip = report(&app, "cpu", cpu, watch.cpu);
                keep_trip(&app, trip);
            }
        }
    });
}

// ---------- commands ----------

#[tauri::command]
pub fn meter_watch_get(store: State<SettingsStore>) -> MeterWatch {
    store.get().meter_watch.bounded()
}

#[tauri::command]
pub fn meter_watch_save(store: State<SettingsStore>, on: bool, cpu: u8, memory: u8) -> Result<MeterWatch, String> {
    let watch = MeterWatch { on, cpu, memory }.bounded();
    let mut next = store.get();
    next.meter_watch = watch;
    crate::settings::save(&store.path, &next).map_err(|_| "The gear could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(watch)
}

/// The gear's Tidy RAM now: the same trim, the MB given back (None where there is no trim).
#[tauri::command]
pub async fn meter_trim_now() -> Result<Option<u64>, String> {
    tauri::async_runtime::spawn_blocking(trim).await.map_err(|_| "The tidy could not run.".to_string())
}

#[tauri::command]
pub fn meter_trips(watch: State<Watch>) -> Vec<TripReport> {
    watch.0.lock().unwrap().iter().cloned().collect()
}

/// Closes one app from the card, only when it is still the same program and still closable.
#[tauri::command]
pub async fn meter_close(app: AppHandle, pid: u32, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let found = eaters(&app).into_iter().find(|e| e.pid == pid);
        let Some(eater) = found else { return Err("It has already closed.".to_string()) };
        if eater.name.to_lowercase() != name.to_lowercase() {
            return Err("That program has changed, so nothing was closed.".to_string());
        }
        if !eater.closable {
            return Err(format!("{} is protected. Castle never closes it.", eater.name));
        }
        let Ok(mut system) = sysinfo::System::new() else { return Err("The program list could not be read.".to_string()) };
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]), true);
        match system.process(sysinfo::Pid::from_u32(pid)) {
            Some(process) if process.kill() => Ok(()),
            Some(_) => Err(format!("{} would not close {}.", if cfg!(windows) { "Windows" } else { "macOS" }, eater.name)),
            None => Err("It has already closed.".to_string()),
        }
    })
    .await
    .map_err(|_| "The close could not run.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_system_programs_and_webkit_are_never_offered_close() {
        assert!(in_macos(Some(Path::new("/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer")), 412));
        assert!(in_macos(Some(Path::new("/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent")), 900));
        assert!(in_macos(None, 1), "launchd");
        assert!(!in_macos(Some(Path::new("/Applications/Spotify.app/Contents/MacOS/Spotify")), 901));
        assert!(!in_macos(None, 902));
    }

    fn eater(pid: u32, cpu: f32, mem_mb: u64) -> Eater {
        Eater { pid, name: format!("p{pid}"), cpu, mem_mb, owner: String::new(), closable: true }
    }

    #[test]
    fn a_meter_trips_after_three_in_a_row_then_rests() {
        let mut t = Trip::default();
        assert!(!t.step(95, 90, 0));
        assert!(!t.step(92, 90, 5));
        assert!(t.step(90, 90, 10));
        // resting: a long streak does not trip again inside ten minutes
        for i in 0..20 {
            assert!(!t.step(99, 90, 15 + i * 5));
        }
        assert!(t.step(99, 90, 10 + COOLDOWN_SECS));
    }

    #[test]
    fn a_dip_breaks_the_streak() {
        let mut t = Trip::default();
        assert!(!t.step(95, 90, 0));
        assert!(!t.step(95, 90, 5));
        assert!(!t.step(70, 90, 10));
        assert!(!t.step(95, 90, 15));
        assert!(!t.step(95, 90, 20));
        assert!(t.step(95, 90, 25));
    }

    #[test]
    fn the_watch_starts_on_at_ninety_and_stays_in_the_gear_bounds() {
        assert_eq!(MeterWatch::default(), MeterWatch { on: true, cpu: 90, memory: 90 });
        let w = MeterWatch { on: true, cpu: 10, memory: 120 }.bounded();
        assert_eq!((w.cpu, w.memory), (MIN_LIMIT, MAX_LIMIT));
    }

    #[test]
    fn only_ordinary_apps_may_be_closed() {
        let mut procs = HashMap::new();
        procs.insert(100, Proc { pid: 100, parent: None, name: "memorycore-castle.exe".into(), folder: None });
        procs.insert(101, Proc { pid: 101, parent: Some(100), name: "msedgewebview2.exe".into(), folder: None });
        procs.insert(102, Proc { pid: 102, parent: Some(100), name: "cmd.exe".into(), folder: None });
        procs.insert(103, Proc { pid: 103, parent: Some(102), name: "node.exe".into(), folder: None });
        procs.insert(300, Proc { pid: 300, parent: Some(1), name: "chrome.exe".into(), folder: None });
        let chrome = Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe");
        let windows = Path::new(r"C:\Windows\System32\notepad.exe");
        assert_eq!(judge(300, "chrome.exe", Some(chrome), Owner::Hidden, 100, &procs), (String::new(), true));
        assert_eq!(judge(101, "msedgewebview2.exe", None, Owner::Hidden, 100, &procs).0, "castle");
        assert_eq!(judge(103, "node.exe", None, Owner::Other, 100, &procs).0, "castle");
        assert_eq!(judge(400, "claude.exe", None, Owner::Hidden, 100, &procs), ("companion".into(), false));
        assert_eq!(judge(401, "codex.exe", None, Owner::Hidden, 100, &procs).1, false);
        assert_eq!(judge(402, "mariadbd.exe", None, Owner::Other, 100, &procs), ("database".into(), false));
        assert_eq!(judge(403, "mysqld.exe", None, Owner::Hidden, 100, &procs).0, "database");
        assert_eq!(judge(500, "node.exe", None, Owner::Other, 100, &procs).0, "server");
        assert_eq!(judge(501, "java.exe", None, Owner::Folder(2), 100, &procs).0, "project");
        assert_eq!(judge(502, "python.exe", None, Owner::Tab(0), 100, &procs).0, "project");
        assert_eq!(judge(503, "Code.exe", None, Owner::Hidden, 100, &procs), ("editor".into(), false));
        assert_eq!(judge(504, "Cursor.exe", None, Owner::Hidden, 100, &procs).1, false);
        assert_eq!(judge(505, "notepad.exe", Some(windows), Owner::Hidden, 100, &procs), ("system".into(), false));
        assert_eq!(judge(506, "explorer.exe", None, Owner::Hidden, 100, &procs).1, false);
        assert_eq!(judge(4, "System", None, Owner::Hidden, 100, &procs).1, false);
        assert_eq!(judge(507, "", None, Owner::Hidden, 100, &procs).1, false);
    }

    #[test]
    fn the_top_five_follow_the_tripped_meter() {
        let all = vec![eater(1, 5.0, 900), eater(2, 40.0, 100), eater(3, 12.5, 2000), eater(4, 0.0, 50), eater(5, 3.0, 300), eater(6, 1.0, 1200)];
        assert_eq!(top_by(all.clone(), "cpu", 3).iter().map(|e| e.pid).collect::<Vec<_>>(), [2, 3, 1]);
        assert_eq!(top_by(all, "memory", 5).iter().map(|e| e.pid).collect::<Vec<_>>(), [3, 6, 1, 5, 2]);
    }

    #[test]
    #[ignore = "trims this computer's working sets"]
    fn real_trim() {
        println!("freed {:?} MB", trim());
    }
}
