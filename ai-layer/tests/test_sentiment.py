"""Test the consolidated sentiment agent — always votes (never abstains like the old finbert/
cryptobert without torch), handles negation + intensity + de-duplication, capped confidence.

Run:  python ai-layer/tests/test_sentiment.py   (pytest not required)
Or:   pytest ai-layer/tests
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from agents.base import MarketContext, BUY, SELL, HOLD
from agents.sentiment import SentimentAgent


def _ctx(headlines):
    return MarketContext("BTC", "THB", [], 100.0, extra={"headlines": headlines})


def _analyze(headlines):
    return SentimentAgent().analyze(_ctx(headlines))


def test_bullish_news_votes_buy():
    r = _analyze(["Bitcoin surges to record high as spot ETF inflows soar",
                  "Major partnership drives bullish rally and adoption"])
    assert r.action == BUY and r.ok is True
    assert r.confidence <= 0.60  # supporting lens — confidence is capped


def test_bearish_news_votes_sell():
    r = _analyze(["Bitcoin crashes in massive selloff after exchange hacked",
                  "Regulators announce ban amid fraud investigation"])
    assert r.action == SELL and r.ok is True


def test_negation_flips_polarity():
    # "not a bullish breakout" must NOT read as bullish
    r = _analyze(["Analysts warn this is not a bullish breakout"])
    assert r.action != BUY


def test_no_headlines_abstains():
    r = _analyze([])
    assert r.ok is False  # genuinely no data → fail (only legitimate abstain reason)


def test_always_votes_when_headlines_exist():
    # the whole point of the consolidation: with headlines present it ALWAYS casts a real vote
    # (the old finbert/cryptobert abstained on every cycle when torch was missing)
    r = _analyze(["Ethereum sees steady inflows and a new mainnet milestone"])
    assert r.ok is True and r.action in (BUY, SELL, HOLD)


def test_duplicate_headlines_counted_once():
    h = "Bitcoin surges to a record high on strong ETF inflows"
    assert _analyze([h]).extra["headlines"] == 1
    assert _analyze([h, h, h]).extra["headlines"] == 1  # de-duplicated, not amplified


def test_intensity_strengthens_signal():
    mild = _analyze(["Bitcoin sees a selloff today"]).extra["net"]
    strong = _analyze(["Bitcoin sees a massive selloff today"]).extra["net"]
    assert strong < mild  # 'massive' intensifies the bearish cue


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
