mod helpers;

use game_types::object_types::ObjectInfo;
use helpers::world_test_helpers::{add_local_group, add_script, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};

#[tokio::test]
async fn template_service_cut_and_paste_template_returns_a_live_object_from_ts() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let script_object_id = 2;
    let source_group_object_id = 3;

    add_script(&mut world, script_object_id, root_object_id)?;
    add_local_group(&mut world, source_group_object_id, root_object_id)?;
    world.objectIdCounter = world
        .objectOfId
        .keys()
        .copied()
        .max()
        .expect("Expected script setup to leave at least one object in the world");

    // This checks the server script API for storing a live object subtree as a template and creating it later.
    world.markScriptAsReadyToRun(
        script_object_id,
        r#"
const sourceGroup = game.objectService.getObjectByName("local-group-3");
if (sourceGroup === null || sourceGroup.type !== "group") {
    throw new Error("Expected source group");
}

game.templateService.cutObjectToTemplate(sourceGroup.id, "stored_group");
if (game.objectService.getObjectById(sourceGroup.id) !== null) {
    throw new Error("Expected source group to be deleted after cutting it to a template");
}

const pastedGroup = game.templateService.pasteTemplate("stored_group", game.world.id);
if (pastedGroup === null) {
    throw new Error("Expected pasted group");
}
if (pastedGroup.type !== "group") {
    throw new Error("Expected pasted group type, got " + pastedGroup.type);
}

pastedGroup.name = "pasted_from_template";
const pastedParent = pastedGroup.parent;
if (pastedParent === null || pastedParent.id !== game.world.id) {
    throw new Error("Expected pasted group parent to be the world root");
}
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    assert!(!world.hasObject(source_group_object_id));
    assert!(world.templateOfName.contains_key("stored_group"));
    let pasted_group_object_id = *world
        .getObjectIdsByName("pasted_from_template")
        .first()
        .ok_or_else(|| anyhow::anyhow!("Expected pasted template object"))?;
    let pasted_group_object = world.getObject(pasted_group_object_id)?;
    let ObjectInfo::LocalGroup(_) = &pasted_group_object.info else {
        return Err(anyhow::anyhow!("Expected pasted group info"));
    };
    assert_eq!(world.parentOfId.get(&pasted_group_object_id).copied(), Some(root_object_id));

    Ok(())
}
