mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, PhysicsInfo3D, ScriptInfo};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_brick, add_player, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, callJsFn, callRegisteredServerFn, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::world::World;

const CHARACTER_ID: ObjectId = 3;

fn physics_x(x: f32) -> PhysicsInfo3D {
    PhysicsInfo3D::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO)
}

fn x_at(world: &World, est_server_sim_ms: f64) -> Option<f32> {
    world
        .getCharacterLocationAtTime(CHARACTER_ID, est_server_sim_ms)
        .map(|location| location.position.x)
}

#[test]
fn history_keeps_the_last_second_and_goes_with_the_object() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_brick(&mut world, CHARACTER_ID, root_id, Vec3::ZERO, false)?;
    for stamp_ms in [0.0, 500.0, 1000.0, 1500.0] {
        world.recordCharacterPose(CHARACTER_ID, stamp_ms, physics_x((stamp_ms / 100.0) as f32), 0);
    }

    assert_eq!(x_at(&world, 499.0), None);
    assert_eq!(x_at(&world, 500.0), Some(5.0));
    assert!(world.getCharacterLocationAtTime(CHARACTER_ID + 1, 500.0).is_none());

    world.removeObject(CHARACTER_ID)?;
    assert_eq!(x_at(&world, 500.0), None);
    Ok(())
}

#[test]
fn player_metadata_at_time_is_the_newest_one_due_and_goes_with_the_player() -> anyhow::Result<()> {
    let player_id = 4;
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_player(&mut world, player_id, root_id, None)?;
    world.recordPlayerMetadata(player_id, 100.0, Some(r#""a""#.to_string()));
    world.recordPlayerMetadata(player_id, 200.0, None);
    world.recordPlayerMetadata(player_id, 300.0, Some(r#""c""#.to_string()));

    assert_eq!(world.getPlayerMetadataAtTime(player_id, 50.0), None);
    assert_eq!(world.getPlayerMetadataAtTime(player_id, 150.0), Some(r#""a""#));
    assert_eq!(world.getPlayerMetadataAtTime(player_id, 250.0), None);
    assert_eq!(world.getPlayerMetadataAtTime(player_id, 900.0), Some(r#""c""#));

    world.recordPlayerMetadata(player_id, 1250.0, Some(r#""d""#.to_string()));
    assert_eq!(world.getPlayerMetadataAtTime(player_id, 260.0), None);
    assert_eq!(world.getPlayerMetadataAtTime(player_id, 300.0), Some(r#""c""#));

    world.removeObject(player_id)?;
    assert_eq!(world.getPlayerMetadataAtTime(player_id, 300.0), None);
    Ok(())
}

#[tokio::test]
async fn server_functions_see_their_caller_and_where_characters_were() -> anyhow::Result<()> {
    let player_id = 4;
    let script_id = 2;
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_brick(&mut world, CHARACTER_ID, root_id, Vec3::ZERO, false)?;
    add_player(&mut world, player_id, root_id, Some(CHARACTER_ID))?;
    world.addObjectWithId(
        script_id,
        "checks".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_id,
    );
    world.markScriptAsReadyToRun(
        script_id,
        format!(
            r#"'use server'
export const check = () => {{
    return [
        game.playerService.getServerFnCaller(),
        game.playerService.getCharacterLocationAtTime({CHARACTER_ID}, 110),
        game.playerService.getCharacterLocationAtTime({CHARACTER_ID}, 50),
        game.playerService.getPlayerMetadataAtTime({player_id}, 150),
    ];
}};
"#
        ),
    );
    world.recordCharacterPose(CHARACTER_ID, 100.0, physics_x(0.0), 0);
    world.recordCharacterPose(
        CHARACTER_ID,
        120.0,
        PhysicsInfo3D::new(Vec3::new(10.0, 2.0, 0.0), Quat::from_rotation_y(90.0_f32.to_radians()), Vec3::ZERO, Vec3::ZERO),
        0,
    );

    world.recordPlayerMetadata(player_id, 100.0, Some(r#"{"anim":"duck"}"#.to_string()));
    world.recordPlayerMetadata(player_id, 200.0, Some(r#"{"anim":"run"}"#.to_string()));

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;

    let result = callRegisteredServerFn(
        &mut js_runtime,
        &mut world,
        300.0,
        None,
        &script_manager_rc,
        player_id,
        "check#root/checks",
        r#"{"json":[]}"#,
    )
    .await?;
    let result = helpers::remote_json_result::<serde_json::Value>(&result)?;
    assert_eq!(result[0], serde_json::json!(player_id));
    let seen = &result[1];
    let seen_numbers = [&seen["position"]["x"], &seen["position"]["y"], &seen["position"]["z"], &seen["rotation"]["y"]].map(|value| value.as_f64());
    assert_eq!(seen_numbers, [Some(5.0), Some(1.0), Some(0.0), Some(45.0)]);
    assert_eq!(result[2], serde_json::Value::Null);
    assert_eq!(result[3], serde_json::json!({ "anim": "duck" }));

    let check = script_manager_rc
        .borrow()
        .get_server_js_fn("check#root/checks")
        .expect("Expected the registered server function")
        .clone();
    let result = callJsFn(&mut js_runtime, &mut world, 300.0, None, &check, r#"{"json":[]}"#).await?;
    assert_eq!(helpers::remote_json_result::<serde_json::Value>(&result)?[0], serde_json::Value::Null);
    Ok(())
}
