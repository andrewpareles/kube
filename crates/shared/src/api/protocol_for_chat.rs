mod client_editor_tool_handlers;
mod tool_registry;

pub use client_editor_tool_handlers::*;
use serde::{Deserialize, Serialize};
pub use tool_registry::*;

// The hosted runtime executes the original registry; AI SDK owns chat orchestration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolResultFinal {
    pub is_error: bool,
    pub text: String,
    pub images: Vec<ToolImage>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolImage {
    pub media_type: String,
    pub base64: String,
}
