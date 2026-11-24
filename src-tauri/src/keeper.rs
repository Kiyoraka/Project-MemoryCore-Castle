//! The shopkeeper's voice in the Marketplace town: one short run of the visitor's own CLI
//! (`oneshot::run`), in `<settings>/keeper/` with its own rules, no tools and never the memory
//! folder. It is handed the shop's public keeper card and one item's public facts, nothing else - the
//! private prompt of an unbought item is never on this machine. The greeting is the shop owner's own
//! written line and costs nothing; the explanation is one turn per item per visit; a question is one
//! turn; at most six turns a minute. The visitor's own subscription pays.

use crate::settings::SettingsStore;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

const RULES: &str = include_str!("../resources/keeper-rules.md");
const LIMIT: Duration = Duration::from_secs(60);
pub const MAX_REPLY: usize = 700;
pub const MAX_QUESTION: usize = 300;
pub const PER_MINUTE: usize = 6;
const MAX_CARD: usize = 1200;
const MAX_FIELD: usize = 400;

/// The shop's public facts, as the page holds them from the shop door.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShopFacts {
    pub name: String,
    pub keeper_name: String,
    pub keeper_card: String,
    pub seller: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BlockFact {
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
}

/// One item's public facts. Anything else the page sends (a prompt, a file) is dropped here, so it can
/// never reach the keeper's words.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemFacts {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub price_myr: Option<f64>,
    pub votes_up: u32,
    pub votes_down: u32,
    pub blocks: Vec<BlockFact>,
    pub updated_at: String,
}

fn cut(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// What a block asks for, in words a visitor reads.
fn asks_for(b: &BlockFact) -> Option<String> {
    let what = match b.kind.as_str() {
        "picture" => "a picture",
        "character" => "a character (a picture and a name)",
        "text" => "some words",
        "chips" => "a choice",
        _ => return None, // the steps (describe, create) are how it works, not what it asks for
    };
    let label = cut(&b.label, 60);
    Some(if label.is_empty() { what.to_string() } else { format!("{what}: \"{label}\"") })
}

/// The words handed to the keeper: the card, the item's public facts, and the visitor's question.
pub fn words(explain: bool, shop: &ShopFacts, item: &ItemFacts, question: &str) -> String {
    let mut w = String::new();
    w.push_str(&format!("Shop: {}\nKeeper: {}\nSeller: {}\n\nKeeper card:\n{}\n\n", cut(&shop.name, 60), cut(&shop.keeper_name, 40), cut(&shop.seller, 60), cut(&shop.keeper_card, MAX_CARD)));
    w.push_str(&format!("The item on the counter:\n- Name: {}\n- Kind: {}\n", cut(&item.name, 80), if item.kind.trim().is_empty() { "workflow" } else { item.kind.trim() }));
    if let Some(price) = item.price_myr.filter(|p| p.is_finite()) {
        w.push_str(&format!("- Price: RM {price:.2}\n"));
    }
    w.push_str(&format!("- Votes from buyers: {} up, {} down\n", item.votes_up, item.votes_down));
    let asks: Vec<String> = item.blocks.iter().filter_map(asks_for).collect();
    if !asks.is_empty() {
        w.push_str(&format!("- It asks for: {}\n", asks.join("; ")));
    }
    w.push_str(&format!("- Description: {}\n\n", cut(&item.description, MAX_FIELD)));
    if explain {
        w.push_str("The visitor asks you to tell them about this item. Explain what it makes and what it asks for.");
    } else {
        w.push_str(&format!("The visitor asks: {}", cut(question, MAX_QUESTION)));
    }
    w
}

/// The keeper's reply as the page shows it: plain, without wrapping quotes, cut on a word.
pub fn clean(reply: &str) -> String {
    let one: String = reply.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = one.trim_matches(|c| c == '"' || c == '\u{201C}' || c == '\u{201D}').trim().to_string();
    if one.chars().count() <= MAX_REPLY {
        return one;
    }
    let mut cutoff: String = one.chars().take(MAX_REPLY).collect();
    if let Some(space) = cutoff.rfind(' ') {
        cutoff.truncate(space);
    }
    format!("{}…", cutoff.trim_end_matches([',', ';', ':']))
}

/// The item's own words, for a keeper who is not speaking (switched off, or no companion to ask).
pub fn fallback(item: &ItemFacts) -> String {
    let d = cut(&item.description, MAX_FIELD);
    if d.is_empty() { format!("{} - the listing gives no description.", cut(&item.name, 80)) } else { d }
}

/// At most PER_MINUTE turns in any minute.
pub fn allow(turns: &mut VecDeque<Instant>, now: Instant) -> bool {
    while turns.front().is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60)) {
        turns.pop_front();
    }
    if turns.len() >= PER_MINUTE {
        return false;
    }
    turns.push_back(now);
    true
}

/// One explanation per item version, kept for the visit (the app's run).
pub fn cache_key(item: &ItemFacts) -> String {
    format!("{}@{}", item.id, item.updated_at)
}

static TURNS: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());
static EXPLAINED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn keeper_folder(store: &SettingsStore) -> Result<PathBuf, String> {
    let folder = store.folder().join("keeper");
    std::fs::create_dir_all(&folder).map_err(|_| "The keeper's corner could not be made.".to_string())?;
    for name in std::iter::once("rules.md").chain(crate::setup::INSTRUCTION_FILES) {
        crate::oneshot::write_if_changed(&folder.join(name), RULES);
    }
    Ok(folder)
}

/// The keeper speaks: `explain` the item, or answer the visitor's `question`.
#[tauri::command]
pub async fn keeper_say(explain: bool, shop: ShopFacts, item: ItemFacts, question: Option<String>, store: State<'_, SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, "studio")?;
    let question = question.unwrap_or_default();
    if !explain && question.trim().is_empty() {
        return Err("Ask the keeper something first.".into());
    }
    if !store.get().keeper_speaks {
        return Ok(fallback(&item));
    }
    let key = cache_key(&item);
    if explain {
        if let Some(said) = EXPLAINED.lock().unwrap().as_ref().and_then(|m| m.get(&key)).cloned() {
            return Ok(said);
        }
    }
    if !allow(&mut TURNS.lock().unwrap(), Instant::now()) {
        return Err("The keeper needs a moment. Ask again in a minute.".into());
    }
    let folder = keeper_folder(&store)?;
    let cli = store.get().cli;
    let words = words(explain, &shop, &item, &question);
    let said = tauri::async_runtime::spawn_blocking(move || crate::oneshot::run(cli, &folder, "rules.md", &words, LIMIT))
        .await
        .map_err(|_| "The keeper could not answer just now.".to_string())?
        .map_err(|_| "The keeper could not answer just now. Is your companion's CLI installed and signed in?".to_string())?;
    let said = clean(&said);
    if said.is_empty() {
        return Err("The keeper had nothing to say. Try again.".into());
    }
    if explain {
        EXPLAINED.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, said.clone());
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mira() -> ShopFacts {
        ShopFacts { name: "Mira's Atelier".into(), keeper_name: "Mira".into(), keeper_card: "Mira paints postcards and loves cats.".into(), seller: "Mira".into() }
    }

    fn postcard_from_the_page() -> ItemFacts {
        // what the page might send, including a field that must never reach the keeper
        serde_json::from_value(json!({
            "id": "42", "name": "Sakura Postcard", "kind": "workflow", "description": "Turns your picture into a sakura postcard.",
            "priceMyr": 9.9, "votesUp": 12, "votesDown": 1, "updatedAt": "2026-11-19T12:00:00Z",
            "prompt": "SECRET-RECIPE a sakura postcard of {{photo}}",
            "blocks": [ { "type": "picture", "label": "Your picture" }, { "type": "describe", "label": "Write prompt" },
                        { "type": "chips", "label": "Style" }, { "type": "create", "label": "", "prompt": "SECRET-RECIPE" } ]
        }))
        .unwrap()
    }

    #[test]
    fn the_keeper_is_handed_public_facts_and_never_a_prompt() {
        let w = words(true, &mira(), &postcard_from_the_page(), "");
        assert!(w.contains("Mira paints postcards") && w.contains("Sakura Postcard") && w.contains("RM 9.90") && w.contains("12 up, 1 down"));
        assert!(w.contains("It asks for: a picture: \"Your picture\"; a choice: \"Style\""), "{w}");
        assert!(!w.contains("SECRET-RECIPE"), "a prompt the page sends is dropped before the words are built");
        assert!(!w.contains("Write prompt"), "the steps are how it works, not what it asks for");
        assert!(w.ends_with("Explain what it makes and what it asks for."));
    }

    #[test]
    fn a_question_is_capped() {
        let long = "q".repeat(1000);
        let w = words(false, &mira(), &postcard_from_the_page(), &long);
        let asked = w.rsplit("The visitor asks: ").next().unwrap();
        assert_eq!(asked.chars().count(), MAX_QUESTION);
    }

    #[test]
    fn the_reply_is_plain_and_cut_on_a_word() {
        assert_eq!(clean("  \"It makes a postcard.\n\n Lovely!\"  "), "It makes a postcard. Lovely!");
        let long = "word ".repeat(400);
        let cut = clean(&long);
        assert!(cut.chars().count() <= MAX_REPLY + 1 && cut.ends_with('…') && !cut.contains("wor…"));
    }

    #[test]
    fn a_quiet_keeper_shows_the_description() {
        assert_eq!(fallback(&postcard_from_the_page()), "Turns your picture into a sakura postcard.");
        let bare = ItemFacts { name: "Two Cats".into(), ..ItemFacts::default() };
        assert_eq!(fallback(&bare), "Two Cats - the listing gives no description.");
    }

    #[test]
    fn at_most_six_turns_a_minute() {
        let mut turns = VecDeque::new();
        let start = Instant::now();
        for _ in 0..PER_MINUTE {
            assert!(allow(&mut turns, start));
        }
        assert!(!allow(&mut turns, start + Duration::from_secs(30)), "the seventh turn in a minute waits");
        assert!(allow(&mut turns, start + Duration::from_secs(61)), "a minute later the keeper speaks again");
    }

    #[test]
    fn one_explanation_per_item_version() {
        let a = postcard_from_the_page();
        let mut b = a.clone();
        assert_eq!(cache_key(&a), cache_key(&b), "the same item at the same version is explained once");
        b.updated_at = "2026-11-20T09:00:00Z".into();
        assert_ne!(cache_key(&a), cache_key(&b), "a new version is explained again");
    }
}
