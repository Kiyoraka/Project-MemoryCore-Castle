//! The member's receipts, in their own memory database (`finance_receipts`, 028_finance.sql).
//! A receipt is proposed by the companion and kept or dismissed only by the member. What is kept
//! never leaves this computer. A full account number is never stored: only its last four digits,
//! and any run of five or more digits in the words is masked.

use chrono::{Local, NaiveDate};
use mysql::{prelude::Queryable, Conn};
use serde::{Deserialize, Serialize};

/// The categories a receipt may be filed under.
pub const CATEGORIES: [&str; 9] = ["food", "groceries", "transport", "bills", "shopping", "health", "home", "fun", "other"];
/// Where a receipt came from.
pub const SOURCES: [&str; 4] = ["drop", "whatsapp", "telegram", "chat"];
/// The largest total a receipt may carry, in cents (ten billion in any currency).
const MAX_CENTS: i64 = 1_000_000_000_000;
/// How many kept receipts a month's view lists, and how many wait at most.
const LATEST: usize = 60;
const WAITING: usize = 100;

/// What the companion (or the member's correction) says a receipt holds, before it is checked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub shop: String,
    /// "12.50", "1,234.50" or "12,50"
    pub total: String,
    pub currency: String,
    /// YYYY-MM-DD
    pub date: String,
    pub category: String,
    pub last4: String,
    pub note: String,
}

/// A draft that passed every check.
#[derive(Debug, Clone, PartialEq)]
pub struct Fields {
    pub shop: String,
    pub total_cents: i64,
    pub currency: String,
    pub spent_on: NaiveDate,
    pub category: String,
    pub last4: String,
    pub note: String,
}

/// One receipt as the FINANCE room shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: u64,
    pub state: String,
    pub shop: String,
    pub total_cents: i64,
    pub currency: String,
    pub spent_on: String,
    pub category: String,
    pub last4: String,
    pub note: String,
    /// relative to the memory folder
    pub picture_path: String,
    pub source: String,
    pub proposed_at: String,
}

/// A month's spending in one currency and category.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Total {
    pub currency: String,
    pub category: String,
    pub cents: i64,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Month {
    pub month: String,
    pub totals: Vec<Total>,
    pub kept: Vec<Receipt>,
    pub waiting: Vec<Receipt>,
}

/// Every run of five or more digits replaced, so a card or account number never lands in a row.
pub fn mask_numbers(text: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.chars().count() >= 5 {
            out.push_str("[number]");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// A total in words -> cents: a dot is the decimal point; a comma is one too when it is the only
/// mark and two digits follow it ("12,50"), else it groups thousands ("1,234.50").
pub fn parse_total(text: &str) -> Result<i64, String> {
    let t: String = text.trim().chars().filter(|c| !c.is_whitespace()).collect();
    let bad = || format!("\"{}\" is not a total (write it like 12.50).", text.trim());
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
        return Err(bad());
    }
    let decimal_comma = !t.contains('.') && t.matches(',').count() == 1 && t.split(',').nth(1).is_some_and(|d| d.len() == 2);
    let plain = if decimal_comma { t.replace(',', ".") } else { t.replace(',', "") };
    let (whole, part) = plain.split_once('.').unwrap_or((&plain, ""));
    if whole.is_empty() || part.len() > 2 || part.contains('.') {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let part: i64 = if part.is_empty() { 0 } else { format!("{part:0<2}").parse().map_err(|_| bad())? };
    let cents = whole.checked_mul(100).and_then(|w| w.checked_add(part)).filter(|c| *c <= MAX_CENTS).ok_or_else(bad)?;
    Ok(cents)
}

/// Checks a draft, as of `today`. Every refusal says what to write instead.
pub fn check(d: &Draft, today: NaiveDate) -> Result<Fields, String> {
    let shop = mask_numbers(d.shop.trim());
    if shop.is_empty() || shop.chars().count() > 120 {
        return Err("The shop's name is needed (at most 120 characters).".into());
    }
    let total_cents = parse_total(&d.total)?;
    let currency = d.currency.trim().to_ascii_uppercase();
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("The currency is its three letters, like MYR, USD or EUR.".into());
    }
    let spent_on = NaiveDate::parse_from_str(d.date.trim(), "%Y-%m-%d").map_err(|_| "The date is written YYYY-MM-DD.".to_string())?;
    let earliest = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
    if spent_on < earliest || spent_on > today + chrono::Duration::days(1) {
        return Err("That date is not a day a receipt could carry.".into());
    }
    let category = d.category.trim().to_ascii_lowercase();
    if !CATEGORIES.contains(&category.as_str()) {
        return Err(format!("The category is one of: {}.", CATEGORIES.join(", ")));
    }
    let last4 = d.last4.trim().to_string();
    if !last4.is_empty() && !(last4.len() == 4 && last4.chars().all(|c| c.is_ascii_digit())) {
        return Err("Only the card's last four digits are kept, like 1234.".into());
    }
    let note = mask_numbers(d.note.trim());
    if note.chars().count() > 255 {
        return Err("The note is at most 255 characters.".into());
    }
    Ok(Fields { shop, total_cents, currency, spent_on, category, last4, note })
}

const COLUMNS: &str = "id, state, shop, total_cents, currency, DATE_FORMAT(spent_on, '%Y-%m-%d'), category, account_last4, note, picture_path, source, DATE_FORMAT(proposed_at, '%Y-%m-%d %H:%i')";

type Row = (u64, String, String, i64, String, String, String, String, String, String, String, String);

fn receipt(r: Row) -> Receipt {
    let (id, state, shop, total_cents, currency, spent_on, category, last4, note, picture_path, source, proposed_at) = r;
    Receipt { id, state, shop, total_cents, currency, spent_on, category, last4, note, picture_path, source, proposed_at }
}

fn query(c: &mut Conn, tail: &str, params: impl Into<mysql::Params>) -> Result<Vec<Receipt>, String> {
    let rows: Vec<Row> = c.exec(format!("SELECT {COLUMNS} FROM finance_receipts {tail}"), params).map_err(|_| "The receipts could not be read.".to_string())?;
    Ok(rows.into_iter().map(receipt).collect())
}

pub fn get(c: &mut Conn, id: u64) -> Result<Receipt, String> {
    query(c, "WHERE id=?", (id,))?.into_iter().next().ok_or_else(|| format!("There is no receipt #{id}."))
}

/// Files a proposal: it waits for the member in FINANCE. One picture is filed once; a second time
/// names the receipt it already is.
pub fn propose(c: &mut Conn, f: &Fields, picture_path: &str, sha256: &str, source: &str) -> Result<u64, String> {
    if !SOURCES.contains(&source) {
        return Err("Unknown source.".into());
    }
    let earlier: Option<(u64, String)> = c
        .exec_first("SELECT id, state FROM finance_receipts WHERE picture_sha256=?", (sha256,))
        .map_err(|_| "The receipts could not be read.".to_string())?;
    if let Some((id, state)) = earlier {
        return Err(format!("This picture is already receipt #{id} ({state})."));
    }
    let done = c.exec_drop(
        "INSERT INTO finance_receipts (shop, total_cents, currency, spent_on, category, account_last4, note, picture_path, picture_sha256, source) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (&f.shop, f.total_cents, &f.currency, f.spent_on.format("%Y-%m-%d").to_string(), &f.category, &f.last4, &f.note, picture_path, sha256, source),
    );
    match done {
        Ok(()) => Ok(c.last_insert_id()),
        // the same picture proposed twice at once: the unique key keeps one
        Err(mysql::Error::MySqlError(e)) if e.code == 1062 => Err("This picture was just proposed.".into()),
        Err(_) => Err("The receipt could not be saved.".into()),
    }
}

/// The member's Keep (with any fields they corrected) or Dismiss of a waiting receipt.
pub fn decide(c: &mut Conn, id: u64, keep: bool, edited: Option<&Fields>) -> Result<(), String> {
    let r = get(c, id)?;
    if r.state != "proposed" {
        return Err(format!("Receipt #{id} was already {}.", r.state));
    }
    let result = match (keep, edited) {
        (true, Some(f)) => c.exec_drop(
            "UPDATE finance_receipts SET state='kept', decided_at=NOW(), shop=?, total_cents=?, currency=?, spent_on=?, category=?, account_last4=?, note=? WHERE id=? AND state='proposed'",
            (&f.shop, f.total_cents, &f.currency, f.spent_on.format("%Y-%m-%d").to_string(), &f.category, &f.last4, &f.note, id),
        ),
        (true, None) => c.exec_drop("UPDATE finance_receipts SET state='kept', decided_at=NOW() WHERE id=? AND state='proposed'", (id,)),
        (false, _) => c.exec_drop("UPDATE finance_receipts SET state='dismissed', decided_at=NOW() WHERE id=? AND state='proposed'", (id,)),
    };
    result.map_err(|_| "The receipt could not be saved.".to_string())
}

/// A month's spending: totals per currency and category, the kept receipts and everything waiting.
pub fn month(c: &mut Conn, month: &str) -> Result<Month, String> {
    let (from, to) = month_range(month)?;
    let (a, b) = (from.format("%Y-%m-%d").to_string(), to.format("%Y-%m-%d").to_string());
    let rows: Vec<(String, String, i64, u32)> = c
        .exec(
            "SELECT currency, category, CAST(SUM(total_cents) AS SIGNED), CAST(COUNT(*) AS UNSIGNED) FROM finance_receipts WHERE state='kept' AND spent_on >= ? AND spent_on < ? GROUP BY currency, category ORDER BY currency, SUM(total_cents) DESC",
            (&a, &b),
        )
        .map_err(|_| "The receipts could not be read.".to_string())?;
    let totals = rows.into_iter().map(|(currency, category, cents, count)| Total { currency, category, cents, count }).collect();
    let kept = query(c, &format!("WHERE state='kept' AND spent_on >= ? AND spent_on < ? ORDER BY spent_on DESC, id DESC LIMIT {LATEST}"), (&a, &b))?;
    let waiting = query(c, &format!("WHERE state='proposed' ORDER BY proposed_at, id LIMIT {WAITING}"), ())?;
    Ok(Month { month: from.format("%Y-%m").to_string(), totals, kept, waiting })
}

/// The receipts for the companion's `finance list`: one month, or the newest, in a state or any.
pub fn list(c: &mut Conn, month: Option<&str>, state: Option<&str>) -> Result<Vec<Receipt>, String> {
    if let Some(s) = state {
        if !["proposed", "kept", "dismissed"].contains(&s) {
            return Err("The state is proposed, kept or dismissed.".into());
        }
    }
    let state = state.unwrap_or("");
    match month {
        Some(m) => {
            let (from, to) = month_range(m)?;
            query(c, "WHERE (?='' OR state=?) AND spent_on >= ? AND spent_on < ? ORDER BY spent_on DESC, id DESC LIMIT 200", (state, state, from.format("%Y-%m-%d").to_string(), to.format("%Y-%m-%d").to_string()))
        }
        None => query(c, "WHERE (?='' OR state=?) ORDER BY spent_on DESC, id DESC LIMIT 50", (state, state)),
    }
}

/// "2026-11" -> its first day and the first day after it.
pub fn month_range(month: &str) -> Result<(NaiveDate, NaiveDate), String> {
    let from = NaiveDate::parse_from_str(&format!("{}-01", month.trim()), "%Y-%m-%d").map_err(|_| "The month is written YYYY-MM.".to_string())?;
    let to = if from.month0() == 11 { NaiveDate::from_ymd_opt(from.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(from.year(), from.month() + 1, 1) };
    Ok((from, to.ok_or("The month is written YYYY-MM.")?))
}

use chrono::Datelike;

/// This month, in the member's own clock.
pub fn this_month() -> String {
    Local::now().format("%Y-%m").to_string()
}

/// A CSV cell: quoted when needed, and never read as a formula by a spreadsheet.
fn cell(text: &str) -> String {
    let safe = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text.to_string() };
    if safe.contains([',', '"', '\n', '\r']) { format!("\"{}\"", safe.replace('"', "\"\"")) } else { safe }
}

/// "12.50" from 1250 cents.
pub fn amount(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, (cents % 100).abs())
}

/// The kept receipts as CSV, for the member's own accountant: one month, or everything.
pub fn csv(rows: &[Receipt]) -> String {
    let mut out = String::from("date,shop,category,total,currency,card_last4,note,picture\r\n");
    for r in rows {
        let line = [r.spent_on.as_str(), &r.shop, &r.category, &amount(r.total_cents), &r.currency, &r.last4, &r.note, &r.picture_path].map(cell).join(",");
        out.push_str(&line);
        out.push_str("\r\n");
    }
    out
}

/// Every kept receipt of a month (or all of them), oldest first, for the CSV.
pub fn kept_for_export(c: &mut Conn, month: Option<&str>) -> Result<Vec<Receipt>, String> {
    match month {
        Some(m) => {
            let (from, to) = month_range(m)?;
            query(c, "WHERE state='kept' AND spent_on >= ? AND spent_on < ? ORDER BY spent_on, id", (from.format("%Y-%m-%d").to_string(), to.format("%Y-%m-%d").to_string()))
        }
        None => query(c, "WHERE state='kept' ORDER BY spent_on, id", ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()
    }

    fn draft() -> Draft {
        Draft { shop: "Kedai Runcit".into(), total: "12.50".into(), currency: "myr".into(), date: "2026-11-24".into(), category: "Groceries".into(), last4: "".into(), note: "".into() }
    }

    #[test]
    fn a_total_is_read_in_cents() {
        assert_eq!(parse_total("12.50"), Ok(1250));
        assert_eq!(parse_total("12.5"), Ok(1250));
        assert_eq!(parse_total("12"), Ok(1200));
        assert_eq!(parse_total("1,234.50"), Ok(123_450));
        assert_eq!(parse_total("12,50"), Ok(1250), "a decimal comma");
        assert_eq!(parse_total("1,234"), Ok(123_400), "a thousands comma");
        assert_eq!(parse_total(" 3 000.00 "), Ok(300_000));
        for bad in ["", "abc", "12.505", "-5", ".50", "1.2.3", "99999999999999"] {
            assert!(parse_total(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_good_draft_is_cleaned() {
        let f = check(&draft(), today()).unwrap();
        assert_eq!((f.currency.as_str(), f.category.as_str(), f.total_cents), ("MYR", "groceries", 1250));
        assert_eq!(f.spent_on, NaiveDate::from_ymd_opt(2026, 11, 24).unwrap());
    }

    #[test]
    fn every_bad_field_is_refused_in_words() {
        let bad = |change: fn(&mut Draft)| {
            let mut d = draft();
            change(&mut d);
            check(&d, today()).unwrap_err()
        };
        assert!(bad(|d| d.shop = "  ".into()).contains("shop"));
        assert!(bad(|d| d.currency = "RM".into()).contains("three letters"));
        assert!(bad(|d| d.date = "24/11/2026".into()).contains("YYYY-MM-DD"));
        assert!(bad(|d| d.date = "2027-01-01".into()).contains("not a day"));
        assert!(bad(|d| d.category = "luxury".into()).contains("groceries"));
        assert!(bad(|d| d.last4 = "4111111111111111".into()).contains("last four"));
        assert!(bad(|d| d.last4 = "12a4".into()).contains("last four"));
    }

    #[test]
    fn a_long_number_never_lands_in_the_words() {
        assert_eq!(mask_numbers("card 4111111111111111 ok"), "card [number] ok");
        assert_eq!(mask_numbers("table 12, 2 pax, ref 1234"), "table 12, 2 pax, ref 1234");
        assert_eq!(mask_numbers("12345"), "[number]");
        let mut d = draft();
        d.note = "paid with 5500 1234 5678 9012".into();
        d.shop = "Shop 0123456789".into();
        let f = check(&d, today()).unwrap();
        assert_eq!(f.note, "paid with 5500 1234 5678 9012", "groups of four stay (the last4 rule keeps the card out)");
        assert_eq!(f.shop, "Shop [number]");
    }

    #[test]
    fn a_month_is_its_first_day_to_the_next_month() {
        let d = |y, m| NaiveDate::from_ymd_opt(y, m, 1).unwrap();
        assert_eq!(month_range("2026-11"), Ok((d(2026, 11), d(2026, 12))));
        assert_eq!(month_range("2026-12"), Ok((d(2026, 12), d(2027, 1))));
        assert!(month_range("Nov").is_err());
    }

    #[test]
    fn the_csv_is_safe_in_a_spreadsheet() {
        let r = Receipt {
            id: 1, state: "kept".into(), shop: "=HYPERLINK(\"x\")".into(), total_cents: 1205, currency: "MYR".into(), spent_on: "2026-11-24".into(),
            category: "food".into(), last4: "1234".into(), note: "lunch, with Rin".into(), picture_path: "finance/receipts/2026-11/ab.jpg".into(),
            source: "drop".into(), proposed_at: "".into(),
        };
        let text = csv(&[r]);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("date,shop,category,total,currency,card_last4,note,picture"));
        assert_eq!(lines.next(), Some("2026-11-24,\"'=HYPERLINK(\"\"x\"\")\",food,12.05,MYR,1234,\"lunch, with Rin\",finance/receipts/2026-11/ab.jpg"));
        assert_eq!(amount(7), "0.07");
    }
}
