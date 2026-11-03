use serde_json::{Value, json};
use std::collections::HashMap;

use crate::object_types::Location3D;

pub fn updateToVersion(dehydratedGltfJson: &mut Value, targetVersion: u64) -> anyhow::Result<()> {
    let storedVersion = dehydratedGltfJson
        .get("version")
        .and_then(|version| version.as_u64())
        .ok_or_else(|| anyhow::anyhow!("updateToVersion: dehydrated glTF has no version"))?;

    if storedVersion >= targetVersion {
        return Ok(());
    }

    for version in (storedVersion + 1)..=targetVersion {
        match version {
            5 => stepToVersion5(dehydratedGltfJson)?,
            6 => stepToVersion6(dehydratedGltfJson),
            _ => {}
        }
    }

    dehydratedGltfJson["version"] = Value::from(targetVersion);

    Ok(())
}

// Keyframe poses used to hold each joint's whole pose, and now hold its offset from the joint's rest pose.
fn stepToVersion5(dehydratedGltfJson: &mut Value) -> anyhow::Result<()> {
    let Some(skeletons) = dehydratedGltfJson
        .get_mut("dehydrated_skeletons")
        .and_then(|skeletons| skeletons.as_array_mut())
    else {
        return Ok(());
    };

    for skeleton in skeletons {
        let mut restPoseOfJointName: HashMap<String, Location3D> = HashMap::new();
        for joint in skeleton.get("jointOfIndex").and_then(|joints| joints.as_array()).into_iter().flatten() {
            let (Some(name), Ok(restPose)) = (joint["name"].as_str(), serde_json::from_value::<Location3D>(joint["rest_pose"].clone())) else {
                continue;
            };
            restPoseOfJointName.entry(name.to_string()).or_insert(restPose);
        }

        let animations = skeleton.get_mut("animations").and_then(|animations| animations.as_array_mut());
        for animation in animations.into_iter().flatten() {
            let keyframes = animation.get_mut("sk_keyframes").and_then(|keyframes| keyframes.as_array_mut());
            for keyframe in keyframes.into_iter().flatten() {
                let poses = keyframe.get_mut("skKeyframePoses").and_then(|poses| poses.as_array_mut());
                for pose in poses.into_iter().flatten() {
                    let (Some(restPose), Ok(localPose)) = (
                        pose["jointName"].as_str().and_then(|jointName| restPoseOfJointName.get(jointName)),
                        serde_json::from_value::<Location3D>(pose["localPose"].clone()),
                    ) else {
                        continue;
                    };
                    let restRotationInverse = restPose.quaternion.inverse();
                    pose["localPose"] = serde_json::to_value(Location3D {
                        position: restRotationInverse * (localPose.position - restPose.position),
                        quaternion: (restRotationInverse * localPose.quaternion).normalize(),
                    })?;
                }
            }
        }
    }

    Ok(())
}

// Each mesh keeps its Blender object's name, and its material moves under `material` with the Blender material's name. Older
// models get the numbered names they were shown with.
fn stepToVersion6(dehydratedGltfJson: &mut Value) {
    let meshes = dehydratedGltfJson.get_mut("dehydrated_meshes").and_then(|meshes| meshes.as_array_mut());
    for (mesh_index, mesh) in meshes.into_iter().flatten().enumerate() {
        let Some(mesh) = mesh.as_object_mut() else {
            continue;
        };
        mesh.insert("name".to_string(), Value::from(format!("mesh_{}", mesh_index + 1)));
        let material = match mesh.remove("material_info") {
            None | Some(Value::Null) => Value::Null,
            Some(material_info) => json!({ "name": format!("material_{}", mesh_index + 1), "material_info": material_info }),
        };
        mesh.insert("material".to_string(), material);
    }
}
