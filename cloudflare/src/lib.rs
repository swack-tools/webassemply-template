//! Pure-Rust Cloudflare Worker Template using Cloudflare Workers KV (`STATE_KV`) as State Storage.

pub mod auth;
pub mod reconciler;
pub mod state;
pub mod ui;

use crate::auth::{extract_cookie_value, verify_session_token, SESSION_COOKIE_NAME};
use crate::reconciler::run_reconciliation_tick;
use crate::state::{
    load_state, save_state_if_changed, StateItem, KV_STATE_KEY, SNAPSHOT_SCHEMA, WRITER_SLOT_EDGE,
};
use serde::{Deserialize, Serialize};
use worker::*;

pub const MAX_AUTH_BODY_BYTES: usize = 16 * 1024; // 16 KiB
pub const MAX_API_BODY_BYTES: usize = 256 * 1024; // 256 KiB

pub fn current_time_epoch() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (worker::Date::now().as_millis() / 1000) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Decode percent escapes, resolve dot segments, collapse slashes, and lowercase.
pub fn normalize_path_for_blocking(pathname: &str) -> Option<String> {
    let mut decoded = pathname.to_string();
    for _ in 0..3 {
        match percent_encoding::percent_decode_str(&decoded).decode_utf8() {
            Ok(next) => {
                let next_str = next.into_owned();
                if next_str == decoded {
                    break;
                }
                decoded = next_str;
            }
            Err(_) => return None,
        }
    }

    let mut unified = decoded.replace('\\', "/");
    while unified.contains("//") {
        unified = unified.replace("//", "/");
    }

    let url_str = if unified.starts_with('/') {
        format!("https://normalize.invalid{}", unified)
    } else {
        format!("https://normalize.invalid/{}", unified)
    };

    let parsed = url::Url::parse(&url_str).ok()?;
    let mut path = parsed.path().to_lowercase();
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    Some(path)
}

/// Check if a path targets blocked internal control plane routes (`/api/control*`).
pub fn is_control_path(pathname: &str) -> bool {
    let normalized = match normalize_path_for_blocking(pathname) {
        Some(p) => p,
        None => return true,
    };

    if normalized == "/api/control" || normalized.starts_with("/api/control/") {
        return true;
    }
    if normalized.starts_with("/api/control") {
        let remainder = &normalized["/api/control".len()..];
        if remainder.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
            return true;
        }
    }
    false
}

/// Check if a path is an authentication route subject to the 16 KiB body cap.
pub fn is_auth_path(pathname: &str) -> bool {
    let normalized = match normalize_path_for_blocking(pathname) {
        Some(p) => p,
        None => return true,
    };
    normalized == "/auth"
        || normalized.starts_with("/auth/")
        || normalized == "/api/auth"
        || normalized.starts_with("/api/auth/")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpsertItemRequest {
    pub key: String,
    pub value: String,
}

pub fn json_response<T: Serialize>(status: u16, data: &T) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("Cache-Control", "no-store, no-cache, must-revalidate")?;
    let body = serde_json::to_string(data)?;
    Ok(Response::ok(body)?
        .with_status(status)
        .with_headers(headers))
}

pub fn error_response(status: u16, error: &str, message: &str) -> Result<Response> {
    let body = serde_json::json!({
        "error": error,
        "message": message,
    });
    json_response(status, &body)
}

fn require_authenticated_session(
    req: &Request,
    env: &Env,
    now: u64,
) -> std::result::Result<String, Response> {
    let secret = env
        .secret("SESSION_SECRET")
        .or_else(|_| env.var("SESSION_SECRET"))
        .map(|s| s.to_string())
        .unwrap_or_default();
    if secret.trim().is_empty() {
        return Err(
            error_response(503, "auth_unconfigured", "SESSION_SECRET is not configured").unwrap(),
        );
    }

    let not_before = env
        .var("SESSION_NOT_BEFORE")
        .ok()
        .and_then(|v| v.to_string().trim().parse::<u64>().ok());

    let cookie_header = req
        .headers()
        .get("Cookie")
        .ok()
        .flatten()
        .unwrap_or_default();
    let token = match extract_cookie_value(&cookie_header, SESSION_COOKIE_NAME) {
        Some(t) => t,
        None => {
            return Err(error_response(401, "unauthorized", "Missing session cookie").unwrap());
        }
    };

    match verify_session_token(&token, secret.as_bytes(), now, not_before) {
        Ok(claims) => Ok(claims.login),
        Err(err) => Err(error_response(401, "unauthorized", &err.to_string()).unwrap()),
    }
}

#[event(fetch)]
pub async fn fetch(mut req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path = url.path();

    if is_control_path(path) {
        return error_response(404, "not_found", "Endpoint not found");
    }

    let normalized = match normalize_path_for_blocking(path) {
        Some(p) => p,
        None => return error_response(400, "bad_request", "Malformed request path"),
    };

    let now = current_time_epoch();

    match (req.method(), normalized.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            let headers = Headers::new();
            headers.set("Content-Type", "text/html; charset=utf-8")?;
            headers.set("Cache-Control", "no-store")?;
            Ok(Response::ok(ui::DASHBOARD_HTML)?.with_headers(headers))
        }
        (Method::Get, "/api/health") => {
            let kv = env.kv("STATE_KV")?;
            let (bundle, cache_source) = load_state(&kv, now)
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            let payload = serde_json::json!({
                "status": "ok",
                "schema": SNAPSHOT_SCHEMA,
                "kv_key": KV_STATE_KEY,
                "cache_source": cache_source,
                "state_version": bundle.state_version,
                "state_fingerprint": bundle.state_fingerprint,
                "items_count": bundle.state.items.len(),
            });
            json_response(200, &payload)
        }
        (Method::Get, "/api/state") => {
            let kv = env.kv("STATE_KV")?;
            let (bundle, cache_source) = load_state(&kv, now)
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            let payload = serde_json::json!({
                "cache_source": cache_source,
                "bundle": bundle,
            });
            json_response(200, &payload)
        }
        (Method::Post, "/api/items") => {
            let actor = match require_authenticated_session(&req, &env, now) {
                Ok(login) => login,
                Err(resp) => return Ok(resp),
            };
            let body_bytes = req.bytes().await?;
            let max_bytes = if is_auth_path(path) {
                MAX_AUTH_BODY_BYTES
            } else {
                MAX_API_BODY_BYTES
            };
            if body_bytes.len() > max_bytes {
                return error_response(413, "payload_too_large", "Request body exceeds size cap");
            }
            let input: UpsertItemRequest = match serde_json::from_slice(&body_bytes) {
                Ok(v) => v,
                Err(e) => return error_response(400, "invalid_json", &e.to_string()),
            };
            if input.key.trim().is_empty() {
                return error_response(400, "invalid_key", "Item key must not be empty");
            }

            let kv = env.kv("STATE_KV")?;
            let (mut bundle, _) = load_state(&kv, now)
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            bundle.state.items.insert(
                input.key.clone(),
                StateItem {
                    key: input.key.clone(),
                    value: input.value,
                    updated_at: now,
                    updated_by: actor,
                },
            );
            bundle.state.updated_at = now;
            let outcome = save_state_if_changed(&kv, &mut bundle, WRITER_SLOT_EDGE, now)
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            let payload = serde_json::json!({
                "write_outcome": outcome,
                "state_version": bundle.state_version,
                "state_fingerprint": bundle.state_fingerprint,
            });
            json_response(200, &payload)
        }
        (Method::Post, "/api/reconcile/tick") => {
            let _actor = match require_authenticated_session(&req, &env, now) {
                Ok(login) => login,
                Err(resp) => return Ok(resp),
            };
            let kv = env.kv("STATE_KV")?;
            let report = run_reconciliation_tick(&kv, "manual-edge-tick", now, false)
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            json_response(200, &report)
        }
        _ => error_response(404, "not_found", "Endpoint not found"),
    }
}

#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Ok(kv) = env.kv("STATE_KV") {
        let now = current_time_epoch();
        let _ = run_reconciliation_tick(&kv, "scheduled-cron", now, false).await;
    }
}
