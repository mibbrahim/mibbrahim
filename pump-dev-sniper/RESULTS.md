# First run: 30 Sep 2026

Strategy: buy 0.05 SOL on block +1/+2 after a serial dev's launch, sell at a
small net profit, a stop, a dev sell, or a time limit.

## Costs per round trip at 0.05 SOL

| item | cost |
|---|---|
| pump.fun fee (95 bps protocol + 30 bps creator, per side; read from each coin on-chain) | 2.50% |
| priority fee + tip (0.001 buy / 0.0003 sell) + signature fees | 2.62% |
| **price rise needed just to break even** | **~5.1%** |

Some devs set a creator fee of up to 300 bps, which makes break-even about 10%.

## Backtest (walk-forward)

The dataset is 23 active serial devs (8 to 495 launches in 30 days) and 624 coins
with their full early trade history. The rest of the 48-dev set was still
downloading because of pump.fun API rate limits.

* **Baseline, every coin from every dev: about −4.5% per trade** across all 288
  parameter sets. The best was block+1, 30% entry cap, 2% take-profit, 15% stop,
  100-slot hold: test 175 trades, 9% win rate, −4.48% average.
* **Walk-forward: no dev qualified.** No dev was profitable on its older 60% of
  coins under any parameter set, so the watchlist is empty.
* **Upside after a block+1 entry**: 90% of coins never beat +2.3% net within 100
  slots (about 40 s), and only 7.9% ever reach +5% net.
* **Why**: many of these devs bundle the launch block. The dev buy plus 5–6 of
  their own wallets push the price +35% to +106% inside block 0, then they sell
  into block+1/+2 buyers. Devs who don't bundle mostly get no buyers at all.
  Either way block+1 is too late for the move.
* **Best case** (first in block, zero-latency exits, zero tips) on 610 launches
  recorded slot-by-slot from the live stream: +0.04% per trade, i.e. break-even.

## Live paper session (25 min, public RPC websocket)

Watched the 9 least-bad devs from the backtest, with parameters block+1, 30%
entry cap, 5% take-profit, 15% stop and 100-slot hold.

| coin | dev | entry | result |
|---|---|---|---|
| I/acc | 7vtQb2Zn | blk+1 @ launch price | timeout −4.93% |
| I/acc | 7vtQb2Zn | blk+1 @ launch price | timeout −4.99% |
| BlackCat | 7vtQb2Zn | blk+1 @ launch price | timeout −4.99% |
| ARCADS | GjGXyrPd | blk+1 @ +29% | timeout −5.07% |
| SI | CkPgUhSr | blk+1 @ launch price | timeout −4.99% |
| ORANGIE | CkPgUhSr | blk+1 @ launch price | timeout −4.20% |
| memepad | GjGXyrPd | — | skipped: already >30% up at block+1 |
| AI | 6zZyanyf | blk+1 @ +15% | timeout −5.04% |
| MRSTONK | CkPgUhSr | blk+1 @ launch price | timeout −4.99% |

**9 launches in 25 min, 8 trades, 0 wins, −4.90% average, −0.020 SOL.**
That matches the backtest: this is the fee drag of trades where nobody else
buys.

## Conclusion

The frequency condition holds (9 launches in 25 min), but the edge does not.
At 0.05 SOL per trade the fixed costs are about 5%. These devs' coins almost
never rise that much after block 0. So taking 2–5% or 5–10% per trade on
block+1/+2 loses about 0.0025 SOL per trade. At 1,000 trades a day that is
roughly −2.5 SOL/day, not +$30–40.

What would have to change for this to work, in order of impact:

1. **Being inside block 0**, i.e. in the same block as the launch. Only
   the dev's bundle gets that reliably.
2. **Much lower costs**: tips near zero and a bigger size so the fixed tip is a
   smaller share. The pump.fun fee (2.5% round trip) stays either way.
3. **A different selection signal** than "serial dev". For example, coins
   whose first block shows organic buyers.
   `python -m sniper.record` plus `python -m sniper.backtest --stream`
   is the loop for testing ideas like that.

The live trader (`python -m sniper.live`) is built, with its transactions
verified by mainnet simulation and risk limits in place. It should not be run
with this watchlist: the backtest and paper session both say it would lose.
