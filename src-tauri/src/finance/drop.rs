//! Receipt pictures dropped on FINANCE: each is checked and saved in the memory folder's
//! `finance/inbox/`, and one turn asks the companion to read them. A picture already filed is
//! named instead of sent again. At most `PER_TURN` go in one drop, so one turn stays readable.

use super::companion::Picture;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// How many pictures one drop sends the companion.
pub const PER_TURN: usize = 20;

/// What a drop did, for the room's one line.
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    /// pictures sent to the companion
    pub sent: usize,
    /// "r.jpg is already receipt #4 (kept)"
    pub already: Vec<String>,
    /// "notes.pdf: only a PNG, JPEG or WebP picture can be a receipt"
    pub refused: Vec<String>,
    /// good pictures over PER_TURN, to drop again after the companion has read these
    pub left: usize,
}

/// The pictures of a drop sorted out, and the ones to send saved into `inbox`. `filed` names the
/// receipt a fingerprint already is ("#4 (kept)").
pub fn prepare(paths: &[PathBuf], inbox: &Path, mut filed: impl FnMut(&str) -> Option<String>) -> (Vec<PathBuf>, Dropped) {
    let mut out = Dropped::default();
    let mut send = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
        let picture = match Picture::open(path) {
            Ok(p) => p,
            Err(why) => {
                out.refused.push(format!("{name}: {}", why.trim_end_matches('.').to_lowercase()));
                continue;
            }
        };
        if !seen.insert(picture.sha256.clone()) {
            continue;
        }
        if let Some(receipt) = filed(&picture.sha256) {
            out.already.push(format!("{name} is already receipt {receipt}"));
            continue;
        }
        if send.len() == PER_TURN {
            out.left += 1;
            continue;
        }
        let file = inbox.join(format!("{}.{}", &picture.sha256[..8], picture.ext));
        let saved = file.is_file() || std::fs::create_dir_all(inbox).and_then(|_| std::fs::write(&file, &picture.bytes)).is_ok();
        if saved {
            send.push(file);
        } else {
            out.refused.push(format!("{name}: it could not be saved in the memory folder"));
        }
    }
    out.sent = send.len();
    (send, out)
}

/// The companion's turn for the pictures.
pub fn turn(pictures: &[PathBuf]) -> String {
    let list: Vec<String> = pictures.iter().map(|p| p.display().to_string()).collect();
    format!(
        "[FINANCE] {} receipt picture{} dropped in FINANCE:\n{}\nRead each and propose it with castle-memory finance propose --source drop (Finance in your Castle block). Then say in one line what you proposed and what you could not read.",
        list.len(),
        if list.len() == 1 { "" } else { "s" },
        list.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(n: u8) -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, n]
    }

    #[test]
    fn a_drop_is_sorted_saved_and_capped() {
        let root = std::env::temp_dir().join(format!("castle-finance-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (outside, inbox) = (root.join("Downloads"), root.join("memory").join("finance").join("inbox"));
        std::fs::create_dir_all(&outside).unwrap();
        let mut paths = Vec::new();
        for n in 0..23u8 {
            let p = outside.join(format!("r{n}.jpg"));
            std::fs::write(&p, jpeg(n)).unwrap();
            paths.push(p);
        }
        paths.push(paths[0].clone());
        let pdf = outside.join("notes.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        paths.push(pdf);
        let filed_sha = Picture::open(&paths[1]).unwrap().sha256;
        let (send, done) = prepare(&paths, &inbox, |sha| (sha == filed_sha).then(|| "#4 (kept)".to_string()));
        assert_eq!(done.sent, PER_TURN);
        assert_eq!(send.len(), PER_TURN);
        assert!(send.iter().all(|p| p.starts_with(&inbox) && p.is_file()), "sent from the memory folder");
        assert_eq!(done.already, vec!["r1.jpg is already receipt #4 (kept)".to_string()]);
        assert_eq!(done.refused, vec!["notes.pdf: only a png, jpeg or webp picture can be a receipt".to_string()]);
        assert_eq!(done.left, 2, "23 good pictures, one filed, the same one dropped twice counted once");
        let words = turn(&send[..2]);
        assert!(words.starts_with("[FINANCE] 2 receipt pictures dropped in FINANCE:\n"));
        assert!(words.contains("--source drop"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
