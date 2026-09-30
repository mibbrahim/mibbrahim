"""Sanity tests for curve math and the strategy state machine: `python -m pytest tests`."""
from sniper.pump import Curve, INIT_V_SOL, quote_buy, quote_sell
from sniper.sim import Params, simulate_coin

LAUNCH = Curve.initial().price


def test_curve_roundtrip_costs_fees_only():
    c = Curve.initial()
    tok = quote_buy(c, 50_000_000, 125)
    c.apply_buy(50_000_000 * 10_000 // 10_125, tok)
    back = quote_sell(c, tok, 125)
    assert 0.974 < back / 50_000_000 < 0.976          # ~2.5% round trip on pump fees


def test_from_price_recovers_reserves():
    c = Curve.initial()
    c.apply_buy(2_000_000_000, c.tokens_for_sol(2_000_000_000))
    r = Curve.from_price(c.price)
    assert abs(r.v_sol - c.v_sol) / c.v_sol < 1e-6


def _trade(slot, mult, user="u", buy=True):
    return {"slot": slot, "price": LAUNCH * mult, "user": user, "buy": buy}


def test_take_profit_after_pump():
    trades = [_trade(100, 1.0), _trade(101, 1.02), _trade(103, 1.25), _trade(104, 1.3)]
    r = simulate_coin("m", "dev", trades, Params(take_profit=0.05))
    assert r.entered and r.reason == "take_profit" and r.pnl_pct > 0.05
    assert r.entry_slot == 101 and r.exit_slot == 104


def test_dead_coin_loses_fees():
    r = simulate_coin("m", "dev", [_trade(100, 1.0)], Params())
    assert r.entered and r.reason == "timeout" and -0.06 < r.pnl_pct < -0.04


def test_entry_cap_skips_bundled_launch():
    r = simulate_coin("m", "dev", [_trade(100, 2.0), _trade(101, 2.05)], Params(max_entry_pump=0.6))
    assert not r.entered and r.reason == "entry_too_high"


def test_dev_sell_exit():
    trades = [_trade(100, 1.0), _trade(101, 1.0), _trade(102, 1.01), _trade(103, 0.97, "dev", False), _trade(104, 0.6)]
    r = simulate_coin("m", "dev", trades, Params(stop_loss=0.5))
    assert r.reason == "dev_sell" and r.exit_slot == 104
