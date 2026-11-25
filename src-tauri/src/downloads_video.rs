//! Videos from a link: YouTube, Threads and most video sites (ported from the Alice Keep's Phase 192
//! Task 31). Through **yt-dlp**, fetched once on the member's press from its own GitHub release with its
//! SHA-256 checked, kept in Castle's local data folder (`tools`) and updated once a week; **ffmpeg** (on
//! the member's PATH) joins video and sound. The chooser shows the qualities that video really has; the
//! last pick is remembered. DRM-protected video is never attempted (yt-dlp refuses it). Threads: yt-dlp
//! first; when it cannot, the open page's own `<video>` address is downloaded instead. YouTube's terms
//! allow downloading only through its own app: this is for the member's own videos, ones they have the
//! right to keep, and offline viewing.

use crate::downloads::{self, Item};
use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::time::SystemTime;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

const RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
/// The release's file for this system.
#[cfg(windows)]
const TOOL: &str = "yt-dlp.exe";
#[cfg(not(windows))]
const TOOL: &str = "yt-dlp_macos";
/// The progress lines Castle asks yt-dlp for, and the line naming the finished file.
const PROGRESS: &str = "CASTLE ";
const FINISHED: &str = "CASTLEDONE ";

// ---------- pure rules (tested) ----------

/// The qualities a video really has, from yt-dlp's `-J` answer: the heights with a video stream (best
/// first, as 4K / 1440p / 1080p / 720p / 480p / 360p), then Audio MP3 when it has sound.
pub fn choices_of(info: &Value) -> Vec<String> {
    let formats = info["formats"].as_array().cloned().unwrap_or_default();
    let mut heights: Vec<u64> = formats.iter().filter(|f| f["vcodec"].as_str().is_some_and(|v| v != "none")).filter_map(|f| f["height"].as_u64()).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    let mut out: Vec<String> = Vec::new();
    for h in heights {
        let label = match h {
            h if h >= 2000 => "4K".to_string(),
            h if h >= 1400 => "1440p".to_string(),
            h if h >= 1000 => "1080p".to_string(),
            h if h >= 700 => "720p".to_string(),
            h if h >= 460 => "480p".to_string(),
            h if h >= 340 => "360p".to_string(),
            _ => continue,
        };
        if !out.contains(&label) {
            out.push(label);
        }
    }
    let audio = formats.iter().any(|f| f["acodec"].as_str().is_some_and(|a| a != "none"));
    if audio {
        out.push("Audio MP3".into());
    }
    out
}

/// The highlighted choice: the last pick when this video has it, else 1080p or the best below it.
pub fn default_pick(choices: &[String], last: Option<&str>) -> String {
    if let Some(l) = last.filter(|l| choices.iter().any(|c| c == l)) {
        return l.to_string();
    }
    let order = ["1080p", "720p", "1440p", "4K", "480p", "360p", "Audio MP3"];
    order.iter().find(|o| choices.iter().any(|c| c == *o)).map(|s| s.to_string()).unwrap_or_default()
}

/// yt-dlp's arguments for a choice.
pub fn format_args(choice: &str) -> Vec<String> {
    let height = match choice {
        "4K" => Some(2160),
        "1440p" => Some(1440),
        "1080p" => Some(1080),
        "720p" => Some(720),
        "480p" => Some(480),
        "360p" => Some(360),
        _ => None,
    };
    match height {
        Some(h) => vec!["-f".into(), format!("bv*[height<={h}]+ba/b[height<={h}]"), "--merge-output-format".into(), "mp4".into()],
        None => vec!["-x".into(), "--audio-format".into(), "mp3".into()],
    }
}

/// One progress line as (bytes done, bytes in all, bytes per second); None for any other line.
pub fn progress_of(line: &str) -> Option<(u64, Option<u64>, u64)> {
    let rest = line.trim().strip_prefix(PROGRESS)?;
    let mut parts = rest.split_whitespace();
    let num = |s: Option<&str>| s.and_then(|v| v.parse::<f64>().ok()).map(|v| v as u64);
    let done = num(parts.next())?;
    let total = num(parts.next());
    let speed = num(parts.next()).unwrap_or(0);
    Some((done, total, speed))
}

/// The SHA-256 a release lists for the tool, from its `SHA2-256SUMS` file.
pub fn sum_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (hash, name) = (it.next()?, it.next()?);
        (name.trim_start_matches('*') == file).then(|| hash.to_ascii_lowercase())
    })
}

/// The open page's own video address: a real http(s) file, never a `blob:` stream; an `.mp4` first.
pub fn pick_video_src(sources: &[String]) -> Option<String> {
    let web: Vec<&String> = sources.iter().filter(|s| s.starts_with("https://") || s.starts_with("http://")).collect();
    web.iter().find(|s| s.split('?').next().unwrap_or("").ends_with(".mp4")).or_else(|| web.first()).map(|s| s.to_string())
}

// ---------- the tool ----------

fn tool_path(app: &AppHandle) -> PathBuf {
    app.path().app_local_data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("tools").join(TOOL)
}

fn ffmpeg() -> Option<PathBuf> {
    crate::env::find_on_path("ffmpeg").filter(|p| p.is_file())
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for Command {
    /// No console window flashes up on Windows; nothing to do elsewhere.
    fn creation_flags_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x0800_0000);
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolState {
    pub installed: bool,
    pub ffmpeg: bool,
}

#[tauri::command]
pub fn video_tool_state(app: AppHandle) -> ToolState {
    ToolState { installed: tool_path(&app).is_file(), ffmpeg: ffmpeg().is_some() }
}

/// Get the video tool (the member's press): yt-dlp's own release, its SHA-256 checked before it is kept.
#[tauri::command]
pub async fn video_tool_get(app: AppHandle) -> Result<ToolState, String> {
    let path = tool_path(&app);
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let c = reqwest::blocking::Client::builder().timeout(Duration::from_secs(300)).build().map_err(|e| e.to_string())?;
        let sums = c.get(format!("{RELEASE}/SHA2-256SUMS")).send().and_then(|r| r.error_for_status()).and_then(|r| r.text()).map_err(|_| "The tool's checksums could not be fetched.".to_string())?;
        let want = sum_for(&sums, TOOL).ok_or("The release did not list the tool.")?;
        let bytes = c.get(format!("{RELEASE}/{TOOL}")).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()).map_err(|_| "The tool could not be fetched.".to_string())?;
        if !crate::fetch::matches(&bytes, &want) {
            return Err("The tool did not match its checksum, so it was not kept.".into());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|_| "The tools folder could not be made.".to_string())?;
        }
        std::fs::write(&path, &bytes).map_err(|_| "The tool could not be saved.".to_string())?;
        // macOS: the download is a program only once it may run
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|_| "The tool could not be made runnable.".to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|_| "The tool could not be fetched.".to_string())??;
    Ok(video_tool_state(app))
}

/// Once a week the tool updates itself before a use.
fn keep_fresh(tool: &Path) {
    let old = std::fs::metadata(tool).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(7 * 86_400));
    if old {
        let _ = Command::new(tool).arg("-U").creation_flags_no_window().output();
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Formats {
    pub title: String,
    pub choices: Vec<String>,
    pub pick: String,
}

/// The qualities this video has, and the one to highlight.
#[tauri::command]
pub async fn video_formats(app: AppHandle, url: String, store: State<'_, crate::settings::SettingsStore>) -> Result<Formats, String> {
    let tool = tool_path(&app);
    if !tool.is_file() {
        return Err("Get the video tool first.".into());
    }
    let last = store.get().downloads_video_pick;
    let json = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        keep_fresh(&tool);
        let out = Command::new(&tool).args(["-J", "--no-playlist", "--no-warnings", &url]).creation_flags_no_window().output().map_err(|_| "The video tool could not start.".to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(if err.contains("DRM") { "This video is DRM-protected - it cannot be downloaded.".into() } else { "The video tool could not read this link.".into() });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    })
    .await
    .map_err(|_| "The video tool could not start.".to_string())??;
    let info: Value = serde_json::from_str(&json).map_err(|_| "The video tool answered in a shape I do not know.".to_string())?;
    let choices = choices_of(&info);
    if choices.is_empty() {
        return Err("No downloadable video was found at this link.".into());
    }
    let pick = default_pick(&choices, last.as_deref());
    Ok(Formats { title: info["title"].as_str().unwrap_or("Video").chars().take(150).collect(), choices, pick })
}

/// A video in the queue at the chosen quality (the pick remembered for next time).
#[tauri::command]
pub async fn video_add(app: AppHandle, url: String, choice: String, title: String, store: State<'_, crate::settings::SettingsStore>) -> Result<u64, String> {
    if !crate::browser_tabs::may_load(&url) || url == "about:blank" {
        return Err("That is not a web address.".into());
    }
    {
        let mut next = store.get();
        next.downloads_video_pick = Some(choice.clone());
        if crate::settings::save(&store.path, &next).is_ok() {
            *store.current.lock().unwrap() = next;
        }
    }
    let ext = if choice == "Audio MP3" { "mp3" } else { "mp4" };
    let name = format!("{}.{ext}", downloads::safe_name(&title));
    Ok(downloads::add_item(&app, Item {
        url,
        name: name.clone(),
        category: downloads::category_of(&name).into(),
        state: "queued".into(),
        kind: "video".into(),
        format: choice,
        pausable: true,
        parts: 1,
        ..Item::default()
    }))
}

/// The open Threads page's own video, when yt-dlp cannot read the post: its address as a file download.
#[tauri::command]
pub async fn video_from_page(app: AppHandle, id: u32) -> Result<u64, String> {
    let view = app.get_webview(&crate::browser::tab_label(id)).ok_or("This tab has no page yet.")?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    view.eval_with_callback("[...document.querySelectorAll('video')].flatMap((v) => [v.currentSrc, v.src, ...[...v.querySelectorAll('source')].map((s) => s.src)]).filter(Boolean)", move |json| {
        let _ = tx.send(json);
    })
    .map_err(|_| "The page did not answer.".to_string())?;
    let json = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(5))).await.map_err(|_| "The page did not answer.".to_string())?.map_err(|_| "The page took too long.".to_string())?;
    let sources: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
    let src = pick_video_src(&sources).ok_or("No downloadable video is on this page (it streams in pieces).")?;
    let (private, referer) = app.state::<crate::browser::Browser>().0.lock().unwrap().list.iter().find(|t| t.id == id).map(|t| (t.private, t.url.clone().unwrap_or_default())).unwrap_or((false, String::new()));
    downloads::add_from_page(&app, &src, &referer, "", private, id)
}

/// Runs one video download with yt-dlp; `stop` ends the process (resume runs it again and it continues).
pub fn run(app: &AppHandle, item: &Item, stop: Arc<AtomicBool>) -> Result<bool, String> {
    let tool = tool_path(app);
    if !tool.is_file() {
        return Err("Get the video tool first.".into());
    }
    let ff = ffmpeg();
    if ff.is_none() {
        return Err("ffmpeg was not found - it joins the picture and the sound.".into());
    }
    let sample = if item.format == "Audio MP3" { "a.mp3" } else { "a.mp4" };
    let folder = downloads::folder_of(app, sample);
    let started = SystemTime::now();
    let mut cmd = Command::new(&tool);
    // UTF-8 both ways: a title with a character the console's code page lacks ("｜") comes back whole
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .args(format_args(&item.format))
        .args(["--no-playlist", "--newline", "--progress", "--no-simulate", "--no-warnings", "--continue", "--no-mtime", "--encoding", "utf-8"])
        .arg("--progress-template")
        .arg(format!("download:{PROGRESS}%(progress.downloaded_bytes)s %(progress.total_bytes,progress.total_bytes_estimate)s %(progress.speed)s"))
        .arg("--print")
        .arg(format!("after_move:{FINISHED}%(filepath)s"))
        .arg("--ffmpeg-location")
        .arg(ff.unwrap_or_default())
        .arg("-o")
        .arg(folder.join("%(title).150B.%(ext)s"))
        .arg(&item.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags_no_window();
    let mut child = cmd.spawn().map_err(|_| "The video tool could not start.".to_string())?;
    let stdout = child.stdout.take().ok_or("The video tool gave no answer.")?;
    let id = item.id;
    let final_path = Arc::new(std::sync::Mutex::new(None::<String>));
    let fp = final_path.clone();
    let reader = std::thread::spawn(move || {
        // bytes, never lines: one line the decoder dislikes must not end the reading
        for raw in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_string();
            if let Some((done, total, speed)) = progress_of(&line) {
                downloads::set(id, |i| {
                    i.done = done;
                    if total.is_some() {
                        i.size = total;
                    }
                    i.speed = speed;
                });
            } else if let Some(path) = line.trim().strip_prefix(FINISHED) {
                *fp.lock().unwrap() = Some(path.to_string());
            }
        }
    });
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = reader.join();
            return Ok(false);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = reader.join();
                if !status.success() {
                    return Err("The video tool could not download this (it may be private, removed or DRM-protected).".into());
                }
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(300)),
            Err(_) => return Err("The video tool stopped.".into()),
        }
    }
    let reported = final_path.lock().unwrap().clone();
    let path = finished_file(reported.as_deref(), &folder, started).ok_or("The video tool finished, but its file was not found in the folder.")?;
    downloads::mark_from_internet(&path, &item.url, "");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    downloads::set(id, |i| {
        i.path = path.display().to_string();
        if !name.is_empty() {
            i.name = name.clone();
            i.category = downloads::category_of(&name).into();
        }
        i.done = size;
        i.size = Some(size);
    });
    Ok(true)
}

/// The file a video run left: the path the tool named when it is really there, else the newest
/// finished file it wrote in the folder (never a half file, never an empty one).
pub fn finished_file(reported: Option<&str>, folder: &Path, since: SystemTime) -> Option<PathBuf> {
    let real = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0);
    if let Some(r) = reported.map(str::trim).filter(|r| !r.is_empty()) {
        let p = PathBuf::from(r);
        if real(&p) {
            return Some(p);
        }
    }
    let since = since.checked_sub(Duration::from_secs(2)).unwrap_or(since);
    let half = ["part", "ytdl", "json", "temp", "tmp"];
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| real(p))
        .filter(|p| !p.extension().and_then(|e| e.to_str()).is_some_and(|e| half.contains(&e.to_ascii_lowercase().as_str())))
        .filter_map(|p| std::fs::metadata(&p).and_then(|m| m.modified()).ok().filter(|t| *t >= since).map(|t| (t, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_file_is_found_even_when_the_tool_names_it_wrongly() {
        let dir = std::env::temp_dir().join(format!("castle-video-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let before = SystemTime::now();
        let real = dir.join("Sparkle \u{ff5c} Your Name AMV.mp4");
        std::fs::write(&real, b"video").unwrap();
        std::fs::write(dir.join("Sparkle \u{ff5c} Your Name AMV.mp4.part"), b"half").unwrap();
        std::fs::write(dir.join("empty.mp4"), b"").unwrap();
        // the name it gave, read in the wrong code page, is not there: the newest real file is
        let wrong = dir.join("Sparkle ? Your Name AMV.mp4").display().to_string();
        assert_eq!(finished_file(Some(&wrong), &dir, before), Some(real.clone()));
        // the name it gave, when it is there, wins
        assert_eq!(finished_file(Some(&real.display().to_string()), &dir, before), Some(real.clone()));
        // nothing new since the run began: nothing
        let later = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(finished_file(None, &dir, later), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_chooser_shows_the_qualities_a_video_really_has() {
        let info = serde_json::json!({ "title": "x", "formats": [
            { "vcodec": "avc1", "acodec": "none", "height": 2160 },
            { "vcodec": "avc1", "acodec": "none", "height": 1080 },
            { "vcodec": "vp9", "acodec": "none", "height": 1080 },
            { "vcodec": "avc1", "acodec": "none", "height": 720 },
            { "vcodec": "none", "acodec": "opus" },
            { "vcodec": "avc1", "acodec": "none", "height": 144 },
        ]});
        assert_eq!(choices_of(&info), vec!["4K", "1080p", "720p", "Audio MP3"]);
        assert!(choices_of(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn his_last_pick_is_remembered_when_the_video_has_it() {
        let c: Vec<String> = ["4K", "1080p", "720p", "Audio MP3"].iter().map(|s| s.to_string()).collect();
        assert_eq!(default_pick(&c, Some("720p")), "720p");
        assert_eq!(default_pick(&c, Some("480p")), "1080p");
        assert_eq!(default_pick(&c, None), "1080p");
        let small: Vec<String> = vec!["480p".into(), "Audio MP3".into()];
        assert_eq!(default_pick(&small, None), "480p");
    }

    #[test]
    fn a_choice_becomes_the_tools_arguments_and_its_progress_is_read() {
        assert_eq!(format_args("1080p"), vec!["-f", "bv*[height<=1080]+ba/b[height<=1080]", "--merge-output-format", "mp4"]);
        assert_eq!(format_args("Audio MP3"), vec!["-x", "--audio-format", "mp3"]);
        assert_eq!(progress_of("CASTLE 1048576 10485760 524288.5"), Some((1_048_576, Some(10_485_760), 524_288)));
        assert_eq!(progress_of("CASTLE 2048 NA NA"), Some((2048, None, 0)));
        assert_eq!(progress_of("[youtube] abc: Downloading webpage"), None);
        assert_eq!(progress_of("CASTLEDONE C:/x.mp4"), None, "the finished line is not progress");
    }

    #[test]
    fn the_tool_is_checked_against_its_release_sum() {
        let sums = "aaaa1111  yt-dlp\nBBBB2222  yt-dlp.exe\ncccc  yt-dlp_macos\n";
        assert_eq!(sum_for(sums, "yt-dlp.exe").as_deref(), Some("bbbb2222"));
        assert_eq!(sum_for(sums, "yt-dlp_macos").as_deref(), Some("cccc"));
        assert_eq!(sum_for(sums, "nope.exe"), None);
    }

    #[test]
    fn a_threads_page_gives_its_real_file_never_a_blob() {
        let s = vec!["blob:https://www.threads.com/abc".to_string(), "https://scontent.cdninstagram.com/v/t66/video.m3u8".to_string(), "https://scontent.cdninstagram.com/v/t50/clip.mp4?efg=1".to_string()];
        assert_eq!(pick_video_src(&s).as_deref(), Some("https://scontent.cdninstagram.com/v/t50/clip.mp4?efg=1"));
        assert_eq!(pick_video_src(&["blob:https://x".to_string()]), None);
    }
}
