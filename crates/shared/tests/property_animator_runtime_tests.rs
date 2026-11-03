use render_core::{
    computePropertyValue::{
        ComputedPropertyAnimation, ComputedPropertyOverrides, applyPropertyOverridesToRenderObjectOfId, computePropertyOverridesOfObjectIdAtTime,
    },
    renderable::Renderable,
    renderer_types::{RenderBall, RenderObject, RenderTransform},
};
mod helpers;

use std::{collections::HashMap, sync::Arc};

use game_types::{
    consts::SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK,
    object_types::{
        BallMaterialSettings, ClockState, InterpolationStyle, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PhysicsInfo3D,
        PropertyAnimationInfo, PropertyAnimatorInfo, PropertyKeyframeInfo, VisualSettings,
    },
    physics_primitive_types::{BallPrimitiveSettings, PrimitiveSettings},
};
use glam::{EulerRot, Quat, Vec3};
use helpers::world_test_helpers::{
    add_ball, add_brick, add_clock, add_cylinder, add_label, add_local_group, add_pivot, add_property_animator, add_rigid_group, make_world,
};
use shared::game::interpolate_physics::interpolate_between_physics_info_3Ds_hermite;
use shared::game::physics::SetOrForce;
use shared::game::world::{World, WorldControlMode};

fn property_path(path: &[&str]) -> Vec<String> {
    path.iter().map(|path_part| path_part.to_string()).collect()
}

// None of these cases animate rotation, so they need no rotation base: the base only ever supplies the axes an
// animator is not driving, and it is only consulted when some rotation axis is actually being animated.
fn no_rotation_bases() -> HashMap<ObjectId, Vec3> {
    HashMap::new()
}

fn configure_property_animator(
    world: &mut shared::game::world::World,
    property_animator_object_id: ObjectId,
    target_object_id: ObjectId,
    clock_object_id: ObjectId,
    path: &[&str],
    start_value: f32,
    end_value: f32,
) -> anyhow::Result<()> {
    configure_property_animator_keyframes(
        world,
        property_animator_object_id,
        target_object_id,
        clock_object_id,
        path,
        InterpolationStyle::Linear,
        vec![
            PropertyKeyframeInfo { time: 0.0, value: start_value },
            PropertyKeyframeInfo { time: 1.0, value: end_value },
        ],
    )
}

fn configure_property_animator_keyframes(
    world: &mut shared::game::world::World,
    property_animator_object_id: ObjectId,
    target_object_id: ObjectId,
    clock_object_id: ObjectId,
    path: &[&str],
    interpolation_style: InterpolationStyle,
    keyframes: Vec<PropertyKeyframeInfo>,
) -> anyhow::Result<()> {
    let property_animator_object = world.getObjectMut(property_animator_object_id)?;
    let ObjectInfo::PropertyAnimator(property_animator) = &mut property_animator_object.info else {
        return Err(anyhow::anyhow!("Expected property animator object"));
    };
    property_animator.targetObjectId = Some(ObjectIdPointer(target_object_id));
    property_animator.propertyPath = property_path(path);
    property_animator.clockId = Some(ObjectIdPointer(clock_object_id));
    property_animator.priority = 5;
    property_animator.weight = 100.0;

    let root_object_id = world.getRootObjectId();
    // these tests assign object ids by hand, so keep the animation objects in their own id range
    let property_animation_id = property_animator_object_id * 1000;
    world.addObjectWithId(
        property_animation_id,
        format!("property-animation-{property_animator_object_id}"),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: interpolation_style,
        }),
        root_object_id,
    );
    for (keyframe_index, keyframe) in keyframes.into_iter().enumerate() {
        world.addObjectWithId(
            property_animation_id + 1 + keyframe_index as ObjectId,
            format!("property-keyframe-{property_animator_object_id}-{keyframe_index}"),
            ObjectBeingAdded::PropertyKeyframe(keyframe),
            property_animation_id,
        );
    }

    let property_animator_object = world.getObjectMut(property_animator_object_id)?;
    let ObjectInfo::PropertyAnimator(property_animator) = &mut property_animator_object.info else {
        return Err(anyhow::anyhow!("Expected property animator object"));
    };
    property_animator.propertyAnimationId = Some(ObjectIdPointer(property_animation_id));
    Ok(())
}

fn computed_property_animation(
    interpolation_style: InterpolationStyle,
    keyframes: Vec<PropertyKeyframeInfo>,
    should_loop: bool,
    speed: f32,
    weight: f32,
) -> ComputedPropertyAnimation {
    ComputedPropertyAnimation {
        property_keyframes_arc: Arc::new(keyframes),
        duration: 1.0,
        interpolationStyle: interpolation_style,
        clock_state: ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Client,
        },
        should_loop,
        speed,
        priority: 5,
        weight,
        weight_transition: None,
    }
}

fn computed_property_overrides(
    path: &[&str],
    interpolation_style: InterpolationStyle,
    keyframes: Vec<PropertyKeyframeInfo>,
    should_loop: bool,
    speed: f32,
    weight: f32,
) -> ComputedPropertyOverrides {
    ComputedPropertyOverrides {
        computed_property_animations: vec![(
            property_path(path),
            vec![computed_property_animation(interpolation_style, keyframes, should_loop, speed, weight)],
        )],
    }
}

fn property_override_value(
    property_override_of_path_of_object_id: &HashMap<ObjectId, HashMap<Vec<String>, f32>>,
    object_id: ObjectId,
    path: &[&str],
) -> anyhow::Result<f32> {
    let property_path = property_path(path);
    property_override_of_path_of_object_id
        .get(&object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected property overrides for object {}", object_id))?
        .get(&property_path)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Expected property override for {}", property_path.join(".")))
}

fn assert_f32_near(label: &str, actual: f32, expected: f32) {
    let difference = (actual - expected).abs();
    assert!(difference <= 1e-5, "{} mismatch: actual {}, expected {}", label, actual, expected);
}

fn render_ball_object(position: Vec3, material: Option<BallMaterialSettings>) -> RenderObject {
    RenderObject::Ball(RenderBall {
        transform: RenderTransform {
            teleport_count: 0,
            physicsInfo3D: PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            rigid_group_to_follow: None,
            sk_joint_to_follow_arc: None,
        },
        visualSettings: VisualSettings::TYPICAL_OPAQUE,
        material,
        primitive: BallPrimitiveSettings { radius: 1.0 },
    })
}

fn apply_property_animator_overrides_to_render_object_at_time(
    render_object_of_id: &mut HashMap<ObjectId, RenderObject>,
    computed_property_overrides_of_object_id: &HashMap<ObjectId, ComputedPropertyOverrides>,
    wall_ms: f64,
) {
    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(computed_property_overrides_of_object_id, &no_rotation_bases(), wall_ms, Some(wall_ms));
    applyPropertyOverridesToRenderObjectOfId(render_object_of_id, &property_override_of_path_of_object_id, &HashMap::new());
}

#[test]
fn apply_property_overrides_mutates_render_object_fields() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let mut render_object_of_id = HashMap::from([(
        ball_object_id,
        RenderObject::Ball(RenderBall {
            transform: RenderTransform {
                teleport_count: 0,
                physicsInfo3D: PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: Some(BallMaterialSettings {
                materialInfoId: ObjectIdPointer(9),
                tileSizeU: 1.0,
                tileSizeV: 2.0,
                tileOffsetU: 0.1,
                tileOffsetV: 0.2,
            }),
            primitive: BallPrimitiveSettings { radius: 1.0 },
        }),
    )]);
    let property_override_of_path_of_object_id = HashMap::from([(
        ball_object_id,
        HashMap::from([
            (property_path(&["opacity"]), 0.5),
            (property_path(&["color", "r"]), 0.25),
            (property_path(&["location", "position", "x"]), 10.0),
            (property_path(&["location", "rotation", "y"]), 90.0),
            (property_path(&["radius"]), 3.0),
            (property_path(&["material", "tileSizeU"]), 4.0),
            (property_path(&["material", "tileOffsetV"]), 0.75),
        ]),
    )]);

    applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &property_override_of_path_of_object_id, &HashMap::new());

    let Some(RenderObject::Ball(ball)) = render_object_of_id.get(&ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball render object"));
    };
    assert!((ball.visualSettings.opacity - 0.5).abs() <= 1e-5);
    assert!((ball.visualSettings.color.r - 0.25).abs() <= 1e-5);
    assert!((ball.transform.physicsInfo3D.location.position.x - 10.0).abs() <= 1e-5);
    let (_, rotation_y, _) = ball.transform.physicsInfo3D.location.quaternion.to_euler(EulerRot::XYZ);
    assert!((rotation_y.to_degrees() - 90.0).abs() <= 1e-4);
    assert!((ball.primitive.radius - 3.0).abs() <= 1e-5);
    let Some(material) = ball.material.as_ref() else {
        return Err(anyhow::anyhow!("Expected ball material"));
    };
    assert!((material.tileSizeU - 4.0).abs() <= 1e-5);
    assert!((material.tileSizeV - 2.0).abs() <= 1e-5);
    assert!((material.tileOffsetU - 0.1).abs() <= 1e-5);
    assert!((material.tileOffsetV - 0.75).abs() <= 1e-5);
    Ok(())
}

#[test]
fn apply_property_overrides_ignores_non_render_paths() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let mut render_object_of_id = HashMap::from([(
        ball_object_id,
        RenderObject::Ball(RenderBall {
            transform: RenderTransform {
                teleport_count: 0,
                physicsInfo3D: PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                rigid_group_to_follow: None,
                sk_joint_to_follow_arc: None,
            },
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
            material: None,
            primitive: BallPrimitiveSettings { radius: 1.0 },
        }),
    )]);
    let property_override_of_path_of_object_id = HashMap::from([(
        ball_object_id,
        HashMap::from([(property_path(&["density"]), 4.0), (property_path(&["movement", "velocity", "x"]), 10.0)]),
    )]);

    applyPropertyOverridesToRenderObjectOfId(&mut render_object_of_id, &property_override_of_path_of_object_id, &HashMap::new());

    let Some(RenderObject::Ball(ball)) = render_object_of_id.get(&ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball render object"));
    };
    assert!((ball.primitive.radius - 1.0).abs() <= 1e-5);
    assert!((ball.transform.physicsInfo3D.location.position.x - 0.0).abs() <= 1e-5);
    assert!((ball.visualSettings.opacity - VisualSettings::TYPICAL_OPAQUE.opacity).abs() <= 1e-5);
    Ok(())
}

#[test]
fn apply_property_animations_use_animation_value_without_base_value() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        ball_object_id,
        computed_property_overrides(
            &["opacity"],
            InterpolationStyle::Linear,
            vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 1.0, value: 1.0 }],
            false,
            1.0,
            50.0,
        ),
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));

    let opacity = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["opacity"])?;
    let difference = (opacity - 0.5).abs();
    assert!(difference <= 1e-5, "opacity mismatch: {}", opacity);
    Ok(())
}

#[test]
fn step_property_animation_holds_value_until_next_keyframe() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        ball_object_id,
        computed_property_overrides(
            &["opacity"],
            InterpolationStyle::Step,
            vec![PropertyKeyframeInfo { time: 0.0, value: 0.2 }, PropertyKeyframeInfo { time: 1.0, value: 0.8 }],
            false,
            1.0,
            100.0,
        ),
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));
    let opacity = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["opacity"])?;
    assert!((opacity - 0.2).abs() <= 1e-5, "opacity mismatch: {}", opacity);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 1000.0, Some(1000.0));
    let opacity = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["opacity"])?;
    assert!((opacity - 0.8).abs() <= 1e-5, "opacity mismatch: {}", opacity);
    Ok(())
}

#[test]
fn looping_property_animation_wraps_negative_speed() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        ball_object_id,
        computed_property_overrides(
            &["opacity"],
            InterpolationStyle::Linear,
            vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 1.0, value: 1.0 }],
            true,
            -1.0,
            100.0,
        ),
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 250.0, Some(250.0));

    let opacity = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["opacity"])?;
    let difference = (opacity - 0.75).abs();
    assert!(difference <= 1e-5, "opacity mismatch: {}", opacity);
    Ok(())
}

#[test]
fn apply_property_animations_updates_render_location_path() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        ball_object_id,
        computed_property_overrides(
            &["location", "position", "x"],
            InterpolationStyle::Linear,
            vec![PropertyKeyframeInfo { time: 0.0, value: 20.0 }, PropertyKeyframeInfo { time: 1.0, value: 30.0 }],
            false,
            1.0,
            100.0,
        ),
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));

    let position_x = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["location", "position", "x"])?;
    assert!((position_x - 25.0).abs() <= 1e-5, "position.x mismatch: {}", position_x);
    Ok(())
}

#[test]
fn apply_property_animations_updates_render_rotation_path_in_degrees() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        ball_object_id,
        computed_property_overrides(
            &["location", "rotation", "y"],
            InterpolationStyle::Linear,
            vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 1.0, value: 90.0 }],
            false,
            1.0,
            100.0,
        ),
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));

    let rotation_y = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["location", "rotation", "y"])?;
    assert!((rotation_y - 45.0).abs() <= 1e-4, "rotation.y mismatch: {}", rotation_y);
    Ok(())
}

#[test]
fn apply_property_animations_updates_render_material_tile_path() -> anyhow::Result<()> {
    let ball_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        ball_object_id,
        ComputedPropertyOverrides {
            computed_property_animations: vec![
                (
                    property_path(&["material", "tileSizeU"]),
                    vec![computed_property_animation(
                        InterpolationStyle::Linear,
                        vec![PropertyKeyframeInfo { time: 0.0, value: 4.0 }, PropertyKeyframeInfo { time: 1.0, value: 8.0 }],
                        false,
                        1.0,
                        100.0,
                    )],
                ),
                (
                    property_path(&["material", "tileOffsetV"]),
                    vec![computed_property_animation(
                        InterpolationStyle::Linear,
                        vec![PropertyKeyframeInfo { time: 0.0, value: -0.5 }, PropertyKeyframeInfo { time: 1.0, value: 0.5 }],
                        false,
                        1.0,
                        100.0,
                    )],
                ),
            ],
        },
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));

    let tile_size_u = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["material", "tileSizeU"])?;
    assert!((tile_size_u - 6.0).abs() <= 1e-5, "tileSizeU mismatch: {}", tile_size_u);
    let tile_offset_v = property_override_value(&property_override_of_path_of_object_id, ball_object_id, &["material", "tileOffsetV"])?;
    assert!((tile_offset_v - 0.0).abs() <= 1e-5, "tileOffsetV mismatch: {}", tile_offset_v);
    Ok(())
}

#[test]
fn apply_property_animations_updates_world_lighting_sun_orientation_in_degrees() -> anyhow::Result<()> {
    let world_lighting_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        world_lighting_object_id,
        ComputedPropertyOverrides {
            computed_property_animations: vec![
                (
                    property_path(&["sunOrientation", "theta"]),
                    vec![computed_property_animation(
                        InterpolationStyle::Linear,
                        vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 1.0, value: 90.0 }],
                        false,
                        1.0,
                        100.0,
                    )],
                ),
                (
                    property_path(&["sunOrientation", "phi"]),
                    vec![computed_property_animation(
                        InterpolationStyle::Linear,
                        vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 1.0, value: 30.0 }],
                        false,
                        1.0,
                        100.0,
                    )],
                ),
            ],
        },
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));

    let theta = property_override_value(&property_override_of_path_of_object_id, world_lighting_object_id, &["sunOrientation", "theta"])?;
    let phi = property_override_value(&property_override_of_path_of_object_id, world_lighting_object_id, &["sunOrientation", "phi"])?;
    assert!((theta - 45.0).abs() <= 1e-4, "sunOrientation.theta mismatch: {}", theta);
    assert!((phi - 15.0).abs() <= 1e-4, "sunOrientation.phi mismatch: {}", phi);
    Ok(())
}

#[test]
fn apply_property_animations_updates_nested_mesh_scale_path() -> anyhow::Result<()> {
    let mesh_object_id = 2;
    let computed_property_overrides_of_object_id = HashMap::from([(
        mesh_object_id,
        computed_property_overrides(
            &["scale", "x"],
            InterpolationStyle::Linear,
            vec![PropertyKeyframeInfo { time: 0.0, value: 4.0 }, PropertyKeyframeInfo { time: 1.0, value: 8.0 }],
            false,
            1.0,
            100.0,
        ),
    )]);

    let property_override_of_path_of_object_id =
        computePropertyOverridesOfObjectIdAtTime(&computed_property_overrides_of_object_id, &no_rotation_bases(), 500.0, Some(500.0));

    let scale_x = property_override_value(&property_override_of_path_of_object_id, mesh_object_id, &["scale", "x"])?;
    assert!((scale_x - 6.0).abs() <= 1e-5, "scale.x mismatch: {}", scale_x);
    Ok(())
}

#[test]
fn property_animator_bobs_render_position_y_smoothly() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator_keyframes(
        &mut world,
        property_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["location", "position", "y"],
        InterpolationStyle::EaseInOut,
        vec![
            PropertyKeyframeInfo { time: 0.0, value: 0.0 },
            PropertyKeyframeInfo { time: 0.5, value: 2.0 },
            PropertyKeyframeInfo { time: 1.0, value: 0.0 },
        ],
    )?;
    let computed_property_overrides_of_object_id = world.collectComputedPropertyOverridesOfObjectId();
    let mut render_object_of_id = HashMap::from([(ball_object_id, render_ball_object(Vec3::ZERO, None))]);
    let samples = [
        (0.0, 0.0),
        (125.0, 0.25),
        (250.0, 1.0),
        (375.0, 1.75),
        (500.0, 2.0),
        (625.0, 1.75),
        (750.0, 1.0),
        (875.0, 0.25),
        (1000.0, 0.0),
        (1125.0, 0.25),
    ];

    for (wall_ms, expected_position_y) in samples {
        apply_property_animator_overrides_to_render_object_at_time(&mut render_object_of_id, &computed_property_overrides_of_object_id, wall_ms);

        let Some(RenderObject::Ball(ball)) = render_object_of_id.get(&ball_object_id) else {
            return Err(anyhow::anyhow!("Expected ball render object"));
        };
        assert_f32_near("position.y", ball.transform.physicsInfo3D.location.position.y, expected_position_y);
    }
    Ok(())
}

#[test]
fn property_animator_changes_render_material_tile_size_u_v_smoothly() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let tile_size_u_animator_object_id = 4;
    let tile_size_v_animator_object_id = 5;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, tile_size_u_animator_object_id, root_object_id)?;
    add_property_animator(&mut world, tile_size_v_animator_object_id, root_object_id)?;
    {
        let clock_object = world.getObjectMut(clock_object_id)?;
        let ObjectInfo::Clock(clock) = &mut clock_object.info else {
            return Err(anyhow::anyhow!("Expected clock object"));
        };
        clock.shouldLoop = false;
    }
    configure_property_animator_keyframes(
        &mut world,
        tile_size_u_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["material", "tileSizeU"],
        InterpolationStyle::EaseInOut,
        vec![PropertyKeyframeInfo { time: 0.0, value: 1.0 }, PropertyKeyframeInfo { time: 1.0, value: 5.0 }],
    )?;
    configure_property_animator_keyframes(
        &mut world,
        tile_size_v_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["material", "tileSizeV"],
        InterpolationStyle::EaseInOut,
        vec![PropertyKeyframeInfo { time: 0.0, value: 2.0 }, PropertyKeyframeInfo { time: 1.0, value: 10.0 }],
    )?;
    let computed_property_overrides_of_object_id = world.collectComputedPropertyOverridesOfObjectId();
    let mut render_object_of_id = HashMap::from([(
        ball_object_id,
        render_ball_object(
            Vec3::ZERO,
            Some(BallMaterialSettings {
                materialInfoId: ObjectIdPointer(99),
                tileSizeU: 1.0,
                tileSizeV: 2.0,
                tileOffsetU: 0.0,
                tileOffsetV: 0.0,
            }),
        ),
    )]);
    let samples = [(0.0, 1.0, 2.0), (250.0, 1.5, 3.0), (500.0, 3.0, 6.0), (750.0, 4.5, 9.0), (1000.0, 5.0, 10.0)];

    for (wall_ms, expected_tile_size_u, expected_tile_size_v) in samples {
        apply_property_animator_overrides_to_render_object_at_time(&mut render_object_of_id, &computed_property_overrides_of_object_id, wall_ms);

        let Some(RenderObject::Ball(ball)) = render_object_of_id.get(&ball_object_id) else {
            return Err(anyhow::anyhow!("Expected ball render object"));
        };
        let Some(material) = ball.material.as_ref() else {
            return Err(anyhow::anyhow!("Expected ball material"));
        };
        assert_f32_near("tileSizeU", material.tileSizeU, expected_tile_size_u);
        assert_f32_near("tileSizeV", material.tileSizeV, expected_tile_size_v);
    }
    Ok(())
}

#[test]
fn property_animator_updates_world_movement_using_object_update_path() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, true)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    {
        let clock_object = world.getObjectMut(clock_object_id)?;
        let ObjectInfo::Clock(clock) = &mut clock_object.info else {
            return Err(anyhow::anyhow!("Expected clock object"));
        };
        clock.clockState = ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Server,
        };
    }
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["movement", "angularVelocity", "y"],
        0.0,
        180.0,
    )?;

    world.step_server(0, 0.0, 500.0)?;

    let Some(physics_info) = world.physics.getObjectPhysicsInfo3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics info"));
    };
    assert!(
        (physics_info.movement.angularVelocity.y.to_degrees() - 90.0).abs() <= 1e-4,
        "angularVelocity.y mismatch: {}",
        physics_info.movement.angularVelocity.y.to_degrees()
    );
    Ok(())
}

#[test]
fn property_animator_updates_density_and_primitive_settings_without_dirty_replication() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let brick_object_id = 3;
    let cylinder_object_id = 4;
    let clock_object_id = 5;
    let ball_density_animator_object_id = 6;
    let ball_radius_animator_object_id = 7;
    let brick_size_animator_object_id = 8;
    let cylinder_length_animator_object_id = 9;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, true)?;
    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, true)?;
    add_cylinder(&mut world, cylinder_object_id, root_object_id, Vec3::ZERO, true)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, ball_density_animator_object_id, root_object_id)?;
    add_property_animator(&mut world, ball_radius_animator_object_id, root_object_id)?;
    add_property_animator(&mut world, brick_size_animator_object_id, root_object_id)?;
    add_property_animator(&mut world, cylinder_length_animator_object_id, root_object_id)?;
    {
        let clock_object = world.getObjectMut(clock_object_id)?;
        let ObjectInfo::Clock(clock) = &mut clock_object.info else {
            return Err(anyhow::anyhow!("Expected clock object"));
        };
        clock.clockState = ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Server,
        };
    }
    configure_property_animator(
        &mut world,
        ball_density_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["density"],
        2.0,
        4.0,
    )?;
    configure_property_animator(
        &mut world,
        ball_radius_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["radius"],
        3.0,
        5.0,
    )?;
    configure_property_animator(
        &mut world,
        brick_size_animator_object_id,
        brick_object_id,
        clock_object_id,
        &["size", "y"],
        4.0,
        8.0,
    )?;
    configure_property_animator(
        &mut world,
        cylinder_length_animator_object_id,
        cylinder_object_id,
        clock_object_id,
        &["length"],
        6.0,
        10.0,
    )?;
    let _ = world.flushDirtyObjects();

    world.step_server(0, 0.0, 500.0)?;

    let Some(ball_physics_toggles) = world.physics.getObjectPhysicsToggles3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics toggles"));
    };
    assert!(
        (ball_physics_toggles.density - 3.0).abs() <= 1e-5,
        "density mismatch: {}",
        ball_physics_toggles.density
    );
    let Some(PrimitiveSettings::Ball(ball_primitive_settings)) = world.physics.getPrimitiveSettings(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball primitive settings"));
    };
    assert!(
        (ball_primitive_settings.radius - 4.0).abs() <= 1e-5,
        "radius mismatch: {}",
        ball_primitive_settings.radius
    );
    let Some(PrimitiveSettings::Brick(brick_primitive_settings)) = world.physics.getPrimitiveSettings(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick primitive settings"));
    };
    assert!(
        (brick_primitive_settings.size.y - 6.0).abs() <= 1e-5,
        "size.y mismatch: {}",
        brick_primitive_settings.size.y
    );
    let Some(PrimitiveSettings::Cylinder(cylinder_primitive_settings)) = world.physics.getPrimitiveSettings(cylinder_object_id) else {
        return Err(anyhow::anyhow!("Expected cylinder primitive settings"));
    };
    assert!(
        (cylinder_primitive_settings.length - 8.0).abs() <= 1e-5,
        "length mismatch: {}",
        cylinder_primitive_settings.length
    );

    let dirty_objects = world.flushDirtyObjects();
    for object_id in [ball_object_id, brick_object_id, cylinder_object_id] {
        let Some(modified_object_flags) = dirty_objects.modifiedObjectFlags.get(&object_id) else {
            continue;
        };
        assert!(!modified_object_flags.modifiedPhysicsInfo3D);
        assert!(!modified_object_flags.modifiedPhysicsToggles3D);
        assert!(!modified_object_flags.modifiedPrimitiveSettings);
    }
    Ok(())
}

#[test]
fn property_animator_moves_physics_without_dirty_replication() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    // Starts where the animation starts. From the origin the first step would have to cover the whole 15 units at
    // once, and rapier caps the velocity it derives for a kinematic body at 400 units/s
    // (IntegrationParameters::normalized_max_linear_velocity), so the derived value would saturate instead of
    // describing the motion. Starting on the curve keeps the step to 5 units, which is inside the cap.
    add_ball(&mut world, ball_object_id, root_object_id, Vec3::new(10.0, 0.0, 0.0), false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    {
        let clock_object = world.getObjectMut(clock_object_id)?;
        let ObjectInfo::Clock(clock) = &mut clock_object.info else {
            return Err(anyhow::anyhow!("Expected clock object"));
        };
        clock.clockState = ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Server,
        };
    }
    configure_property_animator_keyframes(
        &mut world,
        property_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["location", "position", "x"],
        InterpolationStyle::Linear,
        vec![PropertyKeyframeInfo { time: 0.0, value: 10.0 }, PropertyKeyframeInfo { time: 1.0, value: 20.0 }],
    )?;

    let computed_property_overrides_of_object_id = world.collectComputedPropertyOverridesOfObjectId();
    assert_eq!(computed_property_overrides_of_object_id.len(), 1);
    let Some(computed_property_overrides) = computed_property_overrides_of_object_id.get(&ball_object_id) else {
        return Err(anyhow::anyhow!("Expected computed property overrides"));
    };
    assert_eq!(computed_property_overrides.computed_property_animations.len(), 1);
    assert_eq!(computed_property_overrides.computed_property_animations[0].1.len(), 1);
    world.step_server(1, 1.0 / 60.0, 500.0)?;

    let Some(physics_info) = world.physics.getObjectPhysicsInfo3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics info"));
    };
    assert!(
        (physics_info.location.position.x - 15.0).abs() <= 1e-5,
        "position.x mismatch: {}",
        physics_info.location.position.x
    );
    // one step of 1/60s covering 10 -> 15
    assert!(
        (physics_info.movement.velocity.x - 5.0 * 60.0).abs() <= 1e-3,
        "velocity.x mismatch: {}",
        physics_info.movement.velocity.x
    );

    let dirty_objects = world.flushDirtyObjects();
    let Some(modified_object_flags) = dirty_objects.modifiedObjectFlags.get(&ball_object_id) else {
        return Ok(());
    };
    assert!(!modified_object_flags.modifiedPhysicsInfo3D);
    Ok(())
}

// Animators write inside the step loop before physics steps, so one failed write keeps the whole world from stepping.
#[test]
fn an_animator_on_one_axis_of_a_label_offset_lets_physics_step() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let label_object_id = 3;
    let clock_object_id = 4;
    let property_animator_object_id = 5;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, true)?;
    add_label(&mut world, label_object_id, root_object_id)?;
    let ObjectInfo::Label(label_info) = &mut world.getObjectMut(label_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected label object"));
    };
    label_info.physicalObjectId = Some(ObjectIdPointer(ball_object_id));
    label_info.offsetFromObject = Vec3::new(1.0, 2.0, 3.0);
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        label_object_id,
        clock_object_id,
        &["offsetFromObject", "x"],
        0.0,
        10.0,
    )?;

    world.step_server(1, 1.0 / 60.0, 500.0)?;

    let ObjectInfo::Label(label_info) = &world.getObject(label_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected label object"));
    };
    assert_vec3_is(label_info.offsetFromObject, Vec3::new(5.0, 2.0, 3.0), "label.offsetFromObject");
    let Some(ball_physics_info) = world.physics.getObjectPhysicsInfo3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics info"));
    };
    assert!(
        ball_physics_info.movement.velocity.y < 0.0,
        "the ball should have started to fall: {:?}",
        ball_physics_info.movement.velocity
    );
    Ok(())
}

// A group has no location and a ball has no rest pose, so those two animators can't write. Only they are skipped: the
// world still steps, and the brick's animator still plays.
#[test]
fn an_animator_whose_target_cannot_take_its_path_is_skipped() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let brick_object_id = 3;
    let local_group_object_id = 4;
    let clock_object_id = 5;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, true)?;
    add_brick(&mut world, brick_object_id, root_object_id, Vec3::new(10.0, 0.0, 0.0), false)?;
    add_local_group(&mut world, local_group_object_id, root_object_id)?;
    add_clock_playing_once(&mut world, clock_object_id)?;
    for (property_animator_object_id, target_object_id, path) in [
        (6, brick_object_id, ["location", "position", "x"]),
        (7, local_group_object_id, ["location", "position", "y"]),
        (8, ball_object_id, ["restPose", "rotation", "y"]),
    ] {
        add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
        configure_property_animator(&mut world, property_animator_object_id, target_object_id, clock_object_id, &path, 10.0, 20.0)?;
    }

    world.step_server(1, 1.0 / 60.0, 500.0)?;

    let Some(brick_physics_info) = world.physics.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };
    assert_vec3_is(brick_physics_info.location.position, Vec3::new(15.0, 0.0, 0.0), "brick position");
    let Some(ball_physics_info) = world.physics.getObjectPhysicsInfo3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics info"));
    };
    assert!(
        ball_physics_info.movement.velocity.y < 0.0,
        "the ball should have started to fall: {:?}",
        ball_physics_info.movement.velocity
    );
    Ok(())
}

// Builds an unpushable (so kinematic) brick spun by a property animator, the same setup the spinning-platform
// worlds use: 360 degrees of yaw over 6 seconds, driven by a looping server clock.
fn make_world_with_spinning_brick(brick_object_id: ObjectId, clock_object_id: ObjectId, property_animator_object_id: ObjectId) -> anyhow::Result<World> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;

    let clock_object = world.getObjectMut(clock_object_id)?;
    let ObjectInfo::Clock(clock) = &mut clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    clock.clockState = ClockState::Playing {
        localStartMs: 0.0,
        timeLocation: ObjectLocation::Server,
    };

    configure_property_animator_keyframes(
        &mut world,
        property_animator_object_id,
        brick_object_id,
        clock_object_id,
        &["location", "rotation", "y"],
        InterpolationStyle::Linear,
        vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 6.0, value: 360.0 }],
    )?;

    Ok(world)
}

// A catch-up batch has to sample the animation once per step, not once for the whole batch. Rapier derives a
// kinematic body's velocity from how far it moves during a step, so one sample for the batch put all of the
// motion on the first step and left the rest deriving nothing - and the renderer, which extrapolates
// orientation from that derived velocity, threw animated platforms into wild poses on slow frames.
#[test]
fn animated_platform_catch_up_batch_matches_stepping_one_at_a_time() -> anyhow::Result<()> {
    let brick_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    let dt_secs = 1.0 / 60.0;
    let dt_ms = dt_secs * 1000.0;
    let num_steps = 4;
    let batch_end_ms = 500.0;

    // one step per tick, the way a machine keeping up runs
    let mut steady_world = make_world_with_spinning_brick(brick_object_id, clock_object_id, property_animator_object_id)?;
    for step_index in 0..num_steps {
        let step_ms = batch_end_ms - ((num_steps - 1 - step_index) as f32 * dt_ms) as f64;
        steady_world.step_server(1, dt_secs, step_ms)?;
    }
    let Some(steady_physics_info) = steady_world.physics.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };

    // the whole backlog in one tick, the way a stalled machine catches up
    let mut catch_up_world = make_world_with_spinning_brick(brick_object_id, clock_object_id, property_animator_object_id)?;
    catch_up_world.step_server(num_steps, dt_secs, batch_end_ms)?;
    let Some(catch_up_physics_info) = catch_up_world.physics.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };

    // the batch used to end its last step already sitting on its target, deriving no velocity at all
    assert!(
        catch_up_physics_info.movement.angularVelocity.length() > 1e-3,
        "catch-up batch derived no angular velocity: {:?}",
        catch_up_physics_info.movement.angularVelocity
    );

    assert_f32_near(
        "angularVelocity.y",
        catch_up_physics_info.movement.angularVelocity.y,
        steady_physics_info.movement.angularVelocity.y,
    );
    // compared as a dot product rather than an angle, because angle_between is acos near 1 and loses most of its precision there
    let pose_alignment = catch_up_physics_info.location.quaternion.dot(steady_physics_info.location.quaternion).abs();
    assert!(
        (1.0 - pose_alignment) <= 1e-6,
        "catch-up batch ended at a different pose than stepping one at a time: alignment {}",
        pose_alignment
    );

    Ok(())
}

// The end-to-end version of the fix, driven through a real world and rapier rather than hand-built inputs.
//
// A stall long enough to hit the step cap discards its backlog and jumps the world clock forward (see
// get_num_physics_steps_to_take), so the tick that follows advances the platform by far more than one step's worth.
// What must not happen is the pose moving while the velocity derived alongside it says otherwise: everything
// downstream reads that velocity as a description of the motion, and the renderer extrapolates orientation from it.
// Sampling the animators once for the whole batch used to leave the pose 30 degrees further on with a derived
// velocity of exactly zero, which bowed the rendered platform 7.5 degrees off the arc between the two poses.
#[test]
fn a_stalled_tick_keeps_the_derived_velocity_consistent_with_the_pose() -> anyhow::Result<()> {
    let brick_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    let dt_secs = 1.0 / 60.0;
    let dt_ms = dt_secs * 1000.0;
    let expected_radians_per_sec = 60.0_f32.to_radians();

    let mut world = make_world_with_spinning_brick(brick_object_id, clock_object_id, property_animator_object_id)?;

    // a machine that is keeping up
    let mut tick_ms = 0.0;
    for _ in 0..10 {
        tick_ms += dt_ms as f64;
        world.step_server(1, dt_secs, tick_ms)?;
    }
    let Some(before_stall) = world.physics.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };

    // half a second of nothing, then the capped catch-up tick that follows it
    let stall_ms = 500.0;
    tick_ms += stall_ms as f64;
    world.step_server(SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK, dt_secs, tick_ms)?;
    let Some(after_stall) = world.physics.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };

    // the pose is allowed to jump - the world clock really did skip - but the velocity has to still describe it
    assert_f32_near(
        "angularVelocity.y after a stall",
        after_stall.movement.angularVelocity.y,
        expected_radians_per_sec,
    );

    // and the renderer, which predicts orientation from that velocity, has to stay on the arc between the two poses.
    // the span is the wall time between the two snapshots, which during a stall is the length of the stall.
    let snapshot_span_secs = stall_ms / 1000.0;
    for step in 0..=10 {
        let alpha = step as f32 / 10.0;
        let rendered = interpolate_between_physics_info_3Ds_hermite(&before_stall, &after_stall, snapshot_span_secs, alpha);
        let on_the_arc = before_stall.location.quaternion.slerp(after_stall.location.quaternion, alpha);
        let alignment = rendered.location.quaternion.dot(on_the_arc).abs();
        assert!(
            (1.0 - alignment) <= 1e-6,
            "alpha {}: the rendered pose left the arc between the two snapshots, alignment {}",
            alpha,
            alignment
        );
    }
    Ok(())
}

// A stored world rewires every object pointer on load, so the animator's propertyAnimationId has to survive
// the round trip and the keyframe children have to rebuild their cache.
#[test]
fn property_animation_objects_survive_a_stored_world_round_trip() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator_keyframes(
        &mut world,
        property_animator_object_id,
        ball_object_id,
        clock_object_id,
        &["opacity"],
        InterpolationStyle::EaseInOut,
        vec![PropertyKeyframeInfo { time: 0.0, value: 0.0 }, PropertyKeyframeInfo { time: 1.0, value: 1.0 }],
    )?;

    let stored_world_string = world.toStoredWorldString()?;
    let reloaded_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;

    let mut num_animators = 0;
    for live_object in reloaded_world.objectOfId.values() {
        let ObjectInfo::PropertyAnimator(property_animator) = &live_object.info else {
            continue;
        };
        num_animators += 1;
        let property_animation_id = property_animator
            .propertyAnimationId
            .as_ref()
            .map(|pointer| pointer.0)
            .ok_or_else(|| anyhow::anyhow!("Expected the reloaded animator to still point at a PropertyAnimation"))?;
        let ObjectInfo::PropertyAnimation(_) = &reloaded_world.getObject(property_animation_id)?.info else {
            return Err(anyhow::anyhow!("Expected object {property_animation_id} to be a PropertyAnimation"));
        };
        let keyframes = reloaded_world
            .propertyKeyframesArcOfPropertyAnimationId
            .get(&property_animation_id)
            .ok_or_else(|| anyhow::anyhow!("Expected the reloaded keyframe cache to be populated"))?;
        assert_eq!(keyframes.len(), 2);
    }
    assert_eq!(num_animators, 1);

    assert!(
        !reloaded_world.collectComputedPropertyOverridesOfObjectId().is_empty(),
        "Expected the reloaded animator to still produce property overrides"
    );
    Ok(())
}

// Deleting a PropertyAnimation must null out every animator pointing at it, the same way deleting a
// SkAnimation nulls skAnimationId. Otherwise the animator keeps a dangling id that a later object could reuse.
#[test]
fn deleting_a_property_animation_clears_the_animator_pointer() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;
    let property_animation_id = 4000;

    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    world.addObjectWithId(
        property_animation_id,
        "animation".to_string(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root_object_id,
    );
    world.addObjectWithId(
        property_animation_id + 1,
        "start".to_string(),
        ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 0.0, value: 10.0 }),
        property_animation_id,
    );

    // create the animator with the pointer already in its info, the way a script or a loaded world does
    world.addObjectWithId(
        property_animator_object_id,
        "animator".to_string(),
        ObjectBeingAdded::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: Some(ObjectIdPointer(ball_object_id)),
            propertyPath: property_path(&["location", "position", "x"]),
            propertyAnimationId: Some(ObjectIdPointer(property_animation_id)),
            clockId: Some(ObjectIdPointer(clock_object_id)),
            priority: 5,
            weight: 100.0,
            weightTransition: None,
        }),
        root_object_id,
    );
    assert_eq!(world.collectComputedPropertyOverridesOfObjectId().len(), 1);

    world.removeObject(property_animation_id)?;

    let ObjectInfo::PropertyAnimator(property_animator) = &world.getObject(property_animator_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected property animator object"));
    };
    assert!(
        property_animator.propertyAnimationId.is_none(),
        "propertyAnimationId should be cleared, got {:?}",
        property_animator.propertyAnimationId.as_ref().map(|pointer| pointer.0)
    );

    // the keyframe children go with it, so nothing is left in the sampler cache either
    assert!(world.propertyKeyframesArcOfPropertyAnimationId.get(&property_animation_id).is_none());
    assert!(world.collectComputedPropertyOverridesOfObjectId().is_empty());

    Ok(())
}

// Builds an unpushable brick with one animator per named rotation axis, each holding that axis at a fixed angle.
fn make_world_with_rotation_animators(rotation_degrees_of_axis: &[(&str, f32, f32)]) -> anyhow::Result<(shared::game::world::World, ObjectId)> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let brick_object_id = 2;
    let clock_object_id = 3;

    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    {
        let clock_object = world.getObjectMut(clock_object_id)?;
        let ObjectInfo::Clock(clock) = &mut clock_object.info else {
            return Err(anyhow::anyhow!("Expected clock object"));
        };
        clock.clockState = ClockState::Playing {
            localStartMs: 0.0,
            timeLocation: ObjectLocation::Server,
        };
    }
    for (animator_index, (rotation_axis, start_degrees, end_degrees)) in rotation_degrees_of_axis.iter().enumerate() {
        let property_animator_object_id = 4 + animator_index as ObjectId;
        add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
        configure_property_animator(
            &mut world,
            property_animator_object_id,
            brick_object_id,
            clock_object_id,
            &["location", "rotation", rotation_axis],
            *start_degrees,
            *end_degrees,
        )?;
    }
    Ok((world, brick_object_id))
}

fn assert_rotation_is(world: &shared::game::world::World, brick_object_id: ObjectId, expected_quaternion: Quat, label: &str) -> anyhow::Result<()> {
    let Some(physics_info) = world.physics.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };
    // a quaternion and its negation are the same rotation, so compare them by how aligned they are.
    let alignment = physics_info.location.quaternion.dot(expected_quaternion).abs();
    assert!(
        (alignment - 1.0).abs() <= 1e-4,
        "{}: actual {:?}, expected {:?}",
        label,
        physics_info.location.quaternion,
        expected_quaternion
    );
    Ok(())
}

// One animator per axis has to survive a rotation that no single euler triple describes: quat_to_euler_degrees only
// reports an x in [-90, 90], so writing the axes one at a time would hand the next axis a folded y and z to build on,
// and 120 degrees of pitch would come out as 60 with y and z turned by 180.
#[test]
fn property_animators_write_every_rotation_axis_of_an_object_together() -> anyhow::Result<()> {
    let (mut world, brick_object_id) = make_world_with_rotation_animators(&[("x", 120.0, 120.0), ("y", 30.0, 30.0), ("z", 60.0, 60.0)])?;
    // the angles apply y first, then x, then z
    let expected_quaternion =
        Quat::from_rotation_y(30.0f32.to_radians()) * Quat::from_rotation_x(120.0f32.to_radians()) * Quat::from_rotation_z(60.0f32.to_radians());
    // stepping repeatedly is the part that used to drift: every tick rebuilt the rotation out of the previous tick's.
    for step_index in 0..5 {
        world.step_server(0, 0.0, 500.0)?;
        assert_rotation_is(&world, brick_object_id, expected_quaternion, &format!("rotation on step {step_index}"))?;
    }
    Ok(())
}

// Scripts read an animated rotation as the angles its animators wrote, not as a decomposition that reports 120 degrees
// of pitch as 60 with y and z turned by 180.
#[test]
fn an_animated_rotation_reads_back_as_the_animated_angles() -> anyhow::Result<()> {
    let (mut world, brick_object_id) = make_world_with_rotation_animators(&[("x", 120.0, 120.0), ("y", 30.0, 30.0), ("z", 60.0, 60.0)])?;
    world.step_server(0, 0.0, 500.0)?;
    let Some(rotation_degrees) = world.getRotationDegrees(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick rotation"));
    };
    assert_vec3_is(rotation_degrees, Vec3::new(120.0, 30.0, 60.0), "animated rotation read back");
    Ok(())
}

// The base is captured by decomposing the object's rotation, and that capture does not always happen while the
// object is still untouched - a client joining a game in progress captures its own bases against whatever rotation
// each object has by then. An object already tipped past straight up decomposes to a folded triple, and capturing
// the fold as the base mirrors every angle the animator asks for afterwards: 165 degrees of pitch would draw as 15
// for the rest of that client's session. Yaw no longer folds at all, and is still checked here so it stays that way.
#[test]
fn a_rotation_base_captured_on_an_already_turned_object_is_not_folded() -> anyhow::Result<()> {
    fn assert_rotation_base_holds(rotation_axis: &str, axis: Vec3, starting_degrees: f32) -> anyhow::Result<()> {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let brick_object_id = 2;
        let clock_object_id = 3;
        let property_animator_object_id = 4;

        add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
        // turn it before anything animates it, the way the patrol animation has already turned an NPC by the time a
        // joining client first sees it
        let starting_quaternion = Quat::from_axis_angle(axis, starting_degrees.to_radians());
        world.setObjectRotationRecursive(brick_object_id, starting_quaternion, Vec3::ZERO, SetOrForce::Force)?;

        add_clock(&mut world, clock_object_id, root_object_id)?;
        {
            let clock_object = world.getObjectMut(clock_object_id)?;
            let ObjectInfo::Clock(clock) = &mut clock_object.info else {
                return Err(anyhow::anyhow!("Expected clock object"));
            };
            clock.clockState = ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            };
        }
        add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
        // the animator asks for exactly the angle the object already has, so nothing may move
        configure_property_animator(
            &mut world,
            property_animator_object_id,
            brick_object_id,
            clock_object_id,
            &["location", "rotation", rotation_axis],
            starting_degrees,
            starting_degrees,
        )?;

        for step_index in 0..3 {
            world.step_server(0, 0.0, 500.0)?;
            assert_rotation_is(
                &world,
                brick_object_id,
                starting_quaternion,
                &format!("{rotation_axis} {starting_degrees} held on step {step_index} after capturing the base on an already-turned object"),
            )?;
        }
        Ok(())
    }

    for (rotation_axis, axis) in [("x", Vec3::X), ("y", Vec3::Y)] {
        for starting_degrees in [0.0f32, 45.0, 89.0, 91.0, 105.0, 135.0, 165.0, -105.0, -135.0, 179.0] {
            assert_rotation_base_holds(rotation_axis, axis, starting_degrees)?;
        }
    }
    Ok(())
}

// A lone animator sweeping a full turn is the case that folds worst: with one axis animated, the other two used to
// come from decomposing the animator's own output, so past a quarter turn the motion visibly reversed. They now come
// from the base angles captured when the animation started, which nothing animated ever touches. Pitch is the angle
// that folds, and yaw is the most common spin.
#[test]
fn property_animator_sweeps_a_full_turn_without_folding() -> anyhow::Result<()> {
    for (rotation_axis, axis) in [("x", Vec3::X), ("y", Vec3::Y)] {
        let (mut world, brick_object_id) = make_world_with_rotation_animators(&[(rotation_axis, 0.0, 360.0)])?;
        for frame_index in 0..60 {
            let live_server_sim_ms = frame_index as f64 * 1000.0 / 60.0;
            world.step_server(0, 0.0, live_server_sim_ms)?;
            let expected_degrees = live_server_sim_ms as f32 / 1000.0 * 360.0;
            let expected_quaternion = Quat::from_axis_angle(axis, expected_degrees.to_radians());
            assert_rotation_is(
                &world,
                brick_object_id,
                expected_quaternion,
                &format!("{rotation_axis} at frame {frame_index} (expected {expected_degrees} degrees)"),
            )?;
        }
    }
    Ok(())
}

// Adds a clock that plays once on the server's time, so an animator holds its last keyframe instead of starting over.
fn add_clock_playing_once(world: &mut shared::game::world::World, clock_object_id: ObjectId) -> anyhow::Result<()> {
    let root_object_id = world.getRootObjectId();
    add_clock(world, clock_object_id, root_object_id)?;
    let clock_object = world.getObjectMut(clock_object_id)?;
    let ObjectInfo::Clock(clock) = &mut clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock object"));
    };
    clock.shouldLoop = false;
    clock.clockState = ClockState::Playing {
        localStartMs: 0.0,
        timeLocation: ObjectLocation::Server,
    };
    Ok(())
}

fn assert_vec3_is(actual: Vec3, expected: Vec3, label: &str) {
    assert!((actual - expected).length() <= 1e-4, "{label}: actual {actual:?}, expected {expected:?}");
}

// A drawbridge placed facing east (its own z along world +x) and animated on x has to tip forward around its own x
// axis, which stays put along world -z. When the angles applied x last, x turned around the world's x axis instead,
// so the same animator made the bridge fall over sideways.
#[test]
fn a_pitch_animator_tips_an_east_facing_object_around_its_own_x_axis() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let brick_object_id = 2;
    let clock_object_id = 3;
    let property_animator_object_id = 4;

    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    world.setObjectRotationRecursive(brick_object_id, Quat::from_rotation_y(90.0f32.to_radians()), Vec3::ZERO, SetOrForce::Force)?;
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        brick_object_id,
        clock_object_id,
        &["location", "rotation", "x"],
        0.0,
        90.0,
    )?;

    for live_server_sim_ms in [250.0f64, 500.0, 750.0, 1000.0] {
        world.step_server(0, 0.0, live_server_sim_ms)?;
        let Some(physics_info) = world.getObjectPhysicsInfo3D(brick_object_id) else {
            return Err(anyhow::anyhow!("Expected brick physics info"));
        };
        let pitch_radians = (live_server_sim_ms as f32 / 1000.0 * 90.0).to_radians();
        let quaternion = physics_info.location.quaternion;
        assert_vec3_is(quaternion * Vec3::X, Vec3::NEG_Z, &format!("hinge axis at {live_server_sim_ms}ms"));
        assert_vec3_is(
            quaternion * Vec3::Z,
            Vec3::new(pitch_radians.cos(), -pitch_radians.sin(), 0.0),
            &format!("facing at {live_server_sim_ms}ms"),
        );
    }
    Ok(())
}

// A door is its own unpushable RigidGroup placed on the hinge line, with the panel inside it off to one side. Animating
// the group's rotation has to swing the panel around the hinge, and the panel's collision has to go with it.
#[test]
fn a_rotation_animator_swings_a_rigid_group_door_around_its_hinge() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let door_group_object_id = 2;
    let door_panel_object_id = 3;
    let clock_object_id = 4;
    let property_animator_object_id = 5;

    // the hinge is at the origin, and the 2x2x2 panel starts just east of it
    add_rigid_group(&mut world, door_group_object_id, root_object_id, Vec3::ZERO, None)?;
    add_brick(&mut world, door_panel_object_id, door_group_object_id, Vec3::new(1.0, 0.0, 0.0), false)?;
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        door_group_object_id,
        clock_object_id,
        &["location", "rotation", "y"],
        0.0,
        90.0,
    )?;

    // step past the end of the animation, so the door has swung a quarter turn and physics has moved its collider
    world.step_server(1, 1.0 / 60.0, 1500.0)?;
    let Some(door_panel_physics_info) = world.getObjectPhysicsInfo3D(door_panel_object_id) else {
        return Err(anyhow::anyhow!("Expected door panel physics info"));
    };
    assert_vec3_is(door_panel_physics_info.location.position, Vec3::new(0.0, 0.0, -1.0), "door panel center");

    let hit_object_id_below = |x: f32, z: f32| {
        world
            .physics
            .castRay(Vec3::new(x, 10.0, z), Vec3::NEG_Y, 20.0, false, &|_object_id: ObjectId| false)
            .map(|hit| hit.object_id)
    };
    assert_eq!(
        hit_object_id_below(-0.5, -1.5),
        Some(door_panel_object_id),
        "the swung panel should block where it now is"
    );
    assert_eq!(hit_object_id_below(1.5, 0.5), None, "nothing should be left where the panel started");
    Ok(())
}

// -------------------------- Pivots --------------------------

// Builds a 2x2x2 door whose center is 1 unit east of its hinge at the origin, with a Pivot on the hinge.
fn make_world_with_pivoted_door(door_object_id: ObjectId, pivot_object_id: ObjectId) -> anyhow::Result<shared::game::world::World> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, door_object_id, root_object_id, Vec3::new(1.0, 0.0, 0.0), false)?;
    add_pivot(&mut world, pivot_object_id, root_object_id, Some(door_object_id), Vec3::new(-1.0, 0.0, 0.0))?;
    Ok(world)
}

fn door_position(world: &shared::game::world::World, door_object_id: ObjectId) -> anyhow::Result<Vec3> {
    let Some(physics_info) = world.getObjectPhysicsInfo3D(door_object_id) else {
        return Err(anyhow::anyhow!("Expected door physics info"));
    };
    Ok(physics_info.location.position)
}

// A Pivot on a door's hinge makes a rotation animator swing the door around the hinge instead of its center, with no
// RigidGroup, and the door's collision goes with it.
#[test]
fn a_rotation_animator_swings_an_object_around_its_pivot() -> anyhow::Result<()> {
    let (door_object_id, pivot_object_id, clock_object_id, property_animator_object_id) = (2, 3, 4, 5);
    let mut world = make_world_with_pivoted_door(door_object_id, pivot_object_id)?;
    let root_object_id = world.getRootObjectId();
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        door_object_id,
        clock_object_id,
        &["location", "rotation", "y"],
        0.0,
        90.0,
    )?;

    world.step_server(0, 0.0, 500.0)?;
    assert_vec3_is(
        door_position(&world, door_object_id)?,
        Quat::from_rotation_y(45.0_f32.to_radians()) * Vec3::X,
        "door center halfway",
    );

    // step past the end of the animation, so physics has moved the collider too
    world.step_server(1, 1.0 / 60.0, 1500.0)?;
    assert_vec3_is(
        door_position(&world, door_object_id)?,
        Vec3::new(0.0, 0.0, -1.0),
        "door center after a quarter turn",
    );
    assert_rotation_is(
        &world,
        door_object_id,
        Quat::from_rotation_y(90.0_f32.to_radians()),
        "door rotation after a quarter turn",
    )?;
    let hit_object_id_below = |x: f32, z: f32| {
        world
            .physics
            .castRay(Vec3::new(x, 10.0, z), Vec3::NEG_Y, 20.0, false, &|_object_id: ObjectId| false)
            .map(|hit| hit.object_id)
    };
    assert_eq!(
        hit_object_id_below(-0.5, -1.5),
        Some(door_object_id),
        "the swung door should block where it now is"
    );
    assert_eq!(hit_object_id_below(1.5, 0.5), None, "nothing should be left where the door started");
    Ok(())
}

// On an object with a Pivot, a position animator moves the pivot point, and the rotation animator turns the object around
// wherever the pivot is. Here the hinge rises 5 units while the door swings a quarter turn.
#[test]
fn a_position_animator_moves_the_pivot_of_a_pivoted_object() -> anyhow::Result<()> {
    let (door_object_id, pivot_object_id, clock_object_id) = (2, 3, 4);
    let (rotation_animator_object_id, position_animator_object_id) = (5, 6);
    let mut world = make_world_with_pivoted_door(door_object_id, pivot_object_id)?;
    let root_object_id = world.getRootObjectId();
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, rotation_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        rotation_animator_object_id,
        door_object_id,
        clock_object_id,
        &["location", "rotation", "y"],
        0.0,
        90.0,
    )?;
    add_property_animator(&mut world, position_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        position_animator_object_id,
        door_object_id,
        clock_object_id,
        &["location", "position", "y"],
        0.0,
        5.0,
    )?;

    world.step_server(0, 0.0, 1500.0)?;
    assert_vec3_is(
        door_position(&world, door_object_id)?,
        Vec3::new(0.0, 5.0, -1.0),
        "door center after the hinge rose and the door swung",
    );
    Ok(())
}

// A player who joins mid-swing sees the door already turned around its hinge. The pivot point is read off the door's
// current pose, and turning around a point never moves that point, so the hinge is found in the same place.
#[test]
fn a_pivot_read_off_a_door_already_mid_swing_is_the_same_hinge() -> anyhow::Result<()> {
    let (door_object_id, pivot_object_id, clock_object_id, property_animator_object_id) = (2, 3, 4, 5);
    let mut world = make_world_with_pivoted_door(door_object_id, pivot_object_id)?;
    let root_object_id = world.getRootObjectId();
    let swung_rotation = Quat::from_rotation_y(45.0_f32.to_radians());
    world.setObjectRotationRecursive(door_object_id, swung_rotation, Vec3::ZERO, SetOrForce::Force)?;
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    // the animator asks for exactly the swing the door already has, so nothing may move
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        door_object_id,
        clock_object_id,
        &["location", "rotation", "y"],
        45.0,
        45.0,
    )?;

    for step_index in 0..3 {
        world.step_server(0, 0.0, 500.0)?;
        assert_vec3_is(
            door_position(&world, door_object_id)?,
            swung_rotation * Vec3::X,
            &format!("door center on step {step_index}"),
        );
    }
    Ok(())
}

// Like SkAttachments, when several Pivots point at one object the lowest id wins, in the world and in the renderer.
#[test]
fn the_lowest_id_pivot_is_the_one_an_object_turns_around() -> anyhow::Result<()> {
    let (door_object_id, pivot_object_id, clock_object_id, property_animator_object_id) = (2, 3, 4, 5);
    let mut world = make_world_with_pivoted_door(door_object_id, pivot_object_id)?;
    let root_object_id = world.getRootObjectId();
    add_pivot(&mut world, 7, root_object_id, Some(door_object_id), Vec3::new(1.0, 0.0, 0.0))?;
    add_clock_playing_once(&mut world, clock_object_id)?;
    add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
    configure_property_animator(
        &mut world,
        property_animator_object_id,
        door_object_id,
        clock_object_id,
        &["location", "rotation", "y"],
        0.0,
        90.0,
    )?;

    assert_eq!(
        world.getRenderSnapshot().pivot_offset_of_id.get(&door_object_id),
        Some(&Vec3::new(-1.0, 0.0, 0.0))
    );
    world.step_server(0, 0.0, 1500.0)?;
    assert_vec3_is(
        door_position(&world, door_object_id)?,
        Vec3::new(0.0, 0.0, -1.0),
        "door center after turning around the lowest-id pivot",
    );
    Ok(())
}

#[test]
fn deleting_an_object_clears_its_pivots_pointer() -> anyhow::Result<()> {
    let (door_object_id, pivot_object_id) = (2, 3);
    let mut world = make_world_with_pivoted_door(door_object_id, pivot_object_id)?;
    world.removeObject(door_object_id)?;
    let ObjectInfo::Pivot(pivot) = &world.getObject(pivot_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected pivot object"));
    };
    assert!(pivot.physicalObjectId.is_none());
    Ok(())
}
