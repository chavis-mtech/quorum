"""Self-learning layer — the part of Quorum that gets better on its own.

The council (technical/trend_ml/flow) + judge produce a verdict every cycle. This package
remembers every verdict, watches what the market actually did next (settling each one against
the real candle high/low path — a live backtest of the bot's OWN calls), and feeds that back:

  • journal.py  — durable store of decisions + learned per-setup statistics (survives deploys)
  • settle.py   — given the candles after a decision, did the trade hit target (+R) or stop (−R)?
  • edge.py     — per "setup bucket" expectancy; decide block / allow / size for a new signal
  • brain.py    — orchestrates: settle past → learn → gate the new verdict → record (live or shadow)

"Shadow trades" are hypothetical orders recorded even when the bot does NOT trade live — so it
keeps learning whether a setup WOULD have worked, without risking money. That is the
"จำลอง order ในใจ" the user asked for.
"""
