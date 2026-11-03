use anyhow::{Result, bail, ensure};
use render_core::dehydrated_gltf::DehydratedGltf;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::{
    api::{
        protocol_for_assets::{Asset, AssetSummary, AssetPublishData, AssetPublishRequest, AssetSearchRequest},
        protocol_for_chat::ClientEditorToolHandlers,
    },
    game::{
        add_dehydrated_gltf_to_world::add_dehydrated_gltf_to_world,
        data_fetcher::DataFetcherArc,
        world::{TemplateItem, World, WorldControlMode},
    },
};
use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    data_types::DataId,
    object_types::{
        BillboardMode, BrickInfo, ClockInfo, ClockState, LabelImageInfo, LiveObject, LocalGroupInfo, Location3D, Movement3D, ObjectBeingAdded, ObjectId,
        ObjectIdPointer, ObjectInfo, ObjectKind, PhysicsInfo3D, PhysicsToggles3D, SoundPlayerInfo, VisualSettings, WorldLightingInfo,
    },
    physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings},
    stored_world_type::{StoredMaterial, StoredWorld},
};
use glam::{Quat, Vec2, Vec3};

// -------------------------- Asset Types --------------------------

// Each asset is complete and reusable. Publishing stores its raw data first;
// mesh segments and texture dependencies do not become separate assets.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum AssetType {
    Mesh,
    Texture,
    Sound,
    Object,
    Material,
    Skybox,
    Sfx,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum AssetData {
    Mesh(DehydratedGltf),
    Texture(DataId),
    Sound(DataId),
    Object(StoredAssetObject),
    Material(StoredMaterial),
    // A panorama twice as wide as it is tall, which the sky shader wraps around the world.
    Skybox(DataId), 
    Sfx(DataId),
}

impl AssetData {
    pub fn asset_type(&self) -> AssetType {
        match self {
            Self::Mesh(..) => AssetType::Mesh,
            Self::Texture(_) => AssetType::Texture,
            Self::Sound(_) => AssetType::Sound,
            Self::Object(_) => AssetType::Object,
            Self::Material(_) => AssetType::Material,
            Self::Skybox(_) => AssetType::Skybox,
            Self::Sfx(_) => AssetType::Sfx,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Texture(id) | Self::Sound(id) | Self::Skybox(id) | Self::Sfx(id) => ensure!(*id > 0, "Data id must be positive"),
            Self::Mesh(gltf) => gltf.validate()?,
            Self::Object(object) => object.validate()?,
            Self::Material(StoredMaterial { worldSchemaVersion, material }) => {
                ensure!(
                    *worldSchemaVersion == STORED_WORLD_CURRENT_VERSION,
                    "Unsupported material version: {worldSchemaVersion}"
                );
                let texture_data_ids = [
                    material.albedoTextureDataId,
                    material.metallicTextureDataId,
                    material.roughnessTextureDataId,
                    material.normalTextureDataId,
                ];
                ensure!(texture_data_ids.iter().any(Option::is_some), "A material needs at least one texture");
                ensure!(texture_data_ids.iter().flatten().all(|id| *id > 0), "Data id must be positive");
            }
        }
        Ok(())
    }
}

// -------------------------- Asset Functions --------------------------

// The editor stores all data dependencies and returns the completed asset.
pub async fn publish_to_assets(asset_id: Option<i64>, asset_name: &str, data: AssetPublishData) -> Result<Asset> {
    ensure!(!asset_name.trim().is_empty(), "Asset name cannot be empty");
    data.validate()?;
    let asset_type = data.asset_type();
    let request = AssetPublishRequest {
        asset_id,
        name: asset_name.to_string(),
        data,
    };
    let asset = ClientEditorToolHandlers::get_curr()?.assets_handler.publish(request).await?;
    ensure!(
        asset.asset_type == asset_type && asset.data_json.asset_type() == asset_type,
        "Asset type does not match its upload"
    );
    asset.data_json.validate()?;
    Ok(asset)
}

pub async fn search_assets(request: &AssetSearchRequest) -> Result<Vec<AssetSummary>> {
    ClientEditorToolHandlers::get_curr()?.assets_handler.search(request).await
}

pub async fn download_asset(asset_id: i64) -> Result<Asset> {
    let asset = ClientEditorToolHandlers::get_curr()?.assets_handler.download(asset_id).await?;
    ensure!(
        asset.asset_type == asset.data_json.asset_type(),
        "Asset type does not match its data"
    );
    asset.data_json.validate()?;
    Ok(asset)
}

pub async fn record_asset_import(asset_id: i64, version_num: i64) -> Result<()> {
    ClientEditorToolHandlers::get_curr()?.assets_handler.record_import(asset_id, version_num).await
}

// Downloading inserts a fresh instance. Texture entries become pictures, and sounds get a paused clock.
// TODO: AI-PICKED-VALUE: New sounds use volume and speed 1, with looping off, until the game configures playback.
pub fn add_asset_to_world(world: &mut World, name: String, data: AssetData) -> Result<ObjectId> {
    data.validate()?;
    let root_id = world.getRootObjectId();
    let object_id = match data {
        AssetData::Mesh(gltf) => add_dehydrated_gltf_to_world(world, &name, gltf, root_id)?,
        AssetData::Object(object) => {
            let template = object.to_object_template(&name)?;
            world.addObjectFromTemplate(&template, root_id)?
        }
        AssetData::Texture(id) => add_picture_to_world(world, name, id, Vec2::splat(PICTURE_LONGEST_SIDE)),
        AssetData::Material(StoredMaterial { material, .. }) => world.addObject(name, ObjectBeingAdded::Material(material), root_id),
        AssetData::Sound(id) | AssetData::Sfx(id) => {
            let sound_id = world.addObject(
                name,
                ObjectBeingAdded::SoundPlayer(SoundPlayerInfo {
                    soundDataId: id,
                    volume: 1.0,
                    clockId: None,
                }),
                root_id,
            );
            let clock_id = world.addObject(
                "Clock".to_string(),
                ObjectBeingAdded::Clock(ClockInfo {
                    clockState: ClockState::Paused { animationTimeSecs: 0.0 },
                    shouldLoop: false,
                    speed: 1.0,
                }),
                sound_id,
            );
            world.setSoundPlayerInfo(
                sound_id,
                SoundPlayerInfo {
                    soundDataId: id,
                    volume: 1.0,
                    clockId: Some(ObjectIdPointer(clock_id)),
                },
            )?;
            sound_id
        }
        // A skybox becomes the sky of the world's lighting, which is made first when the world has none.
        AssetData::Skybox(id) => {
            let world_lighting_object_id = world
                ._kindOfObjectId
                .get_keys(&ObjectKind::WorldLighting)
                .and_then(|object_ids| object_ids.iter().copied().min());
            let world_lighting_info = match world_lighting_object_id {
                Some(object_id) => match world.getObject(object_id)?.info {
                    ObjectInfo::WorldLighting(world_lighting_info) => world_lighting_info,
                    _ => bail!("Object {object_id} is not world lighting"),
                },
                None => WorldLightingInfo::default(),
            };
            // A world that had its skybox hidden should still show the one it just picked.
            let world_lighting_info = WorldLightingInfo {
                skyboxTextureDataId: Some(id),
                skyboxTextureOpacity: 1.0,
                ..world_lighting_info
            };
            match world_lighting_object_id {
                Some(object_id) => {
                    world.setWorldLightingInfo(object_id, world_lighting_info)?;
                    object_id
                }
                None => world.addObject("worldLighting".to_string(), ObjectBeingAdded::WorldLighting(world_lighting_info), root_id),
            }
        }
    };
    Ok(object_id)
}

// TODO: AI-PICKED-VALUE: A downloaded picture's longest side is 4 units.
const PICTURE_LONGEST_SIDE: f32 = 4.0;
// TODO: AI-PICKED-VALUE: The board a downloaded picture hangs on is this thick.
const PICTURE_BOARD_DEPTH: f32 = 0.1;

// A downloaded picture keeps its texture's shape, with its longest side PICTURE_LONGEST_SIDE. A texture that can't be
// downloaded comes in square.
pub async fn picture_size_of_texture(data_fetcher: &DataFetcherArc, texture_data_id: DataId) -> Vec2 {
    let (_, texture_data_arc_of_id) = data_fetcher.waitForMeshAndTextureData(HashSet::new(), HashSet::from([texture_data_id])).await;
    let Some(texture_data) = texture_data_arc_of_id.get(&texture_data_id) else {
        return Vec2::splat(PICTURE_LONGEST_SIDE);
    };
    let texture_size = Vec2::new(texture_data.width.max(1) as f32, texture_data.height.max(1) as f32);
    texture_size * (PICTURE_LONGEST_SIDE / texture_size.max_element())
}

// A downloaded texture comes in as a picture on the front of a board its own size, standing on the ground, so it can be
// moved around and hung on a wall.
pub fn add_picture_to_world(world: &mut World, name: String, texture_data_id: DataId, size: Vec2) -> ObjectId {
    let root_object_id = world.getRootObjectId();
    let board_id = world.addObject(
        name.clone(),
        ObjectBeingAdded::Brick(
            BrickInfo {
                material: None,
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
            BrickPrimitiveSettings {
                size: size.extend(PICTURE_BOARD_DEPTH),
            },
            PhysicsInfo3D {
                location: Location3D {
                    position: Vec3::new(0.0, size.y / 2.0, 0.0),
                    quaternion: Quat::IDENTITY,
                },
                movement: Movement3D {
                    velocity: Vec3::ZERO,
                    angularVelocity: Vec3::ZERO,
                },
            },
            PhysicsToggles3D {
                isWalkthrough: false,
                isPushable: false,
                density: 1.0,
            },
        ),
        root_object_id,
    );
    world.addObject(
        name,
        ObjectBeingAdded::LabelImage(LabelImageInfo {
            textureDataId: texture_data_id,
            width: size.x,
            height: size.y,
            offsetFromObject: Vec3::new(0.0, 0.0, PICTURE_BOARD_DEPTH / 2.0),
            rotationFromObject: Quat::IDENTITY,
            physicalObjectId: Some(ObjectIdPointer(board_id)),
            billboardMode: BillboardMode::None,
            isConstantSize: false,
            visualSettings: VisualSettings::TYPICAL_OPAQUE,
        }),
        board_id,
    );
    board_id
}

// -------------------------- StoredAssetObject --------------------------

// we reuse the StoredWorld schema for StoredAssetObject
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct StoredAssetObject(StoredWorld);

// Object assets use the stored-world schema, restricted to one subtree under a LocalGroup.
// Validate before inserting anything so a broken hierarchy or reference cannot partially change a game.
impl StoredAssetObject {
    pub fn validate(&self) -> Result<()> {
        let Self(stored) = self;
        ensure!(
            stored.worldSchemaVersion == STORED_WORLD_CURRENT_VERSION,
            "Unsupported asset object world schema version"
        );
        let root = stored
            .objectOfId
            .get(&stored.rootObjectId)
            .ok_or_else(|| anyhow::anyhow!("Asset object root is missing"))?;
        ensure!(
            match root.info {
                ObjectInfo::LocalGroup(_) => true,
                _ => false,
            },
            "Asset object root must be a LocalGroup"
        );
        let mut visited = HashSet::new();
        let mut pending = vec![stored.rootObjectId];
        while let Some(id) = pending.pop() {
            ensure!(visited.insert(id), "Asset object contains a cycle or repeated child {id}");
            let object = stored.objectOfId.get(&id).ok_or_else(|| anyhow::anyhow!("Asset child {id} is missing"))?;
            ensure!(object.id == id, "Asset object id does not match its map key");
            if let Some(children) = stored.childrenOfId.get(&id) {
                pending.extend(children);
            }
        }
        ensure!(visited.len() == stored.objectOfId.len(), "Asset object contains objects outside its root");
        let idOfId: HashMap<_, _> = visited.iter().map(|id| (*id, *id)).collect();
        for (id, object) in &stored.objectOfId {
            let mut rewired_info = object.info.clone();
            World::rewireObjectInfoPointers(&mut rewired_info, &object.info, &idOfId, false)?;
            ensure!(
                serde_json::to_value(&rewired_info)? == serde_json::to_value(&object.info)?,
                "Object {id} references something outside the published hierarchy"
            );
            let is_primitive = match &object.info {
                ObjectInfo::Ball(_) => {
                    ensure!(
                        match stored.primitiveSettingsOfId.get(id) {
                            Some(PrimitiveSettings::Ball(_)) => true,
                            _ => false,
                        },
                        "Missing ball shape for {id}"
                    );
                    true
                }
                ObjectInfo::Brick(_) => {
                    ensure!(
                        match stored.primitiveSettingsOfId.get(id) {
                            Some(PrimitiveSettings::Brick(_)) => true,
                            _ => false,
                        },
                        "Missing brick shape for {id}"
                    );
                    true
                }
                ObjectInfo::Cylinder(_) => {
                    ensure!(
                        match stored.primitiveSettingsOfId.get(id) {
                            Some(PrimitiveSettings::Cylinder(_)) => true,
                            _ => false,
                        },
                        "Missing cylinder shape for {id}"
                    );
                    true
                }
                ObjectInfo::RigidGroup(_) => {
                    ensure!(stored.physicsInfo3DOfId.contains_key(id), "Missing physics for {id}");
                    false
                }
                ObjectInfo::Script(_) => {
                    ensure!(stored.scriptRawCodeOfId.contains_key(id), "Missing script code for {id}");
                    false
                }
                _ => false,
            };
            if is_primitive {
                ensure!(
                    stored.physicsInfo3DOfId.contains_key(id) && stored.physicsToggles3DOfId.contains_key(id),
                    "Missing physics for {id}"
                );
            }
        }
        ensure!(
            stored
                .childrenOfId
                .keys()
                .chain(stored.scriptRawCodeOfId.keys())
                .chain(stored.physicsInfo3DOfId.keys())
                .chain(stored.physicsToggles3DOfId.keys())
                .chain(stored.primitiveSettingsOfId.keys())
                .all(|id| visited.contains(id)),
            "Asset object contains state outside its root"
        );
        Ok(())
    }

    // Capture only this subtree, including scripts and physics that live outside LiveObject.
    pub fn from_world_object(world: &World, object_id: ObjectId) -> Result<Self> {
        let mut stored = StoredWorld {
            worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
            fallback_camera_location: world.fallback_camera_location,
            rootObjectId: object_id,
            objectOfId: HashMap::new(),
            childrenOfId: HashMap::new(),
            scriptRawCodeOfId: HashMap::new(),
            physicsInfo3DOfId: HashMap::new(),
            physicsToggles3DOfId: HashMap::new(),
            primitiveSettingsOfId: HashMap::new(),
        };
        let mut pending = vec![object_id];
        while let Some(id) = pending.pop() {
            stored.objectOfId.insert(id, world.getObject(id)?.clone());
            let children: Vec<_> = world.getDirectChildrenIds(id)?.iter().copied().collect();
            pending.extend(&children);
            stored.childrenOfId.insert(id, children);
            if let Some(code) = world.scriptRawCodeOfId.get(&id) {
                stored.scriptRawCodeOfId.insert(id, code.clone());
            }
            if let Some(info) = world.getObjectPhysicsInfo3D(id) {
                stored.physicsInfo3DOfId.insert(id, info);
            }
            if let Some(toggles) = world.getObjectPhysicsToggles3D(id) {
                stored.physicsToggles3DOfId.insert(id, toggles);
            }
            if let Some(shape) = world.physics.primitiveSettingsOfId.get(&id) {
                stored.primitiveSettingsOfId.insert(id, shape.clone());
            }
        }
        let root_id = world
            .objectOfId
            .keys()
            .max()
            .copied()
            .unwrap()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Object id overflow"))?;
        stored.objectOfId.insert(
            root_id,
            LiveObject {
                id: root_id,
                name: world.getObject(object_id)?.name.clone(),
                info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
            },
        );
        stored.childrenOfId.insert(root_id, vec![object_id]);
        stored.rootObjectId = root_id;
        let stored = Self(stored);
        stored.validate()?;
        Ok(stored)
    }

    // Reuse world loading to build a template. The destination World handles insertion and pointer rewiring.
    pub fn to_object_template(self, name: &str) -> Result<TemplateItem> {
        self.validate()?;
        let Self(object) = self;
        let imported = World::fromStoredWorldString(&serde_json::to_string(&object)?, WorldControlMode::Editor, None)?;
        ensure!(imported.objectOfId.len() == object.objectOfId.len(), "Some asset objects could not be loaded");
        let mut template = imported.createTemplateFrom(imported.getRootObjectId())?;
        template.name = name.to_string();
        Ok(template)
    }
}
