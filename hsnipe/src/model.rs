//! Normalized events. The gRPC stream, the RPC backfill and the replay file all
//! produce these, so every rule runs on exactly the same data in every mode.

use crate::types::{Pk, Sig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateEv {
    pub slot: u64,
    pub idx: u32,
    pub ts: i64,
    pub sig: Sig,
    pub mint: Pk,
    pub creator: Pk,
    pub user: Pk,
    #[serde(default)]
    pub bonding_curve: Pk,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mayhem: bool,
    #[serde(default, skip_serializing_if = "Pk::is_zero")]
    pub quote: Pk,
    #[serde(default)]
    pub token_program: Pk,
    #[serde(default)]
    pub vsol: u64,
    #[serde(default)]
    pub vtok: u64,
    #[serde(default)]
    pub rtok: u64,
    #[serde(default)]
    pub supply: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub creator_fee_bps: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub holder_reward: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TradeEv {
    pub slot: u64,
    pub idx: u32,
    pub ts: i64,
    pub sig: Sig,
    pub mint: Pk,
    pub user: Pk,
    pub buy: bool,
    pub sol: u64,
    pub tok: u64,
    pub vsol: u64,
    pub vtok: u64,
    pub rsol: u64,
    pub rtok: u64,
    pub fee_bps: u16,
    pub cfee_bps: u16,
    pub creator: Pk,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mayhem: bool,
    #[serde(default, skip_serializing_if = "Pk::is_zero")]
    pub quote: Pk,
    /// ComputeBudget SetComputeUnitPrice (micro-lamports/CU) of the tx.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub cu_price: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub cu_limit: u32,
    /// v1 transaction-config priority fee (SIMD-0385), if any.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub prio: u64,
    /// Jito tip paid by the tx, lamports.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tip: u64,
    /// Trader's SOL balance after the tx (0 if unknown).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub post_sol: u64,
    /// Fee payer of the tx (differs from `user` for relayed/router trades).
    #[serde(default)]
    pub payer: Pk,
    #[serde(default)]
    pub ix: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AmmEv {
    pub slot: u64,
    pub idx: u32,
    pub ts: i64,
    pub sig: Sig,
    pub pool: Pk,
    pub mint: Pk,
    pub user: Pk,
    pub buy: bool,
    pub base: u64,
    pub quote: u64,
    pub pb: u64,
    pub pq: u64,
    pub lp_bps: u16,
    pub proto_bps: u16,
    pub cc_bps: u16,
    #[serde(default)]
    pub coin_creator: Pk,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MigrateEv {
    pub slot: u64,
    pub idx: u32,
    pub ts: i64,
    pub mint: Pk,
    pub pool: Pk,
    pub base: u64,
    pub quote: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OwnEv {
    pub slot: u64,
    pub ts: i64,
    pub sig: Sig,
    #[serde(default)]
    pub err: Option<String>,
    /// Change of our wallet's SOL balance in this tx (includes fee, tip, rent).
    pub sol_delta: i64,
    pub fee: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t")]
pub enum Event {
    #[serde(rename = "create")]
    Create(CreateEv),
    #[serde(rename = "trade")]
    Trade(TradeEv),
    #[serde(rename = "amm")]
    Amm(AmmEv),
    #[serde(rename = "complete")]
    Complete { slot: u64, ts: i64, mint: Pk },
    #[serde(rename = "migrate")]
    Migrate(MigrateEv),
    #[serde(rename = "block")]
    Block {
        slot: u64,
        ts: i64,
        hash: String,
        #[serde(default)]
        height: Option<u64>,
        #[serde(default)]
        block_time: Option<i64>,
    },
    /// First time we learned the slot exists (FIRST_SHRED_RECEIVED, or first tx seen).
    #[serde(rename = "slot")]
    Slot { slot: u64, ts: i64 },
    #[serde(rename = "price")]
    Price { ts: i64, usd: f64 },
    #[serde(rename = "own")]
    Own(OwnEv),
}

impl Event {
    pub fn ts(&self) -> i64 {
        match self {
            Event::Create(e) => e.ts,
            Event::Trade(e) => e.ts,
            Event::Amm(e) => e.ts,
            Event::Complete { ts, .. } => *ts,
            Event::Migrate(e) => e.ts,
            Event::Block { ts, .. } => *ts,
            Event::Slot { ts, .. } => *ts,
            Event::Price { ts, .. } => *ts,
            Event::Own(e) => e.ts,
        }
    }
    pub fn slot(&self) -> Option<u64> {
        match self {
            Event::Create(e) => Some(e.slot),
            Event::Trade(e) => Some(e.slot),
            Event::Amm(e) => Some(e.slot),
            Event::Complete { slot, .. } => Some(*slot),
            Event::Migrate(e) => Some(e.slot),
            Event::Block { slot, .. } => Some(*slot),
            Event::Slot { slot, .. } => Some(*slot),
            Event::Price { .. } => None,
            Event::Own(e) => Some(e.slot),
        }
    }
}

/// Where a cloned instruction trades.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Venue {
    Curve,
    Amm,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AcctMeta {
    pub pk: Pk,
    pub signer: bool,
    pub writable: bool,
}

/// A real on-chain pump.fun / PumpSwap instruction, kept so live trading can clone it
/// and swap in only our user-specific accounts.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IxTemplate {
    pub venue: Venue,
    pub is_buy: bool,
    pub program: Pk,
    pub accounts: Vec<AcctMeta>,
    pub data: Vec<u8>,
    /// The trader whose accounts get substituted.
    pub user: Pk,
    pub mint: Pk,
    pub slot: u64,
    /// PumpSwap only: pool and the pool's coin creator (for cross-coin reuse).
    #[serde(default)]
    pub pool: Pk,
    #[serde(default)]
    pub coin_creator: Pk,
}

pub type Tmpl = Arc<IxTemplate>;
