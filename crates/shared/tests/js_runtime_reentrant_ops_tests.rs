mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use shared::game::gui_types::GuiSerializedNode;
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};

// These exercise the real JS/Rust boundary with getters that read and modify engine state.
// Input getters must finish before an op borrows state; output construction must not run setters.
#[tokio::test]
async fn gui_getters_can_read_the_world_and_modify_the_script_manager() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        2,
        "gui-getters".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        2,
        r#"'use client'
Deno.core.ops.__setRustGuiRoot__(script.id, "hud", JSON.stringify({ children: [] }));
Deno.core.ops.__setRustGuiRoot__(script.id, "hud", JSON.stringify({
    get children() {
        Deno.core.ops.__clearRustGuiRoot__(script.id, "hud");
        return [{ text: { nodeId: "message", get text() { return game.world.name; } } }];
    },
}));
let rejected = false;
try {
    Deno.core.ops.__setRustGuiRoot__(script.id, "hud", JSON.stringify({ get children() { throw new Error("GUI getter failed"); } }));
} catch (error) {
    rejected = true;
    if (!String(error).includes("GUI getter failed")) throw error;
}
if (!rejected) throw new Error("Expected GUI input conversion to fail");
rejected = false;
try { Deno.core.ops.__setRustGuiRoot__(script.id, "hud", "{"); } catch (error) {
    rejected = true;
    if (!(error instanceof Error)) throw error;
}
if (!rejected) throw new Error("Expected invalid GUI JSON to fail");
"#
        .to_string(),
    );
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let script_manager = script_manager_rc.borrow();
    let gui_root = script_manager
        .gui_root_of_script_gui_key()
        .get(&(2, "hud".to_string()))
        .expect("Expected replacement GUI root");
    let [GuiSerializedNode::Text(node)] = gui_root.children.as_slice() else {
        panic!("Expected text from the nested getter");
    };
    assert_eq!(node.text, world.getObject(root_object_id)?.name);
    Ok(())
}
