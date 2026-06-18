"""Trend gate — anti "falling-knife" + conviction/reversal-risk scoring.

This is the missing *mirror* of entry_discipline. entry_discipline stops the bot from
buying TOPS (over-extended); trend_gate stops it from buying KNIVES (a confirmed downtrend).

Applied to every BUY verdict (LLM and rule-based alike), deterministically:

  1) TREND GATE — a BUY *into a confirmed downtrend* is converted to HOLD unless a bullish
     reversal is independently confirmed (≥2 reversal signals). Balanced: dips in an uptrend
     or a sideways range are still allowed — only the falling knife is refused.

  2) CONVICTION vs REVERSAL-RISK — two 0..1 scores answering the user's question
     "is the next move likely to follow the calculated direction, or is it risky?":
       • conviction    = how aligned & strong the setup is (regime, structure, momentum, agreement)
       • reversal_risk = how likely price turns against the action (chop, divergence, into resistance)
     They are attached to the verdict (the UI shows a meter) and used to size the position —
     bigger when conviction is high and reversal-risk is low, smaller when it is shaky.

Every input comes from data the council already computed — the enriched market_structure
(EMA stack, MACD, RSI divergence from `technical`; structure/efficiency/prob_up from `trend_ml`)
plus the regime and the consensus vote tally. No new market calls; no Rust changes.
"""
from __future__ import annotations

from dataclasses import dataclass, asdict
from typing import Any

# A BUY is allowed in a prior downtrend only when a reversal is CONFIRMED: at least one STRONG
# signal (price reclaimed EMA26 / MACD cross / structure turned up) plus ≥2 signals total.
# Weak signals alone (RSI divergence + a 1-bar higher-low) are exactly a dead-cat bounce — the
# falling knife the user wants to avoid — so they never confirm a reversal on their own.
_REVERSAL_MIN_TOTAL = 2
# Conviction-scaled sizing: final size = base * (FLOOR + SPAN*conviction) * (1 - RISK_CUT*reversal_risk)
_SIZE_FLOOR = 0.65
_SIZE_SPAN = 0.35
_SIZE_RISK_CUT = 0.40
_SIZE_MIN = 0.02


def _f(d: dict[str, Any], key: str, default: float = 0.0) -> float:
    try:
        return float(d.get(key) or default)
    except (TypeError, ValueError):
        return default


def _ema_state(s: dict[str, Any]) -> str:
    """'up' | 'down' | 'unknown' from the 12/26/50 EMA stack + price location."""
    e12, e26, e50 = _f(s, "ema12"), _f(s, "ema26"), _f(s, "ema50")
    price = _f(s, "price") or _f(s, "last_price")
    if not (e12 and e26):
        return "unknown"
    if e50:
        if e12 < e26 < e50 and (not price or price < e12):
            return "down"
        if e12 > e26 > e50 and (not price or price > e12):
            return "up"
    return "up" if e12 > e26 else "down"


def trend_direction(s: dict[str, Any]) -> str:
    """Confirmed trend direction from independent lenses (EMA stack, trend_ml structure,
    prob_up, multi-scale momentum). 'down'/'up' require ≥2 lenses to agree → 'sideways' otherwise."""
    down = up = 0
    ema = _ema_state(s)
    if ema == "down":
        down += 1
    elif ema == "up":
        up += 1

    ts = str(s.get("trend_structure") or "")
    if ts == "downtrend":
        down += 1
    elif ts == "uptrend":
        up += 1

    prob_up = _f(s, "prob_up", 0.5)
    if prob_up < 0.45:
        down += 1
    elif prob_up > 0.55:
        up += 1

    m20, m50 = _f(s, "momentum_20"), _f(s, "momentum_50")
    if m20 < 0 and m50 < 0:
        down += 1
    elif m20 > 0 and m50 > 0:
        up += 1

    if down >= 2 and down > up:
        return "down"
    if up >= 2 and up > down:
        return "up"
    return "sideways"


def bullish_reversal_signals(s: dict[str, Any]) -> tuple[list[str], list[str]]:
    """Signs a downtrend may be turning up, split into STRONG (a real structural/momentum turn)
    and WEAK (early/often-noisy hints). Returns (strong, weak)."""
    strong: list[str] = []
    weak: list[str] = []
    price, e26 = _f(s, "price"), _f(s, "ema26")
    if price and e26 and price > e26:
        strong.append("reclaimed EMA26")
    if str(s.get("trend_structure") or "") == "uptrend":
        strong.append("structure turned uptrend")
    # A MACD bullish cross is only STRONG above the zero line — a cross deep BELOW zero during a
    # downtrend is just the decline decelerating (a classic falling-knife/dead-cat tell), so it
    # only counts as a WEAK early hint, never enough on its own to confirm a reversal.
    macd, sig, hist = _f(s, "macd"), _f(s, "macd_signal"), _f(s, "macd_hist")
    if macd > sig and hist > 0:
        (strong if macd >= 0 else weak).append(
            "MACD cross above zero" if macd >= 0 else "MACD momentum ticking up (below zero)")
    if str(s.get("rsi_divergence")) == "bullish":
        weak.append("RSI bullish divergence")
    if _f(s, "momentum_5") > 0 > _f(s, "momentum_20"):
        weak.append("short-term higher-low")
    return strong, weak


def reversal_confirmed(strong: list[str], weak: list[str]) -> bool:
    """A reversal is confirmed only with ≥1 STRONG signal AND ≥2 signals overall — so a pure
    dead-cat bounce (weak signals only) is NOT enough to buy into a downtrend."""
    return len(strong) >= 1 and (len(strong) + len(weak)) >= _REVERSAL_MIN_TOTAL


def conviction_score(s: dict[str, Any], regime: str, action: str,
                     agreement_ratio: float, vote_conf: float) -> float:
    """0..1 — how strongly the setup supports continuation in `action`'s direction."""
    score = {"trending": 0.30, "weak-trend": 0.15}.get(regime, 0.0)
    score += min(0.20, _f(s, "efficiency_ratio") * 0.4)         # ER 0.5 → +0.20

    ts = str(s.get("trend_structure") or "")
    if (action == "BUY" and ts == "uptrend") or (action == "SELL" and ts == "downtrend"):
        score += 0.18
    elif (action == "BUY" and ts == "downtrend") or (action == "SELL" and ts == "uptrend"):
        score -= 0.15

    sign_up = action == "BUY"
    aligned = sum(1 for m in (_f(s, "momentum_5"), _f(s, "momentum_20"), _f(s, "momentum_50"))
                  if m != 0 and (m > 0) == sign_up)
    score += 0.05 * aligned                                     # up to +0.15

    adx = _f(s, "adx")
    score += 0.10 if adx >= 25 else (0.05 if adx >= 20 else 0.0)
    score += 0.15 * max(0.0, min(1.0, agreement_ratio))
    score += 0.10 * max(0.0, min(1.0, vote_conf))
    return round(max(0.0, min(1.0, score)), 3)


def reversal_risk_score(s: dict[str, Any], regime: str, action: str) -> float:
    """0..1 — how likely the next move goes AGAINST `action` (chop / divergence / into a wall)."""
    risk = {"ranging": 0.30, "weak-trend": 0.12}.get(regime, 0.0)
    er = _f(s, "efficiency_ratio")
    if 0.0 < er < 0.20:
        risk += 0.15

    div = str(s.get("rsi_divergence"))
    if (action == "BUY" and div == "bearish") or (action == "SELL" and div == "bullish"):
        risk += 0.25

    rsi, room = _f(s, "rsi"), _f(s, "room_to_resistance_20")
    if action == "BUY" and rsi >= 68 and 0.0 < room < 0.03:     # overbought into resistance
        risk += 0.20

    ts = str(s.get("trend_structure") or "")
    if (action == "BUY" and ts == "downtrend") or (action == "SELL" and ts == "uptrend"):
        risk += 0.20
    if ts == "expanding":                                       # whipsaw range
        risk += 0.10
    if _f(s, "atr_pct") > 0.03:                                 # very volatile bar-to-bar
        risk += 0.10
    return round(max(0.0, min(1.0, risk)), 3)


@dataclass
class TrendAssessment:
    trend_dir: str               # up | down | sideways
    conviction: float            # 0..1
    reversal_risk: float         # 0..1
    reversal_signals: list[str]  # bullish reversal signs found
    gate: str                    # aligned | reversal-confirmed | blocked | n/a

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def assess_trend(structure: dict[str, Any], regime: str, action: str,
                 agreement_ratio: float = 0.0, vote_conf: float = 0.0) -> TrendAssessment:
    """Pure scorer — no mutation. Returns direction + conviction + reversal-risk for `action`."""
    action = (action or "HOLD").upper()
    regime = (regime or "unknown").lower()
    tdir = trend_direction(structure)
    strong, weak = bullish_reversal_signals(structure)
    conv = conviction_score(structure, regime, action, agreement_ratio, vote_conf)
    rrisk = reversal_risk_score(structure, regime, action)
    if action == "BUY" and tdir == "down":
        gate = "reversal-confirmed" if reversal_confirmed(strong, weak) else "blocked"
    elif action in ("BUY", "SELL"):
        gate = "aligned"
    else:
        gate = "n/a"
    return TrendAssessment(tdir, conv, rrisk, strong + weak, gate)


def apply_trend_gate(v: dict[str, Any], ctx: dict[str, Any] | None,
                     consensus: dict[str, Any] | None) -> dict[str, Any]:
    """Gate + score + size. Attaches conviction/reversal_risk/trend_dir/trend_gate to the verdict.

    BUY into a confirmed downtrend (no confirmed reversal) → HOLD. Sizing is scaled by
    conviction & reversal-risk (only ever tightened, never inflated beyond what was set).
    """
    s = (ctx or {}).get("market_structure") or {}
    if s.get("quality") != "ok":
        return v  # not enough structure to judge trend → don't interfere

    regime = str((ctx or {}).get("regime") or "unknown").lower()
    action = str(v.get("action") or "HOLD").upper()
    cons = consensus or {}
    votes = cons.get("votes") or []
    voted = int(cons.get("voted") or sum(1 for x in votes if x.get("ok")))
    agreement = int(cons.get("agreement") or 0)
    agree_ratio = (agreement / voted) if voted else 0.0
    vote_conf = _f(v, "confidence")

    a = assess_trend(s, regime, action, agree_ratio, vote_conf)
    v["conviction"] = a.conviction
    v["reversal_risk"] = a.reversal_risk
    v["trend_dir"] = a.trend_dir
    v["trend_gate"] = a.gate

    if a.gate == "blocked":
        v["action"] = "HOLD"
        v["entry_type"] = "none"
        v["entry_price"] = 0.0
        v["suggested_size_pct"] = 0.0
        if not v.get("invalidation"):
            v["invalidation"] = "wait for a confirmed reversal (reclaim EMA26 + MACD turn / higher-low)"
        v["reasoning"] = (str(v.get("reasoning", "")) +
                          " | trend-gate: BUY blocked — EMA/structure/momentum confirm a downtrend and "
                          f"no reversal is confirmed (signals: {', '.join(a.reversal_signals) or 'none'}) → "
                          f"HOLD to avoid catching a falling knife "
                          f"(conviction={a.conviction:.2f}, reversal-risk={a.reversal_risk:.2f})")[:4000]
        return v

    if a.gate == "reversal-confirmed":
        v["reasoning"] = (str(v.get("reasoning", "")) +
                          f" | trend-gate: downtrend but reversal confirmed ({', '.join(a.reversal_signals)}) "
                          "→ entry allowed")[:4000]

    # conviction-scaled sizing — only tightens (factor ≤ 1.0)
    if action == "BUY":
        base = _f(v, "suggested_size_pct")
        if base > 0:
            factor = (_SIZE_FLOOR + _SIZE_SPAN * a.conviction) * (1.0 - _SIZE_RISK_CUT * a.reversal_risk)
            v["suggested_size_pct"] = round(max(_SIZE_MIN, min(base, base * factor)), 3)
    return v
