//! WebSocket — stream LiveEvent to UI (GET /ws?token=<jwt>&account_id=<id>[&fmt=bin])
//!
//! Token must be attached (browsers cannot send headers in WS → use query param) and account must be selected.
//! Events are filtered to the specified account only (events from other users will not leak across).
//!
//! ?fmt=bin  → send binary QPACK (40-60% smaller than JSON)
//! ?fmt=json → force JSON (debug)
//! (default)  → JSON text

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use super::state::AppState;
use crate::infrastructure::qpack;

#[derive(Deserialize)]
pub struct WsParams {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub account_id: Option<i64>,
    /// "bin" = QPACK binary, "json" = force JSON text (debug)
    #[serde(default)]
    pub fmt: String,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(st): State<AppState>,
    Query(p): Query<WsParams>,
) -> impl IntoResponse {
    // verify token + select authorized account
    let claims = st.auth.verify(&p.token);
    let allowed: Option<i64> = match claims {
        Ok(c) => {
            let accounts = st.accounts.for_user(c.sub).await.unwrap_or_default();
            match p.account_id {
                Some(aid) if accounts.iter().any(|a| a.id == aid) => Some(aid),
                Some(_) => return axum::http::StatusCode::FORBIDDEN.into_response(),
                None => accounts.first().map(|a| a.id),
            }
        }
        Err(_) => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
    };
    let allowed = match allowed {
        Some(a) => a,
        None => return axum::http::StatusCode::FORBIDDEN.into_response(),
    };
    let binary = p.fmt == "bin" && p.fmt != "json";
    ws.on_upgrade(move |socket| handle(socket, st, allowed, binary))
}

async fn handle(mut socket: WebSocket, st: AppState, account_id: i64, binary: bool) {
    let mut rx = st.events.subscribe();

    // hello handshake
    let hello = if binary {
        let v = serde_json::json!({ "type": "status", "message": "connected", "healthy": true });
        Message::Binary(qpack::to_vec(&v).into())
    } else {
        let txt = serde_json::json!({ "type": "status", "message": "connected", "healthy": true });
        Message::Text(txt.to_string().into())
    };
    if socket.send(hello).await.is_err() {
        return;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                // filter: account-bound events must match this account only (None = broadcast to all)
                if let Some(ev_acct) = event.account_id() {
                    if ev_acct != account_id {
                        continue;
                    }
                }

                let sent = if binary {
                    // QPACK path: serialize → JSON Value → QPACK bytes
                    match serde_json::to_value(&event) {
                        Ok(v) => {
                            let bytes = qpack::to_vec(&v);
                            socket.send(Message::Binary(bytes.into())).await
                        }
                        Err(_) => continue,
                    }
                } else {
                    // JSON text path
                    match serde_json::to_string(&event) {
                        Ok(txt) => socket.send(Message::Text(txt.into())).await,
                        Err(_) => continue,
                    }
                };

                if sent.is_err() {
                    break;
                }
            }
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!("ws client lagged, skipped {skipped} events");
                continue;
            }
            Err(RecvError::Closed) => break,
        }
    }
}
