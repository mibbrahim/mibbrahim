//! Transaction -> events. `RawTx` is a transport-neutral view of a transaction that
//! the gRPC stream and the getBlock backfill both fill in.

use crate::model::*;
use crate::pump::{self, *};
use crate::types::{Pk, Sig};
use std::sync::Arc;

#[derive(Default, Debug)]
pub struct RawIx {
    pub program: u32,
    pub accounts: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct RawTx {
    pub slot: u64,
    pub idx: u32,
    pub ts: i64,
    pub sig: Sig,
    /// static keys, then loaded writable, then loaded readonly
    pub keys: Vec<Pk>,
    /// num_required_signatures, num_readonly_signed, num_readonly_unsigned
    pub header: [u32; 3],
    pub n_static: usize,
    pub n_loaded_w: usize,
    pub outer: Vec<RawIx>,
    /// (outer instruction index, inner instructions)
    pub inner: Vec<(u32, Vec<RawIx>)>,
    pub pre: Vec<u64>,
    pub post: Vec<u64>,
    pub err: Option<String>,
    pub fee: u64,
    pub v1_prio: Option<u64>,
    pub v1_cu_limit: Option<u32>,
}

impl RawTx {
    pub fn is_signer(&self, i: usize) -> bool {
        i < self.header[0] as usize
    }
    pub fn is_writable(&self, i: usize) -> bool {
        let (nrs, nros, nrou) = (self.header[0] as usize, self.header[1] as usize, self.header[2] as usize);
        if i < self.n_static {
            if i < nrs {
                i < nrs.saturating_sub(nros)
            } else {
                i < self.n_static.saturating_sub(nrou)
            }
        } else {
            i < self.n_static + self.n_loaded_w
        }
    }
    fn key(&self, i: u8) -> Option<&Pk> {
        self.keys.get(i as usize)
    }
}

#[derive(Default)]
pub struct Decoded {
    pub events: Vec<Event>,
    pub templates: Vec<Tmpl>,
}

#[derive(Clone, Default)]
pub struct Decoder {
    pub wallet: Option<Pk>,
    pub capture_templates: bool,
}

struct TxCtx {
    cu_price: u64,
    cu_limit: u32,
    prio: u64,
    tip: u64,
}

impl Decoder {
    pub fn decode(&self, tx: &RawTx, out: &mut Decoded) {
        if let Some(w) = self.wallet {
            let signers = (tx.header[0] as usize).min(tx.keys.len());
            if let Some(pos) = tx.keys[..signers].iter().position(|k| *k == w) {
                let delta = tx.post.get(pos).copied().unwrap_or(0) as i64 - tx.pre.get(pos).copied().unwrap_or(0) as i64;
                out.events.push(Event::Own(OwnEv {
                    slot: tx.slot,
                    ts: tx.ts,
                    sig: tx.sig,
                    err: tx.err.clone(),
                    sol_delta: delta,
                    fee: tx.fee,
                }));
            }
        }
        if tx.err.is_some() {
            return;
        }
        let ctx = self.tx_ctx(tx);
        // Walk outer instructions, each followed by its inner (CPI) instructions, in
        // execution order. Events are self-CPIs emitted after their instruction.
        let mut last_pump_ix: Option<&RawIx> = None;
        let mut last_amm_ix: Option<&RawIx> = None;
        for (oi, ix) in tx.outer.iter().enumerate() {
            self.visit(tx, ix, &ctx, &mut last_pump_ix, &mut last_amm_ix, out);
            for (idx, list) in &tx.inner {
                if *idx as usize == oi {
                    for iix in list {
                        self.visit(tx, iix, &ctx, &mut last_pump_ix, &mut last_amm_ix, out);
                    }
                }
            }
        }
    }

    fn tx_ctx(&self, tx: &RawTx) -> TxCtx {
        let mut c = TxCtx { cu_price: 0, cu_limit: 0, prio: tx.v1_prio.unwrap_or(0), tip: 0 };
        if let Some(l) = tx.v1_cu_limit {
            c.cu_limit = l;
        }
        for ix in &tx.outer {
            let Some(p) = tx.keys.get(ix.program as usize) else { continue };
            if *p == *COMPUTE_BUDGET {
                match ix.data.first() {
                    Some(2) if ix.data.len() >= 5 => c.cu_limit = u32::from_le_bytes(ix.data[1..5].try_into().unwrap()),
                    Some(3) if ix.data.len() >= 9 => c.cu_price = u64::from_le_bytes(ix.data[1..9].try_into().unwrap()),
                    _ => {}
                }
            } else if *p == *SYSTEM && ix.data.len() == 12 && ix.data[..4] == [2, 0, 0, 0] && ix.accounts.len() >= 2 {
                if let Some(to) = tx.key(ix.accounts[1]) {
                    if JITO_TIPS.contains(to) {
                        c.tip += u64::from_le_bytes(ix.data[4..12].try_into().unwrap());
                    }
                }
            }
        }
        c
    }

    fn visit<'a>(
        &self,
        tx: &RawTx,
        ix: &'a RawIx,
        ctx: &TxCtx,
        last_pump_ix: &mut Option<&'a RawIx>,
        last_amm_ix: &mut Option<&'a RawIx>,
        out: &mut Decoded,
    ) {
        let Some(program) = tx.keys.get(ix.program as usize) else { return };
        let is_pump = *program == *PUMP;
        let is_amm = *program == *AMM;
        if !is_pump && !is_amm {
            return;
        }
        let d = &ix.data;
        if d.len() < 8 {
            return;
        }
        let dsc = disc(d);
        if dsc == EVENT_TAG && d.len() >= 16 {
            let ev = disc(&d[8..]);
            let body = &d[16..];
            if is_pump {
                self.pump_event(tx, ev, body, ctx, *last_pump_ix, out);
            } else {
                self.amm_event(tx, ev, body, *last_amm_ix, out);
            }
            return;
        }
        if is_pump {
            if is_pump_buy_ix(&dsc) || is_pump_sell_ix(&dsc) {
                *last_pump_ix = Some(ix);
            } else if dsc == IX_CREATE || dsc == IX_CREATE_V2 {
                // Fallback create (used only if the tx carries no CreateEvent).
                let has_event = tx.inner.iter().flat_map(|(_, l)| l.iter()).any(|i| {
                    tx.keys.get(i.program as usize) == Some(&*PUMP)
                        && i.data.len() >= 16
                        && disc(&i.data) == EVENT_TAG
                        && disc(&i.data[8..]) == EV_CREATE
                });
                if !has_event {
                    if let (Some((name, symbol, creator, mayhem)), Some(mint)) =
                        (pump::decode_create_ix(d), ix.accounts.first().and_then(|a| tx.key(*a)))
                    {
                        let v2 = dsc == IX_CREATE_V2;
                        let user_idx = if v2 { 5 } else { 7 };
                        out.events.push(Event::Create(CreateEv {
                            slot: tx.slot,
                            idx: tx.idx,
                            ts: tx.ts,
                            sig: tx.sig,
                            mint: *mint,
                            creator,
                            user: ix.accounts.get(user_idx).and_then(|a| tx.key(*a)).copied().unwrap_or_default(),
                            bonding_curve: ix.accounts.get(2).and_then(|a| tx.key(*a)).copied().unwrap_or_default(),
                            name,
                            symbol,
                            mayhem,
                            quote: Pk::ZERO,
                            token_program: if v2 { *TOKEN_2022 } else { *TOKEN },
                            vsol: 30_000_000_000,
                            vtok: 1_073_000_000_000_000,
                            rtok: CURVE_SUPPLY,
                            supply: TOTAL_SUPPLY,
                            creator_fee_bps: 0,
                            holder_reward: false,
                        }));
                    }
                }
            }
        } else if is_amm_trade_ix(&dsc) {
            *last_amm_ix = Some(ix);
        }
    }

    fn pump_event(&self, tx: &RawTx, ev: [u8; 8], body: &[u8], ctx: &TxCtx, last_ix: Option<&RawIx>, out: &mut Decoded) {
        if ev == EV_TRADE {
            let Some(t) = pump::decode_trade(body) else { return };
            let upos = tx.keys.iter().position(|k| *k == t.user);
            let post_sol = upos.and_then(|p| tx.post.get(p)).copied().unwrap_or(0);
            if self.capture_templates {
                if let (Some(ix), Some(p)) = (last_ix, upos) {
                    let dsc = disc(&ix.data);
                    let kind_ok = if t.is_buy { is_pump_buy_ix(&dsc) } else { is_pump_sell_ix(&dsc) };
                    if kind_ok && tx.is_signer(p) {
                        if let Some(tm) = self.template(tx, ix, Venue::Curve, t.is_buy, t.user, t.mint, Pk::ZERO, t.creator) {
                            out.templates.push(Arc::new(tm));
                        }
                    }
                }
            }
            out.events.push(Event::Trade(TradeEv {
                slot: tx.slot,
                idx: tx.idx,
                ts: tx.ts,
                sig: tx.sig,
                mint: t.mint,
                user: t.user,
                buy: t.is_buy,
                sol: t.sol,
                tok: t.tokens,
                vsol: t.vsol,
                vtok: t.vtok,
                rsol: t.rsol,
                rtok: t.rtok,
                fee_bps: t.fee_bps.min(u16::MAX as u64) as u16,
                cfee_bps: t.creator_fee_bps.min(u16::MAX as u64) as u16,
                creator: t.creator,
                mayhem: t.mayhem,
                quote: t.quote_mint,
                cu_price: ctx.cu_price,
                cu_limit: ctx.cu_limit,
                prio: ctx.prio,
                tip: ctx.tip,
                post_sol,
                payer: tx.keys.first().copied().unwrap_or_default(),
                ix: t.ix,
            }));
        } else if ev == EV_CREATE {
            let Some(c) = pump::decode_create_event(body) else { return };
            out.events.push(Event::Create(CreateEv {
                slot: tx.slot,
                idx: tx.idx,
                ts: tx.ts,
                sig: tx.sig,
                mint: c.mint,
                creator: c.creator,
                user: c.user,
                bonding_curve: c.bonding_curve,
                name: c.name,
                symbol: c.symbol,
                mayhem: c.mayhem,
                quote: c.quote_mint,
                token_program: c.token_program,
                vsol: c.vsol,
                vtok: c.vtok,
                rtok: c.rtok,
                supply: c.supply,
                creator_fee_bps: c.creator_fee_bps,
                holder_reward: c.holder_reward,
            }));
        } else if ev == EV_COMPLETE {
            if let Some(mint) = pump::decode_complete(body) {
                out.events.push(Event::Complete { slot: tx.slot, ts: tx.ts, mint });
            }
        } else if ev == EV_MIGRATION {
            if let Some(m) = pump::decode_migration(body) {
                out.events.push(Event::Migrate(MigrateEv {
                    slot: tx.slot,
                    idx: tx.idx,
                    ts: tx.ts,
                    mint: m.mint,
                    pool: m.pool,
                    base: m.mint_amount,
                    quote: m.sol_amount,
                }));
            }
        }
    }

    fn amm_event(&self, tx: &RawTx, ev: [u8; 8], body: &[u8], last_ix: Option<&RawIx>, out: &mut Decoded) {
        let is_buy = if ev == EV_AMM_BUY {
            true
        } else if ev == EV_AMM_SELL {
            false
        } else {
            return;
        };
        let Some(a) = pump::decode_amm_trade(is_buy, body) else { return };
        // pool -> (base_mint, quote_mint) from the instruction accounts: 0 pool, 3 base, 4 quote.
        let Some(ix) = last_ix.filter(|ix| ix.accounts.first().and_then(|i| tx.key(*i)) == Some(&a.pool)) else {
            return;
        };
        let (Some(base), Some(quote)) = (ix.accounts.get(3).and_then(|i| tx.key(*i)), ix.accounts.get(4).and_then(|i| tx.key(*i))) else {
            return;
        };
        if *quote != *WSOL {
            return;
        }
        if self.capture_templates {
            if let Some(p) = tx.keys.iter().position(|k| *k == a.user) {
                let dsc = disc(&ix.data);
                let kind_ok = if is_buy { dsc == IX_BUY || dsc == IX_AMM_BUY_EXACT_QUOTE_IN } else { dsc == IX_SELL };
                if kind_ok && tx.is_signer(p) {
                    if let Some(tm) = self.template(tx, ix, Venue::Amm, is_buy, a.user, *base, a.pool, a.coin_creator) {
                        out.templates.push(Arc::new(tm));
                    }
                }
            }
        }
        out.events.push(Event::Amm(AmmEv {
            slot: tx.slot,
            idx: tx.idx,
            ts: tx.ts,
            sig: tx.sig,
            pool: a.pool,
            mint: *base,
            user: a.user,
            buy: is_buy,
            base: a.base,
            quote: a.quote_user,
            pb: a.pool_base_after,
            pq: a.pool_quote_after,
            lp_bps: a.lp_bps.min(u16::MAX as u64) as u16,
            proto_bps: a.protocol_bps.min(u16::MAX as u64) as u16,
            cc_bps: a.creator_bps.min(u16::MAX as u64) as u16,
            coin_creator: a.coin_creator,
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn template(&self, tx: &RawTx, ix: &RawIx, venue: Venue, is_buy: bool, user: Pk, mint: Pk, pool: Pk, coin_creator: Pk) -> Option<IxTemplate> {
        let program = *tx.key(ix.program.min(255) as u8)?;
        let mut accounts = Vec::with_capacity(ix.accounts.len());
        for &a in &ix.accounts {
            let pk = *tx.key(a)?;
            accounts.push(AcctMeta { pk, signer: tx.is_signer(a as usize), writable: tx.is_writable(a as usize) });
        }
        Some(IxTemplate { venue, is_buy, program, accounts, data: ix.data.clone(), user, mint, slot: tx.slot, pool, coin_creator })
    }
}
