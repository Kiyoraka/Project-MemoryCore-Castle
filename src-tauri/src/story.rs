//! STORY: the member's adventures, MemoryCore's Interactive Story in a room of its own. The STORY
//! session runs in <memory>/adventures with Castle's rulebook written into CLAUDE.md and AGENTS.md
//! there; worlds already in adventures/ are listed and read, whatever made them.

use crate::session::STORY_FOLDER;
use crate::settings::SettingsStore;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

const RULES: &str = include_str!("../resources/story-rules.md");
const BEGIN: &str = "<!-- castle:story:begin";
const END: &str = "<!-- castle:story:end -->";

/// `text` with the story rules put in place between their markers; everything else kept.
pub fn with_rules(text: &str, rules: &str) -> String {
    let rules = rules.trim_end();
    if let (Some(start), Some(end)) = (text.find(BEGIN), text.find(END)) {
        if end > start {
            return format!("{}{}{}", &text[..start], rules, &text[end + END.len()..]);
        }
    }
    if text.trim().is_empty() {
        format!("{rules}\n")
    } else {
        format!("{}\n\n{rules}\n", text.trim_end())
    }
}

/// Makes adventures/ (when missing) and writes the rulebook with the names into its CLAUDE.md and
/// AGENTS.md, so the STORY session plays by MemoryCore's Interactive Story rules on either CLI.
pub fn prepare(memory: &Path, companion: &str, member: &str) -> Result<PathBuf, String> {
    let folder = memory.join(STORY_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|_| "The adventures folder could not be created.".to_string())?;
    let rules = RULES.replace("{{AI_NAME}}", companion.trim()).replace("{{USER_NAME}}", member.trim());
    for name in crate::setup::INSTRUCTION_FILES {
        let path = folder.join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let next = with_rules(&text, &rules);
        // only a real change is written, so opening STORY never touches a file for nothing
        if next != text {
            std::fs::write(&path, next).map_err(|_| format!("{name} could not be written in adventures."))?;
        }
    }
    Ok(folder)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct World {
    pub id: String,
    pub name: String,
    pub status: String,
    pub chapter: Option<String>,
    /// the story files, oldest first (relative to the memory folder)
    pub parts: Vec<String>,
    pub summary: Option<String>,
    /// scene pictures (absolute paths), newest last
    pub scenes: Vec<String>,
    /// the world folder (absolute) - a beat card's scene and voice paths are inside it
    pub folder: String,
    /// cast portraits: (name from the file, absolute path)
    pub cast: Vec<CastMember>,
    /// songs (absolute paths), an opening theme first
    pub music: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CastMember {
    /// "old-mira" -> "old mira"; matched to a speaker's name without case
    pub name: String,
    pub path: String,
}

fn pictures_in(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| matches!(p.extension().and_then(|x| x.to_str()).map(|x| x.to_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "webp")))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn songs_in(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.is_dir() {
            songs_in(&p, found);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("mp3")) {
            found.push(p);
        }
    }
}

/// The newest beat card in <world>/stage/ that reads whole (a card still being written is skipped).
pub fn newest_beat(world: &Path) -> Option<serde_json::Value> {
    let mut cards: Vec<(u32, PathBuf)> = std::fs::read_dir(world.join("stage"))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let stem = p.file_stem()?.to_str()?.to_string();
            let n = stem.strip_prefix("beat-")?.parse::<u32>().ok()?;
            (p.extension().and_then(|x| x.to_str()) == Some("json")).then_some((n, p))
        })
        .collect();
    cards.sort_by(|a, b| b.0.cmp(&a.0));
    cards.into_iter().find_map(|(_, p)| {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        v.get("lines").and_then(|l| l.as_array()).filter(|l| !l.is_empty())?;
        Some(v)
    })
}

fn field(text: &str, label: &str) -> Option<String> {
    // "- **Status**: In Progress", "**Status**: Completed" or "Status: Completed"
    text.lines().find_map(|l| {
        let plain = l.replace('*', "");
        let plain = plain.trim().trim_start_matches(['-', ' ']);
        let value = plain.strip_prefix(&format!("{label}:"))?.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Every world in adventures/: its name (setting.md's first heading), status and chapter.
pub fn worlds(memory: &Path) -> Vec<World> {
    let root = memory.join(STORY_FOLDER);
    let rel = |p: &Path| p.strip_prefix(memory).map(|r| r.display().to_string().replace('\\', "/")).unwrap_or_default();
    let mut found: Vec<World> = std::fs::read_dir(&root)
        .map(|e| e.flatten().map(|e| e.path()).filter(|p| p.join("setting.md").is_file()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| {
            let setting = std::fs::read_to_string(dir.join("setting.md")).unwrap_or_default();
            let id = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            let name = setting.lines().find_map(|l| l.trim().strip_prefix("# ").map(|h| h.trim().to_string())).unwrap_or_else(|| id.replace('-', " "));
            let mut parts: Vec<PathBuf> = std::fs::read_dir(dir.join("story"))
                .map(|e| e.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md")).collect())
                .unwrap_or_default();
            // story-2 before story-10
            parts.sort_by_key(|p| {
                let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                stem.rsplit('-').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0)
            });
            let scenes = pictures_in(&dir.join("scenes"));
            let cast = pictures_in(&dir.join("cast"))
                .into_iter()
                .map(|p| CastMember { name: p.file_stem().unwrap_or_default().to_string_lossy().replace(['-', '_'], " "), path: p.display().to_string() })
                .collect();
            let mut music = Vec::new();
            songs_in(&dir.join("music"), &mut music);
            // an opening theme first, then the rest by name
            music.sort_by_key(|p| (!p.to_string_lossy().to_lowercase().contains("opening"), p.clone()));
            World {
                folder: dir.display().to_string(),
                cast,
                music: music.iter().map(|p| p.display().to_string()).collect(),
                status: field(&setting, "Status").unwrap_or_else(|| "In Progress".into()),
                chapter: field(&setting, "Current Chapter"),
                summary: dir.join("summary.md").is_file().then(|| rel(&dir.join("summary.md"))),
                parts: parts.iter().map(|p| rel(p)).collect(),
                scenes: scenes.iter().map(|p| p.display().to_string()).collect(),
                id,
                name,
            }
        })
        .collect();
    // worlds in progress first, then by name
    found.sort_by(|a, b| (a.status != "In Progress").cmp(&(b.status != "In Progress")).then(a.name.cmp(&b.name)));
    found
}

#[tauri::command]
pub fn story_prepare(store: State<SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, "story")?;
    let s = store.get();
    let memory = s.memory_folder.clone().map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    let member = s.member_name.clone().unwrap_or_else(|| "the member".into());
    prepare(&memory, &s.companion_label(), &member).map(|p| p.display().to_string())
}

/// The newest beat card of one world, for the stage (None while there is none yet).
#[tauri::command]
pub fn story_beat(world: String, store: State<SettingsStore>) -> Result<Option<serde_json::Value>, String> {
    crate::rooms::need(&store, "story")?;
    if world.is_empty() || !world.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("That is not one of your worlds.".into());
    }
    let memory = store.get().memory_folder.map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    Ok(newest_beat(&memory.join(STORY_FOLDER).join(world)))
}

#[tauri::command]
pub fn story_worlds(store: State<SettingsStore>) -> Vec<World> {
    store.get().memory_folder.map(|m| worlds(Path::new(&m))).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_go_in_with_the_names_and_the_members_words_stay() {
        let memory = std::env::temp_dir().join(format!("castle-story-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&memory);
        std::fs::create_dir_all(memory.join("adventures")).unwrap();
        std::fs::write(memory.join("adventures").join("CLAUDE.md"), "# my story notes\n").unwrap();
        prepare(&memory, "Lyra", "Rin").unwrap();
        let written = std::fs::metadata(memory.join("adventures").join("CLAUDE.md")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        prepare(&memory, "Lyra", "Rin").unwrap();
        // opening STORY again changes nothing, so the file is not touched
        assert_eq!(std::fs::metadata(memory.join("adventures").join("CLAUDE.md")).unwrap().modified().unwrap(), written);
        let claude = std::fs::read_to_string(memory.join("adventures").join("CLAUDE.md")).unwrap();
        assert!(claude.starts_with("# my story notes"));
        assert_eq!(claude.matches(BEGIN).count(), 1);
        assert!(claude.contains("Lyra") && !claude.contains("{{AI_NAME}}") && !claude.contains("{{USER_NAME}}"));
        assert!(memory.join("adventures").join("AGENTS.md").is_file());
        let _ = std::fs::remove_dir_all(memory);
    }

    #[test]
    fn worlds_are_listed_with_status_chapter_and_parts_in_order() {
        let memory = std::env::temp_dir().join(format!("castle-worlds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&memory);
        let world = memory.join("adventures").join("sky-kingdom");
        std::fs::create_dir_all(world.join("story")).unwrap();
        std::fs::create_dir_all(world.join("scenes")).unwrap();
        std::fs::write(world.join("setting.md"), "# The Sky Kingdom\n- **Status**: In Progress\n- **Current Chapter**: 3\n").unwrap();
        for n in [10, 2, 1] {
            std::fs::write(world.join("story").join(format!("story-{n}.md")), "x").unwrap();
        }
        std::fs::write(world.join("scenes").join("01-gate.png"), b"png").unwrap();
        let done = memory.join("adventures").join("old-tale");
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(done.join("setting.md"), "# An Old Tale\n**Status**: Completed\n").unwrap();
        let list = worlds(&memory);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "The Sky Kingdom");
        assert_eq!(list[0].chapter.as_deref(), Some("3"));
        assert_eq!(list[0].parts, ["adventures/sky-kingdom/story/story-1.md", "adventures/sky-kingdom/story/story-2.md", "adventures/sky-kingdom/story/story-10.md"]);
        assert_eq!(list[0].scenes.len(), 1);
        assert_eq!(list[1].status, "Completed");
        let _ = std::fs::remove_dir_all(memory);
    }

    #[test]
    fn the_stage_gets_the_newest_whole_card_the_cast_and_the_opening_first() {
        let memory = std::env::temp_dir().join(format!("castle-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&memory);
        let world = memory.join("adventures").join("sky-kingdom");
        for sub in ["stage", "cast", "music/opening", "music/calm"] {
            std::fs::create_dir_all(world.join(sub)).unwrap();
        }
        std::fs::write(world.join("setting.md"), "# The Sky Kingdom\n").unwrap();
        std::fs::write(world.join("stage/beat-0001.json"), r#"{"beat":1,"lines":[{"speaker":"","text":"first"}],"choices":[]}"#).unwrap();
        std::fs::write(world.join("stage/beat-0002.json"), r#"{"beat":2,"lines":[{"speaker":"Nova","text":"second"}],"choices":["a","b"]}"#).unwrap();
        // a card still being written is skipped
        std::fs::write(world.join("stage/beat-0003.json"), r#"{"beat":3,"lines":[{"speak"#).unwrap();
        assert_eq!(newest_beat(&world).unwrap()["beat"], 2);
        std::fs::write(world.join("cast/old-mira.png"), b"png").unwrap();
        std::fs::write(world.join("music/calm/rest.mp3"), b"mp3").unwrap();
        std::fs::write(world.join("music/opening/theme.mp3"), b"mp3").unwrap();
        let list = worlds(&memory);
        assert_eq!(list[0].cast, vec![CastMember { name: "old mira".into(), path: world.join("cast").join("old-mira.png").display().to_string() }]);
        assert!(list[0].music[0].contains("opening"));
        assert_eq!(list[0].music.len(), 2);
        assert!(newest_beat(&memory.join("nowhere")).is_none());
        let _ = std::fs::remove_dir_all(memory);
    }
}
