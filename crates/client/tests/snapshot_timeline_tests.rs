mod helpers;

use client::play_runner::snapshot_buffer::SnapshotInterpolationBuffer;
use game_types::{
    consts::{CLIENT_INTERPOLATION_WINDOW_MS, STORED_WORLD_CURRENT_VERSION},
    object_types::{
        BrickInfo, LiveObject, LocalGroupInfo, ObjectBeingAdded, ObjectId, ObjectInfo, Orientation3D, PhysicsInfo3D, PhysicsToggles3D, VisualSettings,
    },
    physics_primitive_types::BrickPrimitiveSettings,
    stored_world_type::StoredWorld,
};
use glam::{Quat, Vec3};
use helpers::fallback_camera_location_for_tests;
use shared::{
    game::{
        physics::SetPhysicsInfo3D,
        world::{World, WorldControlMode},
    },
    protocol::messages::{
        ForeignCharacterDelta, ForeignPlayerDelta, MessageNum, NetworkedCoords, NetworkedPhysicsInfo3D, ObjectDelta, ObjectModifyDelta, ServerSnapshotMessage,
    },
};
use std::collections::HashMap;

const OBJECT_ID: ObjectId = 2;

fn snapshot(message_num: MessageNum, live_server_sim_ms: f64, delta: ObjectDelta) -> ServerSnapshotMessage {
    ServerSnapshotMessage {
        message_num,
        live_server_sim_ms,
        deltaOfObjectId: HashMap::from([(OBJECT_ID, delta)]),
        deltaOfObjectId2: HashMap::new(),
    }
}

fn physics_x(x: f32) -> PhysicsInfo3D {
    PhysicsInfo3D::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO)
}

fn foreign_character(est_live_server_sim_ms: f64, x: f32) -> ObjectDelta {
    ObjectDelta::ForeignCharacter(ForeignCharacterDelta {
        physicsInfo3D: physics_x(x),
        est_live_server_sim_ms,
        teleportCount: 0,
    })
}

fn server_pose(x: f32, teleport_count: u64) -> ObjectDelta {
    ObjectDelta::Modify(ObjectModifyDelta {
        modifiedObjectInfo: None,
        modifiedPhysicsToggles3D: None,
        modifiedPhysicsInfo3D: Some(NetworkedPhysicsInfo3D {
            coords: NetworkedCoords::WorldCoords(physics_x(x)),
            teleportCount: teleport_count,
        }),
        modifiedPrimitiveSettings: None,
        modifiedParent: None,
        modifiedName: None,
    })
}

// the pose the play loop applies to the object at this moment, as (x, teleport count)
fn interpolated_pose(buffer: &mut SnapshotInterpolationBuffer, est_dely_server_sim_ms: f64) -> Option<(f32, u64)> {
    buffer.forget_old_timeline_entries(est_dely_server_sim_ms);
    let (_, pose) = buffer
        .get_interpolated_poses(est_dely_server_sim_ms)
        .into_iter()
        .find(|(object_id, _)| *object_id == OBJECT_ID)?;
    let NetworkedCoords::WorldCoords(physics_info_3d) = pose.coords else {
        return None;
    };
    Some((physics_info_3d.location.position.x, pose.teleportCount))
}

#[test]
fn a_foreign_characters_pose_is_shown_at_its_own_time_not_its_snapshots() {
    let mut buffer = SnapshotInterpolationBuffer::new();
    buffer.process_new_snapshot(snapshot(1, 200.0, foreign_character(100.0, 1.0)));
    buffer.process_new_snapshot(snapshot(2, 220.0, foreign_character(120.0, 2.0)));

    assert_eq!(interpolated_pose(&mut buffer, 99.0), None);
    assert_eq!(interpolated_pose(&mut buffer, 110.0), Some((1.5, 0)));
}

// Before timelines, a popped snapshot put the character back at its older pose whenever the next one was late.
#[test]
fn a_late_next_pose_holds_the_newest_instead_of_going_back() {
    let mut buffer = SnapshotInterpolationBuffer::new();
    buffer.process_new_snapshot(snapshot(1, 130.0, foreign_character(100.0, 1.0)));
    buffer.process_new_snapshot(snapshot(2, 146.0, foreign_character(116.0, 2.0)));

    assert_eq!(interpolated_pose(&mut buffer, 120.0), Some((2.0, 0)));
    let _ = buffer.pop_due_snapshots(135.0);
    assert_eq!(interpolated_pose(&mut buffer, 135.0), Some((2.0, 0)));
}

#[test]
fn a_jump_is_not_blended() {
    let mut buffer = SnapshotInterpolationBuffer::new();
    buffer.process_new_snapshot(snapshot(1, 100.0, server_pose(1.0, 0)));
    buffer.process_new_snapshot(snapshot(2, 120.0, server_pose(50.0, 1)));
    buffer.process_new_snapshot(snapshot(3, 140.0, server_pose(52.0, 1)));

    assert_eq!(interpolated_pose(&mut buffer, 110.0), Some((1.0, 0)));
    assert_eq!(interpolated_pose(&mut buffer, 120.0), Some((50.0, 1)));
    assert_eq!(interpolated_pose(&mut buffer, 130.0), Some((51.0, 1)));
}

#[test]
fn camera_and_metadata_are_shown_with_the_pose_they_were_sent_with() {
    let mut buffer = SnapshotInterpolationBuffer::new();
    buffer.process_new_snapshot(snapshot(1, 1000.0, foreign_character(960.0, 1.0)));
    let foreign_player = ObjectDelta::ForeignPlayer(ForeignPlayerDelta {
        cameraOrientation: Orientation3D::Z(),
        playerMetadata: Some(r#""waving""#.to_string()),
        est_live_server_sim_ms: 960.0,
    });
    buffer.process_new_snapshot(snapshot(2, 1000.0, foreign_player));

    assert!(buffer.get_due_foreign_players(950.0).is_empty());
    assert_eq!(interpolated_pose(&mut buffer, 950.0), None);

    let foreign_players = buffer.get_due_foreign_players(965.0);
    assert_eq!(foreign_players[0].1.playerMetadata.as_deref(), Some(r#""waving""#));
    assert_eq!(interpolated_pose(&mut buffer, 965.0), Some((1.0, 0)));
}

// so a pose the server sent once doesn't keep fighting whatever else moves the object, like an animation
#[test]
fn a_finished_timeline_stops_being_applied() {
    let mut buffer = SnapshotInterpolationBuffer::new();
    buffer.process_new_snapshot(snapshot(1, 100.0, server_pose(1.0, 0)));

    let last_held_ms = 100.0 + CLIENT_INTERPOLATION_WINDOW_MS as f64;
    assert_eq!(interpolated_pose(&mut buffer, last_held_ms), Some((1.0, 0)));
    assert_eq!(interpolated_pose(&mut buffer, last_held_ms + 1.0), None);
}

// An object the server spawns and moves in the same tick arrives with its jump already counted, so its next pose is smoothed.
#[test]
fn an_added_object_arrives_with_its_teleport_count() -> anyhow::Result<()> {
    let mut server_world = make_test_world();
    let root_object_id = server_world.getRootObjectId();
    add_brick(&mut server_world, OBJECT_ID, root_object_id);
    server_world.forcePhysicsInfo3D(
        OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(Vec3::X * 10.0),
            ..Default::default()
        },
        true,
    );

    let mut client_world = make_test_world();
    client_world.addObjectWithIdFromDelta(OBJECT_ID, server_world.buildAddDelta(OBJECT_ID)?);
    assert_eq!(client_world.getTeleportCount(OBJECT_ID), 1);
    Ok(())
}

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

fn add_brick(world: &mut World, object_id: ObjectId, parent_id: ObjectId) {
    world.addObjectWithId(
        object_id,
        "brick".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings {
                size: Vec3::new(2.0, 2.0, 2.0),
            },
            physics_x(0.0),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        parent_id,
    );
}
