"""Sentiment agent — one consolidated, dependency-free market-sentiment lens.

Replaces the old finbert + cryptobert pair. Those wrapped Hugging-Face models that needed
transformers + torch (~2 GB); on a small server without them they *abstained on every cycle*,
so the user saw "2 broken AIs". They also overlapped heavily with each other and with `news`.

This single advisor needs no ML runtime. It scores the same news + live-web headlines with a
curated financial + crypto-native lexicon and three pieces of real language handling:

  • negation   — "not bullish", "no breakout", "denies hack" flip the polarity of the next word
  • intensity  — "massive selloff", "sharp drop", "record high" weigh more than the bare word
  • de-dup     — near-identical headlines (same lead words) are counted once, not amplified

It always casts a real, calibrated vote (capped, low weight): sentiment SUPPORTS a setup, it
never drives one. The aggregator's structural rule still requires technical/trend_ml to agree
before any trade, and `news` remains the separate veto/safety layer.
"""
from __future__ import annotations

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD

# weighted lexicon — bigger magnitude = stronger signal. Merges the strongest cues from the old
# finbert (financial), cryptobert (crypto-native) and news lexicons, de-duplicated.
_WEIGHTS: dict[str, float] = {
    # ── bullish ──────────────────────────────────────────────────────────────
    "surge": 1.5, "soar": 1.5, "rally": 1.3, "breakout": 1.3, "moon": 1.3,
    "all-time high": 1.5, "ath": 1.3, "record": 1.0, "record high": 1.4,
    "bullish": 1.2, "gain": 0.8, "gains": 0.8, "upgrade": 1.0, "approval": 1.2,
    "approved": 1.2, "etf": 1.2, "spot etf": 1.4, "adoption": 1.1, "partnership": 1.0,
    "institutional": 1.0, "inflow": 1.1, "inflows": 1.1, "accumulation": 1.0,
    "accumulate": 0.9, "buyback": 1.0, "listing": 0.9, "listed": 0.8, "launch": 0.7,
    "mainnet": 0.8, "milestone": 0.8, "halving": 1.0, "staking": 0.6, "burn": 0.7,
    "rebound": 1.0, "recover": 0.9, "recovery": 0.9, "outperform": 1.1, "beat": 0.9,
    "profit": 0.7, "growth": 0.8, "pump": 1.0, "long": 0.5, "hodl": 0.6, "whale": 0.5,
    # ── bearish ──────────────────────────────────────────────────────────────
    "plunge": -1.5, "crash": -1.6, "selloff": -1.4, "sell-off": -1.4, "dump": -1.3,
    "tumble": -1.3, "slump": -1.2, "collapse": -1.6, "bearish": -1.2, "decline": -0.9,
    "drop": -0.9, "fall": -0.8, "falls": -0.8, "loss": -0.8, "losses": -0.8,
    "downgrade": -1.1, "outflow": -1.1, "outflows": -1.1, "liquidation": -1.2,
    "liquidated": -1.2, "correction": -0.9, "capitulation": -1.4, "panic": -1.3,
    "fear": -1.0, "fud": -0.9, "rug": -1.4, "rug pull": -1.6, "exploit": -1.3,
    "hack": -1.4, "hacked": -1.4, "breach": -1.2, "lawsuit": -1.1, "fraud": -1.4,
    "scam": -1.3, "ban": -1.2, "banned": -1.2, "delist": -1.3, "delisted": -1.3,
    "halt": -1.1, "halted": -1.1, "investigation": -0.9, "bankrupt": -1.5,
    "bankruptcy": -1.5, "insolvent": -1.5, "short": -0.5, "breakdown": -1.1,
    "weak": -0.6, "warning": -0.7, "risk": -0.4, "concern": -0.5, "crackdown": -1.0,
}

_NEGATIONS = {"no", "not", "never", "without", "denies", "denied", "avoid", "avoids",
              "isn't", "aren't", "wasn't", "weren't", "fails", "fail", "halts", "ends"}
_INTENSIFIERS = {"very": 1.4, "massive": 1.6, "huge": 1.5, "sharp": 1.4, "sharply": 1.4,
                 "record": 1.4, "extreme": 1.6, "strong": 1.3, "strongly": 1.3,
                 "major": 1.3, "significant": 1.3, "steep": 1.4, "deep": 1.3}
# longest multi-word phrases first so "rug pull" / "all-time high" match before "rug" / "high"
_PHRASES = sorted((k for k in _WEIGHTS if " " in k), key=len, reverse=True)


def _dedup(headlines: list[str]) -> list[str]:
    """Drop near-duplicate headlines (same first 8 lowercased words) so a story repeated across
    sources is counted once, not amplified into a fake-strong signal."""
    seen: set[str] = set()
    out: list[str] = []
    for h in headlines:
        key = " ".join(h.lower().split()[:8])
        if key and key not in seen:
            seen.add(key)
            out.append(h)
    return out


def _score_headline(text: str) -> tuple[float, list[tuple[str, float]]]:
    """Return (net_score, [(keyword, signed_weight), ...]) for one headline."""
    low = " " + text.lower() + " "
    for ph in _PHRASES:                      # collapse phrases to single tokens
        low = low.replace(" " + ph + " ", " " + ph.replace(" ", "_") + " ")
    tokens = low.split()
    hits: list[tuple[str, float]] = []
    total = 0.0
    for i, tok in enumerate(tokens):
        key = tok.replace("_", " ")
        w = _WEIGHTS.get(key)
        if w is None:
            continue
        # intensity: a modifier in the 2 tokens before the cue scales it
        mult = 1.0
        for j in (i - 1, i - 2):
            if j >= 0:
                mult = max(mult, _INTENSIFIERS.get(tokens[j], 1.0))
        # negation: a negator in the 3 tokens before flips polarity
        if any(tokens[j] in _NEGATIONS for j in range(max(0, i - 3), i)):
            w = -w * 0.8                     # negated cue is weaker than a direct opposite cue
        signed = w * mult
        total += signed
        hits.append((key, round(signed, 2)))
    return total, hits


class SentimentAgent(Agent):
    name = "sentiment"

    def analyze(self, ctx: MarketContext) -> AgentResult:
        headlines: list[str] = _dedup(ctx.extra.get("headlines", []))
        if not headlines:
            return self._fail("No news/web headlines available to analyze")

        net = 0.0
        pos: dict[str, int] = {}
        neg: dict[str, int] = {}
        for h in headlines[:30]:
            sc, hits = _score_headline(h)
            net += sc
            for kw, sw in hits:
                (pos if sw > 0 else neg)[kw] = (pos if sw > 0 else neg).get(kw, 0) + 1

        # normalise by headline count so 3 strong cues in 4 headlines outweigh 3 in 40
        norm = net / max(len(headlines), 1)
        # confidence stays modest — this is a SUPPORTING lens, capped at 0.60
        confidence = min(0.60, 0.30 + abs(norm) * 0.6)

        if norm > 0.20:
            action = BUY
        elif norm < -0.20:
            action = SELL
        else:
            action, confidence = HOLD, min(confidence, 0.32)

        def _fmt(d: dict[str, int]) -> str:
            return ", ".join(f"{k}×{n}" if n > 1 else k
                             for k, n in sorted(d.items(), key=lambda x: -x[1])[:5]) or "—"

        lines = [
            f"🗞️ Analyzed {len(headlines)} unique headlines (negation- & intensity-aware lexicon)",
            f"🟢 bullish cues: {_fmt(pos)}",
            f"🔴 bearish cues: {_fmt(neg)}",
            f"🧮 net sentiment {net:+.2f} → normalized {norm:+.2f} "
            f"(BUY>+0.20, SELL<-0.20) → {action} (conf {confidence:.2f}, supporting weight)",
        ]
        return AgentResult(
            agent=self.name, action=action, confidence=confidence,
            reasoning="\n".join(lines), horizon="short",
            extra={"net": round(net, 2), "normalized": round(norm, 3),
                   "headlines": len(headlines), "engine": "lexicon-v2"},
        )
