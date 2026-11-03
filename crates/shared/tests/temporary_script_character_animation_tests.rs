mod helpers;

use game_types::object_types::{ObjectId, ObjectInfo};
use glam::Vec3;
use helpers::world_test_helpers::make_world;
use shared::game::jsRuntime::runTemporaryScript;
use shared::game::physics::SetOrForce;
use shared::game::world::World;

// The bake resolution is arbitrary: the script samples walkPoseAtTime(t) every KEYFRAME_DT_SECS.
// Changing it changes how many keyframes get baked; the assertions below adapt because the
// interpolation tolerances are computed from the same constant.
const KEYFRAME_DT_SECS: f32 = 0.1;
const WALK_CYCLE_DURATION_SECS: f32 = 1.0;
const LEG_SWING_AMPLITUDE: f32 = 0.5;
const ARM_SWING_AMPLITUDE: f32 = 0.35;
const TORSO_BOB_AMPLITUDE: f32 = 0.08;

const ORB_BOB_AMPLITUDE: f32 = 0.6;
const ORB_BASE_POSITION: Vec3 = Vec3::new(2.5, 3.0, 0.0);

// Where the walker's body (rigid group) starts, and how far the test carries it mid-animation.
const BODY_START_POSITION: Vec3 = Vec3::new(0.0, 1.5, 0.0);
const TRAVEL_OFFSET: Vec3 = Vec3::new(3.0, 0.0, 5.0);

// Where each joint sits relative to the body. These must stay in sync with the localBindOf
// definitions inside WALKER_TEMP_SCRIPT_BODY.
const TORSO_LOCAL_BIND: Vec3 = Vec3::new(0.0, 0.3, 0.0);
const HEAD_LOCAL_BIND: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const LEFT_LEG_LOCAL_BIND: Vec3 = Vec3::new(-0.3, -0.9, 0.0);
const RIGHT_LEG_LOCAL_BIND: Vec3 = Vec3::new(0.3, -0.9, 0.0);
const LEFT_ARM_LOCAL_BIND: Vec3 = Vec3::new(-0.65, 0.3, 0.0);
const RIGHT_ARM_LOCAL_BIND: Vec3 = Vec3::new(0.65, 0.3, 0.0);

// Shared helpers for both temporary scripts below.
const TEMP_SCRIPT_HELPERS: &str = r#"
const v = (x, y, z) => ({ x, y, z });
const loc = (position) => ({ position, rotation: v(0, 0, 0) });
const phys = (position) => ({
    location: loc(position),
    movement: { velocity: v(0, 0, 0), angularVelocity: v(0, 0, 0) },
});
const brickInfo = (position, size) => ({
    type: "brick", material: null, opacity: 1, color: { r: 0.6, g: 0.6, b: 0.9 },
    isAlwaysOnTop: false, size, isWalkthrough: false, isPushable: false, density: 1,
    ...phys(position),
});
const ballInfo = (position, radius) => ({
    type: "ball", material: null, opacity: 1, color: { r: 0.9, g: 0.8, b: 0.6 },
    isAlwaysOnTop: false, radius, isWalkthrough: false, isPushable: false, density: 1,
    ...phys(position),
});
const mustCreate = (name, info, parentId) => {
    const created = game.objectService.createObject(name, info, parentId);
    if (created === null) throw new Error("Failed to create " + name);
    return created;
};
const add = (a, b) => v(a.x + b.x, a.y + b.y, a.z + b.z);
"#;

// This is the reference recipe for an AI-authored animated character, submitted through
// run_temporary_script:
//   (a) build the character: a self-contained kit under one root group — a Skeleton whose joint
//       binds say where each joint sits relative to the body, a following rigid group with
//       same-named parts, and auto-created attachments;
//   (b) define fn(t) (here a walk cycle: legs and arms counter-swing, torso and head bob at twice
//       the stride frequency) and bake it into SkKeyframes every dt seconds;
//   (c) play it indefinitely with a looping Server-time clock driving a SkAnimator.
// Because poses are relative to the body, the character keeps walking wherever the body goes.
const WALKER_TEMP_SCRIPT_BODY: &str = r#"
// ---- (a) create the character: one self-contained kit under a root group ----
const localBindOf = {
    "Torso": v(0, 0.3, 0),
    "Head": v(0, 1.0, 0),
    "LeftLeg": v(-0.3, -0.9, 0),
    "RightLeg": v(0.3, -0.9, 0),
    "LeftArm": v(-0.65, 0.3, 0),
    "RightArm": v(0.65, 0.3, 0),
};
const partShapeOf = {
    "Torso": { size: v(0.8, 1.2, 0.5) },
    "Head": { radius: 0.35 },
    "LeftLeg": { size: v(0.3, 1.0, 0.3) },
    "RightLeg": { size: v(0.3, 1.0, 0.3) },
    "LeftArm": { size: v(0.25, 0.9, 0.25) },
    "RightArm": { size: v(0.25, 0.9, 0.25) },
};

const characterRoot = mustCreate("WalkerCharacter", { type: "group" }, game.world.id);
const skeleton = mustCreate("Joints", { type: "skeleton" }, characterRoot.id);
for (const jointName of Object.keys(localBindOf)) {
    mustCreate(jointName, { type: "skJoint", restPose: loc(localBindOf[jointName]) }, skeleton.id);
}

const body = mustCreate(
    "WalkerBody",
    { type: "rigidGroup", followSkeletonId: skeleton.id, ...phys(bodyStart) },
    characterRoot.id,
);
// Parts sit at bodyStart + localBind, exactly on their joints, so the attachments get zero offsets.
for (const partName of Object.keys(localBindOf)) {
    const partPosition = add(bodyStart, localBindOf[partName]);
    const shape = partShapeOf[partName];
    const info = shape.radius !== undefined ? ballInfo(partPosition, shape.radius) : brickInfo(partPosition, shape.size);
    mustCreate(partName, info, body.id);
}
game.objectService.createSkAttachmentsInRigidGroup(skeleton.id, body.id);

// ---- (b) fn(t): the walk cycle as offsets from each joint's bind (SkKeyframePoses are applied on top of it) ----
const walkPoseAtTime = (t) => {
    const phase = 2 * Math.PI * t / duration;
    const legSwing = legSwingAmplitude * Math.sin(phase);
    const armSwing = armSwingAmplitude * Math.sin(phase);
    const bob = torsoBobAmplitude * Math.sin(2 * phase);
    return {
        "Torso": v(0, bob, 0),
        "Head": v(0, bob, 0),
        "LeftLeg": v(0, 0, legSwing),
        "RightLeg": v(0, 0, -legSwing),
        "LeftArm": v(0, 0, -armSwing),
        "RightArm": v(0, 0, armSwing),
    };
};

const animation = mustCreate(
    "WalkCycleAnimation",
    { type: "skAnimation", duration, interpolationStyle: "Linear" },
    characterRoot.id,
);
const numSteps = Math.ceil(duration / dt - 1e-4);
for (let frameIndex = 0; frameIndex <= numSteps; frameIndex++) {
    const t = frameIndex < numSteps ? frameIndex * dt : duration;
    const keyframe = mustCreate("Keyframe" + frameIndex, { type: "skKeyframe", time: t }, animation.id);
    const poseOfJointName = walkPoseAtTime(t);
    for (const jointName of Object.keys(poseOfJointName)) {
        mustCreate(
            jointName + "Pose" + frameIndex,
            { type: "skKeyframePose", jointName, localPose: loc(poseOfJointName[jointName]) },
            keyframe.id,
        );
    }
}

// ---- (c) play it indefinitely: a looping Server-time clock drives the animator ----
// Server time keeps every client in sync and stays valid on the dedicated server (a Client clock
// would be NaN there).
const clock = mustCreate("WalkClock", {
    type: "clock",
    clockState: { Playing: { localStartMs: 0, timeLocation: "Server" } },
    shouldLoop: true,
    speed: 1,
}, characterRoot.id);
mustCreate("WalkAnimator", {
    type: "skAnimator",
    skeletonId: skeleton.id,
    skAnimationId: animation.id,
    clockId: clock.id,
    priority: 1,
    weight: 100,
    weightTransition: null,
}, characterRoot.id);
console.log("Baked " + (numSteps + 1) + " walk keyframes at dt=" + dt);
"#;

// The PropertyAnimation flavor of the same idea: bake fn(t) -> a single scalar (the orb's world y)
// into the animator's inline keyframes array. Unlike SkAnimation there are no child keyframe
// objects; the samples live on the PropertyAnimator itself.
const ORB_TEMP_SCRIPT_BODY: &str = r#"
const orb = mustCreate("BobbingOrb", ballInfo(v(orbBaseX, orbBaseY, orbBaseZ), 0.5), game.world.id);

// fn(t): the orb's absolute world y at animation time t. Property overrides replace the value
// outright, so the base height is baked into every sample.
const orbHeightAtTime = (t) => orbBaseY + orbBobAmplitude * Math.sin(2 * Math.PI * t / duration);

const animation = mustCreate("OrbBobAnimation", {
    type: "propertyAnimation",
    duration,
    interpolationStyle: "Linear",
}, game.world.id);

const numSteps = Math.ceil(duration / dt - 1e-4);
let numKeyframes = 0;
for (let frameIndex = 0; frameIndex <= numSteps; frameIndex++) {
    const t = frameIndex < numSteps ? frameIndex * dt : duration;
    mustCreate("OrbBobKeyframe" + frameIndex, {
        type: "propertyKeyframe",
        time: t,
        value: orbHeightAtTime(t),
    }, animation.id);
    numKeyframes++;
}

const clock = mustCreate("OrbBobClock", {
    type: "clock",
    clockState: { Playing: { localStartMs: 0, timeLocation: "Server" } },
    shouldLoop: true,
    speed: 1,
}, game.world.id);
mustCreate("OrbBobAnimator", {
    type: "propertyAnimator",
    targetObjectId: orb.id,
    propertyPath: ["location", "position", "y"],
    propertyAnimationId: animation.id,
    clockId: clock.id,
    priority: 1,
    weight: 100,
    weightTransition: null,
}, game.world.id);
console.log("Baked " + numKeyframes + " property keyframes at dt=" + dt);
"#;

fn walker_temp_script_source() -> String {
    format!(
        "const dt = {dt:?};\nconst duration = {duration:?};\nconst legSwingAmplitude = {leg:?};\nconst armSwingAmplitude = {arm:?};\nconst torsoBobAmplitude = {bob:?};\nconst bodyStart = {{ x: {x:?}, y: {y:?}, z: {z:?} }};\n{helpers}{body}",
        dt = KEYFRAME_DT_SECS,
        duration = WALK_CYCLE_DURATION_SECS,
        leg = LEG_SWING_AMPLITUDE,
        arm = ARM_SWING_AMPLITUDE,
        bob = TORSO_BOB_AMPLITUDE,
        x = BODY_START_POSITION.x,
        y = BODY_START_POSITION.y,
        z = BODY_START_POSITION.z,
        helpers = TEMP_SCRIPT_HELPERS,
        body = WALKER_TEMP_SCRIPT_BODY,
    )
}

fn orb_temp_script_source() -> String {
    format!(
        "const dt = {dt:?};\nconst duration = {duration:?};\nconst orbBobAmplitude = {orb_bob:?};\nconst orbBaseX = {x:?};\nconst orbBaseY = {y:?};\nconst orbBaseZ = {z:?};\n{helpers}{body}",
        dt = KEYFRAME_DT_SECS,
        duration = WALK_CYCLE_DURATION_SECS,
        orb_bob = ORB_BOB_AMPLITUDE,
        x = ORB_BASE_POSITION.x,
        y = ORB_BASE_POSITION.y,
        z = ORB_BASE_POSITION.z,
        helpers = TEMP_SCRIPT_HELPERS,
        body = ORB_TEMP_SCRIPT_BODY,
    )
}

fn expected_keyframe_count() -> usize {
    ((WALK_CYCLE_DURATION_SECS / KEYFRAME_DT_SECS) as f64 - 1e-4).ceil() as usize + 1
}

fn find_object_id(world: &World, name: &str, matches: impl Fn(&ObjectInfo) -> bool) -> anyhow::Result<ObjectId> {
    world
        .getObjectIdsByName(name)
        .iter()
        .copied()
        .find(|object_id| world.getObject(*object_id).map(|object| matches(&object.info)).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("Expected object named {name}"))
}

fn child_object_ids_of(world: &World, parent_object_id: ObjectId) -> Vec<ObjectId> {
    let mut child_object_ids: Vec<ObjectId> = world
        .parentOfId
        .iter()
        .filter_map(|(object_id, parent_id)| (*parent_id == parent_object_id).then_some(*object_id))
        .collect();
    child_object_ids.sort_unstable();
    child_object_ids
}

fn assert_vec3_near(actual: Vec3, expected: Vec3, tolerance: f32, label: &str) {
    let delta = actual - expected;
    assert!(
        delta.length() < tolerance,
        "{label}: expected {expected:?}, got {actual:?}, delta {delta:?}, tolerance {tolerance}"
    );
}

fn part_position(world: &World, object_id: ObjectId) -> Vec3 {
    world
        .getObjectPhysicsInfo3D(object_id)
        .unwrap_or_else(|| panic!("Expected physics info for a character part"))
        .location
        .position
}

// The baked keyframes linearly interpolate the sampled sine, so the worst-case deviation from the
// exact fn(t) is bounded by the chord error: amplitude * (omega * dt)^2 / 8. Deriving tolerances
// from KEYFRAME_DT_SECS keeps the test valid for any bake resolution.
fn chord_tolerance(amplitude: f32, omega: f32) -> f32 {
    amplitude * (omega * KEYFRAME_DT_SECS).powi(2) / 8.0 + 2e-3
}

// Asserts every walker part matches fn(t) at the given wall-clock time, with the body at
// body_position. Stepping passes no client wall clock, exactly like the dedicated server.
fn assert_walker_matches_fn_of_t(world: &mut World, live_server_sim_secs: f32, body_position: Vec3, label: &str) -> anyhow::Result<()> {
    world.step_physics(0, 0.0, live_server_sim_secs as f64 * 1000.0, None)?;

    let omega = std::f32::consts::TAU / WALK_CYCLE_DURATION_SECS;
    let animation_time = live_server_sim_secs % WALK_CYCLE_DURATION_SECS;
    let leg_swing = LEG_SWING_AMPLITUDE * (omega * animation_time).sin();
    let arm_swing = ARM_SWING_AMPLITUDE * (omega * animation_time).sin();
    let bob = TORSO_BOB_AMPLITUDE * (2.0 * omega * animation_time).sin();

    let swing_tolerance = chord_tolerance(LEG_SWING_AMPLITUDE, omega);
    let bob_tolerance = chord_tolerance(TORSO_BOB_AMPLITUDE, 2.0 * omega);

    let expectations = [
        ("Torso", TORSO_LOCAL_BIND + Vec3::new(0.0, bob, 0.0), bob_tolerance),
        ("Head", HEAD_LOCAL_BIND + Vec3::new(0.0, bob, 0.0), bob_tolerance),
        ("LeftLeg", LEFT_LEG_LOCAL_BIND + Vec3::new(0.0, 0.0, leg_swing), swing_tolerance),
        ("RightLeg", RIGHT_LEG_LOCAL_BIND + Vec3::new(0.0, 0.0, -leg_swing), swing_tolerance),
        ("LeftArm", LEFT_ARM_LOCAL_BIND + Vec3::new(0.0, 0.0, -arm_swing), swing_tolerance),
        ("RightArm", RIGHT_ARM_LOCAL_BIND + Vec3::new(0.0, 0.0, arm_swing), swing_tolerance),
    ];
    for (part_name, local_pose, tolerance) in expectations {
        let part_object_id = find_object_id(world, part_name, |info| matches!(info, ObjectInfo::Brick(_) | ObjectInfo::Ball(_)))?;
        assert_vec3_near(
            part_position(world, part_object_id),
            body_position + local_pose,
            tolerance,
            &format!("{label} {part_name}"),
        );
    }
    Ok(())
}

#[test]
fn temporary_script_bakes_fn_of_t_walk_cycle_that_travels_with_the_character() -> anyhow::Result<()> {
    let mut world = make_world()?;

    let console_logs = runTemporaryScript(&mut world, walker_temp_script_source())?;
    assert!(!console_logs.is_empty(), "Expected the temporary script to log its keyframe summary");

    // This checks the baked animation structure: one keyframe per dt step plus the final frame at
    // t=duration, each carrying one pose per animated joint.
    let animation_object_id = find_object_id(&world, "WalkCycleAnimation", |info| matches!(info, ObjectInfo::SkAnimation(_)))?;
    let mut keyframe_times = Vec::new();
    for keyframe_object_id in child_object_ids_of(&world, animation_object_id) {
        let ObjectInfo::SkKeyframe(keyframe_info) = &world.getObject(keyframe_object_id)?.info else {
            return Err(anyhow::anyhow!("Expected only SkKeyframe children under WalkCycleAnimation"));
        };
        keyframe_times.push(keyframe_info.time);

        let pose_object_ids = child_object_ids_of(&world, keyframe_object_id);
        assert_eq!(pose_object_ids.len(), 6, "Expected one SkKeyframePose per animated joint on every keyframe");
    }
    keyframe_times.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(keyframe_times.len(), expected_keyframe_count());
    assert_eq!(keyframe_times[0], 0.0);
    assert!((keyframe_times[keyframe_times.len() - 1] - WALK_CYCLE_DURATION_SECS).abs() < 1e-4);

    // The parts sit exactly on their joints, so every auto-created attachment must have a zero
    // offset.
    for part_name in ["Torso", "Head", "LeftLeg", "RightLeg", "LeftArm", "RightArm"] {
        let attachment_object_id = find_object_id(&world, &format!("{part_name}SkAttachment"), |info| matches!(info, ObjectInfo::SkAttachment(_)))?;
        let ObjectInfo::SkAttachment(attachment_info) = &world.getObject(attachment_object_id)?.info else {
            unreachable!();
        };
        assert_vec3_near(attachment_info.offsetFromJoint.position, Vec3::ZERO, 1e-4, &format!("{part_name} attachment"));
    }

    // The walk cycle plays indefinitely: off-keyframe sample times, including ones past several
    // loop wraps, must all match fn(t mod duration) around the standing body.
    for live_server_sim_secs in [0.13_f32, 0.58, 2.33] {
        assert_walker_matches_fn_of_t(
            &mut world,
            live_server_sim_secs,
            BODY_START_POSITION,
            &format!("standing t={live_server_sim_secs}s"),
        )?;
    }

    // Carry the body somewhere else mid-animation (as the player controller or a script would):
    // the walk cycle must continue around the moved body.
    let body_object_id = find_object_id(&world, "WalkerBody", |info| matches!(info, ObjectInfo::RigidGroup(_)))?;
    world.setObjectPositionOffsetRecursive(body_object_id, TRAVEL_OFFSET, SetOrForce::Force)?;
    for live_server_sim_secs in [3.02_f32, 3.71] {
        assert_walker_matches_fn_of_t(
            &mut world,
            live_server_sim_secs,
            BODY_START_POSITION + TRAVEL_OFFSET,
            &format!("traveled t={live_server_sim_secs}s"),
        )?;
    }

    Ok(())
}

#[test]
fn temporary_script_bakes_fn_of_t_into_a_property_animation_bob() -> anyhow::Result<()> {
    let mut world = make_world()?;

    let console_logs = runTemporaryScript(&mut world, orb_temp_script_source())?;
    assert!(!console_logs.is_empty(), "Expected the temporary script to log its keyframe summary");

    // This checks the baked animator structure: the fn(t) samples live as PropertyKeyframe children
    // of the animator's PropertyAnimation, one per dt step plus the final frame.
    let orb_object_id = find_object_id(&world, "BobbingOrb", |info| matches!(info, ObjectInfo::Ball(_)))?;
    let animator_object_id = find_object_id(&world, "OrbBobAnimator", |info| matches!(info, ObjectInfo::PropertyAnimator(_)))?;
    let ObjectInfo::PropertyAnimator(animator_info) = &world.getObject(animator_object_id)?.info else {
        unreachable!();
    };
    assert_eq!(animator_info.targetObjectId.as_ref().map(|pointer| pointer.0), Some(orb_object_id));
    assert_eq!(animator_info.propertyPath, vec!["location", "position", "y"]);
    let property_animation_id = animator_info
        .propertyAnimationId
        .as_ref()
        .map(|pointer| pointer.0)
        .ok_or_else(|| anyhow::anyhow!("Expected the animator to point at a PropertyAnimation"))?;
    let ObjectInfo::PropertyAnimation(property_animation_info) = &world.getObject(property_animation_id)?.info else {
        return Err(anyhow::anyhow!("Expected object {property_animation_id} to be a PropertyAnimation"));
    };
    assert!((property_animation_info.duration - WALK_CYCLE_DURATION_SECS).abs() < 1e-4);

    // keyframe objects live in a HashSet, so sort by time rather than assuming creation order survived.
    let mut keyframes = world
        .propertyKeyframesArcOfPropertyAnimationId
        .get(&property_animation_id)
        .ok_or_else(|| anyhow::anyhow!("Expected cached keyframes for PropertyAnimation {property_animation_id}"))?
        .to_vec();
    keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
    assert_eq!(keyframes.len(), expected_keyframe_count());
    assert_eq!(keyframes[0].time, 0.0);
    assert!((keyframes[keyframes.len() - 1].time - WALK_CYCLE_DURATION_SECS).abs() < 1e-4);
    assert!((keyframes[0].value - ORB_BASE_POSITION.y).abs() < 1e-4, "fn(0) should be the base height");

    let omega = std::f32::consts::TAU / WALK_CYCLE_DURATION_SECS;
    let tolerance = chord_tolerance(ORB_BOB_AMPLITUDE, omega);

    // Off-keyframe sample times plus loop wraps, stepped with no client wall clock like the
    // dedicated server.
    for live_server_sim_secs in [0.13_f32, 0.58, 2.33] {
        world.step_physics(0, 0.0, live_server_sim_secs as f64 * 1000.0, None)?;

        let animation_time = live_server_sim_secs % WALK_CYCLE_DURATION_SECS;
        let expected_y = ORB_BASE_POSITION.y + ORB_BOB_AMPLITUDE * (omega * animation_time).sin();
        let orb_position = world
            .getObjectPhysicsInfo3D(orb_object_id)
            .ok_or_else(|| anyhow::anyhow!("Expected physics info for the orb"))?
            .location
            .position;

        assert_vec3_near(
            orb_position,
            Vec3::new(ORB_BASE_POSITION.x, expected_y, ORB_BASE_POSITION.z),
            tolerance,
            &format!("t={live_server_sim_secs}s orb"),
        );
    }

    Ok(())
}

// The point of making keyframes real objects: a script can edit one and the curve changes on the spot.
// This drives the whole path - script write -> rustUpdateObject -> afterObjectInfoChange -> keyframe cache
// rebuild -> next sample - rather than trusting that the cache invalidation is wired up.
#[test]
fn editing_one_property_keyframe_from_a_script_changes_the_sampled_curve() -> anyhow::Result<()> {
    let mut world = make_world()?;

    runTemporaryScript(&mut world, orb_temp_script_source())?;

    let orb_object_id = find_object_id(&world, "BobbingOrb", |info| matches!(info, ObjectInfo::Ball(_)))?;
    let animator_object_id = find_object_id(&world, "OrbBobAnimator", |info| matches!(info, ObjectInfo::PropertyAnimator(_)))?;
    let ObjectInfo::PropertyAnimator(animator_info) = &world.getObject(animator_object_id)?.info else {
        unreachable!();
    };
    let property_animation_id = animator_info
        .propertyAnimationId
        .as_ref()
        .map(|pointer| pointer.0)
        .ok_or_else(|| anyhow::anyhow!("Expected the animator to point at a PropertyAnimation"))?;

    // the keyframe sitting exactly at t=0, found by time because children come back in arbitrary order
    let keyframe_at_zero_id = world
        ._childrenOfId
        .get(&property_animation_id)
        .ok_or_else(|| anyhow::anyhow!("Expected keyframe children"))?
        .iter()
        .copied()
        .find(|keyframe_id| match world.getObject(*keyframe_id).map(|object| &object.info) {
            Ok(ObjectInfo::PropertyKeyframe(keyframe)) => keyframe.time.abs() < 1e-6,
            _ => false,
        })
        .ok_or_else(|| anyhow::anyhow!("Expected a keyframe at t=0"))?;

    // sample at t=0 before the edit
    world.step_physics(0, 0.0, 0.0, None)?;
    let position_before = world
        .getObjectPhysicsInfo3D(orb_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected orb physics"))?
        .location
        .position;
    assert!((position_before.y - ORB_BASE_POSITION.y).abs() < 1e-4);

    // now have a script rewrite that one keyframe's value, exactly as game code would
    let edit_source = format!(
        "const keyframe = game.objectService.getObjectById({keyframe_at_zero_id});\nif (keyframe === null) throw new Error(\"missing keyframe\");\nkeyframe.value = {new_value:?};\n",
        keyframe_at_zero_id = keyframe_at_zero_id,
        new_value = ORB_BASE_POSITION.y + 12.0,
    );
    runTemporaryScript(&mut world, edit_source)?;

    // the cache must have rebuilt, so the very next sample reflects the new value
    let keyframes = world
        .propertyKeyframesArcOfPropertyAnimationId
        .get(&property_animation_id)
        .ok_or_else(|| anyhow::anyhow!("Expected cached keyframes"))?;
    let cached_value_at_zero = keyframes
        .iter()
        .find(|keyframe| keyframe.time.abs() < 1e-6)
        .ok_or_else(|| anyhow::anyhow!("Expected the t=0 keyframe in the cache"))?
        .value;
    assert!(
        (cached_value_at_zero - (ORB_BASE_POSITION.y + 12.0)).abs() < 1e-4,
        "keyframe cache did not pick up the script's edit: {cached_value_at_zero}"
    );

    world.step_physics(1, 0.0, 0.0, None)?;
    let position_after = world
        .getObjectPhysicsInfo3D(orb_object_id)
        .ok_or_else(|| anyhow::anyhow!("Expected orb physics"))?
        .location
        .position;
    assert!(
        (position_after.y - (ORB_BASE_POSITION.y + 12.0)).abs() < 1e-3,
        "expected the orb to jump to the edited keyframe value, got {}",
        position_after.y
    );

    Ok(())
}
