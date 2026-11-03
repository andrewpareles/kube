mod helpers;

use deno_core::{JsRuntime, RuntimeOptions, serde_v8};
use game_types::object_types::{
    BillboardMode, ClockState, InterpolationStyle, LabelHorizontalAlign, MaterialInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation,
    PhysicsInfo3D, PhysicsToggles3D, WorldLightingInfo, material::AlphaMode, shared::euler_degrees_to_quat,
};
use glam::{EulerRot, Quat, Vec3};
use helpers::world_test_helpers::{
    add_ball, add_brick, add_clock, add_cylinder, add_label, add_label_image, add_local_group, add_mesh, add_pivot, add_player, add_property_animation,
    add_property_animator, add_property_keyframe, add_rigid_group, add_script, add_sk_animation, add_sk_animator, add_sk_attachment, add_sk_keyframe,
    add_sk_keyframe_pose, add_skeleton, add_sound_player, make_world,
};
use serde_json::{Value, json};
use shared::game::jsRustConversions::{ValueWithPath, jsLiveObjectOfRustLiveObject, jsObjectId, rustObjectBeingAddedOfJSObjectInfo, rustUpdateObject};
use shared::game::physics::SetOrForce;
use shared::game::world::World;

// -------------------------- Conversion Test Helpers ---------------

const FLOAT_EPSILON: f32 = 1e-5;
const QUAT_EPSILON: f32 = 1e-4;

fn assert_close_f32(actual: f32, expected: f32, label: &str) {
    let difference = (actual - expected).abs();
    assert!(
        difference <= FLOAT_EPSILON,
        "{label} mismatch: actual={actual}, expected={expected}, difference={difference}"
    );
}

fn assert_vec3_close(actual: Vec3, expected: Vec3, label: &str) {
    assert_close_f32(actual.x, expected.x, &format!("{label}.x"));
    assert_close_f32(actual.y, expected.y, &format!("{label}.y"));
    assert_close_f32(actual.z, expected.z, &format!("{label}.z"));
}

fn assert_quat_close(actual: Quat, expected: Quat, label: &str) {
    let actual_normalized = actual.normalize();
    let expected_normalized = expected.normalize();
    let dot = actual_normalized.dot(expected_normalized).abs();
    let difference = (1.0 - dot).abs();
    assert!(difference <= QUAT_EPSILON, "{label} mismatch: abs(dot)={dot}, difference={difference}");
}

fn update_object_leaf(world: &mut World, object_id: ObjectId, path: &[&str], leaf_value: Value) -> anyhow::Result<()> {
    let path_strings = path.iter().map(|field_name| (*field_name).to_string()).collect::<Vec<String>>();
    let value_with_path = ValueWithPath::new(&leaf_value, &path_strings);
    let object_modify_flags = rustUpdateObject(world, object_id, value_with_path, SetOrForce::Force)?;
    world.serverMarkObjectAsModified(object_id, object_modify_flags);
    Ok(())
}

fn js_live_object_json(js_runtime: &mut JsRuntime, world: &World, object_id: ObjectId) -> anyhow::Result<Value> {
    deno_core::scope!(scope, js_runtime);
    let js_object = jsLiveObjectOfRustLiveObject(world, object_id, scope)?;
    let json_value: Value = serde_v8::from_v8(scope, js_object.into())?;
    Ok(json_value)
}

fn add_material_without_textures(world: &mut World, object_id: ObjectId, parent_id: ObjectId) -> anyhow::Result<()> {
    let material_info = MaterialInfo {
        albedoTextureDataId: None,
        metallicTextureDataId: None,
        roughnessTextureDataId: None,
        normalTextureDataId: None,
        alphaMode: AlphaMode::Opaque,
        isDoubleSided: false,
    };

    world.addObjectWithId(object_id, format!("material-{object_id}"), ObjectBeingAdded::Material(material_info), parent_id);
    Ok(())
}

fn expect_pointer_id(pointer: Option<ObjectIdPointer>) -> Option<ObjectId> {
    pointer.map(ObjectId::from)
}

// -------------------------- JS -> Rust Object Add Conversions ---------------

#[test]
fn rust_object_being_added_converts_all_supported_types() -> anyhow::Result<()> {
    // This section checks conversion for clock objects.
    let clock_value = json!({
        "type": "clock",
        "shouldLoop": true,
        "clockState": {
            "Playing": {
                "localStartMs": 123.25,
                "timeLocation": "Server"
            }
        },
        "speed": 2.5,
    });
    let clock_converted = rustObjectBeingAddedOfJSObjectInfo(&clock_value)?;
    let ObjectBeingAdded::Clock(clock_info) = clock_converted else {
        return Err(anyhow::anyhow!("Expected clock conversion"));
    };
    assert_eq!(
        clock_info.clockState,
        ClockState::Playing {
            localStartMs: 123.25,
            timeLocation: ObjectLocation::Server,
        }
    );
    assert!(clock_info.shouldLoop);
    assert_close_f32(clock_info.speed, 2.5, "clock.speed");

    // This section checks conversion for ball objects, including nested physics and visuals.
    let ball_value = json!({
        "type": "ball",
        "radius": 1.75,
        "material": {
            "materialInfoId": 17,
            "tileSizeU": 2.5,
            "tileSizeV": 1.5,
            "tileOffsetU": 0.25,
            "tileOffsetV": 0.75
        },
        "opacity": 0.75,
        "color": { "r": 0.1, "g": 0.2, "b": 0.3 },
        "glow": 0.4,
        "isAlwaysOnTop": true,
        "isWalkthrough": true,
        "isPushable": true,
        "density": 8.5,
        "location": {
            "position": { "x": 1.0, "y": 2.0, "z": 3.0 },
            "rotation": { "x": 15.0, "y": -20.0, "z": 30.0 }
        },
        "movement": {
            "velocity": { "x": 4.0, "y": 5.0, "z": 6.0 },
            "angularVelocity": { "x": -10.0, "y": -20.0, "z": -30.0 }
        }
    });
    let ball_converted = rustObjectBeingAddedOfJSObjectInfo(&ball_value)?;
    let ObjectBeingAdded::Ball(ball_info, ball_primitive_settings, ball_physics_info, ball_physics_toggles) = ball_converted else {
        return Err(anyhow::anyhow!("Expected ball conversion"));
    };
    assert_close_f32(ball_primitive_settings.radius, 1.75, "ball.radius");
    let Some(ball_material_settings_3d) = ball_info.material else {
        return Err(anyhow::anyhow!("Expected ball material settings"));
    };
    assert_eq!(expect_pointer_id(Some(ball_material_settings_3d.materialInfoId)), Some(17));
    assert_close_f32(ball_material_settings_3d.tileSizeU, 2.5, "ball.material.tileSizeU");
    assert_close_f32(ball_material_settings_3d.tileSizeV, 1.5, "ball.material.tileSizeV");
    assert_close_f32(ball_material_settings_3d.tileOffsetU, 0.25, "ball.material.tileOffsetU");
    assert_close_f32(ball_material_settings_3d.tileOffsetV, 0.75, "ball.material.tileOffsetV");
    assert_close_f32(ball_info.visualSettings.opacity, 0.75, "ball.opacity");
    assert_close_f32(ball_info.visualSettings.glow, 0.4, "ball.glow");
    assert!(ball_info.visualSettings.isAlwaysOnTop);
    assert!(ball_physics_toggles.isWalkthrough);
    assert!(ball_physics_toggles.isPushable);
    assert_close_f32(ball_physics_toggles.density, 8.5, "ball.density");
    assert_vec3_close(ball_physics_info.location.position, Vec3::new(1.0, 2.0, 3.0), "ball.location.position");
    assert_quat_close(
        ball_physics_info.location.quaternion,
        // the angles apply y first, then x, then z
        Quat::from_rotation_y(-20.0_f32.to_radians()) * Quat::from_rotation_x(15.0_f32.to_radians()) * Quat::from_rotation_z(30.0_f32.to_radians()),
        "ball.location.rotation",
    );
    assert_vec3_close(ball_physics_info.movement.velocity, Vec3::new(4.0, 5.0, 6.0), "ball.movement.velocity");
    assert_vec3_close(
        ball_physics_info.movement.angularVelocity,
        Vec3::new(-10.0_f32.to_radians(), -20.0_f32.to_radians(), -30.0_f32.to_radians()),
        "ball.movement.angularVelocity",
    );

    // This section checks conversion for cylinder objects, including its Z-axis primitive dimensions.
    let cylinder_value = json!({
        "type": "cylinder",
        "radius": 1.25,
        "length": 7.5,
        "material": {
            "materialInfoId": 18,
            "tileSizeU": 3.5,
            "tileSizeV": 2.5,
            "tileOffsetU": -0.25,
            "tileOffsetV": 1.25
        },
        "opacity": 0.6,
        "color": { "r": 0.7, "g": 0.8, "b": 0.9 },
        "isAlwaysOnTop": false,
        "isWalkthrough": false,
        "isPushable": false,
        "density": 4.5,
        "location": {
            "position": { "x": 3.0, "y": 2.0, "z": 1.0 },
            "rotation": { "x": 0.0, "y": 0.0, "z": 45.0 }
        },
        "movement": {
            "velocity": { "x": 0.0, "y": 1.0, "z": 2.0 },
            "angularVelocity": { "x": 5.0, "y": 10.0, "z": 15.0 }
        }
    });
    let cylinder_converted = rustObjectBeingAddedOfJSObjectInfo(&cylinder_value)?;
    let ObjectBeingAdded::Cylinder(cylinder_info, cylinder_primitive_settings, cylinder_physics_info, cylinder_physics_toggles) = cylinder_converted else {
        return Err(anyhow::anyhow!("Expected cylinder conversion"));
    };
    assert_close_f32(cylinder_primitive_settings.radius, 1.25, "cylinder.radius");
    assert_close_f32(cylinder_primitive_settings.length, 7.5, "cylinder.length");
    let Some(cylinder_material_settings_3d) = cylinder_info.material else {
        return Err(anyhow::anyhow!("Expected cylinder material settings"));
    };
    assert_eq!(expect_pointer_id(Some(cylinder_material_settings_3d.materialInfoId)), Some(18));
    assert_close_f32(cylinder_material_settings_3d.tileSizeU, 3.5, "cylinder.material.tileSizeU");
    assert_close_f32(cylinder_material_settings_3d.tileSizeV, 2.5, "cylinder.material.tileSizeV");
    assert_close_f32(cylinder_material_settings_3d.tileOffsetU, -0.25, "cylinder.material.tileOffsetU");
    assert_close_f32(cylinder_material_settings_3d.tileOffsetV, 1.25, "cylinder.material.tileOffsetV");
    assert_close_f32(cylinder_info.visualSettings.opacity, 0.6, "cylinder.opacity");
    assert!(!cylinder_physics_toggles.isPushable);
    assert_close_f32(cylinder_physics_toggles.density, 4.5, "cylinder.density");
    assert_vec3_close(cylinder_physics_info.location.position, Vec3::new(3.0, 2.0, 1.0), "cylinder.location.position");
    assert_quat_close(
        cylinder_physics_info.location.quaternion,
        Quat::from_euler(EulerRot::XYZ, 0.0, 0.0, 45.0_f32.to_radians()),
        "cylinder.location.rotation",
    );

    // This section checks conversion for brick objects, including defaults for optional visual fields.
    let brick_value = json!({
        "type": "brick",
        "size": { "x": 4.0, "y": 5.0, "z": 6.0 },
        "material": null,
        "opacity": 0.4,
        "color": { "r": 0.4, "g": 0.5, "b": 0.6 },
        "isWalkthrough": false,
        "isPushable": false,
        "density": 3.0,
        "location": {
            "position": { "x": -1.0, "y": -2.0, "z": -3.0 },
            "rotation": { "x": 0.0, "y": 45.0, "z": 0.0 }
        },
        "movement": {
            "velocity": { "x": 1.0, "y": 0.0, "z": -1.0 },
            "angularVelocity": { "x": 0.0, "y": 90.0, "z": 0.0 }
        }
    });
    let brick_converted = rustObjectBeingAddedOfJSObjectInfo(&brick_value)?;
    let ObjectBeingAdded::Brick(brick_info, brick_primitive_settings, brick_physics_info, brick_physics_toggles) = brick_converted else {
        return Err(anyhow::anyhow!("Expected brick conversion"));
    };
    assert_vec3_close(brick_primitive_settings.size, Vec3::new(4.0, 5.0, 6.0), "brick.size");
    assert!(brick_info.material.is_none());
    assert_close_f32(brick_info.visualSettings.opacity, 0.4, "brick.opacity");
    assert!(!brick_info.visualSettings.isAlwaysOnTop);
    assert!(!brick_physics_toggles.isWalkthrough);
    assert!(!brick_physics_toggles.isPushable);
    assert_close_f32(brick_physics_toggles.density, 3.0, "brick.density");
    assert_vec3_close(brick_physics_info.location.position, Vec3::new(-1.0, -2.0, -3.0), "brick.location.position");
    assert_quat_close(
        brick_physics_info.location.quaternion,
        Quat::from_euler(EulerRot::XYZ, 0.0, 45.0_f32.to_radians(), 0.0),
        "brick.location.rotation",
    );

    // This section checks that physics density falls back to 1.0 when scripts omit it.
    let default_density_ball_value = json!({
        "type": "ball",
        "radius": 0.5,
        "material": null,
        "opacity": 1.0,
        "color": { "r": 1.0, "g": 1.0, "b": 1.0 },
        "isAlwaysOnTop": false,
        "isWalkthrough": false,
        "isPushable": true,
        "location": {
            "position": { "x": 0.0, "y": 0.0, "z": 0.0 },
            "rotation": { "x": 0.0, "y": 0.0, "z": 0.0 }
        },
        "movement": {
            "velocity": { "x": 0.0, "y": 0.0, "z": 0.0 },
            "angularVelocity": { "x": 0.0, "y": 0.0, "z": 0.0 }
        }
    });
    let default_density_ball_converted = rustObjectBeingAddedOfJSObjectInfo(&default_density_ball_value)?;
    let ObjectBeingAdded::Ball(_, _, _, default_density_ball_physics_toggles) = default_density_ball_converted else {
        return Err(anyhow::anyhow!("Expected ball conversion with default density"));
    };
    assert_close_f32(default_density_ball_physics_toggles.density, 1.0, "ball.defaultDensity");

    // This section checks conversion for local-group objects.
    let group_value = json!({
        "type": "group"
    });
    let group_converted = rustObjectBeingAddedOfJSObjectInfo(&group_value)?;
    let ObjectBeingAdded::LocalGroup(_) = group_converted else {
        return Err(anyhow::anyhow!("Expected group conversion"));
    };

    // This section checks conversion for material data-id optionals.
    let material_value = json!({
        "type": "material",
        "albedoTextureDataId": 101,
        "metallicTextureDataId": null,
        "roughnessTextureDataId": 303,
        "normalTextureDataId": null,
        "isDoubleSided": true,
        "alphaMode": {
            "Mask": {
                "cutoff": 0.42
            }
        }
    });
    let material_converted = rustObjectBeingAddedOfJSObjectInfo(&material_value)?;
    let ObjectBeingAdded::Material(material_info) = material_converted else {
        return Err(anyhow::anyhow!("Expected material conversion"));
    };
    assert_eq!(material_info.albedoTextureDataId, Some(101));
    assert_eq!(material_info.metallicTextureDataId, None);
    assert_eq!(material_info.roughnessTextureDataId, Some(303));
    assert_eq!(material_info.normalTextureDataId, None);
    let AlphaMode::Mask { cutoff } = material_info.alphaMode else {
        return Err(anyhow::anyhow!("Expected material alpha mode mask"));
    };
    assert_close_f32(cutoff, 0.42, "material.alphaMode.Mask.cutoff");
    assert!(material_info.isDoubleSided);

    // This section checks conversion for world lighting.
    let world_lighting_value = json!({
        "type": "worldLighting",
        "sunOrientation": { "theta": 90.0, "phi": -30.0 },
        "sunColor": { "r": 1.0, "g": 0.9, "b": 0.8 },
        "sunIntensity": 3.25,
        "sunShadowSoftness": 1.5,
        "ambientTopColor": { "r": 0.04, "g": 0.05, "b": 0.06 },
        "ambientBottomColor": { "r": 0.01, "g": 0.02, "b": 0.03 },
        "ambientIntensity": 0.75,
        "ambientOcclusionIntensity": 0.4,
        "ambientOcclusionRadius": 2.5,
        "ambientOcclusionSunlightStrength": 0.2,
        "skyTopColor": { "r": 0.12, "g": 0.18, "b": 0.28 },
        "skyHorizonColor": { "r": 0.62, "g": 0.74, "b": 0.86 },
        "skyBottomColor": { "r": 0.2, "g": 0.19, "b": 0.17 },
        "skyboxTextureDataId": 987,
        "skyboxTextureOpacity": 0.35,
        "skyReflectionStrength": 0.45,
        "vibrance": 0.35
    });
    let world_lighting_converted = rustObjectBeingAddedOfJSObjectInfo(&world_lighting_value)?;
    let ObjectBeingAdded::WorldLighting(world_lighting_info) = world_lighting_converted else {
        return Err(anyhow::anyhow!("Expected world lighting conversion"));
    };
    assert_close_f32(
        world_lighting_info.sunOrientation.theta,
        90.0_f32.to_radians(),
        "worldLighting.sunOrientation.theta",
    );
    assert_close_f32(
        world_lighting_info.sunOrientation.phi,
        30.0_f32.to_radians(),
        "worldLighting.sunOrientation.phi",
    );
    assert_close_f32(world_lighting_info.sunColor.g, 0.9, "worldLighting.sunColor.g");
    assert_close_f32(world_lighting_info.sunIntensity, 3.25, "worldLighting.sunIntensity");
    assert_close_f32(world_lighting_info.sunShadowSoftness, 1.5, "worldLighting.sunShadowSoftness");
    assert_close_f32(world_lighting_info.ambientTopColor.b, 0.06, "worldLighting.ambientTopColor.b");
    assert_close_f32(world_lighting_info.ambientBottomColor.r, 0.01, "worldLighting.ambientBottomColor.r");
    assert_close_f32(world_lighting_info.ambientIntensity, 0.75, "worldLighting.ambientIntensity");
    assert_close_f32(world_lighting_info.ambientOcclusionIntensity, 0.4, "worldLighting.ambientOcclusionIntensity");
    assert_close_f32(world_lighting_info.ambientOcclusionRadius, 2.5, "worldLighting.ambientOcclusionRadius");
    assert_close_f32(
        world_lighting_info.ambientOcclusionSunlightStrength,
        0.2,
        "worldLighting.ambientOcclusionSunlightStrength",
    );
    assert_close_f32(world_lighting_info.skyTopColor.g, 0.18, "worldLighting.skyTopColor.g");
    assert_close_f32(world_lighting_info.skyHorizonColor.g, 0.74, "worldLighting.skyHorizonColor.g");
    assert_close_f32(world_lighting_info.skyBottomColor.g, 0.19, "worldLighting.skyBottomColor.g");
    assert_eq!(world_lighting_info.skyboxTextureDataId, Some(987));
    assert_close_f32(world_lighting_info.skyboxTextureOpacity, 0.35, "worldLighting.skyboxTextureOpacity");
    assert_close_f32(world_lighting_info.skyReflectionStrength, 0.45, "worldLighting.skyReflectionStrength");
    assert_close_f32(world_lighting_info.vibrance, 0.35, "worldLighting.vibrance");

    // This section checks conversion for labels, which a script must be able to create so that
    // text can be put in the world while the game runs, not only while it is being authored.
    let mut label_value = json!({
        "type": "label",
        "text": "HAPPY BIRTHDAY",
        "lineHeight": 0.75,
        "maxWidth": 6.5,
        "horizontalAlign": "right",
        "backgroundColor": { "r": 0.1, "g": 0.2, "b": 0.3 },
        "backgroundOpacity": 0.6,
        "offsetFromObject": { "x": 1.0, "y": 2.0, "z": 3.0 },
        "rotationFromObject": { "x": 0.0, "y": 90.0, "z": 0.0 },
        "physicalObjectId": 41,
        "billboardMode": "full",
        "isConstantSize": true,
        "opacity": 0.8,
        "color": { "r": 0.5, "g": 0.6, "b": 0.7 },
        "isAlwaysOnTop": true
    });
    let label_converted = rustObjectBeingAddedOfJSObjectInfo(&label_value)?;
    let ObjectBeingAdded::Label(label_info) = label_converted else {
        return Err(anyhow::anyhow!("Expected label conversion"));
    };
    assert_eq!(label_info.text, "HAPPY BIRTHDAY");
    assert_close_f32(label_info.lineHeight, 0.75, "label.lineHeight");
    assert_eq!(label_info.maxWidth, Some(6.5));
    assert_eq!(label_info.horizontalAlign, LabelHorizontalAlign::Right);
    let Some(label_background_color) = label_info.backgroundColor else {
        return Err(anyhow::anyhow!("Expected label background color"));
    };
    assert_close_f32(label_background_color.b, 0.3, "label.backgroundColor.b");
    assert_close_f32(label_info.backgroundOpacity, 0.6, "label.backgroundOpacity");
    assert_vec3_close(label_info.offsetFromObject, Vec3::new(1.0, 2.0, 3.0), "label.offsetFromObject");
    assert_quat_close(
        label_info.rotationFromObject,
        Quat::from_euler(EulerRot::XYZ, 0.0, 90.0_f32.to_radians(), 0.0),
        "label.rotationFromObject",
    );
    assert_eq!(expect_pointer_id(label_info.physicalObjectId), Some(41));
    assert_eq!(label_info.billboardMode, BillboardMode::Full);
    assert!(label_info.isConstantSize);
    assert_close_f32(label_info.visualSettings.opacity, 0.8, "label.visualSettings.opacity");
    assert!(label_info.visualSettings.isAlwaysOnTop);

    // Label images too, so a script can put a picture in the world.
    let label_image_value = json!({
        "type": "labelImage",
        "textureDataId": 77,
        "width": 3.0,
        "height": 1.5,
        "offsetFromObject": { "x": 0.0, "y": 0.0, "z": 0.51 },
        "rotationFromObject": { "x": 0.0, "y": 90.0, "z": 0.0 },
        "physicalObjectId": 41,
        "billboardMode": "none",
        "isConstantSize": false,
        "opacity": 0.9,
        "color": { "r": 1.0, "g": 1.0, "b": 1.0 },
        "isAlwaysOnTop": false
    });
    let ObjectBeingAdded::LabelImage(label_image_info) = rustObjectBeingAddedOfJSObjectInfo(&label_image_value)? else {
        return Err(anyhow::anyhow!("Expected label image conversion"));
    };
    assert_eq!(label_image_info.textureDataId, 77);
    assert_close_f32(label_image_info.width, 3.0, "labelImage.width");
    assert_close_f32(label_image_info.height, 1.5, "labelImage.height");
    assert_vec3_close(label_image_info.offsetFromObject, Vec3::new(0.0, 0.0, 0.51), "labelImage.offsetFromObject");
    assert_quat_close(
        label_image_info.rotationFromObject,
        Quat::from_euler(EulerRot::XYZ, 0.0, 90.0_f32.to_radians(), 0.0),
        "labelImage.rotationFromObject",
    );
    assert_eq!(expect_pointer_id(label_image_info.physicalObjectId), Some(41));
    assert_eq!(label_image_info.billboardMode, BillboardMode::None);
    assert_close_f32(label_image_info.visualSettings.opacity, 0.9, "labelImage.visualSettings.opacity");

    // Removed billboard modes must fail when scripts create a label.
    label_value["billboardMode"] = json!("upright");
    let error = rustObjectBeingAddedOfJSObjectInfo(&label_value).expect_err("upright billboarding should be rejected");
    assert!(error.to_string().contains("billboardMode must be none|full"));

    // A label that never wraps and has no background passes null for both, and it must round
    // trip as None rather than being rejected.
    let plain_label_value = json!({
        "type": "label",
        "text": "MOM",
        "lineHeight": 0.5,
        "maxWidth": null,
        "horizontalAlign": "center",
        "backgroundColor": null,
        "backgroundOpacity": 0.0,
        "offsetFromObject": { "x": 0.0, "y": 0.45, "z": 0.0 },
        "rotationFromObject": { "x": 0.0, "y": 0.0, "z": 0.0 },
        "physicalObjectId": null,
        "billboardMode": "none",
        "isConstantSize": false,
        "opacity": 1.0,
        "color": { "r": 1.0, "g": 1.0, "b": 1.0 },
        "isAlwaysOnTop": false
    });
    let plain_label_converted = rustObjectBeingAddedOfJSObjectInfo(&plain_label_value)?;
    let ObjectBeingAdded::Label(plain_label_info) = plain_label_converted else {
        return Err(anyhow::anyhow!("Expected plain label conversion"));
    };
    assert_eq!(plain_label_info.maxWidth, None);
    assert!(plain_label_info.backgroundColor.is_none());
    assert_eq!(expect_pointer_id(plain_label_info.physicalObjectId), None);
    assert_eq!(plain_label_info.billboardMode, BillboardMode::None);

    // This section checks conversion for mesh pointer fields, nested material settings, and visual defaults.
    let mesh_value = json!({
        "type": "mesh",
        "scale": { "x": 2.0, "y": 3.0, "z": 4.0 },
        "meshDataId": 7001,
        "followSkeletonId": 31,
        "physicalObjectId": 32,
        "material": { "materialInfoId": 33 },
        "opacity": 0.95,
        "color": { "r": 0.2, "g": 0.3, "b": 0.4 },
        "isAlwaysOnTop": false
    });
    let mesh_converted = rustObjectBeingAddedOfJSObjectInfo(&mesh_value)?;
    let ObjectBeingAdded::Mesh(mesh_info) = mesh_converted else {
        return Err(anyhow::anyhow!("Expected mesh conversion"));
    };
    assert_vec3_close(mesh_info.scale, Vec3::new(2.0, 3.0, 4.0), "mesh.scale");
    assert_eq!(mesh_info.meshDataId, 7001);
    assert_eq!(expect_pointer_id(mesh_info.followSkeletonId), Some(31));
    assert_eq!(expect_pointer_id(mesh_info.physicalObjectId), Some(32));
    assert_eq!(
        expect_pointer_id(mesh_info.material.map(|mesh_material_settings| mesh_material_settings.materialInfoId)),
        Some(33)
    );
    assert!(!mesh_info.visualSettings.isAlwaysOnTop);

    // This section checks conversion for player object pointers.
    let player_value = json!({
        "type": "player",
        "username": "player-name",
        "userid": 17,
        "preferredColor": null,
        "equippedItemId": null,
        "headObjectId": 89,
        "characterObjectId": 88
    });
    let player_converted = rustObjectBeingAddedOfJSObjectInfo(&player_value)?;
    let ObjectBeingAdded::Player(player_info) = player_converted else {
        return Err(anyhow::anyhow!("Expected player conversion"));
    };
    assert_eq!(player_info.username, "player-name");
    assert_eq!(player_info.userid, Some(17));
    assert_eq!(expect_pointer_id(player_info.headObjectId), Some(89));
    assert_eq!(expect_pointer_id(player_info.characterObjectId), Some(88));

    // This section checks conversion for rigid-group visual toggles and physics info.
    let rigid_group_value = json!({
        "type": "rigidGroup",
        "followSkeletonId": 77,
        "location": {
            "position": { "x": 9.0, "y": 8.0, "z": 7.0 },
            "rotation": { "x": 45.0, "y": -30.0, "z": 0.0 }
        },
        "movement": {
            "velocity": { "x": 0.5, "y": 1.5, "z": 2.5 },
            "angularVelocity": { "x": -5.0, "y": -15.0, "z": -25.0 }
        }
    });
    let rigid_group_converted = rustObjectBeingAddedOfJSObjectInfo(&rigid_group_value)?;
    let ObjectBeingAdded::RigidGroup(rigid_group_info, rigid_group_physics_info) = rigid_group_converted else {
        return Err(anyhow::anyhow!("Expected rigid-group conversion"));
    };
    assert_eq!(expect_pointer_id(rigid_group_info.followSkeletonId), Some(77));
    assert_vec3_close(
        rigid_group_physics_info.movement.angularVelocity,
        Vec3::new(-5.0_f32.to_radians(), -15.0_f32.to_radians(), -25.0_f32.to_radians()),
        "rigidGroup.movement.angularVelocity",
    );

    // This section checks conversion for attaching an object to a Skeleton joint.
    let sk_attachment_value = json!({
        "type": "skAttachment",
        "objectId": 44,
        "jointName": "arm_joint",
        "offsetFromJoint": {
            "position": { "x": 1.0, "y": 2.0, "z": 3.0 },
            "rotation": { "x": 0.0, "y": 0.0, "z": 0.0 }
        }
    });
    let sk_attachment_converted = rustObjectBeingAddedOfJSObjectInfo(&sk_attachment_value)?;
    let ObjectBeingAdded::SkAttachment(sk_attachment_info) = sk_attachment_converted else {
        return Err(anyhow::anyhow!("Expected joint-attachment conversion"));
    };
    assert_eq!(expect_pointer_id(sk_attachment_info.objectId), Some(44));
    assert_eq!(sk_attachment_info.jointName, "arm_joint");
    assert_vec3_close(
        sk_attachment_info.offsetFromJoint.position,
        Vec3::new(1.0, 2.0, 3.0),
        "skAttachment.offsetFromJoint.position",
    );

    // This section checks conversion for script raw code.
    let script_value = json!({
        "type": "script",
        "objectLocation": "Client",
        "rawCode": "console.log('hello');"
    });
    let script_converted = rustObjectBeingAddedOfJSObjectInfo(&script_value)?;
    let ObjectBeingAdded::Script(script_info) = script_converted else {
        return Err(anyhow::anyhow!("Expected script conversion"));
    };
    assert_eq!(script_info.objectLocation, ObjectLocation::Client);

    // This section checks conversion for sound player pointers.
    let sound_player_value = json!({
        "type": "soundPlayer",
        "soundDataId": 9001,
        "volume": 0.75,
        "clockId": 123
    });
    let sound_player_converted = rustObjectBeingAddedOfJSObjectInfo(&sound_player_value)?;
    let ObjectBeingAdded::SoundPlayer(sound_player_info) = sound_player_converted else {
        return Err(anyhow::anyhow!("Expected sound player conversion"));
    };
    assert_eq!(sound_player_info.soundDataId, 9001);
    assert_close_f32(sound_player_info.volume, 0.75, "soundPlayer.volume");
    assert_eq!(expect_pointer_id(sound_player_info.clockId), Some(123));

    // This section checks conversion for SkAnimation, keyframes, and poses.
    let sk_animation_value = json!({
        "type": "skAnimation",
        "duration": 1.5,
        "interpolationStyle": "EaseInOut"
    });
    let sk_animation_converted = rustObjectBeingAddedOfJSObjectInfo(&sk_animation_value)?;
    let ObjectBeingAdded::SkAnimation(sk_animation_info) = sk_animation_converted else {
        return Err(anyhow::anyhow!("Expected SkAnimation conversion"));
    };
    assert_close_f32(sk_animation_info.duration, 1.5, "skAnimation.duration");
    match sk_animation_info.interpolationStyle {
        InterpolationStyle::EaseInOut => {}
        InterpolationStyle::Linear => return Err(anyhow::anyhow!("Expected EaseInOut SkAnimation conversion")),
        InterpolationStyle::Step => return Err(anyhow::anyhow!("Expected EaseInOut SkAnimation conversion")),
        InterpolationStyle::EaseIn => return Err(anyhow::anyhow!("Expected EaseInOut SkAnimation conversion")),
        InterpolationStyle::EaseOut => return Err(anyhow::anyhow!("Expected EaseInOut SkAnimation conversion")),
    }

    let sk_keyframe_value = json!({
        "type": "skKeyframe",
        "time": 0.25
    });
    let sk_keyframe_converted = rustObjectBeingAddedOfJSObjectInfo(&sk_keyframe_value)?;
    let ObjectBeingAdded::SkKeyframe(sk_keyframe_info) = sk_keyframe_converted else {
        return Err(anyhow::anyhow!("Expected SkKeyframe conversion"));
    };
    assert_close_f32(sk_keyframe_info.time, 0.25, "skKeyframe.time");

    let sk_keyframe_pose_value = json!({
        "type": "skKeyframePose",
        "jointName": "left-arm",
        "localPose": {
            "position": { "x": 1.0, "y": 2.0, "z": 3.0 },
            "rotation": { "x": 0.0, "y": 0.0, "z": 0.0 }
        }
    });
    let sk_keyframe_pose_converted = rustObjectBeingAddedOfJSObjectInfo(&sk_keyframe_pose_value)?;
    let ObjectBeingAdded::SkKeyframePose(sk_keyframe_pose_info) = sk_keyframe_pose_converted else {
        return Err(anyhow::anyhow!("Expected SkKeyframePose conversion"));
    };
    assert_eq!(sk_keyframe_pose_info.jointName, "left-arm");
    assert_vec3_close(
        sk_keyframe_pose_info.localPose.position,
        Vec3::new(1.0, 2.0, 3.0),
        "skKeyframePose.localPose.position",
    );

    // This section checks conversion for joint-animation object ids, priority clamping, and f32 weight clamping.
    let sk_animator_value = json!({
        "type": "skAnimator",
        "skeletonId": 45,
        "clockId": 46,
        "skAnimationId": 47,
        "priority": 999.8,
        "weight": -2.4,
        "weightTransition": {
            "startedAtMs": 10.0,
            "timeLocation": "Client",
            "initValue": -20.0,
            "finalValue": 130.0,
            "durationMs": 40.0,
            "easing": "EaseOut"
        }
    });
    let sk_animator_converted = rustObjectBeingAddedOfJSObjectInfo(&sk_animator_value)?;
    let ObjectBeingAdded::SkAnimator(sk_animator_info) = sk_animator_converted else {
        return Err(anyhow::anyhow!("Expected joint-animation conversion"));
    };
    assert_eq!(expect_pointer_id(sk_animator_info.skeletonId), Some(45));
    assert_eq!(expect_pointer_id(sk_animator_info.clockId), Some(46));
    assert_eq!(expect_pointer_id(sk_animator_info.skAnimationId), Some(47));
    assert_eq!(sk_animator_info.priority, 10);
    assert_close_f32(sk_animator_info.weight, 0.0, "skAnimator.weight");
    let Some(weight_transition) = sk_animator_info.weightTransition else {
        return Err(anyhow::anyhow!("Expected SkAnimator weight transition"));
    };
    assert_close_f32(weight_transition.startedAtMs as f32, 10.0, "skAnimator.weightTransition.startedAtMs");
    assert_eq!(weight_transition.timeLocation, ObjectLocation::Client);
    assert_close_f32(weight_transition.initValue, 0.0, "skAnimator.weightTransition.initValue");
    assert_close_f32(weight_transition.finalValue, 100.0, "skAnimator.weightTransition.finalValue");
    assert_close_f32(weight_transition.durationMs, 40.0, "skAnimator.weightTransition.durationMs");
    assert_eq!(weight_transition.easing, InterpolationStyle::EaseOut);

    let property_animator_value = json!({
        "type": "propertyAnimator",
        "targetObjectId": 48,
        "propertyPath": ["color"],
        "propertyAnimationId": 50,
        "clockId": 49,
        "priority": 9.0,
        "weight": 75.0,
        "weightTransition": null
    });
    let property_animator_converted = rustObjectBeingAddedOfJSObjectInfo(&property_animator_value)?;
    let ObjectBeingAdded::PropertyAnimator(property_animator_info) = property_animator_converted else {
        return Err(anyhow::anyhow!("Expected property animator conversion"));
    };
    assert_eq!(expect_pointer_id(property_animator_info.targetObjectId), Some(48));
    assert_eq!(expect_pointer_id(property_animator_info.clockId), Some(49));
    assert_eq!(property_animator_info.propertyPath, vec!["color".to_string()]);
    assert_eq!(property_animator_info.priority, 9);
    assert_close_f32(property_animator_info.weight, 75.0, "propertyAnimator.weight");
    assert_eq!(expect_pointer_id(property_animator_info.propertyAnimationId), Some(50));

    let property_animation_value = json!({
        "type": "propertyAnimation",
        "duration": 2.0,
        "interpolationStyle": "EaseOut"
    });
    let property_animation_converted = rustObjectBeingAddedOfJSObjectInfo(&property_animation_value)?;
    let ObjectBeingAdded::PropertyAnimation(property_animation_info) = property_animation_converted else {
        return Err(anyhow::anyhow!("Expected property animation conversion"));
    };
    assert_close_f32(property_animation_info.duration, 2.0, "propertyAnimation.duration");
    match property_animation_info.interpolationStyle {
        InterpolationStyle::EaseOut => {}
        InterpolationStyle::Step | InterpolationStyle::Linear | InterpolationStyle::EaseInOut | InterpolationStyle::EaseIn => {
            return Err(anyhow::anyhow!("Expected EaseOut property animation conversion"));
        }
    }

    let property_keyframe_value = json!({
        "type": "propertyKeyframe",
        "time": 0.0,
        "value": 0.25
    });
    let property_keyframe_converted = rustObjectBeingAddedOfJSObjectInfo(&property_keyframe_value)?;
    let ObjectBeingAdded::PropertyKeyframe(property_keyframe_info) = property_keyframe_converted else {
        return Err(anyhow::anyhow!("Expected property keyframe conversion"));
    };
    assert_close_f32(property_keyframe_info.time, 0.0, "propertyKeyframe.time");
    assert_close_f32(property_keyframe_info.value, 0.25, "propertyKeyframe.value");

    Ok(())
}

#[test]
fn rust_object_being_added_handles_supported_and_unknown_types() {
    let supported_skeleton_value = json!({
        "type": "skeleton"
    });
    let supported_skeleton_result = rustObjectBeingAddedOfJSObjectInfo(&supported_skeleton_value);
    assert!(supported_skeleton_result.is_ok());

    let unknown_type_value = json!({
        "type": "definitelyUnknownObjectType"
    });
    let unknown_type_result = rustObjectBeingAddedOfJSObjectInfo(&unknown_type_value);
    assert!(unknown_type_result.is_err());
}

// -------------------------- JS -> Rust Update Conversions ---------------

// Label updates accept both supported modes and leave the label unchanged when a mode is rejected.
#[test]
fn rust_update_label_accepts_full_and_none_and_rejects_upright() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let label_object_id = 2000;
    add_label(&mut world, label_object_id, root_object_id)?;

    for (mode_name, expected_mode) in [("none", BillboardMode::None), ("full", BillboardMode::Full)] {
        update_object_leaf(&mut world, label_object_id, &["billboardMode"], json!(mode_name))?;
        let ObjectInfo::Label(label_info) = &world.getObject(label_object_id)?.info else {
            panic!("expected a label");
        };
        assert_eq!(label_info.billboardMode, expected_mode);
    }

    let error = update_object_leaf(&mut world, label_object_id, &["billboardMode"], json!("upright")).expect_err("upright billboarding should be rejected");
    assert!(error.to_string().contains("billboardMode must be none|full"));
    let ObjectInfo::Label(label_info) = &world.getObject(label_object_id)?.info else {
        panic!("expected a label");
    };
    assert_eq!(label_info.billboardMode, BillboardMode::Full);
    Ok(())
}

// Players start with no preferredColor and labels with no backgroundColor. Setting one while it was null used to fail, which broke the
// profile refresh of every guest and of every player who never picked a color. A set color changes one channel at a time, and null clears it.
#[test]
fn rust_update_object_sets_changes_and_clears_optional_colors() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let player_object_id = 2000;
    let label_object_id = 2001;
    add_player(&mut world, player_object_id, root_object_id, None)?;
    add_label(&mut world, label_object_id, root_object_id)?;
    let color_of = |world: &World, object_id: ObjectId| match &world.getObject(object_id).expect("the object should exist").info {
        ObjectInfo::Player(player) => player.preferredColor,
        ObjectInfo::Label(label) => label.backgroundColor,
        _ => panic!("expected a player or a label"),
    };

    for (object_id, field_name) in [(player_object_id, "preferredColor"), (label_object_id, "backgroundColor")] {
        update_object_leaf(&mut world, object_id, &[field_name], Value::Null)?;
        assert!(color_of(&world, object_id).is_none(), "{field_name} should stay null");
        assert!(
            update_object_leaf(&mut world, object_id, &[field_name, "r"], json!(1.0)).is_err(),
            "a null {field_name} has no channel to set"
        );

        update_object_leaf(&mut world, object_id, &[field_name], json!({ "r": 0.1, "g": 0.2, "b": 0.3 }))?;
        update_object_leaf(&mut world, object_id, &[field_name, "g"], json!(0.5))?;
        let Some(color) = color_of(&world, object_id) else {
            panic!("{field_name} should be set");
        };
        assert_close_f32(color.r, 0.1, field_name);
        assert_close_f32(color.g, 0.5, field_name);
        assert_close_f32(color.b, 0.3, field_name);

        update_object_leaf(&mut world, object_id, &[field_name], Value::Null)?;
        assert!(color_of(&world, object_id).is_none(), "null should clear {field_name}");
    }
    Ok(())
}

#[test]
fn rust_update_object_updates_clock_fields() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2000;

    add_clock(&mut world, clock_object_id, root_object_id)?;

    update_object_leaf(
        &mut world,
        clock_object_id,
        &["clockState"],
        json!({
            "Playing": {
                "localStartMs": 456.0,
                "timeLocation": "Server"
            }
        }),
    )?;
    {
        let clock_object = world.getObject(clock_object_id)?;
        let ObjectInfo::Clock(clock_info) = &clock_object.info else {
            return Err(anyhow::anyhow!("Expected clock after state update"));
        };
        assert_eq!(
            clock_info.clockState,
            ClockState::Playing {
                localStartMs: 456.0,
                timeLocation: ObjectLocation::Server,
            }
        );
    }
    update_object_leaf(
        &mut world,
        clock_object_id,
        &["clockState"],
        json!({
            "Playing": {
                "localStartMs": 600.0,
                "timeLocation": "Client"
            }
        }),
    )?;
    update_object_leaf(&mut world, clock_object_id, &["clockState", "Playing", "localStartMs"], json!(750.0))?;
    update_object_leaf(&mut world, clock_object_id, &["clockState", "Playing", "timeLocation"], json!("Server"))?;
    update_object_leaf(&mut world, clock_object_id, &["speed"], json!(3.75))?;
    update_object_leaf(&mut world, clock_object_id, &["shouldLoop"], json!(false))?;

    let clock_object = world.getObject(clock_object_id)?;
    let ObjectInfo::Clock(clock_info) = &clock_object.info else {
        return Err(anyhow::anyhow!("Expected clock after update"));
    };

    assert_eq!(
        clock_info.clockState,
        ClockState::Playing {
            localStartMs: 750.0,
            timeLocation: ObjectLocation::Server,
        }
    );
    assert_close_f32(clock_info.speed, 3.75, "clock.speed");
    assert!(!clock_info.shouldLoop);

    let Some(clock_flags) = world.modifiedObjectFlags.get(&clock_object_id) else {
        return Err(anyhow::anyhow!("Expected modified flags for clock"));
    };
    assert!(clock_flags.modifiedObjectInfo);

    Ok(())
}

// A path can write a whole enum variant, or a field inside one even while the object is on the other variant. A path that goes
// on past a plain field is refused.
#[test]
fn rust_update_object_writes_enum_variants_by_path_and_refuses_longer_paths() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2000;
    let material_object_id = 2001;
    let ball_object_id = 2002;
    let sk_animator_object_id = 2003;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_material_without_textures(&mut world, material_object_id, root_object_id)?;
    add_ball(&mut world, ball_object_id, root_object_id, Vec3::ZERO, true)?;
    add_sk_animator(&mut world, sk_animator_object_id, root_object_id)?;

    let clock_state = |world: &World| -> anyhow::Result<ClockState> {
        let ObjectInfo::Clock(clock_info) = &world.getObject(clock_object_id)?.info else {
            return Err(anyhow::anyhow!("Expected clock"));
        };
        Ok(clock_info.clockState)
    };
    update_object_leaf(&mut world, clock_object_id, &["clockState", "Paused"], json!({ "animationTimeSecs": 2.0 }))?;
    assert_eq!(clock_state(&world)?, ClockState::Paused { animationTimeSecs: 2.0 });
    update_object_leaf(
        &mut world,
        clock_object_id,
        &["clockState", "Playing"],
        json!({ "localStartMs": 500.0, "timeLocation": "Server" }),
    )?;
    update_object_leaf(&mut world, clock_object_id, &["clockState", "Paused", "animationTimeSecs"], json!(3.0))?;
    assert_eq!(
        clock_state(&world)?,
        ClockState::Paused { animationTimeSecs: 3.0 },
        "a Paused field switches a playing clock to Paused"
    );

    update_object_leaf(&mut world, material_object_id, &["alphaMode", "Mask"], json!({ "cutoff": 0.3 }))?;
    let ObjectInfo::Material(material_info) = &world.getObject(material_object_id)?.info else {
        return Err(anyhow::anyhow!("Expected material"));
    };
    let AlphaMode::Mask { cutoff } = material_info.alphaMode else {
        return Err(anyhow::anyhow!("Expected a Mask alpha mode"));
    };
    assert_close_f32(cutoff, 0.3, "material.alphaMode.Mask.cutoff");

    assert!(update_object_leaf(&mut world, ball_object_id, &["isPushable", "x"], json!(false)).is_err());
    let weight_transition = json!({
        "startedAtMs": 10.0,
        "timeLocation": "Client",
        "initValue": 0.0,
        "finalValue": 100.0,
        "durationMs": 40.0,
        "easing": "EaseOut"
    });
    update_object_leaf(&mut world, sk_animator_object_id, &["weightTransition"], weight_transition.clone())?;
    assert!(update_object_leaf(&mut world, sk_animator_object_id, &["weightTransition", "durationMs"], weight_transition).is_err());
    Ok(())
}

#[test]
fn rust_update_object_updates_ball_brick_and_cylinder_paths() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    let material_object_id = 2100;
    let ball_object_id = 2101;
    let brick_object_id = 2102;
    let cylinder_object_id = 2103;

    add_material_without_textures(&mut world, material_object_id, root_object_id)?;
    add_ball(&mut world, ball_object_id, root_object_id, Vec3::new(1.0, 2.0, 3.0), true)?;
    add_brick(&mut world, brick_object_id, root_object_id, Vec3::new(4.0, 5.0, 6.0), true)?;
    add_cylinder(&mut world, cylinder_object_id, root_object_id, Vec3::new(7.0, 8.0, 9.0), true)?;

    // This section updates ball fields, nested visuals, and physics conversion paths.
    update_object_leaf(
        &mut world,
        ball_object_id,
        &["material"],
        json!({
            "materialInfoId": material_object_id,
            "tileSizeU": 3.0,
            "tileSizeV": 4.0,
            "tileOffsetU": 0.0,
            "tileOffsetV": 0.5
        }),
    )?;
    update_object_leaf(&mut world, ball_object_id, &["material", "tileSizeU"], json!(4.5))?;
    update_object_leaf(&mut world, ball_object_id, &["material", "tileOffsetU"], json!(0.25))?;
    update_object_leaf(&mut world, ball_object_id, &["opacity"], json!(0.65))?;
    update_object_leaf(&mut world, ball_object_id, &["color", "r"], json!(0.62))?;
    update_object_leaf(&mut world, ball_object_id, &["glow"], json!(0.8))?;
    update_object_leaf(&mut world, ball_object_id, &["isAlwaysOnTop"], json!(true))?;
    update_object_leaf(&mut world, ball_object_id, &["isWalkthrough"], json!(true))?;
    update_object_leaf(&mut world, ball_object_id, &["isPushable"], json!(false))?;
    update_object_leaf(&mut world, ball_object_id, &["density"], json!(9.5))?;
    update_object_leaf(&mut world, ball_object_id, &["radius"], json!(4.5))?;
    update_object_leaf(&mut world, ball_object_id, &["location", "position", "x"], json!(12.0))?;
    update_object_leaf(&mut world, ball_object_id, &["movement", "angularVelocity", "z"], json!(-4.0))?;

    let ball_object = world.getObject(ball_object_id)?;
    let ObjectInfo::Ball(ball_info) = &ball_object.info else {
        return Err(anyhow::anyhow!("Expected ball after update"));
    };
    let Some(ball_material_settings_3d) = ball_info.material else {
        return Err(anyhow::anyhow!("Expected ball material settings after update"));
    };
    assert_eq!(expect_pointer_id(Some(ball_material_settings_3d.materialInfoId)), Some(material_object_id));
    assert_close_f32(ball_material_settings_3d.tileSizeU, 4.5, "ball.material.tileSizeU");
    assert_close_f32(ball_material_settings_3d.tileSizeV, 4.0, "ball.material.tileSizeV");
    assert_close_f32(ball_material_settings_3d.tileOffsetU, 0.25, "ball.material.tileOffsetU");
    assert_close_f32(ball_material_settings_3d.tileOffsetV, 0.5, "ball.material.tileOffsetV");
    assert_close_f32(ball_info.visualSettings.opacity, 0.65, "ball.opacity");
    assert_close_f32(ball_info.visualSettings.color.r, 0.62, "ball.color.r");
    assert_close_f32(ball_info.visualSettings.glow, 0.8, "ball.glow");
    assert!(ball_info.visualSettings.isAlwaysOnTop);

    let Some(ball_physics_toggles) = world.getObjectPhysicsToggles3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics toggles after update"));
    };
    assert!(ball_physics_toggles.isWalkthrough);
    assert!(!ball_physics_toggles.isPushable);
    assert_close_f32(ball_physics_toggles.density, 9.5, "ball.density");

    let Some(ball_physics_info) = world.getObjectPhysicsInfo3D(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball physics info after update"));
    };
    assert_close_f32(ball_physics_info.location.position.x, 12.0, "ball.position.x");
    assert_close_f32(ball_physics_info.movement.angularVelocity.z, 0.0, "ball.angularVelocity.z");
    let Some(ball_primitive_settings) = world.physics.getBallPrimitiveSettings(ball_object_id) else {
        return Err(anyhow::anyhow!("Expected ball primitive info after update"));
    };
    assert_close_f32(ball_primitive_settings.radius, 4.5, "ball.radius");

    // This section updates cylinder fields and keeps radius and length in the physics-owned primitive settings.
    update_object_leaf(
        &mut world,
        cylinder_object_id,
        &["material"],
        json!({
            "materialInfoId": material_object_id,
            "tileSizeU": 5.0,
            "tileSizeV": 6.0,
            "tileOffsetU": 0.75,
            "tileOffsetV": 1.0
        }),
    )?;
    update_object_leaf(&mut world, cylinder_object_id, &["material", "tileSizeV"], json!(6.5))?;
    update_object_leaf(&mut world, cylinder_object_id, &["material", "tileOffsetV"], json!(1.5))?;
    update_object_leaf(&mut world, cylinder_object_id, &["opacity"], json!(0.55))?;
    update_object_leaf(&mut world, cylinder_object_id, &["color", "b"], json!(0.73))?;
    update_object_leaf(&mut world, cylinder_object_id, &["isWalkthrough"], json!(true))?;
    update_object_leaf(&mut world, cylinder_object_id, &["density"], json!(5.5))?;
    update_object_leaf(&mut world, cylinder_object_id, &["radius"], json!(2.25))?;
    update_object_leaf(&mut world, cylinder_object_id, &["length"], json!(8.75))?;
    update_object_leaf(&mut world, cylinder_object_id, &["location", "position", "z"], json!(13.0))?;

    let cylinder_object = world.getObject(cylinder_object_id)?;
    let ObjectInfo::Cylinder(cylinder_info) = &cylinder_object.info else {
        return Err(anyhow::anyhow!("Expected cylinder after update"));
    };
    let Some(cylinder_material_settings_3d) = cylinder_info.material else {
        return Err(anyhow::anyhow!("Expected cylinder material settings after update"));
    };
    assert_eq!(expect_pointer_id(Some(cylinder_material_settings_3d.materialInfoId)), Some(material_object_id));
    assert_close_f32(cylinder_material_settings_3d.tileSizeU, 5.0, "cylinder.material.tileSizeU");
    assert_close_f32(cylinder_material_settings_3d.tileSizeV, 6.5, "cylinder.material.tileSizeV");
    assert_close_f32(cylinder_material_settings_3d.tileOffsetU, 0.75, "cylinder.material.tileOffsetU");
    assert_close_f32(cylinder_material_settings_3d.tileOffsetV, 1.5, "cylinder.material.tileOffsetV");
    assert_close_f32(cylinder_info.visualSettings.opacity, 0.55, "cylinder.opacity");
    assert_close_f32(cylinder_info.visualSettings.color.b, 0.73, "cylinder.color.b");

    let Some(cylinder_physics_toggles) = world.getObjectPhysicsToggles3D(cylinder_object_id) else {
        return Err(anyhow::anyhow!("Expected cylinder physics toggles after update"));
    };
    assert!(cylinder_physics_toggles.isWalkthrough);
    assert_close_f32(cylinder_physics_toggles.density, 5.5, "cylinder.density");

    let Some(cylinder_physics_info) = world.getObjectPhysicsInfo3D(cylinder_object_id) else {
        return Err(anyhow::anyhow!("Expected cylinder physics info after update"));
    };
    assert_close_f32(cylinder_physics_info.location.position.z, 13.0, "cylinder.position.z");
    let Some(cylinder_primitive_settings) = world.physics.getCylinderPrimitiveSettings(cylinder_object_id) else {
        return Err(anyhow::anyhow!("Expected cylinder primitive info after update"));
    };
    assert_close_f32(cylinder_primitive_settings.radius, 2.25, "cylinder.radius");
    assert_close_f32(cylinder_primitive_settings.length, 8.75, "cylinder.length");

    // This section updates brick fields and nested rotation conversion.
    update_object_leaf(&mut world, brick_object_id, &["material"], Value::Null)?;
    update_object_leaf(&mut world, brick_object_id, &["opacity"], json!(0.45))?;
    update_object_leaf(&mut world, brick_object_id, &["color", "g"], json!(0.87))?;
    update_object_leaf(&mut world, brick_object_id, &["isAlwaysOnTop"], json!(true))?;
    update_object_leaf(&mut world, brick_object_id, &["isWalkthrough"], json!(true))?;
    update_object_leaf(&mut world, brick_object_id, &["density"], json!(14.0))?;
    update_object_leaf(&mut world, brick_object_id, &["size", "y"], json!(9.0))?;
    update_object_leaf(&mut world, brick_object_id, &["location", "rotation", "y"], json!(0.5))?;
    update_object_leaf(&mut world, brick_object_id, &["movement", "velocity", "x"], json!(7.0))?;

    let brick_object = world.getObject(brick_object_id)?;
    let ObjectInfo::Brick(brick_info) = &brick_object.info else {
        return Err(anyhow::anyhow!("Expected brick after update"));
    };
    assert!(brick_info.material.is_none());
    assert_close_f32(brick_info.visualSettings.opacity, 0.45, "brick.opacity");
    assert_close_f32(brick_info.visualSettings.color.g, 0.87, "brick.color.g");
    assert!(brick_info.visualSettings.isAlwaysOnTop);

    let Some(brick_physics_toggles) = world.getObjectPhysicsToggles3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics toggles after update"));
    };
    assert!(brick_physics_toggles.isWalkthrough);
    assert_close_f32(brick_physics_toggles.density, 14.0, "brick.density");

    let Some(brick_physics_info) = world.getObjectPhysicsInfo3D(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick physics info after update"));
    };
    assert_quat_close(
        brick_physics_info.location.quaternion,
        Quat::from_euler(EulerRot::XYZ, 0.0, 0.5_f32.to_radians(), 0.0),
        "brick.rotation",
    );
    assert_close_f32(brick_physics_info.movement.velocity.x, 7.0, "brick.velocity.x");
    let Some(brick_primitive_settings) = world.physics.getBrickPrimitiveSettings(brick_object_id) else {
        return Err(anyhow::anyhow!("Expected brick primitive info after update"));
    };
    assert_close_f32(brick_primitive_settings.size.y, 9.0, "brick.size.y");

    Ok(())
}

#[test]
fn rust_update_object_updates_material_mesh_player_rigid_group_sk_animator_and_sound_player() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();

        let skeleton_object_id = 2200;
        let material_object_id = 2201;
        let clock_object_id = 2202;
        let ball_object_id = 2203;
        let brick_object_id = 2204;
        let local_group_object_id = 2205;
        let mesh_object_id = 2206;
        let player_object_id = 2207;
        let rigid_group_object_id = 2208;
        let script_object_id = 2209;
        let sk_animator_object_id = 2210;
        let sound_player_object_id = 2211;
        let sk_animation_object_id = 2212;
        let sk_attachment_object_id = 2213;
        let sk_keyframe_object_id = 2214;
        let sk_keyframe_pose_object_id = 2215;
        let world_lighting_object_id = 2216;

        add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
        add_material_without_textures(&mut world, material_object_id, root_object_id)?;
        add_clock(&mut world, clock_object_id, root_object_id)?;
        add_ball(&mut world, ball_object_id, root_object_id, Vec3::new(0.0, 0.0, 0.0), true)?;
        add_brick(&mut world, brick_object_id, root_object_id, Vec3::new(1.0, 0.0, 0.0), true)?;
        add_local_group(&mut world, local_group_object_id, root_object_id)?;
        add_mesh(&mut world, mesh_object_id, root_object_id)?;
        add_player(&mut world, player_object_id, root_object_id, Some(ball_object_id))?;
        add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(0.0, 1.0, 0.0), None)?;
        add_script(&mut world, script_object_id, root_object_id)?;
        add_sk_animation(&mut world, sk_animation_object_id, root_object_id)?;
        add_sk_attachment(&mut world, sk_attachment_object_id, root_object_id, None)?;
        add_sk_keyframe(&mut world, sk_keyframe_object_id, sk_animation_object_id)?;
        add_sk_keyframe_pose(&mut world, sk_keyframe_pose_object_id, sk_keyframe_object_id)?;
        add_sk_animator(&mut world, sk_animator_object_id, root_object_id)?;
        add_sound_player(&mut world, sound_player_object_id, root_object_id, None)?;
        world.addObjectWithId(
            world_lighting_object_id,
            "world-lighting".to_string(),
            ObjectBeingAdded::WorldLighting(WorldLightingInfo::default()),
            root_object_id,
        );

        // This section confirms local-group updates are rejected.
        let local_group_update_result = update_object_leaf(&mut world, local_group_object_id, &["nonEditableField"], json!({}));
        assert!(local_group_update_result.is_err(), "Expected local-group update to fail");

        // This section updates all material conversion fields, including nested alphaMode updates.
        update_object_leaf(&mut world, material_object_id, &["albedoTextureDataId"], json!(9011))?;
        update_object_leaf(&mut world, material_object_id, &["metallicTextureDataId"], json!(9012))?;
        update_object_leaf(&mut world, material_object_id, &["roughnessTextureDataId"], json!(9013))?;
        update_object_leaf(&mut world, material_object_id, &["normalTextureDataId"], json!(9014))?;
        update_object_leaf(&mut world, material_object_id, &["alphaMode"], json!({ "Mask": { "cutoff": 1.5 } }))?;
        update_object_leaf(&mut world, material_object_id, &["alphaMode", "Mask", "cutoff"], json!(-0.25))?;

        let material_object = world.getObject(material_object_id)?;
        let ObjectInfo::Material(material_info) = &material_object.info else {
            return Err(anyhow::anyhow!("Expected material after update"));
        };
        assert_eq!(material_info.albedoTextureDataId, Some(9011));
        assert_eq!(material_info.metallicTextureDataId, Some(9012));
        assert_eq!(material_info.roughnessTextureDataId, Some(9013));
        assert_eq!(material_info.normalTextureDataId, Some(9014));
        let AlphaMode::Mask { cutoff } = material_info.alphaMode else {
            return Err(anyhow::anyhow!("Expected material alpha mode mask after update"));
        };
        assert_close_f32(cutoff, 0.0, "material.alphaMode.Mask.cutoff");

        update_object_leaf(&mut world, material_object_id, &["isDoubleSided"], json!(true))?;
        update_object_leaf(&mut world, material_object_id, &["alphaMode"], json!("Blend"))?;
        let material = world.getObject(material_object_id)?;
        let ObjectInfo::Material(info) = &material.info else { unreachable!() };
        assert!(match info.alphaMode {
            AlphaMode::Blend => true,
            _ => false,
        });

        // This section updates world lighting conversion fields.
        update_object_leaf(&mut world, world_lighting_object_id, &["sunOrientation", "phi"], json!(-45.0))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["sunColor", "g"], json!(0.72))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["sunIntensity"], json!(5.5))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["sunShadowSoftness"], json!(4.0))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["ambientTopColor", "r"], json!(0.08))?;
        update_object_leaf(
            &mut world,
            world_lighting_object_id,
            &["ambientBottomColor"],
            json!({ "r": 0.02, "g": 0.03, "b": 0.04 }),
        )?;
        update_object_leaf(&mut world, world_lighting_object_id, &["ambientIntensity"], json!(0.6))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["ambientOcclusionIntensity"], json!(0.9))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["ambientOcclusionRadius"], json!(3.5))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["ambientOcclusionSunlightStrength"], json!(0.1))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["skyTopColor", "b"], json!(0.44))?;
        update_object_leaf(
            &mut world,
            world_lighting_object_id,
            &["skyHorizonColor"],
            json!({ "r": 0.6, "g": 0.7, "b": 0.8 }),
        )?;
        update_object_leaf(&mut world, world_lighting_object_id, &["skyBottomColor", "r"], json!(0.31))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["skyboxTextureDataId"], json!(9090))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["skyboxTextureOpacity"], json!(0.25))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["skyReflectionStrength"], json!(0.3))?;
        update_object_leaf(&mut world, world_lighting_object_id, &["vibrance"], json!(0.5))?;

        let world_lighting_object = world.getObject(world_lighting_object_id)?;
        let ObjectInfo::WorldLighting(world_lighting_info) = &world_lighting_object.info else {
            return Err(anyhow::anyhow!("Expected world lighting after update"));
        };
        assert_close_f32(
            world_lighting_info.sunOrientation.phi,
            45.0_f32.to_radians(),
            "worldLighting.sunOrientation.phi",
        );
        assert_close_f32(world_lighting_info.sunColor.g, 0.72, "worldLighting.sunColor.g");
        assert_close_f32(world_lighting_info.sunIntensity, 5.5, "worldLighting.sunIntensity");
        assert_close_f32(world_lighting_info.sunShadowSoftness, 4.0, "worldLighting.sunShadowSoftness");
        assert_close_f32(world_lighting_info.ambientTopColor.r, 0.08, "worldLighting.ambientTopColor.r");
        assert_close_f32(world_lighting_info.ambientBottomColor.b, 0.04, "worldLighting.ambientBottomColor.b");
        assert_close_f32(world_lighting_info.ambientIntensity, 0.6, "worldLighting.ambientIntensity");
        assert_close_f32(world_lighting_info.ambientOcclusionIntensity, 0.9, "worldLighting.ambientOcclusionIntensity");
        assert_close_f32(world_lighting_info.ambientOcclusionRadius, 3.5, "worldLighting.ambientOcclusionRadius");
        assert_close_f32(
            world_lighting_info.ambientOcclusionSunlightStrength,
            0.1,
            "worldLighting.ambientOcclusionSunlightStrength",
        );
        assert_close_f32(world_lighting_info.skyTopColor.b, 0.44, "worldLighting.skyTopColor.b");
        assert_close_f32(world_lighting_info.skyHorizonColor.b, 0.8, "worldLighting.skyHorizonColor.b");
        assert_close_f32(world_lighting_info.skyBottomColor.r, 0.31, "worldLighting.skyBottomColor.r");
        assert_eq!(world_lighting_info.skyboxTextureDataId, Some(9090));
        assert_close_f32(world_lighting_info.skyboxTextureOpacity, 0.25, "worldLighting.skyboxTextureOpacity");
        assert_close_f32(world_lighting_info.skyReflectionStrength, 0.3, "worldLighting.skyReflectionStrength");
        assert_close_f32(world_lighting_info.vibrance, 0.5, "worldLighting.vibrance");

        // This section updates all mesh conversion fields.
        update_object_leaf(&mut world, mesh_object_id, &["scale", "z"], json!(6.5))?;
        update_object_leaf(&mut world, mesh_object_id, &["meshDataId"], json!(9001))?;
        update_object_leaf(&mut world, mesh_object_id, &["followSkeletonId"], json!(skeleton_object_id))?;
        update_object_leaf(&mut world, mesh_object_id, &["physicalObjectId"], json!(brick_object_id))?;
        update_object_leaf(&mut world, mesh_object_id, &["material"], json!({ "materialInfoId": material_object_id }))?;
        update_object_leaf(&mut world, mesh_object_id, &["material", "materialInfoId"], json!(material_object_id))?;
        update_object_leaf(&mut world, mesh_object_id, &["opacity"], json!(0.58))?;
        update_object_leaf(&mut world, mesh_object_id, &["color", "b"], json!(0.91))?;
        update_object_leaf(&mut world, mesh_object_id, &["isAlwaysOnTop"], json!(true))?;

        let mesh_object = world.getObject(mesh_object_id)?;
        let ObjectInfo::Mesh(mesh_info) = &mesh_object.info else {
            return Err(anyhow::anyhow!("Expected mesh after update"));
        };
        assert_close_f32(mesh_info.scale.z, 6.5, "mesh.scale.z");
        assert_eq!(mesh_info.meshDataId, 9001);
        assert_eq!(expect_pointer_id(mesh_info.followSkeletonId), Some(skeleton_object_id));
        assert_eq!(expect_pointer_id(mesh_info.physicalObjectId), Some(brick_object_id));
        assert_eq!(
            expect_pointer_id(mesh_info.material.map(|mesh_material_settings| mesh_material_settings.materialInfoId)),
            Some(material_object_id)
        );
        assert_close_f32(mesh_info.visualSettings.opacity, 0.58, "mesh.opacity");
        assert_close_f32(mesh_info.visualSettings.color.b, 0.91, "mesh.color.b");
        assert!(mesh_info.visualSettings.isAlwaysOnTop);

        // This section updates player conversion fields.
        update_object_leaf(&mut world, player_object_id, &["username"], json!("new-username"))?;
        assert!(
            update_object_leaf(&mut world, player_object_id, &["userid"], json!(18)).is_err(),
            "userid should not be editable"
        );
        update_object_leaf(&mut world, player_object_id, &["headObjectId"], json!(brick_object_id))?;
        update_object_leaf(&mut world, player_object_id, &["characterObjectId"], Value::Null)?;

        let player_object = world.getObject(player_object_id)?;
        let ObjectInfo::Player(player_info) = &player_object.info else {
            return Err(anyhow::anyhow!("Expected player after update"));
        };
        assert_eq!(player_info.username, "new-username");
        assert_eq!(player_info.userid, Some(player_object_id as i64));
        assert_eq!(expect_pointer_id(player_info.headObjectId), Some(brick_object_id));
        assert_eq!(expect_pointer_id(player_info.characterObjectId), None);

        // This section updates rigid-group conversion fields and nested physics updates.
        update_object_leaf(&mut world, rigid_group_object_id, &["location", "position", "z"], json!(15.0))?;
        update_object_leaf(&mut world, rigid_group_object_id, &["movement", "velocity", "y"], json!(-2.0))?;
        update_object_leaf(&mut world, rigid_group_object_id, &["followSkeletonId"], json!(skeleton_object_id))?;

        let rigid_group_object = world.getObject(rigid_group_object_id)?;
        let ObjectInfo::RigidGroup(rigid_group_info) = &rigid_group_object.info else {
            return Err(anyhow::anyhow!("Expected rigid group after update"));
        };
        assert_eq!(expect_pointer_id(rigid_group_info.followSkeletonId), Some(skeleton_object_id));

        let Some(rigid_group_physics_info) = world.getObjectPhysicsInfo3D(rigid_group_object_id) else {
            return Err(anyhow::anyhow!("Expected rigid-group physics info after update"));
        };
        assert_close_f32(rigid_group_physics_info.location.position.z, 15.0, "rigidGroup.position.z");
        assert_close_f32(rigid_group_physics_info.movement.velocity.y, -2.0, "rigidGroup.velocity.y");

        // This section updates joint-attachment conversion fields and nested offsets.
        update_object_leaf(&mut world, sk_attachment_object_id, &["objectId"], json!(brick_object_id))?;
        update_object_leaf(&mut world, sk_attachment_object_id, &["jointName"], json!("left_hand_joint"))?;
        update_object_leaf(&mut world, sk_attachment_object_id, &["offsetFromJoint", "position", "z"], json!(4.0))?;

        let sk_attachment_object = world.getObject(sk_attachment_object_id)?;
        let ObjectInfo::SkAttachment(sk_attachment_info) = &sk_attachment_object.info else {
            return Err(anyhow::anyhow!("Expected joint-attachment after update"));
        };
        assert_eq!(expect_pointer_id(sk_attachment_info.objectId), Some(brick_object_id));
        assert_eq!(sk_attachment_info.jointName, "left_hand_joint");
        assert_vec3_close(
            sk_attachment_info.offsetFromJoint.position,
            Vec3::new(0.0, 0.0, 4.0),
            "skAttachment.offsetFromJoint.position",
        );

        // This section updates the script object info field that is currently exposed through JS conversions.
        update_object_leaf(&mut world, script_object_id, &["objectLocation"], json!("Client"))?;

        let script_object = world.getObject(script_object_id)?;
        let ObjectInfo::Script(script_info) = &script_object.info else {
            return Err(anyhow::anyhow!("Expected script after update"));
        };
        assert_eq!(script_info.objectLocation, ObjectLocation::Client);

        // This section updates SkAnimation conversion fields.
        update_object_leaf(&mut world, sk_animation_object_id, &["duration"], json!(2.5))?;
        update_object_leaf(&mut world, sk_animation_object_id, &["interpolationStyle"], json!("Linear"))?;

        let sk_animation_object = world.getObject(sk_animation_object_id)?;
        let ObjectInfo::SkAnimation(sk_animation_info) = &sk_animation_object.info else {
            return Err(anyhow::anyhow!("Expected SkAnimation after update"));
        };
        assert_close_f32(sk_animation_info.duration, 2.5, "updatedSkAnimation.duration");
        match sk_animation_info.interpolationStyle {
            InterpolationStyle::Linear => {}
            InterpolationStyle::EaseInOut => return Err(anyhow::anyhow!("Expected updated SkAnimation interpolation style")),
            InterpolationStyle::Step => return Err(anyhow::anyhow!("Expected updated SkAnimation interpolation style")),
            InterpolationStyle::EaseIn => return Err(anyhow::anyhow!("Expected updated SkAnimation interpolation style")),
            InterpolationStyle::EaseOut => return Err(anyhow::anyhow!("Expected updated SkAnimation interpolation style")),
        }

        update_object_leaf(&mut world, sk_keyframe_object_id, &["time"], json!(0.5))?;
        update_object_leaf(&mut world, sk_keyframe_pose_object_id, &["jointName"], json!("root"))?;
        update_object_leaf(&mut world, sk_keyframe_pose_object_id, &["localPose", "position", "x"], json!(4.0))?;

        let sk_keyframe_pose_object = world.getObject(sk_keyframe_pose_object_id)?;
        let ObjectInfo::SkKeyframePose(sk_keyframe_pose_info) = &sk_keyframe_pose_object.info else {
            return Err(anyhow::anyhow!("Expected SkKeyframePose after update"));
        };
        assert_eq!(sk_keyframe_pose_info.jointName, "root");
        assert_vec3_close(
            sk_keyframe_pose_info.localPose.position,
            Vec3::new(4.0, 0.0, 0.0),
            "updatedSkKeyframePose.localPose.position",
        );

        let sk_keyframe_object = world.getObject(sk_keyframe_object_id)?;
        let ObjectInfo::SkKeyframe(sk_keyframe_info) = &sk_keyframe_object.info else {
            return Err(anyhow::anyhow!("Expected SkKeyframe after update"));
        };
        assert_close_f32(sk_keyframe_info.time, 0.5, "updatedSkKeyframe.time");

        // This section updates SkAnimator conversion fields, with priority clamping and f32 weight clamping.
        update_object_leaf(&mut world, sk_animator_object_id, &["skeletonId"], json!(skeleton_object_id))?;
        update_object_leaf(&mut world, sk_animator_object_id, &["clockId"], json!(clock_object_id))?;
        update_object_leaf(&mut world, sk_animator_object_id, &["skAnimationId"], json!(sk_animation_object_id))?;
        update_object_leaf(&mut world, sk_animator_object_id, &["priority"], json!(400.9))?;
        update_object_leaf(&mut world, sk_animator_object_id, &["weight"], json!(-4.8))?;

        let sk_animator_object = world.getObject(sk_animator_object_id)?;
        let ObjectInfo::SkAnimator(sk_animator_info) = &sk_animator_object.info else {
            return Err(anyhow::anyhow!("Expected SkAnimator after update"));
        };
        assert_eq!(expect_pointer_id(sk_animator_info.skeletonId), Some(skeleton_object_id));
        assert_eq!(expect_pointer_id(sk_animator_info.clockId), Some(clock_object_id));
        assert_eq!(expect_pointer_id(sk_animator_info.skAnimationId), Some(sk_animation_object_id));
        assert_eq!(sk_animator_info.priority, 10);
        assert_close_f32(sk_animator_info.weight, 0.0, "updatedSkAnimator.weight");

        // This section updates sound player conversion fields.
        update_object_leaf(&mut world, sound_player_object_id, &["soundDataId"], json!(888))?;
        update_object_leaf(&mut world, sound_player_object_id, &["volume"], json!(0.88))?;
        update_object_leaf(&mut world, sound_player_object_id, &["clockId"], json!(clock_object_id))?;

        let sound_player_object = world.getObject(sound_player_object_id)?;
        let ObjectInfo::SoundPlayer(sound_player_info) = &sound_player_object.info else {
            return Err(anyhow::anyhow!("Expected sound player after update"));
        };
        assert_eq!(sound_player_info.soundDataId, 888);
        assert_close_f32(sound_player_info.volume, 0.88, "soundPlayer.volume");
        assert_eq!(expect_pointer_id(sound_player_info.clockId), Some(clock_object_id));

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

#[test]
fn rust_update_object_returns_errors_for_non_editable_or_invalid_paths() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();

    let clock_object_id = 2300;
    let script_object_id = 2301;
    let skeleton_object_id = 2302;
    let sound_player_object_id = 2303;
    let ball_object_id = 2304;
    let rigid_group_object_id = 2305;

    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_script(&mut world, script_object_id, root_object_id)?;
    add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
    add_sound_player(&mut world, sound_player_object_id, root_object_id, None)?;
    add_ball(&mut world, ball_object_id, root_object_id, Vec3::new(0.0, 0.0, 0.0), true)?;
    add_rigid_group(&mut world, rigid_group_object_id, root_object_id, Vec3::new(0.0, 0.0, 0.0), None)?;

    let script_update_result = update_object_leaf(&mut world, script_object_id, &["rawCode"], json!("new code"));
    assert!(script_update_result.is_err());

    let skeleton_update_result = update_object_leaf(&mut world, skeleton_object_id, &["joints"], json!([]));
    assert!(skeleton_update_result.is_err());

    let object_id_update_result = update_object_leaf(&mut world, sound_player_object_id, &["id"], json!(44));
    assert!(object_id_update_result.is_err());

    let object_type_update_result = update_object_leaf(&mut world, sound_player_object_id, &["type"], json!("ball"));
    assert!(object_type_update_result.is_err());

    let rigid_group_opacity_update_result = update_object_leaf(&mut world, rigid_group_object_id, &["opacity"], json!(0.25));
    assert!(rigid_group_opacity_update_result.is_err());

    let rigid_group_is_always_on_top_update_result = update_object_leaf(&mut world, rigid_group_object_id, &["isAlwaysOnTop"], json!(true));
    assert!(rigid_group_is_always_on_top_update_result.is_err());

    Ok(())
}

// Name and parent are written through the world's own setters, which keep its lookups up to date.
#[test]
fn rust_update_object_writes_name_and_parent() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let clock_object_id = 2300;
    let folder_object_id = 2301;
    add_clock(&mut world, clock_object_id, root_object_id)?;
    add_local_group(&mut world, folder_object_id, root_object_id)?;

    update_object_leaf(&mut world, clock_object_id, &["name"], json!("clock-renamed-from-test"))?;
    assert_eq!(world.getObject(clock_object_id)?.name, "clock-renamed-from-test");

    update_object_leaf(&mut world, clock_object_id, &["parent"], json!(folder_object_id))?;
    assert_eq!(world.parentOfId.get(&clock_object_id), Some(&folder_object_id));

    Ok(())
}

// -------------------------- Rust -> JS Conversions ---------------

#[test]
fn js_live_object_of_rust_live_object_converts_all_object_variants() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async {
        let mut world = make_world()?;
        let root_object_id = world.getRootObjectId();

        let clock_object_id = 2400;
        let sound_player_clock_object_id = 2411;
        let animation_clock_object_id = 2412;
        let property_animation_clock_object_id = 2419;
        let material_object_id = 2401;
        let skeleton_object_id = 2402;
        let ball_object_id = 2403;
        let brick_object_id = 2404;
        let cylinder_object_id = 2405;
        let mesh_object_id = 2406;
        let player_object_id = 2407;
        let rigid_group_object_id = 2408;
        let script_object_id = 2409;
        let sound_player_object_id = 2410;
        let sk_animator_object_id = 2413;
        let sk_animation_object_id = 2414;
        let sk_attachment_object_id = 2415;
        let sk_keyframe_object_id = 2416;
        let sk_keyframe_pose_object_id = 2417;
        let world_lighting_object_id = 2418;
        let property_animator_object_id = 2420;
        let property_animation_object_id = 2421;
        let property_keyframe_object_id = 2422;

        add_clock(&mut world, clock_object_id, root_object_id)?;
        add_clock(&mut world, sound_player_clock_object_id, root_object_id)?;
        add_clock(&mut world, animation_clock_object_id, root_object_id)?;
        add_clock(&mut world, property_animation_clock_object_id, root_object_id)?;
        add_material_without_textures(&mut world, material_object_id, root_object_id)?;
        add_skeleton(&mut world, skeleton_object_id, root_object_id)?;
        add_ball(&mut world, ball_object_id, root_object_id, Vec3::new(2.0, 3.0, 4.0), true)?;
        add_brick(&mut world, brick_object_id, root_object_id, Vec3::new(5.0, 6.0, 7.0), false)?;
        add_cylinder(&mut world, cylinder_object_id, root_object_id, Vec3::new(6.0, 7.0, 8.0), false)?;
        add_mesh(&mut world, mesh_object_id, root_object_id)?;
        add_player(&mut world, player_object_id, root_object_id, Some(ball_object_id))?;
        add_rigid_group(
            &mut world,
            rigid_group_object_id,
            root_object_id,
            Vec3::new(8.0, 9.0, 10.0),
            Some(skeleton_object_id),
        )?;
        add_script(&mut world, script_object_id, root_object_id)?;
        add_sound_player(&mut world, sound_player_object_id, root_object_id, Some(sound_player_clock_object_id))?;
        add_sk_animation(&mut world, sk_animation_object_id, root_object_id)?;
        add_sk_attachment(&mut world, sk_attachment_object_id, root_object_id, Some(brick_object_id))?;
        add_sk_keyframe(&mut world, sk_keyframe_object_id, sk_animation_object_id)?;
        add_sk_keyframe_pose(&mut world, sk_keyframe_pose_object_id, sk_keyframe_object_id)?;
        add_sk_animator(&mut world, sk_animator_object_id, root_object_id)?;
        add_property_animator(&mut world, property_animator_object_id, root_object_id)?;
        add_property_animation(&mut world, property_animation_object_id, root_object_id)?;
        add_property_keyframe(&mut world, property_keyframe_object_id, property_animation_object_id)?;
        world.addObjectWithId(
            world_lighting_object_id,
            "world-lighting".to_string(),
            ObjectBeingAdded::WorldLighting(WorldLightingInfo::default()),
            root_object_id,
        );

        update_object_leaf(&mut world, sk_animator_object_id, &["skeletonId"], json!(skeleton_object_id))?;
        update_object_leaf(&mut world, sk_animator_object_id, &["clockId"], json!(animation_clock_object_id))?;
        update_object_leaf(&mut world, sk_animator_object_id, &["skAnimationId"], json!(sk_animation_object_id))?;
        update_object_leaf(&mut world, property_animator_object_id, &["targetObjectId"], json!(ball_object_id))?;
        update_object_leaf(&mut world, property_animator_object_id, &["clockId"], json!(property_animation_clock_object_id))?;
        update_object_leaf(&mut world, ball_object_id, &["location", "rotation", "y"], json!(90.0))?;
        update_object_leaf(
            &mut world,
            world_lighting_object_id,
            &["sunOrientation"],
            json!({ "theta": 90.0, "phi": -30.0 }),
        )?;

        let mut js_runtime = JsRuntime::new(RuntimeOptions::default());

        // This section checks local-group conversion for the root object.
        let root_json = js_live_object_json(&mut js_runtime, &world, root_object_id)?;
        assert_eq!(root_json["id"].as_i64(), Some(jsObjectId(root_object_id)));
        assert_eq!(root_json["type"].as_str(), Some("group"));

        // This section checks clock conversion.
        let clock_json = js_live_object_json(&mut js_runtime, &world, clock_object_id)?;
        assert_eq!(clock_json["id"].as_i64(), Some(jsObjectId(clock_object_id)));
        assert_eq!(clock_json["type"].as_str(), Some("clock"));
        assert_eq!(clock_json["clockState"]["Playing"]["localStartMs"].as_f64(), Some(0.0));
        assert_eq!(clock_json["clockState"]["Playing"]["timeLocation"].as_str(), Some("Client"));

        // This section checks material conversion.
        let material_json = js_live_object_json(&mut js_runtime, &world, material_object_id)?;
        assert_eq!(material_json["type"].as_str(), Some("material"));
        assert_eq!(material_json["albedoTextureDataId"], Value::Null);
        assert_eq!(material_json["alphaMode"].as_str(), Some("Opaque"));
        assert!(material_json.get("mapping3D").is_none());
        update_object_leaf(&mut world, material_object_id, &["isDoubleSided"], json!(true))?;
        update_object_leaf(&mut world, material_object_id, &["alphaMode"], json!("Blend"))?;
        let material_json = js_live_object_json(&mut js_runtime, &world, material_object_id)?;
        assert_eq!(material_json["alphaMode"], json!("Blend"));
        assert_eq!(material_json["isDoubleSided"], json!(true));

        // This section checks world lighting conversion.
        let world_lighting_json = js_live_object_json(&mut js_runtime, &world, world_lighting_object_id)?;
        assert_eq!(world_lighting_json["type"].as_str(), Some("worldLighting"));
        assert!(world_lighting_json["sunOrientation"].is_object());
        assert_close_f32(
            world_lighting_json["sunOrientation"]["theta"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("Expected sun theta"))? as f32,
            90.0,
            "worldLightingJson.sunOrientation.theta",
        );
        assert_close_f32(
            world_lighting_json["sunOrientation"]["phi"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("Expected sun phi"))? as f32,
            -30.0,
            "worldLightingJson.sunOrientation.phi",
        );
        assert!(world_lighting_json["sunColor"].is_object());
        assert!(world_lighting_json["sunIntensity"].is_number());
        assert!(world_lighting_json["sunShadowSoftness"].is_number());
        assert!(world_lighting_json["ambientTopColor"].is_object());
        assert!(world_lighting_json["ambientBottomColor"].is_object());
        assert!(world_lighting_json["ambientIntensity"].is_number());
        assert!(world_lighting_json["ambientOcclusionIntensity"].is_number());
        assert!(world_lighting_json["ambientOcclusionRadius"].is_number());
        assert!(world_lighting_json["ambientOcclusionSunlightStrength"].is_number());
        assert!(world_lighting_json["skyTopColor"].is_object());
        assert!(world_lighting_json["skyHorizonColor"].is_object());
        assert!(world_lighting_json["skyBottomColor"].is_object());
        assert_eq!(world_lighting_json["skyboxTextureDataId"], Value::Null);
        assert_close_f32(
            world_lighting_json["skyboxTextureOpacity"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("Expected skybox texture opacity"))? as f32,
            1.0,
            "worldLightingJson.skyboxTextureOpacity",
        );
        assert!(world_lighting_json["skyReflectionStrength"].is_number());
        assert!(world_lighting_json["vibrance"].is_number());

        // This section checks ball conversion, including spread physics and visuals.
        let ball_json = js_live_object_json(&mut js_runtime, &world, ball_object_id)?;
        assert_eq!(ball_json["type"].as_str(), Some("ball"));
        assert!(ball_json["radius"].is_number());
        assert!(ball_json["material"].is_null());
        assert!(ball_json["location"].is_object());
        assert_close_f32(
            ball_json["location"]["rotation"]["y"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("Expected ball rotation y"))? as f32,
            90.0,
            "ballJson.location.rotation.y",
        );
        assert!(ball_json["movement"].is_object());
        assert!(ball_json["color"].is_object());
        assert!(ball_json["isPushable"].is_boolean());

        // This section checks brick conversion.
        let brick_json = js_live_object_json(&mut js_runtime, &world, brick_object_id)?;
        assert_eq!(brick_json["type"].as_str(), Some("brick"));
        assert!(brick_json["size"].is_object());
        assert!(brick_json["material"].is_null());
        assert!(brick_json["location"].is_object());

        // This section checks cylinder conversion.
        let cylinder_json = js_live_object_json(&mut js_runtime, &world, cylinder_object_id)?;
        assert_eq!(cylinder_json["type"].as_str(), Some("cylinder"));
        assert!(cylinder_json["radius"].is_number());
        assert!(cylinder_json["length"].is_number());
        assert!(cylinder_json["material"].is_null());
        assert!(cylinder_json["location"].is_object());

        // This section checks mesh conversion.
        let mesh_json = js_live_object_json(&mut js_runtime, &world, mesh_object_id)?;
        assert_eq!(mesh_json["type"].as_str(), Some("mesh"));
        assert!(mesh_json["scale"].is_object());
        assert!(mesh_json["meshDataId"].is_number());
        assert!(mesh_json["material"].is_null());

        // This section checks player conversion.
        let player_json = js_live_object_json(&mut js_runtime, &world, player_object_id)?;
        assert_eq!(player_json["type"].as_str(), Some("player"));
        assert!(player_json["headObjectId"].is_null());
        assert!(player_json["characterObjectId"].is_number());

        // This section checks rigid-group conversion.
        let rigid_group_json = js_live_object_json(&mut js_runtime, &world, rigid_group_object_id)?;
        assert_eq!(rigid_group_json["type"].as_str(), Some("rigidGroup"));
        assert_eq!(rigid_group_json["followSkeletonId"].as_i64(), Some(jsObjectId(skeleton_object_id)));
        assert!(rigid_group_json.get("opacity").is_none());
        assert!(rigid_group_json.get("isAlwaysOnTop").is_none());
        assert!(rigid_group_json["location"].is_object());

        // This section checks joint-attachment conversion.
        let sk_attachment_json = js_live_object_json(&mut js_runtime, &world, sk_attachment_object_id)?;
        assert_eq!(sk_attachment_json["type"].as_str(), Some("skAttachment"));
        assert_eq!(sk_attachment_json["objectId"].as_i64(), Some(jsObjectId(brick_object_id)));
        assert_eq!(sk_attachment_json["jointName"].as_str(), Some("joint"));
        assert!(sk_attachment_json["offsetFromJoint"].is_object());

        // This section checks script conversion.
        let script_json = js_live_object_json(&mut js_runtime, &world, script_object_id)?;
        assert_eq!(script_json["type"].as_str(), Some("script"));

        // This section checks sound player conversion.
        let sound_player_json = js_live_object_json(&mut js_runtime, &world, sound_player_object_id)?;
        assert_eq!(sound_player_json["type"].as_str(), Some("soundPlayer"));
        assert!(sound_player_json["soundDataId"].is_number());
        assert_eq!(sound_player_json["clockId"].as_i64(), Some(jsObjectId(sound_player_clock_object_id)));

        // This section checks Skeleton conversion.
        let skeleton_json = js_live_object_json(&mut js_runtime, &world, skeleton_object_id)?;
        assert_eq!(skeleton_json["type"].as_str(), Some("skeleton"));

        // This section checks joint-animation conversion.
        let sk_animation_json = js_live_object_json(&mut js_runtime, &world, sk_animation_object_id)?;
        assert_eq!(sk_animation_json["type"].as_str(), Some("skAnimation"));
        assert_eq!(sk_animation_json["duration"].as_f64(), Some(0.0));
        assert_eq!(sk_animation_json["interpolationStyle"].as_str(), Some("Step"));

        // This section checks joint-keyframe and joint-pose conversion.
        let sk_keyframe_json = js_live_object_json(&mut js_runtime, &world, sk_keyframe_object_id)?;
        assert_eq!(sk_keyframe_json["type"].as_str(), Some("skKeyframe"));
        assert_eq!(sk_keyframe_json["time"].as_f64(), Some(0.0));

        let sk_keyframe_pose_json = js_live_object_json(&mut js_runtime, &world, sk_keyframe_pose_object_id)?;
        assert_eq!(sk_keyframe_pose_json["type"].as_str(), Some("skKeyframePose"));
        assert_eq!(sk_keyframe_pose_json["jointName"].as_str(), Some("joint"));
        assert!(sk_keyframe_pose_json["localPose"].is_object());

        // This section checks joint-animator conversion.
        let sk_animator_json = js_live_object_json(&mut js_runtime, &world, sk_animator_object_id)?;
        assert_eq!(sk_animator_json["type"].as_str(), Some("skAnimator"));
        assert_eq!(sk_animator_json["skeletonId"].as_i64(), Some(jsObjectId(skeleton_object_id)));
        assert_eq!(sk_animator_json["clockId"].as_i64(), Some(jsObjectId(animation_clock_object_id)));
        assert_eq!(sk_animator_json["skAnimationId"].as_i64(), Some(jsObjectId(sk_animation_object_id)));

        // This section checks property-animator conversion.
        let property_animator_json = js_live_object_json(&mut js_runtime, &world, property_animator_object_id)?;
        assert_eq!(property_animator_json["type"].as_str(), Some("propertyAnimator"));
        assert_eq!(property_animator_json["targetObjectId"].as_i64(), Some(jsObjectId(ball_object_id)));
        assert_eq!(property_animator_json["clockId"].as_i64(), Some(jsObjectId(property_animation_clock_object_id)));
        assert!(property_animator_json["propertyAnimationId"].is_null());
        assert!(property_animator_json["weightTransition"].is_null());

        // This section checks property-animation and property-keyframe conversion.
        let property_animation_json = js_live_object_json(&mut js_runtime, &world, property_animation_object_id)?;
        assert_eq!(property_animation_json["type"].as_str(), Some("propertyAnimation"));
        assert_eq!(property_animation_json["duration"].as_f64(), Some(0.0));
        assert_eq!(property_animation_json["interpolationStyle"].as_str(), Some("Step"));

        let property_keyframe_json = js_live_object_json(&mut js_runtime, &world, property_keyframe_object_id)?;
        assert_eq!(property_keyframe_json["type"].as_str(), Some("propertyKeyframe"));
        assert_eq!(property_keyframe_json["time"].as_f64(), Some(0.0));
        assert_eq!(property_keyframe_json["value"].as_f64(), Some(0.0));

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

// A Pivot's pointer and point survive every direction a script touches them: creating one, editing one field, and reading it back.
#[test]
fn pivot_converts_from_and_to_scripts_and_updates_its_fields() -> anyhow::Result<()> {
    let ObjectBeingAdded::Pivot(pivot_info) = rustObjectBeingAddedOfJSObjectInfo(&json!({
        "type": "pivot",
        "physicalObjectId": 41,
        "offsetFromObject": { "x": -1.0, "y": 0.5, "z": 2.0 },
    }))?
    else {
        return Err(anyhow::anyhow!("Expected pivot conversion"));
    };
    assert_eq!(expect_pointer_id(pivot_info.physicalObjectId), Some(41));
    assert_vec3_close(pivot_info.offsetFromObject, Vec3::new(-1.0, 0.5, 2.0), "pivot.offsetFromObject");

    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let door_object_id = 2;
    let pivot_object_id = 3;
    add_brick(&mut world, door_object_id, root_object_id, Vec3::ZERO, false)?;
    add_pivot(&mut world, pivot_object_id, root_object_id, None, Vec3::ZERO)?;
    update_object_leaf(&mut world, pivot_object_id, &["physicalObjectId"], json!(door_object_id))?;
    update_object_leaf(&mut world, pivot_object_id, &["offsetFromObject", "x"], json!(-1.5))?;

    let mut js_runtime = JsRuntime::new(RuntimeOptions::default());
    let pivot_json = js_live_object_json(&mut js_runtime, &world, pivot_object_id)?;
    assert_eq!(pivot_json["type"], json!("pivot"));
    assert_eq!(pivot_json["physicalObjectId"], json!(door_object_id));
    for (axis, expected) in [("x", -1.5), ("y", 0.0), ("z", 0.0)] {
        assert_eq!(pivot_json["offsetFromObject"][axis].as_f64(), Some(expected), "pivot.offsetFromObject.{axis}");
    }
    Ok(())
}

// A script or animator writing one axis of a label's or label image's offset or rotation keeps the other two axes.
#[test]
fn label_and_label_image_offsets_and_rotations_take_one_axis() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let label_object_id = 2;
    let label_image_object_id = 3;
    add_label(&mut world, label_object_id, root_object_id)?;
    add_label_image(&mut world, label_image_object_id, root_object_id)?;

    for object_id in [label_object_id, label_image_object_id] {
        update_object_leaf(&mut world, object_id, &["offsetFromObject"], json!({ "x": 1.0, "y": 2.0, "z": 3.0 }))?;
        update_object_leaf(&mut world, object_id, &["offsetFromObject", "x"], json!(-1.5))?;
        update_object_leaf(&mut world, object_id, &["rotationFromObject"], json!({ "x": 0.0, "y": 30.0, "z": 0.0 }))?;
        update_object_leaf(&mut world, object_id, &["rotationFromObject", "x"], json!(20.0))?;

        let (offset_from_object, rotation_from_object) = match &world.getObject(object_id)?.info {
            ObjectInfo::Label(label_info) => (label_info.offsetFromObject, label_info.rotationFromObject),
            ObjectInfo::LabelImage(label_image_info) => (label_image_info.offsetFromObject, label_image_info.rotationFromObject),
            _ => return Err(anyhow::anyhow!("Expected a label or a label image")),
        };
        assert_vec3_close(offset_from_object, Vec3::new(-1.5, 2.0, 3.0), &format!("{object_id}.offsetFromObject"));
        assert_quat_close(
            rotation_from_object,
            euler_degrees_to_quat(Vec3::new(20.0, 30.0, 0.0)),
            &format!("{object_id}.rotationFromObject"),
        );
    }
    Ok(())
}

#[test]
fn js_object_id_converts_usize_to_i64() {
    let converted_id = jsObjectId(91234);
    assert_eq!(converted_id, 91234_i64);
}

// -------------------------- Regression Guard for Physics Struct Sync ---------------

#[test]
fn converted_physics_info_matches_expected_shape_for_ball_and_rigid_group() -> anyhow::Result<()> {
    // This test guards the shared physics conversion shape used by both Ball and RigidGroup paths.
    let ball_value = json!({
        "type": "ball",
        "radius": 1.0,
        "material": {
            "materialInfoId": 4242,
            "tileSizeU": 1.0,
            "tileSizeV": 1.0,
            "tileOffsetU": 0.0,
            "tileOffsetV": 0.0
        },
        "opacity": 1.0,
        "color": { "r": 0.5, "g": 0.5, "b": 0.5 },
        "isWalkthrough": false,
        "isPushable": false,
        "density": 1.0,
        "location": {
            "position": { "x": 3.0, "y": 2.0, "z": 1.0 },
            "rotation": { "x": 0.0, "y": 0.0, "z": 0.0 }
        },
        "movement": {
            "velocity": { "x": 6.0, "y": 5.0, "z": 4.0 },
            "angularVelocity": { "x": 9.0, "y": 8.0, "z": 7.0 }
        }
    });

    let rigid_group_value = json!({
        "type": "rigidGroup",
        "followSkeletonId": null,
        "location": {
            "position": { "x": 3.0, "y": 2.0, "z": 1.0 },
            "rotation": { "x": 0.0, "y": 0.0, "z": 0.0 }
        },
        "movement": {
            "velocity": { "x": 6.0, "y": 5.0, "z": 4.0 },
            "angularVelocity": { "x": 9.0, "y": 8.0, "z": 7.0 }
        }
    });

    let ball_converted = rustObjectBeingAddedOfJSObjectInfo(&ball_value)?;
    let rigid_group_converted = rustObjectBeingAddedOfJSObjectInfo(&rigid_group_value)?;

    let ball_physics = match ball_converted {
        ObjectBeingAdded::Ball(_, _, physics_info, physics_toggles) => {
            assert_eq!(
                physics_toggles.isPushable,
                PhysicsToggles3D {
                    isWalkthrough: false,
                    isPushable: false,
                    density: 1.0,
                }
                .isPushable
            );
            physics_info
        }
        _ => {
            return Err(anyhow::anyhow!("Expected ball conversion"));
        }
    };

    let rigid_group_physics = match rigid_group_converted {
        ObjectBeingAdded::RigidGroup(_, physics_info) => physics_info,
        _ => {
            return Err(anyhow::anyhow!("Expected rigid-group conversion"));
        }
    };

    let PhysicsInfo3D {
        location: ball_location,
        movement: ball_movement,
    } = ball_physics;
    let PhysicsInfo3D {
        location: rigid_group_location,
        movement: rigid_group_movement,
    } = rigid_group_physics;

    assert_vec3_close(ball_location.position, rigid_group_location.position, "sharedPhysics.location.position");
    assert_quat_close(ball_location.quaternion, rigid_group_location.quaternion, "sharedPhysics.location.rotation");
    assert_vec3_close(ball_movement.velocity, rigid_group_movement.velocity, "sharedPhysics.movement.velocity");
    assert_vec3_close(
        ball_movement.angularVelocity,
        rigid_group_movement.angularVelocity,
        "sharedPhysics.movement.angularVelocity",
    );
    assert_vec3_close(
        ball_movement.angularVelocity,
        Vec3::new(9.0_f32.to_radians(), 8.0_f32.to_radians(), 7.0_f32.to_radians()),
        "sharedPhysics.movement.angularVelocity.degreesToRadians",
    );

    Ok(())
}
