//! Quorum backend — composition root
//! Assembles adapters with use cases then starts the web server + watch loop (multi-tenant)

mod application;
mod config;
mod domain;
mod infrastructure;
mod presentation;

use std::sync::Arc;

use application::trading::TradingService;
use application::watch::Watcher;
use config::AppConfig;
use domain::ports::{
    AccountStore, AlertStore, Broker, BrokerResolver, HistoryRepository, MarketData, MarketScanner,
    PaperWallet, PlanRepository, SecretStore, SettingsStore, TradeRepository, UserStore,
    WatchlistStore,
};
use infrastructure::ai_sidecar::SidecarAiEngine;
use infrastructure::auth::Auth;
use infrastructure::bitkub::BitkubBroker;
use infrastructure::broker_resolver::PgBrokerResolver;
use infrastructure::events::BroadcastSink;
use infrastructure::market::BitkubMarket;
use infrastructure::postgres::PgStore;
use infrastructure::scanner::MomentumScanner;
use presentation::state::AppState;

const DEFAULT_ACCOUNT_ID: i64 = 1; // paper account for the default user (from migration)

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename("../.env");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,quorum=debug".into()),
        )
        .init();

    let cfg = AppConfig::load("../config/quorum.toml");
    tracing::info!(broker = %cfg.broker, "starting Quorum backend");

    // --- infrastructure: DB + migrate ---
    let store = PgStore::connect(&cfg.database_url).await?;
    store.migrate().await?;
    let store = Arc::new(store);

    // --- auth ---
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        let s = Auth::random_hex(32);
        tracing::warn!(
            "JWT_SECRET not found — using random temporary value (set JWT_SECRET in .env to keep sessions across restarts)"
        );
        s
    });
    let auth = Arc::new(Auth::new(jwt_secret.as_bytes()));

    // bootstrap: set the real password for the default user (first run) from ADMIN_PASSWORD
    let admin_pass = std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| Auth::random_hex(6));
    let hash = Auth::hash_password(&admin_pass)?;
    if let Some(email) = store.finalize_default_user(&hash).await? {
        tracing::warn!("🔑 Set initial password for admin account: {email} / {admin_pass}  (can be changed in the profile page)");
    }

    // --- ports from PgStore ---
    let repo: Arc<dyn HistoryRepository> = store.clone();
    let secrets: Arc<dyn SecretStore> = store.clone();
    let settings: Arc<dyn SettingsStore> = store.clone();
    let wallet: Arc<dyn PaperWallet> = store.clone();
    let trades: Arc<dyn TradeRepository> = store.clone();
    let plans: Arc<dyn PlanRepository> = store.clone();
    let watch_store: Arc<dyn WatchlistStore> = store.clone();
    let users: Arc<dyn UserStore> = store.clone();
    let accounts: Arc<dyn AccountStore> = store.clone();
    let alerts: Arc<dyn AlertStore> = store.clone();

    let ai = Arc::new(SidecarAiEngine::new(cfg.ai_sidecar_url.clone()));
    let market: Arc<dyn MarketData> = Arc::new(BitkubMarket::new("THB"));
    let scanner: Arc<dyn MarketScanner> = Arc::new(MomentumScanner::new(market.clone()));
    let events = BroadcastSink::new(512);

    // live_broker = public price feed (no key needed) ; order execution/balance uses resolver bound per-user key
    let live_broker: Arc<dyn Broker> = Arc::new(BitkubBroker::new(None));
    let broker_resolver: Arc<dyn BrokerResolver> =
        Arc::new(PgBrokerResolver::new(accounts.clone(), secrets.clone(), settings.clone()));

    // --- application ---
    let service = Arc::new(TradingService {
        ai: ai.clone(),
        live_broker: live_broker.clone(),
        broker_resolver: broker_resolver.clone(),
        repo: repo.clone(),
        trades: trades.clone(),
        wallet: wallet.clone(),
        plans: plans.clone(),
        settings: settings.clone(),
        secrets: secrets.clone(),
        accounts: accounts.clone(),
        events: Arc::new(events.clone()),
        alerts: alerts.clone(),
    });

    // seed initial watchlist for the default account if still empty
    if let Ok(syms) = watch_store.get_symbols(DEFAULT_ACCOUNT_ID).await {
        if syms.is_empty() {
            watch_store
                .set_symbols(DEFAULT_ACCOUNT_ID, &cfg.symbols)
                .await
                .ok();
        }
    }

    // --- watch loop (background, iterates all accounts with auto-trade enabled) ---
    let watcher = Watcher {
        service: service.clone(),
        accounts: accounts.clone(),
        settings: settings.clone(),
        watch_store: watch_store.clone(),
        scanner: scanner.clone(),
        events: Arc::new(events.clone()),
        monitor_interval: std::time::Duration::from_secs(cfg.watch_interval_secs),
        deep_interval: std::time::Duration::from_secs(cfg.deep_interval_secs),
        no_plan_cooldown: std::time::Duration::from_secs(cfg.no_plan_cooldown_secs),
    };
    tokio::spawn(watcher.run());

    // --- presentation ---
    let app_state = AppState {
        service,
        repo,
        trades,
        plans,
        secrets,
        settings,
        wallet,
        market,
        scanner,
        live_broker,
        watch_store,
        users,
        accounts,
        alerts,
        auth,
        events,
    };
    let app = presentation::router(app_state, &cfg.frontend_dir);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!("listening at http://{}", cfg.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
