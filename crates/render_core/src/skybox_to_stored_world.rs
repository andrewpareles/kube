use std::collections::HashMap;

use glam::Vec3;

use game_types::object_types::{CameraLocation, LiveObject, LocalGroupInfo, ObjectInfo, Orientation3D, WorldLightingInfo};
use game_types::{consts::STORED_WORLD_CURRENT_VERSION, data_types::DataId, stored_world_type::StoredWorld};

// this file is for previewing a skybox asset

// A skybox has no shape of its own, so its asset preview is an empty world whose sky is the skybox, fully opaque.
pub fn skybox_to_stored_world(skybox_name: &str, texture_data_id: DataId) -> StoredWorld {
    let root_id = 1;
    let world_lighting_id = 2;
    StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: CameraLocation {
            position: Vec3::ZERO,
            orientation: Orientation3D::Z(),
        },
        rootObjectId: root_id,
        objectOfId: HashMap::from([
            (
                root_id,
                LiveObject {
                    id: root_id,
                    name: skybox_name.to_string(),
                    info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
                },
            ),
            (
                world_lighting_id,
                LiveObject {
                    id: world_lighting_id,
                    name: skybox_name.to_string(),
                    info: ObjectInfo::WorldLighting(WorldLightingInfo {
                        skyboxTextureDataId: Some(texture_data_id),
                        skyboxTextureOpacity: 1.0,
                        ..WorldLightingInfo::default()
                    }),
                },
            ),
        ]),
        childrenOfId: HashMap::from([(root_id, vec![world_lighting_id]), (world_lighting_id, Vec::new())]),
        scriptRawCodeOfId: HashMap::new(),
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    }
}
