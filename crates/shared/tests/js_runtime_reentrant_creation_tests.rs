mod helpers;

use glam::Vec3;
use helpers::world_test_helpers::{add_brick, make_world};
use shared::game::jsRuntime::runTemporaryScript;

// Object creation deserializes JS properties before borrowing the world. Those properties can
// invoke Rust ops through live-object proxies or getters, including another createObject call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_object_accepts_a_live_object_proxy_in_a_temporary_script() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let source_object_id = 2;
    let source_position = Vec3::new(3.0, 4.0, 5.0);
    add_brick(&mut world, source_object_id, root_object_id, source_position, false)?;
    world.objectIdCounter = source_object_id;

    runTemporaryScript(
        &mut world,
        r#"
const source = game.objectService.getObjectByName("brick-2");
const copy = game.objectService.createObject("copy", source, game.world.id);
if (copy === null || copy.id === source.id || copy.name !== "copy") {
    throw new Error("Expected an independent copy of the source brick");
}
copy.location.position.x = 12;
"#
        .to_string(),
    )?;

    let copy_object_id = *world.getObjectIdsByName("copy").first().expect("Expected the copied brick");
    assert_eq!(world.getObjectPhysicsInfo3D(source_object_id).unwrap().location.position, source_position);
    assert_eq!(
        world.getObjectPhysicsInfo3D(copy_object_id).unwrap().location.position,
        Vec3::new(12.0, 4.0, 5.0)
    );
    assert_eq!(world.getObject(source_object_id)?.name, "brick-2");
    Ok(())
}

#[test]
fn create_object_getters_can_read_modify_and_create_world_objects() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.objectIdCounter = world.getRootObjectId();

    runTemporaryScript(
        &mut world,
        r#"
const source = game.objectService.createObject("source", { type: "group" }, game.world.id);
const copy = game.objectService.createObject("copy", {
    get type() {
        source.name = "renamed-source";
        const nested = game.objectService.createObject("nested", source, game.world.id);
        return game.objectService.getObjectById(nested.id).type;
    },
}, game.world.id);
if (copy === null || copy.type !== "group" || source.name !== "renamed-source") {
    throw new Error("Expected nested ops to finish before creating the outer object");
}
"#
        .to_string(),
    )?;

    for name in ["renamed-source", "nested", "copy"] {
        assert_eq!(world.getObjectIdsByName(name).len(), 1, "Expected {name} to be created");
    }
    Ok(())
}

#[test]
fn create_object_with_an_invalid_getter_value_fails_without_stopping_the_runtime() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.objectIdCounter = world.getRootObjectId();

    runTemporaryScript(
        &mut world,
        r#"
let didReject = false;
try {
    game.objectService.createObject("invalid", {
        get type() { return game.world.name; },
    }, game.world.id);
} catch (error) {
    didReject = true;
}

if (!didReject) throw new Error("Expected invalid object type to fail");
game.objectService.createObject("after-error", { type: "group" }, game.world.id);
"#
        .to_string(),
    )?;

    assert!(world.getObjectIdsByName("invalid").is_empty());
    assert_eq!(world.getObjectIdsByName("after-error").len(), 1);
    Ok(())
}

#[test]
fn deleted_handles_and_throwing_or_cyclic_inputs_leave_the_runtime_usable() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.objectIdCounter = world.getRootObjectId();
    runTemporaryScript(
        &mut world,
        r#"
const source = game.objectService.createObject("source", { type: "group" }, game.world.id);
game.objectService.deleteObject(source.id);
const cyclic = { type: "group" };
cyclic.self = cyclic;
for (const [input, message] of [
    [source, "Object or field no longer exists"],
    [{ get type() { throw new Error("type getter failed"); } }, "type getter failed"],
    [{ get toJSON() { throw new Error("toJSON getter failed"); } }, "toJSON getter failed"],
    [cyclic, "circular"],
]) {
    let rejected = false;
    try { game.objectService.createObject("invalid", input, game.world.id); }
    catch (error) {
        rejected = true;
        if (!String(error).includes(message)) throw error;
    }
    if (!rejected) throw new Error("Expected input conversion to fail");
}
const valid = game.objectService.createObject("after-errors", { type: "group" }, game.world.id);
if (valid.name !== "after-errors") throw new Error("Runtime did not recover");
"#
        .to_string(),
    )?;
    assert!(world.getObjectIdsByName("invalid").is_empty());
    assert_eq!(world.getObjectIdsByName("after-errors").len(), 1);
    Ok(())
}
