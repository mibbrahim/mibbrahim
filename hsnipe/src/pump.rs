//! pump.fun bonding-curve + PumpSwap AMM: program ids, Anchor discriminators, event
//! decoders and swap math. Layouts follow the public IDLs in
//! github.com/pump-fun/pump-public-docs (checked against mainnet transactions).

use crate::types::Pk;
use std::sync::LazyLock;

pub static PUMP: LazyLock<Pk> = LazyLock::new(|| Pk::lit("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"));
pub static AMM: LazyLock<Pk> = LazyLock::new(|| Pk::lit("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"));
pub static COMPUTE_BUDGET: LazyLock<Pk> = LazyLock::new(|| Pk::lit("ComputeBudget111111111111111111111111111111"));
pub static SYSTEM: LazyLock<Pk> = LazyLock::new(|| Pk::lit("11111111111111111111111111111111"));
pub static TOKEN: LazyLock<Pk> = LazyLock::new(|| Pk::lit("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"));
pub static TOKEN_2022: LazyLock<Pk> = LazyLock::new(|| Pk::lit("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"));
pub static ATA_PROGRAM: LazyLock<Pk> = LazyLock::new(|| Pk::lit("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"));
pub static WSOL: LazyLock<Pk> = LazyLock::new(|| Pk::lit("So11111111111111111111111111111111111111112"));

pub static JITO_TIPS: LazyLock<Vec<Pk>> = LazyLock::new(|| {
    [
        "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
        "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
        "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
        "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
        "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
        "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
        "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
        "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
    ]
    .iter()
    .map(|s| Pk::lit(s))
    .collect()
});

/// Anchor `emit_cpi!` prefix on self-CPI event instructions.
pub const EVENT_TAG: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

// Event discriminators: sha256("event:<Name>")[..8]
pub const EV_TRADE: [u8; 8] = [0xbd, 0xdb, 0x7f, 0xd3, 0x4e, 0xe6, 0x61, 0xee];
pub const EV_CREATE: [u8; 8] = [0x1b, 0x72, 0xa9, 0x4d, 0xde, 0xeb, 0x63, 0x76];
pub const EV_COMPLETE: [u8; 8] = [0x5f, 0x72, 0x61, 0x9c, 0xd4, 0x2e, 0x98, 0x08];
pub const EV_MIGRATION: [u8; 8] = [0xbd, 0xe9, 0x5d, 0xb9, 0x5c, 0x94, 0xea, 0x94];
pub const EV_AMM_BUY: [u8; 8] = [0x67, 0xf4, 0x52, 0x1f, 0x2c, 0xf5, 0x77, 0x77];
pub const EV_AMM_SELL: [u8; 8] = [0x3e, 0x2f, 0x37, 0x0a, 0xa5, 0x03, 0xdc, 0x2a];

// Instruction discriminators: sha256("global:<name>")[..8]
pub const IX_BUY: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
pub const IX_SELL: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
pub const IX_BUY_EXACT_SOL_IN: [u8; 8] = [0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
pub const IX_BUY_V2: [u8; 8] = [0xb8, 0x17, 0xee, 0x61, 0x67, 0xc5, 0xd3, 0x3d];
pub const IX_SELL_V2: [u8; 8] = [0x5d, 0xf6, 0x82, 0x3c, 0xe7, 0xe9, 0x40, 0xb2];
pub const IX_BUY_EXACT_QUOTE_IN_V2: [u8; 8] = [0xc2, 0xab, 0x1c, 0x46, 0x68, 0x4d, 0x5b, 0x2f];
pub const IX_CREATE: [u8; 8] = [0x18, 0x1e, 0xc8, 0x28, 0x05, 0x1c, 0x07, 0x77];
pub const IX_CREATE_V2: [u8; 8] = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4];
pub const IX_AMM_BUY_EXACT_QUOTE_IN: [u8; 8] = [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70];

/// Raw token supply of every pump.fun coin (1B tokens, 6 decimals).
pub const TOTAL_SUPPLY: u64 = 1_000_000_000_000_000;
/// Tokens sold through the bonding curve (the "curve supply").
pub const CURVE_SUPPLY: u64 = 793_100_000_000_000;

pub fn disc(d: &[u8]) -> [u8; 8] {
    let mut a = [0u8; 8];
    if d.len() >= 8 {
        a.copy_from_slice(&d[..8]);
    }
    a
}

pub fn is_pump_buy_ix(d: &[u8; 8]) -> bool {
    *d == IX_BUY || *d == IX_BUY_EXACT_SOL_IN || *d == IX_BUY_V2 || *d == IX_BUY_EXACT_QUOTE_IN_V2
}
pub fn is_pump_sell_ix(d: &[u8; 8]) -> bool {
    *d == IX_SELL || *d == IX_SELL_V2
}
pub fn is_amm_trade_ix(d: &[u8; 8]) -> bool {
    *d == IX_BUY || *d == IX_SELL || *d == IX_AMM_BUY_EXACT_QUOTE_IN
}

/// True for the quote mints that mean "native SOL" on the bonding curve.
pub fn is_sol_quote(q: &Pk) -> bool {
    q.is_zero() || *q == *WSOL
}

/// Little-endian Borsh reader that returns None instead of panicking on short data.
pub struct Rd<'a> {
    b: &'a [u8],
    o: usize,
}

impl<'a> Rd<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Rd { b, o: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.o + n > self.b.len() {
            return None;
        }
        let s = &self.b[self.o..self.o + n];
        self.o += n;
        Some(s)
    }
    pub fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|v| v != 0)
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
    }
    pub fn i128(&mut self) -> Option<i128> {
        self.take(16).map(|s| i128::from_le_bytes(s.try_into().unwrap()))
    }
    pub fn pk(&mut self) -> Option<Pk> {
        self.take(32).and_then(Pk::from_slice)
    }
    pub fn str(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        if n > 4096 {
            return None;
        }
        self.take(n)
    }
}

/// Instruction name carried in TradeEvent.ix_name, as a compact code.
pub fn ix_code(name: &[u8]) -> u8 {
    match name {
        b"buy" => 1,
        b"sell" => 2,
        b"buy_exact_sol_in" => 3,
        b"buy_v2" => 4,
        b"sell_v2" => 5,
        b"buy_exact_quote_in_v2" | b"buy_exact_quote_in" => 6,
        _ => 0,
    }
}

#[derive(Debug, Clone, Default)]
pub struct TradeRaw {
    pub mint: Pk,
    pub sol: u64,
    pub tokens: u64,
    pub is_buy: bool,
    pub user: Pk,
    pub vsol: u64,
    pub vtok: u64,
    pub rsol: u64,
    pub rtok: u64,
    pub fee_bps: u64,
    pub fee: u64,
    pub creator: Pk,
    pub creator_fee_bps: u64,
    pub creator_fee: u64,
    pub ix: u8,
    pub mayhem: bool,
    pub quote_mint: Pk,
}

/// Decode a TradeEvent body (after the 16 tag+discriminator bytes).
pub fn decode_trade(b: &[u8]) -> Option<TradeRaw> {
    let mut r = Rd::new(b);
    let mut t = TradeRaw {
        mint: r.pk()?,
        sol: r.u64()?,
        tokens: r.u64()?,
        is_buy: r.bool()?,
        user: r.pk()?,
        vsol: {
            r.skip(8)?; // timestamp
            r.u64()?
        },
        vtok: r.u64()?,
        rsol: r.u64()?,
        rtok: r.u64()?,
        ..Default::default()
    };
    r.skip(32)?; // fee_recipient
    t.fee_bps = r.u64()?;
    t.fee = r.u64()?;
    t.creator = r.pk()?;
    t.creator_fee_bps = r.u64()?;
    t.creator_fee = r.u64()?;
    // Everything below was appended over time; older events simply end earlier.
    let _ = (|| -> Option<()> {
        r.skip(1 + 8 + 8 + 8 + 8)?; // track_volume, unclaimed, claimed, current_sol_volume, last_update
        t.ix = ix_code(r.str()?);
        t.mayhem = r.bool()?;
        r.skip(8 + 8 + 8 + 8)?; // cashback bps/amount, buyback bps/amount
        let n = r.u32()? as usize; // shareholders: Vec<{pubkey,u16}>
        r.skip(n.checked_mul(34)?)?;
        t.quote_mint = r.pk()?;
        Some(())
    })();
    Some(t)
}

#[derive(Debug, Clone, Default)]
pub struct CreateRaw {
    pub name: String,
    pub symbol: String,
    pub mint: Pk,
    pub bonding_curve: Pk,
    pub user: Pk,
    pub creator: Pk,
    pub vtok: u64,
    pub vsol: u64,
    pub rtok: u64,
    pub supply: u64,
    pub token_program: Pk,
    pub mayhem: bool,
    pub cashback: bool,
    pub quote_mint: Pk,
    pub creator_fee_bps: u64,
    pub holder_reward: bool,
}

fn lossy(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    s.chars().filter(|c| !c.is_control()).take(40).collect()
}

pub fn decode_create_event(b: &[u8]) -> Option<CreateRaw> {
    let mut r = Rd::new(b);
    let name = lossy(r.str()?);
    let symbol = lossy(r.str()?);
    r.str()?; // uri
    let mut c = CreateRaw {
        name,
        symbol,
        mint: r.pk()?,
        bonding_curve: r.pk()?,
        user: r.pk()?,
        creator: r.pk()?,
        vtok: {
            r.skip(8)?; // timestamp
            r.u64()?
        },
        vsol: r.u64()?,
        rtok: r.u64()?,
        supply: r.u64()?,
        ..Default::default()
    };
    let _ = (|| -> Option<()> {
        c.token_program = r.pk()?;
        c.mayhem = r.bool()?;
        c.cashback = r.bool()?;
        c.quote_mint = r.pk()?;
        r.skip(8)?; // virtual_quote_reserves
        c.creator_fee_bps = r.u64()?;
        c.holder_reward = r.bool()?;
        Some(())
    })();
    Some(c)
}

/// `create` / `create_v2` instruction args (fallback when no CreateEvent is present).
/// Returns (name, symbol, creator, mayhem). The Mayhem flag is the byte right after
/// the creator pubkey, i.e. offset 32 after the three strings.
pub fn decode_create_ix(data: &[u8]) -> Option<(String, String, Pk, bool)> {
    let v2 = disc(data) == IX_CREATE_V2;
    let mut r = Rd::new(&data[8..]);
    let name = lossy(r.str()?);
    let symbol = lossy(r.str()?);
    r.str()?;
    let creator = r.pk()?;
    let mayhem = if v2 { r.bool().unwrap_or(false) } else { false };
    Some((name, symbol, creator, mayhem))
}

/// CompleteEvent: user, mint, bonding_curve, timestamp, [quote_mint]
pub fn decode_complete(b: &[u8]) -> Option<Pk> {
    let mut r = Rd::new(b);
    r.skip(32)?;
    r.pk()
}

#[derive(Debug, Clone, Default)]
pub struct MigrationRaw {
    pub mint: Pk,
    pub mint_amount: u64,
    pub sol_amount: u64,
    pub pool: Pk,
}

/// CompletePumpAmmMigrationEvent: user, mint, mint_amount, sol_amount, pool_migration_fee,
/// bonding_curve, timestamp, pool, [quote_mint]
pub fn decode_migration(b: &[u8]) -> Option<MigrationRaw> {
    let mut r = Rd::new(b);
    r.skip(32)?;
    let mint = r.pk()?;
    let mint_amount = r.u64()?;
    let sol_amount = r.u64()?;
    r.skip(8 + 32 + 8)?; // pool_migration_fee, bonding_curve, timestamp
    let pool = r.pk()?;
    Some(MigrationRaw { mint, mint_amount, sol_amount, pool })
}

#[derive(Debug, Clone, Default)]
pub struct AmmTradeRaw {
    pub base: u64,
    /// SOL the user actually paid (buy) or received (sell), fees included/excluded accordingly.
    pub quote_user: u64,
    pub pool_base_after: u64,
    pub pool_quote_after: u64,
    pub lp_bps: u64,
    pub protocol_bps: u64,
    pub creator_bps: u64,
    pub pool: Pk,
    pub user: Pk,
    pub coin_creator: Pk,
}

/// PumpSwap BuyEvent/SellEvent. Pool reserves in the event are pre-trade; we return
/// post-trade effective reserves (quote includes virtual_quote_reserves).
pub fn decode_amm_trade(is_buy: bool, b: &[u8]) -> Option<AmmTradeRaw> {
    let mut r = Rd::new(b);
    r.skip(8)?; // timestamp
    let base = r.u64()?;
    r.skip(8 + 8 + 8)?; // limit, user_base_reserves, user_quote_reserves
    let pool_base = r.u64()?;
    let pool_quote = r.u64()?;
    r.u64()?; // quote_amount_in / quote_amount_out (gross)
    let lp_bps = r.u64()?;
    r.u64()?;
    let protocol_bps = r.u64()?;
    r.u64()?;
    let pool_quote_delta = r.u64()?; // with lp fee (buy) / without lp fee (sell)
    let quote_user = r.u64()?;
    let pool = r.pk()?;
    let user = r.pk()?;
    r.skip(32 * 4)?;
    let coin_creator = r.pk()?;
    let creator_bps = r.u64()?;
    r.u64()?; // coin_creator_fee
    let vqr: i128 = (|| -> Option<i128> {
        if is_buy {
            r.skip(1 + 8 + 8 + 8 + 8 + 8)?; // track_volume .. last_update, min_base_amount_out
            r.str()?; // ix_name
        }
        r.skip(8 * 4)?; // cashback bps/amount, buyback bps/amount
        r.i128()
    })()
    .unwrap_or(0);
    let (pb, pq) = if is_buy {
        (pool_base.saturating_sub(base), pool_quote.saturating_add(pool_quote_delta))
    } else {
        (pool_base.saturating_add(base), pool_quote.saturating_sub(pool_quote_delta))
    };
    let pq_eff = (pq as i128 + vqr).clamp(0, u64::MAX as i128) as u64;
    Some(AmmTradeRaw {
        base,
        quote_user,
        pool_base_after: pb,
        pool_quote_after: pq_eff,
        lp_bps,
        protocol_bps,
        creator_bps,
        pool,
        user,
        coin_creator,
    })
}

// ---------------------------------------------------------------------------------
// Swap math

fn fee_ceil(amount: u64, bps: u64) -> u64 {
    ((amount as u128 * bps as u128 + 9_999) / 10_000) as u64
}

/// Bonding-curve buy with a total SOL budget (fees included), like `buy_exact_sol_in`.
/// Returns (tokens_out, sol_into_curve, fees).
pub fn curve_buy(vsol: u64, vtok: u64, rtok: u64, budget: u64, fee_bps: u64, cfee_bps: u64) -> (u64, u64, u64) {
    if vsol == 0 || vtok == 0 || budget == 0 {
        return (0, 0, 0);
    }
    let total_bps = 10_000 + fee_bps + cfee_bps;
    let mut sol_in = (budget as u128 * 10_000 / total_bps as u128) as u64;
    // Like the program: never let sol + rounded-up fees exceed the budget.
    while sol_in > 0 && sol_in + fee_ceil(sol_in, fee_bps) + fee_ceil(sol_in, cfee_bps) > budget {
        sol_in -= 1;
    }
    let mut tokens = (sol_in as u128 * vtok as u128 / (vsol as u128 + sol_in as u128)) as u64;
    let mut sol_used = sol_in;
    if tokens > rtok {
        // Curve runs out: pay only for what is left.
        tokens = rtok;
        sol_used = ((tokens as u128 * vsol as u128) / (vtok as u128 - tokens as u128).max(1) + 1) as u64;
    }
    let fees = fee_ceil(sol_used, fee_bps) + fee_ceil(sol_used, cfee_bps);
    (tokens, sol_used, fees)
}

/// Bonding-curve sell. Returns (net_sol_to_user, gross_sol, fees).
pub fn curve_sell(vsol: u64, vtok: u64, tokens: u64, fee_bps: u64, cfee_bps: u64) -> (u64, u64, u64) {
    if vsol == 0 || vtok == 0 || tokens == 0 {
        return (0, 0, 0);
    }
    let gross = (tokens as u128 * vsol as u128 / (vtok as u128 + tokens as u128)) as u64;
    let fees = fee_ceil(gross, fee_bps) + fee_ceil(gross, cfee_bps);
    (gross.saturating_sub(fees), gross, fees)
}

/// PumpSwap sell of `base_in` tokens. Returns (net_quote_to_user, gross, fees).
pub fn amm_sell(pool_base: u64, pool_quote: u64, base_in: u64, lp_bps: u64, protocol_bps: u64, creator_bps: u64) -> (u64, u64, u64) {
    if pool_base == 0 || pool_quote == 0 || base_in == 0 {
        return (0, 0, 0);
    }
    let gross = (base_in as u128 * pool_quote as u128 / (pool_base as u128 + base_in as u128)) as u64;
    let fees = fee_ceil(gross, lp_bps) + fee_ceil(gross, protocol_bps) + fee_ceil(gross, creator_bps);
    (gross.saturating_sub(fees), gross, fees)
}

/// Market cap in SOL from virtual reserves (price x 1B supply).
#[cfg(test)]
pub fn mcap_sol(vsol: u64, vtok: u64) -> f64 {
    if vtok == 0 {
        return 0.0;
    }
    vsol as f64 * (TOTAL_SUPPLY as f64 / 1e9) / vtok as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn anchor(s: &str) -> [u8; 8] {
        let h = Sha256::digest(s.as_bytes());
        h[..8].try_into().unwrap()
    }

    #[test]
    fn discriminators_match_anchor_hashes() {
        assert_eq!(anchor("event:TradeEvent"), EV_TRADE);
        assert_eq!(anchor("event:CreateEvent"), EV_CREATE);
        assert_eq!(anchor("event:CompleteEvent"), EV_COMPLETE);
        assert_eq!(anchor("event:CompletePumpAmmMigrationEvent"), EV_MIGRATION);
        assert_eq!(anchor("event:BuyEvent"), EV_AMM_BUY);
        assert_eq!(anchor("event:SellEvent"), EV_AMM_SELL);
        assert_eq!(anchor("global:buy"), IX_BUY);
        assert_eq!(anchor("global:sell"), IX_SELL);
        assert_eq!(anchor("global:buy_exact_sol_in"), IX_BUY_EXACT_SOL_IN);
        assert_eq!(anchor("global:buy_v2"), IX_BUY_V2);
        assert_eq!(anchor("global:sell_v2"), IX_SELL_V2);
        assert_eq!(anchor("global:buy_exact_quote_in_v2"), IX_BUY_EXACT_QUOTE_IN_V2);
        assert_eq!(anchor("global:create"), IX_CREATE);
        assert_eq!(anchor("global:create_v2"), IX_CREATE_V2);
        assert_eq!(anchor("global:buy_exact_quote_in"), IX_AMM_BUY_EXACT_QUOTE_IN);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // Real mainnet TradeEvent (slot 452024288, 0.7 SOL buy of EM1W...pump).
    const TRADE_HEX: &str = "e445a52e51cb9a1dbddb7fd34ee661eec64820775191c522d7efd717bb0c4fc542646c0f61c7d62424396cbbf4265c8f8027b92900000000beda7d0540160000018ae6a2a667c302c533cd414f72b0544fd53ee2d604c0bf5c8a4e406615b0863bd23ebd6a00000000f1beeb2507000000ee60f01d9bb90300f112c82900000000eec8ddd109bb02004ac2f8d0dd5cbc97e3289c197cb5062a54f3d956b9ce6e5115f96567aa5cb3e65f0000000000000092786500000000001febbcc1ce2f593312ba79aa756cc0c770f5996b13557594bcacfc2f80039be31e00000000000000210b200000000000000000000000000000000000000000000000000000000000000000000000000000030000006275790000000000000000000000000000000000881300000000000049bc3200000000000000000000000000000000000000000000000000000000000000000000000000000000008027b92900000000f1beeb2507000000f112c829000000001e00000000000000210b200000000000";

    #[test]
    fn decodes_real_trade_event() {
        let d = hex(TRADE_HEX);
        assert_eq!(d[..8], EVENT_TAG);
        assert_eq!(d[8..16], EV_TRADE);
        let t = decode_trade(&d[16..]).unwrap();
        assert_eq!(t.mint.b58(), "EM1WnMcHLeMqBFWKoNstRj7qautMwSphqRQVBVw1pump");
        assert_eq!(t.user.b58(), "AMDEmVocLtN5ji2gBY7BrmSHEyM5vt6TGszBbg4crMXg");
        assert!(t.is_buy);
        assert_eq!(t.sol, 700_000_128);
        assert_eq!(t.tokens, 24_464_225_852_094);
        assert_eq!(t.vsol, 30_700_977_905);
        assert_eq!(t.rtok, 768_600_803_494_126);
        assert_eq!((t.fee_bps, t.fee), (95, 6_650_002));
        assert_eq!((t.creator_fee_bps, t.creator_fee), (30, 2_100_001));
        assert_eq!(t.ix, 1);
        assert!(!t.mayhem);
        assert!(t.quote_mint.is_zero());
        // Legacy `buy` fixes the token amount; the program charges ceil(x*vsol/(vtok-x)).
        let vsol0 = t.vsol - t.sol;
        let vtok0 = t.vtok + t.tokens;
        let cost = (t.tokens as u128 * vsol0 as u128).div_ceil(vtok0 as u128 - t.tokens as u128) as u64;
        assert!(cost.abs_diff(t.sol) <= 1, "{cost} vs {}", t.sol);
        assert_eq!(fee_ceil(t.sol, 95), t.fee);
        assert_eq!(fee_ceil(t.sol, 30), t.creator_fee);
    }

    #[test]
    fn curve_roundtrip_loses_fees_only() {
        let (vsol, vtok, rtok) = (30_000_000_000u64, 1_073_000_000_000_000u64, 793_100_000_000_000u64);
        let (tok, sol_in, fees) = curve_buy(vsol, vtok, rtok, 100_000_000, 95, 30);
        assert!(sol_in + fees <= 100_000_000);
        // Same split as real buy_exact_sol_in(0.1 SOL) trades on mainnet.
        assert_eq!((sol_in, fees), (98_765_431, 938_272 + 296_297));
        let (net, gross, _) = curve_sell(vsol + sol_in, vtok - tok, tok, 95, 30);
        assert!(gross <= sol_in);
        let loss = 1.0 - net as f64 / 100_000_000.0;
        assert!(loss > 0.024 && loss < 0.026, "loss {loss}");
        assert!((mcap_sol(vsol, vtok) - 27.96).abs() < 0.01);
    }
}
