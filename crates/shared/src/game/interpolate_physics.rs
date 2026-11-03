use crate::game::timeline::Timeline;
use game_types::{
    consts::CLIENT_INTERPOLATION_WINDOW_MS,
    object_delta::{NetworkedCoords, NetworkedPhysicsInfo3D},
    object_types::{InterpolationStyle, Location3D, PhysicsInfo3D},
};
use glam::{Quat, Vec3};

// How far a predicted orientation is allowed to run ahead over one snapshot span before the prediction is thrown
// away and the two poses are just slerped.
//
// A prediction is built as an axis-angle, which wraps at half a turn: past that it comes back around describing a
// rotation the wrong way, and the slerp between the two wrapped endpoints sweeps a path with no relation to either
// pose - a platform snapping to a wild orientation for a frame. Half a turn is where it breaks, so predictions stop
// being trusted at a quarter turn.
//
// This is reachable in practice because a kinematic body's velocity is not measured, it is derived: rapier divides
// however far an authored transform moved by one step's dt. A frame that stalls and then skips the world clock
// forward writes a whole backlog of motion as a single step, and the velocity that falls out describes motion that
// never happened. Anything genuinely turning more than a quarter turn per span is far too fast to read on screen
// anyway, so the fallback costs nothing real.
const MAX_PREDICTED_ROTATION_RADIANS: f32 = std::f32::consts::FRAC_PI_2;

// alpha = between 0 and 1
// dt_secs is unused for lerp but kept so callers can swap interpolation modes with the same function signature.
pub fn interpolate_between_physics_info_3Ds_lerp(physics_info_0: &PhysicsInfo3D, physics_info_1: &PhysicsInfo3D, _dt_secs: f32, alpha: f32) -> PhysicsInfo3D {
    let position = physics_info_0.location.position.lerp(physics_info_1.location.position, alpha);
    let quaternion = physics_info_0.location.quaternion.slerp(physics_info_1.location.quaternion, alpha);
    let velocity = physics_info_0.movement.velocity.lerp(physics_info_1.movement.velocity, alpha);
    let angular_velocity = physics_info_0.movement.angularVelocity.lerp(physics_info_1.movement.angularVelocity, alpha);
    PhysicsInfo3D::new(position, quaternion, velocity, angular_velocity)
}

pub fn interpolate_between_location_3Ds_lerp(location_0: &Location3D, location_1: &Location3D, alpha: f32) -> Location3D {
    let position = location_0.position.lerp(location_1.position, alpha);
    let quaternion = location_0.quaternion.slerp(location_1.quaternion, alpha);
    Location3D { position, quaternion }
}

// alpha = between 0 and 1
// dt_secs = the true time between the two events
// function to interpolate between (p0, m0) and (p1, m1), at time t - this just evalutes a deg 3 polynomial with endpoints p0 and p1 and slopes m0 and m1 at time t
pub fn interpolate_between_physics_info_3Ds_hermite(physics_info_0: &PhysicsInfo3D, physics_info_1: &PhysicsInfo3D, dt_secs: f32, alpha: f32) -> PhysicsInfo3D {
    fn hermite_vec3(p0: Vec3, p1: Vec3, m0: Vec3, m1: Vec3, t: f32) -> Vec3 {
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        p0 * h00 + m0 * h10 + p1 * h01 + m1 * h11
    }
    fn hermite_vec3_derivative(p0: Vec3, p1: Vec3, m0: Vec3, m1: Vec3, t: f32) -> Vec3 {
        let t2 = t * t;
        let h00_prime = 6.0 * t2 - 6.0 * t;
        let h10_prime = 3.0 * t2 - 4.0 * t + 1.0;
        let h01_prime = -6.0 * t2 + 6.0 * t;
        let h11_prime = 3.0 * t2 - 2.0 * t;
        p0 * h00_prime + m0 * h10_prime + p1 * h01_prime + m1 * h11_prime
    }

    let p0 = physics_info_0.location.position;
    let p1 = physics_info_1.location.position;
    let m0 = physics_info_0.movement.velocity * dt_secs;
    let m1 = physics_info_1.movement.velocity * dt_secs;
    let position = hermite_vec3(p0, p1, m0, m1, alpha);
    let velocity = hermite_vec3_derivative(p0, p1, m0, m1, alpha) / dt_secs;

    let dt0_secs = dt_secs * alpha;
    let dt1_secs = dt_secs * (1.0 - alpha);
    let q0 = physics_info_0.location.quaternion;
    let q1 = physics_info_1.location.quaternion;
    // Both endpoints are judged over the whole span rather than their own share of it, so the choice cannot flip
    // partway through and pop.
    let predicted_rotation_0_radians = (physics_info_0.movement.angularVelocity * dt_secs).length();
    let predicted_rotation_1_radians = (physics_info_1.movement.angularVelocity * dt_secs).length();
    let can_predict_rotation = predicted_rotation_0_radians.max(predicted_rotation_1_radians) <= MAX_PREDICTED_ROTATION_RADIANS;
    let quaternion = if can_predict_rotation {
        let q0_pred = Quat::from_scaled_axis(physics_info_0.movement.angularVelocity * dt0_secs) * q0;
        let q1_pred = Quat::from_scaled_axis(-physics_info_1.movement.angularVelocity * dt1_secs) * q1;
        q0_pred.slerp(q1_pred, alpha)
    } else {
        q0.slerp(q1, alpha)
    };
    let angularVelocity = physics_info_0.movement.angularVelocity.lerp(physics_info_1.movement.angularVelocity, alpha);

    PhysicsInfo3D::new(position, quaternion, velocity, angularVelocity)
}

// Where a body would be dt_secs after this pose if it kept its velocity and spin.
pub fn extrapolate_physics_info_3D(physics_info_3d: &PhysicsInfo3D, dt_secs: f32) -> PhysicsInfo3D {
    let position = physics_info_3d.location.position + physics_info_3d.movement.velocity * dt_secs;
    let rotation = physics_info_3d.movement.angularVelocity * dt_secs;
    let quaternion = if rotation.length() <= MAX_PREDICTED_ROTATION_RADIANS {
        Quat::from_scaled_axis(rotation) * physics_info_3d.location.quaternion
    } else {
        physics_info_3d.location.quaternion
    };
    PhysicsInfo3D::new(
        position,
        quaternion,
        physics_info_3d.movement.velocity,
        physics_info_3d.movement.angularVelocity,
    )
}

// The last pose at or before this time, blended into the next one unless the object jumped to it.
// Poses come every tick while something moves, so two more than CLIENT_INTERPOLATION_WINDOW_MS apart had it standing still in between, and aren't blended either.
pub fn get_interpolated_pose(pose_timeline: &Timeline<NetworkedPhysicsInfo3D>, est_server_sim_ms: f64) -> Option<NetworkedPhysicsInfo3D> {
    let (from_est_live_server_sim_ms, from_pose) = pose_timeline.find_last_entry_at_or_before(est_server_sim_ms)?;
    let Some((to_est_live_server_sim_ms, to_pose)) = pose_timeline.find_first_entry_after(est_server_sim_ms) else {
        return Some(from_pose.clone());
    };
    let is_jump = to_pose.teleportCount != from_pose.teleportCount;
    let gap_ms = (to_est_live_server_sim_ms - from_est_live_server_sim_ms) as f32;
    let is_too_far_apart = gap_ms > CLIENT_INTERPOLATION_WINDOW_MS;
    if is_jump || is_too_far_apart {
        return Some(from_pose.clone());
    }

    let alpha = InterpolationStyle::Linear.computeAlpha(0.0, gap_ms, (est_server_sim_ms - from_est_live_server_sim_ms) as f32);
    let coords = match (&from_pose.coords, &to_pose.coords) {
        (NetworkedCoords::WorldCoords(from_physics_info_3d), NetworkedCoords::WorldCoords(to_physics_info_3d)) => {
            let dt_secs = gap_ms / 1000.0;
            NetworkedCoords::WorldCoords(interpolate_between_physics_info_3Ds_lerp(
                from_physics_info_3d,
                to_physics_info_3d,
                dt_secs,
                alpha,
            ))
        }
        (NetworkedCoords::SubitemRelativeCoords(from_location_3d), NetworkedCoords::SubitemRelativeCoords(to_location_3d)) => {
            NetworkedCoords::SubitemRelativeCoords(interpolate_between_location_3Ds_lerp(from_location_3d, to_location_3d, alpha))
        }
        _ => from_pose.coords.clone(),
    };
    Some(NetworkedPhysicsInfo3D {
        coords,
        teleportCount: from_pose.teleportCount,
    })
}
