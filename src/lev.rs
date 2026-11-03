//! The leverage study: the funding-stability study's chosen trades (data/fstab_trades.json, `dexarb fstab`) replayed on
//! each leg's own 15-minute bars (high / low) to see what a leverage does to them - the liquidations and the guards.
//!
//! The account: one leg a venue, cross margin, the venue's equity E = N / L (L the EFFECTIVE leverage: the notional over
//! the money on that venue - a "reserve" left on the venue in cross is the same as a lower L). The capital 2 N / L.
//! A leg is liquidated when its venue's equity at the bar's worst price (the long's low, the short's high) falls under
//! the maintenance margin (0.5 / the market's max leverage of the notional): the venue's equity is lost.
//! The guards (policies):
//!   none      a liquidation: the other leg closed at market; the trade ends.
//!   rehedge   a liquidation: half the other venue's equity sent over (arrives after T, the other leg naked meanwhile),
//!             the leg reopened at market at the same size.
//!   rebal     a venue's equity under 50% of its start: the excess of the richer venue (over its initial margin) sent so
//!             both are equal (arrives after T); 75% of the way to the liquidation with a transfer pending: both legs cut
//!             by a third at market; a liquidation anyway: as rehedge.
//! The costs at market: 7 bp a leg (the taker + half the spread); the entry / exit as in fstab (the trade's rt). The
//! price PnL of the two legs (their venues' prices diverging) counted at the exit's closes.
//! A leg's venue without public bars (Lighter, GMX) takes another venue's bars for the same asset (no divergence on it).

use crate::model::f;
use crate::registry::{self, Registry};
use crate::venues::{candles, ALL, V};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

const TK: f64 = 0.0007;

#[derive(Clone, Copy)]
struct B {
    c: f64,
    hi: f64,
    lo: f64,
}

struct Trade {
    asset: String,
    long: String,
    short: String,
    entry: i64,
    exit: i64,
    funding: f64,
    rt: f64,
    half: i64,
    stock: bool,
}

/// One leg's bars by the bar index (ts / 15 min), the closes median-filtered (a bad print), the high / low clipped to 30%
/// off the close.
fn clean(v: Vec<candles::Bar>) -> BTreeMap<i64, B> {
    let ks: Vec<(i64, f64, f64, f64)> = v.iter().map(|b| (b.ts / candles::BAR_MS, b.close, b.hi, b.lo)).collect();
    let mut out = BTreeMap::new();
    for i in 0..ks.len() {
        let lo_i = i.saturating_sub(2);
        let hi_i = (i + 3).min(ks.len());
        let mut w: Vec<f64> = ks[lo_i..hi_i].iter().map(|x| x.1).collect();
        w.sort_by(|a, b| a.total_cmp(b));
        let med = w[w.len() / 2];
        let c = if (ks[i].1 / med - 1.0).abs() > 0.05 { med } else { ks[i].1 };
        out.insert(ks[i].0, B { c, hi: ks[i].2.min(c * 1.3).max(c), lo: ks[i].3.max(c / 1.3).min(c) });
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Pol {
    None,
    Rehedge,
    Rebal,
}

#[derive(Default, Clone)]
struct Res {
    pnl: f64,
    liq: u32,
    cut: u32,
    xfer: u32,
    hold_h: f64,
    price: f64,
}

/// One trade at the effective leverage l under the policy; the result per unit of one leg's notional.
/// A trade both legs of which are the same token with sane bars: the entry prices within 2%, the closes never 30% apart.
fn valid(t: &Trade, bl: &BTreeMap<i64, B>, bs: &BTreeMap<i64, B>) -> bool {
    let (k0, k1) = (t.entry / candles::BAR_MS, t.exit / candles::BAR_MS);
    let (Some(a), Some(b)) = (bl.range(..=k0).next_back(), bs.range(..=k0).next_back()) else { return false };
    let (pa, pb) = (a.1.c, b.1.c);
    if (pa / pb - 1.0).abs() > 0.02 {
        return false;
    }
    (k0..=k1).all(|k| match (bl.get(&k), bs.get(&k)) {
        (Some(x), Some(y)) => ((x.c / pa) - (y.c / pb)).abs() < 0.30,
        _ => true,
    })
}

/// wick: the liquidation at the bars' high / low (the trade prints); else at the closes (~ the mark price, smoother).
fn sim(t: &Trade, bl: &BTreeMap<i64, B>, bs: &BTreeMap<i64, B>, mmr_l: f64, mmr_s: f64, l: f64, pol: Pol, delay_bars: i64, wick: bool) -> Option<Res> {
    let (k0, k1) = (t.entry / candles::BAR_MS, t.exit / candles::BAR_MS);
    let p_l0 = bl.range(..=k0).next_back()?.1.c;
    let p_s0 = bs.range(..=k0).next_back()?.1.c;
    let nbars = (k1 - k0).max(1) as f64;
    // the state: units, entry prices, each venue's cash (equity less the open PnL)
    let (mut ql, mut qs) = (1.0 / p_l0, 1.0 / p_s0);
    let (mut el, mut es) = (p_l0, p_s0);
    let (mut ca, mut cb) = (1.0 / l, 1.0 / l);
    let mut r = Res::default();
    let mut pnl = -t.rt / 2.0; // the entry's half of the round trip
    let mut pend: Option<(i64, f64, bool)> = None; // (arrives at bar, amount, to the long's venue)
    let mut naked_l: Option<i64> = None; // the long leg's venue liquidated: the reopen at bar
    let mut naked_s: Option<i64> = None;
    let mut cut_done = false;
    let (mut last_l, mut last_s) = (B { c: p_l0, hi: p_l0, lo: p_l0 }, B { c: p_s0, hi: p_s0, lo: p_s0 });
    let mut ended = false;
    for k in (k0 + 1)..=k1 {
        if let Some(b) = bl.get(&k) {
            last_l = *b;
        }
        if let Some(b) = bs.get(&k) {
            last_s = *b;
        }
        let (mut bl_, mut bs_) = (last_l, last_s);
        if !wick {
            bl_.lo = bl_.c;
            bs_.hi = bs_.c;
        }
        // the funding of the bar on the hedged size (a naked bar earns half: one leg)
        let frac = (ql * el).min(qs * es);
        pnl += t.funding / nbars * frac * if naked_l.is_some() || naked_s.is_some() { 0.5 } else { 1.0 };
        // a transfer arriving
        if let Some((at, amt, to_l)) = pend {
            if k >= at {
                if to_l { ca += amt; cb -= amt; } else { cb += amt; ca -= amt; }
                pend = None;
                r.xfer += 1;
            }
        }
        // a reopen of a liquidated leg (the money arrived)
        // the reopen: the surviving leg's PnL realised, half its venue's equity sent over, both legs resized to the
        // leverage l on that half (the surviving leg cut at market to match)
        if let Some(at) = naked_l {
            if k >= at {
                cb += qs * (es - bs_.c);
                es = bs_.c;
                let half = (cb / 2.0).max(0.0);
                cb -= half;
                ca = half;
                let n_new = l * half;
                pnl -= TK * n_new + TK * (qs * es - n_new).abs();
                ql = n_new / bl_.c;
                el = bl_.c;
                qs = n_new / es;
                naked_l = None;
            }
        }
        if let Some(at) = naked_s {
            if k >= at {
                ca += ql * (bl_.c - el);
                el = bl_.c;
                let half = (ca / 2.0).max(0.0);
                ca -= half;
                cb = half;
                let n_new = l * half;
                pnl -= TK * n_new + TK * (ql * el - n_new).abs();
                qs = n_new / bs_.c;
                es = bs_.c;
                ql = n_new / el;
                naked_s = None;
            }
        }
        // the liquidation checks at the worst prices
        let liq_l = naked_l.is_none() && ql > 0.0 && ca + ql * (bl_.lo - el) < mmr_l * ql * bl_.lo;
        let liq_s = naked_s.is_none() && qs > 0.0 && cb + qs * (es - bs_.hi) < mmr_s * qs * bs_.hi;
        for (is_l, hit) in [(true, liq_l), (false, liq_s)] {
            if !hit {
                continue;
            }
            r.liq += 1;
            // the venue's equity is gone (its realised loss = the cash; the open PnL is the position's)
            if is_l { ca = 0.0; ql = 0.0; } else { cb = 0.0; qs = 0.0; }
            if pol == Pol::None {
                // the other leg closed at market: its PnL realised into its venue's cash
                if is_l { cb += qs * (es - bs_.c); pnl -= TK * qs * bs_.c; qs = 0.0; } else { ca += ql * (bl_.c - el); pnl -= TK * ql * bl_.c; ql = 0.0; }
                ended = true;
            } else if is_l {
                naked_l = Some(k + delay_bars);
            } else {
                naked_s = Some(k + delay_bars);
            }
            pend = None;
        }
        if ended {
            r.hold_h = (k - k0) as f64 / 4.0;
            break;
        }
        // the guards
        if pol == Pol::Rebal && naked_l.is_none() && naked_s.is_none() {
            let (a, b) = (ca + ql * (bl_.c - el), cb + qs * (es - bs_.c));
            let start = 1.0 / l * (ql * el).max(qs * es);
            if pend.is_none() && (a < 0.5 * start || b < 0.5 * start) {
                let to_l = a < b;
                // the richer venue keeps its initial margin (the notional / the max leverage ~ 2 x the maintenance)
                let (rich, keep) = if to_l { (b, 2.0 * mmr_s * qs * bs_.c) } else { (a, 2.0 * mmr_l * ql * bl_.c) };
                let amt = ((a - b).abs() / 2.0).min(rich - keep).max(0.0);
                if amt > 0.0 {
                    pend = Some((k + delay_bars, amt, to_l));
                }
            }
            // 75% of the way to the liquidation with the money not there yet: both legs cut by a third
            let dist = |e: f64, m: f64| (e - m) / (start - m).max(1e-12);
            let dl = dist(a, mmr_l * ql * bl_.c);
            let ds = dist(b, mmr_s * qs * bs_.c);
            if pend.is_some() && !cut_done && (dl < 0.25 || ds < 0.25) {
                let (xl, xs) = (ql / 3.0, qs / 3.0);
                ca += xl * (bl_.c - el);
                cb += xs * (es - bs_.c);
                pnl -= TK * (xl * bl_.c + xs * bs_.c);
                ql -= xl;
                qs -= xs;
                // the realised PnL stays on the venue's cash (counted at the end with the equity)
                r.cut += 1;
                cut_done = true;
            }
        }
    }
    if !ended {
        r.hold_h = (k1 - k0) as f64 / 4.0;
    }
    // the exit: the price PnL of what is open (the fstab rt's exit half on the open size); a naked leg closed at market
    let pr = ql * (last_l.c - el) + qs * (es - last_s.c);
    pnl += pr - t.rt / 2.0 * (ql * last_l.c).min(qs * last_s.c) - TK * ((ql * last_l.c) - (qs * last_s.c)).abs();
    r.price = pr;
    // the venues' cash (moved by transfers, cuts, a liquidation's loss, a closed leg) less the start: realised
    pnl += ca + cb - 2.0 / l;
    r.pnl = pnl;
    Some(r)
}

pub async fn run(h: &crate::http::Http) -> Result<()> {
    let dd = crate::data_dir();
    let raw: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(dd.join("fstab_trades.json"))?)?;
    let trades: Vec<Trade> = raw.iter().map(|x| Trade {
        asset: x["asset"].as_str().unwrap_or("").into(),
        long: x["long"].as_str().unwrap_or("").into(),
        short: x["short"].as_str().unwrap_or("").into(),
        entry: x["entry_ms"].as_i64().unwrap_or(0),
        exit: x["exit_ms"].as_i64().unwrap_or(0),
        funding: x["funding"].as_f64().unwrap_or(0.0),
        rt: x["rt"].as_f64().unwrap_or(0.0),
        half: x["half"].as_i64().unwrap_or(0),
        stock: x["stock"].as_bool().unwrap_or(false),
    }).collect();
    let reg = Registry::load(h, &ALL).await;
    let shared = reg.shared();
    let from = trades.iter().map(|t| t.entry).min().unwrap_or(0) - 2 * 3_600_000;
    let to = trades.iter().map(|t| t.exit).max().unwrap_or(0) + 3_600_000;
    let bdir = dd.join("lev_bars");
    std::fs::create_dir_all(&bdir)?;
    // every (asset, venue) the trades touch, and every venue of that asset (the proxies)
    let mut need: Vec<crate::model::Instrument> = Vec::new();
    let mut maxlev: HashMap<(String, String), f64> = HashMap::new();
    for a in trades.iter().map(|t| t.asset.clone()).collect::<std::collections::BTreeSet<_>>() {
        for i in shared.get(&a).into_iter().flatten() {
            maxlev.insert((a.clone(), i.venue.clone()), i.max_leverage.map(f).unwrap_or(10.0).max(1.0));
            if !matches!(registry::venue_of(i), V::Lighter | V::Gmx) {
                need.push(i.clone());
            }
        }
    }
    let mut bars: HashMap<(String, String), BTreeMap<i64, B>> = HashMap::new();
    let path = |i: &crate::model::Instrument| bdir.join(format!("{}__{}.csv", i.base.replace([':', '/'], "_"), i.venue));
    let mut todo = Vec::new();
    for i in &need {
        match std::fs::read_to_string(path(i)) {
            Ok(s) => {
                let v: Vec<candles::Bar> = s.lines().filter_map(|l| {
                    let x: Vec<f64> = l.split(',').filter_map(|y| y.parse().ok()).collect();
                    (x.len() == 4).then(|| candles::Bar { ts: x[0] as i64, close: x[1], hi: x[2], lo: x[3], vol: 1.0 })
                }).collect();
                bars.insert((i.base.clone(), i.venue.clone()), clean(v));
            }
            Err(_) => todo.push(i.clone()),
        }
    }
    println!("bars: {} cached, {} to fetch", bars.len(), todo.len());
    for c in todo.chunks(6) {
        let r = futures::future::join_all(c.iter().map(|i| async move { (i.clone(), candles::candles(registry::venue_of(i), h, i, from, to).await) })).await;
        for (i, x) in r {
            match x {
                Ok(v) if v.len() > 20 => {
                    let body: String = v.iter().map(|b| format!("{},{},{},{}\n", b.ts, b.close, b.hi, b.lo)).collect();
                    let _ = std::fs::write(path(&i), body);
                    bars.insert((i.base.clone(), i.venue.clone()), clean(v));
                }
                Ok(v) => eprintln!("{} {}: {} bars", i.venue, i.base, v.len()),
                Err(e) => eprintln!("{} {}: {e:#}", i.venue, i.base),
            }
        }
    }
    // a leg's bars: its own venue's, else a proxy (Hyperliquid first, then any)
    let pick = |a: &str, v: &str| -> Option<(&BTreeMap<i64, B>, bool)> {
        if let Some(b) = bars.get(&(a.to_string(), v.to_string())) {
            return Some((b, false));
        }
        for p in ["Hyperliquid", "Aster", "Extended", "Pacifica", "edgeX", "ApeX", "GRVT", "StandX"] {
            if let Some(b) = bars.get(&(a.to_string(), p.to_string())) {
                return Some((b, true));
            }
        }
        None
    };
    let mut rep = String::new();
    writeln!(rep, "# Leverage on the funding trades ({} trades: fstab's chosen rule, both halves)\n", trades.len())?;
    // the paths: the worst adverse move of each leg over the hold, the divergence of the two
    let mut adv: Vec<f64> = Vec::new();
    let mut div: Vec<f64> = Vec::new();
    let mut paths = String::from("asset,long,short,proxy_l,proxy_s,entry,hold_h,p_l0,p_s0,worst_long,worst_short,worst_long_close,worst_short_close,div_max,div_end,funding_bp,stock\n");
    let (mut n_ok, mut n_proxy) = (0, 0);
    let mut n_bad = 0;
    for t in &trades {
        let (Some((bl, pl)), Some((bs, ps))) = (pick(&t.asset, &t.long), pick(&t.asset, &t.short)) else { continue };
        if !valid(t, bl, bs) {
            n_bad += 1;
            continue;
        }
        let k0 = t.entry / candles::BAR_MS;
        let k1 = t.exit / candles::BAR_MS;
        let (Some(l0), Some(s0)) = (bl.range(..=k0).next_back(), bs.range(..=k0).next_back()) else { continue };
        n_ok += 1;
        n_proxy += (pl || ps) as usize;
        let mut worst = 0.0f64;
        let mut wd = 0.0f64;
        let (mut wl, mut ws, mut wlc, mut wsc, mut dend) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for k in k0..=k1 {
            if let Some(a) = bl.get(&k) { wl = wl.max(1.0 - a.lo / l0.1.c); wlc = wlc.max(1.0 - a.c / l0.1.c); }
            if let Some(b) = bs.get(&k) { ws = ws.max(b.hi / s0.1.c - 1.0); wsc = wsc.max(b.c / s0.1.c - 1.0); }
            if let (Some(a), Some(b)) = (bl.get(&k), bs.get(&k)) {
                worst = worst.max(1.0 - a.lo / l0.1.c).max(b.hi / s0.1.c - 1.0);
                wd = wd.max(((a.c / l0.1.c) - (b.c / s0.1.c)).abs());
                dend = (a.c / l0.1.c) - (b.c / s0.1.c);
            }
        }
        adv.push(worst);
        div.push(wd);
        let _ = writeln!(paths, "{},{},{},{},{},{},{:.0},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.1},{}", t.asset, t.long, t.short, pl, ps, t.entry, (t.exit - t.entry) as f64 / 3.6e6, l0.1.c, s0.1.c, wl, ws, wlc, wsc, wd, dend, t.funding * 1e4, t.stock);
    }
    std::fs::write(dd.join("lev_paths.csv"), &paths)?;
    let q = |x: &mut Vec<f64>, p: f64| { x.sort_by(|a, b| a.total_cmp(b)); if x.is_empty() { f64::NAN } else { x[((x.len() - 1) as f64 * p) as usize] } };
    writeln!(rep, "Trades with bars on both legs and the same token: {n_ok} (a leg on a proxy venue's bars: {n_proxy}); dropped: {n_bad} (the entry prices 2%+ apart - another token under the ticker - or a leg's closes 30%+ off the other's: a bad bar).\n")?;
    writeln!(rep, "| over the hold | median | p75 | p90 | p95 | max |\n|---|---|---|---|---|---|")?;
    writeln!(rep, "| the worst adverse move of either leg (the long's low / the short's high) | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% |", q(&mut adv, 0.5) * 100.0, q(&mut adv, 0.75) * 100.0, q(&mut adv, 0.9) * 100.0, q(&mut adv, 0.95) * 100.0, q(&mut adv, 1.0) * 100.0)?;
    writeln!(rep, "| the widest divergence of the two legs' closes | {:.2}% | {:.2}% | {:.2}% | {:.2}% | {:.2}% |\n", q(&mut div, 0.5) * 100.0, q(&mut div, 0.75) * 100.0, q(&mut div, 0.9) * 100.0, q(&mut div, 0.95) * 100.0, q(&mut div, 1.0) * 100.0)?;

    {
        let v: Vec<&Trade> = trades.iter().filter(|t| matches!((pick(&t.asset, &t.long), pick(&t.asset, &t.short)), (Some((a, _)), Some((b, _))) if valid(t, a, b))).collect();
        let n = v.len().max(1) as f64;
        writeln!(rep, "The valid trades: the funding {:+.1} bp, the round trip {:.1} bp, the funding less it {:+.1} bp a trade (of one leg's notional); the dropped ones {:+.1} bp.\n",
            v.iter().map(|t| t.funding).sum::<f64>() / n * 1e4, v.iter().map(|t| t.rt).sum::<f64>() / n * 1e4, v.iter().map(|t| t.funding - t.rt).sum::<f64>() / n * 1e4,
            { let d: Vec<&Trade> = trades.iter().filter(|t| !v.iter().any(|x| std::ptr::eq(*x, *t))).collect(); d.iter().map(|t| t.funding - t.rt).sum::<f64>() / d.len().max(1) as f64 * 1e4 })?;
    }
    writeln!(rep, "Per trade: the PnL per unit of the capital (2 N / L). APR = the mean PnL on the capital x 8760 / the mean hold (the capital always in use).\n")?;
    for wick in [false, true] {
    writeln!(rep, "\n### The liquidation at the {}\n", if wick { "bars' high / low (the trade prints' wicks: a stress)" } else { "15-min closes (~ the mark price)" })?;
    writeln!(rep, "| L | policy | transfer h | trades | liquidated | cuts | mean % on capital | half 1 / half 2 | worst % | APR | price PnL bp (mean) |\n|---|---|---|---|---|---|---|---|---|---|---|")?;
    for l in [1.0f64, 1.5, 2.0, 3.0, 5.0, 10.0] {
        for (pol, name) in [(Pol::None, "none"), (Pol::Rehedge, "rehedge"), (Pol::Rebal, "rebal")] {
            for dh in [1i64, 4] {
                if pol == Pol::None && dh == 4 {
                    continue;
                }
                let mut rs: Vec<(Res, i64, f64)> = Vec::new();
                for t in &trades {
                    let (Some((bl, _)), Some((bs, _))) = (pick(&t.asset, &t.long), pick(&t.asset, &t.short)) else { continue };
                    if !valid(t, bl, bs) {
                        continue;
                    }
                    let ml = maxlev.get(&(t.asset.clone(), t.long.clone())).copied().unwrap_or(10.0);
                    let ms = maxlev.get(&(t.asset.clone(), t.short.clone())).copied().unwrap_or(10.0);
                    // the effective leverage no higher than a venue allows
                    let le = l.min(ml).min(ms);
                    if let Some(r) = sim(t, bl, bs, 0.5 / ml, 0.5 / ms, le, pol, dh * 4, wick) {
                        let cap = 2.0 / le;
                        rs.push((r, t.half, cap));
                    }
                }
                if rs.is_empty() {
                    continue;
                }
                let n = rs.len() as f64;
                let on_cap: Vec<f64> = rs.iter().map(|(r, _, c)| r.pnl / c).collect();
                let m = on_cap.iter().sum::<f64>() / n;
                let hm = |hh: i64| { let x: Vec<f64> = rs.iter().zip(&on_cap).filter(|((_, h_, _), _)| *h_ == hh).map(|(_, v)| *v).collect(); if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 } };
                let hold = rs.iter().map(|(r, _, _)| r.hold_h).sum::<f64>() / n;
                let worst = on_cap.iter().cloned().fold(f64::MAX, f64::min);
                writeln!(rep, "| {l} | {name} | {} | {} | {} | {} | {:+.2}% | {:+.2}% / {:+.2}% | {:+.1}% | {:+.0}% | {:+.1} |",
                    if pol == Pol::None { "-".to_string() } else { dh.to_string() }, rs.len(),
                    rs.iter().map(|(r, _, _)| r.liq).sum::<u32>(), rs.iter().map(|(r, _, _)| r.cut).sum::<u32>(),
                    m * 100.0, hm(1) * 100.0, hm(2) * 100.0, worst * 100.0, m * 8760.0 / hold.max(1.0) * 100.0,
                    rs.iter().map(|(r, _, _)| r.price).sum::<f64>() / n * 1e4)?;
            }
        }
    }
    }
    // the stocks apart (their own vol)
    let ns = trades.iter().filter(|t| t.stock).count();
    writeln!(rep, "\n{ns} of the trades on stock-like markets.")?;
    std::fs::write(dd.join("lev.md"), &rep)?;
    println!("{rep}");
    Ok(())
}

/// `dexarb bars [--days D] [--stocks]`: 15-min bars (close, high, low) of every shared market (--stocks: the
/// underlyings with a Hyperliquid HIP-3 market) on every venue with public bars, over the last D days, to
/// data/lev_bars/<base>__<venue>.csv (a file there kept).
pub async fn fetch_bars(h: &crate::http::Http, days: f64, stocks: bool) -> Result<()> {
    let reg = Registry::load(h, &ALL).await;
    let shared = reg.shared();
    let to = crate::model::now_ms();
    let from = to - (days * 86_400_000.0) as i64;
    let bdir = crate::data_dir().join("lev_bars");
    std::fs::create_dir_all(&bdir)?;
    let mut todo = Vec::new();
    for (_, v) in &shared {
        if stocks && !v.iter().any(|i| i.venue == "Hyperliquid" && i.category.starts_with("hip3")) {
            continue;
        }
        for i in v {
            if matches!(registry::venue_of(i), V::Lighter | V::Gmx) {
                continue;
            }
            let p = bdir.join(format!("{}__{}.csv", i.base.replace([':', '/'], "_"), i.venue));
            if !p.exists() {
                todo.push((i.clone(), p));
            }
        }
    }
    println!("{} to fetch", todo.len());
    for c in todo.chunks(4) {
        let r = futures::future::join_all(c.iter().map(|(i, p)| async move { (i, p, candles::candles(registry::venue_of(i), h, i, from, to).await) })).await;
        for (i, p, x) in r {
            match x {
                Ok(v) if v.len() > 20 => {
                    let body: String = v.iter().map(|b| format!("{},{},{},{}\n", b.ts, b.close, b.hi, b.lo)).collect();
                    let _ = std::fs::write(p, body);
                }
                Ok(v) => eprintln!("{} {}: {} bars", i.venue, i.base, v.len()),
                Err(e) => eprintln!("{} {}: {e:#}", i.venue, i.base),
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    Ok(())
}
