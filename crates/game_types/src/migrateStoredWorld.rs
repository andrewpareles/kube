use crate::object_types::{Location3D, WorldLightingInfo};
use glam::Quat;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

// An asset object is one saved subtree of a world, so it never gets what a whole game needs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StoredWorldKind {
    GameWorld,
    AssetObject,
}

pub fn updateToVersion(storedWorldJson: &mut Value, targetVersion: u64, storedWorldKind: StoredWorldKind) -> anyhow::Result<()> {
    let storedVersion = storedWorldJson
        .get("worldSchemaVersion")
        .and_then(|version| version.as_u64())
        .ok_or_else(|| anyhow::anyhow!("updateToVersion: stored world has no worldSchemaVersion"))?;

    if storedVersion >= targetVersion {
        return Ok(());
    }

    for version in (storedVersion + 1)..=targetVersion {
        match version {
            70 => stepToVersion70(storedWorldJson)?,
            73 => stepToVersion73(storedWorldJson)?,
            75 => stepToVersion75(storedWorldJson)?,
            76 => stepToVersion76(storedWorldJson)?,
            77 => stepToVersion77(storedWorldJson)?,
            80 => stepToVersion80(storedWorldJson)?,
            81 => stepToVersion81(storedWorldJson)?,
            83 if storedWorldKind == StoredWorldKind::GameWorld => stepToVersion83(storedWorldJson)?,
            84 => stepToVersion84(storedWorldJson)?,
            85 => stepToVersion85(storedWorldJson)?,
            86 => stepToVersion86(storedWorldJson)?,
            87 => stepToVersion87(storedWorldJson)?,
            88 if storedWorldKind == StoredWorldKind::GameWorld => stepToVersion88(storedWorldJson)?,
            89 => stepToVersion89(storedWorldJson)?,
            90 => stepToVersion90(storedWorldJson)?,
            91 => stepToVersion91(storedWorldJson)?,
            92 => stepToVersion92(storedWorldJson)?,
            _ => {}
        }
    }

    storedWorldJson["worldSchemaVersion"] = Value::from(targetVersion);

    Ok(())
}

// Materials were first stored at version 84, and no step since has changed MaterialInfo. A step that does has to change stored
// materials here too.
pub fn updateStoredMaterialToVersion(storedMaterialJson: &mut Value, targetVersion: u64) -> anyhow::Result<()> {
    let storedVersion = storedMaterialJson
        .get("worldSchemaVersion")
        .and_then(|version| version.as_u64())
        .ok_or_else(|| anyhow::anyhow!("updateStoredMaterialToVersion: stored material has no worldSchemaVersion"))?;

    if storedVersion >= targetVersion {
        return Ok(());
    }

    storedMaterialJson["worldSchemaVersion"] = Value::from(targetVersion);

    Ok(())
}

fn stepToVersion70(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    for (_, object) in objectOfId.iter_mut() {
        let Some(worldLighting) = object
            .get_mut("info")
            .and_then(|info| info.get_mut("WorldLighting"))
            .and_then(|worldLighting| worldLighting.as_object_mut())
        else {
            continue;
        };
        let Some(skyColor) = worldLighting.remove("skyColor") else {
            continue;
        };

        for stopName in ["skyTopColor", "skyHorizonColor", "skyBottomColor"] {
            worldLighting.entry(stopName).or_insert_with(|| skyColor.clone());
        }
    }

    Ok(())
}

// Convert upright labels to full billboarding before deserializing the saved world.
fn stepToVersion73(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    for (_, object) in objectOfId.iter_mut() {
        let Some(billboardMode) = object
            .get_mut("info")
            .and_then(|info| info.get_mut("Label"))
            .and_then(|label| label.get_mut("billboardMode"))
        else {
            continue;
        };
        if billboardMode.as_str() == Some("Upright") {
            *billboardMode = Value::from("Full");
        }
    }

    Ok(())
}

// Materials saved before double-sided rendering existed keep the original single-sided behavior.
fn stepToVersion75(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    for (_, object) in objectOfId.iter_mut() {
        let Some(material) = object
            .get_mut("info")
            .and_then(|info| info.get_mut("Material"))
            .and_then(|material| material.as_object_mut())
        else {
            continue;
        };
        material.entry("isDoubleSided").or_insert(Value::Bool(false));
    }

    Ok(())
}

// World lighting saved before ambient occlusion and shadow softness settings existed gets the default ones.
fn stepToVersion76(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    let defaultWorldLightingInfo = WorldLightingInfo::default();
    for (_, object) in objectOfId.iter_mut() {
        let Some(worldLighting) = object
            .get_mut("info")
            .and_then(|info| info.get_mut("WorldLighting"))
            .and_then(|worldLighting| worldLighting.as_object_mut())
        else {
            continue;
        };
        worldLighting
            .entry("sunShadowSoftness")
            .or_insert(Value::from(defaultWorldLightingInfo.sunShadowSoftness));
        worldLighting
            .entry("ambientOcclusionIntensity")
            .or_insert(Value::from(defaultWorldLightingInfo.ambientOcclusionIntensity));
        worldLighting
            .entry("ambientOcclusionRadius")
            .or_insert(Value::from(defaultWorldLightingInfo.ambientOcclusionRadius));
        worldLighting
            .entry("ambientOcclusionSunlightStrength")
            .or_insert(Value::from(defaultWorldLightingInfo.ambientOcclusionSunlightStrength));
    }

    keepTheLightOfLinearColors(storedWorldJson);
    Ok(())
}

// Worlds saved before v76 used every color as linear light, and colors are sRGB since then. Plain colors are left
// alone, so they now show the way they were picked. Tints on textures and the colors of the sun and ambient light
// would give less light than before, so they are encoded to give the same light, along with the keyframes that
// animate them. Lighting still at the old defaults, which nobody picked, gets the new defaults instead.
fn keepTheLightOfLinearColors(storedWorldJson: &mut Value) {
    let Some(objectOfId) = storedWorldJson.get("objectOfId").and_then(|objects| objects.as_object()) else {
        return;
    };
    let texturedMaterialIds = texturedMaterialIdsOf(objectOfId);
    let oldDefaultWorldLighting = [
        ("sunColor", json!({ "r": 1.0, "g": 0.93333334, "b": 0.8888889 })),
        ("sunIntensity", json!(4.5)),
        ("ambientTopColor", json!({ "r": 0.05, "g": 0.06, "b": 0.08 })),
        ("ambientBottomColor", json!({ "r": 0.03, "g": 0.025, "b": 0.02 })),
        ("ambientIntensity", json!(1.0)),
        ("skyTopColor", json!({ "r": 0.29, "g": 0.51, "b": 0.85 })),
        ("skyHorizonColor", json!({ "r": 0.75, "g": 0.86, "b": 0.94 })),
        ("skyBottomColor", json!({ "r": 0.24, "g": 0.22, "b": 0.2 })),
    ];
    let newDefaultWorldLighting = serde_json::to_value(WorldLightingInfo::default()).unwrap_or(Value::Null);

    // Each object's colors that now give the same light, by the name they are animated under.
    let mut encodedColorNamesOfId: HashMap<u64, &[&str]> = HashMap::new();
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return;
    };
    for (id, object) in objectOfId.iter_mut() {
        let Ok(id) = id.parse::<u64>() else {
            continue;
        };
        let Some(info) = object.get_mut("info").and_then(|info| info.as_object_mut()) else {
            continue;
        };
        for (kind, objectInfo) in info.iter_mut() {
            let isTinted = match kind.as_str() {
                "Mesh" => true,
                "Brick" | "Ball" | "Cylinder" => objectInfo
                    .get("material")
                    .and_then(|material| material.get("materialInfoId"))
                    .and_then(|materialInfoId| materialInfoId.as_u64())
                    .is_some_and(|materialInfoId| texturedMaterialIds.contains(&materialInfoId)),
                _ => false,
            };
            if isTinted {
                if let Some(color) = objectInfo.get_mut("visualSettings").and_then(|visualSettings| visualSettings.get_mut("color")) {
                    encodeSrgbColor(color);
                }
                encodedColorNamesOfId.insert(id, &["color"]);
            }
            if kind == "WorldLighting" {
                let Some(worldLighting) = objectInfo.as_object_mut() else {
                    continue;
                };
                let isStillOldDefaults = oldDefaultWorldLighting
                    .iter()
                    .all(|(name, oldDefault)| worldLighting.get(*name).is_some_and(|value| isNearlyEqual(value, oldDefault)));
                if isStillOldDefaults {
                    for (name, _) in &oldDefaultWorldLighting {
                        if let Some(newDefault) = newDefaultWorldLighting.get(*name) {
                            worldLighting.insert(name.to_string(), newDefault.clone());
                        }
                    }
                } else {
                    for name in ["sunColor", "ambientTopColor", "ambientBottomColor"] {
                        if let Some(color) = worldLighting.get_mut(name) {
                            encodeSrgbColor(color);
                        }
                    }
                }
                encodedColorNamesOfId.insert(id, &["sunColor", "ambientTopColor", "ambientBottomColor"]);
            }
        }
    }

    // Keyframes follow the colors they animate, unless their animation also drives something that did not change.
    let mut isEncodedColorOfAnimationId: HashMap<u64, Vec<bool>> = HashMap::new();
    for object in objectOfId.values() {
        let Some(animator) = object.get("info").and_then(|info| info.get("PropertyAnimator")) else {
            continue;
        };
        let Some(animationId) = animator.get("propertyAnimationId").and_then(|animationId| animationId.as_u64()) else {
            continue;
        };
        let colorNames = animator
            .get("targetObjectId")
            .and_then(|targetObjectId| targetObjectId.as_u64())
            .and_then(|targetObjectId| encodedColorNamesOfId.get(&targetObjectId));
        let propertyPath: Vec<&str> = animator
            .get("propertyPath")
            .and_then(|propertyPath| propertyPath.as_array())
            .map(|propertyPath| propertyPath.iter().filter_map(|name| name.as_str()).collect())
            .unwrap_or_default();
        let isEncodedColor = match (colorNames, propertyPath.as_slice()) {
            (Some(colorNames), [colorName, "r" | "g" | "b"]) => colorNames.contains(colorName),
            _ => false,
        };
        isEncodedColorOfAnimationId.entry(animationId).or_default().push(isEncodedColor);
    }
    let keyframeIds: Vec<String> = isEncodedColorOfAnimationId
        .into_iter()
        .filter(|(_, isEncodedColor)| isEncodedColor.iter().all(|isEncoded| *isEncoded))
        .filter_map(|(animationId, _)| storedWorldJson.get("childrenOfId")?.get(animationId.to_string())?.as_array())
        .flatten()
        .filter_map(|keyframeId| keyframeId.as_u64().map(|keyframeId| keyframeId.to_string()))
        .collect();
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return;
    };
    for keyframeId in keyframeIds {
        let Some(value) = objectOfId
            .get_mut(&keyframeId)
            .and_then(|keyframe| keyframe.get_mut("info"))
            .and_then(|info| info.get_mut("PropertyKeyframe"))
            .and_then(|keyframe| keyframe.get_mut("value"))
        else {
            continue;
        };
        if let Some(linear) = value.as_f64() {
            *value = Value::from(srgbEncodedChannel(linear));
        }
    }
}

// The Materials with a color texture, which the color of an object wearing them tints.
fn texturedMaterialIdsOf(objectOfId: &Map<String, Value>) -> HashSet<u64> {
    objectOfId
        .iter()
        .filter_map(|(id, object)| {
            let albedoTextureDataId = object.get("info")?.get("Material")?.get("albedoTextureDataId")?;
            if albedoTextureDataId.is_null() { None } else { id.parse().ok() }
        })
        .collect()
}

// The sRGB value that decodes to this linear one, the inverse of decode_srgb_color in render_scene.rs.
fn srgbEncodedChannel(linear: f64) -> f64 {
    if linear == 1.0 {
        1.0
    } else if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn encodeSrgbColor(color: &mut Value) {
    let Some(color) = color.as_object_mut() else {
        return;
    };
    for channel in ["r", "g", "b"] {
        if let Some(linear) = color.get(channel).and_then(|value| value.as_f64()) {
            color.insert(channel.to_string(), Value::from(srgbEncodedChannel(linear)));
        }
    }
}

// Numbers match within float rounding, and colors match channel by channel.
fn isNearlyEqual(value: &Value, expected: &Value) -> bool {
    match (value, expected) {
        (Value::Number(value), Value::Number(expected)) => match (value.as_f64(), expected.as_f64()) {
            (Some(value), Some(expected)) => (value - expected).abs() < 1e-4,
            _ => false,
        },
        (Value::Object(value), Value::Object(expected)) => expected
            .iter()
            .all(|(name, expected)| value.get(name).is_some_and(|value| isNearlyEqual(value, expected))),
        _ => false,
    }
}

// World lighting saved before the sky reflection strength setting existed gets the default one.
fn stepToVersion77(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    let defaultWorldLightingInfo = WorldLightingInfo::default();
    for (_, object) in objectOfId.iter_mut() {
        let Some(worldLighting) = object
            .get_mut("info")
            .and_then(|info| info.get_mut("WorldLighting"))
            .and_then(|worldLighting| worldLighting.as_object_mut())
        else {
            continue;
        };
        worldLighting
            .entry("skyReflectionStrength")
            .or_insert(Value::from(defaultWorldLightingInfo.skyReflectionStrength));
    }

    Ok(())
}

// World lighting saved before the vibrance setting existed gets the default one.
fn stepToVersion80(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    let defaultWorldLightingInfo = WorldLightingInfo::default();
    for (_, object) in objectOfId.iter_mut() {
        let Some(worldLighting) = object
            .get_mut("info")
            .and_then(|info| info.get_mut("WorldLighting"))
            .and_then(|worldLighting| worldLighting.as_object_mut())
        else {
            continue;
        };
        worldLighting.entry("vibrance").or_insert(Value::from(defaultWorldLightingInfo.vibrance));
    }

    Ok(())
}

// Objects saved before glow existed don't glow.
fn stepToVersion81(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };

    for (_, object) in objectOfId.iter_mut() {
        let Some(info) = object.get_mut("info").and_then(|info| info.as_object_mut()) else {
            continue;
        };
        for (_, objectInfo) in info.iter_mut() {
            let Some(visualSettings) = objectInfo.get_mut("visualSettings").and_then(|visualSettings| visualSettings.as_object_mut()) else {
                continue;
            };
            visualSettings.entry("glow").or_insert(Value::from(0.0));
        }
    }

    Ok(())
}

pub const CHARACTER_CONTROLLER_FOLDER_NAME: &str = "CharacterController";
pub const CHARACTER_CONTROLLER_SCRIPT_NAME: &str = "controller";
pub const CHARACTER_CONTROLLER_SCRIPT: &str = include_str!("../../../assets/scripts/character_controller.ts");

// Characters used to be moved by Rust. Worlds saved before that get the script that moves them now, the same folder the asset adds.
fn stepToVersion83(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let rootObjectId = storedWorldJson
        .get("rootObjectId")
        .and_then(|id| id.as_u64())
        .ok_or_else(|| anyhow::anyhow!("stepToVersion83: stored world has no rootObjectId"))?;
    // a world that already has the folder keeps the one it has
    let hasControllerFolder = storedWorldJson["childrenOfId"][rootObjectId.to_string()]
        .as_array()
        .into_iter()
        .flatten()
        .any(|childId| storedWorldJson["objectOfId"][childId.to_string()]["name"] == CHARACTER_CONTROLLER_FOLDER_NAME);
    if hasControllerFolder {
        return Ok(());
    }
    let objectOfId = storedWorldJson
        .get_mut("objectOfId")
        .and_then(|objects| objects.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("stepToVersion83: stored world has no objectOfId"))?;
    let maxObjectId = objectOfId.keys().filter_map(|id| id.parse::<u64>().ok()).max().unwrap_or(rootObjectId);
    let folderId = maxObjectId + 1;
    let scriptId = maxObjectId + 2;
    objectOfId.insert(
        folderId.to_string(),
        serde_json::json!({ "id": folderId, "name": CHARACTER_CONTROLLER_FOLDER_NAME, "info": { "LocalGroup": {} } }),
    );
    objectOfId.insert(
        scriptId.to_string(),
        serde_json::json!({ "id": scriptId, "name": CHARACTER_CONTROLLER_SCRIPT_NAME, "info": { "Script": { "objectLocation": "Client" } } }),
    );

    let childrenOfId = storedWorldJson
        .get_mut("childrenOfId")
        .and_then(|children| children.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("stepToVersion83: stored world has no childrenOfId"))?;
    let rootChildren = childrenOfId.entry(rootObjectId.to_string()).or_insert_with(|| Value::Array(vec![]));
    rootChildren
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("stepToVersion83: root children are not a list"))?
        .push(Value::from(folderId));
    childrenOfId.insert(folderId.to_string(), serde_json::json!([scriptId]));

    let scriptRawCodeOfId = storedWorldJson
        .get_mut("scriptRawCodeOfId")
        .and_then(|scripts| scripts.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("stepToVersion83: stored world has no scriptRawCodeOfId"))?;
    scriptRawCodeOfId.insert(scriptId.to_string(), Value::from(CHARACTER_CONTROLLER_SCRIPT));

    Ok(())
}

// Keyframe poses used to hold each joint's whole pose, and now hold its offset from the joint's rest pose. Skeletons that scripts
// build while the game runs aren't saved, so their joints are taken to rest unturned where the animation's first keyframe puts them.
fn stepToVersion84(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    fn infoOf<'a>(objectOfId: &'a Map<String, Value>, id: u64, kind: &str) -> Option<&'a Value> {
        objectOfId.get(&id.to_string())?.get("info")?.get(kind)
    }
    fn childIdsOf(childrenOfId: &Map<String, Value>, id: u64) -> Vec<u64> {
        let Some(childIds) = childrenOfId.get(&id.to_string()).and_then(|childIds| childIds.as_array()) else {
            return Vec::new();
        };
        childIds.iter().filter_map(|childId| childId.as_u64()).collect()
    }

    let (Some(objectOfId), Some(childrenOfId)) = (
        storedWorldJson.get("objectOfId").and_then(|objects| objects.as_object()),
        storedWorldJson.get("childrenOfId").and_then(|children| children.as_object()),
    ) else {
        return Ok(());
    };
    let idsOf = |kind: &str| {
        let mut ids: Vec<u64> = objectOfId
            .iter()
            .filter(|(_, object)| object.get("info").and_then(|info| info.get(kind)).is_some())
            .filter_map(|(id, _)| id.parse().ok())
            .collect();
        ids.sort_unstable();
        ids
    };

    // the engine walks a Skeleton's SkJoints depth first in id order, and skips a joint whose name it has already seen
    let skeletonIds = idsOf("Skeleton");
    let mut restPoseOfJointNameOfSkeletonId: HashMap<u64, HashMap<String, Location3D>> = HashMap::new();
    for &skeletonId in &skeletonIds {
        let jointChildIdsOf = |id: u64| {
            let mut jointIds: Vec<u64> = childIdsOf(childrenOfId, id)
                .into_iter()
                .filter(|childId| infoOf(objectOfId, *childId, "SkJoint").is_some())
                .collect();
            jointIds.sort_unstable_by(|a, b| b.cmp(a));
            jointIds
        };
        let mut restPoseOfJointName = HashMap::new();
        let mut visitedJointIds = HashSet::new();
        let mut pendingJointIds = jointChildIdsOf(skeletonId);
        while let Some(jointId) = pendingJointIds.pop() {
            let Some(joint) = objectOfId.get(&jointId.to_string()) else {
                continue;
            };
            let (Some(name), Ok(restPose)) = (
                joint["name"].as_str(),
                serde_json::from_value::<Location3D>(joint["info"]["SkJoint"]["restPose"].clone()),
            ) else {
                continue;
            };
            if !visitedJointIds.insert(jointId) || restPoseOfJointName.contains_key(name) {
                continue;
            }
            restPoseOfJointName.insert(name.to_string(), restPose);
            pendingJointIds.extend(jointChildIdsOf(jointId));
        }
        restPoseOfJointNameOfSkeletonId.insert(skeletonId, restPoseOfJointName);
    }

    let mut skeletonIdOfAnimationId: HashMap<u64, u64> = HashMap::new();
    for animatorId in idsOf("SkAnimator") {
        let Some(animator) = infoOf(objectOfId, animatorId, "SkAnimator") else {
            continue;
        };
        let (Some(animationId), Some(skeletonId)) = (animator["skAnimationId"].as_u64(), animator["skeletonId"].as_u64()) else {
            continue;
        };
        if restPoseOfJointNameOfSkeletonId.contains_key(&skeletonId) {
            skeletonIdOfAnimationId.entry(animationId).or_insert(skeletonId);
        }
    }

    let mut offsetOfPoseId: Vec<(u64, Location3D)> = Vec::new();
    for animationId in idsOf("SkAnimation") {
        let mut keyframeIds: Vec<(f64, u64)> = childIdsOf(childrenOfId, animationId)
            .into_iter()
            .filter_map(|keyframeId| Some((infoOf(objectOfId, keyframeId, "SkKeyframe")?["time"].as_f64()?, keyframeId)))
            .collect();
        keyframeIds.sort_by(|(time, _), (otherTime, _)| time.total_cmp(otherTime));
        let mut poses: Vec<(u64, String, Location3D)> = Vec::new();
        for (_, keyframeId) in keyframeIds {
            for poseId in childIdsOf(childrenOfId, keyframeId) {
                let Some(pose) = infoOf(objectOfId, poseId, "SkKeyframePose") else {
                    continue;
                };
                let (Some(jointName), Ok(localPose)) = (pose["jointName"].as_str(), serde_json::from_value::<Location3D>(pose["localPose"].clone())) else {
                    continue;
                };
                poses.push((poseId, jointName.to_string(), localPose));
            }
        }
        if poses.is_empty() {
            continue;
        }

        // the Skeleton an SkAnimator plays it on, or else the first one with every joint it keys
        let skeletonId = skeletonIdOfAnimationId.get(&animationId).copied().or_else(|| {
            skeletonIds.iter().copied().find(|skeletonId| {
                let restPoseOfJointName = &restPoseOfJointNameOfSkeletonId[skeletonId];
                poses.iter().all(|(_, jointName, _)| restPoseOfJointName.contains_key(jointName))
            })
        });
        let restPoseOfJointName = match skeletonId {
            Some(skeletonId) => restPoseOfJointNameOfSkeletonId[&skeletonId].clone(),
            None => {
                let mut restPoseOfJointName = HashMap::new();
                for (_, jointName, localPose) in &poses {
                    restPoseOfJointName.entry(jointName.clone()).or_insert(Location3D {
                        position: localPose.position,
                        quaternion: Quat::IDENTITY,
                    });
                }
                restPoseOfJointName
            }
        };
        for (poseId, jointName, localPose) in poses {
            let Some(restPose) = restPoseOfJointName.get(&jointName) else {
                continue;
            };
            let restRotationInverse = restPose.quaternion.inverse();
            offsetOfPoseId.push((
                poseId,
                Location3D {
                    position: restRotationInverse * (localPose.position - restPose.position),
                    quaternion: (restRotationInverse * localPose.quaternion).normalize(),
                },
            ));
        }
    }

    for (poseId, offset) in offsetOfPoseId {
        storedWorldJson["objectOfId"][poseId.to_string()]["info"]["SkKeyframePose"]["localPose"] = serde_json::to_value(offset)?;
    }
    Ok(())
}

// GUIs used to take an anchor: `game.guiService.show(guiKey, root, anchor)` put the root in a column the size of the screen, lined
// up against an edge, a corner or the middle ("top" when the anchor was left out), and text was light gray at 12.5px by default.
// GUIs place themselves with Tailwind now, so each script that uses the GUI service gets this stand-in for it, whose `show` still
// takes the anchor and shows the GUI the old way. Every `game.guiService` in the script becomes the stand-in, so copies of the
// service like `const gui = game.guiService` use it too.
const GUI_SERVICE_WITH_OLD_ANCHOR_CODE: &str = r#"// Kube.new added this when GUIs stopped taking an anchor. It shows this script's GUIs where their anchor used to put them.
const guiServiceWithOldAnchor = {
  show: (guiKey: string, rootRenderable: Parameters<typeof game.guiService.show>[1], anchor: string = "top") => {
    const placementClassNameOfAnchor: { [anchor: string]: string | undefined } = {
      topLeft: "justify-start items-start",
      top: "justify-start items-center",
      topRight: "justify-start items-end",
      left: "justify-center items-start",
      center: "justify-center items-center",
      right: "justify-center items-end",
      bottomLeft: "justify-end items-start",
      bottom: "justify-end items-center",
      bottomRight: "justify-end items-end",
    };
    const placementClassName = placementClassNameOfAnchor[anchor] ?? placementClassNameOfAnchor.top;
    game.guiService.show(guiKey, () => (
      <div className={`pointer-events-none absolute inset-0 flex flex-col text-[12.5px] text-[#8c8c8c] *:pointer-events-auto ${placementClassName}`}>
        {typeof rootRenderable === "function" ? rootRenderable() : rootRenderable}
      </div>
    ));
  },
  hide: (guiKey: string) => game.guiService.hide(guiKey),
};
"#;

fn stepToVersion85(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(scriptRawCodeOfId) = storedWorldJson.get_mut("scriptRawCodeOfId").and_then(|scripts| scripts.as_object_mut()) else {
        return Ok(());
    };
    for scriptRawCode in scriptRawCodeOfId.values_mut() {
        let Some(code) = scriptRawCode.as_str() else {
            continue;
        };
        // Hiding a GUI works the same as before, so a script that only hides GUIs stays as it is.
        if !code.replace("game.guiService.hide(", "").contains("game.guiService") {
            continue;
        }
        // A 'use client' or 'use server' line only counts at the very top of a script, so the stand-in goes after it.
        let codeAfterWhitespace = code.trim_start();
        let directive = ["'use client'", "\"use client\"", "'use server'", "\"use server\""]
            .into_iter()
            .find(|directive| codeAfterWhitespace.starts_with(directive))
            .map_or("", |directive| {
                let directiveEnd = code.len() - codeAfterWhitespace.len() + directive.len();
                let semicolonLength = usize::from(code[directiveEnd..].starts_with(';'));
                &code[..directiveEnd + semicolonLength]
            });
        let codeAfterDirective = code[directive.len()..].replace("game.guiService", "guiServiceWithOldAnchor");
        let lineBreakAfterDirective = if directive.is_empty() { "" } else { "\n" };
        *scriptRawCode = Value::from(format!(
            "{directive}{lineBreakAfterDirective}{GUI_SERVICE_WITH_OLD_ANCHOR_CODE}{codeAfterDirective}"
        ));
    }

    Ok(())
}

// Objects used to say whether they were anchored, fixed in place. They say whether they are pushable now, the opposite, and
// scripts that set or read anchoring are rewritten to set or read pushability instead.
fn stepToVersion86(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    if let Some(physicsToggles3DOfId) = storedWorldJson.get_mut("physicsToggles3DOfId").and_then(|toggles| toggles.as_object_mut()) {
        for physicsToggles3D in physicsToggles3DOfId.values_mut() {
            let Some(physicsToggles3D) = physicsToggles3D.as_object_mut() else {
                continue;
            };
            if let Some(isAnchored) = physicsToggles3D.remove("isAnchored") {
                let isAnchored = isAnchored.as_bool().ok_or_else(|| anyhow::anyhow!("stepToVersion86: isAnchored is not a bool"))?;
                physicsToggles3D.insert("isPushable".to_string(), Value::from(!isAnchored));
            }
        }
    }
    if let Some(scriptRawCodeOfId) = storedWorldJson.get_mut("scriptRawCodeOfId").and_then(|scripts| scripts.as_object_mut()) {
        for scriptRawCode in scriptRawCodeOfId.values_mut() {
            if let Some(code) = scriptRawCode.as_str()
                && code.contains("Anchored")
            {
                *scriptRawCode = Value::from(scriptWithPushableInsteadOfAnchored(code));
            }
        }
    }
    Ok(())
}

fn isIdentifierByte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

// The index just past the string or comment that starts at `start`, or None when nothing starts there.
fn skipStringOrComment(code: &[u8], start: usize) -> Option<usize> {
    match code[start] {
        quote @ (b'\'' | b'"' | b'`') => {
            let mut index = start + 1;
            while index < code.len() && code[index] != quote {
                index += if code[index] == b'\\' { 2 } else { 1 };
            }
            Some((index + 1).min(code.len()))
        }
        b'/' if code.get(start + 1) == Some(&b'/') => Some(code[start..].iter().position(|&byte| byte == b'\n').map_or(code.len(), |offset| start + offset)),
        b'/' if code.get(start + 1) == Some(&b'*') => Some(
            code[start + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(code.len(), |offset| start + 2 + offset + 2),
        ),
        _ => None,
    }
}

// Where the expression that starts at `start` ends: the first of `stops` outside any brackets, strings or comments, or the
// bracket that closes one opened before `start`. A line break in `stops` only ends an expression that has started.
fn expressionEnd(code: &[u8], start: usize, stops: &[u8]) -> usize {
    let mut depth = 0;
    let mut index = start;
    let mut hasStarted = false;
    while index < code.len() {
        if let Some(end) = skipStringOrComment(code, index) {
            index = end;
            hasStarted = true;
            continue;
        }
        let byte = code[index];
        if depth == 0 && ((stops.contains(&byte) && (byte != b'\n' || hasStarted)) || matches!(byte, b')' | b']' | b'}')) {
            return index;
        }
        hasStarted |= !byte.is_ascii_whitespace();
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    code.len()
}

// The start of the object a member access like `a.b[0].c().isAnchored` reads from, given the index of its dot.
fn memberReceiverStart(code: &[u8], dotIndex: usize) -> usize {
    let mut index = dotIndex;
    while index > 0 {
        let byte = code[index - 1];
        if isIdentifierByte(byte) || byte == b'.' || (byte == b'?' && code.get(index) == Some(&b'.')) {
            index -= 1;
        } else if byte == b')' || byte == b']' {
            let open = if byte == b')' { b'(' } else { b'[' };
            let mut depth = 0;
            let mut position = index - 1;
            loop {
                if code[position] == byte {
                    depth += 1;
                } else if code[position] == open {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                if position == 0 {
                    return index;
                }
                position -= 1;
            }
            index = position;
        } else {
            break;
        }
    }
    index
}

fn negatedExpression(expression: &str) -> String {
    let isSimple = |text: &str| !text.is_empty() && text.bytes().all(|byte| isIdentifierByte(byte) || byte == b'.');
    match expression {
        "true" => "false".to_string(),
        "false" => "true".to_string(),
        _ if expression.strip_prefix('!').is_some_and(isSimple) => expression[1..].to_string(),
        _ if isSimple(expression) => format!("!{expression}"),
        _ => format!("!({expression})"),
    }
}

// Replaces code[start..end] with its negation, keeping the whitespace around it.
fn pushNegated(output: &mut String, code: &str, start: usize, end: usize) {
    let expression = &code[start..end];
    let trimmed = expression.trim();
    let leading = &expression[..expression.len() - expression.trim_start().len()];
    let trailing = &expression[expression.trim_end().len()..];
    output.push_str(leading);
    output.push_str(&negatedExpression(trimmed));
    output.push_str(trailing);
}

fn scriptWithPushableInsteadOfAnchored(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut output = String::with_capacity(code.len());
    let mut copiedUntil = 0;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = skipStringOrComment(bytes, index) {
            // `'isAnchored' in object` asks whether the object has the field.
            let isFieldNameBeforeIn = code[end..].trim_start().strip_prefix("in").is_some_and(|rest| !rest.bytes().next().is_some_and(isIdentifierByte));
            if end - index == "'isAnchored'".len() && &code[index + 1..end - 1] == "isAnchored" && isFieldNameBeforeIn {
                output.push_str(&code[copiedUntil..index + 1]);
                output.push_str("isPushable");
                copiedUntil = end - 1;
            }
            index = end;
            continue;
        }
        let startsWord = index == 0 || !isIdentifierByte(bytes[index - 1]);
        let wordAt = |word: &str| startsWord && code[index..].starts_with(word) && !bytes.get(index + word.len()).is_some_and(|&byte| isIdentifierByte(byte));
        if wordAt("setObjectAnchored") {
            let afterName = index + "setObjectAnchored".len();
            let openParen = afterName + code[afterName..].len() - code[afterName..].trim_start().len();
            let argumentsEnd = expressionEnd(bytes, openParen + 1, b"");
            let afterArguments = code.get(argumentsEnd + 1..).unwrap_or("").trim_start();
            // A script's own function that happens to have the name is left alone.
            let isDefinition = code[..index].trim_end().ends_with("function") || afterArguments.starts_with('{') || afterArguments.starts_with("=>");
            if bytes.get(openParen) == Some(&b'(') && !isDefinition {
                let firstArgumentEnd = expressionEnd(bytes, openParen + 1, b",");
                if bytes.get(firstArgumentEnd) == Some(&b',') && !code[firstArgumentEnd + 1..expressionEnd(bytes, firstArgumentEnd + 1, b",")].trim().is_empty() {
                    let secondArgumentEnd = expressionEnd(bytes, firstArgumentEnd + 1, b",");
                    output.push_str(&code[copiedUntil..index]);
                    output.push_str("setObjectPushable");
                    output.push_str(&code[afterName..firstArgumentEnd + 1]);
                    pushNegated(&mut output, code, firstArgumentEnd + 1, secondArgumentEnd);
                    copiedUntil = secondArgumentEnd;
                    index = secondArgumentEnd;
                    continue;
                }
            }
        } else if wordAt("isAnchored") {
            let afterName = index + "isAnchored".len();
            let isMember = index > 0 && bytes[index - 1] == b'.';
            let rest = code[afterName..].trim_start();
            let restStart = code.len() - rest.len();
            let previousByte = code[..index].trim_end().bytes().last();
            if isMember && rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>") {
                let valueEnd = expressionEnd(bytes, restStart + 1, b",;\n");
                if code[restStart + 1..valueEnd].trim().is_empty() {
                    index = afterName;
                    continue;
                }
                output.push_str(&code[copiedUntil..index]);
                output.push_str("isPushable");
                output.push_str(&code[afterName..restStart + 1]);
                pushNegated(&mut output, code, restStart + 1, valueEnd);
                copiedUntil = valueEnd;
                index = valueEnd;
                continue;
            }
            if isMember {
                let receiverStart = memberReceiverStart(bytes, index - 1);
                let dotStart = if index >= 2 && bytes[index - 2] == b'?' { index - 2 } else { index - 1 };
                output.push_str(&code[copiedUntil..receiverStart]);
                output.push_str("(!");
                output.push_str(&code[receiverStart..dotStart]);
                output.push_str(&code[dotStart..index]);
                output.push_str("isPushable)");
                copiedUntil = afterName;
                index = afterName;
                continue;
            }
            if rest.starts_with(':') && matches!(previousByte, Some(b'{' | b',' | b';')) {
                let valueEnd = expressionEnd(bytes, restStart + 1, b",;");
                output.push_str(&code[copiedUntil..index]);
                output.push_str("isPushable");
                output.push_str(&code[afterName..restStart + 1]);
                if code[restStart + 1..valueEnd].trim() == "boolean" {
                    output.push_str(&code[restStart + 1..valueEnd]);
                } else {
                    pushNegated(&mut output, code, restStart + 1, valueEnd);
                }
                copiedUntil = valueEnd;
                index = valueEnd;
                continue;
            }
        }
        index += 1;
    }
    output.push_str(&code[copiedUntil..]);
    output
}

// A saved player's userid was the id of its connection, not an account, so saved players become guests. Scripts used userid to tell
// players apart, which a Player object's id does now, so they use that instead.
fn stepToVersion87(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    if let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) {
        for (_, object) in objectOfId.iter_mut() {
            let Some(player) = object
                .get_mut("info")
                .and_then(|info| info.get_mut("Player"))
                .and_then(|player| player.as_object_mut())
            else {
                continue;
            };
            player.insert("userid".to_string(), Value::Null);
        }
    }
    if let Some(scriptRawCodeOfId) = storedWorldJson.get_mut("scriptRawCodeOfId").and_then(|scripts| scripts.as_object_mut()) {
        for scriptRawCode in scriptRawCodeOfId.values_mut() {
            if let Some(code) = scriptRawCode.as_str()
                && code.to_ascii_lowercase().contains("userid")
            {
                *scriptRawCode = Value::from(scriptWithPlayerIdInsteadOfUserid(code));
            }
        }
    }

    Ok(())
}

// `.userid` becomes `.id`, and a check that a user id is a string, like `typeof userId !== 'string'` in a server function, checks for
// a number instead. A server function's first argument, `args[0]`, counts as a user id, since clients passed theirs first.
fn scriptWithPlayerIdInsteadOfUserid(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut output = String::with_capacity(code.len());
    let mut copiedUntil = 0;
    let mut index = 0;
    while index < bytes.len() {
        // a template literal can hold code in `${...}`, so only comments and quoted strings are skipped
        if bytes[index] != b'`'
            && let Some(end) = skipStringOrComment(bytes, index)
        {
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b".userid") && !bytes.get(index + ".userid".len()).is_some_and(|byte| isIdentifierByte(*byte)) {
            output.push_str(&code[copiedUntil..index]);
            output.push_str(".id");
            index += ".userid".len();
            copiedUntil = index;
            continue;
        }
        let startsWord = index == 0 || !isIdentifierByte(bytes[index - 1]);
        if startsWord
            && bytes[index..].starts_with(b"typeof")
            && let Some((end, checkForNumber)) = userIdStringCheckAt(code, index)
        {
            output.push_str(&code[copiedUntil..index]);
            output.push_str(&checkForNumber);
            index = end;
            copiedUntil = index;
            continue;
        }
        index += 1;
    }
    output.push_str(&code[copiedUntil..]);
    output
}

// When `typeof <user id> === 'string'` (or `!==`, `==`, `!=`) starts at `start`, where it ends and how it reads checking for a number.
fn userIdStringCheckAt(code: &str, start: usize) -> Option<(usize, String)> {
    let afterTypeof = code[start..].strip_prefix("typeof")?;
    let operandAndRest = afterTypeof.trim_start();
    if operandAndRest.len() == afterTypeof.len() {
        return None;
    }
    let operandLength = operandAndRest
        .bytes()
        .take_while(|byte| isIdentifierByte(*byte) || [b'.', b'?', b'[', b']'].contains(byte))
        .count();
    let operand = &operandAndRest[..operandLength];
    if operand != "args[0]" && !operand.to_ascii_lowercase().contains("userid") {
        return None;
    }
    let afterOperand = operandAndRest[operandLength..].trim_start();
    let operatorLength = ["===", "!==", "==", "!="]
        .into_iter()
        .find(|operator| afterOperand.starts_with(operator))?
        .len();
    let afterOperator = afterOperand[operatorLength..].trim_start();
    let quote = *afterOperator.as_bytes().first()?;
    if (quote != b'\'' && quote != b'"') || !afterOperator[1..].starts_with("string") || afterOperator.as_bytes().get(7) != Some(&quote) {
        return None;
    }
    let checkBeforeString = &code[start..code.len() - afterOperator.len()];
    let quote = quote as char;
    Some((
        code.len() - afterOperator.len() + "'string'".len(),
        format!("{}{quote}number{quote}", checkBeforeString.replace(".userid", ".id")),
    ))
}

pub const CHARACTER_APPEARANCE_SCRIPT_NAME: &str = "appearance";
pub const CHARACTER_APPEARANCE_SCRIPT: &str = include_str!("../../../assets/scripts/character_appearance.ts");

// Players wear their kube.new hat. A Character built like the default one, a Body holding a visible Head and no hat of its own, gets
// a HatSpot at the top middle of its Head that moves with the Head. A game with a HatSpot, added here or placed by hand, gets the
// appearance script that puts each player's hat there, unless it has one or its scripts already put hats on. These games' characters keep
// their own colors, so their appearance script paints no parts.
fn stepToVersion88(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    struct HatSpotToAdd {
        bodyId: String,
        location: Location3D,
        toggles: Value,
        attachment: Option<(String, String, Location3D)>, // the Head's SkAttachment: its name, joint, and the HatSpot's offset from the joint
    }

    let (Some(objectOfId), Some(childrenOfId)) = (
        storedWorldJson.get("objectOfId").and_then(|objects| objects.as_object()),
        storedWorldJson.get("childrenOfId").and_then(|children| children.as_object()),
    ) else {
        return Ok(());
    };
    let childIdsOf = |id: &str| -> Vec<String> {
        childrenOfId
            .get(id)
            .and_then(|childIds| childIds.as_array())
            .into_iter()
            .flatten()
            .map(|childId| childId.to_string())
            .collect()
    };
    let nameOf = |id: &str| objectOfId.get(id).and_then(|object| object["name"].as_str()).unwrap_or("");

    let mut hatSpotsToAdd = Vec::new();
    let mut hasHatSpot = false;
    for (characterId, character) in objectOfId {
        if character["name"] != "Character" {
            continue;
        }
        let Some(bodyId) = childIdsOf(characterId)
            .into_iter()
            .find(|childId| nameOf(childId) == "Body" && objectOfId[childId]["info"].get("RigidGroup").is_some())
        else {
            continue;
        };
        let partIds = childIdsOf(&bodyId);
        hasHatSpot |= partIds.iter().any(|partId| nameOf(partId) == "HatSpot");
        if partIds.iter().any(|partId| nameOf(partId).starts_with("Hat")) {
            continue;
        }
        let Some(headId) = partIds.into_iter().find(|partId| nameOf(partId) == "Head") else {
            continue;
        };
        let isVisible = ["Brick", "Ball", "Cylinder"].into_iter().any(|kind| {
            objectOfId[&headId]["info"][kind]["visualSettings"]["opacity"]
                .as_f64()
                .is_some_and(|opacity| opacity > 0.0)
        });
        let primitive = &storedWorldJson["primitiveSettingsOfId"][&headId];
        let halfHeight = primitive["Brick"]["size"][1]
            .as_f64()
            .map(|height| height / 2.0)
            .or_else(|| primitive["Ball"]["radius"].as_f64())
            .or_else(|| primitive["Cylinder"]["length"].as_f64().map(|length| length / 2.0));
        let Ok(headLocation) = serde_json::from_value::<Location3D>(storedWorldJson["physicsInfo3DOfId"][&headId]["location"].clone()) else {
            continue;
        };
        let (true, Some(halfHeight)) = (isVisible, halfHeight) else {
            continue;
        };
        let topOffset = glam::Vec3::new(0.0, halfHeight as f32, 0.0);
        let attachment = childIdsOf(&headId).into_iter().find_map(|attachmentId| {
            let attachment = &objectOfId[&attachmentId];
            let jointName = attachment["info"]["SkAttachment"]["jointName"].as_str()?;
            let headOffset = serde_json::from_value::<Location3D>(attachment["info"]["SkAttachment"]["offsetFromJoint"].clone()).ok()?;
            Some((
                nameOf(&attachmentId).to_string(),
                jointName.to_string(),
                Location3D {
                    position: headOffset.position + headOffset.quaternion * topOffset,
                    quaternion: headOffset.quaternion,
                },
            ))
        });
        let mut toggles = match storedWorldJson["physicsToggles3DOfId"][&headId].clone() {
            Value::Object(toggles) => Value::Object(toggles),
            _ => json!({ "isPushable": true, "density": 1.0 }),
        };
        toggles["isWalkthrough"] = Value::from(true);
        hatSpotsToAdd.push(HatSpotToAdd {
            bodyId,
            location: Location3D {
                position: headLocation.position + headLocation.quaternion * topOffset,
                quaternion: headLocation.quaternion,
            },
            toggles,
            attachment,
        });
    }
    if hatSpotsToAdd.is_empty() && !hasHatSpot {
        return Ok(());
    }
    let isDressedByAScript = objectOfId
        .values()
        .any(|object| object["name"] == CHARACTER_APPEARANCE_SCRIPT_NAME && object["info"].get("Script").is_some())
        || storedWorldJson["scriptRawCodeOfId"]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(_, code)| code.as_str().is_some_and(|code| code.contains("getEquippedItemId")));
    let rootObjectId = storedWorldJson["rootObjectId"].to_string();
    let scriptsFolderId = childIdsOf(&rootObjectId).into_iter().find(|childId| nameOf(childId) == "scripts");
    let mut nextObjectId = objectOfId.keys().filter_map(|id| id.parse::<u64>().ok()).max().unwrap_or(0) + 1;

    let mut addObject = |storedWorldJson: &mut Value, parentId: &str, object: Value| -> String {
        let id = nextObjectId;
        nextObjectId += 1;
        let mut object = object;
        object["id"] = Value::from(id);
        storedWorldJson["objectOfId"][id.to_string()] = object;
        storedWorldJson["childrenOfId"][id.to_string()] = json!([]);
        if let Some(siblingIds) = storedWorldJson["childrenOfId"][parentId].as_array_mut() {
            siblingIds.push(Value::from(id));
        }
        id.to_string()
    };
    for HatSpotToAdd {
        bodyId,
        location,
        toggles,
        attachment,
    } in hatSpotsToAdd
    {
        let hatSpotId = addObject(
            storedWorldJson,
            &bodyId,
            json!({ "name": "HatSpot", "info": { "Brick": { "material": null, "visualSettings": { "opacity": 0.0, "color": { "r": 1.0, "g": 1.0, "b": 1.0 }, "glow": 0.0, "isAlwaysOnTop": false } } } }),
        );
        storedWorldJson["physicsInfo3DOfId"][&hatSpotId] =
            json!({ "location": location, "movement": { "velocity": [0.0, 0.0, 0.0], "angularVelocity": [0.0, 0.0, 0.0] } });
        storedWorldJson["physicsToggles3DOfId"][&hatSpotId] = toggles;
        storedWorldJson["primitiveSettingsOfId"][&hatSpotId] = json!({ "Brick": { "size": [0.1, 0.1, 0.1] } });
        if let Some((attachmentName, jointName, offsetFromJoint)) = attachment {
            let hatSpotObjectId: u64 = hatSpotId.parse()?;
            addObject(
                storedWorldJson,
                &hatSpotId,
                json!({ "name": attachmentName, "info": { "SkAttachment": { "objectId": hatSpotObjectId, "jointName": jointName, "offsetFromJoint": offsetFromJoint } } }),
            );
        }
    }
    if !isDressedByAScript {
        let scriptId = addObject(
            storedWorldJson,
            &scriptsFolderId.unwrap_or(rootObjectId),
            json!({ "name": CHARACTER_APPEARANCE_SCRIPT_NAME, "info": { "Script": { "objectLocation": "Server" } } }),
        );
        let code = CHARACTER_APPEARANCE_SCRIPT.replace(
            "const PREFERRED_COLOR_PARTS = [\"Torso\", \"Cap\"];",
            "const PREFERRED_COLOR_PARTS: string[] = [];",
        );
        storedWorldJson["scriptRawCodeOfId"][&scriptId] = Value::from(code);
    }
    Ok(())
}

// Imported models were tinted gray, which darkened their textures. A model whose texture is still under that gray, saved before or
// after v76 made colors sRGB, shows the texture's own colors. A model without a texture keeps its gray.
fn stepToVersion89(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(objectOfId) = storedWorldJson.get_mut("objectOfId").and_then(|objects| objects.as_object_mut()) else {
        return Ok(());
    };
    let texturedMaterialIds = texturedMaterialIdsOf(objectOfId);
    let oldGrays = [0.5, srgbEncodedChannel(0.5)].map(|gray| json!({ "r": gray, "g": gray, "b": gray }));
    for (_, object) in objectOfId.iter_mut() {
        let Some(mesh) = object.get_mut("info").and_then(|info| info.get_mut("Mesh")) else {
            continue;
        };
        let isTextured = mesh
            .get("material")
            .and_then(|material| material.get("materialInfoId"))
            .and_then(|materialInfoId| materialInfoId.as_u64())
            .is_some_and(|materialInfoId| texturedMaterialIds.contains(&materialInfoId));
        let Some(color) = mesh.get_mut("visualSettings").and_then(|visualSettings| visualSettings.get_mut("color")) else {
            continue;
        };
        if isTextured && oldGrays.iter().any(|oldGray| isNearlyEqual(color, oldGray)) {
            *color = json!({ "r": 1.0, "g": 1.0, "b": 1.0 });
        }
    }
    Ok(())
}

// getPreferredColor and getEquippedItemId are gone, because a player's preferredColor and equippedItemId are on the player object now.
// Each call reads that field instead, still as a promise, so whatever waited on the call, like an await, Promise.all or .catch, still works.
fn stepToVersion90(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    let Some(scriptRawCodeOfId) = storedWorldJson.get_mut("scriptRawCodeOfId").and_then(|scripts| scripts.as_object_mut()) else {
        return Ok(());
    };
    for scriptRawCode in scriptRawCodeOfId.values_mut() {
        let Some(code) = scriptRawCode.as_str() else {
            continue;
        };
        let bytes = code.as_bytes();
        let mut output = String::with_capacity(code.len());
        let mut copiedUntil = 0;
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'`'
                && let Some(end) = skipStringOrComment(bytes, index)
            {
                index = end;
                continue;
            }
            let Some((call, field)) = [
                ("game.playerService.getPreferredColor(", "preferredColor"),
                ("game.playerService.getEquippedItemId(", "equippedItemId"),
            ]
            .into_iter()
            .find(|(call, _)| bytes[index..].starts_with(call.as_bytes())) else {
                index += 1;
                continue;
            };
            let argumentsStart = index + call.len();
            let argumentsEnd = expressionEnd(bytes, argumentsStart, b"");
            if bytes.get(argumentsEnd) != Some(&b')') {
                index += 1;
                continue;
            }
            output.push_str(&code[copiedUntil..index]);
            output.push_str(&format!(
                "Promise.resolve(((player) => player?.type === \"player\" ? player.{field} : null)(game.objectService.getObjectById({})))",
                &code[argumentsStart..argumentsEnd]
            ));
            index = argumentsEnd + 1;
            copiedUntil = index;
        }
        if copiedUntil > 0 {
            output.push_str(&code[copiedUntil..]);
            *scriptRawCode = Value::from(output);
        }
    }
    Ok(())
}

// Renames every whole oldName in every script, comments and strings included, so none is missed however the script reaches it.
fn renameInEveryScript(storedWorldJson: &mut Value, oldName: &str, newName: &str) {
    let Some(scriptRawCodeOfId) = storedWorldJson.get_mut("scriptRawCodeOfId").and_then(|scripts| scripts.as_object_mut()) else {
        return;
    };
    for scriptRawCode in scriptRawCodeOfId.values_mut() {
        let Some(code) = scriptRawCode.as_str() else {
            continue;
        };
        let bytes = code.as_bytes();
        let mut output = String::with_capacity(code.len());
        let mut copiedUntil = 0;
        for (index, _) in code.match_indices(oldName) {
            let isWholeName =
                (index == 0 || !isIdentifierByte(bytes[index - 1])) && !bytes.get(index + oldName.len()).is_some_and(|&byte| isIdentifierByte(byte));
            if !isWholeName {
                continue;
            }
            output.push_str(&code[copiedUntil..index]);
            output.push_str(newName);
            copiedUntil = index + oldName.len();
        }
        if copiedUntil > 0 {
            output.push_str(&code[copiedUntil..]);
            *scriptRawCode = Value::from(output);
        }
    }
}

// Every move a script makes is smooth now, and teleportObjectTo is the one that jumps. moveObjectTo used to jump, so each call to it
// becomes a call to teleportObjectTo. Every moveObjectTo is renamed, comments and strings included, so none is missed however the script reaches it.
fn stepToVersion91(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    renameInEveryScript(storedWorldJson, "moveObjectTo", "teleportObjectTo");
    Ok(())
}

// The script API package is kube-ts-api now. Its old name, voidscript-js, can be in a script's imports or its JSX pragma.
fn stepToVersion92(storedWorldJson: &mut Value) -> anyhow::Result<()> {
    renameInEveryScript(storedWorldJson, "voidscript-js", "kube-ts-api");
    Ok(())
}
