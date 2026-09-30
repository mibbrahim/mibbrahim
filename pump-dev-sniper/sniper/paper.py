"""Live paper trading: watch pump.fun in real time and paper-snipe every new coin
launched by a watchlisted dev, using exactly the backtest's decision logic on
the live, slot-stamped trade stream. No transactions are sent.

    python -m sniper.paper --minutes 25
    python -m sniper.paper --minutes 25 --ws wss://your-rpc/ws
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import time
from collections import Counter
from dataclasses import replace
from pathlib import Path

from .feed import PumpFeed
from .pump import SOL_QUOTE_MINTS, CreateEvent, TradeEvent
from .sim import Params, Position, summarize

DATA = Path(__file__).resolve().parent.parent / "data"


def load_watchlist(path: Path) -> tuple[Params, dict[str, dict]]:
    w = json.loads(path.read_text())
    p = Params(**w["params"])
    return p, {d["creator"]: d for d in w["devs"]}


class PaperTrader:
    def __init__(self, params: Params, watch: dict[str, dict], all_devs: bool, log_path: Path):
        self.p = params
        self.watch = watch
        self.all_devs = all_devs
        self.open: dict[str, Position] = {}
        self.meta: dict[str, dict] = {}
        self.results = []
        self.creators_seen: Counter = Counter()
        self.watch_launches = 0
        self.detect_lags: list[int] = []
        self.log = log_path.open("a")
        self.feed: PumpFeed | None = None

    def on_create(self, slot: int, sig: str, ev: CreateEvent) -> None:
        if ev.is_mayhem or ev.quote_mint not in SOL_QUOTE_MINTS:
            return
        self.creators_seen[ev.creator] += 1
        if not (self.all_devs or ev.creator in self.watch):
            return
        self.watch_launches += 1
        lag = (self.feed.latest_slot - slot) if self.feed else 0
        self.detect_lags.append(lag)
        pos = Position(ev.mint, ev.creator, slot, replace(self.p, fee_bps=ev.fee_bps))
        pos.last_slot, pos.last_price = slot, pos.launch_price   # state right after the create
        self.open[ev.mint] = pos
        self.meta[ev.mint] = {"symbol": ev.symbol, "name": ev.name, "create_slot": slot, "sig": sig,
                              "fee_bps": ev.fee_bps, "seen_at": time.time(), "detect_lag_slots": lag}
        tag = "WATCHED DEV" if ev.creator in self.watch else "any dev"
        print(f"{time.strftime('%H:%M:%S')}  LAUNCH  {ev.symbol[:12]:12s} {ev.mint}  dev {ev.creator[:8]} ({tag}) "
              f"slot {slot}  fee {ev.fee_bps}bps")

    def on_trade(self, slot: int, sig: str, ev: TradeEvent) -> None:
        pos = self.open.get(ev.mint)
        if pos is None:
            return
        price = (ev.v_sol / 1e9) / (ev.v_tok / 1e6) if ev.v_tok else pos.last_price
        was = pos.state
        pos.on_trade(slot, price, ev.user, ev.is_buy)
        self._report(ev.mint, pos, was)

    def tick(self) -> None:
        if not self.feed:
            return
        now = self.feed.latest_slot - 2          # allow stragglers from the last couple of slots
        for mint, pos in list(self.open.items()):
            was = pos.state
            pos.tick(now)
            if pos.state == "waiting" and now > pos.create_slot + 400:
                pos.finish(now)
            self._report(mint, pos, was)

    def _report(self, mint: str, pos: Position, was: str) -> None:
        m = self.meta[mint]
        if was == "waiting" and pos.state in ("holding", "exiting"):
            print(f"{time.strftime('%H:%M:%S')}    BUY   {m['symbol'][:12]:12s} blk+{pos.entry_slot - pos.create_slot} "
                  f"@ {pos.entry_price:.3e}  {pos.tokens / 1e6:,.0f} tokens for {self.p.buy_sol} SOL")
        if pos.state == "closed":
            r = pos.result
            self.open.pop(mint, None)
            self.results.append(r)
            rec = {**r.to_dict(), **m, "closed_at": time.time()}
            self.log.write(json.dumps(rec) + "\n")
            self.log.flush()
            if r.entered:
                print(f"{time.strftime('%H:%M:%S')}    SELL  {m['symbol'][:12]:12s} {r.reason:12s} "
                      f"{r.pnl_pct:+7.2%}  {r.pnl_sol:+.5f} SOL  held {r.hold_slots} slots  peak {r.peak_pct:+.1%}")
            else:
                print(f"{time.strftime('%H:%M:%S')}    SKIP  {m['symbol'][:12]:12s} {r.reason}")

    def summary(self) -> str:
        s = summarize(self.results)
        return (f"launches by watched devs: {self.watch_launches} | closed trades: {s['trades']} "
                f"win {s['win_rate']:.0%} avg {s['avg_pnl_pct']:+.2%} total {s['pnl_sol']:+.5f} SOL | "
                f"open {len(self.open)}")


async def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--minutes", type=float, default=25)
    ap.add_argument("--ws", default=os.environ.get("WS_URL", "wss://api.mainnet-beta.solana.com"))
    ap.add_argument("--watchlist", type=Path, default=DATA / "watchlist.json")
    ap.add_argument("--all-devs", action="store_true", help="paper-snipe every new coin (baseline)")
    ap.add_argument("--log", type=Path, default=DATA / "paper_trades.jsonl")
    args = ap.parse_args()

    params, watch = load_watchlist(args.watchlist)
    print(f"paper trading {len(watch)} watched devs for {args.minutes} min  params: {params.label()}  "
          f"size {params.buy_sol} SOL  tip {params.buy_tip_sol}/{params.sell_tip_sol} SOL")
    trader = PaperTrader(params, watch, args.all_devs, args.log)
    feed = PumpFeed(args.ws)
    trader.feed = feed
    stop = asyncio.Event()
    task = asyncio.create_task(feed.run(trader.on_create, trader.on_trade, stop))
    t_end = time.time() + args.minutes * 60
    last_summary = time.time()
    try:
        while time.time() < t_end:
            await asyncio.sleep(0.4)
            trader.tick()
            if time.time() - last_summary > 60:
                last_summary = time.time()
                print(f"--- {trader.summary()} | feed {feed.notifications} msgs")
    finally:
        # let open positions run to completion for up to 60 s
        deadline = time.time() + 60
        while trader.open and time.time() < deadline:
            await asyncio.sleep(0.4)
            trader.tick()
        stop.set()
        await task
        for mint, pos in list(trader.open.items()):
            pos.finish(feed.latest_slot)
            trader._report(mint, pos, pos.state)
        seen = DATA / "seen_creators.txt"
        prev = set(seen.read_text().split()) if seen.exists() else set()
        seen.write_text("\n".join(sorted(prev | {c for c, n in trader.creators_seen.items() if n >= 2})))
        print(f"\n=== PAPER RESULT ({args.minutes} min) ===\n{trader.summary()}")
        print(f"trades logged to {args.log}")


if __name__ == "__main__":
    asyncio.run(main())
