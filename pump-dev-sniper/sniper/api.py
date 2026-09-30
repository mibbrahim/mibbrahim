"""Async clients for pump.fun's public HTTP APIs and Solana JSON-RPC, with
per-host rate limiting, 429 back-off and an on-disk cache for immutable data."""
from __future__ import annotations

import asyncio
import json
import os
import ssl
import time
from pathlib import Path
from typing import Any

import aiohttp

FRONTEND = "https://frontend-api-v3.pump.fun"
SWAP = "https://swap-api.pump.fun"
HEADERS = {"User-Agent": "Mozilla/5.0", "Origin": "https://pump.fun", "Accept": "application/json"}


def _ssl_context() -> ssl.SSLContext:
    ctx = ssl.create_default_context()
    for var in ("SSL_CERT_FILE", "REQUESTS_CA_BUNDLE"):
        if os.environ.get(var) and Path(os.environ[var]).exists():
            ctx.load_verify_locations(os.environ[var])
    return ctx


class RateLimiter:
    def __init__(self, per_second: float):
        self.interval = 1.0 / per_second
        self.next_at = 0.0
        self.lock = asyncio.Lock()

    async def wait(self) -> None:
        async with self.lock:
            now = time.monotonic()
            if self.next_at > now:
                await asyncio.sleep(self.next_at - now)
            self.next_at = max(now, self.next_at) + self.interval


class Http:
    def __init__(self, rps: dict[str, float] | None = None):
        self.rps = rps or {}
        self.limiters: dict[str, RateLimiter] = {}
        self.session: aiohttp.ClientSession | None = None

    async def __aenter__(self) -> "Http":
        self.session = aiohttp.ClientSession(
            trust_env=True,
            connector=aiohttp.TCPConnector(ssl=_ssl_context(), limit=32),
            timeout=aiohttp.ClientTimeout(total=20),
        )
        return self

    async def __aexit__(self, *exc) -> None:
        if self.session:
            await self.session.close()

    def _limiter(self, url: str) -> RateLimiter:
        host = url.split("/")[2]
        if host not in self.limiters:
            self.limiters[host] = RateLimiter(self.rps.get(host, 4.0))
        return self.limiters[host]

    async def request(self, method: str, url: str, *, params=None, json_body=None, tries: int = 10) -> Any:
        assert self.session is not None
        delay = 0.5
        for attempt in range(tries):
            await self._limiter(url).wait()
            try:
                headers = HEADERS if url.split("/")[2].endswith("pump.fun") else None
                async with self.session.request(method, url, params=params, json=json_body, headers=headers) as r:
                    text = await r.text()
                    if r.status == 429:
                        try:
                            delay = max(delay, json.loads(text).get("retryAfterMs", 500) / 1000)
                        except ValueError:
                            pass
                        await asyncio.sleep(delay)
                        delay = min(delay * 2, 10)
                        continue
                    if r.status >= 500:
                        raise aiohttp.ClientResponseError(r.request_info, (), status=r.status, message=text[:200])
                    if r.status >= 400:
                        raise RuntimeError(f"{r.status} {url}: {text[:200]}")
                    return json.loads(text)
            except (aiohttp.ClientError, asyncio.TimeoutError) as e:
                if attempt == tries - 1:
                    raise RuntimeError(f"{url}: {e}") from e
                await asyncio.sleep(delay)
                delay = min(delay * 2, 10)
        raise RuntimeError(f"{url}: rate limited after {tries} tries")

    async def get(self, url: str, **params) -> Any:
        return await self.request("GET", url, params={k: v for k, v in params.items() if v is not None})


class PumpApi:
    """pump.fun public API: coin listings, coins by creator, per-coin trades."""

    def __init__(self, http: Http, cache_dir: Path):
        self.http = http
        self.cache_dir = cache_dir
        (cache_dir / "trades").mkdir(parents=True, exist_ok=True)

    async def latest_coins(self, offset: int = 0, limit: int = 50) -> list[dict]:
        return await self.http.get(f"{FRONTEND}/coins", offset=offset, limit=limit, sort="created_timestamp",
                                   order="DESC", includeNsfw="true")

    async def coins_by_creator(self, creator: str, since_ms: int, max_coins: int = 200) -> list[dict]:
        """Coins created by `creator` since `since_ms`, newest first (at most max_coins)."""
        out: list[dict] = []
        offset, limit = 0, 100
        while len(out) < max_coins:
            page = await self.http.get(f"{FRONTEND}/coins", creator=creator, offset=offset, limit=limit,
                                       sort="created_timestamp", order="DESC", includeNsfw="true")
            if not page:
                break
            page = [c for c in page if c.get("creator") == creator]
            out += [c for c in page if c["created_timestamp"] >= since_ms]
            if not page or page[-1]["created_timestamp"] < since_ms:
                break
            offset += limit   # the API can return fewer rows than `limit` even when more exist
        return out[:max_coins]

    async def early_trades(self, mint: str, max_pages: int = 30) -> dict:
        """All trades of a coin, oldest first (paged newest->oldest). Cached on disk.

        Returns {"trades": [...], "complete": bool}; complete=False means the coin had
        more trades than max_pages*100 and the earliest ones were not reached.
        """
        path = self.cache_dir / "trades" / f"{mint}.json"
        if path.exists():
            return json.loads(path.read_text())
        trades: list[dict] = []
        cursor = None
        complete = False
        for _ in range(max_pages):
            d = await self.http.get(f"{SWAP}/v2/coins/{mint}/trades", limit=100, cursor=cursor)
            trades += d.get("trades", [])
            pg = d.get("pagination", {})
            if not pg.get("hasMore"):
                complete = True
                break
            cursor = pg.get("nextCursor")
        trades.sort(key=lambda t: t["slotIndexId"])
        res = {"trades": [_slim(t) for t in trades], "complete": complete}
        if complete:
            path.write_text(json.dumps(res))
        return res


def _slim(t: dict) -> dict:
    sid = t["slotIndexId"]
    return {
        "slot": int(sid[:12]),
        "idx": int(sid[12:]),
        "ts": t["timestamp"],
        "user": t["userAddress"],
        "buy": t["type"] == "buy",
        "sol": float(t.get("quoteAmount") or t.get("amountSol") or 0),
        "tok": float(t.get("baseAmount") or 0),
        "price": float(t["priceSol"]),
        "tx": t["tx"],
    }


class Rpc:
    def __init__(self, http: Http, url: str):
        self.http, self.url = http, url
        self._id = 0

    async def call(self, method: str, params: list) -> Any:
        self._id += 1
        d = await self.http.request("POST", self.url, json_body={"jsonrpc": "2.0", "id": self._id,
                                                                 "method": method, "params": params})
        if "error" in d:
            raise RuntimeError(f"rpc {method}: {d['error']}")
        return d["result"]
