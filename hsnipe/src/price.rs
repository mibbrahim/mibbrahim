//! SOL/USD for the volume filter. Tries Jupiter, then Coinbase, then Binance.

use crate::engine::Msg;
use crate::model::Event;
use crate::types::now_ms;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

pub async fn fetch(http: &reqwest::Client) -> Option<f64> {
    let tries: [(&str, fn(&Value) -> Option<f64>); 3] = [
        ("https://lite-api.jup.ag/price/v3?ids=So11111111111111111111111111111111111111112", |v| {
            v["So11111111111111111111111111111111111111112"]["usdPrice"].as_f64()
        }),
        ("https://api.coinbase.com/v2/prices/SOL-USD/spot", |v| v["data"]["amount"].as_str()?.parse().ok()),
        ("https://api.binance.com/api/v3/ticker/price?symbol=SOLUSDT", |v| v["price"].as_str()?.parse().ok()),
    ];
    for (url, parse) in tries {
        if let Ok(r) = http.get(url).timeout(Duration::from_secs(5)).send().await {
            if let Ok(v) = r.json::<Value>().await {
                if let Some(p) = parse(&v).filter(|p| *p > 1.0 && *p < 100_000.0) {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub async fn run(tx: UnboundedSender<Msg>, every: Duration) {
    let http = reqwest::Client::new();
    loop {
        if let Some(usd) = fetch(&http).await {
            if tx.send(Msg::Ev { ev: Event::Price { ts: now_ms(), usd }, recv: Instant::now(), src: 250 }).is_err() {
                return;
            }
        }
        tokio::time::sleep(every).await;
    }
}

/// Historical 5-minute SOL-USD closes from Coinbase for [start, end] (unix seconds).
pub async fn history(http: &reqwest::Client, start: i64, end: i64) -> Vec<(i64, f64)> {
    let mut out = vec![];
    let mut t = start - 300;
    while t < end {
        let t2 = (t + 300 * 290).min(end + 300);
        let url = format!("https://api.exchange.coinbase.com/products/SOL-USD/candles?granularity=300&start={t}&end={t2}");
        if let Ok(r) = http.get(&url).header("User-Agent", "hsnipe").timeout(Duration::from_secs(10)).send().await {
            if let Ok(Value::Array(rows)) = r.json::<Value>().await {
                for row in rows {
                    if let (Some(ts), Some(close)) = (row[0].as_i64(), row[4].as_f64()) {
                        out.push((ts, close));
                    }
                }
            }
        }
        t = t2;
    }
    out.sort_by_key(|x| x.0);
    out.dedup_by_key(|x| x.0);
    out
}
