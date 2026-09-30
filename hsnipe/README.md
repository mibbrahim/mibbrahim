# hsnipe — Powerful Bot, PowerBot, Gymer Bot

A low-latency pump.fun bot in Rust. One binary, three strategies (`HS_MODE=powerful|powerbot|gymer`),
paper trading on three landing-delay books, optional live trading for the Powerful Bot.

**Status:** paper trading is ready to run. Live trading is built and its transactions pass
mainnet simulation, but it has **not** been run with real funds. Paper test first, then
`HS_LIVE_SIMULATE=1`, then a tiny size.

## Quick start (paper)

```bash
# on the VPS (Rust 1.82+)
cargo build --release
cargo test --release                      # 14 tests, incl. real mainnet transactions

export HS_GRPC_URLS=https://your-kaldera-fra,https://your-kaldera-ny
export HS_GRPC_X_TOKEN=...
export HS_RPC_URL=https://your-constantk-rpc
HS_MODE=powerful ./target/release/hsnipe run      # logs -> logs/powerful/
```

For 24/7 runs use the systemd units (one per strategy):

```bash
sudo useradd -r -s /usr/sbin/nologin hsnipe
sudo mkdir -p /opt/hsnipe/bin /opt/hsnipe/{powerful,powerbot,gymer} /etc/hsnipe
sudo cp target/release/hsnipe /opt/hsnipe/bin/
sudo cp deploy/env/common.env.example /etc/hsnipe/common.env        # fill in endpoints + token
for m in powerful powerbot gymer; do sudo cp deploy/env/$m.env.example /etc/hsnipe/$m.env; done
sudo chown -R hsnipe: /opt/hsnipe && sudo chmod 600 /etc/hsnipe/*.env
sudo cp deploy/systemd/*.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now hsnipe-powerful hsnipe-powerbot hsnipe-gymer
```

## Commands

| command | what it does |
| --- | --- |
| `hsnipe run` | Yellowstone gRPC stream (processed) -> strategy -> paper books (+ live if `HS_LIVE=1`) |
| `hsnipe replay FILE...` | replays recorded or backfilled events through the same engine (used by backtests) |
| `hsnipe backfill --hours 24 --out F.jsonl.gz` | fetches history over RPC `getBlock`, decodes it with the live decoder |
| `hsnipe simtest [--slot S]` | clones real buys/sells from a mainnet block onto *other* wallets and runs `simulateTransaction`; no funds move |
| `hsnipe config` | prints the effective configuration |

## Reports and backtests (Python 3, stdlib only)

```bash
python3 scripts/report.py stats   logs/powerful   # win rate / avg win / avg loss / P&L by landing delay, exit, entry mcap
python3 scripts/report.py trades  logs/powerful --csv trades.csv   # per-trade history with token addresses
python3 scripts/report.py latency logs/powerful   # recv->decision, ms into slot, send, confirm, landed-slot delta
python3 scripts/report.py compare logs/powerful   # live vs paper on the same triggers
python3 scripts/report.py near    logs/powerful   # near misses by failed rule (for tuning)

# replay recorded data (the bot records every tracked event by default) with a parameter grid
python3 scripts/backtest.py --events 'logs/powerful/events-*.jsonl.gz' --mode powerful \
    --grid "PB_PP_TP_PCT=0,0.4,0.6;PB_MAX_ENTRY_MCAP_SOL=0,150,200;PB_GRAD_DELAY_MS=0,3000" --delay 1
# or fetch 24 h over RPC first (~216k getBlock calls; heavy)
python3 scripts/backtest.py --fetch-hours 24 --rpc $HS_RPC_URL --out data/day.jsonl.gz --mode powerful
```

## Logs (`HS_LOG_DIR`)

| file | content |
| --- | --- |
| `triggers.jsonl` | every entry signal: all features, `decision_us` (gRPC receive -> decision), `ms_into_slot` |
| `fills.jsonl` | every paper leg per book: signal slot, landing slot, tokens, SOL, fees, slippage vs signal |
| `closes.jsonl` | every closed paper trade: entry/exit slot + time, features, exit path, peak/low %, hold, P&L SOL and % |
| `near_misses.jsonl` | coins that passed every rule but one, with the rule and the features |
| `summary.json` | refreshed every 30 s: counters, rejects by rule, per-book stats, latency percentiles, stream health |
| `live_legs.jsonl`, `live_closes.jsonl` | live: every leg (signal vs landed slot, fill vs expected, fees, route acks, confirm time) |
| `events-*.jsonl.gz` | hourly recording of all tracked events, the input for `replay` / backtests |
| `stream.jsonl` | gRPC connects/disconnects |

## How paper fills work

* Three books per strategy: the transaction lands 0, 1 or 2 slots after the signal slot.
* `HS_PAPER_FILL_AT=end` (default) fills after every other trade in the landing slot, which is conservative.
  `start` fills before them (for delay 0: right after the trade that triggered the signal).
* Fills use the coin's real curve (or PumpSwap pool) reserves at that point and the exact fee split the
  programs use (checked lamport-for-lamport against mainnet `buy_exact_sol_in` trades), plus a fixed
  tx cost per leg (`HS_PAPER_*_COST_SOL`; emergency exits cost more).
* All exit percentages are **net P&L of the remaining tokens**: what selling them now would return
  after fees, versus what they cost including fees. A fresh position therefore starts at about −4%
  (pump fees both ways plus the buy's tx cost).
* Take-profit fractions are fractions of the *original* position. The trailing stop measures the drop from the peak.
* After graduation, fills use the PumpSwap pool. Its opening reserves come from pump's migration event.

## What was verified here

* Decoders against real mainnet data: `TradeEvent`, `CreateEvent`, migration, PumpSwap `BuyEvent`/`SellEvent`,
  v0 + v1 transactions. The gRPC path and the RPC/backfill path produce byte-identical events (unit test on 10 real txs).
* A 20-minute mainnet slice (2,993 blocks, 648 launches) replayed through all three strategies. Rule features
  were recomputed independently and matched.
* `simtest` on live mainnet blocks: cloned curve buys, curve sells and PumpSwap sells **pass
  `simulateTransaction` for a different wallet**.
* Exit logic scenario tests: TP1 → TP2 → trailing, hard stop, creator sold, graduation on PumpSwap, time stop.
* Not verified: the live gRPC stream itself (needs your Kaldera token), and sending real transactions.

## Review of the spec — what I found and changed

1. **pump.fun changed since the spec.** `TradeEvent` has new trailing fields (quote mint, buyback fee,
   holder rewards). There are new `buy_v2`/`sell_v2` instructions, and legacy `buy` now carries extra trailing
   accounts. Cloning real instructions (your design) copes with all of that; hand-built instructions would not.
2. **Non-SOL launches.** In the 20-minute sample, **24% of launches were not paired with SOL** (custom quote
   tokens). The SOL maths and the $ volume rule don't apply to them, so they are skipped and counted
   (`creates_non_sol`). USDC pairs are announced next.
3. **Mayhem.** 10% of launches. Their trades are fee-free on different curve parameters, driven by the Mayhem
   agent wallet. Powerful Bot rejects them (spec), and PowerBot/Gymer now do too (`*_REJECT_MAYHEM=1`).
4. **Graduation** is a separate `migrate_v2` transaction, usually seconds after the curve completes. Nothing can
   trade in between; exits wait for the pool. The migration event gives the opening pool reserves, and
   `PB_GRAD_DELAY_MS` delays the sell (trap 4).
5. **Bundle detection (trap 1).** Implemented from the stream: identical (CU limit, CU price) clusters among early
   buyers, runs of ≥4 adjacent same-slot buys from distinct wallets, and the share of buyers left with
   <0.01 SOL. **Same-funding-source detection is not implemented**: it needs system-transfer history that a
   pump.fun-only stream doesn't carry, and RPC lookups are too slow for a 1–5 minute entry. All thresholds are
   placeholders; tune them from `near_misses.jsonl` and `report.py stats` (the blow-ups table shows the bundle
   features of every ≤−70% trade).
6. **Profit protection (trap 2)** is `PB_PP_TP_PCT/PB_PP_TP_FRAC` (partial below 2x) and
   `PB_BE_ARM_PCT/PB_BE_STOP_PCT` (break-even stop). **Entry mcap cap (trap 3)** is `PB_MAX_ENTRY_MCAP_SOL`,
   and entry mcap is logged on every trade. All default to off (= spec), so you can backtest them.
7. **PowerBot/Gymer trigger.** "Buy when the coin reaches 50 holders" is implemented as *judge the moment
   holders cross 50* (`PWB_TRIGGER=cross`). `continuous` re-judges on every trade while ≥50. The spec gives no
   values for the Gymer "passing coins" set, so its defaults are stricter **placeholders**.
8. **Holder counts** come from traded balances. Plain token transfers aren't in the pump.fun stream, so
   top-10/dev/sniper shares are close approximations. Shares are of the 1B total supply.
9. **The 90%-of-curve-supply rule** (714M tokens in one wallet) practically never fires. Kept as specified.
10. **Cross-coin PumpSwap sell templates** (reusing a sell from another pool) failed mainnet simulation
    (`InvalidPool`, because pools differ in trailing accounts), so this is off by default. Graduation sells wait
    for a same-pool sell, which appears within seconds on graduated pools.
11. **Cashback coins** add the trader's PumpSwap volume-accumulator account to sells. This is now substituted
    too; without it, cloned sells on those coins would have carried the other trader's account.
12. **24h backfill over RPC is heavy**: ~216k `getBlock` calls of ~7 MB each (gzip helps). Leaving the
    recorder on while paper trading gives you the same data at no extra cost.

## Live trading (Powerful Bot only)

Order of operations: `hsnipe simtest` → `HS_LIVE=1 HS_LIVE_SIMULATE=1` (builds, signs and simulates every
buy/sell the bot would send, logged in `live_legs.jsonl`; `HS_LIVE_SIMULATE_AS=<your wallet>` needs no
key) → real, with `HS_LIVE_SIZE_SOL` small.

* Buy is cloned from a real buy, sell from a real sell (per coin; entry is refused until both exist).
  Only the trader's own accounts are swapped: wallet, token accounts and the pump/PumpSwap
  volume-accumulator PDAs (plus their WSOL accounts).
* Sent to staked RPC, normal RPC and Jito at the same time, with a CU limit/price and a Jito tip.
  Emergency exits (hard stop, creator sold, graduation, time stop, kill switch) use the `EMERG` fees and
  `min_out = 1`. A failed sell is retried as an emergency.
* The blockhash comes from `blocks_meta`. It refuses to send if the blockhash is more than 100 slots old,
  and falls back to RPC when the block feed stalls for more than 1.5 s.
* A `STOP` file blocks new entries. A `KILL` file also emergency-sells everything. It also refuses entries
  below `HS_LIVE_MIN_BALANCE_SOL`.
* Open positions persist to `positions.json` and are restored on restart. The token-account rent
  (~0.002 SOL) is reclaimed after a full exit.
* Known gap: a crash between sending a buy and seeing it confirmed loses track of that buy until a
  trade event for it arrives. Orphaned buys that land later are adopted and managed.

## Code map

`src/pump.rs` layouts + swap maths · `src/decode.rs` tx → events · `src/grpc.rs` Yellowstone +
reconnect · `src/coin.rs` per-coin state/features · `src/strategy.rs` entry rules + exit policy ·
`src/paper.rs` books · `src/engine.rs` event loop · `src/live.rs` + `src/tx.rs` live trading ·
`src/backfill.rs`, `src/selftest.rs`, `scripts/*.py` tooling.
