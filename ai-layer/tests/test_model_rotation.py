"""Test Gemini model auto-rotation: a rate-limited model is skipped, the next is tried,
and a 429'd model is put on cooldown — so the bot survives free-tier limits without stalling.

Run:  python ai-layer/tests/test_model_rotation.py   (no network — _openai_compatible is stubbed)
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import judge


def _reset():
    judge._MODEL_COOLDOWN.clear()


def test_detects_rate_limit_errors():
    assert judge._is_rate_limited_err("HTTP 429: Too Many Requests")
    assert judge._is_rate_limited_err("RESOURCE_EXHAUSTED: quota exceeded")
    assert judge._is_rate_limited_err("error: too many requests")
    assert not judge._is_rate_limited_err("HTTP 500: server error")
    assert not judge._is_rate_limited_err(None)


def test_is_gemini_cfg_by_provider_and_baseurl():
    assert judge._is_gemini_cfg("gemini", {})
    assert judge._is_gemini_cfg("custom", {"base_url": "https://generativelanguage.googleapis.com/v1beta/openai"})
    assert not judge._is_gemini_cfg("custom", {"base_url": "https://api.openai.com/v1"})
    assert not judge._is_gemini_cfg("openai", {})


def test_model_order_puts_configured_first_and_dedups():
    _reset()
    order = judge._gemini_model_order({"model": "gemini-2.5-flash"})
    assert order[0] == "gemini-2.5-flash"          # configured model leads
    assert len(order) == len(set(order))            # no duplicates
    assert "gemini-3.1-flash-lite" in order         # built-in rotation appended


def test_rotate_skips_rate_limited_then_succeeds():
    _reset()
    calls: list[str] = []

    def fake(prompt, key, base, model, timeout=120.0):
        calls.append(model)
        if model == "gemini-3.1-flash-lite":
            return None, "HTTP 429: rate limit exceeded"
        return {"verdict": {"action": "BUY", "confidence": 0.7}, "thinking": ""}, None

    orig = judge._openai_compatible
    judge._openai_compatible = fake
    try:
        out, err = judge._gemini_rotate("prompt", {"api_key": "k", "model": "gemini-3.1-flash-lite"})
    finally:
        judge._openai_compatible = orig

    assert err is None
    assert calls[0] == "gemini-3.1-flash-lite"             # tried configured model first
    assert out["_model"] == "gemini-2.5-flash-lite"        # then fell over to the next
    assert judge._MODEL_COOLDOWN.get("gemini-3.1-flash-lite", 0) > 0  # 429'd model cooling down
    assert "rotated to" in out.get("_rotation", "")


def test_cooldown_deprioritises_a_limited_model():
    _reset()
    judge._MODEL_COOLDOWN["gemini-3.1-flash-lite"] = judge.time.time() + 600
    order = judge._gemini_model_order({"model": "gemini-3.1-flash-lite"})
    # the cooling model is pushed to the back even though it's the configured one
    assert order[-1] == "gemini-3.1-flash-lite"
    assert order[0] != "gemini-3.1-flash-lite"


def test_rotate_requires_api_key():
    _reset()
    out, err = judge._gemini_rotate("p", {"model": "gemini-2.5-flash"})
    assert out is None and "API key" in err


def test_all_models_rate_limited_returns_error_and_cools_all():
    _reset()

    def fake(prompt, key, base, model, timeout=120.0):
        return None, "429 RESOURCE_EXHAUSTED"

    orig = judge._openai_compatible
    judge._openai_compatible = fake
    try:
        out, err = judge._gemini_rotate("p", {"api_key": "k", "model": "gemini-2.5-flash"})
    finally:
        judge._openai_compatible = orig

    assert out is None and "unavailable" in err
    assert all(judge._MODEL_COOLDOWN.get(m, 0) > 0 for m in judge._GEMINI_ROTATION)


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"  ✓ {fn.__name__}")
    print(f"\n{len(fns)}/{len(fns)} passed")


if __name__ == "__main__":
    _run_all()
