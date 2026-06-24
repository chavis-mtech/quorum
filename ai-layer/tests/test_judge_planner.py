"""Tests for the rule-based planner RR + stop clamp — the fix that makes the proven edge tradeable.

The self-learning brain proved the edge (ranging|up|mid 74% win) at RR≈1.6-1.7, but the old planner
demanded RR≥2.5 in a ranging market and so HOLD'd every winner. These lock in:
  • a ranging BUY now produces a real plan whose RR clears the backend's hard MIN_REWARD_RISK=1.35
  • the planned stop is never wider than the backend's −6% catastrophic cap (no planned-vs-realized skew)
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from judge import _plan_from_consensus

_MIN_REWARD_RISK = 1.35  # backend const (validate_long_plan)


def _ctx(price, atr_pct, support, resistance, regime):
    return {
        "last_price": price,
        "regime": regime,
        "market_structure": {
            "quality": "ok", "price": price, "atr_14": price * atr_pct, "atr_pct": atr_pct,
            "support_20": support, "resistance_20": resistance, "rsi": 55.0,
            "efficiency_ratio": 0.25, "adx": 22.0,
            # keep the trend gate from interfering (clean uptrend)
            "ema12": price * 1.01, "ema26": price * 1.005, "ema50": price,
            "trend_structure": "uptrend", "prob_up": 0.65,
            "momentum_20": 0.02, "momentum_50": 0.03, "room_to_resistance_20": 0.06,
        },
    }


def _consensus(action="BUY", conf=0.6, regime="ranging"):
    return {"action": action, "confidence": conf, "regime": regime,
            "passed_threshold": True, "reasoning": "test"}


def test_ranging_buy_now_produces_a_tradeable_plan():
    # ranging market with room to a resistance well above price → used to HOLD at RR<2.5
    ctx = _ctx(price=100.0, atr_pct=0.02, support=97.0, resistance=110.0, regime="ranging")
    v = _plan_from_consensus(_consensus(regime="ranging"), "test", ctx)
    assert v["action"] == "BUY"
    entry, target, stop = v["entry_price"], v["target_price"], v["stop_price"]
    rr = (target - entry) / (entry - stop)
    assert rr >= _MIN_REWARD_RISK, f"RR {rr:.2f} must clear backend floor {_MIN_REWARD_RISK}"


def test_uptrend_breakout_target_unblocks_buy_near_resistance():
    # price just under resistance: capping target AT resistance gives RR<floor → HOLD.
    # In a confirmed uptrend the planner now projects the target a measured step ABOVE
    # resistance (res + ATR for trending) so the proven-winner setup clears the RR bar.
    # support far → stop = 1.5*ATR (tight); res only ~3.3% up.
    ctx = _ctx(price=100.0, atr_pct=0.02, support=90.0, resistance=103.3, regime="trending")
    v = _plan_from_consensus(_consensus(regime="trending"), "test", ctx)
    assert v["action"] == "BUY", "confirmed uptrend near resistance must now be tradeable"
    entry, target, stop = v["entry_price"], v["target_price"], v["stop_price"]
    assert target > 103.3, "target should project above resistance in a confirmed uptrend"
    assert (target - entry) / (entry - stop) >= _MIN_REWARD_RISK


def test_ranging_still_caps_target_at_resistance():
    # the mean-reversion discipline must stay: a RANGE never projects past the range top,
    # so the same near-resistance geometry stays HOLD (RR<floor) in a ranging regime.
    ctx = _ctx(price=100.0, atr_pct=0.02, support=90.0, resistance=103.3, regime="ranging")
    v = _plan_from_consensus(_consensus(regime="ranging"), "test", ctx)
    if v["action"] == "BUY":
        assert v["target_price"] <= 103.3 + 1e-6, "ranging target must stay capped at resistance"


def test_planned_stop_never_wider_than_catastrophic_cap():
    # a low support (would imply a ~-9% stop) must be clamped to ≥ -5.5%
    ctx = _ctx(price=100.0, atr_pct=0.04, support=91.0, resistance=112.0, regime="trending")
    v = _plan_from_consensus(_consensus(regime="trending"), "test", ctx)
    if v["action"] == "BUY":
        assert v["stop_price"] >= 100.0 * (1.0 - 0.055) - 1e-6, \
            f"planned stop {v['stop_price']} must not be wider than -5.5%"
