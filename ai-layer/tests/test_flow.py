"""Tests for the flow agent — money-flow / volume pressure (web-free)."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from agents.base import MarketContext
from agents.flow import FlowAgent


def _accumulation(n=40, vol=10.0):
    """Rising price, closes printing near the highs, volume on up bars → accumulation."""
    candles = []
    price = 100.0
    for _ in range(n):
        openp = price
        close = openp * 1.006
        candles.append({"ts": 0, "open": openp, "high": close * 1.0008,
                        "low": openp * 0.999, "close": close, "volume": vol})
        price = close
    return candles


def _distribution(n=40, vol=10.0):
    """Falling price, closes printing near the lows → distribution."""
    candles = []
    price = 100.0
    for _ in range(n):
        openp = price
        close = openp * 0.994
        candles.append({"ts": 0, "open": openp, "high": openp * 1.001,
                        "low": close * 0.9992, "close": close, "volume": vol})
        price = close
    return candles


def test_accumulation_votes_buy():
    r = FlowAgent().analyze(MarketContext("BTC", "THB", _accumulation()))
    assert r.ok is True
    assert r.action == "BUY"
    assert r.extra["cmf"] > 0
    assert r.extra["obv_slope"] > 0


def test_distribution_votes_sell():
    r = FlowAgent().analyze(MarketContext("BTC", "THB", _distribution()))
    assert r.ok is True
    assert r.action == "SELL"
    assert r.extra["cmf"] < 0


def test_no_volume_abstains():
    candles = _accumulation()
    for c in candles:
        c["volume"] = 0.0
    r = FlowAgent().analyze(MarketContext("BTC", "THB", candles))
    assert r.ok is False  # abstains cleanly instead of voting noise


def test_too_few_candles_fails():
    r = FlowAgent().analyze(MarketContext("BTC", "THB", _accumulation(n=10)))
    assert r.ok is False
