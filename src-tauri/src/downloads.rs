//! The downloader in BROWSER (ported from the Alice Keep's Phase 192, browser-downloads-2). Every download
//! a page starts comes here, a pasted link too.
//!
//! - **Parts at once**: a server that allows ranges gives a big file in up to eight parts fetched together
//!   into one pre-sized `.part` file; otherwise one stream.
//! - **Pause and resume**: each download keeps a state file beside its `.part` (the address, the size, each
//!   part's reached byte, the ETag); resume goes on from there, after a pause, a closed Castle or a dropped
//!   line. A changed file (its ETag) or a server that stops allowing ranges starts it over, and the card
//!   says so.
//! - **The queue**: three at a time. **Folders**: the member's folder, sorted by type unless turned off.
//! - **Windows' mark**: on Windows every finished file carries `Zone.Identifier` (ZoneId=3), so
//!   SmartScreen and Defender still check it when it is opened.
//! - **The guards**: the address goes through the browser's guard and Google's lists first; a dangerous
//!   one is refused.
//! - **The list** survives a restart in the settings folder (`downloads.json`); a private tab's download is
//!   never written there.

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The most downloads running at once, and parts per file.
pub const RUNNING: usize = 3;
pub const PARTS: usize = 8;
/// A file smaller than this is one stream; each part is at least this big.
const PART_MIN: u64 = 1024 * 1024;
const LIST_FILE: &str = "downloads.json";
pub const STATE_EVENT: &str = "downloads-state";
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

// ---------------------------------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------------------------------

/// The folder a file is sorted into, by its extension.
pub fn category_of(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "flv" | "m4v" | "3gp" | "ts" => "Video",
        "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" | "wma" => "Music",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "rtf" | "odt" | "epub" | "md" => "Documents",
        "exe" | "msi" | "msix" | "appx" | "bat" | "cmd" | "apk" | "dmg" => "Programs",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "tgz" => "Compressed",
        _ => "Other",
    }
}

/// A file name made safe for Windows: no path, no reserved characters, never empty, at most 180 characters.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_matches('.').to_string();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "LPT1", "LPT2", "LPT3"];
    let stem = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
    let clean = if clean.is_empty() || reserved.contains(&stem.as_str()) { format!("download{}", if clean.is_empty() { String::new() } else { format!("-{clean}") }) } else { clean };
    clean.chars().take(180).collect()
}

/// The file name from a `Content-Disposition` header, else from the address's last part.
pub fn name_from(disposition: Option<&str>, url: &str) -> String {
    if let Some(d) = disposition {
        let lower = d.to_ascii_lowercase();
        if let Some(i) = lower.find("filename*=") {
            let v = d[i + 10..].split(';').next().unwrap_or("").trim();
            let v = v.rsplit("''").next().unwrap_or(v).trim_matches('"');
            let decoded = percent_decode(v);
            if !decoded.is_empty() {
                return safe_name(&decoded);
            }
        }
        if let Some(i) = lower.find("filename=") {
            let v = d[i + 9..].split(';').next().unwrap_or("").trim().trim_matches('"');
            if !v.is_empty() {
                return safe_name(v);
            }
        }
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    safe_name(&percent_decode(last))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A path in the folder that does not overwrite anything: `name (1).ext`, `name (2).ext` and so on.
pub fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() && !part_of(&first).exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..10_000).map(|n| folder.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists() && !part_of(p).exists()).unwrap_or(first)
}

pub fn part_of(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", path.display()))
}

fn state_of(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part.json", path.display()))
}

/// The byte ranges of a file split into parts (inclusive ends), each at least PART_MIN.
pub fn split_parts(size: u64, most: usize) -> Vec<Part> {
    if size == 0 {
        return vec![Part { start: 0, end: 0, pos: 0 }];
    }
    let n = ((size / PART_MIN).max(1) as usize).min(most.max(1)) as u64;
    let step = size / n;
    (0..n)
        .map(|i| {
            let start = i * step;
            let end = if i == n - 1 { size - 1 } else { (i + 1) * step - 1 };
            Part { start, end, pos: start }
        })
        .collect()
}

/// Whether a saved download must start over: the file changed, or ranges are no longer allowed.
pub fn must_restart(saved_etag: &str, now_etag: &str, ranges: bool) -> bool {
    !ranges || (!saved_etag.is_empty() && !now_etag.is_empty() && saved_etag != now_etag)
}

/// A page's cookies as one `Cookie` header, so a signed-in download still works when Castle fetches it.
pub fn cookie_header(pairs: &[(String, String)]) -> String {
    pairs.iter().filter(|(n, _)| !n.is_empty()).map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ")
}

/// Whether Castle's own fetch was refused in a way the webview's own download may still get past (a
/// sign-in wall, or the page instead of the file).
pub fn needs_native(error: &str) -> bool {
    error.starts_with("The server refused") || error.starts_with("The server sent a web page")
}

/// Marks a finished file as from the internet (Windows' `Zone.Identifier` stream); nothing elsewhere.
pub fn mark_from_internet(path: &Path, url: &str, referer: &str) {
    #[cfg(windows)]
    {
        let _ = std::fs::write(format!("{}:Zone.Identifier", path.display()), zone_text(url, referer));
    }
    // macOS keeps its own quarantine mark for what a person opens; the words are not written there
    #[cfg(not(windows))]
    let _ = (path, zone_text(url, referer));
}

/// The words Windows reads to know a file came from the internet.
pub fn zone_text(url: &str, referer: &str) -> String {
    let mut s = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if !referer.is_empty() {
        s.push_str(&format!("ReferrerUrl={referer}\r\n"));
    }
    s.push_str(&format!("HostUrl={url}\r\n"));
    s
}

// ---------------------------------------------------------------------------------------------------
// The state of one download
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct Part {
    pub start: u64,
    pub end: u64,
    /// The next byte to fetch.
    pub pos: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Saved {
    url: String,
    size: Option<u64>,
    etag: String,
    ranges: bool,
    parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    pub id: u64,
    pub url: String,
    pub name: String,
    pub path: String,
    pub category: String,
    /// queued | running | paused | done | failed
    pub state: String,
    pub size: Option<u64>,
    pub done: u64,
    pub parts: u32,
    pub speed: u64,
    pub line: String,
    pub private: bool,
    pub referer: String,
    /// file | video
    pub kind: String,
    /// The video's chosen format.
    pub format: String,
    pub finished_at: u64,
    /// Only Castle's own fetch can pause; the webview's own fallback cannot.
    pub pausable: bool,
    /// The tab it came from (0: a pasted link or a pop-up), for the webview's own fallback.
    pub tab: u32,
    #[serde(skip)]
    pub cookies: String,
}

/// The items worth keeping on disk: never a private tab's.
pub fn persisted(items: &[Item]) -> Vec<Item> {
    items.iter().filter(|i| !i.private).cloned().map(|mut i| {
        if i.state == "running" {
            i.state = "paused".into();
            i.speed = 0;
        }
        i.cookies.clear();
        i
    }).collect()
}

// ---------------------------------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------------------------------

pub struct Probe {
    pub size: Option<u64>,
    pub ranges: bool,
    pub etag: String,
    pub name: String,
    /// The server answered with a web page, not a file (a sign-in wall).
    pub page: bool,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(None::<Duration>)
        .build()
        .map_err(|e| e.to_string())
}

fn request(c: &reqwest::blocking::Client, url: &str, referer: &str, cookies: &str) -> reqwest::blocking::RequestBuilder {
    let mut r = c.get(url);
    if !referer.is_empty() {
        r = r.header("Referer", referer);
    }
    if !cookies.is_empty() {
        r = r.header("Cookie", cookies);
    }
    r
}

/// One small ask (`bytes=0-0`) for the size, whether ranges work, the ETag and the name.
pub fn probe(url: &str, referer: &str, cookies: &str) -> Result<Probe, String> {
    let c = client()?;
    let resp = request(&c, url, referer, cookies).header("Range", "bytes=0-0").send().map_err(|_| "The server could not be reached.".to_string())?;
    let status = resp.status().as_u16();
    if status == 401 || status == 403 {
        return Err(format!("The server refused the download ({status})."));
    }
    if !(200..300).contains(&status) {
        return Err(format!("The server answered {status}."));
    }
    let h = resp.headers();
    let text = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    let etag = text("etag").unwrap_or_default();
    let name = name_from(text("content-disposition").as_deref(), url);
    let page = text("content-type").is_some_and(|t| t.starts_with("text/html")) && text("content-disposition").is_none();
    let (size, ranges) = if status == 206 {
        let total = text("content-range").and_then(|r| r.rsplit('/').next().and_then(|n| n.trim().parse::<u64>().ok()));
        (total, total.is_some())
    } else {
        (text("content-length").and_then(|n| n.parse::<u64>().ok()), false)
    };
    Ok(Probe { size, ranges, etag, name, page })
}

/// Fetches one part into the file at its offset, moving `pos` as it goes; stops when `stop` is set.
fn fetch_part(c: &reqwest::blocking::Client, url: &str, referer: &str, cookies: &str, file: &Path, part: Arc<Mutex<Part>>, ranged: bool, stop: Arc<AtomicBool>, moved: Arc<AtomicU64>) -> Result<(), String> {
    let (pos, end) = {
        let p = part.lock().unwrap();
        (p.pos, p.end)
    };
    if ranged && pos > end {
        return Ok(());
    }
    let mut req = request(c, url, referer, cookies);
    if ranged {
        req = req.header("Range", format!("bytes={pos}-{end}"));
    }
    let mut resp = req.send().map_err(|_| "The line dropped.".to_string())?;
    if ranged && resp.status().as_u16() != 206 {
        return Err("The server stopped allowing parts.".into());
    }
    let mut out = std::fs::OpenOptions::new().write(true).open(file).map_err(|_| "The file could not be written.".to_string())?;
    out.seek(SeekFrom::Start(if ranged { pos } else { 0 })).map_err(|_| "The file could not be written.".to_string())?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let n = resp.read(&mut buf).map_err(|_| "The line dropped.".to_string())?;
        if n == 0 {
            return Ok(());
        }
        out.write_all(&buf[..n]).map_err(|_| "The disk would not take more.".to_string())?;
        part.lock().unwrap().pos += n as u64;
        moved.fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// Fetches a whole file into `path.part` (resuming from its state file), then moves it into place and
/// marks it as from the internet. `progress` hears (bytes done, bytes per second) about twice a second.
pub fn fetch(url: &str, referer: &str, cookies: &str, path: &Path, stop: Arc<AtomicBool>, progress: &dyn Fn(u64, u64, u32, &str)) -> Result<bool, String> {
    let probe = probe(url, referer, cookies)?;
    if probe.page {
        return Err("The server sent a web page, not a file.".into());
    }
    let part_path = part_of(path);
    let state_path = state_of(path);
    let saved: Option<Saved> = std::fs::read_to_string(&state_path).ok().and_then(|t| serde_json::from_str(&t).ok());
    let mut note = "";
    let mut plan = match saved {
        Some(s) if s.url == url && part_path.exists() && !must_restart(&s.etag, &probe.etag, probe.ranges) && s.size == probe.size => s,
        Some(_) => {
            note = "The file changed on the server - started again.";
            Saved { url: url.into(), size: probe.size, etag: probe.etag.clone(), ranges: probe.ranges, parts: Vec::new() }
        }
        None => Saved { url: url.into(), size: probe.size, etag: probe.etag.clone(), ranges: probe.ranges, parts: Vec::new() },
    };
    if plan.parts.is_empty() {
        plan.parts = match (plan.ranges, plan.size) {
            (true, Some(size)) => split_parts(size, PARTS),
            _ => vec![Part::default()],
        };
        let f = std::fs::File::create(&part_path).map_err(|_| "The file could not be made.".to_string())?;
        if let (true, Some(size)) = (plan.ranges, plan.size) {
            f.set_len(size).map_err(|_| "There is not room on the disk.".to_string())?;
        }
    }
    let ranged = plan.ranges && plan.size.is_some();
    let c = client()?;
    let parts: Vec<Arc<Mutex<Part>>> = plan.parts.iter().map(|p| Arc::new(Mutex::new(*p))).collect();
    let moved = Arc::new(AtomicU64::new(0));
    let failed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let handles: Vec<_> = parts
        .iter()
        .cloned()
        .map(|part| {
            let (c, url, referer, cookies, file, stop, moved, failed) = (c.clone(), url.to_string(), referer.to_string(), cookies.to_string(), part_path.clone(), stop.clone(), moved.clone(), failed.clone());
            std::thread::spawn(move || {
                let mut tries = 0;
                loop {
                    match fetch_part(&c, &url, &referer, &cookies, &file, part.clone(), ranged, stop.clone(), moved.clone()) {
                        Ok(()) => return,
                        Err(e) if tries < 3 && ranged => {
                            tries += 1;
                            std::thread::sleep(Duration::from_secs(2 * tries));
                            let _ = e;
                        }
                        Err(e) => {
                            *failed.lock().unwrap() = Some(e);
                            stop.store(true, Ordering::Relaxed);
                            return;
                        }
                    }
                }
            })
        })
        .collect();
    let done_of = |parts: &[Arc<Mutex<Part>>]| -> u64 {
        parts.iter().map(|p| {
            let p = p.lock().unwrap();
            p.pos - p.start
        }).sum()
    };
    let save = |parts: &[Arc<Mutex<Part>>]| {
        let s = Saved { parts: parts.iter().map(|p| *p.lock().unwrap()).collect(), ..plan.clone() };
        if let Ok(t) = serde_json::to_string(&s) {
            let _ = std::fs::write(&state_path, t);
        }
    };
    let mut last = Instant::now();
    let mut last_moved = 0u64;
    while handles.iter().any(|h| !h.is_finished()) {
        std::thread::sleep(Duration::from_millis(250));
        if last.elapsed() >= Duration::from_millis(500) {
            let now = moved.load(Ordering::Relaxed);
            let speed = ((now - last_moved) as f64 / last.elapsed().as_secs_f64()) as u64;
            last_moved = now;
            last = Instant::now();
            save(&parts);
            progress(done_of(&parts), speed, parts.len() as u32, note);
        }
    }
    for h in handles {
        let _ = h.join();
    }
    save(&parts);
    if let Some(e) = failed.lock().unwrap().take() {
        return Err(e);
    }
    if stop.load(Ordering::Relaxed) {
        return Ok(false); // paused or cancelled: the state file stays for resume
    }
    let complete = match (ranged, plan.size) {
        (true, Some(_)) => parts.iter().all(|p| {
            let p = p.lock().unwrap();
            p.pos > p.end
        }),
        _ => true,
    };
    if !complete {
        return Err("The download stopped short.".into());
    }
    std::fs::rename(&part_path, path).map_err(|_| "The finished file could not be moved into place.".to_string())?;
    let _ = std::fs::remove_file(&state_path);
    mark_from_internet(path, url, referer);
    progress(done_of(&parts), 0, parts.len() as u32, note);
    Ok(true)
}

// ---------------------------------------------------------------------------------------------------
// The book: the list, the queue, the commands
// ---------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Book {
    items: Vec<Item>,
    next: u64,
    stops: Vec<(u64, Arc<AtomicBool>)>,
    folder: PathBuf,
}

fn book() -> &'static Mutex<Book> {
    static BOOK: OnceLock<Mutex<Book>> = OnceLock::new();
    BOOK.get_or_init(|| Mutex::new(Book::default()))
}

fn app() -> Option<&'static AppHandle> {
    APP.get()
}
static APP: OnceLock<AppHandle> = OnceLock::new();

fn tell() {
    let (items, folder) = {
        let b = book().lock().unwrap();
        (b.items.clone(), b.folder.clone())
    };
    // the browser's own window, and Castle's main window for the room inside it
    if let Some(app) = app() {
        let _ = app.emit_to("main", STATE_EVENT, &items);
        let _ = app.emit_to(crate::browser::UI, STATE_EVENT, &items);
    }
    if let Ok(text) = serde_json::to_string(&persisted(&items)) {
        let _ = std::fs::write(folder.join(LIST_FILE), text);
    }
}

fn update(id: u64, f: impl FnOnce(&mut Item)) {
    if let Some(i) = book().lock().unwrap().items.iter_mut().find(|i| i.id == id) {
        f(i);
    }
}

/// The list from disk at start (a download that was running is paused), and the app kept for the events.
pub fn start(app: &AppHandle, settings_folder: PathBuf) {
    let _ = APP.set(app.clone());
    let items: Vec<Item> = std::fs::read_to_string(settings_folder.join(LIST_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let items = persisted(&items);
    let mut b = book().lock().unwrap();
    b.next = items.iter().map(|i| i.id).max().unwrap_or(0);
    b.items = items;
    b.folder = settings_folder;
}

/// The folder a new file lands in: the member's folder, and its type's folder when sorting is on.
fn folder_for(app: &AppHandle, name: &str) -> PathBuf {
    let s = app.state::<crate::settings::SettingsStore>().get();
    let base = s.downloads_folder.clone().map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()).or_else(|| app.path().download_dir().ok()).unwrap_or_else(|| PathBuf::from("."));
    let folder = if s.downloads_sort { base.join(category_of(name)) } else { base };
    let _ = std::fs::create_dir_all(&folder);
    folder
}

/// The folder a file of this name lands in (for the video runner).
pub fn folder_of(app: &AppHandle, name: &str) -> PathBuf {
    folder_for(app, name)
}

/// A new download in the queue (a pasted link, or one a page started).
pub fn add(app: &AppHandle, url: &str, referer: &str, cookies: &str, private: bool) -> Result<u64, String> {
    if !crate::browser_tabs::may_load(url) || url == "about:blank" {
        return Err("That is not a web address.".into());
    }
    let name = name_from(None, url);
    let id = {
        let mut b = book().lock().unwrap();
        b.next += 1;
        let id = b.next;
        b.items.insert(0, Item {
            id,
            url: url.into(),
            name: name.clone(),
            category: category_of(&name).into(),
            state: "queued".into(),
            private,
            referer: referer.into(),
            cookies: cookies.into(),
            kind: "file".into(),
            pausable: true,
            ..Item::default()
        });
        id
    };
    tell();
    pump(app);
    Ok(id)
}

/// Starts queued downloads while fewer than three run.
fn pump(app: &AppHandle) {
    loop {
        let next = {
            let b = book().lock().unwrap();
            if b.items.iter().filter(|i| i.state == "running").count() >= RUNNING {
                return;
            }
            b.items.iter().rev().find(|i| i.state == "queued").cloned()
        };
        let Some(item) = next else { return };
        update(item.id, |i| i.state = "running".into());
        let app = app.clone();
        std::thread::spawn(move || run(&app, item));
    }
}

const DANGEROUS: &str = "This address is dangerous - it is not downloaded.";

/// The browser's guard (what it keeps in memory) and, when the address hashes to a prefix on Google's
/// lists, Google's own answer. Blocks on the network for a listed prefix: call it off the main thread.
fn is_dangerous(url: &str) -> bool {
    let host = url.parse::<tauri::Url>().ok().and_then(|u| u.host_str().and_then(crate::site_rules::clean_host));
    if let Some(host) = &host {
        if crate::browser_guard::check(url, host).is_some() {
            return true;
        }
    }
    if !crate::browser_safe::local_hits(url).is_empty() && crate::browser_safe::confirm(url, crate::key::load(crate::browser_safe::KEY).as_deref()).is_some() {
        return true;
    }
    false
}

/// One download, from the guard to the finished file.
fn run(app: &AppHandle, item: Item) {
    let stop = Arc::new(AtomicBool::new(false));
    book().lock().unwrap().stops.push((item.id, stop.clone()));
    let result = (|| -> Result<bool, String> {
        // the guards first: a dangerous address never downloads
        if is_dangerous(&item.url) {
            return Err(DANGEROUS.into());
        }
        if item.kind == "video" {
            return crate::downloads_video::run(app, &item, stop.clone());
        }
        let path = if item.path.is_empty() {
            let p = probe(&item.url, &item.referer, &item.cookies)?;
            let name = if p.name.is_empty() { item.name.clone() } else { p.name };
            let path = unique_path(&folder_for(app, &name), &name);
            update(item.id, |i| {
                i.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name.clone());
                i.category = category_of(&i.name).into();
                i.path = path.display().to_string();
                i.size = p.size;
            });
            path
        } else {
            PathBuf::from(&item.path)
        };
        let id = item.id;
        fetch(&item.url, &item.referer, &item.cookies, &path, stop.clone(), &|done, speed, parts, note| {
            update(id, |i| {
                i.done = done;
                i.speed = speed;
                i.parts = parts;
                if !note.is_empty() {
                    i.line = note.into();
                }
            });
            tell();
        })
    })();
    book().lock().unwrap().stops.retain(|(i, _)| *i != item.id);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    update(item.id, |i| match &result {
        Ok(true) => {
            i.state = "done".into();
            i.speed = 0;
            i.finished_at = now;
            if let Some(s) = i.size {
                i.done = s;
            }
        }
        Ok(false) => {
            if i.state != "removed" {
                i.state = "paused".into();
            }
            i.speed = 0;
        }
        Err(e) => {
            i.state = "failed".into();
            i.speed = 0;
            i.line = e.clone();
        }
    });
    // refused with the page's cookies: the page itself takes it (its own card replaces this one)
    if let Err(e) = &result {
        if item.kind == "file" && needs_native(e) && fallback(app, &item) {
            update(item.id, |i| i.state = "removed".into());
        }
    }
    book().lock().unwrap().items.retain(|i| i.state != "removed");
    tell();
    pump(app);
}

// ---------- the webview's own downloads: made inside a page, or Castle's fetch was refused ----------

/// Addresses the next webview download may take itself (the fallback), once each.
static NATIVE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Takes an address's one native pass.
pub fn native_allowed(url: &str) -> bool {
    let mut n = NATIVE.lock().unwrap();
    match n.iter().position(|u| u == url) {
        Some(i) => {
            n.remove(i);
            true
        }
        None => false,
    }
}

/// A download the webview does itself, into the member's folder: its card (no pause), and where the file goes.
pub fn begin_native(app: &AppHandle, url: &str, suggested: &str, referer: &str, private: bool, tab: u32) -> PathBuf {
    let name = if suggested.trim().is_empty() { name_from(None, url) } else { safe_name(suggested) };
    let path = unique_path(&folder_for(app, &name), &name);
    let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
    add_item(app, Item {
        url: url.into(),
        name: shown.clone(),
        path: path.display().to_string(),
        category: category_of(&shown).into(),
        state: "native".into(),
        private,
        referer: referer.into(),
        kind: "file".into(),
        pausable: false,
        tab,
        parts: 1,
        line: "Saved by the page itself".into(),
        ..Item::default()
    });
    path
}

/// Addresses whose webview download Castle cancelled to fetch them itself: their Finished is ignored.
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn note_cancelled(url: &str) {
    CANCELLED.lock().unwrap().push(url.to_string());
}

/// The referrer a page download is fetched with (pure): the page it came from, but never the file's
/// own address (a file opened straight from the address bar has no page; some servers refuse a file
/// that names itself as its referrer) and never anything but a web page.
pub fn referer_for(page: &str, file: &str) -> String {
    let bare = |u: &str| u.split('#').next().unwrap_or("").trim_end_matches('/').to_string();
    if !(page.starts_with("https://") || page.starts_with("http://")) || bare(page) == bare(file) {
        String::new()
    } else {
        page.to_string()
    }
}

/// The webview's own download finished (or failed). The Finished of a try Castle cancelled is ignored,
/// and a "finished" with no file on disk is a failure, never a done card with nothing behind it.
pub fn finish_native(url: &str, success: bool) {
    {
        let mut cancelled = CANCELLED.lock().unwrap();
        if let Some(at) = cancelled.iter().position(|u| u == url) {
            cancelled.remove(at);
            return;
        }
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let id = book().lock().unwrap().items.iter().find(|i| i.url == url && i.state == "native").map(|i| i.id);
    if let Some(id) = id {
        set(id, |i| {
            let landed = success && std::path::Path::new(&i.path).is_file();
            i.state = if landed { "done" } else { "failed" }.into();
            i.finished_at = now;
            if landed {
                i.done = std::fs::metadata(&i.path).map(|m| m.len()).unwrap_or(0);
                i.size = Some(i.done);
            } else {
                i.line = "The page's own download did not finish.".into();
            }
        });
    }
}

/// Castle's fetch was refused: the page downloads it itself, once, into the same folder.
fn fallback(app: &AppHandle, item: &Item) -> bool {
    if item.tab == 0 {
        return false;
    }
    let Some(view) = app.get_webview(&crate::browser::tab_label(item.tab)) else { return false };
    let Ok(to) = item.url.parse() else { return false };
    NATIVE.lock().unwrap().push(item.url.clone());
    view.navigate(to).is_ok()
}

/// A download a page started: Castle fetches it with the page's cookies and address as the referrer.
pub fn add_from_page(app: &AppHandle, url: &str, referer: &str, cookies: &str, private: bool, tab: u32) -> Result<u64, String> {
    let id = add(app, url, referer, cookies, private)?;
    update(id, |i| i.tab = tab);
    Ok(id)
}

fn stop_of(id: u64) -> Option<Arc<AtomicBool>> {
    book().lock().unwrap().stops.iter().find(|(i, _)| *i == id).map(|(_, s)| s.clone())
}

#[tauri::command]
pub fn downloads_list() -> Vec<Item> {
    book().lock().unwrap().items.clone()
}

/// A pasted link (never private: the paste field is not a tab).
#[tauri::command]
pub async fn download_add(app: AppHandle, url: String) -> Result<u64, String> {
    let url = url.trim().to_string();
    add(&app, &url, "", "", false)
}

#[tauri::command]
pub fn download_pause(id: u64) -> Result<(), String> {
    if let Some(s) = stop_of(id) {
        s.store(true, Ordering::Relaxed);
    }
    update(id, |i| {
        if i.state == "queued" {
            i.state = "paused".into();
        }
    });
    tell();
    Ok(())
}

#[tauri::command]
pub fn download_resume(app: AppHandle, id: u64) -> Result<(), String> {
    update(id, |i| {
        if i.state == "paused" || i.state == "failed" {
            i.state = "queued".into();
            i.line.clear();
        }
    });
    tell();
    pump(&app);
    Ok(())
}

/// Cancel: it stops, its `.part` and state file go, and it leaves the list.
#[tauri::command]
pub fn download_cancel(id: u64) -> Result<(), String> {
    let path = book().lock().unwrap().items.iter().find(|i| i.id == id).map(|i| i.path.clone()).unwrap_or_default();
    match stop_of(id) {
        Some(s) => {
            s.store(true, Ordering::Relaxed);
            update(id, |i| i.state = "removed".into());
        }
        None => book().lock().unwrap().items.retain(|i| i.id != id),
    }
    if !path.is_empty() {
        let p = PathBuf::from(&path);
        let _ = std::fs::remove_file(part_of(&p));
        let _ = std::fs::remove_file(state_of(&p));
    }
    tell();
    Ok(())
}

/// Removed from the list only (the file stays).
#[tauri::command]
pub fn download_remove(id: u64) -> Result<(), String> {
    book().lock().unwrap().items.retain(|i| i.id != id || i.state == "running");
    tell();
    Ok(())
}

#[tauri::command]
pub fn download_open(id: u64) -> Result<(), String> {
    let path = book().lock().unwrap().items.iter().find(|i| i.id == id && i.state == "done").map(|i| i.path.clone()).ok_or("That download is not finished.")?;
    open_file(&path)
}

/// The file in its folder, selected (Explorer on Windows, Finder on macOS).
#[tauri::command]
pub fn download_show(id: u64) -> Result<(), String> {
    let path = book().lock().unwrap().items.iter().find(|i| i.id == id).map(|i| i.path.clone()).ok_or("That download is gone.")?;
    show_file(&path)
}

/// Opens a finished file with the program the system picks for it.
fn open_file(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    let started = std::process::Command::new("explorer.exe").arg(path).spawn(); // platform: windows-only
    #[cfg(not(windows))]
    let started = std::process::Command::new("open").arg(path).spawn();
    started.map(|_| ()).map_err(|_| "The file could not be opened.".to_string())
}

/// Shows a file selected in its folder.
fn show_file(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    let started = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn(); // platform: windows-only
    #[cfg(not(windows))]
    let started = std::process::Command::new("open").arg("-R").arg(path).spawn();
    started.map(|_| ()).map_err(|_| "The folder could not be opened.".to_string())
}

/// Adds an item that is already decided (a video, or the webview's own download) and runs the queue.
pub fn add_item(app: &AppHandle, mut item: Item) -> u64 {
    let id = {
        let mut b = book().lock().unwrap();
        b.next += 1;
        item.id = b.next;
        let id = item.id;
        b.items.insert(0, item);
        id
    };
    tell();
    pump(app);
    id
}

/// Changes one item and tells the room (for the video runner and the webview's own downloads).
pub fn set(id: u64, f: impl FnOnce(&mut Item)) {
    update(id, f);
    tell();
}

#[cfg(test)]
mod referer_tests {
    use super::*;

    #[test]
    fn a_file_is_never_its_own_referrer() {
        let file = "http://ipv4.download.thinkbroadband.com/1GB.zip";
        assert_eq!(referer_for(file, file), "", "opened straight from the address bar: no referrer");
        assert_eq!(referer_for(&format!("{file}#x"), file), "");
        assert_eq!(referer_for("https://www.thinkbroadband.com/download", file), "https://www.thinkbroadband.com/download");
        assert_eq!(referer_for("about:blank", file), "");
        assert_eq!(referer_for("", file), "");
    }

    #[test]
    fn a_cancelled_tries_finish_never_ends_a_native_card() {
        note_cancelled("https://x.example/a.zip");
        // the cancelled try's Finished is swallowed (no card, nothing to change, no panic)
        finish_native("https://x.example/a.zip", true);
        assert!(!CANCELLED.lock().unwrap().iter().any(|u| u == "https://x.example/a.zip"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A small file server for the tests: ranges (unless `ranges` is false) and an ETag.
    fn serve(body: Vec<u8>, etag: &'static str, ranges: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let body = body.clone();
                std::thread::spawn(move || {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = s.read(&mut buf) else { return };
                        if n == 0 {
                            return;
                        }
                        req.extend_from_slice(&buf[..n]);
                    }
                    let text = String::from_utf8_lossy(&req).to_lowercase();
                    let range = text.lines().find_map(|l| l.strip_prefix("range: bytes=")).map(|r| r.trim().to_string());
                    let (status, from, to) = match (ranges, range) {
                        (true, Some(r)) => {
                            let (a, b) = r.split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() { body.len() - 1 } else { b.parse::<usize>().unwrap().min(body.len() - 1) };
                            (206, a, b)
                        }
                        _ => (200, 0, body.len() - 1),
                    };
                    let chunk = &body[from..=to];
                    let mut head = format!("HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nETag: {etag}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n", chunk.len());
                    if status == 206 {
                        head.push_str(&format!("Content-Range: bytes {from}-{to}/{}\r\n", body.len()));
                    }
                    head.push_str("\r\n");
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(chunk);
                });
            }
        });
        format!("http://{addr}/files/test.bin")
    }

    fn body(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("castle-dl-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_big_file_splits_into_parts_and_a_small_one_does_not() {
        let p = split_parts(10 * PART_MIN, PARTS);
        assert_eq!(p.len(), 8);
        assert_eq!((p[0].start, p[7].end), (0, 10 * PART_MIN - 1));
        assert!(p.windows(2).all(|w| w[0].end + 1 == w[1].start));
        assert_eq!(split_parts(PART_MIN / 2, PARTS).len(), 1);
        assert_eq!(split_parts(3 * PART_MIN, PARTS).len(), 3);
    }

    #[test]
    fn a_file_downloads_in_parts_and_comes_out_whole_and_marked() {
        let data = body(3 * PART_MIN as usize + 12345);
        let url = serve(data.clone(), "\"v1\"", true);
        let dir = temp("parts");
        let path = dir.join("test.bin");
        let done = fetch(&url, "https://example.com/page", "", &path, Arc::new(AtomicBool::new(false)), &|_, _, _, _| {}).unwrap();
        assert!(done);
        assert_eq!(std::fs::read(&path).unwrap(), data);
        assert!(!part_of(&path).exists() && !state_of(&path).exists());
        #[cfg(windows)]
        {
            let zone = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display())).unwrap_or_default();
            assert!(zone.contains("ZoneId=3"), "Windows' mark is written (NTFS)");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_download_resumes_from_its_saved_bytes() {
        let data = body(2 * PART_MIN as usize + 999);
        let url = serve(data.clone(), "\"v1\"", true);
        let dir = temp("resume");
        let path = dir.join("test.bin");
        // half of each part was fetched before a pause: the rest comes on resume
        let mut parts = split_parts(data.len() as u64, PARTS);
        let mut f = std::fs::File::create(part_of(&path)).unwrap();
        f.set_len(data.len() as u64).unwrap();
        for p in parts.iter_mut() {
            let half = p.start + (p.end - p.start) / 2;
            f.seek(SeekFrom::Start(p.start)).unwrap();
            f.write_all(&data[p.start as usize..half as usize]).unwrap();
            p.pos = half;
        }
        drop(f);
        let saved = Saved { url: url.clone(), size: Some(data.len() as u64), etag: "\"v1\"".into(), ranges: true, parts };
        std::fs::write(state_of(&path), serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(fetch(&url, "", "", &path, Arc::new(AtomicBool::new(false)), &|_, _, _, _| {}).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), data);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_server_without_ranges_is_one_stream_and_a_changed_file_starts_over() {
        let data = body(PART_MIN as usize * 2);
        let url = serve(data.clone(), "\"v2\"", false);
        let dir = temp("noranges");
        let path = dir.join("test.bin");
        // a state file from an older copy of the file: started over
        let old = Saved { url: url.clone(), size: Some(5), etag: "\"v1\"".into(), ranges: true, parts: vec![Part { start: 0, end: 4, pos: 3 }] };
        std::fs::write(state_of(&path), serde_json::to_string(&old).unwrap()).unwrap();
        std::fs::write(part_of(&path), b"abc").unwrap();
        assert!(fetch(&url, "", "", &path, Arc::new(AtomicBool::new(false)), &|_, _, _, _| {}).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), data);
        assert!(must_restart("\"v1\"", "\"v2\"", true));
        assert!(must_restart("", "", false));
        assert!(!must_restart("\"v1\"", "\"v1\"", true));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn files_sort_by_type_and_names_are_safe_and_never_overwrite() {
        assert_eq!(category_of("Sunset_Drive_4K.mp4"), "Video");
        assert_eq!(category_of("Lofi.MP3"), "Music");
        assert_eq!(category_of("Design_Spec.pdf"), "Documents");
        assert_eq!(category_of("Castle_Setup.exe"), "Programs");
        assert_eq!(category_of("ubuntu.iso"), "Compressed");
        assert_eq!(category_of("photos.zip"), "Compressed");
        assert_eq!(category_of("thing"), "Other");
        assert_eq!(safe_name("..\\..\\evil:name?.exe"), "evil_name_.exe");
        assert_eq!(safe_name("CON.txt"), "download-CON.txt");
        assert_eq!(safe_name(""), "download");
        assert_eq!(name_from(Some("attachment; filename=\"Report Q3.pdf\""), "https://x.com/dl?id=1"), "Report Q3.pdf");
        assert_eq!(name_from(Some("attachment; filename*=UTF-8''%E6%97%A5%E6%9C%AC.zip"), "https://x.com/a"), "日本.zip");
        assert_eq!(name_from(None, "https://x.com/files/My%20File.pdf?sig=abc"), "My File.pdf");
        let dir = temp("unique");
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.pdf"), dir.join("a (1).pdf"));
        assert_eq!(unique_path(&dir, "b.pdf"), dir.join("b.pdf"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_pages_cookies_travel_and_a_refusal_falls_back_to_the_page() {
        let pairs = vec![("sid".to_string(), "abc".to_string()), ("lang".to_string(), "en".to_string()), (String::new(), "x".to_string())];
        assert_eq!(cookie_header(&pairs), "sid=abc; lang=en");
        assert!(needs_native("The server refused the download (403)."));
        assert!(needs_native("The server sent a web page, not a file."));
        assert!(!needs_native("The line dropped."));
        NATIVE.lock().unwrap().push("https://x.com/f.zip".into());
        assert!(native_allowed("https://x.com/f.zip"));
        assert!(!native_allowed("https://x.com/f.zip")); // once
    }

    #[test]
    fn a_private_download_is_never_written_and_windows_mark_reads_right() {
        let items = vec![
            Item { id: 1, private: true, state: "done".into(), ..Item::default() },
            Item { id: 2, state: "running".into(), speed: 9, cookies: "sid=1".into(), ..Item::default() },
        ];
        let kept = persisted(&items);
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].id, kept[0].state.as_str(), kept[0].speed, kept[0].cookies.as_str()), (2, "paused", 0, ""));
        assert!(!serde_json::to_string(&items[1]).unwrap().contains("sid=1")); // cookies never written
        assert_eq!(zone_text("https://x.com/f.exe", "https://x.com/"), "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://x.com/\r\nHostUrl=https://x.com/f.exe\r\n");
    }
}
