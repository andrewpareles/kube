mod helpers;

use game_types::{
    object_types::{BrickInfo, ObjectBeingAdded, ObjectInfo, PhysicsInfo3D, PhysicsToggles3D, VisualSettings},
    physics_primitive_types::BrickPrimitiveSettings,
};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::make_world;
use serde_json::{Value, json};
use shared::game::world::{World, WorldControlMode};

// Objects saved before glow existed load without glow, and keep every visual setting they were saved with.
#[test]
fn objects_saved_before_glow_existed_load_without_glow() -> anyhow::Result<()> {
    let mut world = make_world()?;
    world.addObjectWithId(
        11,
        "brick".to_string(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings {
                    opacity: 0.5,
                    ..VisualSettings::TYPICAL_OPAQUE
                },
            },
            BrickPrimitiveSettings { size: Vec3::ONE },
            PhysicsInfo3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO),
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        1,
    );
    let mut stored_world: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored_world["worldSchemaVersion"] = json!(80);
    stored_world["objectOfId"]["11"]["info"]["Brick"]["visualSettings"]
        .as_object_mut()
        .expect("the saved brick should hold its visual settings")
        .remove("glow");

    let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
    let object_id = *reloaded_world.getObjectIdsByName("brick").first().expect("the brick should still exist");
    let ObjectInfo::Brick(brick_info) = &reloaded_world.getObject(object_id)?.info else {
        panic!("the brick should still be a brick");
    };
    assert_eq!(brick_info.visualSettings.glow, 0.0);
    assert_eq!(brick_info.visualSettings.opacity, 0.5);
    Ok(())
}
