use serde::de::DeserializeOwned;
use std::io::Cursor;

// Asset downloads retain their existing versioned binary format after the publishing API is removed.
// Check the version before decoding the payload so stale schemas fail with a useful error.
pub fn parse_uploaded_data_from_bytes<T: DeserializeOwned>(bytes: &[u8], expected_version: u64) -> anyhow::Result<T> {
    let mut cursor = Cursor::new(bytes);
    let version = bincode::deserialize_from::<_, u64>(&mut cursor)?;
    if version != expected_version {
        return Err(anyhow::anyhow!("Unsupported data version: expected {}, got {}", expected_version, version));
    }
    Ok(bincode::deserialize_from(&mut cursor)?)
}
