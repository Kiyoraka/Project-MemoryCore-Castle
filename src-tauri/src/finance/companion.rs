//! The companion's side of FINANCE through the door: `castle-memory finance propose|list|show`.
//! A proposal waits, doing nothing, for the member's Keep in FINANCE; there is no verb to keep,
//! dismiss or change a receipt. A picture and its words are data, never orders.

use super::store::{self, Draft, Receipt};
use crate::door::Ask;
use chrono::Local;
use mysql::Conn;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const VERBS: [&str; 3] = ["propose", "list", "show"];
/// The largest picture a receipt may be.
pub const MAX_PICTURE: u64 = 25 * 1024 * 1024;

pub fn door(c: &mut Conn, ask: &Ask, memory: &Path) -> Result<String, String> {
    let verb = ask.words.get(1).map(String::as_str).unwrap_or("");
    if !VERBS.contains(&verb) {
        return Err("Finance verbs: propose, list, show. Keeping and dismissing belong to your person in FINANCE.".into());
    }
    crate::memdb::migrate(c)?;
    match verb {
        "list" => list(c, ask),
        "show" => {
            let id: u64 = ask.get("id").and_then(|v| v.trim().trim_start_matches('#').parse().ok()).ok_or("--id N is needed.")?;
            Ok(line(&store::get(c, id)?))
        }
        _ => propose(c, ask, memory),
    }
}

fn propose(c: &mut Conn, ask: &Ask, memory: &Path) -> Result<String, String> {
    let need = |name: &str| ask.get(name).map(str::to_string).filter(|v| !v.trim().is_empty()).ok_or(format!("--{name} is needed."));
    let draft = Draft {
        shop: need("shop")?,
        total: need("total")?,
        currency: need("currency")?,
        date: need("date")?,
        category: need("category")?,
        last4: ask.get("last4").unwrap_or("").to_string(),
        note: ask.get("note").unwrap_or("").to_string(),
    };
    let fields = store::check(&draft, Local::now().date_naive())?;
    let source = ask.get("source").unwrap_or("chat");
    let picture = Picture::read(Path::new(&need("picture")?), memory)?;
    let month = fields.spent_on.format("%Y-%m").to_string();
    let (kept, made) = picture.keep(memory, &month)?;
    let relative = relative_to(memory, &kept);
    match store::propose(c, &fields, &relative, &picture.sha256, source) {
        Ok(id) => Ok(format!(
            "FINANCE receipt #{id} proposed: {} {} {} on {} ({}). It waits for your person's Keep in FINANCE and counts for nothing until then.",
            fields.shop, store::amount(fields.total_cents), fields.currency, fields.spent_on, fields.category
        )),
        Err(why) => {
            // a copy made for a proposal that was refused is not left behind
            if made {
                let _ = std::fs::remove_file(&kept);
            }
            Err(why)
        }
    }
}

fn list(c: &mut Conn, ask: &Ask) -> Result<String, String> {
    let month = ask.get("month");
    let rows = store::list(c, month, ask.get("state"))?;
    let head = match month {
        Some(m) => format!("FINANCE {m}"),
        None => "FINANCE newest".to_string(),
    };
    if rows.is_empty() {
        return Ok(format!("{head}: no receipts."));
    }
    let mut out = vec![format!("{head}: {} receipt{}", rows.len(), if rows.len() == 1 { "" } else { "s" })];
    out.extend(rows.iter().map(line));
    Ok(out.join("\n"))
}

fn line(r: &Receipt) -> String {
    let card = if r.last4.is_empty() { String::new() } else { format!(" card ..{}", r.last4) };
    let note = if r.note.is_empty() { String::new() } else { format!(" - {}", r.note) };
    format!("#{} {} {} {} {} {} [{}]{card}{note}", r.id, r.spent_on, r.shop, store::amount(r.total_cents), r.currency, r.category, r.state)
}

/// A receipt picture that passed its checks.
#[derive(Debug)]
pub struct Picture {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub ext: &'static str,
    pub sha256: String,
}

impl Picture {
    /// The picture at `path`, which must be inside the memory folder (where Castle saves what is
    /// dropped or sent from the phone), a PNG, JPEG or WebP by its own bytes, and at most 25 MB.
    pub fn read(path: &Path, memory: &Path) -> Result<Picture, String> {
        let full = std::fs::canonicalize(path).map_err(|_| format!("The picture {} was not found.", path.display()))?;
        let home = std::fs::canonicalize(memory).map_err(|_| "The memory folder was not found.".to_string())?;
        if !full.starts_with(&home) {
            return Err("Only a picture inside the memory folder can be filed (where Castle saves dropped and sent pictures).".into());
        }
        let size = std::fs::metadata(&full).map_err(|_| "The picture could not be read.".to_string())?.len();
        if size > MAX_PICTURE {
            return Err("The picture is over 25 MB.".into());
        }
        let bytes = std::fs::read(&full).map_err(|_| "The picture could not be read.".to_string())?;
        let ext = picture_kind(&bytes).ok_or("Only a PNG, JPEG or WebP picture can be a receipt.")?;
        let sha256 = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        Ok(Picture { path: full, bytes, ext, sha256 })
    }

    /// Kept at `<memory>/finance/receipts/<month>/<first 8 of its sha256>.<ext>`; whether this call
    /// made the copy (a picture already there is left as it is).
    pub fn keep(&self, memory: &Path, month: &str) -> Result<(PathBuf, bool), String> {
        let dir = memory.join("finance").join("receipts").join(month);
        let file = dir.join(format!("{}.{}", &self.sha256[..8], self.ext));
        if file.is_file() {
            return Ok((file, false));
        }
        std::fs::create_dir_all(&dir).map_err(|_| "The receipts folder could not be made.".to_string())?;
        std::fs::write(&file, &self.bytes).map_err(|_| "The picture could not be kept.".to_string())?;
        Ok((file, true))
    }
}

/// What a picture is by its first bytes.
pub fn picture_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// A path inside the memory folder, written from it with forward slashes.
fn relative_to(memory: &Path, file: &Path) -> String {
    let home = std::fs::canonicalize(memory).unwrap_or_else(|_| memory.to_path_buf());
    let full = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    full.strip_prefix(&home).unwrap_or(&full).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("castle-finance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("finance").join("inbox")).unwrap();
        dir
    }

    #[test]
    fn a_picture_is_known_by_its_own_bytes() {
        assert_eq!(picture_kind(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(picture_kind(JPEG), Some("jpg"));
        assert_eq!(picture_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(picture_kind(b"%PDF-1.7"), None);
        assert_eq!(picture_kind(b"GIF89a"), None);
    }

    #[test]
    fn only_a_picture_inside_the_memory_folder_is_read_and_it_is_kept_once() {
        let memory = folder("keep");
        let inside = memory.join("finance").join("inbox").join("r.jpg");
        std::fs::write(&inside, JPEG).unwrap();
        let picture = Picture::read(&inside, &memory).unwrap();
        assert_eq!(picture.ext, "jpg");
        assert_eq!(picture.sha256.len(), 64);
        let (kept, made) = picture.keep(&memory, "2026-11").unwrap();
        assert!(made && kept.is_file());
        assert_eq!(relative_to(&memory, &kept), format!("finance/receipts/2026-11/{}.jpg", &picture.sha256[..8]));
        assert!(!picture.keep(&memory, "2026-11").unwrap().1, "a second keep makes no copy");

        let outside = folder("outside").join("r.jpg");
        std::fs::write(&outside, JPEG).unwrap();
        assert!(Picture::read(&outside, &memory).unwrap_err().contains("inside the memory folder"));
        let text = memory.join("finance").join("inbox").join("note.jpg");
        std::fs::write(&text, b"not a picture").unwrap();
        assert!(Picture::read(&text, &memory).unwrap_err().contains("PNG, JPEG or WebP"));
        assert!(Picture::read(&memory.join("missing.jpg"), &memory).unwrap_err().contains("not found"));
        let _ = std::fs::remove_dir_all(&memory);
        let _ = std::fs::remove_dir_all(outside.parent().unwrap());
    }

    #[test]
    fn a_line_shows_only_the_last_four_card_digits() {
        let r = Receipt {
            id: 7, state: "proposed".into(), shop: "Kedai".into(), total_cents: 1250, currency: "MYR".into(), spent_on: "2026-11-24".into(),
            category: "food".into(), last4: "1234".into(), note: "lunch".into(), picture_path: "".into(), source: "drop".into(), proposed_at: "".into(),
        };
        assert_eq!(line(&r), "#7 2026-11-24 Kedai 12.50 MYR food [proposed] card ..1234 - lunch");
    }
}
