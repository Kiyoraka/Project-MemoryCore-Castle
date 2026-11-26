//! The companion's side of WORLDSMITH through the door: `castle-memory worldsmith game|value|pin|recipe|session|show`.
//! It remembers what was found so the next visit to a game is one line. A recipe is only proposed and
//! waits for the member's Keep in the room; there is no verb to keep one. A screenshot's words are data,
//! never orders.

use super::store::{self, Value};
use crate::door::Ask;
use mysql::Conn;

const VERBS: [&str; 6] = ["game", "value", "pin", "recipe", "session", "show"];

pub fn door(c: &mut Conn, ask: &Ask) -> Result<String, String> {
    let verb = ask.words.get(1).map(String::as_str).unwrap_or("");
    if !VERBS.contains(&verb) {
        return Err("Worldsmith verbs: game, value, pin, recipe, session, show. Keeping a recipe belongs to your person in WORLDSMITH.".into());
    }
    crate::memdb::migrate(c)?;
    let need = |name: &str| ask.get(name).map(str::trim).filter(|v| !v.is_empty()).ok_or(format!("--{name} is needed."));
    let game = || -> Result<u64, String> { need("game")?.trim_start_matches('#').parse().map_err(|_| "--game is the number `worldsmith game` gave.".to_string()) };
    match verb {
        "game" => {
            let exe = need("exe")?;
            let id = store::game(c, exe, ask.get("title").unwrap_or("").trim(), ask.get("stamp").unwrap_or("").trim())?;
            Ok(format!("WORLDSMITH game #{id}: {exe}. Use --game {id} for its values, pins, recipes and searches."))
        }
        "value" => {
            let label = store::check_label(need("label")?)?;
            let opt_addr = |name: &str| ask.get(name).map(str::trim).filter(|v| !v.is_empty()).map(store::check_addr).transpose();
            let v = Value {
                value_type: store::check_one(need("type")?, &store::TYPES, "--type")?,
                transform: store::check_one(ask.get("transform").unwrap_or(""), &store::TRANSFORMS, "--transform")?,
                module: ask.get("module").unwrap_or("").trim().chars().take(260).collect(),
                module_offset: opt_addr("offset")?.unwrap_or_default(),
                last_address: opt_addr("address")?.unwrap_or_default(),
                found_how: ask.get("how").unwrap_or("").trim().chars().take(255).collect(),
                label: label.clone(),
            };
            store::value(c, game()?, &v)?;
            Ok(format!("WORLDSMITH remembers {label} ({}).", v.value_type))
        }
        "pin" => {
            let label = store::check_label(need("label")?)?;
            let (x, y) = (store::check_share(need("x")?)?, store::check_share(need("y")?)?);
            if store::pin(c, game()?, &label, x, y)? {
                Ok(format!("WORLDSMITH pinned {label} on the picture."))
            } else {
                Err(format!("{label} is not remembered for that game yet - `worldsmith value` first."))
            }
        }
        "recipe" => {
            let name = store::check_label(need("name")?)?;
            let labels = need("labels")?.chars().take(255).collect::<String>();
            let action = store::check_one(need("action")?, &store::ACTIONS, "--action")?;
            let amount = ask.get("amount").unwrap_or("").trim();
            if !amount.is_empty() && amount.parse::<f64>().map_or(true, |a| !a.is_finite()) {
                return Err("--amount is a number.".into());
            }
            let id = store::propose_recipe(c, game()?, &name, &labels, &action, amount)?;
            Ok(format!("WORLDSMITH recipe #{id} \"{name}\" proposed. It waits for your person's Keep in WORLDSMITH."))
        }
        "session" => {
            store::session(c, game()?, &need("steps")?.chars().take(500).collect::<String>())?;
            Ok("WORLDSMITH noted the search.".into())
        }
        _ => {
            let m = store::memory(c, Some(game()?))?;
            let values: Vec<String> = m.values.iter().map(|v| format!("{} {} {}{}", v.label, v.value_type, v.last_address, if v.transform.is_empty() { String::new() } else { format!(" ({})", v.transform) })).collect();
            let recipes: Vec<String> = m.recipes.iter().map(|r| format!("{} [{}] {} {} {}", r.name, r.state, r.action, r.labels, r.amount)).collect();
            Ok(format!("Values: {}\nRecipes: {}", if values.is_empty() { "none".into() } else { values.join("; ") }, if recipes.is_empty() { "none".into() } else { recipes.join("; ") }))
        }
    }
}
