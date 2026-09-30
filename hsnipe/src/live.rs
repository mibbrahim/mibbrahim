//! Live trading for the Powerful Bot.
//!
//! Transactions are built by cloning a real pump.fun / PumpSwap instruction seen on
//! chain (buy from a buy, sell from a sell) and substituting only the trader-specific
//! accounts. Every tx goes to staked RPC (SWQoS), normal RPC and Jito at once.
//! Emergency exits pay far higher fees and accept any price (min_out = 1).

use crate::coin::Coin;
use crate::config::{Config, LiveCfg};
use crate::coin::Feat;
use crate::engine::Msg;
use crate::logger::{Logger, Samples};
use crate::model::{Event, IxTemplate, OwnEv, Tmpl, Venue};
use crate::pump::{self, *};
use crate::rpc::{jito_send, Rpc};
use crate::strategy::{check_exit, ExitAction, ExitInput, ExitKind, ExitState};
use crate::tx::{self, Ix, Meta};
use crate::types::{lamports, now_ms, r6, sol, sol_i, Pk, Sig};
use anyhow::{anyhow, bail};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::json;
use solana_keypair::Keypair;
use solana_signer::Signer;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

pub enum LiveMsg {
    Sent { sig: Sig, route: &'static str, ok: bool, detail: String, ms: f64 },
    Simulated { mint: Pk, kind: &'static str, result: serde_json::Value },
    Blockhash { hash: [u8; 32], slot: u64 },
    Balance(u64),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosState {
    Buying,
    Open,
    Selling,
    Closed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LiveLeg {
    pub kind: String,
    pub sig: String,
    pub slot: u64,
    pub tokens: u64,
    pub sol: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LivePos {
    pub mint: Pk,
    pub symbol: String,
    pub trigger_id: u64,
    pub signal_slot: u64,
    pub signal_ts: i64,
    pub entry_slot: u64,
    pub entry_ts: i64,
    pub token_program: Pk,
    /// SOL paid into the trade (curve sol + protocol/creator fees)
    pub cost: u64,
    /// Sum of our wallet balance changes over every leg (fees, tips, rent included)
    pub wallet_delta: i64,
    pub tokens_orig: u64,
    pub tokens_left: u64,
    pub proceeds: u64,
    pub ex: ExitState,
    pub state: PosState,
    pub sell_attempts: u32,
    pub last_pct: f64,
    pub legs: Vec<LiveLeg>,
    /// Last sell template seen for this mint (kept across restarts as a fallback)
    pub sell_tmpl: Option<IxTemplate>,
    pub exit_path: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Buy,
    Sell(ExitAction),
    CloseAta,
}

struct Order {
    sig: Sig,
    mint: Pk,
    kind: Kind,
    expected: u64,
    signal_slot: u64,
    recv: Option<Instant>,
    decision: Instant,
    built: Instant,
    sent_slot: u64,
    cu_price: u64,
    tip: u64,
    into_slot_ms: Option<i64>,
    acks: Vec<(&'static str, bool, f64)>,
    landed: Option<(u64, Option<String>, i64, u64)>,
    filled_tokens: u64,
    filled_sol: u64,
    confirm_ms: Option<f64>,
    done: bool,
}

pub struct Live {
    cfg: Arc<Config>,
    lc: LiveCfg,
    log: Logger,
    tx: UnboundedSender<Msg>,
    kp: Arc<Keypair>,
    pub wallet: Pk,
    rpc: Rpc,
    staked: Option<Rpc>,
    http: reqwest::Client,
    bh: Option<([u8; 32], u64, i64)>,
    last_block_ms: i64,
    last_bh_fetch: i64,
    balance: u64,
    last_balance_fetch: i64,
    pub positions: HashMap<Pk, LivePos>,
    orders: HashMap<Sig, Order>,
    global_amm_sell: Option<Tmpl>,
    stop: bool,
    kill: bool,
    last_file_check: i64,
    dirty: bool,
    closed: u64,
    pnl_event: f64,
    pnl_wallet: f64,
    fails: HashMap<String, u64>,
    lat_build_us: Samples,
    lat_confirm_ms: Samples,
    lat_ack_ms: Samples,
    slot_delta: Samples,
    my_ata_cache: HashMap<Pk, Pk>,
}

fn now() -> i64 {
    now_ms()
}

impl Live {
    pub fn new(cfg: Arc<Config>, log: Logger, tx: UnboundedSender<Msg>) -> anyhow::Result<Live> {
        let lc = cfg.live.clone();
        // Simulation can run as any address (sigVerify is off), so no key is needed for it.
        let kp = if lc.keypair_path.is_empty() && lc.simulate {
            Keypair::new()
        } else {
            solana_keypair::read_keypair_file(&lc.keypair_path).map_err(|e| anyhow!("read keypair {}: {e}", lc.keypair_path))?
        };
        let wallet = match (&lc.simulate_as, lc.simulate) {
            (Some(a), true) => Pk::parse(a)?,
            _ => Pk(kp.pubkey().to_bytes()),
        };
        let mut positions = HashMap::new();
        if Path::new(&lc.positions_file).exists() {
            let body = std::fs::read_to_string(&lc.positions_file)?;
            let list: Vec<LivePos> = serde_json::from_str(&body)?;
            for p in list.into_iter().filter(|p| p.state != PosState::Closed && p.tokens_left > 0) {
                eprintln!("restored live position {} ({} tokens)", p.mint, p.tokens_left);
                let mut p = p;
                p.state = PosState::Open;
                positions.insert(p.mint, p);
            }
        }
        Ok(Live {
            rpc: Rpc::new(&cfg.rpc_url),
            staked: cfg.staked_rpc_url.as_deref().map(Rpc::new),
            http: reqwest::Client::builder().tcp_nodelay(true).timeout(Duration::from_secs(10)).build()?,
            cfg,
            lc,
            log,
            tx,
            kp: Arc::new(kp),
            wallet,
            bh: None,
            last_block_ms: 0,
            last_bh_fetch: 0,
            balance: 0,
            last_balance_fetch: 0,
            positions,
            orders: HashMap::new(),
            global_amm_sell: None,
            stop: false,
            kill: false,
            last_file_check: 0,
            dirty: false,
            closed: 0,
            pnl_event: 0.0,
            pnl_wallet: 0.0,
            fails: HashMap::new(),
            lat_build_us: Samples::default(),
            lat_confirm_ms: Samples::default(),
            lat_ack_ms: Samples::default(),
            slot_delta: Samples::default(),
            my_ata_cache: HashMap::new(),
        })
    }

    /// (mint, entry_ts, entry_slot) of restored positions.
    pub fn restored(&self) -> Vec<(Pk, i64, u64)> {
        self.positions.values().map(|p| (p.mint, p.entry_ts, p.entry_slot)).collect()
    }

    pub fn holds(&self, mint: &Pk) -> bool {
        self.positions.contains_key(mint)
    }

    fn fail(&mut self, why: impl Into<String>, mint: &Pk) {
        let why = why.into();
        *self.fails.entry(why.clone()).or_default() += 1;
        self.log.line("live_legs.jsonl", &json!({"kind":"skip","ts":now(),"mint":mint,"reason":why}));
    }

    // ------------------------------------------------------------------ inputs

    pub fn on_template(&mut self, t: Tmpl, coins: &mut HashMap<Pk, Coin>) {
        if t.venue == Venue::Amm && !t.is_buy {
            self.global_amm_sell = Some(t.clone());
        }
        if let Some(p) = self.positions.get_mut(&t.mint) {
            if !t.is_buy && t.venue == Venue::Curve {
                p.sell_tmpl = Some((*t).clone());
            }
        }
        if let Some(c) = coins.get_mut(&t.mint) {
            match (t.venue, t.is_buy) {
                (Venue::Curve, true) => c.last_buy_tmpl = Some(t),
                (Venue::Curve, false) => c.last_sell_tmpl = Some(t),
                (Venue::Amm, false) => c.last_amm_sell_tmpl = Some(t),
                _ => {}
            }
        }
    }

    pub fn on_block(&mut self, slot: u64, hash: &str, now_ms: i64) {
        if let Ok(h) = crate::rpc::decode_hash(hash) {
            if self.bh.is_none_or(|b| slot >= b.1) {
                self.bh = Some((h, slot, now_ms));
            }
            self.last_block_ms = now();
        }
    }

    pub fn on_msg(&mut self, m: LiveMsg, _coins: &HashMap<Pk, Coin>, _cur_slot: u64, _now: i64) {
        match m {
            LiveMsg::Sent { sig, route, ok, detail, ms } => {
                self.lat_ack_ms.push(ms);
                if let Some(o) = self.orders.get_mut(&sig) {
                    o.acks.push((route, ok, ms));
                }
                if !ok {
                    *self.fails.entry(format!("send_{route}")).or_default() += 1;
                    self.log.line("live_legs.jsonl", &json!({"kind":"send_error","ts":now(),"sig":sig,"route":route,"detail":detail}));
                }
            }
            LiveMsg::Simulated { mint, kind, result } => {
                let err = result["value"]["err"].clone();
                let logs = result["value"]["logs"].as_array().map(|l| l.iter().rev().take(6).rev().cloned().collect::<Vec<_>>()).unwrap_or_default();
                self.log.line(
                    "live_legs.jsonl",
                    &json!({"kind":"simulated","side":kind,"ts":now(),"mint":mint,"ok":err.is_null(),"err":err,
                        "units":result["value"]["unitsConsumed"],"logs_tail":logs}),
                );
                *self.fails.entry(format!("sim_{kind}_{}", if err.is_null() { "ok" } else { "err" })).or_default() += 1;
            }
            LiveMsg::Blockhash { hash, slot } => {
                if self.bh.is_none_or(|b| slot > b.1) {
                    self.bh = Some((hash, slot, now()));
                }
            }
            LiveMsg::Balance(b) => self.balance = b,
        }
    }

    /// Our own transaction landed (or failed). Fill amounts arrive with the trade event.
    pub fn on_own(&mut self, o: &OwnEv, coins: &HashMap<Pk, Coin>, cur_slot: u64, now_ms: i64) {
        let Some(ord) = self.orders.get_mut(&o.sig) else { return };
        if ord.landed.is_some() {
            return;
        }
        ord.landed = Some((o.slot, o.err.clone(), o.sol_delta, o.fee));
        ord.confirm_ms = Some(ord.built.elapsed().as_secs_f64() * 1000.0);
        let sig = o.sig;
        if o.err.is_some() {
            self.finish_order(sig, coins, cur_slot, now_ms);
        }
        // success: wait for the Trade/Amm event in the same message to fill amounts
    }

    /// Trades and pool swaps: catch our own fills.
    pub fn on_market_event(&mut self, ev: &Event, coins: &HashMap<Pk, Coin>, cur_slot: u64, now_ms: i64) {
        let (sig, tokens, sol_amt) = match ev {
            Event::Trade(t) if t.user == self.wallet => {
                let fees = fee(t.sol, t.fee_bps as u64) + fee(t.sol, t.cfee_bps as u64);
                (t.sig, t.tok, if t.buy { t.sol + fees } else { t.sol.saturating_sub(fees) })
            }
            Event::Amm(a) if a.user == self.wallet => (a.sig, a.base, a.quote),
            _ => return,
        };
        if let Some(ord) = self.orders.get_mut(&sig) {
            ord.filled_tokens += tokens;
            ord.filled_sol += sol_amt;
            if ord.landed.is_none() {
                ord.landed = Some((ev.slot().unwrap_or(cur_slot), None, 0, 0));
                ord.confirm_ms = Some(ord.built.elapsed().as_secs_f64() * 1000.0);
            }
            self.finish_order(sig, coins, cur_slot, now_ms);
        } else if let Event::Trade(t) = ev {
            // A buy we had given up on (timeout) landed after all: adopt it so it gets sold.
            if t.buy && !self.positions.contains_key(&t.mint) {
                self.log.line("live_legs.jsonl", &json!({"kind":"orphan_buy_adopted","ts":now_ms,"mint":t.mint,"sig":t.sig,"tokens":t.tok}));
                let coin = coins.get(&t.mint);
                self.positions.insert(t.mint, LivePos {
                    mint: t.mint,
                    symbol: coin.map(|c| c.symbol.clone()).unwrap_or_default(),
                    trigger_id: 0,
                    signal_slot: t.slot,
                    signal_ts: now_ms,
                    entry_slot: t.slot,
                    entry_ts: now_ms,
                    token_program: coin.map(|c| c.token_program).unwrap_or(*TOKEN_2022),
                    cost: sol_amt,
                    wallet_delta: -(sol_amt as i64),
                    tokens_orig: t.tok,
                    tokens_left: t.tok,
                    proceeds: 0,
                    ex: ExitState::default(),
                    state: PosState::Open,
                    sell_attempts: 0,
                    last_pct: 0.0,
                    legs: vec![],
                    sell_tmpl: coin.and_then(|c| c.last_sell_tmpl.as_ref().map(|t| (**t).clone())),
                    exit_path: vec![],
                });
                self.dirty = true;
            }
        }
    }

    fn finish_order(&mut self, sig: Sig, coins: &HashMap<Pk, Coin>, cur_slot: u64, now_ms: i64) {
        let Some(mut o) = self.orders.remove(&sig) else { return };
        if o.done {
            return;
        }
        o.done = true;
        let (landed_slot, err, delta, fee_paid) = o.landed.clone().unwrap_or((0, Some("timeout".into()), 0, 0));
        if landed_slot > 0 && err.is_none() {
            self.slot_delta.push(landed_slot.saturating_sub(o.signal_slot) as f64);
        }
        if let Some(c) = o.confirm_ms {
            self.lat_confirm_ms.push(c);
        }
        let kind_s = match o.kind {
            Kind::Buy => "buy".to_string(),
            Kind::Sell(a) => format!("sell:{}", a.kind.name()),
            Kind::CloseAta => "close_ata".to_string(),
        };
        self.log.line(
            "live_legs.jsonl",
            &json!({"kind":kind_s,"ts":now_ms,"mint":o.mint,"sig":sig,"ok":err.is_none(),"err":err,
                "signal_slot":o.signal_slot,"sent_slot":o.sent_slot,"landed_slot":landed_slot,
                "slot_delta":if landed_slot>0 {landed_slot as i64 - o.signal_slot as i64} else {-1},
                "expected":o.expected,"filled_tokens":o.filled_tokens,"filled_sol":r6(sol(o.filled_sol)),
                "fill_vs_expected":r6(match o.kind { Kind::Buy => if o.expected>0 {o.filled_tokens as f64/o.expected as f64 - 1.0} else {0.0},
                                                     _ => if o.expected>0 {o.filled_sol as f64/o.expected as f64 - 1.0} else {0.0} }),
                "wallet_delta_sol":r6(sol_i(delta)),"tx_fee":fee_paid,"cu_price":o.cu_price,"tip":o.tip,
                "recv_to_decision_us":o.recv.map(|r| o.decision.duration_since(r).as_micros() as u64),
                "decision_to_signed_us":o.built.duration_since(o.decision).as_micros() as u64,
                "send_to_confirm_ms":o.confirm_ms.map(r6),"acks":o.acks.iter().map(|(r,ok,ms)| json!({"route":r,"ok":ok,"ms":r6(*ms)})).collect::<Vec<_>>(),
                "ms_into_slot":o.into_slot_ms}),
        );
        let ok = err.is_none() && landed_slot > 0;
        match o.kind {
            Kind::Buy => {
                let Some(p) = self.positions.get_mut(&o.mint) else { return };
                if ok && o.filled_tokens > 0 {
                    p.state = PosState::Open;
                    p.entry_slot = landed_slot;
                    p.entry_ts = now_ms;
                    p.cost = o.filled_sol;
                    p.tokens_orig = o.filled_tokens;
                    p.tokens_left = o.filled_tokens;
                    p.wallet_delta += delta;
                    p.legs.push(LiveLeg { kind: "buy".into(), sig: sig.b58(), slot: landed_slot, tokens: o.filled_tokens, sol: r6(sol(o.filled_sol)) });
                    self.dirty = true;
                } else {
                    *self.fails.entry(format!("buy_failed:{}", err.clone().unwrap_or_default())).or_default() += 1;
                    self.positions.remove(&o.mint);
                }
            }
            Kind::Sell(act) => {
                let Some(p) = self.positions.get_mut(&o.mint) else { return };
                p.state = PosState::Open;
                if ok && o.filled_tokens > 0 {
                    p.tokens_left = p.tokens_left.saturating_sub(o.filled_tokens);
                    p.proceeds += o.filled_sol;
                    p.wallet_delta += delta;
                    p.sell_attempts = 0;
                    p.exit_path.push(act.kind.name().into());
                    p.legs.push(LiveLeg { kind: act.kind.name().into(), sig: sig.b58(), slot: landed_slot, tokens: o.filled_tokens, sol: r6(sol(o.filled_sol)) });
                    self.dirty = true;
                    if p.tokens_left == 0 || (act.frac.is_none() && p.tokens_left < p.tokens_orig / 1000) {
                        self.close_position(o.mint, coins, now_ms, landed_slot);
                    }
                } else {
                    p.wallet_delta += delta;
                    p.sell_attempts += 1;
                    let attempts = p.sell_attempts;
                    *self.fails.entry(format!("sell_failed:{}", err.clone().unwrap_or_default())).or_default() += 1;
                    // Retry right away; after the first failure every retry is an emergency.
                    if attempts <= self.lc.max_sell_retries {
                        let retry = ExitAction { kind: act.kind, frac: act.frac, emergency: true };
                        if let Some(c) = coins.get(&o.mint) {
                            self.sell(c, retry, cur_slot, now_ms);
                        }
                    }
                }
            }
            Kind::CloseAta => {}
        }
    }

    fn close_position(&mut self, mint: Pk, coins: &HashMap<Pk, Coin>, now_ms: i64, slot: u64) {
        let Some(mut p) = self.positions.remove(&mint) else { return };
        p.state = PosState::Closed;
        let pnl = p.proceeds as i64 - p.cost as i64;
        self.closed += 1;
        self.pnl_event += sol_i(pnl);
        self.pnl_wallet += sol_i(p.wallet_delta);
        self.dirty = true;
        self.log.line(
            "live_closes.jsonl",
            &json!({"strategy":self.cfg.mode.name(),"mint":mint,"symbol":p.symbol,"trigger":p.trigger_id,
                "signal_slot":p.signal_slot,"entry_slot":p.entry_slot,"exit_slot":slot,"entry_ts":p.entry_ts,"exit_ts":now_ms,
                "hold_s":r6((now_ms-p.entry_ts) as f64/1000.0),"cost_sol":r6(sol(p.cost)),"proceeds_sol":r6(sol(p.proceeds)),
                "pnl_sol":r6(sol_i(pnl)),"pnl_pct":r6(if p.cost>0 {pnl as f64/p.cost as f64} else {0.0}),
                "wallet_pnl_sol":r6(sol_i(p.wallet_delta)),"peak_pct":r6(p.ex.peak_pct),"low_pct":r6(p.ex.low_pct),
                "exit_path":p.exit_path,"legs":p.legs}),
        );
        if self.lc.close_ata && !self.lc.simulate {
            if let Some(c) = coins.get(&mint) {
                let tp = if p.token_program.is_zero() { c.token_program } else { p.token_program };
                let ata = self.my_ata(&mint, &tp);
                let ixs = vec![tx::set_cu_limit(20_000), tx::set_cu_price(10_000), tx::close_account(tp, ata, self.wallet, self.wallet)];
                let _ = self.submit(ixs, mint, Kind::CloseAta, 0, slot, None, Instant::now(), None, 0, 0, false);
            }
        }
    }

    // ------------------------------------------------------------------ decisions

    #[allow(clippy::too_many_arguments)]
    pub fn on_signal(&mut self, coin: &Coin, _feat: &Arc<Feat>, trigger_id: u64, recv: Option<Instant>, decision: Instant, slot: u64, now_ms: i64, into_slot: Option<i64>) {
        if self.stop || self.kill {
            return self.fail("stop_file", &coin.mint);
        }
        if self.positions.contains_key(&coin.mint) {
            return;
        }
        if self.positions.len() >= self.lc.max_open {
            return self.fail("max_open", &coin.mint);
        }
        if !self.lc.simulate && self.balance < lamports(self.lc.min_balance_sol + self.lc.size_sol) {
            return self.fail("low_balance", &coin.mint);
        }
        if coin.complete || coin.migrated {
            return self.fail("curve_complete", &coin.mint);
        }
        let Some(buy_t) = coin.last_buy_tmpl.clone() else { return self.fail("no_buy_template", &coin.mint) };
        if coin.last_sell_tmpl.is_none() {
            return self.fail("no_sell_template", &coin.mint);
        }
        let budget = lamports(self.lc.size_sol);
        let (exp_tokens, _, _) = coin.buy_quote(budget);
        if exp_tokens == 0 {
            return self.fail("zero_quote", &coin.mint);
        }
        let min_tokens = (exp_tokens as u128 * (10_000 - self.lc.buy_slippage_bps.min(9_900)) as u128 / 10_000) as u64;
        let tp = token_program_of(&buy_t, coin);
        let ix = match substitute(&buy_t, &self.wallet, &coin.mint, &[]).and_then(|mut ix| {
            ix.data = buy_data(&buy_t.data, budget, min_tokens)?;
            Ok(ix)
        }) {
            Ok(ix) => ix,
            Err(e) => return self.fail(format!("buy_template: {e}"), &coin.mint),
        };
        let ixs = vec![
            tx::set_cu_limit(self.lc.cu_limit),
            tx::set_cu_price(self.lc.cu_price),
            tx::create_ata_idempotent(self.wallet, self.wallet, coin.mint, tp),
            ix,
        ];
        let (cu, tip) = (self.lc.cu_price, self.lc.jito_tip);
        match self.submit(ixs, coin.mint, Kind::Buy, exp_tokens, slot, recv, decision, into_slot, cu, tip, true) {
            Ok(_) if self.lc.simulate => {}
            Ok(_) => {
                self.positions.insert(coin.mint, LivePos {
                    mint: coin.mint,
                    symbol: coin.symbol.clone(),
                    trigger_id,
                    signal_slot: slot,
                    signal_ts: now_ms,
                    entry_slot: 0,
                    entry_ts: now_ms,
                    token_program: tp,
                    cost: 0,
                    wallet_delta: 0,
                    tokens_orig: 0,
                    tokens_left: 0,
                    proceeds: 0,
                    ex: ExitState::default(),
                    state: PosState::Buying,
                    sell_attempts: 0,
                    last_pct: 0.0,
                    legs: vec![],
                    sell_tmpl: coin.last_sell_tmpl.as_ref().map(|t| (**t).clone()),
                    exit_path: vec![],
                });
            }
            Err(e) => self.fail(format!("submit: {e}"), &coin.mint),
        }
        if self.lc.simulate {
            // Also exercise the sell path (expected to fail on balance, but validates accounts).
            let act = ExitAction { kind: ExitKind::TimeStop, frac: None, emergency: true };
            if let Ok(ixs) = self.sell_ixs(coin, None, exp_tokens, 1, act) {
                let _ = self.submit(ixs, coin.mint, Kind::Sell(act), 0, slot, None, Instant::now(), None, 0, 0, false);
            }
        }
    }

    pub fn evaluate(&mut self, coin: &Coin, now_ms: i64, cur_slot: u64) {
        let Some(p) = self.positions.get_mut(&coin.mint) else { return };
        if p.state != PosState::Open || p.tokens_left == 0 {
            return;
        }
        if self.kill {
            let act = ExitAction { kind: ExitKind::KillSwitch, frac: None, emergency: true };
            return self.sell(coin, act, cur_slot, now_ms);
        }
        let tradable = coin.tradable() && coin.vtok > 0 || coin.migrated && coin.amm.is_some();
        if !tradable {
            return;
        }
        let net = coin.sell_quote(p.tokens_left);
        let basis = p.cost as f64 * p.tokens_left as f64 / p.tokens_orig.max(1) as f64;
        let pct = if basis > 0.0 { net as f64 / basis - 1.0 } else { 0.0 };
        p.last_pct = pct;
        p.ex.observe(pct);
        let x = ExitInput {
            pct,
            age_s: (now_ms - p.entry_ts) as f64 / 1000.0,
            creator_sold_after_entry: coin.creator_sold && coin.creator_sold_ts >= p.signal_ts,
            migrated: coin.migrated,
            ms_since_migration: if coin.migrated { now_ms - coin.migrate_ts } else { 0 },
        };
        if let Some(act) = check_exit(&self.cfg, &mut p.ex, &x) {
            self.dirty = true;
            self.sell(coin, act, cur_slot, now_ms);
        }
    }

    fn sell(&mut self, coin: &Coin, act: ExitAction, cur_slot: u64, _now_ms: i64) {
        let Some(p) = self.positions.get(&coin.mint) else { return };
        let tokens = match act.frac {
            None => p.tokens_left,
            Some(f) => ((p.tokens_orig as f64 * f).round() as u64).min(p.tokens_left),
        };
        if tokens == 0 {
            return;
        }
        let expected = coin.sell_quote(tokens);
        let min_out = if act.emergency {
            1
        } else {
            (expected as u128 * (10_000 - self.lc.sell_slippage_bps.min(9_900)) as u128 / 10_000) as u64
        };
        let persisted = p.sell_tmpl.clone();
        let ixs = match self.sell_ixs(coin, persisted, tokens, min_out, act) {
            Ok(v) => v,
            Err(e) => return self.fail(format!("sell_build: {e}"), &coin.mint),
        };
        let (cu, tip) = if act.emergency { (self.lc.emergency_cu_price, self.lc.emergency_jito_tip) } else { (self.lc.cu_price, self.lc.jito_tip) };
        match self.submit(ixs, coin.mint, Kind::Sell(act), expected, cur_slot, None, Instant::now(), None, cu, tip, true) {
            Ok(_) => {
                if let Some(p) = self.positions.get_mut(&coin.mint) {
                    p.state = PosState::Selling;
                }
            }
            Err(e) => self.fail(format!("sell_submit: {e}"), &coin.mint),
        }
    }

    fn sell_ixs(&mut self, coin: &Coin, persisted: Option<IxTemplate>, tokens: u64, min_out: u64, act: ExitAction) -> anyhow::Result<Vec<Ix>> {
        let (cu, _) = if act.emergency { (self.lc.emergency_cu_price, 0) } else { (self.lc.cu_price, 0) };
        let mut ixs = vec![tx::set_cu_limit(self.lc.cu_limit), tx::set_cu_price(cu)];
        if coin.migrated {
            let wsol_ata = tx::ata(&self.wallet, &WSOL, &TOKEN);
            let (t, extra) = match &coin.last_amm_sell_tmpl {
                Some(t) => (t.clone(), vec![]),
                None if self.lc.amm_cross_coin_template => {
                    let g = self.global_amm_sell.clone().ok_or_else(|| anyhow!("no PumpSwap sell template yet"))?;
                    let pool = coin.pool.ok_or_else(|| anyhow!("pool unknown"))?;
                    let extra = cross_coin_amm_map(&g, pool, coin)?;
                    (g, extra)
                }
                None => bail!("no PumpSwap sell template for this pool"),
            };
            let mut ix = substitute(&t, &self.wallet, &coin.mint, &extra)?;
            ix.data = sell_data(&t.data, tokens, min_out);
            ixs.push(tx::create_ata_idempotent(self.wallet, self.wallet, *WSOL, *TOKEN));
            ixs.push(ix);
            ixs.push(tx::close_account(*TOKEN, wsol_ata, self.wallet, self.wallet));
        } else {
            let t: IxTemplate = match (&coin.last_sell_tmpl, persisted) {
                (Some(t), _) => (**t).clone(),
                (None, Some(t)) => t,
                (None, None) => bail!("no curve sell template"),
            };
            let mut ix = substitute(&t, &self.wallet, &coin.mint, &[])?;
            ix.data = sell_data(&t.data, tokens, min_out);
            ixs.push(ix);
        }
        Ok(ixs)
    }

    fn my_ata(&mut self, mint: &Pk, tp: &Pk) -> Pk {
        let w = self.wallet;
        *self.my_ata_cache.entry(*mint).or_insert_with(|| tx::ata(&w, mint, tp))
    }

    /// Sign and fan out to every route (or simulate). Adds the Jito tip when `tip` > 0.
    #[allow(clippy::too_many_arguments)]
    fn submit(
        &mut self,
        mut ixs: Vec<Ix>,
        mint: Pk,
        kind: Kind,
        expected: u64,
        signal_slot: u64,
        recv: Option<Instant>,
        decision: Instant,
        into_slot: Option<i64>,
        cu_price: u64,
        tip: u64,
        _use_jito: bool,
    ) -> anyhow::Result<Sig> {
        let (hash, bh_slot, _) = self.bh.ok_or_else(|| anyhow!("no blockhash yet"))?;
        if !self.lc.simulate && signal_slot.saturating_sub(bh_slot) > self.lc.max_blockhash_age_slots {
            bail!("blockhash too old ({} slots)", signal_slot.saturating_sub(bh_slot));
        }
        if tip > 0 {
            let to = *JITO_TIPS.choose(&mut rand::thread_rng()).unwrap();
            ixs.push(tx::transfer(self.wallet, to, tip));
        }
        let msg = tx::compile(&self.wallet, &ixs, &hash)?;
        let (sig_bytes, wire) = tx::sign(&self.kp, &msg);
        if wire.len() > tx::MAX_TX_SIZE {
            bail!("tx too large: {} bytes", wire.len());
        }
        let sig = Sig(sig_bytes);
        let built = Instant::now();
        self.lat_build_us.push(built.duration_since(decision).as_micros() as f64);
        if self.lc.simulate {
            let rpc = self.rpc.clone();
            let txc = self.tx.clone();
            let kind_s: &'static str = match kind {
                Kind::Buy => "buy",
                Kind::Sell(_) => "sell",
                Kind::CloseAta => "close_ata",
            };
            tokio::spawn(async move {
                let result = rpc.simulate(&wire).await.unwrap_or_else(|e| json!({"value":{"err": e.to_string()}}));
                let _ = txc.send(Msg::Live(LiveMsg::Simulated { mint, kind: kind_s, result }));
            });
            return Ok(sig);
        }
        let wire = Arc::new(wire);
        let mut routes: Vec<(&'static str, String)> = vec![];
        if self.lc.send_staked {
            if let Some(s) = &self.staked {
                routes.push(("staked", s.url.clone()));
            }
        }
        if self.lc.send_rpc {
            routes.push(("rpc", self.rpc.url.clone()));
        }
        if self.lc.send_jito && tip > 0 {
            for u in &self.cfg.jito_urls {
                routes.push(("jito", u.clone()));
            }
        }
        for (route, url) in routes {
            let wire = wire.clone();
            let txc = self.tx.clone();
            let http = self.http.clone();
            tokio::spawn(async move {
                let t0 = Instant::now();
                let r = if route == "jito" { jito_send(&http, &url, &wire).await } else { Rpc::new(&url).send_tx(&wire).await };
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let (ok, detail) = match r {
                    Ok(s) => (true, s),
                    Err(e) => (false, format!("{e:#}")),
                };
                let _ = txc.send(Msg::Live(LiveMsg::Sent { sig, route, ok, detail, ms }));
            });
        }
        self.orders.insert(sig, Order {
            sig,
            mint,
            kind,
            expected,
            signal_slot,
            recv,
            decision,
            built,
            sent_slot: signal_slot,
            cu_price,
            tip,
            into_slot_ms: into_slot,
            acks: vec![],
            landed: None,
            filled_tokens: 0,
            filled_sol: 0,
            confirm_ms: None,
            done: false,
        });
        Ok(sig)
    }

    pub fn on_tick(&mut self, coins: &HashMap<Pk, Coin>, now_ms: i64, cur_slot: u64) {
        let wall = now();
        if wall - self.last_file_check >= 1000 {
            self.last_file_check = wall;
            let stop = Path::new(&self.lc.stop_file).exists();
            let kill = Path::new(&self.lc.kill_file).exists();
            if stop != self.stop || kill != self.kill {
                self.log.line("live_legs.jsonl", &json!({"kind":"switch","ts":wall,"stop":stop,"kill":kill}));
            }
            self.stop = stop;
            self.kill = kill;
        }
        if !self.lc.simulate && wall - self.last_balance_fetch >= 5000 {
            self.last_balance_fetch = wall;
            let rpc = self.rpc.clone();
            let w = self.wallet.b58();
            let txc = self.tx.clone();
            tokio::spawn(async move {
                if let Ok(b) = rpc.balance(&w).await {
                    let _ = txc.send(Msg::Live(LiveMsg::Balance(b)));
                }
            });
        }
        // Block feed stalled: fall back to RPC for a fresh blockhash.
        if wall - self.last_block_ms > 1500 && wall - self.last_bh_fetch >= 1000 {
            self.last_bh_fetch = wall;
            let rpc = self.rpc.clone();
            let txc = self.tx.clone();
            tokio::spawn(async move {
                if let Ok((hash, slot)) = rpc.latest_blockhash().await {
                    let _ = txc.send(Msg::Live(LiveMsg::Blockhash { hash, slot }));
                }
            });
        }
        // Orders that never showed up.
        let timed_out: Vec<Sig> = self
            .orders
            .values()
            .filter(|o| o.landed.is_none() && cur_slot.saturating_sub(o.sent_slot) > self.lc.confirm_timeout_slots)
            .map(|o| o.sig)
            .collect();
        for s in timed_out {
            if let Some(o) = self.orders.get_mut(&s) {
                o.landed = Some((0, Some("timeout".into()), 0, 0));
            }
            self.finish_order(s, coins, cur_slot, now_ms);
        }
        // Time-based exits and kill switch.
        let mints: Vec<Pk> = self.positions.keys().copied().collect();
        for m in mints {
            if let Some(c) = coins.get(&m) {
                self.evaluate(c, now_ms, cur_slot);
            }
        }
        if self.dirty {
            self.dirty = false;
            let list: Vec<&LivePos> = self.positions.values().filter(|p| p.state != PosState::Buying || p.tokens_left > 0).collect();
            if let Ok(body) = serde_json::to_string_pretty(&list) {
                let tmp = format!("{}.tmp", self.lc.positions_file);
                if std::fs::write(&tmp, body).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.lc.positions_file);
                }
            }
        }
    }

    pub fn summary(&self) -> serde_json::Value {
        json!({
            "wallet": self.wallet,
            "simulate": self.lc.simulate,
            "balance_sol": r6(sol(self.balance)),
            "stop": self.stop,
            "kill": self.kill,
            "blockhash_slot": self.bh.map(|b| b.1),
            "open": self.positions.values().map(|p| json!({"mint":p.mint,"state":p.state,"tokens_left":p.tokens_left,
                "cost_sol":r6(sol(p.cost)),"last_pct":r6(p.last_pct),"peak_pct":r6(p.ex.peak_pct)})).collect::<Vec<_>>(),
            "orders_inflight": self.orders.len(),
            "closed": self.closed,
            "pnl_sol_events": r6(self.pnl_event),
            "pnl_sol_wallet": r6(self.pnl_wallet),
            "fails": self.fails,
            "latency": {
                "decision_to_signed_us": self.lat_build_us.pct(),
                "route_ack_ms": self.lat_ack_ms.pct(),
                "send_to_confirm_ms": self.lat_confirm_ms.pct(),
                "landed_slot_minus_signal_slot": self.slot_delta.pct(),
            }
        })
    }
}

fn fee(amount: u64, bps: u64) -> u64 {
    ((amount as u128 * bps as u128).div_ceil(10_000)) as u64
}

fn token_program_of(t: &IxTemplate, coin: &Coin) -> Pk {
    if !coin.token_program.is_zero() {
        return coin.token_program;
    }
    if t.accounts.iter().any(|a| a.pk == *TOKEN_2022) {
        *TOKEN_2022
    } else {
        *TOKEN
    }
}

/// Clone a template instruction for our wallet: swap the template trader, their token
/// accounts and their volume-accumulator PDAs for ours; keep everything else as-is.
pub fn substitute(t: &IxTemplate, wallet: &Pk, mint: &Pk, extra: &[(Pk, Pk)]) -> anyhow::Result<Ix> {
    let u = t.user;
    let mut map: Vec<(Pk, Pk)> = Vec::with_capacity(12);
    map.push((u, *wallet));
    for tp in [*TOKEN, *TOKEN_2022] {
        map.push((tx::ata(&u, &t.mint, &tp), tx::ata(wallet, mint, &tp)));
    }
    map.push((tx::ata(&u, &WSOL, &TOKEN), tx::ata(wallet, &WSOL, &TOKEN)));
    let uva_pump_u = tx::pda(&[b"user_volume_accumulator", &u.0], &PUMP);
    let uva_pump_w = tx::pda(&[b"user_volume_accumulator", &wallet.0], &PUMP);
    map.push((uva_pump_u, uva_pump_w));
    map.push((tx::ata(&uva_pump_u, &WSOL, &TOKEN), tx::ata(&uva_pump_w, &WSOL, &TOKEN)));
    // PumpSwap volume accumulator and its WSOL account (cashback remaining accounts).
    let uva_amm_u = tx::pda(&[b"user_volume_accumulator", &u.0], &AMM);
    let uva_amm_w = tx::pda(&[b"user_volume_accumulator", &wallet.0], &AMM);
    map.push((uva_amm_u, uva_amm_w));
    map.push((tx::ata(&uva_amm_u, &WSOL, &TOKEN), tx::ata(&uva_amm_w, &WSOL, &TOKEN)));
    map.extend_from_slice(extra);
    if t.mint != *mint {
        map.push((t.mint, *mint));
    }
    let base_atas = [map[1].0, map[2].0];
    let mut replaced_base = false;
    let mut accounts = Vec::with_capacity(t.accounts.len());
    for a in &t.accounts {
        let mut meta = Meta { pk: a.pk, signer: a.signer, writable: a.writable };
        if let Some((_, to)) = map.iter().find(|(from, _)| *from == a.pk) {
            if base_atas.contains(&a.pk) {
                replaced_base = true;
            }
            meta.pk = *to;
        }
        if meta.pk == *wallet {
            meta.signer = true;
            meta.writable = true;
        } else if meta.signer {
            bail!("template has another signer {}", a.pk);
        }
        accounts.push(meta);
    }
    if accounts.iter().any(|a| a.pk == u) {
        bail!("template user still present after substitution");
    }
    if !replaced_base {
        bail!("template trader does not use the canonical token account");
    }
    Ok(Ix { program: t.program, accounts, data: t.data.clone() })
}

/// Rewrite buy args. `buy`/`buy_v2`: (amount, max_sol_cost); `buy_exact_*_in`: (spend, min_out).
pub fn buy_data(tmpl: &[u8], budget: u64, min_tokens: u64) -> anyhow::Result<Vec<u8>> {
    if tmpl.len() < 24 {
        bail!("short buy data");
    }
    let d = pump::disc(tmpl);
    let (a, b) = if d == IX_BUY || d == IX_BUY_V2 {
        (min_tokens, budget)
    } else if d == IX_BUY_EXACT_SOL_IN || d == IX_BUY_EXACT_QUOTE_IN_V2 {
        (budget, min_tokens)
    } else {
        bail!("unknown buy discriminator");
    };
    let mut out = tmpl.to_vec();
    out[8..16].copy_from_slice(&a.to_le_bytes());
    out[16..24].copy_from_slice(&b.to_le_bytes());
    Ok(out)
}

/// `sell`/`sell_v2`/PumpSwap `sell`: (amount, min_out)
pub fn sell_data(tmpl: &[u8], tokens: u64, min_out: u64) -> Vec<u8> {
    let mut out = tmpl.to_vec();
    if out.len() < 24 {
        out.resize(24, 0);
    }
    out[8..16].copy_from_slice(&tokens.to_le_bytes());
    out[16..24].copy_from_slice(&min_out.to_le_bytes());
    out
}

/// Map a PumpSwap sell template from another pool onto ours (pool-specific accounts).
pub fn cross_coin_amm_map(t: &IxTemplate, pool: Pk, coin: &Coin) -> anyhow::Result<Vec<(Pk, Pk)>> {
    if t.pool.is_zero() || t.coin_creator.is_zero() {
        bail!("template lacks pool info");
    }
    let base_tp = if coin.token_program.is_zero() { *TOKEN_2022 } else { coin.token_program };
    // PumpSwap account 11 is base_token_program; both coins must use the same one.
    let their_tp = t.accounts.get(11).map(|a| a.pk).unwrap_or(base_tp);
    if their_tp != base_tp {
        bail!("template pool uses a different base token program");
    }
    let our_creator = if coin.amm_creator.is_zero() { coin.creator } else { coin.amm_creator };
    let cva = |c: &Pk| tx::pda(&[b"creator_vault", &c.0], &AMM);
    let (cva_t, cva_o) = (cva(&t.coin_creator), cva(&our_creator));
    Ok(vec![
        (t.pool, pool),
        (tx::ata(&t.pool, &t.mint, &their_tp), tx::ata(&pool, &coin.mint, &base_tp)),
        (tx::ata(&t.pool, &WSOL, &TOKEN), tx::ata(&pool, &WSOL, &TOKEN)),
        (cva_t, cva_o),
        (tx::ata(&cva_t, &WSOL, &TOKEN), tx::ata(&cva_o, &WSOL, &TOKEN)),
    ])
}
