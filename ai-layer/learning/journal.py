"""Durable journal for the self-learning layer.

Stores two things in ONE JSON file, written atomically under a lock:
  • "open"   — decisions whose outcome is not yet known (still being settled against new candles)
  • "stats"  — learned per-bucket aggregates that PERSIST even after individual decisions are pruned
  • "recent_live" — rolling list of the last live-trade R outcomes (portfolio-level circuit breaker)

CRITICAL — the data dir must live OUTSIDE ai-layer/, because the deploy swaps the whole ai-layer/
directory. Default: <runtime_root>/var/brain (runtime_root = parent of ai-layer/). Override with
the QUORUM_DATA_DIR env var. Everything degrades gracefully: any IO error returns empty state so a
learning hiccup can never block a trading decision.
"""
from __future__ import annotations

import json
import os
import tempfile
import threading
from pathlib import Path
from typing import Any

_LOCK = threading.RLock()
_VERSION = 1
# keep the file bounded
_MAX_OPEN = 600          # open (unsettled) decisions retained
_MAX_RECENT_LIVE = 40    # rolling window for the portfolio circuit breaker


def data_dir() -> Path:
    """Resolve the persistent data dir (survives ai-layer/ swaps on deploy)."""
    env = os.environ.get("QUORUM_DATA_DIR")
    if env:
        return Path(env)
    # journal.py → learning/ → ai-layer/ → <runtime_root>
    root = Path(__file__).resolve().parent.parent.parent
    return root / "var" / "brain"


def _store_path() -> Path:
    return data_dir() / "brain.json"


def _empty() -> dict[str, Any]:
    return {"version": _VERSION, "open": [], "stats": {}, "recent_live": []}


def load() -> dict[str, Any]:
    """Load state; never raises — returns empty state on any problem."""
    with _LOCK:
        p = _store_path()
        try:
            if not p.exists():
                return _empty()
            with open(p, "r", encoding="utf-8") as f:
                data = json.load(f)
            if not isinstance(data, dict):
                return _empty()
            data.setdefault("version", _VERSION)
            data.setdefault("open", [])
            data.setdefault("stats", {})
            data.setdefault("recent_live", [])
            return data
        except Exception:
            return _empty()


def save(data: dict[str, Any]) -> bool:
    """Atomically persist state; never raises — returns False on failure."""
    with _LOCK:
        try:
            # bound the growth
            if len(data.get("open", [])) > _MAX_OPEN:
                data["open"] = data["open"][-_MAX_OPEN:]
            if len(data.get("recent_live", [])) > _MAX_RECENT_LIVE:
                data["recent_live"] = data["recent_live"][-_MAX_RECENT_LIVE:]
            d = data_dir()
            d.mkdir(parents=True, exist_ok=True)
            fd, tmp = tempfile.mkstemp(dir=str(d), prefix=".brain.", suffix=".tmp")
            try:
                with os.fdopen(fd, "w", encoding="utf-8") as f:
                    json.dump(data, f, ensure_ascii=False)
                os.replace(tmp, str(_store_path()))
            finally:
                if os.path.exists(tmp):
                    try:
                        os.remove(tmp)
                    except OSError:
                        pass
            return True
        except Exception:
            return False
