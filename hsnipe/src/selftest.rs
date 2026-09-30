//! `hsnipe simtest --slot S`: prove the clone-and-substitute transaction builder
//! against mainnet without spending anything.
//!
//! Takes real pump.fun / PumpSwap trades from a block, clones each instruction onto a
//! DIFFERENT real wallet, and runs simulateTransaction (sigVerify off):
//!   * curve buys are simulated for a funded wallet from the same block,
//!   * sells are simulated for another trader of the same coin, selling what they bought,
//!   * PumpSwap sells are also cross-coin mapped (template from pool A, sell on pool B).
//! A passing buy/sell here means account substitution and data rewriting are right.

use crate::backfill::decode_block_json;
use crate::coin::{AmmState, Coin};
use crate::decode::Decoder;
use crate::live::{buy_data, cross_coin_amm_map, sell_data, substitute};
use crate::model::{Event, Venue};
use crate::pump::{self, TOKEN, TOKEN_2022, WSOL};
use crate::rpc::Rpc;
use crate::tx;
use crate::types::Pk;
use serde_json::Value;
use solana_keypair::Keypair;
use std::collections::HashMap;

fn tail(v: &Value) -> String {
    let logs = v["value"]["logs"].as_array().cloned().unwrap_or_default();
    logs.iter().rev().take(3).rev().filter_map(|l| l.as_str()).map(|s| s.chars().take(110).collect::<String>()).collect::<Vec<_>>().join(" | ")
}

async fn sim(rpc: &Rpc, payer: Pk, ixs: Vec<tx::Ix>) -> anyhow::Result<(bool, String, u64)> {
    let msg = tx::compile(&payer, &ixs, &[1u8; 32])?;
    let (_, wire) = tx::sign(&Keypair::new(), &msg);
    let v = rpc.simulate(&wire).await?;
    let err = v["value"]["err"].clone();
    Ok((err.is_null(), if err.is_null() { String::new() } else { format!("{err} :: {}", tail(&v)) }, wire.len() as u64))
}

pub async fn run(rpc_url: &str, slot: Option<u64>, max_each: usize) -> anyhow::Result<()> {
    let rpc = Rpc::new(rpc_url);
    let slot = match slot {
        Some(s) => s,
        None => rpc.get_slot("confirmed").await? - 3,
    };
    let body = rpc.get_block(slot).await?.unwrap_or_default();
    let dec = Decoder { wallet: None, capture_templates: true };
    // Decode again to also collect templates (decode_block_json only returns events).
    let Some((_, events)) = decode_block_json(slot, &body, 0, &Decoder::default())? else {
        anyhow::bail!("slot {slot} has no block");
    };
    let templates = crate::backfill::templates_from_block_json(slot, &body, &dec)?;
    println!("slot {slot}: {} events, {} templates", events.len(), templates.len());

    // Latest trade per (mint,user) and a funded wallet for buys.
    let mut trades: Vec<&crate::model::TradeEv> = vec![];
    let mut amms: Vec<&crate::model::AmmEv> = vec![];
    for e in &events {
        match e {
            Event::Trade(t) => trades.push(t),
            Event::Amm(a) => amms.push(a),
            _ => {}
        }
    }
    let rich = trades.iter().filter(|t| t.post_sol > 2_000_000_000).map(|t| t.user).next();
    let (mut ok, mut bad) = (0, 0);
    let mut done: HashMap<(Venue, bool), usize> = HashMap::new();
    let mut xcoin_done = 0;
    for t in &templates {
        let n = done.entry((t.venue, t.is_buy)).or_default();
        if *n >= max_each {
            continue;
        }
        let tp = if t.accounts.iter().any(|a| a.pk == *TOKEN_2022) { *TOKEN_2022 } else { *TOKEN };
        let res = match (t.venue, t.is_buy) {
            (Venue::Curve, true) => {
                let Some(w) = rich.filter(|w| *w != t.user) else { continue };
                let Some(tr) = trades.iter().find(|x| x.mint == t.mint) else { continue };
                let mut coin = Coin::bare(t.mint, slot, 0);
                coin.vsol = tr.vsol;
                coin.vtok = tr.vtok;
                coin.rtok = tr.rtok;
                coin.fee_bps = tr.fee_bps as u64;
                coin.cfee_bps = tr.cfee_bps as u64;
                let budget = 10_000_000; // 0.01 SOL
                let (exp, _, _) = coin.buy_quote(budget);
                let mut ix = match substitute(t, &w, &t.mint, &[]) {
                    Ok(ix) => ix,
                    Err(e) => {
                        println!("  curve buy  {}  build error: {e}", t.mint);
                        continue;
                    }
                };
                ix.data = buy_data(&t.data, budget, exp / 2)?;
                let ixs = vec![tx::set_cu_limit(300_000), tx::create_ata_idempotent(w, w, t.mint, tp), ix];
                Some(("curve buy ", w, sim(&rpc, w, ixs).await?))
            }
            (Venue::Curve, false) => {
                // Another trader whose LAST trade of this coin in the block is a buy: sell half of it.
                let last_of = |u: &Pk| trades.iter().rev().find(|x| x.mint == t.mint && x.user == *u);
                let Some(other) = trades.iter().find(|x| {
                    x.mint == t.mint && x.buy && x.user != t.user && x.tok > 0 && last_of(&x.user).is_some_and(|l| l.buy)
                }) else {
                    continue;
                };
                let mut ix = match substitute(t, &other.user, &t.mint, &[]) {
                    Ok(ix) => ix,
                    Err(e) => {
                        println!("  curve sell {}  build error: {e}", t.mint);
                        continue;
                    }
                };
                ix.data = sell_data(&t.data, other.tok / 2, 1);
                Some(("curve sell", other.user, sim(&rpc, other.user, vec![tx::set_cu_limit(300_000), ix]).await?))
            }
            (Venue::Amm, false) => {
                let Some(other) = amms.iter().find(|x| x.pool == t.pool && x.buy && x.user != t.user && x.base > 0) else { continue };
                let mut ix = match substitute(t, &other.user, &t.mint, &[]) {
                    Ok(ix) => ix,
                    Err(e) => {
                        println!("  amm sell   {}  build error: {e}", t.mint);
                        continue;
                    }
                };
                ix.data = sell_data(&t.data, other.base, 1);
                let wsol = tx::ata(&other.user, &WSOL, &TOKEN);
                let ixs = vec![
                    tx::set_cu_limit(300_000),
                    tx::create_ata_idempotent(other.user, other.user, *WSOL, *TOKEN),
                    ix,
                    tx::close_account(*TOKEN, wsol, other.user, other.user),
                ];
                Some(("amm sell  ", other.user, sim(&rpc, other.user, ixs).await?))
            }
            _ => None,
        };
        if let Some((what, w, (pass, err, size))) = res {
            *n += 1;
            if pass {
                ok += 1
            } else {
                bad += 1
            }
            println!("  {what} {}  as {}  {} ({} bytes) {}", t.mint, w, if pass { "OK " } else { "ERR" }, size, err);
        }
        // Cross-coin: PumpSwap sell template from this pool applied to a different pool.
        if t.venue == Venue::Amm && !t.is_buy && xcoin_done < max_each {
            if let Some(other) = amms.iter().find(|x| x.pool != t.pool && x.buy && x.base > 0 && !x.coin_creator.is_zero()) {
                let mut coin = Coin::bare(other.mint, slot, 0);
                coin.token_program = tp;
                coin.amm_creator = other.coin_creator;
                coin.amm = Some(AmmState { base: other.pb, quote: other.pq, ..Default::default() });
                let r = cross_coin_amm_map(t, other.pool, &coin).and_then(|extra| {
                    let mut ix = substitute(t, &other.user, &other.mint, &extra)?;
                    ix.data = sell_data(&t.data, other.base, 1);
                    Ok(ix)
                });
                if std::env::var("SIMTEST_DEBUG").is_ok() {
                    if let (Ok(ix), Some(real)) = (&r, templates.iter().find(|x| x.venue == Venue::Amm && x.pool == other.pool)) {
                        println!("    ours n={} real n={} real_is_buy={}", ix.accounts.len(), real.accounts.len(), real.is_buy); for i in 0..ix.accounts.len().max(real.accounts.len()) { let (Some(ia), Some(ra)) = (ix.accounts.get(i), real.accounts.get(i)) else { println!("    [{i}] ours {:?} real {:?}", ix.accounts.get(i).map(|a| a.pk), real.accounts.get(i).map(|a| a.pk)); continue };
                            let (a, b) = (ia.pk, ra.pk);
                            if a != b && b != real.user && i != 1 && i != 5 && i != 6 {
                                println!("    diff[{i}] ours {a} real {b}");
                            }
                        }
                    }
                }
                match r {
                    Ok(ix) => {
                        let wsol = tx::ata(&other.user, &WSOL, &TOKEN);
                        let ixs = vec![
                            tx::set_cu_limit(300_000),
                            tx::create_ata_idempotent(other.user, other.user, *WSOL, *TOKEN),
                            ix,
                            tx::close_account(*TOKEN, wsol, other.user, other.user),
                        ];
                        let (pass, err, size) = sim(&rpc, other.user, ixs).await?;
                        xcoin_done += 1;
                        if pass {
                            ok += 1
                        } else {
                            bad += 1
                        }
                        println!("  amm x-coin {} -> {}  {} ({} bytes) {}", t.mint, other.mint, if pass { "OK " } else { "ERR" }, size, err);
                    }
                    Err(e) => println!("  amm x-coin build error: {e}"),
                }
            }
        }
    }
    let _ = pump::is_sol_quote;
    println!("simulations: {ok} ok, {bad} failed");
    Ok(())
}
