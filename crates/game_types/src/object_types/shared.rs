use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};

// ------------------------------------------------------- ObjectId -------------------------------------------------------
pub type ObjectId = usize;

// The objects a client makes for itself count down from the top of the id range, so they never take an id the server hands out.
pub const TOP_CLIENT_OBJECT_ID: ObjectId = u32::MAX as ObjectId;
pub const CLIENT_OBJECT_ID_FLOOR: ObjectId = 1 << 31;
pub fn is_client_object_id(object_id: ObjectId) -> bool {
    object_id >= CLIENT_OBJECT_ID_FLOOR
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ObjectIdPointer(pub ObjectId);

// ObjectId -> ObjectIdPointer
impl From<&ObjectIdPointer> for ObjectId {
    fn from(value: &ObjectIdPointer) -> Self {
        let ObjectIdPointer(id) = value;
        *id
    }
}
impl From<ObjectIdPointer> for ObjectId {
    fn from(value: ObjectIdPointer) -> Self {
        let ObjectIdPointer(id) = value;
        id
    }
}

// ObjectIdPointer -> ObjectId
impl From<ObjectId> for ObjectIdPointer {
    fn from(value: ObjectId) -> Self {
        ObjectIdPointer(value)
    }
}

// ------------------------------------------------------- Positions + Velocities -------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location3D {
    pub position: Vec3,
    pub quaternion: Quat,
}

// People and scripts see a rotation as x, y and z degrees, applied the way most engines apply them: turn y degrees
// around the world's up axis first, then tip x degrees around the object's own x axis, then roll z degrees around its
// own z axis. That is glam's EulerRot::YXZ, which takes and returns the angles in y, x, z order.
pub fn euler_degrees_to_quat(euler_degrees: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, euler_degrees.y.to_radians(), euler_degrees.x.to_radians(), euler_degrees.z.to_radians())
}

// This only ever reports an x in [-90, 90], so an object tipped past straight up comes back with its y and z turned
// by 180 degrees.
pub fn quat_to_euler_degrees(quaternion: Quat) -> Vec3 {
    let (y, x, z) = quaternion.to_euler(EulerRot::YXZ);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Movement3D {
    pub velocity: Vec3,
    pub angularVelocity: Vec3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicsToggles3D {
    pub isWalkthrough: bool,
    pub isPushable: bool,
    pub density: f32,
}
impl PhysicsToggles3D {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.density.is_finite() && self.density >= 0.0, "Invalid density.");
        Ok(())
    }
}

// Physical object properties at a point in time
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicsInfo3D {
    pub location: Location3D,
    pub movement: Movement3D,
}
impl PhysicsInfo3D {
    pub fn new(position: Vec3, quaternion: Quat, velocity: Vec3, angularVelocity: Vec3) -> Self {
        Self {
            location: Location3D { position, quaternion },
            movement: Movement3D { velocity, angularVelocity },
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.location.position.is_finite() && self.location.quaternion.is_normalized() && self.movement.velocity.is_finite() && self.movement.angularVelocity.is_finite(),
            "Invalid transform or movement."
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Orientation3D {
    // this only has 2 DoF because rotation around the Y axis is fixed in this DS (always vertical)
    pub theta: f32,
    pub phi: f32,
}
impl Orientation3D {
    const PHI_CLAMP: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

    // pub fn to_quat(&self) -> Quat {
    //     Quat::from_rotation_y(self.theta) * Quat::from_rotation_x(self.phi)
    // }

    pub fn Z() -> Self {
        Self { theta: 0.0, phi: 0.0 }
    }

    pub fn to_unit_direction(&self) -> Vec3 {
        // theta=0 means facing +Z, theta=PI/2 means facing +X.
        // phi=0 means horizontal, phi=PI/2 means facing downward.

        let phi = self.phi;
        let cos = phi.cos();
        let sin = phi.sin();

        let theta = self.theta;
        let x = cos * theta.sin();
        let z = cos * theta.cos();
        let y = -sin;

        Vec3::new(x, y, z)
    }

    // The opposite of to_unit_direction. Straight up or down is tilted slightly, like turning, so the camera can still face it.
    pub fn from_unit_direction(unit_direction: Vec3) -> Self {
        Self {
            theta: unit_direction.x.atan2(unit_direction.z),
            phi: (-unit_direction.y).clamp(-1.0, 1.0).asin().clamp(-Self::PHI_CLAMP, Self::PHI_CLAMP),
        }
    }

    pub fn rotate_as_camera(&mut self, delta_theta: f32, delta_phi: f32) {
        self.theta += delta_theta;
        self.phi = (self.phi + delta_phi).clamp(-Self::PHI_CLAMP, Self::PHI_CLAMP);
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CameraLocation {
    pub position: Vec3,
    pub orientation: Orientation3D,
}

// ------------------------------------------------------- Visuals -------------------------------------------------------

// colors
// Color3 - opaque [r,g, b], all 0 to 1
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Color3 {
    pub r: f32, // 0 to 1
    pub g: f32, // 0 to 1
    pub b: f32, // 0 to 1
}
impl Color3 {
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0 };
    pub const DEFAULT: Self = Self { r: 0.5, g: 0.5, b: 0.5 };
}

// for primitives only (ball, brick, cylinder, mesh, label)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VisualSettings {
    pub opacity: f32,
    pub color: Color3, // base color
    pub glow: f32,     // 0-1: shines in its own color whatever the light, and glows onto its surroundings
    pub isAlwaysOnTop: bool,
}
impl VisualSettings {
    pub const TYPICAL_OPAQUE: Self = Self {
        opacity: 1.0,
        color: Color3::DEFAULT,
        glow: 0.0,
        isAlwaysOnTop: false,
    };
}
