"""Judge — a disciplined, hedge-fund-level professional crypto trader.

Philosophy:
  A good trader is skilled not because they trade often, but because they trade at the right moment.
  Wait for a quality setup → enter decisively → manage the position well → cut losses quickly.
  Never trade a sideways market with a large position.
"""
from __future__ import annotations

import json
import os
import re
import urllib.error
import urllib.request
from datetime import datetime, timezone
from typing import Any

_OLLAMA_KEEP_ALIVE = os.environ.get("OLLAMA_KEEP_ALIVE", "120s")
_JSON_RE = re.compile(r"\{.*\}", re.S)

# ─── Entry discipline (regime-aware, anti-chase) ─────────────────────────────────
# Entry STYLE must match the market regime — do not blindly demand a cheaper price:
#   • trending   → momentum IS the signal; ride strength / breakouts. Enter at market unless the
#                  move is truly parabolic (blow-off). Demanding a pullback here just misses it.
#   • weak-trend → moderate: enter at market on a clean setup, pull back if stretched.
#   • ranging    → the edge is mean-reversion; buy a dip toward support, never chase a local high.
# When a market BUY is judged over-extended FOR ITS REGIME, it is converted to a small pullback
# LIMIT (or HOLD if no safe pullback keeps the reward:risk). Thresholds are deterministic and
# computed from real structure — they do NOT depend on the (sometimes unreliable) local LLM.
#
# Per regime: (min_room_to_resistance, max_dist_above_support, hot_momentum_5, overbought_rsi).
# A market BUY is "extended" if room-to-resistance is below / distance-above-support, 5-bar
# momentum, or RSI is at-or-above the regime's bar.
_EXT_THRESHOLDS: dict[str, tuple[float, float, float, float]] = {
    "trending":   (0.010, 0.30, 0.18, 82.0),  # lenient — strong trend, let the entry run
    "weak-trend": (0.025, 0.15, 0.10, 74.0),  # moderate
    "ranging":    (0.040, 0.08, 0.06, 68.0),  # strict — only buy pullbacks to support
    "unknown":    (0.030, 0.12, 0.08, 72.0),  # neutral default
}
_EXT_PULLBACK_ATR = 0.5          # aim the limit ~0.5*ATR below market
_EXT_PULLBACK_PCT = 0.02         # fallback pullback when ATR is unavailable (2% below market)
_EXT_MIN_PULLBACK_PCT = 0.015    # the limit must sit at least 1.5% below market
_EXT_MAX_PULLBACK_PCT = 0.045    # ...but within 4.5% (Rust rejects pending entries >5% away)


def _extract_json(text: str) -> dict[str, Any]:
    try:
        return json.loads(text)
    except Exception:
        m = _JSON_RE.search(text or "")
        if m:
            try:
                return json.loads(m.group(0))
            except Exception:
                return {}
        return {}


_SYSTEM = """\
You are a professional hedge-fund-level crypto trader — with iron discipline, careful risk management, and a focus on long-term profitability.
You receive data from multiple AI agents (technical, trend_ml, sentiment, news) and must produce a single JSON decision.

━━━━━━ Thinking steps (must follow in order) ━━━━━━
1) Read the market regime (trending / weak-trend / ranging) — this affects every criterion below.
2) Evaluate the setup: technical structure + momentum + sentiment + news/web combined.
3) Form a thesis in 1-2 sentences — if the thesis is not clear = HOLD.
4) Choose action & entry STYLE — MATCH THE REGIME; do not blindly wait for a cheaper price:
   • Trending (ADX>25, ER≥0.35): momentum IS the signal. ENTER AT MARKET on a clean setup —
     ride strength and breakouts. Use a limit pullback ONLY if the move is truly parabolic
     (RSI≥80, just spiked hard, or almost no room to resistance). Demanding a dip in a strong
     trend is exactly how you miss the move.
   • Weak-trend (ER 0.20-0.35): enter at market on a clean setup; use a small limit pullback
     (entry 1.5-4.5% below market, near EMA20 / 20-bar support / price - 0.5*ATR) if stretched.
   • Ranging (ER<0.20, ADX<20): the edge is mean-reversion. BUY a pullback toward support (limit),
     never chase a local high; if price sits near the top of the range → HOLD.
   • HOLD: unclear setup / RR below regime threshold / a good pullback entry is too far (>5%).
5) Set target/stop referencing real prices (ATR, support, resistance).
   The system independently converts an over-extended market BUY into a regime-appropriate pullback
   limit (or HOLD) — so be decisive: if the setup is clean FOR ITS REGIME, take the entry now.
6) Specify invalidation (what breaks the thesis) and next_step.

━━━━━━ REWARD:RISK rules by regime ━━━━━━
• trending  (ADX>25, ER≥0.35):  RR ≥ 1.5:1
• weak-trend (ER 0.20-0.35):    RR ≥ 2.0:1
• ranging    (ER<0.20, ADX<20): RR ≥ 2.5:1 (if not achievable → always HOLD)
Never widen stop just to engineer an RR ratio — if RR does not meet regime threshold → HOLD.

━━━━━━ Portfolio risk management rules (iron) ━━━━━━
• session_pnl < -3%:   halve suggested_size_pct (still losing → trade light).
• session_pnl < -6%:   HOLD all new positions (hit dangerous daily loss limit → stop trading).
• portfolio_deployed > 70%: HOLD new positions (little capital remaining).
• portfolio_deployed > 85%: HOLD absolutely (near full capital deployment).

━━━━━━ Position sizing rules (suggested_size_pct) ━━━━━━
• conf > 0.80 + trending:    45-60% of capital per trade.
• conf 0.65-0.80 or weak:    25-40%.
• conf < 0.65 or ranging:    10-20% (small / probing the market).
Halve size if session_pnl is negative.

━━━━━━ For SELL (exiting a held position) ━━━━━━
• The original buy thesis has broken → exit immediately, do not wait.
• Position is in good profit + clear reversal signal → exit as planned.
• Act offensively — no hesitation, do not wait for more evidence if it is already clear.

━━━━━━ Managing an OPEN position (when CURRENT POSITION is shown) ━━━━━━
• A trailing stop already protects the downside — do NOT sell a healthy winner just to "lock in" a small gain; that is the trailing stop's job. A long drawdown that is still thesis-valid is fine; do not panic-sell it.
• SELL only when the thesis has broken or there is a clear reversal — then exit decisively.
• KEY: if the setup is clearly breaking down (momentum/structure turning against you, likely heading to the stop) while you are STILL in profit or near break-even, LOCK IT IN NOW — a smaller certain gain beats riding it down to a full stop-loss. This is the one time to take profit early.
• Otherwise HOLD and let the winner run; you may tighten the plan by returning a higher stop_price/target_price (the system never widens risk — a lower stop is ignored).

━━━━━━ Reasoning requirements ━━━━━━
• 4-8 sentences in English, do not give terse answers.
• Must cite real numbers: RSI, EMA, MACD, ATR, levels, momentum, ADX/ER (regime), news.
• Must state reward:risk as a number with the basis — entry/target/stop.
• If HOLD: explain what is missing and what you need to see before changing your mind.
• thesis: readable and understandable immediately in 1-2 sentences.

Reply with JSON only (no text outside the JSON):
{"action":"BUY|SELL|HOLD","confidence":0.0-1.0,"thesis":"...","entry_type":"market|limit|none","entry_price":number,"target_price":number,"stop_price":number,"invalidation":"...","next_step":"...","reasoning":"...","suggested_size_pct":0.0-1.0}
"""


def _build_prompt(consensus: dict[str, Any], ctx: dict[str, Any], web: list[str],
                  custom_instruction: str = "") -> str:
    votes = "\n".join(
        f"- {v['agent']}: {v['action']} (conf {v['confidence']:.2f}) — {v['reasoning'][:200]}"
        for v in consensus.get("votes", [])
    )
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    web_txt = "\n".join(web[:8]) if web else "(no web data)"
    price = ctx.get("last_price")
    regime = consensus.get("regime", ctx.get("regime", "unknown"))

    # price structure
    structure = ctx.get("market_structure") or {}
    atr = structure.get("atr_14", 0)
    atr_pct = structure.get("atr_pct", 0) * 100
    support = structure.get("support_20", 0)
    resistance = structure.get("resistance_20", 0)
    structure_txt = (
        f"ATR(14)={atr:.4g} ({atr_pct:.2f}%) · support20={support:.4g} · resistance20={resistance:.4g} · "
        f"room_to_res={structure.get('room_to_resistance_20', 0)*100:.1f}% · dist_to_sup={structure.get('distance_to_support_20', 0)*100:.1f}%"
    ) if structure else "(none)"

    # position context — when present, the judge is MANAGING an already-open trade
    pos = ctx.get("position")
    pos_txt = ""
    if pos:
        stop = float(pos.get("stop", 0) or 0)
        target = float(pos.get("target", 0) or 0)
        profit_r = float(pos.get("profit_r", 0) or 0)
        pos_txt = (
            f"\n═══ CURRENT POSITION (you are MANAGING this trade) ═══\n"
            f"Holding {pos.get('symbol')} amount {pos.get('amount')} · avg cost {pos.get('avg_price')}\n"
            f"Current price {pos.get('last_price')} · P&L {pos.get('pnl_pct', 0):+.1f}% ({profit_r:+.2f}R)\n"
            f"Active stop {stop:.6g} · target {target:.6g} (a trailing stop already protects the downside)\n"
            f"Decide HOLD (thesis intact — let the trailing stop manage risk) or "
            f"SELL (thesis broken / clear reversal — exit now, do not wait).\n"
        )

    # portfolio context
    pf = ctx.get("portfolio")
    pf_txt = ""
    if pf:
        spnl = float(pf.get("session_pnl_pct", 0.0))
        dep = float(pf.get("deployed_pct", 0.0))
        cash = float(pf.get("cash_thb", 0.0))
        pf_txt = (
            f"\n═══ PORTFOLIO STATUS ═══\n"
            f"Session P&L: {spnl:+.1f}% · Deployed: {dep:.0f}% · Cash: {cash:,.0f} THB\n"
        )
        # warn judge if in a dangerous state
        if spnl < -6:
            pf_txt += "⚠️ SESSION P&L BELOW -6% → must be HOLD unless SELL to reduce position\n"
        elif spnl < -3:
            pf_txt += "⚠️ Session negative > 3% → halve position size\n"
        if dep > 85:
            pf_txt += "⚠️ Deployed >85% → absolute HOLD for new positions\n"
        elif dep > 70:
            pf_txt += "⚠️ Deployed >70% → caution with new positions\n"

    return (
        f"Date: {today}\n"
        f"Asset: {ctx.get('symbol')}/{ctx.get('quote')}  Market price: {price}\n"
        f"Market regime: {regime} (from trend_ml)\n"
        f"{pos_txt}{pf_txt}"
        f"\n═══ PRICE STRUCTURE ═══\n{structure_txt}\n"
        f"\n═══ LIVE WEB/NEWS DATA ═══\n{web_txt}\n"
        f"\n═══ ANALYST VOTES ═══\n{votes}\n"
        f"\n═══ PRELIMINARY CONSENSUS ═══\n"
        f"{consensus.get('action')} (agreement {consensus.get('agreement')}/{consensus.get('voted')}, "
        f"conf {consensus.get('confidence'):.2f}, vetoed={consensus.get('vetoed')})\n"
        f"\nPlan the trade for {ctx.get('symbol')} @ {price} — reply in JSON."
        + (f"\n\n═══ SPECIAL USER INSTRUCTION ═══\n{custom_instruction}" if custom_instruction else "")
    )


def _defaults(v: dict[str, Any], price: float | None) -> dict[str, Any]:
    v.setdefault("entry_type", "market")
    v.setdefault("entry_price", 0.0)
    v.setdefault("target_price", 0.0)
    v.setdefault("stop_price", 0.0)
    v.setdefault("thesis", "")
    v.setdefault("invalidation", "")
    v.setdefault("next_step", "")
    v.setdefault("suggested_size_pct", 0.0)
    for k in ("entry_price", "target_price", "stop_price", "confidence", "suggested_size_pct"):
        try:
            v[k] = float(v.get(k) or 0.0)
        except Exception:
            v[k] = 0.0
    action = str(v.get("action") or "HOLD").upper()
    if action == "WAIT":
        action = "HOLD"
    if action not in {"BUY", "SELL", "HOLD"}:
        action = "HOLD"
    v["action"] = action
    entry_type = str(v.get("entry_type") or "none").lower()
    if entry_type not in {"market", "limit", "none"}:
        entry_type = "none"
    if v["entry_price"] > 0 and action == "HOLD":
        entry_type = "limit"
    if action == "BUY" and entry_type == "limit" and v["entry_price"] <= 0:
        entry_type = "market"
    v["entry_type"] = entry_type
    v["confidence"] = max(0.0, min(1.0, v["confidence"]))
    v["suggested_size_pct"] = max(0.0, min(1.0, v["suggested_size_pct"]))
    return v


def _apply_entry_discipline(v: dict[str, Any], ctx: dict[str, Any] | None) -> dict[str, Any]:
    """Regime-aware anti-chase guard applied to EVERY final verdict (LLM and rule-based alike).

    A market BUY that is over-extended FOR ITS REGIME is converted to a pullback LIMIT entry —
    or HOLD when no safe pullback keeps the reward:risk. In a strong trend the bar for "extended"
    is high (ride the move); in a range it is low (only buy dips). This stops the bot from both
    chasing blow-off tops AND from missing clean trend entries by always waiting for a dip.
    """
    if str(v.get("action")) != "BUY" or str(v.get("entry_type")) != "market":
        return v  # only market BUYs can chase; limit/none/HOLD/SELL are left untouched
    s = (ctx or {}).get("market_structure") or {}
    if s.get("quality") != "ok":
        return v
    price = float((ctx or {}).get("last_price") or v.get("entry_price") or 0.0)
    if price <= 0:
        return v

    regime = str((ctx or {}).get("regime") or "unknown").lower()
    min_room, max_dist, hot_mom, ob_rsi = _EXT_THRESHOLDS.get(
        regime, _EXT_THRESHOLDS["unknown"]
    )

    atr = float(s.get("atr_14") or 0.0)
    sup = float(s.get("support_20") or 0.0)
    room = float(s.get("room_to_resistance_20") or 0.0)      # (resistance/price - 1)
    dist_sup = float(s.get("distance_to_support_20") or 0.0)  # (price/support - 1)
    mom5 = float(s.get("momentum_5") or 0.0)
    rsi = float(s.get("rsi") or 0.0)

    reasons: list[str] = []
    if 0.0 < room < min_room:
        reasons.append(f"only {room * 100:.1f}% room to resistance")
    if dist_sup > max_dist:
        reasons.append(f"{dist_sup * 100:.1f}% above support")
    if mom5 > hot_mom:
        reasons.append(f"5-bar momentum +{mom5 * 100:.1f}%")
    if rsi >= ob_rsi:
        reasons.append(f"RSI {rsi:.0f} overbought")
    if not reasons:
        return v  # clean entry for this regime → take it at market (ride strength)

    # Build a sane pullback limit: aim ~0.5*ATR below market, pulled at least 1.5% below
    # market so it is a real dip-entry, then floored so it never sits below 20-bar support
    # and never more than 4.5% below market (Rust rejects pending entries >5% away).
    pullback = price - _EXT_PULLBACK_ATR * atr if atr > 0 else price * (1 - _EXT_PULLBACK_PCT)
    pullback = min(pullback, price * (1 - _EXT_MIN_PULLBACK_PCT))
    floor = price * (1 - _EXT_MAX_PULLBACK_PCT)
    if sup > 0:
        floor = max(floor, sup * 1.002)  # support floor wins over the min-pullback target
    pullback = max(pullback, floor)

    stop = float(v.get("stop_price") or 0.0)
    target = float(v.get("target_price") or 0.0)
    note = f"{regime}: " + "; ".join(reasons)
    # If the pullback would break the stop or overshoot the target, there is no clean
    # entry left → stand aside rather than chase.
    if pullback <= 0 or (stop > 0 and pullback <= stop) or (target > 0 and target <= pullback):
        v["action"] = "HOLD"
        v["entry_type"] = "none"
        v["entry_price"] = 0.0
        v["reasoning"] = (str(v.get("reasoning", "")) +
                          f" | entry-discipline: over-extended ({note}); no safe pullback → HOLD")[:4000]
        return v
    v["entry_type"] = "limit"
    v["entry_price"] = round(pullback, 8)
    v["reasoning"] = (str(v.get("reasoning", "")) +
                      f" | entry-discipline: over-extended ({note}) → wait for pullback to {pullback:.6g}")[:4000]
    return v


def _finalize_llm_verdict(out: dict[str, Any], prov: str, cfg: dict[str, Any],
                          price: float | None, ctx: dict[str, Any]) -> dict[str, Any]:
    """Normalise an LLM verdict, stamp the engine/thinking, then apply entry discipline."""
    v = _defaults(out["verdict"], price)
    v["engine"] = f"{prov}:{cfg.get('model')}"
    v["thinking"] = out.get("thinking", "")
    return _apply_entry_discipline(v, ctx)


def _ollama(prompt: str, model: str, url: str, want_think: bool, timeout: float = 180.0):
    payload: dict[str, Any] = {
        "model": model,
        "prompt": f"{_SYSTEM}\n\n{prompt}",
        "stream": False,
        "think": want_think,
        "keep_alive": _OLLAMA_KEEP_ALIVE,
        "options": {"temperature": 0.15},  # lower temperature → more stable responses
    }
    if not want_think:
        payload["format"] = "json"
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(f"{url}/api/generate", data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        return {"verdict": _extract_json(data.get("response", "")),
                "thinking": data.get("thinking", "") or ""}, None
    except Exception as e:
        return None, f"ollama ({url}): {type(e).__name__}: {e}"


def _ollama_stream_gen(prompt: str, model: str, url: str, want_think: bool,
                       timeout: float = 600.0):
    payload: dict[str, Any] = {
        "model": model,
        "prompt": f"{_SYSTEM}\n\n{prompt}",
        "stream": True,
        "think": want_think,
        "keep_alive": _OLLAMA_KEEP_ALIVE,
        "options": {"temperature": 0.15},
    }
    if not want_think:
        payload["format"] = "json"
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(f"{url}/api/generate", data=body,
                                 headers={"Content-Type": "application/json"})
    resp_chunks: list[str] = []
    think_chunks: list[str] = []
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            for raw in resp:
                raw = raw.strip()
                if not raw:
                    continue
                try:
                    obj = json.loads(raw)
                except Exception:
                    continue
                t = obj.get("thinking") or ""
                if t:
                    think_chunks.append(t)
                    yield ("think", t)
                r = obj.get("response") or ""
                if r:
                    resp_chunks.append(r)
                if obj.get("done"):
                    break
    except Exception as e:
        yield ("error", f"ollama stream: {type(e).__name__}: {e}")
        yield ("result", None)
        return
    yield ("result", {"verdict": _extract_json("".join(resp_chunks)),
                      "thinking": "".join(think_chunks)})


def judge_stream(consensus: dict[str, Any], ctx: dict[str, Any], cfg: dict[str, Any],
                 web_snippets: list[str] | None = None):
    web_snippets = web_snippets or []
    price = ctx.get("last_price")
    if not cfg.get("enabled", True):
        yield ("verdict", _plan_from_consensus(consensus, "judge disabled (using rule-based planner)", ctx))
        return

    prompt = _build_prompt(consensus, ctx, web_snippets, cfg.get("custom_instruction", ""))
    want_think = bool(cfg.get("thinking", True))
    provider = cfg.get("provider", "ollama")
    chain = [provider] + [p for p in (cfg.get("fallback") or []) if p != provider]

    errors: list[str] = []
    for prov in chain:
        if prov == "none":
            yield ("verdict", _plan_from_consensus(
                consensus, "no LLM (using rule-based planner)", ctx, errors))
            return
        out, err = None, None
        if prov == "ollama":
            for kind, val in _ollama_stream_gen(
                prompt, cfg.get("model", "qwen3:14b"),
                cfg.get("ollama_url", "http://localhost:11434"), want_think
            ):
                if kind == "think":
                    yield ("think", val)
                elif kind == "error":
                    err = val
                else:
                    out = val
        else:
            out, err = _call_provider(prov, prompt, cfg, want_think)
        if out and out["verdict"].get("action"):
            yield ("verdict", _finalize_llm_verdict(out, prov, cfg, price, ctx))
            return
        if out and not out["verdict"].get("action"):
            err = err or "LLM responded but JSON has no action field"
        errors.append(f"{prov}: {err or 'unknown error'}")
    yield ("verdict", _plan_from_consensus(
        consensus, "judge failed on all providers (using rule-based planner)", ctx, errors))


def _http_json(url: str, headers: dict[str, str], payload: dict[str, Any],
               timeout: float = 120.0) -> tuple[dict[str, Any] | None, str | None]:
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json", **headers})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8")), None
    except urllib.error.HTTPError as e:
        detail = ""
        try:
            detail = e.read().decode("utf-8", "replace")[:200]
        except Exception:
            pass
        return None, f"HTTP {e.code}: {detail or e.reason}"
    except urllib.error.URLError as e:
        return None, f"connection failed: {e.reason}"
    except Exception as e:
        return None, f"{type(e).__name__}: {e}"


def _anthropic_rejects_sampling(model: str) -> bool:
    m = (model or "").lower()
    return any(tag in m for tag in ("opus-4-8", "opus-4-7"))


def _anthropic(prompt: str, api_key: str, model: str, timeout: float = 120.0):
    payload: dict[str, Any] = {
        "model": model, "max_tokens": 2048, "system": _SYSTEM,
        "messages": [{"role": "user", "content": prompt}],
    }
    if not _anthropic_rejects_sampling(model):
        payload["temperature"] = 0.15
    data, err = _http_json(
        "https://api.anthropic.com/v1/messages",
        {"x-api-key": api_key, "anthropic-version": "2023-06-01"},
        payload, timeout,
    )
    if not data:
        return None, err or "no response data"
    parts = data.get("content") or []
    text = "".join(p.get("text", "") for p in parts if p.get("type") == "text")
    return {"verdict": _extract_json(text), "thinking": ""}, None


def _openai_compatible(prompt: str, api_key: str, base_url: str, model: str, timeout: float = 120.0):
    base = (base_url or "https://api.openai.com/v1").rstrip("/")
    data, err = _http_json(
        f"{base}/chat/completions",
        {"Authorization": f"Bearer {api_key}"},
        {"model": model, "temperature": 0.15,
         "messages": [{"role": "system", "content": _SYSTEM},
                      {"role": "user", "content": prompt}]},
        timeout,
    )
    if not data:
        return None, err or "no response data"
    if data.get("error"):
        return None, f"API error: {json.dumps(data['error'], ensure_ascii=False)[:200]}"
    choices = data.get("choices") or []
    text = choices[0].get("message", {}).get("content", "") if choices else ""
    if not text:
        return None, f"response has no content: {json.dumps(data, ensure_ascii=False)[:200]}"
    return {"verdict": _extract_json(text), "thinking": ""}, None


def _call_provider(provider: str, prompt: str, cfg: dict[str, Any], want_think: bool):
    model = cfg.get("model", "qwen3:14b")
    if provider == "ollama":
        return _ollama(prompt, model, cfg.get("ollama_url", "http://localhost:11434"), want_think)
    if provider in ("anthropic", "claude"):
        key = cfg.get("api_key", "")
        if not key:
            return None, f"{provider}: API key not configured"
        return _anthropic(prompt, key, model)
    if provider in ("openai", "groq", "openrouter", "openai_compatible", "custom"):
        key = cfg.get("api_key", "")
        if not key:
            return None, f"{provider}: API key not configured"
        return _openai_compatible(prompt, key, cfg.get("base_url", ""), model)
    return None, f"unknown provider: {provider}"


def judge(consensus: dict[str, Any], ctx: dict[str, Any], cfg: dict[str, Any],
          web_snippets: list[str] | None = None) -> dict[str, Any]:
    web_snippets = web_snippets or []
    price = ctx.get("last_price")
    if not cfg.get("enabled", True):
        return _plan_from_consensus(consensus, "judge disabled (using rule-based planner)", ctx)

    prompt = _build_prompt(consensus, ctx, web_snippets, cfg.get("custom_instruction", ""))
    want_think = bool(cfg.get("thinking", True))
    provider = cfg.get("provider", "ollama")
    chain = [provider] + [p for p in (cfg.get("fallback") or []) if p != provider]

    errors: list[str] = []
    for prov in chain:
        if prov == "none":
            return _plan_from_consensus(consensus, "no LLM (using rule-based planner)", ctx, errors)
        out, err = _call_provider(prov, prompt, cfg, want_think)
        if out and out["verdict"].get("action"):
            return _finalize_llm_verdict(out, prov, cfg, price, ctx)
        if out and not out["verdict"].get("action"):
            err = err or "LLM responded but JSON has no action field"
        errors.append(f"{prov}: {err or 'unknown error'}")
    return _plan_from_consensus(consensus, "judge failed on all providers (using rule-based planner)", ctx, errors)


def _plan_from_consensus(consensus: dict[str, Any], note: str, ctx: dict[str, Any] | None,
                         errors: list[str] | None = None) -> dict[str, Any]:
    """rule-based planner — computes entry/target/stop from real market structure
    with regime-aware RR threshold and portfolio-aware sizing.
    """
    ctx = ctx or {}
    price = float(ctx.get("last_price") or 0.0)
    s = ctx.get("market_structure") or {}
    action = consensus.get("action", "HOLD")
    conf = float(consensus.get("confidence") or 0.0)
    regime = consensus.get("regime", "unknown")
    err_txt = ("\n⚠️ LLM unavailable reason: " + " | ".join(errors)) if errors else ""

    # Regime-aware RR threshold
    rr_required = {"trending": 1.5, "weak-trend": 2.0, "ranging": 2.5}.get(regime, 1.5)

    base: dict[str, Any] = {
        "action": action, "confidence": conf, "thesis": note,
        "entry_type": "none", "entry_price": 0.0, "target_price": 0.0, "stop_price": 0.0,
        "invalidation": "", "next_step": "", "suggested_size_pct": 0.0,
        "engine": "rule-based", "thinking": "",
        "reasoning": f"{note}. {consensus.get('reasoning', '')}{err_txt}",
    }

    passed = bool(consensus.get("passed_threshold"))
    if action not in ("BUY", "SELL") or not passed or price <= 0:
        return base

    atr = float(s.get("atr_14") or 0.0) or price * 0.02
    sup = float(s.get("support_20") or 0.0)
    res = float(s.get("resistance_20") or 0.0)

    # portfolio-aware sizing
    pf = ctx.get("portfolio") or {}
    spnl = float(pf.get("session_pnl_pct", 0.0))
    dep = float(pf.get("deployed_pct", 0.0))

    if action == "BUY":
        # Block if portfolio is not suitable
        if spnl < -6 or dep > 85:
            base["action"] = "HOLD"
            base["reasoning"] = (
                f"{note}. rule-based planner blocked BUY: "
                f"session P&L={spnl:+.1f}% / deployed={dep:.0f}%{err_txt}"
            )
            return base

        stop = sup if 0 < sup < price and (price - sup) <= 2.5 * atr else price - 1.5 * atr
        stop = min(stop, price * 0.995)
        risk = price - stop
        if risk <= 0:
            return base

        # check RR by regime
        max_target = res if res > price else price + rr_required * 1.2 * risk
        target = price + max(rr_required * risk * 1.1, 2.0 * atr)
        if res > price:
            target = min(target, res)
        rr = (target - price) / risk

        if rr < rr_required:
            base["action"] = "HOLD"
            base["reasoning"] = (
                f"{note}. rule-based planner: BUY passed threshold but RR {rr:.2f} < {rr_required} "
                f"(regime={regime}) — not entering. ATR={atr:.4g} sup={sup:.4g} res={res:.4g}{err_txt}"
            )
            return base

        # Position sizing by conf + regime + P&L
        if regime == "trending":
            base_size = 0.25 + conf * 0.35
        elif regime == "weak-trend":
            base_size = 0.15 + conf * 0.25
        else:
            base_size = 0.10 + conf * 0.15
        if spnl < -3:
            base_size *= 0.5  # session is losing → halve size
        size = round(min(0.60, max(0.05, base_size)), 2)

        base.update({
            "entry_type": "market", "entry_price": price,
            "target_price": round(target, 8), "stop_price": round(stop, 8),
            "suggested_size_pct": size,
            "thesis": (f"rule-based ({regime}): BUY @ {price:.4g} · RR={rr:.2f} · size={size*100:.0f}%"),
            "invalidation": f"price closes below stop {stop:.4g}",
            "next_step": f"monitor target {target:.4g} / stop {stop:.4g}",
            "reasoning": (
                f"{note} — rule-based planner (regime={regime}):\n"
                f"• market entry at {price:.4g}\n"
                f"• stop {stop:.4g} ({'support level' if stop == sup else '1.5×ATR'}, risk {risk/price*100:.1f}%)\n"
                f"• target {target:.4g} ({'capped at resistance' if res > price and target == res else f'{rr_required}×risk'})\n"
                f"• RR={rr:.2f} (≥{rr_required} pass) · size={size*100:.0f}% · ATR={atr:.4g}{err_txt}"
            ),
        })
        # Same anti-chase guard as the LLM path: don't market-buy an over-extended price.
        return _apply_entry_discipline(base, ctx)

    # SELL
    base.update({
        "entry_type": "market", "entry_price": price,
        "thesis": f"rule-based: SELL consensus passed (conf {conf:.2f}) — exit position",
        "suggested_size_pct": round(min(1.0, 0.5 + conf * 0.5), 2),
        "reasoning": f"{note} — SELL at market {price:.4g}{err_txt}",
        "next_step": "after sell, wait for a fresh BUY signal",
    })
    return base
