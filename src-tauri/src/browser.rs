//! The BROWSER window (CASTLE-12): web pages beside Castle, on the right half of the screen.
//!
//! One native window holds two kinds of webview:
//! - `browser-ui`, Castle's own page (index.html, `{ view: 'browser' }`): the tabs and the toolbar,
//!   or the new-tab page. It is the only one a capability names (capabilities/browser.json).
//! - `browser-tab-<id>`, one per open web page. No capability names them, so a page can never call
//!   Castle's core; they only ever load http(s) (`browser_tabs::may_load`), and they keep their
//!   cookies and sign-ins in a store of their own, apart from Castle's (so clearing it never
//!   touches Castle, and Castle's own storage is never seen by a page).
//!
//! Links from the chats come through `link_open`, which follows Settings > Rooms > Browser.

use crate::browser_tabs::{self, LinkWay, Tabs};
use crate::settings::SettingsStore;
use std::sync::Mutex;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Rect, State, Url, WebviewBuilder, WebviewUrl, Window, WindowEvent};

pub const WINDOW: &str = "browser";
pub const UI: &str = "browser-ui";
/// The tabs and toolbar above a page, in logical pixels (the UI page draws exactly this tall).
const BAR: f64 = 94.0;
/// Where the Artifacts tile and the new-tab shortcut go (CASTLE-13).
pub const ARTIFACTS: &str = "https://claude.ai/code/artifacts";
/// The pages' own store on macOS (WKWebsiteDataStore): fixed, so sign-ins stay between runs.
const PAGE_STORE: [u8; 16] = *b"castle-browser01";

/// The window's tabs, kept while it is open.
#[derive(Default)]
pub struct Browser(pub Mutex<Tabs>);

fn tab_label(id: u32) -> String {
    format!("browser-tab-{id}")
}

fn tabs(app: &AppHandle) -> Tabs {
    app.state::<Browser>().0.lock().unwrap().clone()
}

/// Tells the toolbar what changed.
fn tell(app: &AppHandle) {
    let _ = app.emit_to(UI, "browser-state", tabs(app));
}

/// The window, opened on the right half of the screen Castle is on when it is not open yet.
fn window(app: &AppHandle) -> Result<Window, String> {
    if let Some(window) = app.get_window(WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(window);
    }
    let window = tauri::window::WindowBuilder::new(app, WINDOW)
        .title("MemoryCore · Browser")
        .min_inner_size(560.0, 420.0)
        .visible(false)
        .build()
        .map_err(|_| "The browser window could not be opened.".to_string())?;
    let monitor = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        let (x, y, w, h) = browser_tabs::right_half(area.position.x, area.position.y, area.size.width, area.size.height);
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.set_size(PhysicalSize::new(w, h));
    }
    let (w, h) = logical_size(&window);
    let ui = WebviewBuilder::new(UI, WebviewUrl::App("index.html".into())).initialization_script(UI_VIEW);
    window
        .add_child(ui, LogicalPosition::new(0.0, 0.0), LogicalSize::new(w, h))
        .map_err(|_| "The browser window could not be opened.".to_string())?;
    let events = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => layout(&events),
        WindowEvent::Destroyed => *events.state::<Browser>().0.lock().unwrap() = Tabs::default(),
        _ => {}
    });
    window.show().map_err(|_| "The browser window could not be shown.".to_string())?;
    let _ = window.set_focus();
    Ok(window)
}

/// Marks the UI page as the browser before it loads (main.js), with no start-up screen.
const UI_VIEW: &str = "window.__CASTLE_VIEW__ = { view: 'browser' };(function(){var m=function(){var d=document.documentElement;if(d){d.setAttribute('data-castle-bare','');}};m();new MutationObserver(m).observe(document,{childList:true});})();";

fn logical_size(window: &Window) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    window.inner_size().map(|s| s.to_logical::<f64>(scale)).map(|s| (s.width, s.height)).unwrap_or((960.0, 800.0))
}

/// The toolbar on top and the tab in front below it; a new tab (no page yet) shows the UI page
/// whole, with its new-tab page.
fn layout(app: &AppHandle) {
    let Some(window) = app.get_window(WINDOW) else { return };
    let (w, h) = logical_size(&window);
    let tabs = tabs(app);
    let page = tabs.current().is_some_and(|t| t.url.is_some());
    if let Some(ui) = app.get_webview(UI) {
        let _ = ui.set_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(w, if page { BAR } else { h }).into() });
    }
    for tab in &tabs.list {
        let Some(view) = app.get_webview(&tab_label(tab.id)) else { continue };
        if Some(tab.id) == tabs.active && tab.url.is_some() {
            let _ = view.set_bounds(Rect { position: LogicalPosition::new(0.0, BAR).into(), size: LogicalSize::new(w, (h - BAR).max(0.0)).into() });
            let _ = view.show();
        } else {
            let _ = view.hide();
        }
    }
}

/// The web page for tab `id`, made the first time the tab gets an address.
fn add_page(app: &AppHandle, window: &Window, id: u32, url: &str) -> Result<(), String> {
    let page: Url = url.parse().map_err(|_| "That address could not be opened.".to_string())?;
    let remember = app.state::<SettingsStore>().get().browser_remember;
    let titles = app.clone();
    let loads = app.clone();
    let opens = app.clone();
    let mut builder = WebviewBuilder::new(tab_label(id), WebviewUrl::External(page))
        .incognito(!remember)
        .on_navigation(|to| browser_tabs::may_load(to.as_str()))
        .on_document_title_changed(move |_, title| {
            if let Some(tab) = titles.state::<Browser>().0.lock().unwrap().get_mut(id) {
                tab.title = browser_tabs::tab_title(tab.url.as_deref(), Some(&title));
            }
            tell(&titles);
        })
        .on_page_load(move |_, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let now = payload.url().to_string();
                if let Some(tab) = loads.state::<Browser>().0.lock().unwrap().get_mut(id) {
                    tab.url = Some(now);
                }
                tell(&loads);
            }
        })
        // a page asking for a new window (target=_blank, window.open) gets a new tab instead
        .on_new_window(move |to, _| {
            let app = opens.clone();
            let to = to.to_string();
            if browser_tabs::may_load(&to) {
                tauri::async_runtime::spawn(async move {
                    let _ = open_in_tab(&app, Some(to));
                });
            }
            NewWindowResponse::Deny
        });
    if let Ok(folder) = app.path().app_local_data_dir() {
        builder = builder.data_directory(folder.join("browser-pages"));
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(PAGE_STORE);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = PAGE_STORE;
    let (w, h) = logical_size(window);
    window
        .add_child(builder, LogicalPosition::new(0.0, BAR), LogicalSize::new(w, (h - BAR).max(0.0)))
        .map_err(|_| "The page could not be opened.".to_string())?;
    Ok(())
}

/// A new tab (`url` None: the new-tab page), in front.
fn open_in_tab(app: &AppHandle, url: Option<String>) -> Result<(), String> {
    if let Some(url) = &url {
        if !browser_tabs::may_load(url) {
            return Err("That address could not be opened.".into());
        }
    }
    let window = window(app)?;
    let id = app.state::<Browser>().0.lock().unwrap().open(url.clone()).ok_or(format!("All {} tabs are open - close one first.", browser_tabs::MAX_TABS))?;
    if let Some(url) = url {
        if let Err(problem) = add_page(app, &window, id, &url) {
            app.state::<Browser>().0.lock().unwrap().close(id);
            tell(app);
            return Err(problem);
        }
    }
    layout(app);
    tell(app);
    Ok(())
}

/// A link clicked in a chat: where Settings says, `ask` for the chat's own menu.
#[tauri::command]
pub async fn link_open(app: AppHandle, url: String, store: State<'_, SettingsStore>) -> Result<String, String> {
    match browser_tabs::link_way(&store.get().link_open, &url) {
        LinkWay::Castle => open_in_tab(&app, Some(url)).map(|_| "castle".into()),
        LinkWay::System => outside(&url).map(|_| "system".into()),
        LinkWay::Ask => Ok("ask".into()),
        LinkWay::Refuse => Err("That link does not open from Castle.".into()),
    }
}

/// The member's own browser: https through open_url, a server on this computer through the DEV opener.
fn outside(url: &str) -> Result<(), String> {
    if url.starts_with("https://") {
        crate::commands::open_url(url.to_string())
    } else {
        crate::dev_git::dev_open_link(url.to_string())
    }
}

/// Opens the window: on a page (a link the member chose, the Artifacts tile) or a new tab.
#[tauri::command]
pub async fn browser_open(app: AppHandle, url: Option<String>) -> Result<(), String> {
    match url {
        Some(url) if browser_tabs::link_way("castle", &url) == LinkWay::Refuse => Err("That link does not open from Castle.".into()),
        url => open_in_tab(&app, url),
    }
}

#[tauri::command]
pub fn browser_state(state: State<'_, Browser>) -> Tabs {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
pub async fn browser_switch(app: AppHandle, id: u32) -> Result<(), String> {
    if !app.state::<Browser>().0.lock().unwrap().switch(id) {
        return Err("That tab is not open.".into());
    }
    layout(&app);
    tell(&app);
    Ok(())
}

/// Closes a tab and its page; closing the last one closes the window.
#[tauri::command]
pub async fn browser_close(app: AppHandle, id: u32) -> Result<(), String> {
    if let Some(view) = app.get_webview(&tab_label(id)) {
        let _ = view.close();
    }
    let empty = {
        let state = app.state::<Browser>();
        let mut tabs = state.0.lock().unwrap();
        tabs.close(id);
        tabs.list.is_empty()
    };
    if empty {
        if let Some(window) = app.get_window(WINDOW) {
            let _ = window.close();
        }
        return Ok(());
    }
    layout(&app);
    tell(&app);
    Ok(())
}

/// What was typed in the address bar, in the tab in front.
#[tauri::command]
pub async fn browser_go(app: AppHandle, input: String) -> Result<(), String> {
    let url = browser_tabs::address(&input).ok_or("Type an address or something to search for.")?;
    if !browser_tabs::may_load(&url) {
        return Err("That address could not be opened.".into());
    }
    let window = window(&app)?;
    let id = match app.state::<Browser>().0.lock().unwrap().current().map(|t| t.id) {
        Some(id) => id,
        None => return open_in_tab(&app, Some(url)),
    };
    match app.get_webview(&tab_label(id)) {
        Some(view) => view.navigate(url.parse().map_err(|_| "That address could not be opened.".to_string())?).map_err(|_| "That address could not be opened.".to_string())?,
        None => add_page(&app, &window, id, &url)?,
    }
    if let Some(tab) = app.state::<Browser>().0.lock().unwrap().get_mut(id) {
        tab.url = Some(url.clone());
        tab.title = browser_tabs::tab_title(Some(&url), None);
    }
    layout(&app);
    tell(&app);
    Ok(())
}

/// Back, forward or reload in the tab in front.
#[tauri::command]
pub async fn browser_step(app: AppHandle, step: String) -> Result<(), String> {
    let Some(id) = app.state::<Browser>().0.lock().unwrap().current().map(|t| t.id) else { return Ok(()) };
    let Some(view) = app.get_webview(&tab_label(id)) else { return Ok(()) };
    let done = match step.as_str() {
        "back" => view.eval("history.back()"),
        "forward" => view.eval("history.forward()"),
        "reload" => view.reload(),
        _ => return Err("Back, forward or reload.".into()),
    };
    done.map_err(|_| "The page did not answer.".to_string())
}

/// The page in front in the member's own browser.
#[tauri::command]
pub async fn browser_outside(url: String) -> Result<(), String> {
    match browser_tabs::link_way("system", &url) {
        LinkWay::Refuse => Err("That page does not open outside.".into()),
        _ => outside(&url),
    }
}

/// The page's address into HOME's chat box, for the companion.
#[tauri::command]
pub async fn browser_put_in_chat(app: AppHandle, url: String) -> Result<(), String> {
    if !browser_tabs::may_load(&url) || url == "about:blank" {
        return Err("There is no page to put in the chat.".into());
    }
    app.emit_to("main", "browser-put-in-chat", url).map_err(|_| "Castle's chat could not be reached.".to_string())?;
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
    Ok(())
}

/// Signs out of every site and clears the pages' history and storage: only the pages' own store,
/// never Castle's. Needs a page open to reach the store through.
#[tauri::command]
pub async fn browser_clear(app: AppHandle) -> Result<(), String> {
    let open = tabs(&app).list.iter().find_map(|t| app.get_webview(&tab_label(t.id)));
    let view = open.ok_or("Open a page in the browser window first, then clear it from there.")?;
    view.clear_all_browsing_data().map_err(|_| "The browsing data could not be cleared.".to_string())
}
