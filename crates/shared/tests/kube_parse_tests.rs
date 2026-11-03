use anyhow::Context;
use game_types::consts::STORED_WORLD_CURRENT_VERSION;
use game_types::object_types::{LocalGroupInfo, ObjectBeingAdded, ObjectInfo, shared::Color3};
use serde_json::{Value, json};
use shared::game::migrateStoredWorld::{StoredWorldKind, updateToVersion};
use shared::game::world::{World, WorldControlMode};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn should_skip_dir(path: &Path) -> bool {
    match path.file_name().and_then(|name| name.to_str()) {
        // .claude holds worktrees of other branches, whose .kube files are not this branch's content.
        Some(".git") | Some(".claude") | Some("target") | Some("node_modules") => true,
        Some(_) => false,
        None => false,
    }
}

fn collect_kube_paths(dir: &Path, kube_paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("Could not read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("Could not read entry in {}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            collect_kube_paths(&path, kube_paths)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("kube") {
            kube_paths.push(path);
        }
    }

    Ok(())
}

fn all_repo_kube_paths() -> anyhow::Result<Vec<PathBuf>> {
    let mut kube_paths = Vec::new();
    collect_kube_paths(&repo_root(), &mut kube_paths)?;
    kube_paths.sort();
    Ok(kube_paths)
}

fn relative_repo_path(path: &Path) -> anyhow::Result<String> {
    Ok(path.strip_prefix(repo_root())?.display().to_string())
}

fn assert_kube_file_parses(path: &Path) -> anyhow::Result<()> {
    let path_display = relative_repo_path(path)?;
    let stored_world_string = std::fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;

    // Exercise the same stored-world loader in both the server and player control paths.
    let _server_world =
        World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None).with_context(|| format!("Server mode failed for {path_display}"))?;
    let _player_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Player(999), None)
        .with_context(|| format!("Player mode failed for {path_display}"))?;

    Ok(())
}

// A world saved before the sky gradient existed stored one flat skyColor. Loading it has to keep that color
// on every stop, so the sky still looks the way its author left it instead of falling back to the default blue.
#[test]
fn pre_gradient_stored_world_keeps_its_authored_sky_color() -> anyhow::Result<()> {
    let path = repo_root().join("evals/eval-2026-08-15/pig-tag-game/world-opus-8807.kube");
    let stored_world_string = std::fs::read_to_string(&path).with_context(|| format!("Could not read {}", path.display()))?;
    let world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;

    let world_lighting_info = world
        .objectOfId
        .values()
        .find_map(|object| match &object.info {
            ObjectInfo::WorldLighting(world_lighting_info) => Some(*world_lighting_info),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("Expected a world lighting object in {}", path.display()))?;

    // The skyColor this world was saved with, before the field became a three stop gradient.
    let stored_sky_color = Color3 { r: 0.45, g: 0.68, b: 0.95 };
    assert_eq!(world_lighting_info.skyTopColor.r, stored_sky_color.r);
    assert_eq!(world_lighting_info.skyTopColor.g, stored_sky_color.g);
    assert_eq!(world_lighting_info.skyTopColor.b, stored_sky_color.b);
    assert_eq!(world_lighting_info.skyHorizonColor.r, stored_sky_color.r);
    assert_eq!(world_lighting_info.skyHorizonColor.g, stored_sky_color.g);
    assert_eq!(world_lighting_info.skyHorizonColor.b, stored_sky_color.b);
    assert_eq!(world_lighting_info.skyBottomColor.r, stored_sky_color.r);
    assert_eq!(world_lighting_info.skyBottomColor.g, stored_sky_color.g);
    assert_eq!(world_lighting_info.skyBottomColor.b, stored_sky_color.b);

    Ok(())
}

// The editor keeps every saved id, so ids in the chat history still name the same objects after the
// editor server restarts. The world's ids have gaps, so new objects must start above the largest one.
#[test]
fn the_editor_keeps_saved_ids_and_adds_new_objects_above_them() -> anyhow::Result<()> {
    let stored_world_string = std::fs::read_to_string(repo_root().join("game-pigball.kube"))?;
    let mut stored: Value = serde_json::from_str(&stored_world_string)?;
    updateToVersion(&mut stored, STORED_WORLD_CURRENT_VERSION, StoredWorldKind::GameWorld)?;
    let mut world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Editor, None)?;

    let stored_objects = stored["objectOfId"].as_object().unwrap();
    assert_eq!(world.objectOfId.len(), stored_objects.len());
    for (id, object) in stored_objects {
        assert_eq!(world.getObject(id.parse::<usize>()?)?.name, object["name"].as_str().unwrap(), "Object {id} keeps its id");
    }

    let largest_id = stored_objects.keys().map(|id| id.parse::<usize>().unwrap()).max().unwrap();
    let added = world.addObject("Added".into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), world.getRootObjectId());
    assert_eq!(added, largest_id + 1);
    assert_eq!(world.getObject(added)?.name, "Added");
    Ok(())
}

#[test]
fn all_repo_kube_files_parse_via_from_stored_world_string() -> anyhow::Result<()> {
    let kube_paths = all_repo_kube_paths()?;
    assert!(!kube_paths.is_empty(), "Expected at least one .kube file in the repo");

    for kube_path in kube_paths {
        assert_kube_file_parses(&kube_path)?;
    }

    Ok(())
}

// Published worlds must still open in the browser editor after the material schema changes.
#[test]
fn older_materials_load_and_preserve_explicit_double_sided_settings() -> anyhow::Result<()> {
    for is_double_sided in [None, Some(false), Some(true)] {
        let mut stored: Value = serde_json::from_str(include_str!("../../editor_server/initial-draft-gameworld.kube"))?;
        stored["worldSchemaVersion"] = json!(72);
        let mut material = json!({
            "albedoTextureDataId": null,
            "metallicTextureDataId": null,
            "roughnessTextureDataId": null,
            "normalTextureDataId": null,
            "alphaMode": "Opaque",
        });
        if let Some(is_double_sided) = is_double_sided {
            material["isDoubleSided"] = json!(is_double_sided);
        }
        stored["objectOfId"]["2"] = json!({ "id": 2, "name": "material", "info": { "Material": material } });
        stored["childrenOfId"]["1"] = json!([2]);
        stored["childrenOfId"]["2"] = json!([]);
        let world = World::fromStoredWorldString(&serde_json::to_string(&stored)?, WorldControlMode::Editor, None)?;
        let material = world
            .objectOfId
            .values()
            .find_map(|object| match &object.info {
                ObjectInfo::Material(material) => Some(material),
                _ => None,
            })
            .expect("material must survive world loading");
        assert_eq!(material.isDoubleSided, is_double_sided.unwrap_or(false));
        World::fromStoredWorldString(&world.toStoredWorldString()?, WorldControlMode::Server, None)?;
    }
    Ok(())
}
