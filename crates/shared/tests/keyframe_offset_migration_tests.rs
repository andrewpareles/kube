use game_types::{
    consts::{DEHYDRATED_GLTF_CURRENT_VERSION, STORED_WORLD_CURRENT_VERSION},
    object_types::{InterpolationStyle, Location3D, SkAnimationInfo},
};
use glam::{Quat, Vec3};
use render_core::{
    computeSkeletonPose::computeLocalTransformOfJointAtTime,
    renderer_types::{RenderSkJoint, RenderSkKeyframe, RenderSkKeyframePose},
};
use serde_json::{Value, json};
use shared::{
    api::protocol_for_assets::Asset,
    game::{
        assets::AssetData,
        migrateStoredWorld::{StoredWorldKind, updateToVersion},
        world::{World, WorldControlMode},
    },
};

const INITIAL_DRAFT_WORLD: &str = include_str!("../../editor_server/initial-draft-gameworld.kube");

fn turned_rest_pose() -> Location3D {
    Location3D {
        position: Vec3::new(0.0, 1.2, 0.1),
        quaternion: Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.2),
    }
}

// The neck's whole pose at 0s and 0.5s, the way worlds saved it before keyframes held offsets.
fn saved_poses() -> [(f32, Location3D); 2] {
    [
        (
            0.0,
            Location3D {
                position: Vec3::new(0.05, 1.25, 0.1),
                quaternion: Quat::from_rotation_y(0.9),
            },
        ),
        (
            0.5,
            Location3D {
                position: Vec3::new(0.0, 1.4, 0.2),
                quaternion: Quat::from_rotation_z(0.3) * Quat::from_rotation_y(0.7),
            },
        ),
    ]
}

fn add_object(stored_world: &mut Value, parent_id: u64, id: u64, name: &str, info: Value) {
    stored_world["objectOfId"][id.to_string()] = json!({ "id": id, "name": name, "info": info });
    stored_world["childrenOfId"][id.to_string()] = json!([]);
    stored_world["childrenOfId"][parent_id.to_string()].as_array_mut().unwrap().push(json!(id));
}

// A world from before offsets with a "look" animation (id 6) at the root that keys the neck. Its later keyframe (poses 8) comes
// first among the animation's children, and its first keyframe holds pose 10.
fn world_with_look_animation() -> Value {
    let mut stored_world: Value = serde_json::from_str(INITIAL_DRAFT_WORLD).unwrap();
    stored_world["worldSchemaVersion"] = json!(83);
    add_object(
        &mut stored_world,
        1,
        6,
        "look",
        json!({ "SkAnimation": { "duration": 1.0, "interpolationStyle": "Linear" } }),
    );
    let [(first_time, first_pose), (later_time, later_pose)] = saved_poses();
    add_object(&mut stored_world, 6, 7, "later", json!({ "SkKeyframe": { "time": later_time } }));
    add_object(
        &mut stored_world,
        7,
        8,
        "neck",
        json!({ "SkKeyframePose": { "jointName": "neck", "localPose": later_pose } }),
    );
    add_object(&mut stored_world, 6, 9, "first", json!({ "SkKeyframe": { "time": first_time } }));
    add_object(
        &mut stored_world,
        9,
        10,
        "neck",
        json!({ "SkKeyframePose": { "jointName": "neck", "localPose": first_pose } }),
    );
    stored_world
}

fn add_skeleton(stored_world: &mut Value, skeleton_id: u64, joint_name: &str, rest_pose: Location3D) {
    add_object(stored_world, 1, skeleton_id, "skeleton", json!({ "Skeleton": {} }));
    add_object(
        stored_world,
        skeleton_id,
        skeleton_id + 1,
        joint_name,
        json!({ "SkJoint": { "restPose": rest_pose } }),
    );
}

fn migrated_poses(stored_world: &mut Value) -> anyhow::Result<[(f32, Location3D); 2]> {
    updateToVersion(stored_world, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;
    let pose_of_id = |id: u64| serde_json::from_value::<Location3D>(stored_world["objectOfId"][id.to_string()]["info"]["SkKeyframePose"]["localPose"].clone());
    Ok([(0.0, pose_of_id(10)?), (0.5, pose_of_id(8)?)])
}

// Where the engine puts the neck now, with these keyframes playing on a joint with this rest pose.
fn played_pose(rest_pose: &Location3D, keyframes: &[(f32, Location3D)], time: f32) -> Location3D {
    let joints = [RenderSkJoint {
        name: "neck".to_string(),
        parent_joint_index: None,
        rest_pose: rest_pose.clone(),
    }];
    let sk_keyframes: Vec<RenderSkKeyframe> = keyframes
        .iter()
        .map(|(time, local_pose)| RenderSkKeyframe {
            time: *time,
            skKeyframePoses: vec![RenderSkKeyframePose {
                jointName: "neck".to_string(),
                localPose: local_pose.clone(),
            }],
        })
        .collect();
    let sk_animation_info = SkAnimationInfo {
        duration: 1.0,
        interpolationStyle: InterpolationStyle::Linear,
    };
    let (position, quaternion, _scale) = computeLocalTransformOfJointAtTime(&joints, &sk_animation_info, &sk_keyframes, time)[0].unwrap();
    Location3D { position, quaternion }
}

fn assert_same_pose(actual: &Location3D, expected: &Location3D) {
    assert!(
        actual.position.abs_diff_eq(expected.position, 1e-5),
        "{} != {}",
        actual.position,
        expected.position
    );
    assert!(
        actual.quaternion.abs_diff_eq(expected.quaternion, 1e-5) || actual.quaternion.abs_diff_eq(-expected.quaternion, 1e-5),
        "{} != {}",
        actual.quaternion,
        expected.quaternion
    );
}

#[test]
fn a_saved_animation_plays_the_same_poses_on_the_skeleton_its_animator_plays_it_on() -> anyhow::Result<()> {
    let mut stored_world = world_with_look_animation();
    add_skeleton(
        &mut stored_world,
        2,
        "neck",
        Location3D {
            position: Vec3::new(0.0, 5.0, 0.0),
            quaternion: Quat::IDENTITY,
        },
    );
    add_skeleton(&mut stored_world, 4, "neck", turned_rest_pose());
    add_object(
        &mut stored_world,
        1,
        11,
        "animator",
        json!({ "SkAnimator": { "skeletonId": 4, "skAnimationId": 6, "clockId": null, "priority": 0, "weight": 1.0, "weightTransition": null } }),
    );

    let migrated_poses = migrated_poses(&mut stored_world)?;
    for (time, saved_pose) in saved_poses() {
        assert_same_pose(&played_pose(&turned_rest_pose(), &migrated_poses, time), &saved_pose);
    }
    World::fromStoredWorldString(&stored_world.to_string(), WorldControlMode::Editor, None)?;
    Ok(())
}

#[test]
fn an_animation_no_animator_plays_converts_against_the_first_skeleton_with_all_its_joints() -> anyhow::Result<()> {
    let mut stored_world = world_with_look_animation();
    add_skeleton(
        &mut stored_world,
        2,
        "spine",
        Location3D {
            position: Vec3::new(0.0, 5.0, 0.0),
            quaternion: Quat::IDENTITY,
        },
    );
    add_skeleton(&mut stored_world, 4, "neck", turned_rest_pose());

    let migrated_poses = migrated_poses(&mut stored_world)?;
    for (time, saved_pose) in saved_poses() {
        assert_same_pose(&played_pose(&turned_rest_pose(), &migrated_poses, time), &saved_pose);
    }
    Ok(())
}

#[test]
fn a_joint_with_no_saved_skeleton_rests_unturned_where_the_first_keyframe_puts_it() -> anyhow::Result<()> {
    let mut stored_world = world_with_look_animation();

    let [(_, first_offset), (_, later_offset)] = migrated_poses(&mut stored_world)?;
    let [(_, first_pose), (_, later_pose)] = saved_poses();
    assert_same_pose(
        &first_offset,
        &Location3D {
            position: Vec3::ZERO,
            quaternion: first_pose.quaternion,
        },
    );
    assert_same_pose(
        &later_offset,
        &Location3D {
            position: later_pose.position - first_pose.position,
            quaternion: later_pose.quaternion,
        },
    );
    Ok(())
}

fn asset_mesh_json(version: u64) -> Value {
    let (_, saved_pose) = saved_poses()[1].clone();
    json!({
        "version": version,
        "blender_id": null,
        "import_scale": 1.0,
        "dehydrated_meshes": [],
        "dehydrated_skeletons": [{
            "skeleton_index": 0,
            "jointOfIndex": [{ "name": "neck", "parent_joint_index": null, "rest_pose": turned_rest_pose() }],
            "animations": [{
                "name": "look",
                "sk_animation_info": { "duration": 1.0, "interpolationStyle": "Linear" },
                "sk_keyframes": [{ "time": 0.5, "skKeyframePoses": [{ "jointName": "neck", "localPose": saved_pose }] }],
            }],
        }],
    })
}

// An asset row as the database returns it.
fn asset_row_json(mesh_json: Value) -> Value {
    json!({ "asset_id": 1, "version_num": 1, "name": "look", "created_at": "", "asset_type": "Mesh", "data_json": { "Mesh": mesh_json } })
}

fn asset_mesh_keyframes(asset: Asset) -> (u64, Vec<(f32, Location3D)>) {
    let AssetData::Mesh(gltf) = asset.data_json else {
        panic!("expected a mesh");
    };
    let keyframes = gltf.dehydrated_skeletons[0].animations[0]
        .sk_keyframes
        .iter()
        .map(|keyframe| (keyframe.time, keyframe.skKeyframePoses[0].localPose.clone()))
        .collect();
    (gltf.version, keyframes)
}

#[test]
fn an_asset_mesh_published_before_offsets_plays_the_same_poses_once_read() -> anyhow::Result<()> {
    for old_version in [3, 4] {
        let (version, keyframes) = asset_mesh_keyframes(Asset::fromJson(asset_row_json(asset_mesh_json(old_version)))?);
        assert_eq!(version, DEHYDRATED_GLTF_CURRENT_VERSION);
        assert_same_pose(&played_pose(&turned_rest_pose(), &keyframes, 0.5), &saved_poses()[1].1);
    }

    let (version, keyframes) = asset_mesh_keyframes(Asset::fromJson(asset_row_json(asset_mesh_json(DEHYDRATED_GLTF_CURRENT_VERSION)))?);
    assert_eq!(version, DEHYDRATED_GLTF_CURRENT_VERSION);
    assert_same_pose(&keyframes[0].1, &saved_poses()[1].1);
    Ok(())
}

// Two skeletons can name a joint the same, and each skeleton's animations convert against its own joints' rest poses.
#[test]
fn each_skeleton_converts_its_animations_against_its_own_rest_poses() -> anyhow::Result<()> {
    let upright_rest_pose = Location3D {
        position: Vec3::new(0.0, 5.0, 0.0),
        quaternion: Quat::IDENTITY,
    };
    let mut mesh_json = asset_mesh_json(4);
    let mut second_skeleton = mesh_json["dehydrated_skeletons"][0].clone();
    second_skeleton["skeleton_index"] = json!(1);
    second_skeleton["jointOfIndex"][0]["rest_pose"] = serde_json::to_value(&upright_rest_pose)?;
    mesh_json["dehydrated_skeletons"].as_array_mut().unwrap().push(second_skeleton);

    let AssetData::Mesh(gltf) = Asset::fromJson(asset_row_json(mesh_json))?.data_json else {
        panic!("expected a mesh");
    };
    for (skeleton, rest_pose) in gltf.dehydrated_skeletons.iter().zip([turned_rest_pose(), upright_rest_pose]) {
        let keyframes: Vec<(f32, Location3D)> = skeleton.animations[0]
            .sk_keyframes
            .iter()
            .map(|keyframe| (keyframe.time, keyframe.skKeyframePoses[0].localPose.clone()))
            .collect();
        assert_same_pose(&played_pose(&rest_pose, &keyframes, 0.5), &saved_poses()[1].1);
    }
    Ok(())
}

// Every stored mesh carries its version inside it, so one without is refused rather than read as a version it may not be. That
// includes the old [mesh, version] pair, which a database migration moved into the mesh instead of any code reading it.
#[test]
fn an_asset_mesh_with_no_version_inside_it_is_refused() {
    let mut mesh_json = asset_mesh_json(3);
    let version = mesh_json.as_object_mut().unwrap().remove("version").unwrap();
    assert!(Asset::fromJson(asset_row_json(mesh_json.clone())).is_err());
    assert!(Asset::fromJson(asset_row_json(json!([mesh_json, version]))).is_err());
}
