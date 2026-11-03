#[path = "../../shared/tests/helpers/world_test_helpers.rs"]
mod world_test_helpers;

use deno_core::JsRuntime;
use game_types::consts::{CLIENT_SIM_AND_SEND_SECS, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES};
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, Orientation3D, PhysicsInfo3D, ScriptInfo};
use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
use glam::{Quat, Vec3};
use shared::game::jsRuntime::{
    BuiltinMouseCursor, CameraLimits, JsConsoleLogEntry, JsRuntimeCameraOrientation, JsRuntimeCameraZoom, JsRuntimeDesiredCameraZoom, JsRuntimeInitialization,
    MouseCursor, ScriptManager, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime, runJsCallbacks,
};
use shared::game::migrateStoredWorld::{CHARACTER_CONTROLLER_SCRIPT, CHARACTER_CONTROLLER_SCRIPT_NAME};
use shared::game::physics::SetPhysicsInfo3D;
use shared::game::world::{JsCallbackInputs, World};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use world_test_helpers::{add_ball, add_brick, add_player, add_rigid_group, make_world_controlled_by_player};

const PLAYER_OBJECT_ID: ObjectId = 90;
const CHARACTER_OBJECT_ID: ObjectId = 91;
const FLOOR_OBJECT_ID: ObjectId = 92;
const SCRIPT_OBJECT_ID: ObjectId = 93;
const LEDGE_OBJECT_ID: ObjectId = 94;
const CEILING_OBJECT_ID: ObjectId = 95;
const DT_SECS: f32 = CLIENT_SIM_AND_SEND_SECS;
const SET_MOVEMENT_FN_ID: &str = "setMovement#root/controller";
// the controller script's movementSettings
const DEFAULT_SPEED: f32 = 7.0;
const DEFAULT_JUMP_HEIGHT: f32 = 3.0;
const TALL_CHARACTER_SIZE: Vec3 = Vec3::new(1.0, 2.0, 1.0);
const STANDING_Y: f32 = 2.0; // the tall character standing on a floor whose top is at y = 1
const BEFORE_STEP: JsCallbackInputs = JsCallbackInputs::BeforeCharacterStep {
    character_object_id: CHARACTER_OBJECT_ID,
    dt_secs: DT_SECS,
};
const AFTER_STEP: JsCallbackInputs = JsCallbackInputs::AfterCharacterStep {
    character_object_id: CHARACTER_OBJECT_ID,
    dt_secs: DT_SECS,
};

// -------------------------- helpers --------------------------

fn add_sized_brick(world: &mut World, object_id: ObjectId, position: Vec3, size: Vec3, rotation: Quat, is_pushable: bool) -> anyhow::Result<()> {
    let root_object_id = world.getRootObjectId();
    add_brick(world, object_id, root_object_id, position, is_pushable)?;
    world.setPrimitiveSettings(object_id, PrimitiveSettings::Brick(BrickPrimitiveSettings { size }), true)?;
    world.forcePhysicsInfo3D(
        object_id,
        SetPhysicsInfo3D {
            rotation: Some(rotation),
            ..Default::default()
        },
        false,
    );
    Ok(())
}

// The floor's top is at y = 1 unless it is rotated.
fn make_world(floor_size: Vec3, floor_rotation: Quat, character_position: Vec3, character_size: Vec3) -> anyhow::Result<World> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_sized_brick(&mut world, FLOOR_OBJECT_ID, Vec3::ZERO, floor_size, floor_rotation, false)?;
    add_sized_brick(&mut world, CHARACTER_OBJECT_ID, character_position, character_size, Quat::IDENTITY, true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    Ok(world)
}

fn make_flat_world(character_size: Vec3) -> anyhow::Result<World> {
    make_world(
        Vec3::new(400.0, 2.0, 400.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 1.0 + character_size.y / 2.0, 0.0),
        character_size,
    )
}

fn settle(world: &mut World, num_steps: usize) -> anyhow::Result<()> {
    for _ in 0..num_steps {
        world.step_physics(1, DT_SECS, 0.0, None)?;
    }
    Ok(())
}

fn character_physics(world: &World) -> PhysicsInfo3D {
    world
        .getObjectPhysicsInfo3D(CHARACTER_OBJECT_ID)
        .expect("the character should be a physical object")
}

fn set_character_rotation(world: &mut World, rotation: Quat) {
    world.forcePhysicsInfo3D(
        CHARACTER_OBJECT_ID,
        SetPhysicsInfo3D {
            rotation: Some(rotation),
            ..Default::default()
        },
        false,
    );
}

fn yaw_of(rotation: Quat) -> f32 {
    let forward = rotation * Vec3::Z;
    forward.x.atan2(forward.z)
}

struct Controller {
    js_runtime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    console_log_entry_rx: mpsc::Receiver<JsConsoleLogEntry>,
}

async fn start_default_controller(world: &mut World) -> anyhow::Result<Controller> {
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        CHARACTER_CONTROLLER_SCRIPT_NAME.to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(SCRIPT_OBJECT_ID, CHARACTER_CONTROLLER_SCRIPT.to_string());

    let (console_log_entry_tx, console_log_entry_rx) = mpsc::channel();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await?;
    Ok(Controller {
        js_runtime,
        script_manager_rc,
        console_log_entry_rx,
    })
}

impl Controller {
    fn run(&mut self, world: &mut World, step_time_ms: f64, inputs: Vec<JsCallbackInputs>) -> anyhow::Result<()> {
        runJsCallbacks(&mut self.js_runtime, world, step_time_ms, Some(step_time_ms), &self.script_manager_rc, inputs)
    }

    // What a server script's setMovement(player.id, changes) call runs on this client.
    async fn set_movement(&mut self, world: &mut World, changes: serde_json::Value) -> anyhow::Result<()> {
        let set_movement = self
            .script_manager_rc
            .borrow()
            .get_client_js_fn(SET_MOVEMENT_FN_ID)
            .expect("the controller should export setMovement")
            .clone();
        let args = serde_json::json!({ "json": [1, changes] }).to_string();
        callJsFn(&mut self.js_runtime, world, 0.0, Some(0.0), &set_movement, &args).await?;
        Ok(())
    }

    fn hold(&mut self, world: &mut World, key: &str) -> anyhow::Result<()> {
        self.run(world, 0.0, vec![JsCallbackInputs::KeyDown { key: key.to_string() }])
    }

    fn release(&mut self, world: &mut World, key: &str) -> anyhow::Result<()> {
        self.run(world, 0.0, vec![JsCallbackInputs::KeyUp { key: key.to_string() }])
    }

    fn look(&mut self, theta: f32) {
        self.js_runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeCameraOrientation>()
            .set(Orientation3D { theta, phi: 0.0 });
    }

    // The calls the client play loop makes around each physics step.
    fn step(&mut self, world: &mut World, step_index: usize) -> anyhow::Result<()> {
        let step_time_ms = step_index as f64 * DT_SECS as f64 * 1000.0;
        self.run(world, step_time_ms, vec![BEFORE_STEP])?;
        world.step_physics(1, DT_SECS, step_time_ms, Some(step_time_ms))?;
        self.run(world, step_time_ms, vec![AFTER_STEP])
    }

    fn assert_no_console_errors(&self) {
        let errors: Vec<String> = self
            .console_log_entry_rx
            .try_iter()
            .filter(|console_log_entry| console_log_entry.log_level == "error")
            .map(|console_log_entry| console_log_entry.message)
            .collect();
        assert!(errors.is_empty(), "the controller logged errors: {errors:?}");
    }
}

// -------------------------- behavior --------------------------

#[tokio::test]
async fn the_character_walks_where_the_camera_looks() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    set_character_rotation(&mut world, Quat::from_rotation_y(2.0));
    let start = character_physics(&world).location.position;

    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    let mut grounded_steps = 0;
    for step_index in 0..120 {
        controller.step(&mut world, step_index)?;
        if world
            .getGroundSupport(CHARACTER_OBJECT_ID, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
            .is_some()
        {
            grounded_steps += 1;
        }
    }

    let end = character_physics(&world);
    let travelled = end.location.position - start;
    println!("travelled {travelled:?}, velocity {:?}, grounded {grounded_steps}/120", end.movement.velocity);
    assert!(
        travelled.z > 1.8 * DEFAULT_SPEED && travelled.x.abs() < 0.5,
        "should walk along +Z: {travelled:?}"
    );
    assert!(
        (end.movement.velocity.z - DEFAULT_SPEED).abs() < 0.5,
        "should reach walking speed: {:?}",
        end.movement.velocity
    );
    assert!(grounded_steps > 110, "should stay on the ground, was grounded {grounded_steps}/120 steps");
    assert!(
        yaw_of(end.location.quaternion).abs() < 0.3,
        "should turn toward +Z: {}",
        yaw_of(end.location.quaternion)
    );
    assert!((end.location.quaternion * Vec3::Y).y > 0.999, "should stay upright");
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn the_arrow_keys_walk_like_wasd() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    let start = character_physics(&world).location.position;

    controller.look(0.0);
    controller.hold(&mut world, "ArrowLeft")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }

    // with the camera looking along +Z, left is +X
    let travelled = character_physics(&world).location.position - start;
    assert!(travelled.x > 5.0 && travelled.z.abs() < 0.5, "should walk left: {travelled:?}");
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn a_walking_turn_reaches_its_heading_without_passing_it() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    set_character_rotation(&mut world, Quat::from_rotation_y(172_f32.to_radians()));

    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    let mut min_yaw = f32::INFINITY;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
        min_yaw = min_yaw.min(yaw_of(character_physics(&world).location.quaternion));
    }

    let yaw = yaw_of(character_physics(&world).location.quaternion);
    println!("{}° off after 1 s, lowest yaw {}°", yaw.to_degrees(), min_yaw.to_degrees());
    assert!(yaw.abs() < 6_f32.to_radians(), "should face +Z a second into the turn: {}°", yaw.to_degrees());
    assert!(min_yaw > -1_f32.to_radians(), "should not turn past +Z: {}°", min_yaw.to_degrees());
    controller.assert_no_console_errors();
    Ok(())
}

// A speed of 0 keeps the character standing, so the floor's friction holds it for the whole turn.
#[tokio::test]
async fn a_standing_turn_reaches_its_heading() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.set_movement(&mut world, serde_json::json!({ "speed": 0.0 })).await?;
    set_character_rotation(&mut world, Quat::from_rotation_y(1.0));

    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }

    let yaw = yaw_of(character_physics(&world).location.quaternion);
    println!("{}° off after 1 s", yaw.to_degrees());
    assert!(yaw.abs() < 6_f32.to_radians(), "should face +Z a second into the turn: {}°", yaw.to_degrees());
    controller.assert_no_console_errors();
    Ok(())
}

// Shift turns shift lock on, which locks the mouse, shows a crosshair, moves the camera over the right shoulder and turns even a
// standing character to face where the camera looks. Shift again turns it off, and a standing character then keeps its facing.
#[tokio::test]
async fn shift_lock_turns_a_standing_character_to_the_camera() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;

    controller.look(1.0);
    controller.hold(&mut world, "ShiftLeft")?;
    controller.release(&mut world, "ShiftLeft")?;
    let CameraLimits { is_mouse_locked, offset, .. } = controller.script_manager_rc.borrow().camera_limits();
    assert!(is_mouse_locked, "shift lock should lock the mouse");
    assert!(
        offset.x > 0.0 && offset.y == 0.0 && offset.z == 0.0,
        "shift lock should move the camera to its right: {offset}"
    );
    assert_eq!(
        controller.script_manager_rc.borrow().mouse_cursor(),
        MouseCursor::Builtin(BuiltinMouseCursor::Crosshair),
        "shift lock should show a crosshair"
    );
    for step_index in 0..15 {
        controller.step(&mut world, step_index)?;
    }
    let shift_lock_yaw = yaw_of(character_physics(&world).location.quaternion);
    println!("{}° after 0.25 s", shift_lock_yaw.to_degrees());
    // The floor's friction stops the last of the turn, once it's slower than friction can hold back.
    assert!(
        (shift_lock_yaw - 1.0).abs() < 2_f32.to_radians(),
        "should face the camera a quarter second in: {}°",
        shift_lock_yaw.to_degrees()
    );

    controller.hold(&mut world, "ShiftRight")?;
    controller.release(&mut world, "ShiftRight")?;
    let CameraLimits { is_mouse_locked, offset, .. } = controller.script_manager_rc.borrow().camera_limits();
    assert!(!is_mouse_locked, "turning shift lock off should free the mouse");
    assert_eq!(offset, Vec3::ZERO, "turning shift lock off should put the camera back");
    assert_eq!(
        controller.script_manager_rc.borrow().mouse_cursor(),
        MouseCursor::Builtin(BuiltinMouseCursor::Default),
        "turning shift lock off should put the cursor back"
    );
    controller.look(-1.0);
    for step_index in 15..75 {
        controller.step(&mut world, step_index)?;
    }
    let yaw = yaw_of(character_physics(&world).location.quaternion);
    assert!(
        (yaw - shift_lock_yaw).abs() < 1_f32.to_radians(),
        "should keep facing where it was: {}° after {}°",
        yaw.to_degrees(),
        shift_lock_yaw.to_degrees()
    );
    controller.assert_no_console_errors();
    Ok(())
}

// Shift lock's offset shrinks as the player zooms in, to nothing at zoom 0, so a camera zoomed all the way in sits in the head. It
// follows the zoom the player picked, so a wall that pulls the camera in to 0.2 doesn't shrink it.
#[tokio::test]
async fn shift_lock_moves_the_camera_onto_the_head_as_the_player_zooms_in() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.js_runtime.op_state().borrow_mut().borrow_mut::<JsRuntimeCameraZoom>().set(0.2);

    controller.hold(&mut world, "ShiftLeft")?;
    controller.release(&mut world, "ShiftLeft")?;
    for (step_index, (desired_zoom, offset_x)) in [(10.0, 1.5), (3.0, 1.5), (1.5, 0.75), (0.0, 0.0)].into_iter().enumerate() {
        controller
            .js_runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<JsRuntimeDesiredCameraZoom>()
            .set(desired_zoom);
        controller.step(&mut world, step_index)?;
        let CameraLimits { offset, .. } = controller.script_manager_rc.borrow().camera_limits();
        assert_eq!(offset, Vec3::new(offset_x, 0.0, 0.0), "at picked zoom {desired_zoom}");
    }
    controller.assert_no_console_errors();
    Ok(())
}

// In shift lock, walking doesn't turn the character, so A and D step sideways.
#[tokio::test]
async fn shift_lock_walks_sideways_facing_the_camera() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    let start = character_physics(&world).location.position;

    controller.look(0.0);
    controller.hold(&mut world, "ShiftLeft")?;
    controller.hold(&mut world, "D")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }

    // with the camera looking along +Z, right is -X
    let end = character_physics(&world);
    let travelled = end.location.position - start;
    assert!(travelled.x < -5.0 && travelled.z.abs() < 0.5, "should walk right: {travelled:?}");
    let yaw = yaw_of(end.location.quaternion);
    assert!(yaw.abs() < 1_f32.to_radians(), "should keep facing +Z: {}°", yaw.to_degrees());
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn a_jump_reaches_the_jump_height_and_lands_once() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    for step_index in 0..10 {
        controller.step(&mut world, step_index)?;
    }
    let start_y = character_physics(&world).location.position.y;

    controller.hold(&mut world, "Space")?;
    let mut peak_y = start_y;
    let mut num_takeoffs = 0;
    let mut was_rising = false;
    for step_index in 10..160 {
        controller.step(&mut world, step_index)?;
        if step_index == 10 {
            controller.release(&mut world, "Space")?;
        }
        let physics_info_3d = character_physics(&world);
        peak_y = peak_y.max(physics_info_3d.location.position.y);
        let is_rising = physics_info_3d.movement.velocity.y > 1.0;
        if is_rising && !was_rising {
            num_takeoffs += 1;
        }
        was_rising = is_rising;
    }

    let end = character_physics(&world);
    println!("start_y={start_y} peak_y={peak_y} end_y={}", end.location.position.y);
    assert!(
        (peak_y - start_y - DEFAULT_JUMP_HEIGHT).abs() < 0.2,
        "should rise about {}: rose {}",
        DEFAULT_JUMP_HEIGHT,
        peak_y - start_y
    );
    assert!(
        (end.location.position.y - start_y).abs() < 0.02,
        "should land where it started: {}",
        end.location.position.y
    );
    assert!(
        end.movement.velocity.y.abs() < 0.1,
        "should be at rest after landing: {:?}",
        end.movement.velocity
    );
    assert_eq!(num_takeoffs, 1, "should jump once, and not again after Space is released");
    controller.assert_no_console_errors();
    Ok(())
}

// Returns how far along +Z the character got, and its height, after walking into a ledge 2 blocks ahead.
async fn walk_into_ledge(ledge_height: f32) -> anyhow::Result<Vec3> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    add_sized_brick(
        &mut world,
        LEDGE_OBJECT_ID,
        Vec3::new(0.0, 1.0 + ledge_height / 2.0, 7.0),
        Vec3::new(10.0, ledge_height, 10.0),
        Quat::IDENTITY,
        false,
    )?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }
    controller.assert_no_console_errors();
    Ok(character_physics(&world).location.position)
}

#[tokio::test]
async fn walking_into_a_low_ledge_steps_up_onto_it() -> anyhow::Result<()> {
    let position = walk_into_ledge(0.08).await?;
    println!("after walking into a low ledge: {position:?}");
    assert!(position.z > 4.0, "should keep walking on top of the ledge: {position:?}");
    assert!((position.y - (STANDING_Y + 0.08)).abs() < 0.02, "should stand on the ledge: {position:?}");

    let position = walk_into_ledge(0.3).await?;
    println!("after walking into a high ledge: {position:?}");
    assert!(
        position.z < 2.5 && position.y < STANDING_Y + 0.2,
        "a ledge taller than the step height should block: {position:?}"
    );
    Ok(())
}

#[tokio::test]
async fn walking_off_a_small_drop_ends_on_the_lower_floor() -> anyhow::Result<()> {
    const DROP: f32 = 0.05;
    let mut world = make_world(Vec3::new(10.0, 2.0, 4.0), Quat::IDENTITY, Vec3::new(0.0, STANDING_Y, 0.0), TALL_CHARACTER_SIZE)?;
    add_sized_brick(
        &mut world,
        LEDGE_OBJECT_ID,
        Vec3::new(0.0, -DROP, 12.0),
        Vec3::new(10.0, 2.0, 20.0),
        Quat::IDENTITY,
        false,
    )?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }

    let end = character_physics(&world);
    println!("end {:?}", end.location.position);
    assert!(end.location.position.z > 4.0, "should have walked past the drop: {:?}", end.location.position);
    assert!(
        (end.location.position.y - (STANDING_Y - DROP)).abs() < 0.01,
        "should stand on the lower floor: {:?}",
        end.location.position
    );
    controller.assert_no_console_errors();
    Ok(())
}

// Stepping up onto a ledge and snapping back down off it are smooth moves, so other screens blend the character over them
// instead of holding it for a tick and then jumping it there.
#[tokio::test]
async fn stepping_up_and_snapping_down_are_not_jumps() -> anyhow::Result<()> {
    const LEDGE_HEIGHT: f32 = 0.08;
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    add_sized_brick(
        &mut world,
        LEDGE_OBJECT_ID,
        Vec3::new(0.0, 1.0 + LEDGE_HEIGHT / 2.0, 5.0),
        Vec3::new(10.0, LEDGE_HEIGHT, 4.0),
        Quat::IDENTITY,
        false,
    )?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    let teleport_count = world.getTeleportCount(CHARACTER_OBJECT_ID);
    let mut highest_y = f32::MIN;
    for step_index in 0..90 {
        controller.step(&mut world, step_index)?;
        highest_y = highest_y.max(character_physics(&world).location.position.y);
        assert_eq!(world.getTeleportCount(CHARACTER_OBJECT_ID), teleport_count, "step {step_index} was a jump");
    }

    let end = character_physics(&world).location.position;
    assert!(end.z > 8.0, "should have walked over the ledge: {end:?}");
    assert!(
        (highest_y - (STANDING_Y + LEDGE_HEIGHT)).abs() < 0.02,
        "should have stood on the ledge: {highest_y}"
    );
    assert!((end.y - STANDING_Y).abs() < 0.01, "should be back on the floor: {end:?}");
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn standing_just_above_the_ground_snaps_down_onto_it() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    for step_index in 0..10 {
        controller.step(&mut world, step_index)?;
    }
    let standing_y = character_physics(&world).location.position.y;

    world.forcePhysicsInfo3D(
        CHARACTER_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(0.0, standing_y + 0.05, 0.0)),
            ..Default::default()
        },
        false,
    );
    // ground checks read the contacts from the last step, so let physics see the gap before the controller does
    world.step_physics(1, DT_SECS, 0.0, None)?;
    assert!(
        character_physics(&world).location.position.y > standing_y + 0.02,
        "should still be off the ground"
    );
    controller.step(&mut world, 10)?;

    let end = character_physics(&world);
    assert!(
        (end.location.position.y - standing_y).abs() < 0.005,
        "should be back on the ground: {:?}",
        end.location.position
    );
    assert!(end.movement.velocity.y.abs() < 1e-3, "should not keep falling: {:?}", end.movement.velocity);
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn standing_still_on_a_spinning_platform_rides_it() -> anyhow::Result<()> {
    let mut world = make_world(Vec3::new(20.0, 2.0, 20.0), Quat::IDENTITY, Vec3::new(6.0, STANDING_Y, 0.0), TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    let spin_radians_per_sec = 60_f32.to_radians();
    let mut max_relative_speed = 0.0_f32;
    for step_index in 0..240 {
        world.setObjectNextPhysicsInfo3D(
            FLOOR_OBJECT_ID,
            PhysicsInfo3D::new(
                Vec3::ZERO,
                Quat::from_rotation_y(spin_radians_per_sec * DT_SECS * (step_index + 1) as f32),
                Vec3::ZERO,
                Vec3::ZERO,
            ),
        )?;
        controller.step(&mut world, step_index)?;
        // the first second is the controller getting up to the platform's speed
        if step_index < 60 {
            continue;
        }
        let relative_velocity = world
            .getVelocityRelativeToGround(CHARACTER_OBJECT_ID, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
            .expect("the character should be on the platform");
        max_relative_speed = max_relative_speed.max(Vec3::new(relative_velocity.x, 0.0, relative_velocity.z).length());
    }

    let end = character_physics(&world);
    println!("max relative speed {max_relative_speed}, spin {:?}", end.movement.angularVelocity);
    assert!(max_relative_speed < 0.5, "should move with the platform, slid at up to {max_relative_speed}");
    assert!(
        (end.movement.angularVelocity.y - spin_radians_per_sec).abs() < 0.1,
        "should turn with the platform: {:?}",
        end.movement.angularVelocity
    );
    controller.assert_no_console_errors();
    Ok(())
}

// A server script changes one player's movement while playing, like a speed of 0 to stun someone.
#[tokio::test]
async fn set_movement_changes_the_walking_speed_while_playing() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.set_movement(&mut world, serde_json::json!({ "speed": 4.0 })).await?;
    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }
    let velocity = character_physics(&world).movement.velocity;
    assert!((velocity.z - 4.0).abs() < 0.3, "should walk at the player's speed: {velocity:?}");

    controller.set_movement(&mut world, serde_json::json!({ "speed": 0.0 })).await?;
    for step_index in 60..90 {
        controller.step(&mut world, step_index)?;
    }
    let stopped_z = character_physics(&world).location.position.z;
    for step_index in 90..120 {
        controller.step(&mut world, step_index)?;
    }
    let moved = character_physics(&world).location.position.z - stopped_z;
    assert!(moved.abs() < 0.01, "a speed of 0 should stop the character, it moved {moved}");
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn set_movement_changes_the_jump_height() -> anyhow::Result<()> {
    const JUMP_HEIGHT: f32 = 1.5;
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.set_movement(&mut world, serde_json::json!({ "jumpHeight": JUMP_HEIGHT })).await?;
    for step_index in 0..10 {
        controller.step(&mut world, step_index)?;
    }
    let start_y = character_physics(&world).location.position.y;

    controller.hold(&mut world, "Space")?;
    let mut peak_y = start_y;
    for step_index in 10..100 {
        controller.step(&mut world, step_index)?;
        if step_index == 10 {
            controller.release(&mut world, "Space")?;
        }
        peak_y = peak_y.max(character_physics(&world).location.position.y);
    }
    assert!(
        (peak_y - start_y - JUMP_HEIGHT).abs() < 0.15,
        "should rise about {JUMP_HEIGHT}: rose {}",
        peak_y - start_y
    );
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn a_step_height_of_zero_turns_stepping_off() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    add_sized_brick(
        &mut world,
        LEDGE_OBJECT_ID,
        Vec3::new(0.0, 1.04, 7.0),
        Vec3::new(10.0, 0.08, 10.0),
        Quat::IDENTITY,
        false,
    )?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.set_movement(&mut world, serde_json::json!({ "stepHeight": 0.0 })).await?;
    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }
    let position = character_physics(&world).location.position;
    assert!(
        position.z < 2.5 && position.y < STANDING_Y + 0.2,
        "should not step onto the ledge: {position:?}"
    );
    controller.assert_no_console_errors();
    Ok(())
}

#[tokio::test]
async fn a_low_ceiling_stops_stepping_up_a_ledge() -> anyhow::Result<()> {
    let mut world = make_flat_world(TALL_CHARACTER_SIZE)?;
    add_sized_brick(
        &mut world,
        LEDGE_OBJECT_ID,
        Vec3::new(0.0, 1.04, 7.0),
        Vec3::new(10.0, 0.08, 10.0),
        Quat::IDENTITY,
        false,
    )?;
    // its underside is 0.05 above the character's head, less than the default step height
    add_sized_brick(
        &mut world,
        CEILING_OBJECT_ID,
        Vec3::new(0.0, 4.05, 0.0),
        Vec3::new(10.0, 2.0, 10.0),
        Quat::IDENTITY,
        false,
    )?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    for step_index in 0..60 {
        controller.step(&mut world, step_index)?;
    }
    let position = character_physics(&world).location.position;
    assert!(
        position.z < 2.5 && position.y < STANDING_Y + 0.05,
        "should not step up under the ceiling: {position:?}"
    );
    controller.assert_no_console_errors();
    Ok(())
}

// Most characters are a RigidGroup, which the ground checks find through its parts.
#[tokio::test]
async fn a_rigid_group_character_walks_forward() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_sized_brick(&mut world, FLOOR_OBJECT_ID, Vec3::ZERO, Vec3::new(400.0, 2.0, 400.0), Quat::IDENTITY, false)?;
    add_rigid_group(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 3.0, 0.0), None)?;
    add_ball(&mut world, 100, CHARACTER_OBJECT_ID, Vec3::new(0.0, 3.5, 0.0), true)?;
    add_ball(&mut world, 101, CHARACTER_OBJECT_ID, Vec3::new(-0.5, 2.5, 0.0), true)?;
    add_ball(&mut world, 102, CHARACTER_OBJECT_ID, Vec3::new(0.5, 2.5, 0.0), true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    settle(&mut world, 60)?;
    let mut controller = start_default_controller(&mut world).await?;
    let start = character_physics(&world).location.position;

    controller.look(0.0);
    controller.hold(&mut world, "W")?;
    let mut grounded_steps = 0;
    for step_index in 0..120 {
        controller.step(&mut world, step_index)?;
        if world
            .getGroundSupport(CHARACTER_OBJECT_ID, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
            .is_some()
        {
            grounded_steps += 1;
        }
    }

    let travelled = character_physics(&world).location.position - start;
    println!("rigid group travelled {travelled:?}, grounded {grounded_steps}/120");
    assert!(travelled.z > 1.8 * DEFAULT_SPEED, "should walk forward: {travelled:?}");
    assert!(grounded_steps > 110, "should stay on the ground, was grounded {grounded_steps}/120 steps");
    controller.assert_no_console_errors();
    Ok(())
}

// The client does not advance animation time by exactly one dt per step: stepClientPhysics walks the
// intermediate steps on tick boundaries and then jumps the LAST step of the burst to the real current
// wall clock. A PropertyAnimator therefore moves the platform by a variable amount on that step, while
// rapier still divides the pose delta by the fixed dt to derive the platform's velocity. This measures
// what that does to the signal the walk animation thresholds against.
#[tokio::test]
async fn a_variable_animation_clock_spikes_the_ground_relative_speed() -> anyhow::Result<()> {
    let mut world = make_world(Vec3::new(20.0, 2.0, 20.0), Quat::IDENTITY, Vec3::new(6.0, STANDING_Y, 0.0), TALL_CHARACTER_SIZE)?;
    settle(&mut world, 30)?;
    let mut controller = start_default_controller(&mut world).await?;
    let spin_radians_per_sec = 60_f32.to_radians();
    let mut animation_secs = 0.0_f32;
    let mut max_relative_speed = 0.0_f32;
    for step_index in 0..240 {
        // every 7th step lands at the end of a burst and carries 1.6 dt of animation time
        animation_secs += if step_index % 7 == 6 { DT_SECS * 1.6 } else { DT_SECS };
        world.setObjectNextPhysicsInfo3D(
            FLOOR_OBJECT_ID,
            PhysicsInfo3D::new(Vec3::ZERO, Quat::from_rotation_y(spin_radians_per_sec * animation_secs), Vec3::ZERO, Vec3::ZERO),
        )?;
        controller.step(&mut world, step_index)?;
        if step_index < 60 {
            continue;
        }
        let relative_velocity = world
            .getVelocityRelativeToGround(CHARACTER_OBJECT_ID, DEFAULT_MAX_DISTANCE_TO_GROUND, DEFAULT_MAX_GROUND_SLOPE_DEGREES)
            .expect("the character should be on the platform");
        max_relative_speed = max_relative_speed.max(Vec3::new(relative_velocity.x, 0.0, relative_velocity.z).length());
    }

    println!("variable clock: max_relative_speed={max_relative_speed:.4} (WALK_START_SPEED is 0.5)");
    // Documents the cause of the "walks for a split second while standing still" flicker: standing still on
    // the same platform with a steady clock stays under 0.5 (see the test above), so this is entirely the clock
    // mismatch, not physics noise. Flip this to `< 0.5` once animation time is advanced in whole physics steps.
    assert!(
        max_relative_speed > 0.5,
        "expected the clock mismatch to still spike the reading, got {max_relative_speed}"
    );
    controller.assert_no_console_errors();
    Ok(())
}
