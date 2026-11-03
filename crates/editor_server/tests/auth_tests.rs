#[path = "../src/api_error.rs"]
mod api_error;
#[path = "../src/auth.rs"]
mod auth;

use auth::require_server_token;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;

// These credentials use the same HMAC as metadata readiness checks and Node tool calls.
#[test]
fn deployment_credentials_cannot_cross_editors_or_be_replaced_by_browser_tokens() {
    let secret = b"test-only-editor-server-secret";
    let tokens = ["current-editor", "old-editor"].map(|request_id| {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(request_id.as_bytes());
        mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    });
    let [current, old] = tokens;
    let mut headers = HeaderMap::new();
    for authorization in [
        String::new(),
        format!("Bearer {old}"),
        "Bearer website-login-token".into(),
        "Invite game-invitation".into(),
        format!("Invite {current}"),
        current.clone(),
        format!("Bearer {current}extra"),
    ] {
        headers.insert("authorization", HeaderValue::from_str(&authorization).unwrap());
        assert_eq!(require_server_token(&headers, &current).unwrap_err().status, StatusCode::UNAUTHORIZED);
    }
    headers.insert("authorization", HeaderValue::from_str(&format!("Bearer {current}")).unwrap());
    assert!(require_server_token(&headers, &current).is_ok());
    headers.clear();
    assert_eq!(require_server_token(&headers, &current).unwrap_err().status, StatusCode::UNAUTHORIZED);
}
