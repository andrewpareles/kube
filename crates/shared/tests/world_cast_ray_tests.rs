mod helpers;

use game_types::consts::DEFAULT_MAX_GROUND_SLOPE_DEGREES;
use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
use glam::Vec3;
use helpers::world_test_helpers::{add_brick, add_cylinder, add_rigid_group, make_world};
use shared::game::physics::{CastHit, SetPhysicsToggles3D};

#[test]
fn cast_ray_can_include_walkthrough_colliders_when_requested() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let walkthrough_object_id = 10;
    let solid_object_id = 11;

    // Put a walkthrough object in front of a solid object so the two ray modes return different hits.
    add_brick(&mut world, walkthrough_object_id, root_object_id, Vec3::ZERO, false)?;
    world.setPhysicsToggles3D(
        walkthrough_object_id,
        SetPhysicsToggles3D {
            isWalkthrough: Some(true),
            ..Default::default()
        },
        true,
    );
    add_brick(&mut world, solid_object_id, root_object_id, Vec3::new(4.0, 0.0, 0.0), false)?;
    let _ = world.physics.step(0.0);

    let ray_origin = Vec3::new(-4.0, 0.0, 0.0);
    let ray_direction = Vec3::X;
    let ray_length = 20.0;
    let should_exclude_hit_object = |_| false;

    let should_include_walkthrough = false;
    let Some(CastHit {
        object_id: solid_hit_object_id,
        ..
    }) = world.castRay(ray_origin, ray_direction, ray_length, should_include_walkthrough, &should_exclude_hit_object)
    else {
        panic!("solid-only ray should hit the solid object behind the walkthrough object");
    };
    assert_eq!(solid_hit_object_id, solid_object_id);

    let should_include_walkthrough = true;
    let Some(CastHit {
        object_id: walkthrough_hit_object_id,
        ..
    }) = world.castRay(ray_origin, ray_direction, ray_length, should_include_walkthrough, &should_exclude_hit_object)
    else {
        panic!("walkthrough-including ray should hit the walkthrough object");
    };
    assert_eq!(walkthrough_hit_object_id, walkthrough_object_id);

    Ok(())
}

#[test]
fn cast_ray_still_hits_brick_after_a_negative_size_is_refused() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let brick_object_id = 12;

    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    let Err(error) = world.setPrimitiveSettings(
        brick_object_id,
        PrimitiveSettings::Brick(BrickPrimitiveSettings {
            size: Vec3::new(-2.0, -2.0, -2.0),
        }),
        true,
    ) else {
        panic!("a negative brick size should be refused");
    };
    assert!(error.to_string().contains("Invalid shape dimensions"), "unexpected error: {error}");
    let _ = world.physics.step(0.0);

    let ray_origin = Vec3::new(-4.0, 0.0, 0.0);
    let ray_direction = Vec3::X;
    let should_include_walkthrough = true;
    let should_exclude_hit_object = |_| false;
    let Some(CastHit { object_id: hit_object_id, .. }) = world.castRay(ray_origin, ray_direction, 20.0, should_include_walkthrough, &should_exclude_hit_object)
    else {
        panic!("ray should still hit the brick at its old size");
    };

    assert_eq!(hit_object_id, brick_object_id);
    Ok(())
}

#[test]
fn ground_support_cast_does_not_panic_for_rigid_group_with_cylinder_child() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ground_object_id = 20;
    let rigid_group_object_id = 21;
    let cylinder_object_id = 22;

    add_brick(&mut world, ground_object_id, root_object_id, Vec3::new(0.0, -1.5, 0.0), false)?;
    world.setPrimitiveSettings(
        ground_object_id,
        PrimitiveSettings::Brick(BrickPrimitiveSettings {
            size: Vec3::new(20.0, 1.0, 20.0),
        }),
        true,
    )?;
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(0.0, 2.0, 0.0), None)?;
    add_cylinder(&mut world, cylinder_object_id, rigid_group_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    let _ = world.physics.step(0.0);

    let support_hit = world.getGroundSupport(rigid_group_object_id, 4.0, DEFAULT_MAX_GROUND_SLOPE_DEGREES);
    assert!(support_hit.is_some(), "rigid group cylinder child should be shape-castable");

    Ok(())
}
