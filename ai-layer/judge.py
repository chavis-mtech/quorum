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
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from typing import Any

# Deterministic decision rules live in their own package (small, pure, unit-tested) — judge.py
# stays focused on LLM/provider plumbing. _apply_entry_discipline is re-exported under its old
# name so tests/imports that reference judge._apply_entry_discipline keep working.
from strategy.entry_discipline import apply_entry_discipline as _apply_entry_discipline
from strategy.trend_gate import apply_trend_gate as _apply_trend_gate, trend_direction

_OLLAMA_KEEP_ALIVE = os.environ.get("OLLAMA_KEEP_ALIVE", "120s")
_JSON_RE = re.compile(r"\{.*\}", re.S)


def _discipline(v: dict[str, Any], ctx: dict[str, Any] | None,
                consensus: dict[str, Any] | None) -> dict[str, Any]:
    """Apply both deterministic guards in order to a verdict:
    1) anti-chase (over-extended market BUY → pullback limit / HOLD)
    2) trend gate (BUY into a confirmed downtrend → HOLD) + conviction/reversal-risk scoring + sizing.
    """
    v = _apply_entry_discipline(v, ctx)
    return _apply_trend_gate(v, ctx, consensus)


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


def _portfolio_loss_limit(pf: dict[str, Any] | None) -> float:
    """Configured account loss limit in percentage points.

    Older callers do not send it, so retain the legacy 6% fallback. A non-positive value is
    invalid for a hard-stop threshold and also falls back safely.
    """
    try:
        value = float((pf or {}).get("loss_limit_pct", 6.0))
    except (TypeError, ValueError):
        return 6.0
    return value if value > 0.0 else 6.0


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
• trending  (ADX>25, ER≥0.35):  RR ≥ 1.4:1
• weak-trend (ER 0.20-0.35):    RR ≥ 1.45:1
• ranging    (ER<0.20, ADX<20): RR ≥ 1.5:1 (if not achievable → always HOLD)
Never widen stop just to engineer an RR ratio — if RR does not meet regime threshold → HOLD.
TARGET PLACEMENT (critical — this is what lets a good trend actually trade):
• In a CONFIRMED UPTREND (trending / weak-trend, price making higher highs) resistance_20 is just
  the recent high a breakout clears — you MAY place the target a measured step ABOVE it
  (≈ resistance + 1×ATR). Do NOT cap the target at the recent high when momentum is clearly up;
  capping there forfeits the move and forces a needless HOLD.
• In a RANGE, do the opposite — cap the target at the range top (resistance_20); the edge is
  mean-reversion, so never project a breakout you don't expect.

━━━━━━ Portfolio risk management rules (iron) ━━━━━━
• session_pnl < -3%:   halve suggested_size_pct (still losing → trade light).
• session_pnl <= -configured_loss_limit_pct: HOLD all new positions.
• The configured loss limit is supplied in PORTFOLIO STATUS. Never invent a fixed -6% limit;
  the backend risk governor is authoritative for the user's account.
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
        loss_limit = _portfolio_loss_limit(pf)
        dep = float(pf.get("deployed_pct", 0.0))
        cash = float(pf.get("cash_thb", 0.0))
        pf_txt = (
            f"\n═══ PORTFOLIO STATUS ═══\n"
            f"Session P&L: {spnl:+.1f}% · Configured loss limit: -{loss_limit:.1f}% · "
            f"Deployed: {dep:.0f}% · Cash: {cash:,.0f} THB\n"
        )
        # warn judge if in a dangerous state
        if spnl <= -loss_limit:
            pf_txt += (
                f"⚠️ SESSION P&L HIT THE CONFIGURED -{loss_limit:.1f}% LIMIT "
                "→ must be HOLD unless SELL to reduce position\n"
            )
        elif spnl < -3:
            pf_txt += (
                "⚠️ Session is down more than 3%, but remains inside the configured loss limit "
                "→ halve position size; do not force HOLD solely because an old fixed -6% rule was crossed\n"
            )
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


def _finalize_llm_verdict(out: dict[str, Any], prov: str, cfg: dict[str, Any],
                          price: float | None, ctx: dict[str, Any],
                          consensus: dict[str, Any] | None = None) -> dict[str, Any]:
    """Normalise an LLM verdict, stamp the engine/thinking, then apply the discipline guards."""
    v = _defaults(out["verdict"], price)
    model_used = out.get("_model") or cfg.get("model")
    label = "gemini" if _is_gemini_cfg(prov, cfg) else prov
    v["engine"] = f"{label}:{model_used}"
    v["thinking"] = out.get("thinking", "")
    if out.get("_rotation"):
        v["reasoning"] = (str(v.get("reasoning", "")) + f" | {out['_rotation']}")[:4000]
    return _discipline(v, ctx, consensus)


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
            yield ("verdict", _finalize_llm_verdict(out, prov, cfg, price, ctx, consensus))
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


# ─── Gemini model auto-rotation (free-tier rate-limit survival) ──────────────────
# Google's free tier rate-limits PER MODEL (RPM/TPM/RPD). When the active Gemini model
# returns 429 / RESOURCE_EXHAUSTED / quota, transparently fall over to the next Gemini
# text model instead of giving up the LLM judge. Models are ordered by how much free-tier
# headroom they have (RPD first). A model that 429s is put on a short cooldown so we stop
# hammering it; once every model is cooling down the normal provider chain takes over
# (→ ollama / rule-based planner), so trading never stalls.
_GEMINI_BASE_URL = "https://generativelanguage.googleapis.com/v1beta/openai"
_GEMINI_ROTATION = [
    "gemini-3.1-flash-lite",  # 15 RPM / 500 RPD — most daily headroom
    "gemini-2.5-flash-lite",  # 10 RPM / 20 RPD
    "gemini-2.5-flash",       #  5 RPM / 20 RPD
    "gemini-3-flash",         #  5 RPM / 20 RPD
]
_MODEL_COOLDOWN: dict[str, float] = {}   # model -> epoch seconds until which it is skipped
_RATE_LIMIT_COOLDOWN_S = 600             # skip a rate-limited model for 10 minutes


def _is_rate_limited_err(err: str | None) -> bool:
    e = (err or "").lower()
    return any(s in e for s in ("429", "rate limit", "resource_exhausted",
                                "quota", "too many requests"))


def _is_gemini_cfg(provider: str, cfg: dict[str, Any]) -> bool:
    return provider == "gemini" or "generativelanguage.googleapis.com" in (cfg.get("base_url") or "")


def _gemini_model_order(cfg: dict[str, Any]) -> list[str]:
    """Configured model first, then any user fallback_models, then the built-in rotation —
    de-duplicated. Models not on cooldown are tried before ones that are (last-resort)."""
    ordered: list[str] = []
    for m in [(cfg.get("model") or "").strip(), *cfg.get("fallback_models", []), *_GEMINI_ROTATION]:
        if m and m not in ordered:
            ordered.append(m)
    now = time.time()
    ready = [m for m in ordered if _MODEL_COOLDOWN.get(m, 0.0) <= now]
    cooling = [m for m in ordered if _MODEL_COOLDOWN.get(m, 0.0) > now]
    return ready + cooling


def _gemini_rotate(prompt: str, cfg: dict[str, Any]):
    """Try Gemini models in order until one answers; cooldown any that hit a rate limit."""
    key = cfg.get("api_key", "")
    if not key:
        return None, "gemini: API key not configured"
    base = cfg.get("base_url") or _GEMINI_BASE_URL
    errors: list[str] = []
    for model in _gemini_model_order(cfg):
        out, err = _openai_compatible(prompt, key, base, model)
        if out and out.get("verdict", {}).get("action"):
            out["_model"] = model                 # let the engine label show what actually answered
            _MODEL_COOLDOWN.pop(model, None)
            if errors:
                out["_rotation"] = f"rotated to {model} after: {' | '.join(errors)}"
            return out, None
        if _is_rate_limited_err(err):
            _MODEL_COOLDOWN[model] = time.time() + _RATE_LIMIT_COOLDOWN_S
            errors.append(f"{model}: rate-limited")
        else:
            errors.append(f"{model}: {err or 'no action in response'}")
    return None, "all Gemini models unavailable → " + " | ".join(errors)


def _call_provider(provider: str, prompt: str, cfg: dict[str, Any], want_think: bool):
    model = cfg.get("model", "qwen3:14b")
    if provider == "ollama":
        return _ollama(prompt, model, cfg.get("ollama_url", "http://localhost:11434"), want_think)
    if provider in ("anthropic", "claude"):
        key = cfg.get("api_key", "")
        if not key:
            return None, f"{provider}: API key not configured"
        return _anthropic(prompt, key, model)
    # Gemini (explicit provider, or any openai-compatible config pointed at Google's endpoint)
    # → auto-rotate across Gemini models on rate limits.
    if _is_gemini_cfg(provider, cfg):
        return _gemini_rotate(prompt, cfg)
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
            return _finalize_llm_verdict(out, prov, cfg, price, ctx, consensus)
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

    # Regime-aware RR threshold. Ranging/choppy conditions require more payoff because false
    # breakouts and round-trip fees consume a larger share of the available move.
    rr_required = {"trending": 1.4, "weak-trend": 1.45, "ranging": 1.5}.get(regime, 1.4)

    base: dict[str, Any] = {
        "action": action, "confidence": conf, "thesis": note,
        "entry_type": "none", "entry_price": 0.0, "target_price": 0.0, "stop_price": 0.0,
        "invalidation": "", "next_step": "", "suggested_size_pct": 0.0,
        "engine": "rule-based", "thinking": "",
        "reasoning": f"{note}. {consensus.get('reasoning', '')}{err_txt}",
    }

    passed = bool(consensus.get("passed_threshold"))
    if action not in ("BUY", "SELL") or not passed or price <= 0:
        return _discipline(base, ctx, consensus)  # attach conviction/reversal-risk even for HOLD

    atr = float(s.get("atr_14") or 0.0) or price * 0.02
    sup = float(s.get("support_20") or 0.0)
    res = float(s.get("resistance_20") or 0.0)

    # portfolio-aware sizing
    pf = ctx.get("portfolio") or {}
    spnl = float(pf.get("session_pnl_pct", 0.0))
    loss_limit = _portfolio_loss_limit(pf)
    dep = float(pf.get("deployed_pct", 0.0))

    if action == "BUY":
        # Block if portfolio is not suitable
        if spnl <= -loss_limit or dep > 85:
            base["action"] = "HOLD"
            base["reasoning"] = (
                f"{note}. rule-based planner blocked BUY: "
                f"session P&L={spnl:+.1f}% / configured limit=-{loss_limit:.1f}% / "
                f"deployed={dep:.0f}%{err_txt}"
            )
            return _discipline(base, ctx, consensus)

        stop = sup if 0 < sup < price and (price - sup) <= 2.5 * atr else price - 1.5 * atr
        stop = min(stop, price * 0.995)
        # Never PLAN a stop wider than the backend's catastrophic cap (MAX_LOSS_PCT=6%). Otherwise
        # the planner prices RR off a wide stop the backend will truncate at -6% → realized loss is
        # bigger than planned (the AERO/ENA negative-skew mechanism). Clamp so planned RR == realized.
        stop = max(stop, price * (1.0 - 0.055))
        risk = price - stop
        if risk <= 0:
            return _discipline(base, ctx, consensus)

        # Target ceiling depends on regime + trend direction. In a RANGE the edge is
        # mean-reversion → cap the target at the range top (resistance_20). In a confirmed
        # UPtrend, resistance_20 is just the recent high a breakout clears — hard-capping there
        # throttles momentum setups to RR<floor and is exactly why the proven-winner buckets
        # (the brain measured ranging|up & weak-trend|up at RR≈1.6-1.7 on ATR-projected targets)
        # could never trade live. Give a confirmed UPtrend a measured breakout projection above
        # resistance so those setups clear the RR bar. Stop is unchanged → per-trade risk is
        # identical; this only opens upside headroom for setups already learned to win.
        tdir = trend_direction(s)
        target = price + max(rr_required * risk * 1.1, 2.0 * atr)
        if res > price:
            if regime in ("trending", "weak-trend") and tdir == "up":
                headroom = (1.0 if regime == "trending" else 0.5) * atr
                target = min(target, res + headroom)
            else:
                target = min(target, res)
        rr = (target - price) / risk

        if rr < rr_required:
            base["action"] = "HOLD"
            base["reasoning"] = (
                f"{note}. rule-based planner: BUY passed threshold but RR {rr:.2f} < {rr_required} "
                f"(regime={regime}) — not entering. ATR={atr:.4g} sup={sup:.4g} res={res:.4g}{err_txt}"
            )
            return _discipline(base, ctx, consensus)

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
        # Same guards as the LLM path: anti-chase (don't market-buy an over-extended price)
        # + trend gate (don't buy into a confirmed downtrend) + conviction-scaled sizing.
        return _discipline(base, ctx, consensus)

    # SELL
    base.update({
        "entry_type": "market", "entry_price": price,
        "thesis": f"rule-based: SELL consensus passed (conf {conf:.2f}) — exit position",
        "suggested_size_pct": round(min(1.0, 0.5 + conf * 0.5), 2),
        "reasoning": f"{note} — SELL at market {price:.4g}{err_txt}",
        "next_step": "after sell, wait for a fresh BUY signal",
    })
    return _discipline(base, ctx, consensus)  # SELL isn't gated; this attaches scores for the UI
