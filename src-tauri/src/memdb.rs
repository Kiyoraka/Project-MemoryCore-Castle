//! The member's memory database: its name, its own user, its migrations, and the connection
//! details written into the memory folder's .env so the companion's door can find it.
//!
//! ```text
//!  <ainame>_memorycoredb          one database per companion ("lyra_memorycoredb")
//!  castle_<ainame>@localhost      its own user, only this database, a random password
//!  <memory>/.env                  MEMORY_DB_HOST / PORT / NAME / USER / PASSWORD
//! ```

use mysql::prelude::Queryable;
use mysql::{Conn, Opts, OptsBuilder};
use std::path::Path;
use std::time::Duration;

/// Every member database ends with this ("lyra_memorycoredb").
pub const NAME_SUFFIX: &str = "_memorycoredb";
pub const HOST: &str = "127.0.0.1";
/// The lines Castle keeps in the memory folder's .env.
pub const ENV_KEYS: [&str; 5] = ["MEMORY_DB_HOST", "MEMORY_DB_PORT", "MEMORY_DB_NAME", "MEMORY_DB_USER", "MEMORY_DB_PASSWORD"];

/// The migrations, in order. A released file is never edited; a change is a new entry.
pub const MIGRATIONS: [(&str, &str); 8] = [
    ("001_core", include_str!("../resources/schema/001_core.sql")),
    ("002_imported_files", include_str!("../resources/schema/002_imported_files.sql")),
    ("003_memory_skills", include_str!("../resources/schema/003_memory_skills.sql")),
    ("004_skill_stub", include_str!("../resources/schema/004_skill_stub.sql")),
    ("005_memory_title", include_str!("../resources/schema/005_memory_title.sql")),
    ("006_diary_history", include_str!("../resources/schema/006_diary_history.sql")),
    ("007_project_plans", include_str!("../resources/schema/007_project_plans.sql")),
    ("008_companion_traits", include_str!("../resources/schema/008_companion_traits.sql")),
];

/// How to reach one database.
#[derive(Debug, Clone, PartialEq)]
pub struct Access {
    pub port: u16,
    pub name: String,
    pub user: String,
    pub password: String,
}

// ---------- pure ----------

/// The companion's name as a database word: lowercase letters, digits and single underscores.
/// "Lyra" -> "lyra", "Ai-chan 2" -> "ai_chan_2", "" or "アリス" -> "companion".
pub fn slug(companion: &str) -> String {
    let mut out = String::new();
    for c in companion.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out: String = out.trim_matches('_').chars().take(40).collect();
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "companion".into() } else { out }
}

pub fn database_name(companion: &str) -> String {
    format!("{}{NAME_SUFFIX}", slug(companion))
}

pub fn user_name(companion: &str) -> String {
    format!("castle_{}", slug(companion))
}

/// A migration file as single statements (comment lines dropped; no statement holds a ';').
pub fn statements(sql: &str) -> Vec<String> {
    let body: String = sql.lines().filter(|l| !l.trim_start().starts_with("--")).collect::<Vec<_>>().join("\n");
    body.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The .env text with Castle's five lines set and every other line kept as it was.
pub fn env_with(text: &str, access: &Access) -> String {
    let values = [HOST.to_string(), access.port.to_string(), access.name.clone(), access.user.clone(), access.password.clone()];
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !ENV_KEYS.iter().any(|k| l.trim_start().starts_with(&format!("{k}="))))
        .map(str::to_string)
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    for (key, value) in ENV_KEYS.iter().zip(values) {
        lines.push(format!("{key}={value}"));
    }
    lines.join("\n") + "\n"
}

/// The .env text with Castle's five lines taken out and every other line kept.
pub fn env_without(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().filter(|l| !ENV_KEYS.iter().any(|k| l.trim_start().starts_with(&format!("{k}=")))).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" }
}

/// Takes the connection details out of <memory>/.env (the memory is back in its files).
pub fn clear_env(memory: &Path) -> Result<(), String> {
    let path = memory.join(".env");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(()) };
    std::fs::write(&path, env_without(&text)).map_err(|_| "The .env could not be written.".to_string())
}

/// The connection details a .env holds, when all five are there.
pub fn access_from_env(text: &str) -> Option<Access> {
    let get = |key: &str| {
        text.lines().find_map(|l| l.trim().strip_prefix(&format!("{key}=")).map(|v| v.trim().trim_matches('"').to_string()))
    };
    Some(Access {
        port: get("MEMORY_DB_PORT")?.parse().ok()?,
        name: get("MEMORY_DB_NAME").filter(|v| !v.is_empty())?,
        user: get("MEMORY_DB_USER").filter(|v| !v.is_empty())?,
        password: get("MEMORY_DB_PASSWORD")?,
    })
}

/// 32 hex characters from the system's random source: safe inside a quoted SQL string.
pub fn random_password() -> Result<String, String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|_| "The computer could not make a database password.".to_string())?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

// ---------- connections ----------

fn connect(port: u16, user: &str, password: &str, database: Option<&str>) -> Result<Conn, String> {
    let opts = OptsBuilder::new()
        .ip_or_hostname(Some(HOST))
        .tcp_port(port)
        .user(Some(user))
        .pass(Some(password))
        .db_name(database)
        .prefer_socket(false)
        .tcp_connect_timeout(Some(Duration::from_secs(5)));
    Conn::new(Opts::from(opts)).map_err(|e| match e {
        mysql::Error::MySqlError(ref server) if server.code == 1045 => "The database did not accept that user and password.".to_string(),
        _ => "The database could not be reached. Is it running?".to_string(),
    })
}

/// Opens the member's database with its own user.
pub fn open(access: &Access) -> Result<Conn, String> {
    connect(access.port, &access.user, &access.password, Some(&access.name))
}

/// Opens the memory folder's database from its .env.
pub fn open_for(memory: &Path) -> Result<Conn, String> {
    let text = std::fs::read_to_string(memory.join(".env")).map_err(|_| "This memory has no database yet.".to_string())?;
    open(&access_from_env(&text).ok_or("This memory has no database yet.")?)
}

fn sql_err(what: &str) -> impl Fn(mysql::Error) -> String + '_ {
    move |_| format!("{what} did not work.")
}

// ---------- provision ----------

/// Creates the companion's database and its own user (as the server's admin), then applies the
/// migrations as that user. Safe to run again: the password is renewed and nothing is dropped.
pub fn provision(port: u16, admin_user: &str, admin_password: &str, companion: &str) -> Result<Access, String> {
    let access = Access { port, name: database_name(companion), user: user_name(companion), password: random_password()? };
    let mut admin = connect(port, admin_user, admin_password, None)?;
    // name and user are [a-z0-9_] only and the password is hex: nothing here needs escaping
    admin
        .query_drop(format!("CREATE DATABASE IF NOT EXISTS `{}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci", access.name))
        .map_err(sql_err("Creating the database"))?;
    for host in ["localhost", HOST] {
        admin
            .query_drop(format!("CREATE USER IF NOT EXISTS '{}'@'{host}' IDENTIFIED BY '{}'", access.user, access.password))
            .map_err(sql_err("Creating the database user"))?;
        admin
            .query_drop(format!("ALTER USER '{}'@'{host}' IDENTIFIED BY '{}'", access.user, access.password))
            .map_err(sql_err("Setting the database password"))?;
        admin
            .query_drop(format!("GRANT ALL PRIVILEGES ON `{}`.* TO '{}'@'{host}'", access.name, access.user))
            .map_err(sql_err("Giving the user its database"))?;
    }
    drop(admin);
    migrate(&mut open(&access)?)?;
    Ok(access)
}

/// Applies every migration not yet in schema_migrations; returns the names it applied.
pub fn migrate(conn: &mut Conn) -> Result<Vec<String>, String> {
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS schema_migrations (name VARCHAR(64) NOT NULL PRIMARY KEY, applied_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP) \
         ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
    )
    .map_err(sql_err("Preparing the database"))?;
    let done: Vec<String> = conn.query("SELECT name FROM schema_migrations").map_err(sql_err("Reading the database's version"))?;
    let mut applied = Vec::new();
    for (name, sql) in MIGRATIONS {
        if done.iter().any(|d| d == name) {
            continue;
        }
        for statement in statements(sql) {
            conn.query_drop(statement).map_err(|_| format!("The database update {name} did not finish."))?;
        }
        conn.exec_drop("INSERT INTO schema_migrations (name) VALUES (?)", (name,)).map_err(sql_err("Recording the database's version"))?;
        applied.push(name.to_string());
    }
    Ok(applied)
}

/// Writes the connection details into <memory>/.env (other lines kept) and makes sure the
/// memory backup never carries that file.
pub fn write_env(memory: &Path, access: &Access) -> Result<(), String> {
    let path = memory.join(".env");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, env_with(&text, access)).map_err(|_| "The database details could not be written to .env.".to_string())?;
    crate::backup::ensure_safety_ignore(memory).map_err(|_| "The backup's safety list could not be written.".to_string())
}

/// Gives Castle's own engine's root a password when it still has none; returns the password
/// root has afterwards. (A fresh portable MariaDB lets anyone on this PC in as root.)
pub fn secure_root(port: u16, kept: Option<String>) -> Result<String, String> {
    if let Some(password) = kept {
        if connect(port, "root", &password, None).is_ok() {
            return Ok(password);
        }
    }
    let mut root = connect(port, "root", "", None)?;
    let password = random_password()?;
    for host in ["localhost", HOST, "::1"] {
        // a host row that does not exist is fine
        let _ = root.query_drop(format!("ALTER USER 'root'@'{host}' IDENTIFIED BY '{password}'"));
    }
    drop(root);
    connect(port, "root", &password, None).map(|_| password).map_err(|_| "The database's own password could not be set.".to_string())
}

/// Asks the server to shut down over SQL (used when root has a password).
pub fn shutdown(port: u16, root_password: &str) -> Result<(), String> {
    let mut root = connect(port, "root", root_password, None)?;
    // the server closes the connection while answering: an error here is the success
    let _ = root.query_drop("SHUTDOWN");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_companion_name_becomes_a_database_word() {
        assert_eq!(slug("Lyra"), "lyra");
        assert_eq!(slug("  Ai-chan 2 "), "ai_chan_2");
        assert_eq!(slug("アリス"), "companion");
        assert_eq!(slug("a'; DROP TABLE x; --"), "a_drop_table_x");
        assert_eq!(slug(&"x".repeat(60)).len(), 40);
        assert_eq!(database_name("Lyra"), "lyra_memorycoredb");
        assert_eq!(user_name("Lyra"), "castle_lyra");
    }

    #[test]
    fn a_migration_file_is_read_as_statements() {
        let found = statements("-- a note\nCREATE TABLE a (\n  id INT\n);\n\n-- more\nCREATE TABLE b (id INT);\n");
        assert_eq!(found, vec!["CREATE TABLE a (\n  id INT\n)", "CREATE TABLE b (id INT)"]);
        let core = statements(MIGRATIONS[0].1);
        assert_eq!(core.len(), 5);
        assert!(core.iter().all(|s| s.starts_with("CREATE TABLE ") && s.contains("utf8mb4")));
        assert_eq!(statements(MIGRATIONS[3].1), vec!["ALTER TABLE skills ADD COLUMN stubbed TINYINT(1) NOT NULL DEFAULT 0"]);
    }

    #[test]
    fn the_env_gains_castles_lines_and_keeps_the_members_own() {
        let access = Access { port: 3307, name: "lyra_memorycoredb".into(), user: "castle_lyra".into(), password: "abc123".into() };
        let text = env_with("OPENAI_API_KEY=sk-1\nMEMORY_DB_PORT=1\n# mine\n\n", &access);
        assert_eq!(
            text,
            "OPENAI_API_KEY=sk-1\n# mine\n\nMEMORY_DB_HOST=127.0.0.1\nMEMORY_DB_PORT=3307\nMEMORY_DB_NAME=lyra_memorycoredb\nMEMORY_DB_USER=castle_lyra\nMEMORY_DB_PASSWORD=abc123\n"
        );
        assert_eq!(access_from_env(&text), Some(access.clone()));
        // written twice, still once
        assert_eq!(env_with(&text, &access), text);
        assert_eq!(access_from_env("OPENAI_API_KEY=sk-1\n"), None);
        assert_eq!(env_with("", &access).lines().count(), 5);
        // and out again: the member's own lines stay, Castle's five go
        assert_eq!(env_without(&text), "OPENAI_API_KEY=sk-1\n# mine\n");
        assert_eq!(env_without(&env_with("", &access)), "");
        assert_eq!(access_from_env(&env_without(&text)), None);
    }

    #[test]
    fn a_password_is_32_hex_characters_and_new_each_time() {
        let a = random_password().unwrap();
        assert!(a.len() == 32 && a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_password().unwrap());
    }

    /// On a throwaway engine only (CASTLE_TEST_OWN_PORT): root gains a password, the old empty
    /// one stops working, the kept one is reused, and the server shuts down over SQL.
    #[test]
    #[ignore]
    fn live_root_is_locked_then_the_engine_shuts_down() {
        let port: u16 = std::env::var("CASTLE_TEST_OWN_PORT").expect("set CASTLE_TEST_OWN_PORT").parse().unwrap();
        let password = secure_root(port, None).unwrap();
        assert!(connect(port, "root", "", None).is_err());
        assert_eq!(secure_root(port, Some(password.clone())).unwrap(), password);
        shutdown(port, &password).unwrap();
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(15) && connect(port, "root", &password, None).is_ok() {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(connect(port, "root", &password, None).is_err());
    }

    /// Against a real server, on demand: CASTLE_TEST_DB="<port>:<admin user>:<admin password>"
    /// (`cargo test --lib memdb::tests::live -- --ignored`). Uses and drops its own database.
    #[test]
    #[ignore]
    fn live_provision_migrate_and_reconnect() {
        let spec = std::env::var("CASTLE_TEST_DB").expect("set CASTLE_TEST_DB");
        let mut parts = spec.splitn(3, ':');
        let port: u16 = parts.next().unwrap().parse().unwrap();
        let (admin, admin_pw) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let companion = format!("Castle Test {}", std::process::id());

        let access = provision(port, &admin, &admin_pw, &companion).unwrap();
        assert!(access.name.starts_with("castle_test_") && access.name.ends_with(NAME_SUFFIX));
        let mut conn = open(&access).unwrap();
        let tables: Vec<String> = conn.query("SHOW TABLES").unwrap();
        for table in ["diary_entries", "reminders", "projects", "link_terminals", "link_notes", "imported_files", "schema_migrations"] {
            assert!(tables.iter().any(|t| t == table), "missing {table}");
        }
        // a second run applies nothing and the new password works
        assert!(migrate(&mut conn).unwrap().is_empty());
        let again = provision(port, &admin, &admin_pw, &companion).unwrap();
        assert_ne!(again.password, access.password);
        assert!(open(&access).is_err());
        let mut conn = open(&again).unwrap();
        conn.exec_drop("INSERT INTO diary_entries (entry_date, title, body) VALUES (?, ?, ?)", ("2026-11-10", "アリス 💜", "unicode holds")).unwrap();
        let title: Option<String> = conn.query_first("SELECT title FROM diary_entries").unwrap();
        assert_eq!(title.as_deref(), Some("アリス 💜"));

        // the .env round trip
        let folder = std::env::temp_dir().join(format!("castle-memdb-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        write_env(&folder, &again).unwrap();
        assert!(open_for(&folder).is_ok());
        assert!(std::fs::read_to_string(folder.join(".gitignore")).unwrap().contains(".env"));
        std::fs::remove_dir_all(&folder).unwrap();

        let mut root = connect(port, &admin, &admin_pw, None).unwrap();
        root.query_drop(format!("DROP DATABASE `{}`", again.name)).unwrap();
        for host in ["localhost", HOST] {
            root.query_drop(format!("DROP USER '{}'@'{host}'", again.user)).unwrap();
        }
    }
}
