mod helpers;

use std::collections::BTreeSet;

use game_types::object_types::{
    ClockState, MeshInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectInfoTrackedFields, ObjectKind, ObjectLocation, VisualSettings,
};
use glam::Vec3;
use helpers::world_test_helpers::{
    add_ball, add_brick, add_clock, add_mesh, add_object_for_kind_for_player_control_tests, add_player, add_sk_animator, add_skeleton,
    assert_is_controlled_by_us, assert_player_controls, make_world, make_world_for_callbacks, set_player_controls_and_run_after_change,
};

fn run_with_current_thread_runtime(run_test: impl FnOnce() -> anyhow::Result<()>) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async { run_test() })?;
    Ok(())
}

#[test]
fn callback_after_add_player_without_existing_control_does_not_auto_spawn_control_tree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let player_object_id = 600;

    let object_ids_before_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
    add_player(&mut world, player_object_id, root_object_id, None)?;
    let object_ids_after_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();

    let added_object_ids = object_ids_after_add.difference(&object_ids_before_add).copied().collect::<Vec<ObjectId>>();

    assert_eq!(
        added_object_ids,
        vec![player_object_id],
        "Adding a player with no controlled object should only add the player. Avatar setup now lives in scripts."
    );
    assert_eq!(world.parentOfId.get(&player_object_id).copied(), Some(root_object_id));
    assert!(
        world.getPlayerCharacterObject(player_object_id).is_none(),
        "Adding a player should not auto-create or auto-assign a controlled object."
    );

    Ok(())
}

#[test]
fn callback_after_add_player_with_existing_control_is_noop_for_spawn() -> anyhow::Result<()> {
    let mut world = make_world_for_callbacks()?;
    let root_object_id = world.getRootObjectId();
    let controlled_ball_object_id = 610;
    let player_object_id = 611;

    add_ball(&mut world, controlled_ball_object_id, root_object_id, Vec3::new(4.0, 2.0, 0.0), true)?;

    let object_ids_before_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
    add_player(&mut world, player_object_id, root_object_id, Some(controlled_ball_object_id))?;
    let object_ids_after_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();

    let added_object_ids = object_ids_after_add.difference(&object_ids_before_add).copied().collect::<Vec<ObjectId>>();

    assert_eq!(
        added_object_ids,
        vec![player_object_id],
        "Player add callback should not spawn avatar objects when characterObjectId is already set"
    );
    assert_player_controls(&world, player_object_id, Some(controlled_ball_object_id))?;

    Ok(())
}

#[test]
fn callback_after_change_player_keeps_allowed_control_target_kinds() -> anyhow::Result<()> {
    let allowed_target_kinds = [ObjectKind::Ball, ObjectKind::Brick, ObjectKind::Cylinder, ObjectKind::RigidGroup];

    for allowed_target_kind in allowed_target_kinds {
        let mut world = make_world_for_callbacks()?;
        let root_object_id = world.getRootObjectId();

        let player_object_id = 620;
        let old_controlled_object_id = 621;
        let player_control_source_object_id = 622;
        let new_controlled_object_id = 623;

        add_ball(&mut world, old_controlled_object_id, root_object_id, Vec3::new(-2.0, 2.0, 0.0), true)?;
        add_ball(&mut world, player_control_source_object_id, root_object_id, Vec3::new(2.0, 2.0, 0.0), true)?;
        add_player(&mut world, player_object_id, root_object_id, Some(old_controlled_object_id))?;
        add_object_for_kind_for_player_control_tests(
            &mut world,
            allowed_target_kind,
            new_controlled_object_id,
            root_object_id,
            player_control_source_object_id,
        )?;

        set_player_controls_and_run_after_change(&mut world, player_object_id, Some(new_controlled_object_id))?;

        assert_player_controls(&world, player_object_id, Some(new_controlled_object_id))?;
        assert_eq!(
            world
                ._objectsThatPointToId
                .get(&new_controlled_object_id)
                .map(|pointer_ids| pointer_ids.contains(&player_object_id)),
            Some(true),
            "Expected player {} to keep control over {:?} target {}",
            player_object_id,
            allowed_target_kind,
            new_controlled_object_id
        );
        assert!(
            !world
                ._objectsThatPointToId
                .get(&old_controlled_object_id)
                .map(|pointer_ids| pointer_ids.contains(&player_object_id))
                .unwrap_or(false),
            "Expected old controlled object {} to no longer be pointed to after switching to {:?}",
            old_controlled_object_id,
            allowed_target_kind
        );
    }

    Ok(())
}

#[test]
fn callback_after_change_player_currently_keeps_previously_disallowed_control_target_kinds() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let disallowed_target_kinds = [
            ObjectKind::Clock,
            ObjectKind::LocalGroup,
            ObjectKind::Material,
            ObjectKind::Mesh,
            ObjectKind::Player,
            ObjectKind::Script,
            ObjectKind::SoundPlayer,
            ObjectKind::Skeleton,
            ObjectKind::SkAnimation,
            ObjectKind::SkAttachment,
            ObjectKind::SkKeyframe,
            ObjectKind::SkKeyframePose,
            ObjectKind::SkAnimator,
            ObjectKind::PropertyAnimator,
            ObjectKind::Tween,
        ];

        for disallowed_target_kind in disallowed_target_kinds {
            let mut world = make_world_for_callbacks()?;
            let root_object_id = world.getRootObjectId();

            let player_object_id = 640;
            let old_controlled_object_id = 641;
            let player_control_source_object_id = 642;
            let new_controlled_object_id = 643;

            add_ball(&mut world, old_controlled_object_id, root_object_id, Vec3::new(-2.0, 2.0, 0.0), true)?;
            add_ball(&mut world, player_control_source_object_id, root_object_id, Vec3::new(2.0, 2.0, 0.0), true)?;
            add_player(&mut world, player_object_id, root_object_id, Some(old_controlled_object_id))?;
            add_object_for_kind_for_player_control_tests(
                &mut world,
                disallowed_target_kind,
                new_controlled_object_id,
                root_object_id,
                player_control_source_object_id,
            )?;

            set_player_controls_and_run_after_change(&mut world, player_object_id, Some(new_controlled_object_id))?;

            assert_player_controls(&world, player_object_id, Some(new_controlled_object_id))?;
            assert_eq!(
                world
                    ._objectsThatPointToId
                    .get(&new_controlled_object_id)
                    .map(|pointer_ids| pointer_ids.contains(&player_object_id)),
                Some(true),
                "Expected player {} to keep pointing to target {:?} {}",
                player_object_id,
                disallowed_target_kind,
                new_controlled_object_id
            );
            assert!(
                !world
                    ._objectsThatPointToId
                    .get(&old_controlled_object_id)
                    .map(|pointer_ids| pointer_ids.contains(&player_object_id))
                    .unwrap_or(false),
                "Expected old controlled object {} to no longer be pointed to after switching to {:?}",
                old_controlled_object_id,
                disallowed_target_kind
            );
            assert_is_controlled_by_us(&world, old_controlled_object_id, true)?;
        }
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

#[test]
fn callback_before_remove_player_without_existing_control_removes_only_player() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let player_object_id = 660;

    add_player(&mut world, player_object_id, root_object_id, None)?;

    world.removeObject(player_object_id)?;

    assert!(!world.objectOfId.contains_key(&player_object_id));
    assert_eq!(world.objectOfId.len(), 1);
    assert!(world.objectOfId.contains_key(&root_object_id));

    Ok(())
}

#[test]
fn callback_before_remove_player_with_existing_control_removes_controlled_target() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let controlled_ball_object_id = 670;
    let player_object_id = 671;

    add_ball(&mut world, controlled_ball_object_id, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_player(&mut world, player_object_id, root_object_id, Some(controlled_ball_object_id))?;

    world.removeObject(player_object_id)?;

    assert!(!world.objectOfId.contains_key(&player_object_id));
    assert!(!world.objectOfId.contains_key(&controlled_ball_object_id));
    assert_eq!(world.objectOfId.len(), 1);
    assert!(world.objectOfId.contains_key(&root_object_id));

    Ok(())
}

#[test]
fn callback_after_add_mesh_without_existing_physical_keeps_mesh_without_spawned_child() -> anyhow::Result<()> {
    run_with_current_thread_runtime(|| {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let mesh_object_id = 680;

        let object_ids_before_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
        add_mesh(&mut world, mesh_object_id, root_object_id)?;
        let object_ids_after_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
        let added_object_ids = object_ids_after_add.difference(&object_ids_before_add).copied().collect::<Vec<ObjectId>>();

        assert_eq!(
            added_object_ids,
            vec![mesh_object_id],
            "Adding a mesh with no physicalObjectId should not auto-spawn a physical child."
        );

        let mesh_object = world.getObject(mesh_object_id)?;
        let ObjectInfo::Mesh(mesh_info) = &mesh_object.info else {
            return Err(anyhow::anyhow!("Expected object {} to be mesh", mesh_object_id));
        };
        let mesh_child_ids = world.getDirectChildrenIds(mesh_object_id)?;
        assert!(mesh_info.physicalObjectId.is_none());
        assert!(mesh_child_ids.is_empty());

        Ok(())
    })
}

#[test]
fn callback_after_add_mesh_with_existing_physical_is_noop_for_spawn() -> anyhow::Result<()> {
    run_with_current_thread_runtime(|| {
        let mut world = make_world_for_callbacks()?;
        let root_object_id = world.getRootObjectId();
        let physical_object_id = 690;
        let mesh_object_id = 691;

        add_brick(&mut world, physical_object_id, root_object_id, Vec3::new(3.0, 1.0, -2.0), false)?;

        let object_ids_before_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
        let mesh_info = MeshInfo {
            scale: Vec3::ONE,
            meshDataId: 777,
            followSkeletonId: None,
            physicalObjectId: Some(ObjectIdPointer(physical_object_id)),
            material: None,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
        };
        world.addObjectWithId(
            mesh_object_id,
            format!("mesh-{mesh_object_id}"),
            ObjectBeingAdded::Mesh(mesh_info),
            root_object_id,
        );
        let object_ids_after_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
        let added_object_ids = object_ids_after_add.difference(&object_ids_before_add).copied().collect::<Vec<ObjectId>>();

        assert_eq!(
            added_object_ids,
            vec![mesh_object_id],
            "Mesh add callback should not spawn a second physical object when physicalObjectId is already set"
        );
        assert_eq!(world.parentOfId.get(&physical_object_id).copied(), Some(root_object_id));
        let mesh_pointer_ids = world
            ._objectsThatPointToId
            .get(&physical_object_id)
            .ok_or_else(|| anyhow::anyhow!("Expected physical object {} to have mesh pointer tracking", physical_object_id))?;
        assert!(mesh_pointer_ids.contains(&mesh_object_id));

        Ok(())
    })
}

#[test]
fn callback_after_add_sk_animator_without_clock_keeps_clock_empty() -> anyhow::Result<()> {
    run_with_current_thread_runtime(|| {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let skeleton_object_id = 680;
        let sk_animator_object_id = 681;

        add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
        let object_ids_before_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
        add_sk_animator(&mut world, sk_animator_object_id, root_object_id)?;
        let object_ids_after_add = world.objectOfId.keys().copied().collect::<BTreeSet<ObjectId>>();
        let added_object_ids = object_ids_after_add.difference(&object_ids_before_add).copied().collect::<Vec<ObjectId>>();

        assert_eq!(
            added_object_ids,
            vec![sk_animator_object_id],
            "Adding a SkAnimator without clockId should not auto-create a clock."
        );

        let sk_animator_object = world.getObject(sk_animator_object_id)?;
        let ObjectInfo::SkAnimator(sk_animator_info) = &sk_animator_object.info else {
            return Err(anyhow::anyhow!("Object {} is not a SkAnimator", sk_animator_object_id));
        };
        assert!(sk_animator_info.clockId.is_none());

        Ok(())
    })
}

#[test]
fn callback_before_remove_mesh_keeps_physical_object_when_mesh_only_points_to_it() -> anyhow::Result<()> {
    run_with_current_thread_runtime(|| {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let physical_object_id = 700;
        let mesh_object_id = 701;

        add_brick(&mut world, physical_object_id, root_object_id, Vec3::new(-4.0, 1.5, 2.0), false)?;

        let mesh_info = MeshInfo {
            scale: Vec3::ONE,
            meshDataId: 888,
            followSkeletonId: None,
            physicalObjectId: Some(ObjectIdPointer(physical_object_id)),
            material: None,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
        };
        world.addObjectWithId(
            mesh_object_id,
            format!("mesh-{mesh_object_id}"),
            ObjectBeingAdded::Mesh(mesh_info),
            root_object_id,
        );

        world.removeObject(mesh_object_id)?;

        assert!(!world.objectOfId.contains_key(&mesh_object_id));
        assert!(world.objectOfId.contains_key(&physical_object_id));
        assert_eq!(world.parentOfId.get(&physical_object_id).copied(), Some(root_object_id));
        let did_clear_mesh_pointer = world
            ._objectsThatPointToId
            .get(&physical_object_id)
            .is_none_or(|mesh_pointer_ids| !mesh_pointer_ids.contains(&mesh_object_id));
        assert!(did_clear_mesh_pointer);

        Ok(())
    })
}

#[test]
fn callback_before_remove_sk_animator_without_clock_only_removes_animation_in_server_mode() -> anyhow::Result<()> {
    run_with_current_thread_runtime(|| {
        let mut world = make_world_for_callbacks()?;
        let root_object_id = world.getRootObjectId();
        let skeleton_object_id = 690;
        let sk_animator_object_id = 691;

        add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
        add_sk_animator(&mut world, sk_animator_object_id, root_object_id)?;

        world.removeObject(sk_animator_object_id)?;

        assert!(!world.objectOfId.contains_key(&sk_animator_object_id));
        assert!(world.objectOfId.contains_key(&skeleton_object_id));
        assert!(world.objectOfId.contains_key(&root_object_id));
        assert_eq!(world.objectOfId.len(), 2);

        Ok(())
    })
}

#[test]
fn callback_after_change_clock_speed_does_not_reset_started_at_for_sk_animator_clock_reference() -> anyhow::Result<()> {
    run_with_current_thread_runtime(|| {
        let mut world = make_world_for_callbacks()?;
        let root_object_id = world.getRootObjectId();
        let skeleton_object_id = 700;
        let sk_animator_object_id = 701;
        let clock_object_id = 702;

        add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
        add_sk_animator(&mut world, sk_animator_object_id, root_object_id)?;
        add_clock(&mut world, clock_object_id, sk_animator_object_id)?;

        let old_sk_animator_tracked_fields = {
            let sk_animator_object = world.getObject(sk_animator_object_id)?;
            ObjectInfoTrackedFields::from_object_info(&sk_animator_object.info)
        };
        {
            let sk_animator_object = world.getObjectMut(sk_animator_object_id)?;
            let ObjectInfo::SkAnimator(sk_animator_info) = &mut sk_animator_object.info else {
                return Err(anyhow::anyhow!("Object {} is not a SkAnimator", sk_animator_object_id));
            };
            sk_animator_info.clockId = Some(ObjectIdPointer(clock_object_id));
        }
        let new_sk_animator_tracked_fields = {
            let sk_animator_object = world.getObject(sk_animator_object_id)?;
            ObjectInfoTrackedFields::from_object_info(&sk_animator_object.info)
        };
        world.afterObjectInfoChange(sk_animator_object_id, old_sk_animator_tracked_fields, new_sk_animator_tracked_fields);

        let old_tracked_fields = {
            let clock_object = world.getObject(clock_object_id)?;
            ObjectInfoTrackedFields::from_object_info(&clock_object.info)
        };

        {
            let clock_object = world.getObjectMut(clock_object_id)?;
            let ObjectInfo::Clock(clock_info) = &mut clock_object.info else {
                return Err(anyhow::anyhow!("Object {} is not a clock", clock_object_id));
            };
            clock_info.clockState = ClockState::Playing {
                localStartMs: 123.0,
                timeLocation: ObjectLocation::Client,
            };
            clock_info.speed = 2.5;
        }

        let new_tracked_fields = {
            let clock_object = world.getObject(clock_object_id)?;
            ObjectInfoTrackedFields::from_object_info(&clock_object.info)
        };

        world.afterObjectInfoChange(clock_object_id, old_tracked_fields, new_tracked_fields);

        let clock_object = world.getObject(clock_object_id)?;
        let ObjectInfo::Clock(clock_info) = &clock_object.info else {
            return Err(anyhow::anyhow!("Object {} is not a clock", clock_object_id));
        };
        assert_eq!(
            clock_info.clockState,
            ClockState::Playing {
                localStartMs: 123.0,
                timeLocation: ObjectLocation::Client,
            }
        );

        Ok(())
    })
}
