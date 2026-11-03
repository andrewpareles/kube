mod helpers;

use game_types::object_types::ObjectId;
use glam::Vec3;
use helpers::world_test_helpers::{add_ball, add_player, make_world, make_world_controlled_by_player};
use shared::game::{
    jsRustConversions::{ValueWithPath, rustUpdateObject},
    physics::{SetOrForce, SetPhysicsInfo3D},
    world::World,
};

const PLAYER_OBJECT_ID: ObjectId = 90;
const CHARACTER_OBJECT_ID: ObjectId = 91;
const SCENERY_OBJECT_ID: ObjectId = 92;

fn move_to(world: &mut World, object_id: ObjectId, position: Vec3, should_server_mark_dirty: bool) {
    world.forcePhysicsInfo3D(
        object_id,
        SetPhysicsInfo3D {
            position: Some(position),
            ..Default::default()
        },
        should_server_mark_dirty,
    );
}

// The server refuses a client's character physics while it still owes that client a change to the client's own
// player object or character - "owes" meaning the change is sitting in that user's dirty or unacked queue. These
// tests cover the world-side half of that: whether a given kind of change actually lands in those queues. Without
// it the server would either wrongly keep trusting a client it has moved, or wrongly ignore one it hasn't.

#[test]
fn a_server_move_of_a_players_character_is_owed_to_that_client() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    let _ = world.flushDirtyObjects(); // drop the adds so we only look at what the move dirties

    move_to(&mut world, CHARACTER_OBJECT_ID, Vec3::new(1000.0, 2.0, 0.0), true);

    let dirty = world.flushDirtyObjects();
    let character_flags = dirty
        .modifiedObjectFlags
        .get(&CHARACTER_OBJECT_ID)
        .expect("expected the moved character to be dirty");
    assert!(
        character_flags.modifiedPhysicsInfo3D,
        "the server's move has to reach the client, and it is that pending change the server waits on before it trusts the client's position again"
    );

    Ok(())
}

#[test]
fn client_side_moves_of_our_own_character_are_not_owed_to_anyone() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    let _ = world.flushDirtyObjects();

    // a client walking around is the normal case, and must never look like a change the server owes the client
    move_to(&mut world, CHARACTER_OBJECT_ID, Vec3::new(1000.0, 2.0, 0.0), true);

    let dirty = world.flushDirtyObjects();
    assert!(
        dirty.modifiedObjectFlags.is_empty(),
        "a client's own movement is not a server change, so nothing should be queued for replication"
    );

    Ok(())
}

#[test]
fn moving_unrelated_objects_does_not_stop_us_trusting_the_client() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_ball(&mut world, SCENERY_OBJECT_ID, root_object_id, Vec3::new(5.0, 2.0, 0.0), true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    let _ = world.flushDirtyObjects();

    move_to(&mut world, SCENERY_OBJECT_ID, Vec3::new(9.0, 2.0, 0.0), true);

    let dirty = world.flushDirtyObjects();
    assert!(
        !dirty.modifiedObjectFlags.contains_key(&CHARACTER_OBJECT_ID) && !dirty.modifiedObjectFlags.contains_key(&PLAYER_OBJECT_ID),
        "scenery is server-owned, and moving it says nothing about any player's character"
    );

    Ok(())
}

#[test]
fn handing_a_player_a_different_body_is_owed_to_that_client() -> anyhow::Result<()> {
    // This is what replaces the old explicit "does the client agree about which object it controls" check. The
    // client never names the object it controls; characterObjectId lives in the player object's info, so a body
    // swap is itself a pending change to the player object, and the client has to ack it before the server will
    // trust its position again. If that stopped being true, a client would keep control across a swap and write
    // the old body's position onto the new one.
    const REPLACEMENT_CHARACTER_OBJECT_ID: ObjectId = 93;

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_ball(&mut world, REPLACEMENT_CHARACTER_OBJECT_ID, root_object_id, Vec3::new(0.0, 2.0, 0.0), true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, root_object_id, Some(CHARACTER_OBJECT_ID))?;
    let _ = world.flushDirtyObjects();

    // go through the same conversion + marking a script write does (`player.characterObjectId = id`)
    let update_value = serde_json::json!(REPLACEMENT_CHARACTER_OBJECT_ID);
    let property_path = vec!["characterObjectId".to_string()];
    let object_modify_flags = rustUpdateObject(
        &mut world,
        PLAYER_OBJECT_ID,
        ValueWithPath::new(&update_value, &property_path),
        SetOrForce::Force,
    )?;
    world.serverMarkObjectAsModified(PLAYER_OBJECT_ID, object_modify_flags);

    let dirty = world.flushDirtyObjects();
    let player_flags = dirty
        .modifiedObjectFlags
        .get(&PLAYER_OBJECT_ID)
        .expect("expected the player object to be dirtied so the client learns which body it now controls");
    assert!(
        player_flags.modifiedObjectInfo,
        "characterObjectId replicates as ordinary object info, which is what makes the swap block the client's input until it is acked"
    );

    Ok(())
}
