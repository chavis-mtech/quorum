"""Integration tests for the self-learning brain — record → settle → gate."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from learning import brain, journal

STEP = 3600


def _candles(n, start_ts=0, base=100.0):
    """Flat-ish rising series with real volume; ts increases by STEP."""
    out = []
    price = base
    for i in range(n):
        openp = price
        close = openp * 1.0005
        out.append({"ts": start_ts + i * STEP, "open": openp, "high": close * 1.001,
                    "low": openp * 0.999, "close": close, "volume": 10.0})
        price = close
    return out


def _structure(price):
    """Enriched structure that the trend gate reads as a clean UPtrend."""
    return {
        "quality": "ok", "price": price, "atr_pct": 0.02, "rsi": 55.0,
        "efficiency_ratio": 0.45, "adx": 30.0,
        "ema12": price * 1.01, "ema26": price * 1.005, "ema50": price,
        "trend_structure": "uptrend", "prob_up": 0.7,
        "momentum_20": 0.03, "momentum_50": 0.05,
        "room_to_resistance_20": 0.05, "support_20": price * 0.97,
        "macd": 1.0, "macd_signal": 0.5, "macd_hist": 0.5,
    }


def _verdict(action="BUY", entry=100.0, target=103.0, stop=98.0, conf=0.7):
    return {"action": action, "entry_price": entry, "target_price": target,
            "stop_price": stop, "confidence": conf, "reasoning": "base",
            "conviction": 0.6, "reversal_risk": 0.2}


def _cfg():
    return {"learning": {}, "market_quality": {}}


def test_records_a_live_buy_then_settles_it_as_a_win(monkeypatch, tmp_path):
    monkeypatch.setenv("QUORUM_DATA_DIR", str(tmp_path))
    cfg = _cfg()
    consensus = {"action": "BUY", "regime": "trending"}

    # cycle 1: clean BUY → recorded live, nothing settled yet
    c1 = _candles(60)
    price = c1[-1]["close"]
    info1 = brain.evaluate("BTC", c1, _structure(price), consensus,
                           _verdict(entry=price, target=price * 1.03, stop=price * 0.98),
                           "trending", False, cfg)
    assert info1["enabled"] is True
    assert info1["recorded"] == "live"
    assert info1["block"] is False
    state = journal.load()
    assert len(state["open"]) == 1 and state["open"][0]["kind"] == "live"

    # cycle 2: add bars that spike through the target → the open live trade settles as a win
    entry_ts = c1[-1]["ts"]
    spike = [{"ts": entry_ts + STEP, "open": price, "high": price * 1.04,
              "low": price * 0.999, "close": price * 1.035, "volume": 12.0}]
    c2 = c1 + spike
    info2 = brain.evaluate("BTC", c2, _structure(c2[-1]["close"]), consensus,
                           _verdict(), "trending", False, cfg)
    assert info2["overall"]["n"] >= 1          # something settled
    assert info2["live_scoreboard"]["n"] >= 1  # and it counted as a live outcome
    assert info2["live_scoreboard"]["expectancy"] > 0  # the win was positive R


def test_proven_loser_bucket_blocks_and_apply_converts_to_hold(monkeypatch, tmp_path):
    monkeypatch.setenv("QUORUM_DATA_DIR", str(tmp_path))
    # pre-seed the journal so the current setup's bucket is a proven loser
    bucket = "trending|up|mid"
    journal.save({"version": 1, "open": [],
                  "stats": {bucket: {"n": 12, "wins": 1, "sum_r": -9.0}},
                  "recent_live": []})

    c = _candles(60)
    price = c[-1]["close"]
    v = _verdict(entry=price, target=price * 1.03, stop=price * 0.98)
    info = brain.evaluate("BTC", c, _structure(price), {"action": "BUY", "regime": "trending"},
                          v, "trending", False, _cfg())
    assert info["block"] is True
    assert info["block_kind"] == "learned"

    out = brain.apply(v, info)
    assert out["action"] == "HOLD"
    assert out["entry_price"] == 0.0
    assert "learning-gate" in out["reasoning"]


def test_chop_market_blocks_buy(monkeypatch, tmp_path):
    monkeypatch.setenv("QUORUM_DATA_DIR", str(tmp_path))
    c = _candles(60)
    price = c[-1]["close"]
    s = _structure(price)
    s.update({"efficiency_ratio": 0.08, "adx": 14.0})  # chop
    info = brain.evaluate("BTC", c, s, {"action": "BUY", "regime": "ranging"},
                          _verdict(entry=price, target=price * 1.03, stop=price * 0.98),
                          "ranging", False, _cfg())
    assert info["block"] is True
    assert info["block_kind"] == "quality"


def test_reentry_cooldown_blocks_buy_right_after_a_loss(monkeypatch, tmp_path):
    monkeypatch.setenv("QUORUM_DATA_DIR", str(tmp_path))
    cfg = _cfg()
    consensus = {"action": "BUY", "regime": "trending"}

    # cycle 1: record a live BUY at a high entry; stop just below
    c1 = _candles(60)
    price = c1[-1]["close"]
    brain.evaluate("BTC", c1, _structure(price), consensus,
                   _verdict(entry=price, target=price * 1.03, stop=price * 0.99),
                   "trending", False, cfg)

    # cycle 2: next bar gaps down through the stop → the trade settles as a loss (≤ -0.5R),
    # which arms the re-entry cooldown
    entry_ts = c1[-1]["ts"]
    crash = [{"ts": entry_ts + STEP, "open": price, "high": price * 1.001,
              "low": price * 0.97, "close": price * 0.975, "volume": 11.0}]
    c2 = c1 + crash
    p2 = c2[-1]["close"]
    info2 = brain.evaluate("BTC", c2, _structure(p2), consensus,
                           _verdict(entry=p2, target=p2 * 1.03, stop=p2 * 0.98),
                           "trending", False, cfg)
    # a fresh BUY on BTC in the same/next bar must be blocked by the cooldown
    assert info2["block"] is True
    assert info2["block_kind"] == "cooldown"


def test_synthetic_data_skips_learning(monkeypatch, tmp_path):
    monkeypatch.setenv("QUORUM_DATA_DIR", str(tmp_path))
    info = brain.evaluate("BTC", _candles(60), _structure(100.0), {"action": "BUY"},
                          _verdict(), "trending", True, _cfg())  # synthetic=True
    assert info["enabled"] is False
