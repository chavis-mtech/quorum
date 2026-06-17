//! HTTP handlers — convert request → call use case → respond with JSON
//! After multi-tenant: every handler that requires login receives `Ctx` (user + selected account)

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Instant;

use super::state::AppState;
use crate::application::trading::{ai_provider_needs_key, normalize_ai_provider, TradingService};
use crate::domain::models::{
    AccountKind, Action, BrokerCredentials, Ctx, LiveEvent, PlanState, TargetStatus, TradingMode,
    UserView,
};
use crate::domain::ports::EventSink;
use crate::infrastructure::auth::Auth;

fn err(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": msg.to_string() })))
}

// ============================================================
//  AUTH
// ============================================================

#[derive(Deserialize)]
pub struct RegisterBody {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub display_name: String,
}

async fn auth_payload(st: &AppState, user_id: i64, email: &str) -> impl IntoResponse {
    let token = match st.auth.issue(user_id, email) {
        Ok(t) => t,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let user = st.users.by_id(user_id).await.ok().flatten();
    let accounts = st.accounts.for_user(user_id).await.unwrap_or_default();
    Json(json!({
        "token": token,
        "user": user.as_ref().map(UserView::from),
        "accounts": accounts,
    }))
    .into_response()
}

pub async fn register(
    State(st): State<AppState>,
    Json(b): Json<RegisterBody>,
) -> impl IntoResponse {
    let email = b.email.trim().to_lowercase();
    if !email.contains('@') || email.len() < 3 {
        return err(StatusCode::BAD_REQUEST, "Invalid email address").into_response();
    }
    if b.password.len() < 6 {
        return err(StatusCode::BAD_REQUEST, "Password must be at least 6 characters").into_response();
    }
    let hash = match Auth::hash_password(&b.password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let display = if b.display_name.trim().is_empty() {
        email.split('@').next().unwrap_or("trader").to_string()
    } else {
        b.display_name.trim().to_string()
    };
    // The very first user in the system is automatically admin
    let role = if st.users.count().await.unwrap_or(1) == 0 {
        "admin"
    } else {
        "user"
    };
    let user = match st.users.create(&email, &hash, &display, role).await {
        Ok(u) => u,
        Err(e) => return err(StatusCode::BAD_REQUEST, e).into_response(),
    };
    // Automatically create paper + live accounts
    st.accounts
        .create(user.id, AccountKind::Paper, "Paper")
        .await
        .ok();
    st.accounts
        .create(user.id, AccountKind::Live, "Live")
        .await
        .ok();
    auth_payload(&st, user.id, &user.email)
        .await
        .into_response()
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

pub async fn login(State(st): State<AppState>, Json(b): Json<LoginBody>) -> impl IntoResponse {
    let user = match st.users.by_email(&b.email.trim().to_lowercase()).await {
        Ok(Some(u)) => u,
        _ => return err(StatusCode::UNAUTHORIZED, "Invalid email or password").into_response(),
    };
    if !Auth::verify_password(&b.password, &user.password_hash) {
        return err(StatusCode::UNAUTHORIZED, "Invalid email or password").into_response();
    }
    auth_payload(&st, user.id, &user.email)
        .await
        .into_response()
}

pub async fn me(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    let user = st.users.by_id(ctx.user_id).await.ok().flatten();
    let accounts = st.accounts.for_user(ctx.user_id).await.unwrap_or_default();
    let bitkub = st
        .secrets
        .exists(ctx.user_id, "bitkub")
        .await
        .unwrap_or(false);
    Json(json!({
        "user": user.as_ref().map(UserView::from),
        "accounts": accounts,
        "active_account_id": ctx.account_id,
        "bitkub_configured": bitkub,
    }))
}

#[derive(Deserialize)]
pub struct ProfileBody {
    pub display_name: String,
}
pub async fn update_profile(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<ProfileBody>,
) -> impl IntoResponse {
    match st
        .users
        .update_profile(ctx.user_id, b.display_name.trim())
        .await
    {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PasswordBody {
    pub current_password: String,
    pub new_password: String,
}
pub async fn update_password(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<PasswordBody>,
) -> impl IntoResponse {
    if b.new_password.len() < 6 {
        return err(StatusCode::BAD_REQUEST, "New password must be at least 6 characters").into_response();
    }
    let user = match st.users.by_id(ctx.user_id).await {
        Ok(Some(u)) => u,
        _ => return err(StatusCode::NOT_FOUND, "User not found").into_response(),
    };
    if !Auth::verify_password(&b.current_password, &user.password_hash) {
        return err(StatusCode::FORBIDDEN, "Current password is incorrect").into_response();
    }
    let hash = match Auth::hash_password(&b.new_password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    match st.users.update_password(ctx.user_id, &hash).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn list_accounts(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.accounts.for_user(ctx.user_id).await {
        Ok(a) => Json(a).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateAccountBody {
    pub kind: String, // paper | live
    pub name: String,
}
pub async fn create_account(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<CreateAccountBody>,
) -> impl IntoResponse {
    let kind = AccountKind::parse(&b.kind);
    let name = b.name.trim();
    if name.is_empty() {
        return err(StatusCode::BAD_REQUEST, "Account name is required").into_response();
    }
    match st.accounts.create(ctx.user_id, kind, name).await {
        Ok(a) => Json(a).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

/// DELETE /api/accounts/{id} — delete account (cascade deletes all account data)
/// Guard against deleting the last account: user must retain at least 1 account
pub async fn delete_account(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let accs = match st.accounts.for_user(ctx.user_id).await {
        Ok(a) => a,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    if !accs.iter().any(|a| a.id == id) {
        return err(StatusCode::NOT_FOUND, "Account not found in your accounts").into_response();
    }
    if accs.len() <= 1 {
        return err(
            StatusCode::BAD_REQUEST,
            "At least 1 account must remain — cannot delete the last account",
        )
        .into_response();
    }
    match st.accounts.delete(ctx.user_id, id).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

// ============================================================
//  HEALTH / CREDENTIALS
// ============================================================

pub async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let ai_ok = st.service.ai.health().await.is_ok();
    Json(json!({
        "ai_engine": ai_ok,
        "broker": st.live_broker.name(),
    }))
}

/// GET /api/about — build info + system status (no login required)
pub async fn about(State(st): State<AppState>) -> impl IntoResponse {
    let ai_ok = st.service.ai.health().await.is_ok();
    Json(json!({
        "name": "Quorum",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Multi-agent consensus trading — built for precision, not guessing",
        "tagline": "AI consensus trading system — analyze together, decide together",
        "wire_protocol": "QPACK v1 (custom binary, zero-dep)",
        "architecture": "Rust + axum (clean arch) · Python AI sidecar · SolidJS · PostgreSQL",
        "components": {
            "ai_sidecar": if ai_ok { "online" } else { "offline" },
            "broker": st.live_broker.name(),
            "wire": "QPACK/1 (Accept: application/x-qpack | ?fmt=bin)",
        },
        "phases_shipped": [
            "Phase 0 — correctness audit + WS lag fix",
            "Phase 1 — identity / multi-tenant (users + accounts)",
            "Phase 2 — paper / live account isolation",
            "Phase 3 — capital governor (halted/scanning/paused + kill-switch)",
            "Phase 4 — target pipeline visibility (why hasn't it bought?)",
            "Phase 5 — AI provider settings (Ollama / Claude / OpenAI / Groq / OpenRouter)",
            "Phase 6 — QPACK binary wire (REST + WS, ~50% size reduction)",
            "Phase 7 — executive polish (this endpoint, branding, docs)",
        ],
        "signature": "Crafted with Claude · Quorum"
    }))
}

#[derive(Deserialize)]
pub struct BrokerQuery {
    #[serde(default = "default_broker")]
    pub broker: String,
}
fn default_broker() -> String {
    "bitkub".into()
}

pub async fn credentials_status(
    State(st): State<AppState>,
    ctx: Ctx,
    Query(q): Query<BrokerQuery>,
) -> impl IntoResponse {
    match st.secrets.meta(ctx.user_id, &q.broker).await {
        Ok(meta) => Json(json!({
            "broker": q.broker,
            "configured": meta.is_some(),
            "api_key_hint": meta.as_ref().map(|m| m.api_key_hint.as_str()).unwrap_or(""),
            "api_secret_hint": meta.as_ref().map(|m| m.api_secret_hint.as_str()).unwrap_or(""),
            "updated_at": meta.as_ref().map(|m| m.updated_at),
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn set_credentials(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(c): Json<BrokerCredentials>,
) -> impl IntoResponse {
    if c.api_key.trim().is_empty() || c.api_secret.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "api_key/api_secret must not be empty").into_response();
    }
    match st.secrets.set(ctx.user_id, &c).await {
        Ok(_) => {
            if c.broker.eq_ignore_ascii_case("bitkub") {
                st.events.publish(&LiveEvent::Status {
                    message: "Bitkub API key configured".into(),
                    healthy: true,
                });
            }
            Json(json!({ "ok": true, "broker": c.broker })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================
//  AI JUDGE SETTINGS / CREDENTIALS (per-user key, per-account preference)
// ============================================================

#[derive(Deserialize)]
pub struct AiProviderQuery {
    #[serde(default = "default_ai_provider")]
    pub provider: String,
}
fn default_ai_provider() -> String {
    "ollama".into()
}

fn ai_secret_name(provider: &str) -> String {
    format!("ai:{}", normalize_ai_provider(provider))
}

pub async fn ai_credentials_status(
    State(st): State<AppState>,
    ctx: Ctx,
    Query(q): Query<AiProviderQuery>,
) -> impl IntoResponse {
    let provider = normalize_ai_provider(&q.provider);
    let needs_key = ai_provider_needs_key(&provider);
    let meta = if needs_key {
        st.secrets
            .meta(ctx.user_id, &ai_secret_name(&provider))
            .await
            .unwrap_or(None)
    } else {
        None
    };
    let configured = if needs_key { meta.is_some() } else { true };
    Json(json!({
        "provider": provider,
        "needs_key": needs_key,
        "configured": configured,
        "api_key_hint": meta.as_ref().map(|m| m.api_key_hint.as_str()).unwrap_or(""),
        "updated_at": meta.as_ref().map(|m| m.updated_at),
    }))
}

#[derive(Deserialize)]
pub struct AiCredentialsBody {
    pub provider: String,
    pub api_key: String,
}
pub async fn set_ai_credentials(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<AiCredentialsBody>,
) -> impl IntoResponse {
    let provider = normalize_ai_provider(&b.provider);
    if !ai_provider_needs_key(&provider) {
        return Json(json!({ "ok": true, "provider": provider, "configured": true }))
            .into_response();
    }
    let api_key = b.api_key.trim();
    if api_key.is_empty() {
        return err(StatusCode::BAD_REQUEST, "API key is required for cloud AI").into_response();
    }
    let creds = BrokerCredentials {
        broker: ai_secret_name(&provider),
        api_key: api_key.into(),
        api_secret: String::new(),
    };
    match st.secrets.set(ctx.user_id, &creds).await {
        Ok(_) => {
            Json(json!({ "ok": true, "provider": provider, "configured": true })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct AiModelsQuery {
    #[serde(default = "default_ai_provider")]
    pub provider: String,
}

/// List of models available in the dropdown
/// - ollama → fetched from the actual machine (`/api/tags`) using the URL in account settings
/// - cloud → curated catalog (can be typed manually in the UI if not in the list)
pub async fn ai_models(
    State(st): State<AppState>,
    ctx: Ctx,
    Query(q): Query<AiModelsQuery>,
) -> impl IntoResponse {
    let provider = normalize_ai_provider(&q.provider);
    if provider == "ollama" {
        let url = st
            .settings
            .get(ctx.account_id)
            .await
            .map(|s| s.ai_judge_ollama_url)
            .unwrap_or_else(|_| "http://localhost:11434".into());
        let (models, ok) = fetch_ollama_models(&url).await;
        return Json(json!({
            "provider": provider,
            "source": "ollama",
            "ok": ok,
            "models": models,
        }))
        .into_response();
    }
    Json(json!({
        "provider": provider,
        "source": "catalog",
        "ok": true,
        "models": curated_cloud_models(&provider),
    }))
    .into_response()
}

/// Ask Ollama which models are installed (returns (model names, success))
async fn fetch_ollama_models(base_url: &str) -> (Vec<String>, bool) {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(_) => return (Vec::new(), false),
    };
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return (Vec::new(), false),
    };
    if !resp.status().is_success() {
        return (Vec::new(), false);
    }
    let body = match resp.json::<serde_json::Value>().await {
        Ok(v) => v,
        Err(_) => return (Vec::new(), false),
    };
    let mut models: Vec<String> = body
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    (models, true)
}

/// Curated recommended models per provider (field can still be typed manually if not in this list)
fn curated_cloud_models(provider: &str) -> Vec<&'static str> {
    match provider {
        // Anthropic Claude — current ids (Opus 4.7/4.8 = most expensive/capable, Haiku = fast/cheap)
        "anthropic" => vec![
            "claude-haiku-4-5",
            "claude-sonnet-4-6",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-opus-4-5",
        ],
        "openai" => vec![
            "gpt-4o-mini",
            "gpt-4o",
            "gpt-4.1-mini",
            "gpt-4.1",
            "o4-mini",
        ],
        "groq" => vec![
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "qwen-2.5-32b",
            "deepseek-r1-distill-llama-70b",
        ],
        "openrouter" => vec![
            "openai/gpt-4o-mini",
            "anthropic/claude-sonnet-4-6",
            "google/gemini-2.0-flash-001",
            "meta-llama/llama-3.3-70b-instruct",
            "deepseek/deepseek-chat",
        ],
        _ => Vec::new(),
    }
}

#[derive(Deserialize)]
pub struct AiCompareBody {
    pub symbol: String,
}

async fn ai_compare_leg(
    label: &str,
    provider: &str,
    st: &AppState,
    symbol: &str,
    judge_override: serde_json::Value,
) -> serde_json::Value {
    let started = Instant::now();
    match st
        .service
        .ai
        .analyze_with(symbol, Some(judge_override))
        .await
    {
        Ok(a) => json!({
            "label": label,
            "provider": provider,
            "ok": true,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "engine": &a.verdict.engine,
            "action": a.verdict.action.as_str(),
            "confidence": a.verdict.confidence,
            "reasoning": &a.verdict.reasoning,
            "thesis": &a.verdict.thesis,
            "next_step": &a.verdict.next_step,
            "synthetic": a.synthetic,
        }),
        Err(e) => json!({
            "label": label,
            "provider": provider,
            "ok": false,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "error": e.to_string(),
        }),
    }
}

pub async fn ai_compare(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<AiCompareBody>,
) -> impl IntoResponse {
    let symbol = b.symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return err(StatusCode::BAD_REQUEST, "Symbol is required").into_response();
    }
    let settings = match st.settings.get(ctx.account_id).await {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let selected_provider = normalize_ai_provider(&settings.ai_judge_provider);
    let local = ai_compare_leg(
        "local",
        "ollama",
        &st,
        &symbol,
        TradingService::local_judge_override(&settings),
    )
    .await;

    let provider_configured = if ai_provider_needs_key(&selected_provider) {
        st.secrets
            .exists(ctx.user_id, &ai_secret_name(&selected_provider))
            .await
            .unwrap_or(false)
    } else {
        true
    };
    let selected = if !provider_configured {
        json!({
            "label": "selected",
            "provider": selected_provider,
            "ok": false,
            "elapsed_ms": 0,
            "error": "API key not configured for this provider",
        })
    } else {
        let judge = st
            .service
            .judge_override_for_account(ctx.account_id, &symbol, &settings)
            .await
            .unwrap_or_else(|| json!({}));
        ai_compare_leg("selected", &selected_provider, &st, &symbol, judge).await
    };

    Json(json!({
        "symbol": symbol,
        "provider_configured": provider_configured,
        "local": local,
        "selected": selected,
    }))
    .into_response()
}

// ============================================================
//  WATCHLIST (per-account)
// ============================================================

pub async fn get_watch(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.watch_store.get_symbols(ctx.account_id).await {
        Ok(s) => Json(json!({ "symbols": s })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct WatchBody {
    pub symbols: Vec<String>,
}
pub async fn set_watch(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<WatchBody>,
) -> impl IntoResponse {
    let cleaned: Vec<String> = b
        .symbols
        .into_iter()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    if let Err(e) = st.watch_store.set_symbols(ctx.account_id, &cleaned).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    Json(json!({ "symbols": cleaned })).into_response()
}

// ============================================================
//  ANALYZE / DECISIONS / REPORT (per-account)
// ============================================================

#[derive(Deserialize)]
pub struct AnalyzeBody {
    pub symbol: String,
}
pub async fn analyze_now(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<AnalyzeBody>,
) -> impl IntoResponse {
    match st
        .service
        .run_once(ctx.account_id, &b.symbol.trim().to_uppercase())
        .await
    {
        Ok(rec) => Json(rec).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct LimitQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 {
    50
}

pub async fn recent_decisions(
    State(st): State<AppState>,
    ctx: Ctx,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    match st.repo.recent_decisions(ctx.account_id, q.limit).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn decisions_for_symbol(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(symbol): Path<String>,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    match st
        .repo
        .decisions_for_symbol(ctx.account_id, &symbol.to_uppercase(), q.limit)
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn decision_analysis(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match st.repo.decision_analysis(ctx.account_id, id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn report(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.repo.report_summary(ctx.account_id).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================
//  SYMBOLS (public market data)
// ============================================================

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}
fn default_search_limit() -> usize {
    12
}
pub async fn symbol_search(
    State(st): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    match st.market.search(&q.q, q.limit).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e).into_response(),
    }
}
pub async fn symbol_ticker(
    State(st): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    match st.market.ticker(&symbol).await {
        Ok(t) => Json(t).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, e).into_response(),
    }
}

// ============================================================
//  SETTINGS (per-account)
// ============================================================

pub async fn get_settings(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.settings.get(ctx.account_id).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}
pub async fn put_settings(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(s): Json<crate::domain::models::TradingSettings>,
) -> impl IntoResponse {
    // Prevent paper accounts from setting mode=live (avoids confusion / accidental real money trades)
    let mut s = s;
    if matches!(ctx.account_kind, AccountKind::Paper) && matches!(s.mode, TradingMode::Live) {
        s.mode = TradingMode::Paper;
    }
    if matches!(ctx.account_kind, AccountKind::Live) && matches!(s.mode, TradingMode::Paper) {
        // Live accounts can only choose live/signal-only
        s.mode = TradingMode::SignalOnly;
    }
    // Normalize broker to a known value (unknown values fall back to bitkub) — prevent garbage in DB
    s.broker = crate::domain::models::BrokerKind::parse(&s.broker)
        .as_str()
        .to_string();
    match st.settings.set(ctx.account_id, &s).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================
//  WALLET (paper, per-account)
// ============================================================

pub async fn get_wallet(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    let view = match st.service.wallet_view(ctx.account_id).await {
        Ok(v) => v,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    Json(view).into_response()
}

#[derive(Deserialize)]
pub struct ResetBody {
    #[serde(default = "default_starting")]
    pub starting_cash: f64,
}
fn default_starting() -> f64 {
    100_000.0
}
pub async fn reset_wallet(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<ResetBody>,
) -> impl IntoResponse {
    match st.wallet.reset(ctx.account_id, b.starting_cash).await {
        Ok(_) => Json(json!({ "ok": true, "starting_cash": b.starting_cash })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================
//  ACCOUNT BALANCE (live, per-user creds)
// ============================================================

pub async fn account_balance(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    if let Ok(settings) = st.settings.get(ctx.account_id).await {
        if matches!(ctx.account_kind, AccountKind::Live)
            || matches!(settings.mode, TradingMode::Live)
        {
            if let Err(e) = st
                .service
                .reconcile_live_account(ctx.account_id, &settings)
                .await
            {
                tracing::warn!("sync Bitkub live account before showing balance failed: {e}");
            }
        }
    }
    let broker = match st.service.broker_resolver.resolve(ctx.account_id).await {
        Ok(b) => b,
        Err(e) => return err(StatusCode::BAD_GATEWAY, e).into_response(),
    };
    match broker.balances().await {
        Ok(v) => Json(json!({
            "broker": broker.name(),
            "simulated": broker.is_simulated(),
            "balances": v,
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

// ============================================================
//  TRADES (per-account)
// ============================================================

pub async fn recent_trades(
    State(st): State<AppState>,
    ctx: Ctx,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    match st.trades.recent(ctx.account_id, q.limit).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn trades_for_symbol(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(symbol): Path<String>,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    match st
        .trades
        .for_symbol(ctx.account_id, &symbol.to_uppercase(), q.limit)
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn clear_trades(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.trades.clear_all(ctx.account_id).await {
        Ok(n) => {
            st.wallet.reset_session(ctx.account_id).await.ok();
            Json(json!({ "ok": true, "deleted": n })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct TradeBody {
    pub symbol: String,
    pub side: String,
    pub amount_quote: f64,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}
pub async fn manual_trade(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<TradeBody>,
) -> impl IntoResponse {
    let settings = st.settings.get(ctx.account_id).await.ok();
    // Mode follows account kind (paper account → paper, live account → live) unless overridden
    let default_mode = match ctx.account_kind {
        AccountKind::Paper => TradingMode::Paper,
        AccountKind::Live => TradingMode::Live,
    };
    let mode = match b.mode.as_deref() {
        Some("live") => TradingMode::Live,
        Some("paper") => TradingMode::Paper,
        Some("signal-only") => TradingMode::SignalOnly,
        _ => settings
            .map(|s| match s.mode {
                TradingMode::SignalOnly => TradingMode::SignalOnly,
                _ => default_mode,
            })
            .unwrap_or(default_mode),
    };
    if matches!(mode, TradingMode::SignalOnly) {
        return err(StatusCode::BAD_REQUEST, "Cannot trade in signal-only mode").into_response();
    }
    // Prevent paper accounts from sending live orders and vice versa
    if matches!(mode, TradingMode::Live) && matches!(ctx.account_kind, AccountKind::Paper) {
        return err(StatusCode::BAD_REQUEST, "Paper account cannot trade real money").into_response();
    }
    if matches!(mode, TradingMode::Paper) && matches!(ctx.account_kind, AccountKind::Live) {
        return err(StatusCode::BAD_REQUEST, "Live account cannot use paper wallet").into_response();
    }
    let side = Action::parse(&b.side);
    let quote = b.quote.unwrap_or_else(|| "THB".into());
    match st
        .service
        .execute(
            ctx.account_id,
            &b.symbol.to_uppercase(),
            &quote,
            side,
            b.amount_quote,
            mode,
            None,
        )
        .await
    {
        Ok(t) => Json(t).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

// ============================================================
//  STATS (per-account)
// ============================================================

pub async fn get_stats(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    let since = match st.wallet.session_start(ctx.account_id).await {
        Ok(t) => t,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    // Backfill realized P&L from trade history in DB before counting (pure recompute, does not touch broker) —
    // prevents Dashboard showing win/loss as 0 when positions have already been closed
    st.service.backfill_realized(ctx.account_id).await;
    match st.trades.paper_stats(ctx.account_id, since).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn reset_stats(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.wallet.reset_session(ctx.account_id).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ---- governor (capital/risk state) ----
pub async fn get_governor(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    Json(st.service.governor_state(ctx.account_id).await)
}

/// GET /api/alerts — recent events the user should know about (insufficient funds, order failures, plan cancellations, etc.)
pub async fn get_alerts(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    match st.alerts.recent(ctx.account_id, 100).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ---- kill-switch (pause/resume automatic trading for an account) ----
#[derive(Deserialize)]
pub struct PauseBody {
    pub paused: bool,
}
pub async fn set_pause(
    State(st): State<AppState>,
    ctx: Ctx,
    Json(b): Json<PauseBody>,
) -> impl IntoResponse {
    match st.settings.set_paused(ctx.account_id, b.paused).await {
        Ok(_) => {
            st.events.publish(&LiveEvent::Status {
                message: if b.paused {
                    "⏸️ Automatic trading paused (kill-switch)".into()
                } else {
                    "▶️ Automatic trading resumed".into()
                },
                healthy: true,
            });
            Json(json!({ "ok": true, "paused": b.paused })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================
//  TRADE PLANS (per-account)
// ============================================================

pub async fn get_plans(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    if let Ok(settings) = st.settings.get(ctx.account_id).await {
        if matches!(ctx.account_kind, AccountKind::Live)
            || matches!(settings.mode, TradingMode::Live)
        {
            if let Err(e) = st
                .service
                .reconcile_live_account(ctx.account_id, &settings)
                .await
            {
                tracing::warn!("sync Bitkub live account before showing plans failed: {e}");
            }
        }
    }
    match st.plans.active(ctx.account_id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================
//  OPEN ORDERS — pending limit orders (funds/coins locked at Bitkub)
// ============================================================

pub async fn get_open_orders(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    // Only live accounts have open orders at the broker; paper accounts do not
    if !matches!(ctx.account_kind, AccountKind::Live) {
        return Json(json!({ "open_orders": [] })).into_response();
    }
    // Only check symbols being tracked (watchlist union plans) since Bitkub requires a specific symbol
    let watch = st
        .watch_store
        .get_symbols(ctx.account_id)
        .await
        .unwrap_or_default();
    let plans = st.plans.active(ctx.account_id).await.unwrap_or_default();
    let mut symbols: Vec<String> = watch;
    for p in &plans {
        if !symbols.iter().any(|s| s == &p.symbol) {
            symbols.push(p.symbol.clone());
        }
    }
    let orders = st.service.live_open_orders(ctx.account_id, &symbols).await;
    Json(json!({ "open_orders": orders })).into_response()
}

// ============================================================
//  TARGETS — tracked targets + reason why not yet bought (Phase 4)
// ============================================================

fn target_base(symbol: String, last: f64) -> TargetStatus {
    TargetStatus {
        symbol,
        state: String::new(),
        reason: String::new(),
        last_price: last,
        entry_price: 0.0,
        target_price: 0.0,
        stop_price: 0.0,
        confidence: 0.0,
        action: String::new(),
        decision_id: None,
        updated_at: None,
    }
}

fn fmt_price(n: f64) -> String {
    if n >= 1.0 {
        format!("{n:.2}")
    } else if n > 0.0 {
        format!("{n:.6}")
    } else {
        "—".into()
    }
}

pub async fn get_targets(State(st): State<AppState>, ctx: Ctx) -> impl IntoResponse {
    // Live account: sync Bitkub first so coins held without a plan get a rescue plan and appear in targets
    if let Ok(settings) = st.settings.get(ctx.account_id).await {
        if matches!(ctx.account_kind, AccountKind::Live)
            || matches!(settings.mode, TradingMode::Live)
        {
            if let Err(e) = st
                .service
                .reconcile_live_account(ctx.account_id, &settings)
                .await
            {
                tracing::warn!("sync Bitkub before showing targets failed: {e}");
            }
        }
    }
    let min_conf = st
        .settings
        .get(ctx.account_id)
        .await
        .map(|s| s.min_confidence)
        .unwrap_or(0.65);
    let watch = st
        .watch_store
        .get_symbols(ctx.account_id)
        .await
        .unwrap_or_default();
    let plans = st.plans.active(ctx.account_id).await.unwrap_or_default();

    // union: watchlist + symbols with plans (e.g. from discovery)
    let mut symbols: Vec<String> = watch.clone();
    for p in &plans {
        if !symbols.iter().any(|s| s == &p.symbol) {
            symbols.push(p.symbol.clone());
        }
    }

    let mut out: Vec<TargetStatus> = Vec::with_capacity(symbols.len());
    for sym in symbols {
        let last = st.market.ticker(&sym).await.map(|t| t.last).unwrap_or(0.0);
        if let Some(p) = plans.iter().find(|p| p.symbol == sym) {
            let mut tgt = target_base(sym.clone(), last);
            tgt.entry_price = p.entry_price;
            tgt.target_price = p.target_price;
            tgt.stop_price = p.stop_price;
            tgt.confidence = p.confidence;
            tgt.action = p.action.as_str().into();
            tgt.decision_id = p.decision_id;
            tgt.updated_at = Some(p.updated_at);
            match p.state {
                PlanState::Open => {
                    tgt.state = "holding".into();
                    tgt.reason = if p.thesis.is_empty() {
                        "Holding — watching for exit at target/stop-loss".into()
                    } else {
                        format!("Holding — {}", p.thesis)
                    };
                }
                _ => {
                    tgt.state = "plan_pending".into();
                    tgt.reason = format!(
                        "⏳ Waiting for price to reach {} to enter (currently {})",
                        fmt_price(p.entry_price),
                        fmt_price(last)
                    );
                }
            }
            out.push(tgt);
            continue;
        }

        // No plan → check latest analysis result
        let recent = st
            .repo
            .decisions_for_symbol(ctx.account_id, &sym, 1)
            .await
            .unwrap_or_default();
        let mut tgt = target_base(sym.clone(), last);
        if let Some(d) = recent.first() {
            let conf = d.consensus_confidence;
            tgt.confidence = conf;
            tgt.action = d.final_action.as_str().into();
            tgt.decision_id = Some(d.id);
            tgt.updated_at = Some(d.created_at);
            if d.vetoed {
                tgt.state = "skipped".into();
                tgt.reason = "🚫 An analyst vetoed this — skipping".into();
            } else if d.final_action == Action::Buy && conf >= min_conf {
                tgt.state = "candidate".into();
                tgt.reason = format!(
                    "⭐ Buy candidate (confidence {:.0}%) — waiting for entry / enable auto-trade to act",
                    conf * 100.0
                );
            } else if conf < min_conf {
                tgt.state = "waiting".into();
                tgt.reason = format!(
                    "🔸 Below threshold — AI says {} with {:.0}% confidence (threshold {:.0}%)",
                    d.final_action.as_str(),
                    conf * 100.0,
                    min_conf * 100.0
                );
            } else {
                tgt.state = "waiting".into();
                tgt.reason = format!(
                    "🔸 AI reviewed and not yet a buy opportunity ({} {:.0}% confidence) — watching",
                    d.final_action.as_str(),
                    conf * 100.0
                );
            }
        } else {
            tgt.state = "queued".into();
            tgt.reason = "🕓 Not yet analyzed — waiting for next cycle (or press \"Analyze Now\")".into();
        }
        out.push(tgt);
    }

    Json(out)
}

// ============================================================
//  MARKET DISCOVERY (public)
// ============================================================

#[derive(Deserialize)]
pub struct ScanQuery {
    #[serde(default = "default_top_n")]
    pub top_n: usize,
}
fn default_top_n() -> usize {
    8
}
pub async fn market_scan(
    State(st): State<AppState>,
    Query(q): Query<ScanQuery>,
) -> impl IntoResponse {
    match st.scanner.scan(q.top_n).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e).into_response(),
    }
}
