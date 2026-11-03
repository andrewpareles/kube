use game_types::object_types::ObjectId;
use serde::{Deserialize, Serialize};

pub type ScriptGuiKey = (ObjectId, String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiRenderedRoot {
    pub children: Vec<GuiSerializedNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuiSerializedNode {
    Element(GuiSerializedElementNode),
    Text(GuiSerializedTextNode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuiElementKind {
    Div,
    Button,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiSerializedElementNode {
    pub element_kind: GuiElementKind,
    pub node_id: String,
    pub class_name: String,
    pub on_click_handler_id: Option<String>,
    pub on_right_click_handler_id: Option<String>,
    pub on_mouse_down_handler_id: Option<String>,
    pub on_mouse_up_handler_id: Option<String>,
    pub on_hover_handler_id: Option<String>,
    pub children: Vec<GuiSerializedNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiSerializedTextNode {
    pub node_id: String,
    pub text: String,
}
