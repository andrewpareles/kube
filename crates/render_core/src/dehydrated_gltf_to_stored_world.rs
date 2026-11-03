use std::collections::HashMap;

use anyhow::Result;
use glam::{Quat, Vec3};

use crate::dehydrated_gltf::{DehydratedGltf, DehydratedMaterial, DehydratedSkeleton};
use game_types::consts::ANIMATOR_WEIGHT_MAX;
use game_types::object_types::shared::Color3;
use game_types::object_types::sk_animator::SkAnimatorInfo;
use game_types::object_types::{
    BallInfo, CameraLocation, ClockInfo, ClockState, LiveObject, LocalGroupInfo, Location3D, MeshInfo, MeshMaterialSettings, Movement3D, ObjectBeingAdded,
    ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PhysicsInfo3D, PhysicsToggles3D, RigidGroupInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo,
    SkeletonInfo, VisualSettings,
};
use game_types::physics_primitive_types::{BallPrimitiveSettings, PrimitiveSettings};
use game_types::{consts::STORED_WORLD_CURRENT_VERSION, stored_world_type::StoredWorld};

// This rebuilds the same world objects the old import flow created, but it now
// starts from dehydrated ids instead of raw mesh, animation, and texture data.
// Desktop imports and browser previews both rebuild this exact hierarchy before rendering it.
pub fn dehydrated_gltf_to_stored_world(gltfName: &str, dehydratedGltf: DehydratedGltf) -> Result<StoredWorld> {
    dehydrated_gltf_to_stored_world_from_id(gltfName, dehydratedGltf, 1)
}

// The import goes under parent_id. Its ids start past every id already in the world, so nothing has to be renumbered.
pub fn add_dehydrated_gltf_to_stored_world(stored: &mut StoredWorld, gltfName: &str, dehydratedGltf: DehydratedGltf, parent_id: ObjectId) -> Result<ObjectId> {
    let first_id = stored.objectOfId.keys().max().map_or(1, |max_id| max_id + 1);
    let import = dehydrated_gltf_to_stored_world_from_id(gltfName, dehydratedGltf, first_id)?;
    stored.objectOfId.extend(import.objectOfId);
    stored.childrenOfId.extend(import.childrenOfId);
    stored.scriptRawCodeOfId.extend(import.scriptRawCodeOfId);
    stored.physicsInfo3DOfId.extend(import.physicsInfo3DOfId);
    stored.physicsToggles3DOfId.extend(import.physicsToggles3DOfId);
    stored.primitiveSettingsOfId.extend(import.primitiveSettingsOfId);
    stored.childrenOfId.entry(parent_id).or_default().push(import.rootObjectId);
    Ok(import.rootObjectId)
}

// The import's group takes first_id, and each object after it takes the next id.
fn dehydrated_gltf_to_stored_world_from_id(gltfName: &str, dehydratedGltf: DehydratedGltf, first_id: ObjectId) -> Result<StoredWorld> {
    dehydratedGltf.validate()?;
    let mut stored = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: CameraLocation {
            position: Vec3::ZERO,
            orientation: game_types::object_types::Orientation3D::Z(),
        },
        rootObjectId: first_id,
        objectOfId: HashMap::new(),
        childrenOfId: HashMap::new(),
        scriptRawCodeOfId: HashMap::new(),
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };
    // The import creates only these two kinds of physics objects. Other nodes hold all their state in ObjectInfo.
    fn add_object(stored: &mut StoredWorld, name: String, object: ObjectBeingAdded, parent: Option<ObjectId>) -> ObjectId {
        let id = stored.rootObjectId + stored.objectOfId.len();
        if let Some(physics) = object.get_physics_info() {
            stored.physicsInfo3DOfId.insert(id, physics.clone());
        }
        if let ObjectBeingAdded::Ball(_, shape, _, toggles) = &object {
            stored.primitiveSettingsOfId.insert(id, PrimitiveSettings::Ball(shape.clone()));
            stored.physicsToggles3DOfId.insert(id, toggles.clone());
        }
        stored.objectOfId.insert(
            id,
            LiveObject {
                id,
                name,
                info: object.to_object_info(),
            },
        );
        stored.childrenOfId.insert(id, Vec::new());
        if let Some(parent) = parent {
            stored.childrenOfId.get_mut(&parent).unwrap().push(id);
        }
        id
    }
    log::info!("Adding dehydrated glTF to world: {:?}", dehydratedGltf);
    let DehydratedGltf {
        version: _,
        blender_id: _,
        import_scale,
        dehydrated_meshes,
        dehydrated_skeletons,
    } = dehydratedGltf;

    // Create one parent group for the imported objects so the whole import stays together.
    let group_name = format!("{gltfName}");
    let group_id = add_object(&mut stored, group_name, ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), None);

    // Rebuild each imported Skeleton as one Skeleton object plus its joint subtree and animation children.
    let mut skeletonIdOfSkeletonIndex: HashMap<usize, ObjectId> = HashMap::new();
    for dehydrated_skeleton in dehydrated_skeletons {
        let DehydratedSkeleton {
            skeleton_index,
            jointOfIndex,
            animations,
        } = dehydrated_skeleton;
        let skeleton_name = format!("skeleton_{}", skeleton_index + 1);
        let skeleton_id = add_object(&mut stored, skeleton_name, ObjectBeingAdded::Skeleton(SkeletonInfo {}), Some(group_id));
        skeletonIdOfSkeletonIndex.insert(skeleton_index, skeleton_id);

        let mut joint_id_of_joint_index: Vec<ObjectId> = Vec::with_capacity(jointOfIndex.len());
        for joint in jointOfIndex {
            let parent_id = match joint.parent_joint_index {
                Some(parent_joint_index) => joint_id_of_joint_index[parent_joint_index],
                None => skeleton_id,
            };
            let joint_id = add_object(
                &mut stored,
                joint.name,
                ObjectBeingAdded::SkJoint(SkJointInfo { restPose: joint.rest_pose }),
                Some(parent_id),
            );
            joint_id_of_joint_index.push(joint_id);
        }

        for (animation_index, dehydrated_animation) in animations.into_iter().enumerate() {
            let animation_name = dehydrated_animation.name;
            let sk_animation_info = dehydrated_animation.sk_animation_info;
            let sk_keyframes = dehydrated_animation.sk_keyframes;

            let sk_animation_id = add_object(
                &mut stored,
                animation_name.clone(),
                ObjectBeingAdded::SkAnimation(sk_animation_info),
                Some(skeleton_id),
            );

            for sk_keyframe in sk_keyframes {
                let keyframe_id = add_object(
                    &mut stored,
                    format!("keyframe_{}", sk_keyframe.time),
                    ObjectBeingAdded::SkKeyframe(SkKeyframeInfo { time: sk_keyframe.time }),
                    Some(sk_animation_id),
                );
                for sk_keyframe_pose in sk_keyframe.skKeyframePoses {
                    add_object(
                        &mut stored,
                        sk_keyframe_pose.jointName.clone(),
                        ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
                            jointName: sk_keyframe_pose.jointName,
                            localPose: sk_keyframe_pose.localPose,
                        }),
                        Some(keyframe_id),
                    );
                }
            }

            let animator_id = stored.rootObjectId + stored.objectOfId.len();
            let clock_id = animator_id + 1;
            add_object(
                &mut stored,
                format!("{}_animator", animation_name),
                ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
                    skeletonId: Some(ObjectIdPointer(skeleton_id)),
                    clockId: Some(ObjectIdPointer(clock_id)),
                    skAnimationId: Some(ObjectIdPointer(sk_animation_id)),
                    priority: 0,
                    weight: if animation_index == 0 { ANIMATOR_WEIGHT_MAX } else { 0.0 },
                    weightTransition: None,
                }),
                Some(skeleton_id),
            );
            add_object(
                &mut stored,
                format!("clock_{}", animation_index + 1),
                ObjectBeingAdded::Clock(ClockInfo {
                    shouldLoop: true,
                    clockState: ClockState::Playing {
                        localStartMs: 0.0,
                        timeLocation: ObjectLocation::Client,
                    },
                    speed: 1.0,
                }),
                Some(animator_id),
            );
        }
    }

    // Build the shared physics collider as a rigid group so future imports can add more child colliders without changing mesh wiring.
    let physics_collider_rigid_group_id = add_object(
        &mut stored,
        format!("physics_collider"),
        ObjectBeingAdded::RigidGroup(
            RigidGroupInfo { followSkeletonId: None },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
        ),
        Some(group_id),
    );

    // Keep one child inside the rigid group for now so it is clear this collider can grow into multiple sub-colliders later.
    let mut visual_settings = VisualSettings::TYPICAL_OPAQUE;
    visual_settings.opacity = 0.0;
    add_object(
        &mut stored,
        format!("physics_collider_child"),
        ObjectBeingAdded::Ball(
            BallInfo {
                material: None,
                visualSettings: visual_settings,
            },
            BallPrimitiveSettings { radius: 0.5 },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::ZERO,
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: true,
                density: 1.0,
            },
        ),
        Some(physics_collider_rigid_group_id),
    );

    // add a mesh object for each dehydrated mesh
    for dehydratedMesh in dehydrated_meshes {
        let is_textured = match &dehydratedMesh.material {
            Some(DehydratedMaterial { material_info, .. }) => material_info.albedoTextureDataId.is_some(),
            None => false,
        };
        let mut visual_settings = VisualSettings::TYPICAL_OPAQUE;
        visual_settings.isAlwaysOnTop = false;
        if is_textured {
            visual_settings.color = Color3::WHITE;
        }
        let mesh_info = MeshInfo {
            scale: Vec3::splat(import_scale),
            meshDataId: dehydratedMesh.mesh_data_id,
            followSkeletonId: None, // set next
            material: None,         // set next
            physicalObjectId: Some(ObjectIdPointer(physics_collider_rigid_group_id)),
            visualSettings: visual_settings,
        };
        let mesh_object_id = add_object(&mut stored, dehydratedMesh.name, ObjectBeingAdded::Mesh(mesh_info), Some(group_id));

        let follow_skeleton_id = match dehydratedMesh.skinned_skeleton_index {
            Some(skeleton_index) => Some(ObjectIdPointer(skeletonIdOfSkeletonIndex[&skeleton_index])),
            None => None,
        };

        // material
        let mesh_material_settings = if let Some(DehydratedMaterial { name, material_info }) = dehydratedMesh.material {
            let material_object_id = add_object(&mut stored, name, ObjectBeingAdded::Material(material_info), Some(mesh_object_id));
            Some(MeshMaterialSettings {
                materialInfoId: ObjectIdPointer(material_object_id),
            })
        } else {
            None
        };

        // modify mesh info to have the new Skeleton and material that we put as children to the mesh object
        let Some(mesh_live_object) = stored.objectOfId.get_mut(&mesh_object_id) else {
            log::error!("Imported mesh object was not a mesh");
            continue;
        };
        let ObjectInfo::Mesh(mesh_info) = &mut mesh_live_object.info else {
            log::error!("Imported mesh object was not a mesh");
            continue;
        };

        // set the new fields
        mesh_info.followSkeletonId = follow_skeleton_id;
        mesh_info.material = mesh_material_settings;
    }

    Ok(stored)
}
