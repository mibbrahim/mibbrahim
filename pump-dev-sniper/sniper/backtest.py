"""Walk-forward backtest of the snipe strategy on the serial-dev dataset.

For every dev, coins are split chronologically: the older 60% (train) are used
to pick parameters and decide which devs to follow, and the newer 40% (test)
show how that choice would have done on coins it never saw. Only the test
numbers are an honest estimate.

    python -m sniper.backtest            # grid search + watchlist.json
    python -m sniper.backtest --tip 0.002 --buy 0.05
"""
from __future__ import annotations

import argparse
import itertools
import json
from dataclasses import replace
from pathlib import Path

from .record import load_stream
from .sim import Params, TradeResult, simulate_coin, summarize

DATA = Path(__file__).resolve().parent.parent / "data"


def load(dataset_path: Path) -> list[dict]:
    ds = json.loads(dataset_path.read_text())
    devs = []
    for d in ds["devs"]:
        coins = []
        for c in sorted(d["coins"], key=lambda c: c["created_timestamp"]):
            f = DATA / "cache" / "trades" / f"{c['mint']}.json"
            if f.exists():
                tr = json.loads(f.read_text())["trades"]
                if tr:
                    coins.append({"mint": c["mint"], "created": c["created_timestamp"], "trades": tr,
                                  "fee_bps": c.get("fee_bps")})
        if coins:
            devs.append({**{k: v for k, v in d.items() if k != "coins"}, "coins": coins})
    return devs


def run(devs: list[dict], p: Params, part: str, split: float) -> dict[str, list[TradeResult]]:
    out = {}
    for d in devs:
        coins = d["coins"]
        cut = int(len(coins) * split)
        sel = coins if part == "all" else coins[:cut] if part == "train" else coins[cut:]
        out[d["creator"]] = [simulate_coin(c["mint"], d["creator"], c["trades"],
                                           replace(p, fee_bps=c["fee_bps"]) if c["fee_bps"] else p) for c in sel]
    return out


def select_devs(res: dict[str, list[TradeResult]], min_trades: int, min_win: float) -> set[str]:
    keep = set()
    for cr, rs in res.items():
        s = summarize(rs)
        if s["trades"] >= min_trades and s["pnl_sol"] > 0 and s["win_rate"] >= min_win:
            keep.add(cr)
    return keep


def pooled(res: dict[str, list[TradeResult]], devs: set[str] | None = None) -> dict:
    rs = [r for cr, lst in res.items() if devs is None or cr in devs for r in lst]
    return summarize(rs)


def fmt(s: dict) -> str:
    return (f"trades {s['trades']:5d}  win {s['win_rate']:6.1%}  avg {s['avg_pnl_pct']:+7.2%}  "
            f"total {s['pnl_sol']:+8.4f} SOL")


def eval_stream(path: Path, p: Params, watch: set[str]) -> None:
    """Forward test: apply a watchlist chosen on older data to launches recorded afterwards."""
    coins = load_stream(path)
    res_w, res_all = [], []
    for mint, c in coins.items():
        if not c["trades"] and c["create_slot"] is None:
            continue
        r = simulate_coin(mint, c["creator"], c["trades"], replace(p, fee_bps=c["fee_bps"]), c["create_slot"])
        res_all.append(r)
        if c["creator"] in watch:
            res_w.append(r)
    print(f"\n== forward test on recorded stream ({len(coins)} launches in {path})")
    print(f"  watchlisted devs: launches {len(res_w):4d} | {fmt(summarize(res_w))}")
    print(f"  every launch    : launches {len(res_all):4d} | {fmt(summarize(res_all))}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dataset", type=Path, default=DATA / "dataset.json")
    ap.add_argument("--buy", type=float, default=0.05, help="SOL per trade")
    ap.add_argument("--tip", type=float, default=0.001, help="priority fee + Jito tip on the buy (SOL)")
    ap.add_argument("--sell-tip", type=float, default=0.0003)
    ap.add_argument("--fee-bps", type=int, default=125, help="fallback when a coin's on-chain fee is unknown")
    ap.add_argument("--split", type=float, default=0.6)
    ap.add_argument("--min-dev-trades", type=int, default=4)
    ap.add_argument("--min-dev-win", type=float, default=0.5)
    ap.add_argument("--quick", action="store_true", help="smaller grid")
    ap.add_argument("--stream", type=Path, help="recorded stream (file or dir) to forward-test the watchlist on")
    ap.add_argument("--stream-only", action="store_true", help="skip the grid; forward-test data/watchlist.json")
    args = ap.parse_args()

    if args.stream_only:
        w = json.loads((DATA / "watchlist.json").read_text())
        eval_stream(args.stream, Params(**w["params"]), {d["creator"] for d in w["devs"]})
        return

    devs = load(args.dataset)
    n_coins = sum(len(d["coins"]) for d in devs)
    print(f"dataset: {len(devs)} devs, {n_coins} coins with full early-trade history\n")
    base = Params(buy_sol=args.buy, buy_tip_sol=args.tip, sell_tip_sol=args.sell_tip, fee_bps=args.fee_bps)
    print(f"round-trip fixed cost at {args.buy} SOL: pump fee {2 * args.fee_bps / 100:.2f}% + tips/sig "
          f"{(args.tip + args.sell_tip + 2 * base.sig_fee_sol) / args.buy:.2%} "
          f"=> price must rise ~{2 * args.fee_bps / 10_000 + (args.tip + args.sell_tip) / args.buy:.1%} just to break even\n")

    grid = dict(
        max_entry_pump=[0.3, 1.0, 100.0] if args.quick else [0.3, 0.6, 1.5, 100.0],
        entry_slot_offset=[1, 2],
        take_profit=[0.02, 0.05, 0.10] if args.quick else [0.02, 0.05, 0.10, 0.20],
        stop_loss=[0.10, 0.25] if args.quick else [0.05, 0.15, 0.35],
        max_hold_slots=[10, 40] if args.quick else [5, 25, 100],
    )
    rows = []
    for combo in itertools.product(*grid.values()):
        p = replace(base, **dict(zip(grid.keys(), combo)))
        train = run(devs, p, "train", args.split)
        test = run(devs, p, "test", args.split)
        chosen = select_devs(train, args.min_dev_trades, args.min_dev_win)
        rows.append({
            "params": p, "chosen": chosen,
            "all_train": pooled(train), "all_test": pooled(test),
            "sel_train": pooled(train, chosen), "sel_test": pooled(test, chosen),
        })

    print("== baseline: snipe EVERY serial dev's coin, no dev selection — top 8 parameter sets")
    for r in sorted(rows, key=lambda r: r["all_train"]["pnl_sol"] + r["all_test"]["pnl_sol"], reverse=True)[:8]:
        print(f"  {r['params'].label():28s} train {fmt(r['all_train'])} | test {fmt(r['all_test'])}")
    best = max(rows, key=lambda r: r["sel_train"]["pnl_sol"])
    print("\n== walk-forward: params + dev list chosen on TRAIN, scored on unseen TEST coins")
    for r in sorted(rows, key=lambda r: r["sel_train"]["pnl_sol"], reverse=True)[:8]:
        print(f"  {r['params'].label():28s} devs {len(r['chosen']):3d} | train {fmt(r['sel_train'])} | "
              f"TEST {fmt(r['sel_test'])}")

    p = best["params"]
    print(f"\nchosen: {p.label()}  ->  TEST: {fmt(best['sel_test'])}")
    print(f"        exit reasons on test: {best['sel_test']['exit_reasons']}")

    # Optimistic bound: we land first in the target block and our sells land instantly.
    opt = replace(p, entry_position="start", exit_latency_slots=0)
    opt_test = pooled(run(devs, opt, "test", args.split), best["chosen"])
    print(f"optimistic (first in block, zero-latency exits) TEST: {fmt(opt_test)}")

    # Final watchlist: re-select on the full history with the chosen params.
    full = run(devs, p, "all", args.split)
    keep = select_devs(full, args.min_dev_trades, args.min_dev_win)
    by_dev = {d["creator"]: d for d in devs}
    watch = []
    for cr in keep:
        s = summarize(full[cr])
        d = by_dev[cr]
        watch.append({"creator": cr, "launches_30d": d["launches"], "per_day": round(d["per_day"], 1),
                      "active_days": d["active_days"], "hours_since_last": round(d["hours_since_last"], 1),
                      "bt_trades": s["trades"], "bt_win_rate": round(s["win_rate"], 3),
                      "bt_avg_pnl_pct": round(s["avg_pnl_pct"], 4), "bt_pnl_sol": round(s["pnl_sol"], 5)})
    watch.sort(key=lambda w: w["bt_pnl_sol"], reverse=True)
    per_day = sum(w["per_day"] for w in watch)
    test_avg = best["sel_test"]["avg_pnl_sol"]
    print(f"\nwatchlist: {len(watch)} devs, ~{per_day:.0f} launches/day between them "
          f"=> at the TEST average of {test_avg:+.5f} SOL/trade that is ~{per_day * test_avg:+.3f} SOL/day")
    for w in watch[:25]:
        print(f"  {w['creator']}  {w['per_day']:5.1f}/day  bt {w['bt_trades']:3d} trades  "
              f"win {w['bt_win_rate']:.0%}  avg {w['bt_avg_pnl_pct']:+.2%}")

    (DATA / "watchlist.json").write_text(json.dumps({"params": vars(p), "devs": watch}, indent=1))
    report = {
        "dataset": {"devs": len(devs), "coins": n_coins},
        "chosen_params": vars(p),
        "test": best["sel_test"], "train": best["sel_train"], "optimistic_test": opt_test,
        "grid": [{"label": r["params"].label(), "devs": len(r["chosen"]), "sel_train": r["sel_train"],
                  "sel_test": r["sel_test"], "all_train": r["all_train"], "all_test": r["all_test"]} for r in rows],
        "watchlist_size": len(watch), "watchlist_launches_per_day": per_day,
    }
    (DATA / "backtest_report.json").write_text(json.dumps(report, indent=1))
    print(f"\nsaved data/watchlist.json and data/backtest_report.json")
    if args.stream:
        eval_stream(args.stream, p, keep)


if __name__ == "__main__":
    main()
