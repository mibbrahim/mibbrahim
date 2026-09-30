//! Historical data: fetch blocks over RPC (getBlock), run them through the same decoder
//! as the live stream, and write an event file the replay mode consumes.

use crate::decode::{Decoded, Decoder, RawIx, RawTx};
use crate::model::Event;
use crate::pump::{AMM, PUMP};
use crate::rpc::Rpc;
use crate::types::{Pk, Sig};
use anyhow::Context;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Resp {
    result: Option<Block>,
    error: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    blockhash: String,
    block_time: Option<i64>,
    block_height: Option<u64>,
    transactions: Vec<Tx>,
}

#[derive(Deserialize)]
struct Tx {
    transaction: TxInner,
    meta: Option<Meta>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxInner {
    signatures: Vec<String>,
    message: Msg,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Msg {
    account_keys: Vec<String>,
    header: Header,
    instructions: Vec<Ix>,
    #[serde(default)]
    transaction_config: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    num_required_signatures: u32,
    num_readonly_signed_accounts: u32,
    num_readonly_unsigned_accounts: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ix {
    program_id_index: u32,
    accounts: Vec<u32>,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    err: Option<Value>,
    fee: u64,
    pre_balances: Vec<u64>,
    post_balances: Vec<u64>,
    #[serde(default)]
    inner_instructions: Option<Vec<Inner>>,
    #[serde(default)]
    loaded_addresses: Option<Loaded>,
}

#[derive(Deserialize)]
struct Inner {
    index: u32,
    instructions: Vec<Ix>,
}

#[derive(Deserialize, Default)]
struct Loaded {
    writable: Vec<String>,
    readonly: Vec<String>,
}

fn conv_ix(i: Ix) -> RawIx {
    RawIx {
        program: i.program_id_index,
        accounts: i.accounts.into_iter().map(|a| a.min(255) as u8).collect(),
        data: bs58::decode(i.data).into_vec().unwrap_or_default(),
    }
}

/// Convert one getBlock JSON body into (block event, decoded events). Only transactions
/// touching pump.fun or PumpSwap are decoded.
pub fn decode_block_json(slot: u64, body: &str, ts: i64, decoder: &Decoder) -> anyhow::Result<Option<(Event, Vec<Event>)>> {
    Ok(decode_block(slot, body, ts, decoder)?.map(|(b, d)| (b, d.events)))
}

/// Instruction templates (for live cloning) found in one getBlock body.
pub fn templates_from_block_json(slot: u64, body: &str, decoder: &Decoder) -> anyhow::Result<Vec<crate::model::Tmpl>> {
    Ok(decode_block(slot, body, 0, decoder)?.map(|(_, d)| d.templates).unwrap_or_default())
}

fn decode_block(slot: u64, body: &str, ts: i64, decoder: &Decoder) -> anyhow::Result<Option<(Event, Decoded)>> {
    let r: Resp = serde_json::from_str(body).context("getBlock json")?;
    if let Some(e) = r.error {
        let code = e["code"].as_i64().unwrap_or(0);
        // skipped / missing / not available slots
        if code == -32007 || code == -32009 || code == -32004 {
            return Ok(None);
        }
        anyhow::bail!("getBlock {slot}: {e}");
    }
    let Some(b) = r.result else { return Ok(None) };
    let pump = PUMP.b58();
    let amm = AMM.b58();
    let mut out = Decoded::default();
    for (idx, t) in b.transactions.into_iter().enumerate() {
        let Some(meta) = t.meta else { continue };
        let loaded = meta.loaded_addresses.unwrap_or_default();
        let touches = |s: &String| *s == pump || *s == amm;
        if !(t.transaction.message.account_keys.iter().any(touches) || loaded.writable.iter().any(touches) || loaded.readonly.iter().any(touches)) {
            continue;
        }
        let m = t.transaction.message;
        let n_static = m.account_keys.len();
        let n_loaded_w = loaded.writable.len();
        let keys: Vec<Pk> = m
            .account_keys
            .iter()
            .chain(loaded.writable.iter())
            .chain(loaded.readonly.iter())
            .map(|s| Pk::parse(s).unwrap_or_default())
            .collect();
        let cfgv = m.transaction_config.unwrap_or(Value::Null);
        let raw = RawTx {
            slot,
            idx: idx as u32,
            ts,
            sig: t.transaction.signatures.first().and_then(|s| Sig::parse(s).ok()).unwrap_or_default(),
            keys,
            header: [m.header.num_required_signatures, m.header.num_readonly_signed_accounts, m.header.num_readonly_unsigned_accounts],
            n_static,
            n_loaded_w,
            outer: m.instructions.into_iter().map(conv_ix).collect(),
            inner: meta.inner_instructions.unwrap_or_default().into_iter().map(|g| (g.index, g.instructions.into_iter().map(conv_ix).collect())).collect(),
            pre: meta.pre_balances,
            post: meta.post_balances,
            err: meta.err.map(|e| e.to_string()),
            fee: meta.fee,
            v1_prio: cfgv["priorityFee"].as_u64(),
            v1_cu_limit: cfgv["computeUnitLimit"].as_u64().map(|v| v as u32),
        };
        decoder.decode(&raw, &mut out);
    }
    let block = Event::Block { slot, ts, hash: b.blockhash, height: b.block_height, block_time: b.block_time };
    Ok(Some((block, out)))
}

fn block_time_of(body: &str) -> Option<i64> {
    // Cheap scan instead of a second full parse.
    let i = body.find("\"blockTime\":")?;
    let rest = &body[i + 12..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-')?;
    rest[..end].parse().ok()
}

pub async fn run(rpc_url: &str, from: u64, to: u64, out_path: &str, concurrency: usize) -> anyhow::Result<()> {
    let rpc = Rpc::new(rpc_url);
    let decoder = Decoder::default();
    let file = std::fs::File::create(out_path)?;
    let mut w: Box<dyn Write> = if out_path.ends_with(".gz") {
        Box::new(flate2::write::GzEncoder::new(std::io::BufWriter::new(file), flate2::Compression::fast()))
    } else {
        Box::new(std::io::BufWriter::with_capacity(1 << 20, file))
    };
    eprintln!("backfill slots {from}..={to} ({} slots) from {rpc_url} with concurrency {concurrency}", to - from + 1);

    let fetch = |slot: u64| {
        let rpc = rpc.clone();
        async move {
            let mut delay = Duration::from_millis(500);
            for _ in 0..8 {
                match rpc.get_block(slot).await {
                    Ok(Some(body)) if body.contains("\"code\":429") || body.contains("Too many requests") => {}
                    Ok(Some(body)) => return (slot, Some(body)),
                    Ok(None) => return (slot, None),
                    Err(_) => {}
                }
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(10));
            }
            eprintln!("slot {slot}: giving up");
            (slot, None)
        }
    };

    let mut stream = futures::stream::iter(from..=to).map(fetch).buffered(concurrency.max(1));
    let mut created: HashSet<Pk> = HashSet::new();
    let mut pools: HashSet<Pk> = HashSet::new();
    let (mut last_bt, mut same_bt) = (0i64, 0i64);
    let (mut n_blocks, mut n_events) = (0u64, 0u64);
    let started = Instant::now();
    let mut prices: Vec<(i64, f64)> = vec![];
    let mut price_i = 0usize;
    let http = reqwest::Client::new();
    while let Some((slot, body)) = stream.next().await {
        let Some(body) = body else { continue };
        let bt = block_time_of(&body).unwrap_or(last_bt);
        if prices.is_empty() && bt > 0 {
            prices = crate::price::history(&http, bt, bt + ((to - slot) as i64 * 400 / 1000) + 600).await;
            eprintln!("fetched {} SOL-USD candles", prices.len());
        }
        if bt == last_bt {
            same_bt += 1;
        } else {
            last_bt = bt;
            same_bt = 0;
        }
        let ts = bt * 1000 + (same_bt * 400).min(999);
        let Some((block, evs)) = decode_block_json(slot, &body, ts, &decoder)? else { continue };
        while price_i < prices.len() && prices[price_i].0 * 1000 <= ts {
            let p = Event::Price { ts: prices[price_i].0 * 1000, usd: prices[price_i].1 };
            serde_json::to_writer(&mut w, &p)?;
            w.write_all(b"\n")?;
            price_i += 1;
        }
        serde_json::to_writer(&mut w, &Event::Slot { slot, ts })?;
        w.write_all(b"\n")?;
        for ev in evs {
            // Keep only coins launched inside the window (others cannot be evaluated).
            let keep = match &ev {
                Event::Create(c) => {
                    created.insert(c.mint);
                    true
                }
                Event::Trade(t) => created.contains(&t.mint),
                Event::Complete { mint, .. } => created.contains(mint),
                Event::Migrate(m) => {
                    if created.contains(&m.mint) {
                        pools.insert(m.pool);
                        true
                    } else {
                        false
                    }
                }
                Event::Amm(a) => pools.contains(&a.pool) || created.contains(&a.mint),
                _ => false,
            };
            if keep {
                serde_json::to_writer(&mut w, &ev)?;
                w.write_all(b"\n")?;
                n_events += 1;
            }
        }
        serde_json::to_writer(&mut w, &block)?;
        w.write_all(b"\n")?;
        n_blocks += 1;
        if n_blocks % 500 == 0 {
            let rate = n_blocks as f64 / started.elapsed().as_secs_f64();
            eprintln!(
                "slot {slot}: {n_blocks} blocks, {n_events} events, {} coins, {:.1} blocks/s, eta {:.0} min",
                created.len(),
                rate,
                (to - slot) as f64 / rate.max(0.01) / 60.0
            );
        }
    }
    w.flush()?;
    drop(w);
    eprintln!("done: {n_blocks} blocks, {n_events} events, {} coins -> {out_path}", created.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn block_time_scan() {
        assert_eq!(super::block_time_of(r#"{"x":1,"blockTime":1790787282,"y":2}"#), Some(1790787282));
    }
}
