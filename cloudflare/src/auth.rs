//! Constant-time HMAC-SHA256 session authentication and revocation checks.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const SESSION_PURPOSE_LABEL: &str = "cf-kv-worker/session/v1";
pub const SESSION_COOKIE_NAME: &str = "__Host-cf_kv_session";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionClaims {
    pub login: String,
    pub uid: u64,
    pub iat: u64,
    pub exp: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("Missing or empty session secret")]
    EmptySecret,
    #[error("Malformed session token")]
    MalformedToken,
    #[error("Invalid session signature")]
    InvalidSignature,
    #[error("Session expired at {0}")]
    Expired(u64),
    #[error("Session revoked by SESSION_NOT_BEFORE ({0})")]
    Revoked(u64),
}

fn compute_hmac_tag(secret: &[u8], payload_b64: &str) -> Result<Vec<u8>, AuthError> {
    if secret.is_empty() {
        return Err(AuthError::EmptySecret);
    }
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| AuthError::EmptySecret)?;
    mac.update(SESSION_PURPOSE_LABEL.as_bytes());
    mac.update(b":");
    mac.update(payload_b64.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

pub fn sign_session_token(claims: &SessionClaims, secret: &[u8]) -> Result<String, AuthError> {
    let json = serde_json::to_vec(claims).map_err(|_| AuthError::MalformedToken)?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
    let tag = compute_hmac_tag(secret, &payload_b64)?;
    let sig_b64 = URL_SAFE_NO_PAD.encode(&tag);
    Ok(format!("{}.{}", payload_b64, sig_b64))
}

pub fn verify_session_token(
    token: &str,
    secret: &[u8],
    now: u64,
    not_before: Option<u64>,
) -> Result<SessionClaims, AuthError> {
    let (payload_b64, sig_b64) = token.split_once('.').ok_or(AuthError::MalformedToken)?;
    if payload_b64.is_empty() || sig_b64.is_empty() || sig_b64.contains('.') {
        return Err(AuthError::MalformedToken);
    }

    let provided_tag = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| AuthError::MalformedToken)?;
    let expected_tag = compute_hmac_tag(secret, payload_b64)?;

    if provided_tag.ct_eq(&expected_tag).unwrap_u8() != 1 {
        return Err(AuthError::InvalidSignature);
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| AuthError::MalformedToken)?;
    let claims: SessionClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| AuthError::MalformedToken)?;

    if now >= claims.exp {
        return Err(AuthError::Expired(claims.exp));
    }

    if let Some(cutoff) = not_before {
        if claims.iat < cutoff {
            return Err(AuthError::Revoked(cutoff));
        }
    }

    Ok(claims)
}

pub fn extract_cookie_value(cookie_header: &str, cookie_name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some((name, val)) = trimmed.split_once('=') {
            if name.trim() == cookie_name {
                return Some(val.trim().to_string());
            }
        }
    }
    None
}
