"""Persistent TTL cache for web/news lookups.

News and market context do NOT change every 15-minute analysis cycle, so re-searching the
same symbol every cycle is wasted network + unstable (DuckDuckGo/news endpoints throttle and
flake). This caches each lookup per symbol for a configurable window (default 12h) and — for
stability — serves the last good result when a live fetch fails ("stale-on-error").

Design goals: **never break analysis**. Every disk/JSON error is swallowed and degrades to a
plain fetch; an empty/failed fetch is not cached (so genuinely-missing data is retried, and a
network blip is not frozen in as "no news"). Storage = one small JSON file per key under a
cache dir (cwd/.webcache by default, override with QUORUM_CACHE_DIR) + an in-process layer.
The cache dir lives outside ai-layer/ so a redeploy (which replaces ai-layer/) keeps it warm.
"""
from __future__ import annotations

import hashlib
import json
import os
import time
from typing import Any, Callable

# in-process layer: key -> (timestamp, value)
_MEM: dict[str, tuple[float, Any]] = {}


def _dir() -> str:
    return os.environ.get("QUORUM_CACHE_DIR") or os.path.join(os.getcwd(), ".webcache")


def _path(key: str) -> str:
    h = hashlib.sha1(key.encode("utf-8")).hexdigest()[:16]
    return os.path.join(_dir(), f"{h}.json")


def _read_disk(key: str) -> tuple[float, Any] | None:
    try:
        with open(_path(key), "r", encoding="utf-8") as f:
            obj = json.load(f)
        return float(obj["ts"]), obj["value"]
    except Exception:
        return None


def _write_disk(key: str, ts: float, value: Any) -> None:
    try:
        os.makedirs(_dir(), exist_ok=True)
        tmp = _path(key) + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump({"key": key, "ts": ts, "value": value}, f)
        os.replace(tmp, _path(key))  # atomic; never leaves a half-written cache file
    except Exception:
        pass  # disk cache is best-effort — memory layer still works


def get_or_fetch(
    key: str,
    ttl_seconds: float,
    fetch: Callable[[], Any],
    ok: Callable[[Any], bool] = bool,
) -> dict[str, Any]:
    """Return a fresh-or-cached value for `key`.

    * Fresh cache hit (within ttl)            → return it, no fetch.
    * Miss/expired                            → call fetch(); cache only if ok(value) is truthy.
    * fetch() raises or returns not-ok        → serve the last good value if we have one
                                                (stale-on-error), else return the fetch result.

    Result dict: {value, cached: bool, age_s: int, stale: bool}. `ok` filters out empty/failed
    fetches so a transient network failure is never frozen in as a valid "no data" answer.
    """
    now = time.time()
    entry = _MEM.get(key)
    if entry is None:
        entry = _read_disk(key)
        if entry is not None:
            _MEM[key] = entry

    if entry is not None:
        ts, value = entry
        if now - ts <= ttl_seconds:
            return {"value": value, "cached": True, "age_s": int(now - ts), "stale": False}

    # need fresh data
    fresh: Any = None
    fresh_ok = False
    try:
        fresh = fetch()
        fresh_ok = bool(ok(fresh))
    except Exception:
        fresh_ok = False

    if fresh_ok:
        _MEM[key] = (now, fresh)
        _write_disk(key, now, fresh)
        return {"value": fresh, "cached": False, "age_s": 0, "stale": False}

    # fetch failed or returned nothing useful → fall back to the last good value if any
    if entry is not None:
        ts, value = entry
        return {"value": value, "cached": True, "age_s": int(now - ts), "stale": True}

    return {"value": fresh, "cached": False, "age_s": 0, "stale": False}
