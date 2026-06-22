"""Load config/quorum.toml (supports Python 3.11+ via tomllib)"""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any

try:
    import tomllib  # py3.11+
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None  # type: ignore

_DEFAULT_PATH = Path(__file__).resolve().parent.parent / "config" / "quorum.toml"

_DEFAULTS: dict[str, Any] = {
    "general": {"mode": "signal-only"},
    "market": {"broker": "bitkub", "symbols": ["BTC"], "quote": "THB",
               "candle_interval": "1h", "candle_lookback": 200},
    # council = price+volume voices (technical/trend_ml/flow). `flow` (money-flow/volume) replaces
    # the old web-scraped news+sentiment pair; those only run when [news].enabled is true.
    "agents": {"technical": True, "trend_ml": True, "flow": True,
               "sentiment": True, "news": True},
    # min_confidence 0.52 → BUY consensus bar 0.55 (asymmetric +0.03). trend_ml damps ranging
    # confidence, so proven-winner setups cluster at council ~0.55; 0.55 admits them, 0.58 would
    # not. The aggregator runs BEFORE the brain's conf boost, so this is the binding gate.
    "consensus": {"min_agreement": 3, "min_confidence": 0.52,
                  "weights": {"technical": 1.0, "trend_ml": 1.0, "flow": 0.8,
                              "sentiment": 0.6, "news": 0.8}},
    "judge": {"enabled": True, "provider": "ollama", "model": "qwen3:14b",
              "ollama_url": "http://localhost:11434", "base_url": "",
              "api_key": "", "thinking": True, "fallback": ["ollama", "none"]},
    # news/web fetch OFF by default — flaky/edge-less on a small box. enabled=true restores it.
    "news": {"enabled": False, "provider": "auto", "lookback_hours": 48,
             "max_articles": 20, "cache_ttl_hours": 12},
    # self-learning brain — settles its own past calls vs candles, learns per-setup expectancy,
    # blocks proven-loser setups. See ai-layer/learning/edge.py for what each knob does.
    "learning": {"enabled": True, "fee_per_side": 0.0025, "max_settle_bars": 48,
                 "min_samples_gate": 6, "min_samples_conf": 5, "block_expectancy": -0.08,
                 "block_winrate_n": 8, "block_winrate": 0.34, "breaker_min_samples": 10,
                 "breaker_expectancy": -0.15, "conf_span": 0.5, "conf_lo": 0.75, "conf_hi": 1.25},
    # deterministic "don't trade garbage" guard. Loosened for "small/risky coins ok": liquidity
    # floor 15k (only the very thinnest blocked) and ATR ceiling 4%/bar (1.5×ATR stop = the -6%
    # cap). Downside stays bounded by the planner's -5.5% stop clamp + the backend MAX_LOSS_PCT.
    "market_quality": {"enabled": True, "fee_per_side": 0.0025, "min_target_move": 0.012,
                       "min_net_rr": 1.2, "chop_er": 0.12, "chop_adx": 18.0,
                       "flat_atr_pct": 0.003, "min_liquidity_thb": 15000.0,
                       "max_atr_pct": 0.04},
    "risk": {"max_position_pct": 0.10, "daily_loss_limit": 0.05,
             "max_open_positions": 5},
}


def _merge(base: dict, over: dict) -> dict:
    out = dict(base)
    for k, v in over.items():
        if isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = _merge(out[k], v)
        else:
            out[k] = v
    return out


def load_config(path: str | os.PathLike | None = None) -> dict[str, Any]:
    p = Path(path) if path else _DEFAULT_PATH
    if tomllib is None or not p.exists():
        return _DEFAULTS
    with open(p, "rb") as f:
        loaded = tomllib.load(f)
    return _merge(_DEFAULTS, loaded)
