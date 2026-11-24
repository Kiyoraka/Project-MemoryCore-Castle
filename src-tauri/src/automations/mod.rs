//! Phase 1: schedules, isolated companion briefings, history and HOME.
mod actions;
mod http;
mod chains;
pub mod companion;
mod castle_actions;
mod commands;
mod deliver;
pub mod events;
pub mod hook;
mod runner;
pub mod health;
mod schedule;
mod store;
pub mod triggers;
pub mod watch;
use crate::settings::SettingsStore;
use mysql::{prelude::Queryable, Conn};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
#[derive(Default)]
pub struct Running(pub Mutex<BTreeSet<u64>>, pub Mutex<bool>);
/// A short-lived, time-limited connection for other rooms (the calendar). It never migrates.
pub fn room_connection(store: &SettingsStore) -> Result<Conn, String> {
    let folder = store.get().memory_folder.ok_or("Choose a MemoryCore folder first.")?;
    castle_actions::bounded_connection(std::path::Path::new(&folder), std::time::Duration::from_secs(5))
}
pub fn connection(store: &SettingsStore) -> Result<Conn, String> {
    let folder = store
        .get()
        .memory_folder
        .ok_or("Choose a MemoryCore folder first.")?;
    let mut c = castle_actions::bounded_connection(std::path::Path::new(&folder),std::time::Duration::from_secs(5))?;
    // Database startup owns migrations. Do not race it from a background runner or room load.
    let ready: Option<String> = c
        .exec_first(
            "SELECT name FROM schema_migrations WHERE name=?",
            ("020_automation_companion",),
        )
        .map_err(|_| "The memory database is preparing. Try again in a moment.".to_string())?;
    if ready.is_none() {
        return Err("Automations are preparing. Wait for the database, then try again.".into());
    }
    Ok(c)
}
pub fn allowed(s: &crate::settings::Settings) -> bool {
    s.room_on("automations") && !s.automations_paused
}
pub fn validate(j: &Value) -> Result<(), String> {
    let name = j["name"].as_str().unwrap_or("");
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err("Give the job a name, at most 80 characters.".into());
    }
    if !j["note"].is_null() && j["note"].as_str().is_none_or(|s| s.len() > 8000) {
        return Err("Keep the note under 8,000 bytes.".into());
    }
    let t = &j["trigger"];
    let trigger_kind = j["triggerKind"].as_str().unwrap_or("schedule");
    if trigger_kind == "message" {
        triggers::spec(t)?;
    } else if trigger_kind == "file" {
        watch::spec(t)?;
    } else if trigger_kind == "webhook" {
        hook::spec(t)?;
    } else if trigger_kind == "event" {
        let name = events::spec(t)?;
        events::conflict(&name, &j["action"])?;
    } else if trigger_kind == "schedule" {
        let fields = ["cron", "every_minutes", "at"]
            .iter()
            .filter(|k| !t[**k].is_null())
            .count();
        if let Some(at) = t["at"].as_str() {
            if chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M").is_err() {
                return Err("Choose a valid local date and time.".into());
            }
        }
        if fields != 1 {
            return Err("Choose one schedule.".into());
        }
        if let Some(expr) = t["cron"].as_str() {
            if !schedule::valid(expr) {
                return Err("Use five cron fields: minute, hour, day, month, weekday.".into());
            }
        }
        if !t["at"].is_string() && schedule::next(t, chrono::Local::now()).is_none() {
            return Err("Choose a valid future date, interval or cron schedule.".into());
        }
    } else {
        return Err("Choose a schedule, message, file, web request or event trigger.".into());
    }
    if j["action"]["kind"] == "castle" {
        castle_actions::validate(
            &castle_actions::spec(&j["action"])?,
            j["triggerKind"].as_str().unwrap_or("schedule"),
        )?;
        if j["policy"]["retries"].as_u64().unwrap_or(0) > 0 {
            return Err(
                "Castle actions do not retry automatically, to avoid repeating changes.".into(),
            );
        }
    } else if j["action"]["kind"] == "http" {
        http::spec(&j["action"])?;
        if j["policy"]["retries"].as_u64().unwrap_or(0)>0 { return Err("HTTP actions do not retry automatically.".into()); }
    } else if j["action"]["kind"] == "command" {
        commands::spec(&j["action"])?;
    } else {
        if !j["action"]["kind"].is_null() && j["action"]["kind"] != "companion" {
            return Err("Choose a briefing or command action.".into());
        }
        if j["action"]["mode"].as_str() != Some("think") {
            return Err("Briefing jobs use isolated think mode.".into());
        }
        if !j["action"]["cli"].is_null()
            && !j["action"]["cli"].as_str().is_some_and(|id| id.is_empty() || crate::settings::Cli::from_id(id).is_some())
        {
            return Err("Choose a supported provider.".into());
        }
        if let Some(m) = j["action"]["model"].as_str().filter(|m| !m.is_empty()) {
            if crate::oneshot::clean_model(m).is_none() {
                return Err("That model name is invalid.".into());
            }
        }
        let prompt = j["action"]["prompt"].as_str().unwrap_or("");
        if prompt.trim().is_empty() || prompt.len() > 32000 {
            return Err("Write a prompt of at most 32,000 bytes.".into());
        }
    }
    let policy = &j["policy"];
    for (k, lo, hi, default) in [
        ("timeout_s", 1, 3600, 120),
        ("max_output_kb", 1, 64, 64),
        ("retries", 0, 3, 0),
        ("rate_per_hour", 1, 60, 6),
    ] {
        if !policy[k].is_null() && !policy[k].is_u64() {
            return Err(format!("{k} must be a whole number."));
        }
        let n = policy[k].as_u64().unwrap_or(default);
        if !(lo..=hi).contains(&n) {
            return Err(format!("{k} must be between {lo} and {hi}."));
        }
    }
    if !matches!(
        policy["missed"].as_str().unwrap_or("skip"),
        "skip" | "run_once"
    ) {
        return Err("Choose skip or run once for missed runs.".into());
    }
    chains::validate_policy(j)?;
    validate_deliveries(&j["delivery"])?;
    if !j["policy"]["on_failure_delivery"].is_null() {
        validate_deliveries(&j["policy"]["on_failure_delivery"])?;
    }
    Ok(())
}
#[tauri::command]
pub async fn automation_list(app: AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move||{let store=app.state::<SettingsStore>();let s=store.get();let mut c=connection(&store)?;Ok(json!({"jobs":store::list(&mut c)?,"messageProblem":triggers::problem(),"watchProblem":watch::problem(),"hookProblem":hook::problem(),"eventProblem":events::problem(),"hookPort":hook::PORT,"hooks":store::list(&mut c)?.iter().filter(|j|j["triggerKind"]=="webhook").filter_map(|j|j["id"].as_u64()).map(|id|(id.to_string(),json!(crate::key::hook_secret_load(id).is_some()))).collect::<serde_json::Map<String,Value>>(),"paused":s.automations_paused,"on":s.room_on("automations"),"notices":store::home(&mut c)?,"programs":s.automations_commands.clone(),"projects": (c.query::<String,_>("SELECT name FROM projects WHERE shelf='active' ORDER BY name").map_err(|_|"Projects could not be loaded.".to_string())?),"phone": (["telegram","discord","whatsapp"].iter().map(|p|json!({"kind":p,"ready":crate::bridges::notification_ready(&s,p)})).collect::<Vec<_>>())}))}).await.map_err(|_|"Jobs could not be loaded.".to_string())?
}
#[tauri::command]
pub async fn automation_save(app: AppHandle, job: Value) -> Result<u64, String> {
    validate(&job)?;
    if job["triggerKind"].as_str().unwrap_or("schedule") == "schedule"
        && job["enabled"].as_bool() == Some(true)
        && schedule::next(&job["trigger"], chrono::Local::now()).is_none()
    {
        return Err("Choose a future schedule before switching this job on.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let settings = app.state::<SettingsStore>();
        validate_command(&job, &settings)?;
        let mut c = connection(&settings)?;
        if job["action"]["kind"] == "castle" {
            let memory = settings
                .get()
                .memory_folder
                .ok_or("Choose a MemoryCore folder.")?;
            castle_actions::preview(
                &castle_actions::spec(&job["action"])?,
                &mut c,
                std::path::Path::new(&memory),
            )?;
        }
        for (kind, limit, what) in [("file", watch::MAX_JOBS, "file"), ("webhook", hook::MAX_JOBS, "web request"), ("event", events::MAX_JOBS, "event")] {
            if job["triggerKind"] == kind {
                let count: Option<u64> = c.exec_first("SELECT COUNT(*) FROM automation_jobs WHERE trigger_kind=? AND state='active' AND id != ?", (kind, job["id"].as_u64().unwrap_or(0))).map_err(|_| "Jobs could not be counted.")?;
                if count.unwrap_or(0) >= limit { return Err(format!("Keep at most {limit} active {what} jobs.")); }
            }
        }
        if job["triggerKind"] == "file" {
            watch::check_folder(&watch::spec(&job["trigger"])?)?;
        }
        if job["triggerKind"] == "webhook" && job["enabled"].as_bool() == Some(true) {
            let id = job["id"].as_u64().unwrap_or(0);
            if id == 0 || crate::key::hook_secret_load(id).is_none() {
                return Err("Create the link first, then switch this job on.".into());
            }
        }
        if job["triggerKind"] == "message" {
            let count: Option<u64> = c.exec_first("SELECT COUNT(*) FROM automation_jobs WHERE trigger_kind='message' AND state='active' AND id != ?", (job["id"].as_u64().unwrap_or(0),)).map_err(|_| "Message jobs could not be counted.")?;
            if count.unwrap_or(0) >= 64 { return Err("Keep at most 64 active message jobs.".into()); }
        }
        if job["approveProposal"]==true { companion::approve(&mut c,&job) } else { store::save(&mut c, &job) }
    })
    .await
    .map_err(|_| "Job could not be saved.".to_string())?
}
#[tauri::command]
pub async fn automation_history(app: AppHandle, id: u64) -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        store::runs(&mut connection(&app.state::<SettingsStore>())?, id)
    })
    .await
    .map_err(|_| "History could not be loaded.".to_string())?
}
#[tauri::command]
pub async fn automation_archive(app: AppHandle, id: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = connection(&app.state::<SettingsStore>())?;
        c.exec_drop(
            "UPDATE automation_jobs SET state='archived',enabled=0,next_run_at=NULL WHERE id=?",
            (id,),
        )
        .map_err(|_| "Job could not be archived.".to_string())?;
        // calendar events that started this job no longer do
        crate::calendar::unlink_job(&mut c, id);
        // an archived job's web link stops working for good
        let _ = crate::key::hook_secret_forget(id);
        Ok(())
    })
    .await
    .map_err(|_| "Job could not be archived.".to_string())?
}
#[tauri::command]
pub async fn automation_run(app: AppHandle, id: u64, test: bool) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        runner::launch(&app, id, if test { "test" } else { "manual" })
    })
    .await
    .map_err(|_| "Job could not be started.".to_string())?
}
#[tauri::command]
pub fn automation_pause(app: AppHandle, paused: bool) -> Result<(), String> {
    let store = app.state::<SettingsStore>();
    let mut s = store.get();
    if s.automations_paused==paused{return Ok(());}
    s.automations_paused = paused;
    crate::settings::save(&store.path, &s).map_err(|_| "Pause could not be saved.".to_string())?;
    *store.current.lock().unwrap() = s;
    health::interrupt(&app);
    let _ = app.emit("castle://automations", json!({"paused":paused}));
    Ok(())
}
/// Starts the schedule runner and, from phase 5, the folder watcher and the web-request listener.
pub fn start(app: &AppHandle) {
    runner::start(app);
    watch::start(app);
    hook::start(app);
    events::start(app);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_and_pause_are_gates() {
        let mut s = crate::settings::Settings::default();
        assert!(!allowed(&s));
        s.rooms.insert("automations".into(), true);
        assert!(allowed(&s));
        s.automations_paused = true;
        assert!(!allowed(&s));
    }
    #[test]
    fn unsupported_actions_and_deliveries_are_refused() {
        let mut j = json!({"name":"Briefing","trigger":{"every_minutes":30},"action":{"mode":"think","prompt":"Hi"},"policy":{},"delivery":[{"kind":"home"}]});
        assert!(validate(&j).is_ok());
        j["action"]["mode"] = json!("session");
        assert!(validate(&j).is_err());
        j["action"]["mode"] = json!("think");
        j["delivery"] = json!([{"kind":"other"}]);
        assert!(validate(&j).is_err());
    }
}

pub fn validate_command(job: &Value, store: &SettingsStore) -> Result<(), String> {
    if job["action"]["kind"] == "command" {
        commands::validate(
            &commands::spec(&job["action"])?,
            &store.get().automations_commands,
        )?;
    }
    let encoded = job.to_string();
    if actions::known_secrets(store)
        .iter()
        .any(|s| !s.is_empty() && encoded.contains(s))
    {
        return Err("A job definition contains a stored secret. Keep credentials in protected script configuration.".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn automation_program_pick(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a program jobs may run")
            .blocking_pick_file()
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}
#[tauri::command]
pub async fn automation_programs_save(
    app: AppHandle,
    programs: Vec<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if programs.len() > 32 {
            return Err("Approve at most 32 programs.".into());
        }
        let store = app.state::<SettingsStore>();
        let mut current = store.current.lock().unwrap();
        let mut kept = Vec::new();
        for program in programs {
            let path = if current.automations_commands.contains(&program) {
                program
            } else {
                commands::display(&commands::canonical_program(&program)?)
            };
            if !kept.contains(&path) {
                kept.push(path);
            }
        }
        let mut settings = current.clone();
        settings.automations_commands = kept.clone();
        crate::settings::save(&store.path, &settings)
            .map_err(|_| "Approved programs could not be saved.".to_string())?;
        *current = settings;
        Ok(kept)
    })
    .await
    .map_err(|_| "Approved programs could not be saved.".to_string())?
}

fn validate_deliveries(value: &Value) -> Result<(), String> {
    let list = value.as_array().ok_or("Choose a delivery list.")?;
    let mut seen = std::collections::BTreeSet::new();
    for d in list {
        let kind = d["kind"].as_str().ok_or("Choose a delivery destination.")?;
        if !["home", "telegram", "discord", "whatsapp"].contains(&kind)
            || d.as_object().is_none_or(|o| o.len() != 1)
            || !seen.insert(kind)
        {
            return Err(
                "Choose each supported destination once; phone chats are chosen only in MESSAGING."
                    .into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod delivery_validation_tests {
    use super::*;
    #[test]
    fn only_named_destinations_without_recipient_overrides() {
        assert!(validate_deliveries(&json!([{"kind":"telegram"},{"kind":"home"}])).is_ok());
        for bad in [
            json!([{"kind":"telegram","chat_id":"other"}]),
            json!([{"kind":"telegram"},{"kind":"telegram"}]),
            json!([{"kind":"email"}]),
        ] {
            assert!(validate_deliveries(&bad).is_err());
        }
    }
}
