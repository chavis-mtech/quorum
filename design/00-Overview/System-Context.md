# System Context (C4 — Level 1)

How Quorum sits in the world: who uses it, and which external systems it depends on.

```mermaid
flowchart TB
    subgraph people [People]
        trader([👤 Trader / Tenant<br/>self-signup, runs own capital])
        admin([🛠️ Operator<br/>owner@quorum.local])
    end

    subgraph quorum [⚖️ Quorum Platform]
        sys[Multi-agent consensus<br/>trading system]
    end

    subgraph external [External Systems]
        bitkub[(🏦 Bitkub<br/>prices, balances, orders)]
        llm[🤖 LLM providers<br/>Ollama local / Anthropic / OpenAI-compat]
        web[🌐 Web & News<br/>DuckDuckGo, Finnhub, NewsAPI]
    end

    trader -->|analyses, settings,<br/>kill-switch| sys
    admin -->|operates, deploys| sys
    sys -->|market data + HMAC-signed orders| bitkub
    sys -->|judge reasoning / verdicts| llm
    sys -->|sentiment + event veto| web

    classDef sys fill:#1e3a5f,stroke:#4a90d9,color:#fff;
    classDef ext fill:#2d2d2d,stroke:#888,color:#ddd;
    classDef ppl fill:#3a2d4f,stroke:#9d7ad9,color:#fff;
    class sys sys; class bitkub,llm,web ext; class trader,admin ppl;
```

## Actors

| Actor | Goal | Touchpoints |
|-------|------|-------------|
| **Trader / Tenant** | Grow capital with disciplined automation | Web UI: trigger analysis, set risk/strategy, watch positions, pull the kill-switch |
| **Operator** | Keep the platform healthy | Deploy, monitor governor/alerts, manage the default admin account |

## External dependencies

| System | Role | Failure posture |
|--------|------|-----------------|
| **Bitkub** | Source of truth for prices, balances, and order execution (THB pairs) | Price read fails → skip tick; order fails → record + alert, never silently lose state. See [[Broker-Integration]]. |
| **LLM providers** | The judge's reasoning engine. Local **Ollama** first, cloud (Anthropic / OpenAI-compatible) as fallback | If all providers fail → deterministic **rule-based planner** takes over (never blocks trading). See [[Analysis-Pipeline]]. |
| **Web / News** | Sentiment signal + a hard **veto** on critical events (hacks, delistings) | Unavailable → agents abstain rather than cast bad votes. |

## Trust & data-sensitivity boundaries

- **Bitkub API credentials are per-tenant** and stored encrypted server-side; they never leave the backend and are never shared between tenants. ([[Deployment-and-Security]])
- The **web UI never holds broker secrets** — it authenticates with a JWT and an `X-Account-Id`; the backend resolves the right credentials.
- The **LLM never sees credentials** — only market structure, votes, and portfolio context.

Next: [[Container-Architecture]]
