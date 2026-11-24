//! The Telegram adapter for MemoryCore in your pocket: a bot the member messages from their
//! phone, answered by their own companion's real session (`bridges::send_and_await`). Off until
//! switched on in Settings > Messaging; the bot token lives in Windows Credential Manager like
//! every other key (Settings > Keys > Telegram bot token).
//! The first chat to message the bot is held as "pending" until the member approves it from
//! Settings - after that, only the approved chat gets replies. A leaked token or a found bot
//! would otherwise mean a stranger reaching the member's own machine, not just spending tokens.

use crate::settings::SettingsStore;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

const ACCOUNT: &str = "telegram-bridge";
/// Telegram holds `getUpdates` open this long when nothing is waiting; the request's own timeout
/// must be longer still, or Castle would cut the long poll off itself.
const LONG_POLL_SECS: u64 = 25;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// How long a reply is waited for before Telegram is told it took too long.
const REPLY_TIMEOUT: Duration = Duration::from_secs(180);
/// What `getUpdates` asks for: messages, and the member's reactions on the bot's replies (Telegram
/// sends reactions only when asked by name). `["message","message_reaction"]`, written for a URL.
const ALLOWED_UPDATES: &str = "%5B%22message%22%2C%22message_reaction%22%5D";

#[derive(Default)]
pub struct TelegramState {
    connected: Mutex<bool>,
    problem: Mutex<Option<String>>,
    /// A chat id that messaged the bot and is not yet approved, with the name Telegram gave for it.
    pending: Mutex<Option<(i64, String)>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPending {
    chat_id: i64,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStatus {
    on: bool,
    connected: bool,
    has_token: bool,
    paired: bool,
    pending: Option<TelegramPending>,
    problem: Option<String>,
}

fn api(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}

/// The chat id, its sender's text and the name Telegram gave, from one `getUpdates` item - or
/// None for anything that is not a plain text message (a photo, an edit, a channel post, ...).
fn incoming(update: &Value) -> Option<(i64, String, String)> {
    let message = update.get("message")?;
    let chat_id = message.pointer("/chat/id")?.as_i64()?;
    let text = message.get("text")?.as_str()?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let name = message.pointer("/from/first_name").and_then(Value::as_str).unwrap_or("Someone").to_string();
    Some((chat_id, text, name))
}

/// A photo from one `getUpdates` item: (chat id, the largest size's file id, its caption, the
/// sender's name). A size over 25 MB is passed over (Castle takes receipts up to that).
fn photo_of(update: &Value) -> Option<(i64, String, String, String)> {
    let message = update.get("message")?;
    let chat_id = message.pointer("/chat/id")?.as_i64()?;
    let sizes = message.get("photo")?.as_array()?;
    let largest = sizes
        .iter()
        .filter(|s| s.get("file_size").and_then(Value::as_u64).unwrap_or(0) <= crate::finance::companion::MAX_PICTURE)
        .max_by_key(|s| s.get("width").and_then(Value::as_u64).unwrap_or(0) * s.get("height").and_then(Value::as_u64).unwrap_or(0))?;
    let file_id = largest.get("file_id")?.as_str()?.to_string();
    let caption = message.get("caption").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let name = message.pointer("/from/first_name").and_then(Value::as_str).unwrap_or("Someone").to_string();
    Some((chat_id, file_id, caption, name))
}

/// Downloads a photo by its file id into Castle's own `telegram-media` folder (never the memory
/// folder: only the member's is moved on, by `bridges::take_picture`). The token goes only into
/// the request; it is never written down.
fn download_photo(client: &reqwest::blocking::Client, token: &str, store: &SettingsStore, file_id: &str, name: &str) -> Option<std::path::PathBuf> {
    let body: Value = client.get(format!("{}?file_id={file_id}", api(token, "getFile"))).send().ok()?.json().ok()?;
    let path = body.pointer("/result/file_path")?.as_str()?;
    if body.pointer("/result/file_size").and_then(Value::as_u64).unwrap_or(0) > crate::finance::companion::MAX_PICTURE {
        return None;
    }
    let response = client.get(format!("https://api.telegram.org/file/bot{token}/{path}")).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().ok()?;
    if bytes.len() as u64 > crate::finance::companion::MAX_PICTURE {
        return None;
    }
    let dir = store.folder().join("telegram-media");
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join(format!("{}.img", name.chars().filter(char::is_ascii_alphanumeric).collect::<String>()));
    std::fs::write(&file, &bytes).ok()?;
    Some(file)
}

/// The words a photo becomes: for the approved chat, the picture moved into the memory folder and
/// named with its caption; for anyone else nothing is downloaded and the line alone goes on (so a
/// stranger is told to wait for approval as with words).
fn photo_words(app: &AppHandle, client: &reqwest::blocking::Client, token: &str, chat_id: i64, file_id: &str, caption: &str, update_id: i64) -> String {
    let line = if caption.is_empty() { "(sent a picture)".to_string() } else { format!("(sent a picture) {caption}") };
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    let approved = settings.telegram_bridge_chat_id.as_deref().and_then(|id| id.parse::<i64>().ok()) == Some(chat_id);
    let (true, Some(memory)) = (approved, settings.memory_folder.as_deref()) else { return line };
    download_photo(client, token, &store, file_id, &update_id.to_string())
        .and_then(|saved| crate::bridges::take_picture(std::path::Path::new(memory), &saved, caption))
        .unwrap_or(line)
}

/// A reaction from one `getUpdates` item: (chat id, the reacted message's id, the emoji). An
/// empty emoji means the reaction was taken back; a custom or paid one counts as a star.
fn reaction_of(update: &Value) -> Option<(i64, i64, String)> {
    let reaction = update.get("message_reaction")?;
    let chat_id = reaction.pointer("/chat/id")?.as_i64()?;
    let message_id = reaction.get("message_id")?.as_i64()?;
    let new = reaction.get("new_reaction").and_then(Value::as_array).cloned().unwrap_or_default();
    let emoji = match new.first() {
        None => String::new(),
        Some(first) => first.get("emoji").and_then(Value::as_str).unwrap_or("⭐").to_string(),
    };
    Some((chat_id, message_id, emoji))
}

/// Sends one message; the id Telegram gave it, so a reaction on it can be placed later.
fn send_message(client: &reqwest::blocking::Client, token: &str, chat_id: i64, text: &str) -> Result<Option<i64>, String> {
    let response = client
        .post(api(token, "sendMessage"))
        .json(&json!({ "chat_id": chat_id, "text": text }))
        .send()
        .map_err(|_| "Telegram did not take the reply.".to_string())?;
    if !response.status().is_success() {
        return Err("Telegram refused the reply.".to_string());
    }
    let body: Value = response.json().unwrap_or_default();
    Ok(body.pointer("/result/message_id").and_then(Value::as_i64))
}

/// One incoming message: approved, it goes to the companion's real session and the answer (or
/// the trouble reaching it) is sent back; not yet approved, the sender is told so and held
/// pending for Settings to approve; a different, already-rejected-by-silence chat is ignored.
fn handle(app: &AppHandle, client: &reqwest::blocking::Client, token: &str, chat_id: i64, text: String, name: String, event: String) -> Result<(), String> {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    let approved = settings.telegram_bridge_chat_id.as_deref().and_then(|id| id.parse::<i64>().ok());
    match approved {
        Some(id) if id == chat_id => {
            if !crate::automations::triggers::incoming(app, "telegram", &chat_id.to_string(), &event, &text) { return Ok(()); }
            let reply = crate::bridges::send_and_await(app, &store, &text, REPLY_TIMEOUT).unwrap_or_else(|problem| problem);
            if let Some(id) = send_message(client, token, chat_id, &reply)? {
                crate::bridges::reactions::remember(app, "telegram", &id.to_string(), &reply);
            }
            Ok(())
        }
        Some(_) => Ok(()),
        None => {
            *app.state::<TelegramState>().pending.lock().unwrap() = Some((chat_id, name));
            send_message(client, token, chat_id, "Hi! Ask in Castle's Settings > Messaging to approve this chat before I can answer.").map(|_| ())
        }
    }
}

/// The first look after the bridge starts: asks for no waiting, takes note of the newest update
/// Telegram holds and answers nothing before it. Quick, so the card can say "Connected" at once,
/// and a message that waited while the bridge was off is not answered late.
fn catch_up(client: &reqwest::blocking::Client, token: &str) -> Result<i64, String> {
    let url = format!("{}?timeout=0&offset=-1&allowed_updates={ALLOWED_UPDATES}", api(token, "getUpdates"));
    let body: Value = client.get(url).send().map_err(|_| "Telegram did not answer.".to_string())?.json().map_err(|_| "Telegram sent something unreadable.".to_string())?;
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = body.get("description").and_then(Value::as_str).unwrap_or("Telegram refused the request.");
        return Err(message.to_string());
    }
    let newest = body["result"].as_array().and_then(|u| u.iter().filter_map(|x| x.get("update_id").and_then(Value::as_i64)).max());
    Ok(newest.map_or(0, |id| id + 1))
}

fn poll(app: &AppHandle, client: &reqwest::blocking::Client, token: &str, offset: &mut i64) -> Result<(), String> {
    let url = format!("{}?timeout={LONG_POLL_SECS}&offset={offset}&allowed_updates={ALLOWED_UPDATES}", api(token, "getUpdates"));
    let body: Value = client.get(url).send().map_err(|_| "Telegram did not answer.".to_string())?.json().map_err(|_| "Telegram sent something unreadable.".to_string())?;
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = body.get("description").and_then(Value::as_str).unwrap_or("Telegram refused the request.");
        return Err(message.to_string());
    }
    for update in body["result"].as_array().cloned().unwrap_or_default() {
        let Some(update_id) = update.get("update_id").and_then(Value::as_i64) else { continue };
        *offset = update_id + 1;
        if let Some((chat_id, text, name)) = incoming(&update) {
            let _ = handle(app, client, token, chat_id, text, name, update_id.to_string());
        } else if let Some((chat_id, file_id, caption, name)) = photo_of(&update) {
            let text = photo_words(app, client, token, chat_id, &file_id, &caption, update_id);
            let _ = handle(app, client, token, chat_id, text, name, update_id.to_string());
        } else if let Some((chat_id, message_id, emoji)) = reaction_of(&update) {
            // only the approved chat's hearts are shown
            let approved = app.state::<SettingsStore>().get().telegram_bridge_chat_id.and_then(|id| id.parse::<i64>().ok());
            if approved == Some(chat_id) {
                crate::bridges::reactions::record(app, "telegram", &message_id.to_string(), &emoji);
            }
        }
    }
    Ok(())
}

/// Starts the bridge with Castle; it only calls out while the switch in Settings is on.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut offset: Option<i64> = None;
        loop {
            let state = app.state::<TelegramState>();
            let settings = app.state::<SettingsStore>().get();
            let token = crate::key::load(ACCOUNT);
            let (true, Some(token)) = (settings.telegram_bridge_on, token) else {
                offset = None; // switched on again: do not answer messages sent while it was off
                *state.connected.lock().unwrap() = false;
                *state.problem.lock().unwrap() = None;
                std::thread::sleep(Duration::from_secs(3));
                continue;
            };
            let Ok(client) = reqwest::blocking::Client::builder().timeout(REQUEST_TIMEOUT).build() else {
                *state.problem.lock().unwrap() = Some("The Telegram call could not start.".into());
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let Some(next) = offset else {
                let outcome = catch_up(&client, &token);
                *state.connected.lock().unwrap() = outcome.is_ok();
                match outcome {
                    Ok(next) => {
                        offset = Some(next);
                        *state.problem.lock().unwrap() = None;
                    }
                    Err(problem) => {
                        *state.problem.lock().unwrap() = Some(problem);
                        std::thread::sleep(Duration::from_secs(3));
                    }
                }
                continue;
            };
            let mut next = next;
            let outcome = poll(&app, &client, &token, &mut next);
            offset = Some(next);
            *state.connected.lock().unwrap() = outcome.is_ok();
            *state.problem.lock().unwrap() = outcome.err();
        }
    });
}

#[tauri::command]
pub fn telegram_bridge_status(store: State<SettingsStore>, state: State<TelegramState>) -> TelegramStatus {
    let settings = store.get();
    TelegramStatus {
        on: settings.telegram_bridge_on,
        connected: *state.connected.lock().unwrap(),
        has_token: crate::key::load(ACCOUNT).is_some(),
        paired: settings.telegram_bridge_chat_id.is_some(),
        pending: state.pending.lock().unwrap().clone().map(|(chat_id, name)| TelegramPending { chat_id, name }),
        problem: state.problem.lock().unwrap().clone(),
    }
}

/// Approves the chat currently held pending (if any) as the only one the bridge answers.
#[tauri::command]
pub fn telegram_bridge_approve(store: State<SettingsStore>, state: State<TelegramState>) -> Result<(), String> {
    let Some((chat_id, _)) = state.pending.lock().unwrap().take() else {
        return Err("There is no chat waiting to be approved.".into());
    };
    let mut next = store.get();
    next.telegram_bridge_chat_id = Some(chat_id.to_string());
    crate::settings::save(&store.path, &next).map_err(|_| "The choice could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

/// Forgets the approved chat, so a new one can pair (e.g. the member switched phones).
#[tauri::command]
pub fn telegram_bridge_forget(store: State<SettingsStore>) -> Result<(), String> {
    let mut next = store.get();
    next.telegram_bridge_chat_id = None;
    crate::settings::save(&store.path, &next).map_err(|_| "The choice could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(id: i64, chat_id: i64, name: &str, text: &str) -> Value {
        json!({ "update_id": id, "message": { "chat": { "id": chat_id }, "from": { "first_name": name }, "text": text } })
    }

    #[test]
    fn the_api_address_carries_the_token_and_the_method() {
        assert_eq!(api("abc:123", "getUpdates"), "https://api.telegram.org/botabc:123/getUpdates");
        assert_eq!(api("abc:123", "sendMessage"), "https://api.telegram.org/botabc:123/sendMessage");
    }

    #[test]
    fn a_text_message_is_read_and_anything_else_is_left_out() {
        let got = incoming(&update(1, 42, "Rin", " hello ")).unwrap();
        assert_eq!(got, (42, "hello".to_string(), "Rin".to_string()));
        assert!(incoming(&update(2, 42, "Rin", "  ")).is_none());
        assert!(incoming(&json!({ "update_id": 3, "message": { "chat": { "id": 1 } } })).is_none());
        assert!(incoming(&json!({ "update_id": 4, "edited_message": { "chat": { "id": 1 }, "text": "edit" } })).is_none());
        // no name given: a plain fallback, never a panic
        let anon = incoming(&json!({ "update_id": 5, "message": { "chat": { "id": 7 }, "text": "hi" } })).unwrap();
        assert_eq!(anon.2, "Someone");
        // a reaction is not a message
        assert!(incoming(&json!({ "update_id": 6, "message_reaction": { "chat": { "id": 7 }, "message_id": 3 } })).is_none());
        // a photo is not a text message; photo_of reads it
        let photo = json!({ "update_id": 7, "message": { "chat": { "id": 42 }, "from": { "first_name": "Rin" }, "caption": " lunch ", "photo": [
            { "file_id": "small", "width": 90, "height": 120, "file_size": 2000 },
            { "file_id": "large", "width": 960, "height": 1280, "file_size": 180000 },
            { "file_id": "huge", "width": 9000, "height": 12000, "file_size": 30000000 }
        ] } });
        assert!(incoming(&photo).is_none());
        assert_eq!(photo_of(&photo), Some((42, "large".to_string(), "lunch".to_string(), "Rin".to_string())), "the largest under 25 MB");
        assert!(photo_of(&update(8, 42, "Rin", "hello")).is_none());
    }

    #[test]
    fn a_reaction_names_its_chat_its_message_and_its_emoji() {
        let heart = json!({ "update_id": 1, "message_reaction": { "chat": { "id": 42 }, "message_id": 77, "old_reaction": [], "new_reaction": [{ "type": "emoji", "emoji": "❤" }] } });
        assert_eq!(reaction_of(&heart), Some((42, 77, "❤".to_string())));
        let back = json!({ "update_id": 2, "message_reaction": { "chat": { "id": 42 }, "message_id": 77, "new_reaction": [] } });
        assert_eq!(reaction_of(&back), Some((42, 77, String::new())));
        let custom = json!({ "update_id": 3, "message_reaction": { "chat": { "id": 42 }, "message_id": 78, "new_reaction": [{ "type": "custom_emoji", "custom_emoji_id": "5" }] } });
        assert_eq!(reaction_of(&custom).unwrap().2, "⭐");
        assert!(reaction_of(&json!({ "update_id": 4, "message": { "chat": { "id": 1 }, "text": "hi" } })).is_none());
    }

    #[test]
    fn get_updates_asks_for_messages_and_reactions_by_name() {
        assert_eq!(ALLOWED_UPDATES.replace("%5B", "[").replace("%5D", "]").replace("%22", "\"").replace("%2C", ","), r#"["message","message_reaction"]"#);
    }
}

/// A notice may only use the chat still approved when it is sent.
pub(super) fn notify(store:&SettingsStore,expected:&str,text:&str)->Result<(),String>{
 let current=super::notification_target(&store.get(),"telegram")?;if current!=expected{return Err("The approved Telegram chat changed.".into());}let id=current.parse::<i64>().map_err(|_|"Approve a valid Telegram chat.")?;
 let token=crate::key::load(ACCOUNT).ok_or("The Telegram token is missing.")?;let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|_|"Telegram could not start.")?;
 let response=client.post(api(&token,"sendMessage")).json(&json!({"chat_id":id,"text":text})).send().map_err(|_|"Telegram did not take the notice.")?;
 if !response.status().is_success(){return Err("Telegram refused the notice.".into());}let body:Value=response.json().map_err(|_|"Telegram did not confirm the notice.")?;if body["ok"]==true{Ok(())}else{Err("Telegram did not confirm the notice.".into())}
}
