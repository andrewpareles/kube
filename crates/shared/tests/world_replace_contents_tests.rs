mod helpers;

use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    object_types::{
        CameraLocation, LiveObject, LocalGroupInfo, ObjectId, ObjectIdPointer, ObjectInfo, ObjectKind, Orientation3D, PropertyAnimatorInfo, SoundPlayerInfo,
    },
    stored_world_type::StoredWorld,
};
use glam::Vec3;
use helpers::world_test_helpers::*;
use shared::game::world::{World, WorldControlMode};
use std::collections::HashMap;

fn live_world() -> anyhow::Result<World> {
    World::fromStoredWorldString(include_str!("../../../dev-demo.kube"), WorldControlMode::Editor, None)
}

fn only_id_of_kind(world: &World, kind: ObjectKind) -> ObjectId {
    let ids = world._kindOfObjectId.get_keys(&kind).expect("The world has this kind");
    assert_eq!(ids.len(), 1, "The world has exactly one {kind:?}");
    *ids.iter().next().unwrap()
}

// A sound player and its clock sit in different groups, and an animator targets the root, so the copy
// has to rewire pointers across subtrees and onto the live root. Its other ids collide with the live world's.
fn uploaded_world() -> anyhow::Result<World> {
    let root_id = 900;
    let stored_world = StoredWorld {
        worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
        fallback_camera_location: CameraLocation {
            position: Vec3::new(3.0, 4.0, 5.0),
            orientation: Orientation3D { theta: 0.5, phi: 0.25 },
        },
        objectOfId: HashMap::from([(
            root_id,
            LiveObject {
                id: root_id,
                name: "root".into(),
                info: ObjectInfo::LocalGroup(LocalGroupInfo {}),
            },
        )]),
        rootObjectId: root_id,
        childrenOfId: HashMap::new(),
        scriptRawCodeOfId: HashMap::new(),
        physicsInfo3DOfId: HashMap::new(),
        physicsToggles3DOfId: HashMap::new(),
        primitiveSettingsOfId: HashMap::new(),
    };
    let mut world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Editor, None)?;
    add_local_group(&mut world, 2, root_id)?;
    add_local_group(&mut world, 3, root_id)?;
    add_clock(&mut world, 4, 3)?;
    add_sound_player(&mut world, 5, 2, Some(4))?;
    add_property_animator(&mut world, 6, 2)?;
    let ObjectInfo::PropertyAnimator(animator) = world.getObject(6)?.info.clone() else {
        anyhow::bail!("Expected a property animator");
    };
    world.setObjectInfo(
        6,
        ObjectInfo::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: Some(ObjectIdPointer(root_id)),
            ..animator
        }),
    )?;
    Ok(world)
}

#[test]
fn replacing_contents_keeps_the_root_and_gives_every_uploaded_object_a_new_id() -> anyhow::Result<()> {
    let mut world = live_world()?;
    let root_id = world.getRootObjectId();
    let old_max_id = world.objectOfId.keys().copied().max().unwrap();
    let uploaded = uploaded_world()?;
    assert_ne!(uploaded.getRootObjectId(), root_id);

    world.loadNewWorld(&uploaded)?;

    assert_eq!(world.getRootObjectId(), root_id, "The live root keeps its id");
    assert_eq!(world.objectOfId.len(), uploaded.objectOfId.len(), "Only the uploaded objects remain");
    for id in world.objectOfId.keys().filter(|id| **id != root_id) {
        assert!(*id > old_max_id, "Object {id} reuses an id at or below {old_max_id}");
    }
    let sound_player_id = only_id_of_kind(&world, ObjectKind::SoundPlayer);
    let clock_id = only_id_of_kind(&world, ObjectKind::Clock);
    let ObjectInfo::SoundPlayer(SoundPlayerInfo { clockId, .. }) = &world.getObject(sound_player_id)?.info else {
        anyhow::bail!("Expected a sound player");
    };
    assert_eq!(*clockId, Some(ObjectIdPointer(clock_id)), "The pointer across groups follows the copied clock");
    assert_ne!(world.parentOfId[&sound_player_id], world.parentOfId[&clock_id]);
    let ObjectInfo::PropertyAnimator(animator) = &world.getObject(only_id_of_kind(&world, ObjectKind::PropertyAnimator))?.info else {
        anyhow::bail!("Expected a property animator");
    };
    assert_eq!(
        animator.targetObjectId,
        Some(ObjectIdPointer(root_id)),
        "A pointer to the uploaded root lands on the live root"
    );
    assert!(
        world._objectsThatPointToId[&clock_id].contains(&sound_player_id),
        "The reverse pointer map follows the rewire"
    );
    assert_eq!(world.fallback_camera_location.position, Vec3::new(3.0, 4.0, 5.0));
    assert_eq!(world.fallback_camera_location.orientation.theta, 0.5);
    Ok(())
}

// A real saved world, on an older schema, replaces a live one and survives saving and loading again.
#[test]
fn a_repo_world_file_replaces_a_live_world() -> anyhow::Result<()> {
    let mut world = live_world()?;
    let uploaded = World::fromStoredWorldString(include_str!("../../../game-pigball.kube"), WorldControlMode::Editor, None)?;

    world.loadNewWorld(&uploaded)?;

    let reloaded = World::fromStoredWorldString(&world.toStoredWorldString()?, WorldControlMode::Editor, None)?;
    let read_names_and_kinds = |world: &World| {
        let mut names_and_kinds: Vec<_> = world
            .objectOfId
            .values()
            .map(|object| (object.name.clone(), format!("{:?}", ObjectKind::from_object_info(&object.info))))
            .collect();
        names_and_kinds.sort();
        names_and_kinds
    };
    assert_eq!(read_names_and_kinds(&reloaded), read_names_and_kinds(&uploaded));
    let mut scripts: Vec<_> = reloaded.scriptRawCodeOfId.values().collect();
    let mut uploaded_scripts: Vec<_> = uploaded.scriptRawCodeOfId.values().collect();
    scripts.sort();
    uploaded_scripts.sort();
    assert_eq!(scripts, uploaded_scripts, "Every script comes across unchanged");
    Ok(())
}
