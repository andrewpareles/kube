use anyhow::{Context, Result};
use game_types::{
    consts::{DEHYDRATED_GLTF_CURRENT_VERSION, STORED_WORLD_CURRENT_VERSION},
    migrateDehydratedGltf,
    migrateStoredWorld::{self, StoredWorldKind},
    object_types::{ObjectId, ObjectIdPointer, ObjectInfo, VisualSettings, shared::Color3},
    physics_primitive_types::PrimitiveSettings,
    stored_world_type::StoredWorld,
};
use glam::Vec3;
use render_core::{dehydrated_gltf::DehydratedGltf, dehydrated_gltf_to_stored_world::add_dehydrated_gltf_to_stored_world};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashSet, VecDeque};

// Avatars are dressed the way assets/scripts/character_appearance.ts dresses players in games. Keep the two the same.
const PREFERRED_COLOR_PARTS: [&str; 2] = ["Torso", "Cap"];

// What the site sends to draw a player's avatar: the DefaultCharacter asset's data, the hat they have equipped, the color they wear
// as #rrggbb, and whether to stand in the rest pose instead of the first frame of its animation, for an avatar that never moves.
#[derive(Deserialize)]
pub struct AvatarData {
    character: Value,
    hat: Option<AvatarHat>,
    color: String,
    #[serde(rename = "isInRestPose")]
    pub is_in_rest_pose: bool,
}

#[derive(Deserialize)]
struct AvatarHat {
    name: String,
    #[serde(rename = "dataJson")]
    data_json: Value,
}

impl AvatarData {
    pub fn into_stored_world(self) -> Result<StoredWorld> {
        #[derive(Deserialize)]
        enum CharacterData {
            Object(Value),
        }
        #[derive(Deserialize)]
        enum HatData {
            Mesh(Value),
        }
        let CharacterData::Object(mut character_json) = serde_json::from_value(self.character)?;
        migrateStoredWorld::updateToVersion(&mut character_json, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::AssetObject)?;
        let hat = match self.hat {
            Some(AvatarHat { name, data_json }) => {
                let HatData::Mesh(mut mesh_json) = serde_json::from_value(data_json)?;
                migrateDehydratedGltf::updateToVersion(&mut mesh_json, DEHYDRATED_GLTF_CURRENT_VERSION)?;
                Some((name, serde_json::from_value(mesh_json)?))
            }
            None => None,
        };
        let color = parse_hex_color(&self.color)?;
        dress_avatar(serde_json::from_value(character_json)?, hat, color)
    }
}

// The color paints the Body's parts named above, and the hat goes on its HatSpot in place of its Cap, scaled to the width of
// its Head. The nametag over the character's head is left off.
pub fn dress_avatar(mut stored: StoredWorld, hat: Option<(String, DehydratedGltf)>, color: Color3) -> Result<StoredWorld> {
    if let Some(nametag) = child_named(&stored, character_id(&stored)?, "Nametag") {
        remove_object(&mut stored, nametag);
    }
    paint_avatar(&mut stored, color)?;
    let body = body_id(&stored)?;
    let (Some((hat_name, hat_gltf)), Some(hat_spot)) = (hat, child_named(&stored, body, "HatSpot")) else {
        return Ok(stored);
    };
    let head_width = match child_named(&stored, body, "Head").and_then(|head| stored.primitiveSettingsOfId.get(&head)) {
        Some(PrimitiveSettings::Brick(brick)) => brick.size.x,
        Some(PrimitiveSettings::Ball(ball)) => ball.radius * 2.0,
        Some(PrimitiveSettings::Cylinder(cylinder)) => cylinder.radius * 2.0,
        None => 1.0,
    };
    let hat = add_dehydrated_gltf_to_stored_world(&mut stored, &hat_name, hat_gltf, hat_spot)?;
    // The hat's Meshes follow the HatSpot so they move with the head. Its own physics_collider is left out.
    for part in stored.childrenOfId.get(&hat).cloned().unwrap_or_default() {
        let Some(part_object) = stored.objectOfId.get_mut(&part) else {
            continue;
        };
        if let ObjectInfo::Mesh(mesh) = &mut part_object.info {
            mesh.physicalObjectId = Some(ObjectIdPointer(hat_spot));
            mesh.scale *= head_width;
        }
        if part_object.name == "physics_collider" {
            remove_object(&mut stored, part);
        }
    }
    if let Some(cap) = child_named(&stored, body, "Cap") {
        remove_object(&mut stored, cap);
    }
    Ok(stored)
}

pub fn paint_avatar(stored: &mut StoredWorld, color: Color3) -> Result<()> {
    let body = body_id(stored)?;
    for part_name in PREFERRED_COLOR_PARTS {
        let Some(part) = child_named(stored, body, part_name) else {
            continue;
        };
        if let Some(visual_settings) = stored.objectOfId.get_mut(&part).and_then(|part| visual_settings_mut(&mut part.info)) {
            visual_settings.color = color;
        }
    }
    Ok(())
}

// The parts an avatar's preview frames. A character without a Head is framed whole instead.
pub struct AvatarFraming {
    pub head_ids: HashSet<ObjectId>,
    pub torso_ids: HashSet<ObjectId>,
}

impl AvatarFraming {
    pub fn of(stored: &StoredWorld) -> Option<Self> {
        let body = body_id(stored).ok()?;
        let head = child_named(stored, body, "Head")?;
        Some(Self {
            head_ids: HashSet::from([head]),
            torso_ids: child_named(stored, body, "Torso").into_iter().collect(),
        })
    }
}

// Every avatar is framed as though it wore the tallest hat in the catalog, so the camera stays put whatever hat it wears. On 2026-10-06
// that was the Collapsed Soufflé Toque, which reaches this many Head heights above the top of the Head. A taller hat added later runs
// past the top of the frame.
const TALLEST_HAT_HEIGHT_IN_HEADS: f32 = 1.875;

fn tallest_hat_top((head_min, head_max): (Vec3, Vec3)) -> f32 {
    head_max.y + TALLEST_HAT_HEIGHT_IN_HEADS * (head_max.y - head_min.y)
}

// An avatar's preview shows it from the bottom of its Torso up over the tallest hat, so any hat and the preferred color both show. Only
// that height is framed, so the arms and a wide brim can run past the edge of the circle.
pub fn face_framed_bounds(head: (Vec3, Vec3), torso: Option<(Vec3, Vec3)>) -> (Vec3, Vec3) {
    let (head_min, head_max) = head;
    let face = (head_min + head_max) * 0.5;
    // TODO: AI-PICKED-VALUE: The frame reaches a tenth of the Torso below its bottom, so the tops of the legs just show.
    let bottom = torso.map_or(head_min.y, |(torso_min, torso_max)| torso_min.y - (torso_max.y - torso_min.y) * 0.1);
    (Vec3::new(face.x, bottom, face.z), Vec3::new(face.x, tallest_hat_top(head), face.z))
}

// Profiles store a color as #rrggbb, the way a color picker shows it.
pub fn parse_hex_color(hex: &str) -> Result<Color3> {
    anyhow::ensure!(hex.len() == 7 && hex.starts_with('#'), "Bad color {hex}");
    let channel = |start: usize| -> Result<f32> {
        let digits = hex.get(start..start + 2).with_context(|| format!("Bad color {hex}"))?;
        Ok(f32::from(u8::from_str_radix(digits, 16)?) / 255.0)
    };
    Ok(Color3 {
        r: channel(1)?,
        g: channel(3)?,
        b: channel(5)?,
    })
}

// The DefaultCharacter asset keeps its Character next to its scripts and animations.
fn character_id(stored: &StoredWorld) -> Result<ObjectId> {
    let mut queue = VecDeque::from([stored.rootObjectId]);
    while let Some(id) = queue.pop_front() {
        if stored.objectOfId.get(&id).is_some_and(|object| object.name == "Character") {
            return Ok(id);
        }
        queue.extend(stored.childrenOfId.get(&id).into_iter().flatten());
    }
    anyhow::bail!("The default character has no Character.")
}

fn body_id(stored: &StoredWorld) -> Result<ObjectId> {
    child_named(stored, character_id(stored)?, "Body").context("The default character's Character has no Body.")
}

fn child_named(stored: &StoredWorld, parent: ObjectId, name: &str) -> Option<ObjectId> {
    stored
        .childrenOfId
        .get(&parent)?
        .iter()
        .copied()
        .find(|child| stored.objectOfId.get(child).is_some_and(|object| object.name == name))
}

fn visual_settings_mut(info: &mut ObjectInfo) -> Option<&mut VisualSettings> {
    match info {
        ObjectInfo::Brick(brick) => Some(&mut brick.visualSettings),
        ObjectInfo::Ball(ball) => Some(&mut ball.visualSettings),
        ObjectInfo::Cylinder(cylinder) => Some(&mut cylinder.visualSettings),
        ObjectInfo::Mesh(mesh) => Some(&mut mesh.visualSettings),
        ObjectInfo::Label(label) => Some(&mut label.visualSettings),
        _ => None,
    }
}

// Removes the object and everything under it.
fn remove_object(stored: &mut StoredWorld, id: ObjectId) {
    for children in stored.childrenOfId.values_mut() {
        children.retain(|child| *child != id);
    }
    let mut ids = vec![id];
    while let Some(id) = ids.pop() {
        ids.extend(stored.childrenOfId.remove(&id).unwrap_or_default());
        stored.objectOfId.remove(&id);
        stored.scriptRawCodeOfId.remove(&id);
        stored.physicsInfo3DOfId.remove(&id);
        stored.physicsToggles3DOfId.remove(&id);
        stored.primitiveSettingsOfId.remove(&id);
    }
}
