mod helpers;

use std::collections::HashMap;

use game_types::{
    consts::{SERVER_SIM_AND_SEND_SECS, STORED_WORLD_CURRENT_VERSION},
    object_types::{
        BrickInfo, ClockInfo, ClockState, InterpolationStyle, LiveObject, LocalGroupInfo, Location3D, ObjectBeingAdded, ObjectId, ObjectInfo, ObjectLocation,
        PhysicsInfo3D, PhysicsToggles3D, PlayerInfo, RigidGroupInfo, ScriptInfo, SkAnimationInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo,
        SkeletonInfo, VisualSettings, sk_animator::SkAnimatorInfo,
    },
    physics_primitive_types::BrickPrimitiveSettings,
    stored_world_type::StoredWorld,
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::fallback_camera_location_for_tests;
use shared::game::{
    jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS},
    physics::SetOrForce,
    world::{World, WorldControlMode},
};
use tokio::sync::mpsc;

// This is the reference player-spawn script. Instead of cloning a live character and leaving
// the source visible in the world, it cuts the whole character kit into a template at server
// start — the source disappears — and pastes a fresh, fully-rewired copy per joining player.
//
// The pasted avatar keeps following its Skeleton, and because SkKeyframePoses are relative to the
// body, it stays animated while the player moves around. The attachments are created while the
// source is still live (the offsets come out right even though the template sits parked away
// from the play area) and are captured inside the template as children of the parts.
const SPAWN_SCRIPT: &str = r#"
const characterRoot = game.objectService.getObjectByName("SpawnCharacter");
const templateBody = characterRoot?.getChild("Body");
const templateSkeleton = characterRoot?.getChild("Joints");
if (!characterRoot || !templateBody || templateBody.type !== "rigidGroup" || !templateSkeleton || templateSkeleton.type !== "skeleton") {
  console.error("SpawnCharacter template is missing Body or Joints.");
  await forever();
}

// Glue parts to joints while the source is still live: the attachments become children of the
// attached parts, so they are captured inside the template below.
game.objectService.createSkAttachmentsInRigidGroup(templateSkeleton.id, templateBody.id);

// Cutting removes the source character from the world and stores it as a template. Every paste
// creates fresh objects with all internal pointers (followSkeletonId, attachment targets,
// animator wiring) remapped to the new copy.
game.templateService.cutObjectToTemplate(characterRoot.id, "spawn-character");

const spawnPoints = [
  { x: 10, y: 0, z: 5 },
  { x: -6, y: 0, z: 3 },
];
let nextSpawnIndex = 0;

// How fast the player must move before the walk animation plays, the movement speed at which the
// walk cycle plays at its authored pace, and how long the idle<->walk crossfade takes.
const WALK_START_SPEED = 0.5;
const WALK_FULL_SPEED = 9;
const WALK_FADE_MS = 150;

// playerObjectId -> { walkClockId, walkAnimatorId, isWalking, appliedSpeedMultiplier }
const walkStateOfPlayerId = {};

game.objectService.addObjectCreatedListener((playerObject) => {
  if (playerObject.type !== "player") return;
  if (playerObject.characterObjectId !== null) return;

  const spawn = spawnPoints[nextSpawnIndex % spawnPoints.length];
  nextSpawnIndex += 1;

  const characterCopy = game.templateService.pasteTemplate("spawn-character", game.world.id);
  characterCopy.name = `${playerObject.username}-Character`;
  const body = characterCopy.getChild("Body");
  const head = body?.getChild("Head");
  if (!body || !head) {
    console.error("Pasted character is missing Body or Head.");
    return;
  }

  game.objectService.teleportObjectTo(body.id, spawn);
  game.objectService.setObjectPushable(body.id, true);

  playerObject.headObjectId = head.id;
  playerObject.characterObjectId = body.id;

  const walkClock = characterCopy.getChild("Walk-Clock");
  const walkAnimator = characterCopy.getChild("Walk-Animator");
  if (walkClock && walkAnimator) {
    walkStateOfPlayerId[playerObject.id] = {
      walkClockId: walkClock.id,
      walkAnimatorId: walkAnimator.id,
      isWalking: false,
      appliedSpeedMultiplier: 1,
    };
  }
}, "player");

game.objectService.addObjectDeletedListener((playerObjectId) => {
  delete walkStateOfPlayerId[playerObjectId];
}, "player");

// ---- fade the walk animation in while the player moves, scaled to how fast they move ----
while (true) {
  for (const playerObjectIdString of Object.keys(walkStateOfPlayerId)) {
    const playerObjectId = Number(playerObjectIdString);
    const walkState = walkStateOfPlayerId[playerObjectId];
    const player = game.objectService.getObjectById(playerObjectId);
    if (!player || player.type !== "player" || player.characterObjectId === null) continue;
    const body = game.objectService.getObjectById(player.characterObjectId);
    if (!body || !("movement" in body)) continue;

    const velocity = body.movement.velocity;
    const horizontalSpeed = Math.sqrt(velocity.x * velocity.x + velocity.z * velocity.z);

    const isWalking = horizontalSpeed > WALK_START_SPEED;
    if (isWalking !== walkState.isWalking) {
      walkState.isWalking = isWalking;
      game.animationService.smoothlyChangeWeight(walkState.walkAnimatorId, isWalking ? 100 : 0, WALK_FADE_MS);
    }
    if (isWalking) {
      // Only touch the clock when the speed changed noticeably, instead of rewriting it every tick.
      const speedMultiplier = horizontalSpeed / WALK_FULL_SPEED;
      if (Math.abs(speedMultiplier - walkState.appliedSpeedMultiplier) > 0.05) {
        walkState.appliedSpeedMultiplier = speedMultiplier;
        game.clockService.setSpeed(walkState.walkClockId, speedMultiplier);
      }
    }
  }
  await tick();
}
"#;

const TORSO_JOINT_BIND: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const HEAD_JOINT_BIND: Vec3 = Vec3::new(0.0, 2.0, 0.0);
const SPAWN_POINT_A: Vec3 = Vec3::new(10.0, 0.0, 5.0);
const SPAWN_POINT_B: Vec3 = Vec3::new(-6.0, 0.0, 3.0);
// The head brick sits this far above the body/torso; the assembly must keep it while moving.
const HEAD_OFFSET_FROM_BODY: Vec3 = Vec3::new(0.0, 1.0, 0.0);
// The fixture's walk animation holds the head this far behind its bind, so the tests can tell
// exactly whether the idle pose or the walk pose is showing.
const WALK_POSE_HEAD_OFFSET: Vec3 = Vec3::new(0.0, 0.0, 1.0);
// The template character sits parked away from the play area, like a real world's character that
// an artist moved aside. Pasted copies must spawn at their spawn points, not at the parking spot.
const TEMPLATE_PARK_OFFSET: Vec3 = Vec3::new(-30.0, 0.0, -30.0);

fn identity_location(position: Vec3) -> Location3D {
    Location3D {
        position,
        quaternion: Quat::IDENTITY,
    }
}

fn add_character_brick(world: &mut World, object_id: ObjectId, parent_id: ObjectId, name: &str, position: Vec3) {
    world.addObjectWithId(
        object_id,
        name.to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings {
                size: Vec3::new(0.8, 0.8, 0.8),
            },
            PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        parent_id,
    );
}

// This builds a miniature of the island-adventurer character kit: one root group holding the
// Skeleton, the following rigid group with matching part names, and a wired clock + joint
// animation (posing only the Head joint) + animator. The spawn script must detach the body from
// this kit so the avatar can actually move.
fn build_spawn_template_world(js_callback_inputs_tx: mpsc::UnboundedSender<shared::game::world::JsCallbackInputs>) -> anyhow::Result<World> {
    let root_object_id = 1;
    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: fallback_camera_location_for_tests(),
        objectOfId: HashMap::from([(
            root_object_id,
            LiveObject {
                id: root_object_id,
                name: "root".to_string(),
                info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
            },
        )]),
        rootObjectId: root_object_id,
        childrenOfId: HashMap::from([(root_object_id, Vec::new())]),
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        scriptRawCodeOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };
    let stored_world_string = serde_json::to_string(&stored_world)?;
    let mut world = World::fromStoredWorldString(stored_world_string.as_str(), WorldControlMode::Server, Some(js_callback_inputs_tx))?;

    let character_root_object_id = 2;
    let skeleton_object_id = 3;
    let torso_joint_object_id = 4;
    let head_joint_object_id = 5;
    let body_object_id = 6;
    let torso_brick_object_id = 7;
    let head_brick_object_id = 8;
    let clock_object_id = 9;
    let animation_object_id = 10;
    let keyframe_start_object_id = 11;
    let keyframe_start_pose_object_id = 12;
    let keyframe_end_object_id = 13;
    let keyframe_end_pose_object_id = 14;
    let animator_object_id = 15;
    let script_object_id = 16;

    world.addObjectWithId(
        character_root_object_id,
        "SpawnCharacter".to_string(),
        ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        skeleton_object_id,
        "Joints".to_string(),
        ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        character_root_object_id,
    );
    world.addObjectWithId(
        torso_joint_object_id,
        "Torso".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: identity_location(TORSO_JOINT_BIND),
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        head_joint_object_id,
        "Head".to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: identity_location(HEAD_JOINT_BIND),
        }),
        skeleton_object_id,
    );
    world.addObjectWithId(
        body_object_id,
        "Body".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: Some(game_types::object_types::ObjectIdPointer(skeleton_object_id)),
            },
            PhysicsInfo3D::new(TORSO_JOINT_BIND + TEMPLATE_PARK_OFFSET, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        ),
        character_root_object_id,
    );
    add_character_brick(
        &mut world,
        torso_brick_object_id,
        body_object_id,
        "Torso",
        TORSO_JOINT_BIND + TEMPLATE_PARK_OFFSET,
    );
    add_character_brick(&mut world, head_brick_object_id, body_object_id, "Head", HEAD_JOINT_BIND + TEMPLATE_PARK_OFFSET);

    // Server time location: a Client clock is invalid on the dedicated server (no client wall
    // clock exists there) and would poison animation sampling with NaN.
    world.addObjectWithId(
        clock_object_id,
        "Idle-Clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 1.0,
        }),
        character_root_object_id,
    );
    world.addObjectWithId(
        animation_object_id,
        "Idle-Animation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        character_root_object_id,
    );
    for (keyframe_object_id, pose_object_id, time) in [
        (keyframe_start_object_id, keyframe_start_pose_object_id, 0.0_f32),
        (keyframe_end_object_id, keyframe_end_pose_object_id, 1.0),
    ] {
        world.addObjectWithId(
            keyframe_object_id,
            format!("keyframe-{keyframe_object_id}"),
            ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time }),
            animation_object_id,
        );
        world.addObjectWithId(
            pose_object_id,
            format!("pose-{pose_object_id}"),
            ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
                jointName: "Head".to_string(),
                localPose: identity_location(Vec3::ZERO),
            }),
            keyframe_object_id,
        );
    }
    world.addObjectWithId(
        animator_object_id,
        "Idle-Animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(game_types::object_types::ObjectIdPointer(skeleton_object_id)),
            skAnimationId: Some(game_types::object_types::ObjectIdPointer(animation_object_id)),
            clockId: Some(game_types::object_types::ObjectIdPointer(clock_object_id)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        character_root_object_id,
    );
    // The walk kit: an always-running walk animation that starts at weight 0 (the idle animation
    // below it shows through) until the spawn script's controller fades it in while moving.
    let walk_clock_object_id = 17;
    let walk_animation_object_id = 18;
    let walk_keyframe_start_object_id = 19;
    let walk_keyframe_start_pose_object_id = 20;
    let walk_keyframe_end_object_id = 21;
    let walk_keyframe_end_pose_object_id = 22;
    let walk_animator_object_id = 23;
    world.addObjectWithId(
        walk_clock_object_id,
        "Walk-Clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Server,
            },
            shouldLoop: true,
            speed: 1.0,
        }),
        character_root_object_id,
    );
    world.addObjectWithId(
        walk_animation_object_id,
        "Walk-Animation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        character_root_object_id,
    );
    for (keyframe_object_id, pose_object_id, time) in [
        (walk_keyframe_start_object_id, walk_keyframe_start_pose_object_id, 0.0_f32),
        (walk_keyframe_end_object_id, walk_keyframe_end_pose_object_id, 1.0),
    ] {
        world.addObjectWithId(
            keyframe_object_id,
            format!("walk-keyframe-{keyframe_object_id}"),
            ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time }),
            walk_animation_object_id,
        );
        // The walk pose holds the head at a constant offset, so the tests can tell exactly which
        // animation is showing at any weight.
        world.addObjectWithId(
            pose_object_id,
            format!("walk-pose-{pose_object_id}"),
            ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
                jointName: "Head".to_string(),
                localPose: identity_location(WALK_POSE_HEAD_OFFSET),
            }),
            keyframe_object_id,
        );
    }
    world.addObjectWithId(
        walk_animator_object_id,
        "Walk-Animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(game_types::object_types::ObjectIdPointer(skeleton_object_id)),
            skAnimationId: Some(game_types::object_types::ObjectIdPointer(walk_animation_object_id)),
            clockId: Some(game_types::object_types::ObjectIdPointer(walk_clock_object_id)),
            priority: 2,
            weight: 0.0,
            weightTransition: None,
        }),
        character_root_object_id,
    );

    world.addObjectWithId(
        script_object_id,
        "spawn-script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(script_object_id, SPAWN_SCRIPT.to_string());
    world.objectIdCounter = walk_animator_object_id;

    Ok(world)
}

fn find_child_by_name(world: &World, parent_object_id: ObjectId, name: &str) -> anyhow::Result<ObjectId> {
    world
        .getDirectChildrenIds(parent_object_id)?
        .iter()
        .copied()
        .find(|object_id| world.getObject(*object_id).map(|object| object.name == name).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("Expected child named {name} under object {parent_object_id}"))
}

fn add_player(world: &mut World, root_object_id: ObjectId, username: &str) -> ObjectId {
    world.addObject(
        username.to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: username.to_string(),
            userid: None,
            preferredColor: None,
            equippedItemId: None,
            headObjectId: None,
            characterObjectId: None,
        }),
        root_object_id,
    )
}

struct PastedCharacter {
    body_object_id: ObjectId,
    skeleton_object_id: ObjectId,
    torso_brick_object_id: ObjectId,
    head_brick_object_id: ObjectId,
    walk_clock_object_id: ObjectId,
    walk_animator_object_id: ObjectId,
}

// This resolves one pasted character and checks its internal pointers were rewired to the copy:
// the body follows the copy's own Skeleton, the attachments target the copy's own parts, and the
// animator kit points at the copy's own Skeleton/animation/clock.
fn assert_pasted_character_is_self_contained(world: &World, character_root_name: &str) -> anyhow::Result<PastedCharacter> {
    let character_root_object_id = *world
        .getObjectIdsByName(character_root_name)
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected pasted character {character_root_name}"))?;
    let body_object_id = find_child_by_name(world, character_root_object_id, "Body")?;
    let skeleton_object_id = find_child_by_name(world, character_root_object_id, "Joints")?;
    let torso_brick_object_id = find_child_by_name(world, body_object_id, "Torso")?;
    let head_brick_object_id = find_child_by_name(world, body_object_id, "Head")?;

    let ObjectInfo::RigidGroup(body_info) = &world.getObject(body_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected Body to be a rigid group"));
    };
    assert_eq!(
        body_info.followSkeletonId.map(ObjectId::from),
        Some(skeleton_object_id),
        "{character_root_name}: followSkeletonId must point at the pasted skeleton"
    );
    for (part_object_id, attachment_name) in [(torso_brick_object_id, "TorsoSkAttachment"), (head_brick_object_id, "HeadSkAttachment")] {
        let attachment_object_id = find_child_by_name(world, part_object_id, attachment_name)?;
        let ObjectInfo::SkAttachment(attachment_info) = &world.getObject(attachment_object_id)?.info else {
            return Err(anyhow::anyhow!("Expected {attachment_name} to be a SkAttachment"));
        };
        assert_eq!(
            attachment_info.objectId.as_ref().map(ObjectId::from),
            Some(part_object_id),
            "{character_root_name}: {attachment_name} must target the pasted part"
        );
    }

    // Both animation kits (idle and walk) must be rewired onto the copy's own Skeleton,
    // animation, and clock (not the template's or another player's).
    for (animator_name, animation_name, clock_name) in [
        ("Idle-Animator", "Idle-Animation", "Idle-Clock"),
        ("Walk-Animator", "Walk-Animation", "Walk-Clock"),
    ] {
        let animator_object_id = find_child_by_name(world, character_root_object_id, animator_name)?;
        let animation_object_id = find_child_by_name(world, character_root_object_id, animation_name)?;
        let clock_object_id = find_child_by_name(world, character_root_object_id, clock_name)?;
        let ObjectInfo::SkAnimator(animator_info) = &world.getObject(animator_object_id)?.info else {
            return Err(anyhow::anyhow!("Expected {animator_name} to be a SkAnimator"));
        };
        assert_eq!(animator_info.skeletonId.map(ObjectId::from), Some(skeleton_object_id));
        assert_eq!(animator_info.skAnimationId.map(ObjectId::from), Some(animation_object_id));
        assert_eq!(animator_info.clockId.map(ObjectId::from), Some(clock_object_id));
    }
    let walk_clock_object_id = find_child_by_name(world, character_root_object_id, "Walk-Clock")?;
    let walk_animator_object_id = find_child_by_name(world, character_root_object_id, "Walk-Animator")?;

    Ok(PastedCharacter {
        body_object_id,
        skeleton_object_id,
        torso_brick_object_id,
        head_brick_object_id,
        walk_clock_object_id,
        walk_animator_object_id,
    })
}

fn assert_player_wired_to_character(world: &World, player_object_id: ObjectId, character: &PastedCharacter) -> anyhow::Result<()> {
    let ObjectInfo::Player(player_info) = &world.getObject(player_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected player object"));
    };
    assert_eq!(player_info.characterObjectId.map(ObjectId::from), Some(character.body_object_id));
    assert_eq!(player_info.headObjectId.map(ObjectId::from), Some(character.head_brick_object_id));
    Ok(())
}

fn assert_vec3_near(actual: Vec3, expected: Vec3, label: &str) {
    let delta = actual - expected;
    assert!(delta.length() < 1e-3, "{label}: expected {expected:?}, got {actual:?}, delta {delta:?}");
}

fn part_position(world: &World, object_id: ObjectId) -> Vec3 {
    world
        .getObjectPhysicsInfo3D(object_id)
        .unwrap_or_else(|| panic!("Expected physics info for a character part"))
        .location
        .position
}

#[tokio::test]
async fn player_spawn_script_cuts_template_and_pastes_independent_characters_per_player() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut world = build_spawn_template_world(js_callback_inputs_tx)?;
    let root_object_id = world.getRootObjectId();

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // The source character must be gone from the world (this is the "template must disappear"
    // requirement) and stored as a template instead.
    assert!(
        world.getObjectIdsByName("SpawnCharacter").is_empty(),
        "template source must be cut from the world"
    );
    assert!(!world.hasObject(3), "template skeleton must be cut from the world");
    assert!(!world.hasObject(6), "template body must be cut from the world");
    assert!(world.templateOfName.contains_key("spawn-character"), "cut must store the template");

    let player_a_object_id = add_player(&mut world, root_object_id, "userA");
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let character_a = assert_pasted_character_is_self_contained(&world, "userA-Character")?;
    assert_player_wired_to_character(&world, player_a_object_id, &character_a)?;

    let player_b_object_id = add_player(&mut world, root_object_id, "userB");
    runPendingJS(&mut js_runtime, &mut world, 32.0, Some(32.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let character_b = assert_pasted_character_is_self_contained(&world, "userB-Character")?;
    assert_player_wired_to_character(&world, player_b_object_id, &character_b)?;
    assert_ne!(character_a.body_object_id, character_b.body_object_id);
    assert_ne!(character_a.skeleton_object_id, character_b.skeleton_object_id);

    // Re-check player A's copy after B joined: pasting for B must not rewire anything from A.
    let character_a_after_b = assert_pasted_character_is_self_contained(&world, "userA-Character")?;
    assert_eq!(character_a.body_object_id, character_a_after_b.body_object_id);
    assert_player_wired_to_character(&world, player_a_object_id, &character_a)?;

    // A zero-dt step (like the server's kinematic pass) must leave each character standing at its
    // own spawn point — not at the template's parking spot.
    world.step_physics(0, 0.0, 40.0, None)?;
    for (character, spawn_point, label) in [(&character_a, SPAWN_POINT_A, "player A"), (&character_b, SPAWN_POINT_B, "player B")] {
        assert_vec3_near(part_position(&world, character.torso_brick_object_id), spawn_point, &format!("{label} torso"));
        assert_vec3_near(
            part_position(&world, character.head_brick_object_id),
            spawn_point + HEAD_OFFSET_FROM_BODY,
            &format!("{label} head"),
        );
    }

    // Move each body (as the player controller would) and step again: the move must survive the
    // physics step. The parts must end up around the moved body — not get yanked back to a fixed
    // world position (the old frozen-avatar bug), and not drift off the pose either.
    let nudge = Vec3::new(1.5, 0.0, -2.0);
    for character in [&character_a, &character_b] {
        world.setObjectPositionOffsetRecursive(character.body_object_id, nudge, SetOrForce::Force)?;
    }
    world.step_physics(0, 0.0, 60.0, None)?;
    for (character, spawn_point, label) in [(&character_a, SPAWN_POINT_A, "player A"), (&character_b, SPAWN_POINT_B, "player B")] {
        let body_position = part_position(&world, character.body_object_id);
        let torso_position = part_position(&world, character.torso_brick_object_id);
        let head_position = part_position(&world, character.head_brick_object_id);

        assert_vec3_near(torso_position, spawn_point + nudge, &format!("{label} torso after moving"));
        assert_vec3_near(head_position - torso_position, HEAD_OFFSET_FROM_BODY, &format!("{label} head-torso offset"));
        assert_vec3_near(body_position - torso_position, Vec3::ZERO, &format!("{label} body-torso offset"));
    }

    Ok(())
}

// The walk controller in the spawn script watches each player's movement every tick: standing
// shows the idle pose, and moving fades the walk animation in with its clock speed scaled to how
// fast the player is going.
#[tokio::test]
async fn walk_controller_drives_the_walk_animation_from_player_velocity() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let mut world = build_spawn_template_world(js_callback_inputs_tx)?;
    let root_object_id = world.getRootObjectId();

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let _player_object_id = add_player(&mut world, root_object_id, "userA");
    runPendingJS(&mut js_runtime, &mut world, 16.0, None, &script_manager_rc, &mut js_callback_inputs_rx).await?;
    let character = assert_pasted_character_is_self_contained(&world, "userA-Character")?;

    // Standing still: the idle pose shows because the walk animator starts at weight 0.
    world.step_physics(0, 0.0, 40.0, None)?;
    assert_vec3_near(
        part_position(&world, character.head_brick_object_id),
        SPAWN_POINT_A + HEAD_OFFSET_FROM_BODY,
        "standing head",
    );

    // Start moving at half of the script's WALK_FULL_SPEED (9): one controller tick later, the
    // walk clock runs at half pace and the walk animation is fading in.
    // Use a kinematic target so this also checks the velocity Rapier derives during the real step.
    let moving_body_position = SPAWN_POINT_A + Vec3::new(4.5 * SERVER_SIM_AND_SEND_SECS, 0.0, 0.0);
    world.setObjectNextPhysicsInfo3D(
        character.body_object_id,
        PhysicsInfo3D::new(moving_body_position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
    )?;
    world.step_physics(1, SERVER_SIM_AND_SEND_SECS, 100.0, None)?;
    runPendingJS(&mut js_runtime, &mut world, 100.0, None, &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let ObjectInfo::Clock(walk_clock_info) = &world.getObject(character.walk_clock_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected the walk clock"));
    };
    assert!(
        (walk_clock_info.speed - 0.5).abs() < 1e-5,
        "walk clock speed must scale with movement speed, got {}",
        walk_clock_info.speed
    );
    let ObjectInfo::SkAnimator(walk_animator_info) = &world.getObject(character.walk_animator_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected the walk animator"));
    };
    assert!(
        walk_animator_info.weightTransition.is_some() || (walk_animator_info.weight - 100.0).abs() < 1e-5,
        "moving must fade the walk animation in"
    );

    // After the crossfade finishes, the walk pose shows.
    world.step_physics(0, 0.0, 400.0, None)?;
    assert_vec3_near(
        part_position(&world, character.head_brick_object_id),
        moving_body_position + HEAD_OFFSET_FROM_BODY + WALK_POSE_HEAD_OFFSET,
        "walking head",
    );

    // Stop moving: one controller tick later the walk animation fades back out, and the idle pose
    // returns once the crossfade finishes.
    world.setObjectNextPhysicsInfo3D(
        character.body_object_id,
        PhysicsInfo3D::new(moving_body_position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
    )?;
    world.step_physics(1, SERVER_SIM_AND_SEND_SECS, 500.0, None)?;
    runPendingJS(&mut js_runtime, &mut world, 500.0, None, &script_manager_rc, &mut js_callback_inputs_rx).await?;
    world.step_physics(0, 0.0, 800.0, None)?;
    assert_vec3_near(
        part_position(&world, character.head_brick_object_id),
        moving_body_position + HEAD_OFFSET_FROM_BODY,
        "stopped head",
    );

    Ok(())
}
