use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::AppState;

/// Guards the API. Local requests (the laptop dashboard) are always allowed;
/// anything coming from the LAN needs the token, either as `?k=` or the
/// `x-sync-key` header.
pub async fn require_token(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !path.starts_with("/api/") || path == "/api/info" {
        return next.run(req).await;
    }
    let token = state.token();
    if token.is_empty() || peer.ip().is_loopback() {
        return next.run(req).await;
    }

    let header_ok = req
        .headers()
        .get("x-sync-key")
        .and_then(|v| v.to_str().ok())
        .map(|v| constant_eq(v, &token))
        .unwrap_or(false);
    let query_ok = req
        .uri()
        .query()
        .map(|q| q.split('&').any(|pair| {
            let mut parts = pair.splitn(2, '=');
            matches!(parts.next(), Some("k"))
                && parts.next().map(|v| constant_eq(v, &token)).unwrap_or(false)
        }))
        .unwrap_or(false);

    if header_ok || query_ok {
        next.run(req).await
    } else {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "hint": "Open the link with ?k=<token> or send the x-sync-key header"
            })),
        )
            .into_response()
    }
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
