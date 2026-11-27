mod helpers;

use deno_core::{JsRuntime, RuntimeOptions, serde_v8};
use game_types::object_types::{ObjectId, ObjectInfo, shared::OutlineSettings};
use glam::Vec3;
use helpers::world_test_helpers::{add_ball, add_brick, add_cylinder, add_label, add_label_image, add_mesh, make_world};
use serde_json::{Value, json};
use shared::game::jsRustConversions::{ValueWithPath, jsLiveObjectOfRustLiveObject, rustUpdateObject};
use shared::game::physics::SetOrForce;
use shared::game::world::{World, WorldControlMode};

fn update_object_leaf(world: &mut World, object_id: ObjectId, path: &[&str], leaf_value: Value) -> anyhow::Result<()> {
    let path_strings = path.iter().map(|field_name| (*field_name).to_string()).collect::<Vec<String>>();
    let object_modify_flags = rustUpdateObject(world, object_id, ValueWithPath::new(&leaf_value, &path_strings), SetOrForce::Force)?;
    world.serverMarkObjectAsModified(object_id, object_modify_flags);
    Ok(())
}

fn outline_of(world: &World, object_id: ObjectId) -> Option<OutlineSettings> {
    match &world.getObject(object_id).expect("the object should exist").info {
        ObjectInfo::Ball(ball) => ball.visualSettings.outline,
        ObjectInfo::Brick(brick) => brick.visualSettings.outline,
        ObjectInfo::Cylinder(cylinder) => cylinder.visualSettings.outline,
        ObjectInfo::Mesh(mesh) => mesh.visualSettings.outline,
        ObjectInfo::Label(label) => label.visualSettings.outline,
        ObjectInfo::LabelImage(image) => image.visualSettings.outline,
        _ => panic!("expected an object with visual settings"),
    }
}

// The limits match what the renderer can draw: thinner than half a pixel does not show, and the renderer only reaches 8.
#[test]
fn outline_settings_accept_sensible_values_and_reject_the_rest() {
    let outline = |opacity: f32, thickness: f32| OutlineSettings {
        color: game_types::object_types::shared::Color3::BLACK,
        opacity,
        thickness,
    };
    for (opacity, thickness) in [(1.0, 2.0), (0.0, 0.5), (0.5, 8.0)] {
        assert!(
            outline(opacity, thickness).validate().is_ok(),
            "opacity {opacity} thickness {thickness} should be allowed"
        );
    }
    for (opacity, thickness) in [
        (1.0, 0.4),
        (1.0, 8.5),
        (1.0, f32::NAN),
        (1.0, f32::INFINITY),
        (-0.1, 2.0),
        (1.1, 2.0),
        (f32::NAN, 2.0),
    ] {
        assert!(
            outline(opacity, thickness).validate().is_err(),
            "opacity {opacity} thickness {thickness} should be rejected"
        );
    }
}

// Every object with visual settings takes an outline the same way: set a whole one, change one field, then clear it.
#[test]
fn every_visual_object_sets_changes_and_clears_its_outline() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, 10, root_object_id, Vec3::ZERO, true)?;
    add_brick(&mut world, 11, root_object_id, Vec3::ZERO, true)?;
    add_cylinder(&mut world, 12, root_object_id, Vec3::ZERO, true)?;
    add_mesh(&mut world, 13, root_object_id)?;
    add_label(&mut world, 14, root_object_id)?;
    add_label_image(&mut world, 15, root_object_id)?;
    for object_id in 10..=15 {
        assert!(outline_of(&world, object_id).is_none(), "objects start without an outline");
        assert!(
            update_object_leaf(&mut world, object_id, &["outline", "thickness"], json!(3.0)).is_err(),
            "an object with no outline has no thickness to set"
        );

        update_object_leaf(
            &mut world,
            object_id,
            &["outline"],
            json!({ "color": { "r": 0.1, "g": 0.2, "b": 0.3 }, "opacity": 0.75, "thickness": 2.0 }),
        )?;
        update_object_leaf(&mut world, object_id, &["outline", "thickness"], json!(4.5))?;
        update_object_leaf(&mut world, object_id, &["outline", "color", "g"], json!(0.9))?;
        let outline = outline_of(&world, object_id).expect("the outline should be set");
        assert_eq!((outline.color.r, outline.color.g, outline.color.b), (0.1, 0.9, 0.3));
        assert_eq!((outline.opacity, outline.thickness), (0.75, 4.5));

        for (path, value) in [
            (&["outline", "thickness"][..], json!(9.0)),
            (&["outline", "opacity"][..], json!(2.0)),
            (&["outline", "width"][..], json!(1.0)),
        ] {
            assert!(update_object_leaf(&mut world, object_id, path, value).is_err(), "{path:?} should be rejected");
        }
        assert!(
            update_object_leaf(
                &mut world,
                object_id,
                &["outline"],
                json!({ "color": { "r": 0, "g": 0, "b": 0 }, "opacity": 1, "thickness": 0.1 })
            )
            .is_err(),
            "a whole outline that is too thin should be rejected"
        );
        assert_eq!(outline_of(&world, object_id).expect("a rejected edit leaves the outline").thickness, 4.5);

        update_object_leaf(&mut world, object_id, &["outline"], Value::Null)?;
        assert!(outline_of(&world, object_id).is_none(), "null should clear the outline");
    }
    Ok(())
}

// Scripts see the outline as a nested object, or null when there is none.
#[test]
fn scripts_see_the_outline_or_null() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_ball(&mut world, 10, root_object_id, Vec3::ZERO, true)?;
    let mut js_runtime = JsRuntime::new(RuntimeOptions::default());
    let mut script_view = |world: &World| -> anyhow::Result<Value> {
        deno_core::scope!(scope, &mut js_runtime);
        let js_object = jsLiveObjectOfRustLiveObject(world, 10, scope)?;
        Ok(serde_v8::from_v8(scope, js_object.into())?)
    };
    assert_eq!(script_view(&world)?["outline"], Value::Null);
    update_object_leaf(
        &mut world,
        10,
        &["outline"],
        json!({ "color": { "r": 0, "g": 0, "b": 0 }, "opacity": 1, "thickness": 2 }),
    )?;
    assert_eq!(
        script_view(&world)?["outline"],
        json!({ "color": { "r": 0, "g": 0, "b": 0 }, "opacity": 1, "thickness": 2 })
    );
    Ok(())
}

// Objects saved before outlines existed load without one, and an outline survives a save and load.
#[test]
fn outlines_load_from_old_worlds_and_survive_saving() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_brick(&mut world, 11, root_object_id, Vec3::ZERO, true)?;
    add_ball(&mut world, 12, root_object_id, Vec3::ZERO, true)?;
    update_object_leaf(
        &mut world,
        12,
        &["outline"],
        json!({ "color": { "r": 1, "g": 0, "b": 0 }, "opacity": 0.5, "thickness": 3 }),
    )?;

    let mut stored_world: Value = serde_json::from_str(&world.toStoredWorldString()?)?;
    stored_world["worldSchemaVersion"] = json!(92);
    stored_world["objectOfId"]["11"]["info"]["Brick"]["visualSettings"]
        .as_object_mut()
        .expect("the saved brick should hold its visual settings")
        .remove("outline");
    let reloaded_world = World::fromStoredWorldString(&serde_json::to_string(&stored_world)?, WorldControlMode::Server, None)?;
    // Loading gives objects new ids, so they are found by name.
    let object_id_named = |name: &str| *reloaded_world.getObjectIdsByName(name).first().expect("the object should still exist");
    assert!(
        outline_of(&reloaded_world, object_id_named("brick-11")).is_none(),
        "an old brick should load without an outline"
    );
    let outline = outline_of(&reloaded_world, object_id_named("ball-12")).expect("the ball's outline should survive saving");
    assert_eq!((outline.color.r, outline.opacity, outline.thickness), (1.0, 0.5, 3.0));
    Ok(())
}
