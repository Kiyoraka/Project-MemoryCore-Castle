//! The browser's vault: the member's saved passwords, each sealed with AES-256-GCM before it reaches
//! their own memory database (032_browser_import.sql). The key is made once and kept in the member's
//! own Windows Credential Manager (the Keychain on a Mac), so the database alone opens nothing. Only
//! Castle's own code ever opens a password - for the member's Fill press - and no password is ever
//! returned to the page, logged, or seen by the companion. Bookmarks live beside it.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use mysql::{prelude::Queryable, Conn};
use serde::Serialize;

const ACCOUNT: &str = "browser-vault";
const NONCE: usize = 12;

fn sql(what: &'static str) -> impl Fn(mysql::Error) -> String {
    move |_| format!("The vault: {what} did not work.")
}

/// Seals a password with the key: nonce + ciphertext. Pure but for the random nonce.
pub fn seal(key: &[u8; 32], plain: &str) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "The vault's key is not right.".to_string())?;
    let mut nonce = [0u8; NONCE];
    getrandom::fill(&mut nonce).map_err(|_| "The computer could not make a seal.".to_string())?;
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).map_err(|_| "A password could not be sealed.".to_string())?);
    Ok(out)
}

/// Opens a sealed password; a wrong key or a changed seal opens nothing.
pub fn open(key: &[u8; 32], sealed: &[u8]) -> Result<String, String> {
    if sealed.len() <= NONCE {
        return Err("That seal is broken.".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "The vault's key is not right.".to_string())?;
    let plain = cipher.decrypt(Nonce::from_slice(&sealed[..NONCE]), &sealed[NONCE..]).map_err(|_| "That seal is broken.".to_string())?;
    String::from_utf8(plain).map_err(|_| "That seal is broken.".to_string())
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(crate::key::SERVICE, ACCOUNT).map_err(|_| "The vault's key could not be reached.".to_string())
}

/// The vault's key from the member's Credential Manager, made the first time it is needed.
pub fn key() -> Result<[u8; 32], String> {
    let entry = entry()?;
    let engine = base64::engine::general_purpose::STANDARD;
    if let Ok(text) = entry.get_password() {
        let bytes = engine.decode(text.trim()).map_err(|_| "The vault's key is not right.".to_string())?;
        return bytes.try_into().map_err(|_| "The vault's key is not right.".to_string());
    }
    let mut fresh = [0u8; 32];
    getrandom::fill(&mut fresh).map_err(|_| "The computer could not make the vault's key.".to_string())?;
    entry.set_password(&engine.encode(fresh)).map_err(|_| "The vault's key could not be kept.".to_string())?;
    Ok(fresh)
}

/// Keeps one login (a second one for the same site and username replaces it).
pub fn save(c: &mut Conn, key: &[u8; 32], host: &str, username: &str, password: &str) -> Result<(), String> {
    let sealed = seal(key, password)?;
    c.exec_drop(
        "INSERT INTO browser_vault (host, username, secret) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE secret = VALUES(secret)",
        (host, username.chars().take(320).collect::<String>(), sealed),
    )
    .map_err(sql("keeping a login"))
}

/// One saved login as the page may see it: never the password.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Login {
    pub id: u64,
    pub host: String,
    pub username: String,
}

/// The hosts a login for `host` may be filled on: the host itself and each parent domain with a dot.
/// Pure.
pub fn hosts_for(host: &str) -> Vec<String> {
    let labels: Vec<&str> = host.split('.').collect();
    (0..labels.len().saturating_sub(1)).map(|i| labels[i..].join(".")).collect()
}

/// The logins saved for this host or a parent domain of it (usernames only).
pub fn logins_for(c: &mut Conn, host: &str) -> Result<Vec<Login>, String> {
    let mut out = Vec::new();
    for h in hosts_for(host) {
        let rows: Vec<(u64, String, String)> = c.exec("SELECT id, host, username FROM browser_vault WHERE host = ? ORDER BY username", (&h,)).map_err(sql("reading the logins"))?;
        out.extend(rows.into_iter().map(|(id, host, username)| Login { id, host, username }));
    }
    Ok(out)
}

/// Every saved login, never a password.
pub fn list(c: &mut Conn) -> Result<Vec<Login>, String> {
    let rows: Vec<(u64, String, String)> = c.query("SELECT id, host, username FROM browser_vault ORDER BY host, username").map_err(sql("reading the logins"))?;
    Ok(rows.into_iter().map(|(id, host, username)| Login { id, host, username }).collect())
}

/// A login opened for filling - only when the page's host may take it. (username, password)
pub fn fill_secret(c: &mut Conn, key: &[u8; 32], id: u64, page_host: &str) -> Result<(String, String), String> {
    let row: Option<(String, String, Vec<u8>)> = c.exec_first("SELECT host, username, secret FROM browser_vault WHERE id = ?", (id,)).map_err(sql("reading a login"))?;
    let (host, username, sealed) = row.ok_or("That login is gone.")?;
    if !hosts_for(page_host).contains(&host) {
        return Err("That login belongs to another site.".into());
    }
    Ok((username, open(key, &sealed)?))
}

pub fn forget(c: &mut Conn, id: u64) -> Result<(), String> {
    c.exec_drop("DELETE FROM browser_vault WHERE id = ?", (id,)).map_err(sql("removing a login"))
}

pub fn forget_all(c: &mut Conn) -> Result<(), String> {
    c.query_drop("DELETE FROM browser_vault").map_err(sql("removing the logins"))
}

// ---------- bookmarks ----------

/// One bookmark.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub folder: String,
}

fn url_hash(url: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(url.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Keeps a bookmark (http and https only); a known address keeps its first title. True when new.
pub fn add_bookmark(c: &mut Conn, title: &str, url: &str, folder: &str) -> Result<bool, String> {
    let ok = tauri::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) && url.len() <= 1000;
    if !ok {
        return Err("Only web addresses are kept as bookmarks.".into());
    }
    let title: String = title.trim().chars().take(200).collect();
    let folder: String = folder.trim().chars().take(300).collect();
    c.exec_drop("INSERT IGNORE INTO browser_bookmarks (title, url, url_hash, folder) VALUES (?, ?, ?, ?)", (&title, url, url_hash(url), &folder)).map_err(sql("keeping a bookmark"))?;
    Ok(c.affected_rows() > 0)
}

pub fn remove_bookmark(c: &mut Conn, url: &str) -> Result<(), String> {
    c.exec_drop("DELETE FROM browser_bookmarks WHERE url_hash = ?", (url_hash(url),)).map_err(sql("removing a bookmark"))
}

pub fn is_bookmarked(c: &mut Conn, url: &str) -> Result<bool, String> {
    let n: Option<u64> = c.exec_first("SELECT COUNT(*) FROM browser_bookmarks WHERE url_hash = ?", (url_hash(url),)).map_err(sql("reading a bookmark"))?;
    Ok(n.unwrap_or(0) > 0)
}

/// The newest bookmarks first.
pub fn bookmarks(c: &mut Conn, limit: u32) -> Result<Vec<Bookmark>, String> {
    let rows: Vec<(u64, String, String, String)> = c.exec("SELECT id, title, url, folder FROM browser_bookmarks ORDER BY id DESC LIMIT ?", (limit,)).map_err(sql("reading the bookmarks"))?;
    Ok(rows.into_iter().map(|(id, title, url, folder)| Bookmark { id, title, url, folder }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn a_seal_opens_with_its_key_only_and_never_after_a_change() {
        let sealed = seal(&KEY, "hunter2 é").unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("hunter2"), "the seal never holds the password plainly");
        assert_eq!(open(&KEY, &sealed).unwrap(), "hunter2 é");
        assert!(open(&[8; 32], &sealed).is_err(), "a wrong key opens nothing");
        let mut changed = sealed.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(open(&KEY, &changed).is_err(), "a changed seal opens nothing");
        assert!(open(&KEY, &[1, 2, 3]).is_err());
        assert_ne!(seal(&KEY, "same").unwrap(), seal(&KEY, "same").unwrap(), "every seal has its own nonce");
    }

    #[test]
    fn a_login_fills_on_its_site_and_parent_domains_only() {
        assert_eq!(hosts_for("accounts.google.com"), vec!["accounts.google.com", "google.com"]);
        assert_eq!(hosts_for("example.org"), vec!["example.org"]);
        assert!(hosts_for("localhost").is_empty());
    }

    #[test]
    #[ignore]
    fn live_database_logins_and_bookmarks() {
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Vault Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();

        save(&mut c, &KEY, "example.org", "ari", "s3cret").unwrap();
        save(&mut c, &KEY, "example.org", "ari", "n3wer").unwrap();
        let raw: Vec<u8> = c.query_first("SELECT secret FROM browser_vault").unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("n3wer"), "the database holds only the seal");
        let logins = logins_for(&mut c, "login.example.org").unwrap();
        assert_eq!(logins.len(), 1);
        assert_eq!(serde_json::to_string(&list(&mut c).unwrap()).unwrap().contains("n3wer"), false, "a listing never carries a password");
        assert_eq!(fill_secret(&mut c, &KEY, logins[0].id, "login.example.org").unwrap(), ("ari".into(), "n3wer".into()));
        assert!(fill_secret(&mut c, &KEY, logins[0].id, "evil.example").is_err(), "never filled on another site");
        forget(&mut c, logins[0].id).unwrap();
        assert!(list(&mut c).unwrap().is_empty());

        assert!(add_bookmark(&mut c, "Docs", "https://example.org/docs", "Work/Docs").unwrap());
        assert!(!add_bookmark(&mut c, "Again", "https://example.org/docs", "").unwrap(), "a known address is kept once");
        assert!(add_bookmark(&mut c, "x", "javascript:alert(1)", "").is_err());
        assert!(is_bookmarked(&mut c, "https://example.org/docs").unwrap());
        assert_eq!(bookmarks(&mut c, 12).unwrap()[0].folder, "Work/Docs");
        remove_bookmark(&mut c, "https://example.org/docs").unwrap();
        assert!(!is_bookmarked(&mut c, "https://example.org/docs").unwrap());
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }
}
