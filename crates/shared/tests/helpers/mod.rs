pub mod asset_test_helpers;
pub mod world_test_helpers;

// Tests of JSON-only results read SuperJSON's payload; richer values are checked in the JS runtime.
#[allow(dead_code)]
pub fn remote_json_result<T: serde::de::DeserializeOwned>(encoded: &str) -> anyhow::Result<T> {
    let result: serde_json::Value = serde_json::from_str(encoded)?;
    Ok(T::deserialize(&result["json"])?)
}
