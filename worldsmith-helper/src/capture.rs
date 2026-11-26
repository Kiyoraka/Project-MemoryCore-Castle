//! `shot`: the attached game's window as a PNG in %TEMP%\worldsmith-shots; `thumb`: the same
//! picture as a 32x18 greyscale fingerprint, never saved.
//! Windows.Graphics.Capture first (works for DirectX games and hidden-behind windows); when that
//! fails (old Windows, minimised window, no GPU device) a GDI PrintWindow, then a BitBlt of the
//! window's monitor. Every capture also says where on the screen the picture sits (`rect`, in
//! physical pixels - the process is per-monitor DPI aware, see main.rs).

use std::path::PathBuf;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetMonitorInfoW, MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HDC, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

/// A BGRA image, top-down.
struct Image {
    w: u32,
    h: u32,
    bgra: Vec<u8>,
}

/// An area of the screen in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn from_win(r: &RECT) -> Rect {
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }
}

/// A saved shot: the PNG, how it was captured (`graphics-capture`, `gdi` or `screen`) and the
/// screen area the picture shows.
pub struct Shot {
    pub path: String,
    pub via: &'static str,
    pub rect: Rect,
}

/// A capture held in memory.
struct Grab {
    img: Image,
    via: &'static str,
    rect: Rect,
}

/// Capture `hwnd` and save it as a PNG.
pub fn shot(hwnd: HWND) -> Result<Shot, String> {
    let g = grab(hwnd)?;
    Ok(Shot { path: save(&g.img)?, via: g.via, rect: g.rect })
}

/// Capture `hwnd` without saving; returns the 32x18 fingerprint (see thumb.rs) and how it was
/// captured.
pub fn thumb(hwnd: HWND) -> Result<(String, &'static str), String> {
    let g = grab(hwnd)?;
    Ok((crate::thumb::encode(g.img.w as usize, g.img.h as usize, &g.img.bgra), g.via))
}

fn grab(hwnd: HWND) -> Result<Grab, String> {
    match wgc::capture(hwnd) {
        Ok(img) if looks_real(&img) => {
            // the frame is the window's visible frame (no drop shadow): DWM's extended frame
            // bounds give its place; the frame's own size wins if the two differ slightly
            let origin = frame_bounds(hwnd).or_else(|| window_rect(hwnd)).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
            let rect = Rect { x: origin.x, y: origin.y, w: img.w as i32, h: img.h as i32 };
            Ok(Grab { img, via: "graphics-capture", rect })
        }
        _ => gdi_capture(hwnd),
    }
}

/// The window's visible frame on screen (no invisible resize border, no shadow).
fn frame_bounds(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut r as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        );
        let rect = Rect::from_win(&r);
        (hr == 0 && rect.w > 0 && rect.h > 0).then_some(rect)
    }
}

fn window_rect(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        (GetWindowRect(hwnd, &mut r) != 0).then(|| Rect::from_win(&r))
    }
}

/// The part of `img` (which shows `outer` on screen) that shows `inner`, clipped to `outer`.
/// Returns the cropped picture and the area it now shows.
fn crop(img: Image, outer: Rect, inner: Rect) -> (Image, Rect) {
    let x0 = inner.x.max(outer.x);
    let y0 = inner.y.max(outer.y);
    let x1 = (inner.x + inner.w).min(outer.x + outer.w);
    let y1 = (inner.y + inner.h).min(outer.y + outer.h);
    if x1 <= x0 || y1 <= y0 || (x0, y0, x1, y1) == (outer.x, outer.y, outer.x + outer.w, outer.y + outer.h) {
        return (img, outer);
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let (dx, dy) = ((x0 - outer.x) as usize, (y0 - outer.y) as usize);
    let stride = img.w as usize * 4;
    let mut bgra = Vec::with_capacity(w * h * 4);
    for y in dy..dy + h {
        bgra.extend_from_slice(&img.bgra[y * stride + dx * 4..y * stride + (dx + w) * 4]);
    }
    (Image { w: w as u32, h: h as u32, bgra }, Rect { x: x0, y: y0, w: w as i32, h: h as i32 })
}

/// An all-black frame usually means the capture didn't really see the window.
fn looks_real(img: &Image) -> bool {
    img.w > 0 && img.h > 0 && img.bgra.chunks_exact(4).step_by(97).any(|p| p[0] | p[1] | p[2] != 0)
}

fn save(img: &Image) -> Result<String, String> {
    let dir: PathBuf = std::env::temp_dir().join("worldsmith-shots");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the shots folder: {e}"))?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("shot-{ms}.png"));
    let file = std::fs::File::create(&path).map_err(|e| format!("Couldn't save the shot: {e}"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.w, img.h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut rgb = Vec::with_capacity((img.w * img.h * 3) as usize);
    for p in img.bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[p[2], p[1], p[0]]);
    }
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .map_err(|e| format!("Couldn't save the shot: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Read a bitmap selected out of `mem` as top-down BGRA.
unsafe fn read_bitmap(mem: HDC, bmp: *mut core::ffi::c_void, w: i32, h: i32) -> Option<Image> {
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h; // top-down
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    let lines = GetDIBits(mem, bmp, 0, h as u32, bgra.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS);
    (lines == h).then_some(Image { w: w as u32, h: h as u32, bgra })
}

fn gdi_capture(hwnd: HWND) -> Result<Grab, String> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut r);
        let outer = Rect::from_win(&r);
        let (w, h) = (outer.w, outer.h);
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let mut result = None;
        if w > 0 && h > 0 {
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp);
            // PW_RENDERFULLCONTENT (2) also draws DirectX content on Windows 8.1+
            if PrintWindow(hwnd, mem, 2) != 0 {
                // the bitmap covers the window rect, invisible resize borders included: keep
                // only the visible frame, the same area Windows.Graphics.Capture shows
                result = read_bitmap(mem, bmp, w, h)
                    .map(|img| match frame_bounds(hwnd) {
                        Some(inner) => crop(img, outer, inner),
                        None => (img, outer),
                    })
                    .filter(|(img, _)| looks_real(img))
                    .map(|(img, rect)| Grab { img, via: "gdi", rect });
            }
            SelectObject(mem, old);
            DeleteObject(bmp);
        }
        if result.is_none() {
            // last resort: what the screen shows on the window's monitor
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(mon, &mut mi) != 0 {
                let m = mi.rcMonitor;
                let (mw, mh) = (m.right - m.left, m.bottom - m.top);
                if mw > 0 && mh > 0 {
                    let bmp = CreateCompatibleBitmap(screen, mw, mh);
                    let old = SelectObject(mem, bmp);
                    if BitBlt(mem, 0, 0, mw, mh, screen, m.left, m.top, SRCCOPY) != 0 {
                        result = read_bitmap(mem, bmp, mw, mh)
                            .map(|img| Grab { img, via: "screen", rect: Rect::from_win(&m) });
                    }
                    SelectObject(mem, old);
                    DeleteObject(bmp);
                }
            }
        }
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        result.ok_or_else(|| "Couldn't capture the game's window.".to_string())
    }
}

mod wgc {
    use super::Image;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows::core::Interface;
    use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    fn e(x: windows::core::Error) -> String {
        x.message().to_string()
    }

    /// The D3D11 device, made once and kept for the life of the process. Windows.Graphics.Capture
    /// keeps working on the device for a moment after a session closes; releasing the device right
    /// after a shot freed memory still in use (a crash seen on Windows 11), so it is never released.
    struct Gpu {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        d3d: IDirect3DDevice,
    }
    // D3D11 devices are free-threaded; the immediate context is only used under the mutex.
    unsafe impl Send for Gpu {}

    static GPU: Mutex<Option<Gpu>> = Mutex::new(None);

    unsafe fn make_gpu() -> Result<Gpu, String> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(e)?;
        let device = device.ok_or("no D3D11 device")?;
        let context = context.ok_or("no D3D11 context")?;
        let dxgi: IDXGIDevice = device.cast().map_err(e)?;
        let d3d: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).map_err(e)?.cast().map_err(e)?;
        Ok(Gpu { device, context, d3d })
    }

    pub fn capture(hwnd: *mut core::ffi::c_void) -> Result<Image, String> {
        let mut gpu = GPU.lock().unwrap_or_else(|p| p.into_inner());
        unsafe {
            // the calling thread joins the multithreaded apartment for good (never uninitialised)
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
            if gpu.is_none() {
                *gpu = Some(make_gpu()?);
            }
            let Gpu { device, context, d3d } = gpu.as_ref().unwrap();
            let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(e)?;
            let item: GraphicsCaptureItem = interop.CreateForWindow(HWND(hwnd)).map_err(e)?;

            let size = item.Size().map_err(e)?;
            if size.Width <= 0 || size.Height <= 0 {
                return Err("the window has no size".into());
            }
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                d3d,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                1,
                size,
            )
            .map_err(e)?;
            let session = pool.CreateCaptureSession(&item).map_err(e)?;
            let _ = session.SetIsCursorCaptureEnabled(false);
            let _ = session.SetIsBorderRequired(false);
            session.StartCapture().map_err(e)?;

            let start = Instant::now();
            let frame = loop {
                if let Ok(f) = pool.TryGetNextFrame() {
                    break Some(f);
                }
                if start.elapsed() > Duration::from_millis(1500) {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(15));
            };
            let result = (|| {
                let frame = frame.ok_or("no frame arrived")?;
                let access: IDirect3DDxgiInterfaceAccess = frame.Surface().map_err(e)?.cast().map_err(e)?;
                let tex: ID3D11Texture2D = access.GetInterface().map_err(e)?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                tex.GetDesc(&mut desc);
                desc.Usage = D3D11_USAGE_STAGING;
                desc.BindFlags = 0;
                desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                desc.MiscFlags = 0;
                desc.MipLevels = 1;
                desc.ArraySize = 1;
                let mut staging: Option<ID3D11Texture2D> = None;
                device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(e)?;
                let staging = staging.ok_or("no staging texture")?;
                context.CopyResource(&staging, &tex);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(e)?;
                let (w, h) = (desc.Width as usize, desc.Height as usize);
                if w == 0 || h == 0 {
                    context.Unmap(&staging, 0);
                    return Err("empty frame".to_string());
                }
                let pitch = mapped.RowPitch as usize;
                let src = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * (h - 1) + w * 4);
                let mut bgra = vec![0u8; w * h * 4];
                for y in 0..h {
                    bgra[y * w * 4..(y + 1) * w * 4].copy_from_slice(&src[y * pitch..y * pitch + w * 4]);
                }
                context.Unmap(&staging, 0);
                let _ = frame.Close();
                Ok(Image { w: w as u32, h: h as u32, bgra })
            })();
            let _ = session.Close();
            let _ = pool.Close();
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A w x h image whose pixel (x, y) has blue = x, green = y.
    fn grid(w: u32, h: u32) -> Image {
        let bgra = (0..h).flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, 0, 255])).collect();
        Image { w, h, bgra }
    }

    #[test]
    fn crop_keeps_the_visible_frame() {
        // window rect 100,50 10x8; frame inset 2 left/right, 0 top, 2 bottom
        let outer = Rect { x: 100, y: 50, w: 10, h: 8 };
        let inner = Rect { x: 102, y: 50, w: 6, h: 6 };
        let (img, rect) = crop(grid(10, 8), outer, inner);
        assert_eq!(rect, inner);
        assert_eq!((img.w, img.h), (6, 6));
        assert_eq!(img.bgra.len(), 6 * 6 * 4);
        assert_eq!(&img.bgra[..2], &[2, 0]); // top-left is source (2, 0)
        let last = (6 * 6 - 1) * 4;
        assert_eq!(&img.bgra[last..last + 2], &[7, 5]); // bottom-right is source (7, 5)
    }

    #[test]
    fn crop_clips_and_ignores_no_overlap() {
        let outer = Rect { x: 0, y: 0, w: 4, h: 4 };
        // a frame reaching past the bitmap is clipped to it
        let (img, rect) = crop(grid(4, 4), outer, Rect { x: 1, y: -3, w: 10, h: 5 });
        assert_eq!(rect, Rect { x: 1, y: 0, w: 3, h: 2 });
        assert_eq!((img.w, img.h, img.bgra.len()), (3, 2, 3 * 2 * 4));
        // no overlap, or the same area: unchanged
        let (img, rect) = crop(grid(4, 4), outer, Rect { x: 50, y: 50, w: 2, h: 2 });
        assert_eq!((rect, img.w, img.h), (outer, 4, 4));
        let (_, rect) = crop(grid(4, 4), outer, outer);
        assert_eq!(rect, outer);
    }
}
