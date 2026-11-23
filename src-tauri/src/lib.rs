//! MemoryCore Castle: a full-screen desktop that hosts the member's own AI CLI
//! (Claude Code or Codex) inside their MemoryCore folder.

mod appearance;
mod appearance_db;
mod library_db;
mod spiral;
mod apps;
mod shell;
mod thumbs;
mod backup;
mod chat;
mod chatroom;
mod clipboard;
mod comfy;
mod commands;
mod credits;
mod db;
mod fetch;
mod node_runtime;
mod sweep;
mod dev;
mod dev_files;
mod dev_git;
mod dev_new;
mod dev_tabs;
mod door;
mod door_plan;
mod door_trait;
mod door_look;
mod door_library;
mod emotion;
mod env;
mod os;
mod files;
mod git;
mod import;
mod key;
mod link;
mod memdb;
mod market;
mod markup;
mod sentinel;
mod memory;
mod gpu;
mod disks;
mod weather;
mod meter_watch;
mod mirror;
mod plans;
mod project_room;
mod project_tree;
mod profile;
mod projectdb;
mod projectdb_login;
mod cmd_projectdb;
mod pty;
mod providers;
mod messages;
mod page_reader;
mod oneshot;
mod resume;
mod quota;
mod alerts;
mod bridges;
mod roomalert;
mod roomdraw;
mod roomvoice;
mod roomshelf;
mod mood;
mod door_mood;
mod roomread;
mod rooms;
mod roomturn;
mod signin;
mod screen;
mod servers;
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
mod workflows;
mod automations;
mod calendar;
mod describe;
mod market_shop;
mod keeper;
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
    let changed_automations=settings.room_on("automations")!=store.get().room_on("automations");
    let mut settings = settings.normalized();
    if app.state::<session::MainHost>().0.is_running() {
        // Provider changes go through the queued handoff, not stale room/settings snapshots.
        settings.cli = store.get().cli;
    }
    // the companion's room key is Rust's to keep: a page holding an older copy never writes it back
    {
        let current = store.get();
        settings.automations_paused = current.automations_paused;
        settings.automations_commands = current.automations_commands;
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
        settings.dev_tab_numbers = current.dev_tab_numbers.clone();
        settings.dev_projects = current.dev_projects.clone();
        // the gauges' gear is saved only by its own command (meter_watch_save)
        settings.meter_watch = current.meter_watch;
        // the graphics-card gauges' switch is saved only by its own command (gpu_gauges_save)
        settings.gpu_gauges = current.gpu_gauges.clone();
        // the weather widget's place and units are saved only by its own command (weather_save)
        settings.weather = current.weather.clone();
        // Sweep's gear is saved only by its own command (sweep_settings_save)
        settings.sweep = current.sweep.clone();
        // the Sentinel's gear is saved only by its own command (sentinel_settings_save)
        settings.sentinel = current.sentinel.clone();
        // the credit widgets' figures, states and budget are kept by their own commands
        settings.credits = current.credits.clone();
        // the member's ComfyUI is saved only by its own command (comfy_save)
        settings.comfy = current.comfy.clone();
        // who the phone bridges answer is changed only by their own Approve / Make me / Remove /
        // Forget commands: a Settings page holding an older copy never un-approves anyone
        settings.telegram_bridge_chat_id = current.telegram_bridge_chat_id.clone();
        settings.discord_bridge_channel_id = current.discord_bridge_channel_id.clone();
        settings.whatsapp_bridge_chat_id = current.whatsapp_bridge_chat_id.clone();
        settings.whatsapp_bridge_me_name = current.whatsapp_bridge_me_name.clone();
        settings.whatsapp_bridge_me_digits = current.whatsapp_bridge_me_digits.clone();
        settings.whatsapp_guests = current.whatsapp_guests.clone();
    }
    settings::save(&store.path, &settings).map_err(|_| "The settings could not be saved.".to_string())?;
    *store.startup_note.lock().unwrap() = startup::sync(&app, settings.start_with_windows);
    *store.current.lock().unwrap() = settings;
    if changed_automations{automations::health::interrupt(&app);}
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
    credits::forget_cache();
    Ok(key::statuses())
}

#[tauri::command]
fn key_forget(name: String) -> Result<Vec<key::KeyStatus>, String> {
    key::forget(&name)?;
    credits::forget_cache();
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
    // A clicked notification starts a second Castle with Castle's own address. This new process is the
    // one Windows lets come to the front, so it brings the running Castle forward before handing over
    // (the running Castle alone only gets its taskbar button flashed).
    if roomalert::wanted_room(args.iter().cloned()).is_some() {
        shell::bring_running_castle_forward();
    }
    // `--sentinel-tick [--cause alarm]`: the Sentinel's watch, run by the system's scheduler; it ends, no window
    if args.first().map(String::as_str) == Some(sentinel::watch::TICK_FLAG) {
        std::process::exit(sentinel::watch::tick_cli(&args[1..]));
    }
    // `--sweep-deep <result file>`: Sweep's Deep clean, run elevated by Castle itself; it ends, no window
    if args.first().map(String::as_str) == Some(sweep::DEEP_FLAG) {
        std::process::exit(sweep::run_deep_cli(&args[1..]));
    }
    #[cfg(target_os = "macos")]
    env::adopt_login_path();
    let builder = tauri::Builder::default();
    // the signed self-updater lives only in official builds (scripts/release.ps1)
    #[cfg(feature = "official")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        // Must come first: a second launch only brings the running Castle forward.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tray::show_main(app);
            // a clicked notification starts Castle with its own address: show CHAT
            if let Some(room) = roomalert::wanted_room(args) {
                roomalert::open_room(app, &room);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![startup::SIGN_IN_ARG]),
        ))
        .manage(automations::Running::default())
        .manage(automations::health::Health::default())
        .manage(alerts::ActiveRoom::default())
        .manage(session::MainHost::default())
        .manage(providers::Control::default())
        .manage(providers::Images::default())
        .manage(comfy::Launcher::default())
        .manage(messages::Pending::default())
        .manage(bridges::telegram::TelegramState::default())
        .manage(bridges::discord::DiscordState::default())
        .manage(bridges::whatsapp::WhatsAppState::default())
        .manage(session::StoryHost::default())
        .manage(dev::DevHosts::default())
        .manage(dev::DevChats::default())
        .manage(system::SystemMeters::default())
        .manage(gpu::GpuCache::default())
        .manage(meter_watch::Watch::default())
        .manage(chat::MainChat::default())
        .manage(chat::StoryChat::default())
        .manage(chat::MemberPictures::default())
        .manage(projectdb::Held::default())
        .manage(markup::MarkedCopies::default())
        .manage(roomread::RoomReader::default())
        .manage(market::board::Board::default())
        .manage(market::engine::Viewing::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            project_room::project_state,
            project_room::project_shelve,
            project_room::project_task_add,
            project_room::project_task_done,
            project_room::project_task_remove,
            project_room::project_task_order,
            project_room::project_split_proposals,
            project_room::project_split_apply,
            alerts::alert_sources,
            alerts::alert_active_room,
            roomalert::alert_sounds,
            roomalert::alert_sound_preview,
            roomalert::alert_sounds_folder,
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
            dev::dev_servers,
            dev::dev_close_tab,
            dev::dev_new_session,
            dev::dev_remove_project,
            dev::dev_wake,
            dev::dev_write,
            dev::dev_resize,
            dev::dev_snapshot,
            dev::dev_chat_poll,
            dev::dev_git_dirty,
            dev::dev_plan_items,
            dev::dev_plan_body,
            chat::dev_chat_media,
            chat::dev_chat_media_stream,
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
            gpu::gpu_meters,
            gpu::gpu_gauges_save,
            disks::disk_drives,
            comfy::comfy_get,
            comfy::comfy_save,
            comfy::comfy_check,
            comfy::comfy_start,
            weather::weather_get,
            weather::weather_search,
            weather::weather_save,
            weather::weather_here,
            weather::weather_read,
            sweep::sweep_status,
            sweep::sweep_measure,
            sweep::sweep_clean,
            sweep::sweep_deep,
            sweep::sweep_settings_save,
            credits::credits_read,
            credits::credits_set,
            credits::credits_budget,
            page_reader::credits_sign_in,
            page_reader::credits_read_now,
            page_reader::credits_stop_reading,
            meter_watch::meter_watch_get,
            meter_watch::meter_watch_save,
            meter_watch::meter_trim_now,
            meter_watch::meter_trips,
            meter_watch::meter_close,
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
            automations::automation_list,
            automations::health::automation_health,
            automations::automation_save,
            automations::automation_history,
            automations::automation_run,
            automations::companion::automation_describe,
            automations::triggers::automation_message_match,
            automations::automation_archive,
            automations::automation_pause,
            automations::automation_program_pick,
            automations::automation_programs_save,
            automations::hook::automation_hook_reset,
            calendar::calendar_range,
            calendar::calendar_event_get,
            calendar::calendar_event_save,
            calendar::calendar_event_delete,
            calendar::calendar_event_move,
            calendar::calendar_event_skip,
            calendar::calendar_quickadd_preview,
            calendar::exchange::calendar_ics_preview,
            calendar::exchange::calendar_ics_import,
            calendar::exchange::calendar_ics_discard,
            calendar::exchange::calendar_ics_export,
            calendar::calendar_subs,
            calendar::calendar_sub_add,
            calendar::calendar_sub_set,
            calendar::calendar_sub_remove,
            calendar::calendar_sub_refresh,
            calendar::calendar_link_jobs,
            calendar::calendar_proposals,
            calendar::calendar_proposal_decide,
            calendar::calendar_home,
            calendar::calendar_notice_dismiss,
            workflows::workflow_list,
            workflows::workflow_check,
            workflows::workflow_save,
            workflows::workflow_duplicate,
            workflows::workflow_delete,
            workflows::workflow_folder,
            workflows::workflow_export,
            workflows::workflow_import,
            workflows::workflow_run,
            workflows::workflow_pick_picture,
            describe::picture_describe,
            market_shop::market_shop_listings,
            market_shop::market_shop_listing,
            market_shop::market_shop_checkout,
            market_shop::market_shop_purchases,
            market_shop::market_shop_install,
            market_shop::market_shop_vote,
            market_shop::market_shop_connect,
            market_shop::market_shop_mine,
            market_shop::market_shop_publish,
            market_shop::market_shop_withdraw,
            market_shop::market_shop_shops,
            market_shop::market_shop_shop,
            market_shop::market_shop_my_shop,
            market_shop::market_shop_my_shop_save,
            keeper::keeper_say,
            studio::studio_library,
            thumbs::studio_thumbs,
            studio::studio_powers,
            studio::picture_way,
            studio::studio_delete,
            voicestudio::voicestudio_status,
            bridges::telegram::telegram_bridge_status,
            bridges::telegram::telegram_bridge_approve,
            bridges::telegram::telegram_bridge_forget,
            bridges::discord::discord_bridge_status,
            bridges::discord::discord_bridge_approve,
            bridges::discord::discord_bridge_forget,
            bridges::whatsapp::whatsapp_bridge_status,
            bridges::whatsapp::whatsapp_bridge_approve,
            bridges::whatsapp::whatsapp_bridge_forget,
            bridges::whatsapp::whatsapp_bridge_make_me,
            bridges::whatsapp::whatsapp_bridge_remove_guest,
            bridges::whatsapp::whatsapp_bridge_unpair,
            bridges::reactions::phone_reactions,
            voicestudio::voicestudio_speak,
            appearance::appearance_look,
            appearance::appearance_new,
            appearance::appearance_import,
            appearance::appearance_wear,
            appearance::appearance_save_look,
            appearance::appearance_save_item,
            library_db::library_books,
            library_db::library_read,
            library_db::library_delete,
            spiral::spiral_room,
            spiral::spiral_read,
            spiral::spiral_keep,
            spiral::spiral_kept,
            story::story_prepare,
            story::story_worlds,
            story::story_beat,
            world::world_screens,
            world::world_apply,
            world::castle_move_to_screen,
            world::world_music,
            world::world_identify,
            session::claude_voice,
            quota::cli_quota,
            quota::cli_usage,
            rooms::rooms_apply,
            signin::member_sign_in,
            screen::castle_window_layout,
            signin::member_sign_in_cancel,
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
            roomread::room_reconnect,
            roomturn::room_log,
            roomdraw::room_pictures_status,
            roomvoice::room_voices_status,
            roomshelf::room_shelf,
            roomshelf::room_shelf_refresh,
            roomshelf::room_shelf_remove,
            mood::mood_today,
            mood::mood_history,
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
            chatroom::room_react,
            chatroom::room_file,
            chatroom::room_picture_save,
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
            chat::chat_plan_file,
            cmd_projectdb::projectdb_connection,
            cmd_projectdb::projectdb_save,
            cmd_projectdb::projectdb_forget,
            cmd_projectdb::projectdb_test,
            cmd_projectdb::projectdb_pick_ca,
            cmd_projectdb::projectdb_databases,
            cmd_projectdb::projectdb_tables,
            cmd_projectdb::projectdb_table_rows,
            cmd_projectdb::projectdb_columns,
            cmd_projectdb::projectdb_look,
            cmd_projectdb::projectdb_change,
            cmd_projectdb::projectdb_finish,
            cmd_projectdb::projectdb_export,
            cmd_projectdb::projectdb_dump,
            cmd_projectdb::projectdb_import,
            cmd_projectdb::projectdb_saved,
            cmd_projectdb::projectdb_saved_set,
            chat::chat_media_stream,
            markup::chat_markup_save,
            db::db_status,
            node_runtime::node_status,
            node_runtime::node_prepare,
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
            db::db_skill_stub_all,
            emotion::db_traits,
            emotion::db_trait_save,
            emotion::db_trait_activate,
            emotion::db_trait_retire,
            emotion::emotion_propose,
            profile::profile_read,
            profile::profile_month,
            profile::profile_scan,
            db::db_setup_all,
            market::commands::market_state,
            market::commands::market_symbols,
            market::commands::market_candles,
            market::commands::market_candles_before,
            market::commands::market_preview,
            market::commands::market_place,
            market::commands::market_cancel,
            market::commands::market_close,
            market::commands::market_exits,
            market::commands::market_margin,
            market::commands::market_leverage,
            market::commands::market_new_run,
            market::commands::market_close_run,
            market::commands::market_runs,
            market::commands::market_history,
            market::commands::market_fees,
            market::companion::market_turn,
            market::companion::market_auto_get,
            market::companion::market_auto_save,
            market::companion::market_stop,
            market::companion::market_turns,
            market::companion::market_lessons,
            market::companion::market_lesson_delete,
            db::db_move_out,
            sentinel::sentinel_settings_save,
            sentinel::officer::sentinel_patrol_now,
            sentinel::commands::sentinel_status,
            sentinel::commands::sentinel_findings,
            sentinel::commands::sentinel_click,
            sentinel::commands::sentinel_log,
            sentinel::commands::sentinel_alarms_seen,
            sentinel::commands::sentinel_setup
        ])
        .setup(|app| {
            let config = match settings::test_override(settings::TEST_CONFIG_VAR) {
                Some(folder) => folder,
                None => app.path().app_config_dir()?,
            };
            roomalert::register_address();
            cmd_projectdb::start_sweeper(app.handle().clone());
            alerts::start(app.handle());
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
            // warms the ChatGPT sign-in cache in the background, so STUDIO's or APPEARANCE's
            // first visit is not the one waiting on it (a signed-in Codex draws for any companion;
            // without Codex installed the check returns at once)
            std::thread::spawn(providers::codex_chatgpt_cached);
            providers::start_images(app.handle());
            messages::start(app.handle());
            world::restore(app.handle());
            world::watch_covered(app.handle());
            roomread::start(app.handle());
            bridges::telegram::start(app.handle());
            bridges::discord::start(app.handle());
            bridges::whatsapp::start(app.handle());
            link::start(app.handle());
            meter_watch::start(app.handle());
            page_reader::start(app.handle());
            market::engine::start(app.handle());
            market::companion::start(app.handle());
            sentinel::start(app.handle());
            automations::start(app.handle());
            db::start_with_castle(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides Castle to the tray; Quit in the tray ends it.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    tray::went_hidden(window.app_handle());
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
            // macOS: the Dock icon (or opening Castle again from the Finder) brings the hidden window back
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = event {
                tray::show_main(app);
            }
            // macOS: ⌘Q and the Dock's Quit ask first, as the tray's Quit does, when a session would end
            #[cfg(target_os = "macos")]
            if let RunEvent::ExitRequested { code: None, api, .. } = &event {
                api.prevent_exit();
                tray::quit(app);
            }
            // The companion's sessions belong to Castle: when Castle ends, so do they.
            if let RunEvent::Exit = event {
                session::end_all(app);
                dev::end_all(app);
                db::stop_with_castle(app);
                comfy::stop_with_castle(app);
            }
        });
}
