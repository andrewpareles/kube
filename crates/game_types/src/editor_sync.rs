use crate::{object_delta::ObjectDelta, object_types::ObjectId, stored_world_type::StoredWorld};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// andrew note:
// - EditorState is the shared state between browser and server.
// - EditorSync runs on the editor_server and syncs them as the authority.

#[derive(Debug, Serialize, Deserialize)]
pub struct EditorState {
    // The connection checks this once, before accepting binary deltas.
    pub protocol_version: u64,
    // The number of the last batch this world already includes.
    pub seq: u64,
    pub world: StoredWorld,
    pub editor_status: EditorStatus,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorStatus {
    pub is_playtest_running: bool,
    pub is_chat_running: bool,
    // Whether each editing session has a step to undo and to redo, keyed by a hash of the session so no
    // browser learns another's.
    pub history_of_session: HashMap<String, EditorHistoryAvailability>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorHistoryAvailability {
    pub can_undo: bool,
    pub can_redo: bool,
}

// Editor-only script sources travel with their object changes, never through game networking.
#[derive(Debug, Serialize, Deserialize)]
pub struct EditorServerDelta {
    // Batches are numbered in the order they are sent, so a browser knows when one includes its edit.
    pub seq: u64,
    pub object_deltas: Vec<(ObjectId, ObjectDelta)>,
    pub script_raw_code_of_id: HashMap<ObjectId, String>,
    pub editor_status: EditorStatus,
}

// These names and paths match StoredWorld and the browser property controls exactly.
#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EditorSection {
    objectOfId,
    physicsInfo3DOfId,
    physicsToggles3DOfId,
    primitiveSettingsOfId,
    scriptRawCodeOfId,
}

// One user action, which is one undo step however many objects it touches. The description is
// what the inspector already showed the user, so the editor never rebuilds those labels itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditorClientAction {
    pub description: String,
    pub deltas: Vec<EditorClientDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditorClientDelta {
    pub objectId: ObjectId,
    pub section: EditorSection,
    pub path: Vec<String>,
    pub value: serde_json::Value,
}
