"""Technical agent — full indicator analysis with detailed explanation for each indicator

Indicators used (each gives a bullish/bearish/neutral signal with reasoning):
  1) RSI(14)            — overbought/oversold zone + divergence
  2) MACD(12,26,9)      — line/signal/histogram expanding or contracting
  3) EMA stack 12/26/50 — trend structure aligned correctly?
  4) Bollinger(20,2σ)   — %B position + band squeeze
  5) Stochastic(14,3)   — %K/%D overbought/oversold zone + crossover
  6) ADX(14)            — trend strength (>25=trending, <20=ranging)
  7) Volume             — volume confirming price direction or not
  8) Support/Resistance — 20-bar support and resistance
  9) Momentum           — 10-bar momentum

Weighted aggregate score → BUY/SELL/HOLD + confidence
reasoning explains every significant indicator
"""
from __future__ import annotations

import math

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD


def _rsi(closes: list[float], period: int = 14) -> float | None:
    if len(closes) < period + 1:
        return None
    gains, losses = [], []
    for i in range(1, len(closes)):
        d = closes[i] - closes[i - 1]
        gains.append(max(d, 0.0))
        losses.append(max(-d, 0.0))
    avg_gain = sum(gains[:period]) / period
    avg_loss = sum(losses[:period]) / period
    for i in range(period, len(gains)):
        avg_gain = (avg_gain * (period - 1) + gains[i]) / period
        avg_loss = (avg_loss * (period - 1) + losses[i]) / period
    if avg_loss == 0:
        return 100.0
    return 100.0 - (100.0 / (1.0 + avg_gain / avg_loss))


def _ema_series(values: list[float], period: int) -> list[float]:
    if len(values) < period:
        return []
    k = 2 / (period + 1)
    out = [sum(values[:period]) / period]
    for v in values[period:]:
        out.append(v * k + out[-1] * (1 - k))
    return out


def _macd(closes: list[float]) -> tuple[float, float, float, float] | None:
    e12 = _ema_series(closes, 12)
    e26 = _ema_series(closes, 26)
    if not e12 or not e26:
        return None
    n = min(len(e12), len(e26))
    macd_line = [a - b for a, b in zip(e12[-n:], e26[-n:])]
    sig = _ema_series(macd_line, 9)
    if len(sig) < 2:
        return None
    hist = macd_line[-1] - sig[-1]
    hist_prev = macd_line[-2] - sig[-2]
    return macd_line[-1], sig[-1], hist, hist_prev


def _bollinger(closes: list[float], period: int = 20) -> tuple[float, float, float, float] | None:
    if len(closes) < period:
        return None
    seg = closes[-period:]
    sma = sum(seg) / period
    var = sum((c - sma) ** 2 for c in seg) / period
    sd = math.sqrt(var)
    upper, lower = sma + 2 * sd, sma - 2 * sd
    width = upper - lower
    pct_b = (closes[-1] - lower) / width if width > 0 else 0.5
    bandwidth = width / sma if sma > 0 else 0.0
    return pct_b, bandwidth, upper, lower


def _avg_bandwidth(closes: list[float], period: int = 20, lookback: int = 60) -> float:
    vals = []
    for end in range(max(period, len(closes) - lookback), len(closes) + 1):
        bb = _bollinger(closes[:end], period)
        if bb:
            vals.append(bb[1])
    return sum(vals) / len(vals) if vals else 0.0


def _stochastic(closes: list[float], highs: list[float], lows: list[float],
                k_period: int = 14, d_period: int = 3) -> tuple[float, float] | None:
    """Stochastic %K/%D — returns (pct_k, pct_d) latest values"""
    n = min(len(closes), len(highs), len(lows))
    if n < k_period + d_period:
        return None
    closes, highs, lows = closes[-n:], highs[-n:], lows[-n:]
    k_vals = []
    for i in range(k_period - 1, n):
        lo = min(lows[i - k_period + 1: i + 1])
        hi = max(highs[i - k_period + 1: i + 1])
        k = (closes[i] - lo) / (hi - lo) * 100 if hi != lo else 50.0
        k_vals.append(k)
    if len(k_vals) < d_period:
        return None
    pct_k = k_vals[-1]
    pct_d = sum(k_vals[-d_period:]) / d_period
    return pct_k, pct_d


def _adx(highs: list[float], lows: list[float], closes: list[float],
         period: int = 14) -> float | None:
    """ADX — trend strength 0-100; >25=trending, <20=sideways"""
    n = min(len(highs), len(lows), len(closes))
    if n < period * 2:
        return None
    highs, lows, closes = highs[-n:], lows[-n:], closes[-n:]
    tr_list, pdm_list, ndm_list = [], [], []
    for i in range(1, n):
        hl = highs[i] - lows[i]
        hc = abs(highs[i] - closes[i - 1])
        lc = abs(lows[i] - closes[i - 1])
        tr_list.append(max(hl, hc, lc))
        up = highs[i] - highs[i - 1]
        dn = lows[i - 1] - lows[i]
        pdm_list.append(up if up > dn and up > 0 else 0.0)
        ndm_list.append(dn if dn > up and dn > 0 else 0.0)

    def smooth(vals: list[float]) -> list[float]:
        out = [sum(vals[:period])]
        for v in vals[period:]:
            out.append(out[-1] - out[-1] / period + v)
        return out

    atr_s = smooth(tr_list)
    pdm_s = smooth(pdm_list)
    ndm_s = smooth(ndm_list)
    dx_vals = []
    for a, p, nd in zip(atr_s, pdm_s, ndm_s):
        if a <= 0:
            continue
        pdi = 100 * p / a
        ndi = 100 * nd / a
        s = pdi + ndi
        dx_vals.append(100 * abs(pdi - ndi) / s if s > 0 else 0.0)
    if not dx_vals:
        return None
    adx = sum(dx_vals[-period:]) / period
    return adx


def _rsi_divergence(closes: list[float], rsi_val: float | None,
                    lookback: int = 20) -> str | None:
    """Detect RSI divergence: price makes new high but RSI does not = bearish; opposite = bullish"""
    if rsi_val is None or len(closes) < lookback + 5:
        return None
    seg = closes[-(lookback + 5):]
    rsi_approx_prev = _rsi(seg[:-5])
    if rsi_approx_prev is None:
        return None
    price_up = closes[-1] > max(seg[:-5])
    rsi_up = rsi_val > rsi_approx_prev
    price_down = closes[-1] < min(seg[:-5])
    rsi_down = rsi_val < rsi_approx_prev
    if price_up and not rsi_up:
        return "bearish"
    if price_down and not rsi_down:
        return "bullish"
    return None


class TechnicalAgent(Agent):
    name = "technical"

    def analyze(self, ctx: MarketContext) -> AgentResult:
        closes = ctx.closes
        if len(closes) < 35:
            return self._fail("Too few candles (<35)")

        highs = [float(c.get("high", 0.0)) for c in ctx.candles if c.get("high")]
        lows  = [float(c.get("low",  0.0)) for c in ctx.candles if c.get("low")]
        vols  = [float(c.get("volume", 0.0)) for c in ctx.candles]
        price = float(ctx.last_price or closes[-1])

        rsi  = _rsi(closes)
        macd = _macd(closes)
        e12s = _ema_series(closes, 12)
        e26s = _ema_series(closes, 26)
        e50s = _ema_series(closes, 50)
        bb   = _bollinger(closes)
        stoch = _stochastic(closes, highs, lows)
        adx_val = _adx(highs, lows, closes) if len(highs) >= 28 else None
        div  = _rsi_divergence(closes, rsi)

        if rsi is None or macd is None or bb is None or not e12s or not e26s:
            return self._fail("Incomplete indicator calculation")

        ema12, ema26 = e12s[-1], e26s[-1]
        ema50 = e50s[-1] if e50s else None
        macd_line, macd_sig, hist, hist_prev = macd
        pct_b, bandwidth, bb_up, bb_lo = bb

        votes: list[tuple[float, int, str]] = []

        # 1) RSI + divergence
        if rsi < 30:
            votes.append((1.0, +1, f"📉 RSI(14)={rsi:.1f} — oversold (<30) bounce risk"))
        elif rsi < 45:
            votes.append((0.5, +1, f"📊 RSI(14)={rsi:.1f} — weak zone, price not expensive"))
        elif rsi <= 60:
            votes.append((0.4, 0, f"📊 RSI(14)={rsi:.1f} — neutral zone, no directional bias"))
        elif rsi <= 70:
            votes.append((0.5, -1, f"📈 RSI(14)={rsi:.1f} — getting hot, watch for profit-taking"))
        else:
            votes.append((1.0, -1, f"🔥 RSI(14)={rsi:.1f} — overbought (>70) pullback risk"))

        if div == "bearish":
            votes.append((0.9, -1, f"⚠️ RSI Bearish Divergence: price made new high but RSI did not confirm — bearish reversal signal"))
        elif div == "bullish":
            votes.append((0.9, +1, f"💡 RSI Bullish Divergence: price made new low but RSI did not follow — bullish reversal signal"))

        # 2) MACD
        widening = abs(hist) > abs(hist_prev)
        if macd_line > macd_sig and hist > 0:
            d = "expanding — momentum strong" if widening else "contracting — buying pressure fading"
            votes.append((1.0 if widening else 0.6, +1,
                          f"✅ MACD: line({macd_line:.4g}) > signal({macd_sig:.4g}) hist={hist:.4g} {d}"))
        elif macd_line < macd_sig and hist < 0:
            d = "expanding — downside momentum strong" if widening else "contracting — selling pressure fading, may be near reversal"
            votes.append((1.0 if widening else 0.6, -1,
                          f"❌ MACD: line({macd_line:.4g}) < signal({macd_sig:.4g}) hist={hist:.4g} {d}"))
        else:
            votes.append((0.4, 0, f"➖ MACD near crossover ({macd_line:.4g}≈{macd_sig:.4g}) — inflection point, wait for confirmation"))

        # 3) EMA stack
        if ema50 is not None:
            if ema12 > ema26 > ema50 and price > ema12:
                votes.append((1.2, +1, f"📈 EMA: price({price:.4g})>12({ema12:.4g})>26({ema26:.4g})>50({ema50:.4g}) — full uptrend across all timeframes"))
            elif ema12 < ema26 < ema50 and price < ema12:
                votes.append((1.2, -1, f"📉 EMA: price<12<26<50 — full downtrend, avoid counter-trend trades"))
            elif ema12 > ema26:
                votes.append((0.6, +1, f"↗️ EMA12({ema12:.4g}) > EMA26({ema26:.4g}) — short-term uptrend"))
            else:
                votes.append((0.6, -1, f"↘️ EMA12({ema12:.4g}) < EMA26({ema26:.4g}) — short-term downtrend"))
        else:
            votes.append((0.6, +1 if ema12 > ema26 else -1,
                          f"{'↗️' if ema12 > ema26 else '↘️'} EMA12 {'>' if ema12>ema26 else '<'} EMA26 — short-term trend"))

        # 4) Bollinger
        avg_bw = _avg_bandwidth(closes)
        squeeze = avg_bw > 0 and bandwidth < avg_bw * 0.7
        if squeeze:
            votes.append((0.3, 0, f"🤏 Bollinger squeeze (bw={bandwidth*100:.1f}% < avg {avg_bw*100:.1f}%) — ready to break out in either direction"))
        if pct_b > 1.0:
            votes.append((0.7, -1, f"⚠️ Price broke above Bollinger upper band (%B={pct_b:.2f}) — mean-reversion risk"))
        elif pct_b > 0.8:
            votes.append((0.4, +1, f"💪 Price in upper Bollinger zone (%B={pct_b:.2f}) — buyers in control"))
        elif pct_b < 0.0:
            votes.append((0.7, +1, f"💡 Price broke below Bollinger lower band (%B={pct_b:.2f}) — oversold, bounce risk"))
        elif pct_b < 0.2:
            votes.append((0.4, -1, f"🥶 Price in lower Bollinger zone (%B={pct_b:.2f}) — sellers in control"))

        # 5) Stochastic
        if stoch is not None:
            pct_k, pct_d = stoch
            if pct_k < 20 and pct_d < 20:
                votes.append((0.7, +1, f"📉 Stochastic %K={pct_k:.1f} %D={pct_d:.1f} — oversold zone <20, buy signal"))
            elif pct_k > 80 and pct_d > 80:
                votes.append((0.7, -1, f"📈 Stochastic %K={pct_k:.1f} %D={pct_d:.1f} — overbought zone >80, sell signal"))
            elif pct_k > pct_d and pct_k < 50:
                votes.append((0.4, +1, f"↗️ Stochastic Golden Cross: %K({pct_k:.1f}) crossed above %D({pct_d:.1f}) in low zone"))
            elif pct_k < pct_d and pct_k > 50:
                votes.append((0.4, -1, f"↘️ Stochastic Dead Cross: %K({pct_k:.1f}) crossed below %D({pct_d:.1f}) in high zone"))

        # 6) ADX
        if adx_val is not None:
            if adx_val >= 35:
                votes.append((0.5, 0, f"💪 ADX={adx_val:.1f} (≥35) — very strong trend, momentum reliable"))
            elif adx_val >= 25:
                votes.append((0.3, 0, f"📊 ADX={adx_val:.1f} (25-35) — clear trend, indicators working well"))
            elif adx_val >= 20:
                votes.append((0.2, 0, f"⚠️ ADX={adx_val:.1f} (20-25) — weak trend, watch for false signals"))
            else:
                votes.append((0.8, 0, f"🔴 ADX={adx_val:.1f} (<20) — sideways market, indicators prone to false signals, reducing all signal weights"))
                # Sideways market: reduce weight of all votes by one level
                votes = [(w * 0.6, s, m) for w, s, m in votes[:-1]] + [votes[-1]]

        # 7) Volume
        vol_note = ""
        if len(vols) >= 21 and sum(vols[-21:-1]) > 0:
            vol_sma = sum(vols[-21:-1]) / 20
            vol_ratio = vols[-1] / vol_sma if vol_sma > 0 else 1.0
            bar_up = closes[-1] >= closes[-2]
            if vol_ratio >= 1.5 and bar_up:
                votes.append((0.8, +1, f"🔊 Volume {vol_ratio:.1f}× + price up — buying pressure confirmed"))
            elif vol_ratio >= 1.5 and not bar_up:
                votes.append((0.8, -1, f"🔊 Volume {vol_ratio:.1f}× + price down — selling pressure confirmed"))
            elif vol_ratio < 0.6:
                votes.append((0.3, 0, f"🔇 Light volume ({vol_ratio:.1f}×) — lacks confirmation"))
            else:
                vol_note = f"🔉 Normal volume ({vol_ratio:.1f}×)"

        # 8) Support/Resistance
        if len(lows) >= 20 and len(highs) >= 20:
            sup = min(lows[-20:])
            res = max(highs[-20:])
            d_sup = (price / sup - 1) * 100 if sup > 0 else 99
            d_res = (res / price - 1) * 100 if price > 0 else 99
            if d_sup <= 2.0:
                votes.append((0.6, +1, f"🛡️ Price within {d_sup:.1f}% of support {sup:.4g} — favorable entry point"))
            elif d_res <= 2.0:
                votes.append((0.6, -1, f"🧱 Price approaching resistance {res:.4g}, only {d_res:.1f}% away — limited upside"))
            else:
                votes.append((0.2, 0, f"📐 Distance to support {d_sup:.1f}% / resistance {d_res:.1f}%"))

        # 9) Momentum
        recent = closes[-1] / closes[-10] - 1
        if recent > 0.02:
            votes.append((0.5, +1, f"🚀 10-bar Momentum={recent*100:+.1f}% — upside momentum"))
        elif recent < -0.02:
            votes.append((0.5, -1, f"🪂 10-bar Momentum={recent*100:+.1f}% — downside pressure"))
        else:
            votes.append((0.2, 0, f"➖ 10-bar Momentum={recent*100:+.1f}% — narrow range"))

        # Aggregate score
        total_w = sum(w for w, _, _ in votes) or 1.0
        score = sum(w * s for w, s, _ in votes) / total_w
        n_bull = sum(1 for _, s, _ in votes if s > 0)
        n_bear = sum(1 for _, s, _ in votes if s < 0)
        n_flat = sum(1 for _, s, _ in votes if s == 0)

        if score > 0.18:
            action = BUY
            confidence = min(0.95, 0.45 + score * 0.6)
        elif score < -0.18:
            action = SELL
            confidence = min(0.95, 0.45 + abs(score) * 0.6)
        else:
            action = HOLD
            confidence = 0.3

        lines = [msg for _, _, msg in votes]
        if vol_note:
            lines.append(vol_note)
        lines.append(
            f"🧮 bullish {n_bull} / bearish {n_bear} / neutral {n_flat} → score {score:+.2f} (±0.18) → {action}"
        )

        return AgentResult(
            agent=self.name, action=action, confidence=confidence,
            reasoning="\n".join(lines), horizon="short",
            extra={
                "rsi": round(rsi, 1),
                "ema12": round(ema12, 6), "ema26": round(ema26, 6),
                "ema50": round(ema50, 6) if ema50 is not None else None,
                "macd": round(macd_line, 6), "macd_signal": round(macd_sig, 6),
                "macd_hist": round(hist, 6),
                "pct_b": round(pct_b, 2), "bandwidth_pct": round(bandwidth * 100, 2),
                "squeeze": squeeze,
                "stoch_k": round(stoch[0], 1) if stoch else None,
                "stoch_d": round(stoch[1], 1) if stoch else None,
                "adx": round(adx_val, 1) if adx_val is not None else None,
                "rsi_divergence": div,
                "score": round(score, 3),
            },
        )
