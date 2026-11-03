mod helpers;

use deno_core::JsRuntime;
use game_types::consts::{MAX_CAMERA_ZOOM, MIN_CAMERA_ZOOM};
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, Orientation3D, ScriptInfo};
use glam::Vec3;
use helpers::world_test_helpers::{add_ball, make_world, make_world_controlled_by_player};
use shared::game::jsRuntime::{
    BuiltinMouseCursor, CameraLimits, JsRuntimeCameraOrientation, JsRuntimeInitialization, JsRuntimeMouseRay, MouseCursor, MouseCursorImage, callJsFn,
    initializeJsAndRunInitialScripts, initializeJsRuntime,
};
use shared::game::world::World;

const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!((actual - expected).abs() < 1e-3, "{label}: expected ~{expected}, got {actual}");
}

#[tokio::test]
async fn cast_ray_hits_excludes_and_respects_max_distance() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let ball_object_id = 3;
    add_ball(&mut world, ball_object_id, root_object_id, Vec3::new(10.0, 0.0, 0.0), false)?;

    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "raycast_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use server'
export const cast = (options) => {
    // the direction is deliberately not normalized to prove castRay normalizes it
    const hit = game.objectService.castRay({ x: 0, y: 0, z: 0 }, { x: 5, y: 0, z: 0 }, options);
    if (hit === null) return null;
    return { objectId: hit.object.id, distance: hit.distance, position: hit.position, normal: hit.normal };
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;
    // step once so the physics broad phase knows about the ball
    world.step_server(1, 1.0 / 60.0, 16.0)?;

    let server_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(server_js_fn) = script_manager.get_server_js_fn("cast#root/raycast_script") else {
            return Err(anyhow::anyhow!("Server function not found: cast#root/raycast_script"));
        };
        server_js_fn.clone()
    };
    let mut cast = async |js_args: &str| -> anyhow::Result<serde_json::Value> {
        let result_json = callJsFn(&mut js_runtime, &mut world, 16.0, None, &server_js_fn, &format!(r#"{{"json":{js_args}}}"#)).await?;
        helpers::remote_json_result(&result_json)
    };

    // hit: the ball has radius 1 at x=10, so the ray hits at x=9
    let hit = cast("[{}]").await?;
    assert_eq!(hit["objectId"], serde_json::json!(ball_object_id));
    assert_close(hit["distance"].as_f64().unwrap(), 9.0, "hit distance");
    assert_close(hit["position"]["x"].as_f64().unwrap(), 9.0, "hit position x");
    assert_close(hit["position"]["y"].as_f64().unwrap(), 0.0, "hit position y");
    assert_close(hit["normal"]["x"].as_f64().unwrap(), -1.0, "hit normal x");

    // excluded object: the ray passes through and hits nothing
    let excluded = cast(&format!(r#"[{{"excludeObjectIds": [{ball_object_id}]}}]"#)).await?;
    assert_eq!(excluded, serde_json::Value::Null);

    // max distance shorter than the hit: nothing
    let too_short = cast(r#"[{"maxDistance": 5}]"#).await?;
    assert_eq!(too_short, serde_json::Value::Null);

    Ok(())
}

#[tokio::test]
async fn cast_ray_hit_positions_are_in_short_form() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, 3, root_object_id, Vec3::new(1.1, 0.3, -2.7), false)?;
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "raycast_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use server'
export const cast = () => {
    const hit = game.objectService.castRay({ x: 1.1, y: 10, z: -2.7 }, { x: 0, y: -1, z: 0 });
    return [String(hit.position.x), String(hit.position.z)];
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;
    world.step_server(1, 1.0 / 60.0, 16.0)?;

    let server_js_fn = script_manager_rc
        .borrow()
        .get_server_js_fn("cast#root/raycast_script")
        .expect("Expected registered cast")
        .clone();
    let result_json = callJsFn(&mut js_runtime, &mut world, 16.0, None, &server_js_fn, r#"{"json":[]}"#).await?;
    assert_eq!(
        helpers::remote_json_result::<serde_json::Value>(&result_json)?,
        serde_json::json!(["1.1", "-2.7"])
    );
    Ok(())
}

#[tokio::test]
async fn camera_direction_is_readable_in_client_scripts() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "camera_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const readCamera = () => {
    return { direction: game.clientInputService.getCameraDirection() };
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    // theta: quarter turn (facing +X). phi: PI/4 downward in Rust convention, which JS reports as -45 (positive-up).
    js_runtime
        .op_state()
        .borrow_mut()
        .borrow_mut::<JsRuntimeCameraOrientation>()
        .set(Orientation3D {
            theta: std::f32::consts::FRAC_PI_2,
            phi: std::f32::consts::FRAC_PI_4,
        });
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let client_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(client_js_fn) = script_manager.get_client_js_fn("readCamera#root/camera_script") else {
            return Err(anyhow::anyhow!("Client function not found: readCamera#root/camera_script"));
        };
        client_js_fn.clone()
    };
    let camera_json = callJsFn(&mut js_runtime, &mut world, 16.0, Some(16.0), &client_js_fn, r#"{"json":[]}"#).await?;
    let camera: serde_json::Value = helpers::remote_json_result(&camera_json)?;

    let expected_horizontal = (std::f64::consts::FRAC_PI_4).cos(); // ~0.7071
    assert_close(camera["direction"]["x"].as_f64().unwrap(), expected_horizontal, "direction x");
    assert_close(camera["direction"]["y"].as_f64().unwrap(), -expected_horizontal, "direction y (looking down)");
    assert_close(camera["direction"]["z"].as_f64().unwrap(), 0.0, "direction z");

    Ok(())
}

#[tokio::test]
async fn camera_ops_throw_in_server_scripts() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "server_camera_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use server'
let errorMessage = "did-not-throw";
try {
    game.clientInputService.getCameraDirection();
} catch (error) {
    errorMessage = `${error.message}`;
}
let mouseRayErrorMessage = "did-not-throw";
try {
    game.clientInputService.getMouseRay();
} catch (error) {
    mouseRayErrorMessage = `${error.message}`;
}
export const getErrorMessage = () => errorMessage;
export const getMouseRayErrorMessage = () => mouseRayErrorMessage;
const errorMessageOf = (fn) => {
    try {
        fn();
        return "did-not-throw";
    } catch (error) {
        return `${error.message}`;
    }
};
export const getCameraLimitsErrorMessages = () => [
    errorMessageOf(() => game.clientInputService.lockCameraDirection({ x: 0, y: 0, z: 1 })),
    errorMessageOf(() => game.clientInputService.unlockCameraDirection()),
    errorMessageOf(() => game.clientInputService.setZoomRange(1, 2)),
    errorMessageOf(() => game.clientInputService.getZoom()),
    errorMessageOf(() => game.clientInputService.getDesiredZoom()),
    errorMessageOf(() => game.clientInputService.setFOV(90)),
    errorMessageOf(() => game.clientInputService.setCameraOffset({ x: 1.5, y: 0, z: 0 })),
    errorMessageOf(() => game.clientInputService.lockMouse()),
    errorMessageOf(() => game.clientInputService.unlockMouse()),
    errorMessageOf(() => game.clientInputService.setMouseCursor("pointer")),
];
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, None, &script_manager_rc).await?;

    let server_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(server_js_fn) = script_manager.get_server_js_fn("getErrorMessage#root/server_camera_script") else {
            return Err(anyhow::anyhow!("Server function not found: getErrorMessage#root/server_camera_script"));
        };
        server_js_fn.clone()
    };
    let error_message_json = callJsFn(&mut js_runtime, &mut world, 16.0, None, &server_js_fn, r#"{"json":[]}"#).await?;
    let error_message: String = helpers::remote_json_result(&error_message_json)?;
    assert!(
        error_message.contains("only available on the client"),
        "expected a client-only error, got: {error_message}"
    );

    let mouse_ray_server_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(mouse_ray_server_js_fn) = script_manager.get_server_js_fn("getMouseRayErrorMessage#root/server_camera_script") else {
            return Err(anyhow::anyhow!("Server function not found: getMouseRayErrorMessage#root/server_camera_script"));
        };
        mouse_ray_server_js_fn.clone()
    };
    let mouse_ray_error_message_json = callJsFn(&mut js_runtime, &mut world, 16.0, None, &mouse_ray_server_js_fn, r#"{"json":[]}"#).await?;
    let mouse_ray_error_message: String = helpers::remote_json_result(&mouse_ray_error_message_json)?;
    assert!(
        mouse_ray_error_message.contains("only available on the client"),
        "expected a client-only error, got: {mouse_ray_error_message}"
    );

    let camera_limits_server_js_fn = script_manager_rc
        .borrow()
        .get_server_js_fn("getCameraLimitsErrorMessages#root/server_camera_script")
        .expect("Expected registered getCameraLimitsErrorMessages")
        .clone();
    let camera_limits_error_messages_json = callJsFn(&mut js_runtime, &mut world, 16.0, None, &camera_limits_server_js_fn, r#"{"json":[]}"#).await?;
    let camera_limits_error_messages: Vec<String> = helpers::remote_json_result(&camera_limits_error_messages_json)?;
    for camera_limits_error_message in camera_limits_error_messages {
        assert!(
            camera_limits_error_message.contains("only available on the client"),
            "expected a client-only error, got: {camera_limits_error_message}"
        );
    }

    Ok(())
}

// Client scripts lock the camera and limit its zoom by changing the script manager's settings, which the play loop and
// renderer read. Bad values throw and leave the settings as they were.
#[tokio::test]
async fn camera_lock_and_zoom_range_are_set_by_client_scripts() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "camera_limits_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
const errorMessageOf = (fn) => {
    try {
        fn();
        return null;
    } catch (error) {
        return `${error.message}`;
    }
};
export const lock = (direction) => errorMessageOf(() => game.clientInputService.lockCameraDirection(direction));
export const unlock = () => errorMessageOf(() => game.clientInputService.unlockCameraDirection());
export const setZoomRange = (minZoom, maxZoom) => errorMessageOf(() => game.clientInputService.setZoomRange(minZoom, maxZoom));
export const lockMouse = () => errorMessageOf(() => game.clientInputService.lockMouse());
export const unlockMouse = () => errorMessageOf(() => game.clientInputService.unlockMouse());
export const setCameraOffset = (offset) => errorMessageOf(() => game.clientInputService.setCameraOffset(offset));
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let mut call = async |fn_name: &str, js_args: &str| -> anyhow::Result<Option<String>> {
        let client_js_fn = script_manager_rc
            .borrow()
            .get_client_js_fn(&format!("{fn_name}#root/camera_limits_script"))
            .expect("Expected registered client function")
            .clone();
        let result_json = callJsFn(
            &mut js_runtime,
            &mut world,
            16.0,
            Some(16.0),
            &client_js_fn,
            &format!(r#"{{"json":{js_args}}}"#),
        )
        .await?;
        helpers::remote_json_result(&result_json)
    };
    let get_camera_limits = || script_manager_rc.borrow().camera_limits();

    // nothing is locked and the zoom range is the engine's until a script changes them
    let CameraLimits {
        locked_orientation,
        min_zoom,
        max_zoom,
        fov_degrees,
        is_mouse_locked,
        offset,
    } = get_camera_limits();
    assert!(locked_orientation.is_none());
    assert_eq!((min_zoom, max_zoom), (MIN_CAMERA_ZOOM, MAX_CAMERA_ZOOM));
    assert!(fov_degrees.is_none());
    assert!(!is_mouse_locked);
    assert_eq!(offset, Vec3::ZERO);

    // straight down tilts slightly, like turning does, so the camera can still face it
    assert_eq!(call("lock", r#"[{"x": 0, "y": -1, "z": 0}]"#).await?, None);
    let Some(locked_orientation) = get_camera_limits().locked_orientation else {
        return Err(anyhow::anyhow!("Expected the camera to be locked"));
    };
    assert!(locked_orientation.phi < std::f32::consts::FRAC_PI_2, "phi must stay below straight down");
    assert!(locked_orientation.to_unit_direction().y < -0.99, "the camera must still face down");

    // the direction does not need to be a unit vector
    assert_eq!(call("lock", r#"[{"x": 3, "y": 0, "z": 0}]"#).await?, None);
    let Some(Orientation3D { theta, phi }) = get_camera_limits().locked_orientation else {
        return Err(anyhow::anyhow!("Expected the camera to be locked"));
    };
    assert_close(theta as f64, std::f64::consts::FRAC_PI_2, "theta of +X");
    assert_close(phi as f64, 0.0, "phi of +X");

    // a zero direction throws and keeps the last lock
    let zero_direction_error_message = call("lock", r#"[{"x": 0, "y": 0, "z": 0}]"#).await?.unwrap_or_default();
    assert!(zero_direction_error_message.contains("nonzero length"), "got: {zero_direction_error_message}");
    let Some(Orientation3D { theta, .. }) = get_camera_limits().locked_orientation else {
        return Err(anyhow::anyhow!("Expected the camera to stay locked"));
    };
    assert_close(theta as f64, std::f64::consts::FRAC_PI_2, "theta after a zero direction");

    assert_eq!(call("unlock", "[]").await?, None);
    assert!(get_camera_limits().locked_orientation.is_none());

    assert_eq!(call("setZoomRange", "[5, 20]").await?, None);
    let CameraLimits { min_zoom, max_zoom, .. } = get_camera_limits();
    assert_eq!((min_zoom, max_zoom), (5.0, 20.0));

    // min above max, or min closer than the engine allows, throws and keeps the last range
    for bad_zoom_range in ["[20, 5]", "[0, 5]"] {
        let bad_zoom_range_error_message = call("setZoomRange", bad_zoom_range).await?.unwrap_or_default();
        assert!(bad_zoom_range_error_message.contains("setZoomRange"), "got: {bad_zoom_range_error_message}");
        let CameraLimits { min_zoom, max_zoom, .. } = get_camera_limits();
        assert_eq!((min_zoom, max_zoom), (5.0, 20.0), "after {bad_zoom_range}");
    }

    // the mouse lock leaves the camera's direction and zoom alone
    assert_eq!(call("lockMouse", "[]").await?, None);
    let CameraLimits {
        locked_orientation,
        min_zoom,
        max_zoom,
        is_mouse_locked,
        ..
    } = get_camera_limits();
    assert!(is_mouse_locked);
    assert!(locked_orientation.is_none());
    assert_eq!((min_zoom, max_zoom), (5.0, 20.0));
    assert_eq!(call("unlockMouse", "[]").await?, None);
    assert!(!get_camera_limits().is_mouse_locked);

    // the camera offset leaves the camera's zoom alone, and a bad offset throws and keeps the last one
    assert_eq!(call("setCameraOffset", r#"[{"x": 1.5, "y": 0.5, "z": -2}]"#).await?, None);
    let CameraLimits {
        min_zoom, max_zoom, offset, ..
    } = get_camera_limits();
    assert_eq!(offset, Vec3::new(1.5, 0.5, -2.0));
    assert_eq!((min_zoom, max_zoom), (5.0, 20.0));
    assert!(
        call("setCameraOffset", r#"[{"x": "far", "y": 0, "z": 0}]"#).await?.is_some(),
        "a bad offset should throw"
    );
    assert_eq!(get_camera_limits().offset, Vec3::new(1.5, 0.5, -2.0));

    Ok(())
}

// Client scripts change the mouse cursor through the script manager, which the desktop client reads. A cursor the window
// can't show throws and leaves the last one.
#[tokio::test]
async fn mouse_cursor_is_set_by_client_scripts() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "mouse_cursor_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const setMouseCursor = (mouseCursor) => {
    try {
        game.clientInputService.setMouseCursor(mouseCursor);
        return null;
    } catch (error) {
        return `${error.message}`;
    }
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let set_mouse_cursor_js_fn = script_manager_rc
        .borrow()
        .get_client_js_fn("setMouseCursor#root/mouse_cursor_script")
        .expect("Expected registered client function")
        .clone();
    let mut set_mouse_cursor = async |mouse_cursor_json: &str| -> anyhow::Result<Option<String>> {
        let result_json = callJsFn(
            &mut js_runtime,
            &mut world,
            16.0,
            Some(16.0),
            &set_mouse_cursor_js_fn,
            &format!(r#"{{"json":[{mouse_cursor_json}]}}"#),
        )
        .await?;
        helpers::remote_json_result(&result_json)
    };
    let get_mouse_cursor = || script_manager_rc.borrow().mouse_cursor();

    assert_eq!(get_mouse_cursor(), MouseCursor::Builtin(BuiltinMouseCursor::Default));
    assert_eq!(set_mouse_cursor(r#""crosshair""#).await?, None);
    assert_eq!(get_mouse_cursor(), MouseCursor::Builtin(BuiltinMouseCursor::Crosshair));

    let mouse_cursor_image = MouseCursorImage {
        textureDataId: 7,
        width: 32.0,
        height: 24.0,
        hotspotX: 0.25,
        hotspotY: 1.0,
    };
    assert_eq!(
        set_mouse_cursor(r#"{"textureDataId": 7, "width": 32, "height": 24, "hotspotX": 0.25, "hotspotY": 1}"#).await?,
        None
    );
    assert_eq!(get_mouse_cursor(), MouseCursor::Image(mouse_cursor_image));

    // too small, too big, a point that clicks off the picture, and a name that isn't built in
    for bad_mouse_cursor_json in [
        r#"{"textureDataId": 7, "width": 0, "height": 24, "hotspotX": 0, "hotspotY": 0}"#,
        r#"{"textureDataId": 7, "width": 32, "height": 129, "hotspotX": 0, "hotspotY": 0}"#,
        r#"{"textureDataId": 7, "width": 32, "height": 24, "hotspotX": 1.5, "hotspotY": 0}"#,
        r#""hand""#,
    ] {
        let error_message = set_mouse_cursor(bad_mouse_cursor_json).await?.unwrap_or_default();
        assert!(error_message.contains("setMouseCursor"), "{bad_mouse_cursor_json} gave: {error_message}");
        assert_eq!(get_mouse_cursor(), MouseCursor::Image(mouse_cursor_image), "after {bad_mouse_cursor_json}");
    }

    Ok(())
}

// setFOV replaces the default fov with the script's own until a script passes null. Bad values throw and keep the last fov.
#[tokio::test]
async fn fov_is_set_by_client_scripts() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "fov_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const setFOV = (degrees) => {
    try {
        game.clientInputService.setFOV(degrees);
        return null;
    } catch (error) {
        return `${error.message}`;
    }
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let call = async |js_runtime: &mut JsRuntime, world: &mut World, fn_name: &str, js_args: &str| -> anyhow::Result<serde_json::Value> {
        let client_js_fn = script_manager_rc
            .borrow()
            .get_client_js_fn(&format!("{fn_name}#root/fov_script"))
            .expect("Expected registered client function")
            .clone();
        let result_json = callJsFn(js_runtime, world, 16.0, Some(16.0), &client_js_fn, &format!(r#"{{"json":{js_args}}}"#)).await?;
        helpers::remote_json_result(&result_json)
    };
    let get_fov_degrees = || script_manager_rc.borrow().camera_limits().fov_degrees;

    // the camera uses the default fov until a script sets one
    assert!(get_fov_degrees().is_none());
    assert_eq!(call(&mut js_runtime, &mut world, "setFOV", "[105]").await?, serde_json::Value::Null);
    assert_eq!(get_fov_degrees(), Some(105.0));

    for bad_fov in ["[0]", "[180]", "[-30]"] {
        let bad_fov_error_message = call(&mut js_runtime, &mut world, "setFOV", bad_fov).await?;
        assert!(
            bad_fov_error_message.as_str().unwrap_or_default().contains("setFOV"),
            "got: {bad_fov_error_message}"
        );
        assert_eq!(get_fov_degrees(), Some(105.0), "after {bad_fov}");
    }

    // null goes back to the default fov
    assert_eq!(call(&mut js_runtime, &mut world, "setFOV", "[null]").await?, serde_json::Value::Null);
    assert!(get_fov_degrees().is_none());

    Ok(())
}

// The render loop pushes whatever ray it built for the frame into the runtime, so the script has to read back exactly that,
// and has to see null while the cursor is off the game view.
#[tokio::test]
async fn mouse_ray_is_readable_in_client_scripts() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "mouse_ray_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const readMouseRay = () => {
    return game.clientInputService.getMouseRay();
};
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let client_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(client_js_fn) = script_manager.get_client_js_fn("readMouseRay#root/mouse_ray_script") else {
            return Err(anyhow::anyhow!("Client function not found: readMouseRay#root/mouse_ray_script"));
        };
        client_js_fn.clone()
    };
    let read_mouse_ray = async |js_runtime: &mut JsRuntime, world: &mut World| -> anyhow::Result<serde_json::Value> {
        let result_json = callJsFn(js_runtime, world, 16.0, Some(16.0), &client_js_fn, r#"{"json":[]}"#).await?;
        helpers::remote_json_result(&result_json)
    };

    // no cursor over the game view yet, so there is no ray to read
    let no_ray = read_mouse_ray(&mut js_runtime, &mut world).await?;
    assert_eq!(no_ray, serde_json::Value::Null);

    js_runtime
        .op_state()
        .borrow_mut()
        .borrow_mut::<JsRuntimeMouseRay>()
        .set(Some((Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, -1.0))));
    let ray = read_mouse_ray(&mut js_runtime, &mut world).await?;
    assert_close(ray["origin"]["x"].as_f64().unwrap(), 1.0, "origin x");
    assert_close(ray["origin"]["y"].as_f64().unwrap(), 2.0, "origin y");
    assert_close(ray["origin"]["z"].as_f64().unwrap(), 3.0, "origin z");
    assert_close(ray["direction"]["x"].as_f64().unwrap(), 0.0, "direction x");
    assert_close(ray["direction"]["y"].as_f64().unwrap(), 0.0, "direction y");
    assert_close(ray["direction"]["z"].as_f64().unwrap(), -1.0, "direction z");

    // the cursor leaving the game view puts the script back to null
    js_runtime.op_state().borrow_mut().borrow_mut::<JsRuntimeMouseRay>().set(None);
    let ray_after_cursor_left = read_mouse_ray(&mut js_runtime, &mut world).await?;
    assert_eq!(ray_after_cursor_left, serde_json::Value::Null);

    Ok(())
}
