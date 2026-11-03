mod helpers;

use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    object_types::{BillboardMode, BrickInfo, LabelImageInfo, MaterialInfo, ObjectIdPointer, ObjectInfo, ObjectKind, WorldLightingInfo, material::AlphaMode},
    stored_world_type::StoredMaterial,
};
use glam::{Vec2, Vec3};
use helpers::world_test_helpers::make_world;
use serde_json::{Value, json};
use shared::{
    api::protocol_for_assets::Asset,
    game::assets::{AssetData, AssetType, add_asset_to_world, add_picture_to_world},
};

fn material(albedo: Option<i64>, metallic: Option<i64>, roughness: Option<i64>, normal: Option<i64>) -> MaterialInfo {
    MaterialInfo {
        albedoTextureDataId: albedo,
        metallicTextureDataId: metallic,
        roughnessTextureDataId: roughness,
        normalTextureDataId: normal,
        alphaMode: AlphaMode::Blend,
        isDoubleSided: true,
    }
}

fn asset_material(material: MaterialInfo) -> AssetData {
    AssetData::Material(StoredMaterial {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        material,
    })
}

// A material asset stores every texture map by id, and adding it to a game creates that exact Material.
#[test]
fn asset_material_keeps_every_map_when_added_to_a_game() -> anyhow::Result<()> {
    let data = asset_material(material(Some(11), Some(12), Some(13), Some(14)));
    let restored: AssetData = serde_json::from_str(&serde_json::to_string(&data)?)?;
    restored.validate()?;
    assert_eq!(restored.asset_type(), AssetType::Material);

    let mut world = make_world()?;
    let object_id = add_asset_to_world(&mut world, "Grass".to_string(), restored)?;
    let object = world.getObject(object_id)?;
    assert_eq!(object.name, "Grass");
    let ObjectInfo::Material(added) = &object.info else {
        panic!("Expected a Material object, got {:?}", object.info);
    };
    assert_eq!(
        [
            added.albedoTextureDataId,
            added.metallicTextureDataId,
            added.roughnessTextureDataId,
            added.normalTextureDataId
        ],
        [Some(11), Some(12), Some(13), Some(14)]
    );
    let is_blend = match added.alphaMode {
        AlphaMode::Blend => true,
        AlphaMode::Opaque | AlphaMode::Mask { .. } => false,
    };
    assert!(is_blend && added.isDoubleSided);
    Ok(())
}

#[test]
fn asset_material_needs_a_texture_and_positive_ids() {
    let error = asset_material(material(None, None, None, None)).validate().unwrap_err();
    assert!(error.to_string().contains("at least one texture"), "{error}");
    for invalid_id in [0, -1] {
        let error = asset_material(material(Some(11), None, Some(invalid_id), None)).validate().unwrap_err();
        assert!(error.to_string().contains("must be positive"), "{error}");
    }
    asset_material(material(None, None, None, Some(14))).validate().unwrap();
}

// A material asset keeps the world schema version it was published at, and is brought up to date as it is read. The bare
// material every row held before materials had a version is refused rather than read as some version it may not be, and so
// is a material a newer server wrote.
#[test]
fn a_stored_material_has_to_say_which_version_it_was_published_at() -> anyhow::Result<()> {
    let material_json = serde_json::to_value(material(Some(11), None, None, None))?;
    let row = |data_json: Value| json!({ "asset_id": 1, "version_num": 1, "name": "Grass", "created_at": "", "asset_type": "Material", "data_json": { "Material": data_json } });
    let stored = json!({ "worldSchemaVersion": STORED_WORLD_CURRENT_VERSION, "material": material_json });
    Asset::fromJson(row(stored))?.data_json.validate()?;
    assert!(Asset::fromJson(row(material_json)).is_err());

    let newer = AssetData::Material(StoredMaterial {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION + 1,
        material: material(Some(11), None, None, None),
    });
    let error = newer.validate().unwrap_err();
    assert!(error.to_string().contains("Unsupported material version"), "{error}");
    Ok(())
}

// A downloaded texture comes in as a picture stuck flat on the front of an opaque board its own size, standing on the ground.
#[test]
fn a_downloaded_texture_comes_in_as_a_picture_on_a_thin_board() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let board_id = add_picture_to_world(&mut world, "Poster".to_string(), 42, Vec2::new(4.0, 2.0));
    let board = world.getObject(board_id)?;
    assert_eq!(board.name, "Poster");
    let ObjectInfo::Brick(BrickInfo {
        material: None,
        visualSettings,
    }) = &board.info
    else {
        panic!("Expected a plain brick, got {:?}", board.info);
    };
    assert_eq!(visualSettings.opacity, 1.0);
    let board_size = world.physics.getBrickPrimitiveSettings(board_id).unwrap().size;
    assert_eq!([board_size.x, board_size.y], [4.0, 2.0], "the board should match the picture");
    assert!(board_size.z < 0.5, "the board should be thin: {board_size}");
    let position = world.getObjectPhysicsInfo3D(board_id).unwrap().location.position;
    assert!((position.y - 1.0).abs() < 1e-5, "the board should stand on the ground: {position}");

    let [picture_id] = world._childrenOfId[&board_id].iter().copied().collect::<Vec<_>>()[..] else {
        panic!("The board should hold just its picture");
    };
    let ObjectInfo::LabelImage(LabelImageInfo {
        textureDataId,
        width,
        height,
        offsetFromObject,
        physicalObjectId,
        billboardMode,
        ..
    }) = &world.getObject(picture_id)?.info
    else {
        panic!("The board's child should be a LabelImage");
    };
    assert_eq!(*textureDataId, 42);
    assert_eq!([*width, *height], [4.0, 2.0]);
    assert_eq!(*physicalObjectId, Some(ObjectIdPointer(board_id)));
    assert_eq!(*billboardMode, BillboardMode::None);
    assert_eq!(
        *offsetFromObject,
        Vec3::new(0.0, 0.0, board_size.z / 2.0),
        "the picture should sit on the board's front face"
    );
    Ok(())
}

// A skybox becomes the world's sky: it fills in the world lighting that is there, keeping the rest of it, or adds one.
#[test]
fn a_skybox_asset_becomes_the_sky_of_the_world_lighting() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let world_lighting_id = add_asset_to_world(&mut world, "Dusk".to_string(), AssetData::Skybox(42))?;
    let ObjectInfo::WorldLighting(added) = world.getObject(world_lighting_id)?.info else {
        panic!("Expected world lighting");
    };
    assert_eq!(added.skyboxTextureDataId, Some(42));
    assert_eq!(world.parentOfId.get(&world_lighting_id).copied(), Some(world.getRootObjectId()));

    world.setWorldLightingInfo(
        world_lighting_id,
        WorldLightingInfo {
            sunIntensity: 1.5,
            skyboxTextureOpacity: 0.0,
            ..added
        },
    )?;
    assert_eq!(add_asset_to_world(&mut world, "Noon".to_string(), AssetData::Skybox(43))?, world_lighting_id);
    let ObjectInfo::WorldLighting(changed) = world.getObject(world_lighting_id)?.info else {
        panic!("Expected world lighting");
    };
    assert_eq!(
        (changed.skyboxTextureDataId, changed.skyboxTextureOpacity, changed.sunIntensity),
        (Some(43), 1.0, 1.5)
    );
    assert_eq!(
        world._kindOfObjectId.get_keys(&ObjectKind::WorldLighting).map(|object_ids| object_ids.len()),
        Some(1)
    );
    Ok(())
}
