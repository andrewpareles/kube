mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};

// GUIs stack in the order they were shown, the newest on top. Showing a GUI again keeps its place, and a GUI that was
// hidden goes on top when it is shown again.
#[tokio::test]
async fn guis_stack_in_the_order_they_were_shown() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        2,
        "stacking".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        2,
        r#"'use client'
game.guiService.show("a", <div>a</div>)
game.guiService.show("b", <div>b</div>)
game.guiService.show("a", <div>a again</div>)
game.guiService.show("c", <div>c</div>)
game.guiService.show("d", <div>d</div>)
game.guiService.show("e", <div>e</div>)
game.guiService.hide("c")
game.guiService.show("c", <div>c again</div>)
"#
        .to_string(),
    );
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let script_manager = script_manager_rc.borrow();
    let gui_keys: Vec<&str> = script_manager
        .gui_root_of_script_gui_key()
        .keys()
        .map(|(_, gui_key)| gui_key.as_str())
        .collect();
    assert_eq!(gui_keys, ["a", "b", "d", "e", "c"]);
    Ok(())
}
