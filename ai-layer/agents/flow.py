"""Flow agent — money-flow / volume-pressure lens (web-free, torch-free).

This replaces the old web-scraped news+sentiment pair. On a small server DuckDuckGo /
news fetches were flaky, slow and usually empty, so those two "voices" added latency and
noise without edge. The flow agent answers the same question they tried to ("is smart money
buying or selling?") but from data we already have in hand: the candles' VOLUME and the
shape of each bar. No network, no models — deterministic and fast.

Four independent volume lenses, each with reasoning:
  1) Chaikin Money Flow (20) — where price closes inside each bar × volume → accumulation vs distribution
  2) On-Balance Volume slope — is cumulative volume flow rising or falling?
  3) Up/Down volume ratio    — is volume heavier on up bars or down bars?
  4) Effort vs result        — big volume but no price progress = exhaustion / absorption

A true uptrend should be *confirmed by volume*. Buying strength that volume does NOT confirm
is exactly the kind of move that mean-reverts and stops the bot out — so this lens is a real,
independent filter, not a redundant momentum copy of technical/trend_ml.
"""
from __future__ import annotations

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD


def _chaikin_money_flow(candles: list[dict[str, float]], period: int = 20) -> float | None:
    seg = candles[-period:]
    if len(seg) < period:
        return None
    mfv_sum = 0.0
    vol_sum = 0.0
    for c in seg:
        hi = float(c.get("high", 0.0))
        lo = float(c.get("low", 0.0))
        close = float(c.get("close", 0.0))
        vol = float(c.get("volume", 0.0))
        rng = hi - lo
        if rng <= 0 or vol <= 0:
            continue
        mult = ((close - lo) - (hi - close)) / rng  # +1 close on high, -1 close on low
        mfv_sum += mult * vol
        vol_sum += vol
    return mfv_sum / vol_sum if vol_sum > 0 else None


def _obv_slope(candles: list[dict[str, float]], window: int = 20) -> float | None:
    """Normalised OBV slope: OBV change over `window` bars divided by total volume in
    that window → ~[-1, 1]. Positive = cumulative flow rising (accumulation)."""
    if len(candles) < window + 1:
        return None
    seg = candles[-(window + 1):]
    obv = 0.0
    vol_total = 0.0
    for i in range(1, len(seg)):
        close = float(seg[i].get("close", 0.0))
        prev = float(seg[i - 1].get("close", 0.0))
        vol = float(seg[i].get("volume", 0.0))
        vol_total += vol
        if close > prev:
            obv += vol
        elif close < prev:
            obv -= vol
    return obv / vol_total if vol_total > 0 else None


def _up_down_volume(candles: list[dict[str, float]], window: int = 20) -> float | None:
    """Ratio of volume on up bars to volume on down bars over `window` → centred at 0:
    (up - down) / (up + down). Positive = buyers carrying more volume."""
    seg = candles[-(window + 1):]
    if len(seg) < window + 1:
        return None
    up_vol = 0.0
    dn_vol = 0.0
    for i in range(1, len(seg)):
        close = float(seg[i].get("close", 0.0))
        prev = float(seg[i - 1].get("close", 0.0))
        vol = float(seg[i].get("volume", 0.0))
        if close >= prev:
            up_vol += vol
        else:
            dn_vol += vol
    total = up_vol + dn_vol
    return (up_vol - dn_vol) / total if total > 0 else None


def _effort_vs_result(candles: list[dict[str, float]], window: int = 5) -> tuple[float, float] | None:
    """Recent volume surge vs price progress. Returns (vol_ratio, price_progress_pct).
    High volume + tiny progress = effort without result (absorption / exhaustion)."""
    if len(candles) < 21:
        return None
    recent = candles[-window:]
    base = candles[-21:-1]
    vol_recent = sum(float(c.get("volume", 0.0)) for c in recent) / max(1, len(recent))
    vol_base = sum(float(c.get("volume", 0.0)) for c in base) / max(1, len(base))
    if vol_base <= 0:
        return None
    vol_ratio = vol_recent / vol_base
    p_start = float(recent[0].get("close", 0.0))
    p_end = float(recent[-1].get("close", 0.0))
    progress = (p_end / p_start - 1.0) if p_start > 0 else 0.0
    return vol_ratio, progress


class FlowAgent(Agent):
    name = "flow"

    def analyze(self, ctx: MarketContext) -> AgentResult:
        candles = ctx.candles
        if len(candles) < 25:
            return self._fail("Too few candles (<25) for volume flow")
        # If the feed carries no real volume (some synthetic/edge feeds), abstain rather than
        # vote noise — abstaining (ok=False) keeps it out of the consensus tally cleanly.
        vols = [float(c.get("volume", 0.0)) for c in candles[-20:]]
        if sum(vols) <= 0:
            return self._fail("No volume data on this feed")

        cmf = _chaikin_money_flow(candles)
        obv = _obv_slope(candles)
        udv = _up_down_volume(candles)
        eff = _effort_vs_result(candles)
        if cmf is None or obv is None or udv is None:
            return self._fail("Incomplete volume series")

        votes: list[tuple[float, int, str]] = []

        # 1) Chaikin Money Flow
        if cmf > 0.10:
            votes.append((1.0, +1, f"💰 CMF(20)={cmf:+.2f} — strong accumulation, closes printing in the upper half of bars on volume"))
        elif cmf > 0.02:
            votes.append((0.5, +1, f"💧 CMF(20)={cmf:+.2f} — mild accumulation"))
        elif cmf < -0.10:
            votes.append((1.0, -1, f"🩸 CMF(20)={cmf:+.2f} — strong distribution, sellers closing bars on their lows"))
        elif cmf < -0.02:
            votes.append((0.5, -1, f"🌫️ CMF(20)={cmf:+.2f} — mild distribution"))
        else:
            votes.append((0.3, 0, f"➖ CMF(20)={cmf:+.2f} — flat money flow, no clear hand"))

        # 2) OBV slope
        if obv > 0.20:
            votes.append((0.8, +1, f"📈 OBV slope={obv:+.2f} — cumulative volume flow rising (buyers in control)"))
        elif obv < -0.20:
            votes.append((0.8, -1, f"📉 OBV slope={obv:+.2f} — cumulative volume flow falling (sellers in control)"))
        else:
            votes.append((0.3, 0, f"➖ OBV slope={obv:+.2f} — balanced flow"))

        # 3) Up/Down volume
        if udv > 0.20:
            votes.append((0.7, +1, f"🟢 Up/Down vol={udv:+.2f} — volume concentrated on up bars"))
        elif udv < -0.20:
            votes.append((0.7, -1, f"🔴 Up/Down vol={udv:+.2f} — volume concentrated on down bars"))
        else:
            votes.append((0.3, 0, f"➖ Up/Down vol={udv:+.2f} — volume evenly split"))

        # 4) Effort vs result — exhaustion / absorption warnings (cap aggressive entries)
        if eff is not None:
            vol_ratio, progress = eff
            if vol_ratio >= 1.8 and progress <= 0.002:
                votes.append((0.6, -1, f"🥵 Effort/Result: volume {vol_ratio:.1f}× but price flat ({progress*100:+.1f}%) — heavy effort, no result (likely absorption/exhaustion)"))
            elif vol_ratio >= 1.5 and progress > 0.01:
                votes.append((0.5, +1, f"🚀 Effort/Result: volume {vol_ratio:.1f}× pushing price +{progress*100:.1f}% — demand confirmed by volume"))
            elif vol_ratio < 0.6:
                votes.append((0.3, 0, f"😴 Effort/Result: volume drying up ({vol_ratio:.1f}×) — moves lack conviction"))

        total_w = sum(w for w, _, _ in votes) or 1.0
        score = sum(w * s for w, s, _ in votes) / total_w
        n_bull = sum(1 for _, s, _ in votes if s > 0)
        n_bear = sum(1 for _, s, _ in votes if s < 0)

        if score > 0.20:
            action, confidence = BUY, min(0.90, 0.42 + score * 0.55)
        elif score < -0.20:
            action, confidence = SELL, min(0.90, 0.42 + abs(score) * 0.55)
        else:
            action, confidence = HOLD, 0.30

        lines = [msg for _, _, msg in votes]
        lines.append(
            f"🧮 flow score {score:+.2f} (±0.20) · bullish {n_bull} / bearish {n_bear} → {action}"
        )

        return AgentResult(
            agent=self.name, action=action, confidence=confidence,
            reasoning="\n".join(lines), horizon="short",
            extra={
                "cmf": round(cmf, 4),
                "obv_slope": round(obv, 4),
                "up_down_vol": round(udv, 4),
                "vol_ratio": round(eff[0], 2) if eff else None,
                "flow_score": round(score, 3),
            },
        )
