//! The BROWSER window's plain rules, kept apart from the window itself so they are tested alone:
//! its tabs, what the address bar turns words into, which pages a tab may load, where a link from
//! a chat goes, and where the window sits on the screen (the right half).

use serde::Serialize;

/// The most tabs the window keeps open; one more replaces nothing and is refused.
pub const MAX_TABS: usize = 12;
/// The most pages side by side.
pub const MAX_PANES: usize = 4;
/// What a page is told it runs in when it is one of three or four side by side: Chrome on an
/// Android phone, so sites serve their phone layout.
pub const MOBILE_UA: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36";

/// One tab: `url` is None for a new tab (Castle's own new-tab page shows instead of a web page).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: u32,
    pub url: Option<String>,
    pub title: String,
    /// what the site check found for the page shown (site_rules.rs); None when nothing is wrong
    pub warning: Option<crate::site_rules::Warning>,
    /// a dangerous page held back before it loaded: the DANGER screen shows in its place
    pub danger: Option<Danger>,
    /// how many new things the site says it has, from its own title (unread_of); never kept
    pub unread: u32,
    /// the page is being shown its phone layout (three or four side by side)
    pub phone: bool,
}

/// A page the guard held back (browser_guard.rs): what was asked for and who caught it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Danger {
    /// the address that was held back
    pub url: String,
    /// phishing, malware, unwanted, harmful, or dangerous (a mark with no kind)
    pub threat: String,
    /// why, in one line
    pub reason: String,
    /// the guards that caught it: sentinel, lookalike, google, smartscreen
    pub caught_by: Vec<String>,
}

/// Every tab and the one in front, and the pages side by side: `panes` is the group (two to four
/// tab ids, empty for none), on screen while the tab in front is one of them, each column as wide as
/// its share in `widths`. `phone_view` (on by default) gives three or four pages their phone layout.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tabs {
    pub list: Vec<Tab>,
    pub active: Option<u32>,
    pub panes: Vec<u32>,
    pub widths: Vec<f64>,
    pub phone_view: bool,
    #[serde(skip)]
    next: u32,
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs { list: Vec::new(), active: None, panes: Vec::new(), widths: Vec::new(), phone_view: true, next: 0 }
    }
}

impl Tabs {
    /// A new tab at the end, in front; its id, or None when MAX_TABS are open.
    pub fn open(&mut self, url: Option<String>) -> Option<u32> {
        if self.list.len() >= MAX_TABS {
            return None;
        }
        self.next += 1;
        let id = self.next;
        let title = tab_title(url.as_deref(), None);
        self.list.push(Tab { id, url, title, warning: None, danger: None, unread: 0, phone: false });
        self.active = Some(id);
        Some(id)
    }
    /// The tab in front.
    pub fn current(&self) -> Option<&Tab> {
        self.active.and_then(|id| self.list.iter().find(|t| t.id == id))
    }
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Tab> {
        self.list.iter_mut().find(|t| t.id == id)
    }
    /// Brings a tab to the front; false when there is no such tab.
    pub fn switch(&mut self, id: u32) -> bool {
        let there = self.list.iter().any(|t| t.id == id);
        if there {
            self.active = Some(id);
        }
        there
    }
    /// The tabs on screen now, left to right: the group when the tab in front is in it, else that tab.
    pub fn on_screen(&self) -> Vec<u32> {
        match self.active {
            Some(id) if self.panes.contains(&id) => self.panes.clone(),
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
    /// Puts a tab beside the page in front: the two become a group, or the tab joins the group on
    /// screen, in a column of its own (the columns made equal). It comes to the front. A fifth is
    /// refused.
    pub fn place_beside(&mut self, id: u32) -> Result<(), String> {
        if !self.list.iter().any(|t| t.id == id) {
            return Err("That tab is not open.".into());
        }
        let front = self.active.ok_or("Open a page first.")?;
        if front == id && !self.panes.contains(&id) {
            return Err("Choose another tab to put beside this one.".into());
        }
        if !self.panes.contains(&front) {
            self.panes = vec![front];
        }
        if !self.panes.contains(&id) {
            if self.panes.len() >= MAX_PANES {
                return Err(format!("At most {MAX_PANES} pages fit side by side."));
            }
            self.panes.push(id);
        }
        self.widths = vec![1.0 / self.panes.len() as f64; self.panes.len()];
        self.active = Some(id);
        Ok(())
    }
    /// Takes a tab out of the group: it is a tab on its own again. One page left is no group.
    pub fn unpane(&mut self, id: u32) {
        if let Some(at) = self.panes.iter().position(|p| *p == id) {
            self.panes.remove(at);
            if self.panes.len() < 2 {
                self.panes.clear();
            }
            self.widths = vec![1.0 / self.panes.len().max(1) as f64; self.panes.len()];
        }
    }
    /// New column widths from the grips: one share per column, none below a tenth, made to add up
    /// to the whole. False when they do not fit the group.
    pub fn set_widths(&mut self, widths: &[f64]) -> bool {
        if widths.len() != self.panes.len() || widths.iter().any(|w| !w.is_finite() || *w < 0.1) {
            return false;
        }
        let total: f64 = widths.iter().sum();
        self.widths = widths.iter().map(|w| w / total).collect();
        true
    }
    /// Closes a tab; the one beside it comes to the front. True when it was there.
    pub fn close(&mut self, id: u32) -> bool {
        let Some(at) = self.list.iter().position(|t| t.id == id) else { return false };
        self.unpane(id);
        self.list.remove(at);
        if self.active == Some(id) {
            // the tab that took its place, else the one before it
            self.active = self.list.get(at).or_else(|| self.list.last()).map(|t| t.id);
        }
        true
    }
}

/// The page for what was typed in the address bar: an address as it is, a bare host gets https
/// (a server on this computer gets http), anything else is a web search. None for nothing typed.
pub fn address(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if (input.starts_with("https://") || input.starts_with("http://")) && !input.contains(char::is_whitespace) {
        return Some(input.to_string());
    }
    let local = input.starts_with("localhost:") || input.starts_with("127.0.0.1:");
    let host_like = !input.contains(char::is_whitespace)
        && !input.contains(':')
        && input.split('/').next().is_some_and(|h| h.contains('.') && !h.starts_with('.') && !h.ends_with('.'));
    if local && !input.contains(char::is_whitespace) {
        return Some(format!("http://{input}"));
    }
    if host_like {
        return Some(format!("https://{input}"));
    }
    Some(format!("https://duckduckgo.com/?q={}", search_words(input)))
}

/// The words typed, when the address bar made them a web search (trimmed, spaces made single, at
/// most 200 characters); None for an address. What the suggestions keep (browser_suggest.rs).
pub fn search_term(input: &str) -> Option<String> {
    let words = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let url = address(&words)?;
    url.starts_with("https://duckduckgo.com/?q=").then(|| words.chars().take(200).collect())
}

/// Words for a search address: letters, digits and `-_.~` as they are, a space as `+`, the rest
/// percent-encoded, so nothing typed can end the query or start another part of the address.
fn search_words(words: &str) -> String {
    let mut out = String::new();
    for byte in words.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A tab only ever loads web pages: http(s) and the blank page, never file:, javascript: or a
/// custom scheme that could reach the computer.
pub fn may_load(url: &str) -> bool {
    url == "about:blank" || ((url.starts_with("https://") || url.starts_with("http://")) && !url.contains(char::is_whitespace))
}

/// What a page's request for a new window (`target=_blank`, `window.open`) becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// a real small window joined to its page (`window.opener` works): a sign-in pop-up
    Popup,
    /// a new tab, as a plain link to another page
    Tab,
    /// nothing: not a web page
    Refuse,
}

/// The rule for a new window: a page that asked for a size or a position (what every Google /
/// Apple / Microsoft sign-in pop-up sends) gets a real small window, so the sign-in can hand its
/// answer back to the page; a plain link gets a tab. A site the quick checks flag (`flagged`) is
/// never a pop-up: it opens as a tab, where its warning shows.
pub fn new_window_kind(url: &str, asked_features: bool, flagged: bool) -> WindowKind {
    if !may_load(url) {
        WindowKind::Refuse
    } else if asked_features && !flagged {
        // about:blank too: a sign-in often opens a blank window first and sends it on after
        WindowKind::Popup
    } else {
        WindowKind::Tab
    }
}

/// A number in a title that is a count, not a year or a big figure: 1 to 9999, never 1900 to 2100.
fn count_of(digits: &str) -> Option<u32> {
    if digits.is_empty() || digits.len() > 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (n > 0 && !(1900..=2100).contains(&n)).then_some(n)
}

/// How many new things a site says it has, read from its own tab title (never from the page):
/// "(3) WhatsApp", "(9+) Facebook", "Inbox (12) - me@gmail.com - Gmail", "[4] Forum", and a bare
/// "•" or "*" in front (Discord's "unread, no number") as 1. A year or a price is never a count.
pub fn unread_of(title: &str) -> u32 {
    let t = title.trim();
    let bracketed = |open: char, close: char| -> Option<u32> {
        let rest = t.strip_prefix(open)?;
        let inner = &rest[..rest.find(close)?];
        count_of(inner.strip_suffix('+').unwrap_or(inner))
    };
    if let Some(n) = bracketed('(', ')').or_else(|| bracketed('[', ']')) {
        return n;
    }
    if let Some(at) = t.find("Inbox (") {
        let rest = &t[at + "Inbox (".len()..];
        if let Some(n) = rest.find(')').and_then(|end| count_of(rest[..end].trim_end_matches('+'))) {
            return n;
        }
    }
    if t.starts_with('•') || t.starts_with("* ") {
        return 1;
    }
    0
}

/// The site's short name for the unread summary: the title without its count, Gmail's last part.
pub fn unread_name(title: &str) -> String {
    let t = title.trim();
    let gmail = t.contains("Inbox (");
    let stripped = t.trim_start_matches(['•', '*']).trim_start();
    let stripped = match stripped.chars().next() {
        Some('(') | Some('[') if unread_of(stripped) > 0 => stripped.find([')', ']']).map(|end| stripped[end + 1..].trim()).unwrap_or(stripped),
        _ => stripped,
    };
    let joined = stripped.replace(" | ", " - ").replace(" – ", " - ");
    let parts: Vec<&str> = joined.split(" - ").map(str::trim).filter(|p| !p.is_empty()).collect();
    let name = if gmail { parts.last() } else { parts.first() }.copied().unwrap_or(stripped);
    name.chars().take(40).collect()
}

/// Every tab's news together, for the main window: the sum, and "WhatsApp 3 · Gmail 12".
pub fn unread_summary(tabs: &Tabs) -> (u32, String) {
    let with: Vec<&Tab> = tabs.list.iter().filter(|t| t.unread > 0).collect();
    let total = with.iter().map(|t| t.unread).sum();
    let title = with.iter().map(|t| format!("{} {}", unread_name(&t.title), t.unread)).collect::<Vec<_>>().join(" · ");
    (total, title)
}

/// The title a tab shows: the page's own, else its host, else "New tab".
pub fn tab_title(url: Option<&str>, page_title: Option<&str>) -> String {
    if let Some(title) = page_title.map(str::trim).filter(|t| !t.is_empty()) {
        return title.chars().take(120).collect();
    }
    url.and_then(|u| u.split("://").nth(1))
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .filter(|host| !host.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "New tab".to_string())
}

/// Where a link clicked in a chat goes, from Settings > Rooms > Browser (`link_open`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkWay {
    /// a new tab in the BROWSER window
    Castle,
    /// the member's own browser
    System,
    /// the chat shows a small menu to choose
    Ask,
    /// not a link Castle opens
    Refuse,
}

/// The way for one link: https pages and servers on this computer open, nothing else does.
pub fn link_way(setting: &str, url: &str) -> LinkWay {
    let web = url.starts_with("https://") && url.len() > "https://".len();
    let local = ["http://localhost:", "http://127.0.0.1:"].iter().any(|p| {
        url.strip_prefix(p).is_some_and(|rest| {
            let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
            !port.is_empty() && port.len() <= 5 && (rest.len() == port.len() || rest[port.len()..].starts_with('/'))
        })
    });
    let clean = url.len() <= 2000 && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'');
    if !(clean && (web || local)) {
        return LinkWay::Refuse;
    }
    match setting {
        "system" => LinkWay::System,
        "ask" => LinkWay::Ask,
        _ => LinkWay::Castle,
    }
}

/// Each column's left edge and width across `width`, from the shares in `widths` (one column for
/// none).
pub fn columns(width: f64, widths: &[f64]) -> Vec<(f64, f64)> {
    let shares: Vec<f64> = if widths.is_empty() { vec![1.0] } else { widths.to_vec() };
    let total: f64 = shares.iter().sum::<f64>().max(f64::EPSILON);
    let mut x = 0.0;
    shares
        .iter()
        .map(|s| {
            let w = (width * s / total).round();
            let col = (x, w);
            x += w;
            col
        })
        .collect()
}

/// Whether pages side by side get their phone layout: three or four on screen, while Phone view is on.
pub fn mobile_for(on_screen: usize, phone_view: bool) -> bool {
    phone_view && on_screen >= 3
}

/// The window's place: the right half of the screen's work area (x, y, width, height).
pub fn right_half(area_x: i32, area_y: i32, area_w: u32, area_h: u32) -> (i32, i32, u32, u32) {
    let left = area_w / 2;
    (area_x + left as i32, area_y, area_w - left, area_h)
}

/// The window's inner size (logical) that keeps the WHOLE window, title bar and frame included, inside
/// `area` (logical width, height): the area minus the frame, never below the window's minimum.
pub fn inner_within(area_w: f64, area_h: f64, frame_w: f64, frame_h: f64, min_w: f64, min_h: f64) -> (f64, f64) {
    ((area_w - frame_w.max(0.0)).max(min_w), (area_h - frame_h.max(0.0)).max(min_h))
}

/// Whether a window (outer box, physical x, y, w, h) reaches outside the work area (physical x, y, w, h).
pub fn sticks_out(win: (i32, i32, u32, u32), area: (i32, i32, u32, u32)) -> bool {
    let (wx, wy, ww, wh) = (win.0 as i64, win.1 as i64, win.2 as i64, win.3 as i64);
    let (ax, ay, aw, ah) = (area.0 as i64, area.1 as i64, area.2 as i64, area.3 as i64);
    wx < ax || wy < ay || wx + ww > ax + aw || wy + wh > ay + ah
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_open_switch_and_close() {
        let mut tabs = Tabs::default();
        let a = tabs.open(Some("https://a.example/".into())).unwrap();
        let b = tabs.open(None).unwrap();
        assert_ne!(a, b);
        assert_eq!(tabs.active, Some(b), "a new tab comes to the front");
        assert!(tabs.switch(a));
        assert_eq!(tabs.current().unwrap().url.as_deref(), Some("https://a.example/"));
        assert!(!tabs.switch(99));
        assert!(tabs.close(a));
        assert_eq!(tabs.active, Some(b), "the tab beside the closed one comes to the front");
        assert!(tabs.close(b));
        assert_eq!(tabs.active, None);
        assert!(tabs.list.is_empty());
        assert!(!tabs.close(b));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut tabs = Tabs::default();
        let a = tabs.open(None).unwrap();
        tabs.close(a);
        assert_ne!(tabs.open(None).unwrap(), a, "a closed tab's id never names a new one");
    }

    #[test]
    fn a_full_window_refuses_another_tab() {
        let mut tabs = Tabs::default();
        for _ in 0..MAX_TABS {
            assert!(tabs.open(None).is_some());
        }
        assert!(tabs.open(None).is_none());
        assert_eq!(tabs.list.len(), MAX_TABS);
    }

    #[test]
    fn closing_the_last_in_the_row_brings_the_one_before() {
        let mut tabs = Tabs::default();
        let a = tabs.open(None).unwrap();
        let b = tabs.open(None).unwrap();
        tabs.close(b);
        assert_eq!(tabs.active, Some(a));
    }

    #[test]
    fn the_address_bar_turns_words_into_a_page() {
        assert_eq!(address("https://github.com/x").as_deref(), Some("https://github.com/x"));
        assert_eq!(address("github.com").as_deref(), Some("https://github.com"));
        assert_eq!(address("  claude.ai/code/artifacts ").as_deref(), Some("https://claude.ai/code/artifacts"));
        assert_eq!(address("localhost:5173").as_deref(), Some("http://localhost:5173"));
        assert_eq!(address("127.0.0.1:3000/x").as_deref(), Some("http://127.0.0.1:3000/x"));
        assert_eq!(address("tauri multiwebview").as_deref(), Some("https://duckduckgo.com/?q=tauri+multiwebview"));
        assert_eq!(address("a&b").as_deref(), Some("https://duckduckgo.com/?q=a%26b"));
        assert_eq!(address("   "), None);
        assert_eq!(address("javascript:alert(1)").as_deref(), Some("https://duckduckgo.com/?q=javascript%3Aalert%281%29"), "never a script");
        assert_eq!(address("file:///etc/passwd").as_deref(), Some("https://duckduckgo.com/?q=file%3A%2F%2F%2Fetc%2Fpasswd"));
    }

    #[test]
    fn only_words_that_became_a_search_are_search_words() {
        assert_eq!(search_term("  google   meet ").as_deref(), Some("google meet"));
        assert_eq!(search_term("github.com"), None);
        assert_eq!(search_term("https://x.example/?q=1"), None);
        assert_eq!(search_term("localhost:5173"), None);
        assert_eq!(search_term("   "), None);
        assert_eq!(search_term(&"a ".repeat(300)).map(|t| t.chars().count()), Some(200));
    }

    #[test]
    fn a_tab_loads_web_pages_only() {
        assert!(may_load("https://claude.ai/code/artifacts"));
        assert!(may_load("http://localhost:5173/"));
        assert!(may_load("about:blank"));
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "tauri://localhost", "ipc://x", "data:text/html,hi", "castle://x", ""] {
            assert!(!may_load(bad), "{bad}");
        }
    }

    #[test]
    fn a_sign_in_pop_up_gets_its_own_window_and_a_plain_link_a_tab() {
        let gis = "https://accounts.google.com/gsi/select?client_id=x";
        assert_eq!(new_window_kind(gis, true, false), WindowKind::Popup, "a pop-up that asked for a size");
        assert_eq!(new_window_kind("about:blank", true, false), WindowKind::Popup, "a blank pop-up sent on afterwards");
        assert_eq!(new_window_kind("https://example.com/page", false, false), WindowKind::Tab, "target=_blank");
        assert_eq!(new_window_kind("about:blank", false, false), WindowKind::Tab);
        assert_eq!(new_window_kind("https://paypa1-login.com/", true, true), WindowKind::Tab, "a flagged site is never a pop-up");
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "castle://x"] {
            assert_eq!(new_window_kind(bad, true, false), WindowKind::Refuse, "{bad}");
        }
    }

    #[test]
    fn a_sites_own_title_says_how_much_is_new() {
        assert_eq!(unread_of("(3) WhatsApp"), 3);
        assert_eq!(unread_of("(9+) Facebook"), 9);
        assert_eq!(unread_of("Inbox (12) - me@gmail.com - Gmail"), 12);
        assert_eq!(unread_of("(1) Messenger"), 1);
        assert_eq!(unread_of("(5) Discord | #general | Friends"), 5);
        assert_eq!(unread_of("• Discord | #general | Friends"), 1, "unread with no number");
        assert_eq!(unread_of("[4] Forum - new posts"), 4);
        assert_eq!(unread_of("(2024) Annual report"), 0, "a year is not a count");
        assert_eq!(unread_of("($5) Deal"), 0, "a price is not a count");
        assert_eq!(unread_of("(0) Inbox"), 0);
        assert_eq!(unread_of("(12345) Big"), 0);
        assert_eq!(unread_of("WhatsApp"), 0);
        assert_eq!(unread_of("Inbox - me@gmail.com - Gmail"), 0);
    }

    #[test]
    fn the_summary_names_each_site_with_its_count() {
        let mut tabs = Tabs::default();
        for (title, unread) in [("(3) WhatsApp", 3), ("Inbox (12) - me@gmail.com - Gmail", 12), ("GitHub", 0), ("• Discord | #general", 1)] {
            let id = tabs.open(Some("https://x.example/".into())).unwrap();
            let tab = tabs.get_mut(id).unwrap();
            tab.title = title.into();
            tab.unread = unread;
        }
        assert_eq!(unread_summary(&tabs), (16, "WhatsApp 3 · Gmail 12 · Discord 1".to_string()));
        assert_eq!(unread_summary(&Tabs::default()), (0, String::new()));
    }

    #[test]
    fn a_tab_is_titled_by_its_page_then_its_host() {
        assert_eq!(tab_title(Some("https://github.com/o/r"), Some("o/r: the repo")), "o/r: the repo");
        assert_eq!(tab_title(Some("https://github.com/o/r"), Some("  ")), "github.com");
        assert_eq!(tab_title(Some("http://localhost:5173/x"), None), "localhost:5173");
        assert_eq!(tab_title(None, None), "New tab");
    }

    #[test]
    fn a_chat_link_goes_where_settings_say() {
        let page = "https://github.com/o/r/pull/7";
        assert_eq!(link_way("castle", page), LinkWay::Castle);
        assert_eq!(link_way("system", page), LinkWay::System);
        assert_eq!(link_way("ask", page), LinkWay::Ask);
        assert_eq!(link_way("castle", "http://localhost:5173/x"), LinkWay::Castle);
        assert_eq!(link_way("something old", page), LinkWay::Castle, "an unknown setting is the default");
        for bad in ["javascript:alert(1)", "http://example.com", "file:///x", "https://a b", ""] {
            assert_eq!(link_way("castle", bad), LinkWay::Refuse, "{bad}");
        }
    }

    #[test]
    fn the_whole_window_fits_its_half_of_the_screen() {
        // a 1512x944 (logical) work area's right half, with a 28-point title bar: the inner height leaves room for it
        assert_eq!(inner_within(756.0, 944.0, 0.0, 28.0, 560.0, 420.0), (756.0, 916.0));
        assert_eq!(inner_within(400.0, 300.0, 0.0, 28.0, 560.0, 420.0), (560.0, 420.0), "never below the minimum");
        assert_eq!(inner_within(756.0, 944.0, -4.0, -1.0, 560.0, 420.0), (756.0, 944.0), "a negative frame counts as none");
    }

    #[test]
    fn a_window_past_the_screen_edge_sticks_out() {
        let area = (0, 25, 3024, 1890);
        assert!(!sticks_out((1512, 25, 1512, 1890), area));
        assert!(sticks_out((1512, 25, 1512, 1946), area), "taller than the screen by the title bar");
        assert!(sticks_out((2000, 25, 1512, 1000), area), "past the right edge");
        assert!(sticks_out((-10, 25, 800, 600), area));
    }

    #[test]
    fn pages_go_side_by_side_up_to_four_and_close_from_the_middle() {
        let mut tabs = Tabs::default();
        let ids: Vec<u32> = (0..5).map(|_| tabs.open(Some("https://x.example/".into())).unwrap()).collect();
        tabs.switch(ids[0]);
        assert!(tabs.place_beside(ids[0]).is_err(), "a tab beside itself");
        tabs.place_beside(ids[1]).unwrap();
        assert_eq!((tabs.panes.clone(), tabs.active), (vec![ids[0], ids[1]], Some(ids[1])), "the two are a group, the new one in front");
        tabs.place_beside(ids[2]).unwrap();
        tabs.place_beside(ids[3]).unwrap();
        assert_eq!(tabs.on_screen(), ids[..4].to_vec());
        assert_eq!(tabs.widths, vec![0.25; 4]);
        assert!(tabs.place_beside(ids[4]).is_err(), "a fifth is refused");
        tabs.close(ids[1]);
        assert_eq!(tabs.panes, vec![ids[0], ids[2], ids[3]], "closed from the middle");
        tabs.switch(ids[4]);
        assert_eq!(tabs.on_screen(), vec![ids[4]], "a tab outside the group shows alone");
        tabs.switch(ids[2]);
        assert_eq!(tabs.on_screen().len(), 3, "a group member brings the group back");
        tabs.unpane(ids[0]);
        tabs.unpane(ids[3]);
        assert!(tabs.panes.is_empty(), "one page left is no group");
        assert_eq!(tabs.on_screen(), vec![ids[2]]);
    }

    #[test]
    fn the_columns_follow_the_grips() {
        assert_eq!(columns(1000.0, &[]), vec![(0.0, 1000.0)]);
        assert_eq!(columns(900.0, &[1.0 / 3.0; 3]), vec![(0.0, 300.0), (300.0, 300.0), (600.0, 300.0)]);
        let mut tabs = Tabs::default();
        let a = tabs.open(Some("https://a.example/".into())).unwrap();
        let b = tabs.open(Some("https://b.example/".into())).unwrap();
        tabs.switch(a);
        tabs.place_beside(b).unwrap();
        assert!(tabs.set_widths(&[3.0, 1.0]));
        assert_eq!(tabs.widths, vec![0.75, 0.25]);
        assert!(!tabs.set_widths(&[1.0]), "one share for two columns");
        assert!(!tabs.set_widths(&[0.95, 0.05]), "no column below a tenth");
    }

    #[test]
    fn three_or_four_pages_get_the_phone_layout_unless_switched_off() {
        assert!(!mobile_for(1, true));
        assert!(!mobile_for(2, true));
        assert!(mobile_for(3, true));
        assert!(mobile_for(4, true));
        assert!(!mobile_for(4, false), "Phone view off keeps them on desktop");
        assert!(MOBILE_UA.contains("Android") && MOBILE_UA.contains("Chrome/") && MOBILE_UA.contains("Mobile"));
        assert!(Tabs::default().phone_view, "on to begin with");
    }

    #[test]
    fn the_window_takes_the_right_half() {
        assert_eq!(right_half(0, 25, 3440, 1415), (1720, 25, 1720, 1415));
        assert_eq!(right_half(-1920, 0, 1921, 1080), (-960, 0, 961, 1080), "an odd width gives the extra pixel to the window");
    }
}
