"""News provider — fetches news headlines from the network for the verification layer

Supported providers (selected automatically based on set env vars):
  - Finnhub   : requires FINNHUB_API_KEY
  - NewsAPI   : requires NEWSAPI_KEY
  - none      : returns [] (system still runs, news/finbert agent will report no data)

Do not hardcode keys — read from environment variables only
"""
from __future__ import annotations

import json
import os
import time
import urllib.parse
import urllib.request
from typing import Any

# Full names to help query news matching the asset
_NAME = {
    "BTC": "Bitcoin", "ETH": "Ethereum", "XRP": "Ripple XRP",
    "ADA": "Cardano", "DOGE": "Dogecoin", "SOL": "Solana",
}


def _http_get(url: str, timeout: float = 10.0) -> Any:
    req = urllib.request.Request(url, headers={"User-Agent": "quorum/0.1"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _finnhub(symbol: str, lookback_hours: int, limit: int) -> list[str]:
    key = os.environ.get("FINNHUB_API_KEY")
    if not key:
        return []
    # general crypto news (free tier) — filter by asset name on the client side
    try:
        data = _http_get(f"https://finnhub.io/api/v1/news?category=crypto&token={key}")
        name = _NAME.get(symbol.upper(), symbol).lower()
        out = []
        for art in data:
            head = art.get("headline", "")
            if name in head.lower() or symbol.lower() in head.lower():
                out.append(head)
            if len(out) >= limit:
                break
        return out
    except Exception:
        return []


def _newsapi(symbol: str, lookback_hours: int, limit: int) -> list[str]:
    key = os.environ.get("NEWSAPI_KEY")
    if not key:
        return []
    q = urllib.parse.quote(_NAME.get(symbol.upper(), symbol))
    try:
        data = _http_get(
            f"https://newsapi.org/v2/everything?q={q}&language=en"
            f"&sortBy=publishedAt&pageSize={limit}&apiKey={key}"
        )
        return [a["title"] for a in data.get("articles", [])[:limit] if a.get("title")]
    except Exception:
        return []


def get_headlines(symbol: str, provider: str = "auto",
                  lookback_hours: int = 48, limit: int = 20) -> dict[str, Any]:
    """Returns {'headlines': [...], 'source': str}"""
    order = (["finnhub", "newsapi"] if provider in ("auto", None)
             else [provider])
    for p in order:
        if p == "finnhub":
            h = _finnhub(symbol, lookback_hours, limit)
        elif p == "newsapi":
            h = _newsapi(symbol, lookback_hours, limit)
        else:
            h = []
        if h:
            return {"headlines": h, "source": p}
    return {"headlines": [], "source": "none"}
