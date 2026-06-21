"""Pipeline — one decision cycle with a "reasoning trace" visible to the UI at every step

Steps (each step is recorded in the trace):
  1) data      — fetch candles + price (Bitkub)
  2) web       — fetch live data from the web (DuckDuckGo) + news
  3) agent     — run agents concurrently (individual reasoning visible)
  4) consensus — aggregate votes (voting math visible)
  5) judge     — LLM final ruling (qwen3 'thinking' tokens visible)
"""
from __future__ import annotations

import copy
from concurrent.futures import ThreadPoolExecutor
from typing import Any

from agents.base import MarketContext
from agents.technical import TechnicalAgent
from agents.sentiment import SentimentAgent
from agents.trend_ml import TrendMlAgent
from agents.news import NewsAgent
from agents.flow import FlowAgent
from aggregator import aggregate
from judge import judge as run_judge, judge_stream
from learning import brain
from providers import bitkub, news_provider, web_cache, websearch
from trace import Trace


def _cache_origin(meta: dict) -> str:
    """Human-readable label for how web/news data was served (for the reasoning trace)."""
    age = meta.get("age_s", 0)
    age_str = f"{age // 3600}h" if age >= 3600 else f"{age // 60}m"
    if meta.get("stale"):
        return f"stale {age_str} (live fetch failed)"
    if meta.get("cached"):
        return f"cached {age_str}"
    return "fresh"

_NAME = {"BTC": "Bitcoin", "ETH": "Ethereum", "XRP": "Ripple", "ADA": "Cardano",
         "DOGE": "Dogecoin", "SOL": "Solana", "BNB": "BNB", "USDT": "Tether"}


def _merge(base: dict[str, Any], over: dict[str, Any]) -> dict[str, Any]:
    out = dict(base)
    for k, v in (over or {}).items():
        if isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = _merge(out[k], v)
        else:
            out[k] = v
    return out


def _rsi_simple(closes: list[float], period: int = 14) -> float:
    """Coarse RSI(14) used only as an overbought gate for entry discipline.
    Returns 0.0 when there is not enough data (treated as 'not overbought')."""
    if len(closes) <= period:
        return 0.0
    gains = 0.0
    losses = 0.0
    for i in range(len(closes) - period, len(closes)):
        change = closes[i] - closes[i - 1]
        if change >= 0:
            gains += change
        else:
            losses -= change
    avg_gain = gains / period
    avg_loss = losses / period
    if avg_loss == 0:
        return 100.0
    rs = avg_gain / avg_loss
    return 100.0 - 100.0 / (1.0 + rs)


def _market_structure(candles: list[dict[str, float]], last_price: float | None) -> dict[str, Any]:
    closes = [float(c.get("close", 0.0)) for c in candles if c.get("close")]
    highs = [float(c.get("high", 0.0)) for c in candles if c.get("high")]
    lows = [float(c.get("low", 0.0)) for c in candles if c.get("low")]
    price = float(last_price or (closes[-1] if closes else 0.0))
    if len(closes) < 20 or price <= 0:
        return {"quality": "weak", "reason": "not enough candles for market structure"}

    def pct(lag: int) -> float:
        if len(closes) <= lag or closes[-1 - lag] == 0:
            return 0.0
        return closes[-1] / closes[-1 - lag] - 1.0

    ranges = []
    for i in range(max(1, len(candles) - 14), len(candles)):
        cur = candles[i]
        prev_close = float(candles[i - 1].get("close", cur.get("close", 0.0)))
        high = float(cur.get("high", 0.0))
        low = float(cur.get("low", 0.0))
        ranges.append(max(high - low, abs(high - prev_close), abs(low - prev_close)))
    atr = sum(ranges) / len(ranges) if ranges else 0.0
    support_20 = min(lows[-20:]) if len(lows) >= 20 else 0.0
    resistance_20 = max(highs[-20:]) if len(highs) >= 20 else 0.0
    support_50 = min(lows[-50:]) if len(lows) >= 50 else support_20
    resistance_50 = max(highs[-50:]) if len(highs) >= 50 else resistance_20
    atr_pct = atr / price if price > 0 else 0.0
    room_to_resistance = (resistance_20 / price - 1.0) if resistance_20 else 0.0
    distance_to_support = (price / support_20 - 1.0) if support_20 else 0.0
    return {
        "quality": "ok",
        "price": round(price, 8),
        "support_20": round(support_20, 8),
        "resistance_20": round(resistance_20, 8),
        "support_50": round(support_50, 8),
        "resistance_50": round(resistance_50, 8),
        "atr_14": round(atr, 8),
        "atr_pct": round(atr_pct, 5),
        "rsi": round(_rsi_simple(closes), 1),
        "momentum_5": round(pct(5), 5),
        "momentum_20": round(pct(20), 5),
        "momentum_50": round(pct(50), 5),
        "distance_to_support_20": round(distance_to_support, 5),
        "room_to_resistance_20": round(room_to_resistance, 5),
    }


def _news_enabled(cfg: dict[str, Any]) -> bool:
    """Master switch for the web/news fetch + the headline-dependent agents (news/sentiment).
    Default OFF: on a small server the web/news scrape was flaky, slow and edge-less, so the
    council now runs on price+volume only (technical/trend_ml/flow). Flip `[news] enabled = true`
    to bring the headline lens (and its CRITICAL-news veto) back."""
    return bool(cfg.get("news", {}).get("enabled", False))


def _build_agents(cfg: dict[str, Any]):
    en = cfg["agents"]
    agents = []
    if en.get("technical", True):  agents.append(TechnicalAgent())
    if en.get("trend_ml", True):   agents.append(TrendMlAgent())
    # `flow` = web-free money-flow/volume lens. Replaces the old web-scraped news+sentiment pair
    # as the third independent voice. Default ON (even if a stale server quorum.toml lacks the key).
    if en.get("flow", True):       agents.append(FlowAgent())
    # news/sentiment depend on scraped headlines → only run them when the web/news fetch is on.
    # This keeps the council deterministic regardless of a stale server config.
    if _news_enabled(cfg):
        if en.get("sentiment", True): agents.append(SentimentAgent())
        if en.get("news", True):      agents.append(NewsAgent())
    return agents


def _agent_extra(results, name: str) -> dict[str, Any]:
    """The extra dict of one agent's result (empty if it failed / has none)."""
    return next((r.extra for r in results if r.agent == name and r.ok and r.extra), {})


def _enrich_structure(structure: dict[str, Any], results) -> dict[str, Any]:
    """Fold the EMA stack / MACD / divergence (from `technical`) and the regime structure /
    efficiency ratio / prob_up (from `trend_ml`) into the market-structure dict, so the
    deterministic trend gate can read everything from one place (no extra market calls)."""
    tech = _agent_extra(results, "technical")
    trend = _agent_extra(results, "trend_ml")
    return {
        **structure,
        "ema12": tech.get("ema12"), "ema26": tech.get("ema26"), "ema50": tech.get("ema50"),
        "macd": tech.get("macd"), "macd_signal": tech.get("macd_signal"),
        "macd_hist": tech.get("macd_hist"), "rsi_divergence": tech.get("rsi_divergence"),
        "adx": tech.get("adx") if tech.get("adx") is not None else structure.get("adx"),
        "trend_structure": trend.get("structure"),
        "efficiency_ratio": trend.get("efficiency_ratio"),
        "prob_up": trend.get("prob_up"),
    }


def analyze_symbol_stream(symbol: str, cfg: dict[str, Any],
                          judge_override: dict[str, Any] | None = None):
    """Streaming analysis cycle — yields events during processing so the UI sees real-time progress:
      {"type":"stage","stage":..,"pct":..,"label":..}   <- step progress
      {"type":"think","pct":..,"delta":".."}             <- thinking tokens arriving incrementally while AI reasons
      {"type":"done","analysis":{...}}                    <- final result (same as non-streaming analyze_symbol)
    """
    cfg = copy.deepcopy(cfg)
    if judge_override:
        cfg["judge"] = _merge(cfg.get("judge", {}), judge_override)
    position_ctx = cfg.get("judge", {}).get("position_context")

    tr = Trace()
    mk = cfg["market"]
    quote = mk["quote"]
    tr.add("data", f"Starting analysis {symbol}/{quote}", f"Mode {cfg['general']['mode']}")
    yield {"type": "stage", "stage": "data", "pct": 4, "label": f"Starting analysis {symbol}"}

    # 1) market data
    candle_data = bitkub.get_candles(symbol, quote, mk["candle_interval"], mk["candle_lookback"])
    last_price = bitkub.get_last_price(symbol, quote)
    n = len(candle_data["candles"])
    tr.add("data", "Fetching price data",
           f"Got {n} candles ({mk['candle_interval']}) from {candle_data['source']}"
           + (" — simulated/offline" if candle_data["synthetic"] else ""),
           status="warn" if candle_data["synthetic"] else "done",
           data={"last_price": last_price, "candles": n})
    yield {"type": "stage", "stage": "data", "pct": 14,
           "label": f"Got price {last_price} · {n} candles"}
    structure = _market_structure(candle_data["candles"], last_price)
    tr.add("data", "Price structure summary",
           (f"support20={structure.get('support_20')} · resistance20={structure.get('resistance_20')} "
            f"· ATR={structure.get('atr_pct', 0) * 100:.2f}%"),
           status="done" if structure.get("quality") == "ok" else "warn",
           data=structure)
    yield {"type": "stage", "stage": "data", "pct": 20, "label": "Price structure summary"}

    # 2) live web data + news — OFF by default. On a small server the scrape was flaky/slow and
    #    added no edge, so the council now reads price+volume only. Flip [news] enabled=true to
    #    restore the headline lens (+ CRITICAL-news veto). When off we skip the network entirely.
    sym_u = symbol.upper()
    if _news_enabled(cfg):
        ttl = float(cfg["news"].get("cache_ttl_hours", 12)) * 3600.0
        yield {"type": "stage", "stage": "web", "pct": 24, "label": "Checking news/live data (cached)"}
        _w = web_cache.get_or_fetch(
            f"web:{sym_u}", ttl,
            lambda: websearch.market_context(symbol, _NAME.get(sym_u)),
            ok=lambda v: bool(v and v.get("count", 0) > 0),
        )
        web = _w["value"] or {"query": "", "snippets": [], "source": "none", "count": 0}
        _n = web_cache.get_or_fetch(
            f"news:{sym_u}", ttl,
            lambda: news_provider.get_headlines(symbol, cfg["news"]["provider"],
                                                cfg["news"]["lookback_hours"], cfg["news"]["max_articles"]),
            ok=lambda v: bool(v and v.get("headlines")),
        )
        news = _n["value"] or {"headlines": [], "source": "none"}
        web_headlines = [s.lstrip("- ") for s in web["snippets"]]
        all_headlines = news["headlines"] + web_headlines
        cache_note = f"web={_cache_origin(_w)} · news={_cache_origin(_n)}"
        tr.add("web", "News / live web data",
               f"DuckDuckGo: {web['count']} results · news: {len(news['headlines'])} from {news['source']} · {cache_note}",
               status="done" if (web["count"] or news["headlines"]) else "warn",
               data={"web_source": web["source"], "snippets": web["snippets"][:5], "cache": cache_note})
        yield {"type": "stage", "stage": "web", "pct": 30,
               "label": f"News {len(news['headlines'])} · web {web['count']} ({cache_note})"}
    else:
        web = {"query": "", "snippets": [], "source": "off", "count": 0}
        news = {"headlines": [], "source": "off"}
        all_headlines = []
        tr.add("web", "News / web data disabled",
               "Data-driven mode: trading on price + volume only (news/web fetch off) — "
               "faster and removes a flaky, edge-less input.",
               status="done", data={"news": "disabled"})
        yield {"type": "stage", "stage": "web", "pct": 30, "label": "Data-driven mode (news off)"}

    ctx = MarketContext(
        symbol=symbol, quote=quote, candles=candle_data["candles"], last_price=last_price,
        extra={"headlines": all_headlines, "synthetic": candle_data["synthetic"],
               "news_source": news["source"], "web_snippets": web["snippets"],
               "market_structure": structure},
    )

    # 3) agents run concurrently
    agents = _build_agents(cfg)
    yield {"type": "stage", "stage": "agent", "pct": 34,
           "label": f"{len(agents)} analysts voting"}
    with ThreadPoolExecutor(max_workers=max(len(agents), 1)) as ex:
        results = list(ex.map(lambda a: a.analyze(ctx), agents))
    for r in results:
        tr.add("agent", f"{r.agent} → {r.action} ({r.confidence:.2f})", r.reasoning,
               status="warn" if not r.ok else ("error" if r.veto else "done"),
               data={"action": r.action, "confidence": r.confidence, "veto": r.veto})
    yield {"type": "stage", "stage": "agent", "pct": 50, "label": "All votes received"}

    # 4) aggregate votes
    cons = cfg["consensus"]
    consensus = aggregate(results, weights=cons.get("weights", {}),
                          min_agreement=cons["min_agreement"], min_confidence=cons["min_confidence"])
    cd = consensus.to_dict()
    tr.add("consensus", f"Aggregate verdict → {cd['action']}",
           cd["reasoning"], status="done" if cd["passed_threshold"] else "warn",
           data={"tally": cd.get("tally", {}), "agreement": cd["agreement"],
                 "voted": cd["voted"], "vetoed": cd["vetoed"]})
    yield {"type": "stage", "stage": "consensus", "pct": 56,
           "label": f"Aggregate verdict → {cd['action']}"}

    # 5) judge final ruling — stream thinking tokens incrementally
    tr.add("judge", "Sending to Judge LLM for ruling",
           f"engine: {cfg['judge'].get('provider')}:{cfg['judge'].get('model')}",
           status="thinking",
           data={"position": position_ctx} if position_ctx else None)
    yield {"type": "stage", "stage": "judge", "pct": 58, "label": "AI is thinking..."}
    verdict = None
    think_chars = 0
    judge_cfg = cfg["judge"]
    enriched_structure = _enrich_structure(structure, results)
    judge_ctx: dict[str, Any] = {
        "symbol": symbol, "quote": quote, "last_price": last_price,
        # enriched so the deterministic trend gate sees EMA/MACD/divergence + regime structure
        "market_structure": enriched_structure,
        "regime": cd.get("regime", "unknown"),
    }
    if position_ctx:
        judge_ctx["position"] = position_ctx
    # portfolio context passed from Rust backend via judge_override
    portfolio_ctx = judge_cfg.get("portfolio")
    if portfolio_ctx:
        judge_ctx["portfolio"] = portfolio_ctx
    for kind, val in judge_stream(
        cd, judge_ctx,
        judge_cfg, web_snippets=web["snippets"]
    ):
        if kind == "think":
            think_chars += len(val)
            # gradually advance 58→94% based on thinking length (total unknown, use asymptote)
            pct = 58 + int(min(36, 36 * think_chars / 1600))
            yield {"type": "think", "pct": pct, "delta": val}
        else:
            verdict = val
    verdict = verdict or run_judge(
        cd, judge_ctx,
        judge_cfg, web_snippets=web["snippets"])

    # 5b) self-learning brain — settle past decisions against the new candles, learn the realized
    # edge per setup, and gate THIS verdict: a BUY in a proven-loser setup / chop / fee-unviable
    # conditions is converted to HOLD and shadow-tracked instead. Best-effort: a failure here must
    # never block a decision, so it is fully wrapped and degrades to "no learning this cycle".
    learning_info: dict[str, Any] = {"enabled": False}
    try:
        learning_info = brain.evaluate(
            symbol, candle_data["candles"], enriched_structure, cd, verdict,
            cd.get("regime", "unknown"), candle_data["synthetic"], cfg)
        verdict = brain.apply(verdict, learning_info)
    except Exception as exc:  # pragma: no cover - safety net
        learning_info = {"enabled": False, "summary": f"🧠 learning error: {exc}"}
    if learning_info.get("enabled"):
        tr.add("learning", "Self-learning review", learning_info.get("summary", ""),
               status="warn" if learning_info.get("block") else "done",
               data=learning_info)
        yield {"type": "stage", "stage": "learning", "pct": 95,
               "label": learning_info.get("summary", "Self-learning review")[:90]}

    # conviction / reversal-risk / trend-gate / learning ride along in the trace `data` — the one
    # channel that survives the Rust round-trip untouched (trace is raw JSON; struct fields dropped).
    gate_data = {
        "conviction": verdict.get("conviction"),
        "reversal_risk": verdict.get("reversal_risk"),
        "trend_dir": verdict.get("trend_dir"),
        "trend_gate": verdict.get("trend_gate"),
        "learning": learning_info if learning_info.get("enabled") else None,
    }
    tr.add("judge", f"Final ruling → {verdict['action']} ({verdict['confidence']:.2f})",
           verdict["reasoning"], status="done",
           data={"engine": verdict.get("engine"),
                 "thinking": verdict.get("thinking", ""), **gate_data})
    yield {"type": "stage", "stage": "judge", "pct": 96,
           "label": f"Result → {verdict['action']} ({verdict['confidence']:.2f})"}

    yield {"type": "done", "pct": 100, "analysis": {
        "symbol": symbol, "quote": quote, "last_price": last_price,
        "mode": cfg["general"]["mode"],
        "regime": cd.get("regime", "unknown"),
        "data_source": candle_data["source"], "synthetic": candle_data["synthetic"],
        "news_source": news["source"], "news_count": len(news["headlines"]),
        "web_source": web["source"], "web_count": web["count"],
        "conviction": verdict.get("conviction"), "reversal_risk": verdict.get("reversal_risk"),
        "trend_dir": verdict.get("trend_dir"), "trend_gate": verdict.get("trend_gate"),
        "learning": learning_info if learning_info.get("enabled") else None,
        "consensus": cd, "verdict": verdict,
        "trace": tr.to_list(),
    }}


def analyze_symbol(symbol: str, cfg: dict[str, Any],
                   judge_override: dict[str, Any] | None = None) -> dict[str, Any]:
    """Non-streaming (original) — drain the generator and return the final analysis."""
    out: dict[str, Any] | None = None
    for ev in analyze_symbol_stream(symbol, cfg, judge_override):
        if ev.get("type") == "done":
            out = ev["analysis"]
    if out is None:
        raise RuntimeError("Analysis failed (no final result)")
    return out
