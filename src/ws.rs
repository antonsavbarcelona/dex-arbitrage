//! The WebSocket collector: the best bid / ask and every public trade of the chosen underlyings on all ten venues, with
//! the exchange's timestamp and ours (ms), for the maker-taker study (a resting limit on one venue, the hedge at market
//! on another). Nine venues stream (books kept locally where the feed sends deltas); GMX has no stream - its oracle
//! min / max is polled every second. Each venue reconnects on its own with a backoff; a venue silent for 60 s is
//! reconnected.  Output: data/ws/<day>.csv
//!   B,ts_recv,ts_ex,venue,asset,bid,bid_sz,ask,ask_sz      (only when the best bid / ask or their sizes change)
//!   T,ts_recv,ts_ex,venue,asset,px,sz,side                  (side: 1 the taker bought, -1 sold, 0 not published)
//! Prices per unit of the underlying (the venue's multiplier divided out).  `dexarb ws [--assets A,B,..] [--hours H]`

use crate::http::{num, s};
use crate::model::*;
use crate::registry::{self, Registry};
use crate::venues::{ALL, V};
use anyhow::{Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;

pub const DEFAULT_ASSETS: &str = "BTC,ETH,SOL,XRP,DOGE,HYPE,ZEC,NEAR,PUMP,WLD,ENA,SUI,LINK,AAVE,AVAX";

#[derive(Clone, Debug)]
pub struct Sub {
    pub asset: String,
    pub sym: String,
    pub id: String,
    pub mult: f64,
}

#[derive(Debug)]
pub enum Ev {
    Bbo { venue: &'static str, asset: String, bid: f64, bsz: f64, ask: f64, asz: f64, ts_ex: Option<i64> },
    Trade { venue: &'static str, asset: String, px: f64, sz: f64, side: i8, ts_ex: Option<i64> },
}

/// A book kept from snapshots and deltas (size 0 removes a level).
#[derive(Default)]
struct Lb {
    bids: BTreeMap<D, f64>,
    asks: BTreeMap<D, f64>,
}

impl Lb {
    fn set(&mut self, bid: bool, p: &Value, q: &Value) {
        let (Some(p), q) = (d(&s(p)), num(q)) else { return };
        let side = if bid { &mut self.bids } else { &mut self.asks };
        if q <= 0.0 {
            side.remove(&p);
        } else {
            side.insert(p, q);
        }
    }
    fn levels(&mut self, bid: bool, v: &Value) {
        for l in v.as_array().into_iter().flatten() {
            if let Some(a) = l.as_array() {
                if a.len() >= 2 {
                    self.set(bid, &a[0], &a[1]);
                }
            } else {
                let p = ["price", "px", "p"].iter().find_map(|k| l.get(*k)).cloned().unwrap_or(Value::Null);
                let q = ["size", "sz", "q", "a", "qty"].iter().find_map(|k| l.get(*k)).cloned().unwrap_or(Value::Null);
                self.set(bid, &p, &q);
            }
        }
    }
    fn best(&self) -> Option<(f64, f64, f64, f64)> {
        let (bp, bq) = self.bids.iter().next_back()?;
        let (ap, aq) = self.asks.iter().next()?;
        Some((f(*bp), *bq, f(*ap), *aq))
    }
}

fn bbo(tx: &UnboundedSender<Ev>, venue: &'static str, sub: &Sub, b: Option<(f64, f64, f64, f64)>, ts_ex: Option<i64>) {
    if let Some((bid, bsz, ask, asz)) = b {
        if bid > 0.0 && ask > 0.0 {
            let _ = tx.send(Ev::Bbo { venue, asset: sub.asset.clone(), bid: bid / sub.mult, bsz: bsz * sub.mult, ask: ask / sub.mult, asz: asz * sub.mult, ts_ex });
        }
    }
}

fn trade(tx: &UnboundedSender<Ev>, venue: &'static str, sub: &Sub, px: f64, sz: f64, side: i8, ts_ex: Option<i64>) {
    if px > 0.0 && sz > 0.0 {
        let _ = tx.send(Ev::Trade { venue, asset: sub.asset.clone(), px: px / sub.mult, sz: sz * sub.mult, side, ts_ex });
    }
}

type Handler = Box<dyn FnMut(&Value, &mut HashMap<String, Lb>, &UnboundedSender<Ev>, &mut Vec<String>) + Send>;

/// One connection: the subscriptions sent, every text frame to `h` (which may queue replies), an app-level ping every
/// 20 s when the venue wants one, out when silent 60 s or closed. Reconnects forever with a 1 .. 30 s backoff.
async fn run_ws(venue: &'static str, url: String, subs: Vec<Value>, ping: Option<Value>, mut h: Handler, tx: UnboundedSender<Ev>) {
    let mut backoff = 1u64;
    loop {
        let started = std::time::Instant::now();
        let r: Result<()> = async {
            // a User-Agent: some venues' gateways refuse a handshake without one (Extended: 403)
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            let mut rq = url.as_str().into_client_request()?;
            rq.headers_mut().insert("User-Agent", "dexarb-research/0.1 (read-only)".parse()?);
            let (mut ws, _) = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(rq)).await.map_err(|_| anyhow!("connect timeout"))??;
            for m in &subs {
                ws.send(Message::Text(m.to_string())).await?;
            }
            let mut books: HashMap<String, Lb> = HashMap::new();
            let mut tick = tokio::time::interval(Duration::from_secs(20));
            let mut last = std::time::Instant::now();
            loop {
                tokio::select! {
                    m = ws.next() => {
                        let Some(m) = m else { return Err(anyhow!("closed")) };
                        match m? {
                            Message::Text(t) => {
                                last = std::time::Instant::now();
                                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                                    let mut replies = Vec::new();
                                    h(&v, &mut books, &tx, &mut replies);
                                    for r in replies {
                                        ws.send(Message::Text(r)).await?;
                                    }
                                }
                            }
                            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => last = std::time::Instant::now(),
                            Message::Ping(p) => { last = std::time::Instant::now(); ws.send(Message::Pong(p)).await?; }
                            Message::Close(_) => return Err(anyhow!("close frame")),
                        }
                    }
                    _ = tick.tick() => {
                        if last.elapsed() > Duration::from_secs(60) {
                            return Err(anyhow!("silent 60 s"));
                        }
                        if let Some(p) = &ping {
                            let mut p = p.clone();
                            if let Some(a) = p.get_mut("args") { *a = json!([now_ms().to_string()]); }
                            ws.send(Message::Text(p.to_string())).await?;
                        }
                    }
                }
            }
        }.await;
        if let Err(e) = r {
            eprintln!("ws {venue}: {e:#} - reconnecting in {backoff} s");
        }
        if started.elapsed() > Duration::from_secs(120) {
            backoff = 1;
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

fn by<'a>(subs: &'a [Sub], key: impl Fn(&Sub) -> String) -> HashMap<String, Sub> {
    subs.iter().map(|x| (key(x), x.clone())).collect()
}

/// The venues' streams.
pub fn spawn(v: V, subs: Vec<Sub>, tx: UnboundedSender<Ev>) -> Vec<tokio::task::JoinHandle<()>> {
    let mut hs = Vec::new();
    if subs.is_empty() {
        return hs;
    }
    match v {
        V::Hyperliquid => {
            let m = by(&subs, |x| x.sym.clone());
            let ss = subs.iter().flat_map(|x| [json!({"method": "subscribe", "subscription": {"type": "bbo", "coin": x.sym}}), json!({"method": "subscribe", "subscription": {"type": "trades", "coin": x.sym}})]).collect();
            let h: Handler = Box::new(move |v, _b, tx, _r| match v["channel"].as_str() {
                Some("bbo") => {
                    let d = &v["data"];
                    if let Some(x) = m.get(&s(&d["coin"])) {
                        let (b, a) = (&d["bbo"][0], &d["bbo"][1]);
                        bbo(tx, "Hyperliquid", x, Some((num(&b["px"]), num(&b["sz"]), num(&a["px"]), num(&a["sz"]))), d["time"].as_i64());
                    }
                }
                Some("trades") => {
                    for t in v["data"].as_array().into_iter().flatten() {
                        if let Some(x) = m.get(&s(&t["coin"])) {
                            trade(tx, "Hyperliquid", x, num(&t["px"]), num(&t["sz"]), if t["side"] == "B" { 1 } else { -1 }, t["time"].as_i64());
                        }
                    }
                }
                _ => {}
            });
            hs.push(tokio::spawn(run_ws("Hyperliquid", "wss://api.hyperliquid.xyz/ws".into(), ss, Some(json!({"method": "ping"})), h, tx)));
        }
        V::Lighter => {
            let m = by(&subs, |x| x.id.clone());
            let ss = subs.iter().flat_map(|x| [json!({"type": "subscribe", "channel": format!("order_book/{}", x.id)}), json!({"type": "subscribe", "channel": format!("trade/{}", x.id)})]).collect();
            let h: Handler = Box::new(move |v, books, tx, _r| {
                let ch = s(&v["channel"]);
                if let Some(id) = ch.strip_prefix("order_book:") {
                    let Some(x) = m.get(id) else { return };
                    let b = books.entry(id.to_string()).or_default();
                    if v["type"].as_str().is_some_and(|t| t.starts_with("subscribed")) {
                        *b = Lb::default();
                    }
                    b.levels(true, &v["order_book"]["bids"]);
                    b.levels(false, &v["order_book"]["asks"]);
                    bbo(tx, "Lighter", x, b.best(), v["timestamp"].as_i64());
                } else if let Some(id) = ch.strip_prefix("trade:") {
                    let Some(x) = m.get(id) else { return };
                    if v["type"].as_str().is_some_and(|t| t.starts_with("subscribed")) {
                        return; // the snapshot of past trades
                    }
                    for t in v["trades"].as_array().into_iter().flatten() {
                        let side = match t["is_maker_ask"].as_bool() { Some(true) => 1, Some(false) => -1, None => 0 };
                        trade(tx, "Lighter", x, num(&t["price"]), num(&t["size"]), side, t["timestamp"].as_i64());
                    }
                }
            });
            hs.push(tokio::spawn(run_ws("Lighter", "wss://mainnet.zklighter.elliot.ai/stream".into(), ss, None, h, tx)));
        }
        V::Extended => {
            for x in subs {
                let (xb, xt) = (x.clone(), x.clone());
                let hb: Handler = Box::new(move |v, _b, tx, _r| {
                    let d = &v["data"];
                    let (b, a) = (&d["b"][0], &d["a"][0]);
                    bbo(tx, "Extended", &xb, Some((num(&b["p"]), num(&b["q"]), num(&a["p"]), num(&a["q"]))), v["ts"].as_i64());
                });
                let ht: Handler = Box::new(move |v, _b, tx, _r| {
                    for t in v["data"].as_array().into_iter().flatten() {
                        trade(tx, "Extended", &xt, num(&t["p"]), num(&t["q"]), if t["S"] == "BUY" { 1 } else { -1 }, t["T"].as_i64());
                    }
                });
                let base = "wss://api.starknet.extended.exchange/stream.extended.exchange/v1";
                hs.push(tokio::spawn(run_ws("Extended", format!("{base}/orderbooks/{}?depth=1", x.sym), vec![], None, hb, tx.clone())));
                hs.push(tokio::spawn(run_ws("Extended", format!("{base}/publicTrades/{}", x.sym), vec![], None, ht, tx.clone())));
            }
        }
        V::Aster => {
            let m = by(&subs, |x| x.sym.to_lowercase());
            let streams: Vec<String> = subs.iter().flat_map(|x| { let l = x.sym.to_lowercase(); [format!("{l}@bookTicker"), format!("{l}@aggTrade")] }).collect();
            let h: Handler = Box::new(move |v, _b, tx, _r| {
                let st = s(&v["stream"]);
                let Some((sym, kind)) = st.split_once('@') else { return };
                let Some(x) = m.get(sym) else { return };
                let d = &v["data"];
                if kind == "bookTicker" {
                    bbo(tx, "Aster", x, Some((num(&d["b"]), num(&d["B"]), num(&d["a"]), num(&d["A"]))), d["T"].as_i64());
                } else {
                    trade(tx, "Aster", x, num(&d["p"]), num(&d["q"]), if d["m"] == json!(true) { -1 } else { 1 }, d["T"].as_i64());
                }
            });
            hs.push(tokio::spawn(run_ws("Aster", format!("wss://fstream.asterdex.com/stream?streams={}", streams.join("/")), vec![], None, h, tx)));
        }
        V::Grvt => {
            let m = by(&subs, |x| x.sym.clone());
            let sel: Vec<String> = subs.iter().map(|x| format!("{}@500", x.sym)).collect();
            let ss = vec![json!({"jsonrpc": "2.0", "method": "subscribe", "params": {"stream": "v1.mini.s", "selectors": sel}, "id": 1}),
                json!({"jsonrpc": "2.0", "method": "subscribe", "params": {"stream": "v1.trade", "selectors": sel}, "id": 2})];
            let h: Handler = Box::new(move |v, _b, tx, _r| {
                let f_ = &v["feed"];
                let Some(x) = m.get(&s(&f_["instrument"])) else { return };
                let ts = s(&f_["event_time"]).parse::<i64>().ok().map(|n| n / 1_000_000);
                match v["stream"].as_str() {
                    Some("v1.mini.s") => bbo(tx, "GRVT", x, Some((num(&f_["best_bid_price"]), num(&f_["best_bid_size"]), num(&f_["best_ask_price"]), num(&f_["best_ask_size"]))), ts),
                    Some("v1.trade") if v["sequence_number"] != "0" => trade(tx, "GRVT", x, num(&f_["price"]), num(&f_["size"]), if f_["is_taker_buyer"] == json!(true) { 1 } else { -1 }, ts),
                    _ => {}
                }
            });
            hs.push(tokio::spawn(run_ws("GRVT", "wss://market-data.grvt.io/ws/full".into(), ss, None, h, tx)));
        }
        V::Apex => {
            let m = by(&subs, |x| x.id.clone());
            let args: Vec<String> = subs.iter().flat_map(|x| [format!("orderBook25.H.{}", x.id), format!("recentlyTrade.H.{}", x.id)]).collect();
            let h: Handler = Box::new(move |v, books, tx, _r| {
                let topic = s(&v["topic"]);
                if let Some(sym) = topic.strip_prefix("orderBook25.H.") {
                    let Some(x) = m.get(sym) else { return };
                    let b = books.entry(sym.to_string()).or_default();
                    if v["type"] == "snapshot" {
                        *b = Lb::default();
                    }
                    b.levels(true, &v["data"]["b"]);
                    b.levels(false, &v["data"]["a"]);
                    // ApeX's ts is in microseconds
                    bbo(tx, "ApeX", x, b.best(), v["ts"].as_i64().map(|t| if t > 100_000_000_000_000 { t / 1000 } else { t }));
                } else if let Some(sym) = topic.strip_prefix("recentlyTrade.H.") {
                    let Some(x) = m.get(sym) else { return };
                    if v["type"] == "snapshot" {
                        return;
                    }
                    for t in v["data"].as_array().into_iter().flatten() {
                        trade(tx, "ApeX", x, num(&t["p"]), num(&t["v"]), if t["S"] == "Buy" { 1 } else { -1 }, t["T"].as_i64());
                    }
                }
            });
            hs.push(tokio::spawn(run_ws("ApeX", "wss://quote.omni.apex.exchange/realtime_public?v=2".into(), vec![json!({"op": "subscribe", "args": args})], Some(json!({"op": "ping", "args": []})), h, tx)));
        }
        V::Edgex => {
            let m = by(&subs, |x| x.id.clone());
            let ss = subs.iter().flat_map(|x| [json!({"type": "subscribe", "channel": format!("depth.{}.15", x.id)}), json!({"type": "subscribe", "channel": format!("trades.{}", x.id)})]).collect();
            let h: Handler = Box::new(move |v, books, tx, replies| {
                if v["type"] == "ping" {
                    replies.push(json!({"type": "pong", "time": v["time"]}).to_string());
                    return;
                }
                if v["type"] != "quote-event" {
                    return;
                }
                let ch = s(&v["channel"]);
                let c = &v["content"];
                if let Some(rest) = ch.strip_prefix("depth.") {
                    let id = rest.split('.').next().unwrap_or("").to_string();
                    let Some(x) = m.get(&id) else { return };
                    let b = books.entry(id).or_default();
                    for d in c["data"].as_array().into_iter().flatten() {
                        if c["dataType"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("snapshot")) || d["depthType"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("snapshot")) {
                            *b = Lb::default();
                        }
                        b.levels(true, &d["bids"]);
                        b.levels(false, &d["asks"]);
                    }
                    bbo(tx, "edgeX", x, b.best(), None);
                } else if let Some(id) = ch.strip_prefix("trades.") {
                    let Some(x) = m.get(id) else { return };
                    if c["dataType"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("snapshot")) {
                        return;
                    }
                    for t in c["data"].as_array().into_iter().flatten() {
                        let side = match t["isBuyerMaker"].as_bool() { Some(true) => -1, Some(false) => 1, None => 0 };
                        trade(tx, "edgeX", x, num(&t["price"]), num(&t["size"]), side, s(&t["time"]).parse().ok());
                    }
                }
            });
            hs.push(tokio::spawn(run_ws("edgeX", "wss://edgex-quote-prod-v2.edgex.exchange/api/v1/public/ws".into(), ss, None, h, tx)));
        }
        V::Pacifica => {
            let m = by(&subs, |x| x.sym.clone());
            let ss = subs.iter().flat_map(|x| [json!({"method": "subscribe", "params": {"source": "book", "symbol": x.sym, "agg_level": 1}}), json!({"method": "subscribe", "params": {"source": "trades", "symbol": x.sym}})]).collect();
            let h: Handler = Box::new(move |v, _b, tx, _r| match v["channel"].as_str() {
                Some("book") => {
                    let d = &v["data"];
                    let Some(x) = m.get(&s(&d["s"])) else { return };
                    let mut b = Lb::default();
                    b.levels(true, &d["l"][0]);
                    b.levels(false, &d["l"][1]);
                    bbo(tx, "Pacifica", x, b.best(), d["t"].as_i64());
                }
                Some("trades") => {
                    for t in v["data"].as_array().into_iter().flatten() {
                        let Some(x) = m.get(&s(&t["s"])) else { continue };
                        let dd = s(&t["d"]);
                        let side = if dd == "open_long" || dd == "close_short" { 1 } else if dd == "open_short" || dd == "close_long" { -1 } else { 0 };
                        trade(tx, "Pacifica", x, num(&t["p"]), num(&t["a"]), side, t["t"].as_i64());
                    }
                }
                _ => {}
            });
            hs.push(tokio::spawn(run_ws("Pacifica", "wss://ws.pacifica.fi/ws".into(), ss, Some(json!({"method": "ping"})), h, tx)));
        }
        V::Standx => {
            let m = by(&subs, |x| x.sym.clone());
            let ss = subs.iter().flat_map(|x| [json!({"subscribe": {"channel": "depth_book", "symbol": x.sym}}), json!({"subscribe": {"channel": "public_trade", "symbol": x.sym}})]).collect();
            let h: Handler = Box::new(move |v, _b, tx, _r| {
                let Some(x) = m.get(&s(&v["symbol"])) else { return };
                let d = &v["data"];
                match v["channel"].as_str() {
                    Some("depth_book") => {
                        let mut b = Lb::default();
                        b.levels(true, &d["bids"]);
                        b.levels(false, &d["asks"]);
                        bbo(tx, "StandX", x, b.best(), crate::venues::standx::parse_iso(&s(&d["time"])));
                    }
                    Some("public_trade") => {
                        for t in d.as_array().cloned().unwrap_or_else(|| vec![d.clone()]) {
                            let side = match s(&t["side"]).to_lowercase().as_str() { "buy" => 1, "sell" => -1, _ => 0 };
                            let px = ["price", "px", "p"].iter().map(|k| num(&t[*k])).find(|x| *x > 0.0).unwrap_or(0.0);
                            let sz = ["qty", "size", "q", "sz"].iter().map(|k| num(&t[*k])).find(|x| *x > 0.0).unwrap_or(0.0);
                            trade(tx, "StandX", x, px, sz, side, crate::venues::standx::parse_iso(&s(&t["time"])));
                        }
                    }
                    _ => {}
                }
            });
            hs.push(tokio::spawn(run_ws("StandX", "wss://perps.standx.com/ws-stream/v1".into(), ss, None, h, tx)));
        }
        V::Gmx => {
            hs.push(tokio::spawn(gmx_poll(subs, tx)));
        }
    }
    hs
}

/// GMX: the oracle's min / max each second (no stream; no trades published here).
async fn gmx_poll(subs: Vec<Sub>, tx: UnboundedSender<Ev>) {
    let h = crate::http::Http::new();
    let dec: HashMap<String, i32> = h.get("https://arbitrum-api.gmxinfra.io/tokens").await.ok().map(|r| {
        r["tokens"].as_array().into_iter().flatten().map(|t| (s(&t["address"]).to_lowercase(), t["decimals"].as_i64().unwrap_or(18) as i32)).collect()
    }).unwrap_or_default();
    let m: HashMap<String, Sub> = subs.into_iter().map(|x| (x.id.to_lowercase(), x)).collect();
    loop {
        if let Ok(r) = h.get("https://arbitrum-api.gmxinfra.io/prices/tickers").await {
            for t in r.as_array().into_iter().flatten() {
                let a = s(&t["tokenAddress"]).to_lowercase();
                if let Some(x) = m.get(&a) {
                    let sc = 10f64.powi(30 - *dec.get(&a).unwrap_or(&18));
                    let (lo, hi) = (s(&t["minPrice"]).parse::<f64>().unwrap_or(0.0) / sc, s(&t["maxPrice"]).parse::<f64>().unwrap_or(0.0) / sc);
                    bbo(&tx, "GMX", x, Some((lo, 0.0, hi, 0.0)), t["updatedAt"].as_i64());
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The subscriptions of each venue for the assets (the registry's active instruments).
pub async fn plan(h: &crate::http::Http, assets: &[String]) -> Vec<(V, Vec<Sub>)> {
    let reg = Registry::load(h, &ALL).await;
    let mut out = Vec::new();
    for v in ALL {
        let Some(ins) = reg.by_venue.get(&v) else { continue };
        let mut subs = Vec::new();
        for a in assets {
            if let Some(i) = ins.iter().find(|i| &i.base == a && i.active && registry::usd_like(&i.quote)) {
                subs.push(Sub { asset: a.clone(), sym: i.symbol.clone(), id: i.id.clone().unwrap_or_default(), mult: f(i.multiplier) });
            }
        }
        out.push((v, subs));
    }
    out
}

/// An underlying's funding across the venues now (GMX left out: its side-specific pool rates).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Rank {
    pub asset: String,
    pub venues: usize,
    /// max - min of the venues' hourly rates, x 8760 x 100: the APR a hedge between the two extremes would carry now.
    pub spread_apr: f64,
    pub high: (String, f64),
    pub low: (String, f64),
}

/// The shared underlyings ranked by the cross-venue funding spread now.
pub async fn rank(h: &crate::http::Http) -> Vec<Rank> {
    let reg = Registry::load(h, &ALL).await;
    let shared = reg.shared();
    let mut per_venue: HashMap<V, Vec<Instrument>> = HashMap::new();
    for i in shared.values().flatten() {
        let v = registry::venue_of(i);
        if v != V::Gmx {
            per_venue.entry(v).or_default().push(i.clone());
        }
    }
    let got = futures::future::join_all(per_venue.iter().map(|(v, ins)| async move {
        (*v, tokio::time::timeout(Duration::from_secs(120), v.funding_now(h, ins)).await.ok().and_then(|r| r.ok()).unwrap_or_default(), ins)
    }))
    .await;
    let mut rates: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for (v, fs, ins) in got {
        let by_sym: HashMap<&str, &Instrument> = ins.iter().map(|i| (i.symbol.as_str(), i)).collect();
        for fnow in fs {
            if let Some(i) = by_sym.get(fnow.symbol.as_str()) {
                if fnow.rate_h.is_finite() {
                    rates.entry(i.base.clone()).or_default().push((v.name().to_string(), fnow.rate_h));
                }
            }
        }
    }
    let mut out: Vec<Rank> = rates
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .map(|(asset, v)| {
            let hi = v.iter().cloned().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            let lo = v.iter().cloned().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            Rank { asset, venues: v.len(), spread_apr: (hi.1 - lo.1) * 8760.0 * 100.0, high: (hi.0, hi.1 * 8760.0 * 100.0), low: (lo.0, lo.1 * 8760.0 * 100.0) }
        })
        .collect();
    out.sort_by(|a, b| b.spread_apr.total_cmp(&a.spread_apr));
    out
}

/// The re-rank: right after each hour's funding payment (the user's call: the venues pay every hour), at :02.
fn next_rerank_ms(t: i64) -> i64 {
    (t / 3_600_000 + 1) * 3_600_000 + 120_000
}

/// `fixed`: the assets given; None: the `top` by the cross-venue funding spread, re-ranked each hour after the payment
/// (the streams restarted on the new list).
pub async fn run(h: &crate::http::Http, fixed: Option<Vec<String>>, top: usize, hours: f64) -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let start = now_ms();
    let dir = crate::data_dir().join("ws");
    std::fs::create_dir_all(&dir)?;
    loop {
        let assets = match &fixed {
            Some(a) => a.clone(),
            None => {
                let r = rank(h).await;
                println!("[{}] the funding spread now (APR between the venues' extremes), the top {top}:", now_ms() / 1000);
                for x in r.iter().take(top) {
                    println!("  {:<10} {:>7.1}%  {} venues, high {} {:+.1}%, low {} {:+.1}%", x.asset, x.spread_apr, x.venues, x.high.0, x.high.1, x.low.0, x.low.1);
                }
                let _ = std::fs::write(dir.join(format!("assets_{}.json", now_ms() / 1000)), serde_json::to_string_pretty(&r.iter().take(top).collect::<Vec<_>>()).unwrap_or_default());
                r.into_iter().take(top).map(|x| x.asset).collect()
            }
        };
        let plan = plan(h, &assets).await;
        for (v, s_) in &plan {
            println!("{:<11} {} streams: {}", v.name(), s_.len(), s_.iter().map(|x| x.sym.as_str()).collect::<Vec<_>>().join(" "));
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Ev>();
        let mut handles = Vec::new();
        for (v, s_) in plan {
            handles.extend(spawn(v, s_, tx.clone()));
        }
        drop(tx);
        let end_all = if hours > 0.0 { start + (hours * 3.6e6) as i64 } else { i64::MAX };
        let until = if fixed.is_some() { end_all } else { end_all.min(next_rerank_ms(now_ms())) };
        write(rx, &dir, until).await?;
        for hd in handles {
            hd.abort();
        }
        if now_ms() >= end_all {
            return Ok(());
        }
    }
}

/// The writer: the events to data/ws/<day>.csv until `until_ms`, a health line a minute.
async fn write(mut rx: tokio::sync::mpsc::UnboundedReceiver<Ev>, dir: &std::path::Path, until_ms: i64) -> Result<()> {
    let mut day = -1i64;
    let mut out: Option<std::io::BufWriter<std::fs::File>> = None;
    let mut last: HashMap<(&'static str, String), (f64, f64, f64, f64)> = HashMap::new();
    let (mut nb, mut nt) = (HashMap::<&'static str, u64>::new(), HashMap::<&'static str, u64>::new());
    let mut seen: HashMap<&'static str, i64> = HashMap::new();
    let mut report = tokio::time::interval(Duration::from_secs(60));
    let mut flush = tokio::time::interval(Duration::from_secs(1));
    report.tick().await;
    loop {
        tokio::select! {
            e = rx.recv() => {
                let Some(e) = e else { break };
                let t = now_ms();
                if t / 86_400_000 != day {
                    day = t / 86_400_000;
                    if let Some(mut o) = out.take() { let _ = o.flush(); }
                    out = Some(std::io::BufWriter::with_capacity(1 << 20, std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{day}.csv")))?));
                }
                let o = out.as_mut().unwrap();
                match e {
                    Ev::Bbo { venue, asset, bid, bsz, ask, asz, ts_ex } => {
                        let k = (venue, asset.clone());
                        if last.get(&k) == Some(&(bid, bsz, ask, asz)) { continue; }
                        last.insert(k, (bid, bsz, ask, asz));
                        writeln!(o, "B,{t},{},{venue},{asset},{bid},{bsz},{ask},{asz}", ts_ex.unwrap_or(0))?;
                        *nb.entry(venue).or_default() += 1;
                        seen.insert(venue, t);
                    }
                    Ev::Trade { venue, asset, px, sz, side, ts_ex } => {
                        writeln!(o, "T,{t},{},{venue},{asset},{px},{sz},{side}", ts_ex.unwrap_or(0))?;
                        *nt.entry(venue).or_default() += 1;
                        seen.insert(venue, t);
                    }
                }
                if t >= until_ms { break; }
            }
            _ = flush.tick() => {
                if let Some(o) = out.as_mut() { let _ = o.flush(); }
                if now_ms() >= until_ms { break; }
            }
            _ = report.tick() => {
                let t = now_ms();
                let line: Vec<String> = ALL.iter().map(|v| { let n = v.name(); format!("{n} {}/{} ({}s)", nb.get(n).unwrap_or(&0), nt.get(n).unwrap_or(&0), seen.get(n).map_or(-1, |x| (t - x) / 1000)) }).collect();
                println!("[{}] book changes / trades a minute (last msg age): {}", t / 1000, line.join(" | "));
                nb.clear();
                nt.clear();
            }
        }
    }
    if let Some(mut o) = out.take() {
        let _ = o.flush();
    }
    Ok(())
}
