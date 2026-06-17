"""Test vote aggregation logic — key cases: veto, below threshold, split vote, passes

Run:  python ai-layer/tests/test_aggregator.py   (pytest not required)
Or:   pytest ai-layer/tests
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from agents.base import AgentResult, BUY, SELL, HOLD
from aggregator import aggregate

W = {"technical": 1.0, "finbert": 0.9, "trend_ml": 1.0, "news": 0.8}


def _r(agent, action, conf, can_veto=False, veto=False):
    return AgentResult(agent=agent, action=action, confidence=conf,
                       reasoning="t", can_veto=can_veto, veto=veto)


def test_veto_forces_hold():
    res = aggregate([
        _r("technical", BUY, 0.9), _r("trend_ml", BUY, 0.9),
        _r("finbert", BUY, 0.9),
        _r("news", HOLD, 0.9, can_veto=True, veto=True),
    ], weights=W, min_agreement=3, min_confidence=0.6)
    assert res.action == HOLD and res.vetoed is True


def test_below_agreement_threshold():
    # only 2 agents agree < min_agreement=3
    res = aggregate([
        _r("technical", BUY, 0.9), _r("trend_ml", BUY, 0.9),
        _r("finbert", SELL, 0.8), _r("news", SELL, 0.8),
    ], weights=W, min_agreement=3, min_confidence=0.6)
    assert res.action == HOLD and res.passed_threshold is False


def test_below_confidence_threshold():
    res = aggregate([
        _r("technical", BUY, 0.4), _r("trend_ml", BUY, 0.4),
        _r("finbert", BUY, 0.4), _r("news", HOLD, 0.3),
    ], weights=W, min_agreement=3, min_confidence=0.6)
    assert res.action == HOLD  # 3 agree but average confidence < 0.6


def test_passes_consensus():
    res = aggregate([
        _r("technical", BUY, 0.8), _r("trend_ml", BUY, 0.75),
        _r("finbert", BUY, 0.7), _r("news", HOLD, 0.3),
    ], weights=W, min_agreement=3, min_confidence=0.6)
    assert res.action == BUY and res.passed_threshold is True
    assert res.agreement == 3


def test_split_vote_picks_weighted_winner_but_holds_if_low_agreement():
    res = aggregate([
        _r("technical", BUY, 0.9), _r("trend_ml", SELL, 0.9),
    ], weights=W, min_agreement=3, min_confidence=0.6)
    assert res.action == HOLD  # too few agents to meet threshold


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
