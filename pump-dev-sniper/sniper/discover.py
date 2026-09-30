"""Find serial pump.fun devs and download the early trades of their recent coins.

Active serial launchers are over-represented among the newest launches, so we
sample the latest ~1-2k coins, look up every creator's 30-day launch history,
keep the ones that launch steadily, and fetch the first trades of their coins.

    python -m sniper.discover --days 30 --min-launches 8
"""
from __future__ import annotations

import argparse
import asyncio
import json
import time
from collections import Counter
from pathlib import Path

import base64

from solders.pubkey import Pubkey

from .api import FRONTEND, SWAP, Http, PumpApi, Rpc
from .pump import SOL_QUOTE_MINTS, bonding_curve_pda, curve_account_fee_bps

DATA = Path(__file__).resolve().parent.parent / "data"


def is_tradeable(c: dict) -> bool:
    """SOL-quoted bonding-curve coin without mayhem (agent-traded, different curve)."""
    return (c.get("quote_mint") in SOL_QUOTE_MINTS and c.get("program", "pump") == "pump"
            and not c.get("mayhem_state") and not c.get("is_banned"))


def host(url: str) -> str:
    return url.split("/")[2]


async def sample_creators(api: PumpApi, pages: int, extra: list[str]) -> Counter:
    counts: Counter = Counter()
    for i in range(pages):
        try:
            page = await api.latest_coins(offset=i * 50, limit=50)
        except RuntimeError as e:
            print(f"  listing stopped at offset {i * 50}: {e}")
            break
        if not page:
            break
        counts.update(c["creator"] for c in page if is_tradeable(c))
    for c in extra:
        counts[c] += 0
    return counts


async def creator_history(api: PumpApi, creator: str, since_ms: int, cache: Path, max_age_s: int) -> list[dict]:
    path = cache / f"{creator}.json"
    if path.exists():
        d = json.loads(path.read_text())
        if time.time() - d["fetched_at"] < max_age_s:
            return d["coins"]
    coins = await api.coins_by_creator(creator, since_ms)
    keep = [{k: c.get(k) for k in ("mint", "creator", "created_timestamp", "name", "symbol", "complete",
                                     "ath_market_cap", "quote_mint", "mayhem_state", "program", "token_program",
                                     "is_cashback_enabled", "last_trade_timestamp")} for c in coins]
    path.write_text(json.dumps({"fetched_at": time.time(), "coins": keep}))
    return keep


async def enrich_fees(rpc: Rpc, coins: list[dict]) -> None:
    """Read each coin's BondingCurve account for its real fee (creator fee is dev-configurable)."""
    for i in range(0, len(coins), 100):
        batch = coins[i:i + 100]
        keys = [str(bonding_curve_pda(Pubkey.from_string(c["mint"]))) for c in batch]
        try:
            accs = (await rpc.call("getMultipleAccounts", [keys, {"encoding": "base64"}]))["value"]
        except RuntimeError as e:
            print(f"      fee lookup failed ({e}); assuming default fees")
            continue
        for c, a in zip(batch, accs):
            if a:
                fee, mayhem, quote = curve_account_fee_bps(base64.b64decode(a["data"][0]))
                c["fee_bps"], c["onchain_mayhem"] = fee, mayhem


def dev_activity(coins: list[dict], now_ms: int) -> dict:
    ok = [c for c in coins if is_tradeable(c)]
    days = {int(c["created_timestamp"] // 86_400_000) for c in ok}
    last = max((c["created_timestamp"] for c in ok), default=0)
    return {"launches": len(ok), "active_days": len(days), "hours_since_last": (now_ms - last) / 3.6e6 if last else 1e9,
            "per_day": len(ok) / max(1, len(days))}


async def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--pages", type=int, default=40, help="pages of 50 latest coins to sample creators from")
    ap.add_argument("--max-creators", type=int, default=700)
    ap.add_argument("--min-launches", type=int, default=8)
    ap.add_argument("--min-active-days", type=int, default=3)
    ap.add_argument("--max-hours-since-last", type=float, default=48)
    ap.add_argument("--coins-per-dev", type=int, default=40, help="most recent coins per dev to download trades for")
    ap.add_argument("--max-devs", type=int, default=150)
    ap.add_argument("--extra-creators", type=Path, help="file with one creator address per line to include")
    ap.add_argument("--frontend-rps", type=float, default=1.0)
    ap.add_argument("--swap-rps", type=float, default=3.0)
    ap.add_argument("--rpc", default="https://api.mainnet-beta.solana.com")
    args = ap.parse_args()

    now_ms = int(time.time() * 1000)
    since_ms = now_ms - args.days * 86_400_000
    cache = DATA / "cache"
    (cache / "creators").mkdir(parents=True, exist_ok=True)
    extra = args.extra_creators.read_text().split() if args.extra_creators else []
    seen_file = DATA / "seen_creators.txt"
    if seen_file.exists():   # creators logged by the paper/live runner
        extra += seen_file.read_text().split()

    async with Http({host(FRONTEND): args.frontend_rps, host(SWAP): args.swap_rps}) as http:
        api = PumpApi(http, cache)
        print(f"[1/3] sampling creators from the latest {args.pages * 50} launches ...")
        counts = await sample_creators(api, args.pages, extra)
        creators = [c for c, _ in counts.most_common(args.max_creators)]
        print(f"      {len(counts)} creators seen, looking up {len(creators)} (30d history each)")

        devs = []
        sem = asyncio.Semaphore(4)

        async def look(cr: str) -> None:
            async with sem:
                try:
                    coins = await creator_history(api, cr, since_ms, cache / "creators", 6 * 3600)
                except RuntimeError as e:
                    print(f"      skip {cr[:8]}: {e}")
                    return
            a = dev_activity(coins, now_ms)
            # A dev at the 200-coin lookup cap may have all of them in the last few days: still active.
            steady = a["active_days"] >= args.min_active_days or a["launches"] >= 150
            if a["launches"] >= args.min_launches and steady and a["hours_since_last"] <= args.max_hours_since_last:
                devs.append({"creator": cr, **a, "coins": coins})

        done = 0
        for chunk in range(0, len(creators), 40):
            await asyncio.gather(*(look(c) for c in creators[chunk:chunk + 40]))
            done = min(len(creators), chunk + 40)
            print(f"      {done}/{len(creators)} looked up, {len(devs)} serial devs so far")

        devs.sort(key=lambda d: d["launches"], reverse=True)
        devs = devs[:args.max_devs]
        print(f"[2/3] {len(devs)} active serial devs (>= {args.min_launches} launches on >= {args.min_active_days} days "
              f"in {args.days}d, last launch <= {args.max_hours_since_last}h ago)")

        print(f"[3/3] downloading early trades for up to {args.coins_per_dev} coins per dev ...")
        dataset = []
        tasks = []
        for d in devs:
            recent = sorted((c for c in d["coins"] if is_tradeable(c)), key=lambda c: c["created_timestamp"],
                            reverse=True)[:args.coins_per_dev]
            for c in recent:
                tasks.append((d, c))
        sem2 = asyncio.Semaphore(6)
        stats = Counter()

        async def fetch(d: dict, c: dict) -> None:
            async with sem2:
                try:
                    tr = await api.early_trades(c["mint"], max_pages=8)
                except RuntimeError as e:
                    stats["error"] += 1
                    print(f"      trades {c['mint'][:8]} failed: {e}")
                    return
            stats["complete" if tr["complete"] else "too_many_trades"] += 1
            if tr["complete"]:
                c["n_trades"] = len(tr["trades"])

        for i in range(0, len(tasks), 60):
            await asyncio.gather(*(fetch(d, c) for d, c in tasks[i:i + 60]))
            print(f"      {min(len(tasks), i + 60)}/{len(tasks)} coins  {dict(stats)}")

        with_trades = [c for d in devs for c in d["coins"] if "n_trades" in c]
        print(f"      reading on-chain fee settings for {len(with_trades)} coins ...")
        await enrich_fees(Rpc(http, args.rpc), with_trades)
        for d in devs:
            coins = [c for c in d["coins"] if "n_trades" in c and not c.get("onchain_mayhem")]
            skipped = sum(1 for c in d["coins"] if is_tradeable(c)) - len(coins)
            dataset.append({k: v for k, v in d.items() if k != "coins"} | {"coins": coins, "skipped_big": skipped})

    out = DATA / "dataset.json"
    out.write_text(json.dumps({"built_at": now_ms, "days": args.days, "devs": dataset}, indent=1))
    print(f"saved {out} ({sum(len(d['coins']) for d in dataset)} coins from {len(dataset)} devs). "
          f"Coins skipped for >800 trades: {stats['too_many_trades']}")


if __name__ == "__main__":
    asyncio.run(main())
