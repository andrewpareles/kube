mod helpers;

use glam::Vec3;
use helpers::world_test_helpers::{add_ball, add_brick, add_player, add_script, make_world, physics_info};
use shared::game::jsRuntime::{JsConsoleLogEntry, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runPendingJS};
use shared::game::physics::{SetPhysicsInfo3D, SetPhysicsToggles3D};
use shared::game::world::{World, WorldControlMode};
use std::sync::Arc;

#[tokio::test]
async fn collision_service_listeners_receive_live_objects_with_helpers() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx);

    let root_object_id = world.getRootObjectId();
    let lower_ball_object_id = 2;
    let higher_ball_object_id = 3;
    let script_object_id = 4;

    // This creates two overlapping unpushable balls so the physics step emits a deterministic start event.
    add_ball(&mut world, lower_ball_object_id, root_object_id, Vec3::ZERO, false)?;
    add_ball(&mut world, higher_ball_object_id, root_object_id, Vec3::new(0.5, 0.0, 0.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;

    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
game.collisionService.addCollisionListener((object1, object2) => {
  const lowerObject = object1.id < object2.id ? object1 : object2;
  const higherObject = object1.id < object2.id ? object2 : object1;

  if (lowerObject.type !== "ball" || higherObject.type !== "ball") {
    throw new Error("Expected ball objects in collision start callback");
  }

  const parent = lowerObject.parent;
  if (parent === null || parent.id !== game.world.id) {
    throw new Error("Expected collision object parent helper to point at the world root");
  }

  const siblingIds = parent.getChildren().map((child) => child.id);
  if (!siblingIds.includes(lowerObject.id) || !siblingIds.includes(higherObject.id)) {
    throw new Error("Expected getChildren helper to include both colliding balls");
  }

  lowerObject.name = "collision-start-lower-ball";
  higherObject.name = "collision-start-higher-ball";
});

game.collisionService.addCollisionStopListener((object1, object2) => {
  const lowerObject = object1.id < object2.id ? object1 : object2;
  const higherObject = object1.id < object2.id ? object2 : object1;

  const parent = higherObject.parent;
  if (parent === null || parent.id !== game.world.id) {
    throw new Error("Expected collision stop object parent helper to point at the world root");
  }

  const siblingIds = parent.getChildren().map((child) => child.id);
  if (!siblingIds.includes(lowerObject.id) || !siblingIds.includes(higherObject.id)) {
    throw new Error("Expected collision stop getChildren helper to include both balls");
  }

  lowerObject.name = "collision-stop-lower-ball";
  higherObject.name = "collision-stop-higher-ball";
});

await tick();
"#
        .to_string(),
    );

    // This loads the bundled JS API and registers the real collision listeners through the runtime bridge.
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    world.step_server(1, 1.0 / 60.0, 16.0)?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let lower_ball_object = world.getObject(lower_ball_object_id)?;
    let higher_ball_object = world.getObject(higher_ball_object_id)?;
    assert_eq!(lower_ball_object.name, "collision-start-lower-ball");
    assert_eq!(higher_ball_object.name, "collision-start-higher-ball");

    // This teleports one ball out of overlap so the next physics step emits the stop event.
    world.forcePhysicsInfo3D(
        higher_ball_object_id,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(6.0, 0.0, 0.0)),
            ..Default::default()
        },
        true,
    );
    world.step_server(1, 1.0 / 60.0, 32.0)?;
    runPendingJS(&mut js_runtime, &mut world, 32.0, Some(32.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let lower_ball_object = world.getObject(lower_ball_object_id)?;
    let higher_ball_object = world.getObject(higher_ball_object_id)?;
    assert_eq!(lower_ball_object.name, "collision-stop-lower-ball");
    assert_eq!(higher_ball_object.name, "collision-stop-higher-ball");

    Ok(())
}

#[tokio::test]
async fn collisions_nobody_listens_for_stay_within_the_js_cpu_budget() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;

    // This adds overlapping unpushable balls after the world loads, like a big paste, so the next physics step reports every touching pair as a new collision.
    let root_object_id = world.getRootObjectId();
    let grid_side = 200;
    for index in 0..grid_side * grid_side {
        let position = Vec3::new((index % grid_side) as f32 * 1.8, 0.0, (index / grid_side) as f32 * 1.8);
        add_ball(&mut world, 2 + index, root_object_id, position, false)?;
    }
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx);

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    world.step_server(1, 1.0 / 60.0, 16.0)?;
    assert_eq!(js_callback_inputs_rx.len(), 2 * grid_side * (grid_side - 1));
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    Ok(())
}

#[tokio::test]
async fn objects_already_touching_when_the_world_loads_fire_only_a_stop() -> anyhow::Result<()> {
    // This saves a world with two touching unpushable balls and a script that logs every collision start and stop.
    let mut saved_world = make_world()?;
    let root_object_id = saved_world.getRootObjectId();
    let script_object_id = 4;
    add_ball(&mut saved_world, 2, root_object_id, Vec3::ZERO, false)?;
    add_ball(&mut saved_world, 3, root_object_id, Vec3::new(1.5, 0.0, 0.0), false)?;
    add_script(&mut saved_world, script_object_id, root_object_id)?;
    saved_world.markScriptAsReadyToRun(
        script_object_id,
        r#"
game.collisionService.addCollisionListener((object1, object2) => console.log("start", [object1.name, object2.name].sort().join(" ")));
game.collisionService.addCollisionStopListener((object1, object2) => console.log("stop", [object1.name, object2.name].sort().join(" ")));
"#
        .to_string(),
    );

    // This loads it the way a server or a joining client does, then adds a ball that touches only one of the loaded balls.
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = World::fromStoredWorldString(&saved_world.toStoredWorldString()?, WorldControlMode::Server, Some(js_callback_inputs_tx))?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, 100, root_object_id, Vec3::new(-1.5, 0.0, 0.0), false)?;

    let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel::<JsConsoleLogEntry>();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    world.step_server(1, 1.0 / 60.0, 16.0)?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    let messages: Vec<String> = console_log_entry_rx.try_iter().map(|console_log_entry| console_log_entry.message).collect();
    assert_eq!(messages, vec!["start ball-100 ball-2\n"]);

    // This moves one loaded ball away from the other, so their contact stops.
    let ball_3_object_id = world
        .objectOfId
        .values()
        .find(|object| object.name == "ball-3")
        .map(|object| object.id)
        .ok_or_else(|| anyhow::anyhow!("Expected ball-3 to be loaded"))?;
    world.forcePhysicsInfo3D(
        ball_3_object_id,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(10.0, 0.0, 0.0)),
            ..Default::default()
        },
        true,
    );
    world.step_server(1, 1.0 / 60.0, 32.0)?;
    runPendingJS(&mut js_runtime, &mut world, 32.0, Some(32.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;
    let messages: Vec<String> = console_log_entry_rx.try_iter().map(|console_log_entry| console_log_entry.message).collect();
    assert_eq!(messages, vec!["stop ball-2 ball-3\n"]);

    Ok(())
}

#[tokio::test]
async fn an_unpushable_ball_that_starts_moving_while_touching_another_fires_only_a_stop() -> anyhow::Result<()> {
    // This saves a world with two touching unpushable balls and a script that logs every collision start and stop.
    let mut saved_world = make_world()?;
    let root_object_id = saved_world.getRootObjectId();
    let script_object_id = 4;
    add_ball(&mut saved_world, 2, root_object_id, Vec3::ZERO, false)?;
    add_ball(&mut saved_world, 3, root_object_id, Vec3::new(1.5, 0.0, 0.0), false)?;
    add_script(&mut saved_world, script_object_id, root_object_id)?;
    saved_world.markScriptAsReadyToRun(
        script_object_id,
        r#"
game.collisionService.addCollisionListener((object1, object2) => console.log("start", [object1.name, object2.name].sort().join(" ")));
game.collisionService.addCollisionStopListener((object1, object2) => console.log("stop", [object1.name, object2.name].sort().join(" ")));
"#
        .to_string(),
    );

    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = World::fromStoredWorldString(&saved_world.toStoredWorldString()?, WorldControlMode::Server, Some(js_callback_inputs_tx))?;
    let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel::<JsConsoleLogEntry>();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let ball_3_object_id = world
        .objectOfId
        .values()
        .find(|object| object.name == "ball-3")
        .map(|object| object.id)
        .ok_or_else(|| anyhow::anyhow!("Expected ball-3 to be loaded"))?;

    // This moves one ball the way a script or an animator does: first a little, so it starts moving while still touching
    // the other, then the rest of the way out. Rapier sees a smooth move's contacts change on the step after, so each
    // move runs two ticks.
    let mut messages_of_move = Vec::new();
    for (move_index, x) in [1.6, 4.0].into_iter().enumerate() {
        world.setPhysicsInfo3D(
            ball_3_object_id,
            SetPhysicsInfo3D {
                position: Some(Vec3::new(x, 0.0, 0.0)),
                ..Default::default()
            },
            false,
        );
        for tick_index in 1..=2 {
            let sim_ms = (move_index * 2 + tick_index) as f64 * 1000.0 / 60.0;
            world.step_server(1, 1.0 / 60.0, sim_ms)?;
            runPendingJS(
                &mut js_runtime,
                &mut world,
                sim_ms,
                Some(sim_ms),
                &script_manager_rc,
                &mut js_callback_inputs_rx,
            )
            .await?;
        }
        messages_of_move.push(
            console_log_entry_rx
                .try_iter()
                .map(|console_log_entry| console_log_entry.message)
                .collect::<Vec<String>>(),
        );
    }
    assert_eq!(messages_of_move, vec![vec![], vec!["stop ball-2 ball-3\n".to_string()]]);

    Ok(())
}

#[tokio::test]
async fn a_players_character_walking_into_an_unpushable_walkthrough_brick_on_the_server_fires_a_start() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx);
    let root_object_id = world.getRootObjectId();
    let trigger_object_id = 2;
    let character_object_id = 3;
    let player_object_id = 4;
    let script_object_id = 5;

    // This puts an unpushable walkthrough brick ahead of a player's character, which the server only moves where the player's packets say.
    add_brick(&mut world, trigger_object_id, root_object_id, Vec3::new(5.0, 0.0, 0.0), false)?;
    world.setPhysicsToggles3D(
        trigger_object_id,
        SetPhysicsToggles3D {
            isWalkthrough: Some(true),
            ..Default::default()
        },
        false,
    );
    add_ball(&mut world, character_object_id, root_object_id, Vec3::ZERO, true)?;
    add_player(&mut world, player_object_id, root_object_id, Some(character_object_id))?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"game.collisionService.addCollisionListener((object1, object2) => console.log("start", [object1.name, object2.name].sort().join(" ")));"#.to_string(),
    );

    let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel::<JsConsoleLogEntry>();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // This streams the player walking through the brick, one step of movement per tick.
    for step_index in 1..=60 {
        world.setObjectNextPhysicsInfo3D(character_object_id, physics_info(Vec3::new(step_index as f32 * 0.1, 0.0, 0.0)))?;
        let sim_ms = step_index as f64 * 1000.0 / 60.0;
        world.step_server(1, 1.0 / 60.0, sim_ms)?;
        runPendingJS(
            &mut js_runtime,
            &mut world,
            sim_ms,
            Some(sim_ms),
            &script_manager_rc,
            &mut js_callback_inputs_rx,
        )
        .await?;
    }
    let messages: Vec<String> = console_log_entry_rx.try_iter().map(|console_log_entry| console_log_entry.message).collect();
    assert_eq!(messages, vec!["start ball-3 brick-2\n"]);

    Ok(())
}

#[tokio::test]
async fn a_pile_of_walkthrough_balls_only_fires_collisions_with_the_solid_ball_in_it() -> anyhow::Result<()> {
    // This saves a world with a solid ball, walkthrough balls spread out away from it, and a script that logs every collision start.
    let mut saved_world = make_world()?;
    let root_object_id = saved_world.getRootObjectId();
    let solid_ball_object_id = 2;
    let script_object_id = 3;
    let walkthrough_ball_object_ids: Vec<usize> = (30..50).collect();
    add_ball(&mut saved_world, solid_ball_object_id, root_object_id, Vec3::ZERO, false)?;
    for (index, &walkthrough_ball_object_id) in walkthrough_ball_object_ids.iter().enumerate() {
        add_ball(
            &mut saved_world,
            walkthrough_ball_object_id,
            root_object_id,
            Vec3::new(10.0 + index as f32 * 10.0, 0.0, 0.0),
            false,
        )?;
        saved_world.setPhysicsToggles3D(
            walkthrough_ball_object_id,
            SetPhysicsToggles3D {
                isWalkthrough: Some(true),
                ..Default::default()
            },
            false,
        );
    }
    add_script(&mut saved_world, script_object_id, root_object_id)?;
    saved_world.markScriptAsReadyToRun(
        script_object_id,
        r#"game.collisionService.addCollisionListener((object1, object2) => console.log("start", [object1.name, object2.name].sort().join(" ")));"#.to_string(),
    );

    // This loads it the way a server or a joining client does, so the walkthrough balls are created walkthrough.
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = World::fromStoredWorldString(&saved_world.toStoredWorldString()?, WorldControlMode::Server, Some(js_callback_inputs_tx))?;
    let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel::<JsConsoleLogEntry>();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // This piles every walkthrough ball onto the solid ball, like a script parking unused effects in one spot.
    let object_id_of_name: std::collections::HashMap<String, usize> = world.objectOfId.values().map(|object| (object.name.clone(), object.id)).collect();
    for walkthrough_ball_object_id in walkthrough_ball_object_ids.iter() {
        world.forcePhysicsInfo3D(
            object_id_of_name[&format!("ball-{walkthrough_ball_object_id}")],
            SetPhysicsInfo3D {
                position: Some(Vec3::new(0.1, 0.0, 0.0)),
                ..Default::default()
            },
            true,
        );
    }
    world.step_server(1, 1.0 / 60.0, 16.0)?;
    runPendingJS(&mut js_runtime, &mut world, 16.0, Some(16.0), &script_manager_rc, &mut js_callback_inputs_rx).await?;

    let mut messages: Vec<String> = console_log_entry_rx.try_iter().map(|console_log_entry| console_log_entry.message).collect();
    messages.sort();
    let mut expected_messages: Vec<String> = walkthrough_ball_object_ids
        .iter()
        .map(|walkthrough_ball_object_id| format!("start ball-{solid_ball_object_id} ball-{walkthrough_ball_object_id}\n"))
        .collect();
    expected_messages.sort();
    assert_eq!(messages, expected_messages);

    Ok(())
}

#[tokio::test]
async fn a_ball_that_turns_walkthrough_stops_colliding_with_the_walkthrough_ball_it_overlaps() -> anyhow::Result<()> {
    let (js_callback_inputs_tx, mut js_callback_inputs_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut world = make_world()?;
    world.js_callback_inputs_tx = Some(js_callback_inputs_tx);
    let root_object_id = world.getRootObjectId();
    let walkthrough_ball_object_id = 2;
    let toggled_ball_object_id = 3;
    let script_object_id = 4;

    // This overlaps a walkthrough ball with a solid one, then logs every collision start and stop.
    add_ball(&mut world, walkthrough_ball_object_id, root_object_id, Vec3::ZERO, false)?;
    world.setPhysicsToggles3D(
        walkthrough_ball_object_id,
        SetPhysicsToggles3D {
            isWalkthrough: Some(true),
            ..Default::default()
        },
        false,
    );
    add_ball(&mut world, toggled_ball_object_id, root_object_id, Vec3::new(0.5, 0.0, 0.0), false)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
game.collisionService.addCollisionListener((object1, object2) => console.log("start", [object1.name, object2.name].sort().join(" ")));
game.collisionService.addCollisionStopListener((object1, object2) => console.log("stop", [object1.name, object2.name].sort().join(" ")));
"#
        .to_string(),
    );
    let (console_log_entry_tx, console_log_entry_rx) = std::sync::mpsc::channel::<JsConsoleLogEntry>();
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new(move |console_log_entry| {
            let _ = console_log_entry_tx.send(console_log_entry);
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    // This turns the solid ball walkthrough and back, the way a script's setObjectWalkthrough does, running two ticks after each change.
    let mut messages_of_change = Vec::new();
    for (change_index, is_walkthrough) in [None, Some(true), Some(false)].into_iter().enumerate() {
        if let Some(is_walkthrough) = is_walkthrough {
            world.setPhysicsToggles3D(
                toggled_ball_object_id,
                SetPhysicsToggles3D {
                    isWalkthrough: Some(is_walkthrough),
                    ..Default::default()
                },
                false,
            );
        }
        for tick_index in 1..=2 {
            let sim_ms = (change_index * 2 + tick_index) as f64 * 1000.0 / 60.0;
            world.step_server(1, 1.0 / 60.0, sim_ms)?;
            runPendingJS(
                &mut js_runtime,
                &mut world,
                sim_ms,
                Some(sim_ms),
                &script_manager_rc,
                &mut js_callback_inputs_rx,
            )
            .await?;
        }
        messages_of_change.push(
            console_log_entry_rx
                .try_iter()
                .map(|console_log_entry| console_log_entry.message)
                .collect::<Vec<String>>(),
        );
    }
    assert_eq!(
        messages_of_change,
        vec![
            vec!["start ball-2 ball-3\n".to_string()],
            vec!["stop ball-2 ball-3\n".to_string()],
            vec!["start ball-2 ball-3\n".to_string()],
        ]
    );

    Ok(())
}
