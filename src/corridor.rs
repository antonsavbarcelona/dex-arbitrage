//! The corridor paper run: the funding carry on the cheap venue pairs only, executed on the live books and prints
//! inside the WebSocket collector (`dexarb ws`), to measure what the price actually costs (or gives) on entry and exit.
//!
//! - The signal: the 5-minute funding snapshots. The spread = the short venue's rate - the long's (rate_h, + = longs
//!   pay). Enter when it has been >= THETA_APR for HELD_H hours in a row on a pair whose maker-taker round trip is
//!   <= MAX_RT_BP: the limit on the venue that makes it cheaper, the hedge at market on the other. Costs come from the
//!   venues' fees and each market's measured spread (data/spreads.json).
//! - The entry: a limit of USD on the maker venue, re-pegged to its best price at every book change. A fill needs a
//!   print of >= USD strictly through it. The hedge at market LAT_MS later (+ Lighter's 300 ms) at the other venue's
//!   best. Not filled within ENTRY_TIMEOUT: both legs at market (logged as such).
//! - The exit: after MIN_HOLD_H, once the spread is < 0 (or at MAX_HOLD_H), the mirror.
//! - The funding: each leg's settlement, detected by its venue's next_ms moving on, paid at the last snapshot's
//!   rate x interval.
//!
//! Logs: data/corridor.jsonl (every action), data/corridor.md (the summary, every 10 min),
//! data/corridor_state.json (the state, so a restart keeps the positions).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

const USD: f64 = 500.0;
const THETA_APR: f64 = 0.5;
const HELD_H: f64 = 4.0;
const MAX_RT_BP: f64 = 8.0;
const MIN_HOLD_H: f64 = 72.0;
const MAX_HOLD_H: f64 = 168.0;
const ENTRY_TIMEOUT_MS: i64 = 3_600_000;
const LAT_MS: i64 = 300;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Q {
    bid: f64,
    bsz: f64,
    ask: f64,
    asz: f64,
    t: i64,
}
impl Q {
    fn mid(&self) -> f64 {
        (self.bid + self.ask) / 2.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Leg {
    rate_h: f64,
    interval_h: f64,
    next_ms: i64,
}

/// A working order: the maker leg's limit (buy = true: the maker venue is the long leg).
#[derive(Clone, Serialize, Deserialize)]
struct Working {
    opening: bool,
    maker: String,
    taker: String,
    /// The maker leg buys (it is the long on an entry, the short on an exit).
    buy: bool,
    px: f64,
    since: i64,
    repegs: u32,
    /// The mids at the start (maker, taker) and the taker-taker cost then (bp), for the comparison.
    mid0: (f64, f64),
    tt_cost_bp: f64,
    /// A fill waiting for its hedge: (the fill time, the fill price, the hedge due).
    fill: Option<(i64, f64, i64)>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Pos {
    asset: String,
    long: String,
    short: String,
    rt_model_bp: f64,
    signal_at: i64,
    w: Option<Working>,
    /// Filled: the long's and the short's prices, the time.
    entry: Option<(f64, f64, i64)>,
    exit: Option<(f64, f64, i64)>,
    entry_cost_bp: f64,
    exit_cost_bp: f64,
    entry_mode: String,
    exit_mode: String,
    fees_bp: f64,
    funding_bp: f64,
    /// The legs' next settlement and its rate seen last: (long, short).
    legs: (Option<Leg>, Option<Leg>),
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    open: Vec<Pos>,
    closed: Vec<Pos>,
    /// The time each pair's spread has held >= THETA since (asset|long|short).
    held_since: HashMap<String, i64>,
}

#[derive(Default)]
struct C {
    s: State,
    books: HashMap<(String, String), Q>,
    fees: HashMap<(String, String), (f64, f64)>,
    spreads: HashMap<String, f64>,
    /// The latest funding by (venue, asset).
    rates: HashMap<(String, String), Leg>,
    last_save: i64,
    last_report: i64,
    loaded: bool,
}

static ST: Mutex<Option<C>> = Mutex::new(None);

fn dir() -> std::path::PathBuf {
    crate::data_dir()
}

fn log(v: serde_json::Value) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir().join("corridor.jsonl")) {
        let _ = writeln!(f, "{v}");
    }
}

fn with<R>(f: impl FnOnce(&mut C) -> R) -> R {
    let mut g = ST.lock().unwrap();
    let c = g.get_or_insert_with(C::default);
    if !c.loaded {
        c.loaded = true;
        c.s = std::fs::read_to_string(dir().join("corridor_state.json")).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_default();
        c.spreads = std::fs::read_to_string(dir().join("spreads.json")).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_default();
        if c.s.held_since.is_empty() {
            c.seed();
        }
    }
    f(c)
}

/// Lighter's built-in delay on a taker order (standard account); 0 elsewhere.
fn taker_bump(v: &str) -> i64 {
    if v == "Lighter" { 300 } else { 0 }
}

impl C {
    /// The persistence clocks from the snapshots already written (today's and yesterday's files), so a restart does not
    /// wait HELD_H for its first signals.
    fn seed(&mut self) {
        let day = crate::model::now_ms() / 86_400_000;
        let mut snaps: std::collections::BTreeMap<i64, HashMap<String, Vec<(String, f64)>>> = Default::default();
        for d in [day - 1, day] {
            let Ok(s) = std::fs::read_to_string(dir().join("ws").join(format!("funding_{d}.csv"))) else { continue };
            for l in s.lines() {
                let x: Vec<&str> = l.split(',').collect();
                if x.len() < 5 || x[1] == "GMX" {
                    continue;
                }
                let (Ok(t), Ok(r)) = (x[0].parse::<i64>(), x[4].parse::<f64>()) else { continue };
                snaps.entry(t).or_default().entry(x[2].to_string()).or_default().push((x[1].to_string(), r));
            }
        }
        for (t, by_asset) in snaps {
            for (a, vs) in by_asset {
                for (l, rl) in &vs {
                    for (s, rs) in &vs {
                        if l == s {
                            continue;
                        }
                        let key = format!("{a}|{l}|{s}");
                        if (rs - rl) * 8760.0 >= THETA_APR {
                            self.s.held_since.entry(key).or_insert(t);
                        } else {
                            self.s.held_since.remove(&key);
                        }
                    }
                }
            }
        }
    }

    fn spread(&self, v: &str, a: &str) -> f64 {
        self.spreads.get(&format!("{v}|{a}")).copied().unwrap_or(20.0) / 1e4
    }
    /// The maker-taker round trip (fraction) and the maker venue for (long, short).
    fn cost(&self, a: &str, l: &str, s: &str) -> Option<(f64, String)> {
        let (ml, tl) = *self.fees.get(&(l.to_string(), a.to_string()))?;
        let (ms, ts) = *self.fees.get(&(s.to_string(), a.to_string()))?;
        let maker_l = ml + ts + self.spread(s, a) / 2.0;
        let maker_s = ms + tl + self.spread(l, a) / 2.0;
        Some(if maker_l <= maker_s { (2.0 * maker_l + 2e-4, l.to_string()) } else { (2.0 * maker_s + 2e-4, s.to_string()) })
    }

    fn start_working(&mut self, p: &mut Pos, opening: bool, now: i64) -> bool {
        let Some((_, maker)) = self.cost(&p.asset, &p.long, &p.short) else { return false };
        let taker = if maker == p.long { p.short.clone() } else { p.long.clone() };
        let (Some(qm), Some(qt)) = (self.books.get(&(maker.clone(), p.asset.clone())), self.books.get(&(taker.clone(), p.asset.clone()))) else { return false };
        // the maker leg buys when it is the long on an entry or the short on an exit
        let buy = (maker == p.long) == opening;
        let px = if buy { qm.bid } else { qm.ask };
        // the taker-taker cost now: buy the long at its ask, sell the short at its bid (the mirror on an exit)
        let (ql, qs) = if maker == p.long { (qm, qt) } else { (qt, qm) };
        let tt = if opening { (ql.ask - ql.mid()) / ql.mid() + (qs.mid() - qs.bid) / qs.mid() } else { (ql.mid() - ql.bid) / ql.mid() + (qs.ask - qs.mid()) / qs.mid() };
        p.w = Some(Working { opening, maker, taker, buy, px, since: now, repegs: 0, mid0: (qm.mid(), qt.mid()), tt_cost_bp: tt * 1e4, fill: None });
        true
    }

    fn on_bbo(&mut self, venue: &str, asset: &str, q: Q) {
        self.books.insert((venue.to_string(), asset.to_string()), q);
        for p in self.s.open.iter_mut().filter(|p| p.asset == asset) {
            if let Some(w) = p.w.as_mut() {
                if w.fill.is_none() && w.maker == venue {
                    let np = if w.buy { q.bid } else { q.ask };
                    if np != w.px {
                        w.px = np;
                        w.repegs += 1;
                    }
                }
            }
        }
    }

    fn on_trade(&mut self, venue: &str, asset: &str, px: f64, sz: f64, now: i64) {
        if px * sz < USD {
            return;
        }
        for p in self.s.open.iter_mut().filter(|p| p.asset == asset) {
            if let Some(w) = p.w.as_mut() {
                if w.fill.is_none() && w.maker == venue && ((w.buy && px < w.px) || (!w.buy && px > w.px)) {
                    w.fill = Some((now, w.px, now + LAT_MS + taker_bump(&w.taker)));
                }
            }
        }
    }

    /// Finish a working order: the maker leg at `mpx`, the taker leg at the taker venue's best now.
    fn complete(&mut self, i: usize, mpx: f64, mode: &str, now: i64) {
        let p = &self.s.open[i];
        let w = p.w.clone().unwrap();
        let Some(qt) = self.books.get(&(w.taker.clone(), p.asset.clone())).copied() else { return };
        let Some(qm) = self.books.get(&(w.maker.clone(), p.asset.clone())).copied() else { return };
        // the taker leg trades the other way from the maker leg
        let tpx = if w.buy { qt.bid } else { qt.ask };
        let (lpx, spx) = if w.maker == p.long { (mpx, tpx) } else { (tpx, mpx) };
        let (fm, ft) = (self.fees.get(&(w.maker.clone(), p.asset.clone())).copied().unwrap_or((5e-4, 5e-4)), self.fees.get(&(w.taker.clone(), p.asset.clone())).copied().unwrap_or((5e-4, 5e-4)));
        let fee = if mode == "maker" { fm.0 + ft.1 } else { fm.1 + ft.1 };
        // the execution cost vs the mids at the start (the long bought / the short sold above / below them; the mirror on the exit)
        let (ml0, ms0) = if w.maker == p.long { w.mid0 } else { (w.mid0.1, w.mid0.0) };
        let cost = if w.opening { (lpx - ml0) / ml0 + (ms0 - spx) / ms0 } else { (ml0 - lpx) / ml0 + (spx - ms0) / ms0 };
        let wait_s = (now - w.since) as f64 / 1000.0;
        let p = &mut self.s.open[i];
        p.fees_bp += fee * 1e4;
        if w.opening {
            p.entry = Some((lpx, spx, now));
            p.entry_cost_bp = cost * 1e4;
            p.entry_mode = mode.into();
        } else {
            p.exit = Some((lpx, spx, now));
            p.exit_cost_bp = cost * 1e4;
            p.exit_mode = mode.into();
        }
        p.w = None;
        log(serde_json::json!({"t": now, "what": if w.opening { "entered" } else { "exited" }, "asset": p.asset, "long": p.long, "short": p.short, "maker": w.maker, "mode": mode,
            "long_px": lpx, "short_px": spx, "cost_vs_mids_bp": cost * 1e4, "taker_taker_cost_at_start_bp": w.tt_cost_bp, "fees_bp": fee * 1e4, "wait_s": wait_s, "repegs": w.repegs,
            "maker_mid_now": qm.mid()}));
    }

    fn tick(&mut self, now: i64) {
        let mut i = 0;
        while i < self.s.open.len() {
            let w = self.s.open[i].w.clone();
            if let Some(w) = w {
                if let Some((_, fpx, due)) = w.fill {
                    if now >= due {
                        self.complete(i, fpx, "maker", now);
                    }
                } else if now - w.since >= ENTRY_TIMEOUT_MS {
                    // the maker leg at market too
                    if let Some(qm) = self.books.get(&(w.maker.clone(), self.s.open[i].asset.clone())).copied() {
                        let mpx = if w.buy { qm.ask } else { qm.bid };
                        self.complete(i, mpx, "timeout", now);
                    }
                }
            }
            // a closed position moves to the closed list
            if self.s.open[i].exit.is_some() {
                let p = self.s.open.remove(i);
                let (le, se, t0) = p.entry.unwrap();
                let (lx, sx, t1) = p.exit.unwrap();
                let price_bp = ((lx - le) / le + (se - sx) / se) * 1e4;
                log(serde_json::json!({"t": now, "what": "closed", "asset": p.asset, "long": p.long, "short": p.short, "hold_h": (t1 - t0) as f64 / 3.6e6,
                    "funding_bp": p.funding_bp, "price_bp": price_bp, "fees_bp": p.fees_bp, "net_bp": p.funding_bp + price_bp - p.fees_bp,
                    "entry_cost_bp": p.entry_cost_bp, "exit_cost_bp": p.exit_cost_bp, "rt_model_bp": p.rt_model_bp}));
                self.s.closed.push(p);
                continue;
            }
            i += 1;
        }
        if now - self.last_save > 60_000 {
            self.last_save = now;
            let _ = std::fs::write(dir().join("corridor_state.json"), serde_json::to_string(&self.s).unwrap_or_default());
        }
        if now - self.last_report > 600_000 {
            self.last_report = now;
            self.report(now);
        }
    }

    fn on_funding(&mut self, now: i64, rows: Vec<(String, String, f64, f64, i64)>, fees: HashMap<(String, String), (f64, f64)>) {
        if !fees.is_empty() {
            self.fees = fees;
        }
        for (v, a, r, iv, nx) in rows {
            self.rates.insert((v, a), Leg { rate_h: r, interval_h: iv, next_ms: nx });
        }
        // the settlements since the last snapshot: each leg's next_ms moved on -> paid at the rate seen before
        for p in self.s.open.iter_mut().filter(|p| p.entry.is_some()) {
            for (k, venue, sign) in [(0usize, p.long.clone(), 1.0), (1, p.short.clone(), -1.0)] {
                let Some(now_leg) = self.rates.get(&(venue.clone(), p.asset.clone())).cloned() else { continue };
                let prev = if k == 0 { p.legs.0.clone() } else { p.legs.1.clone() };
                if let Some(pl) = prev {
                    if now_leg.next_ms > pl.next_ms && pl.next_ms > p.entry.unwrap().2 {
                        // a long pays rate x interval (sign +1 for the long leg as a cost)
                        let pay = pl.rate_h * pl.interval_h;
                        p.funding_bp += -sign * pay * 1e4;
                    }
                }
                if k == 0 { p.legs.0 = Some(now_leg) } else { p.legs.1 = Some(now_leg) }
            }
        }
        // the signals: every pair of venues of an asset, the spread held >= THETA for HELD_H
        let mut by_asset: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for ((v, a), l) in &self.rates {
            by_asset.entry(a.clone()).or_default().push((v.clone(), l.rate_h));
        }
        let mut new = Vec::new();
        for (a, vs) in &by_asset {
            for (l, rl) in vs {
                for (s, rs) in vs {
                    if l == s {
                        continue;
                    }
                    let key = format!("{a}|{l}|{s}");
                    let spread = (rs - rl) * 8760.0;
                    if spread >= THETA_APR {
                        self.s.held_since.entry(key.clone()).or_insert(now);
                    } else {
                        self.s.held_since.remove(&key);
                        continue;
                    }
                    let held = (now - self.s.held_since[&key]) as f64 / 3.6e6;
                    if held < HELD_H || self.s.open.iter().any(|p| p.asset == *a && p.long == *l && p.short == *s) {
                        continue;
                    }
                    let Some((rt, _)) = self.cost(a, l, s) else { continue };
                    if rt * 1e4 > MAX_RT_BP {
                        continue;
                    }
                    new.push(Pos { asset: a.clone(), long: l.clone(), short: s.clone(), rt_model_bp: rt * 1e4, signal_at: now, w: None, entry: None, exit: None,
                        entry_cost_bp: 0.0, exit_cost_bp: 0.0, entry_mode: String::new(), exit_mode: String::new(), fees_bp: 0.0, funding_bp: 0.0, legs: (None, None) });
                }
            }
        }
        for mut p in new {
            if self.start_working(&mut p, true, now) {
                log(serde_json::json!({"t": now, "what": "signal", "asset": p.asset, "long": p.long, "short": p.short, "rt_model_bp": p.rt_model_bp,
                    "spread_apr": (self.rates[&(p.short.clone(), p.asset.clone())].rate_h - self.rates[&(p.long.clone(), p.asset.clone())].rate_h) * 8760.0, "maker": p.w.as_ref().map(|w| w.maker.clone())}));
                self.s.open.push(p);
            }
        }
        // the exits: held MIN_HOLD_H and the spread < 0, or MAX_HOLD_H
        let mut idx = Vec::new();
        for (i, p) in self.s.open.iter().enumerate() {
            let Some((_, _, t0)) = p.entry else { continue };
            if p.w.is_some() || p.exit.is_some() {
                continue;
            }
            let h = (now - t0) as f64 / 3.6e6;
            let sp = match (self.rates.get(&(p.short.clone(), p.asset.clone())), self.rates.get(&(p.long.clone(), p.asset.clone()))) {
                (Some(s), Some(l)) => s.rate_h - l.rate_h,
                _ => 0.0,
            };
            if (h >= MIN_HOLD_H && sp < 0.0) || h >= MAX_HOLD_H {
                idx.push(i);
            }
        }
        for i in idx {
            let mut p = self.s.open[i].clone();
            if self.start_working(&mut p, false, now) {
                self.s.open[i] = p;
            }
        }
    }

    fn report(&self, now: i64) {
        let mut r = String::new();
        use std::fmt::Write as _;
        let _ = writeln!(r, "# Corridor paper run ({})\n", now / 1000);
        let _ = writeln!(r, "Rules: spread >= {:.0}% APR held {HELD_H} h, round trip <= {MAX_RT_BP} bp, ${USD} a leg, out after {MIN_HOLD_H} h once the spread < 0 (max {MAX_HOLD_H} h).\n", THETA_APR * 100.0);
        let all: Vec<&Pos> = self.s.open.iter().chain(self.s.closed.iter()).collect();
        let entered: Vec<&&Pos> = all.iter().filter(|p| p.entry.is_some()).collect();
        let maker = entered.iter().filter(|p| p.entry_mode == "maker").count();
        let m = |v: Vec<f64>| if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 };
        let _ = writeln!(r, "Signals {} · entered {} (maker fill {}, timeout {}) · working {} · closed {}\n", all.len(), entered.len(), maker, entered.len() - maker,
            self.s.open.iter().filter(|p| p.w.is_some()).count(), self.s.closed.len());
        let _ = writeln!(r, "Entry cost vs the mids at the start: maker fills {:+.1} bp, timeouts {:+.1} bp; the model's half round trip {:.1} bp\n",
            m(entered.iter().filter(|p| p.entry_mode == "maker").map(|p| p.entry_cost_bp + p.fees_bp).collect()), m(entered.iter().filter(|p| p.entry_mode != "maker").map(|p| p.entry_cost_bp).collect()),
            m(entered.iter().map(|p| p.rt_model_bp / 2.0).collect()));
        let _ = writeln!(r, "| asset | long | short | state | entry cost bp | mode | funding bp | price bp (MTM) | fees bp | net bp | held h |\n|---|---|---|---|---|---|---|---|---|---|---|");
        for p in &all {
            let state = if p.exit.is_some() { "closed" } else if p.w.as_ref().is_some_and(|w| w.opening) { "entering" } else if p.w.is_some() { "exiting" } else { "open" };
            let (price, held) = match p.entry {
                Some((le, se, t0)) => {
                    let (lx, sx, t1) = p.exit.unwrap_or_else(|| {
                        let l = self.books.get(&(p.long.clone(), p.asset.clone())).map_or(le, |q| q.mid());
                        let s = self.books.get(&(p.short.clone(), p.asset.clone())).map_or(se, |q| q.mid());
                        (l, s, now)
                    });
                    (((lx - le) / le + (se - sx) / se) * 1e4, (t1 - t0) as f64 / 3.6e6)
                }
                None => (0.0, 0.0),
            };
            let _ = writeln!(r, "| {} | {} | {} | {state} | {:+.1} | {} | {:+.1} | {:+.1} | {:.1} | {:+.1} | {:.1} |", p.asset, p.long, p.short, p.entry_cost_bp, p.entry_mode, p.funding_bp, price, p.fees_bp,
                p.funding_bp + price - p.fees_bp, held);
        }
        let _ = std::fs::write(dir().join("corridor.md"), r);
    }
}

pub fn on_bbo(venue: &str, asset: &str, bid: f64, bsz: f64, ask: f64, asz: f64, now: i64) {
    with(|c| c.on_bbo(venue, asset, Q { bid, bsz, ask, asz, t: now }));
}
pub fn on_trade(venue: &str, asset: &str, px: f64, sz: f64, now: i64) {
    with(|c| c.on_trade(venue, asset, px, sz, now));
}
pub fn tick(now: i64) {
    with(|c| c.tick(now));
}
/// rows: (venue, asset, rate_h, interval_h, next_ms); fees: (venue, asset) -> (maker, taker).
pub fn on_funding(now: i64, rows: Vec<(String, String, f64, f64, i64)>, fees: HashMap<(String, String), (f64, f64)>) {
    with(|c| c.on_funding(now, rows, fees));
}
