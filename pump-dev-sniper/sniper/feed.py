"""Real-time pump.fun event stream from a Solana websocket (`logsSubscribe` on
the pump program). Works with any RPC; use your own low-latency endpoint in
production (the public one is fine for paper trading but lags by a few slots).
"""
from __future__ import annotations

import asyncio
import json
import time
from typing import Awaitable, Callable

import websockets

from .pump import PUMP_PROGRAM, CreateEvent, TradeEvent, decode_program_data

OnCreate = Callable[[int, str, CreateEvent], Awaitable[None] | None]
OnTrade = Callable[[int, str, TradeEvent], Awaitable[None] | None]


class PumpFeed:
    def __init__(self, ws_url: str, commitment: str = "processed"):
        self.ws_url = ws_url
        self.commitment = commitment
        self.latest_slot = 0
        self.last_msg_at = 0.0
        self.notifications = 0
        self.reconnects = 0

    async def run(self, on_create: OnCreate, on_trade: OnTrade, stop: asyncio.Event) -> None:
        while not stop.is_set():
            try:
                async with websockets.connect(self.ws_url, max_size=2 ** 24, ping_interval=15,
                                              ping_timeout=30, open_timeout=15) as ws:
                    await ws.send(json.dumps({
                        "jsonrpc": "2.0", "id": 1, "method": "logsSubscribe",
                        "params": [{"mentions": [str(PUMP_PROGRAM)]}, {"commitment": self.commitment}],
                    }))
                    while not stop.is_set():
                        try:
                            raw = await asyncio.wait_for(ws.recv(), timeout=1.0)
                        except asyncio.TimeoutError:
                            continue
                        await self._handle(json.loads(raw), on_create, on_trade)
            except Exception as e:
                if stop.is_set():
                    break
                self.reconnects += 1
                print(f"[feed] websocket error ({e!r}); reconnecting")
                await asyncio.sleep(1)

    async def _handle(self, msg: dict, on_create: OnCreate, on_trade: OnTrade) -> None:
        params = msg.get("params")
        if not params:
            return
        res = params["result"]
        slot = res["context"]["slot"]
        val = res["value"]
        self.latest_slot = max(self.latest_slot, slot)
        self.last_msg_at = time.monotonic()
        self.notifications += 1
        if val.get("err"):
            return
        sig = val["signature"]
        for line in val["logs"]:
            ev = decode_program_data(line)
            if ev is None:
                continue
            cb = on_create(slot, sig, ev) if isinstance(ev, CreateEvent) else on_trade(slot, sig, ev)
            if asyncio.iscoroutine(cb):
                await cb
