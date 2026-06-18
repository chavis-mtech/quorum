"""Test the trend gate — BUY into a confirmed downtrend is blocked unless a reversal is
confirmed; conviction/reversal-risk are scored and position size is scaled by them.

Run:  python ai-layer/tests/test_trend_gate.py   (pytest not required)
Or:   pytest ai-layer/tests
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from strategy.trend_gate import apply_trend_gate, assess_trend


def _struct(**over):
    """Neutral 'ok' market structure; override keys to shape the trend."""
    base = {
        "quality": "ok", "price": 100.0, "atr_14": 2.0, "atr_pct": 0.02,
        "support_20": 92.0, "resistance_20": 112.0,
        "room_to_resistance_20": 0.12, "distance_to_support_20": 0.08,
        "rsi": 50.0, "rsi_divergence": None, "adx": 22.0,
        "ema12": 100.0, "ema26": 100.0, "ema50": 100.0,
        "macd": 0.0, "macd_signal": 0.0, "macd_hist": 0.0,
        "momentum_5": 0.0, "momentum_20": 0.0, "momentum_50": 0.0,
        "trend_structure": "ranging", "prob_up": 0.5, "efficiency_ratio": 0.2,
    }
    base.update(over)
    return base


def _downtrend(**over):
    s = _struct(ema12=96.0, ema26=98.0, ema50=100.0, price=95.0,
                trend_structure="downtrend", prob_up=0.30,
                momentum_20=-0.05, momentum_50=-0.08, macd=-1.0, macd_signal=-0.5)
    s.update(over)
    return s


def _uptrend(**over):
    s = _struct(ema12=104.0, ema26=102.0, ema50=100.0, price=105.0,
                trend_structure="uptrend", prob_up=0.70,
                momentum_5=0.02, momentum_20=0.05, momentum_50=0.08,
                macd=1.0, macd_signal=0.5, macd_hist=0.3, adx=28.0, efficiency_ratio=0.45)
    s.update(over)
    return s


def _ctx(structure, regime="unknown"):
    return {"last_price": structure.get("price", 100.0), "regime": regime,
            "market_structure": structure}


def _buy(**over):
    v = {"action": "BUY", "entry_type": "market", "entry_price": 100.0,
         "stop_price": 95.0, "target_price": 112.0, "confidence": 0.7,
         "suggested_size_pct": 0.5, "reasoning": "x"}
    v.update(over)
    return v


_CONS = {"votes": [{"ok": True}] * 4, "voted": 4, "agreement": 3}


# ─── the gate ────────────────────────────────────────────────────────────────────

def test_buy_into_confirmed_downtrend_is_blocked():
    out = apply_trend_gate(_buy(), _ctx(_downtrend(), "weak-trend"), _CONS)
    assert out["action"] == "HOLD" and out["entry_type"] == "none"
    assert out["trend_gate"] == "blocked" and out["trend_dir"] == "down"
    assert out["suggested_size_pct"] == 0.0


def test_downtrend_with_confirmed_reversal_is_allowed():
    # price reclaimed EMA26 + MACD bullish cross = 2 reversal signals → entry allowed
    s = _downtrend(price=99.0, macd=0.2, macd_signal=0.1, macd_hist=0.1)
    out = apply_trend_gate(_buy(), _ctx(s, "weak-trend"), _CONS)
    assert out["action"] == "BUY" and out["trend_gate"] == "reversal-confirmed"


def test_buy_in_uptrend_is_aligned():
    out = apply_trend_gate(_buy(), _ctx(_uptrend(), "trending"), _CONS)
    assert out["action"] == "BUY" and out["trend_gate"] == "aligned"
    assert out["trend_dir"] == "up"


def test_buy_in_sideways_is_allowed():
    out = apply_trend_gate(_buy(), _ctx(_struct(), "ranging"), _CONS)
    assert out["action"] == "BUY" and out["trend_dir"] == "sideways"
    assert out["trend_gate"] == "aligned"


def test_sell_is_never_blocked():
    v = {"action": "SELL", "entry_type": "market", "entry_price": 100.0,
         "confidence": 0.8, "suggested_size_pct": 0.8, "reasoning": "x"}
    out = apply_trend_gate(dict(v), _ctx(_downtrend(), "trending"), _CONS)
    assert out["action"] == "SELL"  # exits are not gated


def test_weak_structure_is_left_untouched():
    v = _buy()
    out = apply_trend_gate(dict(v), {"last_price": 100.0, "regime": "unknown",
                                     "market_structure": {"quality": "weak"}}, _CONS)
    assert out == v  # no data to judge trend → don't interfere


# ─── scoring ───────────────────────────────────────────────────────────────────────

def test_conviction_higher_when_aligned_and_trending():
    strong = assess_trend(_uptrend(), "trending", "BUY", agreement_ratio=1.0, vote_conf=0.9)
    weak = assess_trend(_struct(), "ranging", "BUY", agreement_ratio=0.5, vote_conf=0.5)
    assert strong.conviction > weak.conviction
    assert strong.conviction >= 0.7


def test_reversal_risk_higher_in_ranging_with_bearish_divergence():
    risky = assess_trend(_struct(rsi_divergence="bearish"), "ranging", "BUY")
    calm = assess_trend(_uptrend(), "trending", "BUY")
    assert risky.reversal_risk > calm.reversal_risk


def test_scores_attached_to_verdict():
    out = apply_trend_gate(_buy(), _ctx(_uptrend(), "trending"), _CONS)
    assert 0.0 <= out["conviction"] <= 1.0
    assert 0.0 <= out["reversal_risk"] <= 1.0


# ─── conviction-scaled sizing ────────────────────────────────────────────────────────

def test_size_never_exceeds_base_and_shrinks_when_shaky():
    base = 0.5
    strong = apply_trend_gate(_buy(suggested_size_pct=base), _ctx(_uptrend(), "trending"), _CONS)
    # a sideways/ranging range with a bearish divergence → low conviction, high reversal-risk
    shaky_s = _struct(rsi_divergence="bearish", efficiency_ratio=0.1)
    shaky = apply_trend_gate(_buy(suggested_size_pct=base), _ctx(shaky_s, "ranging"), _CONS)
    assert strong["suggested_size_pct"] <= base
    assert shaky["suggested_size_pct"] <= strong["suggested_size_pct"]
    assert shaky["suggested_size_pct"] >= 0.02


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    passed = 0
    for fn in fns:
        fn()
        print(f"  ✓ {fn.__name__}")
        passed += 1
    print(f"\n{passed}/{len(fns)} passed")


if __name__ == "__main__":
    _run_all()
