#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};

use game_types::object_types::sk_animator::SkAnimatorInfo;
use game_types::object_types::{
    BallInfo, BillboardMode, BrickInfo, CameraLocation, ClockInfo, ClockState, CylinderInfo, InterpolationStyle, LabelHorizontalAlign, LabelImageInfo,
    LabelInfo, LiveObject, LocalGroupInfo, Location3D, MaterialInfo, MeshInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo,
    ObjectInfoTrackedFields, ObjectKind, ObjectLocation, Orientation3D, PhysicsInfo3D, PhysicsToggles3D, PivotInfo, PlayerInfo, PropertyAnimationInfo,
    PropertyAnimatorInfo, PropertyKeyframeInfo, RigidGroupInfo, ScriptInfo, SkAnimationInfo, SkAttachmentInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo,
    SkeletonInfo, SoundPlayerInfo, TweenInfo, VisualSettings, WorldLightingInfo, material::AlphaMode,
};
use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings},
    stored_world_type::StoredWorld,
};
use glam::{Quat, Vec3};
use shared::game::world::{World, WorldControlMode};

#[derive(Debug, PartialEq, Eq)]
pub struct ComparableWorldState {
    root_object_id: ObjectId,
    object_json_of_id: BTreeMap<ObjectId, String>,
    parent_of_id: BTreeMap<ObjectId, ObjectId>,
    children_of_id: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
    physics_info_json_of_id: BTreeMap<ObjectId, String>,
    physics_toggles_json_of_id: BTreeMap<ObjectId, String>,
    pointers_to_id: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
    controlled_by_us_of_id: BTreeMap<ObjectId, bool>,
}

pub fn fallback_camera_location_for_tests() -> CameraLocation {
    CameraLocation {
        position: Vec3::new(0.0, -10.0, 0.0),
        orientation: Orientation3D {
            theta: 0.0,
            phi: std::f32::consts::FRAC_PI_4,
        },
    }
}

impl ComparableWorldState {
    pub fn from(world: &World) -> anyhow::Result<Self> {
        let mut object_ids: Vec<ObjectId> = world.objectOfId.keys().copied().collect();
        object_ids.sort_unstable();

        let mut object_json_of_id = BTreeMap::new();
        let mut physics_info_json_of_id = BTreeMap::new();
        let mut physics_toggles_json_of_id = BTreeMap::new();
        let mut controlled_by_us_of_id = BTreeMap::new();

        for object_id in object_ids {
            object_json_of_id.insert(object_id, serde_json::to_string(world.getObject(object_id)?)?);

            if let Some(physics_info) = world.getObjectPhysicsInfo3D(object_id) {
                physics_info_json_of_id.insert(object_id, serde_json::to_string(&physics_info)?);
            }
            if let Some(physics_toggles) = world.getObjectPhysicsToggles3D(object_id) {
                physics_toggles_json_of_id.insert(object_id, serde_json::to_string(&physics_toggles)?);
            }
            if let Some(is_controlled_by_us) = world.getIsObjectControlledByUs(object_id) {
                controlled_by_us_of_id.insert(object_id, is_controlled_by_us);
            }
        }

        let parent_of_id = world.parentOfId.iter().map(|(object_id, parent_id)| (*object_id, *parent_id)).collect();

        let children_of_id = world
            ._childrenOfId
            .iter()
            .map(|(object_id, children_ids)| (*object_id, children_ids.iter().copied().collect::<BTreeSet<ObjectId>>()))
            .collect();

        let pointers_to_id = world
            ._objectsThatPointToId
            .iter()
            .map(|(target_id, pointer_ids)| (*target_id, pointer_ids.iter().copied().collect::<BTreeSet<ObjectId>>()))
            .collect();

        Ok(ComparableWorldState {
            root_object_id: world.getRootObjectId(),
            object_json_of_id,
            parent_of_id,
            children_of_id,
            physics_info_json_of_id,
            physics_toggles_json_of_id,
            pointers_to_id,
            controlled_by_us_of_id,
        })
    }
}

pub fn make_world() -> anyhow::Result<World> {
    let root_object_id = 1;
    let root_object = LiveObject {
        id: root_object_id,
        name: "root".to_string(),
        info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
    };

    let mut object_of_id = HashMap::new();
    object_of_id.insert(root_object_id, root_object);

    let mut children_of_id = HashMap::new();
    children_of_id.insert(root_object_id, Vec::new());

    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: fallback_camera_location_for_tests(),
        objectOfId: object_of_id,
        rootObjectId: root_object_id,
        childrenOfId: children_of_id,
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        scriptRawCodeOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };

    let stored_world_string = serde_json::to_string(&stored_world)?;
    let world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;
    Ok(world)
}

pub fn make_world_controlled_by_player(player_object_id: ObjectId) -> anyhow::Result<World> {
    let root_object_id = 1;
    let root_object = LiveObject {
        id: root_object_id,
        name: "root".to_string(),
        info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
    };

    let mut object_of_id = HashMap::new();
    object_of_id.insert(root_object_id, root_object);

    let mut children_of_id = HashMap::new();
    children_of_id.insert(root_object_id, Vec::new());

    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: fallback_camera_location_for_tests(),
        objectOfId: object_of_id,
        rootObjectId: root_object_id,
        childrenOfId: children_of_id,
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        scriptRawCodeOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };

    let stored_world_string = serde_json::to_string(&stored_world)?;
    let world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Player(player_object_id), None)?;
    Ok(world)
}

pub fn physics_info(position: Vec3) -> PhysicsInfo3D {
    PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO)
}

pub fn physics_toggles(is_pushable: bool, density: f32) -> PhysicsToggles3D {
    PhysicsToggles3D {
        isWalkthrough: false,
        isPushable: is_pushable,
        density,
    }
}

pub fn add_rigid_group(
    world: &mut World,
    object_id: ObjectId,
    parent_id: ObjectId,
    position: Vec3,
    follow_skeleton_id: Option<ObjectId>,
) -> anyhow::Result<()> {
    let rigid_group_info = RigidGroupInfo {
        followSkeletonId: follow_skeleton_id.map(ObjectIdPointer),
    };

    world.addObjectWithId(
        object_id,
        format!("rigid-group-{object_id}"),
        ObjectBeingAdded::RigidGroup(rigid_group_info, physics_info(position)),
        parent_id,
    );
    Ok(())
}

pub fn add_ball(world: &mut World, object_id: ObjectId, parent_id: ObjectId, position: Vec3, is_pushable: bool) -> anyhow::Result<()> {
    let ball_info = BallInfo {
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };

    world.addObjectWithId(
        object_id,
        format!("ball-{object_id}"),
        ObjectBeingAdded::Ball(
            ball_info,
            BallPrimitiveSettings { radius: 1.0 },
            physics_info(position),
            physics_toggles(is_pushable, 1.0),
        ),
        parent_id,
    );
    Ok(())
}

pub fn add_brick(world: &mut World, object_id: ObjectId, parent_id: ObjectId, position: Vec3, is_pushable: bool) -> anyhow::Result<()> {
    let brick_info = BrickInfo {
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };

    world.addObjectWithId(
        object_id,
        format!("brick-{object_id}"),
        ObjectBeingAdded::Brick(
            brick_info,
            BrickPrimitiveSettings {
                size: Vec3::new(2.0, 2.0, 2.0),
            },
            physics_info(position),
            physics_toggles(is_pushable, 2.0),
        ),
        parent_id,
    );
    Ok(())
}

pub fn add_cylinder(world: &mut World, object_id: ObjectId, parent_id: ObjectId, position: Vec3, is_pushable: bool) -> anyhow::Result<()> {
    let cylinder_info = CylinderInfo {
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };

    world.addObjectWithId(
        object_id,
        format!("cylinder-{object_id}"),
        ObjectBeingAdded::Cylinder(
            cylinder_info,
            CylinderPrimitiveSettings { radius: 1.0, length: 2.0 },
            physics_info(position),
            physics_toggles(is_pushable, 1.0),
        ),
        parent_id,
    );
    Ok(())
}

pub fn add_skeleton(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("skeleton-{object_id}"),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        parent_id,
    );
    Ok(())
}

pub fn add_clock(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let clock_info = ClockInfo {
        shouldLoop: true,
        clockState: ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Client,
        },
        speed: 1.0,
    };
    world.addObjectWithId(object_id, format!("clock-{object_id}"), ObjectBeingAdded::Clock(clock_info), parent_id);
    Ok(())
}

pub fn add_material(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let material_info = MaterialInfo {
        albedoTextureDataId: Some(9001),
        metallicTextureDataId: Some(9002),
        roughnessTextureDataId: Some(9003),
        normalTextureDataId: Some(9004),
        alphaMode: AlphaMode::Opaque,
        isDoubleSided: false,
    };
    world.addObjectWithId(object_id, format!("material-{object_id}"), ObjectBeingAdded::Material(material_info), parent_id);
    Ok(())
}

pub fn add_sound_player(world: &mut World, object_id: ObjectId, parent_id: ObjectId, clock_id: Option<ObjectId>) -> anyhow::Result<()> {
    let sound_player_info = SoundPlayerInfo {
        soundDataId: 777,
        volume: 0.6,
        clockId: clock_id.map(ObjectIdPointer),
    };

    world.addObjectWithId(
        object_id,
        format!("sound-player-{object_id}"),
        ObjectBeingAdded::SoundPlayer(sound_player_info),
        parent_id,
    );
    Ok(())
}

pub fn add_player(world: &mut World, object_id: ObjectId, parent_id: ObjectId, character_object_id: Option<ObjectId>) -> anyhow::Result<()> {
    let player_info = PlayerInfo {
        username: format!("player-{object_id}"),
        userid: Some(object_id as i64),
        preferredColor: None,
        equippedItemId: None,
        headObjectId: None,
        characterObjectId: character_object_id.map(ObjectIdPointer),
    };

    world.addObjectWithId(object_id, format!("player-{object_id}"), ObjectBeingAdded::Player(player_info), parent_id);
    Ok(())
}

pub fn make_world_for_callbacks() -> anyhow::Result<World> {
    let mut world = make_world()?;
    world.worldControlMode = WorldControlMode::Server;
    let highest_object_id = world
        .objectOfId
        .keys()
        .copied()
        .max()
        .ok_or_else(|| anyhow::anyhow!("Expected world to have a root object"))?;
    world.objectIdCounter = highest_object_id;
    Ok(world)
}

pub fn add_local_group(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("local-group-{object_id}"),
        ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
        parent_id,
    );
    Ok(())
}

pub fn add_label(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let label_info = LabelInfo {
        text: format!("label-{object_id}"),
        lineHeight: LabelInfo::DEFAULT_LINE_HEIGHT,
        maxWidth: None,
        horizontalAlign: LabelHorizontalAlign::Center,
        backgroundColor: None,
        backgroundOpacity: 1.0,
        offsetFromObject: Vec3::ZERO,
        rotationFromObject: Quat::IDENTITY,
        physicalObjectId: None,
        billboardMode: BillboardMode::Full,
        isConstantSize: false,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };
    world.addObjectWithId(object_id, format!("label-{object_id}"), ObjectBeingAdded::Label(label_info), parent_id);
    Ok(())
}

pub fn add_label_image(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let label_image_info = LabelImageInfo {
        textureDataId: 789,
        width: 1.0,
        height: 1.0,
        offsetFromObject: Vec3::ZERO,
        rotationFromObject: Quat::IDENTITY,
        physicalObjectId: None,
        billboardMode: BillboardMode::Full,
        isConstantSize: false,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };
    world.addObjectWithId(
        object_id,
        format!("label-image-{object_id}"),
        ObjectBeingAdded::LabelImage(label_image_info),
        parent_id,
    );
    Ok(())
}

pub fn add_mesh(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let mesh_info = MeshInfo {
        scale: Vec3::new(1.0, 1.0, 1.0),
        meshDataId: 456,
        followSkeletonId: None,
        physicalObjectId: None,
        material: None,
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
    };
    world.addObjectWithId(object_id, format!("mesh-{object_id}"), ObjectBeingAdded::Mesh(mesh_info), parent_id);
    Ok(())
}

pub fn add_script(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("script-{object_id}"),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        parent_id,
    );
    world.markScriptAsReadyToRun(object_id, format!("return {object_id};"));
    Ok(())
}

pub fn add_sk_animator(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let sk_animator_info = SkAnimatorInfo {
        skeletonId: None,
        clockId: None,
        skAnimationId: None,
        priority: 1,
        weight: 100.0,
        weightTransition: None,
    };
    world.addObjectWithId(
        object_id,
        format!("joint-animator-{object_id}"),
        ObjectBeingAdded::SkAnimator(sk_animator_info),
        parent_id,
    );
    Ok(())
}

pub fn add_tween(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let tween_info = TweenInfo {
        targetObjectId: None,
        propertyPath: Vec::new(),
        fromValue: 0.0,
        toValue: 0.0,
        duration: 0.0,
        interpolationStyle: InterpolationStyle::Linear,
        clockState: ClockState::Paused { animationTimeSecs: 0.0 },
    };
    world.addObjectWithId(object_id, format!("tween-{object_id}"), ObjectBeingAdded::Tween(tween_info), parent_id);
    Ok(())
}

pub fn add_property_animator(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let property_animator_info = PropertyAnimatorInfo {
        targetObjectId: None,
        propertyPath: Vec::new(),
        propertyAnimationId: None,
        clockId: None,
        priority: 1,
        weight: 100.0,
        weightTransition: None,
    };
    world.addObjectWithId(
        object_id,
        format!("property-animator-{object_id}"),
        ObjectBeingAdded::PropertyAnimator(property_animator_info),
        parent_id,
    );
    Ok(())
}

pub fn add_sk_animation(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let sk_animation_info = SkAnimationInfo {
        duration: 0.0,
        interpolationStyle: InterpolationStyle::Step,
    };
    world.addObjectWithId(
        object_id,
        format!("joint-animation-{object_id}"),
        ObjectBeingAdded::SkAnimation(sk_animation_info),
        parent_id,
    );
    Ok(())
}

pub fn add_property_animation(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let property_animation_info = PropertyAnimationInfo {
        duration: 0.0,
        interpolationStyle: InterpolationStyle::Step,
    };
    world.addObjectWithId(
        object_id,
        format!("property-animation-{object_id}"),
        ObjectBeingAdded::PropertyAnimation(property_animation_info),
        parent_id,
    );
    Ok(())
}

pub fn add_property_keyframe(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("property-keyframe-{object_id}"),
        ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 0.0, value: 0.0 }),
        parent_id,
    );
    Ok(())
}

pub fn add_sk_keyframe(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("joint-keyframe-{object_id}"),
        ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time: 0.0 }),
        parent_id,
    );
    Ok(())
}

pub fn add_sk_keyframe_pose(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("joint-pose-{object_id}"),
        ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
            jointName: "joint".to_string(),
            localPose: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        parent_id,
    );
    Ok(())
}

pub fn add_sk_attachment(world: &mut World, object_id: ObjectId, parent_id: ObjectId, attached_object_id: Option<ObjectId>) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("joint-attachment-{object_id}"),
        ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
            objectId: attached_object_id.map(ObjectIdPointer),
            jointName: "joint".to_string(),
            offsetFromJoint: Location3D {
                position: Vec3::ZERO,
                quaternion: Quat::IDENTITY,
            },
        }),
        parent_id,
    );
    Ok(())
}

pub fn add_pivot(
    world: &mut World,
    object_id: ObjectId,
    parent_id: ObjectId,
    physical_object_id: Option<ObjectId>,
    offset_from_object: Vec3,
) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("pivot-{object_id}"),
        ObjectBeingAdded::Pivot(PivotInfo {
            physicalObjectId: physical_object_id.map(ObjectIdPointer),
            offsetFromObject: offset_from_object,
        }),
        parent_id,
    );
    Ok(())
}

pub fn add_world_lighting(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    world.addObjectWithId(
        object_id,
        format!("world-lighting-{object_id}"),
        ObjectBeingAdded::WorldLighting(WorldLightingInfo::default()),
        parent_id,
    );
    Ok(())
}

pub fn add_object_for_kind_for_player_control_tests(
    world: &mut World,
    object_kind: ObjectKind,
    object_id: ObjectId,
    parent_id: ObjectId,
    player_control_source_object_id: ObjectId,
) -> anyhow::Result<()> {
    match object_kind {
        ObjectKind::Ball => add_ball(world, object_id, parent_id, Vec3::new(1.0, 2.0, 3.0), true)?,
        ObjectKind::SkJoint => {
            world.addObjectWithId(
                object_id,
                format!("joint-{object_id}"),
                ObjectBeingAdded::SkJoint(SkJointInfo {
                    restPose: Location3D {
                        position: Vec3::ZERO,
                        quaternion: Quat::IDENTITY,
                    },
                }),
                parent_id,
            );
        }
        ObjectKind::Skeleton => add_skeleton(world, object_id, parent_id)?,
        ObjectKind::Brick => add_brick(world, object_id, parent_id, Vec3::new(2.0, 2.0, 3.0), true)?,
        ObjectKind::Cylinder => add_cylinder(world, object_id, parent_id, Vec3::new(1.0, 2.0, 3.0), true)?,
        ObjectKind::Clock => add_clock(world, object_id, parent_id)?,
        ObjectKind::LocalGroup => add_local_group(world, object_id, parent_id)?,
        ObjectKind::Material => add_material(world, object_id, parent_id)?,
        ObjectKind::Label => add_label(world, object_id, parent_id)?,
        ObjectKind::LabelImage => add_label_image(world, object_id, parent_id)?,
        ObjectKind::Mesh => add_mesh(world, object_id, parent_id)?,
        ObjectKind::Pivot => add_pivot(world, object_id, parent_id, Some(player_control_source_object_id), Vec3::ZERO)?,
        ObjectKind::Player => add_player(world, object_id, parent_id, Some(player_control_source_object_id))?,
        ObjectKind::RigidGroup => add_rigid_group(world, object_id, parent_id, Vec3::new(0.0, 0.0, 0.0), None)?,
        ObjectKind::Script => add_script(world, object_id, parent_id)?,
        ObjectKind::SoundPlayer => add_sound_player(world, object_id, parent_id, None)?,
        ObjectKind::SkAnimation => add_sk_animation(world, object_id, parent_id)?,
        ObjectKind::SkAttachment => add_sk_attachment(world, object_id, parent_id, Some(player_control_source_object_id))?,
        ObjectKind::SkKeyframe => add_sk_keyframe(world, object_id, parent_id)?,
        ObjectKind::SkKeyframePose => add_sk_keyframe_pose(world, object_id, parent_id)?,
        ObjectKind::SkAnimator => add_sk_animator(world, object_id, parent_id)?,
        ObjectKind::PropertyAnimation => add_property_animation(world, object_id, parent_id)?,
        ObjectKind::PropertyKeyframe => add_property_keyframe(world, object_id, parent_id)?,
        ObjectKind::PropertyAnimator => add_property_animator(world, object_id, parent_id)?,
        ObjectKind::Tween => add_tween(world, object_id, parent_id)?,
        ObjectKind::WorldLighting => add_world_lighting(world, object_id, parent_id)?,
    }
    Ok(())
}

pub fn set_player_controls_and_run_after_change(world: &mut World, player_object_id: ObjectId, controlled_object_id: Option<ObjectId>) -> anyhow::Result<()> {
    let old_tracked_fields = {
        let player_obj = world.getObject(player_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&player_obj.info)
    };

    {
        let player_obj = world.getObjectMut(player_object_id)?;
        let ObjectInfo::Player(player_info) = &mut player_obj.info else {
            return Err(anyhow::anyhow!("Object {} is not a player", player_object_id));
        };
        player_info.characterObjectId = controlled_object_id.map(ObjectIdPointer);
    }

    let new_tracked_fields = {
        let player_obj = world.getObject(player_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&player_obj.info)
    };

    world.afterObjectInfoChange(player_object_id, old_tracked_fields, new_tracked_fields);
    Ok(())
}

pub fn assert_rigid_group_follow_skeleton(world: &World, rigid_group_object_id: ObjectId, expected_skeleton_id: Option<ObjectId>) -> anyhow::Result<()> {
    let object = world.getObject(rigid_group_object_id)?;
    let ObjectInfo::RigidGroup(rigid_group_info) = &object.info else {
        return Err(anyhow::anyhow!("Object {} is not a rigid group", rigid_group_object_id));
    };
    assert_eq!(rigid_group_info.followSkeletonId.map(Into::into), expected_skeleton_id);
    Ok(())
}

pub fn assert_sound_player_clock(world: &World, sound_player_object_id: ObjectId, expected_clock_id: Option<ObjectId>) -> anyhow::Result<()> {
    let object = world.getObject(sound_player_object_id)?;
    let ObjectInfo::SoundPlayer(sound_player_info) = &object.info else {
        return Err(anyhow::anyhow!("Object {} is not a sound player", sound_player_object_id));
    };
    assert_eq!(sound_player_info.clockId.map(Into::into), expected_clock_id);
    Ok(())
}

pub fn assert_player_controls(world: &World, player_object_id: ObjectId, expected_controlled_object_id: Option<ObjectId>) -> anyhow::Result<()> {
    let object = world.getObject(player_object_id)?;
    let ObjectInfo::Player(player_info) = &object.info else {
        return Err(anyhow::anyhow!("Object {} is not a player", player_object_id));
    };
    assert_eq!(player_info.characterObjectId.map(Into::into), expected_controlled_object_id);
    Ok(())
}

pub fn assert_is_controlled_by_us(world: &World, object_id: ObjectId, expected: bool) -> anyhow::Result<()> {
    let is_controlled_by_us = world
        .getIsObjectControlledByUs(object_id)
        .ok_or_else(|| anyhow::anyhow!("Object not found when checking if controlled by us"))?;
    assert_eq!(is_controlled_by_us, expected, "Unexpected controlledByUs for object {}", object_id);
    Ok(())
}
