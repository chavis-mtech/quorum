//! Presentation layer — HTTP/WebSocket (axum)

pub mod handlers;
pub mod middleware;
pub mod state;
pub mod ws;

use axum::{
    middleware as axum_middleware,
    routing::{delete, get, post, put},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::infrastructure::qpack;
use state::AppState;

/// Assemble all routes + serve frontend files (SPA)
pub fn router(st: AppState, frontend_dir: &str) -> Router {
    // --- public (no login required) ---
    let public = Router::new()
        .route("/health", get(handlers::health))
        .route("/about", get(handlers::about))
        .route("/auth/register", post(handlers::register))
        .route("/auth/login", post(handlers::login))
        // public market data (can be used for autocomplete before login)
        .route("/symbols/search", get(handlers::symbol_search))
        .route("/symbols/ticker/{symbol}", get(handlers::symbol_ticker));

    // --- protected (requires login + account selection) ---
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/profile", put(handlers::update_profile))
        .route("/auth/password", put(handlers::update_password))
        .route(
            "/accounts",
            get(handlers::list_accounts).post(handlers::create_account),
        )
        .route("/accounts/{id}", delete(handlers::delete_account))
        .route("/credentials/status", get(handlers::credentials_status))
        .route("/credentials", post(handlers::set_credentials))
        .route(
            "/ai/credentials/status",
            get(handlers::ai_credentials_status),
        )
        .route("/ai/credentials", post(handlers::set_ai_credentials))
        .route("/ai/models", get(handlers::ai_models))
        .route("/ai/compare", post(handlers::ai_compare))
        .route("/watch", get(handlers::get_watch).post(handlers::set_watch))
        .route("/analyze", post(handlers::analyze_now))
        .route("/decisions", get(handlers::recent_decisions))
        .route("/decisions/{symbol}", get(handlers::decisions_for_symbol))
        .route("/decision/{id}/analysis", get(handlers::decision_analysis))
        .route("/report", get(handlers::report))
        .route(
            "/settings",
            get(handlers::get_settings).put(handlers::put_settings),
        )
        .route("/wallet", get(handlers::get_wallet))
        .route("/wallet/reset", post(handlers::reset_wallet))
        .route("/account/balance", get(handlers::account_balance))
        .route(
            "/trades",
            get(handlers::recent_trades).delete(handlers::clear_trades),
        )
        .route("/trades/{symbol}", get(handlers::trades_for_symbol))
        .route("/trade", post(handlers::manual_trade))
        .route("/stats", get(handlers::get_stats))
        .route("/stats/reset", post(handlers::reset_stats))
        .route("/account/pause", post(handlers::set_pause))
        .route("/governor", get(handlers::get_governor))
        .route("/alerts", get(handlers::get_alerts))
        .route("/plans", get(handlers::get_plans))
        .route("/targets", get(handlers::get_targets))
        .route("/open-orders", get(handlers::get_open_orders))
        .route("/market/scan", get(handlers::market_scan))
        .route_layer(axum::middleware::from_fn_with_state(
            st.clone(),
            middleware::require_auth,
        ));

    let api = public
        .merge(protected)
        .with_state(st.clone())
        // QPACK negotiation: if client sends Accept: application/x-qpack or ?fmt=bin
        // middleware converts JSON response → binary automatically (handlers unchanged)
        .layer(axum_middleware::from_fn(qpack::negotiate));

    // SPA: serve dist + fallback to index.html
    let index = format!("{frontend_dir}/index.html");
    let spa = ServeDir::new(frontend_dir).fallback(ServeFile::new(index));

    Router::new()
        .route("/ws", get(ws::ws_handler))
        .nest("/api", api)
        .fallback_service(spa)
        .layer(CorsLayer::permissive())
        .with_state(st)
}
