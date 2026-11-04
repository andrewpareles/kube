#![allow(non_snake_case)]

use crate::playtest_auth::PlaytestServerStatus;
use crate::server_networking::{Server, ServerForeignCharacterDTO, ServerForeignPlayerDTO, ServerMode};
use game_types::{
    consts::{SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK, SERVER_PLAYER_PINGS_INTERVAL_MS, SERVER_SIM_AND_SEND_SECS},
    object_types::{ObjectId, ObjectInfo, ObjectKind},
};
use quinn::Connection;
use shared::game::{
    jsRuntime::{
        JsConsoleLogCallback, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, pollServerFnCall, runPendingJS,
        startRegisteredServerFn,
    },
    world::{JsCallbackInputs, World, WorldControlMode},
};
use shared::{
    helpers::multi_thread_hashmap::MultiThreadHashMap,
    protocol::messages::{MessageNum, NetworkedCoords, NetworkedPhysicsInfo3D, ServerFnCallMessage},
};
use std::{
    collections::HashMap,
    sync::Arc,
    task::Poll,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const SERVER_FN_CALL_START_TIME_PER_TICK: Duration = Duration::from_millis(8); // TODO: AI-PICKED-VALUE: about half of a 60 Hz tick, so calls can't crowd out physics and snapshots.

pub fn start_running_server_in_bg(
    stored_world_string: &str,
    kill_server_cancel_token: CancellationToken,
    player_cleanup_tasks_tracker: TaskTracker,
    server_start_time: Instant,
    server_port: u16,
    server_mode: ServerMode,
    console_log_callback: Option<JsConsoleLogCallback>,
) -> Option<Arc<RwLock<World>>> {
    let (js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
    let world_res = World::fromStoredWorldString(stored_world_string, WorldControlMode::Server, Some(js_callback_inputs_tx.clone())); // server just started, so time since start is 0
    let world = match world_res {
        Ok(world) => world,
        Err(e) => {
            log::error!("Server failed to create world from stored world string: {}", e);
            kill_server_cancel_token.cancel();
            return None;
        }
    };

    // create structs shared around main and networking processes
    let worldArc = Arc::new(RwLock::new(world));
    let connection_of_player_object_id_arc = Arc::new(MultiThreadHashMap::new());
    let serverArc = Arc::new(Server::new(
        server_start_time,
        connection_of_player_object_id_arc.clone(),
        server_mode,
        player_cleanup_tasks_tracker,
        Some(js_callback_inputs_tx),
    ));
    let (foreign_character_dto_tx, foreign_character_dto_rx) = mpsc::unbounded_channel();
    let (foreign_player_dto_tx, foreign_player_dto_rx) = mpsc::unbounded_channel();
    let (server_js_fn_request_tx, server_js_fn_request_rx) =
        mpsc::unbounded_channel::<(ObjectId, ServerFnCallMessage, oneshot::Sender<anyhow::Result<String>>)>();

    // start networking process
    tokio::spawn({
        let serverArc = serverArc.clone();
        let worldArc = worldArc.clone();
        let kill_server_cancel_token = kill_server_cancel_token.clone();
        async move {
            log::info!("Accepting conections...");
            if let Err(e) = serverArc
                .clone()
                .startAcceptingConnectionsForever(
                    worldArc, // takes in worldArc because it adds players to the world and send them world.toStoredWorldString()
                    foreign_character_dto_tx,
                    foreign_player_dto_tx,
                    server_js_fn_request_tx,
                    kill_server_cancel_token.clone(),
                    server_port,
                )
                .await
            {
                kill_server_cancel_token.cancel();
                log::error!("Networking server loop exited with error: {}", e);
            }
            if let ServerMode::Playtest(auth_arc) = &serverArc.server_mode {
                auth_arc.set_status(PlaytestServerStatus::Stopped);
            }
        }
    });

    std::thread::spawn({
        let worldArc = worldArc.clone();
        move || {
            let result = run_server_on_current_thread_forever(
                worldArc,
                serverArc,
                connection_of_player_object_id_arc,
                foreign_character_dto_rx,
                foreign_player_dto_rx,
                server_js_fn_request_rx,
                js_callback_inputs_rx,
                kill_server_cancel_token.clone(),
                console_log_callback,
            );
            match result {
                Ok(()) => log::info!("Server loop exited successfully"),
                Err(e) => log::error!("Server loop exited with error: {}", e),
            }
            // always signal shutdown when the server loop thread exits so main can unblock.
            kill_server_cancel_token.cancel();
        }
    });

    Some(worldArc)
}

// private
// give this fn its own single-threaded tokio runtime, you must call it from inside std::thread::spawn()
// server must not be passed around among worker threads because of jsruntime, must be run on 1 thread, this tells it to do that
#[tokio::main(flavor = "current_thread")]
async fn run_server_on_current_thread_forever(
    worldArc: Arc<RwLock<World>>,
    serverArc: Arc<Server>,
    connection_of_player_object_id_arc: Arc<MultiThreadHashMap<ObjectId, Connection>>,
    mut foreign_character_dto_rx: mpsc::UnboundedReceiver<(ServerForeignCharacterDTO, MessageNum)>, // with the input's latest_applied_message_num
    mut foreign_player_dto_rx: mpsc::UnboundedReceiver<ServerForeignPlayerDTO>,
    mut server_js_fn_request_rx: mpsc::UnboundedReceiver<(ObjectId, ServerFnCallMessage, oneshot::Sender<anyhow::Result<String>>)>,
    mut js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    kill_server_cancel_token: CancellationToken,
    console_log_callback: Option<JsConsoleLogCallback>,
) -> anyhow::Result<()> {
    log::info!("Server loop running!!!!!!!!!!!!!!!!");

    // set up jsRuntime
    let (mut jsRuntime, script_manager_rc) = match initializeJsRuntime(
        JsRuntimeInitialization::ClientFnConnections(connection_of_player_object_id_arc),
        console_log_callback,
    )
    .await
    {
        Ok(result) => result,
        Err(e) => anyhow::bail!("Server failed to initialize JS runtime: {}", e),
    };
    log::info!("JS runtime initialized!!!!!!!!!!!!!!!!");

    // Start the shared framework and the initial world scripts before queued JS callback inputs can fire.
    let mut world_lock = worldArc.write().await;
    let initial_live_server_wall_ms = serverArc.get_live_server_wall_ms();
    initializeJsAndRunInitialScripts(&mut jsRuntime, &mut world_lock, initial_live_server_wall_ms, None, &script_manager_rc).await?;
    drop(world_lock);
    log::info!("Initial JS run complete!!!!!!!!!!!!!!!!");

    // start game loop
    let mut physics_ticker = tokio::time::interval(Duration::from_secs_f32(SERVER_SIM_AND_SEND_SECS));
    physics_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut prev_physics_step_live_server_sim_ms = serverArc.get_live_server_wall_ms();
    let mut foreign_character_dtos = HashMap::new();
    let mut foreign_player_dtos = HashMap::new();
    let mut next_player_pings_live_server_wall_ms = 0.0;
    let mut did_prev_tick_overrun_budget = false;
    let mut waiting_server_fn_calls = Vec::new();

    loop {
        enum Tick {
            Break,
            StepPhysics(tokio::time::Instant),
        }
        let tick = tokio::select! {
                _ = kill_server_cancel_token.cancelled() => Tick::Break,
                tick_instant = physics_ticker.tick() => Tick::StepPhysics(tick_instant),
        };

        // pick which one comes first
        match tick {
            Tick::Break => {
                break;
            }
            Tick::StepPhysics(tick_instant) => {
                let tick_started_at = Instant::now();
                // take world_lock for this whole step
                let mut world_lock = worldArc.write().await;

                // get num_steps
                let live_server_sim_ms = serverArc.get_live_server_sim_ms_at(tick_instant.into_std());
                // Like the client: after a tick that ran over its time, catching up would make the next one longer still, so
                // take one step and drop the backlog. The game runs in slow motion instead of freezing.
                let num_steps = World::get_num_physics_steps_to_take(
                    prev_physics_step_live_server_sim_ms,
                    live_server_sim_ms,
                    SERVER_SIM_AND_SEND_SECS * 1000.0,
                    if did_prev_tick_overrun_budget { 1 } else { SERVER_MAX_NUM_PHYSICS_STEPS_PER_TICK },
                );
                if num_steps == 0 {
                    continue;
                }
                prev_physics_step_live_server_sim_ms = live_server_sim_ms;
                // 1. apply player-owned object updates

                while let Ok((foreign_character_dto, latest_applied_message_num)) = foreign_character_dto_rx.try_recv() {
                    // ignore the user if we should
                    if serverArc
                        .shouldIgnoreForeignCharacterDTO(&world_lock, &foreign_character_dto, latest_applied_message_num)
                        .await
                    {
                        continue;
                    }

                    // get their character object to which we will apply the change
                    let Some(character_object_id) = world_lock.getPlayerCharacterObject(foreign_character_dto.came_from_player_object_id) else {
                        continue;
                    };

                    // update player positions kinematically, unless they jumped
                    let res = world_lock.setNetworkedPhysicsInfo3D(
                        character_object_id,
                        NetworkedPhysicsInfo3D {
                            coords: NetworkedCoords::WorldCoords(foreign_character_dto.physicsInfo3D.clone()),
                            teleportCount: foreign_character_dto.teleportCount,
                        },
                    );
                    if let Err(e) = res {
                        log::warn!("Error setting next kinematic location on server: {}", e);
                        continue;
                    }

                    foreign_character_dtos.insert(character_object_id, foreign_character_dto);
                }

                // each player's camera and metadata, whether or not it has a character
                while let Ok(foreign_player_dto) = foreign_player_dto_rx.try_recv() {
                    let player_object_id = foreign_player_dto.came_from_player_object_id;
                    if !world_lock.hasObject(player_object_id) {
                        continue;
                    }
                    world_lock.setPlayerCameraOrientation(player_object_id, foreign_player_dto.foreign_player_delta.cameraOrientation);
                    world_lock.setPlayerMetadata(player_object_id, foreign_player_dto.foreign_player_delta.playerMetadata.clone());
                    foreign_player_dtos.insert(player_object_id, foreign_player_dto);
                }
                // remember the metadata watchers get this tick, the newest one per player, for getPlayerMetadataAtTime
                for (&player_object_id, foreign_player_dto) in foreign_player_dtos.iter() {
                    world_lock.recordPlayerMetadata(
                        player_object_id,
                        foreign_player_dto.foreign_player_delta.est_live_server_sim_ms,
                        foreign_player_dto.foreign_player_delta.playerMetadata.clone(),
                    );
                }

                // 2. apply server-owned object updates
                // (there are none other than just running physics)

                // 3. start queued server function calls on the JS thread, in order. Each runs up to its first await, and the rest of it
                // runs with the other scripts in step 5. Calls that don't fit in this tick's time start on the next tick.
                let server_fn_calls_started_at = Instant::now();
                while server_fn_calls_started_at.elapsed() < SERVER_FN_CALL_START_TIME_PER_TICK
                    && let Ok((player_object_id, ServerFnCallMessage { fn_id, js_args }, server_js_fn_oneshot_tx)) = server_js_fn_request_rx.try_recv()
                {
                    match startRegisteredServerFn(
                        &mut jsRuntime,
                        &mut world_lock,
                        live_server_sim_ms,
                        &script_manager_rc,
                        player_object_id,
                        &fn_id,
                        &js_args,
                    ) {
                        Ok(server_fn_call) => waiting_server_fn_calls.push((server_fn_call, server_js_fn_oneshot_tx)),
                        Err(error) => {
                            log::warn!("Error calling server function {} for player {}: {}", fn_id, player_object_id, error);
                            let _ = server_js_fn_oneshot_tx.send(Err(error));
                        }
                    }
                }

                // 4. step physics
                let res = world_lock.step_server(num_steps, SERVER_SIM_AND_SEND_SECS, live_server_sim_ms);
                if let Err(e) = res {
                    log::warn!("Error stepping server physics: {}", e);
                }

                // 5. run JS, after physics
                // this way scripts see the most up to date world, and the callbacks (eg for collisions) run on the tick they just happened on
                if let Err(e) = runPendingJS(
                    &mut jsRuntime,
                    &mut world_lock,
                    live_server_sim_ms,
                    None,
                    &script_manager_rc,
                    &mut js_callback_inputs_rx,
                )
                .await
                {
                    log::error!("Error running JS: {}", e);
                }

                // send the result of each server function call that settled. A call whose player left is forgotten, though its JS keeps running.
                for (mut server_fn_call, server_js_fn_oneshot_tx) in std::mem::take(&mut waiting_server_fn_calls) {
                    match pollServerFnCall(&mut jsRuntime, &mut server_fn_call) {
                        Poll::Ready(server_fn_result) => {
                            let _ = server_js_fn_oneshot_tx.send(server_fn_result);
                        }
                        Poll::Pending if server_js_fn_oneshot_tx.is_closed() => {}
                        Poll::Pending => waiting_server_fn_calls.push((server_fn_call, server_js_fn_oneshot_tx)),
                    }
                }

                // 6. apply transitions
                world_lock.flushAndSetFinishedWeightTransitions(live_server_sim_ms, None);

                // 7. send snapshots to users
                let world_dirty_object_flags = world_lock.flushDirtyObjects();

                // Each player's character leaves this tick with one pose: where the server moved it, else where its player sent it.
                // Giving a player a character marks it as moved by the server, so a new character starts where it stands.
                for &player_object_id in world_lock._kindOfObjectId.get_keys(&ObjectKind::Player).into_iter().flatten() {
                    let Some(character_object_id) = world_lock.getPlayerCharacterObject(player_object_id) else {
                        continue;
                    };
                    let is_moved_by_server = world_dirty_object_flags
                        .modifiedObjectFlags
                        .get(&character_object_id)
                        .is_some_and(|flags| flags.modifiedPhysicsInfo3D);
                    if !is_moved_by_server {
                        continue;
                    }
                    let Some(physics_info_3d) = world_lock.getObjectPhysicsInfo3D(character_object_id) else {
                        continue;
                    };
                    foreign_character_dtos.insert(
                        character_object_id,
                        ServerForeignCharacterDTO {
                            came_from_player_object_id: player_object_id,
                            est_live_server_sim_ms: live_server_sim_ms,
                            physicsInfo3D: physics_info_3d,
                            teleportCount: world_lock.getTeleportCount(character_object_id),
                        },
                    );
                }
                // remember the poses watchers get this tick, the newest one per character, for getCharacterLocationAtTime
                for (&character_object_id, foreign_character_dto) in foreign_character_dtos.iter() {
                    world_lock.recordCharacterPose(
                        character_object_id,
                        foreign_character_dto.est_live_server_sim_ms,
                        foreign_character_dto.physicsInfo3D.clone(),
                        foreign_character_dto.teleportCount,
                    );
                }

                // flush foreign character and player deltas we collected this step
                let foreign_character_dtos = std::mem::take(&mut foreign_character_dtos);
                let foreign_player_dtos = std::mem::take(&mut foreign_player_dtos);

                // get new script deltas in addition to the other deltas (it's computed from scripts that were added and marked dirty)
                let mut new_script_deltas = HashMap::new();
                for object_id in world_dirty_object_flags.addedObjectIds.iter().copied() {
                    // elimintate non-scripts
                    let Ok(script_live) = world_lock.getObject(object_id) else {
                        continue;
                    };
                    let ObjectInfo::Script(_) = &script_live.info else {
                        continue;
                    };
                    // get the raw code and put in new_script_deltas
                    let Some(raw_code) = world_lock.scriptRawCodeOfId.get(&object_id).cloned() else {
                        log::error!("A script on the server somehow didn't have content {}", object_id);
                        continue;
                    };
                    new_script_deltas.insert(object_id, raw_code);
                }

                serverArc
                    .sendNewSnapshotToAllUsers(
                        world_dirty_object_flags,
                        foreign_character_dtos,
                        foreign_player_dtos,
                        new_script_deltas,
                        live_server_sim_ms,
                        &world_lock,
                    )
                    .await;

                // 8. once a second, send every player everyone's ping
                let live_server_wall_ms = serverArc.get_live_server_wall_ms();
                if live_server_wall_ms >= next_player_pings_live_server_wall_ms {
                    next_player_pings_live_server_wall_ms = live_server_wall_ms + SERVER_PLAYER_PINGS_INTERVAL_MS as f64;
                    serverArc.sendPlayerPingsToAllUsers().await;
                }

                did_prev_tick_overrun_budget = tick_started_at.elapsed().as_secs_f32() > num_steps as f32 * SERVER_SIM_AND_SEND_SECS;
            }
        }
    }
    Ok(())
}
