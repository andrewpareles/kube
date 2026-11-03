use client::play_runner::play::GuiTriggeredHandler;
use client::play_web_view::{GraphicsQuality, HostToWebViewMessage, PlayersListEntry, ScriptGuiRoot, UiRect, UiRects, WebViewToHostMessage};
use client::splash_runner::SplashScreenContent;
use serde_json::json;
use shared::game::gui_types::{GuiElementKind, GuiRenderedRoot, GuiSerializedElementNode, GuiSerializedNode, GuiSerializedTextNode};

// The play webview page and the client only share these JSON shapes, so they are checked here against
// the same messages tests/play_webview.browser.test.ts sees the page send.
#[test]
fn page_messages_decode_into_the_clients_types() {
    let decode = |json: &str| serde_json::from_str::<WebViewToHostMessage>(json).unwrap();
    assert_eq!(decode(r#""ready""#), WebViewToHostMessage::Ready);
    assert_eq!(
        decode(r#"{"setUiRects":{"uiOnlyRects":[{"x":10.5,"y":20,"width":30,"height":40}],"scrollableRects":[]}}"#),
        WebViewToHostMessage::SetUiRects(UiRects {
            ui_only_rects: vec![UiRect {
                x: 10.5,
                y: 20.0,
                width: 30.0,
                height: 40.0
            }],
            scrollable_rects: vec![],
        })
    );
    assert_eq!(decode(r#"{"setIsChatFocused":true}"#), WebViewToHostMessage::SetIsChatFocused(true));
    assert_eq!(decode(r#"{"setIsMenuOpen":true}"#), WebViewToHostMessage::SetIsMenuOpen(true));
    assert_eq!(
        decode(r#"{"guiHandler":{"scriptObjectId":7,"guiKey":"hud","handlerId":"buy.onClick","button":null}}"#),
        WebViewToHostMessage::GuiHandler(GuiTriggeredHandler {
            script_object_id: 7,
            gui_key: "hud".to_string(),
            handler_id: "buy.onClick".to_string(),
            button: None,
        })
    );
    assert_eq!(
        decode(r#"{"guiHandler":{"scriptObjectId":7,"guiKey":"hud","handlerId":"slot.onMouseDown","button":"right"}}"#),
        WebViewToHostMessage::GuiHandler(GuiTriggeredHandler {
            script_object_id: 7,
            gui_key: "hud".to_string(),
            handler_id: "slot.onMouseDown".to_string(),
            button: Some("right".to_string()),
        })
    );
    assert_eq!(
        decode(r#"{"sendGameChatMessage":{"message":"hello"}}"#),
        WebViewToHostMessage::SendGameChatMessage { message: "hello".to_string() }
    );
    assert_eq!(decode(r#"{"setVolume":0.4}"#), WebViewToHostMessage::SetVolume(0.4));
    assert_eq!(
        decode(r#"{"setGraphicsQuality":"low"}"#),
        WebViewToHostMessage::SetGraphicsQuality(GraphicsQuality::Low)
    );
    assert_eq!(decode(r#""leaveGame""#), WebViewToHostMessage::LeaveGame);
}

#[test]
fn client_messages_encode_the_way_the_page_reads_them() {
    assert_eq!(serde_json::to_value(HostToWebViewMessage::ShowPlayScreen).unwrap(), json!("showPlayScreen"));

    let players_list = [PlayersListEntry {
        username: "Alice".to_string(),
        is_local_player: true,
        ping_ms: Some(42),
    }];
    assert_eq!(
        serde_json::to_value(HostToWebViewMessage::SetPlayersList(&players_list)).unwrap(),
        json!({ "setPlayersList": [{ "username": "Alice", "isLocalPlayer": true, "pingMs": 42 }] })
    );

    let splash_screen_content = SplashScreenContent {
        title_text: "loading".to_string(),
        sub_text: None,
        thumbnailUrl: Some("https://example.com/thumbnail.webp".to_string()),
        is_loading: true,
    };
    assert_eq!(
        serde_json::to_value(HostToWebViewMessage::ShowSplashScreen(&splash_screen_content)).unwrap(),
        json!({ "showSplashScreen": { "titleText": "loading", "subText": null, "thumbnailUrl": "https://example.com/thumbnail.webp", "isLoading": true } })
    );

    let root = GuiRenderedRoot {
        children: vec![GuiSerializedNode::Element(GuiSerializedElementNode {
            element_kind: GuiElementKind::Button,
            node_id: "buy".to_string(),
            class_name: "bg-sky-500".to_string(),
            on_click_handler_id: Some("buy.onClick".to_string()),
            on_right_click_handler_id: Some("buy.onRightClick".to_string()),
            on_mouse_down_handler_id: Some("buy.onMouseDown".to_string()),
            on_mouse_up_handler_id: Some("buy.onMouseUp".to_string()),
            on_hover_handler_id: None,
            children: vec![GuiSerializedNode::Text(GuiSerializedTextNode {
                node_id: "buy.text".to_string(),
                text: "Buy".to_string(),
            })],
        })],
    };
    let script_gui_roots = vec![ScriptGuiRoot {
        script_object_id: 7,
        gui_key: "hud",
        root: &root,
    }];
    assert_eq!(
        serde_json::to_value(HostToWebViewMessage::SetScriptGuiRoots(script_gui_roots)).unwrap(),
        json!({ "setScriptGuiRoots": [{ "scriptObjectId": 7, "guiKey": "hud", "root": { "children": [
            { "element": { "elementKind": "button", "nodeId": "buy", "className": "bg-sky-500", "onClickHandlerId": "buy.onClick", "onRightClickHandlerId": "buy.onRightClick", "onMouseDownHandlerId": "buy.onMouseDown", "onMouseUpHandlerId": "buy.onMouseUp", "onHoverHandlerId": null, "children": [
                { "text": { "nodeId": "buy.text", "text": "Buy" } },
            ] } },
        ] } }] })
    );
}
