//! `shot`: the attached game's window as a PNG in %TEMP%\worldsmith-shots.
//! Windows.Graphics.Capture first (works for DirectX games and hidden-behind windows); when that
//! fails (old Windows, minimised window, no GPU device) a GDI PrintWindow, then a BitBlt of the
//! window's monitor.

use std::path::PathBuf;
use windows_sys::Win32::Foundation::{HWND, RECT};
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

/// Capture `hwnd` and save it; returns the PNG path and how it was captured.
pub fn shot(hwnd: HWND) -> Result<(String, &'static str), String> {
    let (img, via) = match wgc::capture(hwnd) {
        Ok(img) if looks_real(&img) => (img, "graphics-capture"),
        _ => (gdi_capture(hwnd)?, "gdi"),
    };
    Ok((save(&img)?, via))
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

fn gdi_capture(hwnd: HWND) -> Result<Image, String> {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut r);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let mut result = None;
        if w > 0 && h > 0 {
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp);
            // PW_RENDERFULLCONTENT (2) also draws DirectX content on Windows 8.1+
            if PrintWindow(hwnd, mem, 2) != 0 {
                result = read_bitmap(mem, bmp, w, h).filter(looks_real);
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
                        result = read_bitmap(mem, bmp, mw, mh);
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
