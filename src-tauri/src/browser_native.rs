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
