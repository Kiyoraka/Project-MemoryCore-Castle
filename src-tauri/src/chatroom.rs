//! The MemoryCore Foundation chat room, from the member's side (the Portfolio's room contract,
//! /api/room/**). Every call goes from Rust with the member's room key from Credential Manager;
//! the key never reaches the page. Lines are plain text, shown as text.

use crate::key;
use crate::settings::SettingsStore;
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tauri::State;

const USER_AGENT: &str = concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION"));

/// What the page needs to know when a door says no - in words, with the kind of no.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoomRefusal {
    /// "locked" (not a member / revoked) | "muted" | "needsName" | "tooLarge" | "wrongType" |
    /// "slowDown" | "notSetUp" | "offline" | "aiQuiet" (a member's AI waits for a person, E.136) | "other"
    pub kind: String,
    pub message: String,
}

fn refusal(kind: &str, message: impl Into<String>) -> RoomRefusal {
    RoomRefusal { kind: kind.into(), message: message.into() }
}

/// Maps a door's status and its {"error": "..."} body to a plain refusal.
pub fn refusal_for(status: u16, body: &str) -> RoomRefusal {
    let answer = serde_json::from_str::<Value>(body).ok();
    let said = answer.as_ref().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
    let code = answer.as_ref().and_then(|v| v.get("code").and_then(Value::as_str)).unwrap_or("");
    match status {
        // the room's AI-only brake (E.136): a member's AI said its lines since a person spoke - stay quiet, never retry
        409 if code == "WAITING_FOR_A_PERSON" => refusal("aiQuiet", said.unwrap_or_else(|| "The room waits for a person to speak.".into())),
        403 if said.as_deref().is_some_and(|s| s.to_lowercase().contains("muted")) => refusal("muted", said.unwrap_or_default()),
        403 | 401 => refusal("locked", said.unwrap_or_else(|| "This room is for MemoryCore Foundation members".into())),
        409 => refusal("needsName", "Set your Foundation display name on the Portfolio to speak in the room."),
        413 => refusal("tooLarge", "That message is too long (1000 characters at most)."),
        415 => refusal("wrongType", "That file type is not allowed in the room."),
        // the room's own words when it gives them (a daily voice count says how many)
        429 => refusal("slowDown", said.unwrap_or_else(|| "Too many messages at once - wait a moment.".into())),
        _ => refusal("other", said.unwrap_or_else(|| format!("The room answered {status}."))),
    }
}

/// The room's address: the Portfolio address the member saved, plus /api/room.
pub fn room_url(base: &str, path: &str) -> String {
    format!("{}/api/room{}", base.trim_end_matches('/').trim_end_matches("/api"), path)
}

pub struct Room {
    base: String,
    key: String,
}

impl Room {
    pub fn new(base: &str, key: &str) -> Room {
        Room { base: base.to_string(), key: key.to_string() }
    }

    fn client(&self, timeout: Duration) -> Result<reqwest::blocking::Client, RoomRefusal> {
        reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .build()
            .map_err(|_| refusal("other", "The room client could not start."))
    }

    fn answer(response: reqwest::Result<reqwest::blocking::Response>) -> Result<reqwest::blocking::Response, RoomRefusal> {
        let response = response.map_err(|e| {
            if e.is_timeout() {
                refusal("offline", "The room did not answer in time.")
            } else {
                refusal("offline", "The room could not be reached. Check the address and the internet connection.")
            }
        })?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            Ok(response)
        } else {
            Err(refusal_for(status, &response.text().unwrap_or_default()))
        }
    }

    fn get_json(&self, path: &str, timeout: Duration) -> Result<Value, RoomRefusal> {
        let response = self.client(timeout)?.get(room_url(&self.base, path)).bearer_auth(&self.key).send();
        Self::answer(response)?.json().map_err(|_| refusal("other", "The room sent something unreadable."))
    }

    pub fn me(&self) -> Result<Value, RoomRefusal> {
        self.get_json("/me", Duration::from_secs(15))
    }

    /// The lines after `after` (the newest 50 without it); `wait` holds the read up to 25 s.
    pub fn lines(&self, after: Option<u64>, wait: u64) -> Result<Value, RoomRefusal> {
        self.lines_since(after, wait, None, None)
    }

    /// A read of the room that also asks what was cleared and what was reacted to since the
    /// cursors the last answer gave (`clearedAt`, `reactedAt`); a room without reactions ignores it.
    pub fn lines_since(&self, after: Option<u64>, wait: u64, cleared_after: Option<&str>, reacted_after: Option<&str>) -> Result<Value, RoomRefusal> {
        let wait = wait.min(25);
        // the HTTP timeout stays well above the held read
        self.get_json(&lines_path(after, wait, cleared_after, reacted_after), Duration::from_secs(wait + 10))
    }

    /// Adds (`on`) or removes the caller's reaction on a line: `{ id, reactions }`.
    pub fn react(&self, id: u64, emoji: &str, on: bool) -> Result<Value, RoomRefusal> {
        // the six, or any one emoji (a room that offers customReactions takes it; the room has the last word)
        if !REACTIONS.contains(&emoji) && !one_emoji(emoji) {
            return Err(refusal("other", "That reaction is not one the room knows."));
        }
        let url = room_url(&self.base, &format!("/lines/{id}/reactions/{}", encode_part(emoji)));
        let client = self.client(Duration::from_secs(20))?;
        let request = if on { client.put(url) } else { client.delete(url) };
        Self::answer(request.bearer_auth(&self.key).send())?.json().map_err(|_| refusal("other", "The room sent something unreadable."))
    }

    /// A picture line (multipart `{text?, picture}`), for a member's AI within its daily cap: the
    /// picture is made to fit a line first (1.5 MB at most, 1024 px).
    pub fn post_picture(&self, text: Option<&str>, bytes: Vec<u8>, kind: &'static str) -> Result<Value, RoomRefusal> {
        let (bytes, kind) = fit_picture(bytes, kind, ROOM_LINE_PICTURE_MAX, ROOM_LINE_PICTURE_SIDE).map_err(|e| refusal("tooLarge", &e))?;
        let ext = if kind == "image/png" { "png" } else if kind == "image/webp" { "webp" } else { "jpg" };
        let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name(format!("picture.{ext}")).mime_str(kind).map_err(|_| refusal("other", "The picture could not be sent."))?;
        let mut form = reqwest::blocking::multipart::Form::new().part("picture", part);
        if let Some(words) = text.map(str::trim).filter(|w| !w.is_empty()) {
            form = form.text("text", words.chars().take(1000).collect::<String>());
        }
        let response = self.client(Duration::from_secs(60))?.post(room_url(&self.base, "/lines")).bearer_auth(&self.key).multipart(form).send();
        Self::answer(response)?.json().map_err(|_| refusal("other", "The room sent something unreadable."))
    }

    /// A voice line from the member (multipart `{text?, replyTo?, voice}`): mp3 only, 2 MB at most,
    /// checked here before anything is sent. The room counts the member's day (429 past it).
    pub fn post_voice(&self, bytes: Vec<u8>, text: Option<&str>, reply_to: Option<u64>) -> Result<Value, RoomRefusal> {
        check_voice(&bytes)?;
        let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name("voice.mp3").mime_str("audio/mpeg").map_err(|_| refusal("other", "The voice could not be sent."))?;
        let mut form = reqwest::blocking::multipart::Form::new().part("voice", part);
        if let Some(words) = text.map(str::trim).filter(|w| !w.is_empty()) {
            form = form.text("text", words.chars().take(1000).collect::<String>());
        }
        if let Some(id) = reply_to {
            form = form.text("replyTo", id.to_string());
        }
        let response = self.client(Duration::from_secs(60))?.post(room_url(&self.base, "/lines")).bearer_auth(&self.key).multipart(form).send();
        Self::answer(response).map_err(voice_refusal)?.json().map_err(|_| refusal("other", "The room sent something unreadable."))
    }

    /// The same for voices (a member's AI only): (voices a day, voices today).
    pub fn voice_day(&self) -> Option<(u64, u64)> {
        media_voices(&self.me().ok()?)
    }

    /// What this key may still post today (a member's AI only): (pictures a day, pictures today).
    pub fn picture_day(&self) -> Option<(u64, u64)> {
        media_pictures(&self.me().ok()?)
    }

    pub fn presence(&self) -> Result<Value, RoomRefusal> {
        self.get_json("/presence", Duration::from_secs(15))
    }

    pub fn post(&self, text: &str) -> Result<Value, RoomRefusal> {
        self.post_reply(text, None)
    }

    /// A line, as a reply to `reply_to` when given (the room keeps a quote of it).
    pub fn post_reply(&self, text: &str, reply_to: Option<u64>) -> Result<Value, RoomRefusal> {
        let text = text.trim();
        if text.is_empty() {
            return Err(refusal("other", "Write something first."));
        }
        if text.chars().count() > 1000 {
            return Err(refusal("tooLarge", "That message is too long (1000 characters at most)."));
        }
        let response = self
            .client(Duration::from_secs(20))?
            .post(room_url(&self.base, "/lines"))
            .bearer_auth(&self.key)
            .json(&post_body(text, reply_to))
            .send();
        Self::answer(response)?.json().map_err(|_| refusal("other", "The room sent something unreadable."))
    }

    /// A room picture or voice as a data URL (fetched here, so the page never calls the Portfolio).
    pub fn file(&self, id: &str) -> Result<String, RoomRefusal> {
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(refusal("other", "That is not a room file."));
        }
        let response = self.client(Duration::from_secs(30))?.get(room_url(&self.base, &format!("/files/{id}"))).bearer_auth(&self.key).send();
        let response = Self::answer(response)?;
        let kind = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        if !(kind.starts_with("image/") || kind.starts_with("audio/")) {
            return Err(refusal("wrongType", "That room file is not a picture or a voice."));
        }
        let bytes = response.bytes().map_err(|_| refusal("offline", "The file could not be read."))?;
        Ok(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }

    /// Who has read how far: [{name, kind, seen, avatar}] - the ticks.
    pub fn seen(&self) -> Result<Value, RoomRefusal> {
        self.get_json("/seen", Duration::from_secs(15))
    }

    /// A speaker's face (by its avatar token) as a data URL.
    pub fn avatar(&self, token: &str) -> Result<String, RoomRefusal> {
        if token.is_empty() || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(refusal("other", "That is not a face."));
        }
        let response = self.client(Duration::from_secs(20))?.get(room_url(&self.base, &format!("/avatars/{token}"))).bearer_auth(&self.key).send();
        let response = Self::answer(response)?;
        let kind = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        if !kind.starts_with("image/") {
            return Err(refusal("wrongType", "That face is not a picture."));
        }
        let bytes = response.bytes().map_err(|_| refusal("offline", "The face could not be read."))?;
        Ok(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }
}

/// The most a room face door takes, and the side a larger picture is shrunk to.
pub const ROOM_PICTURE_MAX: usize = 512 * 1024;
const ROOM_PICTURE_SIDE: u32 = 512;
/// The same for a picture line (the room takes 2 MB; Castle keeps well under it).
pub const ROOM_LINE_PICTURE_MAX: usize = 1536 * 1024;
const ROOM_LINE_PICTURE_SIDE: u32 = 1024;

/// `me().media.caps.pictures` and `me().media.today.pictures` (a member's AI only). Pure, tested.
/// The reactions the room knows (the room server holds the same list).
pub const REACTIONS: [&str; 6] = ["👍", "❤️", "👏", "😂", "😮", "😢"];

/// Whether a reaction looks like one emoji: not empty, at most 32 bytes, no space, control or ASCII
/// letter, and at least one character at or above U+2000 (emoji, flags, keycaps). A loose check
/// before the request; the room decides. Pure, tested.
pub fn one_emoji(emoji: &str) -> bool {
    !emoji.is_empty()
        && emoji.len() <= 32
        && !emoji.chars().any(|c| c.is_whitespace() || c.is_control() || c.is_ascii_alphabetic())
        && emoji.chars().any(|c| c as u32 >= 0x2000)
}

/// What the room says it can do (`/me`'s `features`); none from a room that says nothing. Pure.
pub fn features(me: &Value) -> Vec<String> {
    me.get("features").and_then(Value::as_array).map(|f| f.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// The read's path with its cursors. Pure, tested.
pub fn lines_path(after: Option<u64>, wait: u64, cleared_after: Option<&str>, reacted_after: Option<&str>) -> String {
    let mut path = match after {
        Some(id) => format!("/lines?after={id}&wait={wait}"),
        None => format!("/lines?wait={wait}"),
    };
    if let Some(at) = cleared_after.filter(|a| !a.is_empty()) {
        path.push_str(&format!("&clearedAfter={}", encode_part(at)));
    }
    if let Some(at) = reacted_after.filter(|a| !a.is_empty()) {
        path.push_str(&format!("&reactedAfter={}", encode_part(at)));
    }
    path
}

/// The body of a post, with `replyTo` only when it is a reply. Pure, tested.
pub fn post_body(text: &str, reply_to: Option<u64>) -> Value {
    match reply_to {
        Some(id) => serde_json::json!({ "text": text, "replyTo": id }),
        None => serde_json::json!({ "text": text }),
    }
}

fn encode_part(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The most a voice line may weigh (the room's own limit for `voice`).
pub const ROOM_LINE_VOICE_MAX: usize = 2 * 1024 * 1024;

/// Whether the bytes are an mp3: an ID3 tag, or an MPEG audio frame sync (11 bits set). Pure, tested.
pub fn is_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
}

/// A voice the room would take: not empty, mp3, within the limit. Pure, tested.
pub fn check_voice(bytes: &[u8]) -> Result<(), RoomRefusal> {
    if bytes.is_empty() {
        return Err(refusal("other", "That recording is empty."));
    }
    if bytes.len() > ROOM_LINE_VOICE_MAX {
        return Err(refusal("tooLarge", "That voice is too long to send (2 MB at most)."));
    }
    if !is_mp3(bytes) {
        return Err(refusal("wrongType", "That recording is not an mp3."));
    }
    Ok(())
}

/// A refusal from the voice door in words about a voice: the room's own sentence for its daily
/// count (429) and for voices switched off (403), the size said as a voice's. Pure, tested.
pub fn voice_refusal(no: RoomRefusal) -> RoomRefusal {
    match no.kind.as_str() {
        "tooLarge" => refusal("tooLarge", "That voice is too long to send (2 MB at most)."),
        "wrongType" => refusal("wrongType", "The room did not take that recording."),
        _ => no,
    }
}

/// `me().media.caps.voices` and `me().media.today.voices`, when the room gives them. Pure, tested.
pub fn media_voices(me: &Value) -> Option<(u64, u64)> {
    let media = me.get("media")?;
    Some((media.pointer("/caps/voices")?.as_u64()?, media.pointer("/today/voices").and_then(Value::as_u64).unwrap_or(0)))
}

pub fn media_pictures(me: &Value) -> Option<(u64, u64)> {
    let media = me.get("media")?;
    Some((media.pointer("/caps/pictures")?.as_u64()?, media.pointer("/today/pictures").and_then(Value::as_u64).unwrap_or(0)))
}

/// The picture's kind from its file name (png, jpeg or webp - what the face doors take).
pub fn picture_kind(picture: &std::path::Path) -> Option<&'static str> {
    match picture.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

/// A picture made ready for a room face door (pure, tested): one within the limit goes as it is;
/// a larger one (a drawn portrait is about 2 MB) is shrunk to 512 px - PNG when it has see-through
/// parts and still fits, otherwise JPEG.
pub fn fit_for_room(bytes: Vec<u8>, kind: &'static str) -> Result<(Vec<u8>, &'static str), String> {
    fit_picture(bytes, kind, ROOM_PICTURE_MAX, ROOM_PICTURE_SIDE)
}

/// A picture within `max` bytes goes as it is; a larger one is shrunk to `side` px - PNG when it
/// has see-through parts and still fits, otherwise JPEG. Pure, tested.
pub fn fit_picture(bytes: Vec<u8>, kind: &'static str, max: usize, side: u32) -> Result<(Vec<u8>, &'static str), String> {
    if bytes.len() <= max {
        return Ok((bytes, kind));
    }
    let unreadable = || "That picture could not be read (png, jpg or webp).".to_string();
    let picture = image::load_from_memory(&bytes).map_err(|_| unreadable())?;
    let small = picture.resize(side, side, image::imageops::FilterType::Lanczos3);
    if small.color().has_alpha() {
        let mut png = Vec::new();
        small.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|_| unreadable())?;
        if png.len() <= max {
            return Ok((png, "image/png"));
        }
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode_image(&small.to_rgb8()).map_err(|_| unreadable())?;
    if jpeg.len() > max {
        return Err("That picture is too large even when shrunk - choose a smaller one.".into());
    }
    Ok((jpeg, "image/jpeg"))
}

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder().user_agent(USER_AGENT).timeout(timeout).build().map_err(|_| "Castle could not reach the internet.".to_string())
}

/// Sends a picture (png, jpeg or webp; shrunk when larger than the door takes) to a Portfolio
/// door as multipart "file".
fn upload_picture(base: &str, token: &str, path: &str, picture: &std::path::Path) -> Result<(), String> {
    let kind = picture_kind(picture).ok_or("Choose a PNG, JPEG or WebP picture.")?;
    let bytes = std::fs::read(picture).map_err(|_| "The picture could not be read.".to_string())?;
    let (bytes, kind) = fit_for_room(bytes, kind)?;
    let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name("face").mime_str(kind).map_err(|_| "The picture could not be sent.".to_string())?;
    let form = reqwest::blocking::multipart::Form::new().part("file", part);
    let answer = http_client(Duration::from_secs(30))?.post(format!("{base}{path}")).bearer_auth(token).multipart(form).send().map_err(|_| "The Portfolio could not be reached.".to_string())?;
    let status = answer.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(refusal_for(status, &answer.text().unwrap_or_default()).message)
}

/// Takes a face off a door (back to the letter circle, or the member's Google picture).
fn clear_picture(base: &str, token: &str, path: &str) -> Result<(), String> {
    let answer = http_client(Duration::from_secs(20))?.delete(format!("{base}{path}")).bearer_auth(token).send().map_err(|_| "The Portfolio could not be reached.".to_string())?;
    if answer.status().is_success() {
        Ok(())
    } else {
        Err(refusal_for(answer.status().as_u16(), &answer.text().unwrap_or_default()).message)
    }
}

/// A room key id is only ever used in a path when it is plain (letters, digits, dashes).
fn plain_id(id: Option<String>) -> Option<String> {
    id.filter(|i| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// The door for the companion's face.
fn companion_face_door(id: &str) -> String {
    format!("/api/member/room-keys/{id}/avatar")
}

/// Makes the companion's room key with the member's sign-in: (its id, the key itself).
fn make_companion_key(base: &str, token: &str, companion: &str) -> Result<(Option<String>, String), String> {
    let answer = http_client(Duration::from_secs(20))?
        .post(format!("{base}/api/member/room-keys"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "label": "MemoryCore Castle", "speaksAs": "ASSISTANT", "assistantName": companion }))
        .send()
        .map_err(|_| "The Portfolio could not be reached.".to_string())?;
    let status = answer.status().as_u16();
    let body = answer.text().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(key_refusal(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "The Portfolio's answer could not be read.".to_string())?;
    let new_key = v.get("key").and_then(Value::as_str).ok_or("The Portfolio's answer had no key.")?.to_string();
    Ok((v.get("id").map(|id| id.to_string().trim_matches('"').to_string()), new_key))
}

/// Removes one of the member's room keys on the Portfolio. Gone already (404) counts as removed;
/// anything else is an error, so Castle keeps the id and can try again instead of leaving a key
/// behind that the member can no longer see from here.
fn delete_key(base: &str, token: &str, id: &str) -> Result<(), String> {
    let answer = http_client(Duration::from_secs(20))?
        .delete(format!("{base}/api/member/room-keys/{id}"))
        .bearer_auth(token)
        .send()
        .map_err(|_| "The Portfolio could not be reached, so the old room key is still there. Try again.".to_string())?;
    delete_outcome(answer.status().as_u16())
}

/// A delete's status as Castle treats it (pure, tested).
pub fn delete_outcome(status: u16) -> Result<(), String> {
    match status {
        200..=299 | 404 => Ok(()),
        401 => Err("Your sign-in has run out - sign in again.".into()),
        s => Err(format!("The old room key could not be removed ({s}). Try again.")),
    }
}

/// Does the companion's room key need remaking? Yes when the name it carries is not the name in
/// Settings (pure, tested). A key made before Castle remembered the name is remade once.
pub fn needs_rename(carried: Option<&str>, current: &str) -> bool {
    carried.map(str::trim) != Some(current.trim())
}

/// What a refresh did: the new key when the name changed, and how the picture went.
pub struct Refreshed {
    pub new_key: Option<(Option<String>, String)>,
    pub face: Result<(), String>,
}

/// Brings the room up to date with Settings: a new name remakes the key (a new one first, then the
/// old one removed - the Portfolio has no rename door, and a failed make leaves the old key as it
/// was), then the picture goes to the key's face door, or the face is cleared when there is none.
pub fn refresh_companion(base: &str, token: &str, key_id: &str, rename_to: Option<&str>, picture: Option<&std::path::Path>) -> Result<Refreshed, String> {
    let mut id = key_id.to_string();
    let mut new_key = None;
    if let Some(name) = rename_to {
        let (made_id, made_key) = make_companion_key(base, token, name)?;
        // a key already gone on the Portfolio is fine
        let _ = http_client(Duration::from_secs(20))?.delete(format!("{base}/api/member/room-keys/{key_id}")).bearer_auth(token).send();
        if let Some(fresh) = plain_id(made_id.clone()) {
            id = fresh;
        }
        new_key = Some((made_id, made_key));
    }
    let door = companion_face_door(&id);
    let face = match picture {
        Some(p) => upload_picture(base, token, &door, p),
        None => clear_picture(base, token, &door),
    };
    Ok(Refreshed { new_key, face })
}

/// The member's own posted line ids (their lines are known by id, never by a display name).
fn mine_file(store: &SettingsStore) -> std::path::PathBuf {
    store.folder().join("room-mine.json")
}

pub fn my_line_ids(store: &SettingsStore) -> Vec<u64> {
    std::fs::read_to_string(mine_file(store)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn remember_mine(store: &SettingsStore, id: u64) {
    let mut ids = my_line_ids(store);
    if !ids.contains(&id) {
        ids.push(id);
    }
    let keep = ids.len().saturating_sub(300);
    let _ = std::fs::write(mine_file(store), serde_json::to_string(&ids[keep..]).unwrap_or_default());
}

/// The member's room (who: "me" with their sign-in, "companion" with the key Castle made for it).
fn room_for(store: &SettingsStore, who: &str) -> Result<Room, RoomRefusal> {
    if !store.get().room_on("chat") {
        return Err(refusal("off", crate::rooms::off_line("chat")));
    }
    let base = crate::signin::portfolio();
    let account = if who == "companion" { "room-companion" } else { "member" };
    let key = key::load(account).ok_or_else(|| {
        refusal("notSetUp", if who == "companion" { "Your companion is not in the room yet." } else { "Sign in to enter the room." })
    })?;
    Ok(Room::new(&base, &key))
}

/// The page's plain line when a room key could not be made (pure, tested).
pub fn key_refusal(status: u16, body: &str) -> String {
    let said = serde_json::from_str::<Value>(body).ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
    match status {
        401 => "Your sign-in has run out - sign in again.".into(),
        403 => said.unwrap_or_else(|| "Only MemoryCore Foundation members can let a companion in.".into()),
        _ => match said {
            Some(s) if s.to_lowercase().contains("key") => format!("{s} - you can remove an old room key on your Library card (the Room keys tab)."),
            Some(s) => s,
            None => format!("The companion's room key could not be made ({status})."),
        },
    }
}

/// Lets the member's companion into the room (Castle makes its own room key with the member's
/// sign-in, speaksAs ASSISTANT, and keeps it in Credential Manager - never shown), or keeps it out
/// again (the key is removed on the Portfolio and here).
#[tauri::command]
pub async fn room_companion_in(on: bool, store: State<'_, SettingsStore>) -> Result<bool, String> {
    let settings = store.get();
    if !settings.room_on("chat") {
        return Err(crate::rooms::off_line("chat"));
    }
    let token = key::load("member").ok_or("Sign in to the room first.")?;
    let base = crate::signin::portfolio();
    let companion = settings.companion_label();
    let had_id = settings.room_companion_key_id.clone();
    let picture = crate::commands::companion_picture(&store);
    let made = tauri::async_runtime::spawn_blocking(move || -> Result<Option<String>, String> {
        if on {
            if key::load("room-companion").is_some() {
                return Ok(had_id);
            }
            // a key Castle made before but lost here (its id kept, the key itself gone from the
            // Keychain) is removed first, so letting the companion in never adds a second key
            if let Some(old) = plain_id(had_id) {
                delete_key(&base, &token, &old)?;
            }
            let (id, new_key) = make_companion_key(&base, &token, &companion)?;
            if let Err(why) = key::save("room-companion", &new_key) {
                // the Portfolio made the key but Castle could not keep it: take it back off the
                // Portfolio, or every retry would leave one more key there
                if let Some(made) = plain_id(id.clone()) {
                    let _ = delete_key(&base, &token, &made);
                }
                return Err(why);
            }
            // the picture chosen in Settings goes with it; a failed upload never undoes the "Let in"
            if let (Some(door_id), Some(p)) = (plain_id(id.clone()), picture) {
                let _ = upload_picture(&base, &token, &companion_face_door(&door_id), &p);
            }
            Ok(id)
        } else {
            if let Some(id) = plain_id(had_id) {
                // only forgotten here once the Portfolio has let it go (or never had it)
                delete_key(&base, &token, &id)?;
            }
            let _ = key::forget("room-companion");
            Ok(None)
        }
    })
    .await
    .map_err(|_| "The room call stopped.".to_string())??;
    let mut next = store.get();
    let made_now = made.is_some() && next.room_companion_key_id != made;
    if made.is_none() {
        next.room_companion_name = None;
    } else if made_now {
        next.room_companion_name = Some(next.companion_label());
    }
    next.room_companion_key_id = made;
    crate::settings::save(&store.path, &next).map_err(|_| "The settings could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(on)
}

/// What a refresh tells the page.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionRefresh {
    /// false when the companion is not in the room (nothing to bring up to date)
    pub in_room: bool,
    pub renamed: bool,
    /// the picture went (or the face was cleared)
    pub face: bool,
}

/// After the companion's name or picture changes in Settings: brings the room up to date (a new
/// name remakes its room key; the picture goes to its face door). Nothing happens while it is out
/// of the room. A failed picture is said in words; a rename that worked is kept either way.
#[tauri::command]
pub async fn room_companion_refresh(store: State<'_, SettingsStore>) -> Result<CompanionRefresh, String> {
    let settings = store.get();
    let Some(key_id) = plain_id(settings.room_companion_key_id.clone()) else {
        return Ok(CompanionRefresh::default());
    };
    if !settings.room_on("chat") {
        return Ok(CompanionRefresh::default());
    }
    let token = key::load("member").ok_or("Sign in to the room first.")?;
    let base = crate::signin::portfolio();
    let name = settings.companion_label();
    let rename_to = needs_rename(settings.room_companion_name.as_deref(), &name).then(|| name.clone());
    let picture = crate::commands::companion_picture(&store);
    let done = tauri::async_runtime::spawn_blocking(move || refresh_companion(&base, &token, &key_id, rename_to.as_deref(), picture.as_deref()))
        .await
        .map_err(|_| "The room call stopped.".to_string())??;
    let renamed = done.new_key.is_some();
    if let Some((id, new_key)) = done.new_key {
        key::save("room-companion", &new_key)?;
        let mut next = store.get();
        if let Some(id) = plain_id(id) {
            next.room_companion_key_id = Some(id);
        }
        next.room_companion_name = Some(name);
        crate::settings::save(&store.path, &next).map_err(|_| "The settings could not be saved.".to_string())?;
        *store.current.lock().unwrap() = next;
    }
    match done.face {
        Ok(()) => Ok(CompanionRefresh { in_room: true, renamed, face: true }),
        Err(why) => Err(format!("The room did not take the picture: {why}")),
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, RoomRefusal> + Send + 'static) -> Result<T, RoomRefusal> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|_| refusal("other", "The room call stopped."))?
}

#[tauri::command]
pub async fn room_me(who: Option<String>, store: State<'_, SettingsStore>) -> Result<Value, RoomRefusal> {
    let who = who.as_deref().unwrap_or("me");
    if !["me", "companion"].contains(&who) { return Err(refusal("other", "That room profile is not available.")); }
    let room = room_for(&store, who)?;
    blocking(move || room.me()).await
}

#[tauri::command]
pub async fn room_lines(after: Option<u64>, wait: u64, store: State<'_, SettingsStore>) -> Result<Value, RoomRefusal> {
    let room = room_for(&store, "me")?;
    blocking(move || room.lines(after, wait)).await
}

#[tauri::command]
pub async fn room_presence(store: State<'_, SettingsStore>) -> Result<Value, RoomRefusal> {
    let room = room_for(&store, "me")?;
    blocking(move || room.presence()).await
}

/// Posts a line as the member ("me") or as their companion ("companion", its own room key).
#[tauri::command]
pub async fn room_post(text: String, who: Option<String>, reply_to: Option<u64>, store: State<'_, SettingsStore>) -> Result<Value, RoomRefusal> {
    let as_member = who.as_deref().unwrap_or("me") == "me";
    let room = room_for(&store, who.as_deref().unwrap_or("me"))?;
    let line = blocking(move || room.post_reply(&text, reply_to)).await?;
    if as_member {
        if let Some(id) = line.get("id").and_then(Value::as_u64) {
            remember_mine(&store, id);
        }
    }
    Ok(line)
}

/// The member's reaction on a line, added (`on`) or taken back: `{ id, reactions }`.
#[tauri::command]
pub async fn room_react(id: u64, emoji: String, on: bool, store: State<'_, SettingsStore>) -> Result<Value, RoomRefusal> {
    let room = room_for(&store, "me")?;
    blocking(move || room.react(id, &emoji, on)).await
}

/// The ids of the lines this member posted from Castle.
#[tauri::command]
pub fn room_mine(store: State<SettingsStore>) -> Vec<u64> {
    my_line_ids(&store)
}

#[tauri::command]
pub async fn room_seen(store: State<'_, SettingsStore>) -> Result<Value, RoomRefusal> {
    let room = room_for(&store, "me")?;
    blocking(move || room.seen()).await
}

static FACES: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> = std::sync::Mutex::new(None);

/// The Portfolio's Foundation profile contract: preserve the other editable fields on PUT.
pub fn display_name_payload(profile: &Value, name: &str) -> Result<Value, RoomRefusal> {
    let name = name.trim();
    if name.chars().count() > 80 { return Err(refusal("other", "Use at most 80 characters for your display name.")); }
    let mut body = serde_json::Map::new();
    for field in ["partnerName", "portfolioUrl", "built", "listed"] {
        let value = profile.get(field).ok_or_else(|| refusal("other", "Your Foundation profile could not be read. Nothing was changed."))?;
        if (field == "listed" && !value.is_boolean()) || (field != "listed" && !value.is_string() && !value.is_null()) {
            return Err(refusal("other", "Your Foundation profile could not be read. Nothing was changed."));
        }
        body.insert(field.into(), value.clone());
    }
    body.insert("displayName".into(), Value::String(name.into()));
    Ok(Value::Object(body))
}

pub fn change_display_name(base: &str, token: &str, name: &str) -> Result<(), RoomRefusal> {
    if name.trim().chars().count() > 80 { return Err(refusal("other", "Use at most 80 characters for your display name.")); }
    let room = Room::new(base, token);
    let client = room.client(Duration::from_secs(20))?;
    let url = format!("{}/api/member/foundation", base.trim_end_matches('/'));
    let profile: Value = Room::answer(client.get(&url).bearer_auth(token).send())?.json().map_err(|_| refusal("other", "Your Foundation profile could not be read."))?;
    let payload = display_name_payload(&profile, name)?;
    Room::answer(client.put(&url).bearer_auth(token).json(&payload).send())?;
    Ok(())
}

#[tauri::command]
pub async fn room_name_set(name: String, store: State<'_, SettingsStore>) -> Result<(), RoomRefusal> {
    crate::rooms::need(&store, "chat").map_err(|e| refusal("other", e))?;
    let token = key::load("member").ok_or_else(|| refusal("locked", "Sign in to the room first."))?;
    let base = crate::signin::portfolio();
    blocking(move || change_display_name(&base, &token, &name)).await?;
    // the chat alerts listen for the member's room name: the kept one is out of date now
    crate::roomalert::forget_name();
    Ok(())
}

/// A face by its avatar token, cached for good (a new picture comes with a new token).
#[tauri::command]
pub async fn room_avatar(token: String, store: State<'_, SettingsStore>) -> Result<String, RoomRefusal> {
    if let Some(found) = FACES.lock().unwrap().as_ref().and_then(|m| m.get(&token).cloned()) {
        return Ok(found);
    }
    let room = room_for(&store, "me")?;
    let key = token.clone();
    let face = blocking(move || room.avatar(&key)).await?;
    FACES.lock().unwrap().get_or_insert_with(Default::default).insert(token, face.clone());
    Ok(face)
}

/// The member's own face in the room: a picture they choose (`path`), or back to their Google
/// picture (`path` = None). `companion` = true sets their companion's picture instead.
#[tauri::command]
pub async fn room_face_set(path: Option<String>, companion: bool, store: State<'_, SettingsStore>) -> Result<(), String> {
    crate::rooms::need(&store, "chat")?;
    let token = key::load("member").ok_or("Sign in to the room first.")?;
    let base = crate::signin::portfolio();
    let key_id = store.get().room_companion_key_id.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let door = if companion {
            companion_face_door(&plain_id(key_id).ok_or("Let your companion in first.")?)
        } else {
            "/api/member/avatar".to_string()
        };
        match path {
            Some(p) => upload_picture(&base, &token, &door, std::path::Path::new(&p)),
            None => clear_picture(&base, &token, &door),
        }
    })
    .await
    .map_err(|_| "The picture could not be sent.".to_string())?
}

/// A room file's id as the page hands it over: the room sends `fileId` as a number
/// (`"picture": {"fileId": 42}`), and text is taken too. Anything else is no file. Pure, tested.
fn file_id(id: &Value) -> Option<String> {
    match id {
        Value::Number(n) => n.as_u64().map(|n| n.to_string()),
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// A picture or voice line from the room, as a data URL. The id arrives as the room wrote it (a
/// number): a `String` parameter refused it before any request, so pictures and voices never showed.
#[tauri::command]
pub async fn room_file(id: Value, store: State<'_, SettingsStore>) -> Result<String, RoomRefusal> {
    let id = file_id(&id).ok_or_else(|| refusal("other", "That is not a room file."))?;
    let room = room_for(&store, "me")?;
    blocking(move || room.file(&id)).await
}

/// The biggest room picture Castle will save, in bytes.
const SAVE_MAX: usize = 25 * 1024 * 1024;

/// The picture inside a data address (`data:image/jpeg;base64,...`, how the room's pictures reach the
/// page), as its type and bytes. Only the picture types the room serves, and not past SAVE_MAX. Pure, tested.
pub fn picture_from_data_address(address: &str) -> Option<(&'static str, Vec<u8>)> {
    let rest = address.trim().strip_prefix("data:")?;
    let (head, body) = rest.split_once(',')?;
    let mut parts = head.split(';');
    let mime = match parts.next()?.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "image/jpeg",
        "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        _ => return None,
    };
    if !parts.any(|p| p.trim().eq_ignore_ascii_case("base64")) || body.len() > SAVE_MAX / 3 * 4 + 8 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(body.trim()).ok()?;
    (!bytes.is_empty() && bytes.len() <= SAVE_MAX).then_some((mime, bytes))
}

/// A file name for a saved picture: plain letters, digits, spaces, dashes and underscores, cut short,
/// with the extension its type needs. Pure, tested.
pub fn picture_file_name(wanted: &str, mime: &str) -> String {
    let ext = match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    };
    let stem = wanted.rsplit_once('.').map_or(wanted, |(stem, _)| stem);
    let plain: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')).take(60).collect();
    let plain = plain.trim();
    format!("{}.{ext}", if plain.is_empty() { "castle-room-picture" } else { plain })
}

/// Saves a picture from the room where the member chooses (Windows' own Save dialog). Returns the saved
/// path, or none when the member closes the dialog.
#[tauri::command]
pub async fn room_picture_save(app: tauri::AppHandle, address: String, name: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (mime, bytes) = picture_from_data_address(&address).ok_or_else(|| "That is not a picture Castle can save.".to_string())?;
    let file_name = picture_file_name(&name, mime);
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = app.dialog().file().set_title("Save the picture").set_file_name(&file_name).add_filter("Pictures", &["jpg", "jpeg", "png", "webp", "gif"]).blocking_save_file();
        let Some(path) = chosen.and_then(|file| file.into_path().ok()) else { return Ok(None) };
        std::fs::write(&path, &bytes).map_err(|_| "The picture could not be saved there.".to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|_| "The save stopped before it finished.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn a_room_picture_is_read_from_its_data_address() {
        let bytes = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        assert_eq!(picture_from_data_address(&format!("data:image/jpeg;base64,{encoded}")), Some(("image/jpeg", bytes.to_vec())));
        assert_eq!(picture_from_data_address(&format!("  data:IMAGE/PNG;base64,{encoded}  ")).map(|p| p.0), Some("image/png"));
        assert_eq!(picture_from_data_address(&format!("data:image/webp;charset=x;base64,{encoded}")).map(|p| p.0), Some("image/webp"));
    }

    #[test]
    fn only_a_picture_in_a_data_address_is_saved() {
        let encoded = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        for bad in [
            format!("data:text/html;base64,{encoded}"),
            format!("data:image/svg+xml;base64,{encoded}"),
            format!("data:application/octet-stream;base64,{encoded}"),
            "data:image/jpeg;base64,".to_string(),
            "data:image/jpeg;base64,!!!not base64!!!".to_string(),
            format!("data:image/jpeg,{encoded}"),
            format!("https://example.com/a.jpg?{encoded}"),
            "C:/Windows/system32/config".to_string(),
            String::new(),
        ] {
            assert_eq!(picture_from_data_address(&bad), None, "should refuse: {}", &bad[..bad.len().min(40)]);
        }
        let huge = "A".repeat(SAVE_MAX / 3 * 4 + 100);
        assert_eq!(picture_from_data_address(&format!("data:image/png;base64,{huge}")), None, "too big");
    }

    #[test]
    fn a_saved_picture_gets_a_plain_name_with_the_right_ending() {
        assert_eq!(picture_file_name("Mira cat in the rain", "image/jpeg"), "Mira cat in the rain.jpg");
        assert_eq!(picture_file_name("tea-table_2.png", "image/png"), "tea-table_2.png");
        assert_eq!(picture_file_name("tea.png", "image/jpeg"), "tea.jpg", "the ending follows the real type");
        assert_eq!(picture_file_name("../../evil:name?.exe", "image/webp"), "evilname.webp", "no path or odd characters");
        assert_eq!(picture_file_name("", "image/gif"), "castle-room-picture.gif");
        assert_eq!(picture_file_name("   ", "image/jpeg"), "castle-room-picture.jpg");
        assert!(picture_file_name(&"x".repeat(200), "image/jpeg").len() <= 64);
    }

    #[test]
    fn a_room_file_id_is_taken_as_the_room_sends_it() {
        // the line shape the room sends: "picture": {"fileId": 42}
        let line = serde_json::json!({"picture": {"fileId": 42}, "voice": {"fileId": "7", "seconds": 3}});
        assert_eq!(file_id(&line["picture"]["fileId"]), Some("42".to_string()));
        assert_eq!(file_id(&line["voice"]["fileId"]), Some("7".to_string()));
        for junk in [serde_json::json!(null), serde_json::json!(-1), serde_json::json!(1.5), serde_json::json!(" "), serde_json::json!({})] {
            assert_eq!(file_id(&junk), None, "{junk}");
        }
    }

    #[test]
    fn display_name_preserves_the_other_foundation_fields_and_validates_input() {
        let profile = serde_json::json!({"displayName":"Before","partnerName":"Mira","portfolioUrl":"https://example.test","built":"A castle","listed":true,"privateServerField":"do not send"});
        let p = display_name_payload(&profile, "  Bernard  ").unwrap();
        assert_eq!(p["displayName"], "Bernard");
        for field in ["partnerName", "portfolioUrl", "built", "listed"] { assert_eq!(p[field], profile[field]); }
        assert!(p.get("privateServerField").is_none());
        assert_eq!(display_name_payload(&profile, "").unwrap()["displayName"], "");
        assert!(display_name_payload(&profile, &"a".repeat(81)).is_err());
        assert!(display_name_payload(&serde_json::json!({}), "Bernard").is_err());
    }

    #[test]
    fn display_name_uses_the_authenticated_profile_contract_without_overwriting_other_fields() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for body in [r#"{"displayName":"Old","partnerName":"Mira","portfolioUrl":"","built":"kept","listed":false}"#, r#"{"displayName":"Bernard"}"#] {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0; 4096]; let n = stream.read(&mut buf).unwrap(); if n == 0 { break; } bytes.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|s| s.trim().parse::<usize>().ok())).unwrap_or(0);
                        if bytes.len() >= end + 4 + length { break; }
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            requests
        });
        change_display_name(&base, "test-token", " Bernard ").unwrap();
        let requests = handle.join().unwrap();
        assert!(requests[0].starts_with("GET /api/member/foundation "));
        assert!(requests[1].starts_with("PUT /api/member/foundation "));
        assert!(requests.iter().all(|r| r.to_lowercase().contains("authorization: bearer test-token")));
        let payload: Value = serde_json::from_str(requests[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(payload["displayName"], "Bernard"); assert_eq!(payload["partnerName"], "Mira"); assert_eq!(payload["built"], "kept"); assert_eq!(payload["listed"], false);
    }

    /// A one-request mock room: answers `status` with `body`, and returns the request it saw.
    fn mock(status: u16, body: &'static str, kind: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(reply.as_bytes()).unwrap();
            request
        });
        (base, handle)
    }

    #[test]
    fn only_an_mp3_within_the_limit_is_a_voice() {
        assert!(is_mp3(b"ID3\x04\x00rest"));
        assert!(is_mp3(&[0xFF, 0xFB, 0x90, 0x64]), "an MPEG-1 layer III frame");
        assert!(is_mp3(&[0xFF, 0xF3, 0x48, 0xC4]), "an MPEG-2 frame, what 24 kHz makes");
        assert!(!is_mp3(&[0x1A, 0x45, 0xDF, 0xA3]), "webm is refused");
        assert!(!is_mp3(b"OggS"), "ogg is refused");
        assert!(!is_mp3(&[0xFF]), "one byte is not a frame");
        assert_eq!(check_voice(&[]).unwrap_err().message, "That recording is empty.");
        assert_eq!(check_voice(b"OggS....").unwrap_err().kind, "wrongType");
        let mut big = vec![0xFF, 0xFB];
        big.resize(ROOM_LINE_VOICE_MAX + 1, 0);
        assert_eq!(check_voice(&big).unwrap_err().kind, "tooLarge");
        big.truncate(ROOM_LINE_VOICE_MAX);
        assert!(check_voice(&big).is_ok(), "exactly 2 MB is taken");
    }

    #[test]
    fn the_voices_left_today_are_read_when_the_room_gives_them() {
        let me = serde_json::json!({ "name": "Rin", "media": { "caps": { "voices": 10 }, "today": { "voices": 3 } } });
        assert_eq!(media_voices(&me), Some((10, 3)));
        assert_eq!(media_voices(&serde_json::json!({ "media": { "caps": { "voices": 0 } } })), Some((0, 0)));
        assert_eq!(media_voices(&serde_json::json!({ "name": "Rin" })), None, "a room that gives no count");
    }

    #[test]
    fn the_rooms_brake_is_its_own_kind_of_no_and_a_name_is_still_a_name() {
        let quiet = refusal_for(409, r#"{"error":"Mira has said enough for now - the room waits for a person","code":"WAITING_FOR_A_PERSON"}"#);
        assert_eq!((quiet.kind.as_str(), quiet.message.as_str()), ("aiQuiet", "Mira has said enough for now - the room waits for a person"));
        assert_eq!(refusal_for(409, r#"{"code":"WAITING_FOR_A_PERSON"}"#).kind, "aiQuiet");
        assert_eq!(refusal_for(409, r#"{"error":"Set a name"}"#).kind, "needsName", "a 409 without the code is the name, as before");
        assert_eq!(refusal_for(409, "").kind, "needsName");
    }

    #[test]
    fn a_voice_refusal_is_said_as_a_voice_and_the_daily_count_in_the_room_words() {
        let capped = refusal_for(429, r#"{"error":"You have used today's 10 voices in the room"}"#);
        assert_eq!(capped.kind, "slowDown");
        assert_eq!(voice_refusal(capped).message, "You have used today's 10 voices in the room");
        assert_eq!(refusal_for(429, "{}").message, "Too many messages at once - wait a moment.");
        let off = voice_refusal(refusal_for(403, r#"{"error":"Voices are switched off in the room for now"}"#));
        assert_eq!((off.kind.as_str(), off.message.as_str()), ("locked", "Voices are switched off in the room for now"));
        assert!(voice_refusal(refusal_for(413, "")).message.contains("voice"));
    }

    #[test]
    fn a_voice_goes_as_multipart_with_its_words_and_the_line_it_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                if n == 0 { break; }
                bytes.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|s| s.trim().parse::<usize>().ok())).unwrap_or(0);
                    if bytes.len() >= end + 4 + length { break; }
                }
            }
            let body = r#"{"id":77,"text":"hello","voice":{"fileId":9,"seconds":4}}"#;
            write!(stream, "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            String::from_utf8_lossy(&bytes).to_string()
        });
        let line = Room::new(&base, "mcr_me").post_voice(vec![0xFF, 0xF3, 0x48, 0xC4, 1, 2, 3], Some(" hello "), Some(12)).unwrap();
        assert_eq!(line["id"], 77);
        let seen = handle.join().unwrap();
        assert!(seen.starts_with("POST /api/room/lines "));
        assert!(seen.to_lowercase().contains("authorization: bearer mcr_me"));
        assert!(seen.to_lowercase().contains("multipart/form-data"));
        assert!(seen.contains(r#"name="voice"; filename="voice.mp3""#));
        assert!(seen.contains("Content-Type: audio/mpeg"));
        assert!(seen.contains("name=\"text\"\r\n\r\nhello\r\n"));
        assert!(seen.contains("name=\"replyTo\"\r\n\r\n12\r\n"));
    }

    #[test]
    fn a_voice_that_is_not_an_mp3_never_leaves_castle() {
        // no room at this address: a refusal before any request proves nothing was sent
        let no = Room::new("http://127.0.0.1:9", "mcr_me").post_voice(b"OggS-not-mp3".to_vec(), None, None).unwrap_err();
        assert_eq!(no.kind, "wrongType");
    }

    #[test]
    fn a_companion_key_that_cannot_be_made_says_why() {
        assert!(key_refusal(401, "").contains("sign in again"));
        assert_eq!(key_refusal(403, r#"{"error":"This room is for MemoryCore Foundation members"}"#), "This room is for MemoryCore Foundation members");
        assert!(key_refusal(409, r#"{"error":"You already have 3 room keys"}"#).contains("Library card"));
        assert!(key_refusal(500, "").contains("500"));
    }

    #[test]
    fn a_room_key_is_forgotten_only_once_the_portfolio_let_it_go() {
        assert!(delete_outcome(204).is_ok());
        assert!(delete_outcome(404).is_ok(), "already gone is fine");
        assert!(delete_outcome(401).unwrap_err().contains("sign in again"));
        assert!(delete_outcome(500).unwrap_err().contains("500"));
    }

    #[test]
    fn the_room_address_is_built_from_the_portfolio_address() {
        assert_eq!(room_url("https://example.test/", "/me"), "https://example.test/api/room/me");
        assert_eq!(room_url("https://example.test/api", "/lines"), "https://example.test/api/room/lines");
    }

    #[test]
    fn refusals_are_told_apart() {
        assert_eq!(refusal_for(403, r#"{"error":"This room is for MemoryCore Foundation members"}"#).kind, "locked");
        assert_eq!(refusal_for(403, r#"{"error":"You are muted in this room for now"}"#).kind, "muted");
        assert_eq!(refusal_for(409, "{}").kind, "needsName");
        assert_eq!(refusal_for(413, "{}").kind, "tooLarge");
        assert_eq!(refusal_for(415, "{}").kind, "wrongType");
        assert_eq!(refusal_for(429, "{}").kind, "slowDown");
    }

    #[test]
    fn a_held_read_sends_the_key_the_cursor_and_a_named_agent() {
        let (base, seen) = mock(200, r#"{"cursor":12,"lines":[{"id":12,"text":"hi","author":{"name":"Rin","kind":"member"}}],"cleared":[],"more":false}"#, "application/json");
        let answer = Room::new(&base, "member-test-token").lines(Some(11), 2).unwrap();
        assert_eq!(answer["cursor"], 12);
        let request = seen.join().unwrap();
        assert!(request.starts_with("GET /api/room/lines?after=11&wait=2 "));
        assert!(request.to_lowercase().contains("authorization: bearer member-test-token"));
        assert!(request.contains("MemoryCoreCastle/"));
    }

    #[test]
    fn a_locked_door_and_a_long_message_are_refused_plainly() {
        let (base, seen) = mock(403, r#"{"error":"This room is for MemoryCore Foundation members"}"#, "application/json");
        let refused = Room::new(&base, "member-old-token").me().unwrap_err();
        assert_eq!(refused.kind, "locked");
        let _ = seen.join();
        assert_eq!(Room::new("http://127.0.0.1:9", "k").post(&"x".repeat(1001)).unwrap_err().kind, "tooLarge");
        assert!(Room::new("http://127.0.0.1:9", "k").file("../etc").is_err());
    }

    /// A mock Portfolio for several requests in a row: answers each with the next (status, body)
    /// and gives back every request's first line (and the whole of the last one).
    fn mock_many(replies: Vec<(u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buf = [0u8; 16384];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let head = text[..end].to_lowercase();
                        let length = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)));
                        let body_len = request.len() - (end + 4);
                        let complete = match length {
                            Some(want) => body_len >= want,
                            None if head.contains("transfer-encoding: chunked") => text.ends_with("0\r\n\r\n"),
                            None => true,
                        };
                        if complete {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8_lossy(&request).to_string());
                let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(reply.as_bytes()).unwrap();
            }
            seen
        });
        (base, handle)
    }

    fn noisy_png(side: u32) -> Vec<u8> {
        let mut seed: u32 = 7;
        let img = image::RgbImage::from_fn(side, side, |_, _| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            image::Rgb([(seed >> 24) as u8, (seed >> 16) as u8, (seed >> 8) as u8])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        png
    }

    #[test]
    fn a_small_picture_goes_as_it_is_and_a_big_one_is_shrunk_to_fit() {
        let small = vec![1u8, 2, 3];
        assert_eq!(fit_for_room(small.clone(), "image/png").unwrap(), (small, "image/png"));
        let big = noisy_png(1200);
        assert!(big.len() > ROOM_PICTURE_MAX);
        let (fitted, kind) = fit_for_room(big, "image/png").unwrap();
        assert!(fitted.len() <= ROOM_PICTURE_MAX);
        assert_eq!(kind, "image/jpeg");
        let back = image::load_from_memory(&fitted).unwrap();
        assert!(back.width() <= 512 && back.height() <= 512);
        assert!(fit_for_room(vec![0u8; ROOM_PICTURE_MAX + 1], "image/png").is_err());
        assert_eq!(picture_kind(std::path::Path::new("a/face.JPG")), Some("image/jpeg"));
        assert_eq!(picture_kind(std::path::Path::new("a/face.gif")), None);
    }

    #[test]
    fn a_drawn_picture_is_made_to_fit_a_line() {
        let drawn = noisy_png(1536);
        assert!(drawn.len() > ROOM_LINE_PICTURE_MAX, "a drawing as big as gpt-image's needs shrinking");
        let (fitted, kind) = fit_picture(drawn, "image/png", ROOM_LINE_PICTURE_MAX, ROOM_LINE_PICTURE_SIDE).unwrap();
        assert!(fitted.len() <= ROOM_LINE_PICTURE_MAX);
        assert_eq!(kind, "image/jpeg");
        let back = image::load_from_memory(&fitted).unwrap();
        assert!(back.width() <= 1024 && back.height() <= 1024 && back.width() > 512, "a line keeps more detail than a face");
        let small = vec![7u8; 1000];
        assert_eq!(fit_picture(small.clone(), "image/webp", ROOM_LINE_PICTURE_MAX, ROOM_LINE_PICTURE_SIDE).unwrap(), (small, "image/webp"));
    }

    #[test]
    fn the_ai_reads_its_picture_day_from_me() {
        let me = serde_json::json!({ "name": "Mika", "media": { "caps": { "voices": 5, "pictures": 2 }, "today": { "voices": 1, "pictures": 1 } } });
        assert_eq!(media_pictures(&me), Some((2, 1)));
        assert_eq!(media_pictures(&serde_json::json!({ "media": { "caps": { "pictures": 0 } } })), Some((0, 0)), "0 = off, nothing used");
        assert_eq!(media_pictures(&serde_json::json!({ "name": "Rin" })), None, "a member's own key has no media");
    }

    #[test]
    fn the_key_is_remade_only_when_its_name_is_not_the_name_in_settings() {
        assert!(!needs_rename(Some("Mika"), "Mika"));
        assert!(!needs_rename(Some(" Mika "), "Mika"));
        assert!(needs_rename(Some("Mika"), "Kociness"));
        assert!(needs_rename(None, "Mika"));
    }

    #[test]
    fn a_rename_makes_the_new_key_first_then_removes_the_old_one_then_sends_the_picture() {
        let folder = std::env::temp_dir().join(format!("castle-face-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let picture = folder.join("avatar.png");
        std::fs::write(&picture, noisy_png(64)).unwrap();
        let (base, seen) = mock_many(vec![(201, r#"{"id":42,"key":"mcr_new"}"#), (204, ""), (200, "{}")]);
        let done = refresh_companion(&base, "member-token", "17", Some("Kociness"), Some(&picture)).unwrap();
        assert_eq!(done.new_key, Some((Some("42".to_string()), "mcr_new".to_string())));
        assert!(done.face.is_ok());
        let seen = seen.join().unwrap();
        assert!(seen[0].starts_with("POST /api/member/room-keys "));
        assert!(seen[0].contains(r#""assistantName":"Kociness""#));
        assert!(seen[1].starts_with("DELETE /api/member/room-keys/17 "));
        assert!(seen[2].starts_with("POST /api/member/room-keys/42/avatar "));
        assert!(seen[2].to_lowercase().contains("multipart/form-data"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn no_picture_clears_the_face_and_a_refused_picture_is_said_in_words() {
        let (base, seen) = mock_many(vec![(204, "")]);
        let done = refresh_companion(&base, "member-token", "17", None, None).unwrap();
        assert!(done.new_key.is_none() && done.face.is_ok());
        assert!(seen.join().unwrap()[0].starts_with("DELETE /api/member/room-keys/17/avatar "));

        let folder = std::env::temp_dir().join(format!("castle-face-refused-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let picture = folder.join("avatar.png");
        std::fs::write(&picture, noisy_png(32)).unwrap();
        let (base, seen) = mock_many(vec![(415, r#"{"error":"Only PNG, JPEG or WebP pictures"}"#)]);
        let done = refresh_companion(&base, "member-token", "17", None, Some(&picture)).unwrap();
        assert_eq!(done.face.unwrap_err(), refusal_for(415, r#"{"error":"Only PNG, JPEG or WebP pictures"}"#).message);
        let _ = seen.join();
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_failed_new_key_leaves_the_old_one_alone() {
        let (base, seen) = mock_many(vec![(409, r#"{"error":"You already have 3 room keys"}"#)]);
        let refused = refresh_companion(&base, "member-token", "17", Some("Kociness"), None).err().unwrap();
        assert!(refused.contains("Library card"));
        assert_eq!(seen.join().unwrap().len(), 1);
    }

    #[test]
    fn a_room_picture_comes_back_as_a_data_url() {
        let (base, seen) = mock(200, "PNG", "image/png");
        let url = Room::new(&base, "member-test-token").file("abc123").unwrap();
        assert_eq!(url, "data:image/png;base64,UE5H");
        let _ = seen.join();
    }

    #[test]
    fn a_read_sends_its_cursors_and_a_reply_carries_its_line() {
        assert_eq!(lines_path(Some(7), 20, None, None), "/lines?after=7&wait=20");
        assert_eq!(lines_path(None, 0, Some("2026-11-15T20:00:00"), Some("2026-11-15T20:01:00")), "/lines?wait=0&clearedAfter=2026-11-15T20%3A00%3A00&reactedAfter=2026-11-15T20%3A01%3A00");
        assert_eq!(lines_path(Some(1), 0, Some(""), None), "/lines?after=1&wait=0");
        assert_eq!(post_body("hi", Some(12)), serde_json::json!({"text": "hi", "replyTo": 12}));
        assert_eq!(post_body("hi", None), serde_json::json!({"text": "hi"}));
    }

    #[test]
    fn a_room_that_says_nothing_has_no_features() {
        assert!(features(&serde_json::json!({"name": "Kiyo"})).is_empty());
        assert_eq!(features(&serde_json::json!({"features": ["replies", 3, "reactions"]})), vec!["replies", "reactions"]);
        assert!(REACTIONS.contains(&"👍") && REACTIONS.len() == 6);
    }

    #[test]
    fn a_reaction_may_be_any_one_emoji_but_never_words() {
        for ok in ["🔥", "🇲🇾", "1️⃣", "👨‍👩‍👧", "👍🏽", "❤️", "❤"] {
            assert!(one_emoji(ok), "{ok}");
        }
        for no in ["", " ", "hi", "a😀", "🔥 ", ":)", "1", &"🔥".repeat(9)] {
            assert!(!one_emoji(no), "{no:?}");
        }
    }

    #[test]
    fn words_as_a_reaction_never_leave_castle() {
        // no room at this address: a refusal before any request proves nothing was sent
        let no = Room::new("http://127.0.0.1:9", "mcr_me").react(5, "hello", true).unwrap_err();
        assert_eq!(no.kind, "other");
    }
}
