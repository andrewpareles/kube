use crate::render_scene::{FontAtlas, LabelGeometry};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use game_types::data_types::DataId;
use game_types::object_types::label::BillboardMode;
use game_types::object_types::shared::Color3;
use game_types::{
    object_types::{
        BallMaterialSettings, BrickMaterialSettings, ClockState, CylinderMaterialSettings, Location3D, MaterialInfo, MeshMaterialSettings, ObjectId,
        PhysicsInfo3D, SkAnimationInfo, VisualSettings, WeightTransition, WorldLightingInfo,
    },
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct SkJointToFollow {
    pub skeleton_id: ObjectId,
    pub joint_name: String,
    pub offset_from_joint: Location3D, // position objects following this joint as offset by this amount

    // Where the rigid group that follows the Skeleton currently is. SkKeyframePoses are relative to the group, so this is what places the animated parts in the world (see computeFollowedSkJointMatrix).
    // Resolve this id in the render snapshot so every part shares the same interpolated group location.
    pub follower_group_id: ObjectId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RenderSkJoint {
    pub name: String,
    pub parent_joint_index: Option<usize>,
    pub rest_pose: Location3D,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderSkeletonInfo {
    pub jointOfIndex: Vec<RenderSkJoint>,
    pub jointIndexOfName: HashMap<String, usize>,
    pub jointIndexOfObjectId: HashMap<ObjectId, usize>, // which SkJoint object each joint came from, so animators on it can find it
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderSkKeyframePose {
    pub jointName: String,
    pub localPose: Location3D,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderSkKeyframe {
    pub time: f32,
    pub skKeyframePoses: Vec<RenderSkKeyframePose>,
}

// sk_joint_to_follow and Skeleton animations are Arc'd because the render thread clones the whole
// RenderObject map every frame; everything else in these structs is flat data, so the clone stays allocation-free.
// A welded primitive stores its owning group and local pose; only independent bodies interpolate world motion.
// Physical render objects share their transform data so snapshots, interpolation, and animation use the same fields.
#[derive(Clone, Serialize, Deserialize)]
pub struct RenderTransform {
    pub physicsInfo3D: PhysicsInfo3D,
    pub teleport_count: u64,
    pub rigid_group_to_follow: Option<(ObjectId, Location3D)>,
    pub sk_joint_to_follow_arc: Option<Arc<SkJointToFollow>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderBall {
    pub transform: RenderTransform,
    pub visualSettings: VisualSettings,
    pub material: Option<BallMaterialSettings>,
    pub primitive: BallPrimitiveSettings,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderBrick {
    pub transform: RenderTransform,
    pub visualSettings: VisualSettings,
    pub material: Option<BrickMaterialSettings>,
    pub primitive: BrickPrimitiveSettings,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderCylinder {
    pub transform: RenderTransform,
    pub visualSettings: VisualSettings,
    pub material: Option<CylinderMaterialSettings>,
    pub primitive: CylinderPrimitiveSettings,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderMesh {
    pub physicalObjectId: ObjectId,
    pub visualSettings: VisualSettings,
    pub material: Option<MeshMaterialSettings>,
    pub meshDataId: DataId,
    pub scale: Vec3,
    pub skin_skeleton_id: Option<ObjectId>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderSkAnimation {
    pub skAnimationInfo: SkAnimationInfo,
    pub skKeyframes: Vec<RenderSkKeyframe>,

    pub clock_state: ClockState,
    pub should_loop: bool,
    pub speed: f32,

    pub priority: u8,
    pub weight: f32,
    pub weight_transition: Option<WeightTransition>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderSkeleton {
    pub renderSkeletonInfoArc: Arc<RenderSkeletonInfo>,
    pub render_sk_animations: Arc<Vec<RenderSkAnimation>>,
}

// Labels and LabelImages follow and orient themselves the same way; only what they draw differs.
#[derive(Clone, Serialize, Deserialize)]
pub struct RenderLabel {
    pub physicalObjectId: ObjectId, // of the object being followed; its orientation is ignored while billboarding
    pub offsetFromObject: Vec3,
    pub rotationFromObject: Quat,
    pub billboardMode: BillboardMode,
    pub isConstantSize: bool,
    pub visualSettings: VisualSettings,
    pub content: RenderLabelContent,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum RenderLabelContent {
    Text {
        scale: f32, // layout units -> world units
        backgroundColor: Option<Color3>,
        backgroundOpacity: f32,
        geometry: Arc<LabelGeometry>,
        atlas: Arc<FontAtlas>,
    },
    Image {
        textureDataId: DataId,
        width: f32,  // world units
        height: f32, // world units
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub enum RenderObject {
    Material(MaterialInfo),
    Skeleton(RenderSkeleton),
    Ball(RenderBall),
    Brick(RenderBrick),
    Cylinder(RenderCylinder),
    Mesh(RenderMesh),
    RenderLabel(RenderLabel),
    // never renders; but is required if we want camera to be able to follow it during rendering
    RigidGroup(RenderTransform),
    WorldLighting(WorldLightingInfo),
}

impl RenderObject {
    // Meshes and labels refer to physical objects; only those objects own transform state.
    pub fn getTransform(&self) -> Option<&RenderTransform> {
        match self {
            Self::Ball(ball) => Some(&ball.transform),
            Self::Brick(brick) => Some(&brick.transform),
            Self::Cylinder(cylinder) => Some(&cylinder.transform),
            Self::RigidGroup(group) => Some(group),
            _ => None,
        }
    }

    pub fn getTransformMut(&mut self) -> Option<&mut RenderTransform> {
        match self {
            Self::Ball(ball) => Some(&mut ball.transform),
            Self::Brick(brick) => Some(&mut brick.transform),
            Self::Cylinder(cylinder) => Some(&mut cylinder.transform),
            Self::RigidGroup(group) => Some(group),
            _ => None,
        }
    }
}
