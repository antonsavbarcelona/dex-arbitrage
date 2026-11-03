//! The funding-stability study (data/funding: 30 days of every venue's settlements for the shared underlyings).
//! No look-ahead: at each hour H a pair's spread is what was KNOWN then - each venue's last settled rate at or before H;
//! what the hedge earns after H is the settlements that come after it (each at its own interval: a position held at a
//! settlement T is paid rate x interval). Questions:
//!   1. retention: when the known spread is X, how much of it is paid over the next 1 / 4 / 8 / 24 / 72 h, and how often
//!      the next 8 h pay < 0 (the spike gone before the payment);
//!   2. persistence: does a spread that has held m checks in a row keep paying better than a fresh one;
//!   3. the spike: a spread far over its own 72 h mean vs one near it;
//!   4. by kind (crypto / stocks & HIP-3 style) and by the venue on the extreme side;
//!   5. trading it: enter when (theta, m) pass, out when the known spread falls under theta / 3 or after 7 days, one
//!      position a pair; the funding paid minus the round trip (taker both legs, or the maker-taker estimate); theta and
//!      m chosen on the first half of the hours, scored on the second.  `dexarb fstab` -> data/fstab.md

use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

#[derive(Clone)]
struct Ser {
    venue: String,
    stock: bool,
    /// (settlement ms, rate_h, the interval h)
    pts: Vec<(i64, f64, f64)>,
    taker: f64,
    maker: f64,
    /// The market's median bid-ask spread (fraction), from the WebSocket books (data/spreads.json; else its venue's median).
    spread: f64,
}

fn maker_of(v: &str) -> f64 {
    match v {
        "Hyperliquid" => 0.00015,
        "Lighter" | "Extended" | "Aster" => 0.0,
        "GRVT" => 0.0003,
        "ApeX" => 0.0002,
        "edgeX" => 0.0004,
        "Pacifica" => 0.00015,
        "StandX" => 0.0001,
        _ => 0.0005,
    }
}

/// The last settled rate at or before t.
fn known(s: &Ser, t: i64) -> Option<f64> {
    let i = s.pts.partition_point(|p| p.0 <= t);
    (i > 0).then(|| s.pts[i - 1].1)
}

/// The funding paid to a position held over (a, b]: each settlement in it, rate_h x its interval.
fn paid(s: &Ser, a: i64, b: i64) -> f64 {
    let i = s.pts.partition_point(|p| p.0 <= a);
    s.pts[i..].iter().take_while(|p| p.0 <= b).map(|p| p.1 * p.2).sum()
}

fn mean(x: &[f64]) -> f64 {
    if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 }
}

pub fn run() -> Result<()> {
    let dir = crate::data_dir().join("funding");
    let meta: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json"))?)?;
    // the markets' spreads (bp) measured on the books; a market not measured: its venue's median
    let spreads: HashMap<String, f64> = std::fs::read_to_string(crate::data_dir().join("spreads.json")).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_default();
    let mut venue_sp: HashMap<String, f64> = HashMap::new();
    {
        let mut per: HashMap<String, Vec<f64>> = HashMap::new();
        for (k, v) in &spreads {
            if let Some((ven, _)) = k.split_once('|') {
                per.entry(ven.to_string()).or_default().push(*v);
            }
        }
        for (ven, mut x) in per {
            x.sort_by(|a, b| a.total_cmp(b));
            venue_sp.insert(ven, x[x.len() / 2]);
        }
    }
    let mut by: BTreeMap<String, Vec<Ser>> = BTreeMap::new();
    for m in &meta {
        let (v, base, file) = (m["venue"].as_str().unwrap_or(""), m["base"].as_str().unwrap_or(""), m["file"].as_str().unwrap_or(""));
        let iv = m["interval_h"].as_f64().unwrap_or(1.0).max(1.0);
        let raw = crate::history::load(&dir, file);
        if raw.len() < 20 {
            continue;
        }
        // the interval from the data where the venue's points are sparser than its nominal one (8 h venues)
        let mut gaps: Vec<i64> = raw.windows(2).map(|w| w[1].ts_ms - w[0].ts_ms).collect();
        gaps.sort();
        let iv_data = (gaps[gaps.len() / 2] as f64 / 3.6e6).round().max(1.0);
        let n = if (iv_data - iv).abs() > 0.5 { iv_data } else { iv };
        let stock = m["symbol"].as_str().is_some_and(|s| s.contains(':')) || m["category"].as_str().is_some_and(|c| c.eq_ignore_ascii_case("equity") || c.eq_ignore_ascii_case("stock"));
        let sp = spreads.get(&format!("{v}|{base}")).copied().or_else(|| venue_sp.get(v).copied()).unwrap_or(10.0) / 1e4;
        by.entry(base.to_string()).or_default().push(Ser { venue: v.into(), stock, pts: raw.iter().map(|p| (p.ts_ms, p.rate_h, n)).collect(), taker: m["taker"].as_f64().unwrap_or(0.0005), maker: maker_of(v), spread: sp });
    }
    // an underlying is "stock-like" when any venue lists it as a HIP-3 / equity market
    let kinds: HashMap<String, bool> = by.iter().map(|(a, v)| (a.clone(), v.iter().any(|s| s.stock))).collect();
    let (t0, t1) = by.values().flatten().fold((i64::MAX, 0i64), |a, s| (a.0.min(s.pts[0].0), a.1.max(s.pts.last().unwrap().0)));
    let mid = t0 + (t1 - t0) / 2;
    let hours: Vec<i64> = ((t0 / 3_600_000 + 48)..=(t1 / 3_600_000 - 1)).map(|h| h * 3_600_000).collect();

    // the observations: (asset, the long venue, the short venue, H, the known spread s, the known spread m checks back ..)
    struct Obs {
        asset: usize,
        long: usize,
        short: usize,
        h: i64,
        s: f64,
        hist: [f64; 24],
        trail72: f64,
        fwd: [f64; 5],
    }
    let assets: Vec<(String, Vec<Ser>)> = by.into_iter().filter(|(_, v)| v.len() >= 2).collect();
    let ks = [1i64, 4, 8, 24, 72];
    let mut obs: Vec<Obs> = Vec::new();
    for (ai, (_, ser)) in assets.iter().enumerate() {
        for (li, la) in ser.iter().enumerate() {
            for (si, sb) in ser.iter().enumerate() {
                if li == si {
                    continue;
                }
                for &h in hours.iter().step_by(1) {
                    let (Some(ra), Some(rb)) = (known(la, h), known(sb, h)) else { continue };
                    let s = rb - ra;
                    // only the observations where the pair receives a meaningful spread (> 10% APR)
                    if s * 8760.0 < 0.10 {
                        continue;
                    }
                    let mut hist = [f64::NAN; 24];
                    for (j, x) in hist.iter_mut().enumerate() {
                        let t = h - j as i64 * 3_600_000;
                        if let (Some(a), Some(b)) = (known(la, t), known(sb, t)) {
                            *x = b - a;
                        }
                    }
                    let tr: Vec<f64> = (0..72).filter_map(|j| { let t = h - j * 3_600_000; Some(known(sb, t)? - known(la, t)?) }).collect();
                    let mut fwd = [f64::NAN; 5];
                    for (j, k) in ks.iter().enumerate() {
                        let b = h + k * 3_600_000;
                        if b <= t1 {
                            // per hour of the hold
                            fwd[j] = (paid(sb, h, b) - paid(la, h, b)) / *k as f64;
                        }
                    }
                    obs.push(Obs { asset: ai, long: li, short: si, h, s, hist, trail72: mean(&tr), fwd });
                }
            }
        }
    }
    let apr = |x: f64| x * 8760.0 * 100.0;
    let mut rep = String::new();
    writeln!(rep, "# Funding stability ({} underlyings on 2+ venues, {:.0} days, {} pair-hours with a known spread > 10% APR)\n", assets.len(), (t1 - t0) as f64 / 8.64e7, obs.len())?;
    writeln!(rep, "The known spread at hour H = the short venue's last settled rate - the long venue's (what one sees then); the paid = the settlements after H (each at its interval), per hour of the hold. Retention = paid / known.\n")?;

    // 1. retention by the level
    writeln!(rep, "## 1. How much of a known spread gets paid, by its level\n")?;
    writeln!(rep, "| known spread (APR) | pair-hours | paid next 1 h | 4 h | 8 h | 24 h | 72 h | next 8 h pay < 0 | retention 24 h |")?;
    writeln!(rep, "|---|---|---|---|---|---|---|---|---|")?;
    let bins = [(0.10, 0.25), (0.25, 0.50), (0.50, 1.0), (1.0, 2.0), (2.0, 5.0), (5.0, 1e9)];
    for &(lo, hi) in &bins {
        let sel: Vec<&Obs> = obs.iter().filter(|o| (lo..hi).contains(&(o.s * 8760.0))).collect();
        if sel.is_empty() {
            continue;
        }
        let f = |j: usize| mean(&sel.iter().map(|o| o.fwd[j]).filter(|x| x.is_finite()).collect::<Vec<_>>());
        let neg = sel.iter().filter(|o| o.fwd[2].is_finite() && o.fwd[2] < 0.0).count() as f64 / sel.iter().filter(|o| o.fwd[2].is_finite()).count().max(1) as f64;
        let ret = f(3) / mean(&sel.iter().map(|o| o.s).collect::<Vec<_>>());
        writeln!(rep, "| {:.0}-{} | {} | {:+.0}% | {:+.0}% | {:+.0}% | {:+.0}% | {:+.0}% | {:.0}% | {:.0}% |", lo * 100.0, if hi > 1e8 { "inf".to_string() } else { format!("{:.0}%", hi * 100.0) }, sel.len(), apr(f(0)), apr(f(1)), apr(f(2)), apr(f(3)), apr(f(4)), neg * 100.0, ret * 100.0)?;
    }

    // 2. persistence: the spread over 50% APR now, held m checks (hours) in a row
    writeln!(rep, "\n## 2. Persistence: a spread >= 50% APR now that has held m hours in a row\n")?;
    writeln!(rep, "| held (h) | pair-hours | known now (APR) | paid next 8 h | 24 h | 72 h | next 24 h pay < 0 | retention 24 h |")?;
    writeln!(rep, "|---|---|---|---|---|---|---|---|")?;
    for m in [1usize, 2, 4, 8, 12, 24] {
        let sel: Vec<&Obs> = obs.iter().filter(|o| o.s * 8760.0 >= 0.5 && o.hist[..m].iter().all(|x| x.is_finite() && x * 8760.0 >= 0.5)).collect();
        if sel.is_empty() {
            continue;
        }
        let f = |j: usize| mean(&sel.iter().map(|o| o.fwd[j]).filter(|x| x.is_finite()).collect::<Vec<_>>());
        let neg = sel.iter().filter(|o| o.fwd[3].is_finite() && o.fwd[3] < 0.0).count() as f64 / sel.iter().filter(|o| o.fwd[3].is_finite()).count().max(1) as f64;
        let s = mean(&sel.iter().map(|o| o.s).collect::<Vec<_>>());
        writeln!(rep, "| {m} | {} | {:+.0}% | {:+.0}% | {:+.0}% | {:+.0}% | {:.0}% | {:.0}% |", sel.len(), apr(s), apr(f(2)), apr(f(3)), apr(f(4)), neg * 100.0, f(3) / s * 100.0)?;
    }

    // 3. the spike: now vs its own 72 h mean
    writeln!(rep, "\n## 3. The spike: the spread now over its own 72 h mean (spreads >= 50% APR)\n")?;
    writeln!(rep, "| now / 72 h mean | pair-hours | known now (APR) | 72 h mean (APR) | paid next 24 h | retention 24 h | next 24 h pay < 0 |")?;
    writeln!(rep, "|---|---|---|---|---|---|---|")?;
    for &(lo, hi) in &[(f64::MIN, 1.5), (1.5, 3.0), (3.0, 10.0), (10.0, f64::MAX)] {
        let sel: Vec<&Obs> = obs.iter().filter(|o| o.s * 8760.0 >= 0.5 && o.trail72 > 0.0 && (lo..hi).contains(&(o.s / o.trail72))).collect();
        let sel2: Vec<&Obs> = if lo == f64::MIN { obs.iter().filter(|o| o.s * 8760.0 >= 0.5 && (o.trail72 <= 0.0 || o.s / o.trail72 < 1.5)).collect() } else { sel };
        if sel2.is_empty() {
            continue;
        }
        let f = mean(&sel2.iter().map(|o| o.fwd[3]).filter(|x| x.is_finite()).collect::<Vec<_>>());
        let s = mean(&sel2.iter().map(|o| o.s).collect::<Vec<_>>());
        let tm = mean(&sel2.iter().map(|o| o.trail72).collect::<Vec<_>>());
        let neg = sel2.iter().filter(|o| o.fwd[3].is_finite() && o.fwd[3] < 0.0).count() as f64 / sel2.iter().filter(|o| o.fwd[3].is_finite()).count().max(1) as f64;
        writeln!(rep, "| {} | {} | {:+.0}% | {:+.0}% | {:+.0}% | {:.0}% | {:.0}% |", if lo == f64::MIN { "< 1.5 (or the mean <= 0)".to_string() } else if hi == f64::MAX { format!(">= {lo}") } else { format!("{lo}-{hi}") }, sel2.len(), apr(s), apr(tm), apr(f), f / s * 100.0, neg * 100.0)?;
    }

    // 4. by kind and by the venues
    writeln!(rep, "\n## 4. By kind and by venue (spreads >= 50% APR held 4 h)\n")?;
    let base4: Vec<&Obs> = obs.iter().filter(|o| o.s * 8760.0 >= 0.5 && o.hist[..4].iter().all(|x| x.is_finite() && x * 8760.0 >= 0.5)).collect();
    writeln!(rep, "| group | pair-hours | known (APR) | paid next 24 h | retention | next 24 h < 0 |")?;
    writeln!(rep, "|---|---|---|---|---|---|")?;
    let mut groups: Vec<(String, Vec<&Obs>)> = vec![
        ("crypto".into(), base4.iter().copied().filter(|o| !kinds[&assets[o.asset].0]).collect()),
        ("stocks / HIP-3 style".into(), base4.iter().copied().filter(|o| kinds[&assets[o.asset].0]).collect()),
    ];
    let venues: Vec<String> = ["Hyperliquid", "Lighter", "Extended", "Aster", "GRVT", "ApeX", "edgeX", "Pacifica", "StandX"].iter().map(|x| x.to_string()).collect();
    for v in &venues {
        groups.push((format!("the SHORT leg on {v}"), base4.iter().copied().filter(|o| &assets[o.asset].1[o.short].venue == v).collect()));
        groups.push((format!("the LONG leg on {v}"), base4.iter().copied().filter(|o| &assets[o.asset].1[o.long].venue == v).collect()));
    }
    for (g, sel) in &groups {
        if sel.len() < 30 {
            continue;
        }
        let f = mean(&sel.iter().map(|o| o.fwd[3]).filter(|x| x.is_finite()).collect::<Vec<_>>());
        let s = mean(&sel.iter().map(|o| o.s).collect::<Vec<_>>());
        let neg = sel.iter().filter(|o| o.fwd[3].is_finite() && o.fwd[3] < 0.0).count() as f64 / sel.iter().filter(|o| o.fwd[3].is_finite()).count().max(1) as f64;
        writeln!(rep, "| {g} | {} | {:+.0}% | {:+.0}% | {:.0}% | {:.0}% |", sel.len(), apr(s), apr(f), f / s * 100.0, neg * 100.0)?;
    }

    // 5. trading it, walked forward
    writeln!(rep, "\n## 5. Trading it (one position a pair; out when the known spread < theta / 3 or after 7 days)\n")?;
    writeln!(rep, "The cost of a round trip, % of one leg, with each market's measured median spread: taker = 2 x (both takers) + both full spreads; maker-taker = 2 x (the limit leg's maker + the hedge's taker + half the hedge's spread) + 2 bp, the limit on whichever venue makes it cheaper. bp per trade of one leg's notional; at 3x the capital is ~ one leg's notional.
")?;
    writeln!(rep, "| theta (APR) | held m h | half 1: trades / mean bp (taker) / (maker-taker) | half 2: trades / mean bp (taker) / (maker-taker) / win (m-t) / mean hold h |")?;
    writeln!(rep, "|---|---|---|---|")?;
    let trade_x = |theta: f64, m: usize, half: bool, maker: bool, exit_frac: f64, min_hold_h: i64| -> (usize, f64, usize, f64) {
        let (mut n, mut tot, mut wins, mut hold) = (0usize, 0.0, 0usize, 0.0);
        for (ai, (_, ser)) in assets.iter().enumerate() {
            for (li, la) in ser.iter().enumerate() {
                for (si, sb) in ser.iter().enumerate() {
                    if li == si {
                        continue;
                    }
                    // the round trip: maker-taker = the limit leg's maker fee + the hedge's taker fee and half its spread, both
                    // ways, + 1 bp of the hedge's adverse move each way (the cheaper choice of the limit's venue); taker = both
                    // takers and both full spreads (half each way)
                    let rt = if maker {
                        let a = (la.maker + sb.taker + sb.spread / 2.0).min(sb.maker + la.taker + la.spread / 2.0);
                        2.0 * a + 2e-4
                    } else {
                        2.0 * (la.taker + sb.taker) + la.spread + sb.spread
                    };
                    let mut i = 0;
                    while i < hours.len() {
                        let h = hours[i];
                        if (h < mid) != half {
                            i += 1;
                            continue;
                        }
                        let ok = (0..m).all(|j| {
                            let t = h - j as i64 * 3_600_000;
                            matches!((known(la, t), known(sb, t)), (Some(a), Some(b)) if (b - a) * 8760.0 >= theta)
                        });
                        if !ok {
                            i += 1;
                            continue;
                        }
                        // hold until the known spread < theta / 3, or 7 days, or the data's end
                        let mut j = i + 1;
                        while j < hours.len() && hours[j] - h < 7 * 86_400_000 {
                            if hours[j] - h < min_hold_h * 3_600_000 {
                                j += 1;
                                continue;
                            }
                            match (known(la, hours[j]), known(sb, hours[j])) {
                                (Some(a), Some(b)) if (b - a) * 8760.0 >= theta * exit_frac && b - a > 0.0 => j += 1,
                                _ => break,
                            }
                        }
                        let j = j.min(hours.len() - 1);
                        let r = paid(sb, h, hours[j]) - paid(la, h, hours[j]) - rt;
                        n += 1;
                        tot += r;
                        wins += (r > 0.0) as usize;
                        hold += (hours[j] - h) as f64 / 3.6e6;
                        let _ = ai;
                        i = j + 1;
                    }
                }
            }
        }
        (n, tot, wins, hold)
    };
    let trade = |theta: f64, m: usize, half: bool, maker: bool| trade_x(theta, m, half, maker, 1.0 / 3.0, 0);
    let mut best = (f64::MIN, 0.0, 0usize);
    for theta in [0.25, 0.5, 1.0, 2.0] {
        for m in [1usize, 4, 8, 24] {
            let (n1, s1, _, _) = trade(theta, m, true, false);
            let (n1m, s1m, _, _) = trade(theta, m, true, true);
            let (n2, s2, _, _) = trade(theta, m, false, false);
            let (n2m, s2m, w2m, h2m) = trade(theta, m, false, true);
            let mm = |s: f64, n: usize| if n > 0 { s / n as f64 * 1e4 } else { 0.0 };
            if n1m >= 30 && mm(s1m, n1m) > best.0 {
                best = (mm(s1m, n1m), theta, m);
            }
            writeln!(rep, "| {:.0}% | {m} | {n1} / {:+.1} / {:+.1} | {n2} / {:+.1} / {:+.1} / {:.0}% / {:.0} |", theta * 100.0, mm(s1, n1), mm(s1m, n1m), mm(s2, n2), mm(s2m, n2m), w2m as f64 / n2m.max(1) as f64 * 100.0, h2m / n2m.max(1) as f64)?;
        }
    }
    let (n2, s2, w2, h2) = trade(best.1, best.2, false, true);
    writeln!(rep, "\n**Chosen on half 1 (maker-taker):** theta {:.0}% APR, held {} h (half 1 {:+.1} bp a trade). **Half 2:** {} trades, {:+.1} bp, win {:.0}%, mean hold {:.0} h.\n",
        best.1 * 100.0, best.2, best.0, n2, if n2 > 0 { s2 / n2 as f64 * 1e4 } else { 0.0 }, w2 as f64 / n2.max(1) as f64 * 100.0, h2 / n2.max(1) as f64)?;
    // 5b. the exits: out only when the spread turns negative, with a minimum hold
    writeln!(rep, "
## 5b. The exit: out when the known spread < theta x f (f = 1/3, or 0: only when it turns negative), after a minimum hold (maker-taker costs)
")?;
    writeln!(rep, "| theta | held m h | exit f | min hold h | half 1: trades / mean bp | half 2: trades / mean bp / win / mean hold h |")?;
    writeln!(rep, "|---|---|---|---|---|---|")?;
    let mut best_b = (f64::MIN, 0.0, 0usize, 0.0, 0i64);
    for theta in [0.5, 1.0, 2.0] {
        for m in [4usize, 8] {
            for (fx, mh) in [(1.0 / 3.0, 0i64), (0.0, 0), (0.0, 24), (0.0, 72)] {
                let (n1, s1, _, _) = trade_x(theta, m, true, true, fx, mh);
                let (n2, s2, w2, h2) = trade_x(theta, m, false, true, fx, mh);
                let mm = |s: f64, n: usize| if n > 0 { s / n as f64 * 1e4 } else { 0.0 };
                if n1 >= 30 && mm(s1, n1) > best_b.0 {
                    best_b = (mm(s1, n1), theta, m, fx, mh);
                }
                writeln!(rep, "| {:.0}% | {m} | {} | {mh} | {n1} / {:+.1} | {n2} / {:+.1} / {:.0}% / {:.0} |", theta * 100.0, if fx > 0.0 { "1/3" } else { "0" }, mm(s1, n1), mm(s2, n2), w2 as f64 / n2.max(1) as f64 * 100.0, h2 / n2.max(1) as f64)?;
            }
        }
    }
    let (n2, s2, w2, h2) = trade_x(best_b.1, best_b.2, false, true, best_b.3, best_b.4);
    writeln!(rep, "
**Chosen on half 1:** theta {:.0}%, held {} h, exit f {}, min hold {} h (half 1 {:+.1} bp). **Half 2:** {} trades, {:+.1} bp, win {:.0}%, mean hold {:.0} h.
",
        best_b.1 * 100.0, best_b.2, if best_b.3 > 0.0 { "1/3" } else { "0" }, best_b.4, best_b.0, n2, if n2 > 0 { s2 / n2 as f64 * 1e4 } else { 0.0 }, w2 as f64 / n2.max(1) as f64 * 100.0, h2 / n2.max(1) as f64)?;
    std::fs::write(crate::data_dir().join("fstab.md"), &rep)?;
    println!("{rep}");
    Ok(())
}
