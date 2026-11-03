use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use shared::postgrest::PostgrestError;

// Public errors have an explicit status; unexpected failures are logged without exposing database details.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}
impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}
impl std::error::Error for ApiError {}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}
pub fn internal_error(error: impl std::fmt::Debug) -> ApiError {
    log::error!("Editor request failed: {error:?}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "The editor request failed.")
}
// A database function's error meant for the user keeps its status and message.
impl From<PostgrestError> for ApiError {
    fn from(error: PostgrestError) -> Self {
        match error.user_status() {
            Some(status) => ApiError::new(status, error.message),
            None => internal_error(error),
        }
    }
}
impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        internal_error(error)
    }
}
