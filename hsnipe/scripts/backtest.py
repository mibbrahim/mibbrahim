#!/usr/bin/env python3
"""Backtest: replay historical on-chain data through the SAME Rust rules (hsnipe replay).

Data comes from either
  * the live recorder (HS_RECORD=1 writes logs/<mode>/events-*.jsonl while the bot runs), or
  * an RPC backfill:  --fetch-hours 24 --rpc https://your-rpc   (runs `hsnipe backfill`)

Parameter grid: --grid "PB_PP_TP_PCT=0,0.5;PB_MAX_ENTRY_MCAP_SOL=0,150,200"
runs every combination (cartesian product) in parallel and prints a ranked table.

Examples
  backtest.py --events logs/powerful/events-*.jsonl --mode powerful
  backtest.py --fetch-hours 24 --rpc $HS_RPC_URL --out data/day.jsonl.gz --mode powerful
  backtest.py --events data/day.jsonl.gz --mode powerful \
      --grid "PB_GRAD_DELAY_MS=0,2000,5000;PB_BE_ARM_PCT=0,0.3" --delay 1
"""
import argparse
import glob
import itertools
import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_BIN = os.path.join(HERE, "..", "target", "release", "hsnipe")


def parse_grid(s):
    if not s:
        return [{}]
    axes = []
    for part in s.split(";"):
        part = part.strip()
        if not part:
            continue
        k, vals = part.split("=", 1)
        axes.append([(k.strip(), v.strip()) for v in vals.split(",")])
    return [dict(c) for c in itertools.product(*axes)]


def run_one(binary, files, mode, overrides, outdir, base_env):
    env = dict(os.environ)
    env.update(base_env)
    env.update(overrides)
    env["HS_MODE"] = mode
    env["HS_LOG_DIR"] = outdir
    env["HS_RECORD"] = "0"
    os.makedirs(outdir, exist_ok=True)
    p = subprocess.run([binary, "replay", *files], env=env, capture_output=True, text=True)
    if p.returncode != 0:
        return overrides, None, p.stderr[-2000:]
    with open(os.path.join(outdir, "summary.json")) as f:
        return overrides, json.load(f), None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--events", nargs="*", default=[], help="event files (globs ok)")
    ap.add_argument("--mode", default="powerful", choices=["powerful", "powerbot", "gymer"])
    ap.add_argument("--grid", default="", help='e.g. "PB_PP_TP_PCT=0,0.5;PB_MAX_ENTRY_MCAP_SOL=0,200"')
    ap.add_argument("--set", action="append", default=[], help="fixed override KEY=VALUE (repeatable)")
    ap.add_argument("--delay", type=int, default=1, help="landing-delay book used for ranking")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 2)
    ap.add_argument("--bin", default=DEFAULT_BIN)
    ap.add_argument("--outdir", default="backtests")
    ap.add_argument("--fetch-hours", type=float, help="backfill this many hours over RPC first")
    ap.add_argument("--rpc", default=os.environ.get("HS_RPC_URL", ""))
    ap.add_argument("--out", default="data/backfill.jsonl.gz", help="where the backfill writes")
    ap.add_argument("--concurrency", type=int, default=16)
    a = ap.parse_args()

    binary = os.path.abspath(a.bin)
    if not os.path.exists(binary):
        sys.exit(f"binary not found: {binary} (cargo build --release)")

    files = []
    for pat in a.events:
        files.extend(sorted(glob.glob(pat)) or [pat])
    if a.fetch_hours:
        if not a.rpc:
            sys.exit("--fetch-hours needs --rpc or HS_RPC_URL")
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        print(f"backfilling {a.fetch_hours}h from {a.rpc} -> {a.out} (this is ~{int(a.fetch_hours * 9000)} getBlock calls)")
        subprocess.run([binary, "backfill", "--hours", str(a.fetch_hours), "--rpc", a.rpc, "--out", a.out,
                        "--concurrency", str(a.concurrency)], check=True)
        files.append(a.out)
    if not files:
        sys.exit("no event files (use --events or --fetch-hours)")

    base_env = dict(kv.split("=", 1) for kv in a.set)
    combos = parse_grid(a.grid)
    print(f"{len(combos)} run(s) of {a.mode} over {len(files)} file(s), {a.jobs} in parallel")

    def job(i_c):
        i, c = i_c
        tag = "_".join(f"{k}-{v}" for k, v in c.items()) or "base"
        return run_one(binary, files, a.mode, c, os.path.join(a.outdir, a.mode, f"{i:03d}_{tag}"), base_env)

    results = []
    with ThreadPoolExecutor(max_workers=a.jobs) as ex:
        for overrides, summary, err in ex.map(job, enumerate(combos)):
            if err:
                print(f"  FAILED {overrides}: {err}")
                continue
            results.append((overrides, summary))

    rows = []
    for overrides, s in results:
        for b in s["paper"]:
            if b["delay"] != a.delay:
                continue
            rows.append((b["pnl_sol"], overrides, b, s))
    rows.sort(key=lambda r: -r[0])
    print(f"\nranked by P&L of the delay-{a.delay} book (size {results[0][1]['config']['paper']['size_sol'] if results else '?'} SOL):")
    hdr = f"  {'pnl_sol':>9} {'roi':>7} {'trades':>6} {'win%':>5} {'avg_win':>8} {'avg_loss':>8} {'open':>4}  overrides"
    print(hdr)
    print("  " + "-" * (len(hdr) + 10))
    for pnl, ov, b, s in rows:
        print(f"  {pnl:>+9.4f} {b['roi'] * 100:>6.1f}% {b['closed']:>6} {b['win_rate'] * 100:>4.0f}% "
              f"{b['avg_win_pct'] * 100:>+7.1f}% {b['avg_loss_pct'] * 100:>+7.1f}% {b['open']:>4}  "
              f"{' '.join(f'{k}={v}' for k, v in ov.items()) or '(defaults)'}")
    if results:
        s = results[0][1]
        print(f"\n  triggers {s['counters']['triggers']}, coins seen {s['counters']['creates']} "
              f"(non-SOL skipped {s['counters']['creates_non_sol']}), near misses {s['counters']['near_misses']}")
        print(f"  per-run logs under {os.path.join(a.outdir, a.mode)}/ — use scripts/report.py on any of them")


if __name__ == "__main__":
    main()
