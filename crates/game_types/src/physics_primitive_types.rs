//! Shape dimensions shared by storage, physics, and rendering.
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BallPrimitiveSettings {
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BrickPrimitiveSettings {
    pub size: Vec3,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CylinderPrimitiveSettings {
    pub radius: f32,
    pub length: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PrimitiveSettings {
    Ball(BallPrimitiveSettings),
    Brick(BrickPrimitiveSettings),
    Cylinder(CylinderPrimitiveSettings),
}
impl PrimitiveSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        let is_valid = match self {
            Self::Ball(shape) => shape.radius.is_finite() && shape.radius >= 0.0,
            Self::Brick(shape) => shape.size.is_finite() && shape.size.min_element() >= 0.0,
            Self::Cylinder(shape) => shape.radius.is_finite() && shape.radius >= 0.0 && shape.length.is_finite() && shape.length >= 0.0,
        };
        anyhow::ensure!(is_valid, "Invalid shape dimensions.");
        Ok(())
    }
}
