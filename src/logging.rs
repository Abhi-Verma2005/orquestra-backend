use std::time::Instant;

use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

use crate::{auth::verify_token, AppState};

fn status_text(status: StatusCode) -> String {
    let reason = status.canonical_reason().unwrap_or("Unknown");
    format!("{} {}", status.as_u16(), reason)
}

fn extract_query_token(req: &Request<Body>) -> Option<String> {
    let query = req.uri().query()?;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            let decoded = urlencoding::decode(value).ok()?;
            return Some(decoded.into_owned());
        }
    }
    None
}

fn extract_user_label(req: &Request<Body>, jwt_secret: &str) -> String {
    let header_token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(ToOwned::to_owned);

    let token = header_token.or_else(|| extract_query_token(req));

    let Some(token) = token else {
        return "anonymous".to_string();
    };

    let Ok(claims) = verify_token(&token, jwt_secret) else {
        return "anonymous".to_string();
    };

    let Ok(user_id) = Uuid::parse_str(&claims.sub) else {
        return "anonymous".to_string();
    };

    format!("user:{user_id}")
}

pub async fn request_logging_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let user = extract_user_label(&req, &state.config.jwt_secret);
    let started = Instant::now();

    let response = next.run(req).await;
    let elapsed_ms = started.elapsed().as_millis();
    let status = response.status();
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let line = format!(
        "{timestamp} | {method} {path} | {} | {elapsed_ms}ms | {user}",
        status_text(status)
    );

    if status.is_server_error() {
        tracing::error!("{line}");
    } else if status.is_client_error() {
        tracing::warn!("{line}");
    } else {
        tracing::info!("{line}");
    }

    response
}
