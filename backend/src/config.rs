//! Config — loaded from env (infra) + config/quorum.toml (market)

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct AppConfig {
    // infra (env)
    pub database_url: String,
    pub ai_sidecar_url: String,
    pub bind_addr: String,
    pub frontend_dir: String,
    pub watch_interval_secs: u64,   // price monitoring cadence (monitor, light)
    pub deep_interval_secs: u64,    // deep analysis cadence (deep, heavy)
    pub no_plan_cooldown_secs: u64, // cooldown for assets analyzed but not yet planned
    // market (toml) — used as seed watchlist for the default account
    pub broker: String,
    pub symbols: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlRoot {
    market: Option<TomlMarket>,
}
#[derive(Debug, Deserialize, Default)]
struct TomlMarket {
    broker: Option<String>,
    symbols: Option<Vec<String>>,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn frontend_dir() -> String {
    let configured = env_or("FRONTEND_DIR", "../frontend/dist");
    let candidates = [
        configured.as_str(),
        "../frontend/dist",
        "frontend/dist",
        "./frontend/dist",
    ];
    for path in candidates {
        if std::path::Path::new(path).join("index.html").is_file() {
            if path != configured {
                tracing::warn!(
                    configured = %configured,
                    fallback = %path,
                    "FRONTEND_DIR not usable; falling back to frontend dist"
                );
            }
            return path.to_string();
        }
    }
    configured
}

impl AppConfig {
    pub fn load(toml_path: &str) -> Self {
        let root: TomlRoot = std::fs::read_to_string(toml_path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();

        let market = root.market.unwrap_or_default();

        AppConfig {
            database_url: env_or(
                "DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/quorum",
            ),
            ai_sidecar_url: env_or("AI_SIDECAR_URL", "http://127.0.0.1:8765"),
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            frontend_dir: frontend_dir(),
            watch_interval_secs: env_or("WATCH_INTERVAL_SECS", "45").parse().unwrap_or(45),
            deep_interval_secs: env_or("DEEP_INTERVAL_SECS", "900").parse().unwrap_or(900),
            no_plan_cooldown_secs: env_or("NO_PLAN_COOLDOWN_SECS", "3600")
                .parse()
                .unwrap_or(3600),
            broker: market.broker.unwrap_or_else(|| "bitkub".into()),
            symbols: market
                .symbols
                .unwrap_or_else(|| vec!["BTC".into(), "ETH".into()]),
        }
    }
}
