"""FinBERT agent — pre-trained model from Hugging Face (ProsusAI/finbert)
Analyzes sentiment of financial news headlines → converts to BUY/SELL/HOLD

If transformers/torch are not installed, falls back to a small lexicon
so the pipeline remains complete (ok=True but low confidence)
"""
from __future__ import annotations

from .base import Agent, AgentResult, MarketContext, BUY, SELL, HOLD, load_text_classifier

_MODEL_ID = "ProsusAI/finbert"
_pipe = None          # lazy singleton
_pipe_failed = False

_POS = {"surge", "gain", "rally", "beat", "bullish", "soar", "record", "upgrade", "profit", "growth"}
_NEG = {"plunge", "drop", "loss", "miss", "bearish", "crash", "ban", "hack", "lawsuit", "fraud", "downgrade"}


def _get_pipe():
    global _pipe, _pipe_failed
    if _pipe is not None or _pipe_failed:
        return _pipe
    try:
        _pipe = load_text_classifier(_MODEL_ID)
    except Exception:
        _pipe_failed = True
        _pipe = None
    return _pipe


def _lexicon_score(headlines: list[str]) -> float:
    score = 0
    for h in headlines:
        words = h.lower().split()
        score += sum(1 for w in words if w in _POS)
        score -= sum(1 for w in words if w in _NEG)
    return score


class FinBertAgent(Agent):
    name = "finbert"

    def analyze(self, ctx: MarketContext) -> AgentResult:
        headlines: list[str] = ctx.extra.get("headlines", [])
        if not headlines:
            return self._fail("No headlines available to analyze")

        pipe = _get_pipe()
        if pipe is not None:
            pos = neg = 0.0
            for h in headlines[:20]:
                try:
                    scores = pipe(h)[0]  # list of {label, score}
                    d = {s["label"].lower(): s["score"] for s in scores}
                    pos += d.get("positive", 0.0)
                    neg += d.get("negative", 0.0)
                except Exception:
                    continue
            net = pos - neg
            confidence = min(0.95, 0.4 + abs(net) / max(len(headlines), 1))
            model_note = "FinBERT"
        else:
            raw = _lexicon_score(headlines)
            # lexicon fallback is not accurate enough to count as a vote — abstain (ok=False)
            # to avoid an empty HOLD blocking a BUY that comes from technical/trend_ml
            return AgentResult(
                agent=self.name, action=HOLD, confidence=0.0,
                reasoning=f"lexicon fallback (no transformers) — abstaining (score={raw:+.1f} from {len(headlines)} headlines)",
                ok=False, horizon="short",
                extra={"net_sentiment": raw, "engine": "lexicon fallback", "abstained": True},
            )

        if net > 0.5:
            action = BUY
        elif net < -0.5:
            action = SELL
        else:
            action = HOLD
            confidence = min(confidence, 0.35)

        return AgentResult(
            agent=self.name, action=action, confidence=confidence,
            reasoning=f"FinBERT: news sentiment net={net:+.2f} from {len(headlines)} headlines",
            horizon="short", extra={"net_sentiment": net, "engine": "FinBERT"},
        )
