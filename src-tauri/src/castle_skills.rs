//! Castle's own skills, seeded into every member's memory database. Their text lives here, in
//! Castle's code - never as a file Castle ships - and reaches the member as rows in `skills` and
//! `skill_files`, served by `castle-memory skill show` like any skill. On disk the member's
//! memory folder holds only the short stub every database skill has (`mirror::stub_of`).
//!
//! A newer Castle replaces a skill's text only when it ships a higher version (the
//! `castle:built-in vN` mark in the text); the text it replaces goes to `skill_file_history`
//! first, so a member's own edit can always be brought back.

use mysql::prelude::Queryable;

/// The plugin Castle's own skills are kept under.
pub const PLUGIN: &str = "castle";

/// Castle's own skills: (name, version, the whole SKILL.md text).
pub const BUILT_IN: [(&str, u32, &str); 1] = [("shard", 1, SHARD)];

/// Shard: a code file grown past 1,000 lines becomes a thin master with focused children, and
/// only on the member's yes.
pub const SHARD: &str = r#"---
name: shard
description: Use when a code file you are editing has more than 1,000 lines, or after an edit pushed a file past 1,000 lines, or when your person says "split this file", "this file is too long", "shard this" or "break this file up". Proposes splitting the file into a thin master and focused children, and splits only on their yes.
---
<!-- castle:built-in v1 -->

# Shard

A file grown too long is hard to read, hard to review and easy to break. Shard turns one long
code file into a thin master that hands the work to a few focused children, without changing
what the program does.

## Size standards

| Lines in one code file | What to do |
|------------------------|------------|
| under 500 | healthy: nothing to do |
| 500 to 1,000 | watch it: mention it once if it keeps growing |
| over 1,000 | propose a split (this skill) |

Never split these, whatever their length: generated files, lock files, minified or bundled
files, vendored code (other people's libraries copied in), and pure data (JSON, CSV, SQL dumps,
translations). Pure styling (CSS) may be mentioned, with the honest note that splitting it
helps less.

## The master and its children

```
LongFile (1,000+ lines)
  -> LongFile         about 100-150 lines: the same public names, each handing the work on
     -> ChildA        one responsibility (150-500 lines)
     -> ChildB        one responsibility (150-500 lines)
     -> Shared        helpers the children share, only if they need them
```

- Group the code by what it does (reading, saving, checking, drawing...), never by size alone.
- The master keeps every public name, route, command and export exactly as it was, so nothing
  that uses the file changes. It only delegates; no logic stays in it.
- The children live beside the master, in its own folder, module or package, named for what
  they do.
- In each language, use its own way: Rust modules (`mod child;` and `pub use`), TypeScript /
  JavaScript modules re-exported from the master, Vue components or composables, PHP traits or
  classes, Java classes in the same package, Python modules imported by the master.

## The flow

1. **Measure** the file (count its lines) and say the number.
2. **Group** its functions, types and parts by responsibility.
3. **Propose** the split as a plan: each child's file name, what moves into it, and its
   estimated lines; the master's estimated lines; and that nothing outside changes.
4. **Wait** for your person's yes. They may change the grouping first.
5. **Split** exactly as agreed. Run the project's tests and build before and after; both must
   pass the same way.
6. **Commit** the split on its own, the way this project's own commits are made, and say in one
   line what moved where.

## Rules <!-- castle:keep -->

- Never split a file without your person's yes; propose it as a plan first.
- Zero change in behaviour: the same public names, routes, commands and results.
- One file per proposal; finish it before proposing the next.
- Run the project's tests and build before and after the split; a split that fails them is put back.
- Never split generated, locked, minified, vendored or pure-data files.
"#;

/// The version a skill's text carries in its `castle:built-in vN` mark (0 when it carries none).
pub fn version_of(text: &str) -> u32 {
    text.split("<!-- castle:built-in v").nth(1).and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// What seeding should do with one built-in skill, given the text the database holds now.
#[derive(Debug, PartialEq)]
pub enum Seed {
    Insert,
    /// a higher version: the old text goes to history first
    Replace,
    Keep,
}

pub fn decide(kept: Option<&str>, shipped_version: u32) -> Seed {
    match kept {
        None => Seed::Insert,
        Some(text) if version_of(text) < shipped_version => Seed::Replace,
        Some(_) => Seed::Keep,
    }
}

/// Puts Castle's own skills into the member's database: a missing one is added (served from the
/// database, as every skill is since 026), a lower version is replaced with its old text kept in
/// history, and the same or a higher version is left exactly as it is.
pub fn seed(conn: &mut impl Queryable) -> Result<u32, String> {
    let mut changed = 0;
    for (name, version, body) in BUILT_IN {
        let kept: Option<String> = conn
            .exec_first("SELECT body FROM skill_files WHERE plugin = ? AND name = ? AND path = 'SKILL.md'", (PLUGIN, name))
            .map_err(|_| "Castle's own skills could not be read.".to_string())?;
        match decide(kept.as_deref(), version) {
            Seed::Keep => continue,
            Seed::Replace => {
                conn.exec_drop("INSERT INTO skill_file_history (plugin, name, path, body) VALUES (?, ?, 'SKILL.md', ?)", (PLUGIN, name, kept.unwrap_or_default()))
                    .map_err(|_| "Castle's own skills could not keep their older text.".to_string())?;
            }
            Seed::Insert => {}
        }
        conn.exec_drop(
            "INSERT INTO skill_files (plugin, name, path, body, updated_at) VALUES (?, ?, 'SKILL.md', ?, NOW()) ON DUPLICATE KEY UPDATE body = VALUES(body), updated_at = NOW()",
            (PLUGIN, name, body),
        )
        .map_err(|_| "Castle's own skills could not be saved.".to_string())?;
        conn.exec_drop(
            "INSERT INTO skills (plugin, name, description, stubbed) VALUES (?, ?, ?, 1) ON DUPLICATE KEY UPDATE description = VALUES(description)",
            (PLUGIN, name, crate::mirror::skill_description(body)),
        )
        .map_err(|_| "Castle's own skills could not be listed.".to_string())?;
        changed += 1;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_skill_is_a_whole_skill_with_its_mark_and_its_kept_rules() {
        for (name, version, body) in BUILT_IN {
            assert!(crate::mirror::front_matter(body).is_some(), "{name}: front matter");
            assert!(body.contains(&format!("name: {name}\n")), "{name}: its name");
            assert!(crate::mirror::skill_description(body).is_some(), "{name}: a description");
            assert_eq!(version_of(body), version, "{name}: the mark carries its version");
            let rules = crate::mirror::kept_rules(body).unwrap();
            assert!(rules.contains("Never split a file without your person's yes"), "{name}: the stub keeps its rules");
            assert!(crate::mirror::folder_name_ok(name));
        }
    }

    #[test]
    fn a_higher_version_replaces_and_the_same_one_never_overwrites_an_edit() {
        assert_eq!(decide(None, 1), Seed::Insert);
        assert_eq!(decide(Some("old text <!-- castle:built-in v1 -->"), 2), Seed::Replace);
        assert_eq!(decide(Some("my own edit <!-- castle:built-in v2 -->"), 2), Seed::Keep);
        assert_eq!(decide(Some("a newer one <!-- castle:built-in v3 -->"), 2), Seed::Keep);
        assert_eq!(version_of("no mark"), 0);
        assert_eq!(version_of("<!-- castle:built-in v12 -->"), 12);
    }

    #[test]
    #[ignore]
    fn live_database_shard_is_seeded_once_served_whole_and_only_its_stub_is_on_disk() {
        let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
            // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
            let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
            let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
            format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
        });
        let mut parts = config.splitn(3, ':');
        let port = parts.next().unwrap().parse().unwrap();
        let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
        let access = crate::memdb::provision(port, &user, &pass, &format!("Skills Test {}", std::process::id())).unwrap();
        let mut c = crate::memdb::open(&access).unwrap();
        crate::memdb::migrate(&mut c).unwrap();
        let memory = std::env::temp_dir().join(format!("castle-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&memory);
        std::fs::create_dir_all(&memory).unwrap();

        // the first sync seeds Shard and writes only its stub
        crate::mirror::sync_skills(&mut c, &memory).unwrap();
        let on_disk = std::fs::read_to_string(memory.join("plugins/castle/skills/shard/SKILL.md")).unwrap();
        assert!(crate::mirror::is_stub(&on_disk) && !on_disk.contains("## The flow"), "the instructions are never in the file");
        let shown = crate::door::run(&mut c, &crate::door::parse(&["skill", "show", "--plugin", "castle", "--name", "shard"].map(String::from)).unwrap(), &memory).unwrap();
        assert!(shown.contains("## The flow") && shown.contains("castle:built-in v1"), "the database serves the whole skill");
        let stubbed: Option<bool> = c.query_first("SELECT stubbed FROM skills WHERE plugin = 'castle' AND name = 'shard'").unwrap();
        assert_eq!(stubbed, Some(true));

        // again: nothing changes
        assert_eq!(seed(&mut c).unwrap(), 0);
        assert_eq!(crate::mirror::sync_skills(&mut c, &memory).unwrap().to_disk, 0);

        // an older version is replaced, its text kept in history
        c.exec_drop("UPDATE skill_files SET body = ? WHERE plugin = 'castle' AND name = 'shard' AND path = 'SKILL.md'", ("---\nname: shard\ndescription: old\n---\n<!-- castle:built-in v0 -->\nold",)).unwrap();
        assert_eq!(seed(&mut c).unwrap(), 1);
        let kept: Option<String> = c.query_first("SELECT body FROM skill_file_history WHERE plugin = 'castle' AND name = 'shard'").unwrap();
        assert!(kept.unwrap().ends_with("old"));

        let _ = std::fs::remove_dir_all(&memory);
        c.query_drop(format!("DROP DATABASE `{}`", access.name)).unwrap();
    }

    #[test]
    fn the_stub_of_shard_keeps_its_rules_and_the_load_line() {
        let stub = crate::mirror::stub_of(PLUGIN, "shard", SHARD, &[]);
        assert!(crate::mirror::is_stub(&stub));
        assert!(stub.contains("castle-memory skill show --plugin \"castle\" --name \"shard\""));
        assert!(stub.contains("Never split a file without your person's yes"));
        assert!(!stub.contains("## The flow"), "the instructions stay in the database");
    }
}
