//! The few things the BROWSER window asks of the pages' own engine, per platform: WebView2 on
//! Windows, WKWebView on macOS. Kept here so browser.rs stays the same on both.

/// Switches the platform's own phishing and malware warning on for a page, explicitly, and says
/// what the engine reports back (Castle's console): SmartScreen on Windows, Safari's fraudulent website
/// warning on macOS. Both are on by default; this makes sure, and proves it.
pub fn harden(view: &tauri::Webview) {
    #[cfg(windows)]
    let _ = view.with_webview(|platform| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings8;
        use windows::core::Interface;
        // SAFETY: the controller is the live page's own, used on the window's thread inside with_webview
        let on = unsafe {
            platform.controller().CoreWebView2().and_then(|core| core.Settings()).and_then(|s| s.cast::<ICoreWebView2Settings8>()).and_then(|s| {
                s.SetIsReputationCheckingRequired(true)?;
                let mut on = windows::core::BOOL::default();
                s.IsReputationCheckingRequired(&mut on)?;
                Ok(on.as_bool())
            })
        };
        eprintln!("MemoryCore Castle: browser: SmartScreen on a page: {}", match on {
            Ok(true) => "on",
            Ok(false) => "OFF (WebView2 refused it)",
            Err(_) => "unknown (this WebView2 is too old to say)",
        });
    });
    #[cfg(target_os = "macos")]
    let _ = view.with_webview(|platform| {
        use objc2_web_kit::WKWebView;
        // SAFETY: inner() is the live WKWebView, used on the main thread inside with_webview
        unsafe {
            let web = &*(platform.inner() as *const WKWebView);
            let prefs = web.configuration().preferences();
            prefs.setFraudulentWebsiteWarningEnabled(true);
            let on = prefs.isFraudulentWebsiteWarningEnabled();
            eprintln!("MemoryCore Castle: browser: Safari's fraud warning on a page: {}", if on { "on" } else { "OFF" });
        }
    });
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = view;
}

/// Gives a page a user agent, live, and reloads it so the site serves the layout for it: `Some` a
/// phone's, `None` the engine's own again. The page is not rebuilt, so its sign-ins stay.
pub fn set_user_agent(view: &tauri::Webview, label: String, ua: Option<&'static str>) {
    #[cfg(windows)]
    let _ = view.with_webview(move |platform| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2;
        use windows::core::{Interface, HSTRING, PWSTR};
        // SAFETY: the live page's own settings, used on the window's thread inside with_webview
        unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else { return };
            let Ok(settings) = core.Settings().and_then(|s| s.cast::<ICoreWebView2Settings2>()) else { return };
            // the engine's own agent is read once, before the first change, to come back to
            let own = {
                let mut kept = OWN_AGENTS.lock().unwrap();
                if !kept.contains_key(&label) {
                    let mut raw = PWSTR::null();
                    if settings.UserAgent(&mut raw).is_ok() && !raw.is_null() {
                        // take_pwstr frees the engine's string as it reads it
                        kept.insert(label.clone(), webview2_com::take_pwstr(raw));
                    }
                }
                kept.get(&label).cloned().unwrap_or_default()
            };
            let next = ua.map(str::to_string).unwrap_or(own);
            if settings.SetUserAgent(&HSTRING::from(next.as_str())).is_ok() {
                let _ = core.Reload();
            }
        }
    });
    #[cfg(target_os = "macos")]
    let _ = view.with_webview(move |platform| {
        use objc2_foundation::NSString;
        use objc2_web_kit::WKWebView;
        let _ = &label;
        // SAFETY: inner() is the live WKWebView, used on the main thread inside with_webview
        unsafe {
            let web = &*(platform.inner() as *const WKWebView);
            web.setCustomUserAgent(ua.map(NSString::from_str).as_deref());
            web.reload();
        }
    });
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (view, label, ua);
}

/// Each page's own user agent on Windows, read before the phone layout first changed it.
#[cfg(windows)]
static OWN_AGENTS: std::sync::Mutex<std::collections::BTreeMap<String, String>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// A closed page's kept agent goes with it.
pub fn forget(label: &str) {
    #[cfg(windows)]
    OWN_AGENTS.lock().unwrap().remove(label);
    #[cfg(not(windows))]
    let _ = label;
}
