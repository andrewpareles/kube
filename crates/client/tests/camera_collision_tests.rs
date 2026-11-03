#[path = "../../shared/tests/helpers/world_test_helpers.rs"]
mod world_test_helpers;

use client::play_runner::{
    input_snapshot::{PlayInputState, getCameraPosition, getCameraTarget},
    mesh_camera_shapes::MeshCameraShapes,
    play::{computeEffectiveCameraZoom, getBlockedCameraZoom},
    play_renderer::computeNextRenderedZoomFromBlocking,
};
use game_types::{
    consts::CAMERA_SIZE,
    data_types::{DataId, MeshData, MeshVertexInternal, SkinningInfoPerVertex},
    object_types::{
        BrickInfo, ClockInfo, ClockState, InterpolationStyle, Location3D, MeshInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectLocation, Orientation3D,
        PhysicsInfo3D, PhysicsToggles3D, PlayerInfo, SkAnimationInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo, VisualSettings,
        sk_animator::SkAnimatorInfo,
    },
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use render_core::camera::{DEFAULT_CAMERA_FOV_DEGREES, build_camera_view_proj_matrix, build_ray_from_view_proj_ndc};
use shared::game::world::World;
use std::sync::Arc;
use world_test_helpers::{add_ball, add_player, add_rigid_group, add_skeleton, make_world};

// Use real world colliders so these tests cover the camera's query and object filtering together.
fn world_with_obstacle(position: Vec3, size: Vec3, is_walkthrough: bool, opacity: f32) -> anyhow::Result<World> {
    let mut world = make_world()?;
    world.addObjectWithId(
        2,
        "obstacle".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings {
                    opacity,
                    ..VisualSettings::TYPICAL_OPAQUE
                },
            },
            BrickPrimitiveSettings { size },
            PhysicsInfo3D::new(position, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: is_walkthrough,
                isPushable: false,
                density: 1.0,
            },
        ),
        world.getRootObjectId(),
    );
    let _ = world.physics.step(0.0);
    Ok(world)
}

#[test]
fn camera_detects_ceiling_edge_that_center_ray_misses() -> anyhow::Result<()> {
    let world = world_with_obstacle(Vec3::new(0.0, 1.0, -3.0), Vec3::new(10.0, 1.0, 2.0), false, 1.0)?;
    let head = Vec3::new(0.0, 0.45, 0.0);
    let orientation = Orientation3D::Z();
    let requested_zoom = 3.0;
    assert!(world.castRay(head, -Vec3::Z * requested_zoom, 1.0, false, &|_| false).is_none());

    let zoom = computeEffectiveCameraZoom(
        head,
        None,
        &orientation,
        requested_zoom,
        &world,
        &mut MeshCameraShapes::new(),
        &|_| None,
        0.0,
        0.0,
    );
    assert!(zoom < requested_zoom);
    let camera = getCameraPosition(head, &orientation, zoom);
    let view_proj = build_camera_view_proj_matrix(camera, head, 16.0 / 9.0, DEFAULT_CAMERA_FOV_DEGREES);
    for x in [-1.0, 1.0] {
        let (near_corner, _) = build_ray_from_view_proj_ndc(view_proj, x, 1.0).unwrap();
        assert!(near_corner.z > -2.0, "near-plane corner should stay in front of the ceiling edge");
    }
    Ok(())
}

#[test]
fn slow_head_bob_keeps_near_plane_below_ceiling() -> anyhow::Result<()> {
    let world = world_with_obstacle(Vec3::new(0.0, 3.0, 0.0), Vec3::new(20.0, 1.0, 20.0), false, 1.0)?;
    let orientation = Orientation3D {
        theta: 0.0,
        phi: std::f32::consts::PI / 6.0,
    };
    let requested_zoom = 5.0;
    let mut zoom = requested_zoom;
    let mut is_recovering = false;
    let mut blocked_elapsed_secs = 0.0;

    // Render one frame ahead of the physics sample, including the existing obstruction delay and recovery smoothing.
    for frame in 0..240 {
        let time_secs = frame as f32 / 60.0;
        let head = Vec3::Y * (-0.5 * (time_secs * std::f32::consts::PI).cos());
        let blocked_zoom = computeEffectiveCameraZoom(
            head,
            None,
            &orientation,
            requested_zoom,
            &world,
            &mut MeshCameraShapes::new(),
            &|_| None,
            0.0,
            0.0,
        );
        (zoom, is_recovering, blocked_elapsed_secs) =
            computeNextRenderedZoomFromBlocking(requested_zoom, zoom, is_recovering, blocked_zoom, blocked_elapsed_secs, 1.0 / 60.0);
        let rendered_head = Vec3::Y * (-0.5 * ((time_secs + 1.0 / 60.0) * std::f32::consts::PI).cos());
        let camera = getCameraPosition(rendered_head, &orientation, zoom);
        let view_proj = build_camera_view_proj_matrix(camera, rendered_head, 16.0 / 9.0, DEFAULT_CAMERA_FOV_DEGREES);
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                let (near_corner, _) = build_ray_from_view_proj_ndc(view_proj, x, y).unwrap();
                assert!(near_corner.y < 2.5, "near-plane corner crossed the ceiling at frame {frame}: {near_corner:?}");
            }
        }
    }
    Ok(())
}

#[test]
fn camera_wall_distance_includes_radius_once() -> anyhow::Result<()> {
    let world = world_with_obstacle(Vec3::new(0.0, 0.0, -3.0), Vec3::splat(2.0), false, 1.0)?;
    let zoom = computeEffectiveCameraZoom(
        Vec3::ZERO,
        None,
        &Orientation3D::Z(),
        4.0,
        &world,
        &mut MeshCameraShapes::new(),
        &|_| None,
        0.0,
        0.0,
    );
    assert!((zoom - (2.0 - CAMERA_SIZE)).abs() < 0.0001);
    Ok(())
}

#[test]
fn camera_still_ignores_walkthrough_transparent_and_player_objects() -> anyhow::Result<()> {
    for (is_walkthrough, opacity, is_player) in [(true, 1.0, false), (false, 0.5, false), (false, 1.0, true)] {
        let mut world = world_with_obstacle(Vec3::new(0.0, 0.0, -3.0), Vec3::splat(2.0), is_walkthrough, opacity)?;
        if is_player {
            let root = world.getRootObjectId();
            add_player(&mut world, 3, root, Some(2))?;
        }
        assert_eq!(
            computeEffectiveCameraZoom(
                Vec3::ZERO,
                None,
                &Orientation3D::Z(),
                4.0,
                &world,
                &mut MeshCameraShapes::new(),
                &|_| None,
                0.0,
                0.0
            ),
            4.0
        );
    }
    Ok(())
}

// Set head and body pointers independently, as avatar scripts do when creating players.
fn add_player_with_head(world: &mut World, player_object_id: ObjectId, head_object_id: ObjectId, character_object_id: Option<ObjectId>) {
    world.addObjectWithId(
        player_object_id,
        format!("player-{player_object_id}"),
        ObjectBeingAdded::Player(PlayerInfo {
            username: format!("player-{player_object_id}"),
            userid: None,
            preferredColor: None,
            equippedItemId: None,
            headObjectId: Some(ObjectIdPointer(head_object_id)),
            characterObjectId: character_object_id.map(ObjectIdPointer),
        }),
        world.getRootObjectId(),
    );
}

#[test]
fn camera_ignores_own_head_with_separate_shared_or_no_controlled_body() -> anyhow::Result<()> {
    for character_object_id in [Some(4), Some(2), None] {
        let mut world = world_with_obstacle(Vec3::ZERO, Vec3::splat(2.0), false, 1.0)?;
        let root = world.getRootObjectId();
        add_ball(&mut world, 4, root, Vec3::X * 5.0, false)?;
        add_player_with_head(&mut world, 3, 2, character_object_id);
        let _ = world.physics.step(0.0);

        let input = PlayInputState::new();
        assert_eq!(
            getBlockedCameraZoom(&world, 3, &input, Vec3::ZERO, &mut MeshCameraShapes::new(), &|_| None, 0.0, 0.0),
            input.desired_camera_zoom
        );
    }
    Ok(())
}

#[test]
fn camera_ignores_every_part_of_own_heads_rigid_group() -> anyhow::Result<()> {
    for character_object_id in [Some(5), Some(2), None] {
        let mut world = make_world()?;
        let root = world.getRootObjectId();
        add_rigid_group(&mut world, 2, root, Vec3::ZERO, None)?;
        add_ball(&mut world, 4, 2, Vec3::ZERO, false)?;
        add_ball(&mut world, 6, 2, Vec3::new(0.0, 0.0, -3.0), false)?;
        add_ball(&mut world, 5, root, Vec3::X * 5.0, false)?;
        add_player_with_head(&mut world, 3, 4, character_object_id);
        let _ = world.physics.step(0.0);

        let input = PlayInputState::new();
        assert_eq!(
            getBlockedCameraZoom(&world, 3, &input, Vec3::ZERO, &mut MeshCameraShapes::new(), &|_| None, 0.0, 0.0),
            input.desired_camera_zoom
        );
    }
    Ok(())
}

#[test]
fn camera_still_detects_walls_and_other_players_heads_beyond_own_head() -> anyhow::Result<()> {
    for is_other_player_head in [false, true] {
        let mut world = world_with_obstacle(Vec3::new(0.0, 0.0, -3.0), Vec3::splat(2.0), false, 1.0)?;
        let root = world.getRootObjectId();
        add_ball(&mut world, 4, root, Vec3::ZERO, false)?;
        add_player_with_head(&mut world, 3, 4, None);
        if is_other_player_head {
            add_player_with_head(&mut world, 5, 2, None);
        }
        let _ = world.physics.step(0.0);

        let zoom = getBlockedCameraZoom(&world, 3, &PlayInputState::new(), Vec3::ZERO, &mut MeshCameraShapes::new(), &|_| None, 0.0, 0.0);
        assert!((zoom - (2.0 - CAMERA_SIZE)).abs() < 0.0001);
    }
    Ok(())
}

// A script's camera offset moves the camera along its own right, up and forward, whichever way the camera faces.
#[test]
fn the_camera_offset_follows_the_cameras_own_directions() {
    let shoulder_offset = Vec3::new(1.5, 0.0, 0.0);
    // looking along +Z, right is -X, and looking along +X, right is +Z
    let looking_along_z = Orientation3D { theta: 0.0, phi: 0.0 };
    let looking_along_x = Orientation3D {
        theta: std::f32::consts::FRAC_PI_2,
        phi: 0.0,
    };
    assert!(getCameraTarget(Vec3::ZERO, &looking_along_z, shoulder_offset).abs_diff_eq(Vec3::new(-1.5, 0.0, 0.0), 1e-5));
    assert!(getCameraTarget(Vec3::ZERO, &looking_along_x, shoulder_offset).abs_diff_eq(Vec3::new(0.0, 0.0, 1.5), 1e-5));

    // looking down, right stays level, up tilts with the camera, and forward is where it looks
    let looking_down = Orientation3D {
        theta: 0.0,
        phi: std::f32::consts::FRAC_PI_4,
    };
    let forward = looking_down.to_unit_direction();
    assert!(getCameraTarget(Vec3::ZERO, &looking_down, shoulder_offset).abs_diff_eq(Vec3::new(-1.5, 0.0, 0.0), 1e-5));
    let up = getCameraTarget(Vec3::ZERO, &looking_down, Vec3::Y);
    assert!(up.dot(forward).abs() < 1e-5 && up.y > 0.0 && (up.length() - 1.0).abs() < 1e-5, "up: {up}");
    assert!(getCameraTarget(Vec3::ZERO, &looking_down, Vec3::Z).abs_diff_eq(forward, 1e-5));
}

// The camera looks at the offset spot, so a wall between that spot and the camera pulls it in even when the head's own view is clear.
#[test]
fn an_offset_camera_is_pulled_in_by_walls_behind_the_offset_spot() -> anyhow::Result<()> {
    let mut world = world_with_obstacle(Vec3::new(-1.5, 0.0, -3.0), Vec3::new(1.0, 2.0, 2.0), false, 1.0)?;
    let root = world.getRootObjectId();
    add_ball(&mut world, 4, root, Vec3::ZERO, false)?;
    add_player_with_head(&mut world, 3, 4, None);
    let _ = world.physics.step(0.0);

    let input = PlayInputState::new();
    assert_eq!(
        getBlockedCameraZoom(&world, 3, &input, Vec3::ZERO, &mut MeshCameraShapes::new(), &|_| None, 0.0, 0.0),
        input.desired_camera_zoom
    );
    let zoom = getBlockedCameraZoom(&world, 3, &input, Vec3::new(1.5, 0.0, 0.0), &mut MeshCameraShapes::new(), &|_| None, 0.0, 0.0);
    assert!((zoom - (2.0 - CAMERA_SIZE)).abs() < 0.0001, "zoom: {zoom}");
    Ok(())
}

// -------------------------- Meshes block the camera --------------------------

const MESH_DATA_ID: DataId = 7;

// A cube from -1 to 1 on every axis, as 12 triangles.
fn cube_positions_and_indices(first_index: u32) -> (Vec<Vec3>, Vec<u32>) {
    let positions = (0..8)
        .map(|corner| Vec3::new([-1.0, 1.0][corner & 1], [-1.0, 1.0][(corner >> 1) & 1], [-1.0, 1.0][(corner >> 2) & 1]))
        .collect();
    let indices = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]]
        .into_iter()
        .flat_map(|[a, b, c, d]| [a, b, c, a, c, d])
        .map(|corner| first_index + corner)
        .collect();
    (positions, indices)
}

fn static_cube_mesh_data() -> MeshData {
    let (positions, indices) = cube_positions_and_indices(0);
    let vertices = positions
        .into_iter()
        .map(|position| MeshVertexInternal {
            position,
            normal: Vec3::Y,
            uv: Vec2::ZERO,
            tangent: Vec4::X,
            skinning: (),
        })
        .collect();
    MeshData::StaticMeshData { vertices, indices }
}

// Two cubes in the same place in the rest pose. The first cube moves with the joint named first, and the second with the other.
fn skinned_two_cube_mesh_data(joint_names: [&str; 2]) -> MeshData {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for joint_index in 0..2 {
        let (positions, cube_indices) = cube_positions_and_indices(vertices.len() as u32);
        vertices.extend(positions.into_iter().map(|position| MeshVertexInternal {
            position,
            normal: Vec3::Y,
            uv: Vec2::ZERO,
            tangent: Vec4::X,
            skinning: SkinningInfoPerVertex {
                jointIndices: [joint_index, 0, 0, 0],
                jointWeights: [1.0, 0.0, 0.0, 0.0],
            },
        }));
        indices.extend(cube_indices);
    }
    MeshData::SkinnedMeshData {
        vertices,
        indices,
        jointNameOfIdx: joint_names.map(String::from).to_vec(),
        inverseBindMatrixOfJointName: joint_names.map(|joint_name| (joint_name.to_string(), Mat4::IDENTITY.to_cols_array())).into(),
    }
}

fn add_mesh_following(world: &mut World, physical_object_id: ObjectId, scale: Vec3, opacity: f32, follow_skeleton_id: Option<ObjectId>) {
    world.addObjectWithId(
        10,
        "mesh".to_string(),
        ObjectBeingAdded::Mesh(MeshInfo {
            meshDataId: MESH_DATA_ID,
            scale,
            material: None,
            followSkeletonId: follow_skeleton_id.map(ObjectIdPointer),
            visualSettings: VisualSettings {
                opacity,
                ..VisualSettings::TYPICAL_OPAQUE
            },
            physicalObjectId: Some(ObjectIdPointer(physical_object_id)),
        }),
        world.getRootObjectId(),
    );
}

fn add_joint(world: &mut World, object_id: ObjectId, joint_name: &str, rest_position: Vec3, skeleton_object_id: ObjectId) {
    world.addObjectWithId(
        object_id,
        joint_name.to_string(),
        ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: Location3D {
                position: rest_position,
                quaternion: Quat::IDENTITY,
            },
        }),
        skeleton_object_id,
    );
}

// The camera looks along +Z from the head, so it moves toward -Z.
fn zoom_with_mesh_data(world: &World, mesh_data: &Arc<MeshData>, head: Vec3, camera_target_object_id: Option<ObjectId>, live_client_sim_ms: f64) -> f32 {
    computeEffectiveCameraZoom(
        head,
        camera_target_object_id,
        &Orientation3D::Z(),
        4.0,
        world,
        &mut MeshCameraShapes::new(),
        &|data_id| (data_id == MESH_DATA_ID).then(|| Arc::clone(mesh_data)),
        0.0,
        live_client_sim_ms,
    )
}

// A mesh blocks the camera where its triangles are drawn, however it is stretched, while the invisible collider it follows doesn't.
#[test]
fn a_mesh_blocks_the_camera_at_its_triangles_not_at_its_invisible_collider() -> anyhow::Result<()> {
    let mesh_data = Arc::new(static_cube_mesh_data());
    for (scale, expected_zoom) in [(Vec3::ONE, 2.0 - CAMERA_SIZE), (Vec3::new(3.0, 3.0, 0.25), 2.75 - CAMERA_SIZE)] {
        let mut world = world_with_obstacle(Vec3::new(0.0, 0.0, -3.0), Vec3::splat(0.1), false, 0.0)?;
        add_mesh_following(&mut world, 2, scale, 1.0, None);
        let zoom_before_download = computeEffectiveCameraZoom(
            Vec3::ZERO,
            None,
            &Orientation3D::Z(),
            4.0,
            &world,
            &mut MeshCameraShapes::new(),
            &|_| None,
            0.0,
            0.0,
        );
        assert_eq!(zoom_before_download, 4.0);
        let zoom = zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, None, 0.0);
        assert!((zoom - expected_zoom).abs() < 0.001, "scale {scale}: zoom {zoom}");
    }
    Ok(())
}

// A mesh blocks the camera however see-through it is, even when it is invisible.
#[test]
fn see_through_and_invisible_meshes_block_the_camera() -> anyhow::Result<()> {
    let mesh_data = Arc::new(static_cube_mesh_data());
    for opacity in [0.5, 0.0] {
        let mut world = world_with_obstacle(Vec3::new(0.0, 0.0, -3.0), Vec3::splat(0.1), false, 0.0)?;
        add_mesh_following(&mut world, 2, Vec3::ONE, opacity, None);
        let zoom = zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, None, 0.0);
        assert!((zoom - (2.0 - CAMERA_SIZE)).abs() < 0.001, "opacity {opacity}: zoom {zoom}");
    }
    Ok(())
}

#[test]
fn the_camera_ignores_meshes_on_walkthrough_objects_or_players() -> anyhow::Result<()> {
    let mesh_data = Arc::new(static_cube_mesh_data());
    // (is the collider walkthrough, is the collider a player's character, is the collider the head the camera follows)
    for (is_walkthrough, is_player_character, is_camera_target) in [(true, false, false), (false, true, false), (false, false, true)] {
        let mut world = world_with_obstacle(Vec3::new(0.0, 0.0, -3.0), Vec3::splat(0.1), is_walkthrough, 0.0)?;
        add_mesh_following(&mut world, 2, Vec3::ONE, 1.0, None);
        if is_player_character {
            let root = world.getRootObjectId();
            add_player(&mut world, 3, root, Some(2))?;
        }
        let camera_target_object_id = is_camera_target.then_some(2);
        assert_eq!(zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, camera_target_object_id, 0.0), 4.0);
    }
    Ok(())
}

// An imported model's mesh follows a rigid group of invisible colliders, and blocks the camera unless all of them are walkthrough.
#[test]
fn a_rigid_groups_mesh_blocks_the_camera_unless_all_its_parts_are_walkthrough() -> anyhow::Result<()> {
    let mesh_data = Arc::new(static_cube_mesh_data());
    for (is_walkthrough_of_part, expected_zoom) in [([false, true], 2.0 - CAMERA_SIZE), ([true, true], 4.0)] {
        let mut world = make_world()?;
        let root = world.getRootObjectId();
        add_rigid_group(&mut world, 2, root, Vec3::new(0.0, 0.0, -3.0), None)?;
        for (part_object_id, is_walkthrough) in [4, 5].into_iter().zip(is_walkthrough_of_part) {
            world.addObjectWithId(
                part_object_id,
                format!("part-{part_object_id}"),
                ObjectBeingAdded::Brick(
                    BrickInfo {
                        material: None,
                        visualSettings: VisualSettings {
                            opacity: 0.0,
                            ..VisualSettings::TYPICAL_OPAQUE
                        },
                    },
                    BrickPrimitiveSettings { size: Vec3::splat(0.1) },
                    PhysicsInfo3D::new(Vec3::new(0.0, 0.0, -3.0), Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
                    PhysicsToggles3D {
                        isWalkthrough: is_walkthrough,
                        isPushable: false,
                        density: 1.0,
                    },
                ),
                2,
            );
        }
        let _ = world.physics.step(0.0);
        add_mesh_following(&mut world, 2, Vec3::ONE, 1.0, None);

        let zoom = zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, None, 0.0);
        assert!(
            (zoom - expected_zoom).abs() < 0.001,
            "walkthrough parts {is_walkthrough_of_part:?}: zoom {zoom}"
        );
    }
    Ok(())
}

// An animated mesh is cut into one piece per joint, and each piece blocks the camera where its joint holds it, not where it rests.
#[test]
fn an_animated_meshs_pieces_block_the_camera_where_their_joints_hold_them() -> anyhow::Result<()> {
    let mut world = world_with_obstacle(Vec3::ZERO, Vec3::splat(0.1), false, 0.0)?;
    let root = world.getRootObjectId();
    add_skeleton(&mut world, 20, root)?;
    add_joint(&mut world, 21, "aside", Vec3::new(5.0, 0.0, 0.0), 20);
    add_joint(&mut world, 22, "behind", Vec3::new(0.0, 0.0, -3.0), 20);
    add_mesh_following(&mut world, 2, Vec3::ONE, 1.0, Some(20));

    // In the rest pose both cubes surround the head. Posed, only the second cube is in the camera's way.
    let mesh_data = Arc::new(skinned_two_cube_mesh_data(["aside", "behind"]));
    let zoom = zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, None, 0.0);
    assert!((zoom - (2.0 - CAMERA_SIZE)).abs() < 0.001, "zoom {zoom}");
    Ok(())
}

// The camera poses an animated mesh at the tick's time, so it is blocked where the animation has moved the mesh by then.
#[test]
fn an_animated_mesh_blocks_the_camera_where_its_animation_has_moved_it() -> anyhow::Result<()> {
    let mut world = world_with_obstacle(Vec3::ZERO, Vec3::splat(0.1), false, 0.0)?;
    let root = world.getRootObjectId();
    add_skeleton(&mut world, 20, root)?;
    add_joint(&mut world, 21, "aside", Vec3::new(5.0, 0.0, 0.0), 20);
    add_joint(&mut world, 22, "behind", Vec3::ZERO, 20);
    add_mesh_following(&mut world, 2, Vec3::ONE, 1.0, Some(20));
    world.addObjectWithId(
        30,
        "clock".to_string(),
        ObjectBeingAdded::Clock(ClockInfo {
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Client,
            },
            shouldLoop: false,
            speed: 1.0,
        }),
        root,
    );
    world.addObjectWithId(
        31,
        "animation".to_string(),
        ObjectBeingAdded::SkAnimation(SkAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Step,
        }),
        root,
    );
    for (keyframe_object_id, time, position) in [(32, 0.0, Vec3::ZERO), (34, 1.0, Vec3::new(0.0, 0.0, -3.0))] {
        world.addObjectWithId(
            keyframe_object_id,
            format!("keyframe-{keyframe_object_id}"),
            ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time }),
            31,
        );
        world.addObjectWithId(
            keyframe_object_id + 1,
            format!("pose-{keyframe_object_id}"),
            ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
                jointName: "behind".to_string(),
                localPose: Location3D {
                    position,
                    quaternion: Quat::IDENTITY,
                },
            }),
            keyframe_object_id,
        );
    }
    world.addObjectWithId(
        36,
        "animator".to_string(),
        ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: Some(ObjectIdPointer(20)),
            clockId: Some(ObjectIdPointer(30)),
            skAnimationId: Some(ObjectIdPointer(31)),
            priority: 1,
            weight: 100.0,
            weightTransition: None,
        }),
        root,
    );

    // At the start the second cube surrounds the head, so the camera stops at its inside. A second later it is behind the head.
    let mesh_data = Arc::new(skinned_two_cube_mesh_data(["aside", "behind"]));
    let zoom_at_start = zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, None, 0.0);
    assert!((zoom_at_start - (1.0 - CAMERA_SIZE)).abs() < 0.001, "zoom at start {zoom_at_start}");
    let zoom_after_a_second = zoom_with_mesh_data(&world, &mesh_data, Vec3::ZERO, None, 1000.0);
    assert!(
        (zoom_after_a_second - (2.0 - CAMERA_SIZE)).abs() < 0.001,
        "zoom after a second {zoom_after_a_second}"
    );
    Ok(())
}

// Like camera_detects_ceiling_edge_that_center_ray_misses: the whole camera ball has to clear a mesh's edge, not just a thin ray.
#[test]
fn camera_detects_mesh_ceiling_edge_that_center_ray_misses() -> anyhow::Result<()> {
    let mut world = world_with_obstacle(Vec3::new(0.0, 1.0, -3.0), Vec3::splat(0.1), false, 0.0)?;
    add_mesh_following(&mut world, 2, Vec3::new(5.0, 0.5, 1.0), 1.0, None);
    let mesh_data = Arc::new(static_cube_mesh_data());
    let head = Vec3::new(0.0, 0.45, 0.0);
    let orientation = Orientation3D::Z();
    let requested_zoom = 3.0;

    let zoom = computeEffectiveCameraZoom(
        head,
        None,
        &orientation,
        requested_zoom,
        &world,
        &mut MeshCameraShapes::new(),
        &|data_id| (data_id == MESH_DATA_ID).then(|| Arc::clone(&mesh_data)),
        0.0,
        0.0,
    );
    assert!(zoom < requested_zoom);
    let camera = getCameraPosition(head, &orientation, zoom);
    let view_proj = build_camera_view_proj_matrix(camera, head, 16.0 / 9.0, DEFAULT_CAMERA_FOV_DEGREES);
    for x in [-1.0, 1.0] {
        let (near_corner, _) = build_ray_from_view_proj_ndc(view_proj, x, 1.0).unwrap();
        assert!(near_corner.z > -2.0, "near-plane corner should stay in front of the mesh's edge");
    }
    Ok(())
}
