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

The first pass used 23 of the 48 devs (624 coins), because the rest were still downloading under pump.fun's API rate limits. The final numbers for all 48 devs follow below.

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

## Final backtest: all 48 devs (1,427 coins)

* Baseline across every dev, on the test coins: 404 trades, 6.9% win rate, **−4.59% average** (best of 288 parameter sets).
* Walk-forward: at most 1 dev qualified on train under any parameter set. The pick with the highest train PnL lost **−14.4%** per trade on its test coins.
* Only candidate: `5ZdusGZMC14SP6eh2XWdoeZDm3612psfxUsKS11DCqkG`.
  * Train: +2.25% average on 8 trades. Test: +6.4% average on 6 trades.
  * The test gain is one +51% coin; the other five were −6% to +5%.
  * 23 of its 37 coins were skipped because the launch block was already bundled.
  * About 14 trades in total. Treat it as noise until paper-trading proves otherwise.

Per-dev results (block+1, 30% entry cap, 5% take-profit, 15% stop, 100-slot hold, whole history):

| # | Dev wallet | Launches 30d | /day | Days | Coins tested | Bought | Skipped (bundled) | Win | Avg net | Reached +5% |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `5ZdusGZMC14SP6eh2XWdoeZDm3612psfxUsKS11DCqkG` | 199 | 19.9 | 10 | 37 | 14 | 23 | 50% | +4.0% | 5 |
| 2 | `CkPgUhSrqAD2J3p8HAe8wJAJz34eZsnZhgBMkxNGSA8W` | 190 | 10.0 | 19 | 40 | 40 | 0 | 8% | -2.1% | 3 |
| 3 | `AeftueU1GtYgZLQ9GtPEHVaXSE5Xit28Kh4j6ACHzBk4` | 200 | 33.3 | 6 | 39 | 39 | 0 | 28% | -2.7% | 7 |
| 4 | `GjGXyrPd1VjHFHQ18BMv8jC4d8HJSbvYQETcH3GRJj2p` | 413 | 15.3 | 27 | 40 | 37 | 3 | 16% | -2.8% | 3 |
| 5 | `7vtQb2ZnhgcpFJKK7Z3xRHZsxAuJCJTwjfTzsM6wx4q5` | 57 | 3.8 | 15 | 35 | 35 | 0 | 6% | -3.0% | 2 |
| 6 | `FQ6WmS5szfK1NRVcGkAqeVbwNeL9kt4xJXhwviyWTB7K` | 194 | 13.9 | 14 | 27 | 27 | 0 | 19% | -3.3% | 4 |
| 7 | `8BuTwTTnciTKJoN8A4vfUGHUqcVGaAGx43uuyUTS9fBx` | 62 | 5.2 | 12 | 40 | 40 | 0 | 10% | -3.3% | 3 |
| 8 | `Ccy5HW7XcDwT95jLgzGY2PaUnswGtfF8Nea8UtFECD1j` | 18 | 3.0 | 6 | 18 | 18 | 0 | 6% | -3.3% | 1 |
| 9 | `6aoG8GtiScsisQC7unezAvzxAEbokvDb84has4nzKBM9` | 68 | 7.6 | 9 | 36 | 36 | 0 | 6% | -3.6% | 2 |
| 10 | `AY7SFX1RUvcJ2aejjw3PbNbLTBJVdHFkPM3Uyw8ZERTV` | 63 | 5.7 | 11 | 39 | 39 | 0 | 5% | -3.7% | 2 |
| 11 | `6zZyanyff5jr3zrWzwN3zEkjCKcADPSNm1c6KoFHxKj8` | 196 | 39.2 | 5 | 40 | 40 | 0 | 5% | -4.5% | 1 |
| 12 | `BYda7W6EKZZNzHcBUqoWBMuEzCVLr1FQ9zcBHSJNJd4s` | 16 | 2.3 | 7 | 16 | 16 | 0 | 0% | -4.5% | 0 |
| 13 | `4CPhM7PmkwPimCKMSML2ZMcSPjThUdr5CP3wnZLU9Tvi` | 68 | 7.6 | 9 | 37 | 37 | 0 | 3% | -4.6% | 1 |
| 14 | `7NP8ppaii2UdmNyhAfDw5kQPmhRGWcSnPMM2JBhbzQFR` | 39 | 1.9 | 21 | 34 | 34 | 0 | 3% | -4.8% | 0 |
| 15 | `6cYrmqubgCbfbtmHAGwdtfmxoXGsPsaTwrZtVkrQopMC` | 14 | 3.5 | 4 | 13 | 13 | 0 | 0% | -4.8% | 0 |
| 16 | `J1aMD3PYDidggPPAgmFMRg5kNHH2KpcBDzknV8Ti1nzJ` | 14 | 2.3 | 6 | 13 | 13 | 0 | 0% | -4.9% | 0 |
| 17 | `6u6hHVH8cs6haavTvT7yTgW6ZwugLT7zBSDMfrVEj9jc` | 13 | 4.3 | 3 | 13 | 13 | 0 | 0% | -4.9% | 0 |
| 18 | `Hfn7j9BaYFjLx3nDeechuBkr1MNeNhxfDEcCvP4Dudj` | 8 | 1.6 | 5 | 6 | 6 | 0 | 0% | -5.0% | 0 |
| 19 | `ATQ9cEoTAvh9naBJmVPXf76JCfKd98mdiEgLwQAoGBsN` | 338 | 84.5 | 4 | 40 | 37 | 3 | 0% | -5.0% | 0 |
| 20 | `9jdoKGF9BYbvWqpnkBy7bcXHdjzhRJ6KRs5rfFa8iH7w` | 55 | 18.3 | 3 | 40 | 40 | 0 | 0% | -5.0% | 0 |
| 21 | `3CQnNpEiGBMhb8Y7zogR5Rf1HK3V5zpLZRZg4BNA4ZXw` | 109 | 5.7 | 19 | 37 | 1 | 36 | 0% | -5.0% | 0 |
| 22 | `GuoGKEnkd3ND3vBcV1LG6atrMUR1ES6f8miQqU6CUEZY` | 274 | 274.0 | 1 | 4 | 4 | 0 | 0% | -5.0% | 0 |
| 23 | `ENhyZMf37rPz3V6tFcDaUkqxKvLhxFTgkWwRJaQbuZXr` | 67 | 22.3 | 3 | 40 | 40 | 0 | 0% | -5.0% | 0 |
| 24 | `8WQoivYAWGWQSi8FL33aC4upMs6yGWZZBUL28gBNVT9m` | 11 | 1.4 | 8 | 11 | 11 | 0 | 9% | -5.1% | 0 |
| 25 | `EFEbRGxLLhfQT9ApSf5TJYd7vQsZ9mGPnXVjGTqc9sAD` | 19 | 2.7 | 7 | 19 | 19 | 0 | 0% | -5.2% | 0 |
| 26 | `GtCySxqpZz4trMcELrgJHXMoSnwcvsX5SN1iSJfuGaFA` | 11 | 1.4 | 8 | 11 | 11 | 0 | 9% | -5.3% | 0 |
| 27 | `3cZQqQHiVG9Tn7Q6ztrYh2xWQJWJaCsrDcobkJVvk4fh` | 133 | 11.1 | 12 | 40 | 40 | 0 | 0% | -5.4% | 0 |
| 28 | `HfLpidy8DVQYFMk845WfiUAWARfwqcMFGGeEb2wxqUff` | 457 | 114.2 | 4 | 35 | 35 | 0 | 0% | -6.0% | 0 |
| 29 | `ebZRtCmGBefPCrj5kJMZBUSDYrsjTweT5EVspfLMuPA` | 183 | 7.0 | 26 | 40 | 40 | 0 | 12% | -6.0% | 1 |
| 30 | `AN38on9YmoH6TQ7KEfnUY1mp88HdNG2m6iV9BgD5NTpK` | 200 | 40.0 | 5 | 22 | 21 | 1 | 0% | -6.0% | 0 |
| 31 | `9QGCTFeKFyQENpiZy3YdETu1P5nnLMwK5kZJxitoNMty` | 15 | 2.5 | 6 | 15 | 15 | 0 | 0% | -6.1% | 0 |
| 32 | `92Yv6GtCZ8iuz986z5kBrUfyQHFLfvNXMVMNYfZt3quZ` | 9 | 1.8 | 5 | 6 | 6 | 0 | 17% | -6.5% | 0 |
| 33 | `28U7RcqyuF7oHzQhhF7x7NwHD1ySgMALHhKpedHnyx28` | 155 | 8.6 | 18 | 40 | 3 | 37 | 0% | -6.5% | 0 |
| 34 | `dtrzJPj7yDdvm6eRqBAgxsK2sMJeD9HhBEBB3XMedXy` | 165 | 23.6 | 7 | 39 | 7 | 32 | 0% | -7.3% | 0 |
| 35 | `HMtLswKnCbtf3UgvZEYQqkRuvShFyVNq8HvcDA4L4uG4` | 175 | 15.9 | 11 | 39 | 18 | 21 | 11% | -7.4% | 2 |
| 36 | `J5EMgApWdjCbVzVuAo7HNTSwAmodYAjApiVUkQQcjpks` | 11 | 2.8 | 4 | 9 | 1 | 8 | 0% | -7.6% | 0 |
| 37 | `7LQ4vhWzLwFf7kVCgrGvEY9KopFiGVgpV88Y4USqCaB2` | 56 | 4.7 | 12 | 35 | 35 | 0 | 9% | -7.9% | 3 |
| 38 | `2pzDP3tKyBGDhbMx3wNTaLoDQK3z5CQGu6U5iGSNbRES` | 16 | 2.7 | 6 | 16 | 16 | 0 | 6% | -8.6% | 1 |
| 39 | `2bRoascfGCKkRnyZLmzJh7cKpiD45zyEzUmg9XPZzueC` | 149 | 29.8 | 5 | 39 | 29 | 10 | 10% | -8.6% | 3 |
| 40 | `B4tWmhfCBLvRYugPcu6bgP36DVweLc457pywc1a3QJju` | 159 | 12.2 | 13 | 39 | 38 | 1 | 16% | -9.2% | 6 |
| 41 | `CzbN6T1gKkKutvuPXcxNmV8FLqzjsDWebWmg9o8e2ZbU` | 115 | 8.8 | 13 | 34 | 2 | 32 | 50% | -9.3% | 0 |
| 42 | `7ZV54HcwtzRhZSEPskT8ox5hn9yNocK9xpe4BQXoziaP` | 162 | 32.4 | 5 | 38 | 1 | 37 | 0% | -11.1% | 0 |
| 43 | `9Q27kHAzwH793kXV86QuBpTVXjLbHmHjQa487o7AcU1A` | 90 | 5.3 | 17 | 37 | 2 | 35 | 0% | -11.6% | 0 |
| 44 | `2JxPmU9UU5KhSQ7h5v8PfhgbxPQhHNyBqNrhPAjPiWCu` | 495 | 33.0 | 15 | 36 | 0 | 36 | — | — | 0 |
| 45 | `7tbx3T6Fxa5HkPviEmUX5D1yw4JTMSYzMBYiU5fC4iGw` | 242 | 8.6 | 28 | 39 | 0 | 39 | — | — | 0 |
| 46 | `CALQ1EwjARtaW6FW6KQDZj4jhye9Vt4NvTzFwmg4JHqC` | 199 | 28.4 | 7 | 40 | 0 | 40 | — | — | 0 |
| 47 | `6DjUJBBgkp3mp1d6D8b8bwgmkEnmoP5Yx8DpQQHaFkVD` | 63 | 5.7 | 11 | 27 | 0 | 27 | — | — | 0 |
| 48 | `68gqSuzZidX16R7QoBrMLeDWDLSgAFdFY5DPgaa31Bjk` | 47 | 15.7 | 3 | 37 | 0 | 37 | — | — | 0 |

All 48 devs pooled: 969 trades, win 7.3%, avg -4.87%, total -2.408 SOL


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
