use editor_server::{
    editor_history::read_object_value,
    editor_world_tracker::{EditorChange, EditorLeafValue, EditorWorldTracker},
};
use game_types::object_types::{LocalGroupInfo, ObjectBeingAdded, ObjectId};
use serde_json::{Value, json};
use shared::{
    api::protocol_for_chat::{ToolCall, apply_editor_tool_call},
    game::{
        data_fetcher::DataFetcherArc,
        world::{World, WorldControlMode},
    },
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;

fn read_world(world: &World) -> BTreeMap<ObjectId, Value> {
    world
        .objectOfId
        .keys()
        .map(|object_id| (*object_id, read_object_value(world, *object_id)))
        .collect()
}

fn read_at<'v>(value: &'v Value, path: &[String]) -> &'v Value {
    path.iter().fold(value, |value, name| &value[name.as_str()])
}

fn write_at(value: &mut Value, path: &[String], new: Value) {
    *path.iter().fold(value, |value, name| &mut value[name.as_str()]) = new;
}

// The world before the drain with every drained leaf written over it, after checking each leaf replaced what
// the world held before.
fn apply_changes(mut world_value: BTreeMap<ObjectId, Value>, changes: &[EditorChange]) -> BTreeMap<ObjectId, Value> {
    let mut hidden_leaves = HashSet::new();
    for leaf in changes.iter().flat_map(|change| &change.leaves) {
        if leaf.is_hidden && !hidden_leaves.insert((leaf.object_id, leaf.path.clone())) {
            continue;
        }
        match (&leaf.old, &leaf.new) {
            (EditorLeafValue::Json(old), EditorLeafValue::Json(new)) => {
                let value = world_value.get_mut(&leaf.object_id).expect("A leaf's object was there");
                assert_eq!(read_at(value, &leaf.path), old, "{} {:?}", leaf.object_id, leaf.path);
                write_at(value, &leaf.path, new.clone());
            }
            (EditorLeafValue::Group(group), EditorLeafValue::Absent) => {
                for (object_id, state) in &group.objects {
                    assert_eq!(world_value.remove(object_id).as_ref(), Some(&state.value), "{object_id}");
                }
                for (object_id, path, old) in &group.links {
                    let value = world_value.get_mut(object_id).expect("A linking object survives");
                    assert_eq!(read_at(value, path), old, "{object_id} {path:?}");
                    write_at(value, path, Value::Null);
                }
            }
            (EditorLeafValue::Absent, EditorLeafValue::Group(group)) => {
                assert!(group.links.is_empty());
                for (object_id, state) in &group.objects {
                    assert!(world_value.insert(*object_id, state.value.clone()).is_none(), "{object_id}");
                }
            }
            (old, new) => panic!("{old:?} -> {new:?}"),
        }
    }
    world_value
}

fn named(world: &World, name: &str) -> ObjectId {
    let object_ids = world.getObjectIdsByName(name);
    assert_eq!(object_ids.len(), 1, "{name}");
    object_ids[0]
}

// The drain accounts for everything the AI's tools change: the world before, with the drained leaves written
// over it, is exactly the world after, and what the tracker saw is the world again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_drain_accounts_for_every_change_the_editor_tools_make() -> anyhow::Result<()> {
    let mut world = World::fromStoredWorldString(include_str!("../../../game-35-latest.kube"), WorldControlMode::Editor, None)?;
    let mut tracker = EditorWorldTracker::new(&mut world);
    let world_arc = Arc::new(RwLock::new(world));
    let (mut hidden_count, mut link_count, mut removed_count, mut added_count) = (0, 0, 0, 0);
    let mut run = async |name: &str, arguments: Value| -> anyhow::Result<()> {
        let before = read_world(&*world_arc.read().await);
        let result = apply_editor_tool_call(world_arc.clone(), DataFetcherArc::new(), None, ToolCall { name: name.into(), arguments }).await;
        assert!(!result.is_error, "{name}: {}", result.text);
        let mut world_lock = world_arc.write().await;
        let changes = tracker.drain(&mut world_lock, false);
        assert_eq!(apply_changes(before, &changes), read_world(&world_lock), "{name}");
        assert_eq!(tracker.read_unmarked_object_ids(&world_lock), Vec::<ObjectId>::new(), "{name}");
        for leaf in changes.iter().flat_map(|change| &change.leaves) {
            hidden_count += leaf.is_hidden as usize;
            match (&leaf.old, &leaf.new) {
                (EditorLeafValue::Group(group), _) => {
                    removed_count += group.objects.len();
                    link_count += group.links.len();
                }
                (_, EditorLeafValue::Group(group)) => added_count += group.objects.len(),
                _ => {}
            }
        }
        Ok(())
    };
    let (root, pig_body, skeleton, planks, fabric, spinners, walk_clock, spinners_script, pig) = {
        let world = world_arc.read().await;
        (
            world.getRootObjectId(),
            named(&world, "PigBody"),
            named(&world, "PigSkeleton"),
            named(&world, "wood_planks"),
            named(&world, "fabric"),
            named(&world, "Spinners"),
            named(&world, "WalkClock"),
            named(&world, "spinners"),
            named(&world, "PigCharacter"),
        )
    };
    let color = json!({ "r": 0.5, "g": 0.25, "b": 1.0 });
    let brick = |parent_id: ObjectId, name: &str, x: f32| json!({ "parent_id": parent_id, "name": name, "center": { "x": x, "y": 5, "z": 0 }, "size": { "x": 2, "y": 1, "z": 2 }, "rotation_degrees": null, "color": null, "opacity": null });

    run("create_brick", brick(root, "Crate", 0.0)).await?;
    run(
        "create_ball",
        json!({ "parent_id": root, "name": "Orb", "center": { "x": 4, "y": 5, "z": 0 }, "radius": 1, "color": color, "opacity": null }),
    )
    .await?;
    run("create_cylinder", json!({ "parent_id": root, "name": "Post", "center": { "x": 8, "y": 5, "z": 0 }, "radius": 0.5, "length": 3, "rotation_degrees": null, "color": null, "opacity": 0.5 })).await?;
    run("create_wall", json!({ "parent_id": root, "name": "Wall", "start": { "x": 0, "y": 0, "z": 10 }, "end": { "x": 10, "y": 0, "z": 10 }, "height": 3, "thickness": 0.5, "color": null, "opacity": null })).await?;
    run("create_box_between_corners", json!({ "parent_id": root, "name": "Box", "corner_a": { "x": 20, "y": 0, "z": 0 }, "corner_b": { "x": 22, "y": 2, "z": 2 }, "color": null, "opacity": null })).await?;
    run("create_brick_between_points", json!({ "parent_id": root, "name": "Beam", "start": { "x": 30, "y": 1, "z": 0 }, "end": { "x": 34, "y": 3, "z": 0 }, "width": 0.5, "height": 0.5, "color": null, "opacity": null })).await?;
    run("create_cylinder_between_points", json!({ "parent_id": root, "name": "Pipe", "start": { "x": 40, "y": 1, "z": 0 }, "end": { "x": 40, "y": 5, "z": 3 }, "radius": 0.25, "color": null, "opacity": null })).await?;
    let crate_id = named(&*world_arc.read().await, "Crate");
    run("create_label", json!({ "parent_id": root, "name": "Tag", "text": "Crate", "follows_object_id": crate_id, "offset": null, "line_height": null, "max_width": null, "background_color": null, "is_always_on_top": null, "color": null, "opacity": null })).await?;
    run(
        "create_object",
        json!({ "object": json!({ "type": "group", "name": "Folder" }).to_string(), "parent_id": root }),
    )
    .await?;
    run("create_object", json!({ "object": json!({ "type": "rigidGroup", "name": "Cart", "followSkeletonId": null, "location": { "position": { "x": 50, "y": 2, "z": 0 }, "rotation": { "x": 0, "y": 30, "z": 0 } }, "movement": { "velocity": { "x": 0, "y": 0, "z": 0 }, "angularVelocity": { "x": 0, "y": 0, "z": 0 } } }).to_string(), "parent_id": root })).await?;
    let (folder, cart) = {
        let world = world_arc.read().await;
        (named(&world, "Folder"), named(&world, "Cart"))
    };
    run("create_brick", brick(folder, "Left", 48.0)).await?;
    run("create_brick", brick(folder, "Right", 52.0)).await?;
    run("edit_object", json!({ "object_id": folder, "jspath": "parent", "jsvalue": cart.to_string() })).await?;
    run("move_by_recursively", json!({ "object_id": cart, "translation": { "x": 3, "y": 0, "z": 1 } })).await?;
    run(
        "rotate_recursively",
        json!({ "object_id": cart, "rotation_axis": { "x": 0, "y": 1, "z": 0 }, "rotation_magnitude": 0.7, "center_point": null }),
    )
    .await?;
    run(
        "edit_object",
        json!({ "object_id": folder, "jspath": "parent", "jsvalue": pig_body.to_string() }),
    )
    .await?;
    run("edit_object", json!({ "object_id": folder, "jspath": "parent", "jsvalue": root.to_string() })).await?;
    run(
        "scale_recursively",
        json!({ "object_id": folder, "scale_multiplier": 2, "center_point": { "x": 50, "y": 5, "z": 0 } }),
    )
    .await?;
    run(
        "move_to_recursively",
        json!({ "object_id": crate_id, "target_point": { "x": -5, "y": 6, "z": -5 } }),
    )
    .await?;
    run("edit_object", json!({ "object_id": crate_id, "jspath": "color", "jsvalue": color.to_string() })).await?;
    run("edit_object", json!({ "object_id": crate_id, "jspath": "material", "jsvalue": json!({ "materialInfoId": planks, "tileSizeU": 2, "tileSizeV": 2, "tileOffsetU": 0, "tileOffsetV": 0 }).to_string() })).await?;
    run("edit_object", json!({ "object_id": crate_id, "jspath": "name", "jsvalue": "\"Crate2\"" })).await?;
    run("edit_object", json!({ "object_id": walk_clock, "jspath": "clockState", "jsvalue": json!({ "Playing": { "localStartMs": 0, "timeLocation": "Server" } }).to_string() })).await?;
    run(
        "copy_and_paste",
        json!({ "object_id": pig_body, "offset": { "x": 0, "y": 0, "z": 20 }, "clone_name": "PigBodyCopy" }),
    )
    .await?;
    run(
        "write_script_raw_code",
        json!({ "object_id": spinners_script, "script_raw_code": "export const spun = 1;\n" }),
    )
    .await?;
    run(
        "edit_script_raw_code",
        json!({ "object_id": spinners_script, "old_string": "spun = 1", "new_string": "spun = 2", "replace_all": null }),
    )
    .await?;
    // run("get_placement_warnings", json!({})).await?;
    let parallel_calls: Vec<Value> = ["LeftPlatform", "RightPlatform"]
        .into_iter()
        .map(|name| json!({ "call_id": name, "tool_name": "create_brick", "args": brick(root, name, 60.0).to_string() }))
        .collect();
    run("run_parallel", json!({ "tool_calls": parallel_calls, "timeout_ms": null })).await?;
    run(
        "run_temporary_script",
        json!({ "script": format!(
            r#"
const crate = game.objectService.getObjectByName("Crate2");
crate.name = "Crate3";
crate.color = {{ r: 0, g: 1, b: 0 }};
crate.parent = game.objectService.getObjectById({folder});
game.objectService.moveObjectBy({folder}, {{ x: 1, y: 0, z: 0 }});
game.objectService.rotateObjectBy({cart}, {{ x: 0, y: 1, z: 0 }}, 45);
game.objectService.getObjectById({folder}).parent = game.objectService.getObjectById({cart});
game.objectService.cloneObject({cart}, "CartCopy");
game.objectService.createSkAttachmentsInRigidGroup({skeleton}, {pig_body});
game.objectService.deleteObject(game.objectService.getObjectByName("Orb").id);
"#
        ) }),
    )
    .await?;
    let (idle_animator, spin_animator, idle_clock) = {
        let world = world_arc.read().await;
        (named(&world, "IdleAnimator"), named(&world, "SpinAnimator"), named(&world, "IdleClock"))
    };
    run(
        "run_temporary_script",
        json!({ "script": format!(
            r#"
game.objectService.setObjectWalkthrough({folder}, true);
game.objectService.setObjectPushable({folder}, true);
game.objectService.setObjectDensity({folder}, 3);
game.objectService.moveObjectTo(game.objectService.getObjectByName("Post").id, {{ x: 1, y: 2, z: 3 }});
game.objectService.scaleObjectBy(game.objectService.getObjectByName("Wall").id, 1.5);
game.clockService.pause({walk_clock});
game.clockService.play({idle_clock}, 2);
game.clockService.setSpeed({idle_clock}, 0.5);
game.animationService.smoothlyChangeWeight({idle_animator}, 20, 500);
game.animationService.tweenTo({spin_animator}, ["weight"], 90, 1000);
game.templateService.cutObjectToTemplate(game.objectService.getObjectByName("Box").id, "BoxTemplate");
game.templateService.pasteTemplate("BoxTemplate", {folder});
"#
        ) }),
    )
    .await?;
    run("delete_object", json!({ "object_id": fabric })).await?;
    run("delete_object", json!({ "object_id": spinners })).await?;
    run("delete_object", json!({ "object_id": cart })).await?;
    run("delete_object", json!({ "object_id": pig })).await?;

    assert!(hidden_count > 0, "Some part only followed its folder");
    assert!(link_count > 0, "Some removal cleared links into it");
    assert!(removed_count > 0 && added_count > 0);
    Ok(())
}

// Only the World's marks bring an object into a drain, so a change made behind its back shows up in the check
// the tests run after every drain, and the full read at the end of a reply still picks it up.
#[test]
fn a_change_the_world_never_marked_is_caught() -> anyhow::Result<()> {
    let mut world = World::fromStoredWorldString(include_str!("../../../game-35-latest.kube"), WorldControlMode::Editor, None)?;
    let mut tracker = EditorWorldTracker::new(&mut world);
    let clock = named(&world, "WalkClock");
    world.getObjectMut(clock)?.name = "Unmarked".into();
    assert!(tracker.drain(&mut world, false).is_empty());
    assert_eq!(tracker.read_unmarked_object_ids(&world), vec![clock]);
    let changes = tracker.drain(&mut world, true);
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].leaves.iter().map(|leaf| (leaf.object_id, leaf.path.clone())).collect::<Vec<_>>(),
        vec![(clock, vec!["name".to_string()])]
    );
    assert_eq!(tracker.read_unmarked_object_ids(&world), Vec::<ObjectId>::new());
    Ok(())
}

// An add is one leaf on its top object, and its group holds the objects parent first.
#[test]
fn an_add_is_one_leaf_on_its_top() -> anyhow::Result<()> {
    let mut world = World::fromStoredWorldString(include_str!("../../../game-35-latest.kube"), WorldControlMode::Editor, None)?;
    let mut tracker = EditorWorldTracker::new(&mut world);
    let root = world.getRootObjectId();
    let child = world.addObject("Child".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root);
    let parent = world.addObject("Parent".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root);
    world.setParent(child, parent)?;
    let changes = tracker.drain(&mut world, false);
    let leaves: Vec<_> = changes.iter().flat_map(|change| &change.leaves).collect();
    assert_eq!(
        leaves.iter().map(|leaf| (leaf.object_id, leaf.path.clone())).collect::<Vec<_>>(),
        vec![(parent, vec!["exists".to_string()])]
    );
    let EditorLeafValue::Group(group) = &leaves[0].new else {
        panic!("An add");
    };
    assert_eq!(group.objects.iter().map(|(object_id, _)| *object_id).collect::<Vec<_>>(), vec![parent, child]);
    Ok(())
}
