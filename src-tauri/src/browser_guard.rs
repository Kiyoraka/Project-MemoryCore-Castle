//! The browser's guard before a page loads: a dangerous page is held back and the DANGER screen
//! shows in its place, where a site can never cover it.
//!
//! What it knows is kept in memory, because a page's navigation cannot wait on the memory database:
//! the hosts the Sentinel marked dangerous and the ones the member marked safe (read when the window
//! opens and kept up to date by every page's check), her active patterns, and what Google Safe
//! Browsing confirmed this run. The quick checks of site_rules.rs need nothing. A one-time pass
//! (Open anyway) lets one page of that site load in that tab, held in memory only.

use crate::browser_tabs::Danger;
use crate::site_rules::{self, Pattern, Verdict};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock, RwLock};

#[derive(Default)]
struct Known {
    /// host -> why the Sentinel marked it dangerous
    dangerous: HashMap<String, String>,
    member_safe: HashSet<String>,
    patterns: Vec<Pattern>,
    /// host -> what Google Safe Browsing found this run
    google: HashMap<String, &'static str>,
}

fn known() -> &'static RwLock<Known> {
    static KNOWN: OnceLock<RwLock<Known>> = OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

fn passes() -> &'static Mutex<HashSet<(u32, String)>> {
    static PASSES: OnceLock<Mutex<HashSet<(u32, String)>>> = OnceLock::new();
    PASSES.get_or_init(Default::default)
}

/// Everything the database says, at once (the window's open).
pub fn remember(dangerous: Vec<(String, String)>, member_safe: Vec<String>, patterns: Vec<Pattern>) {
    let mut k = known().write().unwrap();
    k.dangerous = dangerous.into_iter().collect();
    k.member_safe = member_safe.into_iter().collect();
    k.patterns = patterns;
}

/// One host's mark as a page's check just read it, and the patterns as they stand now.
pub fn learn(host: &str, mark: Option<(&site_rules::Warning, bool)>, patterns: &[Pattern]) {
    let mut k = known().write().unwrap();
    k.dangerous.remove(host);
    k.member_safe.remove(host);
    match mark {
        Some((w, true)) if w.verdict == Verdict::Safe => {
            k.member_safe.insert(host.to_string());
        }
        Some((w, _)) if w.verdict == Verdict::Dangerous => {
            k.dangerous.insert(host.to_string(), w.reason.clone());
        }
        _ => {}
    }
    k.patterns = patterns.to_vec();
}

/// What Google Safe Browsing confirmed for a host this run.
pub fn note_google(host: &str, threat: &'static str) {
    known().write().unwrap().google.insert(host.to_string(), threat);
}

pub fn google_of(host: &str) -> Option<&'static str> {
    known().read().unwrap().google.get(host).copied()
}

/// The guard's rule (pure): a host the member marked safe is never held back; otherwise the quick
/// checks' dangerous finding (a look-alike or a brand with a lure: phishing), the Sentinel's
/// dangerous mark, a dangerous pattern, or Google's finding. `suspicious` is never held back: it
/// stays the amber line under the address.
pub fn decide(url: &str, host: &str, member_safe: bool, marked: Option<&str>, patterns: &[Pattern], google: Option<&str>) -> Option<Danger> {
    if member_safe {
        return None;
    }
    let mut caught: Vec<String> = Vec::new();
    let mut reason: Option<String> = None;
    let mut threat: Option<String> = None;
    if let Some(threat_word) = google {
        caught.push("google".into());
        threat = Some(threat_word.to_string());
        reason = Some(format!("Google Safe Browsing lists this site as {}", match threat_word {
            "phishing" => "a phishing site",
            "malware" => "a malware site",
            "unwanted" => "a site with unwanted software",
            "harmful" => "a site with harmful apps",
            _ => "dangerous",
        }));
    }
    if let Some(w) = site_rules::local_rules(host).filter(|w| w.verdict == Verdict::Dangerous) {
        caught.push("lookalike".into());
        threat.get_or_insert_with(|| "phishing".into());
        reason.get_or_insert(format!("This site looks like a fake: {}", w.reason));
    }
    if let Some(why) = marked {
        caught.push("sentinel".into());
        reason.get_or_insert(format!("Your companion marked this site dangerous: {why}"));
    }
    if let Some(p) = patterns.iter().find(|p| p.verdict == Verdict::Dangerous && site_rules::pattern_matches(host, p)) {
        if !caught.iter().any(|c| c == "sentinel") {
            caught.push("sentinel".into());
        }
        reason.get_or_insert(format!("It matches a pattern your companion learned: {}", p.why));
    }
    if caught.is_empty() {
        return None;
    }
    Some(Danger { url: url.to_string(), threat: threat.unwrap_or_else(|| "dangerous".into()), reason: reason.unwrap_or_default(), caught_by: caught })
}

/// The guard's answer for an address now, from what it keeps in memory.
pub fn check(url: &str, host: &str) -> Option<Danger> {
    let k = known().read().unwrap();
    decide(url, host, k.member_safe.contains(host), k.dangerous.get(host).map(String::as_str), &k.patterns, k.google.get(host).copied())
}

/// Open anyway: one page of that site may load in that tab (its redirects included).
pub fn grant_pass(tab: u32, host: &str) {
    passes().lock().unwrap().insert((tab, host.to_string()));
}

pub fn has_pass(tab: u32, host: &str) -> bool {
    passes().lock().unwrap().contains(&(tab, host.to_string()))
}

/// Uses the pass up once its page has loaded; true when there was one.
pub fn take_pass(tab: u32, host: &str) -> bool {
    passes().lock().unwrap().remove(&(tab, host.to_string()))
}

fn cleared() -> &'static Mutex<HashSet<(u32, String)>> {
    static CLEARED: OnceLock<Mutex<HashSet<(u32, String)>>> = OnceLock::new();
    CLEARED.get_or_init(Default::default)
}

/// An address Google said is not listed, about to be loaded again in that tab: let through once.
pub fn clear_once(tab: u32, url: &str) {
    cleared().lock().unwrap().insert((tab, url.to_string()));
}

pub fn take_cleared(tab: u32, url: &str) -> bool {
    cleared().lock().unwrap().remove(&(tab, url.to_string()))
}

/// A closed tab's passes go with it.
pub fn forget_tab(tab: u32) {
    passes().lock().unwrap().retain(|(t, _)| *t != tab);
    cleared().lock().unwrap().retain(|(t, _)| *t != tab);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(kind: &str, value: &str, verdict: Verdict) -> Pattern {
        Pattern { kind: kind.into(), value: value.into(), verdict, why: "a free host's sign-in copies".into() }
    }

    #[test]
    fn a_look_alike_is_held_back_as_phishing() {
        let d = decide("https://paypa1.com/login", "paypa1.com", false, None, &[], None).unwrap();
        assert_eq!((d.threat.as_str(), d.caught_by.clone()), ("phishing", vec!["lookalike".to_string()]));
        assert!(d.reason.contains("paypal.com"), "{}", d.reason);
    }

    #[test]
    fn the_sentinels_mark_and_patterns_hold_a_page_back() {
        let d = decide("https://x.example/", "x.example", false, Some("a fake bank sign-in"), &[], None).unwrap();
        assert_eq!((d.threat.as_str(), d.caught_by.clone()), ("dangerous", vec!["sentinel".to_string()]));
        let d = decide("https://login.weebly-pay.example/", "login.weebly-pay.example", false, None, &[pattern("keyword", "weebly-pay", Verdict::Dangerous)], None).unwrap();
        assert_eq!(d.caught_by, vec!["sentinel".to_string()]);
    }

    #[test]
    fn google_comes_first_and_names_the_threat() {
        let d = decide("https://paypa1.com/", "paypa1.com", false, Some("x"), &[], Some("malware")).unwrap();
        assert_eq!(d.threat, "malware");
        assert_eq!(d.caught_by, vec!["google".to_string(), "lookalike".into(), "sentinel".into()], "every guard that caught it is lit");
        assert!(d.reason.starts_with("Google Safe Browsing"));
    }

    #[test]
    fn suspicious_stays_a_line_and_the_members_safe_stands() {
        assert_eq!(decide("http://1.2.3.4/", "1.2.3.4", false, None, &[pattern("suffix", ".example", Verdict::Suspicious)], None), None, "a bare address is only suspicious");
        assert_eq!(decide("https://github.com/", "github.com", false, None, &[], None), None, "a clean site");
        assert_eq!(decide("https://paypa1.com/", "paypa1.com", true, Some("x"), &[], Some("phishing")), None, "the member said safe");
    }

    #[test]
    fn a_pass_lasts_one_page_in_one_tab() {
        grant_pass(901, "paypa1.com");
        assert!(has_pass(901, "paypa1.com"));
        assert!(!has_pass(902, "paypa1.com"), "another tab is still held back");
        assert!(take_pass(901, "paypa1.com"));
        assert!(!has_pass(901, "paypa1.com"), "the next visit is held back again");
        grant_pass(903, "a.example");
        forget_tab(903);
        assert!(!has_pass(903, "a.example"));
    }

    #[test]
    fn what_a_page_check_reads_is_learned() {
        let bad = site_rules::Warning { verdict: Verdict::Dangerous, reason: "fake".into() };
        learn("learned.example", Some((&bad, false)), &[]);
        assert_eq!(check("https://learned.example/", "learned.example").unwrap().caught_by, vec!["sentinel".to_string()]);
        let safe = site_rules::Warning { verdict: Verdict::Safe, reason: String::new() };
        learn("learned.example", Some((&safe, true)), &[]);
        assert_eq!(check("https://learned.example/", "learned.example"), None, "the member's safe replaces it");
    }
}
