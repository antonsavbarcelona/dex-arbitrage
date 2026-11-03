//! Hyperliquid: the main dex and the HIP-3 dexes (the builder-deployed perps: stocks, indices, commodities; "dex:NAME",
//! the underlying NAME). Funding hourly, a fraction, + = longs pay.
use super::*;
use crate::http::{num, s};
use serde_json::json;

const URL: &str = "https://api.hyperliquid.xyz/info";

/// The HIP-3 dexes' names ("" = the main dex first).
async fn dexes(h: &Http) -> Vec<String> {
    let mut v = vec![String::new()];
    if let Ok(r) = h.post(URL, &json!({"type": "perpDexs"})).await {
        v.extend(r.as_array().into_iter().flatten().filter_map(|d| d["name"].as_str().map(String::from)));
    }
    v
}

pub async fn instruments(h: &Http) -> Result<Vec<Instrument>> {
    let mut out = Vec::new();
    for d in dexes(h).await {
        let q = if d.is_empty() { json!({"type": "meta"}) } else { json!({"type": "meta", "dex": d}) };
        let Ok(r) = h.post(URL, &q).await else { continue };
        for u in r["universe"].as_array().into_iter().flatten().filter(|u| u["isDelisted"] != json!(true)) {
            let name = s(&u["name"]);
            let base_raw = name.split_once(':').map_or(name.as_str(), |x| x.1).to_string();
            let mut i = inst(V::Hyperliquid, &name, &base_raw);
            i.max_leverage = Some(D::from(u["maxLeverage"].as_i64().unwrap_or(1)));
            if d.is_empty() {
                // base tier (docs.hyperliquid.xyz, fees): taker 0.045%, maker 0.015%
                i.fees = fees("0.00045", "0.00015", Source::Docs("hyperliquid docs: base tier 0.045% / 0.015%".into()));
            } else {
                // HIP-3: the deployer's share on top (docs: ~2x the base tier; verify per dex)
                i.fees = fees("0.0009", "0.0003", Source::Docs("HIP-3: ~2x the base tier (verify per dex)".into()));
            }
            i.funding_cap_h = Some(0.04);
            let iso = u["onlyIsolated"] == json!(true) || u["marginMode"].as_str().is_some_and(|m| m == "noCross" || m == "strictIsolated");
            i.category = format!("{}{}", if d.is_empty() { String::new() } else { format!("hip3:{d}") }, if iso { " isolated-only" } else { "" });
            out.push(i);
        }
    }
    Ok(out)
}

pub async fn book(h: &Http, i: &Instrument) -> Result<Book> {
    let r = h.post(URL, &json!({"type": "l2Book", "coin": i.symbol})).await?;
    let lv = &r["levels"];
    Ok(sort_book(Book { bids: side(&lv[0]), asks: side(&lv[1]), ts_exchange: r["time"].as_i64(), ts_recv: now_ms() }))
}

pub async fn funding_now(h: &Http) -> Result<Vec<FundingNow>> {
    let next = (now_ms() / 3_600_000 + 1) * 3_600_000;
    let mut out = Vec::new();
    for d in dexes(h).await {
        let q = if d.is_empty() { json!({"type": "metaAndAssetCtxs"}) } else { json!({"type": "metaAndAssetCtxs", "dex": d}) };
        let Ok(r) = h.post(URL, &q).await else { continue };
        let (u, c) = (&r[0]["universe"], &r[1]);
        out.extend(u.as_array().into_iter().flatten().zip(c.as_array().into_iter().flatten()).map(|(u, c)| FundingNow {
            symbol: s(&u["name"]), rate_h: num(&c["funding"]), predicted_h: None, next_ms: Some(next), interval_h: 1.0, long_cost_h: None, short_cost_h: None,
        }));
    }
    Ok(out)
}

pub async fn history(h: &Http, i: &Instrument, from: i64, to: i64) -> Result<Vec<FundingPoint>> {
    let mut out = Vec::new();
    let mut t = from;
    while t < to {
        let r = h.post(URL, &json!({"type": "fundingHistory", "coin": i.symbol, "startTime": t, "endTime": to})).await?;
        let v: Vec<FundingPoint> = r.as_array().into_iter().flatten().map(|x| FundingPoint { ts_ms: x["time"].as_i64().unwrap_or(0), rate_h: num(&x["fundingRate"]) }).collect();
        let Some(last) = v.last().map(|p| p.ts_ms) else { break };
        out.extend(v);
        if last <= t {
            break;
        }
        t = last + 1;
    }
    Ok(out)
}
