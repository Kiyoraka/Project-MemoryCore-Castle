//! The Phone panel's commands (S-1124-5): start, read, show, touch and stop a DEV tab's phone.

use crate::emulator::{self, PhoneList, PhoneState, Phones, SLOTS};
use crate::emulator_grpc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

/// Each tab's screen stream: a newer stream for the same tab ends the older one.
#[derive(Default)]
pub struct Screens(pub [Arc<AtomicU64>; SLOTS]);

fn slot_of(slot: u8) -> Result<usize, String> {
    let s = slot as usize;
    if s < SLOTS {
        Ok(s)
    } else {
        Err("That tab does not exist.".into())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "The phone could not be reached.".to_string())
}

#[tauri::command]
pub async fn phone_list(phones: State<'_, Phones>) -> Result<PhoneList, String> {
    let phones = phones.inner().clone();
    blocking(move || emulator::list(&phones)).await
}

#[tauri::command]
pub async fn phone_start(slot: u8, avd: String, phones: State<'_, Phones>) -> Result<PhoneState, String> {
    let s = slot_of(slot)?;
    let phones = phones.inner().clone();
    blocking(move || emulator::start(&phones, s, &avd)).await?
}

#[tauri::command]
pub fn phone_state(slot: u8, phones: State<'_, Phones>) -> Result<PhoneState, String> {
    Ok(phones.state(slot_of(slot)?))
}

#[tauri::command]
pub async fn phone_stop(slot: u8, phones: State<'_, Phones>, screens: State<'_, Screens>) -> Result<(), String> {
    let s = slot_of(slot)?;
    screens.0[s].fetch_add(1, Ordering::SeqCst);
    let phones = phones.inner().clone();
    blocking(move || emulator::stop(&phones, s)).await
}

/// Streams the phone's screen into the panel's channel (raw frames: width, height, RGBA).
#[tauri::command]
pub async fn phone_screen(slot: u8, width: u32, frames: Channel<InvokeResponseBody>, phones: State<'_, Phones>, screens: State<'_, Screens>) -> Result<(), String> {
    let s = slot_of(slot)?;
    let wire = phones.wire(s).ok_or("The phone is not running.")?;
    let generation = screens.0[s].clone();
    let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(async move {
        // a stream that breaks says why in the panel, instead of a screen that just stops
        if let Err(why) = emulator_grpc::stream(wire, width, frames.clone(), generation, mine).await {
            let _ = frames.send(InvokeResponseBody::Json(emulator_grpc::said(&why)));
        }
    });
    Ok(())
}

/// The panel closed: its screen stream ends.
#[tauri::command]
pub fn phone_screen_end(slot: u8, screens: State<'_, Screens>) -> Result<(), String> {
    screens.0[slot_of(slot)?].fetch_add(1, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn phone_touch(slot: u8, x: i32, y: i32, down: bool, phones: State<'_, Phones>) -> Result<(), String> {
    let wire = phones.wire(slot_of(slot)?).ok_or("The phone is not running.")?;
    emulator_grpc::touch(wire, x, y, down).await
}

/// One of the phone's buttons (back, home, recent, power, volume-up, volume-down, enter, backspace)
/// or text typed into it.
#[tauri::command]
pub async fn phone_key(slot: u8, button: Option<String>, text: Option<String>, phones: State<'_, Phones>) -> Result<(), String> {
    let wire = phones.wire(slot_of(slot)?).ok_or("The phone is not running.")?;
    let button = match button.as_deref() {
        Some(name) => Some(emulator_grpc::button_key(name).ok_or("That is not one of the phone's buttons.")?),
        None => None,
    };
    let text = text.map(|t| t.chars().take(200).collect::<String>()).filter(|t| !t.is_empty());
    if button.is_none() && text.is_none() {
        return Ok(());
    }
    emulator_grpc::key(wire, button, text).await
}

#[tauri::command]
pub async fn phone_rotate(slot: u8, phones: State<'_, Phones>) -> Result<(), String> {
    let s = slot_of(slot)?;
    let phones = phones.inner().clone();
    blocking(move || emulator::rotate(&phones, s)).await?
}

// Run, Hot reload, Restart

fn folder_of(store: &crate::settings::SettingsStore, slot: u8) -> Result<std::path::PathBuf, String> {
    crate::dev::folder_of(&store.get(), slot as usize)
}

/// Runs the tab's app: on its phone, or (ios) as the iPhone preview for a Flutter app.
#[tauri::command]
pub async fn phone_run(slot: u8, ios: bool, app: tauri::AppHandle, store: State<'_, crate::settings::SettingsStore>) -> Result<crate::phone_run::RunInfo, String> {
    let s = slot_of(slot)?;
    let folder = folder_of(&store, slot)?;
    blocking(move || crate::phone_run::run(&app, s, &folder, ios)).await?
}

#[tauri::command]
pub fn phone_reload(slot: u8, app: tauri::AppHandle) -> Result<(), String> {
    crate::phone_run::reload(&app, slot_of(slot)?)
}

#[tauri::command]
pub async fn phone_restart(slot: u8, app: tauri::AppHandle, store: State<'_, crate::settings::SettingsStore>) -> Result<crate::phone_run::RunInfo, String> {
    let s = slot_of(slot)?;
    let folder = folder_of(&store, slot)?;
    blocking(move || crate::phone_run::restart(&app, s, &folder)).await?
}

#[tauri::command]
pub async fn phone_run_stop(slot: u8, app: tauri::AppHandle) -> Result<(), String> {
    let s = slot_of(slot)?;
    blocking(move || crate::phone_run::stop(&app, s)).await
}

#[tauri::command]
pub fn phone_run_info(slot: u8, app: tauri::AppHandle) -> Result<crate::phone_run::RunInfo, String> {
    Ok(crate::phone_run::info(&app, slot_of(slot)?))
}
