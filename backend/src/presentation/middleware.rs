//! Auth middleware + Ctx extractor
//!
//! - require_auth: validates `Authorization: Bearer <jwt>` → loads user → selects account from
//!   header `X-Account-Id` (checks ownership) or default = user's paper account
//!   → injects `Ctx` into request extensions
//! - Ctx: extractor that retrieves Ctx from extensions (used in every handler that requires login)

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{request::Parts, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use super::state::AppState;
use crate::domain::models::{AccountKind, Ctx};

fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg }))).into_response()
}

/// middleware: enforces login + selects account → injects Ctx
pub async fn require_auth(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    // 1) bearer token
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.strip_prefix("Bearer ")
                .or_else(|| s.strip_prefix("bearer "))
        })
        .map(|s| s.to_string());
    let token = match token {
        Some(t) => t,
        None => return unauthorized("authentication required (no token)"),
    };
    let claims = match st.auth.verify(&token) {
        Ok(c) => c,
        Err(_) => return unauthorized("token is invalid or expired"),
    };
    let user_id = claims.sub;

    // 2) select account: X-Account-Id (check ownership) or default paper
    let requested = req
        .headers()
        .get("x-account-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());

    let accounts = st.accounts.for_user(user_id).await.unwrap_or_default();
    if accounts.is_empty() {
        return unauthorized("user account is incomplete (no accounts found)");
    }
    let chosen = match requested {
        Some(aid) => match accounts.iter().find(|a| a.id == aid) {
            Some(a) => a.clone(),
            None => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "this account does not belong to you" })),
                )
                    .into_response()
            }
        },
        None => accounts
            .iter()
            .find(|a| matches!(a.kind, AccountKind::Paper))
            .cloned()
            .unwrap_or_else(|| accounts[0].clone()),
    };

    let ctx = Ctx {
        user_id,
        account_id: chosen.id,
        account_kind: chosen.kind,
    };
    req.extensions_mut().insert(ctx);
    next.run(req).await
}

/// extractor used in handlers — retrieves the Ctx injected by middleware
impl<S> FromRequestParts<S> for Ctx
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Ctx>()
            .copied()
            .ok_or_else(|| unauthorized("authentication required"))
    }
}
