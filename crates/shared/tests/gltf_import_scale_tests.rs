mod helpers;

use game_types::{
    consts::DEHYDRATED_GLTF_CURRENT_VERSION,
    object_types::{InterpolationStyle, Location3D, ObjectInfo, SkAnimationInfo},
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::make_world;
use render_core::{
    dehydrated_gltf::{DehydratedAnimation, DehydratedGltf, DehydratedMaterial, DehydratedMesh, DehydratedSkeleton},
    renderer_types::RenderSkJoint,
};
use shared::api::protocol_for_assets::Asset;
use shared::game::add_dehydrated_gltf_to_world::add_dehydrated_gltf_to_world;
use shared::game::assets::AssetData;

#[test]
fn add_dehydrated_gltf_to_world_applies_the_same_import_scale_to_split_meshes() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let import_scale = 0.125;
    let imported_group_id = add_dehydrated_gltf_to_world(
        &mut world,
        "bird",
        DehydratedGltf {
            version: DEHYDRATED_GLTF_CURRENT_VERSION,
            blender_id: None,
            import_scale,
            dehydrated_meshes: vec![
                DehydratedMesh {
                    name: "mesh".to_string(),
                    mesh_data_id: 101,
                    skinned_skeleton_index: None,
                    material: None,
                },
                DehydratedMesh {
                    name: "mesh".to_string(),
                    mesh_data_id: 202,
                    skinned_skeleton_index: None,
                    material: None,
                },
            ],
            dehydrated_skeletons: Vec::new(),
        },
        root_id,
    )?;

    let mut mesh_object_count = 0;
    for child_id in world.getDirectChildrenIds(imported_group_id)?.iter().copied() {
        let child_object = world.getObject(child_id)?;
        let ObjectInfo::Mesh(mesh_info) = &child_object.info else {
            continue;
        };

        assert_eq!(mesh_info.scale, Vec3::splat(import_scale));
        mesh_object_count += 1;
    }

    assert_eq!(mesh_object_count, 2);

    Ok(())
}

#[test]
fn add_dehydrated_gltf_to_world_builds_one_shared_rigid_group_collider() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let imported_group_id = add_dehydrated_gltf_to_world(
        &mut world,
        "proxy",
        DehydratedGltf {
            version: DEHYDRATED_GLTF_CURRENT_VERSION,
            blender_id: None,
            import_scale: 1.0,
            dehydrated_meshes: vec![
                DehydratedMesh {
                    name: "mesh".to_string(),
                    mesh_data_id: 303,
                    skinned_skeleton_index: None,
                    material: None,
                },
                DehydratedMesh {
                    name: "mesh".to_string(),
                    mesh_data_id: 404,
                    skinned_skeleton_index: None,
                    material: None,
                },
            ],
            dehydrated_skeletons: Vec::new(),
        },
        root_id,
    )?;

    let mut mesh_count = 0;
    let mut physical_object_ids = Vec::new();
    let mut physics_collider_rigid_group_id = None;
    for child_id in world.getDirectChildrenIds(imported_group_id)?.iter().copied() {
        let child_object = world.getObject(child_id)?;
        match &child_object.info {
            ObjectInfo::Mesh(mesh_info) => {
                mesh_count += 1;

                let Some(physical_object_id) = mesh_info.physicalObjectId.as_ref().map(usize::from) else {
                    return Err(anyhow::anyhow!("Expected imported mesh {} to point at a shared rigid group collider", child_id));
                };
                physical_object_ids.push(physical_object_id);
            }
            ObjectInfo::RigidGroup(_) => {
                if physics_collider_rigid_group_id.replace(child_id).is_some() {
                    return Err(anyhow::anyhow!("Expected only one shared rigid group collider"));
                }
            }
            _ => {}
        }
    }

    assert_eq!(mesh_count, 2);

    let Some(physics_collider_rigid_group_id) = physics_collider_rigid_group_id else {
        return Err(anyhow::anyhow!("Expected imported group to contain one shared rigid group collider"));
    };
    for physical_object_id in physical_object_ids {
        assert_eq!(physical_object_id, physics_collider_rigid_group_id);
    }

    let Some(physics_info) = world.getObjectPhysicsInfo3D(physics_collider_rigid_group_id) else {
        return Err(anyhow::anyhow!(
            "Expected shared rigid group collider {} to have physics info",
            physics_collider_rigid_group_id
        ));
    };
    assert_eq!(physics_info.location.position, Vec3::ZERO);
    assert_eq!(physics_info.location.quaternion, glam::Quat::IDENTITY);

    let rigid_group_child_ids = world.getDirectChildrenIds(physics_collider_rigid_group_id)?;
    assert_eq!(rigid_group_child_ids.len(), 1);
    let physics_collider_ball_id = rigid_group_child_ids
        .iter()
        .copied()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Expected shared rigid group collider to contain one ball child"))?;

    let physics_collider_ball = world.getObject(physics_collider_ball_id)?;
    let ObjectInfo::Ball(_) = &physics_collider_ball.info else {
        return Err(anyhow::anyhow!(
            "Expected shared rigid group collider child {} to be a ball",
            physics_collider_ball_id
        ));
    };

    let Some(_ball_primitive_settings) = world.physics.getBallPrimitiveSettings(physics_collider_ball_id) else {
        return Err(anyhow::anyhow!(
            "Expected shared rigid group collider child {} to have ball primitive info",
            physics_collider_ball_id
        ));
    };

    Ok(())
}

#[test]
fn add_dehydrated_gltf_to_world_creates_skeleton_joints_and_animations_for_skinned_meshes() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let imported_group_id = add_dehydrated_gltf_to_world(
        &mut world,
        "bird",
        DehydratedGltf {
            version: DEHYDRATED_GLTF_CURRENT_VERSION,
            blender_id: None,
            import_scale: 1.0,
            dehydrated_meshes: vec![DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id: 505,
                skinned_skeleton_index: Some(7),
                material: None,
            }],
            dehydrated_skeletons: vec![DehydratedSkeleton {
                skeleton_index: 7,
                jointOfIndex: vec![
                    RenderSkJoint {
                        name: "hip".to_string(),
                        parent_joint_index: None,
                        rest_pose: Location3D {
                            position: Vec3::new(1.0, 2.0, 3.0),
                            quaternion: Quat::IDENTITY,
                        },
                    },
                    RenderSkJoint {
                        name: "head".to_string(),
                        parent_joint_index: Some(0),
                        rest_pose: Location3D {
                            position: Vec3::new(7.0, 8.0, 9.0),
                            quaternion: Quat::IDENTITY,
                        },
                    },
                ],
                animations: vec![DehydratedAnimation {
                    name: "fly".to_string(),
                    sk_animation_info: SkAnimationInfo {
                        duration: 1.25,
                        interpolationStyle: InterpolationStyle::Step,
                    },
                    sk_keyframes: Vec::new(),
                }],
            }],
        },
        root_id,
    )?;

    let mut imported_skeleton_id = None;
    let mut imported_mesh_id = None;
    for child_id in world.getDirectChildrenIds(imported_group_id)?.iter().copied() {
        let child_object = world.getObject(child_id)?;
        match &child_object.info {
            ObjectInfo::Skeleton(_) => imported_skeleton_id = Some(child_id),
            ObjectInfo::Mesh(_) => imported_mesh_id = Some(child_id),
            _ => {}
        }
    }

    let Some(imported_skeleton_id) = imported_skeleton_id else {
        return Err(anyhow::anyhow!("Expected imported group to contain a skeleton"));
    };
    let Some(imported_mesh_id) = imported_mesh_id else {
        return Err(anyhow::anyhow!("Expected imported group to contain a skinned mesh"));
    };

    let imported_mesh = world.getObject(imported_mesh_id)?;
    let ObjectInfo::Mesh(imported_mesh_info) = &imported_mesh.info else {
        return Err(anyhow::anyhow!("Expected imported mesh object"));
    };
    assert_eq!(imported_mesh_info.followSkeletonId.as_ref().map(usize::from), Some(imported_skeleton_id));

    let render_skeleton_info_arc = world
        .getRenderSkeletonInfoArc(imported_skeleton_id)
        .ok_or_else(|| anyhow::anyhow!("Expected imported skeleton render info"))?;
    let render_skeleton_info = render_skeleton_info_arc.as_ref();
    assert_eq!(render_skeleton_info.jointOfIndex.len(), 2);
    assert_eq!(render_skeleton_info.jointOfIndex[0].name, "hip");
    assert_eq!(render_skeleton_info.jointOfIndex[0].parent_joint_index, None);
    assert_eq!(render_skeleton_info.jointOfIndex[0].rest_pose.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(render_skeleton_info.jointOfIndex[1].name, "head");
    assert_eq!(render_skeleton_info.jointOfIndex[1].parent_joint_index, Some(0));
    assert_eq!(render_skeleton_info.jointIndexOfName.get("hip"), Some(&0));
    assert_eq!(render_skeleton_info.jointIndexOfName.get("head"), Some(&1));

    let mut sk_animator_object_id = None;
    let mut sk_animation_object_id = None;
    let mut root_joint_object_id = None;
    for child_id in world.getDirectChildrenIds(imported_skeleton_id)?.iter().copied() {
        let child_object = world.getObject(child_id)?;
        match &child_object.info {
            ObjectInfo::SkAnimator(_) => sk_animator_object_id = Some(child_id),
            ObjectInfo::SkAnimation(_) => sk_animation_object_id = Some(child_id),
            ObjectInfo::SkJoint(_) => root_joint_object_id = Some(child_id),
            _ => {}
        }
    }

    let Some(sk_animator_object_id) = sk_animator_object_id else {
        return Err(anyhow::anyhow!("Expected imported skeleton to contain one SkAnimator"));
    };
    let Some(sk_animation_object_id) = sk_animation_object_id else {
        return Err(anyhow::anyhow!("Expected imported skeleton to contain one SkAnimation"));
    };
    let Some(root_joint_object_id) = root_joint_object_id else {
        return Err(anyhow::anyhow!("Expected imported skeleton to contain one root joint"));
    };

    let root_joint_object = world.getObject(root_joint_object_id)?;
    assert_eq!(root_joint_object.name, "hip");
    let root_joint_child_ids = world.getDirectChildrenIds(root_joint_object_id)?;
    assert_eq!(root_joint_child_ids.len(), 1);
    let head_joint_object_id = root_joint_child_ids
        .iter()
        .copied()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Expected one child joint"))?;
    let head_joint_object = world.getObject(head_joint_object_id)?;
    assert_eq!(head_joint_object.name, "head");

    let sk_animation_object = world.getObject(sk_animation_object_id)?;
    let ObjectInfo::SkAnimation(sk_animation_info) = &sk_animation_object.info else {
        return Err(anyhow::anyhow!("Expected imported SkAnimation object"));
    };
    assert_eq!(sk_animation_info.duration, 1.25);

    let animation_object = world.getObject(sk_animator_object_id)?;
    let ObjectInfo::SkAnimator(animation_info) = &animation_object.info else {
        return Err(anyhow::anyhow!("Expected imported SkAnimator object"));
    };
    assert_eq!(animation_info.skeletonId.as_ref().map(usize::from), Some(imported_skeleton_id));
    assert_eq!(animation_info.skAnimationId.as_ref().map(usize::from), Some(sk_animation_object_id));
    assert_eq!(animation_info.weight, 100.0);

    let clock_object_id = animation_info
        .clockId
        .as_ref()
        .map(usize::from)
        .ok_or_else(|| anyhow::anyhow!("Expected imported animation clock"))?;
    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected imported clock object"));
    };
    assert!(clock_info.shouldLoop);
    assert_eq!(clock_info.speed, 1.0);

    Ok(())
}

// The browser consumes the portable import directly; the desktop inserts it into an existing
// World with different ids. Compare actual render output after pointer rewiring and animation sampling.
#[test]
fn portable_and_desktop_imports_render_the_same_animated_meshes() -> anyhow::Result<()> {
    use game_types::object_types::{MaterialInfo, material::AlphaMode};
    use render_core::{
        computeSkeletonPose::computeSkeletonPoseOfIdAtTime,
        dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world,
        render_scene::{collect_material_texture_data_ids, collect_mesh_data_ids, computeRenderObjectTransform},
        renderable::Renderable,
        renderer_types::{RenderObject, RenderSkKeyframe, RenderSkKeyframePose},
    };
    use render_web::stored_world_renderable_impl::StoredWorldRenderable;
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let gltf = DehydratedGltf {
        version: DEHYDRATED_GLTF_CURRENT_VERSION,
        blender_id: None,
        import_scale: 0.25,
        dehydrated_meshes: vec![
            DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id: 101,
                skinned_skeleton_index: Some(7),
                material: Some(DehydratedMaterial {
                    name: "material".to_string(),
                    material_info: MaterialInfo {
                        albedoTextureDataId: Some(201),
                        metallicTextureDataId: Some(202),
                        roughnessTextureDataId: Some(203),
                        normalTextureDataId: Some(204),
                        alphaMode: AlphaMode::Mask { cutoff: 0.4 },
                        isDoubleSided: false,
                    },
                }),
            },
            DehydratedMesh {
                name: "mesh".to_string(),
                mesh_data_id: 102,
                skinned_skeleton_index: None,
                material: None,
            },
        ],
        dehydrated_skeletons: vec![DehydratedSkeleton {
            skeleton_index: 7,
            jointOfIndex: vec![RenderSkJoint {
                name: "joint".into(),
                parent_joint_index: None,
                rest_pose: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
            }],
            animations: (0..2)
                .map(|index| DehydratedAnimation {
                    name: format!("animation_{index}"),
                    sk_animation_info: SkAnimationInfo {
                        duration: 2.0,
                        interpolationStyle: InterpolationStyle::Linear,
                    },
                    sk_keyframes: [0.0, 1.0, 2.0]
                        .into_iter()
                        .map(|time| RenderSkKeyframe {
                            time,
                            skKeyframePoses: vec![RenderSkKeyframePose {
                                jointName: "joint".into(),
                                localPose: Location3D {
                                    position: Vec3::X * time,
                                    quaternion: Quat::from_rotation_z(time),
                                },
                            }],
                        })
                        .collect(),
                })
                .collect(),
        }],
    };
    let stored = dehydrated_gltf_to_stored_world("Test mesh", gltf.clone())?;
    let source = StoredWorldRenderable::new(stored)?;
    let group = add_dehydrated_gltf_to_world(&mut world, "Test mesh", gltf.clone(), root_id)?;
    assert_eq!(world.getObject(group)?.name, "Test-mesh");
    let saved = source.getRenderSnapshot().render_object_of_id;
    let live = world.getRenderSnapshot().render_object_of_id;
    assert_eq!(collect_mesh_data_ids(&saved), collect_mesh_data_ids(&live));
    assert_eq!(collect_material_texture_data_ids(&saved), [201, 202, 203, 204].into());
    assert_eq!(collect_material_texture_data_ids(&saved), collect_material_texture_data_ids(&live));
    let mut joint_positions = Vec::new();
    for time in [0.0, 250.0, 750.0, 2250.0] {
        let saved_poses = computeSkeletonPoseOfIdAtTime(&saved, time, Some(time));
        let live_poses = computeSkeletonPoseOfIdAtTime(&live, time, Some(time));
        for (saved_id, saved_object) in &saved {
            let RenderObject::Mesh(saved_mesh) = saved_object else { continue };
            let (live_id, live_mesh) = live
                .iter()
                .find_map(|(id, object)| match object {
                    RenderObject::Mesh(mesh) if mesh.meshDataId == saved_mesh.meshDataId => Some((id, mesh)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(saved_mesh.scale, live_mesh.scale);
            assert_eq!(saved_mesh.visualSettings.opacity, live_mesh.visualSettings.opacity);
            assert_eq!(saved_mesh.skin_skeleton_id.is_some(), live_mesh.skin_skeleton_id.is_some());
            let saved_transform = computeRenderObjectTransform(&saved, &saved_poses, *saved_id).unwrap();
            let live_transform = computeRenderObjectTransform(&live, &live_poses, *live_id).unwrap();
            assert!(saved_transform.abs_diff_eq(live_transform, 0.0001));
            if let (Some(saved_id), Some(live_id)) = (saved_mesh.skin_skeleton_id, live_mesh.skin_skeleton_id) {
                let saved_pose = &saved_poses[&saved_id].jointMatrixOfJointIndex;
                let live_pose = &live_poses[&live_id].jointMatrixOfJointIndex;
                assert_eq!(saved_pose.len(), live_pose.len());
                for (saved, live) in saved_pose.iter().zip(live_pose) {
                    assert!(saved.abs_diff_eq(*live, 0.0001));
                    joint_positions.push(saved.transform_point3(Vec3::ZERO));
                }
            }
            if let (Some(saved_material), Some(live_material)) = (&saved_mesh.material, &live_mesh.material) {
                let RenderObject::Material(saved_material) = &saved[&usize::from(saved_material.materialInfoId)] else {
                    unreachable!()
                };
                let RenderObject::Material(live_material) = &live[&usize::from(live_material.materialInfoId)] else {
                    unreachable!()
                };
                assert_eq!(serde_json::to_value(saved_material)?, serde_json::to_value(live_material)?);
            } else {
                assert_eq!(saved_mesh.material.is_some(), live_mesh.material.is_some());
            }
        }
    }
    assert!(joint_positions.windows(2).any(|pair| pair[0].distance(pair[1]) > 0.1));
    // A second import must point to its own skeleton, clock, material, and collider.
    let second_group = add_dehydrated_gltf_to_world(&mut world, "Second mesh", gltf, root_id)?;
    assert_ne!(group, second_group);
    shared::game::assets::StoredAssetObject::from_world_object(&world, second_group)?.validate()?;
    let object_count = world.objectOfId.len();
    let invalid_import = add_dehydrated_gltf_to_world(
        &mut world,
        "Invalid",
        DehydratedGltf {
            version: DEHYDRATED_GLTF_CURRENT_VERSION,
            blender_id: None,
            import_scale: 1.0,
            dehydrated_meshes: vec![],
            dehydrated_skeletons: vec![],
        },
        root_id,
    );
    assert!(invalid_import.is_err());
    assert_eq!(object_count, world.objectOfId.len());
    Ok(())
}

// Models saved before imports kept their Blender names still show the numbered names they always had.
#[test]
fn a_model_saved_before_blender_names_keeps_its_numbered_names() -> anyhow::Result<()> {
    let material = serde_json::json!({
        "albedoTextureDataId": 7,
        "metallicTextureDataId": null,
        "roughnessTextureDataId": null,
        "normalTextureDataId": null,
        "alphaMode": "Opaque",
        "isDoubleSided": false,
    });
    let asset_row = serde_json::json!({
        "asset_id": 1,
        "version_num": 1,
        "name": "old model",
        "created_at": "",
        "asset_type": "Mesh",
        "data_json": { "Mesh": {
            "version": 5,
            "blender_id": null,
            "import_scale": 1.0,
            "dehydrated_meshes": [
                { "mesh_data_id": 1, "skinned_skeleton_index": null, "material_info": material },
                { "mesh_data_id": 2, "skinned_skeleton_index": null, "material_info": material },
            ],
            "dehydrated_skeletons": [],
        }},
    });
    let AssetData::Mesh(gltf) = Asset::fromJson(asset_row)?.data_json else {
        anyhow::bail!("expected a mesh asset");
    };
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    let imported_group_id = add_dehydrated_gltf_to_world(&mut world, "old model", gltf, root_id)?;
    let mut mesh_and_material_names = Vec::new();
    for child_id in world.getDirectChildrenIds(imported_group_id)?.iter().copied() {
        let child = world.getObject(child_id)?;
        let ObjectInfo::Mesh(_) = child.info else { continue };
        let [material_id] = world.getDirectChildrenIds(child_id)?.iter().copied().collect::<Vec<_>>()[..] else {
            anyhow::bail!("each mesh should hold its material");
        };
        mesh_and_material_names.push((child.name.clone(), world.getObject(material_id)?.name.clone()));
    }
    mesh_and_material_names.sort();
    assert_eq!(
        mesh_and_material_names,
        [
            ("mesh_1".to_string(), "material_1".to_string()),
            ("mesh_2".to_string(), "material_2".to_string())
        ]
    );
    Ok(())
}
