use serde::{Deserialize, Serialize};

// always update version number when changing this
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoundData {
    pub oggVorbisData: Vec<u8>,
}
