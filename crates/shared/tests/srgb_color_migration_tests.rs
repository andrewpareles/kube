mod helpers;

use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    object_types::{ObjectInfo, WorldLightingInfo, shared::Color3},
};
use glam::Vec3;
use helpers::world_test_helpers::{
    add_brick, add_local_group, add_material, add_mesh, add_property_animation, add_property_animator, add_property_keyframe, add_world_lighting, make_world,
};
use serde_json::{Value, json};
use shared::{
    api::protocol_for_assets::Asset,
    game::{
        assets::{AssetData, StoredAssetObject},
        world::{World, WorldControlMode},
    },
};

// sRGB values that decode to 0.5, 0.25 and 0.2 of linear light.
const ENCODED_HALF: f32 = 0.735357;
const ENCODED_QUARTER: f32 = 0.537099;
const ENCODED_FIFTH: f32 = 0.484529;

// Fields that world lighting only got at v76 or later.
const WORLD_LIGHTING_FIELDS_ADDED_SINCE_V75: [&str; 6] = [
    "sunShadowSoftness",
    "ambientOcclusionIntensity",
    "ambientOcclusionRadius",
    "ambientOcclusionSunlightStrength",
    "skyReflectionStrength",
    "vibrance",
];

// The stored form of this world, saved the way a v75 world was: material 20 has a color texture, brick 11 is
// textured with it, and brick 12 is plain.
fn stored_v75_world(world: &World) -> anyhow::Result<Value> {
    let mut stored: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored["worldSchemaVersion"] = json!(75);
    for object in stored["objectOfId"].as_object_mut().expect("the world should hold objects").values_mut() {
        for object_info in object["info"].as_object_mut().expect("every object should hold its info").values_mut() {
            if let Some(visual_settings) = object_info
                .get_mut("visualSettings")
                .and_then(|visual_settings| visual_settings.as_object_mut())
            {
                visual_settings.remove("glow");
            }
        }
        if let Some(world_lighting) = object["info"]
            .get_mut("WorldLighting")
            .and_then(|world_lighting| world_lighting.as_object_mut())
        {
            for field in WORLD_LIGHTING_FIELDS_ADDED_SINCE_V75 {
                world_lighting.remove(field);
            }
        }
    }
    Ok(stored)
}

fn make_world_with_textured_and_plain_bricks() -> anyhow::Result<World> {
    let mut world = make_world()?;
    add_material(&mut world, 20, 1)?;
    add_brick(&mut world, 11, 1, Vec3::ZERO, false)?;
    add_brick(&mut world, 12, 1, Vec3::new(3.0, 0.0, 0.0), false)?;
    Ok(world)
}

fn texture_brick_11_and_color_both_bricks(stored: &mut Value, color: Value) {
    stored["objectOfId"]["11"]["info"]["Brick"]["material"] =
        json!({ "materialInfoId": 20, "tileSizeU": 1.0, "tileSizeV": 1.0, "tileOffsetU": 0.0, "tileOffsetV": 0.0 });
    for brick_id in ["11", "12"] {
        stored["objectOfId"][brick_id]["info"]["Brick"]["visualSettings"]["color"] = color.clone();
    }
}

fn load(stored: &Value) -> anyhow::Result<World> {
    World::fromStoredWorldString(&serde_json::to_string(stored)?, WorldControlMode::Server, None)
}

// Loading hands out fresh object ids, so objects are found by name.
fn object_info<'a>(world: &'a World, name: &str) -> &'a ObjectInfo {
    let object_id = *world.getObjectIdsByName(name).first().unwrap_or_else(|| panic!("{name} should still exist"));
    &world.getObject(object_id).unwrap_or_else(|_| panic!("{name} should still exist")).info
}

fn visual_color(world: &World, name: &str) -> Color3 {
    match object_info(world, name) {
        ObjectInfo::Brick(brick_info) => brick_info.visualSettings.color,
        ObjectInfo::Mesh(mesh_info) => mesh_info.visualSettings.color,
        _ => panic!("{name} should be a brick or a mesh"),
    }
}

fn world_lighting(world: &World, name: &str) -> WorldLightingInfo {
    let ObjectInfo::WorldLighting(world_lighting_info) = object_info(world, name) else {
        panic!("{name} should be world lighting");
    };
    *world_lighting_info
}

fn keyframe_value(world: &World, name: &str) -> f32 {
    let ObjectInfo::PropertyKeyframe(keyframe_info) = object_info(world, name) else {
        panic!("{name} should be a keyframe");
    };
    keyframe_info.value
}

fn assert_color_near(color: Color3, expected: [f32; 3], what: &str) {
    let channels = [color.r, color.g, color.b];
    assert!(
        channels.iter().zip(expected).all(|(channel, expected)| (channel - expected).abs() < 1e-4),
        "{what} should be {expected:?}, not {channels:?}"
    );
}

// Before v76 every color was used as linear light. A tint multiplies a texture that was always decoded correctly, so it
// is encoded to give the texture the same light as before, while a plain color is left to show the way it was picked.
#[test]
fn tints_on_textures_keep_their_light_and_plain_colors_stay_as_picked() -> anyhow::Result<()> {
    let mut world = make_world_with_textured_and_plain_bricks()?;
    add_mesh(&mut world, 13, 1)?;
    let mut stored = stored_v75_world(&world)?;
    texture_brick_11_and_color_both_bricks(&mut stored, json!({ "r": 0.5, "g": 0.25, "b": 1.0 }));
    stored["objectOfId"]["13"]["info"]["Mesh"]["visualSettings"]["color"] = json!({ "r": 0.5, "g": 0.2, "b": 0.0 });

    let loaded_world = load(&stored)?;
    assert_color_near(
        visual_color(&loaded_world, "brick-11"),
        [ENCODED_HALF, ENCODED_QUARTER, 1.0],
        "the textured brick's tint",
    );
    assert_color_near(visual_color(&loaded_world, "brick-12"), [0.5, 0.25, 1.0], "the plain brick's color");
    assert_color_near(visual_color(&loaded_world, "mesh-13"), [ENCODED_HALF, ENCODED_FIFTH, 0.0], "the mesh's tint");
    Ok(())
}

// The sun and ambient light colors keep giving the same light. The sky is seen directly, like a plain color, so it keeps
// its colors, and no intensity changes.
#[test]
fn sun_and_ambient_colors_keep_their_light_and_the_sky_keeps_its_colors() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_world_lighting(&mut world, 30, 1)?;
    let mut stored = stored_v75_world(&world)?;
    let saved_lighting = &mut stored["objectOfId"]["30"]["info"]["WorldLighting"];
    saved_lighting["sunColor"] = json!({ "r": 1.0, "g": 0.5, "b": 0.25 });
    saved_lighting["sunIntensity"] = json!(2.5);
    saved_lighting["ambientTopColor"] = json!({ "r": 0.5, "g": 0.5, "b": 0.5 });
    saved_lighting["ambientBottomColor"] = json!({ "r": 0.25, "g": 0.2, "b": 0.0 });
    saved_lighting["ambientIntensity"] = json!(0.75);
    saved_lighting["skyTopColor"] = json!({ "r": 0.3, "g": 0.5, "b": 0.9 });

    let lighting = world_lighting(&load(&stored)?, "world-lighting-30");
    assert_color_near(lighting.sunColor, [1.0, ENCODED_HALF, ENCODED_QUARTER], "the sun color");
    assert_color_near(lighting.ambientTopColor, [ENCODED_HALF, ENCODED_HALF, ENCODED_HALF], "the ambient top color");
    assert_color_near(lighting.ambientBottomColor, [ENCODED_QUARTER, ENCODED_FIFTH, 0.0], "the ambient bottom color");
    assert_color_near(lighting.skyTopColor, [0.3, 0.5, 0.9], "the sky top color");
    assert_eq!(lighting.sunIntensity, 2.5);
    assert_eq!(lighting.ambientIntensity, 0.75);
    Ok(())
}

// Lighting still at every old default was never picked by anyone, so it gets the new defaults.
#[test]
fn lighting_still_at_the_old_defaults_gets_the_new_defaults() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_world_lighting(&mut world, 30, 1)?;
    let mut stored = stored_v75_world(&world)?;
    let saved_lighting = stored["objectOfId"]["30"]["info"]["WorldLighting"]
        .as_object_mut()
        .expect("the lighting should be saved as an object");
    for (field, old_default) in [
        ("sunColor", json!({ "r": 1.0, "g": 0.93333334, "b": 0.8888889 })),
        ("sunIntensity", json!(4.5)),
        ("ambientTopColor", json!({ "r": 0.05, "g": 0.06, "b": 0.08 })),
        ("ambientBottomColor", json!({ "r": 0.03, "g": 0.025, "b": 0.02 })),
        ("ambientIntensity", json!(1.0)),
        ("skyTopColor", json!({ "r": 0.29, "g": 0.51, "b": 0.85 })),
        ("skyHorizonColor", json!({ "r": 0.75, "g": 0.86, "b": 0.94 })),
        ("skyBottomColor", json!({ "r": 0.24, "g": 0.22, "b": 0.2 })),
    ] {
        saved_lighting.insert(field.to_string(), old_default);
    }

    let lighting = world_lighting(&load(&stored)?, "world-lighting-30");
    let new_defaults = WorldLightingInfo::default();
    for (loaded, new_default, what) in [
        (lighting.sunColor, new_defaults.sunColor, "the sun color"),
        (lighting.ambientTopColor, new_defaults.ambientTopColor, "the ambient top color"),
        (lighting.ambientBottomColor, new_defaults.ambientBottomColor, "the ambient bottom color"),
        (lighting.skyTopColor, new_defaults.skyTopColor, "the sky top color"),
        (lighting.skyHorizonColor, new_defaults.skyHorizonColor, "the sky horizon color"),
        (lighting.skyBottomColor, new_defaults.skyBottomColor, "the sky bottom color"),
    ] {
        assert_color_near(loaded, [new_default.r, new_default.g, new_default.b], what);
    }
    assert_eq!(lighting.sunIntensity, new_defaults.sunIntensity);
    assert_eq!(lighting.ambientIntensity, new_defaults.ambientIntensity);
    Ok(())
}

// Keyframes that animate an encoded color are encoded with it. A keyframe whose animation also drives something that did
// not change keeps its value, and so does one that animates a plain color.
#[test]
fn keyframes_follow_the_colors_they_animate() -> anyhow::Result<()> {
    let mut world = make_world_with_textured_and_plain_bricks()?;
    for (animation_id, keyframe_ids) in [(40, &[41, 42][..]), (50, &[51][..]), (60, &[61][..])] {
        add_property_animation(&mut world, animation_id, 1)?;
        for keyframe_id in keyframe_ids {
            add_property_keyframe(&mut world, *keyframe_id, animation_id)?;
        }
    }
    for animator_id in [43, 52, 53, 62] {
        add_property_animator(&mut world, animator_id, 1)?;
    }
    let mut stored = stored_v75_world(&world)?;
    texture_brick_11_and_color_both_bricks(&mut stored, json!({ "r": 1.0, "g": 1.0, "b": 1.0 }));
    for (keyframe_id, value) in [("41", 0.5), ("42", 0.25), ("51", 0.5), ("61", 0.5)] {
        stored["objectOfId"][keyframe_id]["info"]["PropertyKeyframe"]["value"] = json!(value);
    }
    for (animator_id, target_id, property_path, animation_id) in [
        ("43", 11, json!(["color", "g"]), 40),
        ("52", 11, json!(["color", "r"]), 50),
        ("53", 11, json!(["location", "position", "y"]), 50),
        ("62", 12, json!(["color", "b"]), 60),
    ] {
        let animator = &mut stored["objectOfId"][animator_id]["info"]["PropertyAnimator"];
        animator["targetObjectId"] = json!(target_id);
        animator["propertyPath"] = property_path;
        animator["propertyAnimationId"] = json!(animation_id);
    }

    let loaded_world = load(&stored)?;
    assert!((keyframe_value(&loaded_world, "property-keyframe-41") - ENCODED_HALF).abs() < 1e-4);
    assert!((keyframe_value(&loaded_world, "property-keyframe-42") - ENCODED_QUARTER).abs() < 1e-4);
    assert_eq!(
        keyframe_value(&loaded_world, "property-keyframe-51"),
        0.5,
        "a keyframe that also moves the brick keeps its value"
    );
    assert_eq!(
        keyframe_value(&loaded_world, "property-keyframe-61"),
        0.5,
        "a keyframe on a plain color keeps its value"
    );
    Ok(())
}

// Worlds saved at v76 or later already use sRGB colors, so none of their colors change.
#[test]
fn worlds_saved_with_srgb_colors_keep_every_color() -> anyhow::Result<()> {
    let mut stored = stored_v75_world(&make_world_with_textured_and_plain_bricks()?)?;
    texture_brick_11_and_color_both_bricks(&mut stored, json!({ "r": 0.5, "g": 0.25, "b": 1.0 }));
    stored["worldSchemaVersion"] = json!(76);

    let loaded_world = load(&stored)?;
    assert_color_near(visual_color(&loaded_world, "brick-11"), [0.5, 0.25, 1.0], "the textured brick's tint");
    Ok(())
}

// Asset objects are saved at the world schema version of their day. One from before v76 loads up to date, with its
// tint encoded, instead of being turned away for its version.
#[test]
fn an_old_asset_object_loads_up_to_date() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_local_group(&mut world, 70, 1)?;
    add_material(&mut world, 20, 70)?;
    add_brick(&mut world, 11, 70, Vec3::ZERO, false)?;
    let mut stored: Value = serde_json::from_str(&serde_json::to_string(&StoredAssetObject::from_world_object(&world, 70)?)?)?;
    stored["worldSchemaVersion"] = json!(75);
    let brick = &mut stored["objectOfId"]["11"]["info"]["Brick"];
    brick["material"] = json!({ "materialInfoId": 20, "tileSizeU": 1.0, "tileSizeV": 1.0, "tileOffsetU": 0.0, "tileOffsetV": 0.0 });
    brick["visualSettings"]["color"] = json!({ "r": 0.5, "g": 0.25, "b": 1.0 });
    brick["visualSettings"]
        .as_object_mut()
        .expect("the brick should hold visual settings")
        .remove("glow");

    let asset_row = json!({ "asset_id": 1, "version_num": 1, "name": "brick", "created_at": "", "asset_type": "Object", "data_json": { "Object": stored } });
    let AssetData::Object(asset_object) = Asset::fromJson(asset_row)?.data_json else {
        panic!("expected an asset object");
    };
    asset_object.validate()?;
    let loaded: Value = serde_json::to_value(&asset_object)?;
    assert_eq!(loaded["worldSchemaVersion"], json!(STORED_WORLD_CURRENT_VERSION));
    let loaded_visual_settings = &loaded["objectOfId"]["11"]["info"]["Brick"]["visualSettings"];
    assert_eq!(loaded_visual_settings["glow"], json!(0.0));
    let loaded_color = serde_json::from_value::<Color3>(loaded_visual_settings["color"].clone())?;
    assert_color_near(loaded_color, [ENCODED_HALF, ENCODED_QUARTER, 1.0], "the asset brick's tint");
    Ok(())
}

// Imported models were tinted gray, which darkened their textures. A model whose texture is still under that gray, or under the one
// v76 encoded it to, shows the texture's own colors. A model without a texture keeps its gray, a tint someone picked stays, and so does
// a brick's color.
#[test]
fn models_still_on_the_old_gray_tint_show_their_textures_own_colors() -> anyhow::Result<()> {
    let mut world = make_world_with_textured_and_plain_bricks()?;
    for mesh_id in [13, 14, 15, 16] {
        add_mesh(&mut world, mesh_id, 1)?;
    }
    let mut stored: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored["worldSchemaVersion"] = json!(88);
    texture_brick_11_and_color_both_bricks(&mut stored, json!({ "r": 0.5, "g": 0.5, "b": 0.5 }));
    for mesh_id in ["13", "14", "15"] {
        stored["objectOfId"][mesh_id]["info"]["Mesh"]["material"] = json!({ "materialInfoId": 20 });
    }
    for (mesh_id, color) in [
        ("13", json!({ "r": 0.5, "g": 0.5, "b": 0.5 })),
        ("14", json!({ "r": ENCODED_HALF, "g": ENCODED_HALF, "b": ENCODED_HALF })),
        ("15", json!({ "r": 0.5, "g": 0.25, "b": 1.0 })),
        ("16", json!({ "r": 0.5, "g": 0.5, "b": 0.5 })),
    ] {
        stored["objectOfId"][mesh_id]["info"]["Mesh"]["visualSettings"]["color"] = color;
    }

    let loaded_world = load(&stored)?;
    assert_color_near(visual_color(&loaded_world, "mesh-13"), [1.0, 1.0, 1.0], "the textured mesh on the gray tint");
    assert_color_near(visual_color(&loaded_world, "mesh-14"), [1.0, 1.0, 1.0], "the textured mesh on the encoded gray");
    assert_color_near(visual_color(&loaded_world, "mesh-15"), [0.5, 0.25, 1.0], "the mesh's picked tint");
    assert_color_near(visual_color(&loaded_world, "mesh-16"), [0.5, 0.5, 0.5], "the mesh without a texture");
    assert_color_near(visual_color(&loaded_world, "brick-11"), [0.5, 0.5, 0.5], "the textured brick's tint");
    Ok(())
}

// A textured model saved before v76 on the gray tint of its day shows its texture's own colors too.
#[test]
fn models_saved_before_v76_on_the_gray_tint_show_their_textures_own_colors() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_material(&mut world, 20, 1)?;
    add_mesh(&mut world, 13, 1)?;
    let mut stored = stored_v75_world(&world)?;
    stored["objectOfId"]["13"]["info"]["Mesh"]["material"] = json!({ "materialInfoId": 20 });
    stored["objectOfId"]["13"]["info"]["Mesh"]["visualSettings"]["color"] = json!({ "r": 0.5, "g": 0.5, "b": 0.5 });

    assert_color_near(visual_color(&load(&stored)?, "mesh-13"), [1.0, 1.0, 1.0], "the mesh on the old gray tint");
    Ok(())
}
