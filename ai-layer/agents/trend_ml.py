"""Trend ML agent — multi-scale momentum/trend model + market regime detection

Perspective differs from technical agent:
  1) Momentum across 3 scales (5/20/50 bars) adjusted by volatility → probability of upward move
  2) Efficiency Ratio (Kaufman) — is the market "trending" or "sideways"?
     Sideways = momentum signal less reliable → automatically reduce confidence
  3) Price structure Higher-High/Higher-Low — a true uptrend must lift both highs and lows

reasoning explains every layer: what it sees and why confidence is set at that level
"""
from __future__ import annotations

import math

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD


def _pct_change(closes: list[float], lag: int) -> float | None:
    if len(closes) <= lag or closes[-1 - lag] == 0:
        return None
    return closes[-1] / closes[-1 - lag] - 1.0


def _volatility(closes: list[float], window: int = 20) -> float:
    seg = closes[-window:]
    if len(seg) < 2:
        return 0.0
    rets = [seg[i] / seg[i - 1] - 1 for i in range(1, len(seg))]
    mean = sum(rets) / len(rets)
    var = sum((r - mean) ** 2 for r in rets) / len(rets)
    return math.sqrt(var)


def _efficiency_ratio(closes: list[float], window: int = 20) -> float:
    """Kaufman ER: |net change| / sum of |bar-by-bar changes|
    1.0 = moves in a straight line (true trend), 0.0 = oscillates without progress (sideways)"""
    if len(closes) <= window:
        return 0.0
    seg = closes[-window - 1:]
    net = abs(seg[-1] - seg[0])
    path = sum(abs(seg[i] - seg[i - 1]) for i in range(1, len(seg)))
    return net / path if path > 0 else 0.0


def _structure(highs: list[float], lows: list[float]) -> tuple[str, str]:
    """Compare highs/lows of last 10 bars vs the 10 bars before → (structure, description)"""
    if len(highs) < 20 or len(lows) < 20:
        return "unknown", "Insufficient data to analyze structure"
    rh, ph = max(highs[-10:]), max(highs[-20:-10])
    rl, pl = min(lows[-10:]), min(lows[-20:-10])
    hh, hl = rh > ph, rl > pl
    lh, ll = rh < ph, rl < pl
    if hh and hl:
        return "uptrend", f"Higher-High ({ph:.4g}→{rh:.4g}) + Higher-Low ({pl:.4g}→{rl:.4g}) — true uptrend structure, both highs and lows are rising"
    if lh and ll:
        return "downtrend", f"Lower-High ({ph:.4g}→{rh:.4g}) + Lower-Low ({pl:.4g}→{rl:.4g}) — true downtrend structure, both highs and lows are falling"
    if hh and ll:
        return "expanding", "Expanding range in both directions (higher high but lower low) — high volatility, direction unclear"
    return "ranging", f"Highs/lows close to prior range (H {ph:.4g}→{rh:.4g}, L {pl:.4g}→{rl:.4g}) — sideways consolidation"


class TrendMlAgent(Agent):
    name = "trend_ml"

    def analyze(self, ctx: MarketContext) -> AgentResult:
        closes = ctx.closes
        if len(closes) < 51:
            return self._fail("Too few candles (<51)")

        m_short = _pct_change(closes, 5)
        m_mid = _pct_change(closes, 20)
        m_long = _pct_change(closes, 50)
        if None in (m_short, m_mid, m_long):
            return self._fail("Momentum calculation incomplete")

        highs = [float(c.get("high", 0.0)) for c in ctx.candles if c.get("high")]
        lows = [float(c.get("low", 0.0)) for c in ctx.candles if c.get("low")]

        vol = _volatility(closes)
        er = _efficiency_ratio(closes)
        struct, struct_msg = _structure(highs, lows)

        # Layer 1: weighted multi-scale momentum divided by volatility → P(up)
        raw = 0.5 * m_short + 0.3 * m_mid + 0.2 * m_long
        z = raw / (vol + 1e-6)
        prob_up = 1.0 / (1.0 + math.exp(-z * 1.5))

        aligned = sum(1 for m in (m_short, m_mid, m_long) if m > 0)
        align_msg = {3: "All 3 scales positive simultaneously — trend aligned across all layers",
                     0: "All 3 scales negative simultaneously — selling pressure aligned across all layers"}.get(
            aligned, f"Positive scales {aligned}/3 — signals still conflicting on some layers")

        # Layer 2: regime — sideways reduces confidence (compresses P toward 0.5)
        if er >= 0.35:
            regime, damp = "trending", 1.0
            regime_msg = f"Efficiency Ratio={er:.2f} (≥0.35) — market is genuinely trending, price moving directionally, momentum signal fully reliable"
        elif er >= 0.20:
            regime, damp = "weak-trend", 0.75
            regime_msg = f"Efficiency Ratio={er:.2f} — weak trend, moderate choppiness present, reducing confidence by 25%"
        else:
            regime, damp = "ranging", 0.5
            regime_msg = f"Efficiency Ratio={er:.2f} (<0.20) — market is sideways, price oscillating without progress, momentum prone to false signals, reducing confidence by half"

        prob_adj = 0.5 + (prob_up - 0.5) * damp

        # Layer 3: structure confirms or contradicts
        struct_bonus = 0.0
        if struct == "uptrend" and prob_adj > 0.5:
            struct_bonus = 0.05
            struct_verdict = "HH/HL structure confirms momentum direction → boosting confidence"
        elif struct == "downtrend" and prob_adj < 0.5:
            struct_bonus = -0.05
            struct_verdict = "LH/LL structure confirms momentum direction → boosting confidence"
        elif struct in ("uptrend", "downtrend"):
            struct_verdict = "Price structure conflicts with momentum — signals not unanimous, no confidence bonus added"
        else:
            struct_verdict = "Structure is non-directional — relying primarily on momentum"
        prob_final = max(0.02, min(0.98, prob_adj + struct_bonus))

        if prob_final > 0.58:
            action, confidence = BUY, prob_final
        elif prob_final < 0.42:
            action, confidence = SELL, 1 - prob_final
        else:
            action, confidence = HOLD, 0.3

        confidence = max(0.0, min(0.95, confidence))

        lines = [
            f"📊 Multi-scale Momentum: 5bars={m_short*100:+.1f}% · 20bars={m_mid*100:+.1f}% · 50bars={m_long*100:+.1f}% ({align_msg})",
            f"🌡️ 20-bar volatility={vol*100:.2f}% → z-score={z:+.2f} → raw P(up)={prob_up:.2f}",
            f"🧭 {regime_msg}",
            f"🏗️ {struct_msg}",
            f"⚖️ {struct_verdict}",
            f"🧮 Summary: P(up) after regime+structure adjustment = {prob_final:.2f} "
            f"(threshold BUY>0.58, SELL<0.42) → {action}",
        ]

        return AgentResult(
            agent=self.name, action=action, confidence=confidence,
            reasoning="\n".join(lines), horizon="medium",
            extra={"prob_up": round(prob_final, 3), "prob_raw": round(prob_up, 3),
                   "volatility": round(vol, 5), "efficiency_ratio": round(er, 3),
                   "regime": regime, "structure": struct},
        )
