"""Tests for learning.settle — replaying candles into win/loss/expired outcomes."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from learning import settle


def _bar(hi, lo, close):
    return {"high": hi, "low": lo, "close": close, "open": close, "volume": 1.0}


def test_target_hit_is_a_win_net_of_fees():
    # entry 100, target 110, stop 95 → risk 5. A bar that reaches 110 wins.
    bars = [_bar(108, 99, 105), _bar(111, 106, 110)]
    r = settle.settle_long(100, 110, 95, bars, fee_per_side=0.0025)
    assert r["status"] == "win"
    assert r["exit"] == 110
    # gross R = (110-100)/5 = 2.0; fees (100+110)*0.0025=0.525 → net (10-0.525)/5 ≈ 1.895
    assert 1.85 < r["r"] < 1.92
    assert r["bars_held"] == 2


def test_stop_hit_is_a_loss():
    bars = [_bar(103, 94, 96)]  # low 94 ≤ stop 95
    r = settle.settle_long(100, 110, 95, bars)
    assert r["status"] == "loss"
    assert r["exit"] == 95
    assert r["r"] < -1.0  # a clean stop plus fees is slightly worse than -1R


def test_straddle_bar_is_conservatively_a_loss():
    # one bar that trades through BOTH stop and target → assume stop first
    bars = [_bar(112, 93, 100)]
    r = settle.settle_long(100, 110, 95, bars)
    assert r["status"] == "loss"


def test_unresolved_stays_open_until_max_bars():
    bars = [_bar(104, 99, 101), _bar(105, 100, 102)]
    r = settle.settle_long(100, 110, 95, bars, max_bars=48)
    assert r["status"] == "open"
    assert r["r"] is None


def test_expired_marks_to_market_at_last_close():
    bars = [_bar(104, 99, 103)]  # never hits 110 or 95
    r = settle.settle_long(100, 110, 95, bars, max_bars=1)
    assert r["status"] == "expired"
    assert r["exit"] == 103


def test_invalid_levels():
    assert settle.settle_long(100, 110, 105, [])["status"] == "invalid"  # stop above entry
