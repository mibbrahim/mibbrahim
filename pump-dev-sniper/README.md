# pump-dev-sniper

Finds pump.fun devs who keep launching coins, backtests sniping their launches
on block +1/+2 and selling for a small net gain, paper-trades it live, and,
once you choose to, trades it live with hard risk limits.

```
discover  ->  backtest  ->  paper  ->  live selftest  ->  live run
(devs)        (walk-forward) (real-time, no tx) (simulated tx)   (real SOL)
record    ->  exact launch/trade recorder feeding the backtest (no API limits)
```

## Setup

```bash
cd pump-dev-sniper
python3 -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt
cp .env.example .env   # fill in your RPC / WS / send endpoints; key only for live
set -a; . ./.env; set +a
```

## 1. Find active serial devs

```bash
python -m sniper.discover --days 30 --min-launches 8 --min-active-days 3
```

* Takes the newest ~1,500 launches (pump.fun's listing only goes back ~1 hour)
  plus any creators saved by earlier paper/record runs, then looks up each
  creator's 30-day launch history.
* Keeps a dev only if it launched at least 8 SOL-paired coins, on at least 3
  different days (or hit the 150-coin cap), with a launch in the last 48 h.
* Downloads the full early trade list of each dev's latest 40 coins, and reads
  each coin's on-chain fee. Creators can set their own fee (anything from 30 to
  300 bps), so the real fee is 1.25% to 3.95% per side.
* Leaves out mayhem-mode coins (traded by an agent on a different curve) and
  coins paired with anything other than SOL.

pump.fun rate-limits its API hard (about 1 request/s for listings and 3/s for
trades from one IP), so a full run takes 30–60 minutes. Everything is cached
in `data/cache/`, so later runs only fetch what changed.

## 2. Backtest (walk-forward)

```bash
python -m sniper.backtest                       # full grid
python -m sniper.backtest --buy 0.05 --tip 0.001 --stream data/stream
```

Each dev's coins are split by time. The older 60% are used to pick the
parameters and the devs to follow. The newer 40% are never seen during that
choice, so **only the TEST line is an honest estimate**.

What the simulation assumes (on the cautious side):

* The buy lands in block `create + N`, **after** every other trade in that
  block. Bundled dev buys in block 0 are already in the price you pay.
* The sell lands one block after the trigger, at the price left after every
  trade in that block. If the dev dumps in that block, you eat it.
* Both legs pay the coin's real pump.fun fee, plus priority fee/tip and
  signature fees, plus your own price impact.
* An "optimistic" line (first in the block, sells land instantly) gives the
  upper bound.

Outputs: `data/watchlist.json` (devs plus the chosen parameters) and
`data/backtest_report.json`.

## 3. Paper trade live

```bash
python -m sniper.paper --minutes 25                 # watched devs only
python -m sniper.paper --minutes 25 --all-devs      # baseline: every launch
```

Streams pump.fun's program logs over a websocket and decodes every
Create/Trade event with its slot. For each launch by a watched dev it runs the
same state machine as the backtest. Results go to `data/paper_trades.jsonl`.

## 4. Go live

```bash
python -m sniper.live selftest          # builds buy/sell and simulates them on mainnet, spends nothing
PRIVATE_KEY=... python -m sniper.live run --minutes 30 --buy 0.05 --max-loss 0.15 --balance-floor 0.30
```

* The transactions are built by hand from the official pump IDL, including the
  April 2026 `bonding-curve-v2` and fee-recipient accounts. `selftest` checks
  them against live mainnet state; run it again after any pump.fun upgrade.
* Every tx goes to all `SEND_URLS` at the same time with `skipPreflight`. The
  tip is split between compute-unit price and a transfer to `TIP_ACCOUNTS`
  (Jito by default; set this to your sender's tip accounts).
* Fills are detected from our own TradeEvents in the stream. Sells retry every
  4 slots, and after a failed take-profit attempt the price floor is dropped.
* Safety limits:
  * `--max-loss`: no new entries once the session is down this much.
  * `--balance-floor`: no new entries once the wallet drops below this.
  * `--max-open`: most positions open at once.
  * `min_tokens_out`: every buy fails on-chain if the price is already above
    the entry cap.
  * `touch data/STOP`: stops new entries at once; open positions are still sold.

Use a fresh wallet funded only with what you are prepared to lose.

## Keep the data fresh

```bash
python -m sniper.record --minutes 600      # exact launch + first-300-slot trades, no API limits
python -m sniper.backtest --stream-only --stream data/stream   # forward-test the current watchlist
```

On your own RPC, `record` gives far better data than the public HTTP API. It
has exact slots and reserves, and it stores every launch, not just the ones
the listing returns.

## Results from the first run

See `RESULTS.md`.
