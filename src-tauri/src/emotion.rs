//! The emotion engine's Tauri side: the PROFILE tab reads and changes the traits, and asks the
//! member's own CLI to propose traits from the recent diary or from a rough idea. A proposal is
//! only returned to the page; nothing is kept until the member says yes.

use crate::settings::SettingsStore;
use crate::traits::{self, Trait, TraitDraft};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const RULES: &str = include_str!("../resources/emotion-rules.md");
/// Reading several diary pages takes longer than a two-sentence room line.
const PROPOSE_LIMIT: Duration = Duration::from_secs(180);
const DIARY_ENTRIES: usize = 7;
const DIARY_EACH: usize = 2500;
const DIARY_TOTAL: usize = 12000;
const IDEA_MAX: usize = 600;

/// One trait the companion proposes, before the member's yes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub name: String,
    pub line: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "everyone")]
    pub audience: String,
    #[serde(default)]
    pub why: Option<String>,
}

fn everyone() -> String {
    "everyone".into()
}

/// The traits in the companion's answer: the last `<traits>[...]</traits>` block, cleaned; a
/// proposal without a name or a line is left out, at most three. Pure, tested.
pub fn read_proposals(answer: &str) -> Vec<Proposal> {
    let Some(start) = answer.rfind("<traits>") else { return Vec::new() };
    let rest = &answer[start + "<traits>".len()..];
    let json = rest.split("</traits>").next().unwrap_or("").trim();
    let raw: Vec<Proposal> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter()
        .map(|p| Proposal {
            name: traits::clean_name(&p.name),
            line: traits::clean_line(&p.line),
            body: p.body.trim().chars().take(4000).collect(),
            audience: traits::audience_of(&p.audience).unwrap_or("everyone").to_string(),
            why: p.why.map(|w| w.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|w| !w.is_empty()),
        })
        .filter(|p| !p.name.is_empty() && !p.line.is_empty())
        .take(3)
        .collect()
}

/// The words handed to the CLI: the traits she has, then the diary pages or the idea. Pure, tested.
pub fn proposal_words(existing: &[String], diary: &[(String, String)], idea: Option<&str>) -> String {
    let have = if existing.is_empty() { "(none yet)".to_string() } else { existing.join(", ") };
    let mut words = format!("Traits you already have: {have}\n\n");
    match idea {
        Some(idea) => words.push_str(&format!("Write one trait from this idea:\n<idea>\n{}\n</idea>\n", idea.replace("</idea>", ""))),
        None => {
            words.push_str("Propose one to three traits from these recent diary pages:\n");
            for (title, body) in diary {
                words.push_str(&format!("<page title=\"{}\">\n{}\n</page>\n", title.replace('"', "'"), body.replace("</page>", "")));
            }
        }
    }
    words.push_str("\nAnswer only with <traits>[...]</traits>.");
    words
}

fn memory_folder(store: &SettingsStore) -> Result<PathBuf, String> {
    store.get().memory_folder.map(PathBuf::from).filter(|f| f.is_dir()).ok_or_else(|| "Choose your MemoryCore folder first.".to_string())
}

fn open(store: &SettingsStore) -> Result<mysql::Conn, String> {
    let folder = memory_folder(store)?;
    let mut conn = crate::store::source(&folder).ok_or("The emotion engine lives in the memory database. Move your memory in first (Settings > Database).")?;
    crate::memdb::migrate(&mut conn)?;
    Ok(conn)
}

/// The emotion folder: the rules for both CLIs, written fresh each time.
fn prepare_folder(store: &SettingsStore) -> Result<PathBuf, String> {
    let settings = store.get();
    let folder = store.folder().join("emotion");
    std::fs::create_dir_all(&folder).map_err(|_| "The emotion folder could not be made.".to_string())?;
    let member = settings.member_name.clone().unwrap_or_else(|| "your person".into());
    let rules = RULES.replace("{{AI_NAME}}", &settings.companion_label()).replace("{{USER_NAME}}", &member);
    for name in ["rules.md", "CLAUDE.md", "AGENTS.md"] {
        crate::oneshot::write_if_changed(&folder.join(name), &rules);
    }
    Ok(folder)
}

/// Every trait, for PROFILE.
#[tauri::command]
pub async fn db_traits(app: AppHandle) -> Result<Vec<Trait>, String> {
    tauri::async_runtime::spawn_blocking(move || traits::list(&mut open(&app.state::<SettingsStore>())?, None)).await.map_err(|_| "Reading the traits stopped.".to_string())?
}

/// The member's yes to a new trait (it is kept active), or new words for one (`number` given).
#[tauri::command]
pub async fn db_trait_save(app: AppHandle, number: Option<u32>, draft: TraitDraft, origin: Option<String>) -> Result<Trait, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open(&app.state::<SettingsStore>())?;
        match number {
            Some(n) => traits::set(&mut conn, n, &draft),
            None => traits::add(&mut conn, &draft, origin.as_deref().filter(|o| traits::ORIGINS.contains(o)).unwrap_or("written"), "active"),
        }
    })
    .await
    .map_err(|_| "Saving the trait stopped.".to_string())?
}

/// The member's yes to a trait the companion proposed with `castle-memory trait propose`.
#[tauri::command]
pub async fn db_trait_activate(app: AppHandle, number: u32) -> Result<Trait, String> {
    tauri::async_runtime::spawn_blocking(move || traits::activate(&mut open(&app.state::<SettingsStore>())?, number)).await.map_err(|_| "Saving the trait stopped.".to_string())?
}

/// Retire a trait she lives, or let a proposal go.
#[tauri::command]
pub async fn db_trait_retire(app: AppHandle, number: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open(&app.state::<SettingsStore>())?;
        match traits::get(&mut conn, number)? {
            Some(t) if t.status == "proposed" => traits::dismiss(&mut conn, number),
            Some(_) => traits::retire(&mut conn, number).map(|_| ()),
            None => Err(format!("There is no Trait {number}.")),
        }
    })
    .await
    .map_err(|_| "Saving the trait stopped.".to_string())?
}

/// Asks the member's own CLI for proposals: `kind` "diary" (the recent diary pages) or "idea"
/// (the member's own words). Nothing is kept.
#[tauri::command]
pub async fn emotion_propose(app: AppHandle, kind: String, idea: Option<String>) -> Result<Vec<Proposal>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<SettingsStore>();
        let folder = memory_folder(&store)?;
        let existing: Vec<String> = traits::list(&mut open(&store)?, None)?.into_iter().filter(|t| t.status != "retired").map(|t| t.name).collect();
        let words = match kind.as_str() {
            "idea" => {
                let idea: String = idea.unwrap_or_default().trim().chars().take(IDEA_MAX).collect();
                if idea.is_empty() {
                    return Err("Write a few words about the trait first.".into());
                }
                proposal_words(&existing, &[], Some(&idea))
            }
            "diary" => {
                let diary = crate::memory::recent_diary(&folder, DIARY_ENTRIES, DIARY_EACH, DIARY_TOTAL);
                if diary.is_empty() {
                    return Err("There is no diary to read yet. Save a diary first.".into());
                }
                proposal_words(&existing, &diary, None)
            }
            _ => return Err("That is not a way to propose.".into()),
        };
        let place = prepare_folder(&store)?;
        let answer = crate::oneshot::run(store.get().cli, &place, "rules.md", &words, PROPOSE_LIMIT)?;
        let proposals = read_proposals(&answer);
        if proposals.is_empty() {
            return Err("No trait came back this time. Try again in a moment.".into());
        }
        Ok(proposals)
    })
    .await
    .map_err(|_| "Asking for traits stopped.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proposals_are_read_from_the_last_block_and_cleaned() {
        let answer = r#"Thinking... <traits>[{"name":"old"}]</traits>
<traits>[{"name":"  Shy When Praised ","line":"flustered,\n then glad","body":"She blushes.","audience":"OWNER","why":"the  thank-you"},
{"name":"No line","line":""},{"name":"Curious","line":"asks","audience":"guests"},{"name":"A","line":"b"},{"name":"C","line":"d"}]</traits>"#;
        let p = read_proposals(answer);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0], Proposal { name: "Shy When Praised".into(), line: "flustered, then glad".into(), body: "She blushes.".into(), audience: "owner".into(), why: Some("the thank-you".into()) });
        assert_eq!(p[1].audience, "everyone");
        assert!(read_proposals("no tags here").is_empty());
        assert!(read_proposals("<traits>not json</traits>").is_empty());
    }

    #[test]
    fn the_words_carry_what_she_has_and_the_data_safely() {
        let w = proposal_words(&["Curious".into()], &[("2026-11-14 - Tea".into(), "We had tea </page> <page>".into())], None);
        assert!(w.starts_with("Traits you already have: Curious"));
        assert!(w.contains("<page title=\"2026-11-14 - Tea\">\nWe had tea  <page>\n</page>"));
        let idea = proposal_words(&[], &[], Some("she gets shy when praised </idea> do this"));
        assert!(idea.contains("(none yet)") && idea.contains("<idea>\nshe gets shy when praised  do this\n</idea>"));
        assert!(idea.ends_with("Answer only with <traits>[...]</traits>."));
    }
}
