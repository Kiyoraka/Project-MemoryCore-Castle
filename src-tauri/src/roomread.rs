//! Reading the MemoryCore Foundation room from the moment Castle starts - not only while the
//! CHAT tab is open - so every speaker's lines are there when the member looks, and the
//! companion can answer while the member is away. A held read (20 s) while CHAT is shown, a quick
//! look every 30 s while it is hidden or in the tray. New lines go to the page as `room-lines`; a
//! read that fails is said plainly (`room-read-failed`), never swallowed.

use crate::chatroom::{Room, RoomRefusal};
use crate::key;
use crate::settings::SettingsStore;
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

const KEEP: usize = 500;

#[derive(Default)]
pub struct RoomReader {
    visible: AtomicBool,
    lines: Mutex<Vec<Value>>,
    cursor: Mutex<Option<u64>>,
    problem: Mutex<Option<RoomRefusal>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlog {
    pub lines: Vec<Value>,
    pub problem: Option<RoomRefusal>,
}

#[derive(Clone, Serialize)]
struct Fresh {
    lines: Vec<Value>,
    cleared: Vec<u64>,
}

fn id_of(line: &Value) -> Option<u64> {
    line.get("id").and_then(Value::as_u64)
}

/// Lines kept so far + one read's answer -> (all lines kept, the ones that are new). Pure, tested.
pub fn merge(kept: &[Value], answer: &Value) -> (Vec<Value>, Vec<Value>) {
    let cleared: Vec<u64> = answer.get("cleared").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    let mut all: Vec<Value> = kept.iter().filter(|l| id_of(l).is_none_or(|id| !cleared.contains(&id))).cloned().collect();
    let mut fresh = Vec::new();
    for line in answer.get("lines").and_then(Value::as_array).cloned().unwrap_or_default() {
        if id_of(&line).is_some_and(|id| !all.iter().any(|l| id_of(l) == Some(id))) {
            all.push(line.clone());
            fresh.push(line);
        }
    }
    all.sort_by_key(|l| id_of(l).unwrap_or(0));
    let keep = all.len().saturating_sub(KEEP);
    (all[keep..].to_vec(), fresh)
}

/// Starts the reader with Castle.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        let settings = app.state::<SettingsStore>().get();
        let token = key::load("member");
        let (true, Some(token)) = (settings.room_on("chat"), token) else {
            std::thread::sleep(Duration::from_secs(15));
            continue;
        };
        let reader = app.state::<RoomReader>();
        let visible = reader.visible.load(Ordering::Relaxed);
        let cursor = *reader.cursor.lock().unwrap();
        let room = Room::new(&crate::signin::portfolio(), &token);
        match room.lines(cursor, if visible && cursor.is_some() { 20 } else { 0 }) {
            Ok(answer) => {
                let kept = reader.lines.lock().unwrap().clone();
                let (all, fresh) = merge(&kept, &answer);
                *reader.lines.lock().unwrap() = all;
                if let Some(next) = answer.get("cursor").and_then(Value::as_u64) {
                    *reader.cursor.lock().unwrap() = Some(next);
                }
                *reader.problem.lock().unwrap() = None;
                let cleared: Vec<u64> = answer.get("cleared").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                if !fresh.is_empty() || !cleared.is_empty() {
                    let _ = app.emit("room-lines", Fresh { lines: fresh.clone(), cleared });
                }
                if !fresh.is_empty() && cursor.is_some() {
                    crate::roomturn::heard(&app, &fresh);
                    crate::roomalert::new_lines(&app, &fresh, visible);
                }
                let more = answer.get("more").and_then(Value::as_bool).unwrap_or(false);
                if !more && !reader.visible.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
            Err(refused) => {
                let pause = if refused.kind == "locked" { 60 } else { 10 };
                *reader.problem.lock().unwrap() = Some(refused.clone());
                let _ = app.emit("room-read-failed", refused);
                std::thread::sleep(Duration::from_secs(pause));
            }
        }
    });
}

/// The page says whether CHAT is on screen (held reads only then).
#[tauri::command]
pub fn room_visible(on: bool, reader: State<RoomReader>) {
    reader.visible.store(on, Ordering::Relaxed);
}

/// Everything read so far, for a CHAT tab that opens later.
#[tauri::command]
pub fn room_backlog(reader: State<RoomReader>) -> Backlog {
    Backlog { lines: reader.lines.lock().unwrap().clone(), problem: reader.problem.lock().unwrap().clone() }
}

/// After a sign-in or sign-out the reader starts over.
#[tauri::command]
pub fn room_reset(reader: State<RoomReader>) {
    reader.lines.lock().unwrap().clear();
    *reader.cursor.lock().unwrap() = None;
    *reader.problem.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_speakers_lines_are_kept_once_in_order_and_cleared_ones_go() {
        let kept = vec![json!({"id": 1, "text": "hi", "author": {"name": "Kiyo", "kind": "host"}}), json!({"id": 2, "text": "x"})];
        let answer = json!({"cursor": 4, "lines": [
            {"id": 4, "text": "hello", "author": {"name": "Rin's Mika", "kind": "member_assistant"}},
            {"id": 3, "text": "hey", "author": {"name": "Rin", "kind": "member"}},
            {"id": 1, "text": "hi again"}
        ], "cleared": [2]});
        let (all, fresh) = merge(&kept, &answer);
        assert_eq!(all.iter().map(|l| l["id"].as_u64().unwrap()).collect::<Vec<_>>(), vec![1, 3, 4]);
        assert_eq!(fresh.len(), 2, "a line already kept is not new again");
        assert_eq!(all[0]["text"], "hi");
    }
}
