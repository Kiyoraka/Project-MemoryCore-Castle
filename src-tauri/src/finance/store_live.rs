//! Opt-in test for FINANCE's store on a real database (ignored unless asked for).
use super::store::*;
use chrono::NaiveDate;
use mysql::{prelude::Queryable, Conn};

fn draft(shop: &str, total: &str, currency: &str, date: &str, category: &str) -> Draft {
    Draft { shop: shop.into(), total: total.into(), currency: currency.into(), date: date.into(), category: category.into(), ..Draft::default() }
}

#[test]
#[ignore]
fn live_database_receipts_wait_are_kept_with_edits_and_total_apart_per_currency() {
    let config = std::env::var("CASTLE_TEST_DB").unwrap_or_else(|_| {
        // platform: windows-only - an on-demand live test run on Windows, where Castle's settings live under APPDATA
        let folder = std::env::var("APPDATA").unwrap() + "/com.memorycore.castle/settings.json";
        let port = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(folder).unwrap()).unwrap()["dbPort"].as_u64().unwrap();
        format!("{port}:root:{}", crate::key::db_root().unwrap_or_default())
    });
    let mut parts = config.splitn(3, ':');
    let port = parts.next().unwrap().parse().unwrap();
    let (user, pass) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
    let access = crate::memdb::provision(port, &user, &pass, &format!("Finance Test {}", std::process::id())).unwrap();
    let mut c: Conn = crate::memdb::open(&access).unwrap();
    crate::memdb::migrate(&mut c).unwrap();
    let seen: Option<String> = c.query_first("SELECT name FROM schema_migrations WHERE name='028_finance'").unwrap();
    assert!(seen.is_some());

    let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
    let a = propose(&mut c, &check(&draft("Kedai A", "12.50", "MYR", "2026-11-24", "food"), today).unwrap(), "finance/receipts/2026-11/a.jpg", &"a".repeat(64), "drop").unwrap();
    let b = propose(&mut c, &check(&draft("Shop B", "8", "USD", "2026-11-20", "food"), today).unwrap(), "finance/receipts/2026-11/b.jpg", &"b".repeat(64), "telegram").unwrap();
    let d = propose(&mut c, &check(&draft("Shop D", "99", "MYR", "2026-11-02", "fun"), today).unwrap(), "finance/receipts/2026-11/d.jpg", &"d".repeat(64), "drop").unwrap();
    // the same picture twice names the earlier receipt
    let again = propose(&mut c, &check(&draft("Kedai A", "12.50", "MYR", "2026-11-24", "food"), today).unwrap(), "x.jpg", &"a".repeat(64), "drop").unwrap_err();
    assert!(again.contains(&format!("#{a}")), "{again}");
    assert!(propose(&mut c, &check(&draft("X", "1", "MYR", "2026-11-24", "food"), today).unwrap(), "x.jpg", &"e".repeat(64), "mail").is_err());

    let waiting = month(&mut c, "2026-11").unwrap();
    assert_eq!(waiting.waiting.len(), 3);
    assert!(waiting.kept.is_empty() && waiting.totals.is_empty(), "nothing counts before the member's yes");

    // kept with a correction, kept as it is, dismissed
    decide(&mut c, a, true, Some(&check(&draft("Kedai A", "13.50", "MYR", "2026-11-24", "groceries"), today).unwrap())).unwrap();
    decide(&mut c, b, true, None).unwrap();
    decide(&mut c, d, false, None).unwrap();
    assert!(decide(&mut c, d, true, None).unwrap_err().contains("already dismissed"));

    let m = month(&mut c, "2026-11").unwrap();
    assert!(m.waiting.is_empty());
    assert_eq!(m.kept.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a, b], "newest day first");
    assert_eq!(get(&mut c, a).unwrap().total_cents, 1350);
    let pairs: Vec<(String, String, i64)> = m.totals.iter().map(|t| (t.currency.clone(), t.category.clone(), t.cents)).collect();
    assert_eq!(pairs, vec![("MYR".into(), "groceries".into(), 1350), ("USD".into(), "food".into(), 800)], "never added across currencies");
    assert!(month(&mut c, "2026-10").unwrap().kept.is_empty());

    assert_eq!(list(&mut c, Some("2026-11"), Some("dismissed")).unwrap().len(), 1);
    assert!(list(&mut c, None, Some("lost")).is_err());
    let text = csv(&kept_for_export(&mut c, Some("2026-11")).unwrap());
    assert_eq!(text.lines().count(), 3);
    assert!(text.contains("2026-11-24,Kedai A,groceries,13.50,MYR"));

    // the door: a picture inside the memory folder is copied once and proposed; again, refused
    let memory = std::env::temp_dir().join(format!("castle-finance-door-{}", std::process::id()));
    std::fs::create_dir_all(memory.join("finance").join("inbox")).unwrap();
    let picture = memory.join("finance").join("inbox").join("r.jpg");
    std::fs::write(&picture, [0xFF, 0xD8, 0xFF, 0xE0, 9, 9]).unwrap();
    let words = |extra: &[&str]| {
        let mut args: Vec<String> = ["finance", "propose", "--shop", "Kedai Door", "--total", "4.20", "--currency", "MYR", "--date", "2026-11-24", "--category", "food", "--source", "drop", "--picture"].iter().map(|s| s.to_string()).collect();
        args.push(picture.display().to_string());
        args.extend(extra.iter().map(|s| s.to_string()));
        crate::door::parse(&args).unwrap()
    };
    let said = super::companion::door(&mut c, &words(&[]), &memory).unwrap();
    assert!(said.contains("proposed: Kedai Door 4.20 MYR"), "{said}");
    let kept: Vec<_> = std::fs::read_dir(memory.join("finance").join("receipts").join("2026-11")).unwrap().collect();
    assert_eq!(kept.len(), 1);
    assert!(super::companion::door(&mut c, &words(&[]), &memory).unwrap_err().contains("already receipt"));
    assert!(super::companion::door(&mut c, &words(&["--last4", "411111"]), &memory).unwrap_err().contains("last four"));
    assert!(super::companion::door(&mut c, &crate::door::parse(&["finance".into(), "keep".into()]).unwrap(), &memory).unwrap_err().contains("belong to your person"));
    let _ = std::fs::remove_dir_all(&memory);

    let _ = c.query_drop(format!("DROP DATABASE `{}`", access.name));
}
