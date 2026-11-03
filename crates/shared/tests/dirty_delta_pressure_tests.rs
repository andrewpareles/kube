mod helpers;

use game_types::{
    consts::SERVER_SIM_AND_SEND_SECS,
    object_types::{BrickInfo, ObjectBeingAdded, ObjectId, PhysicsInfo3D, VisualSettings},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_ball, make_world, physics_toggles};
use shared::game::{
    physics::SetPhysicsInfo3D,
    world::{World, WorldControlMode},
};
use std::collections::{HashMap, HashSet};

// The server marks an object dirty - and so sends it a pose delta - for every rigid body rapier reports as
// active. "Active" means awake, not moved: a body stays awake while anything in its collision island is
// awake. These tests pin how much of a tick's delta budget that costs, because every wasted delta pushes a
// real mover (a spinning platform) out of the packet and into a later one.

const DT_SECS: f32 = SERVER_SIM_AND_SEND_SECS;
const NUM_TICKS: usize = 120;

fn load_game_35() -> World {
    let world_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../game-35-latest.kube");
    let stored_world_string = std::fs::read_to_string(world_path).expect("game-35-latest.kube should be readable");
    World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None).expect("game-35 should load")
}

fn object_id_named(world: &World, name: &str) -> Option<ObjectId> {
    world.objectOfId.iter().find(|(_, object)| object.name == name).map(|(&object_id, _)| object_id)
}

fn pose_of(world: &World, object_id: ObjectId) -> Option<(Vec3, Vec3)> {
    let PhysicsInfo3D { location, .. } = world.getObjectPhysicsInfo3D(object_id)?;
    Some((location.position, location.quaternion.to_scaled_axis()))
}

struct TickCensus {
    dirty_count: usize,
    moved_count: usize,
    awake_but_still: Vec<ObjectId>,
}

// Steps the world one tick and reports what it would have sent versus what actually moved.
fn step_and_take_census(world: &mut World, tick_index: usize, prev_pose_of_id: &mut HashMap<ObjectId, (Vec3, Vec3)>) -> TickCensus {
    world
        .step_server(1, DT_SECS, tick_index as f64 * DT_SECS as f64 * 1000.0)
        .expect("server step should succeed");

    let dirty_object_ids: Vec<ObjectId> = world.flushDirtyObjects().modifiedObjectFlags.keys().copied().collect();

    let mut moved_count = 0;
    let mut awake_but_still = Vec::new();
    for &object_id in &dirty_object_ids {
        let Some(pose) = pose_of(world, object_id) else {
            continue;
        };
        match prev_pose_of_id.insert(object_id, pose) {
            Some((prev_position, prev_scaled_axis)) if (prev_position - pose.0).length() > 1e-6 || (prev_scaled_axis - pose.1).length() > 1e-6 => {
                moved_count += 1;
            }
            Some(_) => awake_but_still.push(object_id),
            None => {}
        }
    }

    TickCensus {
        dirty_count: dirty_object_ids.len(),
        moved_count,
        awake_but_still,
    }
}

// An unpushable brick handed an angular velocity keeps moving on its own, so it must be dirty every single
// tick. This is the baseline the starvation tests below measure against.
#[test]
fn a_spinning_platform_is_dirty_every_tick() {
    let mut world = load_game_35();
    let platform_id = object_id_named(&world, "PlatformScripted").expect("world 35 should have PlatformScripted");

    world.setPhysicsInfo3D(
        platform_id,
        SetPhysicsInfo3D {
            position: None,
            rotation: None,
            velocity: None,
            angularVelocity: Some(Vec3::new(0.0, 60.0_f32.to_radians(), 0.0)),
        },
        true,
    );

    let mut prev_pose_of_id = HashMap::new();
    let mut ticks_dirty = 0;
    for tick_index in 0..NUM_TICKS {
        world
            .step_server(1, DT_SECS, tick_index as f64 * DT_SECS as f64 * 1000.0)
            .expect("server step should succeed");
        if world.flushDirtyObjects().modifiedObjectFlags.contains_key(&platform_id) {
            ticks_dirty += 1;
        }
        prev_pose_of_id.insert(platform_id, pose_of(&world, platform_id).expect("platform has physics"));
    }

    assert_eq!(ticks_dirty, NUM_TICKS, "a platform that never stops moving must be sent every tick");
}

// A world where only the platform moves must cost about one delta a tick. It used to cost 219, because the
// sweep reported every AWAKE body and a whole level of motionless scenery stays awake forever, which pushed the
// platform that did move into a packet ~14 ticks later and made it visibly snap on the client.
#[test]
fn idle_scenery_costs_no_deltas() {
    let mut world = load_game_35();
    let platform_id = object_id_named(&world, "PlatformScripted").expect("world 35 should have PlatformScripted");

    world.setPhysicsInfo3D(
        platform_id,
        SetPhysicsInfo3D {
            position: None,
            rotation: None,
            velocity: None,
            angularVelocity: Some(Vec3::new(0.0, 60.0_f32.to_radians(), 0.0)),
        },
        true,
    );

    // long enough that anything rapier intends to put to sleep has had many seconds to do it
    const NUM_SETTLING_TICKS: usize = 600;

    let mut prev_pose_of_id = HashMap::new();
    let mut final_census = None;
    for tick_index in 0..NUM_SETTLING_TICKS {
        let census = step_and_take_census(&mut world, tick_index, &mut prev_pose_of_id);
        if tick_index % 60 == 0 || tick_index == NUM_SETTLING_TICKS - 1 {
            println!(
                "t={:>4} ({:>5.2}s): sent {:>3} deltas, {:>3} moved, {:>3} awake-but-still",
                tick_index,
                tick_index as f32 * DT_SECS,
                census.dirty_count,
                census.moved_count,
                census.awake_but_still.len()
            );
        }
        final_census = Some(census);
    }

    let final_census = final_census.expect("the loop runs at least once");
    let still_but_sent: HashSet<ObjectId> = final_census.awake_but_still.iter().copied().collect();

    // The stragglers are bodies rapier can never sleep: it only sleeps a kinematic body at *exactly* zero
    // velocity, and it derives that velocity from next_position * position.inverse(), which leaves float residue
    // on a rotated pose. World 35 has three (two silos and a watchtower, all rotated cylinders). Their names and
    // residue print here so a regression says which bodies came back and whether this is why.
    for &object_id in still_but_sent.iter().take(10) {
        let residue = world
            .getObjectPhysicsInfo3D(object_id)
            .map(|physics_info| physics_info.movement.velocity.length().max(physics_info.movement.angularVelocity.length()));
        println!(
            "  still-but-sent: {object_id} {:?} residue={residue:?}",
            world.getObject(object_id).map(|object| object.name.clone())
        );
    }

    // One UDP snapshot carries roughly 17 pose deltas, so what matters is that an idle world costs a small
    // fraction of a packet - not that it costs exactly nothing. At 219 the movers were being crowded out.
    const MAX_IDLE_DELTAS_PER_TICK: usize = 8;
    assert!(
        final_census.dirty_count <= MAX_IDLE_DELTAS_PER_TICK,
        "an idle world costs {} deltas a tick ({} of them for bodies that did not move), which crowds real movers out of the packet",
        final_census.dirty_count,
        still_but_sent.len(),
    );
}

// A rotated unpushable brick that nothing moves must never be sent. Rapier works out a kinematic body's velocity as
// next_position * position.inverse(), and on a rotated pose that leaves rounding error, so rapier reports a few mm/s
// for a brick that never moves and never lets it sleep. Game 298's race track had 1,216 of these, which filled every
// packet and left its golf ball sent about once a second. This is one of its rails, at its real place. An unpushable
// brick is only kinematic once something has moved it, so this one is moved once onto its real place first.
#[test]
fn a_still_rotated_brick_costs_no_deltas() {
    let mut world = make_world().expect("world should build");
    let brick_id: ObjectId = 2;
    world.addObjectWithId(
        brick_id,
        "Rail_074".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings {
                size: Vec3::new(0.8, 2.3, 18.6),
            },
            PhysicsInfo3D::new(
                Vec3::new(-102.54018, 265.23276, -135.40819),
                Quat::from_xyzw(0.014859948, 0.92691445, -0.03691179, 0.37315723),
                Vec3::ZERO,
                Vec3::ZERO,
            ),
            physics_toggles(false, 1.0),
        ),
        1,
    );
    world.setPhysicsInfo3D(
        brick_id,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(-102.54018, 265.23276, -134.40819)),
            ..Default::default()
        },
        false,
    );

    // Its first step after being added counts as a move, like any body that just woke up.
    world.step_server(1, DT_SECS, 0.0).expect("server step should succeed");
    world.flushDirtyObjects();

    for tick_index in 1..NUM_TICKS {
        world
            .step_server(1, DT_SECS, tick_index as f64 * DT_SECS as f64 * 1000.0)
            .expect("server step should succeed");
        assert!(
            !world.flushDirtyObjects().modifiedObjectFlags.contains_key(&brick_id),
            "a brick that never moves was sent on tick {tick_index}"
        );
    }

    // Without rapier's rounding error this test would not be testing anything.
    let velocity = world.getObjectPhysicsInfo3D(brick_id).expect("brick has physics").movement.velocity;
    assert_ne!(velocity, Vec3::ZERO, "rapier no longer reports a rounding velocity for this brick");
}

// Whether a body moved is now read from its pose, so a body that really moves must still be sent every tick.
#[test]
fn a_falling_ball_is_dirty_every_tick() {
    let mut world = make_world().expect("world should build");
    let ball_id: ObjectId = 2;
    add_ball(&mut world, ball_id, 1, Vec3::new(0.0, 100.0, 0.0), true).expect("ball should add");
    world.flushDirtyObjects();

    for tick_index in 0..NUM_TICKS {
        world
            .step_server(1, DT_SECS, tick_index as f64 * DT_SECS as f64 * 1000.0)
            .expect("server step should succeed");
        assert!(
            world.flushDirtyObjects().modifiedObjectFlags.contains_key(&ball_id),
            "a falling ball was not sent on tick {tick_index}"
        );
    }
}
