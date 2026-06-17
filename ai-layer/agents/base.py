"""Central contract that all agents must follow — so the aggregator can compare votes"""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field, asdict
from typing import Any

# standard actions
BUY, SELL, HOLD = "BUY", "SELL", "HOLD"
VALID_ACTIONS = {BUY, SELL, HOLD}


def load_text_classifier(model_id: str, **kwargs: Any):
    """Load a HF text-classification pipeline in the lightest way without reducing accuracy

    - GPU (cuda) or Apple MPS available → use float16 (half the VRAM, negligible accuracy loss)
    - CPU only → keep float32 (fp16 on CPU risks unsupported ops → leave it alone)
    - Constrain torch to a single thread to reduce memory arena (does not affect results)
    Returns the pipeline or raises an exception (let the caller fallback to lexicon)
    """
    from transformers import pipeline  # type: ignore

    pipe_kwargs: dict[str, Any] = {"model": model_id, "top_k": None, **kwargs}
    try:
        import torch  # type: ignore

        torch.set_num_threads(1)  # 2 small models don't need multi-thread distribution → less memory
        if torch.cuda.is_available():
            pipe_kwargs["torch_dtype"] = torch.float16
            pipe_kwargs["device"] = 0
        elif getattr(getattr(torch, "backends", None), "mps", None) and torch.backends.mps.is_available():
            pipe_kwargs["torch_dtype"] = torch.float16
            pipe_kwargs["device"] = "mps"
    except Exception:
        pass  # no torch or detection failed → leave default (CPU fp32)
    return pipeline("text-classification", **pipe_kwargs)


@dataclass
class AgentResult:
    """Analysis result from a single agent — uniform format across all agents"""
    agent: str               # "technical" | "finbert" | "trend_ml" | "news"
    action: str              # BUY | SELL | HOLD
    confidence: float        # 0.0 - 1.0
    reasoning: str           # brief explanation of the decision
    horizon: str = "short"   # short | medium | long
    can_veto: bool = False   # whether this agent has veto rights
    veto: bool = False       # True = block the entire signal
    ok: bool = True          # False = this agent failed to run (e.g. missing data/model)
    extra: dict[str, Any] = field(default_factory=dict)

    def __post_init__(self) -> None:
        self.action = self.action.upper()
        if self.action not in VALID_ACTIONS:
            self.action = HOLD
        self.confidence = max(0.0, min(1.0, float(self.confidence)))

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass
class MarketContext:
    """Market data fed to all agents"""
    symbol: str                       # e.g. "BTC"
    quote: str                        # e.g. "THB"
    candles: list[dict[str, float]]   # [{ts, open, high, low, close, volume}, ...]
    last_price: float | None = None
    extra: dict[str, Any] = field(default_factory=dict)

    @property
    def closes(self) -> list[float]:
        return [c["close"] for c in self.candles if "close" in c]


class Agent(ABC):
    """abstract base — subclass and implement analyze()"""
    name: str = "agent"
    can_veto: bool = False

    def __init__(self, config: dict[str, Any] | None = None) -> None:
        self.config = config or {}

    @abstractmethod
    def analyze(self, ctx: MarketContext) -> AgentResult:  # pragma: no cover
        ...

    def _fail(self, reason: str) -> AgentResult:
        """Result when the agent cannot run — counted as HOLD/ok=False without breaking the loop"""
        return AgentResult(
            agent=self.name, action=HOLD, confidence=0.0,
            reasoning=f"[{self.name}] unavailable: {reason}", ok=False,
            can_veto=self.can_veto,
        )
