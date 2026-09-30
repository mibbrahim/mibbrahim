#!/usr/bin/env python3
"""Reports over hsnipe logs (stdlib only).

  report.py trades   LOGDIR [--delay N] [--live] [--csv out.csv]   per-trade history with token addresses
  report.py stats    LOGDIR [--live]                               win rate / avg win / avg loss / P&L by landing delay,
                                                                   exit reason, entry market cap; profit-leak check
  report.py latency  LOGDIR                                        latency breakdown (decision, slot offset, send, confirm)
  report.py compare  LOGDIR                                        live vs paper on the same signals
  report.py near     LOGDIR                                        near misses by failed rule
  report.py all      LOGDIR

LOGDIR is the HS_LOG_DIR of a run (e.g. logs/powerful). Several dirs may be given, comma-separated.
"""
import argparse
import csv
import json
import os
import statistics
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone


def load(dirs, name):
    rows = []
    for d in dirs:
        p = os.path.join(d, name)
        if not os.path.exists(p):
            continue
        with open(p) as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        rows.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass
    return rows


def ts(ms):
    if not ms:
        return "-"
    return datetime.fromtimestamp(ms / 1000, tz=timezone.utc).strftime("%m-%d %H:%M:%S")


def pct(x):
    return f"{x * 100:+.1f}%"


def table(rows, headers):
    if not rows:
        print("  (none)")
        return
    widths = [max(len(str(h)), *(len(str(r[i])) for r in rows)) for i, h in enumerate(headers)]
    print("  " + "  ".join(str(h).ljust(w) for h, w in zip(headers, widths)))
    print("  " + "  ".join("-" * w for w in widths))
    for r in rows:
        print("  " + "  ".join(str(c).ljust(w) for c, w in zip(r, widths)))


def closes(dirs, live=False):
    return load(dirs, "live_closes.jsonl" if live else "closes.jsonl")


def cmd_trades(dirs, a):
    rows = closes(dirs, a.live)
    if a.delay is not None and not a.live:
        rows = [r for r in rows if r.get("delay") == a.delay]
    rows.sort(key=lambda r: r.get("entry_ts", 0))
    out = []
    for r in rows:
        out.append([
            ts(r.get("entry_ts")), r.get("delay", "live"), r["mint"], (r.get("symbol") or "")[:10],
            f"{r.get('hold_s', 0):.0f}s", f"{r.get('entry_mcap_sol', 0):.0f}", pct(r.get("peak_pct", 0)),
            pct(r.get("low_pct", 0)), f"{r.get('pnl_sol', 0):+.4f}", pct(r.get("pnl_pct", 0)),
            ">".join(r.get("exit_path", [])),
        ])
    print(f"== trades ({'live' if a.live else 'paper'}) ==")
    table(out, ["entry(UTC)", "delay", "mint", "symbol", "hold", "mcap", "peak", "low", "pnl_sol", "pnl%", "exits"])
    if a.csv:
        with open(a.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["entry_ts", "exit_ts", "delay", "mint", "symbol", "name", "entry_slot", "exit_slot", "hold_s",
                        "cost_sol", "proceeds_sol", "pnl_sol", "pnl_pct", "peak_pct", "low_pct", "entry_mcap_sol",
                        "exit_reason", "exit_path", "solscan"])
            for r in rows:
                w.writerow([r.get("entry_ts"), r.get("exit_ts"), r.get("delay", "live"), r["mint"], r.get("symbol"),
                            r.get("name"), r.get("entry_slot"), r.get("exit_slot"), r.get("hold_s"), r.get("cost_sol"),
                            r.get("proceeds_sol"), r.get("pnl_sol"), r.get("pnl_pct"), r.get("peak_pct"),
                            r.get("low_pct"), r.get("entry_mcap_sol"), r.get("exit_reason"),
                            ">".join(r.get("exit_path", [])), f"https://solscan.io/token/{r['mint']}"])
        print(f"  wrote {a.csv}")


def agg(rows):
    n = len(rows)
    wins = [r["pnl_pct"] for r in rows if r.get("pnl_sol", 0) > 0]
    losses = [r["pnl_pct"] for r in rows if r.get("pnl_sol", 0) <= 0]
    pnl = sum(r.get("pnl_sol", 0) for r in rows)
    cost = sum(r.get("cost_sol", 0) for r in rows)
    return [
        n,
        f"{len(wins) / n * 100:.0f}%" if n else "-",
        pct(statistics.mean(wins)) if wins else "-",
        pct(statistics.mean(losses)) if losses else "-",
        f"{pnl:+.4f}",
        pct(pnl / cost) if cost else "-",
        f"{statistics.median([r.get('hold_s', 0) for r in rows]):.0f}s" if n else "-",
    ]


HDR = ["trades", "win%", "avg_win", "avg_loss", "pnl_sol", "roi", "med_hold"]


def cmd_stats(dirs, a):
    rows = closes(dirs, a.live)
    label = "live" if a.live else "paper"
    print(f"== {label}: by landing delay ==")
    by = defaultdict(list)
    for r in rows:
        by[r.get("delay", "live")].append(r)
    table([[k] + agg(v) for k, v in sorted(by.items(), key=lambda x: str(x[0]))], ["delay"] + HDR)

    print(f"\n== {label}: by exit reason (per delay) ==")
    by = defaultdict(list)
    for r in rows:
        by[(r.get("delay", "live"), r.get("exit_reason", "?"))].append(r)
    table([[d, e] + agg(v) for (d, e), v in sorted(by.items(), key=lambda x: (str(x[0][0]), x[0][1]))], ["delay", "exit"] + HDR)

    print(f"\n== {label}: by entry market cap (SOL) ==")
    buckets = [(0, 50), (50, 80), (80, 120), (120, 160), (160, 200), (200, 300), (300, 1e9)]
    by = defaultdict(list)
    for r in rows:
        m = r.get("entry_mcap_sol", 0)
        b = next(f"{lo:.0f}-{hi:.0f}" if hi < 1e9 else f"{lo:.0f}+" for lo, hi in buckets if lo <= m < hi)
        by[(r.get("delay", "live"), b)].append(r)
    table([[d, b] + agg(v) for (d, b), v in sorted(by.items(), key=lambda x: (str(x[0][0]), float(x[0][1].split("-")[0].rstrip("+"))))],
          ["delay", "mcap"] + HDR)

    print(f"\n== {label}: profit leak (peaked +30..+100% then closed at a loss) ==")
    leak = [r for r in rows if 0.30 <= r.get("peak_pct", 0) < 1.0 and r.get("pnl_sol", 0) < 0]
    by = defaultdict(list)
    for r in leak:
        by[r.get("delay", "live")].append(r)
    table([[k, len(v), f"{sum(x['pnl_sol'] for x in v):+.4f}", pct(statistics.mean([x['peak_pct'] for x in v]))]
           for k, v in sorted(by.items(), key=lambda x: str(x[0]))], ["delay", "trades", "pnl_sol", "avg_peak"])

    big = [r for r in rows if r.get("pnl_pct", 0) <= -0.7]
    if big:
        print(f"\n== {label}: blow-ups (<= -70%): {len(big)} — check bundle features ==")
        table([[r.get("delay", "live"), r["mint"], pct(r["pnl_pct"]), r.get("exit_reason"),
                r.get("features", {}).get("fee_cluster_share"), r.get("features", {}).get("bundle_groups"),
                r.get("features", {}).get("drained_share")] for r in big[:30]],
              ["delay", "mint", "pnl", "exit", "fee_cluster", "groups", "drained"])


def q(vals, p):
    vals = sorted(v for v in vals if v is not None)
    if not vals:
        return "-"
    return f"{vals[min(len(vals) - 1, int(round((len(vals) - 1) * p)))]:.1f}"


def cmd_latency(dirs, a):
    trig = load(dirs, "triggers.jsonl")
    legs = [l for l in load(dirs, "live_legs.jsonl") if l.get("kind", "").startswith(("buy", "sell"))]
    rows = []

    def add(name, vals, unit):
        vals = [v for v in vals if v is not None]
        rows.append([name, unit, len(vals), q(vals, 0.5), q(vals, 0.9), q(vals, 0.99), q(vals, 1.0)])

    add("gRPC recv -> decision", [t.get("decision_us") for t in trig], "us")
    add("signal ms into its slot", [t.get("ms_into_slot") for t in trig], "ms")
    add("decision -> signed tx", [l.get("decision_to_signed_us") for l in legs], "us")
    for route in ("staked", "rpc", "jito"):
        add(f"send ack ({route})", [x["ms"] for l in legs for x in l.get("acks", []) if x.get("route") == route], "ms")
    add("send -> confirmation (processed)", [l.get("send_to_confirm_ms") for l in legs], "ms")
    add("landed slot - signal slot (buys)", [l.get("slot_delta") for l in legs if l.get("kind") == "buy" and l.get("ok")], "slots")
    add("landed slot - signal slot (sells)", [l.get("slot_delta") for l in legs if str(l.get("kind", "")).startswith("sell") and l.get("ok")], "slots")
    print("== latency ==")
    table(rows, ["step", "unit", "n", "p50", "p90", "p99", "max"])
    buys = [l for l in legs if l.get("kind") == "buy" and l.get("ok")]
    if buys:
        c = Counter(l.get("slot_delta") for l in buys)
        print("\n  buy landing distribution (slots after signal): " + ", ".join(f"{k}:{v}" for k, v in sorted(c.items())))
        same = [l for l in buys if l.get("slot_delta") == 0]
        late = [l for l in buys if (l.get("slot_delta") or 0) > 0]
        if same and late:
            print(f"  ms into slot: same-slot p50 {q([l.get('ms_into_slot') for l in same], .5)} vs later p50 {q([l.get('ms_into_slot') for l in late], .5)}")
    s = os.path.join(dirs[0], "summary.json")
    if os.path.exists(s):
        lat = json.load(open(s)).get("latency", {})
        print(f"\n  server->client (gRPC created_at): {lat.get('server_to_client_ms')}")


def cmd_compare(dirs, a):
    live = closes(dirs, True)
    paper = closes(dirs, False)
    if not live:
        print("no live_closes.jsonl — run with HS_LIVE=1 first")
        return
    by_key = defaultdict(dict)
    for r in paper:
        by_key[(r["mint"], r.get("trigger"))][r["delay"]] = r
    delays = sorted({r["delay"] for r in paper})
    out = []
    diffs = defaultdict(list)
    for l in live:
        pm = by_key.get((l["mint"], l.get("trigger")), {})
        row = [ts(l.get("entry_ts")), l["mint"], (l.get("symbol") or "")[:8],
               l.get("entry_slot", 0) - l.get("signal_slot", 0), pct(l.get("pnl_pct", 0)), f"{l.get('wallet_pnl_sol', 0):+.4f}"]
        for d in delays:
            p = pm.get(d)
            row.append(pct(p["pnl_pct"]) if p else "-")
            if p:
                diffs[d].append(l.get("pnl_pct", 0) - p["pnl_pct"])
        row.append(">".join(l.get("exit_path", [])))
        out.append(row)
    print("== live vs paper (same trigger) ==")
    table(out, ["entry", "mint", "sym", "land_d", "live%", "wallet_sol"] + [f"paper d{d}" for d in delays] + ["live exits"])
    print("\n  mean (live - paper) pnl% by book: " + ", ".join(f"d{d}: {pct(statistics.mean(v))} (n={len(v)})" for d, v in diffs.items() if v))
    if diffs:
        best = min(diffs, key=lambda d: abs(statistics.mean(diffs[d])) if diffs[d] else 9)
        print(f"  closest paper book: delay {best}")
    tot_live = sum(l.get("wallet_pnl_sol", 0) for l in live)
    print(f"  live wallet P&L {tot_live:+.4f} SOL over {len(live)} positions")


def cmd_near(dirs, a):
    rows = load(dirs, "near_misses.jsonl")
    print(f"== near misses: {len(rows)} (passed every rule but one) ==")
    c = Counter(r["failed_rule"] for r in rows)
    table([[k, v] for k, v in c.most_common()], ["failed rule", "coins"])
    keys = {"volume": "vol_usd", "m1_sold": "m1_sold_frac", "stall": "stall_ratio", "top2": "top2_buy_share",
            "m1_buyers": "m1_buyers", "entry_mcap": "mcap_sol", "top10_band": "top10_share", "small_buys": "small_buy_share",
            "snipers": "sniper_pct", "creation_slot_buys": "creation_slot_buys", "bundle_fee": "fee_cluster_share",
            "bundle_groups": "bundle_groups", "bundle_drained": "drained_share", "dev_pct": "dev_pct"}
    out = []
    for rule, _ in c.most_common():
        k = keys.get(rule)
        if not k:
            continue
        vals = [r["features"].get(k) for r in rows if r["failed_rule"] == rule]
        out.append([rule, k, q(vals, 0.1), q(vals, 0.5), q(vals, 0.9)])
    if out:
        print("\n  value of the failing feature (tune thresholds from these):")
        table(out, ["rule", "feature", "p10", "p50", "p90"])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["trades", "stats", "latency", "compare", "near", "all"])
    ap.add_argument("logdir")
    ap.add_argument("--delay", type=int)
    ap.add_argument("--live", action="store_true")
    ap.add_argument("--csv")
    a = ap.parse_args()
    dirs = a.logdir.split(",")
    if a.cmd == "all":
        for f in (cmd_stats, cmd_latency, cmd_near):
            f(dirs, a)
            print()
        if load(dirs, "live_closes.jsonl"):
            cmd_compare(dirs, a)
        return
    {"trades": cmd_trades, "stats": cmd_stats, "latency": cmd_latency, "compare": cmd_compare, "near": cmd_near}[a.cmd](dirs, a)


if __name__ == "__main__":
    sys.exit(main())
