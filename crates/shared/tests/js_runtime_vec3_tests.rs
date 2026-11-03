mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::make_world;
use shared::game::jsRuntime::{JsRuntimeInitialization, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime};

const SCRIPT_OBJECT_ID: ObjectId = 2;

fn assert_vec3(reported: &serde_json::Value, expected: Vec3, label: &str) {
    let component = |axis: &str| reported[axis].as_f64().unwrap_or(f64::NAN) as f32;
    let actual = Vec3::new(component("x"), component("y"), component("z"));
    assert!((actual - expected).length() <= 1e-5, "{label}: reported {reported}, expected {expected:?}");
}

// Scripts use vec3 without importing it, because the runtime injects it next to game and tick.
#[tokio::test]
async fn scripts_can_do_vector_math_with_the_injected_vec3() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "vec3_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use server'
export const doVectorMath = () => {
    const a = { x: 1, y: 2, z: 3 };
    const b = { x: 4, y: -5, z: 6 };
    return {
        added: vec3.add(a, b),
        subtracted: vec3.sub(a, b),
        scaled: vec3.scale(a, 2),
        dotted: vec3.dot(a, b),
        crossed: vec3.cross({ x: 1, y: 0, z: 0 }, { x: 0, y: 1, z: 0 }),
        length: vec3.length({ x: 3, y: 4, z: 0 }),
        normalized: vec3.normalize({ x: 0, y: 0, z: 5 }),
        normalizedZero: vec3.normalize({ x: 0, y: 0, z: 0 }),
        clamped: vec3.clampLength({ x: 3, y: 4, z: 0 }, 1),
        unclamped: vec3.clampLength({ x: 3, y: 4, z: 0 }, 10),
        projected: vec3.projectOntoPlane({ x: 1, y: 2, z: 3 }, { x: 0, y: 1, z: 0 }),
    };
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;
    let server_js_fn = script_manager_rc
        .borrow()
        .get_server_js_fn("doVectorMath#root/vec3_script")
        .expect("Expected registered doVectorMath")
        .clone();
    let result_json = callJsFn(&mut js_runtime, &mut world, 0.0, None, &server_js_fn, r#"{"json":[]}"#).await?;
    let results: serde_json::Value = helpers::remote_json_result(&result_json)?;

    assert_vec3(&results["added"], Vec3::new(5.0, -3.0, 9.0), "add");
    assert_vec3(&results["subtracted"], Vec3::new(-3.0, 7.0, -3.0), "sub");
    assert_vec3(&results["scaled"], Vec3::new(2.0, 4.0, 6.0), "scale");
    assert_eq!(results["dotted"].as_f64(), Some(12.0), "dot");
    assert_vec3(&results["crossed"], Vec3::Z, "cross");
    assert_eq!(results["length"].as_f64(), Some(5.0), "length");
    assert_vec3(&results["normalized"], Vec3::Z, "normalize");
    assert_vec3(&results["normalizedZero"], Vec3::ZERO, "normalize of the zero vector");
    assert_vec3(&results["clamped"], Vec3::new(0.6, 0.8, 0.0), "clampLength when longer");
    assert_vec3(&results["unclamped"], Vec3::new(3.0, 4.0, 0.0), "clampLength when shorter");
    assert_vec3(&results["projected"], Vec3::new(1.0, 0.0, 3.0), "projectOntoPlane");
    Ok(())
}
