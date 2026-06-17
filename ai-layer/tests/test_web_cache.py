"""Test the web/news TTL cache: fresh fetch → cache hit within TTL → refetch after expiry,
empty results aren't cached, a failed fetch serves stale data, and disk survives a restart.

Run:  python ai-layer/tests/test_web_cache.py   (no network — fetch fns are fakes)
"""
from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
os.environ["QUORUM_CACHE_DIR"] = tempfile.mkdtemp(prefix="quorum-webcache-test-")

from providers import web_cache

_OK = lambda v: bool(v and v.get("count", 0) > 0)


def test_fetch_then_cache_hit():
    web_cache._MEM.clear()
    n = {"calls": 0}

    def fetch():
        n["calls"] += 1
        return {"count": 3, "data": "X"}

    r1 = web_cache.get_or_fetch("k1", 1000, fetch, ok=_OK)
    assert r1["cached"] is False and r1["value"]["data"] == "X" and n["calls"] == 1
    r2 = web_cache.get_or_fetch("k1", 1000, fetch, ok=_OK)
    assert r2["cached"] is True and n["calls"] == 1          # served from cache, no 2nd fetch


def test_expiry_refetches():
    web_cache._MEM.clear()
    n = {"calls": 0}

    def fetch():
        n["calls"] += 1
        return {"count": 1}

    web_cache.get_or_fetch("k2", 1000, fetch, ok=_OK)
    ts, val = web_cache._MEM["k2"]
    web_cache._MEM["k2"] = (ts - 10_000, val)               # backdate → now expired
    web_cache.get_or_fetch("k2", 100, fetch, ok=_OK)
    assert n["calls"] == 2


def test_empty_result_not_cached():
    web_cache._MEM.clear()
    r = web_cache.get_or_fetch("k3", 1000, lambda: {"count": 0}, ok=_OK)
    assert r["cached"] is False
    assert "k3" not in web_cache._MEM                        # empty → retried next cycle, not frozen


def test_stale_on_error():
    web_cache._MEM.clear()
    web_cache.get_or_fetch("k4", 1000, lambda: {"count": 2, "v": "good"}, ok=_OK)
    ts, val = web_cache._MEM["k4"]
    web_cache._MEM["k4"] = (ts - 10_000, val)               # expire it

    def boom():
        raise RuntimeError("network down")

    r = web_cache.get_or_fetch("k4", 100, boom, ok=_OK)      # live fetch fails
    assert r["stale"] is True and r["value"]["v"] == "good"  # last good value served


def test_disk_survives_memory_clear():
    web_cache._MEM.clear()
    web_cache.get_or_fetch("k5", 1000, lambda: {"count": 1, "v": "disk"}, ok=_OK)
    web_cache._MEM.clear()                                   # simulate sidecar restart
    n = {"calls": 0}

    def fetch():
        n["calls"] += 1
        return {"count": 9}

    r = web_cache.get_or_fetch("k5", 1000, fetch, ok=_OK)
    assert r["cached"] is True and n["calls"] == 0 and r["value"]["v"] == "disk"


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"  ✓ {fn.__name__}")
    print(f"\n{len(fns)}/{len(fns)} passed")


if __name__ == "__main__":
    _run_all()
