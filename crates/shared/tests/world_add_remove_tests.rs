mod helpers;

use std::collections::HashMap;

use game_types::object_types::sk_animator::SkAnimatorInfo;
use game_types::object_types::{
    BallMaterialSettings, CameraLocation, InterpolationStyle, LiveObject, LocalGroupInfo, Location3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo,
    ObjectInfoTrackedFields, ObjectLocation, Orientation3D, ScriptInfo, SkAnimationInfo, SkKeyframeInfo, SkKeyframePoseInfo,
};
use game_types::{consts::STORED_WORLD_CURRENT_VERSION, stored_world_type::StoredWorld};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::*;
use shared::game::world::{World, WorldControlMode};

#[test]
fn add_object_tracks_parent_children_without_self_cycles() -> anyhow::Result<()> {
    let mut world = make_world_for_callbacks()?;
    let root_object_id = world.getRootObjectId();
    let child_local_group_object_id = 191;

    add_local_group(&mut world, child_local_group_object_id, root_object_id)?;

    let root_children_ids = world.getDirectChildrenIds(root_object_id)?;
    assert!(root_children_ids.contains(&child_local_group_object_id));
    let child_children_ids = world.getDirectChildrenIds(child_local_group_object_id)?;
    assert!(
        !child_children_ids.contains(&child_local_group_object_id),
        "Object {} should never be listed as its own child",
        child_local_group_object_id
    );
    assert_eq!(world.parentOfId.get(&child_local_group_object_id).copied(), Some(root_object_id));

    Ok(())
}

#[test]
fn set_parent_returns_err_when_cycle_would_be_created_in_any_world_mode() -> anyhow::Result<()> {
    fn assert_cycle_is_rejected(world: &mut World) -> anyhow::Result<()> {
        let root_object_id = world.getRootObjectId();
        let ball_object_id = 193;
        let brick_object_id = 194;

        add_ball(world, ball_object_id, root_object_id, Vec3::new(1.0, 1.0, 1.0), true)?;
        add_brick(world, brick_object_id, root_object_id, Vec3::new(2.0, 2.0, 2.0), true)?;

        world.setParent(ball_object_id, brick_object_id)?;
        let result = world.setParent(brick_object_id, ball_object_id);
        assert!(result.is_err(), "Cycle-causing parent changes should be rejected.");
        Ok(())
    }

    let mut server_world = make_world()?;
    assert_cycle_is_rejected(&mut server_world)?;

    let mut player_world = make_world_controlled_by_player(999)?;
    assert_cycle_is_rejected(&mut player_world)?;

    Ok(())
}

#[test]
fn from_stored_world_string_terminates_when_children_contain_cycle() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let child_local_group_object_id = 196;
    add_local_group(&mut world, child_local_group_object_id, root_object_id)?;

    let mut stored_world_value: serde_json::Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    let children_of_id = stored_world_value
        .get_mut("childrenOfId")
        .and_then(|value| value.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("childrenOfId was missing or malformed"))?;
    children_of_id.insert(child_local_group_object_id.to_string(), serde_json::json!([root_object_id]));

    let stored_world_string_with_cycle = serde_json::to_string(&stored_world_value)?;
    let Err(error) = World::fromStoredWorldString(&stored_world_string_with_cycle, WorldControlMode::Server, None) else {
        panic!("A world whose hierarchy loops is refused");
    };
    assert!(format!("{error:#}").contains("appears more than once"));

    Ok(())
}

#[test]
fn stored_world_round_trip_keeps_fallback_camera_location() -> anyhow::Result<()> {
    fn assert_fallback_camera_location_eq(actual: CameraLocation, expected: CameraLocation) {
        assert_eq!(actual.position, expected.position);
        assert!((actual.orientation.theta - expected.orientation.theta).abs() <= f32::EPSILON);
        assert!((actual.orientation.phi - expected.orientation.phi).abs() <= f32::EPSILON);
    }

    let mut world = make_world()?;
    let fallback_camera_location = CameraLocation {
        position: Vec3::new(4.0, 5.0, 6.0),
        orientation: Orientation3D { theta: 0.25, phi: -0.5 },
    };
    world.fallback_camera_location = fallback_camera_location;
    let stored_world_string = world.toStoredWorldString()?;
    let stored_world = serde_json::from_str::<StoredWorld>(&stored_world_string)?;
    assert_fallback_camera_location_eq(stored_world.fallback_camera_location, fallback_camera_location);

    let _loaded_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;

    Ok(())
}

#[test]
fn from_stored_world_string_keeps_server_and_client_scripts_loaded_and_queued_in_server_and_player_worlds() -> anyhow::Result<()> {
    let root_object_id = 1;
    let client_script_object_id = 2;
    let server_script_object_id = 3;
    let client_script_raw_code = "console.log('local script is running and says hello');".to_string();
    let server_script_raw_code = "console.log('server script is running and says hello');".to_string();

    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: fallback_camera_location_for_tests(),
        objectOfId: HashMap::from([
            (
                root_object_id,
                LiveObject {
                    id: root_object_id,
                    name: "root".to_string(),
                    info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
                },
            ),
            (
                client_script_object_id,
                LiveObject {
                    id: client_script_object_id,
                    name: "client_hello_script".to_string(),
                    info: ObjectInfo::Script(ScriptInfo {
                        objectLocation: ObjectLocation::Client,
                    }),
                },
            ),
            (
                server_script_object_id,
                LiveObject {
                    id: server_script_object_id,
                    name: "server_hello_script".to_string(),
                    info: ObjectInfo::Script(ScriptInfo {
                        objectLocation: ObjectLocation::Server,
                    }),
                },
            ),
        ]),
        rootObjectId: root_object_id,
        childrenOfId: HashMap::from([
            (root_object_id, vec![client_script_object_id, server_script_object_id]),
            (client_script_object_id, Vec::new()),
            (server_script_object_id, Vec::new()),
        ]),
        scriptRawCodeOfId: HashMap::from([
            (client_script_object_id, client_script_raw_code.clone()),
            (server_script_object_id, server_script_raw_code.clone()),
        ]),
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };

    let stored_world_string = serde_json::to_string(&stored_world)?;
    let server_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;
    let player_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Player(999), None)?;

    let assert_loaded_scripts = |world: &World| -> anyhow::Result<()> {
        match &world.getObject(client_script_object_id)?.info {
            ObjectInfo::Script(script_info) => {
                assert_eq!(script_info.objectLocation, ObjectLocation::Client);
            }
            other_object_info => {
                panic!("Expected client script object, got {other_object_info:?}");
            }
        }
        match &world.getObject(server_script_object_id)?.info {
            ObjectInfo::Script(script_info) => {
                assert_eq!(script_info.objectLocation, ObjectLocation::Server);
            }
            other_object_info => {
                panic!("Expected server script object, got {other_object_info:?}");
            }
        }

        assert_eq!(
            world.scriptRawCodeOfId.get(&client_script_object_id).map(String::as_str),
            Some(client_script_raw_code.as_str())
        );
        assert_eq!(
            world.scriptRawCodeOfId.get(&server_script_object_id).map(String::as_str),
            Some(server_script_raw_code.as_str())
        );
        assert_eq!(world.readyToAddScriptIds.len(), 2);
        assert!(
            world.readyToAddScriptIds.contains(&client_script_object_id),
            "Client scripts should stay queued everywhere so the runtime can filter them by world mode."
        );
        assert!(
            world.readyToAddScriptIds.contains(&server_script_object_id),
            "Server scripts should stay queued everywhere so the runtime can filter them by world mode."
        );

        Ok(())
    };

    assert_loaded_scripts(&server_world)?;
    assert_loaded_scripts(&player_world)?;

    Ok(())
}

#[test]
fn template_cut_paste_remaps_internal_pointers_and_does_not_persist() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let source_group_object_id = 210;
    let source_material_object_id = 211;
    let source_ball_object_id = 212;
    let source_script_object_id = 213;

    add_local_group(&mut world, source_group_object_id, root_object_id)?;
    add_material(&mut world, source_material_object_id, source_group_object_id)?;
    add_ball(&mut world, source_ball_object_id, source_group_object_id, Vec3::new(4.0, 5.0, 6.0), false)?;
    add_script(&mut world, source_script_object_id, source_group_object_id)?;

    let old_tracked_fields = {
        let ball_object = world.getObject(source_ball_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&ball_object.info)
    };
    {
        let ball_object = world.getObjectMut(source_ball_object_id)?;
        let ObjectInfo::Ball(ball_info) = &mut ball_object.info else {
            return Err(anyhow::anyhow!("Expected source ball"));
        };
        ball_info.material = Some(BallMaterialSettings {
            materialInfoId: ObjectIdPointer(source_material_object_id),
            tileSizeU: 2.0,
            tileSizeV: 3.0,
            tileOffsetU: 0.0,
            tileOffsetV: 0.0,
        });
    }
    let new_tracked_fields = {
        let ball_object = world.getObject(source_ball_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&ball_object.info)
    };
    world.afterObjectInfoChange(source_ball_object_id, old_tracked_fields, new_tracked_fields);

    world.cutObjectToTemplate(source_group_object_id, "crate".to_string())?;
    assert!(!world.hasObject(source_group_object_id));
    assert!(!world.hasObject(source_material_object_id));
    assert!(!world.hasObject(source_ball_object_id));
    assert!(!world.hasObject(source_script_object_id));

    let pasted_group_object_id = world.pasteTemplate("crate", root_object_id)?;
    let pasted_children = world.getDirectChildrenIds(pasted_group_object_id)?.iter().copied().collect::<Vec<_>>();
    let pasted_material_object_id = pasted_children
        .iter()
        .copied()
        .find(|object_id| {
            world
                .getObject(*object_id)
                .map(|object| object.name.as_str() == "material-211")
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow::anyhow!("Expected pasted material"))?;
    let pasted_ball_object_id = pasted_children
        .iter()
        .copied()
        .find(|object_id| world.getObject(*object_id).map(|object| object.name.as_str() == "ball-212").unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("Expected pasted ball"))?;
    let pasted_script_object_id = pasted_children
        .iter()
        .copied()
        .find(|object_id| world.getObject(*object_id).map(|object| object.name.as_str() == "script-213").unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("Expected pasted script"))?;

    let pasted_ball_object = world.getObject(pasted_ball_object_id)?;
    let ObjectInfo::Ball(pasted_ball_info) = &pasted_ball_object.info else {
        return Err(anyhow::anyhow!("Expected pasted ball"));
    };
    let pasted_ball_material = pasted_ball_info.material.ok_or_else(|| anyhow::anyhow!("Expected pasted ball material"))?;
    assert_eq!(ObjectId::from(pasted_ball_material.materialInfoId), pasted_material_object_id);
    assert_eq!(world.scriptRawCodeOfId.get(&pasted_script_object_id).map(String::as_str), Some("return 213;"));

    let stored_world_string = world.toStoredWorldString()?;
    assert!(!stored_world_string.contains("templateOfName"));
    let mut loaded_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;
    let loaded_root_object_id = loaded_world.getRootObjectId();
    assert!(loaded_world.templateOfName.is_empty());
    assert!(loaded_world.pasteTemplate("crate", loaded_root_object_id).is_err());

    Ok(())
}

#[test]
fn render_sk_keyframe_cache_updates_when_sk_keyframe_pose_changes() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let skeleton_object_id = 1501;
    let clock_object_id = 1502;
    let sk_animation_object_id = 1503;
    let sk_animator_object_id = 1504;
    let sk_keyframe_object_id = 1505;
    let sk_keyframe_pose_object_id = 1506;

    add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    world.addObjectWithId(
        sk_animation_object_id,
        "joint-animation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Step,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        sk_animator_object_id,
        "joint-animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(ObjectIdPointer(skeleton_object_id)),
            clockId: Some(ObjectIdPointer(clock_object_id)),
            skAnimationId: Some(ObjectIdPointer(sk_animation_object_id)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        sk_keyframe_object_id,
        "joint-keyframe".to_string(),
        ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time: 0.0 }),
        sk_animation_object_id,
    );
    world.addObjectWithId(
        sk_keyframe_pose_object_id,
        "joint-pose".to_string(),
        ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
            jointName: "root".to_string(),
            localPose: Location3D {
                position: Vec3::new(1.0, 2.0, 3.0),
                quaternion: Quat::IDENTITY,
            },
        }),
        sk_keyframe_object_id,
    );

    let render_sk_animations = world.collectRenderSkAnimationsForSkeleton(skeleton_object_id);
    assert_eq!(render_sk_animations.len(), 1);
    assert_eq!(render_sk_animations[0].skKeyframes.len(), 1);
    assert_eq!(
        render_sk_animations[0].skKeyframes[0].skKeyframePoses[0].localPose.position,
        Vec3::new(1.0, 2.0, 3.0)
    );

    let old_tracked_fields = {
        let sk_keyframe_pose_object = world.getObject(sk_keyframe_pose_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&sk_keyframe_pose_object.info)
    };
    {
        let sk_keyframe_pose_object = world.getObjectMut(sk_keyframe_pose_object_id)?;
        let ObjectInfo::SkKeyframePose(sk_keyframe_pose_info) = &mut sk_keyframe_pose_object.info else {
            return Err(anyhow::anyhow!("Expected SkKeyframePose object"));
        };
        sk_keyframe_pose_info.localPose.position = Vec3::new(4.0, 5.0, 6.0);
    }
    let new_tracked_fields = {
        let sk_keyframe_pose_object = world.getObject(sk_keyframe_pose_object_id)?;
        ObjectInfoTrackedFields::from_object_info(&sk_keyframe_pose_object.info)
    };
    world.afterObjectInfoChange(sk_keyframe_pose_object_id, old_tracked_fields, new_tracked_fields);

    let render_sk_animations = world.collectRenderSkAnimationsForSkeleton(skeleton_object_id);
    assert_eq!(
        render_sk_animations[0].skKeyframes[0].skKeyframePoses[0].localPose.position,
        Vec3::new(4.0, 5.0, 6.0)
    );

    world.removeObject(sk_keyframe_pose_object_id)?;
    let render_sk_animations = world.collectRenderSkAnimationsForSkeleton(skeleton_object_id);
    assert!(render_sk_animations[0].skKeyframes[0].skKeyframePoses.is_empty());

    Ok(())
}

#[test]
fn pointer_tracking_converges_when_targets_are_added_out_of_order() -> anyhow::Result<()> {
    let skeleton_object_id = 300;
    let clock_object_id = 301;
    let rigid_group_object_id = 302;
    let sound_player_object_id = 303;
    let sk_attachment_object_id = 304;
    let attached_brick_object_id = 305;

    let mut targets_first_world = make_world()?;
    let mut refs_first_world = make_world()?;
    let targets_first_root_id = targets_first_world.getRootObjectId();
    let refs_first_root_id = refs_first_world.getRootObjectId();

    add_skeleton(&mut targets_first_world, skeleton_object_id, targets_first_root_id)?;
    add_clock(&mut targets_first_world, clock_object_id, targets_first_root_id)?;
    add_rigid_group(
        &mut targets_first_world,
        rigid_group_object_id,
        targets_first_root_id,
        Vec3::new(3.0, 3.0, 3.0),
        Some(skeleton_object_id),
    )?;
    add_brick(
        &mut targets_first_world,
        attached_brick_object_id,
        targets_first_root_id,
        Vec3::new(4.0, 2.0, 1.0),
        false,
    )?;
    add_sound_player(&mut targets_first_world, sound_player_object_id, targets_first_root_id, Some(clock_object_id))?;
    add_sk_attachment(
        &mut targets_first_world,
        sk_attachment_object_id,
        targets_first_root_id,
        Some(attached_brick_object_id),
    )?;

    add_sk_attachment(
        &mut refs_first_world,
        sk_attachment_object_id,
        refs_first_root_id,
        Some(attached_brick_object_id),
    )?;
    add_sound_player(&mut refs_first_world, sound_player_object_id, refs_first_root_id, Some(clock_object_id))?;
    add_rigid_group(
        &mut refs_first_world,
        rigid_group_object_id,
        refs_first_root_id,
        Vec3::new(3.0, 3.0, 3.0),
        Some(skeleton_object_id),
    )?;
    add_clock(&mut refs_first_world, clock_object_id, refs_first_root_id)?;
    add_skeleton(&mut refs_first_world, skeleton_object_id, refs_first_root_id)?;
    add_brick(
        &mut refs_first_world,
        attached_brick_object_id,
        refs_first_root_id,
        Vec3::new(4.0, 2.0, 1.0),
        false,
    )?;

    assert_eq!(
        ComparableWorldState::from(&targets_first_world)?,
        ComparableWorldState::from(&refs_first_world)?
    );
    assert_rigid_group_follow_skeleton(&targets_first_world, rigid_group_object_id, Some(skeleton_object_id))?;
    assert_sound_player_clock(&targets_first_world, sound_player_object_id, Some(clock_object_id))?;
    assert_eq!(
        targets_first_world
            ._objectsThatPointToId
            .get(&attached_brick_object_id)
            .map(|pointer_ids| pointer_ids.contains(&sk_attachment_object_id)),
        Some(true)
    );
    assert_rigid_group_follow_skeleton(&refs_first_world, rigid_group_object_id, Some(skeleton_object_id))?;
    assert_sound_player_clock(&refs_first_world, sound_player_object_id, Some(clock_object_id))?;
    assert_eq!(
        refs_first_world
            ._objectsThatPointToId
            .get(&attached_brick_object_id)
            .map(|pointer_ids| pointer_ids.contains(&sk_attachment_object_id)),
        Some(true)
    );

    Ok(())
}

#[test]
fn removing_pointer_targets_in_different_orders_converges() -> anyhow::Result<()> {
    let skeleton_object_id = 320;
    let clock_object_id = 321;
    let rigid_group_object_id = 322;
    let sound_player_object_id = 323;
    let sk_attachment_object_id = 324;
    let attached_brick_object_id = 325;

    let mut world_a = make_world()?;
    let mut world_b = make_world()?;
    let world_a_root_id = world_a.getRootObjectId();
    let world_b_root_id = world_b.getRootObjectId();

    add_skeleton(&mut world_a, skeleton_object_id, world_a_root_id)?;
    add_clock(&mut world_a, clock_object_id, world_a_root_id)?;
    add_rigid_group(
        &mut world_a,
        rigid_group_object_id,
        world_a_root_id,
        Vec3::new(2.0, 2.0, 2.0),
        Some(skeleton_object_id),
    )?;
    add_sound_player(&mut world_a, sound_player_object_id, world_a_root_id, Some(clock_object_id))?;
    add_brick(&mut world_a, attached_brick_object_id, world_a_root_id, Vec3::new(1.0, 2.0, 3.0), false)?;
    add_sk_attachment(&mut world_a, sk_attachment_object_id, world_a_root_id, Some(attached_brick_object_id))?;

    add_rigid_group(
        &mut world_b,
        rigid_group_object_id,
        world_b_root_id,
        Vec3::new(2.0, 2.0, 2.0),
        Some(skeleton_object_id),
    )?;
    add_sound_player(&mut world_b, sound_player_object_id, world_b_root_id, Some(clock_object_id))?;
    add_sk_attachment(&mut world_b, sk_attachment_object_id, world_b_root_id, Some(attached_brick_object_id))?;
    add_brick(&mut world_b, attached_brick_object_id, world_b_root_id, Vec3::new(1.0, 2.0, 3.0), false)?;
    add_clock(&mut world_b, clock_object_id, world_b_root_id)?;
    add_skeleton(&mut world_b, skeleton_object_id, world_b_root_id)?;

    world_a.removeObject(attached_brick_object_id)?;
    world_a.removeObject(skeleton_object_id)?;
    world_a.removeObject(clock_object_id)?;

    world_b.removeObject(clock_object_id)?;
    world_b.removeObject(skeleton_object_id)?;
    world_b.removeObject(attached_brick_object_id)?;

    assert_eq!(ComparableWorldState::from(&world_a)?, ComparableWorldState::from(&world_b)?);
    assert_rigid_group_follow_skeleton(&world_a, rigid_group_object_id, None)?;
    assert_sound_player_clock(&world_a, sound_player_object_id, None)?;
    assert_rigid_group_follow_skeleton(&world_b, rigid_group_object_id, None)?;
    assert_sound_player_clock(&world_b, sound_player_object_id, None)?;
    let ObjectInfo::SkAttachment(world_a_sk_attachment_info) = &world_a.getObject(sk_attachment_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected joint-attachment in world_a"));
    };
    assert!(world_a_sk_attachment_info.objectId.is_none());
    let ObjectInfo::SkAttachment(world_b_sk_attachment_info) = &world_b.getObject(sk_attachment_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected joint-attachment in world_b"));
    };
    assert!(world_b_sk_attachment_info.objectId.is_none());

    Ok(())
}

#[test]
fn controlled_by_us_for_standalone_targets_converges_when_player_and_target_are_added_out_of_order() -> anyhow::Result<()> {
    let player_object_id = 510;
    let controlled_ball_object_id = 511;
    let uncontrolled_ball_object_id = 512;

    let mut targets_first_world = make_world()?;
    let mut player_first_world = make_world()?;
    let targets_first_root_id = targets_first_world.getRootObjectId();
    let player_first_root_id = player_first_world.getRootObjectId();

    add_ball(
        &mut targets_first_world,
        controlled_ball_object_id,
        targets_first_root_id,
        Vec3::new(0.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut targets_first_world,
        uncontrolled_ball_object_id,
        targets_first_root_id,
        Vec3::new(4.0, 2.0, 0.0),
        true,
    )?;
    add_player(
        &mut targets_first_world,
        player_object_id,
        targets_first_root_id,
        Some(controlled_ball_object_id),
    )?;

    add_player(&mut player_first_world, player_object_id, player_first_root_id, Some(controlled_ball_object_id))?;
    add_ball(
        &mut player_first_world,
        uncontrolled_ball_object_id,
        player_first_root_id,
        Vec3::new(4.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut player_first_world,
        controlled_ball_object_id,
        player_first_root_id,
        Vec3::new(0.0, 2.0, 0.0),
        true,
    )?;

    assert_eq!(
        ComparableWorldState::from(&targets_first_world)?,
        ComparableWorldState::from(&player_first_world)?
    );
    assert_player_controls(&targets_first_world, player_object_id, Some(controlled_ball_object_id))?;
    assert_player_controls(&player_first_world, player_object_id, Some(controlled_ball_object_id))?;
    assert_is_controlled_by_us(&targets_first_world, controlled_ball_object_id, false)?;
    assert_is_controlled_by_us(&player_first_world, controlled_ball_object_id, false)?;
    assert_is_controlled_by_us(&targets_first_world, uncontrolled_ball_object_id, true)?;
    assert_is_controlled_by_us(&player_first_world, uncontrolled_ball_object_id, true)?;

    Ok(())
}

#[test]
fn controlled_by_us_releases_after_controlled_target_is_removed_and_readded_in_different_orders() -> anyhow::Result<()> {
    let player_object_id = 520;
    let controlled_ball_object_id = 521;

    let mut world_a = make_world()?;
    let mut world_b = make_world()?;
    let world_a_root_id = world_a.getRootObjectId();
    let world_b_root_id = world_b.getRootObjectId();

    add_ball(&mut world_a, controlled_ball_object_id, world_a_root_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_player(&mut world_a, player_object_id, world_a_root_id, Some(controlled_ball_object_id))?;
    world_a.removeObject(controlled_ball_object_id)?;
    add_ball(&mut world_a, controlled_ball_object_id, world_a_root_id, Vec3::new(0.0, 2.0, 0.0), true)?;

    add_player(&mut world_b, player_object_id, world_b_root_id, Some(controlled_ball_object_id))?;
    add_ball(&mut world_b, controlled_ball_object_id, world_b_root_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    world_b.removeObject(controlled_ball_object_id)?;
    add_ball(&mut world_b, controlled_ball_object_id, world_b_root_id, Vec3::new(0.0, 2.0, 0.0), true)?;

    assert_eq!(ComparableWorldState::from(&world_a)?, ComparableWorldState::from(&world_b)?);
    assert_player_controls(&world_a, player_object_id, None)?;
    assert_player_controls(&world_b, player_object_id, None)?;
    assert_is_controlled_by_us(&world_a, controlled_ball_object_id, true)?;
    assert_is_controlled_by_us(&world_b, controlled_ball_object_id, true)?;

    Ok(())
}

#[test]
fn controlled_by_us_client_mode_converges_when_local_player_and_target_are_added_out_of_order() -> anyhow::Result<()> {
    let local_player_object_id = 530;
    let local_target_object_id = 531;
    let non_local_target_object_id = 532;

    let mut targets_first_world = make_world_controlled_by_player(local_player_object_id)?;
    let mut player_first_world = make_world_controlled_by_player(local_player_object_id)?;
    let targets_first_root_id = targets_first_world.getRootObjectId();
    let player_first_root_id = player_first_world.getRootObjectId();

    add_ball(
        &mut targets_first_world,
        local_target_object_id,
        targets_first_root_id,
        Vec3::new(1.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut targets_first_world,
        non_local_target_object_id,
        targets_first_root_id,
        Vec3::new(5.0, 2.0, 0.0),
        true,
    )?;
    add_player(
        &mut targets_first_world,
        local_player_object_id,
        targets_first_root_id,
        Some(local_target_object_id),
    )?;

    add_player(
        &mut player_first_world,
        local_player_object_id,
        player_first_root_id,
        Some(local_target_object_id),
    )?;
    add_ball(
        &mut player_first_world,
        non_local_target_object_id,
        player_first_root_id,
        Vec3::new(5.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut player_first_world,
        local_target_object_id,
        player_first_root_id,
        Vec3::new(1.0, 2.0, 0.0),
        true,
    )?;

    assert_eq!(
        ComparableWorldState::from(&targets_first_world)?,
        ComparableWorldState::from(&player_first_world)?
    );
    assert_is_controlled_by_us(&targets_first_world, local_target_object_id, true)?;
    assert_is_controlled_by_us(&player_first_world, local_target_object_id, true)?;
    assert_is_controlled_by_us(&targets_first_world, non_local_target_object_id, false)?;
    assert_is_controlled_by_us(&player_first_world, non_local_target_object_id, false)?;

    Ok(())
}

#[test]
fn controlled_by_us_server_mode_with_multiple_players_and_targets_converges_across_add_orders() -> anyhow::Result<()> {
    let player_a_object_id = 550;
    let player_b_object_id = 551;
    let target_a_ball_object_id = 552;
    let target_b_ball_object_id = 553;
    let free_ball_object_id = 554;

    let mut targets_first_world = make_world()?;
    let mut players_first_world = make_world()?;
    let targets_first_root_id = targets_first_world.getRootObjectId();
    let players_first_root_id = players_first_world.getRootObjectId();

    add_ball(
        &mut targets_first_world,
        target_a_ball_object_id,
        targets_first_root_id,
        Vec3::new(-3.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut targets_first_world,
        target_b_ball_object_id,
        targets_first_root_id,
        Vec3::new(3.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut targets_first_world,
        free_ball_object_id,
        targets_first_root_id,
        Vec3::new(0.0, 2.0, 0.0),
        true,
    )?;
    add_player(
        &mut targets_first_world,
        player_a_object_id,
        targets_first_root_id,
        Some(target_a_ball_object_id),
    )?;
    add_player(
        &mut targets_first_world,
        player_b_object_id,
        targets_first_root_id,
        Some(target_b_ball_object_id),
    )?;

    add_player(
        &mut players_first_world,
        player_b_object_id,
        players_first_root_id,
        Some(target_b_ball_object_id),
    )?;
    add_ball(
        &mut players_first_world,
        free_ball_object_id,
        players_first_root_id,
        Vec3::new(0.0, 2.0, 0.0),
        true,
    )?;
    add_ball(
        &mut players_first_world,
        target_b_ball_object_id,
        players_first_root_id,
        Vec3::new(3.0, 2.0, 0.0),
        true,
    )?;
    add_player(
        &mut players_first_world,
        player_a_object_id,
        players_first_root_id,
        Some(target_a_ball_object_id),
    )?;
    add_ball(
        &mut players_first_world,
        target_a_ball_object_id,
        players_first_root_id,
        Vec3::new(-3.0, 2.0, 0.0),
        true,
    )?;

    assert_eq!(
        ComparableWorldState::from(&targets_first_world)?,
        ComparableWorldState::from(&players_first_world)?
    );
    assert_is_controlled_by_us(&targets_first_world, target_a_ball_object_id, false)?;
    assert_is_controlled_by_us(&targets_first_world, target_b_ball_object_id, false)?;
    assert_is_controlled_by_us(&targets_first_world, free_ball_object_id, true)?;
    assert_is_controlled_by_us(&players_first_world, target_a_ball_object_id, false)?;
    assert_is_controlled_by_us(&players_first_world, target_b_ball_object_id, false)?;
    assert_is_controlled_by_us(&players_first_world, free_ball_object_id, true)?;

    Ok(())
}
