"""CryptoBERT agent — pre-trained model from Hugging Face (ElKulako/cryptobert)
trained specifically for sentiment on crypto posts/news (Bullish / Neutral / Bearish)

Acts as a "ready-made AI advisor built specifically for crypto analysis".
If transformers/torch are unavailable, falls back to keyword scoring (still runnable).
"""
from __future__ import annotations

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD, load_text_classifier

_MODEL_ID = "ElKulako/cryptobert"
_pipe = None
_pipe_failed = False

_BULL = {"moon", "bullish", "buy", "long", "pump", "breakout", "ath", "rally", "accumulate", "hodl"}
_BEAR = {"dump", "bearish", "sell", "short", "crash", "rug", "fud", "capitulation", "breakdown", "liquidated"}


def _get_pipe():
    global _pipe, _pipe_failed
    if _pipe is not None or _pipe_failed:
        return _pipe
    try:
        _pipe = load_text_classifier(_MODEL_ID, max_length=64, truncation=True)
    except Exception:
        _pipe_failed = True
        _pipe = None
    return _pipe


def _keyword_score(texts: list[str]) -> float:
    score = 0
    for t in texts:
        w = t.lower().split()
        score += sum(1 for x in w if x in _BULL)
        score -= sum(1 for x in w if x in _BEAR)
    return score


class CryptoBertAgent(Agent):
    name = "cryptobert"

    def analyze(self, ctx: MarketContext) -> AgentResult:
        texts: list[str] = ctx.extra.get("headlines", [])
        if not texts:
            return self._fail("No text/news available to analyze")

        pipe = _get_pipe()
        if pipe is not None:
            bull = bear = 0.0
            for t in texts[:20]:
                try:
                    scores = pipe(t)[0]
                    d = {s["label"].lower(): s["score"] for s in scores}
                    # cryptobert labels: Bearish / Neutral / Bullish
                    bull += d.get("bullish", 0.0)
                    bear += d.get("bearish", 0.0)
                except Exception:
                    continue
            net = bull - bear
            confidence = min(0.95, 0.4 + abs(net) / max(len(texts), 1))
            engine = "CryptoBERT"
        else:
            net = _keyword_score(texts)
            # keyword fallback is not accurate enough — abstain (ok=False) so it does not block real BUY signals
            return AgentResult(
                agent=self.name, action=HOLD, confidence=0.0,
                reasoning=f"keyword fallback (no transformers) — abstaining (score={net:+.1f} from {len(texts)} texts)",
                ok=False, horizon="short",
                extra={"net": net, "engine": "keyword fallback", "abstained": True},
            )

        if net > 0.5:
            action = BUY
        elif net < -0.5:
            action = SELL
        else:
            action, confidence = HOLD, min(confidence, 0.35)

        return AgentResult(
            agent=self.name, action=action, confidence=confidence,
            reasoning=f"CryptoBERT: crypto sentiment net={net:+.2f} from {len(texts)} texts",
            horizon="short", extra={"net": net, "engine": "CryptoBERT"},
        )
