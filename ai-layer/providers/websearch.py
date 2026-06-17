"""Web search provider — fetches "current" data from the web to supplement decision-making

Uses DuckDuckGo (keyless) — no API key required
  1) DuckDuckGo Instant Answer API (json)  → short answer/abstract
  2) DuckDuckGo HTML lite                  → search results (title + snippet)

Designed to be resilient: if blocked/network down, returns [] (system continues operating)
Goal: give the Judge LLM fresh data instead of relying on stale knowledge in the model
"""
from __future__ import annotations

import json
import re
import urllib.parse
import urllib.request
from html import unescape
from typing import Any

_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) quorum/0.2"


def _get(url: str, timeout: float = 10.0) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": _UA})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read().decode("utf-8", errors="ignore")


def _instant_answer(query: str) -> list[dict[str, str]]:
    """DuckDuckGo Instant Answer (json) — abstract/definition if available"""
    try:
        url = ("https://api.duckduckgo.com/?q=" + urllib.parse.quote(query) +
               "&format=json&no_redirect=1&no_html=1")
        data = json.loads(_get(url))
        out: list[dict[str, str]] = []
        if data.get("AbstractText"):
            out.append({"title": data.get("Heading", query),
                        "snippet": data["AbstractText"],
                        "url": data.get("AbstractURL", "")})
        for topic in data.get("RelatedTopics", [])[:4]:
            if isinstance(topic, dict) and topic.get("Text"):
                out.append({"title": topic.get("Text", "")[:60],
                            "snippet": topic["Text"],
                            "url": topic.get("FirstURL", "")})
        return out
    except Exception:
        return []


# DDG lite uses single or double quotes interchangeably → support both with ['\"]
_TITLE_RE = re.compile(r"""class=['"]result-link['"][^>]*>(.*?)</a>""", re.S)
_SNIP_RE = re.compile(r"""class=['"]result-snippet['"][^>]*>(.*?)</td>""", re.S)
_TAG_RE = re.compile(r"<[^>]+>")


def _clean(s: str) -> str:
    return unescape(_TAG_RE.sub("", s)).strip()


def _html_search(query: str, limit: int) -> list[dict[str, str]]:
    """DuckDuckGo lite HTML — general search results (title + snippet)"""
    try:
        url = "https://lite.duckduckgo.com/lite/?q=" + urllib.parse.quote(query)
        html = _get(url)
        titles = [_clean(x) for x in _TITLE_RE.findall(html)]
        snippets = [_clean(x) for x in _SNIP_RE.findall(html)]
        out: list[dict[str, str]] = []
        for title, snippet in zip(titles, snippets):
            if title and snippet:
                out.append({"title": title, "snippet": snippet, "url": ""})
            if len(out) >= limit:
                break
        return out
    except Exception:
        return []


def search(query: str, limit: int = 5) -> dict[str, Any]:
    """Returns {'results': [{title, snippet, url}], 'source': str, 'query': str}"""
    results = _instant_answer(query)
    if len(results) < limit:
        results += _html_search(query, limit - len(results))
    return {
        "query": query,
        "results": results[:limit],
        "source": "duckduckgo" if results else "none",
    }


def market_context(symbol: str, name: str | None = None) -> dict[str, Any]:
    """Search for the latest market context of an asset (news/price/sentiment) to feed into Judge"""
    term = name or symbol
    q = f"{term} crypto price news today latest"
    res = search(q, limit=5)
    snippets = [f"- {r['title']}: {r['snippet']}" for r in res["results"]]
    return {
        "query": q,
        "snippets": snippets,
        "source": res["source"],
        "count": len(snippets),
    }
