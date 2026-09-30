"""Strategy engine: buy on block create+N, sell at a small net profit, a stop,
a dev sell, or a time limit. The same `Position` state machine is driven by
historical trades (backtest) and by live trade events (paper/live), so all
three modes make identical decisions.

All PnL numbers are NET: pump.fun fees on both legs, priority fees/tips and
signature fees are subtracted.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, field

from .pump import LAMPORTS, Curve, quote_buy, quote_sell


@dataclass
class Params:
    buy_sol: float = 0.05
    entry_slot_offset: int = 1        # land in create_slot + N (1 = the block after launch)
    entry_position: str = "end"       # "end": we land after everyone else in that slot (conservative)
    max_entry_pump: float = 0.60      # skip if price is already >60% above launch price at entry
    take_profit: float = 0.05         # net return that triggers a sell
    stop_loss: float = 0.10           # net loss that triggers a sell
    max_hold_slots: int = 25          # ~10 s at 400 ms slots
    exit_latency_slots: int = 1       # sell lands this many slots after the trigger is seen
    exit_on_dev_sell: bool = True
    fee_bps: int = 125                # pump.fun protocol + creator fee per side (on-chain FeeConfig)
    buy_tip_sol: float = 0.001        # priority fee + Jito tip on the buy
    sell_tip_sol: float = 0.0003      # priority fee + Jito tip on the sell
    sig_fee_sol: float = 0.000005     # base fee per transaction

    @property
    def cost_basis_sol(self) -> float:
        return self.buy_sol + self.buy_tip_sol + self.sig_fee_sol

    def label(self) -> str:
        cap = "nocap" if self.max_entry_pump >= 10 else f"cap{self.max_entry_pump:.0%}"
        return (f"blk+{self.entry_slot_offset} {cap} tp{self.take_profit:.0%} sl{self.stop_loss:.0%} "
                f"hold{self.max_hold_slots}")


@dataclass
class TradeResult:
    mint: str
    creator: str
    entered: bool
    reason: str = ""
    entry_slot: int = 0
    exit_slot: int = 0
    entry_price: float = 0.0
    exit_price: float = 0.0
    tokens: int = 0
    pnl_sol: float = 0.0
    pnl_pct: float = 0.0
    peak_pct: float = 0.0             # best net return seen while holding
    hold_slots: int = 0

    def to_dict(self) -> dict:
        return asdict(self)


@dataclass
class Position:
    """State machine for one snipe. Feed it `on_trade(slot, price, user, is_buy)`
    in chain order; it reports when to enter and exit."""
    mint: str
    creator: str
    create_slot: int
    p: Params
    launch_price: float = 2.7958993476234855e-08   # 30 SOL / 1.073B tokens
    state: str = "waiting"                          # waiting -> holding -> exiting -> closed
    curve: Curve | None = None
    last_price: float = 0.0
    last_slot: int = 0
    tokens: int = 0
    entry_slot: int = 0
    entry_price: float = 0.0
    exit_at_slot: int = 0
    exit_reason: str = ""
    peak: float = -1.0
    result: TradeResult | None = None
    _pending: list = field(default_factory=list)

    @property
    def target_slot(self) -> int:
        return self.create_slot + self.p.entry_slot_offset

    # -------------------------------------------------------------- valuation
    def net_value_sol(self, price: float) -> float:
        """What selling everything now would return, net of fees, including our own price impact."""
        hist = Curve.from_price(price)
        k = hist.v_sol * hist.v_tok
        v_tok = max(hist.v_tok - self.tokens, 1)          # remove our tokens from the historical curve
        with_us = Curve(k // v_tok, v_tok)
        lamports = quote_sell(with_us, self.tokens, self.p.fee_bps)
        return lamports / LAMPORTS - self.p.sell_tip_sol - self.p.sig_fee_sol

    def net_return(self, price: float) -> float:
        return self.net_value_sol(price) / self.p.cost_basis_sol - 1

    # -------------------------------------------------------------- driving
    def on_trade(self, slot: int, price: float, user: str, is_buy: bool) -> None:
        """Process one trade (already executed on chain) in order."""
        if self.state == "closed":
            return
        # Close out slots that are finished before applying this trade.
        if slot != self.last_slot and self.last_slot:
            self._slot_finished(self.last_slot, slot)
            if self.state == "closed":
                return
        if self.state == "waiting" and slot >= self.target_slot and self.p.entry_position == "start":
            self._enter(slot, self.last_price or self.launch_price)
        self.last_price, self.last_slot = price, slot
        if self.state == "holding" and slot > self.entry_slot:
            self._check_triggers(slot, price, user, is_buy)

    def _slot_finished(self, done_slot: int, next_slot: int) -> None:
        if self.state == "waiting":
            if done_slot >= self.target_slot:
                self._enter(done_slot, self.last_price)
            elif next_slot > self.target_slot and self.p.entry_position == "end":
                # No trades in the target slot: we'd land on the state as of `done_slot`.
                self._enter(self.target_slot, self.last_price)
        if self.state == "holding" and next_slot > self.entry_slot + self.p.max_hold_slots:
            self._trigger(self.entry_slot + self.p.max_hold_slots, "timeout")
        if self.state == "exiting" and next_slot > self.exit_at_slot:
            self._close(self.exit_at_slot, self.last_price)

    def _enter(self, slot: int, price: float) -> None:
        if price > self.launch_price * (1 + self.p.max_entry_pump):
            self.state = "closed"
            self.result = TradeResult(self.mint, self.creator, False, "entry_too_high", entry_slot=slot,
                                      entry_price=price)
            return
        lamports = int(self.p.buy_sol * LAMPORTS)
        self.tokens = quote_buy(Curve.from_price(price), lamports, self.p.fee_bps)
        self.entry_slot, self.entry_price, self.state = slot, price, "holding"
        self.peak = self.net_return(price)

    def _check_triggers(self, slot: int, price: float, user: str, is_buy: bool) -> None:
        r = self.net_return(price)
        self.peak = max(self.peak, r)
        if r >= self.p.take_profit:
            self._trigger(slot, "take_profit")
        elif r <= -self.p.stop_loss:
            self._trigger(slot, "stop_loss")
        elif self.p.exit_on_dev_sell and not is_buy and user == self.creator:
            self._trigger(slot, "dev_sell")

    def _trigger(self, slot: int, reason: str) -> None:
        self.state, self.exit_reason = "exiting", reason
        self.exit_at_slot = slot + self.p.exit_latency_slots

    def _close(self, slot: int, price: float) -> None:
        pnl = self.net_value_sol(price) - self.p.cost_basis_sol
        self.state = "closed"
        self.result = TradeResult(
            self.mint, self.creator, True, self.exit_reason, self.entry_slot, slot, self.entry_price, price,
            self.tokens, pnl, pnl / self.p.cost_basis_sol, self.peak, slot - self.entry_slot)

    def tick(self, now_slot: int) -> None:
        """The chain has moved on to `now_slot` with no new trades for this coin."""
        if self.state != "closed" and self.last_slot and now_slot > self.last_slot:
            self._slot_finished(self.last_slot, now_slot)

    def finish(self, now_slot: int | None = None) -> TradeResult:
        """No more trades will come (history exhausted / coin went quiet): settle at the last price."""
        if self.state != "closed" and self.last_slot:
            end = max(now_slot or 0, self.last_slot + 10_000)
            self._slot_finished(self.last_slot, end)
            if self.state == "holding":
                self._trigger(self.last_slot, "no_more_trades")
                self._close(self.exit_at_slot, self.last_price)
            elif self.state == "exiting":
                self._close(self.exit_at_slot, self.last_price)
        if self.result is None:
            self.state = "closed"
            self.result = TradeResult(self.mint, self.creator, False, "no_trades")
        return self.result


def simulate_coin(mint: str, creator: str, trades: list[dict], p: Params,
                  create_slot: int | None = None) -> TradeResult:
    """Backtest one coin from its full trade list (oldest first).

    `create_slot` is exact for recorded-stream data; for API data the first trade
    (normally the dev buy inside the create transaction) stands in for it."""
    if create_slot is None:
        if not trades:
            return TradeResult(mint, creator, False, "no_trades")
        create_slot = trades[0]["slot"]
    pos = Position(mint, creator, create_slot, p)
    pos.last_slot, pos.last_price = create_slot, pos.launch_price
    for t in trades:
        pos.on_trade(t["slot"], t["price"], t["user"], t["buy"])
        if pos.state == "closed":
            break
    return pos.finish()


def summarize(results: list[TradeResult]) -> dict:
    taken = [r for r in results if r.entered]
    wins = [r for r in taken if r.pnl_sol > 0]
    pnl = sum(r.pnl_sol for r in taken)
    reasons: dict[str, int] = {}
    for r in taken:
        reasons[r.reason] = reasons.get(r.reason, 0) + 1
    return {
        "coins": len(results),
        "trades": len(taken),
        "win_rate": len(wins) / len(taken) if taken else 0.0,
        "pnl_sol": pnl,
        "avg_pnl_sol": pnl / len(taken) if taken else 0.0,
        "avg_pnl_pct": sum(r.pnl_pct for r in taken) / len(taken) if taken else 0.0,
        "avg_win_pct": sum(r.pnl_pct for r in wins) / len(wins) if wins else 0.0,
        "avg_loss_pct": (sum(r.pnl_pct for r in taken if r.pnl_sol <= 0) / max(1, len(taken) - len(wins))),
        "exit_reasons": reasons,
    }
