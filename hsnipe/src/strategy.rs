//! Entry rules for the three strategies and the shared exit policy.

use crate::coin::{Coin, Feat};
use crate::config::{BundleCfg, Config, HolderCfg, Mode, PowerfulCfg};

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub name: &'static str,
    pub pass: bool,
    /// A failure is permanent: stop watching the coin.
    pub fin: bool,
}

pub enum Gate {
    /// Not evaluated yet (too young / not enough holders / no crossing).
    Wait,
    /// Past the evaluation window without qualifying.
    Expired,
    Eval(Vec<Rule>),
}

pub struct Evaluation {
    pub gate: Gate,
    pub feat: Feat,
    /// Every rule was evaluated (only then is a single failure a "near miss").
    pub full: bool,
}

impl Evaluation {
    fn new(gate: Gate, feat: Feat, full: bool) -> Self {
        Evaluation { gate, feat, full }
    }
    pub fn fails(&self) -> Vec<&'static str> {
        match &self.gate {
            Gate::Eval(r) => r.iter().filter(|r| !r.pass).map(|r| r.name).collect(),
            _ => vec![],
        }
    }
    pub fn final_fail(&self) -> Option<&'static str> {
        match &self.gate {
            Gate::Eval(r) => r.iter().find(|r| !r.pass && r.fin).map(|r| r.name),
            _ => None,
        }
    }
}

/// Stable bit per rule name for "near miss logged once per coin per rule".
pub fn rule_bit(name: &str) -> u64 {
    const NAMES: [&str; 24] = [
        "mayhem", "whale90", "creator_sold", "m1_buyers", "volume", "m1_sold", "stall", "net_buy", "top2",
        "bundle_fee", "bundle_groups", "bundle_drained", "entry_mcap", "top10_band", "small_buys",
        "creation_slot_buys", "snipers", "dev_pct", "min_volume", "curve_complete", "", "", "", "",
    ];
    NAMES.iter().position(|n| *n == name).map(|i| 1u64 << i).unwrap_or(1u64 << 63)
}

fn bundle_rules(b: &BundleCfg, f: &Feat, out: &mut Vec<Rule>) {
    if !b.enabled {
        return;
    }
    out.push(Rule {
        name: "bundle_fee",
        pass: !(f.fee_cluster_share >= b.max_fee_cluster_share && f.fee_cluster_n >= b.min_fee_cluster_n),
        fin: false,
    });
    out.push(Rule { name: "bundle_groups", pass: f.bundle_groups < b.max_groups, fin: false });
    out.push(Rule { name: "bundle_drained", pass: f.drained_share < b.max_drained_share, fin: false });
}

pub fn evaluate(cfg: &Config, coin: &Coin, now_ms: i64, sol_usd: f64) -> Evaluation {
    match cfg.mode {
        Mode::Powerful => eval_powerful(&cfg.powerful, coin, now_ms, sol_usd),
        Mode::PowerBot => eval_holder(&cfg.powerbot, coin, now_ms, sol_usd),
        Mode::Gymer => eval_holder(&cfg.gymer, coin, now_ms, sol_usd),
    }
}

fn eval_powerful(c: &PowerfulCfg, coin: &Coin, now_ms: i64, sol_usd: f64) -> Evaluation {
    let age = coin.age_s(now_ms);
    // FINAL rules apply from launch; everything else only between min_age and max_age.
    let early = [Rule { name: "mayhem", pass: !coin.mayhem, fin: true }, Rule { name: "creator_sold", pass: !coin.creator_sold, fin: true }];
    if age < c.min_age_s {
        let feat = coin.features(now_ms, sol_usd, &c.bundle, false);
        if early.iter().any(|r| !r.pass) {
            return Evaluation::new(Gate::Eval(early.to_vec()), feat, false);
        }
        return Evaluation::new(Gate::Wait, feat, false);
    }
    if age > c.max_age_s {
        return Evaluation::new(Gate::Expired, coin.features(now_ms, sol_usd, &c.bundle, false), false);
    }
    // Cheap rules first; the wallet/bundle scans only run when a pass or near miss is possible.
    let feat = coin.features(now_ms, sol_usd, &c.bundle, false);
    let mut r = early.to_vec();
    r.push(Rule { name: "m1_buyers", pass: feat.m1_buyers >= c.min_m1_buyers, fin: true });
    r.push(Rule { name: "volume", pass: feat.vol_usd >= c.min_volume_usd, fin: false });
    r.push(Rule { name: "m1_sold", pass: feat.m1_sold_frac < c.max_m1_sold_frac, fin: false });
    r.push(Rule {
        name: "stall",
        pass: feat.trades_h2 as f64 >= c.min_stall_ratio * feat.trades_h1 as f64,
        fin: false,
    });
    r.push(Rule { name: "net_buy", pass: coin.buy_sol > coin.sell_sol, fin: false });
    r.push(Rule { name: "curve_complete", pass: !coin.complete && !coin.migrated, fin: true });
    if r.iter().filter(|x| !x.pass).count() >= 2 {
        return Evaluation::new(Gate::Eval(r), feat, false);
    }
    let feat = coin.features(now_ms, sol_usd, &c.bundle, true);
    r.push(Rule { name: "whale90", pass: feat.max_wallet_curve_share < c.max_wallet_curve_share, fin: true });
    r.push(Rule { name: "top2", pass: feat.top2_buy_share < c.max_top2_share, fin: false });
    bundle_rules(&c.bundle, &feat, &mut r);
    if c.max_entry_mcap_sol > 0.0 {
        r.push(Rule { name: "entry_mcap", pass: feat.mcap_sol <= c.max_entry_mcap_sol, fin: false });
    }
    Evaluation::new(Gate::Eval(r), feat, true)
}

fn eval_holder(c: &HolderCfg, coin: &Coin, now_ms: i64, sol_usd: f64) -> Evaluation {
    if c.reject_mayhem && coin.mayhem {
        let feat = coin.features(now_ms, sol_usd, &c.bundle, false);
        return Evaluation::new(Gate::Eval(vec![Rule { name: "mayhem", pass: false, fin: true }]), feat, false);
    }
    if coin.age_s(now_ms) > c.max_age_s {
        return Evaluation::new(Gate::Expired, coin.features(now_ms, sol_usd, &c.bundle, false), false);
    }
    let reached = coin.n_holders >= c.min_holders;
    let crossed = reached && coin.prev_holders < c.min_holders;
    let go = if c.trigger == "continuous" { reached } else { crossed };
    if !go {
        return Evaluation::new(Gate::Wait, Feat::default(), false);
    }
    let feat = coin.features(now_ms, sol_usd, &c.bundle, true);
    let mut r = vec![
        Rule { name: "top10_band", pass: feat.top10_share >= c.top10_min && feat.top10_share <= c.top10_max, fin: false },
        Rule { name: "small_buys", pass: feat.small_buy_share < c.max_small_buy_share, fin: false },
        Rule { name: "creation_slot_buys", pass: feat.creation_slot_buys < c.max_creation_slot_buys, fin: false },
        Rule { name: "snipers", pass: feat.sniper_pct < c.max_sniper_pct, fin: false },
    ];
    if c.max_dev_pct > 0.0 {
        r.push(Rule { name: "dev_pct", pass: feat.dev_pct <= c.max_dev_pct, fin: false });
    }
    if c.min_volume_usd > 0.0 {
        r.push(Rule { name: "min_volume", pass: feat.vol_usd >= c.min_volume_usd, fin: false });
    }
    if c.max_entry_mcap_sol > 0.0 {
        r.push(Rule { name: "entry_mcap", pass: feat.mcap_sol <= c.max_entry_mcap_sol, fin: false });
    }
    bundle_rules(&c.bundle, &feat, &mut r);
    r.push(Rule { name: "curve_complete", pass: !coin.complete && !coin.migrated, fin: true });
    Evaluation::new(Gate::Eval(r), feat, true)
}

// ---------------------------------------------------------------------------------
// Exits

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExitKind {
    HardStop,
    CreatorSold,
    ProfitProtect,
    Tp1,
    Tp2,
    Trail,
    BreakEven,
    Graduation,
    TimeStop,
    KillSwitch,
}

impl ExitKind {
    pub fn name(&self) -> &'static str {
        match self {
            ExitKind::HardStop => "hard_stop",
            ExitKind::CreatorSold => "creator_sold",
            ExitKind::ProfitProtect => "profit_protect",
            ExitKind::Tp1 => "tp1",
            ExitKind::Tp2 => "tp2",
            ExitKind::Trail => "trail",
            ExitKind::BreakEven => "break_even",
            ExitKind::Graduation => "graduation",
            ExitKind::TimeStop => "time_stop",
            ExitKind::KillSwitch => "kill_switch",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExitAction {
    pub kind: ExitKind,
    /// Fraction of the ORIGINAL position to sell; None = everything left.
    pub frac: Option<f64>,
    /// Emergency exits pay much higher fees and accept any price.
    pub emergency: bool,
}

/// Position state the exit policy needs (shared by paper and live positions).
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ExitState {
    pub tp_stage: u8,
    pub pp_done: bool,
    pub trail_armed: bool,
    pub peak_pct: f64,
    pub low_pct: f64,
}

pub struct ExitInput {
    pub pct: f64,
    pub age_s: f64,
    pub creator_sold_after_entry: bool,
    pub migrated: bool,
    pub ms_since_migration: i64,
}

impl ExitState {
    pub fn observe(&mut self, pct: f64) {
        if pct > self.peak_pct {
            self.peak_pct = pct;
        }
        if pct < self.low_pct {
            self.low_pct = pct;
        }
    }
}

fn all(kind: ExitKind, emergency: bool) -> Option<ExitAction> {
    Some(ExitAction { kind, frac: None, emergency })
}

/// Decide the next exit (in the spec's priority order). Mutates stage flags only when
/// an action is returned for them; callers must apply the action.
pub fn check_exit(cfg: &Config, st: &mut ExitState, x: &ExitInput) -> Option<ExitAction> {
    match cfg.mode {
        Mode::Powerful => {
            let c = &cfg.powerful;
            if x.pct <= c.hard_stop {
                return all(ExitKind::HardStop, true);
            }
            if x.creator_sold_after_entry {
                return all(ExitKind::CreatorSold, true);
            }
            if c.be_arm_pct > 0.0 && st.peak_pct >= c.be_arm_pct && x.pct <= c.be_stop_pct {
                return all(ExitKind::BreakEven, true);
            }
            if c.pp_tp_pct > 0.0 && !st.pp_done && st.tp_stage == 0 && x.pct >= c.pp_tp_pct {
                st.pp_done = true;
                if c.pp_arms_trail {
                    st.trail_armed = true;
                }
                return Some(ExitAction { kind: ExitKind::ProfitProtect, frac: Some(c.pp_tp_frac), emergency: false });
            }
            if st.tp_stage == 0 && x.pct >= c.tp1_pct {
                st.tp_stage = 1;
                st.trail_armed = true;
                return Some(ExitAction { kind: ExitKind::Tp1, frac: Some(c.tp1_frac), emergency: false });
            }
            if st.tp_stage == 1 && x.pct >= c.tp2_pct {
                st.tp_stage = 2;
                return Some(ExitAction { kind: ExitKind::Tp2, frac: Some(c.tp2_frac), emergency: false });
            }
            if st.trail_armed && (1.0 + x.pct) <= (1.0 + st.peak_pct) * (1.0 - c.trail_pct) {
                return all(ExitKind::Trail, false);
            }
            if x.migrated && x.ms_since_migration >= c.grad_delay_ms {
                return all(ExitKind::Graduation, true);
            }
            if x.age_s >= c.time_stop_s {
                return all(ExitKind::TimeStop, true);
            }
            None
        }
        Mode::PowerBot | Mode::Gymer => {
            let c = if cfg.mode == Mode::PowerBot { &cfg.powerbot } else { &cfg.gymer };
            if !st.trail_armed && st.peak_pct >= c.trail_arm_pct {
                st.trail_armed = true;
            }
            if !st.trail_armed && x.pct <= c.hard_stop {
                return all(ExitKind::HardStop, true);
            }
            if st.trail_armed && (1.0 + x.pct) <= (1.0 + st.peak_pct) * (1.0 - c.trail_pct) {
                return all(ExitKind::Trail, false);
            }
            if c.sell_on_graduation && x.migrated && x.ms_since_migration >= c.grad_delay_ms {
                return all(ExitKind::Graduation, true);
            }
            if x.age_s >= c.time_stop_s {
                return all(ExitKind::TimeStop, true);
            }
            None
        }
    }
}
