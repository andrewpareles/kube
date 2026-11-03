mod helpers;

use deno_core::JsRuntime;
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use shared::game::gui_types::{GuiElementKind, GuiSerializedElementNode, GuiSerializedNode, GuiSerializedTextNode};
use shared::game::jsRuntime::{JsRuntimeInitialization, ScriptManager, callRegisteredGuiHandler, initializeJsAndRunInitialScripts, initializeJsRuntime};
use std::{cell::RefCell, rc::Rc};

const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;

#[tokio::test]
async fn gui_buttons_serialize_as_buttons_while_child_divs_stay_layout() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "gui_button_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
const Controls = () => {
    return (
        <button className="outer" onClick={() => {}}>
            <div className="layout">
                <div>Play</div>
                <div>Start game</div>
            </div>
            <button className="inner" onClick={() => {}}>Delete</button>
        </button>
    );
};

game.guiService.show('controls', Controls);
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc): (JsRuntime, Rc<RefCell<ScriptManager>>) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let script_manager = script_manager_rc.borrow();
    let gui_root = script_manager
        .gui_root_of_script_gui_key()
        .get(&(SCRIPT_OBJECT_ID, "controls".to_string()))
        .ok_or_else(|| anyhow::anyhow!("controls GUI root was not rendered"))?;

    let [GuiSerializedNode::Element(outer_button_node)] = gui_root.children.as_slice() else {
        return Err(anyhow::anyhow!("expected one outer button"));
    };
    assert_eq!(outer_button_node.element_kind, GuiElementKind::Button);
    assert_eq!(outer_button_node.class_name, "outer");
    assert!(outer_button_node.on_click_handler_id.is_some());

    let [GuiSerializedNode::Element(layout_div_node), GuiSerializedNode::Element(inner_button_node)] = outer_button_node.children.as_slice() else {
        return Err(anyhow::anyhow!("expected layout div and nested button"));
    };
    assert_eq!(layout_div_node.element_kind, GuiElementKind::Div);
    assert_eq!(inner_button_node.element_kind, GuiElementKind::Button);
    assert_eq!(inner_button_node.class_name, "inner");
    assert!(inner_button_node.on_click_handler_id.is_some());

    Ok(())
}

// The page sends back which handler the player triggered, with the button for onMouseDown and onMouseUp, and the
// script's handler gets that button.
#[tokio::test]
async fn gui_mouse_button_handlers_get_the_button_the_page_sends() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "gui_mouse_button_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
const Slot = () => {
    const [lastPress, setLastPress] = useState('none');
    return (
        <div
            className="slot"
            onRightClick={() => setLastPress('right click')}
            onMouseDown={(button) => setLastPress(`down ${button}`)}
            onMouseUp={(button) => setLastPress(`up ${button}`)}
        >
            {lastPress}
        </div>
    );
};

game.guiService.show('slot', Slot);
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc): (JsRuntime, Rc<RefCell<ScriptManager>>) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    let get_slot_node = || -> anyhow::Result<GuiSerializedElementNode> {
        let script_manager = script_manager_rc.borrow();
        let gui_root = script_manager
            .gui_root_of_script_gui_key()
            .get(&(SCRIPT_OBJECT_ID, "slot".to_string()))
            .ok_or_else(|| anyhow::anyhow!("slot GUI root was not rendered"))?;
        let [GuiSerializedNode::Element(slot_node)] = gui_root.children.as_slice() else {
            return Err(anyhow::anyhow!("expected one slot div"));
        };
        Ok(slot_node.clone())
    };
    let get_slot_text = || -> anyhow::Result<String> {
        let slot_node = get_slot_node()?;
        let [GuiSerializedNode::Text(GuiSerializedTextNode { text, .. })] = slot_node.children.as_slice() else {
            return Err(anyhow::anyhow!("expected the slot to only hold text"));
        };
        Ok(text.clone())
    };

    let GuiSerializedElementNode {
        on_click_handler_id,
        on_right_click_handler_id,
        on_mouse_down_handler_id,
        on_mouse_up_handler_id,
        on_hover_handler_id,
        ..
    } = get_slot_node()?;
    assert_eq!(on_click_handler_id, None);
    assert_eq!(on_hover_handler_id, None);
    let (Some(on_right_click_handler_id), Some(on_mouse_down_handler_id), Some(on_mouse_up_handler_id)) =
        (on_right_click_handler_id, on_mouse_down_handler_id, on_mouse_up_handler_id)
    else {
        return Err(anyhow::anyhow!("expected right click, mouse down and mouse up handlers"));
    };
    assert_eq!(get_slot_text()?, "none");

    callRegisteredGuiHandler(
        &mut js_runtime,
        &mut world,
        0.0,
        Some(0.0),
        &script_manager_rc,
        SCRIPT_OBJECT_ID,
        "slot",
        &on_mouse_down_handler_id,
        Some("right"),
    )
    .await?;
    assert_eq!(get_slot_text()?, "down right");
    callRegisteredGuiHandler(
        &mut js_runtime,
        &mut world,
        0.0,
        Some(0.0),
        &script_manager_rc,
        SCRIPT_OBJECT_ID,
        "slot",
        &on_mouse_up_handler_id,
        Some("left"),
    )
    .await?;
    assert_eq!(get_slot_text()?, "up left");
    callRegisteredGuiHandler(
        &mut js_runtime,
        &mut world,
        0.0,
        Some(0.0),
        &script_manager_rc,
        SCRIPT_OBJECT_ID,
        "slot",
        &on_right_click_handler_id,
        None,
    )
    .await?;
    assert_eq!(get_slot_text()?, "right click");

    Ok(())
}
