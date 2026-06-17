"""CLI — run analysis directly from the terminal

    python ai-layer/cli.py BTC ETH
    python ai-layer/cli.py            # use symbols from config
    python ai-layer/cli.py --json BTC # raw JSON output (for piping to other programs)
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))  # allow import of agents/...

from config_loader import load_config
from pipeline import analyze_symbol

_C = {"BUY": "\033[32m", "SELL": "\033[31m", "HOLD": "\033[33m",
      "dim": "\033[2m", "b": "\033[1m", "r": "\033[0m"}


def _color(action: str) -> str:
    return _C.get(action, "")


def _print_human(res: dict) -> None:
    v = res["verdict"]
    c = res["consensus"]
    sym = f"{res['symbol']}/{res['quote']}"
    price = res["last_price"]
    print(f"\n{_C['b']}━━━ {sym} ━━━{_C['r']}  price: {price}  mode: {res['mode']}")
    src = res["data_source"] + (" (simulated/offline)" if res["synthetic"] else "")
    print(f"{_C['dim']}data: {src} | news: {res['news_source']} ({res['news_count']} articles){_C['r']}")

    print(f"\n  {_C['b']}Advisor votes:{_C['r']}")
    for vote in c["votes"]:
        mark = "🛑" if vote.get("veto") else "  "
        col = _color(vote["action"])
        ok = "" if vote["ok"] else f" {_C['dim']}(not ready){_C['r']}"
        print(f"   {mark} {col}{vote['action']:<4}{_C['r']} "
              f"{vote['confidence']:.2f}  {_C['b']}{vote['agent']:<10}{_C['r']}"
              f"{ok} {_C['dim']}{vote['reasoning'][:70]}{_C['r']}")

    col = _color(c["action"])
    print(f"\n  {_C['b']}Consensus:{_C['r']} {col}{c['action']}{_C['r']} "
          f"(agreed {c['agreement']}/{c['voted']}, conf {c['confidence']:.2f}, "
          f"passed_threshold={c['passed_threshold']}, veto={c['vetoed']})")
    print(f"  {_C['dim']}{c['reasoning']}{_C['r']}")

    col = _color(v["action"])
    print(f"\n  {_C['b']}⚖️  Final verdict (judge):{_C['r']} {col}{_C['b']}{v['action']}{_C['r']} "
          f"conf {v['confidence']:.2f}  {_C['dim']}[{v.get('engine')}]{_C['r']}")
    print(f"  {v['reasoning']}\n")


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith("--")]
    as_json = "--json" in argv
    cfg = load_config()
    symbols = args or cfg["market"]["symbols"]

    out = []
    for sym in symbols:
        res = analyze_symbol(sym, cfg)
        out.append(res)
        if not as_json:
            _print_human(res)

    if as_json:
        print(json.dumps(out, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
