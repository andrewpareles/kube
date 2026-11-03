mod helpers;

use client::play_runner::snapshot_buffer::SnapshotInterpolationBuffer;
use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    object_types::{LiveObject, LocalGroupInfo, ObjectBeingAdded, ObjectId, ObjectInfo},
    stored_world_type::StoredWorld,
};
use helpers::fallback_camera_location_for_tests;
use shared::{
    game::world::{World, WorldControlMode},
    protocol::messages::{MessageNum, ObjectDelta, ObjectModifyDelta, ServerSnapshotMessage},
};
use std::{collections::HashMap, sync::Once};

fn ensure_test_logger() {
    static INIT_LOGGER: Once = Once::new();
    INIT_LOGGER.call_once(|| {
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .is_test(true)
            .try_init();
    });
}

// These tests only need the root object, so they build a tiny world instead of depending on the default editor world file.
fn make_test_world() -> World {
    let root_object_id = 1;
    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: fallback_camera_location_for_tests(),
        objectOfId: HashMap::from([(
            root_object_id,
            LiveObject {
                id: root_object_id,
                name: "root".to_string(),
                info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
            },
        )]),
        rootObjectId: root_object_id,
        childrenOfId: HashMap::from([(root_object_id, Vec::new())]),
        scriptRawCodeOfId: HashMap::new(),
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };
    let stored_world_string = serde_json::to_string(&stored_world).expect("test stored world should serialize");
    World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None).expect("world should parse")
}

fn build_parent_change_snapshot(message_num: MessageNum, live_server_sim_ms: f64, object_id: ObjectId, new_parent_id: ObjectId) -> ServerSnapshotMessage {
    let mut delta_of_object_id = HashMap::new();
    delta_of_object_id.insert(
        object_id,
        ObjectDelta::Modify(ObjectModifyDelta {
            modifiedObjectInfo: None,
            modifiedPhysicsToggles3D: None,
            modifiedPhysicsInfo3D: None,
            modifiedParent: Some(new_parent_id),
            modifiedPrimitiveSettings: None,
            modifiedName: None,
        }),
    );
    ServerSnapshotMessage {
        message_num,
        live_server_sim_ms,
        deltaOfObjectId: delta_of_object_id,
        deltaOfObjectId2: HashMap::new(),
    }
}

fn build_empty_snapshot(message_num: MessageNum, live_server_sim_ms: f64) -> ServerSnapshotMessage {
    ServerSnapshotMessage {
        message_num,
        live_server_sim_ms,
        deltaOfObjectId: HashMap::new(),
        deltaOfObjectId2: HashMap::new(),
    }
}

fn add_local_group(world: &mut World, object_id: ObjectId, name: &str, parent_id: ObjectId) {
    world.addObjectWithId(object_id, name.to_string(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), parent_id);
}

// ---------------------- tests ----------------------

#[test]
fn parent_change_applied_after_message_is_popped() {
    ensure_test_logger();

    let mut world = make_test_world();
    let root_id = world.getRootObjectId();

    // add two groups: A and B, both children of root
    let a_id: ObjectId = 100;
    let b_id: ObjectId = 101;
    add_local_group(&mut world, a_id, "group-a", root_id);
    add_local_group(&mut world, b_id, "group-b", root_id);
    assert_eq!(world.parentOfId.get(&b_id).copied(), Some(root_id));

    let mut snapshot_buffer = SnapshotInterpolationBuffer::new();

    // receive a snapshot that changes B's parent to A
    snapshot_buffer.process_new_snapshot(build_parent_change_snapshot(1, 100.0, b_id, a_id));

    // before popping: parent changes should NOT be returned (message still in buffer)
    let (_, changes) = snapshot_buffer.pop_due_valid_parent_deltas(&world);
    assert!(changes.is_empty(), "parent changes should not be returned before the message is popped");

    // pop the snapshot
    let _ = snapshot_buffer.pop_due_snapshots(100.0);

    // after popping: the parent change should now be returned
    let (_, changes) = snapshot_buffer.pop_due_valid_parent_deltas(&world);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0], (b_id, a_id));

    // apply it and verify the world updated
    world.setParent(b_id, a_id).expect("setParent should succeed");
    assert_eq!(world.parentOfId.get(&b_id).copied(), Some(a_id));
}

#[test]
fn parent_change_delayed_until_its_message_is_popped_when_received_out_of_order() {
    ensure_test_logger();

    let mut world = make_test_world();
    let root_id = world.getRootObjectId();

    let a_id: ObjectId = 100;
    let b_id: ObjectId = 101;
    add_local_group(&mut world, a_id, "group-a", root_id);
    add_local_group(&mut world, b_id, "group-b", root_id);

    let mut snapshot_buffer = SnapshotInterpolationBuffer::new();

    // receive message 2 first (out of order) — it has the parent change
    snapshot_buffer.process_new_snapshot(build_parent_change_snapshot(2, 200.0, b_id, a_id));
    // then receive message 1 (no parent change, just a filler)
    snapshot_buffer.process_new_snapshot(build_empty_snapshot(1, 100.0));

    // pop message 1 only (wall clock 150ms is past message 1 at 100ms but before message 2 at 200ms)
    let _ = snapshot_buffer.pop_due_snapshots(150.0);

    // parent change from message 2 should NOT be returned yet, since message 2 is still in the buffer
    let (_, changes) = snapshot_buffer.pop_due_valid_parent_deltas(&world);
    assert!(
        changes.is_empty(),
        "parent change from message 2 should not be applied yet — message 2 has not been popped"
    );

    // now pop message 2
    let _ = snapshot_buffer.pop_due_snapshots(250.0);

    // now the parent change should be returned
    let (_, changes) = snapshot_buffer.pop_due_valid_parent_deltas(&world);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0], (b_id, a_id));
}
