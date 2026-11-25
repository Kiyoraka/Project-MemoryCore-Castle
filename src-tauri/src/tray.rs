//! Castle in the tray: the icon, its menu, and the one way the window is shown or hidden.

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

const MAIN_WINDOW: &str = "main";
/// Tells the page to start the companion's session; the page does it, because it must be listening first.
pub const WAKE_EVENT: &str = "castle-wake";
/// Tells the page Castle just went out of sight (the minus button, the tray, the hotkey, the close
/// button): a personal room blurs again so a shared screen never shows it on the way back.
pub const HIDDEN_EVENT: &str = "castle-hidden";

/// Castle went out of sight: the page hears it, and the WORLD on the other screens goes too.
pub fn went_hidden(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let _ = window.emit(HIDDEN_EVENT, ());
    }
    crate::world::set_visible(app, false);
}

/// Brings Castle to the front, whatever state it was in.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = crate::screen::fill_desktop(&window);
        let _ = window.set_focus();
    }
    // the WORLD on the other screens comes back with Castle
    crate::world::set_visible(app, true);
}

/// Hides Castle when it is in front, otherwise brings it forward.
pub fn toggle_main(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let in_front = window.is_visible().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
            && window.is_focused().unwrap_or(false);
        if in_front {
            let _ = window.hide();
            went_hidden(app);
        } else {
            show_main(app);
        }
    }
}

/// Actual Quit ends the process; hiding/minimizing never does.
pub fn quit_notice(session: bool, automations: bool, active: Option<usize>) -> Option<String> {
    if !session && !automations && active == Some(0) {
        return None;
    }
    let mut parts = Vec::new();
    if session {
        parts.push("Your companion's session will end.".to_string());
    }
    if automations || active != Some(0) {
        parts.push("Automations stop until Castle opens again.".to_string());
        if let Some(n) = active.filter(|n| *n > 0) {
            parts.push(format!(
                "{n} run(s) are still active; quitting may leave unconfirmed results."
            ));
        }
    }
    parts.push("Hide Castle to keep it running.".into());
    Some(parts.join(" "))
}
/// Confirms actual Quit when sessions, automations or active runs would be stopped.
pub fn quit(app: &AppHandle) {
    let active = app
        .state::<crate::automations::Running>()
        .0
        .try_lock()
        .ok()
        .map(|r| r.len());
    let Some(notice) = quit_notice(
        crate::sessions_running(app),
        app.state::<crate::settings::SettingsStore>()
            .get()
            .room_on("automations"),
        active,
    ) else {
        app.exit(0);
        return;
    };
    show_main(app);
    let handle = app.clone();
    app.dialog()
        .message(notice)
        .title("Quit MemoryCore Castle?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Quit".into(),
            "Stay".into(),
        ))
        .show(move |quit| {
            if quit {
                handle.exit(0);
            }
        });
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Castle", true, None::<&str>)?;
    let wake = MenuItem::with_id(app, "wake", "Wake companion", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &wake, &quit_item])?;

    let mut tray = TrayIconBuilder::with_id("castle")
        .tooltip("MemoryCore Castle")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "wake" => {
                show_main(app);
                let _ = app.emit(WAKE_EVENT, ());
            }
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg(test)]
mod reliability_tests {
    use super::*;
    #[test]
    fn actual_quit_warns_even_without_a_companion_session() {
        assert!(quit_notice(false, false, Some(0)).is_none());
        let automation = quit_notice(false, true, Some(0)).unwrap();
        assert!(automation.contains("Automations stop"));
        assert!(automation.contains("Hide Castle"));
        assert!(quit_notice(false, false, Some(2))
            .unwrap()
            .contains("2 run(s)"));
        assert!(quit_notice(true, false, Some(0))
            .unwrap()
            .contains("session will end"));
        assert!(quit_notice(false, false, None).is_some());
    }
}
