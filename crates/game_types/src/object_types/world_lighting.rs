use serde::{Deserialize, Serialize};

use crate::object_types::shared::{Color3, Orientation3D};
use crate::data_types::DataId;

// TODO: AI-PICKED-VALUE: This matches the old hardcoded key light direction so adding WorldLightingInfo does not change existing scenes unexpectedly.
const DEFAULT_WORLD_LIGHTING_SUN_ORIENTATION: Orientation3D = Orientation3D {
    theta: 1.0303768,
    phi: -1.041327,
};
// TODO: AI-PICKED-VALUE: A warm white sun. Like every other color here it is sRGB, the color a color picker shows.
const DEFAULT_WORLD_LIGHTING_SUN_COLOR: Color3 = Color3 { r: 1.0, g: 0.95, b: 0.86 };
// TODO: AI-PICKED-VALUE: A clear sunny day. Surfaces facing the sun come out a little brighter than their picked
// color, and the renderer's tone curve rolls the brightest highlights off instead of clipping them.
const DEFAULT_WORLD_LIGHTING_SUN_INTENSITY: f32 = 3.8;
// TODO: AI-PICKED-VALUE: Every shadow edge blurs over about a tenth of a unit: soft, while thin objects like poles still
// cast solid shadows.
const DEFAULT_WORLD_LIGHTING_SUN_SHADOW_SOFTNESS: f32 = 0.1;
// TODO: AI-PICKED-VALUE: Blue sky light from above and warm ground bounce from below, strong enough that shade stays
// soft and colored instead of going near black.
const DEFAULT_WORLD_LIGHTING_AMBIENT_TOP_COLOR: Color3 = Color3 { r: 0.7, g: 0.8, b: 1.0 };
const DEFAULT_WORLD_LIGHTING_AMBIENT_BOTTOM_COLOR: Color3 = Color3 { r: 0.72, g: 0.64, b: 0.52 };
// TODO: AI-PICKED-VALUE: Shade gets roughly a third of the light a sunlit surface gets, about what a clear day gives.
const DEFAULT_WORLD_LIGHTING_AMBIENT_INTENSITY: f32 = 0.6;
// TODO: AI-PICKED-VALUE: Soft ambient occlusion that grounds objects and shades creases. The darkest crease keeps 30% of
// its sky light, the darkening reaches one world unit out, and it takes half as much of the sunlight in creases.
const DEFAULT_WORLD_LIGHTING_AMBIENT_OCCLUSION_INTENSITY: f32 = 0.7;
const DEFAULT_WORLD_LIGHTING_AMBIENT_OCCLUSION_RADIUS: f32 = 1.0;
const DEFAULT_WORLD_LIGHTING_AMBIENT_OCCLUSION_SUNLIGHT_STRENGTH: f32 = 0.5;
// TODO: AI-PICKED-VALUE: These three stops make a clear blue sky overhead fading to a pale band at the horizon, with a
// hazy blue below it so looking down past the edge of the world still reads as open sky instead of a dull grey.
const DEFAULT_WORLD_LIGHTING_SKY_TOP_COLOR: Color3 = Color3 { r: 0.24, g: 0.52, b: 0.94 };
const DEFAULT_WORLD_LIGHTING_SKY_HORIZON_COLOR: Color3 = Color3 { r: 0.72, g: 0.86, b: 0.98 };
const DEFAULT_WORLD_LIGHTING_SKY_BOTTOM_COLOR: Color3 = Color3 { r: 0.52, g: 0.64, b: 0.8 };
// TODO: AI-PICKED-VALUE: Skyboxes should be fully visible by default, while texture alpha can still reveal the gradient.
const DEFAULT_WORLD_LIGHTING_SKYBOX_TEXTURE_OPACITY: f32 = 1.0;
// TODO: AI-PICKED-VALUE: Shiny and metal surfaces reflect the sky at full strength, the way they would in real life.
const DEFAULT_WORLD_LIGHTING_SKY_REFLECTION_STRENGTH: f32 = 1.0;
// TODO: AI-PICKED-VALUE: Dull colors move up to a fifth further from gray and vivid ones barely at all, which keeps worlds
// lively without strong colors going neon.
const DEFAULT_WORLD_LIGHTING_VIBRANCE: f32 = 0.2;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WorldLightingInfo {
    pub sunOrientation: Orientation3D,
    pub sunColor: Color3,
    pub sunIntensity: f32,
    pub sunShadowSoftness: f32,
    pub ambientTopColor: Color3,
    pub ambientBottomColor: Color3,
    pub ambientIntensity: f32,
    pub ambientOcclusionIntensity: f32,
    pub ambientOcclusionRadius: f32,
    pub ambientOcclusionSunlightStrength: f32,
    pub skyTopColor: Color3,
    pub skyHorizonColor: Color3,
    pub skyBottomColor: Color3,
    pub skyboxTextureDataId: Option<DataId>,
    pub skyboxTextureOpacity: f32,
    pub skyReflectionStrength: f32,
    pub vibrance: f32,
}

impl Default for WorldLightingInfo {
    fn default() -> Self {
        Self {
            sunOrientation: DEFAULT_WORLD_LIGHTING_SUN_ORIENTATION,
            sunColor: DEFAULT_WORLD_LIGHTING_SUN_COLOR,
            sunIntensity: DEFAULT_WORLD_LIGHTING_SUN_INTENSITY,
            sunShadowSoftness: DEFAULT_WORLD_LIGHTING_SUN_SHADOW_SOFTNESS,
            ambientTopColor: DEFAULT_WORLD_LIGHTING_AMBIENT_TOP_COLOR,
            ambientBottomColor: DEFAULT_WORLD_LIGHTING_AMBIENT_BOTTOM_COLOR,
            ambientIntensity: DEFAULT_WORLD_LIGHTING_AMBIENT_INTENSITY,
            ambientOcclusionIntensity: DEFAULT_WORLD_LIGHTING_AMBIENT_OCCLUSION_INTENSITY,
            ambientOcclusionRadius: DEFAULT_WORLD_LIGHTING_AMBIENT_OCCLUSION_RADIUS,
            ambientOcclusionSunlightStrength: DEFAULT_WORLD_LIGHTING_AMBIENT_OCCLUSION_SUNLIGHT_STRENGTH,
            skyTopColor: DEFAULT_WORLD_LIGHTING_SKY_TOP_COLOR,
            skyHorizonColor: DEFAULT_WORLD_LIGHTING_SKY_HORIZON_COLOR,
            skyBottomColor: DEFAULT_WORLD_LIGHTING_SKY_BOTTOM_COLOR,
            skyboxTextureDataId: None,
            skyboxTextureOpacity: DEFAULT_WORLD_LIGHTING_SKYBOX_TEXTURE_OPACITY,
            skyReflectionStrength: DEFAULT_WORLD_LIGHTING_SKY_REFLECTION_STRENGTH,
            vibrance: DEFAULT_WORLD_LIGHTING_VIBRANCE,
        }
    }
}
