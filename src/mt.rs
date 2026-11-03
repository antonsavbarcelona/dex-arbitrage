//! The maker-taker simulator on the WebSocket data (data/ws/*.csv[.gz]): a resting limit on venue A priced off venue B's
//! book, the hedge at market on B the moment the limit fills; the exit the mirror. Per pair (asset, A, B), both ways:
//!
//!  * the pair's normal level mu: the EMA (30 min) of the mid basis B / A - 1 on a 1-minute grid, from the minutes before;
//!  * flat: a bid on A at  bid_B(t - Lq) / (1 + mu) x (1 - e)  and an ask at  ask_B(t - Lq) / (1 + mu) x (1 + e), where
//!    e = A's maker fee + B's taker fee + 1 bp of slippage + the target; a quote that would cross A's book is not posted;
//!  * a fill: a print on A strictly through the quote (a sell print under our bid / a buy print over our ask), at our
//!    price; the hedge: B's best on the other side at t + Lh (the latency), a market order (B's taker fee);
//!  * in a position: the closing quote on A (the mirror, the same e), the same fill rule, the hedge back on B at t + Lh;
//!    after `max_hold` both legs out at market (A's taker fee then);
//!  * the result of a round trip, fraction of the notional: the entry basis - the exit basis - the fees; the adverse move
//!    of the hedge (B's best between the fill and the hedge) reported apart. Funding not counted (the holds are short).
//!
//! The venues' own delays on top (Lighter standard: 200 ms on a maker / cancel action, 300 ms on a taker order; premium
//! `--lighter-premium`: 0 / 200 ms and its fees). Timestamps: ours (one clock for every venue).
//! `dexarb mt [--lq MS] [--lh MS] [--hold-min M] [--assets A,B] [--lighter-premium]`

use crate::model::*;
use crate::registry::Registry;
use crate::venues::ALL;
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};

#[derive(Clone, Copy)]
struct Bb {
    t: i64,
    bid: f64,
    bsz: f64,
    ask: f64,
}

#[derive(Clone, Copy)]
struct Tr {
    t: i64,
    px: f64,
    side: i8,
}

#[derive(Default)]
struct Series {
    bb: Vec<Bb>,
    tr: Vec<Tr>,
}

/// The data: asset -> venue -> its books and trades, time-ordered.
fn load(only: &Option<Vec<String>>) -> Result<BTreeMap<String, HashMap<String, Series>>> {
    let dir = crate::data_dir().join("ws");
    let mut files: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| {
        let n = p.file_name().and_then(|x| x.to_str()).unwrap_or("");
        n.ends_with(".csv") || n.ends_with(".csv.gz")
    }).collect();
    files.sort();
    let mut m: BTreeMap<String, HashMap<String, Series>> = BTreeMap::new();
    for p in files {
        let f = std::fs::File::open(&p)?;
        let r: Box<dyn BufRead> = if p.to_string_lossy().ends_with(".gz") { Box::new(BufReader::new(flate2::read::MultiGzDecoder::new(f))) } else { Box::new(BufReader::new(f)) };
        for line in r.lines() {
            let Ok(line) = line else { break };
            let x: Vec<&str> = line.split(',').collect();
            if x.len() < 8 {
                continue;
            }
            if only.as_ref().is_some_and(|o| !o.iter().any(|a| a == x[4])) {
                continue;
            }
            let t: i64 = x[1].parse().unwrap_or(0);
            let s = m.entry(x[4].to_string()).or_default().entry(x[3].to_string()).or_default();
            match x[0] {
                "B" if x.len() >= 9 => {
                    let (bid, bsz, ask) = (x[5].parse().unwrap_or(0.0), x[6].parse().unwrap_or(0.0), x[7].parse().unwrap_or(0.0));
                    if bid > 0.0 && ask > 0.0 && ask >= bid {
                        s.bb.push(Bb { t, bid, bsz, ask });
                    }
                }
                "T" => {
                    let px: f64 = x[5].parse().unwrap_or(0.0);
                    if px > 0.0 {
                        s.tr.push(Tr { t, px, side: x[7].parse().unwrap_or(0) });
                    }
                }
                _ => {}
            }
        }
    }
    for v in m.values_mut() {
        for s in v.values_mut() {
            s.bb.sort_by_key(|b| b.t);
            s.tr.sort_by_key(|b| b.t);
        }
    }
    Ok(m)
}

/// The book at or before t.
fn at(bb: &[Bb], t: i64) -> Option<Bb> {
    let i = bb.partition_point(|b| b.t <= t);
    (i > 0).then(|| bb[i - 1])
}

#[derive(Clone, Default)]
pub struct Res {
    pub trips: usize,
    pub sum: f64,
    pub wins: usize,
    pub worst: f64,
    pub forced: usize,
    pub adverse: f64,
    pub entries: usize,
    pub first_half: f64,
    pub n_first: usize,
}

struct P<'a> {
    a: &'a Series,
    b: &'a Series,
    fee_a_maker: f64,
    fee_a_taker: f64,
    fee_b_taker: f64,
    /// The venues' own delays: A's on posting / cancelling a limit (the quote that much staler), B's on a taker order.
    a_maker_ms: i64,
    b_taker_ms: i64,
}

/// A venue's built-in delays (ms): (on a maker / cancel action, on a taker order). Lighter (apidocs.lighter.xyz, account
/// types): a standard account (0 fees) 200 / 300; premium ~0 / ~200 (its fees 0.004% / 0.028% at the 0-LIT tier). The
/// others publish none: 0 (their network latency is in `--lq` / `--lh`).
fn bumps(venue: &str, lighter_premium: bool) -> (i64, i64) {
    match (venue, lighter_premium) {
        ("Lighter", false) => (200, 300),
        ("Lighter", true) => (0, 200),
        _ => (0, 0),
    }
}

/// One pair, one target: both directions (long A / short B, short A / long B), one position at a time.
fn sim(p: &P, target: f64, lq: i64, lh: i64, hold_ms: i64, mid_t: i64) -> Res {
    let mut r = Res { worst: 0.0, ..Default::default() };
    // mu on a 1-minute grid: the EMA (30 min) of the mid basis at each minute's end
    let (t0, t1) = (p.a.bb.first().map_or(0, |x| x.t).max(p.b.bb.first().map_or(0, |x| x.t)), p.a.bb.last().map_or(0, |x| x.t).min(p.b.bb.last().map_or(0, |x| x.t)));
    if t1 - t0 < 1_800_000 {
        return r;
    }
    let mut mu: HashMap<i64, f64> = HashMap::new();
    let mut e: Option<f64> = None;
    let a_ = 1.0 - (-1.0f64 / 30.0).exp();
    for m in (t0 / 60_000)..=(t1 / 60_000) {
        if let (Some(x), Some(y)) = (at(&p.a.bb, m * 60_000 + 59_999), at(&p.b.bb, m * 60_000 + 59_999)) {
            let basis = ((y.bid + y.ask) / 2.0) / ((x.bid + x.ask) / 2.0) - 1.0;
            e = Some(match e { Some(v) => v + a_ * (basis - v), None => basis });
            mu.insert(m, e.unwrap());
        }
    }
    let warm = t0 + 1_800_000;
    let edge = p.fee_a_maker + p.fee_b_taker + 1e-4 + target;
    // the position: (dir: +1 long A, entry A price, entry B price, the time)
    let mut pos: Option<(i8, f64, f64, i64)> = None;
    for tr in &p.a.tr {
        if tr.t < warm {
            continue;
        }
        let Some(&m) = mu.get(&(tr.t / 60_000 - 1)) else { continue };
        let (Some(bq), Some(aq)) = (at(&p.b.bb, tr.t - lq - p.a_maker_ms), at(&p.a.bb, tr.t)) else { continue };
        match pos {
            None => {
                let buy = bq.bid / (1.0 + m) * (1.0 - edge);
                let sell = bq.ask / (1.0 + m) * (1.0 + edge);
                // a sell print under our resting bid (a bid that would not cross A's ask)
                let fill_buy = buy < aq.ask && tr.px < buy && tr.side <= 0;
                let fill_sell = sell > aq.bid && tr.px > sell && tr.side >= 0;
                if fill_buy || fill_sell {
                    let Some(h) = at(&p.b.bb, tr.t + lh + p.b_taker_ms) else { continue };
                    let dir: i8 = if fill_buy { 1 } else { -1 };
                    let (pa, pb) = if fill_buy { (buy, h.bid) } else { (sell, h.ask) };
                    let ref_b = if fill_buy { at(&p.b.bb, tr.t).map_or(h.bid, |x| x.bid) } else { at(&p.b.bb, tr.t).map_or(h.ask, |x| x.ask) };
                    r.adverse += f64::from(dir) * (pb - ref_b) / ref_b;
                    r.entries += 1;
                    pos = Some((dir, pa, pb, tr.t));
                }
            }
            Some((dir, pa, pb, t_in)) => {
                let forced = tr.t - t_in >= hold_ms;
                let close = if dir > 0 { bq.ask / (1.0 + m) * (1.0 + edge) } else { bq.bid / (1.0 + m) * (1.0 - edge) };
                let filled = if dir > 0 { close > aq.bid && tr.px > close && tr.side >= 0 } else { close < aq.ask && tr.px < close && tr.side <= 0 };
                if !(filled || forced) {
                    continue;
                }
                let Some(h) = at(&p.b.bb, tr.t + lh + p.b_taker_ms) else { continue };
                let (xa, xb, fa) = if forced {
                    (if dir > 0 { aq.bid } else { aq.ask }, if dir > 0 { h.ask } else { h.bid }, p.fee_a_taker)
                } else {
                    (close, if dir > 0 { h.ask } else { h.bid }, p.fee_a_maker)
                };
                // long A / short B: (sell B - buy A) - (buy B - sell A); the mirror for short A
                let d = f64::from(dir);
                let gross = d * ((pb - pa) - (xb - xa)) / pa;
                let net = gross - p.fee_a_maker - fa - 2.0 * p.fee_b_taker;
                r.trips += 1;
                r.sum += net;
                r.wins += (net > 0.0) as usize;
                r.worst = r.worst.min(net);
                r.forced += forced as usize;
                if t_in < mid_t {
                    r.first_half += net;
                    r.n_first += 1;
                }
                pos = None;
            }
        }
    }
    r
}

pub async fn run(h: &crate::http::Http, args: &[String]) -> Result<()> {
    let arg = |k: &str, d: f64| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).and_then(|x| x.parse::<f64>().ok()).unwrap_or(d);
    let (lq, lh, hold) = (arg("--lq", 200.0) as i64, arg("--lh", 300.0) as i64, (arg("--hold-min", 240.0) * 60_000.0) as i64);
    let premium = args.iter().any(|a| a == "--lighter-premium");
    let only: Option<Vec<String>> = args.iter().position(|a| a == "--assets").and_then(|i| args.get(i + 1)).map(|x| x.split(',').map(|y| y.trim().to_uppercase()).collect());
    let reg = Registry::load(h, &ALL).await;
    let fees: HashMap<(String, String), (f64, f64)> = reg.by_venue.values().flatten().map(|i| ((i.venue.clone(), i.base.clone()), (f(i.fees.maker), f(i.fees.taker)))).collect();
    let data = load(&only)?;
    let (tmin, tmax) = data.values().flat_map(|v| v.values()).flat_map(|s| s.bb.first().zip(s.bb.last())).fold((i64::MAX, 0i64), |a, (x, y)| (a.0.min(x.t), a.1.max(y.t)));
    let mid_t = tmin + (tmax - tmin) / 2;
    println!("data: {} assets, {:.1} h ({} .. {}); the quote off B's book {} ms old, the hedge {} ms after the fill (+ the venue's own delay: Lighter {}), out at market after {} min",
        data.len(), (tmax - tmin) as f64 / 3.6e6, tmin / 1000, tmax / 1000, lq, lh, if premium { "premium 0 / 200 ms, fees 0.004% / 0.028%" } else { "standard 200 / 300 ms, fees 0 / 0" }, hold / 60_000);
    let targets = [0.0, 5e-4, 1e-3, 2e-3];
    let mut rows: Vec<(String, String, String, f64, Res, f64)> = Vec::new();
    let mut tot: Vec<Res> = vec![Res::default(); targets.len()];
    for (asset, vs) in &data {
        for (va, sa) in vs {
            if sa.tr.len() < 20 {
                continue;
            }
            for (vb, sb) in vs {
                if va == vb || sb.bb.len() < 20 {
                    continue;
                }
                // the price check: the median mid ratio within 2% (else another token under the same ticker)
                let ratio = {
                    let mut v: Vec<f64> = sb.bb.iter().step_by((sb.bb.len() / 200).max(1)).filter_map(|y| at(&sa.bb, y.t).map(|x| ((y.bid + y.ask) / (x.bid + x.ask)))).collect();
                    v.sort_by(|a, b| a.total_cmp(b));
                    v.get(v.len() / 2).copied().unwrap_or(0.0)
                };
                if !(0.98..=1.02).contains(&ratio) {
                    continue;
                }
                let fee = |v: &String| {
                    if v == "Lighter" && premium { (0.00004, 0.00028) } else { fees.get(&(v.clone(), asset.clone())).copied().unwrap_or((0.0002, 0.0005)) }
                };
                let (fa, fb) = (fee(va), fee(vb));
                let (ba, bb) = (bumps(va, premium), bumps(vb, premium));
                let p = P { a: sa, b: sb, fee_a_maker: fa.0, fee_a_taker: fa.1, fee_b_taker: fb.1, a_maker_ms: ba.0, b_taker_ms: bb.1 };
                for (k, &tg) in targets.iter().enumerate() {
                    let r = sim(&p, tg, lq, lh, hold, mid_t);
                    if r.trips > 0 {
                        let t = &mut tot[k];
                        t.trips += r.trips;
                        t.sum += r.sum;
                        t.wins += r.wins;
                        t.forced += r.forced;
                        t.entries += r.entries;
                        t.adverse += r.adverse;
                        t.first_half += r.first_half;
                        t.n_first += r.n_first;
                        t.worst = t.worst.min(r.worst);
                        rows.push((asset.clone(), va.clone(), vb.clone(), tg, r, fa.0 + fb.1));
                    }
                }
            }
        }
    }
    let days = ((tmax - tmin) as f64 / 8.64e7).max(1e-9);
    println!("\nall pairs, by the target (the edge over the fees each side):");
    println!("{:>8} {:>8} {:>10} {:>7} {:>8} {:>9} {:>12} {:>14} {:>14}", "target", "trips", "trips/day", "win", "forced", "mean bp", "total bp", "adverse bp/fill", "half1 / half2");
    for (k, t) in tot.iter().enumerate() {
        let h2 = t.trips - t.n_first;
        println!("{:>7.0}bp {:>8} {:>10.0} {:>6.0}% {:>7.0}% {:>9.2} {:>12.0} {:>14.2} {:>7.2} / {:.2}", targets[k] * 1e4, t.trips, t.trips as f64 / days, t.wins as f64 / t.trips.max(1) as f64 * 100.0,
            t.forced as f64 / t.trips.max(1) as f64 * 100.0, t.sum / t.trips.max(1) as f64 * 1e4, t.sum * 1e4, t.adverse / t.entries.max(1) as f64 * 1e4,
            t.first_half / t.n_first.max(1) as f64 * 1e4, (t.sum - t.first_half) / h2.max(1) as f64 * 1e4);
    }
    rows.sort_by(|a, b| b.4.sum.total_cmp(&a.4.sum));
    println!("\nthe best pairs (A = the limit's venue, B = the hedge's), total bp over the data:");
    println!("{:<10} {:<11} {:<11} {:>6} {:>6} {:>7} {:>8} {:>9} {:>9} {:>8}", "asset", "A (limit)", "B (hedge)", "target", "trips", "win", "forced", "mean bp", "total bp", "fees bp");
    for (asset, va, vb, tg, r, fee) in rows.iter().filter(|x| x.4.trips >= 3).take(40) {
        println!("{:<10} {:<11} {:<11} {:>5.0}bp {:>6} {:>6.0}% {:>7.0}% {:>9.2} {:>9.0} {:>8.1}", asset, va, vb, tg * 1e4, r.trips, r.wins as f64 / r.trips as f64 * 100.0,
            r.forced as f64 / r.trips as f64 * 100.0, r.sum / r.trips as f64 * 1e4, r.sum * 1e4, fee * 1e4);
    }
    Ok(())
}
