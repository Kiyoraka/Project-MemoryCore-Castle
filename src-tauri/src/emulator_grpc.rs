//! The docked phone's wire (S-1124-5, his pick mobile-emulator-1): the Android emulator's own gRPC
//! channel (`proto/emulator_controller.proto`, Apache-2.0, shipped in the SDK's `emulator/lib`).
//! Its screen streams as RGBA frames scaled to the panel's width, and touches and keys go back the
//! same way, with the token the emulator wrote for this run. Local only: 127.0.0.1.
//! Lifted from the Alice Keep (MIT) with three changes: the last frame inside the rate limit is held
//! and sent (the emulator only sends a frame when the screen changes, so a dropped last frame would
//! leave the panel stale); one connection per phone serves every touch and key; and a stream that
//! breaks tells the panel why instead of going quiet.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::Endpoint;
use tonic::{Request, Status};

#[allow(clippy::all, dead_code)]
pub mod proto {
    tonic::include_proto!("android.emulation.control");
}

use proto::emulator_controller_client::EmulatorControllerClient;
use proto::image_format::ImgFormat;
use proto::keyboard_event::KeyEventType;
use proto::{ImageFormat, KeyboardEvent, Touch, TouchEvent};

/// The most frames a second the panel is sent; the newest frame wins, and the last one is never lost.
pub const MOST_PER_SECOND: u64 = 30;
/// The widest frame asked for, whatever the panel's width.
pub const WIDEST: u32 = 540;

/// Where one running phone listens, and its token.
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    pub port: u16,
    pub token: String,
}

/// The token on every call.
#[derive(Clone)]
pub struct Bearer(MetadataValue<Ascii>);

impl Interceptor for Bearer {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        req.metadata_mut().insert("authorization", self.0.clone());
        Ok(req)
    }
}

type Client = EmulatorControllerClient<InterceptedService<tonic::transport::Channel, Bearer>>;

/// One client per running phone (by port and token): a tonic channel is cheap to clone and connects
/// when first used, so a drag's touches share one connection instead of opening one each.
fn clients() -> &'static Mutex<HashMap<(u16, String), Client>> {
    static CLIENTS: OnceLock<Mutex<HashMap<(u16, String), Client>>> = OnceLock::new();
    CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn client(wire: &Wire) -> Result<Client, String> {
    let key = (wire.port, wire.token.clone());
    let mut all = clients().lock().map_err(|_| "The phone's wire is busy.".to_string())?;
    if let Some(found) = all.get(&key) {
        return Ok(found.clone());
    }
    let channel = Endpoint::from_shared(format!("http://127.0.0.1:{}", wire.port))
        .map_err(|_| "The phone's address is not right.".to_string())?
        .connect_timeout(Duration::from_secs(5))
        .connect_lazy();
    let bearer: MetadataValue<Ascii> = format!("Bearer {}", wire.token).parse().map_err(|_| "The phone's token is not right.".to_string())?;
    let made = EmulatorControllerClient::with_interceptor(channel, Bearer(bearer)).max_decoding_message_size(32 * 1024 * 1024);
    all.retain(|(port, _), _| *port != wire.port); // an old run's client on the same port is gone
    all.insert(key, made.clone());
    Ok(made)
}

/// Forgets a phone's client when it stops.
pub fn forget(port: u16) {
    if let Ok(mut all) = clients().lock() {
        all.retain(|(p, _), _| *p != port);
    }
}

/// A frame as the panel reads it: width and height (u32, little-endian) and then the RGBA bytes.
pub fn framed(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(rgba);
    out
}

/// What the panel is told when the screen stops for a reason (a JSON message, never a frame).
pub fn said(error: &str) -> String {
    serde_json::json!({ "error": error }).to_string()
}

/// The width asked of the emulator: the panel's, at least 120, at most `WIDEST`. The emulator scales
/// only when a height bound is given too, so every ask carries three times the width as its height
/// (taller than any phone): the width decides, the phone's own shape is kept.
pub fn asked_width(panel: u32) -> u32 {
    panel.clamp(120, WIDEST)
}

/// The DOM key names the emulator takes for the phone's own buttons.
pub fn button_key(name: &str) -> Option<&'static str> {
    match name {
        "back" => Some("GoBack"),
        "home" => Some("GoHome"),
        "recent" => Some("AppSwitch"),
        "power" => Some("Power"),
        "volume-up" => Some("AudioVolumeUp"),
        "volume-down" => Some("AudioVolumeDown"),
        "enter" => Some("Enter"),
        "backspace" => Some("Backspace"),
        _ => None,
    }
}

/// A frame worth showing: the display is on and the bytes match the size.
fn frame_of(image: &proto::Image) -> Option<Vec<u8>> {
    let format = image.format.clone().unwrap_or_default();
    let (w, h) = (format.width, format.height);
    (w > 0 && h > 0 && image.image.len() == (w as usize) * (h as usize) * 4).then(|| framed(w, h, &image.image))
}

/// The rate limit's choice for one arriving frame: send it now, or hold it until the gap is over.
pub fn send_now(since_last: Duration, gap: Duration) -> bool {
    since_last >= gap
}

/// Streams the phone's screen into the panel's channel until the panel goes, the phone stops, or a
/// newer stream for the same panel starts (`generation` moved on). A frame that arrives inside the
/// rate limit is held and sent when the gap is over, unless a newer one replaces it first.
pub async fn stream(wire: Wire, width: u32, frames: Channel<InvokeResponseBody>, generation: Arc<AtomicU64>, mine: u64) -> Result<(), String> {
    let mut phone = client(&wire)?;
    let ask = ImageFormat { format: ImgFormat::Rgba8888 as i32, width: asked_width(width), height: asked_width(width) * 3, ..Default::default() };
    let mut incoming = phone.stream_screenshot(ask).await.map_err(|s| format!("The phone refused its screen: {}", s.message()))?.into_inner();
    let gap = Duration::from_millis(1000 / MOST_PER_SECOND);
    let mut last = Instant::now() - gap;
    let mut held: Option<Vec<u8>> = None;
    while generation.load(Ordering::SeqCst) == mine {
        let wait = gap.saturating_sub(last.elapsed());
        let arrived = tokio::select! {
            got = incoming.message() => Some(got),
            _ = tokio::time::sleep(wait), if held.is_some() => None,
        };
        let frame = match arrived {
            None => held.take(),
            Some(Ok(Some(image))) => match frame_of(&image) {
                Some(frame) if send_now(last.elapsed(), gap) => {
                    held = None;
                    Some(frame)
                }
                Some(frame) => {
                    held = Some(frame); // the newest waits for the gap; an older held one is dropped
                    None
                }
                None => None, // the display is off
            },
            Some(Ok(None)) => break, // the phone ended the stream
            Some(Err(_)) => return Err("The phone's screen stopped.".to_string()),
        };
        if let Some(frame) = frame {
            if frames.send(InvokeResponseBody::Raw(frame)).is_err() {
                break; // the panel is gone
            }
            last = Instant::now();
        }
    }
    Ok(())
}

/// One frame of the phone's screen: width, height and its byte count (a check that the wire works).
#[cfg(test)]
pub async fn one_frame(wire: Wire, width: u32) -> Result<(u32, u32, usize), String> {
    let mut phone = client(&wire)?;
    let ask = ImageFormat { format: ImgFormat::Rgba8888 as i32, width: asked_width(width), height: asked_width(width) * 3, ..Default::default() };
    let image = phone.get_screenshot(ask).await.map_err(|s| s.message().to_string())?.into_inner();
    let format = image.format.unwrap_or_default();
    Ok((format.width, format.height, image.image.len()))
}

/// One finger: down (pressure 1) or up (0) at a point on the phone's own screen.
pub async fn touch(wire: Wire, x: i32, y: i32, down: bool) -> Result<(), String> {
    let mut phone = client(&wire)?;
    let event = TouchEvent { touches: vec![Touch { x, y, identifier: 0, pressure: i32::from(down), ..Default::default() }], ..Default::default() };
    phone.send_touch(event).await.map(|_| ()).map_err(|s| s.message().to_string())
}

/// A key press: one of the phone's buttons by name, or text typed into it.
pub async fn key(wire: Wire, button: Option<&'static str>, text: Option<String>) -> Result<(), String> {
    let mut phone = client(&wire)?;
    let event = KeyboardEvent { event_type: KeyEventType::Keypress as i32, key: button.unwrap_or_default().to_string(), text: text.unwrap_or_default(), ..Default::default() };
    phone.send_key(event).await.map(|_| ()).map_err(|s| s.message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_carries_its_size_first() {
        let f = framed(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&f[0..4], &2u32.to_le_bytes());
        assert_eq!(&f[4..8], &1u32.to_le_bytes());
        assert_eq!(f.len(), 16);
    }

    #[test]
    fn a_torn_or_dark_frame_is_not_shown() {
        let ok = proto::Image { format: Some(ImageFormat { width: 1, height: 1, ..Default::default() }), image: vec![9, 9, 9, 255], ..Default::default() };
        assert_eq!(frame_of(&ok).map(|f| f.len()), Some(12));
        let torn = proto::Image { format: Some(ImageFormat { width: 2, height: 1, ..Default::default() }), image: vec![9, 9, 9, 255], ..Default::default() };
        assert_eq!(frame_of(&torn), None);
        assert_eq!(frame_of(&proto::Image::default()), None);
    }

    #[test]
    fn a_frame_inside_the_gap_waits_instead_of_being_lost() {
        let gap = Duration::from_millis(33);
        assert!(send_now(Duration::from_millis(40), gap));
        assert!(!send_now(Duration::from_millis(10), gap));
    }

    #[test]
    fn one_phone_keeps_one_client_and_a_stopped_one_is_forgotten() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let wire = Wire { port: 65001, token: "abc".into() };
            client(&wire).unwrap();
            client(&wire).unwrap();
            let count = |port: u16| clients().lock().unwrap().keys().filter(|(p, _)| *p == port).count();
            assert_eq!(count(65001), 1);
            client(&Wire { port: 65001, token: "new run".into() }).unwrap();
            assert_eq!(count(65001), 1, "a new run on the same port replaces the old client");
            forget(65001);
            assert_eq!(count(65001), 0);
        });
    }

    #[test]
    fn the_width_the_buttons_and_the_error_are_bounded() {
        assert_eq!(asked_width(420), 420);
        assert_eq!(asked_width(4000), WIDEST);
        assert_eq!(asked_width(10), 120);
        assert_eq!(button_key("back"), Some("GoBack"));
        assert_eq!(button_key("home"), Some("GoHome"));
        assert_eq!(button_key("format c:"), None);
        assert_eq!(said("gone"), r#"{"error":"gone"}"#);
    }
}
