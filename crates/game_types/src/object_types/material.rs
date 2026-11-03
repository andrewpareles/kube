use crate::data_types::DataId;
use serde::{Deserialize, Serialize};

// TODO:
// - occlusion (roblox does not support it)
// - emissive (meshy does not support it. but RB does - it's a 1d field w/ rgb + opacity factors - EmmisiveTint (3D) + EmmisiveStrength)
//   for emmisive i was thinking: pub emissive: Option<(DataId, [f32; 3])>, // [e] texture for glow + [r,g,b] opacity factor

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum AlphaMode {
    Opaque,               // ignore albedo's alpha
    Mask { cutoff: f32 }, // use 100% opaque if albedo's alpha < val, and 100% invisible if not
    Blend,                // multiply albedo alpha by object opacity and composite over the background
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaterialInfo {
    // albedo: [r,g,b,a] texture per pixel
    // metallic: [m] texture per pixel
    // roughness: [r] texture per pixel
    // normal: [x,y,z] texture per pixel
    pub albedoTextureDataId: Option<DataId>,
    pub metallicTextureDataId: Option<DataId>,
    pub roughnessTextureDataId: Option<DataId>,
    pub normalTextureDataId: Option<DataId>,
    pub alphaMode: AlphaMode,
    pub isDoubleSided: bool,
}
