//! Per-coin state built from the event stream, and the entry features computed from it.

use crate::config::BundleCfg;
use crate::model::{AmmEv, CreateEv, Tmpl, TradeEv};
use crate::pump::{self, CURVE_SUPPLY, TOTAL_SUPPLY};
use crate::types::{lamports, r6, sol, Pk};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone)]
pub struct Wallet {
    pub bought: u64,
    pub sold: u64,
    pub tokens: u64,
    pub first_buy_ts: i64,
    pub has_sold: bool,
    pub m1: bool,
    pub sniper: bool,
}

#[derive(Clone, Copy)]
pub struct BuyRec {
    pub slot: u64,
    pub idx: u32,
    pub ts_off: u32,
    pub user: Pk,
    pub cu_price: u64,
    pub cu_limit: u32,
    pub prio: u64,
    pub post_sol: u64,
}

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct AmmState {
    pub base: u64,
    pub quote: u64,
    pub lp_bps: u64,
    pub proto_bps: u64,
    pub cc_bps: u64,
    pub slot: u64,
}

/// Per-strategy watch state for this coin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Watch {
    Watching,
    Triggered,
    Rejected,
}

pub struct Coin {
    pub mint: Pk,
    pub creator: Pk,
    pub name: String,
    pub symbol: String,
    pub created_slot: u64,
    pub created_ts: i64,
    pub mayhem: bool,
    pub token_program: Pk,
    pub bonding_curve: Pk,
    pub supply: u64,
    /// true when we saw the launch; false for coins tracked only because of a restored position
    pub seen_create: bool,

    pub vsol: u64,
    pub vtok: u64,
    pub rsol: u64,
    pub rtok: u64,
    pub fee_bps: u64,
    pub cfee_bps: u64,
    pub last_slot: u64,
    pub last_ts: i64,

    pub complete: bool,
    pub complete_ts: i64,
    pub migrated: bool,
    pub migrate_ts: i64,
    pub migrate_slot: u64,
    pub pool: Option<Pk>,
    pub amm: Option<AmmState>,
    /// PumpSwap pool's coin_creator (from swap events)
    pub amm_creator: Pk,

    pub wallets: HashMap<Pk, Wallet>,
    pub n_buyers: u32,
    pub n_sellers: u32,
    pub n_holders: u32,
    pub prev_holders: u32,
    pub m1_buyers: u32,
    pub m1_sold: u32,
    pub buy_sol: u64,
    pub sell_sol: u64,
    pub n_buys: u32,
    pub n_sells: u32,
    pub n_small_buys: u32,
    pub creation_slot_buys: u32,
    /// ms offsets from creation of every trade (sorted, appended)
    pub trade_offs: Vec<u32>,
    pub creator_sold: bool,
    pub creator_sold_ts: i64,
    pub buys: Vec<BuyRec>,

    pub last_buy_tmpl: Option<Tmpl>,
    pub last_sell_tmpl: Option<Tmpl>,
    pub last_amm_sell_tmpl: Option<Tmpl>,

    pub watch: Watch,
    pub near_miss_mask: u64,
}

impl Coin {
    pub fn from_create(c: &CreateEv) -> Coin {
        let mut coin = Coin::bare(c.mint, c.slot, c.ts);
        coin.creator = c.creator;
        coin.name = c.name.clone();
        coin.symbol = c.symbol.clone();
        coin.mayhem = c.mayhem;
        coin.token_program = c.token_program;
        coin.bonding_curve = c.bonding_curve;
        coin.supply = if c.supply > 0 { c.supply } else { TOTAL_SUPPLY };
        coin.vsol = c.vsol;
        coin.vtok = c.vtok;
        coin.rtok = c.rtok;
        coin.seen_create = true;
        coin
    }

    pub fn bare(mint: Pk, slot: u64, ts: i64) -> Coin {
        Coin {
            mint,
            creator: Pk::ZERO,
            name: String::new(),
            symbol: String::new(),
            created_slot: slot,
            created_ts: ts,
            mayhem: false,
            token_program: Pk::ZERO,
            bonding_curve: Pk::ZERO,
            supply: TOTAL_SUPPLY,
            seen_create: false,
            vsol: 0,
            vtok: 0,
            rsol: 0,
            rtok: 0,
            fee_bps: 95,
            cfee_bps: 30,
            last_slot: slot,
            last_ts: ts,
            complete: false,
            complete_ts: 0,
            migrated: false,
            migrate_ts: 0,
            migrate_slot: 0,
            pool: None,
            amm: None,
            amm_creator: Pk::ZERO,
            wallets: HashMap::with_capacity(64),
            n_buyers: 0,
            n_sellers: 0,
            n_holders: 0,
            prev_holders: 0,
            m1_buyers: 0,
            m1_sold: 0,
            buy_sol: 0,
            sell_sol: 0,
            n_buys: 0,
            n_sells: 0,
            n_small_buys: 0,
            creation_slot_buys: 0,
            trade_offs: Vec::with_capacity(256),
            creator_sold: false,
            creator_sold_ts: 0,
            buys: Vec::with_capacity(128),
            last_buy_tmpl: None,
            last_sell_tmpl: None,
            last_amm_sell_tmpl: None,
            watch: Watch::Watching,
            near_miss_mask: 0,
        }
    }

    pub fn age_s(&self, now_ms: i64) -> f64 {
        (now_ms - self.created_ts) as f64 / 1000.0
    }

    pub fn apply_trade(&mut self, t: &TradeEv, small_buy_lamports: u64) {
        self.prev_holders = self.n_holders;
        self.vsol = t.vsol;
        self.vtok = t.vtok;
        self.rsol = t.rsol;
        self.rtok = t.rtok;
        self.fee_bps = t.fee_bps as u64;
        self.cfee_bps = t.cfee_bps as u64;
        self.last_slot = t.slot;
        self.last_ts = t.ts;
        if self.creator.is_zero() {
            self.creator = t.creator;
        }
        if t.rtok == 0 && !self.complete {
            self.complete = true;
            self.complete_ts = t.ts;
        }
        let off = (t.ts - self.created_ts).clamp(0, u32::MAX as i64) as u32;
        // Out-of-order arrivals are rare; keep the vector sorted cheaply.
        if self.trade_offs.last().is_some_and(|&l| l > off) {
            let pos = self.trade_offs.partition_point(|&x| x <= off);
            self.trade_offs.insert(pos, off);
        } else {
            self.trade_offs.push(off);
        }
        let in_m1 = off <= 60_000;
        let creation_slot = t.slot == self.created_slot;
        let is_creator = t.user == self.creator;
        let w = self.wallets.entry(t.user).or_default();
        let had_tokens = w.tokens > 0;
        if t.buy {
            if w.bought == 0 {
                self.n_buyers += 1;
                w.first_buy_ts = t.ts;
                if in_m1 {
                    w.m1 = true;
                    self.m1_buyers += 1;
                    if w.has_sold {
                        self.m1_sold += 1;
                    }
                }
                if creation_slot && !is_creator {
                    w.sniper = true;
                }
            }
            w.bought += t.sol;
            w.tokens = w.tokens.saturating_add(t.tok);
            self.buy_sol += t.sol;
            self.n_buys += 1;
            if t.sol < small_buy_lamports {
                self.n_small_buys += 1;
            }
            if creation_slot {
                self.creation_slot_buys += 1;
            }
            if self.buys.len() < 20_000 {
                self.buys.push(BuyRec {
                    slot: t.slot,
                    idx: t.idx,
                    ts_off: off,
                    user: t.user,
                    cu_price: t.cu_price,
                    cu_limit: t.cu_limit,
                    prio: t.prio,
                    post_sol: t.post_sol,
                });
            }
        } else {
            if !w.has_sold {
                w.has_sold = true;
                self.n_sellers += 1;
                if w.m1 {
                    self.m1_sold += 1;
                }
            }
            w.sold += t.sol;
            w.tokens = w.tokens.saturating_sub(t.tok);
            self.sell_sol += t.sol;
            self.n_sells += 1;
            if is_creator && !self.creator_sold {
                self.creator_sold = true;
                self.creator_sold_ts = t.ts;
            }
        }
        let has_tokens = w.tokens > 0;
        if has_tokens && !had_tokens {
            self.n_holders += 1;
        } else if !has_tokens && had_tokens {
            self.n_holders = self.n_holders.saturating_sub(1);
        }
    }

    pub fn apply_amm(&mut self, a: &AmmEv) {
        self.amm = Some(AmmState {
            base: a.pb,
            quote: a.pq,
            lp_bps: a.lp_bps as u64,
            proto_bps: a.proto_bps as u64,
            cc_bps: a.cc_bps as u64,
            slot: a.slot,
        });
        self.last_slot = a.slot;
        self.last_ts = a.ts;
        if !a.coin_creator.is_zero() {
            self.amm_creator = a.coin_creator;
        }
        if a.user == self.creator && !a.buy && !self.creator_sold {
            self.creator_sold = true;
            self.creator_sold_ts = a.ts;
        }
        let w = self.wallets.entry(a.user).or_default();
        if a.buy {
            w.tokens = w.tokens.saturating_add(a.base);
        } else {
            w.tokens = w.tokens.saturating_sub(a.base);
        }
    }

    /// Can we trade right now, and on which venue?
    pub fn tradable(&self) -> bool {
        if self.migrated {
            self.amm.is_some()
        } else {
            !self.complete && self.vtok > 0
        }
    }

    pub fn price(&self) -> f64 {
        if self.migrated {
            if let Some(a) = self.amm {
                if a.base > 0 {
                    return a.quote as f64 / a.base as f64;
                }
            }
        }
        if self.vtok == 0 {
            0.0
        } else {
            self.vsol as f64 / self.vtok as f64
        }
    }

    pub fn mcap_sol(&self) -> f64 {
        self.price() * self.supply as f64 / 1e9
    }

    /// Net SOL we would receive selling `tokens` right now (fees included, tx cost not).
    pub fn sell_quote(&self, tokens: u64) -> u64 {
        if self.migrated {
            if let Some(a) = self.amm {
                return pump::amm_sell(a.base, a.quote, tokens, a.lp_bps, a.proto_bps, a.cc_bps).0;
            }
            return 0;
        }
        pump::curve_sell(self.vsol, self.vtok, tokens, self.fee_bps, self.cfee_bps).0
    }

    /// (tokens, sol_in_curve, fees) for a buy with total budget in lamports.
    pub fn buy_quote(&self, budget: u64) -> (u64, u64, u64) {
        pump::curve_buy(self.vsol, self.vtok, self.rtok, budget, self.fee_bps, self.cfee_bps)
    }

    /// `full` adds the O(wallets) holder scan and the bundle heuristics.
    pub fn features(&self, now_ms: i64, sol_usd: f64, bundle: &BundleCfg, full: bool) -> Feat {
        let age_s = self.age_s(now_ms);
        let life_ms = (now_ms - self.created_ts).max(0) as u32;
        let half = life_ms / 2;
        let h1 = self.trade_offs.partition_point(|&x| x < half) as u32;
        let h2 = self.trade_offs.len() as u32 - h1;
        let vol_sol = sol(self.buy_sol + self.sell_sol);
        let mut f = Feat {
            age_s: r6(age_s),
            slot: self.last_slot,
            mcap_sol: r6(self.mcap_sol()),
            vsol: r6(sol(self.vsol)),
            buyers: self.n_buyers,
            sellers: self.n_sellers,
            holders: self.n_holders,
            m1_buyers: self.m1_buyers,
            m1_sold: self.m1_sold,
            m1_sold_frac: r6(if self.m1_buyers > 0 { self.m1_sold as f64 / self.m1_buyers as f64 } else { 0.0 }),
            buys: self.n_buys,
            sells: self.n_sells,
            buy_sol: r6(sol(self.buy_sol)),
            sell_sol: r6(sol(self.sell_sol)),
            vol_usd: r6(vol_sol * sol_usd),
            trades_h1: h1,
            trades_h2: h2,
            stall_ratio: r6(if h1 > 0 { h2 as f64 / h1 as f64 } else if h2 > 0 { 9.99 } else { 0.0 }),
            small_buy_share: r6(if self.n_buys > 0 { self.n_small_buys as f64 / self.n_buys as f64 } else { 0.0 }),
            creation_slot_buys: self.creation_slot_buys,
            creator_sold: self.creator_sold,
            mayhem: self.mayhem,
            sol_usd: r6(sol_usd),
            full,
            ..Default::default()
        };
        if !full {
            return f;
        }

        let mut top_buy = [0u64; 2];
        let mut holdings: Vec<u64> = Vec::with_capacity(self.wallets.len());
        let mut max_wallet = 0u64;
        let mut sniper_tokens = 0u64;
        for w in self.wallets.values() {
            if w.bought > top_buy[0] {
                top_buy[1] = top_buy[0];
                top_buy[0] = w.bought;
            } else if w.bought > top_buy[1] {
                top_buy[1] = w.bought;
            }
            if w.tokens > 0 {
                holdings.push(w.tokens);
                max_wallet = max_wallet.max(w.tokens);
                if w.sniper {
                    sniper_tokens += w.tokens;
                }
            }
        }
        let top10: u64 = if holdings.len() > 10 {
            let n = holdings.len();
            holdings.select_nth_unstable(n - 10);
            holdings[n - 10..].iter().sum()
        } else {
            holdings.iter().sum()
        };
        let supply = self.supply.max(1) as f64;
        let dev_tokens = self.wallets.get(&self.creator).map(|w| w.tokens).unwrap_or(0);

        let (fee_share, fee_n, groups, drained) = self.bundle_features(bundle);
        f.top2_buy_share = r6(if self.buy_sol > 0 { (top_buy[0] + top_buy[1]) as f64 / self.buy_sol as f64 } else { 0.0 });
        f.top10_share = r6(top10 as f64 / supply);
        f.dev_pct = r6(dev_tokens as f64 / supply);
        f.sniper_pct = r6(sniper_tokens as f64 / supply);
        f.max_wallet_curve_share = r6(max_wallet as f64 / CURVE_SUPPLY as f64);
        f.fee_cluster_share = r6(fee_share);
        f.fee_cluster_n = fee_n;
        f.bundle_groups = groups;
        f.drained_share = r6(drained);
        f
    }

    /// Bundle heuristics over buyers whose first buy was inside the window:
    /// (largest identical-fee cluster share, its size, adjacent same-slot groups, drained share).
    fn bundle_features(&self, b: &BundleCfg) -> (f64, u32, u32, f64) {
        let win = (b.window_s * 1000.0) as u32;
        let drained_l = lamports(b.drained_sol);
        let mut seen: HashSet<Pk> = HashSet::with_capacity(64);
        let mut clusters: HashMap<(u32, u64, u64), u32> = HashMap::with_capacity(32);
        let mut n = 0u32;
        let mut drained = 0u32;
        for r in &self.buys {
            if r.ts_off > win {
                break;
            }
            if r.user == self.creator || !seen.insert(r.user) {
                continue;
            }
            n += 1;
            if r.cu_price > 0 || r.prio > 0 {
                *clusters.entry((r.cu_limit, r.cu_price, r.prio)).or_default() += 1;
            }
            if r.post_sol > 0 && r.post_sol < drained_l {
                drained += 1;
            }
        }
        let fee_n = clusters.values().copied().max().unwrap_or(0);
        let fee_share = if n > 0 { fee_n as f64 / n as f64 } else { 0.0 };
        let drained_share = if n > 0 { drained as f64 / n as f64 } else { 0.0 };

        // Adjacent-index runs of distinct buyers inside one slot (Jito bundles land contiguously).
        let mut groups = 0u32;
        let mut by_slot: Vec<(u64, u32, Pk)> =
            self.buys.iter().filter(|r| r.ts_off <= win).map(|r| (r.slot, r.idx, r.user)).collect();
        by_slot.sort_unstable_by_key(|x| (x.0, x.1));
        let mut i = 0;
        while i < by_slot.len() {
            let mut j = i + 1;
            while j < by_slot.len()
                && by_slot[j].0 == by_slot[i].0
                && by_slot[j].1 <= by_slot[j - 1].1 + 1
                && by_slot[j].2 != by_slot[j - 1].2
            {
                j += 1;
            }
            if (j - i) as u32 >= b.group_size.max(2) {
                groups += 1;
            }
            i = j;
        }
        (fee_share, fee_n, groups, drained_share)
    }
}

/// Snapshot of everything the rules look at, logged with every trigger / near miss / close.
#[derive(Serialize, Clone, Default, Debug)]
pub struct Feat {
    pub age_s: f64,
    pub slot: u64,
    pub mcap_sol: f64,
    pub vsol: f64,
    pub buyers: u32,
    pub sellers: u32,
    pub holders: u32,
    pub m1_buyers: u32,
    pub m1_sold: u32,
    pub m1_sold_frac: f64,
    pub buys: u32,
    pub sells: u32,
    pub buy_sol: f64,
    pub sell_sol: f64,
    pub vol_usd: f64,
    pub trades_h1: u32,
    pub trades_h2: u32,
    pub stall_ratio: f64,
    pub top2_buy_share: f64,
    pub top10_share: f64,
    pub dev_pct: f64,
    pub sniper_pct: f64,
    pub max_wallet_curve_share: f64,
    pub small_buy_share: f64,
    pub creation_slot_buys: u32,
    pub creator_sold: bool,
    pub mayhem: bool,
    pub fee_cluster_share: f64,
    pub fee_cluster_n: u32,
    pub bundle_groups: u32,
    pub drained_share: f64,
    pub sol_usd: f64,
    /// false when only the cheap features were computed
    pub full: bool,
}
