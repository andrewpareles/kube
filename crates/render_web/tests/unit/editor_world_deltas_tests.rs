use super::{BrowserEditorWorld, StoredWorldRenderable};
use game_types::{
    consts::EDITOR_WORLD_PROTOCOL_VERSION,
    editor_sync::{EditorServerDelta, EditorState, EditorStatus},
    object_delta::{NetworkedCoords, NetworkedPhysicsInfo3D, ObjectAddDelta, ObjectDelta, ObjectModifyDelta},
    object_types::{LocalGroupInfo, ObjectBeingAdded, ObjectInfo, ObjectLocation, ScriptInfo},
    stored_world_type::StoredWorld,
};
use std::collections::{HashMap, HashSet};
use wasm_bindgen_test::wasm_bindgen_test;

// Exercise the wire format against the asset fixture, including the hierarchy used by selection.
fn stored_world() -> anyhow::Result<StoredWorld> {
    let mut asset: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/asset-preview-animated-object.json"))?;
    Ok(serde_json::from_value(asset["Object"].take())?)
}

fn browser_world(world: StoredWorld) -> BrowserEditorWorld {
    let state = EditorState {
        protocol_version: EDITOR_WORLD_PROTOCOL_VERSION,
        seq: 0,
        world,
        editor_status: EditorStatus::default(),
    };
    BrowserEditorWorld::from_json(&serde_json::to_string(&state).unwrap()).unwrap()
}

#[wasm_bindgen_test]
fn streamed_scripts_and_hierarchy_stay_in_sync() -> anyhow::Result<()> {
    let stored = stored_world()?;
    let root = stored.rootObjectId;
    let group = stored.objectOfId.keys().max().unwrap() + 1;
    let script = group + 1;
    let mut world = browser_world(stored);
    let added = EditorServerDelta {
        seq: 1,
        editor_status: EditorStatus::default(),
        object_deltas: vec![
            (
                group,
                ObjectDelta::Add(ObjectAddDelta {
                    name: "Streamed group".into(),
                    objectBeingAdded: ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
                    parentId: root,
                    initSubitemRelativeLocation3D: None,
                    teleportCount: 0,
                }),
            ),
            (
                script,
                ObjectDelta::Add(ObjectAddDelta {
                    name: "Streamed script".into(),
                    objectBeingAdded: ObjectBeingAdded::Script(ScriptInfo {
                        objectLocation: ObjectLocation::Server,
                    }),
                    parentId: group,
                    initSubitemRelativeLocation3D: None,
                    teleportCount: 0,
                }),
            ),
        ],
        script_raw_code_of_id: HashMap::from([(script, "const first = 1;".into())]),
    };
    let output: serde_json::Value = serde_json::from_str(&world.apply_deltas(&bincode::serialize(&added)?).unwrap())?;
    // Serialized straight from the world, so every f32 keeps its short form, as the browser receives it.
    assert_eq!(
        output["world"],
        serde_json::from_str::<serde_json::Value>(&serde_json::to_string(world.world.stored_world())?)?
    );
    assert_eq!(output["seq"], 1);
    assert_eq!(output["editor_status"], serde_json::to_value(&added.editor_status)?);
    assert_eq!(world.world.stored_world().scriptRawCodeOfId[&script], "const first = 1;");
    assert_eq!(world.world.object_and_descendant_ids(group), HashSet::from([group, script]));

    let modified = EditorServerDelta {
        seq: 1,
        editor_status: EditorStatus::default(),
        object_deltas: vec![(
            script,
            ObjectDelta::Modify(ObjectModifyDelta {
                modifiedObjectInfo: None,
                modifiedPhysicsToggles3D: None,
                modifiedPhysicsInfo3D: None,
                modifiedPrimitiveSettings: None,
                modifiedParent: Some(root),
                modifiedName: Some("Moved script".into()),
            }),
        )],
        script_raw_code_of_id: HashMap::from([(script, "const second = 2;".into())]),
    };
    world.apply_deltas(&bincode::serialize(&modified)?).unwrap();
    assert_eq!(world.world.stored_world().scriptRawCodeOfId[&script], "const second = 2;");
    assert_eq!(world.world.stored_world().objectOfId[&script].name, "Moved script");
    assert_eq!(world.world.object_and_descendant_ids(group), HashSet::from([group]));
    assert!(world.world.stored_world().childrenOfId[&root].contains(&script));
    // The resulting state must remain a valid snapshot for both React and the renderer.
    StoredWorldRenderable::new(serde_json::from_str(&serde_json::to_string(world.world.stored_world())?)?)?;

    let removed = EditorServerDelta {
        seq: 1,
        editor_status: EditorStatus::default(),
        object_deltas: vec![(script, ObjectDelta::Remove(())), (group, ObjectDelta::Remove(()))],
        script_raw_code_of_id: HashMap::new(),
    };
    world.apply_deltas(&bincode::serialize(&removed)?).unwrap();
    assert!(!world.world.stored_world().scriptRawCodeOfId.contains_key(&script));
    assert!(!world.world.stored_world().childrenOfId[&root].contains(&script));
    assert!(world.world.object_and_descendant_ids(group).is_empty());
    Ok(())
}

#[wasm_bindgen_test]
fn streamed_physics_preserves_other_authored_data() -> anyhow::Result<()> {
    let stored = stored_world()?;
    let id = *stored.physicsInfo3DOfId.keys().next().unwrap();
    let mut physics = stored.physicsInfo3DOfId[&id].clone();
    physics.location.position += glam::Vec3::new(3.0, 5.0, 7.0);
    let mut world = browser_world(stored.clone());
    let batch = EditorServerDelta {
        seq: 1,
        editor_status: EditorStatus::default(),
        object_deltas: vec![(
            id,
            ObjectDelta::Modify(ObjectModifyDelta {
                modifiedObjectInfo: None,
                modifiedPhysicsToggles3D: None,
                modifiedPhysicsInfo3D: Some(NetworkedPhysicsInfo3D {
                    coords: NetworkedCoords::WorldCoords(physics.clone()),
                    teleportCount: 0,
                }),
                modifiedPrimitiveSettings: None,
                modifiedParent: None,
                modifiedName: None,
            }),
        )],
        script_raw_code_of_id: HashMap::new(),
    };
    world.apply_deltas(&bincode::serialize(&batch)?).unwrap();
    assert_eq!(world.world.stored_world().physicsInfo3DOfId[&id].location.position, physics.location.position);
    assert_eq!(world.world.stored_world().scriptRawCodeOfId, stored.scriptRawCodeOfId);
    for (id, object) in &stored.objectOfId {
        if let ObjectInfo::Clock(_) = object.info {
            assert_eq!(serde_json::to_value(&world.world.stored_world().objectOfId[id])?, serde_json::to_value(object)?);
        }
    }
    Ok(())
}

#[wasm_bindgen_test]
fn incompatible_initial_state_is_rejected() -> anyhow::Result<()> {
    for protocol_version in [EDITOR_WORLD_PROTOCOL_VERSION - 1, EDITOR_WORLD_PROTOCOL_VERSION + 1] {
        let state = EditorState {
            protocol_version,
            seq: 0,
            world: stored_world()?,
            editor_status: EditorStatus::default(),
        };
        assert!(BrowserEditorWorld::from_json(&serde_json::to_string(&state)?).is_err());
    }
    Ok(())
}

#[wasm_bindgen_test]
fn invalid_batches_fail_before_changing_the_world() -> anyhow::Result<()> {
    let stored = stored_world()?;
    let before = serde_json::to_value(&stored)?;
    let root = stored.rootObjectId;
    let mut world = browser_world(stored);
    let batch = EditorServerDelta {
        seq: 1,
        editor_status: EditorStatus::default(),
        object_deltas: vec![(root, ObjectDelta::Remove(()))],
        script_raw_code_of_id: HashMap::new(),
    };
    let mut bytes = bincode::serialize(&batch)?;
    bytes.pop();
    assert!(world.apply_deltas(&bytes).is_err());
    assert!(world.apply_deltas(&[0, 1, 2]).is_err());
    assert_eq!(serde_json::to_value(world.world.stored_world())?, before);
    Ok(())
}

#[wasm_bindgen_test]
fn status_only_batches_return_no_world_and_preserve_browser_state() -> anyhow::Result<()> {
    let stored = stored_world()?;
    let before = serde_json::to_value(&stored)?;
    let mut world = browser_world(stored);
    for is_running in [true, true, false, false] {
        let editor_status = EditorStatus {
            is_playtest_running: is_running,
            is_chat_running: is_running,
            history_of_session: Default::default(),
        };
        let batch = EditorServerDelta {
            seq: 1,
            object_deltas: vec![],
            script_raw_code_of_id: HashMap::new(),
            editor_status: editor_status.clone(),
        };
        let output: serde_json::Value = serde_json::from_str(&world.apply_deltas(&bincode::serialize(&batch)?).unwrap())?;
        assert_eq!(output, serde_json::json!({ "seq": 1, "world": null, "editor_status": editor_status }));
        assert_eq!(serde_json::to_value(world.world.stored_world())?, before);
    }
    Ok(())
}
