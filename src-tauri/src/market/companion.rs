//! The companion at the desk. A turn is one quiet run of the member's own CLI (no tools, its own
//! folder and rules) that reads the desk and answers in JSON; every action it asks for goes through
//! the same trade door as a member's click. It may also trade by itself, on rounds, within the limits
//! the member set (off until they turn it on, and off again whenever Castle starts). After every
//! closed position it writes one short lesson, and its newest lessons ride in every desk.

use crate::market::board::Board;
use crate::market::commands::{close_as, place_as};
use crate::market::desk::{self, Action, Answer, DESK_LESSONS, DESK_TURNS};
use crate::market::feed::BybitFeed;
use crate::market::indicators;
use crate::market::store::{self, CRYPTO};
use crate::market::trade::{OrderInput, Source};
use crate::settings::SettingsStore;
use mysql::Conn;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const RULES: &str = include_str!("../../resources/market-rules.md");
const LESSON_RULES: &str = "You are {{AI_NAME}}, keeping short lessons from paper trades at a practice desk. Answer with one plain sentence and nothing else.";
/// A turn waits this long for the CLI.
const TURN_LIMIT: Duration = Duration::from_secs(180);
const LESSON_LIMIT: Duration = Duration::from_secs(90);
/// The rounds' clock looks this often.
const LOOK_EVERY: Duration = Duration::from_secs(20);
/// Candle symbols on one desk.
const DESK_SYMBOLS: usize = 6;
/// Only positions closed within this long get a lesson.
const LESSON_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;

static TURNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
/// The member's clock offset from UTC in minutes, from the page, for "today".
static OFFSET_MIN: AtomicI64 = AtomicI64::new(0);
static STOPPED_WHY: Mutex<Option<String>> = Mutex::new(None);

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The start of the member's day (ms, UTC), from their clock offset.
pub fn day_start(now: i64, offset_min: i64) -> i64 {
    let day = 86_400_000;
    let local = now + offset_min * 60_000;
    local - local.rem_euclid(day) - offset_min * 60_000
}

fn memory(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.state::<SettingsStore>().get().memory_folder.map(std::path::PathBuf::from).ok_or_else(|| "Choose your memory folder first.".to_string())
}

fn open(app: &AppHandle) -> Result<Conn, String> {
    store::open(&memory(app)?)
}

fn prepare(app: &AppHandle, name: &str, rules: &str) -> Result<std::path::PathBuf, String> {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    let folder = store.folder().join(name);
    std::fs::create_dir_all(&folder).map_err(|_| "The desk's folder could not be made.".to_string())?;
    let rules = if rules.contains("{{MANNER}}") { desk::with_manner(rules, &desk_manner(&settings)) } else { rules.to_string() };
    let text = rules
        .replace("{{AI_NAME}}", &settings.companion_label())
        .replace("{{USER_NAME}}", settings.member_name.as_deref().unwrap_or("your person"))
        .replace("{{STYLE}}", settings.relationship_style.as_deref().unwrap_or("a friendly partner"));
    for file in std::iter::once("rules.md").chain(crate::setup::INSTRUCTION_FILES) {
        crate::oneshot::write_if_changed(&folder.join(file), &text);
    }
    Ok(folder)
}

/// The manner lines her active traits give the desk, or none: MARKET's own switch off, no
/// database yet, or nothing active. The chat room's switch is not read here.
fn desk_manner(settings: &crate::settings::Settings) -> crate::traits::Manner {
    if !settings.emotion_market_voice {
        return crate::traits::Manner::default();
    }
    let Some(mut conn) = settings.memory_folder.as_deref().and_then(|f| crate::store::source(std::path::Path::new(f))) else {
        return crate::traits::Manner::default();
    };
    let traits = crate::traits::list(&mut conn, Some("active")).unwrap_or_default();
    let member = settings.member_name.clone().unwrap_or_default();
    let companion = settings.companion_label();
    crate::traits::manner_block(&traits, &[member.as_str(), companion.as_str()])
}

/// What one action came to, for the trade log.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub action: Action,
    pub result: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    pub reply: String,
    pub done: Vec<Done>,
    pub status: String,
}

// ---------- the desk ----------

fn desk_for(app: &AppHandle, conn: &mut Conn, run: &store::Run, trigger: &str, message: Option<&str>) -> Result<Value, String> {
    let board = app.state::<Board>();
    let shown = store::open_positions(conn, run.id)?;
    let resting = store::resting_orders(conn, run.id)?;
    let tradable: Vec<String> = board.symbols().map(|l| l.into_iter().map(|r| r.symbol).collect()).unwrap_or_default();
    let scan = board.scan().unwrap_or_default();

    let mut wanted: Vec<String> = shown.iter().map(|p| p.symbol.clone()).collect();
    for s in message.map(|m| desk::named_symbols(m, &tradable)).unwrap_or_default().into_iter().chain(scan.candidates()) {
        if !wanted.contains(&s) {
            wanted.push(s);
        }
    }
    wanted.truncate(DESK_SYMBOLS);

    let feed = BybitFeed::new().ok();
    let mut symbols = Vec::new();
    let mut candles = serde_json::Map::new();
    for s in &wanted {
        let rules = board.rules(s).ok();
        let quote = board.quote(s).ok();
        symbols.push(json!({
            "symbol": s,
            "last": quote.as_ref().map(|q| q.last),
            "mark": quote.as_ref().map(|q| q.mark),
            "tickSize": rules.as_ref().map(|r| r.tick_size),
            "qtyStep": rules.as_ref().map(|r| r.qty_step),
            "minQty": rules.as_ref().map(|r| r.min_qty),
            "maxLeverage": rules.as_ref().map(|r| r.max_leverage),
        }));
        if let Some(feed) = feed.as_ref() {
            // Kept at the original 32/24, not bumped to 60/60 - that doubled the raw candle rows
            // in every desk prompt across all 6 symbols and pushed real turns past the 180s
            // limit ("It took too long and was stopped", confirmed 16 Nov 2026). RSI14/Stochastic
            // and SMA20 all get a real reading from this many bars; only SMA50 needs more than
            // 32/24 give it and comes back null, which the companion's own rules already say to
            // read as "unknown" - a smaller loss than breaking every reply.
            let short = feed.candles(s, "15", 32).unwrap_or_default();
            let long = feed.candles(s, "60", 24).unwrap_or_default();
            let row = |c: &crate::market::feed::Candle| json!([c.open_ms, c.open, c.high, c.low, c.close, c.volume]);
            let ta = |rows: &[crate::market::feed::Candle]| {
                let closes: Vec<f64> = rows.iter().map(|c| c.close).collect();
                let highs: Vec<f64> = rows.iter().map(|c| c.high).collect();
                let lows: Vec<f64> = rows.iter().map(|c| c.low).collect();
                let (stoch_k, stoch_d) = indicators::latest_stochastic(&highs, &lows, &closes, 14, 3);
                json!({
                    "rsi14": indicators::latest_rsi(&closes, 14),
                    "sma20": indicators::latest_sma(&closes, 20),
                    "sma50": indicators::latest_sma(&closes, 50),
                    "stochK": stoch_k,
                    "stochD": stoch_d,
                })
            };
            candles.insert(
                s.clone(),
                json!({
                    "15m": short.iter().map(row).collect::<Vec<_>>(),
                    "1h": long.iter().map(row).collect::<Vec<_>>(),
                    "indicators": { "15m": ta(&short), "1h": ta(&long) },
                }),
            );
        }
    }

    let state = crate::market::commands::state(conn, &board, wanted.first().map(String::as_str).unwrap_or("BTCUSDT")).ok();
    let account = state.as_ref().and_then(|s| serde_json::to_value(&s.account).ok()).unwrap_or(Value::Null);
    let positions = state.as_ref().and_then(|s| serde_json::to_value(&s.positions).ok()).unwrap_or_else(|| json!(shown));
    let setting = store::setting(conn, CRYPTO)?;
    let turns: Vec<Value> = store::turns(conn, run.id, DESK_TURNS, 0)?
        .into_iter()
        .map(|t| json!({ "at": t.created_at, "trigger": t.trigger_kind, "member": t.message, "reply": t.reply, "actions": t.actions_json.and_then(|a| serde_json::from_str::<Value>(&a).ok()) }))
        .collect();
    let lessons: Vec<Value> = store::lessons(conn, DESK_LESSONS)?.into_iter().map(|l| json!({ "symbol": l.symbol, "result%": l.result_pct, "by": l.traded_by, "lesson": l.lesson })).collect();
    let limits = json!({
        "roundEveryMinutes": setting.auto_every_minutes,
        "roundsADay": setting.auto_round_cap,
        "stopAtDailyLoss": setting.auto_daily_loss,
        "dailyProfitTarget": setting.auto_daily_target,
        "balanceTarget": setting.auto_balance_target,
    });
    Ok(desk::desk(
        trigger,
        account,
        json!(symbols),
        Value::Object(candles),
        serde_json::to_value(&scan).unwrap_or(Value::Null),
        positions,
        json!(resting),
        json!({ "taker": setting.taker_rate, "maker": setting.maker_rate }),
        json!(turns),
        limits,
        json!(lessons),
    ))
}

// ---------- a turn ----------

fn act(app: &AppHandle, conn: &mut Conn, run_id: u64, turn_id: u64, actions: &[Action], source: Source) -> Vec<Done> {
    let mut done = Vec::new();
    for (n, action) in actions.iter().enumerate() {
        let result = match action {
            Action::PlaceOrder { symbol, side, order_type, qty, price, tp, sl, .. } => {
                let input = OrderInput {
                    symbol: symbol.clone(),
                    side: side.clone(),
                    order_type: order_type.clone(),
                    qty: *qty,
                    price: *price,
                    tp: *tp,
                    sl: *sl,
                    reduce_only: false,
                    client_order_id: format!("companion-{turn_id}-{n}"),
                };
                place_as(app, &input, source).map(|o| if o.status == "FILLED" { "Filled".to_string() } else { "Resting".to_string() })
            }
            Action::ClosePosition { symbol, .. } => match store::open_positions(conn, run_id).ok().and_then(|ps| ps.into_iter().find(|p| p.symbol == *symbol)) {
                Some(p) => close_as(app, p.id, source).map(|_| "Closed".to_string()),
                None => Err(format!("No open position on {symbol}.")),
            },
        };
        match result {
            Ok(r) => done.push(Done { action: action.clone(), result: r, ok: true }),
            Err(why) => done.push(Done { action: action.clone(), result: why, ok: false }),
        }
    }
    done
}

/// One turn: the member's words (ASKED) or the companion's own round (AUTO).
pub fn turn(app: &AppHandle, trigger: &str, message: Option<String>) -> Result<TurnView, String> {
    if TURNING.swap(true, Ordering::SeqCst) {
        return Err("The companion is still thinking about the last turn.".into());
    }
    STOP.store(false, Ordering::SeqCst);
    let result = (|| {
        let mut conn = open(app)?;
        let run = store::open_run(&mut conn, CRYPTO)?.ok_or("Set your paper money first.")?;
        let message = message.map(|m| desk::clean_message(&m)).filter(|m| !m.is_empty());
        if trigger == "ASKED" && message.is_none() {
            return Err("Write something first.".into());
        }
        let settings = app.state::<SettingsStore>().get();
        let cli = settings.cli;
        let started = Instant::now();
        let turn_id = store::add_turn(&mut conn, run.id, &format!("turn-{}-{}", run.id, now_ms()), trigger, cli.label(), message.as_deref())?;
        let desk = desk_for(app, &mut conn, &run, trigger, message.as_deref());
        let desk = match desk {
            Ok(d) => d,
            Err(why) => {
                store::finish_turn(&mut conn, turn_id, "FAILED", Some(&why), None, None, started.elapsed().as_millis() as i64)?;
                return Err(why);
            }
        };
        let member = settings.member_name.clone().unwrap_or_else(|| "Your person".into());
        let words = desk::turn_words(&desk, trigger, message.as_deref(), &member);
        let folder = prepare(app, "market-desk", RULES)?;
        let answer: Result<Answer, String> = crate::oneshot::run(cli, &folder, "rules.md", &words, TURN_LIMIT).and_then(|text| desk::parse_answer(&text));
        let desk_text = serde_json::to_string(&desk).ok();
        let elapsed = || started.elapsed().as_millis() as i64;
        match answer {
            Err(why) => {
                store::finish_turn(&mut conn, turn_id, "FAILED", Some(&why), desk_text.as_deref(), None, elapsed())?;
                Err(why)
            }
            Ok(_) if STOP.load(Ordering::SeqCst) => {
                store::finish_turn(&mut conn, turn_id, "STOPPED", Some("Stopped before it acted."), desk_text.as_deref(), None, elapsed())?;
                Ok(TurnView { reply: "Stopped before it acted.".into(), done: vec![], status: "STOPPED".into() })
            }
            Ok(answer) => {
                let source = if trigger == "AUTO" { Source::CompanionAuto } else { Source::CompanionAsked };
                let done = act(app, &mut conn, run.id, turn_id, &answer.actions, source);
                let actions = serde_json::to_string(&done).ok();
                store::finish_turn(&mut conn, turn_id, "DONE", Some(&answer.reply), desk_text.as_deref(), actions.as_deref(), elapsed())?;
                Ok(TurnView { reply: answer.reply, done, status: "DONE".into() })
            }
        }
    })();
    TURNING.store(false, Ordering::SeqCst);
    crate::market::commands::changed(app);
    result
}

// ---------- rounds by itself ----------

/// Why the rounds stop now, if they must (pure): a cap, a loss limit, a target.
pub fn stop_reason(setting: &store::Setting, rounds_today: u32, result_today: f64, balance: f64) -> Option<String> {
    if rounds_today >= setting.auto_round_cap {
        return Some(format!("Today's {} rounds are done.", setting.auto_round_cap));
    }
    if result_today <= -setting.auto_daily_loss {
        return Some("Today's loss limit was reached.".into());
    }
    if setting.auto_daily_target.is_some_and(|t| result_today >= t) {
        return Some("Today's profit target was reached.".into());
    }
    if setting.auto_balance_target.is_some_and(|t| balance >= t) {
        return Some("The balance target was reached.".into());
    }
    None
}

fn turn_off(conn: &mut Conn, why: &str) {
    if let Ok(mut s) = store::setting(conn, CRYPTO) {
        s.auto_on = false;
        let _ = store::save_setting(conn, CRYPTO, &s);
    }
    *STOPPED_WHY.lock().unwrap() = Some(why.to_string());
}

fn round_due(app: &AppHandle, last_round: &mut Option<Instant>) -> Result<(), String> {
    let mut conn = open(app)?;
    let setting = store::setting(&mut conn, CRYPTO)?;
    if !setting.auto_on {
        return Ok(());
    }
    let Some(run) = store::open_run(&mut conn, CRYPTO)? else {
        turn_off(&mut conn, "The account is closed.");
        return Ok(());
    };
    if crate::market::engine::feed_view().status != "OK" {
        return Ok(());
    }
    if last_round.is_some_and(|t| t.elapsed() < Duration::from_secs(u64::from(setting.auto_every_minutes.max(1)) * 60)) {
        return Ok(());
    }
    let since = day_start(now_ms(), OFFSET_MIN.load(Ordering::Relaxed));
    let rounds = store::auto_rounds_since(&mut conn, run.id, since)?;
    let result = store::result_since(&mut conn, run.id, since)?;
    if let Some(why) = stop_reason(&setting, rounds, result, run.balance) {
        turn_off(&mut conn, &why);
        crate::market::commands::changed(app);
        return Ok(());
    }
    *last_round = Some(Instant::now());
    drop(conn);
    if let Ok(view) = turn(app, "AUTO", None) {
        let acted = view.done.iter().filter(|d| d.ok).count();
        let body = if acted > 0 { format!("{acted} action{} taken.", if acted == 1 { "" } else { "s" }) } else { "No trade this round.".to_string() };
        crate::alerts::fire(app, "market_round", "MARKET round finished", &body);
    }
    Ok(())
}

// ---------- lessons ----------

/// A closed trade's net result as a percent of the margin it used: the size it held at its
/// fullest (never the closed position's `qty`, which is 0), at its entry, over its leverage.
/// None when there is no size or price to measure against. Pure, tested.
pub fn result_on_margin(entry: f64, opened_qty: f64, leverage: Option<f64>, net: f64) -> Option<f64> {
    if !(entry.is_finite() && entry > 0.0 && opened_qty.is_finite() && opened_qty > 0.0 && net.is_finite()) {
        return None;
    }
    let margin = entry * opened_qty / leverage.filter(|l| l.is_finite()).unwrap_or(1.0).max(1.0);
    Some(net / margin * 100.0)
}

fn lesson_due(app: &AppHandle) -> Result<(), String> {
    let mut conn = open(app)?;
    let Some(run) = store::runs(&mut conn, CRYPTO)?.into_iter().next() else { return Ok(()) };
    let now = now_ms();
    let closed = store::closed_positions(&mut conn, run.id, 30)?;
    let Some(p) = closed.into_iter().rev().find(|p| p.closed_at.is_some_and(|c| now - c < LESSON_WINDOW_MS) && !store::has_lesson(&mut conn, p.id).unwrap_or(true)) else {
        return Ok(());
    };
    let net = p.realised_pnl - p.fees_paid - p.funding_paid;
    let opened = store::opened_qty(&mut conn, p.id, &p.side)?;
    // no size to measure against: no lesson rather than a made-up percent
    let Some(result_pct) = result_on_margin(p.entry_price, opened, p.leverage, net) else { return Ok(()) };
    let traded_by = if p.opened_by.starts_with("COMPANION") { "COMPANION" } else { "MEMBER" };
    let cycle = json!({
        "symbol": p.symbol, "side": p.side, "entry": p.entry_price, "leverage": p.leverage,
        "realisedPnl": p.realised_pnl, "feesPaid": p.fees_paid, "fundingPaid": p.funding_paid,
        "resultOnMarginPct": (result_pct * 100.0).round() / 100.0, "openedBy": traded_by,
        "openedAt": p.opened_at, "closedAt": p.closed_at,
    });
    let settings = app.state::<SettingsStore>().get();
    let member = settings.member_name.clone().unwrap_or_else(|| "your person".into());
    let folder = prepare(app, "market-lessons", LESSON_RULES)?;
    drop(conn);
    let text = crate::oneshot::run(settings.cli, &folder, "rules.md", &desk::lesson_words(&cycle, &member), LESSON_LIMIT)?;
    if let Some(lesson) = desk::clean_lesson(&text) {
        let mut conn = open(app)?;
        store::add_lesson(&mut conn, run.id, p.id, &p.symbol, &p.side, result_pct, traded_by, &lesson)?;
        crate::market::commands::changed(app);
    }
    Ok(())
}

/// Starts the rounds' clock and the lesson writer. Trading by itself is off again at every start.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("market-companion".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        if let Ok(mut conn) = open(&app) {
            if store::setting(&mut conn, CRYPTO).is_ok_and(|s| s.auto_on) {
                turn_off(&mut conn, "Castle started again, so trading by itself is off until you turn it on.");
            }
        }
        let mut last_round: Option<Instant> = None;
        loop {
            if !TURNING.load(Ordering::SeqCst) {
                let _ = round_due(&app, &mut last_round);
            }
            if !TURNING.load(Ordering::SeqCst) {
                let _ = lesson_due(&app);
            }
            std::thread::sleep(LOOK_EVERY);
        }
    });
}

// ---------- commands ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoView {
    pub setting: store::Setting,
    pub thinking: bool,
    pub stopped_why: Option<String>,
    pub rounds_today: u32,
    pub result_today: f64,
}

#[tauri::command]
pub async fn market_turn(app: AppHandle, message: String, tz_offset_minutes: i64) -> Result<TurnView, String> {
    OFFSET_MIN.store(tz_offset_minutes, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || turn(&app, "ASKED", Some(message))).await.map_err(|_| "The turn stopped.".to_string())?
}

#[tauri::command]
pub async fn market_auto_get(app: AppHandle, tz_offset_minutes: i64) -> Result<AutoView, String> {
    OFFSET_MIN.store(tz_offset_minutes, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open(&app)?;
        let setting = store::setting(&mut conn, CRYPTO)?;
        let since = day_start(now_ms(), tz_offset_minutes);
        let (rounds_today, result_today) = match store::open_run(&mut conn, CRYPTO)? {
            Some(run) => (store::auto_rounds_since(&mut conn, run.id, since)?, store::result_since(&mut conn, run.id, since)?),
            None => (0, 0.0),
        };
        Ok(AutoView { setting, thinking: TURNING.load(Ordering::SeqCst), stopped_why: STOPPED_WHY.lock().unwrap().clone(), rounds_today, result_today })
    })
    .await
    .map_err(|_| "The settings could not be read.".to_string())?
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn market_auto_save(app: AppHandle, on: bool, every: u32, cap: u32, loss: f64, target: Option<f64>, balance_target: Option<f64>, tz_offset_minutes: i64) -> Result<(), String> {
    OFFSET_MIN.store(tz_offset_minutes, Ordering::Relaxed);
    if !(1..=1440).contains(&every) || !(1..=500).contains(&cap) || !(loss.is_finite() && loss > 0.0) {
        return Err("A round every 1 to 1440 minutes, 1 to 500 rounds a day, and a loss limit above 0.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open(&app)?;
        let mut s = store::setting(&mut conn, CRYPTO)?;
        s.auto_on = on;
        s.auto_every_minutes = every;
        s.auto_round_cap = cap;
        s.auto_daily_loss = loss;
        s.auto_daily_target = target.filter(|t| t.is_finite() && *t > 0.0);
        s.auto_balance_target = balance_target.filter(|t| t.is_finite() && *t > 0.0);
        store::save_setting(&mut conn, CRYPTO, &s)?;
        if on {
            *STOPPED_WHY.lock().unwrap() = None;
        }
        crate::market::commands::changed(&app);
        Ok(())
    })
    .await
    .map_err(|_| "The settings could not be saved.".to_string())?
}

/// Stops trading by itself now; a turn already thinking is not allowed to act when it returns.
#[tauri::command]
pub async fn market_stop(app: AppHandle) -> Result<(), String> {
    STOP.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open(&app)?;
        turn_off(&mut conn, "You stopped it.");
        crate::market::commands::changed(&app);
        Ok(())
    })
    .await
    .map_err(|_| "Stop could not run.".to_string())?
}

#[tauri::command]
pub async fn market_turns(app: AppHandle, run_id: Option<u64>, page: u32) -> Result<Vec<store::Turn>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open(&app)?;
        let run = match run_id {
            Some(id) => Some(id),
            None => store::open_run(&mut conn, CRYPTO)?.map(|r| r.id),
        };
        match run {
            Some(id) => store::turns(&mut conn, id, 20, page * 20),
            None => Ok(vec![]),
        }
    })
    .await
    .map_err(|_| "The trade log could not be read.".to_string())?
}

#[tauri::command]
pub async fn market_lessons(app: AppHandle) -> Result<Vec<store::Lesson>, String> {
    tauri::async_runtime::spawn_blocking(move || store::lessons(&mut open(&app)?, 200)).await.map_err(|_| "The lessons could not be read.".to_string())?
}

#[tauri::command]
pub async fn market_lesson_delete(app: AppHandle, lesson_id: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || store::delete_lesson(&mut open(&app)?, lesson_id)).await.map_err(|_| "The lesson could not be deleted.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting() -> store::Setting {
        store::Setting {
            taker_rate: 0.00055, maker_rate: 0.0002, feed_id: "BYBIT".into(), auto_on: true, auto_every_minutes: 15, auto_round_cap: 20,
            auto_daily_loss: 500.0, auto_daily_target: Some(300.0), auto_balance_target: Some(12000.0),
        }
    }

    #[test]
    fn the_rounds_stop_on_a_cap_a_limit_or_a_target() {
        assert_eq!(stop_reason(&setting(), 3, 10.0, 10_000.0), None);
        assert!(stop_reason(&setting(), 20, 0.0, 10_000.0).unwrap().contains("20 rounds"));
        assert!(stop_reason(&setting(), 1, -500.0, 10_000.0).unwrap().contains("loss limit"));
        assert!(stop_reason(&setting(), 1, 300.0, 10_000.0).unwrap().contains("profit target"));
        assert!(stop_reason(&setting(), 1, 0.0, 12_000.0).unwrap().contains("balance target"));
    }

    #[test]
    fn a_lesson_measures_against_the_size_the_trade_held() {
        // 0.01 BTC at 60,000 with 10x uses 60 of margin; a net -0.30 is -0.5%
        let pct = result_on_margin(60_000.0, 0.01, Some(10.0), -0.30).unwrap();
        assert!((pct + 0.5).abs() < 1e-9, "{pct}");
        // the bug: a closed position's qty is 0, which once made fees alone read as about -1e9 %
        assert_eq!(result_on_margin(60_000.0, 0.0, Some(10.0), -0.30), None);
        assert_eq!(result_on_margin(0.0, 0.01, Some(10.0), -0.30), None);
        // no leverage reads as 1x
        let spot = result_on_margin(100.0, 2.0, None, 4.0).unwrap();
        assert!((spot - 2.0).abs() < 1e-9);
    }

    #[test]
    fn today_starts_at_the_members_midnight() {
        // 2026-11-16 03:00 UTC is 11:00 in UTC+8: that day began at 2026-11-15 16:00 UTC
        let now = crate::quota::iso_ms("2026-11-16T03:00:00Z").unwrap();
        assert_eq!(day_start(now, 480), crate::quota::iso_ms("2026-11-15T16:00:00Z").unwrap());
        assert_eq!(day_start(now, 0), crate::quota::iso_ms("2026-11-16T00:00:00Z").unwrap());
        assert_eq!(day_start(now, -300), crate::quota::iso_ms("2026-11-15T05:00:00Z").unwrap());
    }
}
