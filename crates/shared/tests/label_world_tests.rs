mod helpers;

use game_types::consts::STORED_WORLD_CURRENT_VERSION;
use game_types::object_types::{BillboardMode, LabelHorizontalAlign, LabelInfo, ObjectBeingAdded, ObjectIdPointer, ObjectInfo, VisualSettings, shared::Color3};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{add_brick, make_world};
use serde_json::{Value, json};
use shared::game::{
    jsRustConversions::{ValueWithPath, rustUpdateObject},
    migrateStoredWorld::{StoredWorldKind, updateToVersion},
    physics::SetOrForce,
    world::{World, WorldControlMode},
};

fn label_info_for_test(follows: usize) -> LabelInfo {
    LabelInfo {
        text: "Round Trip".to_string(),
        lineHeight: 0.75,
        maxWidth: Some(4.5),
        horizontalAlign: LabelHorizontalAlign::Right,
        backgroundColor: Some(Color3 { r: 0.1, g: 0.2, b: 0.3 }),
        backgroundOpacity: 0.8,
        offsetFromObject: Vec3::new(0.0, 2.5, 0.0),
        rotationFromObject: Quat::IDENTITY,
        physicalObjectId: Some(ObjectIdPointer(follows)),
        billboardMode: BillboardMode::Full,
        isConstantSize: true,
        visualSettings: VisualSettings {
            opacity: 0.9,
            color: Color3 { r: 0.2, g: 0.4, b: 0.6 },
            glow: 0.0,
            isAlwaysOnTop: true,
        },
    }
}

// Loading a stored world hands out fresh object ids, so tests find their label by name.
fn label_info_of_world(world: &World, name: &str) -> LabelInfo {
    let object_id = *world.getObjectIdsByName(name).first().expect("label should still exist");
    let object = world.getObject(object_id).expect("label should still exist");
    let ObjectInfo::Label(label_info) = &object.info else {
        panic!("object {name} should be a label");
    };
    label_info.clone()
}

#[test]
fn label_survives_a_save_and_load_round_trip() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_brick(&mut world, 10, 1, Vec3::new(0.0, 0.0, 0.0), false)?;
    world.addObjectWithId(11, "nameplate".to_string(), ObjectBeingAdded::Label(label_info_for_test(10)), 1);

    let stored_world_string = world.toStoredWorldString()?;
    let reloaded_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;

    let reloaded_label_info = label_info_of_world(&reloaded_world, "nameplate");
    assert_eq!(reloaded_label_info.text, "Round Trip");
    assert_eq!(reloaded_label_info.lineHeight, 0.75);
    assert_eq!(reloaded_label_info.maxWidth, Some(4.5));
    assert_eq!(reloaded_label_info.horizontalAlign, LabelHorizontalAlign::Right);
    assert_eq!(reloaded_label_info.backgroundOpacity, 0.8);
    assert_eq!(reloaded_label_info.offsetFromObject, Vec3::new(0.0, 2.5, 0.0));
    assert_eq!(reloaded_label_info.billboardMode, BillboardMode::Full);
    assert!(reloaded_label_info.isConstantSize);
    assert!(reloaded_label_info.visualSettings.isAlwaysOnTop);

    // The label has to come back pointing at whatever id the brick was rebuilt with, not the one it was saved with.
    let physical_object_id: usize = reloaded_label_info
        .physicalObjectId
        .expect("a reloaded label should still follow its object")
        .into();
    let followed_object = reloaded_world.getObject(physical_object_id).expect("the followed object should exist");
    assert!(matches!(followed_object.info, ObjectInfo::Brick(_)));

    Ok(())
}

// Migration changes only the retired mode, preserving every other stored value even if run again.
#[test]
fn upright_label_migration_preserves_other_world_data() -> anyhow::Result<()> {
    let mut stored_world = json!({
        "worldSchemaVersion": 72,
        "objectOfId": {
            "1": { "info": { "Label": { "billboardMode": "Upright", "text": "Upright", "isConstantSize": true } } },
            "2": { "info": { "Label": { "billboardMode": "Full", "rotationFromObject": [0, 0, 0, 1] } } },
            "3": { "info": { "Label": { "billboardMode": "None", "offsetFromObject": [0, 2, 0] } } },
            "4": { "info": { "Label": { "text": "No billboard mode" } } },
            "5": { "info": { "Brick": { "visualSettings": { "opacity": 0.5 } } } },
            "6": { "info": { "Label": { "billboardMode": "invalid" } } }
        },
        "scriptRawCodeOfId": { "7": "const text = 'Upright';" }
    });
    let mut expected_world = stored_world.clone();
    expected_world["worldSchemaVersion"] = json!(73);
    expected_world["objectOfId"]["1"]["info"]["Label"]["billboardMode"] = json!("Full");

    updateToVersion(&mut stored_world, 73, StoredWorldKind::GameWorld)?;
    assert_eq!(stored_world, expected_world);
    updateToVersion(&mut stored_world, 73, StoredWorldKind::GameWorld)?;
    assert_eq!(stored_world, expected_world);
    Ok(())
}

// The normal world loader must migrate old labels before the current enum is deserialized.
#[test]
fn old_worlds_with_upright_labels_load_as_full_billboards() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_brick(&mut world, 10, 1, Vec3::ZERO, false)?;
    world.addObjectWithId(11, "nameplate".to_string(), ObjectBeingAdded::Label(label_info_for_test(10)), 1);
    let stored_world_string = world.toStoredWorldString()?;

    for stored_version in [71, 72] {
        let mut stored_world: Value = serde_json::from_str(&stored_world_string)?;
        stored_world["worldSchemaVersion"] = json!(stored_version);
        stored_world["objectOfId"]["11"]["info"]["Label"]["billboardMode"] = json!("Upright");

        let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
        assert_eq!(label_info_of_world(&reloaded_world, "nameplate").billboardMode, BillboardMode::Full);
        let resaved_world: Value = serde_json::from_str(&reloaded_world.toStoredWorldString()?)?;
        assert_eq!(resaved_world["worldSchemaVersion"], json!(STORED_WORLD_CURRENT_VERSION));
    }

    Ok(())
}

#[test]
fn a_label_with_nothing_to_follow_still_loads() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let mut label_info = label_info_for_test(10);
    label_info.physicalObjectId = None;
    world.addObjectWithId(11, "orphan".to_string(), ObjectBeingAdded::Label(label_info), 1);

    let stored_world_string = world.toStoredWorldString()?;
    let reloaded_world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;
    assert_eq!(label_info_of_world(&reloaded_world, "orphan").physicalObjectId, None);

    Ok(())
}

#[test]
fn deleting_the_followed_object_clears_the_labels_pointer() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_brick(&mut world, 10, 1, Vec3::new(0.0, 0.0, 0.0), false)?;
    world.addObjectWithId(11, "nameplate".to_string(), ObjectBeingAdded::Label(label_info_for_test(10)), 1);
    assert_eq!(label_info_of_world(&world, "nameplate").physicalObjectId, Some(ObjectIdPointer(10)));

    world.removeObject(10)?;
    assert_eq!(label_info_of_world(&world, "nameplate").physicalObjectId, None);

    Ok(())
}

#[test]
fn the_label_demo_world_loads_with_its_labels() -> anyhow::Result<()> {
    let kube_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../label-demo.kube");
    let stored_world_string = std::fs::read_to_string(kube_path)?;
    let world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)?;

    let label_count = world
        .getAllObjects()
        .iter()
        .filter(|(_, object)| matches!(object.info, ObjectInfo::Label(_)))
        .count();
    assert!(label_count >= 10, "the demo world should still be full of labels, found {label_count}");

    for (_, object) in world.getAllObjects() {
        let ObjectInfo::Label(label_info) = &object.info else {
            continue;
        };
        let physical_object_id: usize = label_info
            .physicalObjectId
            .unwrap_or_else(|| panic!("demo label {} should follow an object", object.name))
            .into();
        assert!(
            world.getObject(physical_object_id).is_ok(),
            "demo label {} points at a missing object",
            object.name,
        );
    }

    Ok(())
}

// Worlds saved before these fields existed must still load.
#[test]
fn worlds_saved_before_labels_existed_still_load() -> anyhow::Result<()> {
    for kube_file_name in ["dev-demo.kube", "game-bird.kube", "game-pigball.kube", "game-early-baseplate.kube"] {
        let kube_path = format!("{}/../../{}", env!("CARGO_MANIFEST_DIR"), kube_file_name);
        let stored_world_string = std::fs::read_to_string(&kube_path)?;
        let world = World::fromStoredWorldString(&stored_world_string, WorldControlMode::Server, None)
            .unwrap_or_else(|error| panic!("{kube_file_name} should still load: {error}"));

        let has_any_billboarding = world.getAllObjects().iter().any(|(_, object)| match &object.info {
            ObjectInfo::Label(label_info) => label_info.billboardMode != BillboardMode::None,
            _ => false,
        });
        assert!(!has_any_billboarding, "{kube_file_name} should default to no billboarding");
    }

    Ok(())
}

// Emoji are removed from label text, along with the invisible characters that only join emoji together. Symbols that only
// look like emoji when asked to, like (c), hearts and digits, stay.
#[test]
fn label_text_loses_its_emoji_but_keeps_text_symbols() {
    for (text, text_without_emoji) in [
        ("A\u{1F600}B", "AB"),
        ("I \u{2764}\u{FE0F} it", "I  it"),
        ("Go \u{1F3F3}\u{FE0F}\u{200D}\u{1F308} team", "Go  team"),
        ("\u{1F44D}\u{1F3FD}ok", "ok"),
        ("Press 1\u{FE0F}\u{20E3}", "Press "),
        ("\u{1F1FA}\u{1F1F8} flag", " flag"),
        ("\u{A9} 2026 \u{2122} #1 \u{2665}", "\u{A9} 2026 \u{2122} #1 \u{2665}"),
    ] {
        assert_eq!(LabelInfo::text_without_emoji(text), text_without_emoji, "{text:?}");
    }
}

// However label text gets into the world, it arrives without emoji.
#[test]
fn labels_never_hold_emoji() -> anyhow::Result<()> {
    let mut world = make_world()?;
    add_brick(&mut world, 10, 1, Vec3::ZERO, false)?;
    let text_of_label = |world: &World| -> String {
        let ObjectInfo::Label(label_info) = &world.getObject(11).expect("the label should exist").info else {
            panic!("object 11 should be a label");
        };
        label_info.text.clone()
    };

    world.addObjectWithId(
        11,
        "sign".to_string(),
        ObjectBeingAdded::Label(LabelInfo {
            text: "Hi \u{1F600}".to_string(),
            ..label_info_for_test(10)
        }),
        1,
    );
    assert_eq!(text_of_label(&world), "Hi ");

    world.setLabelInfo(
        11,
        LabelInfo {
            text: "Bye \u{1F680}".to_string(),
            ..label_info_for_test(10)
        },
    )?;
    assert_eq!(text_of_label(&world), "Bye ");

    // How a script's `label.text = ...` lands.
    let script_text = json!("Yo \u{1F389}");
    rustUpdateObject(&mut world, 11, ValueWithPath::new(&script_text, &["text".to_string()]), SetOrForce::Force)?;
    assert_eq!(text_of_label(&world), "Yo ");

    // Worlds saved before emoji were removed lose theirs when they load.
    let mut stored_world: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored_world["objectOfId"]["11"]["info"]["Label"]["text"] = json!("Old \u{1F3C6}");
    let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
    assert_eq!(label_info_of_world(&reloaded_world, "sign").text, "Old ");
    Ok(())
}
