use crate::api_error::ApiError;
use axum::http::{HeaderMap, StatusCode};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

// Browser access is either the creator's verified profile or a persistent game invitation.
// Neither credential grants access to deployment-only tools.
#[derive(Clone)]
pub enum BrowserAccess {
    Creator { profile_id: String },
    Guest { invite_token: String },
}

// Health checks and model tools require this deployment's credential, not a website login token.
pub fn require_server_token(headers: &HeaderMap, token: &str) -> Result<(), ApiError> {
    let actual = headers.get("authorization").and_then(|value| value.to_str().ok()).unwrap_or("");
    let expected = format!("Bearer {token}");
    if !bool::from(Sha256::digest(actual.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes()))) {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "Invalid editor server credential."));
    }
    Ok(())
}
