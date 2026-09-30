//! Yellowstone gRPC ingestion with automatic reconnection. One task per endpoint
//! (e.g. Kaldera Frankfurt + New York); the engine keeps whichever copy arrives first.

use crate::decode::{Decoded, Decoder, RawIx, RawTx};
use crate::engine::{Msg, PoolWatch};
use crate::model::Event;
use crate::pump::{AMM, PUMP};
use crate::types::{now_ms, Pk, Sig};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterBlocksMeta,
    SubscribeRequestFilterSlots, SubscribeRequestFilterTransactions, SubscribeRequestPing, SubscribeUpdateTransactionInfo,
};

pub struct GrpcCfg {
    pub url: String,
    pub x_token: Option<String>,
    pub src: u8,
    pub idle_timeout: Duration,
    pub wallet: Option<Pk>,
}

fn build_request(pools: &[Pk], wallet: Option<Pk>) -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        "pump".to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            account_include: vec![PUMP.b58()],
            ..Default::default()
        },
    );
    if !pools.is_empty() {
        // Only the PumpSwap pools of coins we track: an empty include list would mean "everything".
        transactions.insert(
            "amm".to_string(),
            SubscribeRequestFilterTransactions {
                vote: Some(false),
                failed: Some(false),
                account_include: pools.iter().map(|p| p.b58()).collect(),
                account_required: vec![AMM.b58()],
                ..Default::default()
            },
        );
    }
    if let Some(w) = wallet {
        // Our own transactions, including failed ones (confirmation + error reporting).
        transactions.insert(
            "own".to_string(),
            SubscribeRequestFilterTransactions { vote: Some(false), failed: None, account_include: vec![w.b58()], ..Default::default() },
        );
    }
    let mut blocks_meta = HashMap::new();
    blocks_meta.insert("bm".to_string(), SubscribeRequestFilterBlocksMeta {});
    let mut slots = HashMap::new();
    slots.insert(
        "slots".to_string(),
        SubscribeRequestFilterSlots { filter_by_commitment: Some(false), interslot_updates: Some(true) },
    );
    SubscribeRequest {
        transactions,
        blocks_meta,
        slots,
        commitment: Some(CommitmentLevel::Processed as i32),
        ..Default::default()
    }
}

/// Human-readable form of a bincode TransactionError (enough for logs).
pub fn tx_err_string(b: &[u8]) -> String {
    if b.len() >= 4 {
        let v = u32::from_le_bytes(b[..4].try_into().unwrap());
        if v == 8 && b.len() >= 9 {
            let ix = b[4];
            let inner = u32::from_le_bytes(b[5..9].try_into().unwrap());
            if inner == 25 && b.len() >= 13 {
                return format!("ix{ix}:Custom({})", u32::from_le_bytes(b[9..13].try_into().unwrap()));
            }
            return format!("ix{ix}:InstructionError({inner})");
        }
        return format!("TransactionError({v})");
    }
    "error".into()
}

pub fn raw_from_proto(info: SubscribeUpdateTransactionInfo, slot: u64, ts: i64) -> Option<RawTx> {
    let tx = info.transaction?;
    let meta = info.meta?;
    let msg = tx.message?;
    let header = msg.header.unwrap_or_default();
    let n_static = msg.account_keys.len();
    let mut keys = Vec::with_capacity(n_static + meta.loaded_writable_addresses.len() + meta.loaded_readonly_addresses.len());
    for k in msg.account_keys.iter().chain(meta.loaded_writable_addresses.iter()).chain(meta.loaded_readonly_addresses.iter()) {
        keys.push(Pk::from_slice(k).unwrap_or_default());
    }
    let (v1_prio, v1_cu_limit) = msg.config.map(|c| (c.priority_fee, c.compute_unit_limit)).unwrap_or((None, None));
    Some(RawTx {
        slot,
        idx: info.index as u32,
        ts,
        sig: Sig::from_slice(&info.signature),
        keys,
        header: [header.num_required_signatures, header.num_readonly_signed_accounts, header.num_readonly_unsigned_accounts],
        n_static,
        n_loaded_w: meta.loaded_writable_addresses.len(),
        outer: msg.instructions.into_iter().map(|i| RawIx { program: i.program_id_index, accounts: i.accounts, data: i.data }).collect(),
        inner: meta
            .inner_instructions
            .into_iter()
            .map(|g| (g.index, g.instructions.into_iter().map(|i| RawIx { program: i.program_id_index, accounts: i.accounts, data: i.data }).collect()))
            .collect(),
        pre: meta.pre_balances,
        post: meta.post_balances,
        err: meta.err.map(|e| tx_err_string(&e.err)),
        fee: meta.fee,
        v1_prio,
        v1_cu_limit,
    })
}

pub async fn run(cfg: GrpcCfg, decoder: Decoder, tx: UnboundedSender<Msg>, pools: Arc<PoolWatch>) {
    let mut backoff = Duration::from_millis(100);
    loop {
        let started = Instant::now();
        match session(&cfg, &decoder, &tx, &pools).await {
            Ok(()) => {
                let _ = tx.send(Msg::Stream { src: cfg.src, up: false, note: "stream ended".into() });
            }
            Err(e) => {
                let _ = tx.send(Msg::Stream { src: cfg.src, up: false, note: format!("{e:#}") });
                eprintln!("[grpc {}] {e:#}", cfg.src);
            }
        }
        if tx.is_closed() {
            return;
        }
        if started.elapsed() > Duration::from_secs(30) {
            backoff = Duration::from_millis(100);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

async fn session(cfg: &GrpcCfg, decoder: &Decoder, tx: &UnboundedSender<Msg>, pools: &Arc<PoolWatch>) -> anyhow::Result<()> {
    let mut builder = GeyserGrpcClient::build_from_shared(cfg.url.clone())?
        .x_token(cfg.x_token.clone())?
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(10))
        .keep_alive_while_idle(true)
        .max_decoding_message_size(64 * 1024 * 1024);
    if cfg.url.starts_with("https") {
        builder = builder.tls_config(ClientTlsConfig::new().with_native_roots())?;
    }
    let mut client = builder.connect().await?;
    let mut pools_rx = pools.subscribe();
    let current: Vec<Pk> = pools_rx.borrow_and_update().clone();
    let (mut sink, mut stream) = client.subscribe_with_request(Some(build_request(&current, cfg.wallet))).await?;
    let _ = tx.send(Msg::Stream { src: cfg.src, up: true, note: format!("connected {}", cfg.url) });
    let mut ping_id = 0i32;
    loop {
        let next = tokio::select! {
            m = tokio::time::timeout(cfg.idle_timeout, stream.next()) => m,
            r = pools_rx.changed() => {
                if r.is_err() {
                    return Ok(());
                }
                let current: Vec<Pk> = pools_rx.borrow_and_update().clone();
                sink.send(build_request(&current, cfg.wallet)).await?;
                continue;
            }
        };
        let item = match next {
            Err(_) => anyhow::bail!("no message for {:?}", cfg.idle_timeout),
            Ok(None) => return Ok(()),
            Ok(Some(Err(status))) => anyhow::bail!("status: {status}"),
            Ok(Some(Ok(u))) => u,
        };
        let recv = Instant::now();
        let ts = now_ms();
        let server_lag_ms = item.created_at.as_ref().map(|c| ts - (c.seconds * 1000 + c.nanos as i64 / 1_000_000));
        match item.update_oneof {
            Some(UpdateOneof::Transaction(t)) => {
                let slot = t.slot;
                let Some(info) = t.transaction else { continue };
                let Some(raw) = raw_from_proto(info, slot, ts) else { continue };
                let mut out = Decoded::default();
                decoder.decode(&raw, &mut out);
                if out.events.is_empty() {
                    continue;
                }
                let key = raw.sig.key() ^ slot.rotate_left(17);
                if tx
                    .send(Msg::Tx { evs: out.events, tmpls: out.templates, recv, key, src: cfg.src, server_lag_ms })
                    .is_err()
                {
                    return Ok(());
                }
            }
            Some(UpdateOneof::BlockMeta(b)) => {
                let ev = Event::Block {
                    slot: b.slot,
                    ts,
                    hash: b.blockhash,
                    height: b.block_height.map(|h| h.block_height),
                    block_time: b.block_time.map(|t| t.timestamp),
                };
                let _ = tx.send(Msg::Ev { ev, recv, src: cfg.src });
            }
            Some(UpdateOneof::Slot(s)) => {
                // 3 = FIRST_SHRED_RECEIVED, 5 = CREATED_BANK: the earliest signs of a new slot.
                if s.status == 3 || s.status == 5 {
                    let _ = tx.send(Msg::Ev { ev: Event::Slot { slot: s.slot, ts }, recv, src: cfg.src });
                }
            }
            Some(UpdateOneof::Ping(_)) => {
                ping_id = ping_id.wrapping_add(1);
                sink.send(SubscribeRequest { ping: Some(SubscribeRequestPing { id: ping_id }), ..Default::default() }).await?;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backfill::{decode_block_json, templates_from_block_json};
    use serde_json::Value;
    use yellowstone_grpc_proto::prelude::{
        CompiledInstruction, InnerInstruction, InnerInstructions, Message, MessageHeader, Transaction, TransactionConfig,
        TransactionError, TransactionStatusMeta,
    };

    const BLOCK: &str = include_str!("../tests/fixtures/block_pump_sample.json");

    fn b58(s: &Value) -> Vec<u8> {
        bs58::decode(s.as_str().unwrap()).into_vec().unwrap()
    }

    /// Rebuild what Yellowstone would send for a getBlock JSON transaction.
    fn to_proto(tx: &Value, index: u64) -> SubscribeUpdateTransactionInfo {
        let m = &tx["transaction"]["message"];
        let meta = &tx["meta"];
        let ix = |i: &Value| CompiledInstruction {
            program_id_index: i["programIdIndex"].as_u64().unwrap() as u32,
            accounts: i["accounts"].as_array().unwrap().iter().map(|a| a.as_u64().unwrap() as u8).collect(),
            data: b58(&i["data"]),
        };
        let keys = |v: &Value| v.as_array().map(|a| a.iter().map(b58).collect::<Vec<_>>()).unwrap_or_default();
        let cfg = &m["transactionConfig"];
        SubscribeUpdateTransactionInfo {
            signature: b58(&tx["transaction"]["signatures"][0]),
            is_vote: false,
            index,
            transaction: Some(Transaction {
                signatures: vec![b58(&tx["transaction"]["signatures"][0])],
                message: Some(Message {
                    header: Some(MessageHeader {
                        num_required_signatures: m["header"]["numRequiredSignatures"].as_u64().unwrap() as u32,
                        num_readonly_signed_accounts: m["header"]["numReadonlySignedAccounts"].as_u64().unwrap() as u32,
                        num_readonly_unsigned_accounts: m["header"]["numReadonlyUnsignedAccounts"].as_u64().unwrap() as u32,
                    }),
                    account_keys: keys(&m["accountKeys"]),
                    recent_blockhash: b58(&m["recentBlockhash"]),
                    instructions: m["instructions"].as_array().unwrap().iter().map(ix).collect(),
                    versioned: !tx["version"].is_string(),
                    address_table_lookups: vec![],
                    config: cfg.is_object().then(|| TransactionConfig {
                        priority_fee: cfg["priorityFee"].as_u64(),
                        compute_unit_limit: cfg["computeUnitLimit"].as_u64().map(|v| v as u32),
                        ..Default::default()
                    }),
                }),
            }),
            meta: Some(TransactionStatusMeta {
                err: (!meta["err"].is_null()).then(|| TransactionError { err: vec![8, 0, 0, 0, 1, 25, 0, 0, 0, 1, 0, 0, 0] }),
                fee: meta["fee"].as_u64().unwrap(),
                pre_balances: meta["preBalances"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect(),
                post_balances: meta["postBalances"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect(),
                inner_instructions: meta["innerInstructions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|g| InnerInstructions {
                        index: g["index"].as_u64().unwrap() as u32,
                        instructions: g["instructions"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|i| {
                                let c = ix(i);
                                InnerInstruction {
                                    program_id_index: c.program_id_index,
                                    accounts: c.accounts,
                                    data: c.data,
                                    stack_height: i["stackHeight"].as_u64().map(|v| v as u32),
                                }
                            })
                            .collect(),
                    })
                    .collect(),
                loaded_writable_addresses: keys(&meta["loadedAddresses"]["writable"]),
                loaded_readonly_addresses: keys(&meta["loadedAddresses"]["readonly"]),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn grpc_and_rpc_paths_decode_identically() {
        let dec = Decoder { wallet: None, capture_templates: true };
        let (_, json_events) = decode_block_json(7, BLOCK, 0, &dec).unwrap().unwrap();
        let json_tmpls = templates_from_block_json(7, BLOCK, &dec).unwrap();

        let v: Value = serde_json::from_str(BLOCK).unwrap();
        let mut proto_events = vec![];
        let mut proto_tmpls = 0;
        for (i, tx) in v["result"]["transactions"].as_array().unwrap().iter().enumerate() {
            let raw = raw_from_proto(to_proto(tx, i as u64), 7, 0).unwrap();
            let mut out = Decoded::default();
            dec.decode(&raw, &mut out);
            proto_events.extend(out.events);
            proto_tmpls += out.templates.len();
        }
        let a = serde_json::to_string(&json_events).unwrap();
        let b = serde_json::to_string(&proto_events).unwrap();
        assert_eq!(a, b);
        assert_eq!(json_tmpls.len(), proto_tmpls);

        let count = |t: &str| json_events.iter().filter(|e| serde_json::to_value(e).unwrap()["t"] == t).count();
        assert!(count("create") >= 1, "create");
        assert!(count("trade") >= 4, "trades");
        assert!(count("amm") >= 2, "amm");
        assert_eq!(count("migrate"), 1, "migration");
        for e in &json_events {
            match e {
                Event::Migrate(m) => {
                    assert_eq!(m.mint.b58(), "BmCVLzZyCNt6tPciNYP9AvSeidrfFV5pFbzvksZkpump");
                    assert_eq!((m.base, m.quote), (206_900_000_000_000, 84_990_359_056));
                }
                Event::Trade(t) => {
                    // Mayhem coins trade fee-free on their own virtual reserves.
                    assert!(t.vsol > 0 && t.vtok > 0 && (t.fee_bps > 0 || t.mayhem), "{}", serde_json::to_string(t).unwrap());
                    assert!(t.ix > 0, "unknown ix name");
                }
                Event::Amm(a) => assert!(a.pb > 0 && a.pq > 0 && a.lp_bps > 0),
                _ => {}
            }
        }
        // templates: every captured account list has exactly one signer (the trader)
        for t in &json_tmpls {
            assert_eq!(t.accounts.iter().filter(|a| a.signer).count(), 1, "{:?}", t.mint);
            assert!(t.accounts.iter().any(|a| a.pk == t.user && a.signer && a.writable));
        }
    }

    #[test]
    fn tx_error_strings() {
        assert_eq!(tx_err_string(&[8, 0, 0, 0, 2, 25, 0, 0, 0, 0xae, 0x17, 0, 0]), "ix2:Custom(6062)");
    }
}
