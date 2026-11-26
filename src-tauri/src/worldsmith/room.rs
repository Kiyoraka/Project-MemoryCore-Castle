//! WORLDSMITH's own folder for its session: `<memory>/worldsmith`, with Castle's rules for the room
//! written between their markers into CLAUDE.md and AGENTS.md (the STORY room's way). With the
//! member's "only this companion" switch on, the session hears them through its boot file instead.

use crate::session::WORLDSMITH_FOLDER;
use crate::settings::SettingsStore;
use std::path::{Path, PathBuf};
use tauri::State;

const RULES: &str = include_str!("../../resources/worldsmith-rules.md");
const BEGIN: &str = "<!-- castle:worldsmith:begin";
const END: &str = "<!-- castle:worldsmith:end -->";

/// `text` with the rules put in place between their markers; everything else kept. Pure.
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

/// Makes the folder (when missing) and writes the rules with the names, only when they changed.
pub fn prepare(memory: &Path, companion: &str, member: &str) -> Result<PathBuf, String> {
    let folder = memory.join(WORLDSMITH_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|_| "The worldsmith folder could not be created.".to_string())?;
    let rules = RULES.replace("{{AI_NAME}}", companion.trim()).replace("{{USER_NAME}}", member.trim());
    for name in crate::setup::INSTRUCTION_FILES {
        let path = folder.join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let next = with_rules(&text, &rules);
        if next != text {
            std::fs::write(&path, next).map_err(|_| format!("{name} could not be written in the worldsmith folder."))?;
        }
    }
    Ok(folder)
}

#[tauri::command]
pub fn worldsmith_prepare(store: State<SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, super::ROOM)?;
    let s = store.get();
    let memory = s.memory_folder.clone().map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    let member = s.member_name.clone().unwrap_or_else(|| "the member".into());
    prepare(&memory, &s.companion_label(), &member).map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_go_between_their_markers_and_the_members_words_stay() {
        let rules = "<!-- castle:worldsmith:begin -->\nnew rules\n<!-- castle:worldsmith:end -->";
        assert_eq!(with_rules("", rules), format!("{rules}\n"));
        let mine = "# My notes\n\nkeep this";
        assert_eq!(with_rules(mine, rules), format!("{mine}\n\n{rules}\n"));
        let old = format!("{mine}\n\n<!-- castle:worldsmith:begin -->\nold\n<!-- castle:worldsmith:end -->\nafter");
        assert_eq!(with_rules(&old, rules), format!("{mine}\n\n{rules}\nafter"));
    }

    #[test]
    fn the_shipped_rules_carry_their_markers_the_names_and_the_refusal() {
        assert!(RULES.starts_with(BEGIN) && RULES.trim_end().ends_with(END));
        assert!(RULES.contains("{{AI_NAME}}") && RULES.contains("{{USER_NAME}}"));
        assert!(RULES.contains("offline single-player") && RULES.contains("anti-cheat"));
        assert!(RULES.contains("$env:WORLDSMITH_HELPER") && RULES.contains("castle-memory.cmd\" worldsmith"));
    }

    #[test]
    fn prepare_writes_both_instruction_files_once() {
        let memory = std::env::temp_dir().join(format!("castle-worldsmith-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&memory);
        let folder = prepare(&memory, "Lyra", "Rin").unwrap();
        for name in crate::setup::INSTRUCTION_FILES {
            let text = std::fs::read_to_string(folder.join(name)).unwrap();
            assert!(text.contains("You are Lyra") && text.contains("Rin plays") && !text.contains("{{"));
        }
        let _ = std::fs::remove_dir_all(&memory);
    }
}
