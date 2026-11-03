use game_types::object_types::{LiveObject, ObjectInfo, WorldLightingInfo, shared::Color3};
use game_types::stored_world_type::StoredWorld;

// this file is for lighting asset previews

// An asset preview that brings no lighting of its own is lit like a default world, but with white ambient light, so the
// object shows its own colors instead of a blue tint, in front of the default blue sky.
pub fn add_neutral_preview_lighting(stored: &mut StoredWorld) {
    let has_world_lighting = stored.objectOfId.values().any(|object| match object.info {
        ObjectInfo::WorldLighting(_) => true,
        _ => false,
    });
    if has_world_lighting {
        return;
    }
    let world_lighting_id = stored.objectOfId.keys().max().copied().unwrap_or(0) + 1;
    stored.objectOfId.insert(
        world_lighting_id,
        LiveObject {
            id: world_lighting_id,
            name: "Preview Lighting".to_string(),
            info: ObjectInfo::WorldLighting(WorldLightingInfo {
                ambientTopColor: Color3::WHITE,
                ambientBottomColor: Color3::WHITE,
                ..WorldLightingInfo::default()
            }),
        },
    );
    stored.childrenOfId.entry(stored.rootObjectId).or_default().push(world_lighting_id);
    stored.childrenOfId.insert(world_lighting_id, Vec::new());
}
