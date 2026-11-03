mod helpers;

use std::f32::consts::{FRAC_PI_2, PI};

use game_types::{
    consts::{DEFAULT_GRAVITY, SERVER_SIM_AND_SEND_SECS},
    object_types::{
        Location3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, ScriptInfo, SkJointInfo, shared::TOP_CLIENT_OBJECT_ID,
    },
};
use glam::{EulerRot, Quat, Vec3};
use helpers::world_test_helpers::{
    add_ball, add_brick, add_label, add_local_group, add_mesh, add_pivot, add_player, add_rigid_group, add_script, add_skeleton, make_world,
    make_world_controlled_by_player,
};
use shared::game::jsRuntime::{JsConsoleLogCallback, JsConsoleLogEntry, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::physics::{SetOrForce, SetPhysicsInfo3D, SetPhysicsToggles3D};
use std::sync::{Arc, Mutex};

fn add_client_script(world: &mut shared::game::world::World, object_id: ObjectId, parent_id: ObjectId) {
    world.addObjectWithId(
        object_id,
        format!("script-{object_id}"),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        parent_id,
    );
    world.markScriptAsReadyToRun(object_id, format!("return {object_id};"));
}

#[tokio::test]
async fn script_can_read_performance_now_without_panicking() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;

    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
const now = performance.now();
if (typeof now !== "number") {
    throw new Error("Expected performance.now() to return a number");
}
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    Ok(())
}

#[tokio::test]
async fn script_location_write_sweeps_a_non_pushable_object_there() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let platform_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, platform_object_id, root_object_id, Vec3::ZERO, false)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const platform = game.objectService.getObjectById({platform_object_id});
if (platform === null || platform.type !== "brick") {{
    throw new Error("Expected platform brick");
}}
platform.location.position.x = 1.0;
"#
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // the write reads back right away, and it isn't a jump
    let before_step = world
        .getObjectPhysicsInfo3D(platform_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected platform physics"))?;
    assert_eq!(before_step.location.position, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(world.getTeleportCount(platform_object_id), 0, "a location write was sent as a jump");

    // it sweeps there over the next step, so rapier gives it the move's velocity and it carries what's on it
    world.step_server(1, SERVER_SIM_AND_SEND_SECS, SERVER_SIM_AND_SEND_SECS as f64 * 1000.0)?;
    let after_step = world
        .getObjectPhysicsInfo3D(platform_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected platform physics"))?;
    assert!((after_step.location.position.x - 1.0).abs() <= 1e-5);
    assert!(
        (after_step.movement.velocity - Vec3::new(1.0 / SERVER_SIM_AND_SEND_SECS, 0.0, 0.0)).length() <= 1e-2,
        "swept velocity {:?}",
        after_step.movement.velocity
    );
    Ok(())
}

// Game 298's mini golf slowed its ball by writing its movement every 100 ms. Each write was sent as a jump, so clients
// stopped smoothing the ball into it and it hitched ten times a second.
#[tokio::test]
async fn script_movement_and_location_writes_are_not_jumps() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let rolled_ball_id = 3;
    let moved_ball_id = 4;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_ball(&mut world, rolled_ball_id, root_object_id, Vec3::new(0.0, 5.0, 0.0), true)?;
    add_ball(&mut world, moved_ball_id, root_object_id, Vec3::new(10.0, 5.0, 0.0), true)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const rolledBall = game.objectService.getObjectById({rolled_ball_id});
const movedBall = game.objectService.getObjectById({moved_ball_id});
if (rolledBall === null || rolledBall.type !== "ball" || movedBall === null || movedBall.type !== "ball") {{
    throw new Error("Expected balls");
}}
rolledBall.movement = {{ velocity: {{ x: 7, y: 0, z: 0 }}, angularVelocity: {{ x: 0, y: 0, z: 0 }} }};
movedBall.location.position.x = 20.0;
"#
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rolled_ball = world.getObjectPhysicsInfo3D(rolled_ball_id).ok_or_else(|| anyhow::anyhow!("Expected ball physics"))?;
    assert_eq!(rolled_ball.movement.velocity, Vec3::new(7.0, 0.0, 0.0));
    assert_eq!(world.getTeleportCount(rolled_ball_id), 0, "a movement write was sent as a jump");
    assert_eq!(world.getTeleportCount(moved_ball_id), 0, "a location write was sent as a jump");
    Ok(())
}

// Every way a script moves something is a smooth move that other screens blend, and teleportObjectTo is the only jump.
// Each call moves its own group, so the group's child is checked too.
#[tokio::test]
async fn only_teleport_object_to_is_a_jump() -> anyhow::Result<()> {
    let teleport_call = "game.objectService.teleportObjectTo(GROUP, { x: 1, y: 2, z: 3 })";
    let calls = [
        "game.objectService.getObjectById(GROUP).location.position.x = 9",
        "game.objectService.moveObjectBy(GROUP, { x: 1, y: 0, z: 0 })",
        "game.objectService.moveObjectTo(GROUP, { x: 1, y: 2, z: 3 })",
        "game.objectService.rotateObjectBy(GROUP, { x: 0, y: 1, z: 0 }, 90)",
        "game.objectService.rotateObjectTo(GROUP, { x: 0, y: 90, z: 0 })",
        "game.objectService.scaleObjectBy(GROUP, 2)",
        teleport_call,
    ];
    let child_offset = Vec3::new(2.0, 0.0, 0.0);

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    add_script(&mut world, script_object_id, root_object_id)?;
    let mut raw_code = String::new();
    let mut moved_groups = Vec::new();
    for (call_index, call) in calls.into_iter().enumerate() {
        let group_object_id = 10 + 2 * call_index as ObjectId;
        let child_object_id = group_object_id + 1;
        let group_position = Vec3::new(0.0, 1.0, 10.0 * call_index as f32);
        add_rigid_group(&mut world, group_object_id, root_object_id, group_position, None)?;
        add_ball(&mut world, child_object_id, group_object_id, group_position + child_offset, false)?;
        raw_code += &format!("{};\n", call.replace("GROUP", &group_object_id.to_string()));
        moved_groups.push((call, group_object_id, child_object_id, group_position));
    }
    world.markScriptAsReadyToRun(script_object_id, raw_code);

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    for (call, group_object_id, child_object_id, group_position) in moved_groups {
        let Some(child_physics_info) = world.getObjectPhysicsInfo3D(child_object_id) else {
            return Err(anyhow::anyhow!("Expected physics info for object {child_object_id}"));
        };
        assert_ne!(child_physics_info.location.position, group_position + child_offset, "{call} moved nothing");
        let is_teleport = call == teleport_call;
        for object_id in [group_object_id, child_object_id] {
            let teleport_count = world.getTeleportCount(object_id);
            assert_eq!(
                teleport_count != 0,
                is_teleport,
                "{call}: object {object_id} has teleport count {teleport_count}"
            );
        }
        if is_teleport {
            assert_eq!(child_physics_info.location.position, Vec3::new(1.0, 2.0, 3.0) + child_offset);
        }
    }
    Ok(())
}

// A respawn often moves a character and turns it in the same tick. On the server a player's character is kinematic, so a turn that
// swept would hand the player its spin, and a turn before the teleport used to be dropped by it. A teleport covers its whole physics step
// instead, so the turn jumps with it in either order.
#[tokio::test]
async fn a_turn_in_the_same_step_as_a_teleport_jumps_with_it() -> anyhow::Result<()> {
    for code in [
        "game.objectService.teleportObjectTo(BODY, { x: 0, y: 10, z: 0 }); game.objectService.rotateObjectTo(BODY, { x: 0, y: 180, z: 0 });",
        "game.objectService.rotateObjectTo(BODY, { x: 0, y: 180, z: 0 }); game.objectService.teleportObjectTo(BODY, { x: 0, y: 10, z: 0 });",
    ] {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();
        let (character_object_id, player_object_id, script_object_id) = (2, 3, 4);
        add_ball(&mut world, character_object_id, root_object_id, Vec3::new(0.0, -60.0, 0.0), true)?;
        add_player(&mut world, player_object_id, root_object_id, Some(character_object_id))?;
        world.step_server(1, SERVER_SIM_AND_SEND_SECS, SERVER_SIM_AND_SEND_SECS as f64 * 1000.0)?;
        add_script(&mut world, script_object_id, root_object_id)?;
        world.markScriptAsReadyToRun(script_object_id, code.replace("BODY", &character_object_id.to_string()));

        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        world.step_server(1, SERVER_SIM_AND_SEND_SECS, 2.0 * SERVER_SIM_AND_SEND_SECS as f64 * 1000.0)?;

        let Some(physics_info) = world.getObjectPhysicsInfo3D(character_object_id) else {
            return Err(anyhow::anyhow!("Expected the character's physics info"));
        };
        assert_eq!(physics_info.location.position, Vec3::new(0.0, 10.0, 0.0), "{code}");
        let alignment = physics_info.location.quaternion.dot(Quat::from_rotation_y(PI)).abs();
        assert!((alignment - 1.0).abs() <= 1e-5, "{code}: rotation {:?}", physics_info.location.quaternion);
        assert_eq!(physics_info.movement.angularVelocity, Vec3::ZERO, "{code}: the turn swept");
    }
    Ok(())
}

// After a jump, the object's other moves jump with it until the next physics step, and are smooth again after that step.
#[test]
fn moves_jump_with_a_teleport_until_the_next_physics_step() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let platform_object_id = 2;
    add_brick(&mut world, platform_object_id, root_object_id, Vec3::ZERO, false)?;
    let to = |x: f32| SetPhysicsInfo3D {
        position: Some(Vec3::new(x, 0.0, 0.0)),
        ..Default::default()
    };

    world.forcePhysicsInfo3D(platform_object_id, to(10.0), true);
    world.setPhysicsInfo3D(platform_object_id, to(11.0), true);
    world.step_server(1, SERVER_SIM_AND_SEND_SECS, SERVER_SIM_AND_SEND_SECS as f64 * 1000.0)?;
    let after_jump = world
        .getObjectPhysicsInfo3D(platform_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected platform physics"))?;
    assert_eq!(after_jump.location.position, Vec3::new(11.0, 0.0, 0.0));
    assert_eq!(after_jump.movement.velocity, Vec3::ZERO, "the move in the jump's step swept");

    world.setPhysicsInfo3D(platform_object_id, to(12.0), true);
    world.step_server(1, SERVER_SIM_AND_SEND_SECS, 2.0 * SERVER_SIM_AND_SEND_SECS as f64 * 1000.0)?;
    let after_move = world
        .getObjectPhysicsInfo3D(platform_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected platform physics"))?;
    assert!(
        (after_move.movement.velocity - Vec3::new(1.0 / SERVER_SIM_AND_SEND_SECS, 0.0, 0.0)).length() <= 1e-2,
        "a move after the jump's step did not sweep: {:?}",
        after_move.movement.velocity
    );
    Ok(())
}

#[tokio::test]
async fn object_service_reports_velocity_relative_to_moving_ground() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ground_object_id = 2;
    let body_object_id = 3;
    let script_object_id = 4;

    add_brick(&mut world, ground_object_id, root_object_id, Vec3::ZERO, false)?;
    add_ball(&mut world, body_object_id, root_object_id, Vec3::new(0.0, 2.005, 0.0), true)?;
    let _ = world.physics.step(0.0);
    world.forcePhysicsInfo3D(
        ground_object_id,
        SetPhysicsInfo3D {
            velocity: Some(Vec3::new(1.0, 0.0, 0.0)),
            ..Default::default()
        },
        false,
    );
    world.forcePhysicsInfo3D(
        body_object_id,
        SetPhysicsInfo3D {
            velocity: Some(Vec3::new(4.0, 0.0, 0.0)),
            ..Default::default()
        },
        false,
    );
    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const velocity = game.objectService.getVelocityRelativeToGround({body_object_id});
if (Math.abs(velocity.x - 3) > 0.0001 || Math.abs(velocity.y) > 0.0001 || Math.abs(velocity.z) > 0.0001) {{
    throw new Error(`Unexpected relative velocity: ${{velocity.x}}, ${{velocity.y}}, ${{velocity.z}}`);
}}
"#
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    Ok(())
}

// A script can't print a live object, so the scripts below report the hit object's id instead
const HIT_TO_REPORT_JS: &str =
    "const hitToReport = (hit) => hit === null ? null : { objectId: hit.object.id, distance: hit.distance, position: hit.position, normal: hit.normal };";

// One ball hovers half a unit above its floor and another rests on its floor, so both ways of finding ground are
// covered: a shape cast down for the first, the live contact for the second.
#[tokio::test]
async fn object_service_ground_support_finds_walkable_ground_within_reach() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let hovering_ball_object_id = 3;
    let resting_ball_object_id = 4;
    let first_floor_object_id = 5;
    let second_floor_object_id = 6;

    add_script(&mut world, script_object_id, root_object_id)?;
    // each floor's top is at y=-1
    add_brick(&mut world, first_floor_object_id, root_object_id, Vec3::new(0.0, -2.0, 0.0), false)?;
    add_brick(&mut world, second_floor_object_id, root_object_id, Vec3::new(5.0, -2.0, 0.0), false)?;
    add_ball(&mut world, hovering_ball_object_id, root_object_id, Vec3::new(0.0, 0.5, 0.0), true)?;
    add_ball(&mut world, resting_ball_object_id, root_object_id, Vec3::new(5.0, -0.001, 0.0), true)?;
    world.physics.update_collisions();
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
{HIT_TO_REPORT_JS}
const objectService = game.objectService;
const withinReach = hitToReport(objectService.getGroundSupport({hovering_ball_object_id}, {{ maxDistance: 1 }}));
const outOfReach = hitToReport(objectService.getGroundSupport({hovering_ball_object_id}, {{ maxDistance: 0.25 }}));
const touching = hitToReport(objectService.getGroundSupport({resting_ball_object_id}));
const gravity = objectService.getGravity();
let scriptError = null;
try {{
    objectService.getGroundSupport({script_object_id});
}} catch (error) {{
    scriptError = String(error);
}}
console.log("report:" + JSON.stringify({{ withinReach, outOfReach, touching, gravity, scriptError }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_eq!(report["withinReach"]["objectId"], serde_json::json!(first_floor_object_id));
    assert!((report["withinReach"]["distance"].as_f64().unwrap_or(f64::NAN) - 0.5).abs() <= 1e-3, "{report}");
    // a shape cast only pins down a contact point on a flat face to within a few millimeters
    assert_reported_vec3_near(
        &report["withinReach"]["position"],
        Vec3::new(0.0, -1.0, 0.0),
        1e-2,
        "where the hovering ball would land",
    );
    assert_reported_vec3(&report["withinReach"]["normal"], Vec3::Y, "floor normal under the hovering ball");
    assert_eq!(report["outOfReach"], serde_json::Value::Null);

    assert_eq!(report["touching"]["objectId"], serde_json::json!(second_floor_object_id));
    assert_eq!(report["touching"]["distance"].as_f64(), Some(0.0), "{report}");
    assert_reported_vec3(&report["touching"]["position"], Vec3::new(5.0, -1.0, 0.0), "where the resting ball touches");
    assert_reported_vec3(&report["touching"]["normal"], Vec3::Y, "floor normal under the resting ball");

    assert_reported_vec3(&report["gravity"], Vec3::new(0.0, -DEFAULT_GRAVITY, 0.0), "gravity");
    let script_error = report["scriptError"].as_str().unwrap_or_default();
    assert!(script_error.contains("not a physical object"), "scriptError was {script_error:?}");
    Ok(())
}

// A floor tilted 30 degrees counts as ground under a 45 degree limit and not under a 20 degree one, and only ground
// that counts is taken out of the relative velocity. The floor is pushable, because an unpushable body only gets a
// velocity once a real step moves it.
#[tokio::test]
async fn object_service_ground_steeper_than_the_slope_limit_does_not_count() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let ball_object_id = 3;
    let floor_object_id = 4;
    let floor_rotation = Quat::from_rotation_z(30.0_f32.to_radians());
    let floor_up = floor_rotation * Vec3::Y;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, floor_object_id, root_object_id, Vec3::ZERO, true)?;
    // resting on the floor's top face, which is 1 above its center
    add_ball(&mut world, ball_object_id, root_object_id, floor_up * 1.999, true)?;
    world.forcePhysicsInfo3D(
        floor_object_id,
        SetPhysicsInfo3D {
            rotation: Some(floor_rotation),
            ..Default::default()
        },
        false,
    );
    world.physics.update_collisions();
    world.forcePhysicsInfo3D(
        floor_object_id,
        SetPhysicsInfo3D {
            velocity: Some(Vec3::new(1.0, 0.0, 0.0)),
            ..Default::default()
        },
        false,
    );
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
{HIT_TO_REPORT_JS}
const objectService = game.objectService;
console.log("report:" + JSON.stringify({{
    supportWith45: hitToReport(objectService.getGroundSupport({ball_object_id}, {{ maxDistance: 1, maxGroundSlopeDegrees: 45 }})),
    supportWith20: hitToReport(objectService.getGroundSupport({ball_object_id}, {{ maxDistance: 1, maxGroundSlopeDegrees: 20 }})),
    relativeVelocityWith45: objectService.getVelocityRelativeToGround({ball_object_id}, {{ maxGroundSlopeDegrees: 45 }}),
    relativeVelocityWith20: objectService.getVelocityRelativeToGround({ball_object_id}, {{ maxGroundSlopeDegrees: 20 }}),
}}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_eq!(report["supportWith45"]["objectId"], serde_json::json!(floor_object_id));
    assert_reported_vec3(&report["supportWith45"]["normal"], floor_up, "tilted floor normal");
    assert_reported_vec3(&report["supportWith45"]["position"], floor_up, "where the ball touches the tilted floor");
    assert_eq!(report["supportWith20"], serde_json::Value::Null);
    assert_reported_vec3(
        &report["relativeVelocityWith45"],
        Vec3::new(-1.0, 0.0, 0.0),
        "relative velocity on walkable ground",
    );
    assert_reported_vec3(&report["relativeVelocityWith20"], Vec3::ZERO, "relative velocity on too steep ground");
    Ok(())
}

// The wall's face is at x=3 and the ball's side is at x=1. Like castRay, the direction doesn't need to be normalized and
// the distance is unlimited unless maxDistance says otherwise.
#[tokio::test]
async fn object_service_cast_object_shape_returns_the_first_solid_hit_along_the_path() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let ball_object_id = 3;
    let wall_object_id = 4;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, wall_object_id, root_object_id, Vec3::new(4.0, 0.0, 0.0), false)?;
    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, true)?;
    world.physics.update_collisions();
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
{HIT_TO_REPORT_JS}
const castBall = (options) => hitToReport(game.objectService.castObjectShape({ball_object_id}, {{ x: 5, y: 0, z: 0 }}, options));
console.log("report:" + JSON.stringify({{
    intoTheWall: castBall(),
    shortOfTheWall: castBall({{ maxDistance: 1 }}),
    overTheWall: castBall({{ startOffset: {{ x: 0, y: 5, z: 0 }} }}),
    throughTheWall: castBall({{ excludeObjectIds: [{wall_object_id}] }}),
}}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_eq!(report["intoTheWall"]["objectId"], serde_json::json!(wall_object_id));
    assert!((report["intoTheWall"]["distance"].as_f64().unwrap_or(f64::NAN) - 2.0).abs() <= 1e-3, "{report}");
    assert_reported_vec3(&report["intoTheWall"]["position"], Vec3::new(3.0, 0.0, 0.0), "where the ball hits the wall");
    assert_reported_vec3(&report["intoTheWall"]["normal"], Vec3::NEG_X, "wall normal");
    assert_eq!(report["shortOfTheWall"], serde_json::Value::Null);
    assert_eq!(report["overTheWall"], serde_json::Value::Null);
    assert_eq!(report["throughTheWall"], serde_json::Value::Null);
    Ok(())
}

#[tokio::test]
async fn object_service_rotate_object_uses_recursive_world_rotation() -> anyhow::Result<()> {
    fn assert_close_f32(actual: f32, expected: f32, label: &str) {
        let difference = (actual - expected).abs();
        assert!(
            difference <= 1e-5,
            "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
        );
    }

    fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
        assert_close_f32(actual.x, expected.x, &format!("{label}.x"));
        assert_close_f32(actual.y, expected.y, &format!("{label}.y"));
        assert_close_f32(actual.z, expected.z, &format!("{label}.z"));
    }

    fn assert_quat_close(actual: Quat, expected: Quat, label: &str) {
        let actual_normalized = actual.normalize();
        let expected_normalized = expected.normalize();
        let dot = actual_normalized.dot(expected_normalized).abs();
        let difference = (1.0 - dot).abs();
        assert!(difference <= 1e-4, "{label} mismatch: abs(dot)={dot}, difference={difference}");
    }

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let rigid_group_object_id = 2;
    let child_ball_object_id = 3;
    let script_object_id = 4;

    // This creates a physical parent and child so the script-side rotate call has to update the full subtree.
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 1.0, -2.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let rotation_y_degrees = 90.0;
    let rotation_y = std::f32::consts::FRAC_PI_2;
    let rotation_quaternion = Quat::from_euler(EulerRot::XYZ, 0.0, rotation_y, 0.0);
    let center_point = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should have physics")
        .location
        .position;
    let child_position_before = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should have physics")
        .location
        .position;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "game.objectService.rotateObjectBy({}, {{ x: 0.0, y: 1.0, z: 0.0 }}, {});",
            rigid_group_object_id, rotation_y_degrees
        ),
    );

    let expected_child_position = center_point + rotation_quaternion * (child_position_before - center_point);

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rigid_group_physics_info = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should still have physics after rotation");
    let child_ball_physics_info = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should still have physics after rotation");

    // This checks both the pivoted child translation and the shared rotation delta.
    assert_vec3_close(rigid_group_physics_info.location.position, center_point, "rigid_group.position");
    assert_quat_close(rigid_group_physics_info.location.quaternion, rotation_quaternion, "rigid_group.rotation");
    assert_vec3_close(child_ball_physics_info.location.position, expected_child_position, "child_ball.position");
    assert_quat_close(child_ball_physics_info.location.quaternion, rotation_quaternion, "child_ball.rotation");

    Ok(())
}

// rotateObjectTo sets a rotation rather than adding one, so calling it twice with the same angles turns nothing the
// second time. The children turn with the object, around the object's position.
#[tokio::test]
async fn object_service_rotate_object_to_sets_the_rotation_and_carries_children() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let rigid_group_object_id = 2;
    let child_ball_object_id = 3;
    let script_object_id = 4;

    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 1.0, -2.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    // start turned, so the call has to work out the difference instead of just adding its angles
    world.setObjectRotationRecursive(
        rigid_group_object_id,
        Quat::from_rotation_y(30.0_f32.to_radians()),
        Vec3::new(5.0, 1.0, -2.0),
        SetOrForce::Force,
    )?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "game.objectService.rotateObjectTo({rigid_group_object_id}, {{ x: 0, y: 90, z: 0 }});\ngame.objectService.rotateObjectTo({rigid_group_object_id}, {{ x: 0, y: 90, z: 0 }});"
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let expected_rotation = Quat::from_rotation_y(90.0_f32.to_radians());
    for object_id in [rigid_group_object_id, child_ball_object_id] {
        let Some(physics_info) = world.getObjectPhysicsInfo3D(object_id) else {
            return Err(anyhow::anyhow!("Expected physics info for object {object_id}"));
        };
        let alignment = physics_info.location.quaternion.dot(expected_rotation).abs();
        assert!(
            (alignment - 1.0).abs() <= 1e-5,
            "object {object_id} rotation {:?}",
            physics_info.location.quaternion
        );
    }
    let Some(child_ball_physics_info) = world.getObjectPhysicsInfo3D(child_ball_object_id) else {
        return Err(anyhow::anyhow!("Expected child ball physics info"));
    };
    // the child started 2 units along +x from the group, and a quarter turn around y puts it 2 units along -z
    assert!(
        (child_ball_physics_info.location.position - Vec3::new(5.0, 1.0, -4.0)).length() <= 1e-4,
        "child position {:?}",
        child_ball_physics_info.location.position
    );
    Ok(())
}

// Runs the world's scripts once and returns what they printed. A script that throws during startup is only logged, so
// the rotation tests below print their results and let Rust do the checking.
async fn run_scripts_and_read_report(world: &mut shared::game::world::World) -> anyhow::Result<serde_json::Value> {
    let console_log_entries: Arc<Mutex<Vec<JsConsoleLogEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let console_log_entries_for_callback = console_log_entries.clone();
    let console_log_callback: JsConsoleLogCallback = Arc::new(move |entry: JsConsoleLogEntry| {
        console_log_entries_for_callback.lock().unwrap().push(entry);
    });
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, Some(console_log_callback)).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await?;

    let messages = console_log_entries
        .lock()
        .unwrap()
        .iter()
        .map(|entry| entry.message.trim().to_string())
        .collect::<Vec<_>>();
    let Some(report) = messages.iter().find_map(|message| message.strip_prefix("report:")) else {
        return Err(anyhow::anyhow!("the script reported nothing, console was: {messages:?}"));
    };
    Ok(serde_json::from_str(report)?)
}

fn assert_reported_vec3(reported: &serde_json::Value, expected: Vec3, label: &str) {
    assert_reported_vec3_near(reported, expected, 1e-3, label);
}

fn assert_reported_vec3_near(reported: &serde_json::Value, expected: Vec3, tolerance: f32, label: &str) {
    let component = |axis: &str| reported[axis].as_f64().unwrap_or(f64::NAN) as f32;
    let actual = Vec3::new(component("x"), component("y"), component("z"));
    assert!((actual - expected).length() <= tolerance, "{label}: reported {reported}, expected {expected:?}");
}

// Rotation angles read back exactly as written, even past a quarter turn of yaw, because they apply y first and only
// a pitch past straight up can fold. When y was applied in the middle, 120 degrees of yaw read back as x 180, y 60,
// z 180, and writing one angle rebuilt the rotation from those folded numbers.
#[tokio::test]
async fn object_rotation_reads_back_the_angles_that_were_written() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let brick_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const brick = game.objectService.getObjectById({brick_object_id});
brick.location.rotation = {{ x: 30, y: 120, z: -45 }};
const afterWholeWrite = {{ ...brick.location.rotation }};
brick.location.rotation.y = 150;
const afterOneAngleWrite = {{ ...brick.location.rotation }};
console.log("report:" + JSON.stringify({{ afterWholeWrite, afterOneAngleWrite }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(
        &report["afterWholeWrite"],
        Vec3::new(30.0, 120.0, -45.0),
        "rotation after writing all three angles",
    );
    assert_reported_vec3(&report["afterOneAngleWrite"], Vec3::new(30.0, 150.0, -45.0), "rotation after writing only y");
    // the object turns y first, then tips x, then rolls z
    let Some(brick_physics_info) = world.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info"));
    };
    let expected_quaternion =
        Quat::from_rotation_y(150.0_f32.to_radians()) * Quat::from_rotation_x(30.0_f32.to_radians()) * Quat::from_rotation_z(-45.0_f32.to_radians());
    let alignment = brick_physics_info.location.quaternion.dot(expected_quaternion).abs();
    assert!(
        (alignment - 1.0).abs() <= 1e-5,
        "stored rotation {:?}, expected {:?}",
        brick_physics_info.location.quaternion,
        expected_quaternion
    );
    Ok(())
}

// the object turns y first, then tips x, then rolls z
fn assert_turned_by_degrees(world: &shared::game::world::World, object_id: ObjectId, degrees: Vec3, label: &str) -> anyhow::Result<()> {
    let Some(quaternion) = world.getObjectRotation(object_id) else {
        return Err(anyhow::anyhow!("{label}: expected a rotation"));
    };
    let expected_quaternion =
        Quat::from_rotation_y(degrees.y.to_radians()) * Quat::from_rotation_x(degrees.x.to_radians()) * Quat::from_rotation_z(degrees.z.to_radians());
    let alignment = quaternion.dot(expected_quaternion).abs();
    assert!(
        (alignment - 1.0).abs() <= 1e-5,
        "{label}: stored rotation {:?}, expected {:?}",
        quaternion,
        expected_quaternion
    );
    Ok(())
}

// Angles tipped past straight up used to read back folded: x 120 came back as 60 with y and z turned by 180, so writing
// one angle afterwards rebuilt the rotation from the folded numbers, and adding 1 to x every tick stalled at 90. Like
// Godot, the angles last written now read back as written while they still describe the rotation.
#[tokio::test]
async fn object_rotation_reads_back_angles_tipped_past_straight_up() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let written_brick_object_id = 3;
    let counted_brick_object_id = 4;
    let turned_to_brick_object_id = 5;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, written_brick_object_id, root_object_id, Vec3::ZERO, false)?;
    add_brick(&mut world, counted_brick_object_id, root_object_id, Vec3::new(4.0, 0.0, 0.0), false)?;
    add_brick(&mut world, turned_to_brick_object_id, root_object_id, Vec3::new(8.0, 0.0, 0.0), false)?;
    world.objectIdCounter = world.objectOfId.keys().copied().max().unwrap_or(root_object_id);
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const objectService = game.objectService;
const writtenBrick = objectService.getObjectById({written_brick_object_id});
writtenBrick.location.rotation = {{ x: 120, y: 30, z: 0 }};
const afterWholeWrite = {{ ...writtenBrick.location.rotation }};
writtenBrick.location.rotation.y = 50;
const afterOneAngleWrite = {{ ...writtenBrick.location.rotation }};

const countedBrick = objectService.getObjectById({counted_brick_object_id});
countedBrick.location.rotation.x = 80;
for (let tick = 0; tick < 20; tick++) {{
    countedBrick.location.rotation.x += 1;
}}
const afterCountingPastStraightUp = {{ ...countedBrick.location.rotation }};

objectService.rotateObjectTo({turned_to_brick_object_id}, {{ x: 120, y: -60, z: 10 }});
const afterRotateObjectTo = {{ ...objectService.getObjectById({turned_to_brick_object_id}).location.rotation }};

const v = (x, y, z) => ({{ x, y, z }});
const createdBrick = objectService.createObject(
    "CreatedBrick",
    {{
        type: "brick",
        material: null,
        opacity: 1,
        color: {{ r: 1, g: 0, b: 0 }},
        isAlwaysOnTop: false,
        size: v(1, 1, 1),
        isWalkthrough: false,
        isPushable: false,
        density: 1,
        location: {{ position: v(12, 0, 0), rotation: v(150, 0, -30) }},
        movement: {{ velocity: v(0, 0, 0), angularVelocity: v(0, 0, 0) }},
    }},
    game.world.id,
);
const afterCreate = {{ ...createdBrick.location.rotation }};
console.log("report:" + JSON.stringify({{ afterWholeWrite, afterOneAngleWrite, afterCountingPastStraightUp, afterRotateObjectTo, afterCreate }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(
        &report["afterWholeWrite"],
        Vec3::new(120.0, 30.0, 0.0),
        "rotation after writing all three angles",
    );
    assert_reported_vec3(&report["afterOneAngleWrite"], Vec3::new(120.0, 50.0, 0.0), "rotation after writing only y");
    assert_reported_vec3(
        &report["afterCountingPastStraightUp"],
        Vec3::new(100.0, 0.0, 0.0),
        "rotation after adding 1 to x twenty times from 80",
    );
    assert_reported_vec3(&report["afterRotateObjectTo"], Vec3::new(120.0, -60.0, 10.0), "rotation after rotateObjectTo");
    assert_reported_vec3(&report["afterCreate"], Vec3::new(150.0, 0.0, -30.0), "rotation a brick was created with");
    // the angles that read back are the ones the objects are really turned by
    assert_turned_by_degrees(&world, written_brick_object_id, Vec3::new(120.0, 50.0, 0.0), "written brick")?;
    assert_turned_by_degrees(&world, counted_brick_object_id, Vec3::new(100.0, 0.0, 0.0), "counted brick")?;
    assert_turned_by_degrees(
        &world,
        turned_to_brick_object_id,
        Vec3::new(120.0, -60.0, 10.0),
        "brick turned with rotateObjectTo",
    )?;
    Ok(())
}

// A joint's restPose.rotation reads back the angles written to it the same way location.rotation does, so the same
// writes past straight up read back as written instead of folded.
#[tokio::test]
async fn joint_rest_rotation_reads_back_angles_tipped_past_straight_up() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let skeleton_object_id = 3;
    let written_joint_object_id = 4;
    let counted_joint_object_id = 5;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
    for joint_object_id in [written_joint_object_id, counted_joint_object_id] {
        world.addObjectWithId(
            joint_object_id,
            format!("joint-{joint_object_id}"),
            ObjectBeingAdded::SkJoint(SkJointInfo {
                restPose: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
            }),
            skeleton_object_id,
        );
    }
    world.objectIdCounter = world.objectOfId.keys().copied().max().unwrap_or(root_object_id);
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const objectService = game.objectService;
const writtenJoint = objectService.getObjectById({written_joint_object_id});
writtenJoint.restPose.rotation = {{ x: 120, y: 30, z: 0 }};
const afterWholeWrite = {{ ...writtenJoint.restPose.rotation }};
writtenJoint.restPose.rotation.y = 50;
const afterOneAngleWrite = {{ ...writtenJoint.restPose.rotation }};

const countedJoint = objectService.getObjectById({counted_joint_object_id});
countedJoint.restPose.rotation.x = 80;
for (let tick = 0; tick < 20; tick++) {{
    countedJoint.restPose.rotation.x += 1;
}}
const afterCountingPastStraightUp = {{ ...countedJoint.restPose.rotation }};

const v = (x, y, z) => ({{ x, y, z }});
const createdJoint = objectService.createObject(
    "CreatedJoint",
    {{ type: "skJoint", restPose: {{ position: v(0, 1, 0), rotation: v(150, 0, -30) }} }},
    {skeleton_object_id},
);
const afterCreate = {{ ...createdJoint.restPose.rotation }};
console.log("report:" + JSON.stringify({{ afterWholeWrite, afterOneAngleWrite, afterCountingPastStraightUp, afterCreate }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(
        &report["afterWholeWrite"],
        Vec3::new(120.0, 30.0, 0.0),
        "rest rotation after writing all three angles",
    );
    assert_reported_vec3(&report["afterOneAngleWrite"], Vec3::new(120.0, 50.0, 0.0), "rest rotation after writing only y");
    assert_reported_vec3(
        &report["afterCountingPastStraightUp"],
        Vec3::new(100.0, 0.0, 0.0),
        "rest rotation after adding 1 to x twenty times from 80",
    );
    assert_reported_vec3(&report["afterCreate"], Vec3::new(150.0, 0.0, -30.0), "rest rotation a joint was created with");
    // the angles that read back are the ones the joints are really turned by
    assert_turned_by_degrees(&world, written_joint_object_id, Vec3::new(120.0, 50.0, 0.0), "written joint")?;
    assert_turned_by_degrees(&world, counted_joint_object_id, Vec3::new(100.0, 0.0, 0.0), "counted joint")?;
    Ok(())
}

// Written angles only read back while they still describe the object. Once something else turns it, it reads as its
// decomposed rotation again: the same turn, with an x within [-90, 90].
#[tokio::test]
async fn object_rotation_reads_decomposed_angles_once_something_else_turns_it() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let brick_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const brick = game.objectService.getObjectById({brick_object_id});
brick.location.rotation = {{ x: 120, y: 0, z: 0 }};
game.objectService.rotateObjectBy({brick_object_id}, {{ x: 0, y: 1, z: 0 }}, 10);
const afterTurningBy = {{ ...brick.location.rotation }};
console.log("report:" + JSON.stringify({{ afterTurningBy }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    let component = |axis: &str| report["afterTurningBy"][axis].as_f64().unwrap_or(f64::NAN) as f32;
    let after_turning_by = Vec3::new(component("x"), component("y"), component("z"));
    assert!(after_turning_by.x.abs() <= 90.0 + 1e-3, "rotation after turning by read {after_turning_by:?}");
    assert_turned_by_degrees(&world, brick_object_id, after_turning_by, "rotation after turning by")?;
    // and that is 10 more degrees of yaw on top of the 120 of pitch
    assert_turned_by_degrees(&world, brick_object_id, Vec3::new(120.0, 10.0, 0.0), "stored rotation after turning by")?;
    Ok(())
}

// rotateObjectBy turns around any world-space axis, and getObjectAxes reports where the object's own axes point. A bridge
// turned to face east around the world's up, then tipped 60 degrees around its own x axis, keeps its hinge along -z
// and tips its face down toward the ground in front of it. That is the same as writing x 60, y 90.
#[tokio::test]
async fn object_service_rotate_object_turns_around_an_axis_that_get_object_axes_reports() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let bridge_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, bridge_object_id, root_object_id, Vec3::new(4.0, 1.0, -3.0), false)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
game.objectService.rotateObjectBy({bridge_object_id}, {{ x: 0, y: 1, z: 0 }}, 90);
const axesFacingEast = game.objectService.getObjectAxes({bridge_object_id});
game.objectService.rotateObjectBy({bridge_object_id}, axesFacingEast.x, 60);
const axesTipped = game.objectService.getObjectAxes({bridge_object_id});
const bridge = game.objectService.getObjectById({bridge_object_id});
const rotationTipped = {{ ...bridge.location.rotation }};
const positionTipped = {{ ...bridge.location.position }};
let zeroAxisError = null;
try {{
    game.objectService.rotateObjectBy({bridge_object_id}, {{ x: 0, y: 0, z: 0 }}, 90);
}} catch (error) {{
    zeroAxisError = String(error);
}}
console.log("report:" + JSON.stringify({{ axesFacingEast, axesTipped, rotationTipped, positionTipped, zeroAxisError }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(&report["axesFacingEast"]["x"], Vec3::NEG_Z, "own x after facing east");
    assert_reported_vec3(&report["axesFacingEast"]["y"], Vec3::Y, "own y after facing east");
    assert_reported_vec3(&report["axesFacingEast"]["z"], Vec3::X, "own z after facing east");
    let (sin_60, cos_60) = 60.0_f32.to_radians().sin_cos();
    assert_reported_vec3(&report["axesTipped"]["x"], Vec3::NEG_Z, "own x after tipping around it");
    assert_reported_vec3(&report["axesTipped"]["y"], Vec3::new(sin_60, cos_60, 0.0), "own y after tipping");
    assert_reported_vec3(&report["axesTipped"]["z"], Vec3::new(cos_60, -sin_60, 0.0), "own z after tipping");
    assert_reported_vec3(&report["rotationTipped"], Vec3::new(60.0, 90.0, 0.0), "rotation after tipping");
    // it pivots on its own position
    assert_reported_vec3(&report["positionTipped"], Vec3::new(4.0, 1.0, -3.0), "position after both turns");
    let zero_axis_error = report["zeroAxisError"].as_str().unwrap_or_default();
    assert!(
        zero_axis_error.contains("axis must have a nonzero length"),
        "zero axis error was {zero_axis_error:?}"
    );
    Ok(())
}

// rotateObjectTo sets exactly the angles it is given, including ones that used to fold, and both rotate functions refuse
// objects that have no physics to turn.
#[tokio::test]
async fn object_service_rotate_object_to_reads_back_its_angles_and_rejects_objects_without_physics() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let brick_object_id = 3;
    let folder_object_id = 4;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_brick(&mut world, brick_object_id, root_object_id, Vec3::ZERO, false)?;
    add_local_group(&mut world, folder_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
game.objectService.rotateObjectTo({brick_object_id}, {{ x: 30, y: 120, z: -45 }});
const rotation = {{ ...game.objectService.getObjectById({brick_object_id}).location.rotation }};
const errorOf = (rotate) => {{
    try {{
        rotate();
        return null;
    }} catch (error) {{
        return String(error);
    }}
}};
const rotateToError = errorOf(() => game.objectService.rotateObjectTo({folder_object_id}, {{ x: 0, y: 90, z: 0 }}));
const rotateByError = errorOf(() => game.objectService.rotateObjectBy({folder_object_id}, {{ x: 0, y: 1, z: 0 }}, 90));
console.log("report:" + JSON.stringify({{ rotation, rotateToError, rotateByError }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(&report["rotation"], Vec3::new(30.0, 120.0, -45.0), "rotation after rotateObjectTo");
    for error_name in ["rotateToError", "rotateByError"] {
        let error = report[error_name].as_str().unwrap_or_default();
        assert!(error.contains("was not a physical object"), "{error_name} was {error:?}");
    }
    Ok(())
}

// rotateObjectBy and rotateObjectTo turn an object with a Pivot around the Pivot's point, like its animators do.
#[tokio::test]
async fn object_service_rotate_functions_turn_an_object_around_its_pivot() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let door_object_id = 3;
    let pivot_object_id = 4;

    add_script(&mut world, script_object_id, root_object_id)?;
    // the door's center is 1 unit east of its hinge, which sits at the origin
    add_brick(&mut world, door_object_id, root_object_id, Vec3::new(1.0, 0.0, 0.0), false)?;
    add_pivot(&mut world, pivot_object_id, root_object_id, Some(door_object_id), Vec3::new(-1.0, 0.0, 0.0))?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const door = game.objectService.getObjectById({door_object_id});
game.objectService.rotateObjectBy({door_object_id}, {{ x: 0, y: 1, z: 0 }}, 90);
const positionAfterBy = {{ ...door.location.position }};
game.objectService.rotateObjectTo({door_object_id}, {{ x: 0, y: 180, z: 0 }});
const positionAfterTo = {{ ...door.location.position }};
console.log("report:" + JSON.stringify({{ positionAfterBy, positionAfterTo }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(
        &report["positionAfterBy"],
        Vec3::new(0.0, 0.0, -1.0),
        "door center after a quarter turn around the hinge",
    );
    assert_reported_vec3(
        &report["positionAfterTo"],
        Vec3::new(-1.0, 0.0, 0.0),
        "door center after turning to face the other way",
    );
    Ok(())
}

// scaleObjectBy grows an object with a Pivot around the Pivot's point, and the point moves out with the object's shape, so
// a turn afterwards still happens around the same hinge.
#[tokio::test]
async fn object_service_scale_object_by_scales_around_the_pivot_and_keeps_the_hinge() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let door_object_id = 3;
    let pivot_object_id = 4;

    add_script(&mut world, script_object_id, root_object_id)?;
    // the 2x2x2 door's center is 1 unit east of its hinge, which sits at the origin
    add_brick(&mut world, door_object_id, root_object_id, Vec3::new(1.0, 0.0, 0.0), false)?;
    add_pivot(&mut world, pivot_object_id, root_object_id, Some(door_object_id), Vec3::new(-1.0, 0.0, 0.0))?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const door = game.objectService.getObjectById({door_object_id});
game.objectService.scaleObjectBy({door_object_id}, 2);
const positionAfterScale = {{ ...door.location.position }};
game.objectService.rotateObjectBy({door_object_id}, {{ x: 0, y: 1, z: 0 }}, 90);
const positionAfterTurn = {{ ...door.location.position }};
console.log("report:" + JSON.stringify({{ positionAfterScale, positionAfterTurn }}));
"#
        ),
    );
    let report = run_scripts_and_read_report(&mut world).await?;

    assert_reported_vec3(
        &report["positionAfterScale"],
        Vec3::new(2.0, 0.0, 0.0),
        "door center after doubling around the hinge",
    );
    assert_reported_vec3(
        &report["positionAfterTurn"],
        Vec3::new(0.0, 0.0, -2.0),
        "door center after a quarter turn around the same hinge",
    );
    let ObjectInfo::Pivot(pivot) = &world.getObject(pivot_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected pivot object"));
    };
    assert!(
        (pivot.offsetFromObject - Vec3::new(-2.0, 0.0, 0.0)).length() < 1e-4,
        "pivot offset after scaling: {:?}",
        pivot.offsetFromObject
    );
    Ok(())
}

// scaleObjectBy grows what is attached to a scaled object along with it: a model's mesh keeps matching its collider, and a
// label stays on the surface it sits on with its text grown to match. A constant-size label keeps its size on screen.
#[tokio::test]
async fn object_service_scale_object_by_grows_attached_meshes_and_labels() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let (script_object_id, rigid_group_object_id, part_object_id) = (2, 3, 4);
    let (mesh_object_id, sign_label_object_id, nametag_label_object_id) = (5, 6, 7);

    add_script(&mut world, script_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::ZERO, None)?;
    add_brick(&mut world, part_object_id, rigid_group_object_id, Vec3::ZERO, false)?;
    add_mesh(&mut world, mesh_object_id, rigid_group_object_id)?;
    let ObjectInfo::Mesh(mut mesh) = world.getObject(mesh_object_id)?.info.clone() else {
        return Err(anyhow::anyhow!("Expected mesh object"));
    };
    mesh.physicalObjectId = Some(ObjectIdPointer(rigid_group_object_id));
    mesh.scale = Vec3::new(1.0, 2.0, 3.0);
    world.setMeshInfo(mesh_object_id, mesh)?;
    for (label_object_id, offset_from_object, is_constant_size) in [
        (sign_label_object_id, Vec3::new(0.0, 0.0, 1.0), false),
        (nametag_label_object_id, Vec3::new(0.0, 2.0, 0.0), true),
    ] {
        add_label(&mut world, label_object_id, root_object_id)?;
        let ObjectInfo::Label(mut label) = world.getObject(label_object_id)?.info.clone() else {
            return Err(anyhow::anyhow!("Expected label object"));
        };
        label.physicalObjectId = Some(ObjectIdPointer(part_object_id));
        label.offsetFromObject = offset_from_object;
        label.lineHeight = 0.5;
        label.maxWidth = Some(4.0);
        label.isConstantSize = is_constant_size;
        world.setLabelInfo(label_object_id, label)?;
    }
    world.markScriptAsReadyToRun(script_object_id, format!("game.objectService.scaleObjectBy({rigid_group_object_id}, 2);"));

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let ObjectInfo::Mesh(mesh) = &world.getObject(mesh_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected mesh object"));
    };
    assert!((mesh.scale - Vec3::new(2.0, 4.0, 6.0)).length() < 1e-5, "mesh scale {:?}", mesh.scale);
    let ObjectInfo::Label(sign_label) = &world.getObject(sign_label_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected sign label"));
    };
    assert!(
        (sign_label.offsetFromObject - Vec3::new(0.0, 0.0, 2.0)).length() < 1e-5,
        "sign offset {:?}",
        sign_label.offsetFromObject
    );
    assert_eq!((sign_label.lineHeight, sign_label.maxWidth), (1.0, Some(8.0)));
    let ObjectInfo::Label(nametag_label) = &world.getObject(nametag_label_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected nametag label"));
    };
    assert!(
        (nametag_label.offsetFromObject - Vec3::new(0.0, 4.0, 0.0)).length() < 1e-5,
        "nametag offset {:?}",
        nametag_label.offsetFromObject
    );
    assert_eq!((nametag_label.lineHeight, nametag_label.maxWidth), (0.5, Some(4.0)));
    Ok(())
}

#[tokio::test]
async fn object_service_scale_object_uses_recursive_world_positions_and_scale_for_rigid_group() -> anyhow::Result<()> {
    fn assert_close_f32(actual: f32, expected: f32, label: &str) {
        let difference = (actual - expected).abs();
        assert!(
            difference <= 1e-5,
            "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
        );
    }

    fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
        assert_close_f32(actual.x, expected.x, &format!("{label}.x"));
        assert_close_f32(actual.y, expected.y, &format!("{label}.y"));
        assert_close_f32(actual.z, expected.z, &format!("{label}.z"));
    }

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let rigid_group_object_id = 2;
    let child_ball_object_id = 3;
    let script_object_id = 4;

    // This creates a grouped child so the script-side scale call has to keep one shared pivot and resize the primitive.
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(8.0, 10.0, -6.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(8.0, 14.0, -6.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let scale_multiplier = 0.5;
    let rigid_group_position_before = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should have physics")
        .location
        .position;
    // with no Pivot, the group scales around its own position
    let center_point = rigid_group_position_before;
    let child_position_before = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should have physics")
        .location
        .position;
    let child_ball_primitive_settings_before = world
        .physics
        .getBallPrimitiveSettings(child_ball_object_id)
        .expect("child ball should have primitive settings");

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.scaleObjectBy({}, {});", rigid_group_object_id, scale_multiplier),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rigid_group_physics_info = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should still have physics after scaling");
    let child_ball_physics_info = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should still have physics after scaling");
    let expected_rigid_group_position = center_point + (rigid_group_position_before - center_point) * scale_multiplier;
    let expected_child_position = center_point + (child_position_before - center_point) * scale_multiplier;
    let child_ball_primitive_settings = world
        .physics
        .getBallPrimitiveSettings(child_ball_object_id)
        .expect("child ball should still have primitive settings after scaling");
    let expected_child_radius = child_ball_primitive_settings_before.radius * scale_multiplier.abs();

    // This checks both the pivoted child translation and the scaled primitive radius.
    assert_vec3_close(
        rigid_group_physics_info.location.position,
        expected_rigid_group_position,
        "rigid_group.position",
    );
    assert_vec3_close(child_ball_physics_info.location.position, expected_child_position, "child_ball.position");
    assert_close_f32(child_ball_primitive_settings.radius, expected_child_radius, "child_ball.radius");

    Ok(())
}

#[tokio::test]
async fn object_service_move_object_by_uses_recursive_world_offset_for_rigid_group() -> anyhow::Result<()> {
    fn assert_close_f32(actual: f32, expected: f32, label: &str) {
        let difference = (actual - expected).abs();
        assert!(
            difference <= 1e-5,
            "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
        );
    }

    fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
        assert_close_f32(actual.x, expected.x, &format!("{label}.x"));
        assert_close_f32(actual.y, expected.y, &format!("{label}.y"));
        assert_close_f32(actual.z, expected.z, &format!("{label}.z"));
    }

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let rigid_group_object_id = 2;
    let child_ball_object_id = 3;
    let script_object_id = 4;

    // This creates a physical parent so the script-side move call can offset the full subtree from one direct physics root.
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let position_offset = Vec3::new(-3.0, 2.5, 6.0);
    let rigid_group_position_before = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should have physics")
        .location
        .position;
    let child_position_before = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should have physics")
        .location
        .position;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "game.objectService.moveObjectBy({}, {{ x: {}, y: {}, z: {} }});",
            rigid_group_object_id, position_offset.x, position_offset.y, position_offset.z
        ),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rigid_group_physics_info = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should still have physics after moving");
    let child_ball_physics_info = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should still have physics after moving");

    // This checks that every physical descendant received the same world-space offset.
    assert_vec3_close(
        rigid_group_physics_info.location.position,
        rigid_group_position_before + position_offset,
        "rigid_group.position",
    );
    assert_vec3_close(
        child_ball_physics_info.location.position,
        child_position_before + position_offset,
        "child_ball.position",
    );

    Ok(())
}

#[tokio::test]
async fn object_service_move_object_by_moves_physical_descendants_under_nonphysical_root() -> anyhow::Result<()> {
    fn assert_close_f32(actual: f32, expected: f32, label: &str) {
        let difference = (actual - expected).abs();
        assert!(
            difference <= 1e-5,
            "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
        );
    }

    fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
        assert_close_f32(actual.x, expected.x, &format!("{label}.x"));
        assert_close_f32(actual.y, expected.y, &format!("{label}.y"));
        assert_close_f32(actual.z, expected.z, &format!("{label}.z"));
    }
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let local_group_object_id = 2;
    let rigid_group_object_id = 3;
    let child_ball_object_id = 4;
    let script_object_id = 5;
    let position_offset = Vec3::new(1.0, 2.0, 3.0);

    add_local_group(&mut world, local_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, local_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let rigid_group_position_before = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should have physics")
        .location
        .position;
    let child_position_before = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should have physics")
        .location
        .position;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "game.objectService.moveObjectBy({}, {{ x: {}, y: {}, z: {} }});",
            local_group_object_id, position_offset.x, position_offset.y, position_offset.z
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rigid_group_physics_info = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should still have physics after moving");
    let child_ball_physics_info = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should still have physics after moving");

    // This shows that moveObjectBy still applies the recursive offset to physical descendants even when the requested root is nonphysical.
    assert_vec3_close(
        rigid_group_physics_info.location.position,
        rigid_group_position_before + position_offset,
        "rigid_group.position",
    );
    assert_vec3_close(
        child_ball_physics_info.location.position,
        child_position_before + position_offset,
        "child_ball.position",
    );

    Ok(())
}

#[tokio::test]
async fn object_service_move_object_to_uses_recursive_world_offset_for_rigid_group() -> anyhow::Result<()> {
    fn assert_close_f32(actual: f32, expected: f32, label: &str) {
        let difference = (actual - expected).abs();
        assert!(
            difference <= 1e-5,
            "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
        );
    }

    fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
        assert_close_f32(actual.x, expected.x, &format!("{label}.x"));
        assert_close_f32(actual.y, expected.y, &format!("{label}.y"));
        assert_close_f32(actual.z, expected.z, &format!("{label}.z"));
    }

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let rigid_group_object_id = 2;
    let child_ball_object_id = 3;
    let script_object_id = 4;

    // This creates a physical parent so moveObjectTo can move the full subtree from one direct physics root.
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let target_position = Vec3::new(-3.0, 2.5, 6.0);
    let rigid_group_position_before = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should have physics")
        .location
        .position;
    let child_position_before = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should have physics")
        .location
        .position;
    let position_offset = target_position - rigid_group_position_before;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            "game.objectService.moveObjectTo({}, {{ x: {}, y: {}, z: {} }});",
            rigid_group_object_id, target_position.x, target_position.y, target_position.z
        ),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rigid_group_physics_info = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should still have physics after moving");
    let child_ball_physics_info = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should still have physics after moving");

    // This checks that the rigid group lands on the target point and its child keeps the same relative offset.
    assert_vec3_close(rigid_group_physics_info.location.position, target_position, "rigid_group.position");
    assert_vec3_close(
        child_ball_physics_info.location.position,
        child_position_before + position_offset,
        "child_ball.position",
    );

    Ok(())
}

#[tokio::test]
async fn object_service_move_object_to_rejects_nonphysical_root() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let local_group_object_id = 2;
    let rigid_group_object_id = 3;
    let child_ball_object_id = 4;
    let script_object_id = 5;

    add_local_group(&mut world, local_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, local_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, child_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let rigid_group_position_before = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should have physics")
        .location
        .position;
    let child_position_before = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should have physics")
        .location
        .position;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.moveObjectTo({}, {{ x: 1.0, y: 2.0, z: 3.0 }});", local_group_object_id),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let rigid_group_physics_info = world
        .getObjectPhysicsInfo3D(rigid_group_object_id)
        .expect("rigid group should still have physics after the rejected move");
    let child_ball_physics_info = world
        .getObjectPhysicsInfo3D(child_ball_object_id)
        .expect("child ball should still have physics after the rejected move");

    assert_eq!(rigid_group_physics_info.location.position, rigid_group_position_before);
    assert_eq!(child_ball_physics_info.location.position, child_position_before);

    Ok(())
}

#[tokio::test]
async fn object_service_set_object_walkthrough_sets_true_for_every_primitive_in_mixed_subtree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let subtree_group_object_id = 2;
    let rigid_group_object_id = 3;
    let grouped_ball_object_id = 4;
    let nested_group_object_id = 5;
    let nested_brick_object_id = 6;
    let outside_ball_object_id = 7;
    let script_object_id = 8;

    // This creates a mixed subtree so the walkthrough setter has to touch primitive descendants under both local and rigid groups.
    add_local_group(&mut world, subtree_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, subtree_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, grouped_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), false)?;
    add_local_group(&mut world, nested_group_object_id, subtree_group_object_id)?;
    add_brick(&mut world, nested_brick_object_id, nested_group_object_id, Vec3::new(-3.0, 2.0, 6.0), true)?;
    add_ball(&mut world, outside_ball_object_id, root_object_id, Vec3::new(12.0, 0.0, 3.0), true)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let grouped_ball_position_before = world
        .getObjectPhysicsInfo3D(grouped_ball_object_id)
        .expect("grouped ball should have physics")
        .location
        .position;
    let nested_brick_position_before = world
        .getObjectPhysicsInfo3D(nested_brick_object_id)
        .expect("nested brick should have physics")
        .location
        .position;

    assert!(
        !world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should have physics toggles")
            .isWalkthrough
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should have physics toggles")
            .isWalkthrough
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should have physics toggles")
            .isWalkthrough
    );

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.setObjectWalkthrough({}, true);", subtree_group_object_id),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert!(
        world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics toggles")
            .isWalkthrough
    );
    assert!(
        world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should keep physics toggles")
            .isWalkthrough
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should keep physics toggles")
            .isWalkthrough
    );

    // This checks that the walkthrough setter only changes collider flags and does not move the subtree.
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics info")
            .location
            .position,
        grouped_ball_position_before
    );
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(nested_brick_object_id)
            .expect("nested brick should keep physics info")
            .location
            .position,
        nested_brick_position_before
    );

    Ok(())
}

#[tokio::test]
async fn object_service_set_object_walkthrough_sets_false_for_every_primitive_in_mixed_subtree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let subtree_group_object_id = 2;
    let rigid_group_object_id = 3;
    let grouped_ball_object_id = 4;
    let nested_group_object_id = 5;
    let nested_brick_object_id = 6;
    let outside_ball_object_id = 7;
    let script_object_id = 8;

    add_local_group(&mut world, subtree_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, subtree_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, grouped_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), false)?;
    add_local_group(&mut world, nested_group_object_id, subtree_group_object_id)?;
    add_brick(&mut world, nested_brick_object_id, nested_group_object_id, Vec3::new(-3.0, 2.0, 6.0), true)?;
    add_ball(&mut world, outside_ball_object_id, root_object_id, Vec3::new(12.0, 0.0, 3.0), true)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    // This preloads walkthrough=true so the script has to flip the subtree back off without touching outside objects.
    world.setPhysicsToggles3D(
        grouped_ball_object_id,
        SetPhysicsToggles3D {
            isWalkthrough: Some(true),
            ..Default::default()
        },
        true,
    );
    world.setPhysicsToggles3D(
        nested_brick_object_id,
        SetPhysicsToggles3D {
            isWalkthrough: Some(true),
            ..Default::default()
        },
        true,
    );
    world.setPhysicsToggles3D(
        outside_ball_object_id,
        SetPhysicsToggles3D {
            isWalkthrough: Some(true),
            ..Default::default()
        },
        true,
    );

    let grouped_ball_position_before = world
        .getObjectPhysicsInfo3D(grouped_ball_object_id)
        .expect("grouped ball should have physics")
        .location
        .position;
    let nested_brick_position_before = world
        .getObjectPhysicsInfo3D(nested_brick_object_id)
        .expect("nested brick should have physics")
        .location
        .position;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.setObjectWalkthrough({}, false);", subtree_group_object_id),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert!(
        !world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics toggles")
            .isWalkthrough
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should keep physics toggles")
            .isWalkthrough
    );
    assert!(
        world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should keep physics toggles")
            .isWalkthrough
    );

    assert_eq!(
        world
            .getObjectPhysicsInfo3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics info")
            .location
            .position,
        grouped_ball_position_before
    );
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(nested_brick_object_id)
            .expect("nested brick should keep physics info")
            .location
            .position,
        nested_brick_position_before
    );

    Ok(())
}

#[tokio::test]
async fn object_service_set_object_walkthrough_includes_primitive_root() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let target_ball_object_id = 2;
    let outside_ball_object_id = 3;
    let script_object_id = 4;

    // This checks the include-root case so a primitive root can update itself without needing descendants.
    add_ball(&mut world, target_ball_object_id, root_object_id, Vec3::new(1.0, 2.0, 3.0), true)?;
    add_ball(&mut world, outside_ball_object_id, root_object_id, Vec3::new(4.0, 5.0, 6.0), true)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.setObjectWalkthrough({}, true);", target_ball_object_id),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert!(
        world
            .getObjectPhysicsToggles3D(target_ball_object_id)
            .expect("target ball should keep physics toggles")
            .isWalkthrough
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should keep physics toggles")
            .isWalkthrough
    );

    Ok(())
}

#[tokio::test]
async fn object_service_set_object_pushable_sets_false_for_every_primitive_in_mixed_subtree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let subtree_group_object_id = 2;
    let rigid_group_object_id = 3;
    let grouped_ball_object_id = 4;
    let nested_group_object_id = 5;
    let nested_brick_object_id = 6;
    let outside_ball_object_id = 7;
    let script_object_id = 8;

    // This creates a mixed subtree so pushable updates hit primitive descendants under both local and rigid groups.
    add_local_group(&mut world, subtree_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, subtree_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, grouped_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), true)?;
    add_local_group(&mut world, nested_group_object_id, subtree_group_object_id)?;
    add_brick(&mut world, nested_brick_object_id, nested_group_object_id, Vec3::new(-3.0, 2.0, 6.0), true)?;
    add_ball(&mut world, outside_ball_object_id, root_object_id, Vec3::new(12.0, 0.0, 3.0), true)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let grouped_ball_position_before = world
        .getObjectPhysicsInfo3D(grouped_ball_object_id)
        .expect("grouped ball should have physics")
        .location
        .position;
    let nested_brick_position_before = world
        .getObjectPhysicsInfo3D(nested_brick_object_id)
        .expect("nested brick should have physics")
        .location
        .position;

    assert!(
        world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should have physics toggles")
            .isPushable
    );
    assert!(
        world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should have physics toggles")
            .isPushable
    );
    assert!(
        world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should have physics toggles")
            .isPushable
    );

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.setObjectPushable({}, false);", subtree_group_object_id),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert!(
        !world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics toggles")
            .isPushable
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should keep physics toggles")
            .isPushable
    );
    assert!(
        world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should keep physics toggles")
            .isPushable
    );
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics info")
            .location
            .position,
        grouped_ball_position_before
    );
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(nested_brick_object_id)
            .expect("nested brick should keep physics info")
            .location
            .position,
        nested_brick_position_before
    );

    Ok(())
}

#[tokio::test]
async fn object_service_set_object_pushable_sets_true_for_every_primitive_in_mixed_subtree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let subtree_group_object_id = 2;
    let rigid_group_object_id = 3;
    let grouped_ball_object_id = 4;
    let nested_group_object_id = 5;
    let nested_brick_object_id = 6;
    let outside_ball_object_id = 7;
    let script_object_id = 8;

    add_local_group(&mut world, subtree_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, subtree_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, grouped_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), true)?;
    add_local_group(&mut world, nested_group_object_id, subtree_group_object_id)?;
    add_brick(&mut world, nested_brick_object_id, nested_group_object_id, Vec3::new(-3.0, 2.0, 6.0), true)?;
    add_ball(&mut world, outside_ball_object_id, root_object_id, Vec3::new(12.0, 0.0, 3.0), true)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    // This preloads pushable=false so the script has to flip the subtree back on without touching outside objects.
    world.setPhysicsToggles3D(
        grouped_ball_object_id,
        SetPhysicsToggles3D {
            isPushable: Some(false),
            ..Default::default()
        },
        true,
    );
    world.setPhysicsToggles3D(
        nested_brick_object_id,
        SetPhysicsToggles3D {
            isPushable: Some(false),
            ..Default::default()
        },
        true,
    );
    world.setPhysicsToggles3D(
        outside_ball_object_id,
        SetPhysicsToggles3D {
            isPushable: Some(false),
            ..Default::default()
        },
        true,
    );

    let grouped_ball_position_before = world
        .getObjectPhysicsInfo3D(grouped_ball_object_id)
        .expect("grouped ball should have physics")
        .location
        .position;
    let nested_brick_position_before = world
        .getObjectPhysicsInfo3D(nested_brick_object_id)
        .expect("nested brick should have physics")
        .location
        .position;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.setObjectPushable({}, true);", subtree_group_object_id),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert!(
        world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics toggles")
            .isPushable
    );
    assert!(
        world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should keep physics toggles")
            .isPushable
    );
    assert!(
        !world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should keep physics toggles")
            .isPushable
    );

    assert_eq!(
        world
            .getObjectPhysicsInfo3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics info")
            .location
            .position,
        grouped_ball_position_before
    );
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(nested_brick_object_id)
            .expect("nested brick should keep physics info")
            .location
            .position,
        nested_brick_position_before
    );

    Ok(())
}

#[tokio::test]
async fn object_service_set_object_density_sets_value_for_every_primitive_in_mixed_subtree() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let subtree_group_object_id = 2;
    let rigid_group_object_id = 3;
    let grouped_ball_object_id = 4;
    let nested_group_object_id = 5;
    let nested_brick_object_id = 6;
    let outside_ball_object_id = 7;
    let script_object_id = 8;

    // This creates a mixed subtree so density updates hit primitive descendants under both local and rigid groups.
    add_local_group(&mut world, subtree_group_object_id, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, subtree_group_object_id, Vec3::new(5.0, 1.0, -2.0), None)?;
    add_ball(&mut world, grouped_ball_object_id, rigid_group_object_id, Vec3::new(7.0, 4.0, -1.0), true)?;
    add_local_group(&mut world, nested_group_object_id, subtree_group_object_id)?;
    add_brick(&mut world, nested_brick_object_id, nested_group_object_id, Vec3::new(-3.0, 2.0, 6.0), true)?;
    add_ball(&mut world, outside_ball_object_id, root_object_id, Vec3::new(12.0, 0.0, 3.0), true)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    let grouped_ball_position_before = world
        .getObjectPhysicsInfo3D(grouped_ball_object_id)
        .expect("grouped ball should have physics")
        .location
        .position;
    let nested_brick_position_before = world
        .getObjectPhysicsInfo3D(nested_brick_object_id)
        .expect("nested brick should have physics")
        .location
        .position;

    assert_eq!(
        world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should have physics toggles")
            .density,
        1.0
    );
    assert_eq!(
        world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should have physics toggles")
            .density,
        2.0
    );
    assert_eq!(
        world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should have physics toggles")
            .density,
        1.0
    );

    world.markScriptAsReadyToRun(
        script_object_id,
        format!("game.objectService.setObjectDensity({}, 6.5);", subtree_group_object_id),
    );

    // This loads the bundled JS API and runs the script through the real runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert_eq!(
        world
            .getObjectPhysicsToggles3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics toggles")
            .density,
        6.5
    );
    assert_eq!(
        world
            .getObjectPhysicsToggles3D(nested_brick_object_id)
            .expect("nested brick should keep physics toggles")
            .density,
        6.5
    );
    assert_eq!(
        world
            .getObjectPhysicsToggles3D(outside_ball_object_id)
            .expect("outside ball should keep physics toggles")
            .density,
        1.0
    );

    // This checks that the density setter only changes collider density and does not move the subtree.
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(grouped_ball_object_id)
            .expect("grouped ball should keep physics info")
            .location
            .position,
        grouped_ball_position_before
    );
    assert_eq!(
        world
            .getObjectPhysicsInfo3D(nested_brick_object_id)
            .expect("nested brick should keep physics info")
            .location
            .position,
        nested_brick_position_before
    );

    Ok(())
}

#[tokio::test]
async fn object_service_can_create_and_modify_skeletons_and_joints_through_ts() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;

    add_script(&mut world, script_object_id, root_object_id)?;
    world.objectIdCounter = world
        .objectOfId
        .keys()
        .copied()
        .max()
        .expect("Expected script setup to leave at least one object in the world");

    // This exercises the exact script-side flow we want to rely on: create objects, edit fields, and reparent joints.
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const skeleton = game.objectService.createObject(
    "test_skeleton",
    {{
        type: "skeleton",
    }},
    game.world.id,
);
if (skeleton === null) {{
    throw new Error("Expected created skeleton: null");
}}
if (skeleton.type !== "skeleton") {{
    throw new Error("Expected created skeleton type, got " + skeleton.type);
}}

const hipJoint = game.objectService.createObject(
    "test_hip_joint",
    {{
        type: "skJoint",
        restPose: {{
            position: {{ x: 1.0, y: 2.0, z: 3.0 }},
            rotation: {{ x: 0.0, y: 0.0, z: 0.0 }},
        }},
    }},
    skeleton.id,
);
if (hipJoint === null || hipJoint.type !== "skJoint") {{
    throw new Error("Expected created hip joint");
}}

const headJoint = game.objectService.createObject(
    "test_head_joint",
    {{
        type: "skJoint",
        restPose: {{
            position: {{ x: 7.0, y: 8.0, z: 9.0 }},
            rotation: {{ x: 0.0, y: 0.0, z: 0.0 }},
        }},
    }},
    skeleton.id,
);
if (headJoint === null || headJoint.type !== "skJoint") {{
    throw new Error("Expected created head joint");
}}

hipJoint.restPose.position.x = 13.0;
hipJoint.restPose.position.y = 14.0;
hipJoint.restPose.rotation.y = 90.0;

headJoint.parent = hipJoint;
headJoint.restPose.position = {{ x: 15.0, y: 16.0, z: 17.0 }};
"#
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // This checks that script-side creation produced the expected world tree and object data.
    let skeleton_object_id = *world
        .getObjectIdsByName("test_skeleton")
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected created skeleton object"))?;
    let hip_joint_object_id = *world
        .getObjectIdsByName("test_hip_joint")
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected created hip joint object"))?;
    let head_joint_object_id = *world
        .getObjectIdsByName("test_head_joint")
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected created head joint object"))?;

    let skeleton_object = world.getObject(skeleton_object_id)?;
    let ObjectInfo::Skeleton(_) = &skeleton_object.info else {
        return Err(anyhow::anyhow!("Expected created skeleton object info"));
    };

    let hip_joint_object = world.getObject(hip_joint_object_id)?;
    let ObjectInfo::SkJoint(hip_sk_joint_info) = &hip_joint_object.info else {
        return Err(anyhow::anyhow!("Expected created hip joint info"));
    };
    assert_eq!(hip_sk_joint_info.restPose.position, Vec3::new(13.0, 14.0, 3.0));
    assert_eq!(hip_sk_joint_info.restPose.quaternion, Quat::from_euler(EulerRot::XYZ, 0.0, FRAC_PI_2, 0.0));

    let head_joint_object = world.getObject(head_joint_object_id)?;
    let ObjectInfo::SkJoint(head_sk_joint_info) = &head_joint_object.info else {
        return Err(anyhow::anyhow!("Expected created head joint info"));
    };
    assert_eq!(head_sk_joint_info.restPose.position, Vec3::new(15.0, 16.0, 17.0));
    assert_eq!(world.parentOfId.get(&hip_joint_object_id).copied(), Some(skeleton_object_id));
    assert_eq!(world.parentOfId.get(&head_joint_object_id).copied(), Some(hip_joint_object_id));

    // This also proves the Skeleton cache stays in sync after field edits and TS-side reparenting.
    let render_skeleton_info_arc = world
        .getRenderSkeletonInfoArc(skeleton_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected render skeleton info after TS edits"))?;
    let render_skeleton_info = render_skeleton_info_arc.as_ref();
    assert_eq!(render_skeleton_info.jointOfIndex.len(), 2);
    assert_eq!(render_skeleton_info.jointOfIndex[0].name, "test_hip_joint");
    assert_eq!(render_skeleton_info.jointOfIndex[1].name, "test_head_joint");
    assert_eq!(render_skeleton_info.jointOfIndex[1].parent_joint_index, Some(0));

    Ok(())
}

#[tokio::test]
async fn object_service_can_create_sk_attachments_in_rigid_group_for_matching_items() -> anyhow::Result<()> {
    fn assert_attachment_position(world: &shared::game::world::World, attachment_name: &str, expected_position: Vec3) -> anyhow::Result<()> {
        let attachment_object_id = *world
            .getObjectIdsByName(attachment_name)
            .first()
            .ok_or_else(|| anyhow::anyhow!("Expected {attachment_name}"))?;
        let attachment_object = world.getObject(attachment_object_id)?;
        let ObjectInfo::SkAttachment(attachment_info) = &attachment_object.info else {
            return Err(anyhow::anyhow!("Expected {attachment_name} to be a SkAttachment"));
        };
        assert_eq!(attachment_info.offsetFromJoint.position, expected_position);
        assert_eq!(attachment_info.offsetFromJoint.quaternion, Quat::IDENTITY);
        Ok(())
    }

    fn get_brick_object_id(world: &shared::game::world::World, object_name: &str) -> anyhow::Result<ObjectId> {
        let object_id = *world
            .getObjectIdsByName(object_name)
            .iter()
            .find(|object_id| {
                let Ok(object) = world.getObject(**object_id) else {
                    return false;
                };
                let ObjectInfo::Brick(_) = &object.info else {
                    return false;
                };
                true
            })
            .ok_or_else(|| anyhow::anyhow!("Expected {object_name} brick"))?;
        Ok(object_id)
    }

    fn get_ball_object_id(world: &shared::game::world::World, object_name: &str) -> anyhow::Result<ObjectId> {
        let object_id = *world
            .getObjectIdsByName(object_name)
            .iter()
            .find(|object_id| {
                let Ok(object) = world.getObject(**object_id) else {
                    return false;
                };
                let ObjectInfo::Ball(_) = &object.info else {
                    return false;
                };
                true
            })
            .ok_or_else(|| anyhow::anyhow!("Expected {object_name} ball"))?;
        Ok(object_id)
    }

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;

    add_script(&mut world, script_object_id, root_object_id)?;
    world.objectIdCounter = world
        .objectOfId
        .keys()
        .copied()
        .max()
        .expect("Expected script setup to leave at least one object in the world");

    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
const v = (x, y, z) => ({ x, y, z });
const loc = (position) => ({ position, rotation: v(0, 0, 0) });
const phys = (position) => ({
    location: loc(position),
    movement: {
        velocity: v(0, 0, 0),
        angularVelocity: v(0, 0, 0),
    },
});
const brickInfo = (position) => ({
    type: "brick",
    material: null,
    opacity: 1,
    color: { r: 1, g: 0, b: 0 },
    isAlwaysOnTop: false,
    size: v(1, 1, 1),
    isWalkthrough: false,
    isPushable: false,
    density: 1,
    ...phys(position),
});
const ballInfo = (position) => ({
    type: "ball",
    material: null,
    opacity: 1,
    color: { r: 1, g: 0, b: 0 },
    isAlwaysOnTop: false,
    radius: 0.5,
    isWalkthrough: false,
    isPushable: false,
    density: 1,
    ...phys(position),
});

const skeleton = game.objectService.createObject("AutoAttachSkeleton", { type: "skeleton" }, game.world.id);
if (skeleton === null) throw new Error("Expected skeleton");
const torsoJoint = game.objectService.createObject("Torso", { type: "skJoint", restPose: loc(v(1, 2, 3)) }, skeleton.id);
if (torsoJoint === null) throw new Error("Expected torso joint");
const armJoint = game.objectService.createObject("Arm", { type: "skJoint", restPose: loc(v(0, 1, 0)) }, torsoJoint.id);
if (armJoint === null) throw new Error("Expected arm joint");

const rigidGroup = game.objectService.createObject(
    "AutoAttachRigidGroup",
    { type: "rigidGroup", followSkeletonId: skeleton.id, ...phys(v(10, 0, 0)) },
    game.world.id,
);
if (rigidGroup === null) throw new Error("Expected rigid group");
const torso = game.objectService.createObject("Torso", brickInfo(v(2, 4, 6)), rigidGroup.id);
if (torso === null) throw new Error("Expected torso brick");
const existingTorsoAttachment = game.objectService.createObject("TorsoSkAttachment", {
    type: "skAttachment",
    objectId: torso.id,
    jointName: "Torso",
    offsetFromJoint: loc(v(-100, -100, -100)),
}, game.world.id);
if (existingTorsoAttachment === null) throw new Error("Expected existing torso attachment");
const stripe = game.objectService.createObject("Stripe", brickInfo(v(3, 5, 7)), torso.id);
if (stripe === null) throw new Error("Expected stripe brick");
const arm = game.objectService.createObject("Arm", ballInfo(v(2, 5, 6)), rigidGroup.id);
if (arm === null) throw new Error("Expected arm ball");
const noMatch = game.objectService.createObject("NoMatch", brickInfo(v(10, 10, 10)), rigidGroup.id);
if (noMatch === null) throw new Error("Expected no-match brick");

const attachments = game.objectService.createSkAttachmentsInRigidGroup(skeleton.id, rigidGroup.id);
if (attachments.length !== 3) throw new Error("Expected 3 attachments, got " + attachments.length);
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let torso_object_id = get_brick_object_id(&world, "Torso")?;
    let torso_attachment_object_id = *world
        .getObjectIdsByName("TorsoSkAttachment")
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected TorsoSkAttachment"))?;
    let torso_attachment_object = world.getObject(torso_attachment_object_id)?;
    let ObjectInfo::SkAttachment(torso_attachment_info) = &torso_attachment_object.info else {
        return Err(anyhow::anyhow!("Expected TorsoSkAttachment info"));
    };
    assert_eq!(torso_attachment_info.objectId.as_ref().map(ObjectId::from), Some(torso_object_id));
    assert_eq!(torso_attachment_info.jointName, "Torso");

    // Attachment offsets are measured against the group-composed joint transforms (SkKeyframePoses
    // are local to the following rigid group's frame, and the group sits at (10, 0, 0) here).
    assert_attachment_position(&world, "TorsoSkAttachment", Vec3::new(-9.0, 2.0, 3.0))?;
    assert_attachment_position(&world, "StripeSkAttachment", Vec3::new(-8.0, 3.0, 4.0))?;
    assert_attachment_position(&world, "ArmSkAttachment", Vec3::new(-9.0, 2.0, 3.0))?;
    assert!(world.getObjectIdsByName("NoMatchSkAttachment").is_empty());

    let stripe_object_id = get_brick_object_id(&world, "Stripe")?;
    let arm_object_id = get_ball_object_id(&world, "Arm")?;
    for (attachment_name, attached_object_id) in [
        ("TorsoSkAttachment", torso_object_id),
        ("StripeSkAttachment", stripe_object_id),
        ("ArmSkAttachment", arm_object_id),
    ] {
        let attachment_object_id = *world
            .getObjectIdsByName(attachment_name)
            .first()
            .ok_or_else(|| anyhow::anyhow!("Expected {attachment_name}"))?;
        assert_eq!(world.parentOfId.get(&attachment_object_id).copied(), Some(attached_object_id));
    }

    Ok(())
}

#[tokio::test]
async fn object_service_get_objects_by_type_returns_live_objects() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let local_group_object_id_1 = 3;
    let local_group_object_id_2 = 4;
    let rigid_group_object_id = 5;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_local_group(&mut world, local_group_object_id_1, root_object_id)?;
    add_local_group(&mut world, local_group_object_id_2, root_object_id)?;
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::ZERO, None)?;

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const groups = game.objectService.getObjectsByType("group");
if (groups.length !== 3) {{
    throw new Error("Expected 3 groups, got " + groups.length);
}}

const groupIds = groups.map((group) => group.id).sort((a, b) => a - b).join(",");
if (groupIds !== "{},{},{}") {{
    throw new Error("Unexpected group ids: " + groupIds);
}}

const group1 = groups.find((group) => group.id === {});
if (group1 === undefined) {{
    throw new Error("Expected group {}");
}}
if (group1.type !== "group") {{
    throw new Error("Expected group type, got " + group1.type);
}}

group1.name = "local-group-{}-renamed";
"#,
            root_object_id, local_group_object_id_1, local_group_object_id_2, local_group_object_id_1, local_group_object_id_1, local_group_object_id_1
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let renamed_group_object = world.getObject(local_group_object_id_1)?;
    let ObjectInfo::LocalGroup(_) = &renamed_group_object.info else {
        return Err(anyhow::anyhow!("Expected local group info"));
    };
    assert_eq!(renamed_group_object.name, format!("local-group-{}-renamed", local_group_object_id_1));

    Ok(())
}

#[tokio::test]
async fn object_service_clone_object_returns_a_live_object_from_ts() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let source_group_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_local_group(&mut world, source_group_object_id, root_object_id)?;
    world.objectIdCounter = world
        .objectOfId
        .keys()
        .copied()
        .max()
        .expect("Expected script setup to leave at least one object in the world");

    // This exercises the clone return path directly so scripts can keep working with the cloned object without a second lookup.
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
const sourceGroup = game.objectService.getObjectByName("local-group-3");
if (sourceGroup === null || sourceGroup.type !== "group") {
    throw new Error("Expected source group");
}

const clonedGroup = game.objectService.cloneObject(sourceGroup.id, "cloned_group");
if (clonedGroup === null) {
    throw new Error("Expected cloned group");
}
if (clonedGroup.type !== "group") {
    throw new Error("Expected cloned group type, got " + clonedGroup.type);
}

clonedGroup.name = "cloned_group_renamed";
clonedGroup.parent = sourceGroup;

const clonedParent = clonedGroup.parent;
if (clonedParent === null || clonedParent.id !== sourceGroup.id) {
    throw new Error("Expected cloned group parent to be the source group");
}

const sourceChildren = sourceGroup.getChildren();
if (!sourceChildren.some((child) => child.id === clonedGroup.id)) {
    throw new Error("Expected source group children to include cloned group");
}

"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let cloned_group_object_id = *world
        .getObjectIdsByName("cloned_group_renamed")
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected cloned group object"))?;
    let cloned_group_object = world.getObject(cloned_group_object_id)?;
    let ObjectInfo::LocalGroup(_) = &cloned_group_object.info else {
        return Err(anyhow::anyhow!("Expected cloned group info"));
    };
    assert_eq!(world.parentOfId.get(&cloned_group_object_id).copied(), Some(source_group_object_id));

    Ok(())
}

#[tokio::test]
async fn object_service_clone_object_rejects_missing_source_object() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;

    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(script_object_id, r#"game.objectService.cloneObject(999, "missing_clone");"#.to_string());

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    assert!(
        world.getObjectIdsByName("missing_clone").is_empty(),
        "Expected rejected cloneObject to leave the world unchanged"
    );

    Ok(())
}

#[tokio::test]
async fn object_service_parent_setter_rejects_missing_parent_object() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let source_group_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_local_group(&mut world, source_group_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
const sourceGroup = game.objectService.getObjectById({});
if (sourceGroup === null || sourceGroup.type !== "group") {{
    throw new Error("Expected source group");
}}
sourceGroup.parent = {{ id: 999 }};
"#,
            source_group_object_id
        ),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    assert_eq!(world.parentOfId.get(&source_group_object_id).copied(), Some(root_object_id));

    Ok(())
}

// A client can create objects, but only in its own world, with ids of its own.
#[tokio::test]
async fn object_service_creates_client_world_objects_with_ids_of_its_own() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;

    add_client_script(&mut world, script_object_id, root_object_id);
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
game.objectService.createObject(
    "client_created_skeleton",
    {
        type: "skeleton",
    },
    game.world.id,
);
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    assert_eq!(world.getObjectIdsByName("client_created_skeleton"), vec![TOP_CLIENT_OBJECT_ID]);

    Ok(())
}
