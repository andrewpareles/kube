mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectLocation, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_brick, add_script, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::physics::SetPhysicsInfo3D;
use shared::game::world::WorldControlMode;

// Exercise the public API against a real world, including cached nested references and inputs passed back to Rust.
#[tokio::test]
async fn cached_position_and_velocity_follow_physics_and_client_updates() -> anyhow::Result<()> {
    for is_client in [false, true] {
        let mut world = make_world()?;
        let root_id = world.getRootObjectId();
        add_brick(&mut world, 2, root_id, Vec3::Y * 30.0, true)?;
        if is_client {
            world.worldControlMode = WorldControlMode::Player(99);
            world.addObjectWithId(
                3,
                "reader".to_string(),
                ObjectBeingAdded::Script(ScriptInfo {
                    objectLocation: ObjectLocation::Client,
                }),
                root_id,
            );
        } else {
            add_script(&mut world, 3, root_id)?;
        }
        world.markScriptAsReadyToRun(
            3,
            format!(
                r#"'use {}'
const body = game.objectService.getObjectById(2);
const position = body.location.position;
const velocity = body.movement.velocity;
export const readLiveBody = () => ({{ y: position.y, vy: velocity.y }});
"#,
                if is_client { "client" } else { "server" }
            ),
        );
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        let read_body = {
            let script_manager = script_manager_rc.borrow();
            let function = if is_client {
                script_manager.get_client_js_fn("readLiveBody#root/reader")
            } else {
                script_manager.get_server_js_fn("readLiveBody#root/script-3")
            };
            function.expect("Expected registered reader").clone()
        };
        if is_client {
            world.forcePhysicsInfo3D(
                2,
                SetPhysicsInfo3D {
                    position: Some(Vec3::Y * 20.0),
                    velocity: Some(Vec3::NEG_Y * 5.0),
                    ..Default::default()
                },
                false,
            );
        } else {
            world.step_server(30, 1.0 / 60.0, 500.0)?;
        }
        let json = callJsFn(&mut js_runtime, &mut world, 500.0, Some(500.0), &read_body, r#"{"json":[]}"#).await?;
        let result: serde_json::Value = helpers::remote_json_result(&json)?;
        let physics = world.getObjectPhysicsInfo3D(2).expect("Expected physics");
        assert!(physics.location.position.y < 30.0);
        assert!((result["y"].as_f64().expect("Expected Y") as f32 - physics.location.position.y).abs() < 1e-5);
        assert!((result["vy"].as_f64().expect("Expected velocity") as f32 - physics.movement.velocity.y).abs() < 1e-5);
    }
    Ok(())
}

#[tokio::test]
async fn returned_getter_errors_leave_the_runtime_usable() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_script(&mut world, 2, root_id)?;
    world.markScriptAsReadyToRun(
        2,
        r#"'use server'
let callCount = 0;
export const result = () => {
    if (++callCount === 3) return;
    return { get value() {
        Deno.core.ops.__getRustObject__(Deno.core.ops.__getRustRootObjectId__());
        if (callCount === 1) throw new Error("result getter failed");
        return 42;
    } };
};
"#
        .to_string(),
    );
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let function = script_manager_rc
        .borrow()
        .get_server_js_fn("result#root/script-2")
        .expect("Expected registered function")
        .clone();
    let error = callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &function, r#"{"json":[]}"#)
        .await
        .expect_err("Expected the result getter to fail");
    assert!(error.to_string().contains("result getter failed"), "{error}");
    let result = callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &function, r#"{"json":[]}"#).await?;
    assert_eq!(helpers::remote_json_result::<serde_json::Value>(&result)?, serde_json::json!({ "value": 42 }));
    let result = callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &function, r#"{"json":[]}"#).await?;
    assert!(result.contains("undefined"), "Expected an encoded undefined return: {result}");
    Ok(())
}

#[tokio::test]
async fn scripts_read_f32_positions_in_short_form() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_brick(&mut world, 2, root_id, Vec3::new(1.1, 0.3, -2.7), false)?;
    add_script(&mut world, 3, root_id)?;
    world.markScriptAsReadyToRun(
        3,
        r#"'use server'
const position = game.objectService.getObjectById(2).location.position;
export const readPosition = () => [String(position.x), String(position.y), String(position.z), position.x === 1.1];
"#
        .to_string(),
    );
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let read_position = script_manager_rc
        .borrow()
        .get_server_js_fn("readPosition#root/script-3")
        .expect("Expected registered reader")
        .clone();
    let result = callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &read_position, r#"{"json":[]}"#).await?;
    assert_eq!(
        helpers::remote_json_result::<serde_json::Value>(&result)?,
        serde_json::json!(["1.1", "0.3", "-2.7", true])
    );
    Ok(())
}
