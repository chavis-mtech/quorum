"""Tests for learning.edge — bucketing + the block/allow/calibrate decision."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from learning import edge


def test_rsi_zone_and_bucket_key():
    assert edge.rsi_zone(20) == "os"
    assert edge.rsi_zone(40) == "low"
    assert edge.rsi_zone(55) == "mid"
    assert edge.rsi_zone(80) == "hot"
    assert edge.bucket_key("Trending", "Up", 55) == "trending|up|mid"


def test_update_stats_accumulates():
    stats: dict = {}
    edge.update_stats(stats, "b", 2.0)
    edge.update_stats(stats, "b", -1.0)
    s = stats["b"]
    assert s["n"] == 2 and s["wins"] == 1 and abs(s["sum_r"] - 1.0) < 1e-9


def test_unknown_bucket_is_explored_live():
    d = edge.decide({}, "trending|up|mid", [])
    assert d["action"] == "allow"
    assert "exploring" in d["reason"]


def test_proven_loser_bucket_is_blocked():
    # 10 trades, all losers (-1R) → expectancy -1 ≤ block_expectancy
    stats = {"ranging|sideways|hot": {"n": 10, "wins": 0, "sum_r": -10.0}}
    d = edge.decide(stats, "ranging|sideways|hot", [])
    assert d["action"] == "block"
    assert "learned loser" in d["reason"]


def test_positive_bucket_allowed_with_confidence_boost():
    stats = {"trending|up|mid": {"n": 12, "wins": 9, "sum_r": 6.0}}  # +0.5R avg
    d = edge.decide(stats, "trending|up|mid", [])
    assert d["action"] == "allow"
    assert d["conf_mult"] > 1.0


def test_circuit_breaker_pauses_unproven_buckets():
    # recent live trades bleeding badly, current bucket has no proven edge → block
    recent_live = [-0.5] * 12
    d = edge.decide({}, "weak-trend|sideways|low", recent_live)
    assert d["action"] == "block"
    assert d["defensive"] is True
    assert "defensive mode" in d["reason"]


def test_circuit_breaker_still_allows_proven_positive_bucket():
    recent_live = [-0.5] * 12
    stats = {"trending|up|mid": {"n": 10, "wins": 8, "sum_r": 5.0}}  # +0.5R, proven
    d = edge.decide(stats, "trending|up|mid", recent_live)
    assert d["action"] == "allow"
