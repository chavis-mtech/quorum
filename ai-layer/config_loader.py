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
    "agents": {"technical": True, "finbert": True, "cryptobert": True,
               "trend_ml": True, "news": True},
    "consensus": {"min_agreement": 3, "min_confidence": 0.60,
                  "weights": {"technical": 1.0, "finbert": 0.9, "cryptobert": 0.9,
                              "trend_ml": 1.0, "news": 0.8}},
    "judge": {"enabled": True, "provider": "ollama", "model": "qwen3:14b",
              "ollama_url": "http://localhost:11434", "base_url": "",
              "api_key": "", "thinking": True, "fallback": ["ollama", "none"]},
    "news": {"provider": "auto", "lookback_hours": 48, "max_articles": 20},
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
