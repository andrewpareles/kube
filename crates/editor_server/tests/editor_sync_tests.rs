use editor_server::editor_sync::EditorSync;
use game_types::{
    consts::EDITOR_WORLD_PROTOCOL_VERSION,
    editor_sync::{EditorClientDelta, EditorServerDelta, EditorStatus},
    object_types::{LocalGroupInfo, ObjectBeingAdded, ObjectIdPointer, ObjectInfo, ObjectKind, ObjectLocation, ScriptInfo},
    stored_world_type::StoredWorld,
};
use render_web::stored_world_renderable_impl::StoredWorldRenderable;
use serde_json::{Value, json};
use shared::game::world::{World, WorldControlMode};
use std::sync::Arc;
use tokio::sync::{Notify, broadcast::error::TryRecvError, watch};

// Use real World setters, channels, serialization, and the browser's world mutation code.
fn world() -> anyhow::Result<World> {
    let asset: Value = serde_json::from_str(include_str!("../../../tests/fixtures/asset-preview-animated-object.json"))?;
    World::fromStoredWorldString(&asset["Object"].to_string(), WorldControlMode::Editor, None)
}

fn comparable(mut world: StoredWorld) -> anyhow::Result<Value> {
    for children in world.childrenOfId.values_mut() {
        children.sort_unstable();
    }
    Ok(serde_json::to_value(world)?)
}

// Decode server output for these integration tests. Browser wire handling is tested through its WASM entry point.
fn apply_received_delta(client: &mut StoredWorldRenderable, bytes: &[u8]) -> anyhow::Result<EditorStatus> {
    let delta: EditorServerDelta = bincode::deserialize(bytes)?;
    client.apply_object_deltas(delta.object_deltas, delta.script_raw_code_of_id)?;
    Ok(delta.editor_status)
}

#[tokio::test]
async fn snapshots_and_shared_broadcasts_converge_for_browser_and_server_edits() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let script = world.addObject(
        "Script".into(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Server,
        }),
        root,
    );
    world.setScriptRawCode(script, "const initial = 1;".into());
    let initial_status = EditorStatus {
        is_playtest_running: true,
        is_chat_running: true,
        history_of_session: Default::default(),
    };
    let (editor_status_tx, editor_status_rx) = watch::channel(initial_status.clone());
    let sync = EditorSync::new(editor_status_rx);
    let (snapshot_a, mut changes_a) = sync.connect_client(&mut world)?;
    let (snapshot_b, mut changes_b) = sync.connect_client(&mut world)?;
    assert_eq!(snapshot_a.protocol_version, EDITOR_WORLD_PROTOCOL_VERSION);
    assert_eq!(snapshot_b.protocol_version, EDITOR_WORLD_PROTOCOL_VERSION);
    assert_eq!(snapshot_a.editor_status, initial_status);
    assert_eq!(comparable(snapshot_a.world.clone())?, comparable(snapshot_b.world.clone())?);
    let mut client_a = StoredWorldRenderable::new(snapshot_a.world)?;
    let mut client_b = StoredWorldRenderable::new(snapshot_b.world)?;
    assert_eq!(apply_received_delta(&mut client_a, &changes_a.try_recv()?)?, initial_status);
    assert_eq!(changes_a.try_recv(), Err(TryRecvError::Empty));

    // Browser edits and the same setters used by AI tools share the outgoing batch.
    let changes_notify_arc = Arc::new(Notify::new());
    world.changes_notify_arc = Some(changes_notify_arc.clone());
    EditorSync::apply_client_delta(
        &mut world,
        serde_json::from_value(json!({
            "objectId": script, "section": "scriptRawCodeOfId", "path": [], "value": "const browser = 2;"
        }))?,
    )?;
    tokio::time::timeout(std::time::Duration::from_secs(1), changes_notify_arc.notified()).await?;
    world.setObjectName(script, "Renamed by server".into())?;
    editor_status_tx.send_modify(|editor_status| editor_status.is_playtest_running = false);
    sync.broadcast_changes(&mut world)?;
    let bytes_a = changes_a.try_recv()?;
    let bytes_b = changes_b.try_recv()?;
    assert_eq!(bytes_a, bytes_b, "Each client must receive the same drained batch");
    let editor_status = apply_received_delta(&mut client_a, &bytes_a)?;
    apply_received_delta(&mut client_b, &bytes_b)?;
    assert_eq!(editor_status, editor_status_tx.borrow().clone());
    assert_eq!(client_a.stored_world().scriptRawCodeOfId[&script], "const browser = 2;");
    assert_eq!(comparable(client_a.stored_world().clone())?, comparable(world.toStoredWorld())?);
    assert_eq!(comparable(client_b.stored_world().clone())?, comparable(world.toStoredWorld())?);
    sync.broadcast_changes(&mut world)?;
    let repeated: EditorServerDelta = bincode::deserialize(&changes_a.try_recv()?)?;
    assert!(repeated.object_deltas.is_empty() && repeated.script_raw_code_of_id.is_empty());

    // A later server script edit replaces the browser save without a conflict protocol.
    world.setScriptRawCode(script, "const server = 3;".into());
    sync.broadcast_changes(&mut world)?;
    assert_eq!(apply_received_delta(&mut client_a, &changes_a.try_recv()?)?, editor_status_tx.borrow().clone());
    assert_eq!(client_a.stored_world().scriptRawCodeOfId[&script], "const server = 3;");
    Ok(())
}

#[test]
fn joining_flushes_pending_edits_to_existing_clients_without_repeating_them() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let (_editor_status_tx, editor_status_rx) = watch::channel(EditorStatus::default());
    let sync = EditorSync::new(editor_status_rx);
    let (_, mut existing_rx) = sync.connect_client(&mut world)?;
    let group = world.addObject("New group".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root);
    let script = world.addObject(
        "Nested script".into(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        group,
    );
    world.setScriptRawCode(script, "const nested = true;".into());
    let (snapshot, mut joining_rx) = sync.connect_client(&mut world)?;
    let added: EditorServerDelta = bincode::deserialize(&existing_rx.try_recv()?)?;
    let added_ids: Vec<_> = added.object_deltas.into_iter().map(|(id, _)| id).collect();
    assert_eq!(added_ids, [group, script], "Parents must arrive before their children");
    assert_eq!(joining_rx.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(snapshot.world.scriptRawCodeOfId[&script], "const nested = true;");
    let mut client = StoredWorldRenderable::new(snapshot.world)?;
    world.removeObject(group)?;
    sync.broadcast_changes(&mut world)?;
    apply_received_delta(&mut client, &joining_rx.try_recv()?)?;
    assert!(!client.stored_world().objectOfId.contains_key(&group));
    assert!(!client.stored_world().scriptRawCodeOfId.contains_key(&script));
    assert_eq!(comparable(client.stored_world().clone())?, comparable(world.toStoredWorld())?);
    Ok(())
}

#[test]
fn status_only_changes_preserve_world_and_reconnect_reads_current_flags() -> anyhow::Result<()> {
    let mut world = world()?;
    let (editor_status_tx, editor_status_rx) = watch::channel(EditorStatus::default());
    let sync = EditorSync::new(editor_status_rx);
    let (snapshot, mut changes_rx) = sync.connect_client(&mut world)?;
    let before = comparable(snapshot.world.clone())?;
    let mut client = StoredWorldRenderable::new(snapshot.world)?;
    for is_running in [true, false] {
        let editor_status = EditorStatus {
            is_playtest_running: is_running,
            is_chat_running: is_running,
            history_of_session: Default::default(),
        };
        editor_status_tx.send_replace(editor_status.clone());
        sync.broadcast_changes(&mut world)?;
        let changed_status = apply_received_delta(&mut client, &changes_rx.try_recv()?)?;
        assert_eq!(changed_status, editor_status.clone());
        assert_eq!(comparable(client.stored_world().clone())?, before);
        let (reconnected, _) = sync.connect_client(&mut world)?;
        assert_eq!(reconnected.editor_status, editor_status);
        assert_eq!(apply_received_delta(&mut client, &changes_rx.try_recv()?)?, editor_status);
    }
    Ok(())
}

#[tokio::test]
async fn browser_property_sections_use_world_setters_and_round_trip() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = *world._kindOfObjectId.get_keys(&ObjectKind::Ball).unwrap().iter().next().unwrap();
    let (_editor_status_tx, editor_status_rx) = watch::channel(EditorStatus::default());
    let sync = EditorSync::new(editor_status_rx);
    let (snapshot, mut changes_rx) = sync.connect_client(&mut world)?;
    let mut client = StoredWorldRenderable::new(snapshot.world)?;
    let changes_notify_arc = Arc::new(Notify::new());
    world.changes_notify_arc = Some(changes_notify_arc.clone());
    for (section, path, value) in [
        ("objectOfId", json!(["name"]), json!("Browser ball")),
        ("objectOfId", json!(["info", "Ball", "visualSettings", "opacity"]), json!(0.25)),
        ("physicsInfo3DOfId", json!(["location", "position"]), json!([11, 12, 13])),
        ("physicsToggles3DOfId", json!(["isPushable"]), json!(false)),
        ("primitiveSettingsOfId", json!(["Ball", "radius"]), json!(2.5)),
    ] {
        EditorSync::apply_client_delta(
            &mut world,
            serde_json::from_value(json!({ "objectId": ball, "section": section, "path": path, "value": value }))?,
        )?;
        tokio::time::timeout(std::time::Duration::from_secs(1), changes_notify_arc.notified()).await?;
        sync.broadcast_changes(&mut world)?;
        apply_received_delta(&mut client, &changes_rx.try_recv()?)?;
        assert_eq!(comparable(client.stored_world().clone())?, comparable(world.toStoredWorld())?, "{section}");
    }
    assert_eq!(
        world.getObject(ball)?.name,
        "Browser-ball",
        "Browser renames use the existing server name normalization"
    );
    assert_eq!(world.getObjectPhysicsInfo3D(ball).unwrap().location.position, glam::Vec3::new(11.0, 12.0, 13.0));
    assert!(!world.getObjectPhysicsToggles3D(ball).unwrap().isPushable);
    Ok(())
}

#[test]
fn invalid_browser_edits_leave_the_world_unchanged() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = *world._kindOfObjectId.get_keys(&ObjectKind::Ball).unwrap().iter().next().unwrap();
    let before = comparable(world.toStoredWorld())?;
    for (section, path, value) in [
        ("objectOfId", json!(["info", "Script", "objectLocation"]), json!("Client")),
        ("objectOfId", json!(["id"]), json!(0)),
        ("objectOfId", json!(["name"]), json!(42)),
        ("objectOfId", json!(["info", "Ball", "unknown"]), json!(true)),
        ("scriptRawCodeOfId", json!([]), json!("wrong kind")),
        ("physicsInfo3DOfId", json!(["location", "quaternion"]), json!([0, 0, 0, 0])),
        ("physicsToggles3DOfId", json!(["density"]), json!(-1)),
        ("primitiveSettingsOfId", json!(["Ball", "radius"]), json!(-1)),
        ("primitiveSettingsOfId", json!([]), json!({"Brick": {"size": [1, 1, 1]}})),
    ] {
        let delta: EditorClientDelta = serde_json::from_value(json!({ "objectId": ball, "section": section, "path": path, "value": value }))?;
        assert!(EditorSync::apply_client_delta(&mut world, delta).is_err(), "{section}");
        assert_eq!(comparable(world.toStoredWorld())?, before);
    }
    Ok(())
}

#[tokio::test]
async fn world_mutations_wake_publication_without_polling() -> anyhow::Result<()> {
    let mut world = world()?;
    let changes_notify_arc = Arc::new(Notify::new());
    world.changes_notify_arc = Some(changes_notify_arc.clone());
    let root = world.getRootObjectId();
    world.setObjectName(root, "Changed".into())?;
    tokio::time::timeout(std::time::Duration::from_secs(1), changes_notify_arc.notified()).await?;
    let id = world.addObject("Group".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root);
    tokio::time::timeout(std::time::Duration::from_secs(1), changes_notify_arc.notified()).await?;
    world.removeObject(id)?;
    tokio::time::timeout(std::time::Duration::from_secs(1), changes_notify_arc.notified()).await?;
    Ok(())
}

#[test]
fn reference_edits_keep_reverse_lookups_correct() -> anyhow::Result<()> {
    let mut world = world()?;
    let mesh = *world._kindOfObjectId.get_keys(&ObjectKind::Mesh).unwrap().iter().next().unwrap();
    let skeleton = *world._kindOfObjectId.get_keys(&ObjectKind::Skeleton).unwrap().iter().next().unwrap();
    let (_editor_status_tx, editor_status_rx) = watch::channel(EditorStatus::default());
    let sync = EditorSync::new(editor_status_rx);
    let (snapshot, mut changes_rx) = sync.connect_client(&mut world)?;
    let mut client = StoredWorldRenderable::new(snapshot.world)?;
    for value in [json!(skeleton), Value::Null] {
        EditorSync::apply_client_delta(
            &mut world,
            serde_json::from_value(json!({
                "objectId": mesh, "section": "objectOfId", "path": ["info", "Mesh", "followSkeletonId"], "value": value
            }))?,
        )?;
        let is_tracked = world._objectsThatPointToId.get(&skeleton).is_some_and(|ids| ids.contains(&mesh));
        assert_eq!(is_tracked, !value.is_null());
        sync.broadcast_changes(&mut world)?;
        apply_received_delta(&mut client, &changes_rx.try_recv()?)?;
        assert_eq!(comparable(client.stored_world().clone())?, comparable(world.toStoredWorld())?);
    }
    Ok(())
}

#[tokio::test]
async fn typed_info_setters_update_references_and_wake_publication() -> anyhow::Result<()> {
    let mut world = world()?;
    let mesh = *world._kindOfObjectId.get_keys(&ObjectKind::Mesh).unwrap().iter().next().unwrap();
    let skeleton = *world._kindOfObjectId.get_keys(&ObjectKind::Skeleton).unwrap().iter().next().unwrap();
    let changes_notify_arc = Arc::new(Notify::new());
    world.changes_notify_arc = Some(changes_notify_arc.clone());
    world.flushDirtyObjects();
    for follow_skeleton_id in [Some(ObjectIdPointer(skeleton)), None] {
        let ObjectInfo::Mesh(mut info) = world.getObject(mesh)?.info.clone() else {
            unreachable!()
        };
        info.followSkeletonId = follow_skeleton_id;
        world.setMeshInfo(mesh, info)?;
        tokio::time::timeout(std::time::Duration::from_secs(1), changes_notify_arc.notified()).await?;
        let is_tracked = world._objectsThatPointToId.get(&skeleton).is_some_and(|ids| ids.contains(&mesh));
        assert_eq!(is_tracked, follow_skeleton_id.is_some());
        let dirty = world.flushDirtyObjects();
        let delta = world.buildModifyDelta(mesh, &dirty.modifiedObjectFlags[&mesh])?;
        assert_eq!(
            serde_json::to_value(delta.modifiedObjectInfo)?,
            serde_json::to_value(&world.getObject(mesh)?.info)?
        );
    }
    Ok(())
}

#[test]
fn info_setters_reject_type_changes_and_missing_objects_without_mutation() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = *world._kindOfObjectId.get_keys(&ObjectKind::Ball).unwrap().iter().next().unwrap();
    let before = comparable(world.toStoredWorld())?;
    world.flushDirtyObjects();
    assert!(world.setObjectInfo(ball, ObjectInfo::LocalGroup(LocalGroupInfo {})).is_err());
    assert!(world.setObjectInfo(usize::MAX, ObjectInfo::LocalGroup(LocalGroupInfo {})).is_err());
    let script_info = ScriptInfo {
        objectLocation: ObjectLocation::Server,
    };
    let error = world.setScriptInfo(ball, script_info).unwrap_err();
    assert_eq!(error.to_string(), format!("setScriptInfo: object {ball} is not a script"));
    assert_eq!(comparable(world.toStoredWorld())?, before);
    let dirty = world.flushDirtyObjects();
    assert!(dirty.modifiedObjectFlags.is_empty() && dirty.addedObjectIds.is_empty() && dirty.removedObjectIds.is_empty());
    Ok(())
}

// An undo and redo can take an object away and bring it back before the next batch. Browsers must end
// up with it once, where it is, not without it.
#[test]
fn an_object_removed_and_added_back_before_a_batch_stays_in_browsers() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let (_editor_status_tx, editor_status_rx) = watch::channel(EditorStatus::default());
    let sync = EditorSync::new(editor_status_rx);
    let house = world.addObject("House".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), root);
    let door = world.addObject("Door".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), house);
    let (snapshot, mut changes_rx) = sync.connect_client(&mut world)?;
    let mut client = StoredWorldRenderable::new(snapshot.world)?;
    let house_add = world.buildAddDelta(house)?;
    let door_add = world.buildAddDelta(door)?;
    world.removeObject(house)?;
    world.addObjectWithIdFromDelta(house, house_add);
    world.addObjectWithIdFromDelta(door, door_add);
    sync.broadcast_changes(&mut world)?;
    apply_received_delta(&mut client, &changes_rx.try_recv()?)?;
    assert!(client.stored_world().objectOfId.contains_key(&house) && client.stored_world().objectOfId.contains_key(&door));
    assert_eq!(client.stored_world().childrenOfId[&root].iter().filter(|child| **child == house).count(), 1);
    assert_eq!(comparable(client.stored_world().clone())?, comparable(world.toStoredWorld())?);
    Ok(())
}

// A group carries its parts, so moving or turning it moves them in browsers too.
#[test]
fn a_moved_rigid_group_takes_its_parts_along_in_browsers() -> anyhow::Result<()> {
    let mut world = world()?;
    let group = *world._kindOfObjectId.get_keys(&ObjectKind::RigidGroup).unwrap().iter().min().unwrap();
    assert!(!world.physics.getRigidGroupSubitemIds(group).is_empty());
    let (_editor_status_tx, editor_status_rx) = watch::channel(EditorStatus::default());
    let sync = EditorSync::new(editor_status_rx);
    let (snapshot, mut changes_rx) = sync.connect_client(&mut world)?;
    let mut client = StoredWorldRenderable::new(snapshot.world)?;
    for (path, value) in [("position", json!([5.0, 0.5, 7.0])), ("quaternion", json!([0.0, 0.38268343, 0.0, 0.9238795]))] {
        let delta: EditorClientDelta = serde_json::from_value(json!({ "objectId": group, "section": "physicsInfo3DOfId", "path": ["location", path], "value": value }))?;
        EditorSync::apply_client_delta(&mut world, delta)?;
        sync.broadcast_changes(&mut world)?;
        apply_received_delta(&mut client, &changes_rx.try_recv()?)?;
        assert_eq!(comparable(client.stored_world().clone())?, comparable(world.toStoredWorld())?, "{path}");
    }
    Ok(())
}
