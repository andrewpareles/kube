mod helpers;

use game_types::consts::STORED_WORLD_CURRENT_VERSION;
use game_types::object_types::{ObjectBeingAdded, ObjectInfo, WorldLightingInfo};
use helpers::world_test_helpers::make_world;
use serde_json::{Value, json};
use shared::game::world::{World, WorldControlMode};

// World lighting saved before its shadow softness and ambient occlusion settings existed has to load with the default
// ones, and keep every lighting value it was saved with.
#[test]
fn old_world_lighting_loads_with_default_shadow_softness_and_ambient_occlusion() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let saved_world_lighting_info = WorldLightingInfo {
        sunIntensity: 2.25,
        ..WorldLightingInfo::default()
    };
    world.addObjectWithId(11, "lighting".to_string(), ObjectBeingAdded::WorldLighting(saved_world_lighting_info), 1);
    let mut stored_world: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored_world["worldSchemaVersion"] = json!(75);
    let world_lighting_json = stored_world["objectOfId"]["11"]["info"]["WorldLighting"]
        .as_object_mut()
        .expect("the saved world should hold the lighting");
    for field in [
        "sunShadowSoftness",
        "ambientOcclusionIntensity",
        "ambientOcclusionRadius",
        "ambientOcclusionSunlightStrength",
    ] {
        world_lighting_json.remove(field);
    }

    let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
    let object_id = *reloaded_world.getObjectIdsByName("lighting").first().expect("the lighting should still exist");
    let ObjectInfo::WorldLighting(world_lighting_info) = &reloaded_world.getObject(object_id)?.info else {
        panic!("the lighting should still be world lighting");
    };
    let default_world_lighting_info = WorldLightingInfo::default();
    assert_eq!(world_lighting_info.sunShadowSoftness, default_world_lighting_info.sunShadowSoftness);
    assert_eq!(
        world_lighting_info.ambientOcclusionIntensity,
        default_world_lighting_info.ambientOcclusionIntensity
    );
    assert_eq!(world_lighting_info.ambientOcclusionRadius, default_world_lighting_info.ambientOcclusionRadius);
    assert_eq!(
        world_lighting_info.ambientOcclusionSunlightStrength,
        default_world_lighting_info.ambientOcclusionSunlightStrength
    );
    assert_eq!(world_lighting_info.sunIntensity, 2.25);
    let resaved_world: Value = serde_json::from_str(&reloaded_world.toStoredWorldString()?)?;
    assert_eq!(resaved_world["worldSchemaVersion"], json!(STORED_WORLD_CURRENT_VERSION));
    Ok(())
}

// World lighting saved before its sky reflection strength setting existed has to load with the default one.
#[test]
fn world_lighting_saved_before_sky_reflection_strength_loads_with_the_default() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.addObjectWithId(11, "lighting".to_string(), ObjectBeingAdded::WorldLighting(WorldLightingInfo::default()), 1);
    let mut stored_world: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored_world["worldSchemaVersion"] = json!(76);
    stored_world["objectOfId"]["11"]["info"]["WorldLighting"]
        .as_object_mut()
        .expect("the saved world should hold the lighting")
        .remove("skyReflectionStrength");

    let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
    let object_id = *reloaded_world.getObjectIdsByName("lighting").first().expect("the lighting should still exist");
    let ObjectInfo::WorldLighting(world_lighting_info) = &reloaded_world.getObject(object_id)?.info else {
        panic!("the lighting should still be world lighting");
    };
    assert_eq!(world_lighting_info.skyReflectionStrength, WorldLightingInfo::default().skyReflectionStrength);
    Ok(())
}

// World lighting saved before its vibrance setting existed has to load with the default one.
#[test]
fn world_lighting_saved_before_vibrance_loads_with_the_default() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.addObjectWithId(11, "lighting".to_string(), ObjectBeingAdded::WorldLighting(WorldLightingInfo::default()), 1);
    let mut stored_world: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored_world["worldSchemaVersion"] = json!(79);
    stored_world["objectOfId"]["11"]["info"]["WorldLighting"]
        .as_object_mut()
        .expect("the saved world should hold the lighting")
        .remove("vibrance");

    let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
    let object_id = *reloaded_world.getObjectIdsByName("lighting").first().expect("the lighting should still exist");
    let ObjectInfo::WorldLighting(world_lighting_info) = &reloaded_world.getObject(object_id)?.info else {
        panic!("the lighting should still be world lighting");
    };
    assert_eq!(world_lighting_info.vibrance, WorldLightingInfo::default().vibrance);
    Ok(())
}
