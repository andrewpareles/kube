use std::collections::HashMap;

use glam::{Quat, Vec3};

use game_types::object_types::shared::Color3;
use game_types::object_types::{
    BallInfo, BallMaterialSettings, CameraLocation, LiveObject, LocalGroupInfo, Location3D, MaterialInfo, Movement3D, ObjectIdPointer, ObjectInfo,
    Orientation3D, PhysicsInfo3D, PhysicsToggles3D, VisualSettings,
};
use game_types::physics_primitive_types::{BallPrimitiveSettings, PrimitiveSettings};
use game_types::{consts::STORED_WORLD_CURRENT_VERSION, stored_world_type::StoredWorld};

// this file is for previewing a material asset

// TODO: AI-PICKED-VALUE: A ball one unit across with one texture tile per unit shows a few repeats of the pattern.
const PREVIEW_BALL_RADIUS: f32 = 0.5;
const PREVIEW_TILE_SIZE: f32 = 1.0;

// A material has no shape of its own, so its asset preview is a white ball wearing it.
pub fn material_to_stored_world(material_name: &str, material: MaterialInfo) -> StoredWorld {
    let root_id = 1;
    let material_id = 2;
    let ball_id = 3;
    let ball = BallInfo {
        material: Some(BallMaterialSettings {
            materialInfoId: ObjectIdPointer(material_id),
            tileSizeU: PREVIEW_TILE_SIZE,
            tileSizeV: PREVIEW_TILE_SIZE,
            tileOffsetU: 0.0,
            tileOffsetV: 0.0,
        }),
        visualSettings: VisualSettings {
            color: Color3 { r: 1.0, g: 1.0, b: 1.0 },
            ..VisualSettings::TYPICAL_OPAQUE
        },
    };
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
                    name: material_name.to_string(),
                    info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
                },
            ),
            (
                material_id,
                LiveObject {
                    id: material_id,
                    name: material_name.to_string(),
                    info: ObjectInfo::Material(material),
                },
            ),
            (
                ball_id,
                LiveObject {
                    id: ball_id,
                    name: "Preview Ball".to_string(),
                    info: ObjectInfo::Ball(ball),
                },
            ),
        ]),
        childrenOfId: HashMap::from([(root_id, vec![material_id, ball_id]), (material_id, Vec::new()), (ball_id, Vec::new())]),
        scriptRawCodeOfId: HashMap::new(),
        physicsInfo3DOfId: HashMap::from([(
            ball_id,
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
        )]),
        physicsToggles3DOfId: HashMap::from([(
            ball_id,
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        )]),
        primitiveSettingsOfId: HashMap::from([(ball_id, PrimitiveSettings::Ball(BallPrimitiveSettings { radius: PREVIEW_BALL_RADIUS }))]),
    }
}
