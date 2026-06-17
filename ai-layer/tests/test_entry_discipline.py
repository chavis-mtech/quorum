"""Test the anti-chase entry-discipline guard — over-extended market BUYs must be
converted to a pullback LIMIT (or HOLD), while reasonable entries are left untouched.

Run:  python ai-layer/tests/test_entry_discipline.py   (pytest not required)
Or:   pytest ai-layer/tests
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from judge import _apply_entry_discipline


def _ctx(regime="unknown", **structure):
    base = {
        "quality": "ok", "atr_14": 4.0, "support_20": 90.0,
        "room_to_resistance_20": 0.10, "distance_to_support_20": 0.05,
        "momentum_5": 0.01, "rsi": 55.0,
    }
    base.update(structure)
    return {"last_price": 100.0, "regime": regime, "market_structure": base}


def _buy(**over):
    v = {"action": "BUY", "entry_type": "market", "entry_price": 100.0,
         "stop_price": 95.0, "target_price": 112.0, "reasoning": "x"}
    v.update(over)
    return v


def test_overbought_rsi_converts_market_to_limit():
    out = _apply_entry_discipline(_buy(), _ctx(rsi=78.0))
    assert out["action"] == "BUY" and out["entry_type"] == "limit"
    assert 95.5 <= out["entry_price"] < 100.0  # below market, above the 95 stop


def test_no_room_to_resistance_converts_to_limit():
    out = _apply_entry_discipline(_buy(), _ctx(room_to_resistance_20=0.01))
    assert out["entry_type"] == "limit" and out["entry_price"] < 100.0


def test_far_above_support_converts_to_limit():
    out = _apply_entry_discipline(_buy(), _ctx(distance_to_support_20=0.20))
    assert out["entry_type"] == "limit"


def test_hot_momentum_converts_to_limit():
    out = _apply_entry_discipline(_buy(), _ctx(momentum_5=0.15))
    assert out["entry_type"] == "limit"


def test_reasonable_price_keeps_market_entry():
    out = _apply_entry_discipline(_buy(), _ctx())  # not extended on any axis
    assert out["action"] == "BUY" and out["entry_type"] == "market"
    assert out["entry_price"] == 100.0


def test_extended_with_no_safe_pullback_holds():
    # stop sits at 99 — above any sane pullback below 100 → stand aside instead of chasing
    out = _apply_entry_discipline(_buy(stop_price=99.0), _ctx(rsi=80.0))
    assert out["action"] == "HOLD" and out["entry_type"] == "none"


def test_limit_entry_is_left_untouched():
    v = {"action": "BUY", "entry_type": "limit", "entry_price": 96.0,
         "stop_price": 92.0, "target_price": 110.0, "reasoning": "x"}
    out = _apply_entry_discipline(dict(v), _ctx(rsi=80.0))
    assert out == v  # only market entries can chase; a limit is already disciplined


def test_pullback_never_below_support():
    # support at 99 → pullback must clamp to just above it, not the raw 0.5*ATR (98)
    out = _apply_entry_discipline(_buy(stop_price=90.0), _ctx(rsi=80.0, support_20=99.0))
    assert out["entry_type"] == "limit" and out["entry_price"] >= 99.0


def test_sell_and_hold_are_ignored():
    for action in ("SELL", "HOLD"):
        v = {"action": action, "entry_type": "market", "entry_price": 100.0, "reasoning": "x"}
        assert _apply_entry_discipline(dict(v), _ctx(rsi=85.0))["action"] == action


# ─── regime-aware behaviour ──────────────────────────────────────────────────────

def test_trending_keeps_market_on_strong_momentum():
    # in a confirmed trend, RSI 78 + a +12% 5-bar push is healthy strength, not a chase —
    # the bot should enter at market and ride it instead of waiting for a dip that won't come
    out = _apply_entry_discipline(_buy(), _ctx(regime="trending", rsi=78.0, momentum_5=0.12))
    assert out["action"] == "BUY" and out["entry_type"] == "market"
    assert out["entry_price"] == 100.0


def test_trending_still_pulls_back_when_parabolic():
    # truly parabolic (RSI >= 82) → even a strong trend waits for a small dip
    out = _apply_entry_discipline(_buy(), _ctx(regime="trending", rsi=84.0))
    assert out["entry_type"] == "limit" and out["entry_price"] < 100.0


def test_ranging_pulls_back_on_mild_momentum():
    # in a range the edge is mean-reversion: +7% 5-bar momentum already means "near the top"
    out = _apply_entry_discipline(_buy(), _ctx(regime="ranging", momentum_5=0.07))
    assert out["entry_type"] == "limit" and out["entry_price"] < 100.0


def test_ranging_has_a_lower_overbought_bar_than_trending():
    # RSI 70 converts to a limit in a range (bar 68) but would stay market in a trend (bar 82)
    assert _apply_entry_discipline(_buy(), _ctx(regime="ranging", rsi=70.0))["entry_type"] == "limit"
    out_trend = _apply_entry_discipline(_buy(), _ctx(regime="trending", rsi=70.0))
    assert out_trend["entry_type"] == "market"


def _run_all():
    fns = [v for k, v in globals().items() if k.startswith("test_")]
    passed = 0
    for fn in fns:
        fn()
        print(f"  ✓ {fn.__name__}")
        passed += 1
    print(f"\n{passed}/{len(fns)} passed")


if __name__ == "__main__":
    _run_all()
