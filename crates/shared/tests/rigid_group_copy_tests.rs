mod helpers;

use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_brick, add_rigid_group, make_world};
use shared::game::physics::SetPhysicsInfo3D;

// Copying between an animation update and the physics step must preserve the whole group's pose.
#[test]
fn clone_and_template_preserve_pending_group_translation_and_rotation() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let child_offset = Vec3::new(1.0, 2.0, 3.0);
    add_rigid_group(&mut world, 2, root_id, Vec3::new(10.0, 10.0, 0.0), None)?;
    add_brick(&mut world, 3, 2, Vec3::new(10.0, 10.0, 0.0) + child_offset, false)?;
    world.objectIdCounter = 3;

    let position = Vec3::new(20.0, 15.0, -3.0);
    let rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.3);
    world.setPhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            position: Some(position),
            rotation: Some(rotation),
            ..Default::default()
        },
        false,
    );
    let cloned_id = world.addClonedObjectRecursive(2, "clone".to_string())?;
    world.cutObjectToTemplate(2, "template".to_string())?;
    let pasted_id = world.pasteTemplate("template", root_id)?;

    for should_step in [false, true] {
        if should_step {
            world.step_server(1, 1.0 / 60.0, 1000.0 / 60.0)?;
        }
        for group_id in [cloned_id, pasted_id] {
            let child_id = *world.getDirectChildrenIds(group_id)?.iter().next().expect("Expected child");
            let group = world.getObjectPhysicsInfo3D(group_id).expect("Expected group physics");
            let child = world.getObjectPhysicsInfo3D(child_id).expect("Expected child physics");
            assert!(group.location.position.abs_diff_eq(position, 1e-5));
            assert!(group.location.quaternion.abs_diff_eq(rotation, 1e-5));
            assert!(child.location.position.abs_diff_eq(position + rotation * child_offset, 1e-5));
            assert!(child.location.quaternion.abs_diff_eq(rotation, 1e-5));
        }
    }
    Ok(())
}

#[test]
fn child_world_pose_write_uses_the_pending_group_pose() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_rigid_group(&mut world, 2, root_id, Vec3::ZERO, None)?;
    add_brick(&mut world, 3, 2, Vec3::Y * 2.0, false)?;
    world.setPhysicsInfo3D(
        2,
        SetPhysicsInfo3D {
            position: Some(Vec3::X * 10.0),
            rotation: Some(Quat::from_rotation_y(0.7)),
            ..Default::default()
        },
        false,
    );

    let target_position = Vec3::new(11.0, 4.0, 3.0);
    let target_rotation = Quat::from_rotation_z(0.4);
    world.forcePhysicsInfo3D(
        3,
        SetPhysicsInfo3D {
            position: Some(target_position),
            rotation: Some(target_rotation),
            ..Default::default()
        },
        false,
    );
    for should_step in [false, true] {
        if should_step {
            world.step_server(1, 1.0 / 60.0, 1000.0 / 60.0)?;
        }
        let child = world.getObjectPhysicsInfo3D(3).expect("Expected child physics");
        assert!(child.location.position.abs_diff_eq(target_position, 1e-5));
        assert!(child.location.quaternion.abs_diff_eq(target_rotation, 1e-5));
    }
    Ok(())
}
