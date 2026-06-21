"""Tests for strategy.market_quality — the don't-trade-garbage guard."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from strategy import market_quality


def _structure(er=0.4, adx=30.0, atr_pct=0.02):
    return {"efficiency_ratio": er, "adx": adx, "atr_pct": atr_pct, "price": 100.0}


def test_good_setup_passes():
    q = market_quality.assess_quality(_structure(), "trending", 100, 105, 97)
    assert q["ok"] is True


def test_flat_market_blocked():
    q = market_quality.assess_quality(_structure(atr_pct=0.001), "ranging", 100, 105, 97)
    assert q["ok"] is False
    assert "flat market" in q["reason"]


def test_chop_blocked():
    q = market_quality.assess_quality(_structure(er=0.08, adx=15.0), "ranging", 100, 105, 97)
    assert q["ok"] is False
    assert "chop" in q["reason"]


def test_fee_unviable_target_blocked():
    # target only 0.5% away → cannot clear ~0.5% round-trip fee
    q = market_quality.assess_quality(_structure(), "trending", 100, 100.5, 99)
    assert q["ok"] is False
    assert "round-trip fee" in q["reason"] or "net RR" in q["reason"]


def test_thin_net_rr_blocked():
    # target 1.5% away but stop is far → net RR after fees too thin
    q = market_quality.assess_quality(_structure(), "trending", 100, 101.5, 95)
    assert q["ok"] is False
    assert "net RR" in q["reason"]
