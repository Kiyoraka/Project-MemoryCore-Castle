//! "<companion>, read this" (the Keep's "Alice, read this"): on the member's press, the page's words
//! go to their own companion at HOME with one ask - summarize it, say whether it looks safe, or help
//! fill it in - and the answer comes in HOME's chat. Only when asked, never a bank's page or a page
//! with a password box, never a field's contents, and nothing is kept.

use crate::session::MainHost;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

/// The most of a page's words handed over.
pub const READ_CAP: usize = 20_000;

/// The page's own words: everything visible, without fields, scripts, styles or editable areas.
const READ_SCRIPT: &str = r#"(() => {
  try {
    const body = (document.body || document.documentElement).cloneNode(true);
    body.querySelectorAll('input, textarea, select, script, style, noscript, template, [contenteditable]').forEach((e) => e.remove());
    return { url: location.href, title: document.title || '', text: body.innerText || '', hasPassword: !!document.querySelector('input[type=password]') };
  } catch (e) {
    return { url: location.href, title: '', text: '', hasPassword: false };
  }
})()"#;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Page {
    url: String,
    title: String,
    text: String,
    has_password: bool,
}

/// The three asks, in the Keep's words.
pub fn ask_of(kind: &str) -> Option<&'static str> {
    match kind {
        "summary" => Some("Please summarize this page for me in a few lines."),
        "safety" => Some("Is this page safe? Tell me if anything looks like a scam, phishing or a trick."),
        "form" => Some("Help me fill in this page: tell me what each part asks for and what I should put there."),
        _ => None,
    }
}

/// A page's words made plain (pure): control characters gone, blank lines folded, newlines kept, at
/// most `cap` characters.
pub fn clean_words(text: &str, cap: usize) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line: String = line.chars().filter(|c| !c.is_control()).collect::<String>().trim_end().to_string();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(&line);
        out.push('\n');
        if out.chars().count() >= cap {
            break;
        }
    }
    out.trim().chars().take(cap).collect()
}

/// Why a page is not read (pure): a bank, a password box, or nothing to read.
pub fn refusal(url: &str, has_password: bool, text: &str) -> Option<String> {
    if let Some(why) = crate::browser_translate::refused(url, has_password) {
        return Some(why.replace("translated", "read"));
    }
    if text.trim().is_empty() {
        return Some("This page has no words to read.".into());
    }
    None
}

/// The message the companion gets (pure): the ask, the page's title and address, then its words.
pub fn message(ask: &str, title: &str, url: &str, words: &str) -> String {
    let title: String = title.chars().filter(|c| !c.is_control()).take(200).collect();
    format!("{ask}\nTitle: {title}\nAddress: {url}\n\n{words}")
}

/// Read this: the page's words to the member's companion at HOME, with the ask chosen. Refused while
/// the companion is asleep at HOME, and on a bank's or a password page.
#[tauri::command]
pub async fn browser_read(app: AppHandle, id: u32, kind: String) -> Result<(), String> {
    let ask = ask_of(&kind).ok_or("That is not something to ask.")?;
    let companion = app.state::<crate::settings::SettingsStore>().get().companion_name.unwrap_or_else(|| "Your companion".into());
    if !app.state::<MainHost>().0.is_running() {
        return Err(format!("{companion} is asleep at HOME - wake them there first."));
    }
    let json = crate::browser_translate::ask_page(&app, id, READ_SCRIPT.to_string()).await?;
    let page: Page = serde_json::from_str(&json).map_err(|_| "The page answered in a shape Castle does not know.".to_string())?;
    let words = clean_words(&page.text, READ_CAP);
    if let Some(why) = refusal(&page.url, page.has_password, &words) {
        return Err(why);
    }
    crate::messages::send_message(message(ask, &page.title, &page.url, &words), None, app.clone()).map(|_| ())
}

/// Open HOME: Castle comes forward on HOME, where the answer is.
#[tauri::command]
pub fn browser_open_home(app: AppHandle) {
    crate::roomalert::open_room(&app, "home");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_asks_are_the_keeps_and_nothing_else() {
        assert!(ask_of("summary").unwrap().contains("summarize"));
        assert!(ask_of("safety").unwrap().contains("safe"));
        assert!(ask_of("form").unwrap().contains("fill"));
        assert_eq!(ask_of("delete everything"), None);
    }

    #[test]
    fn a_bank_a_password_page_or_an_empty_one_is_never_read() {
        assert!(refusal("https://www.maybank2u.com.my/", false, "words").unwrap().contains("bank"));
        assert!(refusal("https://news.example/", true, "words").unwrap().contains("password"));
        assert!(refusal("https://news.example/", false, "  ").is_some());
        assert_eq!(refusal("https://news.example/", false, "The news today."), None);
    }

    #[test]
    fn the_words_are_plain_capped_and_keep_their_lines() {
        let w = clean_words("Line one\u{7}\n\n\n\nLine two\n", 100);
        assert_eq!(w, "Line one\n\nLine two");
        assert_eq!(clean_words(&"x".repeat(50_000), READ_CAP).chars().count(), READ_CAP);
    }

    #[test]
    fn the_message_carries_the_ask_the_title_the_address_and_the_words() {
        let m = message("Please summarize this page for me in a few lines.", "News\u{1b}[2J", "https://news.example/a", "The news.");
        assert!(m.starts_with("Please summarize"));
        assert!(m.contains("\nTitle: News[2J\nAddress: https://news.example/a\n\nThe news."), "{m}");
    }
}
