use game_types::{
    consts::CLIENT_INTERPOLATION_WINDOW_MS,
    object_types::{Location3D, PhysicsInfo3D},
};
use glam::{Quat, Vec3};
use shared::{
    game::{interpolate_physics::get_interpolated_pose, timeline::Timeline},
    protocol::messages::{NetworkedCoords, NetworkedPhysicsInfo3D},
};

fn pose_x(x: f32, teleport_count: u64) -> NetworkedPhysicsInfo3D {
    NetworkedPhysicsInfo3D {
        coords: NetworkedCoords::WorldCoords(PhysicsInfo3D::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO)),
        teleportCount: teleport_count,
    }
}

fn x_at(timeline: &Timeline<NetworkedPhysicsInfo3D>, est_server_sim_ms: f64) -> Option<f32> {
    match get_interpolated_pose(timeline, est_server_sim_ms)?.coords {
        NetworkedCoords::WorldCoords(physics_info_3d) => Some(physics_info_3d.location.position.x),
        NetworkedCoords::SubitemRelativeCoords(location_3d) => Some(location_3d.position.x),
    }
}

#[test]
fn a_pose_blends_toward_the_next_and_the_newest_is_held() {
    let mut timeline = Timeline::new();
    timeline.insert(100.0, pose_x(0.0, 0));
    timeline.insert(120.0, pose_x(2.0, 0));

    assert_eq!(x_at(&timeline, 99.0), None);
    assert_eq!(x_at(&timeline, 100.0), Some(0.0));
    assert_eq!(x_at(&timeline, 110.0), Some(1.0));
    assert_eq!(x_at(&timeline, 120.0), Some(2.0));
    assert_eq!(x_at(&timeline, 500.0), Some(2.0));
}

#[test]
fn a_jump_is_never_blended() {
    let mut timeline = Timeline::new();
    timeline.insert(100.0, pose_x(0.0, 0));
    timeline.insert(120.0, pose_x(50.0, 1));

    assert_eq!(x_at(&timeline, 119.0), Some(0.0));
    assert_eq!(get_interpolated_pose(&timeline, 120.0).map(|pose| pose.teleportCount), Some(1));
    assert_eq!(x_at(&timeline, 120.0), Some(50.0));
}

// poses come every tick while something moves, so a longer gap means it stood still
#[test]
fn poses_further_apart_than_the_window_are_not_blended() {
    let mut timeline = Timeline::new();
    timeline.insert(0.0, pose_x(0.0, 0));
    timeline.insert(CLIENT_INTERPOLATION_WINDOW_MS as f64, pose_x(2.0, 0));
    timeline.insert(1000.0, pose_x(6.0, 0));

    assert_eq!(x_at(&timeline, CLIENT_INTERPOLATION_WINDOW_MS as f64 / 2.0), Some(1.0));
    assert_eq!(x_at(&timeline, 999.0), Some(2.0));
    assert_eq!(x_at(&timeline, 1000.0), Some(6.0));
}

#[test]
fn rigid_group_parts_blend_where_they_sit_in_their_group() {
    let relative_x = |x: f32| NetworkedPhysicsInfo3D {
        coords: NetworkedCoords::SubitemRelativeCoords(Location3D {
            position: Vec3::new(x, 0.0, 0.0),
            quaternion: Quat::IDENTITY,
        }),
        teleportCount: 0,
    };
    let mut timeline = Timeline::new();
    timeline.insert(100.0, relative_x(0.0));
    timeline.insert(120.0, relative_x(2.0));

    assert_eq!(x_at(&timeline, 110.0), Some(1.0));
}

#[test]
fn entries_can_arrive_out_of_order_and_old_ones_are_forgotten() {
    let mut timeline = Timeline::new();
    timeline.insert(120.0, "b");
    timeline.insert(100.0, "a");
    timeline.insert(140.0, "c");
    assert_eq!(timeline.find_last_entry_at_or_before(110.0), Some(&(100.0, "a")));
    assert_eq!(timeline.find_first_entry_after(110.0), Some(&(120.0, "b")));

    timeline.forget_entries_before(120.0);
    assert_eq!(timeline.find_last_entry_at_or_before(110.0), None);
    assert_eq!(timeline.find_last_entry_at_or_before(130.0), Some(&(120.0, "b")));

    timeline.forget_entries_before(141.0);
    assert!(timeline.is_empty());
}
