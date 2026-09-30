//! Paper books: one per simulated landing delay (0, 1, 2 slots after the signal).
//! Every fill is priced against the coin's real curve/pool state at the landing slot.

use crate::coin::{Coin, Feat};
use crate::config::Config;
use crate::logger::Logger;
use crate::strategy::{check_exit, ExitAction, ExitInput, ExitKind, ExitState};
use crate::types::{lamports, r6, sol, sol_i, Pk};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PendingExit {
    pub act: ExitAction,
    pub signal_slot: u64,
    pub signal_ts: i64,
    pub target_slot: u64,
    pub waits: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Leg {
    pub reason: &'static str,
    pub slot: u64,
    pub ts: i64,
    pub tokens: u64,
    pub net_sol: f64,
    pub pct_at_signal: f64,
}

pub struct PaperPos {
    pub id: u64,
    pub trigger_id: u64,
    pub delay: u64,
    pub mint: Pk,
    pub signal_slot: u64,
    pub signal_ts: i64,
    pub signal_price: f64,
    pub entry_slot: u64,
    pub entry_ts: i64,
    pub entry_price: f64,
    pub entry_mcap: f64,
    pub cost: u64,
    pub tokens_orig: u64,
    pub tokens_left: u64,
    pub proceeds: i64,
    pub ex: ExitState,
    pub pending: Option<PendingExit>,
    pub legs: Vec<Leg>,
    pub feat: Arc<Feat>,
    pub last_pct: f64,
}

pub struct PendingEntry {
    pub id: u64,
    pub trigger_id: u64,
    pub book: usize,
    pub mint: Pk,
    pub signal_slot: u64,
    pub signal_ts: i64,
    pub signal_price: f64,
    pub target_slot: u64,
    pub feat: Arc<Feat>,
}

#[derive(Default, Clone, Serialize)]
pub struct ExitStat {
    pub n: u64,
    pub pnl_sol: f64,
}

#[derive(Default)]
pub struct BookStats {
    pub closed: u64,
    pub wins: u64,
    pub losses: u64,
    pub pnl_sol: f64,
    pub cost_sol: f64,
    pub sum_win_pct: f64,
    pub sum_loss_pct: f64,
    pub entry_failed: u64,
    pub skipped_full: u64,
    pub by_exit: HashMap<&'static str, ExitStat>,
}

pub struct Book {
    pub delay: u64,
    pub open: HashMap<Pk, PaperPos>,
    pub stats: BookStats,
}

pub struct Paper {
    pub books: Vec<Book>,
    pub entries: Vec<PendingEntry>,
    next_id: u64,
    start_mode: bool,
}

impl Paper {
    pub fn new(cfg: &Config) -> Paper {
        Paper {
            books: cfg
                .paper
                .delays
                .iter()
                .map(|&d| Book { delay: d, open: HashMap::new(), stats: BookStats::default() })
                .collect(),
            entries: vec![],
            next_id: 1,
            start_mode: cfg.paper.fill_at == "start",
        }
    }

    pub fn has_work(&self) -> bool {
        !self.entries.is_empty() || self.books.iter().any(|b| !b.open.is_empty())
    }

    pub fn holds(&self, mint: &Pk) -> bool {
        self.entries.iter().any(|e| e.mint == *mint) || self.books.iter().any(|b| b.open.contains_key(mint))
    }

    pub fn on_trigger(&mut self, cfg: &Config, log: &Logger, coin: &Coin, trigger_id: u64, feat: Arc<Feat>, cur_slot: u64, now_ms: i64) {
        for bi in 0..self.books.len() {
            let book = &self.books[bi];
            let pending_here = self.entries.iter().filter(|e| e.book == bi).count();
            if book.open.contains_key(&coin.mint) || self.entries.iter().any(|e| e.book == bi && e.mint == coin.mint) {
                continue;
            }
            if book.open.len() + pending_here >= cfg.paper.max_open {
                self.books[bi].stats.skipped_full += 1;
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            self.entries.push(PendingEntry {
                id,
                trigger_id,
                book: bi,
                mint: coin.mint,
                signal_slot: cur_slot,
                signal_ts: now_ms,
                signal_price: coin.price(),
                target_slot: cur_slot + book.delay,
                feat: feat.clone(),
            });
        }
        if self.start_mode {
            // delay 0 with "start" fills right after the signal trade.
            self.resolve_with(cfg, log, &|m| (*m == coin.mint).then_some(coin), Some(coin.mint), |t| t == cur_slot, now_ms, cur_slot);
        }
    }

    /// Fill everything whose target slot satisfies `ready`.
    pub fn resolve<'a>(&mut self, cfg: &Config, log: &Logger, coins: &'a HashMap<Pk, Coin>, ready: impl Fn(u64) -> bool, now_ms: i64, cur_slot: u64) {
        self.resolve_with(cfg, log, &|m| coins.get(m), None, ready, now_ms, cur_slot)
    }

    fn resolve_with<'a>(
        &mut self,
        cfg: &Config,
        log: &Logger,
        get: &dyn Fn(&Pk) -> Option<&'a Coin>,
        only: Option<Pk>,
        ready: impl Fn(u64) -> bool,
        now_ms: i64,
        cur_slot: u64,
    ) {
        let hit = |mint: &Pk, t: u64| ready(t) && only.is_none_or(|o| o == *mint);
        if self.entries.iter().any(|e| hit(&e.mint, e.target_slot)) {
            let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries).into_iter().partition(|e| hit(&e.mint, e.target_slot));
            self.entries = keep;
            for e in due {
                self.fill_entry(cfg, log, get(&e.mint), e, now_ms);
            }
        }
        for bi in 0..self.books.len() {
            let due: Vec<Pk> = self.books[bi]
                .open
                .iter()
                .filter(|(m, p)| p.pending.as_ref().is_some_and(|x| hit(m, x.target_slot)))
                .map(|(m, _)| *m)
                .collect();
            for m in due {
                if let Some(c) = get(&m) {
                    self.fill_exit(cfg, log, bi, c, now_ms, cur_slot);
                }
            }
        }
    }

    fn fill_entry(&mut self, cfg: &Config, log: &Logger, coin: Option<&Coin>, e: PendingEntry, now_ms: i64) {
        let book = &mut self.books[e.book];
        let Some(coin) = coin else {
            book.stats.entry_failed += 1;
            return;
        };
        let fail = |book: &mut Book, why: &str| {
            book.stats.entry_failed += 1;
            log.line(
                "fills.jsonl",
                &json!({"side":"buy_failed","strategy":cfg.mode.name(),"delay":book.delay,"mint":coin.mint,"reason":why,
                        "signal_slot":e.signal_slot,"target_slot":e.target_slot,"ts":now_ms}),
            );
        };
        if coin.complete || coin.migrated {
            return fail(book, "curve_complete");
        }
        let budget = lamports(cfg.paper.size_sol);
        let (tokens, sol_in, fees) = coin.buy_quote(budget);
        if tokens == 0 {
            return fail(book, "zero_tokens");
        }
        let tx_cost = lamports(cfg.paper.buy_cost_sol);
        let cost = sol_in + fees + tx_cost;
        let price = (sol_in + fees) as f64 / tokens as f64;
        let pos = PaperPos {
            id: e.id,
            trigger_id: e.trigger_id,
            delay: book.delay,
            mint: coin.mint,
            signal_slot: e.signal_slot,
            signal_ts: e.signal_ts,
            signal_price: e.signal_price,
            entry_slot: e.target_slot,
            entry_ts: now_ms,
            entry_price: price,
            entry_mcap: coin.mcap_sol(),
            cost,
            tokens_orig: tokens,
            tokens_left: tokens,
            proceeds: 0,
            ex: ExitState::default(),
            pending: None,
            legs: vec![],
            feat: e.feat,
            last_pct: 0.0,
        };
        log.line(
            "fills.jsonl",
            &json!({"side":"buy","strategy":cfg.mode.name(),"delay":book.delay,"pos":pos.id,"trigger":pos.trigger_id,
                "mint":coin.mint,"symbol":coin.symbol,"signal_slot":e.signal_slot,"fill_slot":e.target_slot,
                "last_trade_slot":coin.last_slot,"signal_ts":e.signal_ts,"ts":now_ms,"tokens":tokens,
                "sol_in":r6(sol(sol_in)),"fees":r6(sol(fees)),"tx_cost":r6(sol(tx_cost)),"cost":r6(sol(cost)),
                "price":price,"signal_price":e.signal_price,
                "slip_vs_signal":r6(if e.signal_price>0.0 { (sol_in as f64/tokens as f64)/e.signal_price-1.0 } else {0.0}),
                "mcap_sol":r6(coin.mcap_sol()),"fee_bps":coin.fee_bps,"cfee_bps":coin.cfee_bps}),
        );
        book.open.insert(coin.mint, pos);
    }

    /// Current P&L % of the remaining tokens vs. their cost basis (fees included).
    fn pct(p: &PaperPos, coin: &Coin) -> Option<f64> {
        if p.tokens_left == 0 || p.tokens_orig == 0 {
            return None;
        }
        let net = coin.sell_quote(p.tokens_left);
        if net == 0 && !coin.tradable() {
            return None;
        }
        let basis = p.cost as f64 * p.tokens_left as f64 / p.tokens_orig as f64;
        Some(net as f64 / basis - 1.0)
    }

    /// Run the exit policy for every position on this coin.
    pub fn evaluate(&mut self, cfg: &Config, log: &Logger, coin: &Coin, now_ms: i64, cur_slot: u64, kill: bool) {
        let mut immediate = false;
        for book in &mut self.books {
            let Some(p) = book.open.get_mut(&coin.mint) else { continue };
            if p.pending.is_some() || p.tokens_left == 0 {
                continue;
            }
            // Between curve completion and pool creation nothing trades: reuse the last mark.
            let pct = if coin.tradable() { Self::pct(p, coin).unwrap_or(p.last_pct) } else { p.last_pct };
            p.last_pct = pct;
            p.ex.observe(pct);
            let x = ExitInput {
                pct,
                age_s: (now_ms - p.entry_ts) as f64 / 1000.0,
                creator_sold_after_entry: coin.creator_sold && coin.creator_sold_ts >= p.signal_ts,
                migrated: coin.migrated,
                ms_since_migration: if coin.migrated { now_ms - coin.migrate_ts } else { 0 },
            };
            let act = if kill {
                Some(ExitAction { kind: ExitKind::KillSwitch, frac: None, emergency: true })
            } else {
                check_exit(cfg, &mut p.ex, &x)
            };
            if let Some(act) = act {
                p.pending = Some(PendingExit { act, signal_slot: cur_slot, signal_ts: now_ms, target_slot: cur_slot + book.delay, waits: 0 });
                if book.delay == 0 && self.start_mode {
                    immediate = true;
                }
            }
        }
        if immediate {
            self.resolve_with(cfg, log, &|m| (*m == coin.mint).then_some(coin), Some(coin.mint), |t| t == cur_slot, now_ms, cur_slot);
        }
    }

    fn fill_exit(&mut self, cfg: &Config, log: &Logger, bi: usize, coin: &Coin, now_ms: i64, cur_slot: u64) {
        let book = &mut self.books[bi];
        let Some(p) = book.open.get_mut(&coin.mint) else { return };
        let Some(pe) = p.pending.clone() else { return };
        if !coin.tradable() {
            // Curve complete but the PumpSwap pool is not live yet: retry next slot.
            let pe2 = p.pending.as_mut().unwrap();
            pe2.waits += 1;
            pe2.target_slot = cur_slot + 1;
            return;
        }
        let tokens = match pe.act.frac {
            None => p.tokens_left,
            Some(f) => ((p.tokens_orig as f64 * f).round() as u64).min(p.tokens_left),
        };
        let net = coin.sell_quote(tokens);
        let tx_cost = lamports(if pe.act.emergency { cfg.paper.emergency_sell_cost_sol } else { cfg.paper.sell_cost_sol });
        p.tokens_left -= tokens;
        p.proceeds += net as i64 - tx_cost as i64;
        p.pending = None;
        let leg = Leg {
            reason: pe.act.kind.name(),
            slot: pe.target_slot,
            ts: now_ms,
            tokens,
            net_sol: r6(sol(net)),
            pct_at_signal: r6(p.last_pct),
        };
        log.line(
            "fills.jsonl",
            &json!({"side":"sell","strategy":cfg.mode.name(),"delay":book.delay,"pos":p.id,"mint":coin.mint,"symbol":coin.symbol,
                "reason":leg.reason,"emergency":pe.act.emergency,"signal_slot":pe.signal_slot,"fill_slot":pe.target_slot,
                "last_trade_slot":coin.last_slot,"signal_ts":pe.signal_ts,"ts":now_ms,"tokens":tokens,"net_sol":leg.net_sol,
                "tx_cost":r6(sol(tx_cost)),"venue":if coin.migrated {"amm"} else {"curve"},"waits":pe.waits,
                "price":coin.price(),"mcap_sol":r6(coin.mcap_sol()),"pct_at_signal":leg.pct_at_signal}),
        );
        p.legs.push(leg);
        if p.tokens_left == 0 {
            let p = book.open.remove(&coin.mint).unwrap();
            let pnl = p.proceeds - p.cost as i64;
            let pnl_pct = pnl as f64 / p.cost as f64;
            let st = &mut book.stats;
            st.closed += 1;
            st.pnl_sol += sol_i(pnl);
            st.cost_sol += sol(p.cost);
            if pnl > 0 {
                st.wins += 1;
                st.sum_win_pct += pnl_pct;
            } else {
                st.losses += 1;
                st.sum_loss_pct += pnl_pct;
            }
            let fin = p.legs.last().map(|l| l.reason).unwrap_or("?");
            let e = st.by_exit.entry(fin).or_default();
            e.n += 1;
            e.pnl_sol += sol_i(pnl);
            log.line(
                "closes.jsonl",
                &json!({"strategy":cfg.mode.name(),"delay":p.delay,"pos":p.id,"trigger":p.trigger_id,"mint":coin.mint,
                    "symbol":coin.symbol,"name":coin.name,"signal_slot":p.signal_slot,"entry_slot":p.entry_slot,
                    "exit_slot":pe.target_slot,"entry_ts":p.entry_ts,"exit_ts":now_ms,"hold_s":r6((now_ms-p.entry_ts) as f64/1000.0),
                    "cost_sol":r6(sol(p.cost)),"proceeds_sol":r6(sol_i(p.proceeds)),"pnl_sol":r6(sol_i(pnl)),"pnl_pct":r6(pnl_pct),
                    "peak_pct":r6(p.ex.peak_pct),"low_pct":r6(p.ex.low_pct),"exit_reason":fin,
                    "exit_path":p.legs.iter().map(|l| l.reason).collect::<Vec<_>>(),"legs":p.legs,
                    "entry_mcap_sol":r6(p.entry_mcap),"exit_mcap_sol":r6(coin.mcap_sol()),
                    "entry_price":p.entry_price,"signal_price":p.signal_price,"graduated":coin.migrated,"features":&*p.feat}),
            );
        }
    }

    pub fn summary(&self) -> serde_json::Value {
        let books: Vec<_> = self
            .books
            .iter()
            .map(|b| {
                let s = &b.stats;
                let open_cost: f64 = b.open.values().map(|p| sol(p.cost)).sum();
                json!({
                    "delay": b.delay,
                    "open": b.open.len(),
                    "open_cost_sol": r6(open_cost),
                    "closed": s.closed,
                    "wins": s.wins,
                    "losses": s.losses,
                    "win_rate": r6(if s.closed > 0 { s.wins as f64 / s.closed as f64 } else { 0.0 }),
                    "avg_win_pct": r6(if s.wins > 0 { s.sum_win_pct / s.wins as f64 } else { 0.0 }),
                    "avg_loss_pct": r6(if s.losses > 0 { s.sum_loss_pct / s.losses as f64 } else { 0.0 }),
                    "pnl_sol": r6(s.pnl_sol),
                    "roi": r6(if s.cost_sol > 0.0 { s.pnl_sol / s.cost_sol } else { 0.0 }),
                    "entry_failed": s.entry_failed,
                    "skipped_full": s.skipped_full,
                    "by_exit": s.by_exit,
                })
            })
            .collect();
        json!(books)
    }
}
