//! WORLDSMITH's memory of each game, in the member's own database (migration 034): the games, the
//! values found in them (how to find them again, where they sit on the screenshot), the recipes the
//! companion proposed (each waits for the member's Keep), a short line per search, and (migration 035)
//! the lines Translate read in each game, so its names stay the same.

use mysql::prelude::Queryable;
use mysql::Conn;
use serde::Serialize;

pub const TYPES: [&str; 4] = ["i32", "i64", "f32", "f64"];
pub const TRANSFORMS: [&str; 4] = ["", "x10", "x100", "neg"];
pub const ACTIONS: [&str; 2] = ["write", "freeze"];

fn sql<E>(_: E) -> String {
    "The WORLDSMITH memory could not be read or written.".to_string()
}

/// A label as the companion named a value: a few plain words. Pure.
pub fn check_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 60 || label.chars().any(char::is_control) {
        return Err("A value's label is a few plain words (at most 60 letters).".into());
    }
    Ok(label.to_string())
}

/// An address or an offset as the helper writes it: 0x and hex. Pure.
pub fn check_addr(value: &str) -> Result<String, String> {
    let v = value.trim();
    let hex = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")).unwrap_or("");
    if hex.is_empty() || hex.len() > 16 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{value} is not an address (it is written 0x...)."));
    }
    Ok(format!("0x{}", hex.to_ascii_uppercase()))
}

/// Where on the screenshot a value sits, as a share of its width or height. Pure.
pub fn check_share(value: &str) -> Result<f64, String> {
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite() && (0.0..=1.0).contains(v)).ok_or_else(|| format!("{value} is not a place on the picture (0 to 1)."))
}

pub fn check_one(value: &str, allowed: &[&str], what: &str) -> Result<String, String> {
    let v = value.trim();
    if allowed.contains(&v) {
        Ok(v.to_string())
    } else {
        Err(format!("{what} is one of: {}.", allowed.iter().filter(|a| !a.is_empty()).copied().collect::<Vec<_>>().join(", ")))
    }
}

/// The game, remembered once per exe and stamp (a patched game is a new game). Its id.
pub fn game(c: &mut Conn, exe: &str, title: &str, stamp: &str) -> Result<u64, String> {
    c.exec_drop(
        "INSERT INTO worldsmith_games (exe, title, stamp) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE title = VALUES(title), last_played = CURRENT_TIMESTAMP",
        (exe, title, stamp),
    )
    .map_err(sql)?;
    c.exec_first("SELECT id FROM worldsmith_games WHERE exe = ? AND stamp = ?", (exe, stamp)).map_err(sql)?.ok_or_else(|| sql(()))
}

pub struct Value {
    pub label: String,
    pub value_type: String,
    pub transform: String,
    pub module: String,
    pub module_offset: String,
    pub last_address: String,
    pub found_how: String,
}

pub fn value(c: &mut Conn, game: u64, v: &Value) -> Result<(), String> {
    c.exec_drop(
        "INSERT INTO worldsmith_values (game_id, label, value_type, transform, module, module_offset, last_address, found_how) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE value_type = VALUES(value_type), transform = VALUES(transform), module = VALUES(module), module_offset = VALUES(module_offset), last_address = VALUES(last_address), found_how = VALUES(found_how), updated_at = CURRENT_TIMESTAMP",
        (game, &v.label, &v.value_type, &v.transform, &v.module, &v.module_offset, &v.last_address, &v.found_how),
    )
    .map_err(sql)
}

pub fn pin(c: &mut Conn, game: u64, label: &str, x: f64, y: f64) -> Result<bool, String> {
    c.exec_drop("UPDATE worldsmith_values SET pin_x = ?, pin_y = ? WHERE game_id = ? AND label = ?", (x, y, game, label)).map_err(sql)?;
    Ok(c.affected_rows() > 0)
}

pub fn propose_recipe(c: &mut Conn, game: u64, name: &str, labels: &str, action: &str, amount: &str) -> Result<u64, String> {
    c.exec_drop(
        "INSERT INTO worldsmith_recipes (game_id, name, labels, action, amount) VALUES (?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE labels = VALUES(labels), action = VALUES(action), amount = VALUES(amount)",
        (game, name, labels, action, amount),
    )
    .map_err(sql)?;
    c.exec_first("SELECT id FROM worldsmith_recipes WHERE game_id = ? AND name = ?", (game, name)).map_err(sql)?.ok_or_else(|| sql(()))
}

/// The member's Keep or No on a proposed recipe (the page's press, never the companion's).
pub fn decide_recipe(c: &mut Conn, id: u64, keep: bool) -> Result<(), String> {
    if keep {
        c.exec_drop("UPDATE worldsmith_recipes SET state = 'kept', decided_at = CURRENT_TIMESTAMP WHERE id = ?", (id,)).map_err(sql)
    } else {
        c.exec_drop("DELETE FROM worldsmith_recipes WHERE id = ? AND state = 'proposed'", (id,)).map_err(sql)
    }
}

pub fn session(c: &mut Conn, game: u64, steps: &str) -> Result<(), String> {
    c.exec_drop("INSERT INTO worldsmith_sessions (game_id, steps) VALUES (?, ?)", (game, steps)).map_err(sql)
}

/// The game remembered for this exe (the newest when it was patched), without remembering it. Read only.
pub fn game_id_by_exe(c: &mut Conn, exe: &str) -> Result<Option<u64>, String> {
    c.exec_first("SELECT id FROM worldsmith_games WHERE exe = ? ORDER BY last_played DESC, id DESC LIMIT 1", (exe,)).map_err(sql)
}

/// The most text and translation a remembered line keeps (the table's own widths).
pub const LINE_SOURCE_MAX: usize = 300;
pub const LINE_TRANSLATED_MAX: usize = 400;

/// A line as it is kept: trimmed and cut to its width; None when either side is empty. Pure.
pub fn line_kept(source: &str, translated: &str) -> Option<(String, String)> {
    let source: String = source.trim().chars().take(LINE_SOURCE_MAX).collect();
    let translated: String = translated.trim().chars().take(LINE_TRANSLATED_MAX).collect();
    (!source.is_empty() && !translated.is_empty()).then_some((source, translated))
}

/// The lines Translate read in this game before, newest first, so names stay the same next time.
pub fn remembered_lines(c: &mut Conn, game: u64, lang: &str, limit: usize) -> Result<Vec<(String, String)>, String> {
    c.exec_map(
        "SELECT source, translated FROM worldsmith_lines WHERE game_id = ? AND lang = ? ORDER BY seen_at DESC, id DESC LIMIT ?",
        (game, lang, limit as u64),
        |(source, translated): (String, String)| (source, translated),
    )
    .map_err(sql)
}

/// The lines just read, kept for the game (a line read again takes its newest translation).
pub fn remember_lines(c: &mut Conn, game: u64, lang: &str, lines: &[(String, String)]) -> Result<(), String> {
    let lang: String = lang.trim().chars().take(10).collect();
    let rows: Vec<(String, String)> = lines.iter().filter_map(|(s, t)| line_kept(s, t)).collect();
    if rows.is_empty() {
        return Ok(());
    }
    c.exec_batch(
        "INSERT INTO worldsmith_lines (game_id, source, translated, lang) VALUES (?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE translated = VALUES(translated), seen_at = CURRENT_TIMESTAMP",
        rows.iter().map(|(s, t)| (game, s, t, &lang)),
    )
    .map_err(sql)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRow {
    pub id: u64,
    pub exe: String,
    pub title: String,
    pub stamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueRow {
    pub label: String,
    pub value_type: String,
    pub transform: String,
    pub last_address: String,
    pub found_how: String,
    pub pin_x: Option<f64>,
    pub pin_y: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeRow {
    pub id: u64,
    pub name: String,
    pub labels: String,
    pub action: String,
    pub amount: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub games: Vec<GameRow>,
    pub values: Vec<ValueRow>,
    pub recipes: Vec<RecipeRow>,
}

/// What the page shows: the games remembered (newest first), and the values and recipes of one.
pub fn memory(c: &mut Conn, game: Option<u64>) -> Result<Memory, String> {
    let games = c
        .query_map("SELECT id, exe, title, stamp FROM worldsmith_games ORDER BY last_played DESC LIMIT 50", |(id, exe, title, stamp)| GameRow { id, exe, title, stamp })
        .map_err(sql)?;
    let Some(game) = game else { return Ok(Memory { games, ..Memory::default() }) };
    let values = c
        .exec_map(
            "SELECT label, value_type, transform, last_address, found_how, pin_x, pin_y FROM worldsmith_values WHERE game_id = ? ORDER BY label",
            (game,),
            |(label, value_type, transform, last_address, found_how, pin_x, pin_y)| ValueRow { label, value_type, transform, last_address, found_how, pin_x, pin_y },
        )
        .map_err(sql)?;
    let recipes = c
        .exec_map("SELECT id, name, labels, action, amount, state FROM worldsmith_recipes WHERE game_id = ? ORDER BY state DESC, name", (game,), |(id, name, labels, action, amount, state)| RecipeRow {
            id,
            name,
            labels,
            action,
            amount,
            state,
        })
        .map_err(sql)?;
    Ok(Memory { games, values, recipes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_addresses_and_places_are_checked_before_anything_is_kept() {
        assert_eq!(check_label("  Gold ").unwrap(), "Gold");
        assert!(check_label("").is_err() && check_label(&"x".repeat(61)).is_err() && check_label("a\nb").is_err());
        assert_eq!(check_addr("0x1a2f40").unwrap(), "0x1A2F40");
        assert!(check_addr("1A2F40").is_err() && check_addr("0x").is_err() && check_addr("0xZZ").is_err() && check_addr("0x11112222333344445").is_err());
        assert_eq!(check_share("0.55").unwrap(), 0.55);
        assert!(check_share("1.2").is_err() && check_share("NaN").is_err() && check_share("-0.1").is_err());
        assert_eq!(check_one("freeze", &ACTIONS, "--action").unwrap(), "freeze");
        assert_eq!(check_one("x9", &TRANSFORMS, "--transform").unwrap_err(), "--transform is one of: x10, x100, neg.");
    }

    #[test]
    fn a_remembered_line_fits_its_columns_and_is_never_empty() {
        assert_eq!(line_kept("  Shop ", " Kedai "), Some(("Shop".into(), "Kedai".into())));
        assert_eq!(line_kept("", "x"), None);
        assert_eq!(line_kept("x", "  "), None);
        let (s, t) = line_kept(&"あ".repeat(400), &"b".repeat(500)).unwrap();
        assert_eq!((s.chars().count(), t.chars().count()), (LINE_SOURCE_MAX, LINE_TRANSLATED_MAX));
    }
}
