//! MemoryCore Castle: a full-screen desktop that hosts the member's own AI CLI
//! (Claude Code or Codex) inside their MemoryCore folder.

mod appearance;
mod apps;
mod backup;
mod chat;
mod chatroom;
mod clipboard;
mod commands;
mod db;
mod dev;
mod dev_files;
mod dev_git;
mod dev_new;
mod dev_tabs;
mod door;
mod door_plan;
mod door_trait;
mod emotion;
mod env;
mod files;
mod git;
mod import;
mod key;
mod link;
mod memdb;
mod memory;
mod mirror;
mod plans;
mod pty;
mod providers;
mod messages;
mod oneshot;
mod quota;
mod roomalert;
mod roomdraw;
mod roomread;
mod rooms;
mod roomturn;
mod signin;
mod screen;
mod session;
mod settings;
mod setup;
mod startup;
mod store;
mod story;
mod studio;
mod voicestudio;
mod system;
mod theme;
mod themes;
mod traits;
mod tree;
mod transcript;
mod tray;
mod update;
mod world;
mod worlds;

use serde::Serialize;
use settings::{Settings, SettingsStore};
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Whether any companion session is running (asked before quitting).
pub fn sessions_running(app: &AppHandle) -> bool {
    session::any_running(app) || dev::any_running(app)
}

/// Whether this launch came from the Windows sign-in entry.
pub struct Launch {
    pub at_sign_in: bool,
}

/// The one automatic start of this run; taken once by the screen.
pub struct AutoWake(pub std::sync::Mutex<bool>);

/// True once, when this run should start the companion's session by itself; false every time after.
#[tauri::command]
fn take_auto_wake(auto_wake: State<AutoWake>) -> bool {
    std::mem::replace(&mut *auto_wake.0.lock().unwrap(), false)
}

/// What the page reads about the settings.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    settings: Settings,
    companion_label: String,
    cli_label: &'static str,
    warning: Option<String>,
    startup_note: Option<String>,
}

fn view(store: &SettingsStore) -> SettingsView {
    let settings = store.get();
    SettingsView {
        companion_label: settings.companion_label(),
        cli_label: settings.cli.label(),
        warning: store.warning.lock().unwrap().clone(),
        startup_note: store.startup_note.lock().unwrap().clone(),
        settings,
    }
}

#[tauri::command]
fn get_settings(store: State<SettingsStore>) -> SettingsView {
    view(&store)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings, store: State<SettingsStore>) -> Result<SettingsView, String> {
    if settings.memory_folder != store.get().memory_folder && (sessions_running(&app) || app.state::<providers::Images>().0.lock().unwrap().iter().any(|j| j.phase == "working")) {
        return Err("End active sessions and image work before changing the MemoryCore folder.".into());
    }
    let mut settings = settings.normalized();
    if app.state::<session::MainHost>().0.is_running() {
        // Provider changes go through the queued handoff, not stale room/settings snapshots.
        settings.cli = store.get().cli;
    }
    // the companion's room key is Rust's to keep: a page holding an older copy never writes it back
    {
        let current = store.get();
        settings.room_companion_key_id = current.room_companion_key_id;
        settings.room_companion_name = current.room_companion_name;
        // the database's port and kind are set by the engine's own buttons
        settings.db_port = current.db_port;
        settings.db_external = current.db_external;
        // the screen Castle lives on is changed only by its own button
        settings.castle_screen = current.castle_screen;
        // the WORLD room's screens and the music are changed only by their own commands (world_apply,
        // world_music): a Settings page holding an older copy must not bring an old music state back
        settings.world = current.world;
        // the DEV tab's projects folder is chosen only by its own command (dev_set_root)
        settings.dev_root = current.dev_root;
        // the DEV tabs and the recent list are kept by the DEV room's own commands, never by the Settings page
        settings.dev_tabs = current.dev_tabs.clone();
        settings.dev_projects = current.dev_projects.clone();
    }
    settings::save(&store.path, &settings).map_err(|_| "The settings could not be saved.".to_string())?;
    *store.startup_note.lock().unwrap() = startup::sync(&app, settings.start_with_windows);
    *store.current.lock().unwrap() = settings;
    *store.warning.lock().unwrap() = None;
    studio::allow_memory_folder(&app);
    Ok(view(&store))
}

/// Opens the folder picker; None when the member closes it without choosing.
#[tauri::command]
async fn choose_folder(app: AppHandle, title: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title.unwrap_or_else(|| "Choose your MemoryCore folder".into()))
            .blocking_pick_folder()
            .and_then(|folder| folder.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// Every key Castle can keep, masked.
#[tauri::command]
fn key_status() -> Vec<key::KeyStatus> {
    key::statuses()
}

#[tauri::command]
fn key_save(name: String, value: String) -> Result<Vec<key::KeyStatus>, String> {
    key::save(&name, &value)?;
    Ok(key::statuses())
}

#[tauri::command]
fn key_forget(name: String) -> Result<Vec<key::KeyStatus>, String> {
    key::forget(&name)?;
    Ok(key::statuses())
}

/// Tests a kept key against the service's free account check.
#[tauri::command]
async fn key_test(name: String) -> Result<key::KeyTest, String> {
    tauri::async_runtime::spawn_blocking(move || key::test(&name)).await.map_err(|_| "The check stopped.".to_string())
}

/// The whole key, only when the member presses Show in Settings.
#[tauri::command]
fn key_reveal(name: String) -> Option<String> {
    key::load(&name)
}

pub fn run() {
    // `memorycore-castle.exe --memory <verb> ...` is the companion's door to the memory
    // database (castle-memory in the tools folder): it answers and ends, no window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(door::FLAG) {
        std::process::exit(door::main(&args[1..]));
    }
    let builder = tauri::Builder::default();
    // the signed self-updater lives only in official builds (scripts/release.ps1)
    #[cfg(feature = "official")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        // Must come first: a second launch only brings the running Castle forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![startup::SIGN_IN_ARG]),
        ))
        .manage(session::MainHost::default())
        .manage(providers::Control::default())
        .manage(providers::Images::default())
        .manage(messages::Pending::default())
        .manage(session::StoryHost::default())
        .manage(dev::DevHosts::default())
        .manage(dev::DevChats::default())
        .manage(system::SystemMeters::default())
        .manage(chat::MainChat::default())
        .manage(chat::StoryChat::default())
        .manage(chat::MemberPictures::default())
        .manage(roomread::RoomReader::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            providers::providers_list,
            providers::image_request,
            providers::image_jobs,
            messages::send_message,
            messages::queued_messages,
            messages::queued_message_action,
            providers::image_retry,
            providers::image_dismiss,
            providers::image_cancel,
            dev::dev_state,
            dev::dev_open,
            dev::dev_close_tab,
            dev::dev_forget,
            dev::dev_wake,
            dev::dev_write,
            dev::dev_resize,
            dev::dev_snapshot,
            dev::dev_chat_poll,
            dev::dev_git_dirty,
            dev::dev_plan_items,
            dev::dev_plan_body,
            chat::dev_chat_media,
            chat::dev_media_open,
            dev_git::dev_git_status,
            dev_git::dev_git_diff,
            dev_git::dev_git_log,
            dev_git::dev_git_show,
            dev_git::dev_git_commit,
            dev_git::dev_git_push,
            dev_git::dev_git_remote,
            dev_git::dev_open_link,
            dev_new::dev_github,
            dev_new::dev_root,
            dev_new::dev_set_root,
            dev_new::dev_kinds,
            dev_new::dev_clone,
            dev_new::dev_create,
            dev_new::dev_connect,
            dev_new::dev_git_init,
            dev_files::dev_tree,
            dev_files::dev_file,
            dev_files::dev_reveal,
            providers::provider_switch,
            providers::provider_switch_status,
            providers::provider_switch_cancel,
            chat::chat_history,
            save_settings,
            choose_folder,
            key_status,
            key_save,
            key_forget,
            key_reveal,
            key_test,
            session::terminal_state,
            session::terminal_start,
            session::terminal_write,
            session::terminal_resize,
            session::story_close,
            commands::system_meters,
            commands::apps_list,
            commands::apps_draw_icons,
            commands::app_open,
            commands::clipboard_paste,
            commands::clipboard_copy,
            commands::user_themes,
            commands::hide_castle,
            commands::media_open,
            commands::memory_features,
            memory::memory_home,
            memory::memory_shelves,
            memory::memory_read,
            theme::theme_create,
            theme::theme_delete,
            theme::theme_folder,
            studio::studio_library,
            studio::studio_powers,
            voicestudio::voicestudio_status,
            voicestudio::voicestudio_speak,
            appearance::appearance_look,
            appearance::appearance_new,
            appearance::appearance_import,
            appearance::appearance_wear,
            story::story_prepare,
            story::story_worlds,
            story::story_beat,
            world::world_screens,
            world::world_apply,
            world::castle_move_to_screen,
            world::world_music,
            world::world_identify,
            quota::cli_quota,
            quota::cli_usage,
            rooms::rooms_apply,
            signin::member_sign_in,
            signin::member_status,
            signin::member_sign_out,
            chatroom::room_companion_in,
            chatroom::room_mine,
            chatroom::room_seen,
            chatroom::room_avatar,
            chatroom::room_face_set,
            chatroom::room_name_set,
            chatroom::room_companion_refresh,
            link::link_view,
            roomread::room_visible,
            roomread::room_backlog,
            roomread::room_reset,
            roomturn::room_log,
            roomdraw::room_pictures_status,
            roomturn::room_card,
            roomturn::room_card_save,
            worlds::worlds_mine,
            worlds::world_new,
            worlds::world_delete,
            worlds::world_prepare,
            chatroom::room_me,
            chatroom::room_lines,
            chatroom::room_presence,
            chatroom::room_post,
            chatroom::room_file,
            update::update_check,
            update::update_install,
            update::version_seen,
            update::releases_page,
            update::diary_stamp,
            commands::choose_picture,
            commands::avatar_set,
            commands::avatar_get,
            commands::avatar_clear,
            setup::setup_inspect,
            setup::setup_create,
            setup::setup_check_cli,
            setup::setup_apply,
            setup::setup_found_keys,
            setup::setup_import_keys,
            backup::backup_status,
            backup::backup_create,
            backup::backup_now,
            backup::backup_sign_in,
            commands::open_url,
            take_auto_wake,
            chat::chat_poll,
            chat::chat_media,
            chat::chat_attach,
            db::db_status,
            db::db_install,
            db::db_start,
            db::db_stop,
            db::db_use_existing,
            db::db_create,
            db::db_move_preview,
            db::db_move_in,
            db::db_memory,
            db::db_restore,
            db::db_export,
            db::db_skills,
            db::db_skill_stub,
            emotion::db_traits,
            emotion::db_trait_save,
            emotion::db_trait_activate,
            emotion::db_trait_retire,
            emotion::emotion_propose,
            db::db_setup_all,
            db::db_move_out
        ])
        .setup(|app| {
            let config = match settings::test_override(settings::TEST_CONFIG_VAR) {
                Some(folder) => folder,
                None => app.path().app_config_dir()?,
            };
            let store = SettingsStore::open(&config);
            session::forget_boot_file(&store);
            let current = store.get();
            *store.startup_note.lock().unwrap() = startup::sync(app.handle(), current.start_with_windows);
            app.manage(store);
            let at_sign_in = startup::is_sign_in_launch(std::env::args());
            app.manage(Launch { at_sign_in });
            app.manage(AutoWake(std::sync::Mutex::new(startup::should_auto_wake(
                at_sign_in,
                current.start_session_on_launch,
            ))));

            // Pasted pictures are only needed for a while; yesterday's go.
            clipboard::clear_old(&clipboard::paste_folder(), clipboard::PASTES_KEPT_FOR);

            tray::install(app.handle())?;
            backup::start_diary_watcher(app.handle());
            studio::start_notice_watcher(app.handle());
            studio::allow_memory_folder(app.handle());

            let toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM);
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, shortcut, event| {
                        if shortcut == &toggle && event.state() == ShortcutState::Pressed {
                            tray::toggle_main(app);
                        }
                    })
                    .build(),
            )?;
            // Another app may already hold the keys; the tray still brings Castle back.
            if let Err(error) = app.global_shortcut().register(toggle) {
                eprintln!("MemoryCore Castle: the hotkey could not be registered: {error}");
            }

            if let Some(window) = app.get_webview_window("main") {
                if !world::place_saved(app.handle(), &window) {
                    screen::fill_desktop(&window)?;
                }
                window.show()?;
            }
            screen::watch_desktop(app.handle());
            providers::start(app.handle());
            providers::start_images(app.handle());
            messages::start(app.handle());
            world::restore(app.handle());
            world::watch_covered(app.handle());
            roomread::start(app.handle());
            link::start(app.handle());
            db::start_with_castle(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides Castle to the tray; Quit in the tray ends it.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    world::set_visible(window.app_handle(), false);
                }
            }
            // Castle minimised to the taskbar comes back with a click there: the WORLD on the other
            // screens, hidden when it went, comes back with it.
            if let WindowEvent::Focused(true) = event {
                if window.label() == "main" && window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
                    world::set_visible(window.app_handle(), true);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("MemoryCore Castle could not start")
        .run(|app, event| {
            // The companion's sessions belong to Castle: when Castle ends, so do they.
            if let RunEvent::Exit = event {
                session::end_all(app);
                dev::end_all(app);
                db::stop_with_castle(app);
            }
        });
}
