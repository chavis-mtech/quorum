"""Bitkub public data provider — price + candles (no API key required)

Uses public endpoints:
  - GET https://api.bitkub.com/api/v3/market/ticker
  - GET https://api.bitkub.com/tradingview/history  (OHLC)

If the network is down or data cannot be fetched, returns synthetic candles
so the cycle can be tested offline — flagged with extra["synthetic"]=True
"""
from __future__ import annotations

import json
import math
import time
import urllib.request
from typing import Any

_BASE = "https://api.bitkub.com"

_INTERVAL_SECONDS = {
    "1m": 60, "5m": 300, "15m": 900, "1h": 3600, "4h": 14400, "1d": 86400,
}
# resolutions supported by the Bitkub tradingview endpoint
_INTERVAL_RES = {
    "1m": "1", "5m": "5", "15m": "15", "1h": "60", "4h": "240", "1d": "1D",
}


def _http_get(url: str, timeout: float = 10.0) -> Any:
    req = urllib.request.Request(url, headers={"User-Agent": "quorum/0.1"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _synthetic_candles(symbol: str, n: int, interval: str) -> list[dict[str, float]]:
    """Generate deterministic synthetic candles (for offline testing)."""
    step = _INTERVAL_SECONDS.get(interval, 3600)
    base = 1_000_000.0 if symbol.upper() == "BTC" else 50_000.0
    candles: list[dict[str, float]] = []
    price = base
    now = int(time.time())
    for i in range(n):
        # sine wave + slight drift (no randomness, so results are reproducible)
        wave = math.sin(i / 7.0) * base * 0.01
        drift = (i - n / 2) * base * 0.0002
        close = base + wave + drift
        openp = price
        high = max(openp, close) * 1.003
        low = min(openp, close) * 0.997
        candles.append({
            "ts": now - (n - i) * step,
            "open": openp, "high": high, "low": low,
            "close": close, "volume": 10 + (i % 5),
        })
        price = close
    return candles


def get_last_price(symbol: str, quote: str = "THB") -> float | None:
    pair = f"{symbol}_{quote}".upper()
    legacy_pair = f"{quote}_{symbol}".upper()
    try:
        data = _http_get(f"{_BASE}/api/v3/market/ticker")
        if isinstance(data, list):
            entry = next((it for it in data if str(it.get("symbol", "")).upper() == pair), {})
        else:
            entry = data.get(pair) or data.get(legacy_pair) or data.get("result", {}).get(pair, {})
        last = entry.get("last")
        return float(last) if last is not None else None
    except Exception:
        return None


def get_candles(symbol: str, quote: str = "THB", interval: str = "1h",
                lookback: int = 200) -> dict[str, Any]:
    """Return {'candles': [...], 'synthetic': bool, 'source': str}"""
    # tradingview/history uses BASE_QUOTE format (e.g. BTC_THB)
    pair = f"{symbol}_{quote}".upper()
    res = _INTERVAL_RES.get(interval, "60")
    step = _INTERVAL_SECONDS.get(interval, 3600)
    to_ts = int(time.time())
    from_ts = to_ts - step * (lookback + 5)
    url = (f"{_BASE}/tradingview/history"
           f"?symbol={pair}&resolution={res}&from={from_ts}&to={to_ts}")
    try:
        data = _http_get(url)
        if data.get("s") == "ok" and data.get("c"):
            candles = [
                {"ts": data["t"][i], "open": float(data["o"][i]),
                 "high": float(data["h"][i]), "low": float(data["l"][i]),
                 "close": float(data["c"][i]), "volume": float(data["v"][i])}
                for i in range(len(data["c"]))
            ][-lookback:]
            if len(candles) >= 30:
                return {"candles": candles, "synthetic": False, "source": "bitkub"}
    except Exception:
        pass
    # offline fallback
    return {"candles": _synthetic_candles(symbol, lookback, interval),
            "synthetic": True, "source": "synthetic"}
