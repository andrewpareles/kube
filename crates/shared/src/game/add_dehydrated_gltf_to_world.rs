use anyhow::Result;
use game_types::object_types::ObjectId;
use render_core::{dehydrated_gltf::DehydratedGltf, dehydrated_gltf_to_stored_world::dehydrated_gltf_to_stored_world};

use crate::game::world::{World, WorldControlMode};

// Build the portable hierarchy first, then let the destination World insert it and rewire its pointers.
pub fn add_dehydrated_gltf_to_world(world: &mut World, gltfName: &str, dehydratedGltf: DehydratedGltf, parent_id: ObjectId) -> Result<ObjectId> {
    let stored = dehydrated_gltf_to_stored_world(gltfName, dehydratedGltf)?;
    let imported = World::fromStoredWorldString(&serde_json::to_string(&stored)?, WorldControlMode::Editor, None)?;
    anyhow::ensure!(
        imported.objectOfId.len() == stored.objectOfId.len(),
        "Some imported model objects could not be loaded"
    );
    let mut template = imported.createTemplateFrom(imported.getRootObjectId())?;
    template.name = gltfName.to_string();
    world.addObjectFromTemplate(&template, parent_id)
}
