//! Single-threaded core: applies events to coin state, runs the strategy, drives the
//! paper books (and live trading), and writes the logs. Live stream, backfill and
//! replay all feed the same `on_event`.

use crate::coin::{AmmState, Coin, Watch};
use crate::config::{Config, Mode};
use crate::live::{Live, LiveMsg};
use crate::logger::{Logger, Samples};
use crate::model::{Event, Tmpl};
use crate::paper::Paper;
use crate::pump::is_sol_quote;
use crate::strategy::{self, rule_bit, Gate};
use crate::types::{r6, Pk};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;

pub enum Msg {
    Tx { evs: Vec<Event>, tmpls: Vec<Tmpl>, recv: Instant, key: u64, src: u8, server_lag_ms: Option<i64> },
    Ev { ev: Event, recv: Instant, src: u8 },
    Stream { src: u8, up: bool, note: String },
    Live(LiveMsg),
    Tick,
}

/// Pools the gRPC tasks should subscribe to (graduated coins we track or hold).
pub struct PoolWatch {
    tx: watch::Sender<Vec<Pk>>,
}

impl PoolWatch {
    pub fn new() -> Arc<PoolWatch> {
        Arc::new(PoolWatch { tx: watch::channel(Vec::new()).0 })
    }
    pub fn set(&self, pools: HashSet<Pk>) {
        let mut v: Vec<Pk> = pools.into_iter().collect();
        v.sort();
        self.tx.send_if_modified(|cur| {
            if *cur != v {
                *cur = v;
                true
            } else {
                false
            }
        });
    }
    pub fn subscribe(&self) -> watch::Receiver<Vec<Pk>> {
        self.tx.subscribe()
    }
}

#[derive(Default, serde::Serialize)]
struct Counters {
    msgs: u64,
    dup_msgs: u64,
    events: u64,
    creates: u64,
    creates_non_sol: u64,
    creates_mayhem: u64,
    trades: u64,
    trades_tracked: u64,
    amm_trades_tracked: u64,
    migrations: u64,
    triggers: u64,
    near_misses: u64,
    coins_dropped_full: u64,
}

#[derive(Default, Clone, serde::Serialize)]
struct StreamInfo {
    up: bool,
    msgs: u64,
    first_wins: u64,
    reconnects: u64,
    last_note: String,
    last_msg_ms: i64,
}

pub struct Engine {
    pub cfg: Arc<Config>,
    pub log: Logger,
    pub coins: HashMap<Pk, Coin>,
    pool_mint: HashMap<Pk, Pk>,
    pub paper: Paper,
    pub live: Option<Live>,
    pub now: i64,
    pub cur_slot: u64,
    slot_first: HashMap<u64, i64>,
    slot_q: VecDeque<u64>,
    pub sol_usd: f64,
    dedup: HashSet<u64>,
    dedup_q: VecDeque<u64>,
    pub pool_watch: Option<Arc<PoolWatch>>,
    trig_seq: u64,
    counters: Counters,
    lat_decision_us: Samples,
    lat_into_slot_ms: Samples,
    lat_server_ms: Samples,
    rejects: HashMap<&'static str, u64>,
    near_by_rule: HashMap<&'static str, u64>,
    streams: Vec<StreamInfo>,
    started_ms: i64,
    last_summary: i64,
    last_gc: i64,
    last_tick: i64,
    pub replay: bool,
    keep_ms: i64,
    small_buy: u64,
}

impl Engine {
    pub fn new(cfg: Arc<Config>, log: Logger, live: Option<Live>, replay: bool) -> Engine {
        let (window_s, stop_s, small) = match cfg.mode {
            Mode::Powerful => (cfg.powerful.max_age_s, cfg.powerful.time_stop_s, 0.02),
            Mode::PowerBot => (cfg.powerbot.max_age_s, cfg.powerbot.time_stop_s, cfg.powerbot.small_buy_sol),
            Mode::Gymer => (cfg.gymer.max_age_s, cfg.gymer.time_stop_s, cfg.gymer.small_buy_sol),
        };
        let mut e = Engine {
            paper: Paper::new(&cfg),
            sol_usd: cfg.sol_price_usd,
            keep_ms: ((window_s + stop_s + 60.0) * 1000.0) as i64,
            small_buy: crate::types::lamports(small),
            cfg,
            log,
            coins: HashMap::with_capacity(4096),
            pool_mint: HashMap::new(),
            live,
            now: 0,
            cur_slot: 0,
            slot_first: HashMap::new(),
            slot_q: VecDeque::new(),
            dedup: HashSet::with_capacity(1 << 17),
            dedup_q: VecDeque::with_capacity(1 << 17),
            pool_watch: None,
            trig_seq: 0,
            counters: Counters::default(),
            lat_decision_us: Samples::default(),
            lat_into_slot_ms: Samples::default(),
            lat_server_ms: Samples::default(),
            rejects: HashMap::new(),
            near_by_rule: HashMap::new(),
            streams: vec![],
            started_ms: 0,
            last_summary: 0,
            last_gc: 0,
            last_tick: 0,
            replay,
        };
        // Restored live positions must be tracked even though we never saw their launch.
        if let Some(l) = &e.live {
            for (mint, ts, slot) in l.restored() {
                e.coins.entry(mint).or_insert_with(|| Coin::bare(mint, slot, ts));
            }
        }
        e
    }

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Tx { evs, tmpls, recv, key, src, server_lag_ms } => {
                self.counters.msgs += 1;
                self.stream(src).msgs += 1;
                if !self.dedup_insert(key) {
                    self.counters.dup_msgs += 1;
                    return;
                }
                self.stream(src).first_wins += 1;
                if let Some(l) = server_lag_ms {
                    self.lat_server_ms.push(l as f64);
                }
                for ev in &evs {
                    self.on_event(ev, Some(recv));
                }
                if let Some(live) = self.live.as_mut() {
                    for t in tmpls {
                        live.on_template(t, &mut self.coins);
                    }
                }
            }
            Msg::Ev { ev, recv, src } => {
                if src < 200 {
                    self.stream(src).msgs += 1;
                }
                let key = match &ev {
                    Event::Block { slot, .. } => Some(slot.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1),
                    Event::Slot { slot, .. } => Some(slot.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 2),
                    _ => None,
                };
                if let Some(k) = key {
                    if !self.dedup_insert(k) {
                        return;
                    }
                }
                self.on_event(&ev, Some(recv));
            }
            Msg::Stream { src, up, note } => {
                let now = crate::types::now_ms();
                let s = self.stream(src);
                if s.up && !up {
                    s.reconnects += 1;
                }
                s.up = up;
                s.last_note = note.clone();
                s.last_msg_ms = now;
                self.log.line("stream.jsonl", &json!({"ts": now, "src": src, "up": up, "note": note}));
            }
            Msg::Live(m) => {
                if let Some(live) = self.live.as_mut() {
                    live.on_msg(m, &self.coins, self.cur_slot, self.now);
                }
            }
            Msg::Tick => {
                let now = crate::types::now_ms().max(self.now);
                self.on_tick(now);
            }
        }
    }

    fn stream(&mut self, src: u8) -> &mut StreamInfo {
        let i = src as usize;
        if self.streams.len() <= i {
            self.streams.resize(i + 1, StreamInfo::default());
        }
        &mut self.streams[i]
    }

    fn dedup_insert(&mut self, key: u64) -> bool {
        if !self.dedup.insert(key) {
            return false;
        }
        self.dedup_q.push_back(key);
        if self.dedup_q.len() > 200_000 {
            if let Some(old) = self.dedup_q.pop_front() {
                self.dedup.remove(&old);
            }
        }
        true
    }

    fn resolve_paper(&mut self, ready: impl Fn(u64) -> bool) {
        if self.paper.has_work() {
            self.paper.resolve(&self.cfg, &self.log, &self.coins, ready, self.now, self.cur_slot);
        }
    }

    pub fn on_event(&mut self, ev: &Event, recv: Option<Instant>) {
        self.counters.events += 1;
        let ts = ev.ts();
        if ts > self.now {
            self.now = ts;
        }
        if self.started_ms == 0 {
            self.started_ms = self.now;
            self.last_summary = self.now;
            self.last_gc = self.now;
            self.last_tick = self.now;
        }
        if self.replay {
            // Replay drives the clock from event timestamps.
            while self.now - self.last_tick >= self.cfg.tick_ms as i64 {
                let t = self.last_tick + self.cfg.tick_ms as i64;
                self.on_tick(t);
            }
        }
        if let Some(s) = ev.slot() {
            if s > self.cur_slot {
                let start = self.cfg.paper.fill_at == "start";
                self.cur_slot = s;
                self.resolve_paper(|t| if start { t <= s } else { t < s });
            }
            if !matches!(ev, Event::Block { .. }) {
                self.note_slot(s, ts);
            }
        }
        match ev {
            Event::Create(c) => {
                self.counters.creates += 1;
                if !is_sol_quote(&c.quote) {
                    self.counters.creates_non_sol += 1;
                    return;
                }
                if c.mayhem {
                    self.counters.creates_mayhem += 1;
                }
                if self.coins.len() >= self.cfg.max_coins {
                    self.counters.coins_dropped_full += 1;
                    return;
                }
                self.coins.entry(c.mint).or_insert_with(|| Coin::from_create(c));
                self.record(ev);
            }
            Event::Trade(t) => {
                self.counters.trades += 1;
                let Some(coin) = self.coins.get_mut(&t.mint) else {
                    if self.cfg.record_all {
                        self.record(ev);
                    }
                    return;
                };
                self.counters.trades_tracked += 1;
                coin.apply_trade(t, self.small_buy);
                if coin.mayhem != t.mayhem && t.mayhem {
                    coin.mayhem = true;
                }
                self.record(ev);
                if let Some(live) = self.live.as_mut() {
                    live.on_market_event(ev, &self.coins, self.cur_slot, self.now);
                }
                self.after_update(t.mint, recv, t.slot);
            }
            Event::Amm(a) => {
                let mint = match self.pool_mint.get(&a.pool) {
                    Some(m) => *m,
                    None if self.coins.contains_key(&a.mint) => {
                        self.pool_mint.insert(a.pool, a.mint);
                        a.mint
                    }
                    None => return,
                };
                let Some(coin) = self.coins.get_mut(&mint) else { return };
                self.counters.amm_trades_tracked += 1;
                if !coin.migrated {
                    coin.migrated = true;
                    coin.migrate_ts = a.ts;
                    coin.migrate_slot = a.slot;
                    coin.pool = Some(a.pool);
                }
                coin.apply_amm(a);
                self.record(ev);
                if let Some(live) = self.live.as_mut() {
                    live.on_market_event(ev, &self.coins, self.cur_slot, self.now);
                }
                self.after_update(mint, recv, a.slot);
            }
            Event::Complete { mint, ts, .. } => {
                if let Some(coin) = self.coins.get_mut(mint) {
                    coin.complete = true;
                    coin.complete_ts = *ts;
                    self.record(ev);
                }
            }
            Event::Migrate(m) => {
                self.counters.migrations += 1;
                let Some(coin) = self.coins.get_mut(&m.mint) else { return };
                coin.complete = true;
                coin.migrated = true;
                coin.migrate_ts = m.ts;
                coin.migrate_slot = m.slot;
                coin.pool = Some(m.pool);
                if coin.amm.is_none() {
                    // Initial pool = tokens + SOL moved from the curve; default PumpSwap fees
                    // until the first swap event reports the real tier.
                    coin.amm = Some(AmmState { base: m.base, quote: m.quote, lp_bps: 20, proto_bps: 5, cc_bps: 5, slot: m.slot });
                }
                self.pool_mint.insert(m.pool, m.mint);
                self.record(ev);
                self.refresh_pool_watch();
                self.after_update(m.mint, recv, m.slot);
            }
            Event::Block { slot, hash, .. } => {
                if self.cfg.record {
                    self.log.record(ev);
                }
                if let Some(live) = self.live.as_mut() {
                    live.on_block(*slot, hash, self.now);
                }
                let s = *slot;
                self.resolve_paper(|t| t <= s);
            }
            Event::Slot { slot, ts } => {
                self.note_slot(*slot, *ts);
                if self.cfg.record {
                    self.log.record(ev);
                }
            }
            Event::Price { usd, .. } => {
                if *usd > 0.0 {
                    self.sol_usd = *usd;
                }
                self.record(ev);
            }
            Event::Own(o) => {
                self.record(ev);
                if let Some(live) = self.live.as_mut() {
                    live.on_own(o, &self.coins, self.cur_slot, self.now);
                }
            }
        }
    }

    fn record(&self, ev: &Event) {
        if self.cfg.record && !self.replay {
            self.log.record(ev);
        }
    }

    fn note_slot(&mut self, slot: u64, ts: i64) {
        if let std::collections::hash_map::Entry::Vacant(v) = self.slot_first.entry(slot) {
            v.insert(ts);
            self.slot_q.push_back(slot);
            if self.slot_q.len() > 4096 {
                if let Some(old) = self.slot_q.pop_front() {
                    self.slot_first.remove(&old);
                }
            }
        }
    }

    /// Strategy + exits after a coin changed.
    fn after_update(&mut self, mint: Pk, recv: Option<Instant>, slot: u64) {
        let now = self.now;
        let mut trigger = None;
        {
            let Some(coin) = self.coins.get_mut(&mint) else { return };
            if coin.watch == Watch::Watching && coin.seen_create {
                let ev = strategy::evaluate(&self.cfg, coin, now, self.sol_usd);
                match &ev.gate {
                    Gate::Wait => {}
                    Gate::Expired => {
                        coin.watch = Watch::Rejected;
                        *self.rejects.entry("expired").or_default() += 1;
                    }
                    Gate::Eval(_) => {
                        let fails = ev.fails();
                        if fails.is_empty() {
                            coin.watch = Watch::Triggered;
                            trigger = Some(ev.feat);
                        } else {
                            if fails.len() == 1 && ev.full && self.cfg.near_miss {
                                let bit = rule_bit(fails[0]);
                                if coin.near_miss_mask & bit == 0 {
                                    coin.near_miss_mask |= bit;
                                    self.counters.near_misses += 1;
                                    *self.near_by_rule.entry(fails[0]).or_default() += 1;
                                    self.log.line(
                                        "near_misses.jsonl",
                                        &json!({"strategy":self.cfg.mode.name(),"ts":now,"slot":slot,"mint":mint,
                                            "symbol":coin.symbol,"failed_rule":fails[0],"features":ev.feat}),
                                    );
                                }
                            }
                            if let Some(f) = ev.final_fail() {
                                coin.watch = Watch::Rejected;
                                *self.rejects.entry(f).or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
        if let Some(feat) = trigger {
            self.fire(mint, feat, recv, slot);
        }
        let coin = &self.coins[&mint];
        self.paper.evaluate(&self.cfg, &self.log, coin, now, self.cur_slot, false);
        if let Some(live) = self.live.as_mut() {
            live.evaluate(coin, now, self.cur_slot);
        }
    }

    fn fire(&mut self, mint: Pk, feat: crate::coin::Feat, recv: Option<Instant>, slot: u64) {
        let decision = Instant::now();
        let decision_us = recv.map(|r| decision.duration_since(r).as_micros() as i64);
        let into_slot = self.slot_first.get(&slot).map(|t0| self.now - t0);
        if let Some(d) = decision_us {
            self.lat_decision_us.push(d as f64);
        }
        if let Some(m) = into_slot {
            self.lat_into_slot_ms.push(m as f64);
        }
        self.trig_seq += 1;
        self.counters.triggers += 1;
        let id = self.trig_seq;
        let coin = &self.coins[&mint];
        self.log.line(
            "triggers.jsonl",
            &json!({"id":id,"strategy":self.cfg.mode.name(),"ts":self.now,"slot":slot,"mint":mint,"symbol":coin.symbol,
                "name":coin.name,"creator":coin.creator,"created_slot":coin.created_slot,"price":coin.price(),
                "decision_us":decision_us,"ms_into_slot":into_slot,"features":feat}),
        );
        let feat = Arc::new(feat);
        if let Some(live) = self.live.as_mut() {
            live.on_signal(coin, &feat, id, recv, decision, slot, self.now, into_slot);
        }
        self.paper.on_trigger(&self.cfg, &self.log, coin, id, feat, self.cur_slot, self.now);
    }

    pub fn on_tick(&mut self, now: i64) {
        self.last_tick = now;
        if now > self.now {
            self.now = now;
        }
        // Time-based exits (time stop, graduation delay) for every held coin.
        let held: Vec<Pk> = self.paper.books.iter().flat_map(|b| b.open.keys().copied()).collect::<HashSet<_>>().into_iter().collect();
        for m in held {
            if let Some(coin) = self.coins.get(&m) {
                self.paper.evaluate(&self.cfg, &self.log, coin, now, self.cur_slot, false);
            }
        }
        if let Some(live) = self.live.as_mut() {
            live.on_tick(&self.coins, now, self.cur_slot);
        }
        if now - self.last_gc >= 5_000 {
            self.last_gc = now;
            self.gc(now);
        }
        if now - self.last_summary >= self.cfg.summary_every_s as i64 * 1000 {
            self.last_summary = now;
            self.write_summary(false);
        }
    }

    fn gc(&mut self, now: i64) {
        let keep = self.keep_ms;
        let paper = &self.paper;
        let live = &self.live;
        let before = self.coins.len();
        self.coins.retain(|m, c| {
            now - c.created_ts < keep || paper.holds(m) || live.as_ref().is_some_and(|l| l.holds(m))
        });
        if self.coins.len() != before {
            let coins = &self.coins;
            self.pool_mint.retain(|_, m| coins.contains_key(m));
            self.refresh_pool_watch();
        }
    }

    fn refresh_pool_watch(&self) {
        if let Some(pw) = &self.pool_watch {
            let set: HashSet<Pk> = self.coins.values().filter_map(|c| c.pool).collect();
            pw.set(set);
        }
    }

    pub fn write_summary(&mut self, final_: bool) {
        let open_marks: Vec<_> = self
            .paper
            .books
            .iter()
            .flat_map(|b| {
                b.open.values().map(|p| {
                    json!({"delay":b.delay,"mint":p.mint,"entry_ts":p.entry_ts,"cost_sol":r6(crate::types::sol(p.cost)),
                        "last_pct":r6(p.last_pct),"peak_pct":r6(p.ex.peak_pct),"tokens_left":p.tokens_left})
                })
            })
            .collect();
        let s = json!({
            "ts": self.now,
            "final": final_,
            "mode": self.cfg.mode.name(),
            "uptime_s": (self.now - self.started_ms) / 1000,
            "cur_slot": self.cur_slot,
            "sol_usd": self.sol_usd,
            "coins_tracked": self.coins.len(),
            "pools_tracked": self.pool_mint.len(),
            "counters": &self.counters,
            "rejects": &self.rejects,
            "near_misses_by_rule": &self.near_by_rule,
            "streams": &self.streams,
            "latency": {
                "grpc_recv_to_decision_us": self.lat_decision_us.pct(),
                "signal_ms_into_slot": self.lat_into_slot_ms.pct(),
                "server_to_client_ms": self.lat_server_ms.pct(),
            },
            "paper": self.paper.summary(),
            "paper_open": open_marks,
            "live": self.live.as_ref().map(|l| l.summary()),
            "config": &*self.cfg,
        });
        self.log.replace("summary.json", serde_json::to_string_pretty(&s).unwrap_or_default());
        self.log.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CreateEv, MigrateEv, TradeEv};
    use crate::types::Sig;

    const VTOK: u64 = 900_000_000_000_000;

    struct Sim {
        e: Engine,
        mint: Pk,
        creator: Pk,
        slot: u64,
        ts: i64,
        n: u64,
    }

    fn pk(i: u64) -> Pk {
        let mut a = [0u8; 32];
        a[..8].copy_from_slice(&i.to_le_bytes());
        a[31] = 7;
        Pk(a)
    }

    impl Sim {
        fn new(cfg: Config) -> Sim {
            let dir = std::env::temp_dir().join(format!("hsnipe-test-{}", rand::random::<u64>()));
            let (log, _h) = Logger::start(dir.to_str().unwrap(), None).unwrap();
            let e = Engine::new(Arc::new(cfg), log, None, true);
            let mut s = Sim { e, mint: pk(999), creator: pk(1), slot: 1000, ts: 1_700_000_000_000, n: 0 };
            let c = CreateEv {
                slot: s.slot,
                idx: 0,
                ts: s.ts,
                sig: Sig::default(),
                mint: s.mint,
                creator: s.creator,
                user: s.creator,
                bonding_curve: Pk::ZERO,
                name: "T".into(),
                symbol: "T".into(),
                mayhem: false,
                quote: Pk::ZERO,
                token_program: *crate::pump::TOKEN_2022,
                vsol: 30_000_000_000,
                vtok: 1_073_000_000_000_000,
                rtok: 793_100_000_000_000,
                supply: 1_000_000_000_000_000,
                creator_fee_bps: 0,
                holder_reward: false,
            };
            s.e.on_event(&Event::Create(c), None);
            s
        }
        /// Advance time/slot and trade at price = vsol/VTOK.
        fn trade(&mut self, dt_ms: i64, user: Pk, buy: bool, sol: u64, vsol: u64) {
            self.ts += dt_ms;
            self.slot += (dt_ms / 400).max(1) as u64;
            self.n += 1;
            let t = TradeEv {
                slot: self.slot,
                idx: 0,
                ts: self.ts,
                sig: Sig::default(),
                mint: self.mint,
                user,
                buy,
                sol,
                tok: sol * 10_000,
                vsol,
                vtok: VTOK,
                rsol: vsol - 30_000_000_000,
                rtok: 400_000_000_000_000,
                fee_bps: 95,
                cfee_bps: 30,
                creator: self.creator,
                mayhem: false,
                quote: Pk::ZERO,
                cu_price: 1000 + self.n,
                cu_limit: 100_000,
                prio: 0,
                tip: 0,
                post_sol: 5_000_000_000,
                payer: user,
                ix: 1,
            };
            self.e.on_event(&Event::Trade(t), None);
        }
        fn block(&mut self) {
            self.slot += 1;
            let s = self.slot;
            self.e.on_event(&Event::Block { slot: s, ts: self.ts, hash: "11111111111111111111111111111111".into(), height: None, block_time: None }, None);
        }
        fn exits(&self) -> Vec<(u64, Vec<&'static str>)> {
            self.e.paper.books.iter().map(|b| (b.stats.closed, b.stats.by_exit.keys().copied().collect())).collect()
        }
    }

    fn cfg() -> Config {
        let mut c = Config::load().unwrap();
        c.mode = Mode::Powerful;
        c.paper.delays = vec![0, 1];
        c.powerful.min_volume_usd = 0.0;
        c.powerful.bundle.enabled = false;
        c
    }

    /// 30 distinct buyers in minute 1, then a trade at 61 s triggers the entry
    /// (trades in the 2nd half of life stay >= 0.5x the 1st half, so no stall).
    fn launch(s: &mut Sim) {
        for i in 0..30 {
            s.trade(1500, pk(100 + i), true, 500_000_000, 60_000_000_000);
        }
        s.trade(16_000, pk(500), true, 100_000_000, 60_000_000_000);
        s.block();
        assert_eq!(s.e.paper.books.iter().map(|b| b.open.len()).sum::<usize>(), 2, "both books should hold");
    }

    #[test]
    fn powerful_tp1_tp2_trail() {
        let mut s = Sim::new(cfg());
        launch(&mut s);
        s.trade(1000, pk(600), true, 1_000_000_000, 125_000_000_000); // ~2.08x -> TP1
        s.block();
        s.trade(1000, pk(601), true, 1_000_000_000, 190_000_000_000); // ~3.2x -> TP2
        s.block();
        s.trade(1000, pk(602), true, 1_000_000_000, 200_000_000_000); // peak
        s.block();
        s.trade(1000, pk(603), false, 1_000_000_000, 130_000_000_000); // -35% from peak -> trail
        s.block();
        s.block();
        for (closed, exits) in s.exits() {
            assert_eq!(closed, 1);
            assert_eq!(exits, vec!["trail"]);
        }
        let b = &s.e.paper.books[0].stats;
        assert!(b.pnl_sol > 0.1, "pnl {}", b.pnl_sol);
    }

    #[test]
    fn powerful_creator_sold_is_emergency_exit() {
        let mut s = Sim::new(cfg());
        launch(&mut s);
        let creator = s.creator;
        s.trade(2000, creator, false, 10_000_000, 59_000_000_000);
        s.block();
        s.block();
        for (closed, exits) in s.exits() {
            assert_eq!((closed, exits), (1, vec!["creator_sold"]));
        }
    }

    #[test]
    fn powerful_hard_stop_and_time_stop() {
        let mut s = Sim::new(cfg());
        launch(&mut s);
        s.trade(2000, pk(700), false, 1_000_000_000, 35_000_000_000); // -42%
        s.block();
        s.block();
        for (closed, exits) in s.exits() {
            assert_eq!((closed, exits), (1, vec!["hard_stop"]));
        }

        let mut s = Sim::new(cfg());
        launch(&mut s);
        // flat for 15 minutes: time stop fires from the tick clock
        for _ in 0..91 {
            s.trade(10_000, pk(701), true, 1_000, 60_000_000_000);
            s.block();
        }
        for (closed, exits) in s.exits() {
            assert_eq!((closed, exits), (1, vec!["time_stop"]));
        }
    }

    #[test]
    fn powerful_graduation_sells_on_pumpswap() {
        let mut s = Sim::new(cfg());
        launch(&mut s);
        s.slot += 5;
        s.ts += 2000;
        // Pool opens at the curve's last price (60 SOL / 900M tokens), so no take-profit fires first.
        let m = MigrateEv { slot: s.slot, idx: 0, ts: s.ts, mint: s.mint, pool: pk(4242), base: 206_900_000_000_000, quote: 13_793_333_333 };
        s.e.on_event(&Event::Migrate(m), None);
        s.block();
        s.block();
        for (closed, exits) in s.exits() {
            assert_eq!((closed, exits), (1, vec!["graduation"]));
        }
    }

    #[test]
    fn creator_selling_before_entry_rejects_coin() {
        let mut s = Sim::new(cfg());
        let creator = s.creator;
        s.trade(1000, creator, false, 1_000_000, 30_000_000_000);
        for i in 0..30 {
            s.trade(1500, pk(100 + i), true, 500_000_000, 60_000_000_000);
        }
        s.trade(16_000, pk(500), true, 100_000_000, 60_000_000_000);
        s.block();
        assert_eq!(s.e.paper.books.iter().map(|b| b.open.len()).sum::<usize>(), 0);
        assert_eq!(s.e.rejects.get("creator_sold"), Some(&1));
    }
}
