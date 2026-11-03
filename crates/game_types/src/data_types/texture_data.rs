use serde::{Deserialize, Serialize};

// always update version number when changing this
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextureData {
    pub ktx2Data: Vec<u8>,
    // redundant internal info for manipulating without loading data
    pub width: u32,
    pub height: u32,
    pub mipLevelCount: u32,
    pub isSrgb: bool, // true if you see it (has colors), false if it's just data like normal maps which should not be postprocessed
    pub hasAlpha: bool,
}
