//! The WhatsApp adapter for MemoryCore in your pocket: a small local sidecar
//! (`resources/tools/castle-whatsapp.cjs`, Castle's bundled Node running Baileys - a WhatsApp Web
//! client, the same library OpenClaw and Hermes both use) that Castle spawns and talks to over a
//! localhost-only HTTP door. Off until switched on in MESSAGING; no token - the "credential" is
//! the QR-paired session itself, kept in Castle's settings folder so a restart does not ask for
//! the QR again.
//!
//! **Unofficial, ban risk**: WhatsApp has no public API for this. A secondary number is the
//! right one to pair, never a primary personal one, same as Hermes' own docs disclose.
//! v1 answers one-to-one chats only - the sidecar script itself drops groups and broadcasts.

use crate::settings::{Settings, SettingsStore, WhatsAppGuest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

const PORT: u16 = 51847;
const REPLY_TIMEOUT: Duration = Duration::from_secs(180);
const TICK: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct WhatsAppState {
    child: Mutex<Option<Child>>,
    connected: Mutex<bool>,
    paired: Mutex<bool>,
    qr: Mutex<Option<String>>,
    problem: Mutex<Option<String>>,
    /// A chat that messaged in and is not approved yet.
    pending: Mutex<Option<Waiting>>,
}

impl Drop for WhatsAppState {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
        }
    }
}

/// A chat waiting for the member's word: its id, the name WhatsApp gave and its phone digits.
#[derive(Clone, Debug, PartialEq)]
struct Waiting {
    chat_id: String,
    name: String,
    digits: Option<String>,
}

/// A person on the WhatsApp card: the one waiting, the member, or a guest.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppPerson {
    chat_id: String,
    name: String,
    /// the number as a person reads it, when WhatsApp gave it
    number: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppStatus {
    on: bool,
    connected: bool,
    paired: bool,
    qr: Option<String>,
    pending: Option<WhatsAppPerson>,
    problem: Option<String>,
    /// the member's own number ("Me")
    me: Option<WhatsAppPerson>,
    /// the others the member approved
    guests: Vec<WhatsAppPerson>,
}

#[derive(Deserialize)]
struct SidecarStatus {
    paired: bool,
    connected: bool,
    qr: Option<String>,
    problem: Option<String>,
}

#[derive(Deserialize)]
struct SidecarIncoming {
    messages: Vec<IncomingMessage>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct IncomingMessage {
    #[serde(default)]
    id: String,
    chat_id: String,
    name: String,
    #[serde(default)]
    text: String,
    /// "text", "media" (a picture, voice note or video, as a line) or "reaction"
    #[serde(default = "text_kind")]
    kind: String,
    /// a reaction's emoji; empty when it was taken back
    #[serde(default)]
    emoji: String,
    /// the id of the message a reaction is on
    #[serde(default)]
    on_id: Option<String>,
    /// the phone number behind the chat, when WhatsApp gives it
    #[serde(default)]
    pn: Option<String>,
    /// a picture the helper saved in `media_dir` for Castle to take (or remove)
    #[serde(default)]
    file: Option<String>,
}

/// Where the helper saves a chat's picture until Castle takes it: Castle's own folder, never the
/// memory folder (a guest's or a stranger's picture is removed there, unseen).
fn media_dir(store: &SettingsStore) -> std::path::PathBuf {
    store.folder().join("whatsapp-media")
}

/// The words a member's picture becomes for the companion: the path in the memory folder and the
/// caption, or None when there is no picture to take (a voice note, a video, a failed download).
fn picture_words(memory: Option<&std::path::Path>, m: &IncomingMessage) -> Option<String> {
    let saved = std::path::PathBuf::from(m.file.as_deref()?);
    let caption = m.text.strip_prefix("(sent a picture)").unwrap_or("");
    match memory {
        Some(memory) => crate::bridges::take_picture(memory, &saved, caption),
        None => {
            let _ = std::fs::remove_file(&saved);
            None
        }
    }
}

fn text_kind() -> String {
    "text".into()
}

#[derive(Deserialize)]
struct SentReply {
    id: Option<String>,
}

/// Sends one message through the helper; the id WhatsApp gave it, so a reaction can be placed.
fn send(client: &reqwest::blocking::Client, to: &str, text: &str) -> Option<String> {
    let response = client.post(door("/send")).json(&json!({ "to": to, "text": text })).send().ok()?;
    response.json::<SentReply>().ok()?.id
}

// ---------- who is writing: the member, a guest, or someone not approved ----------

/// The phone digits behind a chat: from the number WhatsApp gave (`pn`), else from a chat id
/// that is a phone number itself (`<digits>@s.whatsapp.net`, a device suffix `:n` dropped). A
/// hidden-number id (`@lid`) without a `pn` has none.
pub fn digits_of(chat_id: &str, pn: Option<&str>) -> Option<String> {
    let from = |id: &str| -> Option<String> {
        let (user, server) = id.split_once('@')?;
        if server != "s.whatsapp.net" && server != "c.us" {
            return None;
        }
        let user = user.split(':').next().unwrap_or(user);
        let digits: String = user.chars().filter(char::is_ascii_digit).collect();
        (digits.len() >= 6 && digits.len() == user.chars().count()).then_some(digits)
    };
    pn.and_then(from).or_else(|| from(chat_id))
}

/// The number as a person reads it: `+601 2345 6789`.
pub fn readable(digits: &str) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut groups: Vec<String> = chars.rchunks(4).map(|c| c.iter().collect()).collect();
    groups.reverse();
    format!("+{}", groups.join(" "))
}

#[derive(Debug, PartialEq)]
pub enum Who {
    Me,
    Guest(usize),
    Stranger,
}

fn same(id_a: &str, digits_a: Option<&str>, id_b: &str, digits_b: Option<&str>) -> bool {
    id_a == id_b || matches!((digits_a, digits_b), (Some(a), Some(b)) if a == b)
}

/// Who a chat is: the member's own number, one of their guests, or neither. Matched by chat id,
/// or by phone digits when both sides have them (a re-paired Castle number still knows everyone).
pub fn who(settings: &Settings, chat_id: &str, digits: Option<&str>) -> Who {
    if let Some(me) = settings.whatsapp_bridge_chat_id.as_deref() {
        if same(me, settings.whatsapp_bridge_me_digits.as_deref(), chat_id, digits) {
            return Who::Me;
        }
    }
    match settings.whatsapp_guests.iter().position(|g| same(&g.chat_id, g.digits.as_deref(), chat_id, digits)) {
        Some(at) => Who::Guest(at),
        None => Who::Stranger,
    }
}

/// Approves a chat as the member's own number: it leaves the guests if it was one, and the old
/// "me" leaves (a new phone number for the member replaces the old one).
pub fn approve_as_me(settings: &mut Settings, chat_id: &str, name: &str, digits: Option<&str>) {
    settings.whatsapp_guests.retain(|g| !same(&g.chat_id, g.digits.as_deref(), chat_id, digits));
    settings.whatsapp_bridge_chat_id = Some(chat_id.into());
    settings.whatsapp_bridge_me_name = Some(name.into());
    settings.whatsapp_bridge_me_digits = digits.map(str::to_string);
}

/// Approves a chat as a guest. The member's own number is never a guest too.
pub fn approve_as_guest(settings: &mut Settings, chat_id: &str, name: &str, digits: Option<&str>, at: u64) -> Result<(), String> {
    match who(settings, chat_id, digits) {
        Who::Me => Err("That number is you already.".into()),
        Who::Guest(i) => {
            settings.whatsapp_guests[i].name = name.into();
            Ok(())
        }
        Who::Stranger => {
            settings.whatsapp_guests.push(WhatsAppGuest { chat_id: chat_id.into(), name: name.into(), digits: digits.map(str::to_string), approved_at: at });
            Ok(())
        }
    }
}

/// Makes a guest the member's own number; the old "me" leaves.
pub fn make_me(settings: &mut Settings, chat_id: &str) -> Result<(), String> {
    let guest = settings.whatsapp_guests.iter().find(|g| g.chat_id == chat_id).cloned().ok_or("That number is not on the list.")?;
    approve_as_me(settings, &guest.chat_id, &guest.name, guest.digits.as_deref());
    Ok(())
}

/// Lets one guest go.
pub fn remove_guest(settings: &mut Settings, chat_id: &str) -> Result<(), String> {
    let before = settings.whatsapp_guests.len();
    settings.whatsapp_guests.retain(|g| g.chat_id != chat_id);
    if settings.whatsapp_guests.len() == before {
        return Err("That number is not on the list.".into());
    }
    Ok(())
}

/// Forgets the member's own number; the guests stay.
pub fn forget_me(settings: &mut Settings) {
    settings.whatsapp_bridge_chat_id = None;
    settings.whatsapp_bridge_me_name = None;
    settings.whatsapp_bridge_me_digits = None;
}

fn person(chat_id: &str, name: &str, digits: Option<&str>) -> WhatsAppPerson {
    WhatsAppPerson { chat_id: chat_id.into(), name: name.into(), number: digits.map(readable) }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn save_with(store: &SettingsStore, change: impl FnOnce(&mut Settings) -> Result<(), String>) -> Result<(), String> {
    let mut next = store.get();
    change(&mut next)?;
    crate::settings::save(&store.path, &next).map_err(|_| "The choice could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

fn door(path: &str) -> String {
    format!("http://127.0.0.1:{PORT}{path}")
}

fn auth_dir(store: &SettingsStore) -> PathBuf {
    store.folder().join("whatsapp-auth")
}

fn script_path(app: &AppHandle) -> Option<PathBuf> {
    let path = app.path().resource_dir().ok()?.join("tools").join("castle-whatsapp.cjs");
    // Windows hands back a path that starts with `\\?\`; Node cannot resolve that form ("EISDIR ... lstat 'C:'"), so the plain path is used.
    let text = path.to_string_lossy().into_owned();
    let plain = text.strip_prefix("\\\\?\\").map(PathBuf::from).unwrap_or(path);
    plain.is_file().then_some(plain)
}

/// Whether the helper is alive. One that has exited (its port was taken, it crashed) is forgotten,
/// so the loop starts a new one instead of waiting for ever on a helper that is not there.
fn running(state: &WhatsAppState) -> bool {
    let mut child = state.child.lock().unwrap();
    let Some(c) = child.as_mut() else { return false };
    match c.try_wait() {
        Ok(None) => true,
        other => {
            *child = None;
            let how = match other {
                Ok(Some(status)) => format!(" (exit {})", status.code().map_or("?".to_string(), |c| c.to_string())),
                _ => String::new(),
            };
            *state.problem.lock().unwrap() = Some(format!("The WhatsApp helper stopped{how}; starting it again."));
            false
        }
    }
}

fn ensure_started(app: &AppHandle, store: &SettingsStore, state: &WhatsAppState) -> Result<(), String> {
    if running(state) {
        return Ok(());
    }
    let script = script_path(app).ok_or("The WhatsApp helper is missing from this install.")?;
    // What the helper says goes to a log beside its pairing, so a helper that stops can be explained.
    let log = std::fs::File::create(store.folder().join("whatsapp.log")).map_err(|_| "The WhatsApp helper's log could not be opened.".to_string())?;
    let mut command = Command::new(crate::env::node());
    command
        .arg(&script)
        // Node resolves the script's real path from where it starts; Castle's own folder can make that fail ("EISDIR ... lstat 'C:'").
        .current_dir(store.folder())
        .env("WA_AUTH_DIR", auth_dir(store))
        .env("WA_PORT", PORT.to_string())
        .env("WA_MEDIA_DIR", media_dir(store))
        .stdout(log.try_clone().map(Stdio::from).unwrap_or_else(|_| Stdio::null()))
        .stderr(Stdio::from(log))
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window: the helper is invisible, its words go to the log
    }
    let child = command.spawn().map_err(|_| "The WhatsApp helper could not be started.".to_string())?;
    *state.child.lock().unwrap() = Some(child);
    Ok(())
}

fn stop(state: &WhatsAppState) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
    }
    *state.connected.lock().unwrap() = false;
    *state.paired.lock().unwrap() = false;
    *state.qr.lock().unwrap() = None;
}

/// One incoming chat: approved, it goes to the companion's real session (in its own thread, so a
/// slow reply never stalls the poll loop); not yet approved, the sender is told so and held
/// pending; a different, already-ignored chat is left alone.
fn handle(app: &AppHandle, client: &reqwest::blocking::Client, m: IncomingMessage) {
    let app = app.clone();
    let client = client.clone();
    std::thread::spawn(move || {
        let store = app.state::<SettingsStore>();
        let settings = store.get();
        let digits = digits_of(&m.chat_id, m.pn.as_deref());
        let (chat_id, mut text, name) = (m.chat_id.clone(), m.text.clone(), m.name.clone());
        let who = who(&settings, &chat_id, digits.as_deref());
        // a picture reaches only the member's own companion; anyone else's is removed unseen
        if !matches!(who, Who::Me) {
            if let Some(file) = &m.file {
                let _ = std::fs::remove_file(file);
            }
        }
        if matches!(who, Who::Me) && m.kind == "media" {
            match picture_words(settings.memory_folder.as_deref().map(std::path::Path::new), &m) {
                Some(words) => text = words,
                // a voice note or a video: the companion's own session takes words only
                None => return,
            }
        }
        match who {
            Who::Me => {
                // the AUTOMATIONS message trigger only looks at the approved chat itself; a member recognised by number alone is answered as before
                if settings.whatsapp_bridge_chat_id.as_deref() == Some(chat_id.as_str()) && !crate::automations::triggers::incoming(&app, "whatsapp", &chat_id, &m.id, &text) { return; }
                let reply = crate::bridges::send_and_await(&app, &store, &text, REPLY_TIMEOUT).unwrap_or_else(|problem| problem);
                if let Some(id) = send(&client, &chat_id, &reply) {
                    crate::bridges::reactions::remember(&app, "whatsapp", &id, &reply);
                }
            }
            // a guest is answered by the guest-only run, never the companion's real session
            Who::Guest(i) => crate::bridges::guests::heard(&app, settings.whatsapp_guests[i].clone(), text, send, client.clone()),
            Who::Stranger => {
                let wa_state = app.state::<WhatsAppState>();
                let mut pending = wa_state.pending.lock().unwrap();
                // one waits at a time; a second stranger is not told anything until the first is settled
                if pending.as_ref().is_some_and(|w| w.chat_id != chat_id) {
                    return;
                }
                let first = pending.is_none();
                *pending = Some(Waiting { chat_id: chat_id.clone(), name, digits });
                drop(pending);
                if first {
                    let _ = send(&client, &chat_id, "Hi! Approve this chat on Castle's MESSAGING room before I can answer.");
                }
            }
        }
    });
}

fn poll(app: &AppHandle, client: &reqwest::blocking::Client, state: &WhatsAppState) {
    let Ok(response) = client.get(door("/status")).send() else { return }; // the sidecar is still starting up - not an error worth surfacing
    let Ok(status) = response.json::<SidecarStatus>() else { return };
    *state.connected.lock().unwrap() = status.connected;
    *state.paired.lock().unwrap() = status.paired;
    *state.qr.lock().unwrap() = status.qr;
    *state.problem.lock().unwrap() = status.problem;

    let Ok(response) = client.get(door("/incoming")).send() else { return };
    let Ok(incoming) = response.json::<SidecarIncoming>() else { return };
    for m in incoming.messages {
        if m.kind == "reaction" {
            // only the member's own hearts are shown
            let settings = app.state::<SettingsStore>().get();
            let is_me = who(&settings, &m.chat_id, digits_of(&m.chat_id, m.pn.as_deref()).as_deref()) == Who::Me;
            if let (Some(on), true) = (m.on_id.as_deref(), is_me) {
                crate::bridges::reactions::record(app, "whatsapp", on, &m.emoji);
            }
            continue;
        }
        handle(app, client, m);
    }
}

/// Starts the bridge with Castle; the sidecar itself only runs while the switch in the MESSAGING
/// room is on, and is killed the moment it is switched off.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(client) = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build() else { return };
        loop {
            std::thread::sleep(TICK);
            let state = app.state::<WhatsAppState>();
            let store = app.state::<SettingsStore>();
            let settings = store.get();
            if !settings.whatsapp_bridge_on {
                if running(&state) {
                    stop(&state);
                }
                continue;
            }
            if let Err(problem) = ensure_started(&app, &store, &state) {
                *state.problem.lock().unwrap() = Some(problem);
                continue;
            }
            poll(&app, &client, &state);
        }
    });
}

#[tauri::command]
pub fn whatsapp_bridge_status(store: State<SettingsStore>, state: State<WhatsAppState>) -> WhatsAppStatus {
    let settings = store.get();
    WhatsAppStatus {
        on: settings.whatsapp_bridge_on,
        connected: *state.connected.lock().unwrap(),
        paired: *state.paired.lock().unwrap(),
        qr: state.qr.lock().unwrap().clone(),
        pending: state.pending.lock().unwrap().clone().map(|w| person(&w.chat_id, &w.name, w.digits.as_deref())),
        problem: state.problem.lock().unwrap().clone(),
        me: settings.whatsapp_bridge_chat_id.as_deref().map(|id| person(id, settings.whatsapp_bridge_me_name.as_deref().unwrap_or("Me"), settings.whatsapp_bridge_me_digits.as_deref())),
        guests: settings.whatsapp_guests.iter().map(|g| person(&g.chat_id, &g.name, g.digits.as_deref())).collect(),
    }
}

/// Settles the chat waiting: `as_who` "me" (the member's own number, whose words reach the
/// companion's real session) or "guest" (answered by the guest-only run); "no" lets it go
/// unanswered.
#[tauri::command]
pub fn whatsapp_bridge_approve(store: State<SettingsStore>, state: State<WhatsAppState>, as_who: Option<String>) -> Result<(), String> {
    let Some(waiting) = state.pending.lock().unwrap().take() else {
        return Err("There is no chat waiting to be approved.".into());
    };
    match as_who.as_deref().unwrap_or("me") {
        "me" => save_with(&store, |s| {
            approve_as_me(s, &waiting.chat_id, &waiting.name, waiting.digits.as_deref());
            Ok(())
        }),
        "guest" => save_with(&store, |s| approve_as_guest(s, &waiting.chat_id, &waiting.name, waiting.digits.as_deref(), now_secs())),
        _ => Ok(()),
    }
}

/// Makes a guest the member's own number; the old "me" leaves the list.
#[tauri::command]
pub fn whatsapp_bridge_make_me(store: State<SettingsStore>, chat_id: String) -> Result<(), String> {
    save_with(&store, |s| make_me(s, &chat_id))
}

/// Lets one guest go.
#[tauri::command]
pub fn whatsapp_bridge_remove_guest(store: State<SettingsStore>, chat_id: String) -> Result<(), String> {
    save_with(&store, |s| remove_guest(s, &chat_id))
}

/// Forgets the member's own number, so a new one can be approved. The guests stay, and the
/// WhatsApp pairing itself (the QR'd session) is untouched.
#[tauri::command]
pub fn whatsapp_bridge_forget(store: State<SettingsStore>) -> Result<(), String> {
    save_with(&store, |s| {
        forget_me(s);
        Ok(())
    })
}

/// Pairs the Castle with a new WhatsApp number (after a ban, or a logout on the phone): the old
/// pairing is removed and a fresh QR code comes. The member and every guest stay approved.
#[tauri::command]
pub fn whatsapp_bridge_unpair(store: State<SettingsStore>, state: State<WhatsAppState>) -> Result<(), String> {
    stop(&state);
    *state.problem.lock().unwrap() = None;
    let dir = auth_dir(&store);
    if dir.exists() {
        // the helper takes a moment to let go of its files after it is stopped
        let mut tries = 0;
        while std::fs::remove_dir_all(&dir).is_err() {
            tries += 1;
            if tries >= 10 {
                return Err("The old pairing could not be removed. Try again in a moment.".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_members_picture_becomes_its_path_and_caption_and_a_voice_note_nothing() {
        let root = std::env::temp_dir().join(format!("castle-wa-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let saved = root.join("X1.img");
        std::fs::write(&saved, [0xFF, 0xD8, 0xFF, 0xE0, 1]).unwrap();
        let line = format!(r#"{{"id":"X1","chatId":"60123@s.whatsapp.net","name":"Me","kind":"media","text":"(sent a picture) receipt","file":{}}}"#, serde_json::to_string(&saved.display().to_string()).unwrap());
        let m: IncomingMessage = serde_json::from_str(&line).unwrap();
        let memory = root.join("memory");
        let words = picture_words(Some(&memory), &m).unwrap();
        assert!(words.starts_with("(sent a picture: ") && words.ends_with(") receipt"), "{words}");
        // an older helper's line has no file; a voice note is words only
        let voice: IncomingMessage = serde_json::from_str(r#"{"chatId":"60123@s.whatsapp.net","name":"Me","kind":"media","text":"(sent a voice note)"}"#).unwrap();
        assert_eq!(voice.file, None);
        assert_eq!(picture_words(Some(&memory), &voice), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_door_always_points_at_the_local_only_port() {
        assert_eq!(door("/status"), "http://127.0.0.1:51847/status");
        assert_eq!(door("/send"), "http://127.0.0.1:51847/send");
    }

    fn settings_with_me() -> Settings {
        let mut s = Settings::default();
        approve_as_me(&mut s, "6591234567@s.whatsapp.net", "Rin", Some("6591234567"));
        s
    }

    #[test]
    fn the_phone_digits_come_from_the_number_or_a_number_shaped_chat_id() {
        assert_eq!(digits_of("6591234567@s.whatsapp.net", None).as_deref(), Some("6591234567"));
        assert_eq!(digits_of("6591234567:12@s.whatsapp.net", None).as_deref(), Some("6591234567"), "a device suffix is dropped");
        assert_eq!(digits_of("1234567890123@lid", None), None, "a hidden-number id has none");
        assert_eq!(digits_of("1234567890123@lid", Some("60123456789@s.whatsapp.net")).as_deref(), Some("60123456789"));
        assert_eq!(readable("60123456789"), "+601 2345 6789");
    }

    #[test]
    fn there_is_one_me_and_a_number_is_me_or_a_guest_never_both() {
        let mut s = settings_with_me();
        assert_eq!(who(&s, "6591234567@s.whatsapp.net", Some("6591234567")), Who::Me);
        assert!(approve_as_guest(&mut s, "6591234567@s.whatsapp.net", "Rin", Some("6591234567"), 1).is_err());
        approve_as_guest(&mut s, "60123456789@s.whatsapp.net", "Aina", Some("60123456789"), 1).unwrap();
        approve_as_guest(&mut s, "60123456789@s.whatsapp.net", "Aina K", Some("60123456789"), 2).unwrap();
        assert_eq!(s.whatsapp_guests.len(), 1, "the same guest twice is one guest");
        assert_eq!(s.whatsapp_guests[0].name, "Aina K");
        assert_eq!(who(&s, "60123456789@s.whatsapp.net", None), Who::Guest(0));
        assert_eq!(who(&s, "999999999@s.whatsapp.net", None), Who::Stranger);
        // approving a guest as me moves it, and the old me leaves
        approve_as_me(&mut s, "60123456789@s.whatsapp.net", "Aina", Some("60123456789"));
        assert!(s.whatsapp_guests.is_empty());
        assert_eq!(who(&s, "6591234567@s.whatsapp.net", None), Who::Stranger);
    }

    #[test]
    fn make_me_replaces_the_old_me_and_forget_keeps_the_guests() {
        let mut s = settings_with_me();
        approve_as_guest(&mut s, "60123456789@s.whatsapp.net", "Aina", Some("60123456789"), 1).unwrap();
        approve_as_guest(&mut s, "60111111111@s.whatsapp.net", "Farid", None, 1).unwrap();
        make_me(&mut s, "60123456789@s.whatsapp.net").unwrap();
        assert_eq!(s.whatsapp_bridge_chat_id.as_deref(), Some("60123456789@s.whatsapp.net"));
        assert_eq!(s.whatsapp_bridge_me_name.as_deref(), Some("Aina"));
        assert_eq!(s.whatsapp_guests.len(), 1, "the old me is not kept as a guest");
        assert!(make_me(&mut s, "nobody@s.whatsapp.net").is_err());
        forget_me(&mut s);
        assert_eq!(s.whatsapp_bridge_chat_id, None);
        assert_eq!(s.whatsapp_guests.len(), 1);
        remove_guest(&mut s, "60111111111@s.whatsapp.net").unwrap();
        assert!(s.whatsapp_guests.is_empty());
        assert!(remove_guest(&mut s, "60111111111@s.whatsapp.net").is_err());
    }

    #[test]
    fn a_person_is_known_by_their_digits_when_the_chat_id_changes() {
        let s = settings_with_me();
        assert_eq!(who(&s, "777777777777@lid", Some("6591234567")), Who::Me);
        assert_eq!(who(&s, "777777777777@lid", None), Who::Stranger);
    }

    #[test]
    fn the_helpers_lines_read_with_or_without_the_newer_fields() {
        let old: IncomingMessage = serde_json::from_str(r#"{"chatId":"1@s.whatsapp.net","name":"Rin","text":"hi"}"#).unwrap();
        assert_eq!(old.kind, "text");
        assert_eq!(old.on_id, None);
        let heart: IncomingMessage = serde_json::from_str(r#"{"chatId":"1@s.whatsapp.net","pn":null,"name":"Rin","kind":"reaction","text":"","emoji":"❤️","onId":"ABC"}"#).unwrap();
        assert_eq!((heart.kind.as_str(), heart.emoji.as_str(), heart.on_id.as_deref()), ("reaction", "❤️", Some("ABC")));
    }
}

pub(super) fn notify(app:&AppHandle,store:&SettingsStore,expected:&str,text:&str)->Result<(),String>{
 let current=super::notification_target(&store.get(),"whatsapp")?;if current!=expected{return Err("The approved WhatsApp chat changed.".into());}
 if !*app.state::<WhatsAppState>().connected.lock().unwrap(){return Err("The WhatsApp helper is not connected.".into());}
 let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|_|"WhatsApp could not start.")?;let response=client.post(door("/send")).json(&json!({"to":current,"text":text})).send().map_err(|_|"WhatsApp did not take the notice.")?;
 if !response.status().is_success(){return Err("WhatsApp refused the notice.".into());}let body:serde_json::Value=response.json().map_err(|_|"WhatsApp did not confirm the notice.")?;if body["ok"]==true{Ok(())}else{Err("WhatsApp did not confirm the notice.".into())}
}
