mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectLocation, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_brick, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, callRegisteredServerFn, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::physics::SetPhysicsInfo3D;

// Reads share one copy of an object until something could have changed it, so each way of changing it must show in the next read.
#[tokio::test]
async fn reads_see_every_change_made_since_the_last_read() -> anyhow::Result<()> {
    let brick_id = 3;
    let script_id = 2;
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_brick(&mut world, brick_id, root_id, Vec3::new(1.0, 2.0, 3.0), false)?;
    world.addObjectWithId(
        script_id,
        "reads".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_id,
    );
    world.markScriptAsReadyToRun(
        script_id,
        format!(
            r#"'use server'
const brick = game.objectService.getObjectById({brick_id});
export const readAndChange = () => {{
    const xs = [brick.location.position.x];
    brick.location.position.x = 5;
    xs.push(brick.location.position.x);
    game.objectService.moveObjectTo({brick_id}, {{ x: 9, y: 2, z: 3 }});
    xs.push(brick.location.position.x);
    game.objectService.getObjectById({brick_id}).name = "renamed";
    return {{ xs, name: brick.name }};
}};
export const readX = () => brick.location.position.x;
"#
        ),
    );
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;

    let result = callRegisteredServerFn(
        &mut js_runtime,
        &mut world,
        0.0,
        None,
        &script_manager_rc,
        0,
        "readAndChange#root/reads",
        r#"{"json":[]}"#,
    )
    .await?;
    let result = helpers::remote_json_result::<serde_json::Value>(&result)?;
    let xs: Vec<Option<f64>> = result["xs"].as_array().into_iter().flatten().map(|x| x.as_f64()).collect();
    assert_eq!(xs, [Some(1.0), Some(5.0), Some(9.0)]);
    assert_eq!(result["name"], "renamed");

    // changed outside any script, the way physics changes it between ticks
    world.forcePhysicsInfo3D(
        brick_id,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(11.0, 2.0, 3.0)),
            ..Default::default()
        },
        false,
    );
    let result = callRegisteredServerFn(
        &mut js_runtime,
        &mut world,
        0.0,
        None,
        &script_manager_rc,
        0,
        "readX#root/reads",
        r#"{"json":[]}"#,
    )
    .await?;
    assert_eq!(helpers::remote_json_result::<serde_json::Value>(&result)?.as_f64(), Some(11.0));
    Ok(())
}
