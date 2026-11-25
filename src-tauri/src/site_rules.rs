//! Is this website a phishing site? The checks Castle makes on every page load, before any AI is
//! asked: a disguised name (punycode), a bare address, a known brand on someone else's domain, a
//! near-miss of a known domain. Then the Sentinel's own mark for the host and the patterns she has
//! learned (sites.rs). The worst of them is what the browser shows; nothing here ever blocks a page.

use serde::Serialize;

/// How bad a site looks, from nothing to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Safe,
    Suspicious,
    Dangerous,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Safe => "safe",
            Verdict::Suspicious => "suspicious",
            Verdict::Dangerous => "dangerous",
        }
    }

    pub fn from_word(word: &str) -> Option<Verdict> {
        match word.trim().to_ascii_lowercase().as_str() {
            "safe" => Some(Verdict::Safe),
            "suspicious" => Some(Verdict::Suspicious),
            "dangerous" => Some(Verdict::Dangerous),
            _ => None,
        }
    }
}

/// A verdict and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Warning {
    pub verdict: Verdict,
    pub reason: String,
}

/// A learned pattern (sentinel_site_patterns).
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub kind: String,
    pub value: String,
    pub verdict: Verdict,
    pub why: String,
}

/// The pattern kinds the Sentinel may write.
pub const PATTERN_KINDS: [&str; 4] = ["host", "suffix", "keyword", "lookalike"];

/// Known brands and their own domains: a brand's name on any other domain is a warning sign.
const BRANDS: &[(&str, &[&str])] = &[
    ("paypal", &["paypal.com", "paypal.me"]),
    ("google", &["google.com", "google.com.my", "googleapis.com", "gstatic.com", "googleusercontent.com", "youtube.com"]),
    ("gmail", &["gmail.com", "google.com"]),
    ("microsoft", &["microsoft.com", "live.com", "office.com", "microsoftonline.com", "outlook.com"]),
    ("outlook", &["outlook.com", "live.com", "office.com"]),
    ("apple", &["apple.com", "icloud.com"]),
    ("icloud", &["icloud.com", "apple.com"]),
    ("amazon", &["amazon.com", "amazon.co.uk", "amazon.de", "amazon.co.jp", "amazonaws.com"]),
    ("facebook", &["facebook.com", "fb.com", "meta.com"]),
    ("instagram", &["instagram.com"]),
    ("whatsapp", &["whatsapp.com", "whatsapp.net"]),
    ("netflix", &["netflix.com"]),
    ("github", &["github.com", "githubusercontent.com", "github.io"]),
    ("anthropic", &["anthropic.com", "claude.ai", "claude.com"]),
    ("openai", &["openai.com", "chatgpt.com"]),
    ("binance", &["binance.com"]),
    ("coinbase", &["coinbase.com"]),
    ("maybank", &["maybank2u.com.my", "maybank.com", "maybank.com.my"]),
    ("cimb", &["cimbclicks.com.my", "cimb.com", "cimb.com.my"]),
    ("shopee", &["shopee.com.my", "shopee.com", "shopee.sg"]),
    ("lazada", &["lazada.com.my", "lazada.com", "lazada.sg"]),
    ("steam", &["steampowered.com", "steamcommunity.com"]),
];

/// Words a phishing site puts beside a borrowed brand.
const LURES: [&str; 12] = ["login", "signin", "verify", "secure", "account", "update", "wallet", "support", "auth", "confirm", "recover", "billing"];

/// The domains near-misses are measured against.
fn known_domains() -> impl Iterator<Item = &'static str> {
    BRANDS.iter().flat_map(|(_, domains)| domains.iter().copied())
}

/// A host as Castle stores and asks about it: lower case, letters, digits, dots and dashes, at most
/// 253, with a dot. None for anything else.
pub fn clean_host(host: &str) -> Option<String> {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let ok = !h.is_empty() && h.len() <= 253 && h.contains('.') && !h.starts_with('.') && h.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    ok.then_some(h)
}

/// The host is the domain or a subdomain of it.
fn on_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Letters a phisher swaps for digits, put back.
fn undigit(text: &str) -> String {
    text.chars().map(|c| match c { '0' => 'o', '1' => 'l', '3' => 'e', '4' => 'a', '5' => 's', '7' => 't', '8' => 'b', _ => c }).collect()
}

/// How many letters apart two words are (insert, delete, change).
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(row[j + 1] + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// The last two labels (or three, for a country's second level such as `com.my`).
fn registered(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').collect();
    let take = if labels.len() >= 3 && ["com", "co", "net", "org", "gov", "edu"].contains(&labels[labels.len() - 2]) && labels[labels.len() - 1].len() == 2 { 3 } else { 2 };
    labels[labels.len().saturating_sub(take)..].join(".")
}

/// One or two letters off `known` (or the same once digits are read as letters), without being it.
pub fn looks_like(host_domain: &str, known: &str) -> bool {
    if host_domain == known {
        return false;
    }
    // short names are close to too many honest ones: they count only when digits stand in for letters
    let near = match known.len() {
        n if n >= 10 => distance(host_domain, known) <= 2,
        n if n >= 9 => distance(host_domain, known) == 1,
        _ => false,
    };
    undigit(host_domain) == known || near
}

/// The checks that need no AI: the worst found, or None for a clean name.
pub fn local_rules(host: &str) -> Option<Warning> {
    let mut found: Vec<Warning> = Vec::new();
    if host.split('.').any(|label| label.starts_with("xn--")) {
        found.push(Warning { verdict: Verdict::Suspicious, reason: "its name uses look-alike letters from another alphabet".into() });
    }
    if host.parse::<std::net::IpAddr>().is_ok() || host.trim_matches(['[', ']']).parse::<std::net::Ipv6Addr>().is_ok() {
        found.push(Warning { verdict: Verdict::Suspicious, reason: "it is a bare address, not a named site".into() });
    }
    let words: Vec<&str> = host.split(['.', '-']).collect();
    for (brand, domains) in BRANDS {
        // a short brand must start a word ("apple-id..." yes, "pineapple" no); a longer one anywhere
        let named = if brand.len() >= 6 { host.contains(brand) } else { words.iter().any(|w| w.starts_with(brand)) };
        if named && !domains.iter().any(|d| on_domain(host, d)) {
            let lure = LURES.iter().any(|w| host.contains(w));
            let verdict = if lure { Verdict::Dangerous } else { Verdict::Suspicious };
            found.push(Warning { verdict, reason: format!("it carries the name {brand} but is not {brand}'s own site") });
            break;
        }
    }
    let own = registered(host);
    let is_known = known_domains().any(|known| known == own);
    if let Some(known) = known_domains().filter(|_| !is_known).find(|known| looks_like(&own, known)) {
        found.push(Warning { verdict: Verdict::Dangerous, reason: format!("its name is one letter or two away from {known}") });
    }
    found.into_iter().max_by_key(|w| w.verdict)
}

/// Whether a learned pattern catches a host.
pub fn pattern_matches(host: &str, pattern: &Pattern) -> bool {
    let value = pattern.value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return false;
    }
    match pattern.kind.as_str() {
        "host" => on_domain(host, &value),
        "suffix" => host.ends_with(&value),
        "keyword" => host.contains(&value),
        "lookalike" => looks_like(&registered(host), &value),
        _ => false,
    }
}

/// What the browser shows for a host: the worst of the rules, the Sentinel's mark and her active
/// patterns. A mark the member set to safe stands over everything else.
pub fn verdict_for(host: &str, mark: Option<&Warning>, member_said_safe: bool, patterns: &[Pattern]) -> Option<Warning> {
    if member_said_safe {
        return None;
    }
    let mut all: Vec<Warning> = local_rules(host).into_iter().collect();
    if let Some(m) = mark {
        all.push(m.clone());
    }
    for p in patterns.iter().filter(|p| pattern_matches(host, p)) {
        all.push(Warning { verdict: p.verdict, reason: p.why.clone() });
    }
    all.into_iter().filter(|w| w.verdict != Verdict::Safe).max_by_key(|w| w.verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worst(host: &str) -> Option<Verdict> {
        local_rules(host).map(|w| w.verdict)
    }

    #[test]
    fn a_disguised_a_bare_and_a_borrowed_name_are_caught() {
        assert_eq!(worst("xn--pypal-4ve.com"), Some(Verdict::Suspicious));
        assert_eq!(worst("185.220.101.4"), Some(Verdict::Suspicious));
        assert_eq!(worst("paypal-login-secure.top"), Some(Verdict::Dangerous));
        assert!(local_rules("paypal-login-secure.top").unwrap().reason.contains("paypal"));
        assert_eq!(worst("maybank2u-verify.com"), Some(Verdict::Dangerous));
        assert_eq!(worst("apple-id-confirm.net"), Some(Verdict::Dangerous));
        assert_eq!(worst("amazonia-travel.com"), Some(Verdict::Suspicious), "a brand without a lure is only suspicious");
        assert_eq!(worst("pineapple.com"), None, "a short brand inside another word is not the brand");
    }

    #[test]
    fn the_brands_own_sites_are_clean() {
        for host in ["paypal.com", "www.paypal.com", "accounts.google.com", "mail.google.com", "claude.ai", "github.com", "www.maybank2u.com.my", "shopee.com.my", "example.org"] {
            assert_eq!(worst(host), None, "{host}");
        }
    }

    #[test]
    fn a_near_miss_of_a_known_domain_is_caught_but_never_the_domain() {
        assert_eq!(worst("paypa1.com"), Some(Verdict::Dangerous));
        assert_eq!(worst("gooogle.com"), Some(Verdict::Dangerous));
        assert_eq!(worst("githab.com"), Some(Verdict::Dangerous));
        assert!(!looks_like("google.com", "google.com"));
        assert!(!looks_like("example.com", "google.com"));
        for honest in ["love.com", "ab.com", "fun.com", "github.io"] {
            assert_eq!(worst(honest), None, "{honest}: a short honest name is not a near-miss");
        }
    }

    #[test]
    fn each_pattern_kind_matches_as_written() {
        let p = |kind: &str, value: &str| Pattern { kind: kind.into(), value: value.into(), verdict: Verdict::Dangerous, why: "x".into() };
        assert!(pattern_matches("evil.example.top", &p("host", "example.top")) && !pattern_matches("goodexample.top", &p("host", "example.top")));
        assert!(pattern_matches("pay-secure.xyz", &p("suffix", "-secure.xyz")));
        assert!(pattern_matches("my-verify-now.com", &p("keyword", "verify-now")));
        assert!(pattern_matches("mybamk.com", &p("lookalike", "mybank.com")) && !pattern_matches("mybank.com", &p("lookalike", "mybank.com")));
        assert!(!pattern_matches("a.com", &p("unknown", "a.com")));
    }

    #[test]
    fn the_worst_wins_and_the_members_safe_stands_over_all() {
        let mark = Warning { verdict: Verdict::Suspicious, reason: "new and odd".into() };
        let pattern = Pattern { kind: "suffix".into(), value: ".top".into(), verdict: Verdict::Dangerous, why: "the .top sites seen were all fake".into() };
        let got = verdict_for("shop.top", Some(&mark), false, std::slice::from_ref(&pattern)).unwrap();
        assert_eq!((got.verdict, got.reason.as_str()), (Verdict::Dangerous, "the .top sites seen were all fake"));
        assert_eq!(verdict_for("shop.top", Some(&mark), true, &[pattern]), None);
        let safe = Warning { verdict: Verdict::Safe, reason: "fine".into() };
        assert_eq!(verdict_for("example.org", Some(&safe), false, &[]), None, "safe shows nothing");
    }

    #[test]
    fn only_a_plain_host_is_kept_or_asked_about() {
        assert_eq!(clean_host(" Example.COM. ").as_deref(), Some("example.com"));
        for bad in ["", "localhost", "a b.com", "evil.com/path", ".com", "x".repeat(254).as_str()] {
            assert_eq!(clean_host(bad), None, "{bad}");
        }
        assert_eq!(Verdict::from_word("Dangerous"), Some(Verdict::Dangerous));
        assert!(Verdict::Dangerous > Verdict::Suspicious && Verdict::Suspicious > Verdict::Safe);
    }
}
