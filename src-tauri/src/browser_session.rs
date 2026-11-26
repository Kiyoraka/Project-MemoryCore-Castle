//! BROWSER brings its tabs back, like Chrome (S-1126-4, mapped from the Alice Keep): every normal tab's
//! address and title, in order, which one was in front and which were side by side, kept beside Castle's
//! settings as they change - so closing the browser window, quitting, a crash or an update all bring them
//! back. A private tab is never written anywhere, and nothing is kept while Remember sign-ins is off. A tab
//! brought back has no page until it is shown, so many tabs never load at once. The member's switch in
//! Settings > Rooms > Browser turns it off, and then the file is gone.
//!
//! Three moments empty the tabs without meaning to forget them: Castle leaving (a quit, an update), the
//! browser's own window closing (its pages go with it), and Browser opens changing (every page closes to
//! move to its new home). Each pauses the session first; the next time the browser comes up it resumes,
//! and the tabs paused come back.

use crate::browser_tabs::{may_load, Tab, Tabs, HOME, MAX_PANES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// The file beside Castle's settings.
pub const FILE: &str = "browser-session.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    /// The tab lived in a window of its own: when only the home closed, it is still open and stays out
    /// of what comes back.
    pub out: bool,
}

/// What comes back: the normal tabs in order, and as indexes of that list the tab in front and the
/// group side by side (with its column widths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Saved {
    pub tabs: Vec<SavedTab>,
    pub active: Option<usize>,
    pub panes: Vec<usize>,
    pub widths: Vec<f64>,
}

/// Why the session is paused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pause {
    /// Only the home's tabs went (its window closed); tabs out in windows of their own are still open.
    Home,
    /// Every tab went (Castle starting, Browser opens changed).
    All,
}

/// The tabs as they would be kept: normal tabs with an address that may load (a private or an empty tab
/// left out), in tab order, with the front tab and the group turned into indexes of that list. Pure.
pub fn snapshot(tabs: &Tabs) -> Saved {
    let kept: Vec<&Tab> = tabs.list.iter().filter(|t| !t.private && t.url.as_deref().is_some_and(|u| u != "about:blank" && may_load(u))).collect();
    let index = |id: u32| kept.iter().position(|t| t.id == id);
    let panes: Vec<usize> = tabs.panes.iter().filter_map(|id| index(*id)).collect();
    let widths = if panes.len() == tabs.panes.len() && tabs.widths.len() == tabs.panes.len() { tabs.widths.clone() } else { Vec::new() };
    Saved {
        tabs: kept.iter().map(|t| SavedTab { url: t.url.clone().unwrap_or_default(), title: t.title.clone(), out: t.window != HOME }).collect(),
        active: tabs.active.and_then(index),
        panes,
        widths,
    }
}

/// While only the home is closed: the home's tabs as they were when it closed, then the tabs out now
/// (they may still open, close or move). The front tab and the group follow the home's tabs. Pure.
pub fn merge_home(frozen: &Saved, now: &Saved) -> Saved {
    let mut tabs = Vec::new();
    let mut moved: Vec<Option<usize>> = Vec::new();
    for t in &frozen.tabs {
        if t.out {
            moved.push(None);
        } else {
            moved.push(Some(tabs.len()));
            tabs.push(t.clone());
        }
    }
    tabs.extend(now.tabs.iter().filter(|t| t.out).cloned());
    let at = |i: usize| moved.get(i).copied().flatten();
    let panes: Vec<usize> = frozen.panes.iter().filter_map(|i| at(*i)).collect();
    let widths = if panes.len() == frozen.panes.len() { frozen.widths.clone() } else { Vec::new() };
    Saved { tabs, active: frozen.active.and_then(at), panes, widths }
}

/// The saved tabs opened again in the home, each with a fresh id, its address and title, and no page yet;
/// an address that may no longer load is left out, and never past twelve. With `home_only` the tabs that
/// were out are left out too (they are still open). The front tab and a group of two to four come back.
/// Pure.
pub fn restore_into(tabs: &mut Tabs, saved: &Saved, home_only: bool) {
    let mut ids: Vec<Option<u32>> = Vec::new();
    for s in &saved.tabs {
        if (home_only && s.out) || !may_load(&s.url) {
            ids.push(None);
            continue;
        }
        let id = tabs.open_with(Some(s.url.clone()), false);
        if let (Some(id), false) = (id, s.title.trim().is_empty()) {
            if let Some(tab) = tabs.get_mut(id) {
                tab.title = s.title.clone();
            }
        }
        ids.push(id);
    }
    let pick = |i: usize| ids.get(i).copied().flatten();
    if let Some(active) = saved.active.and_then(pick) {
        tabs.active = Some(active);
    }
    let panes: Vec<u32> = saved.panes.iter().filter_map(|i| pick(*i)).collect();
    if tabs.panes.is_empty() && (2..=MAX_PANES).contains(&panes.len()) {
        tabs.widths = if panes.len() == saved.panes.len() && saved.widths.len() == panes.len() {
            saved.widths.clone()
        } else {
            vec![1.0 / panes.len() as f64; panes.len()]
        };
        tabs.panes = panes;
    }
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    let store = app.try_state::<crate::settings::SettingsStore>()?;
    Some(store.folder().join(FILE))
}

/// The member's switch on, and pages allowed to remember at all (Remember sign-ins off keeps nothing,
/// as a private tab keeps nothing).
fn wanted(app: &AppHandle) -> bool {
    app.try_state::<crate::settings::SettingsStore>()
        .map(|s| {
            let now = s.current.lock().unwrap();
            now.browser_restore && now.browser_remember
        })
        .unwrap_or(false)
}

fn read(app: &AppHandle) -> Option<Saved> {
    let text = std::fs::read_to_string(file(app)?).ok()?;
    serde_json::from_str(&text).ok()
}

/// Set once Castle is leaving (a quit, an update): the windows closing on the way out end their tabs,
/// and that is never what the member left.
static LEAVING: AtomicBool = AtomicBool::new(false);
/// The pause, and the tabs as they stood when it began.
static PAUSED: Mutex<Option<(Pause, Saved)>> = Mutex::new(None);
/// The last snapshot written, so a page's title changing alone never rewrites the file needlessly.
static LAST: Mutex<Option<Saved>> = Mutex::new(None);

/// Castle is leaving: nothing more is kept until it opens again.
pub fn leave() {
    LEAVING.store(true, Ordering::SeqCst);
}

/// Castle stays after all (an update that failed to start): keeping goes on.
pub fn stay() {
    LEAVING.store(false, Ordering::SeqCst);
}

/// At start-up, with the switch on: the tabs left last time wait for the browser to come up.
pub fn start(app: &AppHandle) {
    if !wanted(app) {
        return;
    }
    if let Some(saved) = read(app) {
        *LAST.lock().unwrap() = Some(saved.clone());
        *PAUSED.lock().unwrap() = Some((Pause::All, saved));
    }
}

/// The tabs are about to go without being closed by the member (called before they go). A pause already
/// running keeps what it froze; a home pause grows into a whole one.
pub fn pause(app: &AppHandle, why: Pause) {
    if !wanted(app) || LEAVING.load(Ordering::SeqCst) {
        return;
    }
    let mut paused = PAUSED.lock().unwrap();
    match paused.as_mut() {
        Some((kind, _)) => {
            if why == Pause::All {
                *kind = Pause::All;
            }
        }
        None => *paused = Some((why, snapshot(&crate::browser::tabs(app)))),
    }
}

/// The browser came up: the paused tabs come back (no page loads until one is shown). True when any did.
pub fn resume(app: &AppHandle) -> bool {
    let Some((kind, saved)) = PAUSED.lock().unwrap().take() else { return false };
    if !wanted(app) {
        return false;
    }
    let now = {
        let held = app.state::<crate::browser::Browser>();
        let mut tabs = held.0.lock().unwrap();
        restore_into(&mut tabs, &saved, kind == Pause::Home);
        tabs.clone()
    };
    keep(app, &now);
    true
}

/// Keeps the tabs as they stand now (called on every change the browser announces). Written only when it
/// changed, through a temp file and a rename; with the switch off the file is removed and nothing is
/// written.
pub fn keep(app: &AppHandle, tabs: &Tabs) {
    if LEAVING.load(Ordering::SeqCst) {
        return;
    }
    let Some(path) = file(app) else { return };
    if !wanted(app) {
        *PAUSED.lock().unwrap() = None;
        if LAST.lock().unwrap().take().is_some() || path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        return;
    }
    let now = match PAUSED.lock().unwrap().as_ref() {
        None => snapshot(tabs),
        Some((Pause::Home, frozen)) => merge_home(frozen, &snapshot(tabs)),
        Some((Pause::All, _)) => return,
    };
    {
        let mut last = LAST.lock().unwrap();
        if last.as_ref() == Some(&now) {
            return;
        }
        *last = Some(now.clone());
    }
    let Ok(text) = serde_json::to_string_pretty(&now) else { return };
    let temp = path.with_extension("json.partial");
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(&temp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// github (1), a private bank (2), an empty tab (3), the star (4) side by side with github, and a
    /// docs tab out in a window of its own (5).
    fn five() -> Tabs {
        let mut tabs = Tabs::default();
        tabs.open(Some("https://github.com/".into()));
        tabs.open_with(Some("https://bank.example/secret".into()), true);
        tabs.open(None);
        tabs.open(Some("https://www.thestar.com.my/".into()));
        tabs.open(Some("https://docs.example/".into()));
        tabs.get_mut(4).unwrap().title = "The Star".into();
        tabs.get_mut(5).unwrap().window = "browser-out-1".into();
        tabs.panes = vec![1, 4];
        tabs.widths = vec![0.6, 0.4];
        tabs.active = Some(4);
        tabs
    }

    #[test]
    fn a_private_tab_and_an_empty_one_are_never_kept() {
        let saved = snapshot(&five());
        assert_eq!(saved.tabs.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec!["https://github.com/", "https://www.thestar.com.my/", "https://docs.example/"]);
        assert_eq!(saved.tabs[1].title, "The Star");
        assert_eq!(saved.tabs.iter().map(|t| t.out).collect::<Vec<_>>(), vec![false, false, true]);
        assert_eq!(saved.active, Some(1));
        assert_eq!(saved.panes, vec![0, 1]);
        assert_eq!(saved.widths, vec![0.6, 0.4]);
        let mut private_front = five();
        private_front.active = Some(2);
        private_front.panes = vec![1, 2];
        let kept = snapshot(&private_front);
        assert_eq!(kept.active, None, "a private tab in front is never pointed at");
        assert_eq!(kept.panes, vec![0]);
        assert!(kept.widths.is_empty(), "widths that no longer fit the group are dropped");
    }

    #[test]
    fn the_tabs_come_back_with_fresh_ids_titles_the_front_and_the_group() {
        let saved = snapshot(&five());
        let mut fresh = Tabs::default();
        let gone = fresh.open(None).unwrap();
        fresh.close(gone);
        restore_into(&mut fresh, &saved, false);
        assert_eq!(fresh.list.len(), 3);
        assert!(fresh.list.iter().all(|t| t.id > gone && !t.private && t.window == HOME), "fresh ids, all home, none private");
        assert_eq!(fresh.list[1].title, "The Star");
        assert_eq!(fresh.active, Some(fresh.list[1].id));
        assert_eq!(fresh.panes, vec![fresh.list[0].id, fresh.list[1].id]);
        assert_eq!(fresh.widths, vec![0.6, 0.4]);
    }

    #[test]
    fn when_only_the_home_closed_the_tabs_out_are_not_opened_twice() {
        let saved = snapshot(&five());
        let mut still = Tabs::default();
        restore_into(&mut still, &saved, true);
        assert_eq!(still.list.iter().map(|t| t.url.as_deref().unwrap()).collect::<Vec<_>>(), vec!["https://github.com/", "https://www.thestar.com.my/"]);
    }

    #[test]
    fn a_closed_home_keeps_its_tabs_while_the_tabs_out_go_on_changing() {
        let frozen = snapshot(&five());
        let mut later = five();
        let _ = later.keep_out_only();
        later.open(Some("https://news.example/".into()));
        let id = later.active.unwrap();
        later.get_mut(id).unwrap().window = "browser-out-2".into();
        let merged = merge_home(&frozen, &snapshot(&later));
        assert_eq!(merged.tabs.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec!["https://github.com/", "https://www.thestar.com.my/", "https://docs.example/", "https://news.example/"]);
        assert_eq!(merged.active, Some(1));
        assert_eq!(merged.panes, vec![0, 1]);
    }

    #[test]
    fn never_past_twelve_and_never_an_address_that_may_not_load() {
        let saved = Saved {
            tabs: (0..15).map(|i| SavedTab { url: format!("https://site{i}.example/"), ..Default::default() }).chain([SavedTab { url: "file:///C:/secret.txt".into(), title: "x".into(), out: false }]).collect(),
            active: Some(15),
            panes: vec![0, 15],
            widths: vec![0.5, 0.5],
        };
        let mut tabs = Tabs::default();
        restore_into(&mut tabs, &saved, false);
        assert_eq!(tabs.list.len(), crate::browser_tabs::MAX_TABS);
        assert!(tabs.list.iter().all(|t| t.url.as_deref().unwrap().starts_with("https://")));
        assert!(tabs.panes.is_empty(), "one page left of a group is no group");
    }

    #[test]
    fn an_old_or_broken_file_reads_as_nothing() {
        assert_eq!(serde_json::from_str::<Saved>("{}").unwrap(), Saved::default());
        assert!(serde_json::from_str::<Saved>("not json").is_err());
    }
}
