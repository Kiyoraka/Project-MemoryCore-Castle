//! VoiceStudio (a text-to-speech app the member runs on this same computer, on http://127.0.0.1:3900)
//! as an optional STUDIO Voice power, for people who self-host their own voice. It is off until the
//! member turns it on in Settings > Companion; while it is off Castle never contacts it. Nothing
//! leaves this computer, and Castle never starts VoiceStudio: it only talks to it while it runs.

use crate::settings::SettingsStore;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

const BASE: &str = "http://127.0.0.1:3900";

const OFF: &str = "Local VoiceStudio is off. Turn it on in Settings > Companion.";

fn require_on(on: bool) -> Result<(), String> {
    if on { Ok(()) } else { Err(OFF.into()) }
}

/// True only when the member turned it on AND VoiceStudio answers; the page uses it to show the
/// Voice card as ready. A short check, and no request at all while the setting is off.
#[tauri::command]
pub async fn voicestudio_status(store: State<'_, SettingsStore>) -> Result<bool, String> {
    if !store.get().voicestudio_on {
        return Ok(false);
    }
    Ok(tauri::async_runtime::spawn_blocking(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
            .ok()
            .and_then(|c| c.get(format!("{BASE}/health")).send().ok())
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false))
}

/// Speaks `text` with VoiceStudio and saves the clip to media-generation/voice/ in the memory
/// folder, where STUDIO's Voice tab lists it. Returns the saved file's path.
#[tauri::command]
pub async fn voicestudio_speak(text: String, store: State<'_, SettingsStore>) -> Result<String, String> {
    require_on(store.get().voicestudio_on)?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Type what VoiceStudio should say.".into());
    }
    let folder = store.get().memory_folder.ok_or("Choose the memory folder first.")?;
    tauri::async_runtime::spawn_blocking(move || speak(&text, Path::new(&folder))).await.map_err(|_| "VoiceStudio did not answer.".to_string())?
}

/// Used only when VoiceStudio has no saved voice.
const VOICE: &str = "female, low pitch, british accent";
/// A saved voice of VoiceStudio's (its own, not the demo), as (id, language). A saved voice also keeps
/// short words from coming out as noise, and keeps Castle sounding like the same person every time.
fn saved_voice(client: &reqwest::blocking::Client) -> Option<(String, String)> {
    let list = client.get(format!("{BASE}/profiles")).send().ok()?.json::<serde_json::Value>().ok()?;
    pick_voice(&list)
}

/// The member's own voice with a reference sample, else any voice that has one.
fn pick_voice(list: &serde_json::Value) -> Option<(String, String)> {
    let rows = list.as_array()?;
    let usable = |p: &&serde_json::Value| p.get("ref_audio_path").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
    let own = rows.iter().filter(usable).find(|p| p.get("is_demo").and_then(|v| v.as_i64()) == Some(0));
    let pick = own.or_else(|| rows.iter().find(usable))?;
    let id = pick.get("id")?.as_str()?.to_string();
    let language = pick.get("language").and_then(|v| v.as_str()).unwrap_or("English").to_string();
    Some((id, language))
}

/// VoiceStudio's model sometimes returns noise instead of speech (most often for one or two words).
/// Each clip is checked with its own transcriber and made again, up to this many times.
const ATTEMPTS: usize = 5;

fn generate(client: &reqwest::blocking::Client, text: &str, voice: &Option<(String, String)>) -> Result<Vec<u8>, String> {
    let form = reqwest::blocking::multipart::Form::new().text("text", text.to_string());
    let form = match voice {
        Some((id, language)) => form.text("profile_id", id.clone()).text("language", language.clone()),
        None => form.text("instruct", VOICE.to_string()),
    };
    let response = client
        .post(format!("{BASE}/generate"))
        .multipart(form)
        .send()
        .map_err(|_| "VoiceStudio is not running. Open it, wait for it to finish starting, and try again.".to_string())?;
    if !response.status().is_success() {
        return Err(format!("VoiceStudio refused the request ({}).", response.status().as_u16()));
    }
    let bytes = response.bytes().map_err(|_| "VoiceStudio's answer was cut off.".to_string())?;
    if bytes.len() < 64 || &bytes[..4] != b"RIFF" {
        return Err("VoiceStudio did not return audio.".into());
    }
    Ok(bytes.to_vec())
}

/// True when VoiceStudio's transcriber hears words in the clip. If the check itself cannot run,
/// the clip is kept rather than thrown away.
fn has_speech(client: &reqwest::blocking::Client, wav: &[u8]) -> bool {
    let Ok(part) = reqwest::blocking::multipart::Part::bytes(wav.to_vec()).file_name("clip.wav").mime_str("audio/wav") else { return true };
    let form = reqwest::blocking::multipart::Form::new().part("audio", part);
    match client.post(format!("{BASE}/transcribe")).multipart(form).send() {
        Ok(r) if r.status().is_success() => r
            .json::<serde_json::Value>()
            .ok()
            .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|t| !t.trim().is_empty()))
            .unwrap_or(true),
        _ => true,
    }
}

fn speak(text: &str, folder: &Path) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|_| "Castle could not prepare the request.".to_string())?;
    let voice = saved_voice(&client);
    let mut clip = None;
    for _ in 0..ATTEMPTS {
        let wav = generate(&client, text, &voice)?;
        let good = has_speech(&client, &wav);
        clip = Some(wav);
        if good {
            break;
        }
        clip = None;
    }
    let bytes = clip.ok_or("VoiceStudio only made noise for that text. Try a longer sentence.".to_string())?;
    let dir = folder.join("media-generation").join("voice");
    std::fs::create_dir_all(&dir).map_err(|_| "Castle could not create the voice folder.".to_string())?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("voicestudio-{stamp}.wav"));
    std::fs::write(&path, &bytes).map_err(|_| "Castle could not save the clip.".to_string())?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn speaking_is_refused_while_the_setting_is_off() {
        assert!(require_on(true).is_ok());
        assert!(require_on(false).unwrap_err().contains("Settings"));
    }

    #[test]
    fn picks_the_members_own_voice_before_the_demo() {
        let list = json!([
            { "id": "demo0001", "is_demo": 1, "ref_audio_path": "d.wav", "language": "English" },
            { "id": "mine", "is_demo": 0, "ref_audio_path": "m.wav", "language": "Malay" },
        ]);
        assert_eq!(pick_voice(&list), Some(("mine".into(), "Malay".into())));
    }

    #[test]
    fn falls_back_to_the_demo_and_skips_voices_without_a_sample() {
        let list = json!([
            { "id": "empty", "is_demo": 0, "ref_audio_path": "", "language": "English" },
            { "id": "demo0001", "is_demo": 1, "ref_audio_path": "d.wav" },
        ]);
        assert_eq!(pick_voice(&list), Some(("demo0001".into(), "English".into())));
        assert_eq!(pick_voice(&json!([{ "id": "empty", "ref_audio_path": "" }])), None);
        assert_eq!(pick_voice(&json!({})), None);
    }

    /// Needs VoiceStudio running on this computer: `cargo test voicestudio -- --ignored`.
    #[test]
    #[ignore]
    fn speaks_saves_and_shows_in_the_voice_list() {
        let folder = std::env::temp_dir().join(format!("castle-voicestudio-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = speak("Hello Bernard, this is a test of Castle and VoiceStudio.", &folder).expect("speak");
        let bytes = std::fs::read(&path).expect("saved clip");
        assert_eq!(&bytes[..4], b"RIFF");
        assert!(bytes.len() > 10_000, "clip too short: {}", bytes.len());
        let lib = crate::studio::library(&folder);
        assert!(!lib.voices.is_empty(), "the Voice tab should list the clip");
        println!("saved {path} ({} bytes)", bytes.len());
        let client = reqwest::blocking::Client::new();
        for word in ["test", "hello", "okay"] {
            let p = speak(word, &folder).unwrap_or_else(|e| panic!("'{word}': {e}"));
            assert!(has_speech(&client, &std::fs::read(&p).unwrap()), "'{word}' came out as noise");
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
