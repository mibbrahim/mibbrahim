# Build Prompt: Polymarket 5-Minute Up/Down Market-Making Bot

> Copy everything below the line into Claude Code (or another coding agent) to build and deploy the bot.
> Strategy reverse-engineered from polymarket.com/@sirmartingale — a delta-neutral
> split/quote/merge liquidity bot on 5-minute BTC/ETH Up-or-Down markets.

---

## ROLE

You are a senior trading-infrastructure engineer. Build, test, and deploy a production-grade
Polymarket market-making bot in Python 3.11+, then deploy it to a DigitalOcean droplet.
Work incrementally, run the test suite after every phase, and never place a live order
until dry-run acceptance criteria pass.

## THE STRATEGY (context you must implement exactly)

Polymarket lets anyone convert $1 USDC into 1 "Up" + 1 "Down" share of the same market
(`splitPosition` on the Conditional Tokens Framework contract on Polygon), and convert a
matched pair back into $1 (`mergePositions`). Holding both sides is always worth exactly $1,
so split inventory carries zero directional risk.

The bot loop, for every 5-minute "Bitcoin Up or Down" and "Ethereum Up or Down" market:

1. **Discover** the next market before it opens. Slugs are predictable:
   `btc-updown-5m-<unix-start-ts>` and `eth-updown-5m-<unix-start-ts>` (Gamma API:
   `https://gamma-api.polymarket.com/markets?slug=...`).
2. **Split** N USDC into N Up + N Down shares.
3. **Quote both sides**: resting limit asks (and optionally bids) on Up and Down, centered
   on the live mid-price. Polymarket's liquidity rewards program pays a daily USDC pool to
   orders of ≥ `rewardsMinSize` shares (currently 50) within `rewardsMaxSpread` (currently
   4.5¢) of the mid, scored for size, tightness, and two-sidedness. Read both values from
   the Gamma market object at runtime — never hardcode.
4. **Manage fills**: if one side fills, you earned spread over the $0.50 mint cost but now
   hold unhedged inventory of the other side. Re-quote it, and inside the final ~20 seconds
   either dump it or buy the near-certain winner at ≥$0.95 to redeem at $1.00.
5. **Wind down**: cancel all orders ~10s before window end, `mergePositions` all matched
   pairs back to USDC, redeem winning remainders after resolution.
6. Repeat forever, 24/7, for every new window (~576 markets/day).

Income sources: liquidity rewards (primary), maker rebates, 1–2¢ spread scalps.
Primary risk: adverse selection — takers sniping stale quotes the instant BTC/ETH moves.
Defenses: fast cancel-on-move, per-market inventory cap, global kill switch.

## HARD CONSTRAINTS

- **Starting bankroll: $5 USDC.** Be honest about what this means and build around it:
  - $5 cannot meet the 50-share reward minimum (50 shares ≈ $25 notional per side).
    So the bot MUST have a `BANKROLL_TIER` state machine:
    - **Tier 0 — dry-run (default)**: full simulation against live books, zero real orders.
    - **Tier 1 — micro ($5–$50)**: no reward farming possible. Trade only the late-window
      redemption scalp (buy the ≥$0.95 near-certain side with tight slippage limits) and
      1-share spread scalps, to grow the bankroll. Expect single-digit cents per day; this
      tier mainly proves the pipeline works with real money.
    - **Tier 2 — rewards ($50–$500)**: quote one market at a time at exactly
      `rewardsMinSize`, alternating BTC/ETH.
    - **Tier 3 — full ($500+)**: SirMartingale mode — quote every BTC and ETH window
      concurrently, $250–$600 per market.
  - Tier upgrades/downgrades happen automatically from realized bankroll, with hysteresis.
- **Never risk more than the configured bankroll.** No borrowing, no negative balances.
- **All secrets via environment variables** (`POLY_PRIVATE_KEY`, funder address, API creds
  derived via py-clob-client). Never log or commit them.
- **MATIC/POL for gas**: check balance at startup; refuse to start below a threshold and
  document that ~$1 of POL lasts months.
- The operator must check Polymarket's terms/jurisdiction eligibility themselves — print a
  one-time notice at first run; do not attempt to bypass any geo restriction.

## TECH SPEC

- Python 3.11+, `py-clob-client` for orders, `web3.py` for split/merge/redeem calls to the
  CTF contract (`0x4D97DCd97eC945f40cF65F87097ACe5EA0476045`) and USDC approvals.
- Async event loop (`asyncio`): one coroutine per live market + a global risk supervisor.
- CLOB WebSocket (`wss://ws-subscriptions-clob.polymarket.com/ws/market`) for book updates;
  REST fallback with polling if the socket drops.
- Cancel-on-move: if mid shifts more than half your quoted edge, cancel and re-center
  within one tick. Track and log quote-to-cancel latency.
- SQLite ledger: every split, order, fill, merge, redeem, and reward payout with timestamps
  — the P&L report must reconcile ledger vs. on-chain balance daily.
- `config.yaml` for all tunables (markets, size, edge, band, tier thresholds, kill-switch
  drawdown %). Sane defaults matching this spec.
- Structured JSON logs + a tiny status endpoint (`GET /health`, `GET /pnl`) on localhost.
- **Kill switch**: flat-and-halt if daily realized loss exceeds X% of bankroll (default 10%),
  on repeated RPC/API failures, or on `SIGTERM` (graceful: cancel all, merge all, exit).
- Tests: unit tests for sizing/tier logic and quote math with a mocked exchange; an
  integration dry-run that must run 2 hours against live data with zero errors.

## DELIVERABLES / REPO LAYOUT

```
polymarket-updown-bot/
├── bot/                  # package: discovery, quoting, inventory, ctf, risk, ledger
├── tests/
├── config.yaml
├── .env.example
├── Dockerfile            # slim python image, non-root user
├── docker-compose.yml    # bot + restart: unless-stopped
├── deploy/
│   ├── provision.sh      # doctl: create cheapest droplet, firewall (deny all inbound
│   │                     #   except SSH), docker install
│   └── deploy.sh         # rsync/build/up; idempotent
└── README.md             # setup, funding wallet, tier table, risk warnings, runbook
```

## DIGITALOCEAN DEPLOYMENT

- Cheapest droplet (1 vCPU / 512MB–1GB, ~$4–6/mo) — the bot is I/O-bound and fits easily.
  Note in the README that droplet cost will exceed Tier-1 income; that's expected while
  the bankroll grows in Tier 1–2.
- Docker Compose with `restart: unless-stopped`; container healthcheck hitting `/health`.
- Secrets injected via droplet-side `.env` file (chmod 600), never baked into the image.
- Log rotation; optional Telegram/Discord webhook for fills, tier changes, kill-switch
  events, and the daily P&L summary.
- Runbook section: how to pause (flat-and-halt), resume, withdraw, and rotate keys.

## ACCEPTANCE CRITERIA (in order — do not skip ahead)

1. Unit tests green.
2. 2-hour dry run on live data: correct market discovery for every 5-min window, simulated
   quotes stay inside the reward band, simulated merges reconcile to $0.00 ± fees.
3. Deployed to the droplet in dry-run mode, survives a reboot, 24h without crash.
4. Only then: operator manually flips `mode: live` with the $5 bankroll (Tier 1).
5. Daily P&L report reconciles ledger vs. on-chain balance.

## WHAT NOT TO DO

- Don't hardcode reward parameters, market slugs, or fee rates — read them from the APIs.
- Don't use market orders for rebalancing except inside the final-second wind-down path.
- Don't quote wider than the reward band expecting rewards, or below `rewardsMinSize`.
- Don't retry failed on-chain transactions blindly — check nonce/receipt state first.
- Don't ever place live orders while acceptance criteria 1–3 are unmet.
