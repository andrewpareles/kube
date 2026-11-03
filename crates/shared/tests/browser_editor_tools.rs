use game_types::object_types::{ObjectBeingAdded, ObjectIdPointer, ObjectInfo, ObjectLocation, PivotInfo, ScriptInfo};
use glam::Vec3;
use serde_json::json;
use shared::{
    api::protocol_for_chat::{MAX_LISTED_SCRIPT_WARNINGS, ToolCall, apply_editor_tool_call, script_warnings_of_script_files_text},
    game::{
        data_fetcher::DataFetcherArc,
        world::{World, WorldControlMode},
    },
};
use std::sync::Arc;
use tokio::sync::RwLock;

// Parsed, because the saved string lists map entries in a different order each time it is made.
fn stored_world_json(world: &World) -> serde_json::Value {
    serde_json::from_str(&world.toStoredWorldString().unwrap()).unwrap()
}

#[tokio::test]
async fn restored_tools_edit_a_new_draft_and_survive_serialization() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let call: ToolCall = serde_json::from_value(json!({
        "name": "create_brick",
        "arguments": {"parent_id":1,"name":"Platform","center":{"x":0,"y":0,"z":0},"size":{"x":10,"y":1,"z":10},"rotation_degrees":null,"color":null,"opacity":null}
    })).unwrap();
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, call).await;
    assert!(!result.is_error, "{}", result.text);
    let stored = world_arc.read().await.toStoredWorldString().unwrap();
    let restored = World::fromStoredWorldString(&stored, WorldControlMode::Editor, None).unwrap();
    assert_eq!(restored.getObjectIdsByName("Platform").len(), 1);
    World::fromStoredWorldString(&stored, WorldControlMode::Server, None).unwrap();
}
#[tokio::test]
async fn invalid_tool_arguments_do_not_mutate_the_world() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let call = ToolCall {
        name: "create_brick".into(),
        arguments: json!({"parent_id":1}),
    };
    let before = stored_world_json(&*world_arc.read().await);
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, call).await;
    assert!(result.is_error);
    assert_eq!(before, stored_world_json(&*world_arc.read().await));
}

#[tokio::test]
async fn parallel_tools_edit_the_same_world_without_game_identity_parameters() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let tool_calls: Vec<_> = ["LeftPlatform", "RightPlatform"]
        .into_iter()
        .map(|name| {
            let args = json!({"parent_id":1,"name":name,"center":{"x":0,"y":0,"z":0},"size":{"x":10,"y":1,"z":10},"rotation_degrees":null,"color":null,"opacity":null});
            json!({"call_id":name,"tool_name":"create_brick","args":serde_json::to_string(&args).unwrap()})
        })
        .collect();
    let call = ToolCall {
        name: "run_parallel".into(),
        arguments: json!({"tool_calls":tool_calls,"timeout_ms":null}),
    };
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, call).await;
    assert!(!result.is_error, "{}", result.text);
    let world_lock = world_arc.read().await;
    assert_eq!(world_lock.getObjectIdsByName("LeftPlatform").len(), 1);
    assert_eq!(world_lock.getObjectIdsByName("RightPlatform").len(), 1);
    drop(world_lock);
}

#[tokio::test]
async fn inspect_prints_f32_positions_in_short_form() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let create_call = ToolCall {
        name: "create_brick".into(),
        arguments: json!({"parent_id":1,"name":"ShortFloats","center":{"x":1.1,"y":0.3,"z":-2.7},"size":{"x":1,"y":1,"z":1},"rotation_degrees":null,"color":null,"opacity":null}),
    };
    let created = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, create_call).await;
    assert!(!created.is_error, "{}", created.text);
    let object_id = world_arc.read().await.getObjectIdsByName("ShortFloats")[0];
    let position = world_arc.read().await.getObjectPhysicsInfo3D(object_id).unwrap().location.position;
    assert_eq!(position, Vec3::new(1.1, 0.3, -2.7));

    let inspect_call = ToolCall {
        name: "inspect".into(),
        arguments: json!({"object_id":object_id,"mode":"physics"}),
    };
    let inspected = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, inspect_call).await;
    assert!(!inspected.is_error, "{}", inspected.text);
    assert!(!inspected.text.contains("1.100000023841858"), "{}", inspected.text);
    let inspected_json: serde_json::Value = serde_json::from_str(&inspected.text).unwrap();
    assert_eq!(inspected_json["physics"]["location"]["position"], json!([1.1, 0.3, -2.7]));
}

// Without its own center point, rotate_recursively turns an object around its Pivot, the same way scripts and animators do.
#[tokio::test]
async fn rotate_recursively_turns_an_object_around_its_pivot() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let create_door = ToolCall {
        name: "create_brick".into(),
        arguments: json!({"parent_id":1,"name":"Door","center":{"x":1,"y":0,"z":0},"size":{"x":2,"y":2,"z":2},"rotation_degrees":null,"color":null,"opacity":null}),
    };
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, create_door).await;
    assert!(!result.is_error, "{}", result.text);
    let door_object_id = {
        let mut world_lock = world_arc.write().await;
        let door_object_id = world_lock.getObjectIdsByName("Door")[0];
        // the hinge is the door's west edge, at the origin
        let pivot_info = PivotInfo {
            physicalObjectId: Some(ObjectIdPointer(door_object_id)),
            offsetFromObject: Vec3::new(-1.0, 0.0, 0.0),
        };
        world_lock.addObject("Hinge".into(), ObjectBeingAdded::Pivot(pivot_info), 1);
        door_object_id
    };

    let rotate_door = ToolCall {
        name: "rotate_recursively".into(),
        arguments: json!({"object_id":door_object_id,"rotation_axis":{"x":0,"y":1,"z":0},"rotation_magnitude":std::f32::consts::FRAC_PI_2,"center_point":null}),
    };
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, rotate_door).await;
    assert!(!result.is_error, "{}", result.text);
    let door_position = world_arc.read().await.getObjectPhysicsInfo3D(door_object_id).unwrap().location.position;
    assert!((door_position - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4, "door center {door_position:?}");
}

// Without its own center point, scale_recursively grows an object around its Pivot and moves the Pivot's point with it.
#[tokio::test]
async fn scale_recursively_scales_an_object_around_its_pivot() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let create_door = ToolCall {
        name: "create_brick".into(),
        arguments: json!({"parent_id":1,"name":"Door","center":{"x":1,"y":0,"z":0},"size":{"x":2,"y":2,"z":2},"rotation_degrees":null,"color":null,"opacity":null}),
    };
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, create_door).await;
    assert!(!result.is_error, "{}", result.text);
    let (door_object_id, pivot_object_id) = {
        let mut world_lock = world_arc.write().await;
        let door_object_id = world_lock.getObjectIdsByName("Door")[0];
        // the hinge is the door's west edge, at the origin
        let pivot_info = PivotInfo {
            physicalObjectId: Some(ObjectIdPointer(door_object_id)),
            offsetFromObject: Vec3::new(-1.0, 0.0, 0.0),
        };
        let pivot_object_id = world_lock.addObject("Hinge".into(), ObjectBeingAdded::Pivot(pivot_info), 1);
        (door_object_id, pivot_object_id)
    };

    let scale_door = ToolCall {
        name: "scale_recursively".into(),
        arguments: json!({"object_id":door_object_id,"scale_multiplier":2,"center_point":null}),
    };
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, scale_door).await;
    assert!(!result.is_error, "{}", result.text);
    let world_lock = world_arc.read().await;
    let door_position = world_lock.getObjectPhysicsInfo3D(door_object_id).unwrap().location.position;
    assert!((door_position - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4, "door center {door_position:?}");
    let ObjectInfo::Pivot(pivot) = &world_lock.getObject(pivot_object_id).unwrap().info else {
        panic!("Expected pivot object");
    };
    assert!(
        (pivot.offsetFromObject - Vec3::new(-2.0, 0.0, 0.0)).length() < 1e-4,
        "pivot offset {:?}",
        pivot.offsetFromObject
    );
}

#[tokio::test]
async fn set_fallback_camera_points_the_fallback_camera_at_look_at() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let call = ToolCall {
        name: "set_fallback_camera".into(),
        arguments: json!({"camera":{"position":{"x":3,"y":4,"z":5},"look_at":{"x":-2,"y":1,"z":9}}}),
    };
    let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, call).await;
    assert!(!result.is_error, "{}", result.text);
    let camera = world_arc.read().await.fallback_camera_location;
    assert_eq!(camera.position, Vec3::new(3.0, 4.0, 5.0));
    let expected_direction = Vec3::new(-5.0, -3.0, 4.0).normalize();
    assert!(camera.orientation.to_unit_direction().distance(expected_direction) < 1e-5);

    let straight_down = ToolCall {
        name: "set_fallback_camera".into(),
        arguments: json!({"camera":{"position":{"x":0,"y":10,"z":0},"look_at":{"x":0,"y":0,"z":0}}}),
    };
    assert!(apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, straight_down).await.is_error);
    assert_eq!(world_arc.read().await.fallback_camera_location.position, Vec3::new(3.0, 4.0, 5.0));
}

const SCRIPT_BRICK_HELPERS: &str = r#"
const v = (x, y, z) => ({ x, y, z });
const brickInfo = (size) => ({
    type: "brick",
    material: null,
    opacity: 1,
    color: { r: 1, g: 1, b: 1 },
    isAlwaysOnTop: false,
    size,
    isWalkthrough: false,
    isPushable: false,
    density: 1,
    location: { position: v(0, 0, 0), rotation: v(0, 0, 0) },
    movement: { velocity: v(0, 0, 0), angularVelocity: v(0, 0, 0) },
});
"#;

async fn run_temporary_script_tool(world_arc: &Arc<RwLock<World>>, script: &str) -> shared::api::protocol_for_chat::ToolResultFinal {
    let call = ToolCall {
        name: "run_temporary_script".into(),
        arguments: json!({ "script": format!("{SCRIPT_BRICK_HELPERS}{script}") }),
    };
    apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, call).await
}

// A script once gave 16 bricks a width of -1. The editor kept them and saved them, and loading refuses that size, so the game's
// editor crashed every time it opened. Neither a new shape nor a resize may take a size that loading refuses.
#[tokio::test]
async fn shapes_never_take_a_size_that_loading_refuses() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));

    let created = run_temporary_script_tool(
        &world_arc,
        r#"game.objectService.createObject("NegativeBrick", brickInfo(v(-1, 2, 2.6)), game.world.id);"#,
    )
    .await;
    assert!(created.is_error, "{}", created.text);
    assert!(world_arc.read().await.getObjectIdsByName("NegativeBrick").is_empty());

    let resized = run_temporary_script_tool(
        &world_arc,
        r#"const brick = game.objectService.createObject("ResizedBrick", brickInfo(v(1, 2, 2.6)), game.world.id);
brick.size = v(-1, 2, 2.6);"#,
    )
    .await;
    assert!(resized.is_error, "{}", resized.text);
    assert_eq!(world_arc.read().await.getObjectIdsByName("ResizedBrick").len(), 1);

    let create_object_call = ToolCall {
        name: "create_object".into(),
        arguments: json!({"object": json!({"name": "ToolBrick", "type": "cylinder", "material": null, "opacity": 1, "color": {"r": 1, "g": 1, "b": 1}, "isAlwaysOnTop": false, "radius": -0.5, "length": 1, "isWalkthrough": false, "isPushable": false, "density": 1, "location": {"position": {"x": 0, "y": 0, "z": 0}, "rotation": {"x": 0, "y": 0, "z": 0}}, "movement": {"velocity": {"x": 0, "y": 0, "z": 0}, "angularVelocity": {"x": 0, "y": 0, "z": 0}}}).to_string(), "parent_id": null}),
    };
    let tool_created = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, create_object_call).await;
    assert!(tool_created.is_error, "{}", tool_created.text);

    let stored = world_arc.read().await.toStoredWorldString().unwrap();
    World::fromStoredWorldString(&stored, WorldControlMode::Editor, None).unwrap();
}

// A script that copied a character rig once made about 8,000 objects, and listing every one filled the AI's whole conversation.
#[tokio::test]
async fn temporary_scripts_count_what_they_create_by_type() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let result = run_temporary_script_tool(
        &world_arc,
        r#"const pile = game.objectService.createObject("Pile", { type: "group" }, game.world.id);
for (let index = 0; index < 150; index++) game.objectService.createObject(`Made${index}`, brickInfo(v(1, 1, 1)), pile.id);"#,
    )
    .await;
    assert!(!result.is_error, "{}", result.text);
    let result_json: serde_json::Value = serde_json::from_str(&result.text).unwrap();
    assert_eq!(result_json["created"], json!({ "brick": 150, "group": 1 }));
    assert_eq!(result_json["deleted"], json!({}));
    assert!(result.text.len() < 1_000, "{}", result.text);
}

#[tokio::test]
async fn long_tool_results_keep_their_start_and_end_and_say_what_was_cut() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let result = run_temporary_script_tool(
        &world_arc,
        r#"console.log("first log");
for (let index = 0; index < 60; index++) console.log("x".repeat(5000));
console.log("last log");"#,
    )
    .await;
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.chars().count() < 101_000, "{} characters", result.text.chars().count());
    assert!(result.text.contains("characters were cut from the middle of this result"));
    assert!(result.text.contains("first log"));
    assert!(result.text.contains("last log"));
}

async fn run_tool(world_arc: &Arc<RwLock<World>>, name: &str, arguments: serde_json::Value) -> shared::api::protocol_for_chat::ToolResultFinal {
    apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, ToolCall { name: name.into(), arguments }).await
}

fn count_ls_nodes(nodes: &serde_json::Value) -> usize {
    nodes.as_array().unwrap().iter().map(|node| 1 + count_ls_nodes(&node["children"])).sum()
}

// `limit` once applied only to the top level, so one ls listed thousands of animation keyframes under 35 objects.
#[tokio::test]
async fn ls_limit_counts_objects_at_every_level_and_inspect_lists_the_first_100() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let built = run_temporary_script_tool(
        &world_arc,
        r#"const tower = game.objectService.createObject("Tower", { type: "group" }, game.world.id);
for (let floor = 0; floor < 3; floor++) {
    const level = game.objectService.createObject(`Floor${floor}`, { type: "group" }, tower.id);
    for (let index = 0; index < 50; index++) game.objectService.createObject(`Block${index}`, brickInfo(v(1, 1, 1)), level.id);
}
const pile = game.objectService.createObject("Pile", { type: "group" }, game.world.id);
for (let index = 0; index < 150; index++) game.objectService.createObject(`Pebble${index}`, brickInfo(v(1, 1, 1)), pile.id);"#,
    )
    .await;
    assert!(!built.is_error, "{}", built.text);
    let (tower, pile) = {
        let world = world_arc.read().await;
        (world.getObjectIdsByName("Tower")[0], world.getObjectIdsByName("Pile")[0])
    };

    let listed = run_tool(&world_arc, "ls", json!({ "in_object_id": tower, "offset": null, "limit": 20, "depth": 3 })).await;
    assert!(!listed.is_error, "{}", listed.text);
    let listed_json: serde_json::Value = serde_json::from_str(&listed.text).unwrap();
    assert_eq!(count_ls_nodes(&listed_json["results"]), 20);
    let floors = listed_json["results"].as_array().unwrap();
    assert_eq!(floors.len(), 3);
    assert_eq!(floors[0]["child_count"], json!(50));
    assert_eq!(floors[0]["children"].as_array().unwrap().len(), 17);
    assert_eq!(floors[1]["children"], json!([]));

    let inspected = run_tool(&world_arc, "inspect", json!({ "object_id": pile, "mode": null })).await;
    assert!(!inspected.is_error, "{}", inspected.text);
    let inspected_json: serde_json::Value = serde_json::from_str(&inspected.text).unwrap();
    assert_eq!(inspected_json["child_count"], json!(150));
    assert_eq!(inspected_json["children"].as_array().unwrap().len(), 100);
}

#[tokio::test]
async fn scripts_read_as_numbered_lines_and_edit_by_exact_text() {
    let draft = include_str!("../../editor_server/initial-draft-gameworld.kube");
    let world_arc = Arc::new(RwLock::new(World::fromStoredWorldString(draft, WorldControlMode::Editor, None).unwrap()));
    let script_id = {
        let mut world = world_arc.write().await;
        let root = world.getRootObjectId();
        let script_id = world.addObject(
            "Notes".into(),
            ObjectBeingAdded::Script(ScriptInfo {
                objectLocation: ObjectLocation::Server,
            }),
            root,
        );
        world.setScriptRawCode(script_id, "const a = 1;\nconst b = 2;\nconst c = 1;\n".into());
        script_id
    };

    let read = run_tool(
        &world_arc,
        "read_script_raw_code",
        json!({ "object_id": script_id, "start_line": 2, "line_count": 1 }),
    )
    .await;
    assert!(!read.is_error, "{}", read.text);
    assert_eq!(read.text, format!("Script {script_id} \"Notes\", lines 2-2 of 3:\n2\tconst b = 2;\n"));

    let edit = |old_string: &str, new_string: &str, replace_all: Option<bool>| json!({ "object_id": script_id, "old_string": old_string, "new_string": new_string, "replace_all": replace_all });
    let ambiguous = run_tool(&world_arc, "edit_script_raw_code", edit("= 1;", "= 5;", None)).await;
    assert!(ambiguous.is_error && ambiguous.text.contains("appears 2 times"), "{}", ambiguous.text);
    let missing = run_tool(&world_arc, "edit_script_raw_code", edit("const d", "const e", None)).await;
    assert!(missing.is_error && missing.text.contains("was not found"), "{}", missing.text);
    let edited = run_tool(&world_arc, "edit_script_raw_code", edit("const b = 2;", "const b = 3;", None)).await;
    assert!(!edited.is_error, "{}", edited.text);
    let replaced_every_one = run_tool(&world_arc, "edit_script_raw_code", edit("= 1;", "= 5;", Some(true))).await;
    assert!(!replaced_every_one.is_error, "{}", replaced_every_one.text);
    assert_eq!(
        world_arc.read().await.scriptRawCodeOfId[&script_id],
        "const a = 5;\nconst b = 3;\nconst c = 5;\n"
    );

    // The AI already has the code it wrote, and read_script_raw_code is the one tool that shows code.
    let written = run_tool(
        &world_arc,
        "write_script_raw_code",
        json!({ "object_id": script_id, "script_raw_code": "const z = 0;\n" }),
    )
    .await;
    assert!(!written.is_error && !written.text.contains("const z"), "{}", written.text);
    let inspected = run_tool(&world_arc, "inspect", json!({ "object_id": script_id, "mode": null })).await;
    assert!(!inspected.is_error && !inspected.text.contains("const z"), "{}", inspected.text);
}

// One JSX typo once turned into 140,000 characters of TypeScript errors in a single result.
#[test]
fn script_warnings_are_one_line_each_and_stop_at_20_per_script() {
    let diagnostic = json!({ "message": "Type 'string' is not assignable to type 'number'.\n  Because reasons.", "startLineNumber": 22, "startColumn": 15, "endLineNumber": 22, "endColumn": 16, "severity": "error" });
    let script_files_text =
        json!({ "files": [{ "objectId": 343, "scriptWorld": "editor", "path": "root/scripts/spawn", "diagnostics": vec![diagnostic; 30] }] }).to_string();
    assert_eq!(
        script_warnings_of_script_files_text(&script_files_text).unwrap(),
        vec![json!({
            "object_id": 343,
            "path": "root/scripts/spawn",
            "warning_count": 30,
            "warnings": vec!["22:15 error Type 'string' is not assignable to type 'number'. Because reasons."; MAX_LISTED_SCRIPT_WARNINGS],
        })]
    );
}
