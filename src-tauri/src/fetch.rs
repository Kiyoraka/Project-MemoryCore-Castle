//! Downloading a pinned release once and putting it in place: the database engine (MariaDB) and
//! the helpers' Node both come this way. A download is kept only when its SHA-256 is exactly the
//! release's own, and only a whole unpack ever replaces the folder that was there.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The lowercase hex SHA-256 of some bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `bytes` are exactly the file whose SHA-256 is `expected` (lowercase hex).
pub fn matches(bytes: &[u8], expected: &str) -> bool {
    sha256_hex(bytes) == expected.to_ascii_lowercase()
}

/// Downloads `url` whole, reporting `(done, total)` about four times a second. `fallback_total` is
/// the size to report when the server does not say. `what` names the thing in the one error line.
pub fn download(url: &str, fallback_total: u64, what: &str, progress: impl Fn(u64, u64)) -> Result<Vec<u8>, String> {
    let offline = || format!("{what} could not be downloaded. Check the internet connection and try again.");
    let mut response = reqwest::blocking::Client::builder()
        .user_agent(concat!("MemoryCore-Castle/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(900))
        .build()
        .and_then(|c| c.get(url).send())
        .and_then(|r| r.error_for_status())
        .map_err(|_| offline())?;
    let total = response.content_length().unwrap_or(fallback_total);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut chunk = [0u8; 256 * 1024];
    let mut last = Instant::now();
    loop {
        let n = response.read(&mut chunk).map_err(|_| offline())?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if last.elapsed() > Duration::from_millis(250) {
            progress(bytes.len() as u64, total);
            last = Instant::now();
        }
    }
    progress(bytes.len() as u64, total);
    Ok(bytes)
}

/// How many tries in a row may end without one new byte before the download stops and waits for
/// the next press of Install. Tries that move forward do not count.
const STALLED_TRIES: u32 = 6;

/// How long one try may run. A slow server never loses anything to this: the bytes already on disk
/// stay, and the next try asks only for the rest.
const TRY_LIMIT: Duration = Duration::from_secs(120);

/// Throws away a half-finished download, because it is not the release or it is no longer wanted.
pub fn discard(part: &Path) {
    let _ = std::fs::remove_file(part);
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Like [`download`], but the bytes are kept in `part` on disk as they arrive, and a try that
/// stops (a dropped connection, a stalled server, Castle closed) is carried on from where it
/// stopped with a `Range` request, now or on the next press of Install. A slow server is fine: a try
/// that moved forward never counts against the retries, however many it takes. The caller checks the SHA-256
/// and calls [`discard`] once the release is unpacked, or when the checksum is wrong.
pub fn download_resumable(url: &str, expected: u64, what: &str, part: &Path, progress: impl Fn(u64, u64)) -> Result<Vec<u8>, String> {
    download_resumable_with(url, expected, what, part, Duration::from_secs(3), &progress)
}

fn download_resumable_with(url: &str, expected: u64, what: &str, part: &Path, pause: Duration, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
    let offline = || format!("{what} could not be downloaded. Check the internet connection and try again.");
    if let Some(parent) = part.parent() {
        std::fs::create_dir_all(parent).map_err(|_| format!("A folder for the {what} download could not be made."))?;
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("MemoryCore-Castle/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        .timeout(TRY_LIMIT)
        .build()
        .map_err(|_| offline())?;
    let mut stalled = 0u32;
    loop {
        let before = size_of(part);
        if before == expected {
            progress(expected, expected);
            break;
        }
        let answered = fetch_more(&client, url, expected, part, progress).is_ok();
        let after = size_of(part);
        // done only when the whole release is here: an answer that arrived in full can still be just
        // part of it (a bounded range, a short body)
        if after == expected {
            break;
        }
        if after > expected {
            // more than the release: it is not this release, so start again
            discard(part);
        } else if answered && after > before {
            // a complete answer for a part of what was missing: ask for the rest at once
            continue;
        }
        stalled = if after > before && after <= expected { 0 } else { stalled + 1 };
        if stalled >= STALLED_TRIES {
            return Err(format!(
                "The {what} download paused at {} of {} MB. Press Install again and it carries on from there.",
                size_of(part) / 1_000_000,
                expected / 1_000_000
            ));
        }
        std::thread::sleep(pause * stalled.max(1));
    }
    std::fs::read(part).map_err(|_| offline())
}

/// A `Content-Range` header read whole: `bytes 40-59/100` is (40, 59, Some(100)); a total the server
/// does not know (`*`) is None. Anything else that is not that shape is None.
fn parse_content_range(said: &str) -> Option<(u64, u64, Option<u64>)> {
    let (span, total) = said.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let total = if total.trim() == "*" { None } else { Some(total.trim().parse().ok()?) };
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?, total))
}

/// One try: asks for what `part` lacks and appends it. `Ok` only when the whole body of this answer
/// arrived; the caller decides whether that makes the whole release (an answer may hold only a part of it).
fn fetch_more(client: &reqwest::blocking::Client, url: &str, expected: u64, part: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), ()> {
    let mut have = size_of(part);
    if have > expected {
        discard(part);
        have = 0;
    }
    let mut request = client.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut response = request.send().map_err(|_| ())?;
    let status = response.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        discard(part);
        return Err(());
    }
    if !status.is_success() {
        return Err(());
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if resumed {
        let said = response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).unwrap_or("");
        // the whole header is read: where the piece starts and ends and how big the whole is. A piece
        // from somewhere else, of a different whole, or that runs past the release is never glued on.
        let fits = parse_content_range(said).is_some_and(|(from, to, total)| from == have && to >= from && to < expected && total.is_none_or(|t| t == expected));
        if !fits {
            discard(part);
            return Err(());
        }
    }
    // a plain 200 means the server ignored the Range: start over rather than glue a whole file on
    let start = if resumed { have } else { 0 };
    let mut file = if resumed { std::fs::OpenOptions::new().append(true).open(part) } else { std::fs::File::create(part) }.map_err(|_| ())?;
    let server_total = response.content_length().map(|n| start + n);
    // the bar runs to the end of the release, not to the end of this answer
    let shown_total = expected;
    let mut written = 0u64;
    let mut chunk = [0u8; 256 * 1024];
    let mut last = Instant::now();
    loop {
        let n = response.read(&mut chunk).map_err(|_| ())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &chunk[..n]).map_err(|_| ())?;
        written += n as u64;
        if last.elapsed() > Duration::from_millis(250) {
            progress(start + written, shown_total);
            last = Instant::now();
        }
    }
    progress(start + written, shown_total);
    match server_total {
        Some(total) if start + written != total => Err(()),
        _ => Ok(()),
    }
}

/// A release zip holds one top folder (`mariadb-11.4.13-winx64/`, `node-v22.x-win-x64/`); its
/// contents land in `target`, skipping what `keep` refuses. `what` names the folder in the error.
pub fn unzip_dropping_top(bytes: &[u8], target: &Path, keep: impl Fn(&Path) -> bool, what: &str) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "The download was not a readable zip.".to_string())?;
    let partial = target.with_extension("partial");
    let _ = std::fs::remove_dir_all(&partial);
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|_| "The zip could not be read.".to_string())?;
        let Some(inside) = file.enclosed_name() else { continue };
        let relative: PathBuf = inside.components().skip(1).collect();
        if relative.as_os_str().is_empty() || !keep(&relative) {
            continue;
        }
        let out = partial.join(relative);
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
    put_in_place(&partial, target, what)
}

/// A release `.tar.gz` (macOS Node) holds one top folder; its contents land in `target` with their
/// file modes, so `bin/node` stays runnable.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn untar_gz_dropping_top(bytes: &[u8], target: &Path, what: &str) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
    archive.set_preserve_permissions(true);
    let partial = target.with_extension("partial");
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|_| "A folder could not be created.".to_string())?;
    let entries = archive.entries().map_err(|_| "The download was not a readable archive.".to_string())?;
    for entry in entries {
        let mut entry = entry.map_err(|_| "The archive could not be read.".to_string())?;
        let inside = entry.path().map_err(|_| "The archive could not be read.".to_string())?.into_owned();
        // never outside the folder: no absolute paths, no `..`
        if inside.components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
            continue;
        }
        let relative: PathBuf = inside.components().filter(|c| matches!(c, std::path::Component::Normal(_))).skip(1).collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let out = partial.join(&relative);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "A folder could not be created.".to_string())?;
        }
        entry.unpack(&out).map_err(|_| "A file could not be written.".to_string())?;
    }
    put_in_place(&partial, target, what)
}

/// Only a whole unpack becomes the folder.
fn put_in_place(partial: &Path, target: &Path, what: &str) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(target);
    std::fs::rename(partial, target).map_err(|_| format!("The {what} folder could not be put in place."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("castle-fetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_download_is_kept_only_when_it_is_the_release_itself() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(matches(b"abc", "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"));
        assert!(!matches(b"abd", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    }

    #[test]
    fn a_zip_lands_without_its_top_folder_and_skips_what_is_refused() {
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            let options = zip::write::SimpleFileOptions::default();
            zip.add_directory("node-v1-win-x64/", options).unwrap();
            zip.start_file("node-v1-win-x64/node.exe", options).unwrap();
            zip.write_all(b"node").unwrap();
            zip.start_file("node-v1-win-x64/include/node.h", options).unwrap();
            zip.write_all(b"header").unwrap();
            zip.finish().unwrap();
        }
        let dir = temp("zip");
        let target = dir.join("v1");
        std::fs::create_dir_all(target.join("old")).unwrap();
        unzip_dropping_top(&bytes, &target, |p| !p.starts_with("include"), "test").unwrap();
        assert_eq!(std::fs::read(target.join("node.exe")).unwrap(), b"node");
        assert!(!target.join("include").exists(), "a refused path is not unpacked");
        assert!(!target.join("old").exists(), "the whole unpack replaces what was there");
        assert!(!target.with_extension("partial").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_zip_leaves_the_old_folder_alone() {
        let dir = temp("badzip");
        let target = dir.join("v1");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("node.exe"), b"old").unwrap();
        assert!(unzip_dropping_top(b"not a zip", &target, |_| true, "test").is_err());
        assert_eq!(std::fs::read(target.join("node.exe")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tiny local server for the resume tests: it records each request's `Range`, can honour or
    /// ignore it, can cut the first answer short, and can answer every request with one status.
    struct Served {
        url: String,
        ranges: std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
    }

    fn serve(body: Vec<u8>, cut_first_at: Option<usize>, honour_range: bool, always: Option<u16>) -> Served {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file.zip", listener.local_addr().unwrap());
        let ranges = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = ranges.clone();
        std::thread::spawn(move || {
            for (index, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = Some(v.trim().to_string());
                    }
                }
                seen.lock().unwrap().push(range.clone());
                if let Some(code) = always {
                    let _ = write!(stream, "HTTP/1.1 {code} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let from = range.as_ref().filter(|_| honour_range).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                match from {
                    Some(start) if start >= body.len() => {
                        let _ = write!(stream, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                    Some(start) => {
                        let part = &body[start..];
                        let _ = write!(stream, "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n", part.len(), body.len() - 1, body.len());
                        let _ = stream.write_all(part);
                    }
                    None => {
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let cut = if index == 0 { cut_first_at.unwrap_or(body.len()) } else { body.len() };
                        let _ = stream.write_all(&body[..cut]);
                    }
                }
            }
        });
        Served { url, ranges }
    }

    fn release_bytes() -> Vec<u8> {
        (0..1_000_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn run(url: &str, expected: usize, part: &Path) -> Result<Vec<u8>, String> {
        download_resumable_with(url, expected as u64, "Test", part, Duration::ZERO, &|_, _| {})
    }

    #[test]
    fn a_dropped_connection_carries_on_from_where_it_stopped() {
        let body = release_bytes();
        let server = serve(body.clone(), Some(300_000), true, None);
        let dir = temp("resume");
        let part = dir.join("release.zip.part");
        assert_eq!(run(&server.url, body.len(), &part).unwrap(), body);
        let ranges = server.ranges.lock().unwrap().clone();
        assert_eq!(ranges.len(), 2, "one cut try, one that carried on");
        assert_eq!(ranges[0], None);
        let from: u64 = ranges[1].as_ref().unwrap().trim_end_matches('-').parse().unwrap();
        assert!(from > 0 && from <= 300_000, "it asked only for what was missing, not the start again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_part_file_from_an_earlier_run_is_continued() {
        let body = release_bytes();
        let server = serve(body.clone(), None, true, None);
        let dir = temp("earlier");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, &body[..400_000]).unwrap();
        assert_eq!(run(&server.url, body.len(), &part).unwrap(), body);
        assert_eq!(server.ranges.lock().unwrap().clone(), vec![Some("400000-".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_that_ignores_ranges_starts_over_without_gluing_anything_on() {
        let body = release_bytes();
        let server = serve(body.clone(), None, false, None);
        let dir = temp("norange");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, &body[..400_000]).unwrap();
        assert_eq!(run(&server.url, body.len(), &part).unwrap(), body);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_finished_part_file_is_not_downloaded_again() {
        let body = release_bytes();
        let server = serve(body.clone(), None, true, None);
        let dir = temp("finished");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, &body).unwrap();
        assert_eq!(run(&server.url, body.len(), &part).unwrap(), body);
        assert!(server.ranges.lock().unwrap().is_empty(), "no request at all");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_part_file_longer_than_the_release_starts_again() {
        let body = release_bytes();
        let server = serve(body.clone(), None, true, None);
        let dir = temp("toolong");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, vec![7u8; body.len() + 5]).unwrap();
        assert_eq!(run(&server.url, body.len(), &part).unwrap(), body);
        assert_eq!(server.ranges.lock().unwrap().clone(), vec![None]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_that_never_moves_stops_and_keeps_what_it_has() {
        let body = release_bytes();
        let server = serve(body.clone(), None, true, Some(500));
        let dir = temp("stuck");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, &body[..100]).unwrap();
        let error = run(&server.url, body.len(), &part).unwrap_err();
        assert!(error.contains("paused") && error.contains("Press Install again"), "{error}");
        assert_eq!(server.ranges.lock().unwrap().len(), STALLED_TRIES as usize);
        assert_eq!(std::fs::read(&part).unwrap(), &body[..100], "the earlier bytes are still there to continue from");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A server that gives back less than was asked: each answer holds at most `window` bytes. A
    /// bounded `206` names the span it really sent in Content-Range (and `claimed_total` lets it name a
    /// different whole than the release); with `plain_short` it answers `200` with a body of only
    /// `window` bytes whatever the request says.
    fn serve_partial(body: Vec<u8>, window: usize, claimed_total: Option<usize>, plain_short: bool) -> Served {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file.zip", listener.local_addr().unwrap());
        let ranges = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = ranges.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = Some(v.trim().to_string());
                    }
                }
                seen.lock().unwrap().push(range.clone());
                let from = range.as_ref().filter(|_| !plain_short).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                match from {
                    Some(start) if start < body.len() => {
                        let end = (start + window).min(body.len());
                        let part = &body[start..end];
                        let _ = write!(stream, "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n", part.len(), end - 1, claimed_total.unwrap_or(body.len()));
                        let _ = stream.write_all(part);
                    }
                    _ => {
                        let part = &body[..window.min(body.len())];
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len());
                        let _ = stream.write_all(part);
                    }
                }
            }
        });
        Served { url, ranges }
    }

    #[test]
    fn a_valid_answer_for_only_part_of_what_is_missing_is_followed_by_asking_for_the_rest() {
        // the review's case: 40 of 100 bytes saved, the server answers `206`, `bytes 40-59/100`, 20 bytes
        let body: Vec<u8> = (0..100u8).collect();
        let server = serve_partial(body.clone(), 20, None, false);
        let dir = temp("bounded");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, &body[..40]).unwrap();
        assert_eq!(run(&server.url, body.len(), &part).unwrap(), body, "the whole release, not the 60 bytes the first answer made");
        assert_eq!(server.ranges.lock().unwrap().clone(), vec![Some("40-".to_string()), Some("60-".to_string()), Some("80-".to_string())], "each answer asked for what was still missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_range_that_names_a_different_whole_is_never_taken_for_this_release() {
        let body: Vec<u8> = (0..100u8).collect();
        let server = serve_partial(body.clone(), 20, Some(500), false);
        let dir = temp("otherwhole");
        let part = dir.join("release.zip.part");
        std::fs::write(&part, &body[..40]).unwrap();
        let error = run(&server.url, body.len(), &part).unwrap_err();
        assert!(error.contains("paused"), "{error}");
        assert!(size_of(&part) < body.len() as u64, "a piece of some other file never makes up the release");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plain_answer_shorter_than_the_release_is_not_the_release() {
        let body: Vec<u8> = (0..100u8).collect();
        let server = serve_partial(body.clone(), 60, None, true);
        let dir = temp("short200");
        let part = dir.join("release.zip.part");
        let error = run(&server.url, body.len(), &part).unwrap_err();
        assert!(error.contains("paused") && error.contains("Press Install again"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_content_range_is_read_whole_and_nonsense_is_refused() {
        assert_eq!(parse_content_range("bytes 40-59/100"), Some((40, 59, Some(100))));
        assert_eq!(parse_content_range(" bytes 0-0/1 "), Some((0, 0, Some(1))));
        assert_eq!(parse_content_range("bytes 40-59/*"), Some((40, 59, None)));
        for nonsense in ["", "bytes", "bytes 40-/100", "bytes -59/100", "bytes a-b/100", "items 40-59/100", "bytes 40-59/x", "bytes 40/100"] {
            assert_eq!(parse_content_range(nonsense), None, "{nonsense}");
        }
    }

    #[test]
    fn a_tar_gz_lands_without_its_top_folder_and_keeps_the_runnable_bit() {
        let mut bytes = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            let body = b"#!/bin/sh\necho v22.0.0\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "node-v1-darwin-arm64/bin/node", &body[..]).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let dir = temp("tar");
        let target = dir.join("v1");
        untar_gz_dropping_top(&bytes, &target, "test").unwrap();
        let node = target.join("bin").join("node");
        assert!(node.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(std::fs::metadata(&node).unwrap().permissions().mode() & 0o111, 0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
