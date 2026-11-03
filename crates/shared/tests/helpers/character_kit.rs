#![allow(dead_code)]

use shared::game::assets::StoredAssetObject;
use shared::game::world::{World, WorldControlMode};

// The default character that games download from assets: the player from game 142, Freeze Tag: Powerup Arena, without its freeze
// tag parts, kept in fixtures/default_character.kube. It is published by crates/editor_server/tests/publish_default_character.rs.

const DEFAULT_CHARACTER_WORLD: &str = include_str!("../fixtures/default_character.kube");
const CHARACTER_SPAWN_SCRIPT: &str = include_str!("../../../../assets/scripts/character_spawn.ts");
const CHARACTER_ANIMATION_SCRIPT: &str = include_str!("../../../../assets/scripts/character_animation.ts");
const CHARACTER_APPEARANCE_SCRIPT: &str = include_str!("../../../../assets/scripts/character_appearance.ts");
const ZOOM_FADE_SCRIPT: &str = include_str!("../../../../assets/scripts/zoom_fade.ts");

pub fn default_character_asset_object() -> anyhow::Result<StoredAssetObject> {
    let mut world = World::fromStoredWorldString(DEFAULT_CHARACTER_WORLD, WorldControlMode::Editor, None)?;
    for (script_name, script_raw_code) in [
        ("spawn", CHARACTER_SPAWN_SCRIPT),
        ("animation", CHARACTER_ANIMATION_SCRIPT),
        ("appearance", CHARACTER_APPEARANCE_SCRIPT),
        ("zoom_fade", ZOOM_FADE_SCRIPT),
    ] {
        let [script_object_id] = world.getObjectIdsByName(script_name)[..] else {
            anyhow::bail!("the default character should have one {script_name} script");
        };
        world.setScriptRawCode(script_object_id, script_raw_code.to_string());
    }
    let [folder_id] = world.getObjectIdsByName("DefaultCharacter")[..] else {
        anyhow::bail!("the default character should be one DefaultCharacter folder");
    };
    // An asset object is wrapped in a folder of its own, and this one is a folder already, so it is published without the wrapper.
    let mut stored_object = serde_json::to_value(StoredAssetObject::from_world_object(&world, folder_id)?)?;
    let wrapper_id = stored_object["rootObjectId"].to_string();
    stored_object["rootObjectId"] = folder_id.into();
    stored_object["objectOfId"].as_object_mut().unwrap().remove(&wrapper_id);
    stored_object["childrenOfId"].as_object_mut().unwrap().remove(&wrapper_id);
    let stored_object: StoredAssetObject = serde_json::from_value(stored_object)?;
    stored_object.validate()?;
    Ok(stored_object)
}
