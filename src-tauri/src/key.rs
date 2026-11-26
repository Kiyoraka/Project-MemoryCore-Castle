//! The member's keys, kept encrypted by Windows Credential Manager (tied to their Windows sign-in).
//! A key never touches a file and never reaches the page except masked, or when the member
//! asks to see it with Show.

use keyring::Entry;
use serde::Serialize;

pub(crate) const SERVICE: &str = "com.memorycore.castle";

/// Every key Castle can keep: (account, label shown in Settings).
pub const KEYS: [(&str, &str); 10] = [
    ("openai", "ChatGPT (OpenAI)"),
    ("seedance", "Seedance"),
    ("suno", "Suno"),
    ("elevenlabs", "ElevenLabs"),
    // the member's sign-in to the MemoryCore Foundation room (made by Sign in, never pasted)
    ("member", "MemoryCore sign-in"),
    // the companion's own room key (made by Castle with the member's sign-in, never shown)
    ("room-companion", "Companion room key"),
    // the member's own Telegram bot, for MemoryCore in your pocket (MESSAGING)
    ("telegram-bridge", "Telegram bot token"),
    // the member's own Discord bot, for MemoryCore in your pocket (MESSAGING)
    ("discord-bridge", "Discord bot token"),
    ("automation-http", "Automation HTTP authentication"),
    // the member's own Google Safe Browsing key, for the browser's DANGER screen (browser_safe.rs)
    ("google-safe-browsing", "Google Safe Browsing"),
];

fn known(account: &str) -> Result<(), String> {
    if KEYS.iter().any(|(name, _)| *name == account) {
        Ok(())
    } else {
        Err(format!("Castle does not keep a key called {account}."))
    }
}

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|_| format!("{} could not be opened.", if cfg!(windows) { "Windows Credential Manager" } else { "The Keychain" }))
}

/// macOS asks the member before an app reads its own Keychain entries (and asks again whenever the
/// app's build changes), so each key is read from the Keychain at most once per run and kept here
/// until Castle closes. Windows Credential Manager never asks, so Windows reads it every time.
#[cfg(target_os = "macos")]
static READ: std::sync::Mutex<Option<std::collections::HashMap<String, Option<String>>>> = std::sync::Mutex::new(None);

#[cfg(target_os = "macos")]
fn remember(account: &str, key: Option<String>) {
    READ.lock().unwrap().get_or_insert_with(Default::default).insert(account.to_string(), key);
    if let Some(refused) = REFUSED.lock().unwrap().as_mut() {
        refused.remove(account);
    }
}

fn save_as(account: &str, key: &str) -> Result<(), String> {
    entry(account)?
        .set_password(key)
        .map_err(|_| format!("The key could not be kept in {}.", if cfg!(windows) { "Windows Credential Manager" } else { "the Keychain" }))?;
    #[cfg(target_os = "macos")]
    remember(account, Some(key.to_string()).filter(|k| !k.is_empty()));
    Ok(())
}

/// A read the Keychain refused (the member clicked Deny, or missed the prompt after a new build) is
/// tried again after this long - never kept for the whole run, or a bridge stays "not set up" until
/// Castle restarts - but not at once either, so a refused prompt does not come straight back.
#[cfg(target_os = "macos")]
const RETRY_REFUSED: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(target_os = "macos")]
static REFUSED: std::sync::Mutex<Option<std::collections::HashMap<String, std::time::Instant>>> = std::sync::Mutex::new(None);

/// Whether a Keychain answer may be kept for the rest of the run: a key, or "there is none".
/// Anything else (access refused, a platform failure) is asked again later.
fn keepable<T>(read: &Result<T, keyring::Error>) -> bool {
    matches!(read, Ok(_) | Err(keyring::Error::NoEntry))
}

fn load_as(account: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(known) = READ.lock().unwrap().as_ref().and_then(|read| read.get(account)) {
            return known.clone();
        }
        let refused_at = REFUSED.lock().unwrap().as_ref().and_then(|r| r.get(account).copied());
        if refused_at.is_some_and(|at| at.elapsed() < RETRY_REFUSED) {
            return None;
        }
    }
    let read = entry(account).ok()?.get_password();
    let key = read.as_ref().ok().filter(|key| !key.is_empty()).cloned();
    #[cfg(target_os = "macos")]
    if keepable(&read) {
        remember(account, key.clone());
    } else {
        REFUSED.lock().unwrap().get_or_insert_with(Default::default).insert(account.to_string(), std::time::Instant::now());
    }
    key
}

fn forget_as(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            #[cfg(target_os = "macos")]
            remember(account, None);
            Ok(())
        }
        Err(_) => Err(format!("The key could not be removed from {}.", if cfg!(windows) { "Windows Credential Manager" } else { "the Keychain" })),
    }
}

/// The secret in one automation's web-request link, kept by Windows like the keys, never listed
/// in Settings and never put in a job's definition or run output.
fn hook_account(job: u64) -> String {
    format!("automation-hook-{job}")
}

pub fn hook_secret_load(job: u64) -> Option<String> {
    load_as(&hook_account(job))
}

pub fn hook_secret_save(job: u64, secret: &str) -> Result<(), String> {
    save_as(&hook_account(job), secret)
}

pub fn hook_secret_forget(job: u64) -> Result<(), String> {
    forget_as(&hook_account(job))
}

/// The password of Castle's own database engine: kept by Windows like the keys, but never
/// listed in Settings and never shown.
const DB_ROOT: &str = "db-root";

pub fn db_root() -> Option<String> {
    load_as(DB_ROOT)
}

pub fn keep_db_root(password: &str) -> Result<(), String> {
    save_as(DB_ROOT, password)
}

/// A DEV project's database password ("local" or "production"), by the project's id
/// (`projectdb_login::project_id`). Kept by Windows like the keys, never listed in Settings and
/// never sent to the page.
fn db_account(side: &str, id: &str) -> String {
    format!("{}-{id}", if side == "production" { "db-prod" } else { "db-local" })
}

pub fn load_db_password(side: &str, id: &str) -> Option<String> {
    load_as(&db_account(side, id))
}

pub fn save_db_password(side: &str, id: &str, password: &str) -> Result<(), String> {
    save_as(&db_account(side, id), password)
}

pub fn forget_db_password(side: &str, id: &str) -> Result<(), String> {
    forget_as(&db_account(side, id))
}

/// Shows only the ends of a key: "sk-p…a9F2". Short keys show only the last two characters.
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        let tail: String = chars[chars.len().saturating_sub(2)..].iter().collect();
        return format!("…{tail}");
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// What Settings shows for one key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub name: String,
    pub label: String,
    pub set: bool,
    pub masked: Option<String>,
}

pub fn statuses() -> Vec<KeyStatus> {
    KEYS.iter()
        .map(|(name, label)| {
            let key = load_as(name);
            KeyStatus {
                name: (*name).into(),
                label: (*label).into(),
                set: key.is_some(),
                masked: key.as_deref().map(mask),
            }
        })
        .collect()
}

pub fn save(account: &str, key: &str) -> Result<(), String> {
    known(account)?;
    let key = key.trim();
    if key.is_empty() {
        return Err("The key is empty.".into());
    }
    save_as(account, key)
}

/// The key itself, for the CLI's environment or the room client. Never sent to the page
/// except through `reveal`.
pub fn load(account: &str) -> Option<String> {
    known(account).ok()?;
    load_as(account)
}

pub fn forget(account: &str) -> Result<(), String> {
    known(account)?;
    forget_as(account)
}

/// What a key test found, in words.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyTest {
    pub ok: bool,
    pub message: String,
}

/// Checks a kept key against the service's free account endpoint - never anything that costs.
pub fn test(account: &str) -> KeyTest {
    let said = |ok: bool, message: &str| KeyTest { ok, message: message.into() };
    let Some(key) = load(account) else { return said(false, "No key is kept for this yet.") };
    let (url, header) = match account {
        "openai" => ("https://api.openai.com/v1/models", "bearer"),
        "suno" => ("https://api.sunoapi.org/api/v1/generate/credit", "bearer"),
        "elevenlabs" => ("https://api.elevenlabs.io/v1/user", "xi-api-key"),
        "seedance" => return said(true, "Kept. Seedance has no free check, so Castle cannot test it without spending."),
        "google-safe-browsing" => return google_test(&key),
        _ => return said(true, "Kept. The room checks it each time it is used."),
    };
    let client = match reqwest::blocking::Client::builder()
        .user_agent(concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(_) => return said(false, "The check could not start."),
    };
    let request = client.get(url);
    let request = if header == "bearer" { request.bearer_auth(&key) } else { request.header(header, &key) };
    match request.send() {
        Ok(response) if response.status().is_success() => said(true, "The key works."),
        Ok(response) if response.status().as_u16() == 401 || response.status().as_u16() == 403 => said(false, "The service refused this key. Check it and save it again."),
        Ok(response) => said(false, &format!("The service answered {}. Try again later.", response.status().as_u16())),
        Err(_) => said(false, "The service could not be reached. Check the internet connection."),
    }
}

/// Google takes its key in the address: the free list of lists answers 200 for a good key.
fn google_test(key: &str) -> KeyTest {
    let said = |ok: bool, message: &str| KeyTest { ok, message: message.into() };
    let Ok(client) = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(15)).build() else { return said(false, "The check could not start.") };
    match client.get(format!("https://safebrowsing.googleapis.com/v4/threatLists?key={key}")).send() {
        Ok(response) if response.status().is_success() => said(true, "The key works. The browser now checks pages against Google's lists."),
        Ok(response) if [400, 401, 403].contains(&response.status().as_u16()) => said(false, "Google refused this key. Check that the Safe Browsing API is enabled for it, then save it again."),
        Ok(response) => said(false, &format!("Google answered {}. Try again later.", response.status().as_u16())),
        Err(_) => said(false, "Google could not be reached. Check the internet connection."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_a_key_is_read_from_the_keychain_once_per_run() {
        let account = format!("castle-cache-{}", std::process::id());
        save_as(&account, "sk-test-only-1111").unwrap();
        // gone from the Keychain behind Castle's back: this run still has what it read
        entry(&account).unwrap().delete_credential().unwrap();
        assert_eq!(load_as(&account).as_deref(), Some("sk-test-only-1111"));
        forget_as(&account).unwrap();
        assert_eq!(load_as(&account), None);
    }

    #[test]
    fn a_refused_keychain_read_is_asked_again_not_kept_for_the_run() {
        assert!(keepable(&Ok::<_, keyring::Error>("sk")), "a key is kept");
        assert!(keepable(&Err::<String, _>(keyring::Error::NoEntry)), "no key is kept");
        let refused = keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("denied")));
        assert!(!keepable(&Err::<String, _>(refused)), "a refused read is tried again");
        let failed = keyring::Error::PlatformFailure(Box::new(std::io::Error::other("busy")));
        assert!(!keepable(&Err::<String, _>(failed)), "a platform failure is tried again");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_saved_key_clears_an_earlier_refusal() {
        let account = format!("castle-refused-{}", std::process::id());
        REFUSED.lock().unwrap().get_or_insert_with(Default::default).insert(account.clone(), std::time::Instant::now());
        assert_eq!(load_as(&account), None, "inside the wait, the Keychain is not asked");
        save_as(&account, "sk-test-only-2222").unwrap();
        assert_eq!(load_as(&account).as_deref(), Some("sk-test-only-2222"));
        forget_as(&account).unwrap();
    }

    #[test]
    fn a_key_is_kept_read_back_and_forgotten() {
        let account = format!("castle-test-{}", std::process::id());
        assert_eq!(load_as(&account), None);
        save_as(&account, "sk-test-only-0000").unwrap();
        assert_eq!(load_as(&account).as_deref(), Some("sk-test-only-0000"));
        forget_as(&account).unwrap();
        assert_eq!(load_as(&account), None);
        // Forgetting twice is fine.
        forget_as(&account).unwrap();
    }

    #[test]
    fn masks_show_only_the_ends() {
        assert_eq!(mask("sk-proj-abcdefgh1234a9F2"), "sk-p…a9F2");
        assert_eq!(mask("abc"), "…bc");
        assert_eq!(mask(""), "…");
    }

    #[test]
    fn only_known_keys_are_kept() {
        assert!(save("not-a-key", "x").is_err());
        assert!(load("not-a-key").is_none());
        assert!(save("openai", "   ").is_err());
        assert_eq!(statuses().len(), 10);
        assert!(known("automation-http").is_ok());
        assert!(known(crate::browser_safe::KEY).is_ok(), "the browser's Google key is one Castle keeps");
    }
}
