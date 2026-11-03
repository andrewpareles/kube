mod helpers;

use game_types::{
    data_types::DataIds,
    object_types::{ObjectBeingAdded, WorldLightingInfo},
};
use helpers::world_test_helpers::{add_label_image, add_local_group, add_material, add_mesh, add_sound_player, make_world};
use std::collections::HashSet;

// A player loads every data id in the world they're sent when they join, so this counts what nothing draws or plays yet: a Material no
// shape uses, a LabelImage on nothing, and a SoundPlayer with no Clock. Each kind stays in its own list.
#[test]
fn collect_all_data_ids_has_every_kind_of_data_even_what_nothing_draws_or_plays() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_mesh(&mut world, 2, root_object_id)?;
    add_material(&mut world, 3, root_object_id)?;
    add_label_image(&mut world, 4, root_object_id)?;
    add_sound_player(&mut world, 5, root_object_id, None)?;
    let world_lighting_info = WorldLightingInfo {
        skyboxTextureDataId: Some(55),
        ..WorldLightingInfo::default()
    };
    world.addObjectWithId(6, "sky".to_string(), ObjectBeingAdded::WorldLighting(world_lighting_info), root_object_id);

    let expected_data_ids = DataIds {
        mesh_data_ids: HashSet::from([456]),
        texture_data_ids: HashSet::from([9001, 9002, 9003, 9004, 789, 55]),
        sound_data_ids: HashSet::from([777]),
    };
    assert_eq!(world.collectAllDataIds(), expected_data_ids);
    Ok(())
}

// Templates are never sent to players, so the server lists the data in every object of every template, children included. Data that's
// only in the world isn't listed, since players already get the world.
#[test]
fn template_data_ids_have_the_data_of_every_object_in_a_template() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    let group_object_id = 2;
    add_local_group(&mut world, group_object_id, root_object_id)?;
    add_mesh(&mut world, 3, group_object_id)?;
    add_material(&mut world, 4, group_object_id)?;
    add_sound_player(&mut world, 5, group_object_id, None)?;
    add_label_image(&mut world, 6, root_object_id)?;
    world.cutObjectToTemplate(group_object_id, "coin".to_string())?;

    let expected_data_ids = DataIds {
        mesh_data_ids: HashSet::from([456]),
        texture_data_ids: HashSet::from([9001, 9002, 9003, 9004]),
        sound_data_ids: HashSet::from([777]),
    };
    assert_eq!(world.getTemplateDataIds(), expected_data_ids);
    Ok(())
}
