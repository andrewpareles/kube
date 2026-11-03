mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use serde_json::{Value, json};
use shared::game::gui_types::{GuiRenderedRoot, GuiSerializedElementNode, GuiSerializedNode};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};

// GUIs used to take an anchor that placed them on the screen. Worlds saved before that keep their GUIs where the anchor put
// them: each script that uses the GUI service gets a stand-in for it that shows GUIs the old way.

const OLD_GUI_SCRIPT: &str = r#"'use client';
const Corner = () => <div className="bg-black">corner</div>
game.guiService.show("corner", Corner, "bottomRight")
game.guiService.show("banner", <div>banner</div>)
game.guiService.show("unknown", () => <div>unknown</div>, "nowhere")
const gui = game.guiService
gui.show("copied", <div>copied</div>, "left")
"#;

fn migrate_scripts(script_raw_code_of_id: Value) -> anyhow::Result<Value> {
    let mut stored_world = json!({ "worldSchemaVersion": 84, "scriptRawCodeOfId": script_raw_code_of_id });
    updateToVersion(&mut stored_world, 85, StoredWorldKind::GameWorld)?;
    Ok(stored_world["scriptRawCodeOfId"].clone())
}

fn get_single_element(nodes: &[GuiSerializedNode]) -> &GuiSerializedElementNode {
    let [GuiSerializedNode::Element(element_node)] = nodes else {
        panic!("expected one element, got {nodes:?}");
    };
    element_node
}

#[test]
fn scripts_keep_their_directive_first_and_only_scripts_that_show_a_gui_change() -> anyhow::Result<()> {
    let untouched_script = "'use client'\ngame.guiService.hide(\"hud\")\n";
    let script_without_directive = "game.guiService.show(\"hud\", <div />, \"top\")\n";
    let script_that_copies_the_service = "const GUI: any = game.guiService;\nGUI.show(\"hud\", <div />, \"topLeft\")\n";
    let script_raw_code_of_id =
        migrate_scripts(json!({ "2": OLD_GUI_SCRIPT, "3": untouched_script, "4": script_without_directive, "5": script_that_copies_the_service }))?;

    let migrated_script = script_raw_code_of_id["2"].as_str().unwrap();
    assert!(migrated_script.starts_with("'use client';\n// Kube.new added this"), "{migrated_script}");
    assert!(migrated_script.contains("guiServiceWithOldAnchor.show(\"corner\", Corner, \"bottomRight\")"));
    assert!(!migrated_script.contains("game.guiService.show(\"corner\""));
    assert_eq!(script_raw_code_of_id["3"], untouched_script);
    let migrated_script_without_directive = script_raw_code_of_id["4"].as_str().unwrap();
    assert!(migrated_script_without_directive.starts_with("// Kube.new added this"));
    assert!(migrated_script_without_directive.ends_with("guiServiceWithOldAnchor.show(\"hud\", <div />, \"top\")\n"));
    let migrated_script_that_copies_the_service = script_raw_code_of_id["5"].as_str().unwrap();
    assert!(migrated_script_that_copies_the_service.starts_with("// Kube.new added this"));
    assert!(migrated_script_that_copies_the_service.ends_with("const GUI: any = guiServiceWithOldAnchor;\nGUI.show(\"hud\", <div />, \"topLeft\")\n"));
    Ok(())
}

#[tokio::test]
async fn migrated_scripts_show_their_guis_where_the_anchor_put_them() -> anyhow::Result<()> {
    let migrated_script = migrate_scripts(json!({ "2": OLD_GUI_SCRIPT }))?["2"].as_str().unwrap().to_string();
    let mut world = make_world_controlled_by_player(99)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        2,
        "old-gui".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(2, migrated_script);
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let script_manager = script_manager_rc.borrow();
    let get_gui_root = |gui_key: &str| -> &GuiRenderedRoot {
        script_manager
            .gui_root_of_script_gui_key()
            .get(&(2, gui_key.to_string()))
            .unwrap_or_else(|| panic!("expected the {gui_key} GUI to be shown"))
    };

    // The anchor's column covers the screen and lets the pointer through, and only what the script drew catches it.
    for (gui_key, placement_class_name) in [
        ("corner", "justify-end items-end"),
        ("banner", "justify-start items-center"),
        ("unknown", "justify-start items-center"),
        ("copied", "justify-center items-start"),
    ] {
        let column = get_single_element(&get_gui_root(gui_key).children);
        assert!(column.class_name.ends_with(placement_class_name), "{gui_key}: {}", column.class_name);
        for class_name in [
            "pointer-events-none",
            "absolute",
            "inset-0",
            "flex-col",
            "*:pointer-events-auto",
            "text-[#8c8c8c]",
            "text-[12.5px]",
        ] {
            assert!(
                column.class_name.split(' ').any(|column_class_name| column_class_name == class_name),
                "{gui_key}: {}",
                column.class_name
            );
        }
        let GuiSerializedNode::Text(text_node) = &get_single_element(&column.children).children[0] else {
            panic!("{gui_key}: expected the script's own text");
        };
        assert_eq!(text_node.text, gui_key);
    }
    Ok(())
}
