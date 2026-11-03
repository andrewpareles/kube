pub use crate::data_types::{MeshData, MeshVertexInternal, SkinningInfoPerVertex};

pub use crate::object_types::{
    ball::{BallInfo, BallMaterialSettings},
    brick::{BrickInfo, BrickMaterialSettings},
    clock::{ClockInfo, ClockState, InterpolationStyle, WeightTransition},
    cylinder::{CylinderInfo, CylinderMaterialSettings},
    label::{BillboardMode, LabelHorizontalAlign, LabelInfo},
    label_image::LabelImageInfo,
    localGroup::LocalGroupInfo,
    material::MaterialInfo,
    mesh::{MeshInfo, MeshMaterialSettings},
    pivot::PivotInfo,
    player::PlayerInfo,
    property_animation::PropertyAnimationInfo,
    property_animator::PropertyAnimatorInfo,
    property_keyframe::PropertyKeyframeInfo,
    rigidGroup::RigidGroupInfo,
    script::{ObjectLocation, ScriptInfo},
    shared::{CameraLocation, Location3D, Movement3D, ObjectId, ObjectIdPointer, Orientation3D, PhysicsInfo3D, PhysicsToggles3D, VisualSettings},
    sk_animation::SkAnimationInfo,
    sk_attachment::SkAttachmentInfo,
    sk_joint::SkJointInfo,
    sk_keyframe::SkKeyframeInfo,
    sk_keyframe_pose::SkKeyframePoseInfo,
    skeleton::SkeletonInfo,
    sound_player::SoundPlayerInfo,
    tween::TweenInfo,
    world_lighting::WorldLightingInfo,
};
use crate::{
    object_types::sk_animator::SkAnimatorInfoTrackedFields,
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings},
};
use crate::object_types::{
    ball::BallInfoTrackedFields, brick::BrickInfoTrackedFields, cylinder::CylinderInfoTrackedFields, label::LabelInfoTrackedFields,
    label_image::LabelImageInfoTrackedFields, mesh::MeshInfoTrackedFields, pivot::PivotInfoTrackedFields, player::PlayerInfoTrackedFields,
    property_animator::PropertyAnimatorInfoTrackedFields, rigidGroup::RigidGroupInfoTrackedFields, sk_animator::SkAnimatorInfo,
    sk_attachment::SkAttachmentInfoTrackedFields, sound_player::SoundPlayerInfoTrackedFields, tween::TweenInfoTrackedFields,
};
use serde::{Deserialize, Serialize};
pub mod ball;
pub mod brick;
pub mod clock;
pub mod cylinder;
pub mod label;
pub mod label_image;
pub mod localGroup;
pub mod material;
pub mod mesh;
pub mod pivot;
pub mod player;
pub mod property_animation;
pub mod property_animator;
pub mod property_keyframe;
pub mod rigidGroup;
pub mod script;
pub mod shared;
pub mod sk_animation;
pub mod sk_animator;
pub mod sk_attachment;
pub mod sk_joint;
pub mod sk_keyframe;
pub mod sk_keyframe_pose;
pub mod skeleton;
pub mod sound_player;
pub mod tween;
pub mod world_lighting;

// ---------------- ObjectInfo ----------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ObjectInfo {
    Ball(BallInfo),
    SkJoint(SkJointInfo),
    Skeleton(SkeletonInfo),
    Brick(BrickInfo),
    Cylinder(CylinderInfo),
    Clock(ClockInfo),
    Label(LabelInfo),
    LabelImage(LabelImageInfo),
    LocalGroup(LocalGroupInfo),
    Material(MaterialInfo),
    Mesh(MeshInfo),
    Pivot(PivotInfo),
    Player(PlayerInfo),
    RigidGroup(RigidGroupInfo),
    Script(ScriptInfo),
    SoundPlayer(SoundPlayerInfo),
    SkAnimation(SkAnimationInfo),
    SkAttachment(SkAttachmentInfo),
    SkKeyframe(SkKeyframeInfo),
    SkKeyframePose(SkKeyframePoseInfo),
    SkAnimator(SkAnimatorInfo),
    PropertyAnimation(PropertyAnimationInfo),
    PropertyKeyframe(PropertyKeyframeInfo),
    PropertyAnimator(PropertyAnimatorInfo),
    Tween(TweenInfo),
    WorldLighting(WorldLightingInfo),
}

macro_rules! impl_from_object_info {
    ($from:ty, $variant:ident) => {
        impl From<$from> for ObjectInfo {
            fn from(value: $from) -> Self {
                ObjectInfo::$variant(value)
            }
        }
    };
}

impl_from_object_info!(BallInfo, Ball);
impl_from_object_info!(SkJointInfo, SkJoint);
impl_from_object_info!(SkeletonInfo, Skeleton);
impl_from_object_info!(BrickInfo, Brick);
impl_from_object_info!(CylinderInfo, Cylinder);
impl_from_object_info!(ClockInfo, Clock);
impl_from_object_info!(LabelInfo, Label);
impl_from_object_info!(LabelImageInfo, LabelImage);
impl_from_object_info!(LocalGroupInfo, LocalGroup);
impl_from_object_info!(MaterialInfo, Material);
impl_from_object_info!(MeshInfo, Mesh);
impl_from_object_info!(PivotInfo, Pivot);
impl_from_object_info!(PlayerInfo, Player);
impl_from_object_info!(RigidGroupInfo, RigidGroup);
impl_from_object_info!(ScriptInfo, Script);
impl_from_object_info!(SoundPlayerInfo, SoundPlayer);
impl_from_object_info!(SkAnimationInfo, SkAnimation);
impl_from_object_info!(SkAttachmentInfo, SkAttachment);
impl_from_object_info!(SkKeyframeInfo, SkKeyframe);
impl_from_object_info!(SkKeyframePoseInfo, SkKeyframePose);
impl_from_object_info!(SkAnimatorInfo, SkAnimator);
impl_from_object_info!(PropertyAnimationInfo, PropertyAnimation);
impl_from_object_info!(PropertyKeyframeInfo, PropertyKeyframe);
impl_from_object_info!(PropertyAnimatorInfo, PropertyAnimator);
impl_from_object_info!(TweenInfo, Tween);
impl_from_object_info!(WorldLightingInfo, WorldLighting);

// ---------------- Object Kind ----------------
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ObjectKind {
    Ball,
    SkJoint,
    Skeleton,
    Brick,
    Cylinder,
    Clock,
    Label,
    LabelImage,
    LocalGroup,
    Material,
    Mesh,
    Pivot,
    Player,
    RigidGroup,
    Script,
    SoundPlayer,
    SkAnimation,
    SkAttachment,
    SkKeyframe,
    SkKeyframePose,
    SkAnimator,
    PropertyAnimation,
    PropertyKeyframe,
    PropertyAnimator,
    Tween,
    WorldLighting,
}

impl ObjectKind {
    pub fn from_object_info(value: &ObjectInfo) -> Self {
        match value {
            ObjectInfo::Ball(_) => Self::Ball,
            ObjectInfo::SkJoint(_) => Self::SkJoint,
            ObjectInfo::Skeleton(_) => Self::Skeleton,
            ObjectInfo::Brick(_) => Self::Brick,
            ObjectInfo::Cylinder(_) => Self::Cylinder,
            ObjectInfo::Clock(_) => Self::Clock,
            ObjectInfo::Label(_) => Self::Label,
            ObjectInfo::LabelImage(_) => Self::LabelImage,
            ObjectInfo::LocalGroup(_) => Self::LocalGroup,
            ObjectInfo::Material(_) => Self::Material,
            ObjectInfo::Mesh(_) => Self::Mesh,
            ObjectInfo::Pivot(_) => Self::Pivot,
            ObjectInfo::Player(_) => Self::Player,
            ObjectInfo::RigidGroup(_) => Self::RigidGroup,
            ObjectInfo::Script(_) => Self::Script,
            ObjectInfo::SoundPlayer(_) => Self::SoundPlayer,
            ObjectInfo::SkAnimation(_) => Self::SkAnimation,
            ObjectInfo::SkAttachment(_) => Self::SkAttachment,
            ObjectInfo::SkKeyframe(_) => Self::SkKeyframe,
            ObjectInfo::SkKeyframePose(_) => Self::SkKeyframePose,
            ObjectInfo::SkAnimator(_) => Self::SkAnimator,
            ObjectInfo::PropertyAnimation(_) => Self::PropertyAnimation,
            ObjectInfo::PropertyKeyframe(_) => Self::PropertyKeyframe,
            ObjectInfo::PropertyAnimator(_) => Self::PropertyAnimator,
            ObjectInfo::Tween(_) => Self::Tween,
            ObjectInfo::WorldLighting(_) => Self::WorldLighting,
        }
    }

    pub fn to_object_kind_name(&self) -> &'static str {
        match self {
            Self::Ball => "ball",
            Self::SkJoint => "skJoint",
            Self::Skeleton => "skeleton",
            Self::Brick => "brick",
            Self::Cylinder => "cylinder",
            Self::Clock => "clock",
            Self::Label => "label",
            Self::LabelImage => "labelImage",
            Self::LocalGroup => "group",
            Self::Material => "material",
            Self::Mesh => "mesh",
            Self::Pivot => "pivot",
            Self::Player => "player",
            Self::RigidGroup => "rigidGroup",
            Self::Script => "script",
            Self::SoundPlayer => "soundPlayer",
            Self::SkAnimation => "skAnimation",
            Self::SkAttachment => "skAttachment",
            Self::SkKeyframe => "skKeyframe",
            Self::SkKeyframePose => "skKeyframePose",
            Self::SkAnimator => "skAnimator",
            Self::PropertyAnimation => "propertyAnimation",
            Self::PropertyKeyframe => "propertyKeyframe",
            Self::PropertyAnimator => "propertyAnimator",
            Self::Tween => "tween",
            Self::WorldLighting => "worldLighting",
        }
    }

    pub fn from_object_kind_name(object_name: &str) -> Option<Self> {
        match object_name {
            "ball" => Some(Self::Ball),
            "skJoint" => Some(Self::SkJoint),
            "skeleton" => Some(Self::Skeleton),
            "brick" => Some(Self::Brick),
            "cylinder" => Some(Self::Cylinder),
            "clock" => Some(Self::Clock),
            "label" => Some(Self::Label),
            "labelImage" => Some(Self::LabelImage),
            "group" => Some(Self::LocalGroup),
            "material" => Some(Self::Material),
            "mesh" => Some(Self::Mesh),
            "pivot" => Some(Self::Pivot),
            "player" => Some(Self::Player),
            "rigidGroup" => Some(Self::RigidGroup),
            "script" => Some(Self::Script),
            "soundPlayer" => Some(Self::SoundPlayer),
            "skAnimation" => Some(Self::SkAnimation),
            "skAttachment" => Some(Self::SkAttachment),
            "skKeyframe" => Some(Self::SkKeyframe),
            "skKeyframePose" => Some(Self::SkKeyframePose),
            "skAnimator" => Some(Self::SkAnimator),
            "propertyAnimation" => Some(Self::PropertyAnimation),
            "propertyKeyframe" => Some(Self::PropertyKeyframe),
            "propertyAnimator" => Some(Self::PropertyAnimator),
            "tween" => Some(Self::Tween),
            "worldLighting" => Some(Self::WorldLighting),
            _ => None,
        }
    }
}

// ---------------- ObjectAndPhysicsInfo ----------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ObjectBeingAdded {
    Ball(BallInfo, BallPrimitiveSettings, PhysicsInfo3D, PhysicsToggles3D),
    SkJoint(SkJointInfo),
    Skeleton(SkeletonInfo),
    Brick(BrickInfo, BrickPrimitiveSettings, PhysicsInfo3D, PhysicsToggles3D),
    Cylinder(CylinderInfo, CylinderPrimitiveSettings, PhysicsInfo3D, PhysicsToggles3D),
    Clock(ClockInfo),
    Label(LabelInfo),
    LabelImage(LabelImageInfo),
    LocalGroup(LocalGroupInfo),
    Material(MaterialInfo),
    Mesh(MeshInfo),
    Pivot(PivotInfo),
    Player(PlayerInfo),
    RigidGroup(RigidGroupInfo, PhysicsInfo3D),
    Script(ScriptInfo), // contains non-networked script raw_code
    SoundPlayer(SoundPlayerInfo),
    SkAnimation(SkAnimationInfo),
    SkAttachment(SkAttachmentInfo),
    SkKeyframe(SkKeyframeInfo),
    SkKeyframePose(SkKeyframePoseInfo),
    SkAnimator(SkAnimatorInfo),
    PropertyAnimation(PropertyAnimationInfo),
    PropertyKeyframe(PropertyKeyframeInfo),
    PropertyAnimator(PropertyAnimatorInfo),
    Tween(TweenInfo),
    WorldLighting(WorldLightingInfo),
}

impl ObjectBeingAdded {
    pub fn get_physics_info(&self) -> Option<&PhysicsInfo3D> {
        match self {
            ObjectBeingAdded::Ball(_, _, physics_info, _) => Some(physics_info),
            ObjectBeingAdded::Brick(_, _, physics_info, _) => Some(physics_info),
            ObjectBeingAdded::Cylinder(_, _, physics_info, _) => Some(physics_info),
            ObjectBeingAdded::RigidGroup(_, physics_info) => Some(physics_info),
            _ => None,
        }
    }

    pub fn to_object_info(self) -> ObjectInfo {
        match self {
            ObjectBeingAdded::Ball(info, _, _, _) => ObjectInfo::Ball(info),
            ObjectBeingAdded::SkJoint(info) => ObjectInfo::SkJoint(info),
            ObjectBeingAdded::Skeleton(info) => ObjectInfo::Skeleton(info),
            ObjectBeingAdded::Brick(info, _, _, _) => ObjectInfo::Brick(info),
            ObjectBeingAdded::Cylinder(info, _, _, _) => ObjectInfo::Cylinder(info),
            ObjectBeingAdded::RigidGroup(info, _) => ObjectInfo::RigidGroup(info),
            ObjectBeingAdded::Clock(info) => ObjectInfo::Clock(info),
            ObjectBeingAdded::Label(info) => ObjectInfo::Label(info),
            ObjectBeingAdded::LabelImage(info) => ObjectInfo::LabelImage(info),
            ObjectBeingAdded::LocalGroup(info) => ObjectInfo::LocalGroup(info),
            ObjectBeingAdded::Material(info) => ObjectInfo::Material(info),
            ObjectBeingAdded::Mesh(info) => ObjectInfo::Mesh(info),
            ObjectBeingAdded::Pivot(info) => ObjectInfo::Pivot(info),
            ObjectBeingAdded::Player(info) => ObjectInfo::Player(info),
            ObjectBeingAdded::Script(info) => ObjectInfo::Script(info),
            ObjectBeingAdded::SoundPlayer(info) => ObjectInfo::SoundPlayer(info),
            ObjectBeingAdded::SkAnimation(info) => ObjectInfo::SkAnimation(info),
            ObjectBeingAdded::SkAttachment(info) => ObjectInfo::SkAttachment(info),
            ObjectBeingAdded::SkKeyframe(info) => ObjectInfo::SkKeyframe(info),
            ObjectBeingAdded::SkKeyframePose(info) => ObjectInfo::SkKeyframePose(info),
            ObjectBeingAdded::SkAnimator(info) => ObjectInfo::SkAnimator(info),
            ObjectBeingAdded::PropertyAnimation(info) => ObjectInfo::PropertyAnimation(info),
            ObjectBeingAdded::PropertyKeyframe(info) => ObjectInfo::PropertyKeyframe(info),
            ObjectBeingAdded::PropertyAnimator(info) => ObjectInfo::PropertyAnimator(info),
            ObjectBeingAdded::Tween(info) => ObjectInfo::Tween(info),
            ObjectBeingAdded::WorldLighting(info) => ObjectInfo::WorldLighting(info),
        }
    }
}

// ---------------- ObjectInfoTrackedFields ----------------
// fields we need to update internally when they change
pub enum ObjectInfoTrackedFields {
    Ball(Option<BallInfoTrackedFields>),
    SkJoint(),
    Skeleton(),
    Brick(Option<BrickInfoTrackedFields>),
    Cylinder(Option<CylinderInfoTrackedFields>),
    Clock(),
    Label(Option<LabelInfoTrackedFields>),
    LabelImage(Option<LabelImageInfoTrackedFields>),
    LocalGroup(),
    Material(),
    Mesh(Option<MeshInfoTrackedFields>),
    Pivot(Option<PivotInfoTrackedFields>),
    Player(Option<PlayerInfoTrackedFields>),
    RigidGroup(Option<RigidGroupInfoTrackedFields>),
    Script(),
    SoundPlayer(Option<SoundPlayerInfoTrackedFields>),
    SkAnimation(),
    SkAttachment(Option<SkAttachmentInfoTrackedFields>),
    SkKeyframe(),
    SkKeyframePose(),
    SkAnimator(Option<SkAnimatorInfoTrackedFields>),
    PropertyAnimation(),
    PropertyKeyframe(),
    PropertyAnimator(Option<PropertyAnimatorInfoTrackedFields>),
    Tween(Option<TweenInfoTrackedFields>),
    WorldLighting(),
}

impl ObjectInfoTrackedFields {
    pub fn from_object_info(object_info: &ObjectInfo) -> Self {
        match &object_info {
            ObjectInfo::Ball(info) => ObjectInfoTrackedFields::Ball(Some(BallInfoTrackedFields { material: info.material })),
            ObjectInfo::SkJoint(_) => ObjectInfoTrackedFields::SkJoint(),
            ObjectInfo::Skeleton(_) => ObjectInfoTrackedFields::Skeleton(),
            ObjectInfo::Brick(info) => ObjectInfoTrackedFields::Brick(Some(BrickInfoTrackedFields { material: info.material })),
            ObjectInfo::Cylinder(info) => ObjectInfoTrackedFields::Cylinder(Some(CylinderInfoTrackedFields { material: info.material })),
            ObjectInfo::Clock(_) => ObjectInfoTrackedFields::Clock(),
            ObjectInfo::Label(info) => ObjectInfoTrackedFields::Label(Some(LabelInfoTrackedFields {
                physicalObjectId: info.physicalObjectId,
            })),
            ObjectInfo::LabelImage(info) => ObjectInfoTrackedFields::LabelImage(Some(LabelImageInfoTrackedFields {
                physicalObjectId: info.physicalObjectId,
            })),
            ObjectInfo::LocalGroup(_) => ObjectInfoTrackedFields::LocalGroup(),
            ObjectInfo::Material(_) => ObjectInfoTrackedFields::Material(),
            ObjectInfo::Mesh(info) => ObjectInfoTrackedFields::Mesh(Some(MeshInfoTrackedFields {
                followSkeletonId: info.followSkeletonId,
                physicalObjectId: info.physicalObjectId,
                material: info.material,
            })),
            ObjectInfo::Pivot(info) => ObjectInfoTrackedFields::Pivot(Some(PivotInfoTrackedFields {
                physicalObjectId: info.physicalObjectId,
            })),
            ObjectInfo::Player(info) => ObjectInfoTrackedFields::Player(Some(PlayerInfoTrackedFields {
                headObjectId: info.headObjectId,
                characterObjectId: info.characterObjectId,
            })),
            ObjectInfo::RigidGroup(info) => ObjectInfoTrackedFields::RigidGroup(Some(RigidGroupInfoTrackedFields {
                followSkeletonId: info.followSkeletonId,
            })),
            ObjectInfo::Script(_) => ObjectInfoTrackedFields::Script(),
            ObjectInfo::SoundPlayer(info) => ObjectInfoTrackedFields::SoundPlayer(Some(SoundPlayerInfoTrackedFields { clockId: info.clockId })),
            ObjectInfo::SkAnimation(_) => ObjectInfoTrackedFields::SkAnimation(),
            ObjectInfo::SkAttachment(info) => ObjectInfoTrackedFields::SkAttachment(Some(SkAttachmentInfoTrackedFields { objectId: info.objectId })),
            ObjectInfo::SkKeyframe(_) => ObjectInfoTrackedFields::SkKeyframe(),
            ObjectInfo::SkKeyframePose(_) => ObjectInfoTrackedFields::SkKeyframePose(),
            ObjectInfo::SkAnimator(info) => ObjectInfoTrackedFields::SkAnimator(Some(SkAnimatorInfoTrackedFields {
                skeletonId: info.skeletonId,
                clockId: info.clockId,
                skAnimationId: info.skAnimationId,
            })),
            ObjectInfo::PropertyAnimation(_) => ObjectInfoTrackedFields::PropertyAnimation(),
            ObjectInfo::PropertyKeyframe(_) => ObjectInfoTrackedFields::PropertyKeyframe(),
            ObjectInfo::PropertyAnimator(info) => ObjectInfoTrackedFields::PropertyAnimator(Some(PropertyAnimatorInfoTrackedFields {
                targetObjectId: info.targetObjectId,
                clockId: info.clockId,
                propertyAnimationId: info.propertyAnimationId,
            })),
            ObjectInfo::Tween(info) => ObjectInfoTrackedFields::Tween(Some(TweenInfoTrackedFields {
                targetObjectId: info.targetObjectId,
            })),
            ObjectInfo::WorldLighting(_) => ObjectInfoTrackedFields::WorldLighting(),
        }
    }

    pub fn from_nonexistent(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Ball => ObjectInfoTrackedFields::Ball(None),
            ObjectKind::SkJoint => ObjectInfoTrackedFields::SkJoint(),
            ObjectKind::Skeleton => ObjectInfoTrackedFields::Skeleton(),
            ObjectKind::Brick => ObjectInfoTrackedFields::Brick(None),
            ObjectKind::Cylinder => ObjectInfoTrackedFields::Cylinder(None),
            ObjectKind::Clock => ObjectInfoTrackedFields::Clock(),
            ObjectKind::Label => ObjectInfoTrackedFields::Label(None),
            ObjectKind::LabelImage => ObjectInfoTrackedFields::LabelImage(None),
            ObjectKind::LocalGroup => ObjectInfoTrackedFields::LocalGroup(),
            ObjectKind::Material => ObjectInfoTrackedFields::Material(),
            ObjectKind::Mesh => ObjectInfoTrackedFields::Mesh(None),
            ObjectKind::Pivot => ObjectInfoTrackedFields::Pivot(None),
            ObjectKind::Player => ObjectInfoTrackedFields::Player(None),
            ObjectKind::RigidGroup => ObjectInfoTrackedFields::RigidGroup(None),
            ObjectKind::Script => ObjectInfoTrackedFields::Script(),
            ObjectKind::SoundPlayer => ObjectInfoTrackedFields::SoundPlayer(None),
            ObjectKind::SkAnimation => ObjectInfoTrackedFields::SkAnimation(),
            ObjectKind::SkAttachment => ObjectInfoTrackedFields::SkAttachment(None),
            ObjectKind::SkKeyframe => ObjectInfoTrackedFields::SkKeyframe(),
            ObjectKind::SkKeyframePose => ObjectInfoTrackedFields::SkKeyframePose(),
            ObjectKind::SkAnimator => ObjectInfoTrackedFields::SkAnimator(None),
            ObjectKind::PropertyAnimation => ObjectInfoTrackedFields::PropertyAnimation(),
            ObjectKind::PropertyKeyframe => ObjectInfoTrackedFields::PropertyKeyframe(),
            ObjectKind::PropertyAnimator => ObjectInfoTrackedFields::PropertyAnimator(None),
            ObjectKind::Tween => ObjectInfoTrackedFields::Tween(None),
            ObjectKind::WorldLighting => ObjectInfoTrackedFields::WorldLighting(),
        }
    }
}

// ---------------- Live Object ----------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveObject {
    pub id: ObjectId,
    pub name: String,
    pub info: ObjectInfo,
    // parent/children are accounted for live, in the world
}
