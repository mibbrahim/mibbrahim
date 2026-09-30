"""Live sniper. Same strategy as backtest/paper, but sends real transactions.

    # 1) verify the transaction layout on mainnet without spending anything
    python -m sniper.live selftest
    # 2) go live with hard limits (defaults: 0.05 SOL/trade, stop after -0.15 SOL)
    PRIVATE_KEY=... RPC_URL=... WS_URL=... python -m sniper.live run --minutes 30

Safety: every buy caps the entry price (min_tokens_out), the session stops on a
loss limit or wallet balance floor, and creating a file named STOP in data/
halts new entries immediately (open positions are still sold).
"""
from __future__ import annotations

import argparse
import asyncio
import base64
import json
import os
import random
import time
from dataclasses import dataclass, field, replace
from pathlib import Path

import base58
from solders.compute_budget import set_compute_unit_limit, set_compute_unit_price
from solders.hash import Hash
from solders.keypair import Keypair
from solders.message import MessageV0
from solders.pubkey import Pubkey
from solders.signature import Signature
from solders.system_program import TransferParams, transfer
from solders.transaction import VersionedTransaction

from .api import Http, Rpc
from .feed import PumpFeed
from .pump import (LAMPORTS, SOL_QUOTE_MINTS, CoinKeys, CreateEvent, Curve, TradeEvent, ata, bonding_curve_pda,
                   ix_buy_exact_sol_in, ix_close_account, ix_create_ata_idempotent, ix_sell, quote_buy, quote_sell)
from .sim import Params

DATA = Path(__file__).resolve().parent.parent / "data"

JITO_TIP_ACCOUNTS = [Pubkey.from_string(k) for k in (
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5", "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY", "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh", "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL", "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
)]
BUY_CU, SELL_CU = 100_000, 75_000   # measured: buy+ATA ~87k CU, sell ~50k CU


def load_keypair() -> Keypair:
    if os.environ.get("PRIVATE_KEY"):
        return Keypair.from_bytes(base58.b58decode(os.environ["PRIVATE_KEY"].strip()))
    path = os.environ.get("KEYPAIR_PATH")
    if path:
        return Keypair.from_bytes(bytes(json.loads(Path(path).read_text())))
    raise SystemExit("set PRIVATE_KEY (base58) or KEYPAIR_PATH (solana-keygen json)")


@dataclass
class ExecConfig:
    rpc_url: str
    send_urls: list[str]
    tip_accounts: list[Pubkey]
    tip_in_tx: bool = True           # add a SOL transfer to a tip account (Jito / sender services)
    buy_priority_share: float = 0.3  # share of the buy tip spent as compute-unit price (rest = tip transfer)


class Executor:
    def __init__(self, http: Http, cfg: ExecConfig, payer: Keypair | None):
        self.http, self.cfg, self.payer = http, cfg, payer
        self.rpc = Rpc(http, cfg.rpc_url)
        self.blockhash: Hash | None = None

    async def refresh_blockhash_forever(self, stop: asyncio.Event) -> None:
        while not stop.is_set():
            try:
                r = await self.rpc.call("getLatestBlockhash", [{"commitment": "confirmed"}])
                self.blockhash = Hash.from_string(r["value"]["blockhash"])
            except RuntimeError as e:
                print(f"[exec] blockhash refresh failed: {e}")
            await asyncio.sleep(2)

    def _budget(self, cu: int, tip_sol: float) -> tuple[list, int]:
        """Split a tip budget into compute-unit price + an explicit tip transfer."""
        total = int(tip_sol * LAMPORTS)
        prio = int(total * self.cfg.buy_priority_share) if self.cfg.tip_in_tx else total
        ixs = [set_compute_unit_limit(cu), set_compute_unit_price(max(1, prio * 1_000_000 // cu))]
        return ixs, total - prio if self.cfg.tip_in_tx else 0

    def _tip_ix(self, payer: Pubkey, lamports: int):
        return transfer(TransferParams(from_pubkey=payer, to_pubkey=random.choice(self.cfg.tip_accounts),
                                       lamports=lamports))

    def build_buy(self, coin: CoinKeys, payer: Pubkey, spend: int, min_tokens: int, tip_sol: float) -> MessageV0:
        ixs, tip = self._budget(BUY_CU, tip_sol)
        ixs += [ix_create_ata_idempotent(payer, payer, coin.mint, coin.token_program),
                ix_buy_exact_sol_in(coin, payer, spend, min_tokens)]
        if tip:
            ixs.append(self._tip_ix(payer, tip))
        return MessageV0.try_compile(payer, ixs, [], self.blockhash or Hash.default())

    def build_sell(self, coin: CoinKeys, payer: Pubkey, tokens: int, min_sol: int, tip_sol: float,
                   close: bool = True) -> MessageV0:
        ixs, tip = self._budget(SELL_CU, tip_sol)
        ixs.append(ix_sell(coin, payer, tokens, min_sol))
        if close:
            ixs.append(ix_close_account(ata(payer, coin.mint, coin.token_program), payer, payer, coin.token_program))
        if tip:
            ixs.append(self._tip_ix(payer, tip))
        return MessageV0.try_compile(payer, ixs, [], self.blockhash or Hash.default())

    async def send(self, msg: MessageV0) -> str:
        tx = VersionedTransaction(msg, [self.payer])
        raw = base64.b64encode(bytes(tx)).decode()
        body = {"jsonrpc": "2.0", "id": 1, "method": "sendTransaction",
                "params": [raw, {"encoding": "base64", "skipPreflight": True, "maxRetries": 0}]}

        async def one(url: str):
            try:
                return await self.http.request("POST", url, json_body=body, tries=1)
            except RuntimeError as e:
                return {"error": str(e)}
        results = await asyncio.gather(*(one(u) for u in self.cfg.send_urls))
        errs = [r.get("error") for r in results if isinstance(r, dict) and r.get("error")]
        if len(errs) == len(results):
            print(f"[exec] every send endpoint rejected the tx: {errs}")
        return str(tx.signatures[0])

    async def simulate(self, msg: MessageV0) -> dict:
        tx = VersionedTransaction.populate(msg, [Signature.default()])
        return await self.rpc.call("simulateTransaction", [base64.b64encode(bytes(tx)).decode(), {
            "encoding": "base64", "sigVerify": False, "replaceRecentBlockhash": True, "commitment": "processed"}])

    async def balance(self, who: Pubkey) -> float:
        return (await self.rpc.call("getBalance", [str(who), {"commitment": "processed"}]))["value"] / LAMPORTS

    async def statuses(self, sigs: list[str]) -> list:
        return (await self.rpc.call("getSignatureStatuses", [sigs]))["value"] if sigs else []


# ------------------------------------------------------------------ live trading

@dataclass
class LivePos:
    coin: CoinKeys
    symbol: str
    create_slot: int
    fee_bps: int
    state: str = "buying"          # buying -> holding -> selling -> closed | failed
    buy_sig: str = ""
    buy_sent_slot: int = 0
    fill_slot: int = 0
    tokens: int = 0
    cost_sol: float = 0.0
    sell_sigs: list[str] = field(default_factory=list)
    sell_sent_slot: int = 0
    sell_attempts: int = 0
    reason: str = ""
    peak: float = -1.0
    pnl_sol: float = 0.0
    last_curve: Curve | None = None


class LiveTrader:
    def __init__(self, ex: Executor, p: Params, watch: dict, args):
        self.ex, self.p, self.watch, self.args = ex, p, watch, args
        self.me = ex.payer.pubkey()
        self.me_s = str(self.me)
        self.pos: dict[str, LivePos] = {}
        self.realized = 0.0
        self.trades = 0
        self.halted = ""
        self.feed: PumpFeed | None = None
        self.log = (DATA / "live_trades.jsonl").open("a")

    def _now(self) -> int:
        return self.feed.latest_slot if self.feed else 0

    def can_enter(self) -> str:
        if (DATA / "STOP").exists():
            return "STOP file present"
        if self.halted:
            return self.halted
        if sum(1 for q in self.pos.values() if q.state in ("buying", "holding", "selling")) >= self.args.max_open:
            return "max open positions"
        if self.trades >= self.args.max_trades:
            return "max trades reached"
        return ""

    async def on_create(self, slot: int, sig: str, ev: CreateEvent) -> None:
        if ev.creator not in self.watch or ev.is_mayhem or ev.quote_mint not in SOL_QUOTE_MINTS:
            return
        why = self.can_enter()
        lag = self._now() - slot
        if why:
            print(f"{time.strftime('%H:%M:%S')}  LAUNCH {ev.symbol} by watched dev — not entering: {why}")
            return
        if lag > self.p.entry_slot_offset + self.args.max_extra_lag:
            print(f"{time.strftime('%H:%M:%S')}  LAUNCH {ev.symbol} seen {lag} slots late — skipping")
            return
        coin = CoinKeys(Pubkey.from_string(ev.mint), Pubkey.from_string(ev.creator),
                        Pubkey.from_string(ev.token_program))
        spend = int(self.p.buy_sol * LAMPORTS)
        cap = Curve.from_price(2.7958993476234855e-08 * (1 + self.p.max_entry_pump))
        min_tokens = quote_buy(cap, spend, ev.fee_bps)
        msg = self.ex.build_buy(coin, self.me, spend, min_tokens, self.p.buy_tip_sol)
        q = LivePos(coin, ev.symbol, slot, ev.fee_bps, buy_sent_slot=self._now())
        self.pos[ev.mint] = q
        q.buy_sig = await self.ex.send(msg)
        self.trades += 1
        print(f"{time.strftime('%H:%M:%S')}  BUY SENT {ev.symbol[:12]} {ev.mint} (launch slot {slot}, seen +{lag}) "
              f"{q.buy_sig[:16]}…")

    async def on_trade(self, slot: int, sig: str, ev: TradeEvent) -> None:
        q = self.pos.get(ev.mint)
        if q is None or q.state in ("closed", "failed"):
            return
        q.last_curve = Curve(ev.v_sol, ev.v_tok)
        if ev.user == self.me_s:
            await self._own_fill(q, slot, ev)
            return
        if q.state != "holding":
            return
        val = quote_sell(q.last_curve, q.tokens, q.fee_bps) / LAMPORTS - self.p.sell_tip_sol - self.p.sig_fee_sol
        r = val / q.cost_sol - 1
        q.peak = max(q.peak, r)
        if r >= self.p.take_profit:
            await self._sell(q, "take_profit", min_sol=int(val * (1 - self.args.tp_slippage) * LAMPORTS))
        elif r <= -self.p.stop_loss:
            await self._sell(q, "stop_loss")
        elif self.p.exit_on_dev_sell and not ev.is_buy and ev.user == str(q.coin.creator):
            await self._sell(q, "dev_sell")

    async def _own_fill(self, q: LivePos, slot: int, ev: TradeEvent) -> None:
        if ev.is_buy:
            q.state, q.fill_slot, q.tokens = "holding", slot, ev.token_amount
            q.cost_sol = ev.sol_amount * (1 + q.fee_bps / 10_000) / LAMPORTS + self.p.buy_tip_sol + self.p.sig_fee_sol
            print(f"{time.strftime('%H:%M:%S')}    FILLED {q.symbol[:12]} blk+{slot - q.create_slot}  "
                  f"{q.tokens / 1e6:,.0f} tokens  cost {q.cost_sol:.5f} SOL")
        else:
            got = ev.sol_amount * (1 - q.fee_bps / 10_000) / LAMPORTS
            q.pnl_sol = got - self.p.sell_tip_sol * q.sell_attempts - self.p.sig_fee_sol * q.sell_attempts - q.cost_sol
            q.state = "closed"
            self.realized += q.pnl_sol
            print(f"{time.strftime('%H:%M:%S')}    SOLD   {q.symbol[:12]} {q.reason:11s} {q.pnl_sol / q.cost_sol:+7.2%} "
                  f"{q.pnl_sol:+.5f} SOL  held {slot - q.fill_slot} slots | session {self.realized:+.5f} SOL")
            self.log.write(json.dumps({"mint": str(q.coin.mint), "symbol": q.symbol, "reason": q.reason,
                                       "pnl_sol": q.pnl_sol, "cost_sol": q.cost_sol, "entry_blk": q.fill_slot - q.create_slot,
                                       "hold_slots": slot - q.fill_slot, "ts": time.time()}) + "\n")
            self.log.flush()
            if self.realized <= -self.args.max_loss:
                self.halted = f"session loss limit hit ({self.realized:+.4f} SOL)"
                print(f"!!! {self.halted} — no new entries")

    async def _sell(self, q: LivePos, reason: str, min_sol: int = 0) -> None:
        q.state, q.reason = "selling", q.reason or reason
        q.sell_attempts += 1
        q.sell_sent_slot = self._now()
        msg = self.ex.build_sell(q.coin, self.me, q.tokens, min_sol, self.p.sell_tip_sol)
        q.sell_sigs.append(await self.ex.send(msg))
        print(f"{time.strftime('%H:%M:%S')}    SELL SENT {q.symbol[:12]} ({reason}, attempt {q.sell_attempts})")

    async def housekeeping(self) -> None:
        now = self._now()
        for mint, q in list(self.pos.items()):
            if q.state == "buying" and now - q.buy_sent_slot > self.args.buy_timeout_slots:
                st = (await self.ex.statuses([q.buy_sig]))[0]
                if st is None or st.get("err"):
                    q.state = "failed"
                    print(f"{time.strftime('%H:%M:%S')}    BUY {'FAILED' if st else 'DID NOT LAND'} {q.symbol[:12]}"
                          f"{' ' + json.dumps(st['err']) if st else ''}")
            elif q.state == "holding" and now - q.fill_slot > self.p.max_hold_slots:
                await self._sell(q, "timeout")
            elif q.state == "selling" and now - q.sell_sent_slot > self.args.sell_retry_slots:
                # previous sell didn't land (or failed on slippage): retry, dropping the price floor after 2 tries
                await self._sell(q, q.reason, min_sol=0)

    def open_positions(self) -> int:
        return sum(1 for q in self.pos.values() if q.state in ("buying", "holding", "selling"))


async def run_live(args) -> None:
    kp = load_keypair()
    w = json.loads(Path(args.watchlist).read_text())
    p = replace(Params(**w["params"]), buy_sol=args.buy, buy_tip_sol=args.tip, sell_tip_sol=args.sell_tip)
    watch = {d["creator"]: d for d in w["devs"]}
    send_urls = [u for u in (os.environ.get("SEND_URLS", "").split(",")) if u] or [args.rpc]
    tips = [Pubkey.from_string(k) for k in os.environ.get("TIP_ACCOUNTS", "").split(",") if k] or JITO_TIP_ACCOUNTS
    async with Http({args.rpc.split("/")[2]: 20.0}) as http:
        ex = Executor(http, ExecConfig(args.rpc, send_urls, tips, tip_in_tx=not args.no_tip_transfer), kp)
        start_bal = await ex.balance(kp.pubkey())
        print(f"wallet {kp.pubkey()}  balance {start_bal:.4f} SOL  | watching {len(watch)} devs | {p.label()} "
              f"size {p.buy_sol} SOL | loss limit {args.max_loss} SOL | floor {args.balance_floor} SOL")
        if start_bal < args.balance_floor + p.buy_sol + 0.01:
            raise SystemExit("balance too low for the configured floor + one trade")
        trader = LiveTrader(ex, p, watch, args)
        feed = PumpFeed(args.ws)
        trader.feed = feed
        stop = asyncio.Event()
        tasks = [asyncio.create_task(feed.run(trader.on_create, trader.on_trade, stop)),
                 asyncio.create_task(ex.refresh_blockhash_forever(stop))]
        t_end = time.time() + args.minutes * 60
        last_bal = time.time()
        try:
            while time.time() < t_end or trader.open_positions():
                await asyncio.sleep(0.2)
                if time.time() >= t_end and not trader.halted:
                    trader.halted = "session time over"
                await trader.housekeeping()
                if time.time() - last_bal > 15:
                    last_bal = time.time()
                    bal = await ex.balance(kp.pubkey())
                    if bal < args.balance_floor and not trader.halted:
                        trader.halted = f"balance {bal:.4f} below floor {args.balance_floor}"
                        print(f"!!! {trader.halted}")
        finally:
            stop.set()
            await asyncio.gather(*tasks, return_exceptions=True)
            end_bal = await ex.balance(kp.pubkey())
            print(f"\n=== LIVE SESSION === trades sent {trader.trades}  realized {trader.realized:+.5f} SOL  "
                  f"wallet {start_bal:.4f} -> {end_bal:.4f} SOL ({end_bal - start_bal:+.5f})")


# ------------------------------------------------------------------ selftest

async def selftest(args) -> None:
    """Build buy+sell for a live coin and run them through simulateTransaction (no signature, no funds)."""
    async with Http() as http:
        ex = Executor(http, ExecConfig(args.rpc, [args.rpc], JITO_TIP_ACCOUNTS), None)
        r = await ex.rpc.call("getLatestBlockhash", [{"commitment": "confirmed"}])
        ex.blockhash = Hash.from_string(r["value"]["blockhash"])
        coins = await http.get("https://frontend-api-v3.pump.fun/coins", offset=0, limit=50,
                               sort="created_timestamp", order="DESC", includeNsfw="true")
        coin_row = next(c for c in coins if c.get("quote_mint") in SOL_QUOTE_MINTS and not c.get("mayhem_state")
                        and not c.get("complete"))
        mint = Pubkey.from_string(coin_row["mint"])
        acc = await ex.rpc.call("getAccountInfo", [str(bonding_curve_pda(mint)), {"encoding": "base64"}])
        data = base64.b64decode(acc["value"]["data"][0])
        creator = Pubkey.from_bytes(data[49:81])
        v_tok, v_sol = int.from_bytes(data[8:16], "little"), int.from_bytes(data[16:24], "little")
        mint_acc = await ex.rpc.call("getAccountInfo", [str(mint), {"encoding": "base64"}])
        token_program = Pubkey.from_string(mint_acc["value"]["owner"])
        coin = CoinKeys(mint, creator, token_program)
        payer = Pubkey.from_string(args.sim_wallet)
        spend = int(0.01 * LAMPORTS)
        est = quote_buy(Curve(v_sol, v_tok), spend, 125)
        print(f"coin {coin_row['symbol']} {mint}  creator {creator}  token program {token_program}")
        buy = ex.build_buy(coin, payer, spend, int(est * 0.9), 0.001)
        sell_ixs = ex.build_sell(coin, payer, int(est * 0.9), 0, 0.0003, close=False)
        # one transaction: buy then sell most of it back, so the sell is validated against real state too
        combined = MessageV0.try_compile(payer, [set_compute_unit_limit(300_000),
                                                 ix_create_ata_idempotent(payer, payer, mint, token_program),
                                                 ix_buy_exact_sol_in(coin, payer, spend, int(est * 0.9)),
                                                 ix_sell(coin, payer, int(est * 0.9), 0)], [], ex.blockhash)
        for name, msg in (("buy (with tip)", buy), ("buy+sell round trip", combined)):
            res = (await ex.simulate(msg))["value"]
            ok = res["err"] is None
            print(f"\n[{'PASS' if ok else 'FAIL'}] {name}: err={res['err']} units={res.get('unitsConsumed')} "
                  f"tx size={len(bytes(VersionedTransaction.populate(msg, [Signature.default()])))} bytes")
            for line in (res.get("logs") or [])[-8 if ok else -25:]:
                print("    ", line)
        print(f"\nsell tx size {len(bytes(VersionedTransaction.populate(sell_ixs, [Signature.default()])))} bytes")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    st = sub.add_parser("selftest")
    st.add_argument("--rpc", default=os.environ.get("RPC_URL", "https://api.mainnet-beta.solana.com"))
    st.add_argument("--sim-wallet", default="5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9",
                    help="any funded address, used only as the simulated fee payer")
    lv = sub.add_parser("run")
    lv.add_argument("--minutes", type=float, default=30)
    lv.add_argument("--rpc", default=os.environ.get("RPC_URL", "https://api.mainnet-beta.solana.com"))
    lv.add_argument("--ws", default=os.environ.get("WS_URL", "wss://api.mainnet-beta.solana.com"))
    lv.add_argument("--watchlist", default=str(DATA / "watchlist.json"))
    lv.add_argument("--buy", type=float, default=0.05)
    lv.add_argument("--tip", type=float, default=0.001)
    lv.add_argument("--sell-tip", type=float, default=0.0003)
    lv.add_argument("--no-tip-transfer", action="store_true", help="priority fee only, no tip transfer")
    lv.add_argument("--max-open", type=int, default=2)
    lv.add_argument("--max-trades", type=int, default=100)
    lv.add_argument("--max-loss", type=float, default=0.15, help="stop new entries after this session loss (SOL)")
    lv.add_argument("--balance-floor", type=float, default=0.30, help="stop new entries below this balance (SOL)")
    lv.add_argument("--max-extra-lag", type=int, default=1, help="skip launches seen later than entry offset + this")
    lv.add_argument("--buy-timeout-slots", type=int, default=8)
    lv.add_argument("--sell-retry-slots", type=int, default=4)
    lv.add_argument("--tp-slippage", type=float, default=0.03)
    args = ap.parse_args()
    asyncio.run(selftest(args) if args.cmd == "selftest" else run_live(args))


if __name__ == "__main__":
    main()
