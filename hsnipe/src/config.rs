//! Every knob is an environment variable. Defaults reproduce the spec; anything marked
//! "placeholder" is a guess you are expected to tune from paper results.

use serde::Serialize;
use std::str::FromStr;

/// Env value with surrounding whitespace and any trailing ` # comment` removed
/// (systemd EnvironmentFile keeps inline comments as part of the value).
fn raw(k: &str) -> Option<String> {
    let v = std::env::var(k).ok()?;
    let v = match v.find(" #").or_else(|| v.find("\t#")) {
        Some(i) => &v[..i],
        None => &v,
    };
    let v = v.trim().trim_matches('"').trim();
    (!v.is_empty()).then(|| v.to_string())
}
fn env_str(k: &str, d: &str) -> String {
    raw(k).unwrap_or_else(|| d.to_string())
}
fn env_opt(k: &str) -> Option<String> {
    raw(k)
}
fn env<T: FromStr>(k: &str, d: T) -> T {
    match raw(k) {
        Some(v) => v.parse().unwrap_or_else(|_| panic!("env {k}={v} is not a valid value")),
        None => d,
    }
}
fn env_bool(k: &str, d: bool) -> bool {
    match raw(k) {
        Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        None => d,
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Powerful,
    PowerBot,
    Gymer,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Powerful => "powerful",
            Mode::PowerBot => "powerbot",
            Mode::Gymer => "gymer",
        }
    }
}

/// Filters shared by all strategies to catch bundled (one-operator) launches.
#[derive(Serialize, Clone, Debug)]
pub struct BundleCfg {
    pub enabled: bool,
    /// Reject if this share of early buyers paid an identical (CU limit, CU price) pair...
    pub max_fee_cluster_share: f64,
    /// ...and the cluster has at least this many wallets.
    pub min_fee_cluster_n: u32,
    /// Reject if at least this many runs of >= `group_size` adjacent same-slot buys exist.
    pub max_groups: u32,
    pub group_size: u32,
    /// Reject if this share of early buyers were left with < `drained_sol` after buying.
    pub max_drained_share: f64,
    pub drained_sol: f64,
    /// Buyers considered: those who first bought within this many seconds of launch.
    pub window_s: f64,
}

impl BundleCfg {
    fn load(p: &str, enabled_default: bool) -> Self {
        BundleCfg {
            enabled: env_bool(&format!("{p}_BUNDLE"), enabled_default),
            max_fee_cluster_share: env(&format!("{p}_BUNDLE_FEE_SHARE"), 0.40),
            min_fee_cluster_n: env(&format!("{p}_BUNDLE_FEE_MIN_N"), 10),
            max_groups: env(&format!("{p}_BUNDLE_MAX_GROUPS"), 3),
            group_size: env(&format!("{p}_BUNDLE_GROUP_SIZE"), 4),
            max_drained_share: env(&format!("{p}_BUNDLE_DRAINED_SHARE"), 0.50),
            drained_sol: env(&format!("{p}_BUNDLE_DRAINED_SOL"), 0.01),
            window_s: env(&format!("{p}_BUNDLE_WINDOW_S"), 60.0),
        }
    }
}

/// Strategy 1 — Powerful Bot.
#[derive(Serialize, Clone, Debug)]
pub struct PowerfulCfg {
    pub min_age_s: f64,
    pub max_age_s: f64,
    pub min_m1_buyers: u32,
    pub min_volume_usd: f64,
    pub max_m1_sold_frac: f64,
    pub min_stall_ratio: f64,
    pub max_top2_share: f64,
    pub max_wallet_curve_share: f64,
    /// Trap 3: cap on entry market cap in SOL (0 = off).
    pub max_entry_mcap_sol: f64,
    pub bundle: BundleCfg,
    // exits
    pub hard_stop: f64,
    pub tp1_pct: f64,
    pub tp1_frac: f64,
    pub tp2_pct: f64,
    pub tp2_frac: f64,
    pub trail_pct: f64,
    pub time_stop_s: f64,
    /// Trap 4: wait this long after migration before the graduation sell.
    pub grad_delay_ms: i64,
    /// Trap 2: partial take-profit below 2x (0 = off).
    pub pp_tp_pct: f64,
    pub pp_tp_frac: f64,
    pub pp_arms_trail: bool,
    /// Trap 2: break-even stop: once peak >= arm, exit if pnl falls to `be_stop_pct` (0 = off).
    pub be_arm_pct: f64,
    pub be_stop_pct: f64,
}

impl PowerfulCfg {
    fn load() -> Self {
        PowerfulCfg {
            min_age_s: env("PB_MIN_AGE_S", 60.0),
            max_age_s: env("PB_MAX_AGE_S", 300.0),
            min_m1_buyers: env("PB_MIN_M1_BUYERS", 25),
            min_volume_usd: env("PB_MIN_VOLUME_USD", 11_000.0),
            max_m1_sold_frac: env("PB_MAX_M1_SOLD_FRAC", 0.40),
            min_stall_ratio: env("PB_MIN_STALL_RATIO", 0.5),
            max_top2_share: env("PB_MAX_TOP2_SHARE", 0.50),
            max_wallet_curve_share: env("PB_MAX_WALLET_CURVE_SHARE", 0.90),
            max_entry_mcap_sol: env("PB_MAX_ENTRY_MCAP_SOL", 0.0),
            bundle: BundleCfg::load("PB", true),
            hard_stop: env("PB_HARD_STOP", -0.40),
            tp1_pct: env("PB_TP1_PCT", 1.00),
            tp1_frac: env("PB_TP1_FRAC", 1.0 / 3.0),
            tp2_pct: env("PB_TP2_PCT", 2.00),
            tp2_frac: env("PB_TP2_FRAC", 1.0 / 3.0),
            trail_pct: env("PB_TRAIL_PCT", 0.30),
            time_stop_s: env("PB_TIME_STOP_S", 900.0),
            grad_delay_ms: env("PB_GRAD_DELAY_MS", 0),
            pp_tp_pct: env("PB_PP_TP_PCT", 0.0),
            pp_tp_frac: env("PB_PP_TP_FRAC", 1.0 / 3.0),
            pp_arms_trail: env_bool("PB_PP_ARMS_TRAIL", false),
            be_arm_pct: env("PB_BE_ARM_PCT", 0.0),
            be_stop_pct: env("PB_BE_STOP_PCT", 0.0),
        }
    }
}

/// Strategies 2 and 3 — PowerBot and Gymer Bot share one shape with separate numbers.
#[derive(Serialize, Clone, Debug)]
pub struct HolderCfg {
    pub min_holders: u32,
    /// "cross": evaluate only when holders crosses min_holders; "continuous": every trade.
    pub trigger: String,
    pub max_age_s: f64,
    pub top10_min: f64,
    pub top10_max: f64,
    pub small_buy_sol: f64,
    pub max_small_buy_share: f64,
    pub max_creation_slot_buys: u32,
    pub max_sniper_pct: f64,
    pub reject_mayhem: bool,
    // optional extras (0 = off)
    pub max_dev_pct: f64,
    pub min_volume_usd: f64,
    pub max_entry_mcap_sol: f64,
    pub bundle: BundleCfg,
    // exits
    pub hard_stop: f64,
    pub trail_arm_pct: f64,
    pub trail_pct: f64,
    pub time_stop_s: f64,
    pub sell_on_graduation: bool,
    pub grad_delay_ms: i64,
}

impl HolderCfg {
    fn load(p: &str, strict: bool) -> Self {
        // Gymer defaults are stricter placeholders; tune both from near-miss data.
        HolderCfg {
            min_holders: env(&format!("{p}_MIN_HOLDERS"), 50),
            trigger: env_str(&format!("{p}_TRIGGER"), "cross"),
            max_age_s: env(&format!("{p}_MAX_AGE_S"), 1800.0),
            top10_min: env(&format!("{p}_TOP10_MIN"), if strict { 0.25 } else { 0.0 }),
            top10_max: env(&format!("{p}_TOP10_MAX"), if strict { 0.30 } else { 1.0 }),
            small_buy_sol: env(&format!("{p}_SMALL_BUY_SOL"), 0.02),
            max_small_buy_share: env(&format!("{p}_MAX_SMALL_BUY_SHARE"), if strict { 0.30 } else { 0.50 }),
            max_creation_slot_buys: env(&format!("{p}_MAX_CREATION_SLOT_BUYS"), if strict { 3 } else { 5 }),
            max_sniper_pct: env(&format!("{p}_MAX_SNIPER_PCT"), if strict { 0.10 } else { 0.15 }),
            reject_mayhem: env_bool(&format!("{p}_REJECT_MAYHEM"), true),
            max_dev_pct: env(&format!("{p}_MAX_DEV_PCT"), 0.0),
            min_volume_usd: env(&format!("{p}_MIN_VOLUME_USD"), 0.0),
            max_entry_mcap_sol: env(&format!("{p}_MAX_ENTRY_MCAP_SOL"), 0.0),
            bundle: BundleCfg::load(p, false),
            hard_stop: env(&format!("{p}_HARD_STOP"), -0.30),
            trail_arm_pct: env(&format!("{p}_TRAIL_ARM_PCT"), 0.30),
            trail_pct: env(&format!("{p}_TRAIL_PCT"), 0.20),
            time_stop_s: env(&format!("{p}_TIME_STOP_S"), 900.0),
            sell_on_graduation: env_bool(&format!("{p}_SELL_ON_GRAD"), false),
            grad_delay_ms: env(&format!("{p}_GRAD_DELAY_MS"), 0),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct PaperCfg {
    pub delays: Vec<u64>,
    pub size_sol: f64,
    pub max_open: usize,
    /// "end": fill after every other trade in the landing slot (pessimistic);
    /// "start": fill before them (optimistic; delay 0 = right after the signal trade).
    pub fill_at: String,
    pub buy_cost_sol: f64,
    pub sell_cost_sol: f64,
    pub emergency_sell_cost_sol: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct LiveCfg {
    pub enabled: bool,
    /// Build + sign, then simulate on RPC instead of sending (no funds move).
    pub simulate: bool,
    /// Simulate as this address instead of the keypair's (e.g. your funded wallet).
    pub simulate_as: Option<String>,
    pub keypair_path: String,
    pub size_sol: f64,
    pub max_open: usize,
    pub min_balance_sol: f64,
    pub buy_slippage_bps: u64,
    pub sell_slippage_bps: u64,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub jito_tip: u64,
    pub emergency_cu_price: u64,
    pub emergency_jito_tip: u64,
    pub max_blockhash_age_slots: u64,
    pub confirm_timeout_slots: u64,
    pub max_sell_retries: u32,
    pub stop_file: String,
    pub kill_file: String,
    pub positions_file: String,
    pub close_ata: bool,
    pub amm_cross_coin_template: bool,
    pub send_rpc: bool,
    pub send_staked: bool,
    pub send_jito: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub log_dir: String,
    pub grpc_urls: Vec<String>,
    #[serde(skip)]
    pub grpc_x_token: Option<String>,
    #[serde(skip)]
    pub rpc_url: String,
    #[serde(skip)]
    pub staked_rpc_url: Option<String>,
    #[serde(skip)]
    pub jito_urls: Vec<String>,
    pub record: bool,
    pub record_all: bool,
    pub record_rotate_min: u64,
    pub sol_price_usd: f64,
    pub price_refresh_s: u64,
    pub summary_every_s: u64,
    pub tick_ms: u64,
    pub stream_idle_timeout_s: u64,
    pub near_miss: bool,
    pub max_coins: usize,
    pub powerful: PowerfulCfg,
    pub powerbot: HolderCfg,
    pub gymer: HolderCfg,
    pub paper: PaperCfg,
    pub live: LiveCfg,
}

impl Config {
    pub fn load() -> anyhow::Result<Config> {
        let mode = match env_str("HS_MODE", "powerful").to_ascii_lowercase().as_str() {
            "powerful" | "powerful_bot" | "pb" => Mode::Powerful,
            "powerbot" | "power" => Mode::PowerBot,
            "gymer" | "gymer_bot" | "gym" => Mode::Gymer,
            other => anyhow::bail!("HS_MODE must be powerful|powerbot|gymer, got {other}"),
        };
        let delays: Vec<u64> = env_str("HS_PAPER_DELAYS", "0,1,2")
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        let list = |k: &str| -> Vec<String> {
            env_opt(k).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
        };
        let cfg = Config {
            mode,
            log_dir: env_str("HS_LOG_DIR", &format!("logs/{}", mode.name())),
            grpc_urls: list("HS_GRPC_URLS"),
            grpc_x_token: env_opt("HS_GRPC_X_TOKEN"),
            rpc_url: env_str("HS_RPC_URL", "https://api.mainnet-beta.solana.com"),
            staked_rpc_url: env_opt("HS_STAKED_RPC_URL"),
            jito_urls: {
                let v = list("HS_JITO_URLS");
                if v.is_empty() {
                    vec!["https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/transactions".into()]
                } else {
                    v
                }
            },
            record: env_bool("HS_RECORD", true),
            record_all: env_bool("HS_RECORD_ALL", false),
            record_rotate_min: env("HS_RECORD_ROTATE_MIN", 60),
            sol_price_usd: env("HS_SOL_PRICE_USD", 150.0),
            price_refresh_s: env("HS_PRICE_REFRESH_S", 30),
            summary_every_s: env("HS_SUMMARY_EVERY_S", 30),
            tick_ms: env("HS_TICK_MS", 200),
            stream_idle_timeout_s: env("HS_STREAM_IDLE_TIMEOUT_S", 10),
            near_miss: env_bool("HS_NEAR_MISS", true),
            max_coins: env("HS_MAX_COINS", 50_000),
            powerful: PowerfulCfg::load(),
            powerbot: HolderCfg::load("PWB", false),
            gymer: HolderCfg::load("GYM", true),
            paper: PaperCfg {
                delays: if delays.is_empty() { vec![0, 1, 2] } else { delays },
                size_sol: env("HS_PAPER_SIZE_SOL", 0.1),
                max_open: env("HS_MAX_OPEN", 10),
                fill_at: env_str("HS_PAPER_FILL_AT", "end"),
                buy_cost_sol: env("HS_PAPER_BUY_COST_SOL", 0.0012),
                sell_cost_sol: env("HS_PAPER_SELL_COST_SOL", 0.0012),
                emergency_sell_cost_sol: env("HS_PAPER_EMERG_SELL_COST_SOL", 0.004),
            },
            live: LiveCfg {
                enabled: env_bool("HS_LIVE", false),
                simulate: env_bool("HS_LIVE_SIMULATE", false),
                simulate_as: env_opt("HS_LIVE_SIMULATE_AS"),
                keypair_path: env_str("HS_KEYPAIR", ""),
                size_sol: env("HS_LIVE_SIZE_SOL", 0.05),
                max_open: env("HS_LIVE_MAX_OPEN", 3),
                min_balance_sol: env("HS_LIVE_MIN_BALANCE_SOL", 0.3),
                buy_slippage_bps: env("HS_LIVE_BUY_SLIPPAGE_BPS", 1500),
                sell_slippage_bps: env("HS_LIVE_SELL_SLIPPAGE_BPS", 2000),
                cu_limit: env("HS_LIVE_CU_LIMIT", 160_000),
                cu_price: env("HS_LIVE_CU_PRICE", 400_000),
                jito_tip: env("HS_LIVE_JITO_TIP", 200_000),
                emergency_cu_price: env("HS_LIVE_EMERG_CU_PRICE", 3_000_000),
                emergency_jito_tip: env("HS_LIVE_EMERG_JITO_TIP", 1_000_000),
                max_blockhash_age_slots: env("HS_LIVE_MAX_BLOCKHASH_AGE", 100),
                confirm_timeout_slots: env("HS_LIVE_CONFIRM_TIMEOUT_SLOTS", 40),
                max_sell_retries: env("HS_LIVE_MAX_SELL_RETRIES", 8),
                stop_file: env_str("HS_STOP_FILE", "STOP"),
                kill_file: env_str("HS_KILL_FILE", "KILL"),
                positions_file: env_str("HS_POSITIONS_FILE", "positions.json"),
                close_ata: env_bool("HS_LIVE_CLOSE_ATA", true),
                // Experimental: failed mainnet simulation (InvalidPool) on some pools, so off.
                amm_cross_coin_template: env_bool("HS_LIVE_AMM_XCOIN_TEMPLATE", false),
                send_rpc: env_bool("HS_LIVE_SEND_RPC", true),
                send_staked: env_bool("HS_LIVE_SEND_STAKED", true),
                send_jito: env_bool("HS_LIVE_SEND_JITO", true),
            },
        };
        if cfg.live.enabled && cfg.mode != Mode::Powerful {
            anyhow::bail!("live trading is implemented for HS_MODE=powerful only");
        }
        if cfg.live.enabled && cfg.live.keypair_path.is_empty() && !cfg.live.simulate {
            anyhow::bail!("HS_LIVE=1 needs HS_KEYPAIR=/path/to/keypair.json");
        }
        Ok(cfg)
    }
}
