"""Record every pump.fun launch and its first trades, slot-exact, from the
websocket feed. No HTTP API involved, so no rate limits: leave it running on
your VM and the backtest gets an ever-growing, exact dataset
(`python -m sniper.backtest --stream data/stream`).

    python -m sniper.record --minutes 60
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import time
from pathlib import Path

from .feed import PumpFeed
from .pump import SOL_QUOTE_MINTS, CreateEvent, TradeEvent

DATA = Path(__file__).resolve().parent.parent / "data"


class Recorder:
    def __init__(self, out_dir: Path, window_slots: int):
        self.out_dir = out_dir
        out_dir.mkdir(parents=True, exist_ok=True)
        self.window = window_slots
        self.tracked: dict[str, int] = {}     # mint -> create slot
        self.fh = None
        self.hour = ""
        self.creates = self.trades = 0

    def _write(self, rec: dict) -> None:
        hour = time.strftime("%Y%m%d-%H")
        if hour != self.hour:
            if self.fh:
                self.fh.close()
            self.fh = (self.out_dir / f"{hour}.jsonl").open("a")
            self.hour = hour
        self.fh.write(json.dumps(rec, separators=(",", ":")) + "\n")

    def on_create(self, slot: int, sig: str, ev: CreateEvent) -> None:
        if ev.is_mayhem or ev.quote_mint not in SOL_QUOTE_MINTS:
            return
        self.tracked[ev.mint] = slot
        self.creates += 1
        self._write({"t": "c", "slot": slot, "mint": ev.mint, "creator": ev.creator, "user": ev.user,
                     "sym": ev.symbol[:24], "fee": ev.fee_bps, "tp": ev.token_program, "ts": ev.timestamp})

    def on_trade(self, slot: int, sig: str, ev: TradeEvent) -> None:
        cs = self.tracked.get(ev.mint)
        if cs is None:
            return
        if slot - cs > self.window:
            del self.tracked[ev.mint]
            return
        self.trades += 1
        self._write({"t": "t", "slot": slot, "mint": ev.mint, "user": ev.user, "buy": ev.is_buy,
                     "sol": ev.sol_amount, "tok": ev.token_amount, "vs": ev.v_sol, "vt": ev.v_tok})


def load_stream(path: Path) -> dict[str, dict]:
    """Recorded files -> {mint: {"creator", "create_slot", "fee_bps", "trades": [...]}} (trades in API format)."""
    coins: dict[str, dict] = {}
    files = sorted(path.glob("*.jsonl")) if path.is_dir() else [path]
    for f in files:
        for line in f.open():
            try:
                r = json.loads(line)
            except ValueError:
                continue
            if r["t"] == "c":
                coins[r["mint"]] = {"creator": r["creator"], "create_slot": r["slot"], "fee_bps": r["fee"],
                                    "created": r["ts"] * 1000, "trades": []}
            elif r["mint"] in coins and r["vt"]:
                coins[r["mint"]]["trades"].append({
                    "slot": r["slot"], "user": r["user"], "buy": r["buy"],
                    "price": (r["vs"] / 1e9) / (r["vt"] / 1e6)})
    return coins


async def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--minutes", type=float, default=60)
    ap.add_argument("--ws", default=os.environ.get("WS_URL", "wss://api.mainnet-beta.solana.com"))
    ap.add_argument("--window-slots", type=int, default=300, help="record trades this long after launch (~2 min)")
    ap.add_argument("--out", type=Path, default=DATA / "stream")
    args = ap.parse_args()
    rec = Recorder(args.out, args.window_slots)
    feed = PumpFeed(args.ws)
    stop = asyncio.Event()
    task = asyncio.create_task(feed.run(rec.on_create, rec.on_trade, stop))
    t_end = time.time() + args.minutes * 60
    while time.time() < t_end:
        await asyncio.sleep(30)
        if rec.fh:
            rec.fh.flush()
        print(f"{time.strftime('%H:%M:%S')} recorded {rec.creates} launches, {rec.trades} early trades "
              f"(feed {feed.notifications} msgs, {feed.reconnects} reconnects)", flush=True)
    stop.set()
    await task
    if rec.fh:
        rec.fh.close()


if __name__ == "__main__":
    asyncio.run(main())
