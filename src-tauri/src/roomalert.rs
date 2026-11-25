//! A new line in the Foundation room: a sound and a desktop notification when the member is not
//! looking at the CHAT tab. The notification is on by default and the sound off (the sound ignores
//! Windows quiet hours); each has its own switch in Settings > Chat, with the choice of every line
//! or only the lines that name the member or their companion.
//! The room reader (roomread.rs) runs from the moment Castle starts, also while Castle is hidden in
//! the tray, so this reaches the member then too. An alert is best effort: it never stops a read.

use crate::settings::{Settings, SettingsStore};
use serde_json::Value;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

const PREVIEW: usize = 80;

/// The notification for a batch of new lines, leaving out lines the member posted from Castle
/// (`mine`, by id): (title, body), or None when nothing is left to tell. Pure, tested.
/// The line answers one of `mine`. Pure.
pub fn replies_to(line: &Value, mine: &[u64]) -> bool {
    line.pointer("/replyTo/id").and_then(Value::as_u64).is_some_and(|id| mine.contains(&id))
}

fn reaction(line: &Value, emoji: &str) -> (u64, bool) {
    line.get("reactions")
        .and_then(Value::as_array)
        .and_then(|r| r.iter().find(|x| x.get("emoji").and_then(Value::as_str) == Some(emoji)))
        .map(|x| (x.get("count").and_then(Value::as_u64).unwrap_or(0), x.get("mine").and_then(Value::as_bool).unwrap_or(false)))
        .unwrap_or((0, false))
}

/// The news of reactions on the member's own lines (before, after): only a count that grew by
/// someone else's tap, never the member's own. Pure, tested.
pub fn reaction_news(changed: &[(Value, Value)], mine: &[u64]) -> Option<(String, String)> {
    let mut told = Vec::new();
    for (before, after) in changed {
        if !after.get("id").and_then(Value::as_u64).is_some_and(|id| mine.contains(&id)) {
            continue;
        }
        for r in after.get("reactions").and_then(Value::as_array).cloned().unwrap_or_default() {
            let emoji = r.get("emoji").and_then(Value::as_str).unwrap_or("").to_string();
            let (now, mine_now) = reaction(after, &emoji);
            let (was, mine_before) = reaction(before, &emoji);
            let by_me = u64::from(mine_now && !mine_before);
            if now > was + by_me {
                let said = after.get("text").and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                let what = if said.is_empty() { "your picture or voice line".to_string() } else { preview(&said) };
                told.push(format!("{emoji} on \"{what}\""));
            }
        }
    }
    let first = told.first()?.clone();
    let body = if told.len() > 1 { format!("{} new reactions. Latest: {}", told.len(), told.last().unwrap()) } else { format!("Someone reacted {first}") };
    Some(("A reaction in the room".to_string(), body))
}

/// Reactions to the member's own lines, told only when Settings > Chat > Reaction alerts is on.
pub fn reactions(app: &AppHandle, changed: &[(Value, Value)], chat_shown: bool) {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    if !settings.alert_reactions || !(settings.alert_sound || settings.alert_notify) || looking(app, chat_shown) {
        return;
    }
    let Some((title, body)) = reaction_news(changed, &crate::chatroom::my_line_ids(&store)) else { return };
    if settings.alert_sound {
        beep(app, &settings.alert_sound_name);
    }
    if settings.alert_notify {
        toast(app, &title, &body, "chat");
    }
}

pub fn summary(fresh: &[Value], mine: &[u64]) -> Option<(String, String)> {
    let others: Vec<&Value> = fresh.iter().filter(|l| l.get("id").and_then(Value::as_u64).is_none_or(|id| !mine.contains(&id))).collect();
    let last = others.last()?;
    let who = last.pointer("/author/name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Someone");
    let said = last.get("text").and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    let said = if said.is_empty() { "sent a picture or a voice line".to_string() } else { preview(&said) };
    let body = if others.len() > 1 { format!("{} new messages. Latest, {who}: {said}", others.len()) } else { format!("{who}: {said}") };
    Some(("New in the room".to_string(), body))
}

fn preview(text: &str) -> String {
    let mut short: String = text.chars().take(PREVIEW).collect();
    if text.chars().count() > PREVIEW {
        short.push('…');
    }
    short
}

/// Castle's own default pack, as WAV bytes Windows can play straight from memory. It is only the
/// starting set: any `.wav` the member (or a future theme) drops into the `alert-sounds` folder in
/// Castle's data folder is offered too, so nothing here limits what can be chosen.
const BUILT_IN: &[(&str, &str, &[u8])] = &[
    ("glass", "Glass", include_bytes!("../resources/alerts/glass.wav")),
    ("chord", "Chord", include_bytes!("../resources/alerts/chord.wav")),
    ("koto", "Koto", include_bytes!("../resources/alerts/koto.wav")),
    ("woodblock", "Woodblock", include_bytes!("../resources/alerts/woodblock.wav")),
    ("bell", "Information bell", include_bytes!("../resources/alerts/bell.wav")),
    ("flute", "Flit flute", include_bytes!("../resources/alerts/flute.wav")),
    ("glisten", "Glisten", include_bytes!("../resources/alerts/glisten.wav")),
];
/// The sound a fresh install (or a choice that has since gone missing) uses.
pub const DEFAULT_SOUND: &str = "glass";
/// A member's own file is chosen as "file:<name>.wav", so it can never be mistaken for a built-in.
const FILE_PREFIX: &str = "file:";

/// Where the member's own alert sounds go.
fn user_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("alert-sounds"))
}

/// A bare `.wav` file name (no folders, so a setting can never point outside the sounds folder).
fn is_wav_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".wav") && !name.contains(['/', '\\', ':']) && !name.contains("..") && name.len() <= 100
}

/// One choice in Settings' sound list.
#[derive(serde::Serialize)]
pub struct SoundChoice {
    pub id: String,
    pub label: String,
    pub built_in: bool,
}

/// Every sound the member can choose: Windows' own, Castle's pack, then their own files.
pub fn sound_choices(dir: Option<&std::path::Path>) -> Vec<SoundChoice> {
    // the system's own sound keeps the id "windows" on every platform; macOS names it its own way
    let system = if cfg!(target_os = "macos") { "macOS's own sound" } else { "Windows' own sound" };
    let mut all = vec![SoundChoice { id: "windows".into(), label: system.into(), built_in: true }];
    all.extend(BUILT_IN.iter().map(|(id, label, _)| SoundChoice { id: (*id).into(), label: (*label).into(), built_in: true }));
    if let Some(dir) = dir {
        let mut own: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| is_wav_name(n))
            .collect();
        own.sort_by_key(|n| n.to_ascii_lowercase());
        all.extend(own.into_iter().map(|n| SoundChoice { id: format!("{FILE_PREFIX}{n}"), label: n.trim_end_matches(".wav").trim_end_matches(".WAV").to_string(), built_in: false }));
    }
    all
}

/// The WAV bytes for a choice, or None for Windows' own sound or a choice that is gone.
fn wav_for(name: &str, dir: Option<&std::path::Path>) -> Option<Vec<u8>> {
    if let Some((_, _, wav)) = BUILT_IN.iter().find(|(id, _, _)| *id == name) {
        return Some(wav.to_vec());
    }
    let file = name.strip_prefix(FILE_PREFIX).filter(|n| is_wav_name(n))?;
    std::fs::read(dir?.join(file)).ok().filter(|b| b.len() > 44)
}

/// Plays the chosen alert sound at once, without waiting for it to end. Windows' own notification
/// sound follows the member's sound scheme and volume; the others are played from memory. A choice
/// that has gone missing falls back to the default rather than to silence.
#[cfg(windows)]
pub(crate) fn beep(app: &AppHandle, name: &str) -> bool {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    if name == "windows" {
        let alias: Vec<u16> = "SystemNotification\0".encode_utf16().collect();
        return unsafe { PlaySoundW(alias.as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_ASYNC | SND_NODEFAULT) != 0 };
    }
    let dir = user_dir(app);
    let wav = wav_for(name, dir.as_deref()).or_else(|| wav_for(DEFAULT_SOUND, None));
    let Some(wav) = wav else { return false };
    // asynchronous playback reads the buffer after this call returns, so it is kept for the life of
    // the program (a leak of a few hundred KB per distinct sound at most, since this is rare)
    let wav: &'static [u8] = Box::leak(wav.into_boxed_slice());
    unsafe { PlaySoundW(wav.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) != 0 }
}

/// macOS: the system's own notification sound for "windows" (the default system choice), else the
/// chosen WAV, written once to the temp folder and played by `afplay` without waiting.
#[cfg(target_os = "macos")]
pub(crate) fn beep(app: &AppHandle, name: &str) -> bool {
    let play = |file: &std::path::Path| std::process::Command::new("afplay").arg(file).spawn().is_ok();
    if name == "windows" {
        return play(std::path::Path::new("/System/Library/Sounds/Glass.aiff"));
    }
    let dir = user_dir(app);
    let wav = wav_for(name, dir.as_deref()).or_else(|| wav_for(DEFAULT_SOUND, None));
    let Some(wav) = wav else { return false };
    let file = std::env::temp_dir().join(format!("castle-alert-{}.wav", name.replace(|c: char| !c.is_ascii_alphanumeric(), "-")));
    std::fs::write(&file, wav).is_ok() && play(&file)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn beep(_app: &AppHandle, _name: &str) -> bool {
    false
}

/// Settings' sound list.
#[tauri::command]
pub fn alert_sounds(app: AppHandle) -> Vec<SoundChoice> {
    sound_choices(user_dir(&app).as_deref())
}

/// Settings' "Play" button: the member hears the sound they are choosing.
#[tauri::command]
pub fn alert_sound_preview(app: AppHandle, name: String) -> bool {
    beep(&app, &name)
}

/// Settings' "Open sounds folder": makes the folder if it is not there yet and shows it, so the
/// member can drop their own `.wav` files in. Returns the folder's path.
#[tauri::command]
pub fn alert_sounds_folder(app: AppHandle) -> Result<String, String> {
    let dir = user_dir(&app).ok_or("Castle could not find its data folder.")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer.exe").arg(&dir).creation_flags(0x0800_0000).spawn();
    }
    Ok(dir.to_string_lossy().into_owned())
}

/// The member is looking at the room when CHAT is the tab on screen in a visible, focused Castle.
fn looking(app: &AppHandle, chat_shown: bool) -> bool {
    chat_shown && app.get_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

/// Does the text name one of `names` as a whole word (or phrase), case and spacing aside?
/// "Ali" never matches inside "Alice", and "Kiyo," or "@Kiyo" still name Kiyo. Pure, tested.
pub fn names_someone(text: &str, names: &[String]) -> bool {
    let squeeze = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let text = squeeze(text);
    names.iter().map(|n| squeeze(n)).filter(|n| !n.is_empty()).any(|name| {
        let mut from = 0;
        while let Some(at) = text[from..].find(&name) {
            let start = from + at;
            let end = start + name.len();
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
                return true;
            }
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
        false
    })
}

/// The member's display name in the room (their Foundation name), asked once and kept until
/// sign-in, sign-out or a display-name change forgets it.
static ROOM_NAME: Mutex<Option<String>> = Mutex::new(None);

fn room_name() -> Option<String> {
    if let Some(name) = ROOM_NAME.lock().unwrap().clone() {
        return Some(name);
    }
    let token = crate::key::load("member")?;
    let me = crate::chatroom::Room::new(&crate::signin::portfolio(), &token).me().ok()?;
    let name = me.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty())?.to_string();
    *ROOM_NAME.lock().unwrap() = Some(name.clone());
    Some(name)
}

/// The kept room name is out of date (signed in or out, or the display name changed).
pub fn forget_name() {
    *ROOM_NAME.lock().unwrap() = None;
}

/// The names that make a line worth an alert in "named" mode: the member's name in Settings,
/// their name in the room, and their companion's name.
fn names_to_hear(settings: &Settings) -> Vec<String> {
    let mut names: Vec<String> = [settings.member_name.clone(), room_name(), settings.companion_name.clone()].into_iter().flatten().collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by_key(|n| n.to_lowercase());
    names
}

/// Called with the lines a read just found (never the first read after start or sign-in).
pub fn new_lines(app: &AppHandle, fresh: &[Value], chat_shown: bool) {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    if !(settings.alert_sound || settings.alert_notify) || looking(app, chat_shown) {
        return;
    }
    // the member's own companion speaking is not news to tell them about
    let mut fresh: Vec<Value> = fresh.iter().filter(|l| !crate::roomturn::companion_said(&settings, l)).cloned().collect();
    if settings.alert_when == "named" {
        let names = names_to_hear(&settings);
        let mine = crate::chatroom::my_line_ids(&store);
        // a reply to one of the member's own lines counts as naming them
        fresh.retain(|l| l.get("text").and_then(Value::as_str).is_some_and(|t| names_someone(t, &names)) || replies_to(l, &mine));
    }
    let Some((title, body)) = summary(&fresh, &crate::chatroom::my_line_ids(&store)) else { return };
    if settings.alert_sound {
        beep(app, &settings.alert_sound_name);
    }
    if settings.alert_notify {
        // No `.sound(..)`: with tauri-plugin-notification 2.8 / notify-rust 4.19 a Windows toast built
        // without one is `<audio silent="true"/>`, so the Sound switch is the only sound (checked in
        // the dependency source, not by ear).
        toast(app, &title, &body, "chat");
    }
}

/// The address a notification carries, one per room: clicking it (live, or later from the Action
/// Center, even when Castle was closed in between) starts Castle with it, and Castle opens that room.
#[cfg_attr(not(windows), allow(dead_code))] // the Windows notification's parts
pub fn room_url(room: &str) -> String {
    format!("memorycore-castle://open?room={room}")
}

/// A room id as the page names its tabs: lower-case letters only, so an address can't carry more.
fn clean_room(room: &str) -> Option<String> {
    let room = room.trim().to_ascii_lowercase();
    (!room.is_empty() && room.len() <= 24 && room.chars().all(|c| c.is_ascii_lowercase())).then_some(room)
}

/// Brings Castle forward and shows a room (`"chat"`, `"home"`, ...).
pub fn open_room(app: &AppHandle, room: &str) {
    use tauri::Emitter;
    crate::tray::show_main(app);
    // If Windows still kept the keyboard focus from Castle (it may when Castle was not the program the
    // member just clicked), the window is at least raised above the others, not just flashed.
    if let Some(window) = app.get_window("main") {
        let _ = window.set_always_on_top(true);
        let _ = window.set_always_on_top(false);
    }
    let _ = app.emit("castle-open-room", room);
}

/// The room a launch asked for (Castle's own address, from a clicked notification), if it did.
/// `memorycore-castle://chat` from the first version still means CHAT.
pub fn wanted_room<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter().find_map(|a| {
        let lower = a.trim().trim_matches('"').trim_end_matches('/').to_ascii_lowercase();
        let rest = lower.strip_prefix("memorycore-castle://")?.trim_end_matches('/');
        if rest == "chat" {
            return Some("chat".to_string());
        }
        // Windows hands a clicked notification's address over as `memorycore-castle://open/?room=chat`
        // (it adds a "/" after the name), so a "/" before the "?" is accepted as well
        let query = rest.strip_prefix("open?").or_else(|| rest.strip_prefix("open/?"))?;
        query.split('&').find_map(|kv| kv.strip_prefix("room=")).and_then(clean_room)
    })
}

#[cfg_attr(not(windows), allow(dead_code))] // the Windows notification's parts
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The toast's XML: silent (the Sound choice is the only sound) and carrying the room's address, so a click
/// opens Castle through Windows itself and does not depend on this process still listening for it.
#[cfg_attr(not(windows), allow(dead_code))] // the Windows notification's parts
fn toast_xml(title: &str, body: &str, room: &str) -> String {
    format!(
        r#"<toast activationType="protocol" launch="{url}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><audio silent="true"/></toast>"#,
        xml_escape(title),
        xml_escape(body),
        url = xml_escape(&room_url(room))
    )
}

/// Teaches Windows Castle's address, once per run: `memorycore-castle://chat` starts this exe (or,
/// with Castle already running, brings it forward through the single-instance hand-off). Skipped for
/// a build running from its own build folder, so a test build never claims the installed address.
#[cfg(windows)]
pub fn register_address() {
    use std::os::windows::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return };
    if exe.parent().is_some_and(|d| d.ends_with("target/release") || d.ends_with("target/debug")) {
        return;
    }
    std::thread::spawn(move || {
        let key = r"HKCU\Software\Classes\memorycore-castle";
        let command = format!("\"{}\" \"%1\"", exe.display());
        for (path, name, value) in [
            (key.to_string(), None, "URL:MemoryCore Castle".to_string()),
            (key.to_string(), Some("URL Protocol"), String::new()),
            (format!(r"{key}\shell\open\command"), None, command),
        ] {
            let mut cmd = std::process::Command::new("reg");
            cmd.args(["add", &path]);
            match name {
                Some(n) => cmd.args(["/v", n]),
                None => cmd.arg("/ve"),
            };
            let _ = cmd.args(["/d", &value, "/f"]).creation_flags(0x0800_0000).status();
        }
    });
}

#[cfg(not(windows))]
pub fn register_address() {}

/// The desktop notification. On Windows it is built here (not through the plugin) so a click brings
/// Castle forward and opens the room: through the room's address, which works for a notification that has sat in
/// the Action Center too, not only one clicked while it is still on screen. Elsewhere it is the
/// plugin's plain notification.
#[cfg(windows)]
pub(crate) fn toast(app: &AppHandle, title: &str, body: &str, room: &str) {
    use windows::{core::HSTRING, Data::Xml::Dom::XmlDocument, UI::Notifications::{ToastNotification, ToastNotificationManager}};
    // the id the plugin uses: Castle's own when installed, PowerShell's while running from a build folder
    let app_id = match std::env::current_exe() {
        Ok(exe) if exe.parent().is_some_and(|d| d.ends_with("target/release") || d.ends_with("target/debug")) => {
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe".to_string()
        }
        _ => app.config().identifier.clone(),
    };
    let xml = toast_xml(title, body, room);
    let shown = (|| -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let notice = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?.Show(&notice)
    })();
    if shown.is_err() {
        // the toast could not be made: the plugin's plain one still tells the member
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

#[cfg(not(windows))]
pub(crate) fn toast(app: &AppHandle, title: &str, body: &str, _room: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(id: u64, who: &str, text: &str) -> Value {
        json!({ "id": id, "text": text, "author": { "name": who, "kind": "member" } })
    }

    #[test]
    fn a_notification_carries_castles_address_so_a_late_click_still_opens_chat() {
        let xml = toast_xml("A & B", "<hi> \"there\"", "home");
        assert!(xml.contains(r#"activationType="protocol" launch="memorycore-castle://open?room=home""#));
        assert!(xml.contains("A &amp; B") && xml.contains("&lt;hi&gt; &quot;there&quot;"), "text is escaped");
        assert!(xml.contains(r#"<audio silent="true"/>"#), "the Sound choice is the only sound");
    }

    #[test]
    fn a_launch_by_castles_address_names_the_room() {
        let args = |a: &str| vec!["castle.exe".to_string(), a.to_string()];
        assert_eq!(wanted_room(args("memorycore-castle://open?room=home")), Some("home".into()));
        assert_eq!(wanted_room(args("\"MEMORYCORE-CASTLE://open?room=Market/\"")), Some("market".into()));
        assert_eq!(wanted_room(args("memorycore-castle://chat")), Some("chat".into()), "the first version's address still works");
        assert_eq!(wanted_room(args("memorycore-castle://open?room=../x")), None);
        assert_eq!(wanted_room(args("memorycore-castle://open?room=")), None);
        // the forms Windows really passes for a clicked notification (checked on Windows 10: it inserts
        // a "/" after "open", and may end the address with one)
        assert_eq!(wanted_room(args("memorycore-castle://open/?room=chat")), Some("chat".into()));
        assert_eq!(wanted_room(args("memorycore-castle://open/?room=chat/")), Some("chat".into()));
        assert_eq!(wanted_room(args("\"memorycore-castle://open/?room=home\"")), Some("home".into()));
        assert_eq!(wanted_room(args("memorycore-castle://chat/")), Some("chat".into()));
        assert_eq!(wanted_room(args("memorycore-castle://open/?room=../x")), None);
        assert_eq!(wanted_room(args("memorycore-castle://open/?foo=bar")), None);
        assert_eq!(wanted_room(args("--sign-in")), None);
    }

    #[test]
    fn the_sound_list_is_the_pack_plus_whatever_the_member_added() {
        let none = sound_choices(None);
        assert_eq!(none[0].id, "windows");
        assert!(none.iter().any(|c| c.id == DEFAULT_SOUND), "the default is always offered");
        let dir = std::env::temp_dir().join(format!("castle-alert-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("My Ping.wav"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("notes.txt"), b"not a sound").unwrap();
        let with = sound_choices(Some(&dir));
        let mine = with.iter().find(|c| c.id == "file:My Ping.wav").expect("their own wav is offered");
        assert_eq!((mine.label.as_str(), mine.built_in), ("My Ping", false));
        assert!(!with.iter().any(|c| c.id.contains("notes")), "only .wav files are offered");
        assert_eq!(wav_for("file:My Ping.wav", Some(&dir)).map(|b| b.len()), Some(100));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_choice_can_only_name_a_wav_inside_the_sounds_folder() {
        for bad in ["file:..\\secret.wav", "file:/etc/x.wav", "file:C:\\x.wav", "file:a.txt", "file:", "nope"] {
            assert!(wav_for(bad, Some(std::path::Path::new("."))).is_none(), "{bad} must not resolve");
        }
        assert!(wav_for("glass", None).is_some_and(|b| b.starts_with(b"RIFF")), "the pack is real WAV data");
        assert!(wav_for("windows", None).is_none(), "Windows' own sound is not a file");
    }

    #[test]
    fn several_lines_are_counted_and_the_latest_is_shown() {
        let (_, body) = summary(&[line(1, "Kiyo", "a"), line(2, "Alice", "b")], &[]).unwrap();
        assert_eq!(body, "2 new messages. Latest, Alice: b");
    }

    #[test]
    fn the_members_own_lines_never_alert() {
        assert!(summary(&[line(7, "Bernard", "hi")], &[7]).is_none());
        let (_, body) = summary(&[line(7, "Bernard", "hi"), line(8, "Kiyo", "hey")], &[7]).unwrap();
        assert_eq!(body, "Kiyo: hey");
        assert!(summary(&[], &[]).is_none());
    }

    #[test]
    fn long_and_empty_lines_are_handled() {
        let long = "x".repeat(200);
        let (_, body) = summary(&[line(1, "Kiyo", &long)], &[]).unwrap();
        assert_eq!(body.chars().count(), "Kiyo: ".len() + PREVIEW + 1);
        assert!(body.ends_with('…'));
        let (_, body) = summary(&[json!({ "id": 2, "text": "", "author": { "name": "  " } })], &[]).unwrap();
        assert_eq!(body, "Someone: sent a picture or a voice line");
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_name_counts_only_as_a_whole_word() {
        let heard = names(&["Kiyo", "Ali"]);
        assert!(names_someone("hi Kiyo, how are you?", &heard));
        assert!(names_someone("@kiyo look at this", &heard), "case aside, an @ in front is fine");
        assert!(names_someone("thanks KIYO!", &heard));
        assert!(!names_someone("Kiyoraka pushed the PR", &heard), "not inside a longer word");
        assert!(!names_someone("Alice said hello", &heard), "Ali is not inside Alice");
        assert!(names_someone("Ali: are you there", &heard));
    }

    #[test]
    fn a_name_of_two_words_is_heard_as_a_phrase() {
        let heard = names(&["Alice Test"]);
        assert!(names_someone("what do you think, alice   test?", &heard), "spacing aside");
        assert!(!names_someone("Alice is a test", &heard));
        assert!(!names_someone("Alice Tester", &heard));
    }

    #[test]
    fn no_names_hear_nothing() {
        assert!(!names_someone("hello everyone", &[]));
        assert!(!names_someone("hello everyone", &names(&["", "   "])), "empty names never match everything");
        assert!(!names_someone("", &names(&["Kiyo"])));
    }

    #[test]
    fn a_reply_to_my_line_counts_and_only_others_reactions_are_news() {
        use serde_json::json;
        assert!(replies_to(&json!({"id": 9, "replyTo": {"id": 4, "name": "Me", "text": "hi"}}), &[4]));
        assert!(!replies_to(&json!({"id": 9, "replyTo": {"id": 5}}), &[4]));
        assert!(!replies_to(&json!({"id": 9}), &[4]));
        let before = json!({"id": 4, "text": "my line", "reactions": [{"emoji": "👍", "count": 1, "mine": false}]});
        let after = json!({"id": 4, "text": "my line", "reactions": [{"emoji": "👍", "count": 2, "mine": false}]});
        let (title, body) = reaction_news(&[(before.clone(), after)], &[4]).unwrap();
        assert_eq!(title, "A reaction in the room");
        assert!(body.contains("👍") && body.contains("my line"));
        // the member's own tap is not news
        let mine = json!({"id": 4, "text": "my line", "reactions": [{"emoji": "👍", "count": 2, "mine": true}]});
        assert!(reaction_news(&[(before.clone(), mine)], &[4]).is_none());
        // someone else's line is not the member's news
        let other = json!({"id": 7, "text": "x", "reactions": [{"emoji": "👍", "count": 5, "mine": false}]});
        assert!(reaction_news(&[(json!({"id": 7}), other)], &[4]).is_none());
    }
}
