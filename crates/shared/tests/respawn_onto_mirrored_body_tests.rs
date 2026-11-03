mod helpers;

use std::collections::VecDeque;

use game_types::object_types::{ObjectId, PhysicsInfo3D};
use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_brick, add_player, make_world, make_world_controlled_by_player};
use shared::game::physics::SetPhysicsInfo3D;
use shared::game::world::World;

// These run two worlds side by side, each owning one body and mirroring the other one at a network delay, the way
// the server and the clients do. A player is respawned while falling and lands on the body below.

const DT_SECS: f32 = 1.0 / 60.0;
const FLOOR_OBJECT_ID: ObjectId = 400;
const CRATE_OBJECT_ID: ObjectId = 401;
const PLAYER_A_OBJECT_ID: ObjectId = 410;
const CHARACTER_A_OBJECT_ID: ObjectId = 411;
const PLAYER_B_OBJECT_ID: ObjectId = 420;
const CHARACTER_B_OBJECT_ID: ObjectId = 421;
const RESPAWN_TICK: usize = 30;
const NUM_TICKS: usize = 10 * 60;

fn add_floor(world: &mut World) -> anyhow::Result<()> {
    let root_object_id = world.getRootObjectId();
    add_brick(world, FLOOR_OBJECT_ID, root_object_id, Vec3::new(0.0, -1.0, 0.0), false)?;
    world.setPrimitiveSettings(
        FLOOR_OBJECT_ID,
        PrimitiveSettings::Brick(BrickPrimitiveSettings {
            size: Vec3::new(20.0, 2.0, 20.0),
        }),
        true,
    )?;
    Ok(())
}

fn physics_of(world: &World, object_id: ObjectId) -> anyhow::Result<PhysicsInfo3D> {
    world
        .getObjectPhysicsInfo3D(object_id)
        .ok_or_else(|| anyhow::anyhow!("expected physics for {object_id}"))
}

// Hands each pose to the other side `delay_ticks` ticks after it was taken.
struct DelayLine {
    poses: VecDeque<PhysicsInfo3D>,
    delay_ticks: usize,
}

impl DelayLine {
    fn new(delay_ticks: usize) -> Self {
        Self {
            poses: VecDeque::new(),
            delay_ticks,
        }
    }

    fn push_and_read(&mut self, pose: PhysicsInfo3D) -> anyhow::Result<PhysicsInfo3D> {
        self.poses.push_back(pose);
        while self.poses.len() > self.delay_ticks + 1 {
            self.poses.pop_front();
        }
        self.poses.front().cloned().ok_or_else(|| anyhow::anyhow!("empty delay line"))
    }
}

// The brick below rests at a height of about 1. Sinking into the floor or getting launched both leave this band.
fn assert_stayed_on_the_floor(lowest_y: f32, highest_y: f32, label: &str) {
    assert!(
        lowest_y > 0.9 && highest_y < 1.1,
        "{label} left the floor: lowest {lowest_y}, highest {highest_y}"
    );
}

// The server owns a crate on the floor. A script respawns player A just above it while A is still falling, and the
// server ignores A's packets until A has the teleport, like shouldIgnoreForeignObjectDTO.
#[test]
fn a_player_respawned_onto_a_crate_does_not_push_it_through_the_floor() -> anyhow::Result<()> {
    let delay_to_client_ticks = 10;
    let delay_to_server_ticks = 3;
    let mut server = make_world()?;
    let mut client = make_world_controlled_by_player(PLAYER_A_OBJECT_ID)?;
    for world in [&mut server, &mut client] {
        add_floor(world)?;
        let root_object_id = world.getRootObjectId();
        add_brick(world, CRATE_OBJECT_ID, root_object_id, Vec3::new(0.0, 1.0, 0.0), true)?;
        add_brick(world, CHARACTER_A_OBJECT_ID, root_object_id, Vec3::new(5.0, 30.0, 0.0), true)?;
        add_player(world, PLAYER_A_OBJECT_ID, root_object_id, Some(CHARACTER_A_OBJECT_ID))?;
    }

    let teleport_arriving_tick = RESPAWN_TICK + delay_to_client_ticks;
    let ignore_packets_until_tick = teleport_arriving_tick + delay_to_server_ticks + 1;
    let mut teleport_pose = None;

    let mut crate_to_client = DelayLine::new(delay_to_client_ticks);
    let mut character_to_server = DelayLine::new(delay_to_server_ticks);
    let mut lowest_crate_y = f32::INFINITY;
    let mut highest_crate_y = f32::NEG_INFINITY;
    for tick in 1..=NUM_TICKS {
        let crate_pose = crate_to_client.push_and_read(physics_of(&server, CRATE_OBJECT_ID)?)?;
        client.setObjectNextPhysicsInfo3D(CRATE_OBJECT_ID, crate_pose)?;
        if tick == teleport_arriving_tick {
            let teleport_pose = teleport_pose.clone().ok_or_else(|| anyhow::anyhow!("expected the teleport to be sent"))?;
            client.forcePhysicsInfo3D(CHARACTER_A_OBJECT_ID, SetPhysicsInfo3D::from(teleport_pose), false);
        }
        client.step_physics(1, DT_SECS, 0.0, None)?;

        let character_pose = character_to_server.push_and_read(physics_of(&client, CHARACTER_A_OBJECT_ID)?)?;
        // the respawn tick's packets land before its scripts run
        if tick <= RESPAWN_TICK || tick >= ignore_packets_until_tick {
            server.setObjectNextPhysicsInfo3D(CHARACTER_A_OBJECT_ID, character_pose)?;
        }
        server.step_physics(1, DT_SECS, 0.0, None)?;
        if tick == RESPAWN_TICK {
            server.forcePhysicsInfo3D(
                CHARACTER_A_OBJECT_ID,
                SetPhysicsInfo3D {
                    position: Some(Vec3::new(0.0, 3.5, 0.0)),
                    ..Default::default()
                },
                false,
            );
            teleport_pose = Some(physics_of(&server, CHARACTER_A_OBJECT_ID)?);
        }

        let crate_y = physics_of(&server, CRATE_OBJECT_ID)?.location.position.y;
        lowest_crate_y = lowest_crate_y.min(crate_y);
        highest_crate_y = highest_crate_y.max(crate_y);
    }

    assert_stayed_on_the_floor(lowest_crate_y, highest_crate_y, "the crate");
    let copy_offset = physics_of(&server, CHARACTER_A_OBJECT_ID)?.location.position - physics_of(&client, CHARACTER_A_OBJECT_ID)?.location.position;
    assert!(copy_offset.length() < 0.01, "the server's copy of A must end up where A is: {copy_offset:?}");
    Ok(())
}

// Player A is respawned while falling and lands on player B. B's client applies the server's teleport of A with the
// velocity the server reported, and A's movement stops reaching B until the server hears back from A.
#[test]
fn a_player_respawned_onto_another_player_does_not_push_them_through_the_floor() -> anyhow::Result<()> {
    fn make_client(player_object_id: ObjectId) -> anyhow::Result<World> {
        let mut world = make_world_controlled_by_player(player_object_id)?;
        add_floor(&mut world)?;
        let root_object_id = world.getRootObjectId();
        add_brick(&mut world, CHARACTER_B_OBJECT_ID, root_object_id, Vec3::new(0.0, 1.0, 0.0), true)?;
        add_brick(&mut world, CHARACTER_A_OBJECT_ID, root_object_id, Vec3::new(5.0, 30.0, 0.0), true)?;
        add_player(&mut world, PLAYER_A_OBJECT_ID, root_object_id, Some(CHARACTER_A_OBJECT_ID))?;
        add_player(&mut world, PLAYER_B_OBJECT_ID, root_object_id, Some(CHARACTER_B_OBJECT_ID))?;
        Ok(world)
    }
    let delay_ticks = 10;
    let mut client_a = make_client(PLAYER_A_OBJECT_ID)?;
    let mut client_b = make_client(PLAYER_B_OBJECT_ID)?;

    let teleport_arriving_tick = RESPAWN_TICK + delay_ticks;
    let a_movement_paused_until_tick = teleport_arriving_tick + 2 * delay_ticks + 1;

    let mut b_to_a = DelayLine::new(delay_ticks);
    let mut a_to_b = DelayLine::new(delay_ticks);
    let mut lowest_b_y = f32::INFINITY;
    let mut highest_b_y = f32::NEG_INFINITY;
    for tick in 1..=NUM_TICKS {
        let b_pose = b_to_a.push_and_read(physics_of(&client_b, CHARACTER_B_OBJECT_ID)?)?;
        client_a.setObjectNextPhysicsInfo3D(CHARACTER_B_OBJECT_ID, b_pose)?;
        let a_pose = a_to_b.push_and_read(physics_of(&client_a, CHARACTER_A_OBJECT_ID)?)?;
        if tick == teleport_arriving_tick {
            let teleport_pose = PhysicsInfo3D::new(Vec3::new(0.0, 3.5, 0.0), Quat::IDENTITY, a_pose.movement.velocity, Vec3::ZERO);
            client_a.forcePhysicsInfo3D(CHARACTER_A_OBJECT_ID, SetPhysicsInfo3D::from(teleport_pose.clone()), false);
            client_b.forcePhysicsInfo3D(CHARACTER_A_OBJECT_ID, SetPhysicsInfo3D::from(teleport_pose), false);
        } else if tick < teleport_arriving_tick || tick >= a_movement_paused_until_tick {
            client_b.setObjectNextPhysicsInfo3D(CHARACTER_A_OBJECT_ID, a_pose)?;
        }
        client_a.step_physics(1, DT_SECS, 0.0, None)?;
        client_b.step_physics(1, DT_SECS, 0.0, None)?;

        let b_y = physics_of(&client_b, CHARACTER_B_OBJECT_ID)?.location.position.y;
        lowest_b_y = lowest_b_y.min(b_y);
        highest_b_y = highest_b_y.max(b_y);
    }

    assert_stayed_on_the_floor(lowest_b_y, highest_b_y, "player B");
    let copy_offset = physics_of(&client_b, CHARACTER_A_OBJECT_ID)?.location.position - physics_of(&client_a, CHARACTER_A_OBJECT_ID)?.location.position;
    assert!(copy_offset.length() < 0.01, "B's copy of A must end up where A is: {copy_offset:?}");
    Ok(())
}
