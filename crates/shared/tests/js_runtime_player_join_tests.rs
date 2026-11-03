mod helpers;

use std::collections::HashMap;

use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    object_types::{
        BallInfo, LiveObject, LocalGroupInfo, ObjectBeingAdded, ObjectId, ObjectInfo, ObjectLocation, PhysicsInfo3D, PhysicsToggles3D, PlayerInfo,
        RigidGroupInfo, ScriptInfo, VisualSettings, shared::Color3,
    },
    physics_primitive_types::BallPrimitiveSettings,
    stored_world_type::StoredWorld,
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::fallback_camera_location_for_tests;
use shared::game::{
    jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS},
    world::{World, WorldControlMode},
};
use tokio::sync::mpsc;

#[tokio::test]
async fn player_join_listener_receives_a_live_player_object_with_helpers() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
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

    let script_object_id = world.addObject(
        "join-listener-script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
game.objectService.addObjectCreatedListener((player) => {
  if (player.type !== "player") {
    throw new Error("Expected player object in join callback");
  }
  if (player.userid !== 17) {
    throw new Error("Expected the player's userid in join callback");
  }
  if (player.preferredColor?.g !== 0.5 || player.equippedItemId !== 20) {
    throw new Error("Expected the player's profile in join callback");
  }

  const parent = player.parent;
  if (parent === null || parent.id !== game.world.id) {
    throw new Error("Expected player parent helper to point at the world root");
  }

  const siblings = parent.getChildren();
  if (!siblings.some((child) => child.id === player.id)) {
    throw new Error("Expected getChildren helper to include the joined player");
  }

  player.name = "joined-player-renamed";
  player.preferredColor = { r: 0, g: 0, b: 1 };
  player.equippedItemId = null;
}, "player");

await tick();
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let player_object_id = world.addObject(
        "join-test-user".to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: "join-test-user".to_string(),
            userid: Some(17),
            preferredColor: Some(Color3 { r: 1.0, g: 0.5, b: 0.0 }),
            equippedItemId: Some(20),
            headObjectId: None,
            characterObjectId: None,
        }),
        root_object_id,
    );

    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let player_object = world.getObject(player_object_id)?;
    assert_eq!(player_object.name, "joined-player-renamed");
    let ObjectInfo::Player(PlayerInfo {
        preferredColor: Some(Color3 { r, g, b }),
        equippedItemId,
        ..
    }) = player_object.info
    else {
        anyhow::bail!("the player should have the preferredColor the script set");
    };
    assert_eq!(([r, g, b], equippedItemId), ([0.0, 0.0, 1.0], None));

    Ok(())
}

#[tokio::test]
async fn player_join_listener_can_assign_head_and_controls_for_new_server_player() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
    let root_object_id = 1;
    let pig_character_object_id = 2;
    let source_body_object_id = 3;
    let source_head_object_id = 4;
    let script_object_id = 5;
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

    world.addObjectWithId(
        pig_character_object_id,
        "PigCharacter".to_string(),
        ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
        root_object_id,
    );
    world.addObjectWithId(
        source_body_object_id,
        "Body".to_string(),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo { followSkeletonId: None },
            PhysicsInfo3D::new(Vec3::new(5.0, 1.0, -2.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
        ),
        pig_character_object_id,
    );
    world.addObjectWithId(
        source_head_object_id,
        "Head".to_string(),
        ObjectBeingAdded::Ball(
            BallInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BallPrimitiveSettings { radius: 1.0 },
            PhysicsInfo3D::new(Vec3::new(5.0, 2.0, -2.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        source_body_object_id,
    );
    world.addObjectWithId(
        script_object_id,
        "join-avatar-script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );

    world.markScriptAsReadyToRun(
        script_object_id,
        format!(
            r#"
game.objectService.addObjectCreatedListener((playerObject) => {{
  if (playerObject.type !== "player") {{
    throw new Error(`Expected player object in join callback, got ${{playerObject.type}}`);
  }}

  playerObject.headObjectId = {source_head_object_id};
  playerObject.characterObjectId = {source_body_object_id};
}}, "player");

await tick();
"#
        ),
    );

    // This keeps the next auto-generated id from colliding with the manually inserted template objects above.
    world.objectIdCounter = script_object_id;

    let root_object_id = world.getRootObjectId();
    let expected_controls_parent_id = pig_character_object_id;

    // This runs the real bundled JS API plus the test server script, so the join listener is registered exactly like production.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // This mirrors the real server join path: the player starts without a head or controlled object and relies on the join callback to create them.
    let player_object_id = world.addObject(
        "join-test-user".to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: "join-test-user".to_string(),
            userid: None,
            preferredColor: None,
            equippedItemId: None,
            headObjectId: None,
            characterObjectId: None,
        }),
        root_object_id,
    );

    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let player_object = world.getObject(player_object_id)?;
    let ObjectInfo::Player(player_info) = &player_object.info else {
        panic!("expected player object");
    };
    let Some(head_object_id) = player_info.headObjectId.map(ObjectId::from) else {
        panic!("expected join callback to assign headObjectId");
    };
    let Some(character_object_id) = player_info.characterObjectId.map(ObjectId::from) else {
        panic!("expected join callback to assign characterObjectId");
    };
    assert_eq!(head_object_id, source_head_object_id, "expected join callback to point at the head fixture");
    assert_eq!(
        character_object_id, source_body_object_id,
        "expected join callback to point at the body fixture"
    );

    // This proves the server callback assigned a usable avatar tree instead of only mutating player metadata.
    assert_eq!(
        world.parentOfId.get(&character_object_id).copied(),
        Some(expected_controls_parent_id),
        "expected controlled rigid group to keep the template body's parent"
    );
    assert_eq!(
        world.parentOfId.get(&head_object_id).copied(),
        Some(character_object_id),
        "expected head object to be a child of the controlled rigid group"
    );
    assert!(
        world.getObjectPhysicsInfo3D(head_object_id).is_some(),
        "expected head object to have physics so it can be used as the play camera target"
    );
    assert!(
        world.getObjectPhysicsInfo3D(character_object_id).is_some(),
        "expected controlled object to have physics after avatar creation"
    );

    Ok(())
}

#[tokio::test]
async fn player_leave_listener_receives_userid_and_removed_player_id() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = mpsc::unbounded_channel();
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

    let script_object_id = world.addObject(
        "leave-listener-script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
// the player object is gone by the time the deleted listener fires, so record the userid at join time
const useridOfPlayerObjectId = {};
game.objectService.addObjectCreatedListener((player) => {
  useridOfPlayerObjectId[player.id] = player.userid;
}, "player");

game.objectService.addObjectDeletedListener((playerObjectId, objectType) => {
  if (objectType !== "player") {
    throw new Error("Expected player type in filtered deleted callback");
  }

  if (typeof playerObjectId !== "number") {
    throw new Error("Expected player object id in leave callback");
  }

  if (useridOfPlayerObjectId[playerObjectId] !== 23) {
    throw new Error("Expected leave callback userid to match removed player");
  }

  if (game.objectService.getObjectById(playerObjectId) !== null) {
    throw new Error("Expected leaving player object to already be removed");
  }

  game.objectService.createObject(
    "leave-callback-marker",
    { type: "group" },
    game.world.id,
  );
}, "player");

await tick();
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let player_object_id = world.addObject(
        "leave-test-user".to_string(),
        ObjectBeingAdded::Player(PlayerInfo {
            username: "leave-test-user".to_string(),
            userid: Some(23),
            preferredColor: None,
            equippedItemId: None,
            headObjectId: None,
            characterObjectId: None,
        }),
        root_object_id,
    );

    runPendingJS(&mut js_runtime, &mut world, 8.0, Some(8.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    world.removeObject(player_object_id)?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    assert_eq!(world.getObjectIdsByName("leave-callback-marker").len(), 1);
    assert!(!world.hasObject(player_object_id));

    Ok(())
}
