mod helpers;

use std::collections::{HashMap, HashSet};

use game_types::object_types::{
    BallInfo, BallMaterialSettings, ClockInfo, ClockState, Location3D, ObjectId, ObjectIdPointer, ObjectInfo, ObjectLocation, PropertyAnimatorInfo,
    SoundPlayerInfo, VisualSettings,
};
use game_types::physics_primitive_types::{BrickPrimitiveSettings, PrimitiveSettings};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::*;
use shared::game::physics::{SetPhysicsInfo3D, SetPhysicsToggles3D};
use shared::game::world::{PartPlace, RetrackedPart, World, WorldControlMode, WorldEditHistory};

fn editor_world() -> anyhow::Result<World> {
    World::fromStoredWorldString(&make_world()?.toStoredWorldString()?, WorldControlMode::Editor, None)
}

fn edit_history(world: &World) -> &WorldEditHistory {
    world.editHistory.as_ref().expect("The edit history is on")
}

fn retracked_parts_by_id(world: &World) -> Vec<&RetrackedPart> {
    let mut retracked_parts: Vec<&RetrackedPart> = edit_history(world).retracked_parts.iter().collect();
    retracked_parts.sort_by_key(|retracked_part| retracked_part.part_id);
    retracked_parts
}

fn assert_in_group(place: &PartPlace, expected_group_id: ObjectId, expected_location: &Location3D) {
    let PartPlace::InGroup(group_id, location) = place else {
        panic!("Expected a place in group {expected_group_id}, got {place:?}");
    };
    assert_eq!(*group_id, expected_group_id);
    assert_eq!(location.position, expected_location.position);
    assert_eq!(location.quaternion, expected_location.quaternion);
}

fn relative_location(world: &World, part_id: ObjectId) -> Location3D {
    world.physics.getSubitemRelativeLocation3D(part_id).expect("The part is in a rigid group")
}

fn current_place(world: &World, part_id: ObjectId) -> PartPlace {
    match world.physics.getSubitemRigidGroupId(part_id) {
        Some(group_id) => PartPlace::InGroup(group_id, relative_location(world, part_id)),
        None => PartPlace::Free(world.getObjectPhysicsInfo3D(part_id).expect("The part has a place")),
    }
}

fn place_bits(place: &PartPlace) -> (Option<ObjectId>, Vec<u32>) {
    let (group_id, location, movement) = match place {
        PartPlace::InGroup(group_id, location) => (Some(*group_id), location, None),
        PartPlace::Free(physics_info) => (None, &physics_info.location, Some(&physics_info.movement)),
    };
    let mut floats = [location.position.to_array().as_slice(), location.quaternion.to_array().as_slice()].concat();
    if let Some(movement) = movement {
        floats.extend(movement.velocity.to_array().into_iter().chain(movement.angularVelocity.to_array()));
    }
    (group_id, floats.into_iter().map(f32::to_bits).collect())
}

// The edit history pushes exactly the ids that browser sync is told about, once each, in the order they were first marked.
#[test]
fn every_marking_setter_pushes_its_id_in_first_touch_order() -> anyhow::Result<()> {
    let mut world = editor_world()?;
    let root_id = world.getRootObjectId();
    add_clock(&mut world, 2, root_id)?;
    add_ball(&mut world, 3, root_id, Vec3::ZERO, false)?;
    add_local_group(&mut world, 4, root_id)?;
    add_script(&mut world, 5, root_id)?;
    add_brick(&mut world, 6, root_id, Vec3::X, false)?;
    add_cylinder(&mut world, 7, root_id, Vec3::Y, false)?;
    add_sound_player(&mut world, 8, root_id, None)?;
    add_ball(&mut world, 10, root_id, Vec3::Z, false)?;
    add_brick(&mut world, 11, root_id, Vec3::NEG_X, false)?;
    add_ball(&mut world, 13, root_id, Vec3::NEG_Y, false)?;
    add_rigid_group(&mut world, 14, root_id, Vec3::ZERO, None)?;
    add_ball(&mut world, 15, 14, Vec3::Z, false)?;
    assert!(world.editHistory.is_none(), "The edit history is off unless someone turns it on");
    world.flushDirtyObjects();
    world.editHistory = Some(WorldEditHistory::default());

    world.setObjectName(3, "renamed".into())?;
    world.setClockInfo(
        2,
        ClockInfo {
            shouldLoop: false,
            clockState: ClockState::Playing {
                localStartMs: 0.0,
                timeLocation: ObjectLocation::Client,
            },
            speed: 2.0,
        },
    )?;
    world.setParent(6, 4)?;
    world.setPhysicsInfo3D(
        7,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(0.0, 5.0, 0.0)),
            ..Default::default()
        },
        true,
    );
    world.setPhysicsToggles3D(
        10,
        SetPhysicsToggles3D {
            isPushable: Some(true),
            ..Default::default()
        },
        true,
    );
    world.setPrimitiveSettings(
        11,
        PrimitiveSettings::Brick(BrickPrimitiveSettings {
            size: Vec3::new(4.0, 4.0, 4.0),
        }),
        true,
    )?;
    world.setScriptRawCode(5, "return 0;".into());
    world.setSubitemRelativeLocation3D(
        15,
        Location3D {
            position: Vec3::Y,
            quaternion: Quat::IDENTITY,
        },
    )?;
    add_local_group(&mut world, 12, root_id)?;
    world.removeObject(8)?;
    world.setObjectName(3, "renamed-again".into())?;
    world.setPhysicsInfo3D(
        13,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(0.0, -5.0, 0.0)),
            ..Default::default()
        },
        false,
    );

    assert_eq!(edit_history(&world).touched_object_ids, vec![3, 2, 6, 7, 10, 11, 5, 15, 12, 8]);
    let marked_ids: HashSet<ObjectId> = world
        .modifiedObjectFlags
        .keys()
        .chain(&world.addedObjectIds)
        .chain(&world.removedObjectIds)
        .copied()
        .collect();
    assert_eq!(
        edit_history(&world)._touched_object_id_set,
        marked_ids,
        "The edit history holds exactly what browser sync is told about"
    );
    assert_eq!(
        edit_history(&world)
            .retracked_parts
            .iter()
            .map(|retracked_part| (retracked_part.part_id, retracked_part.reparented_object_id))
            .collect::<Vec<_>>(),
        vec![(6, 6)],
        "Only the reparented brick was re-tracked"
    );
    Ok(())
}

// First (at x=10) holds a folder with a part at x=12 and a nested folder with a part at (10, 3, 0). Moving the
// folder into Second changes the group of those two parts, and nothing else.
#[test]
fn moving_a_folder_between_rigid_groups_records_each_part_it_moved() -> anyhow::Result<()> {
    let mut world = editor_world()?;
    let root_id = world.getRootObjectId();
    let (first_id, second_id, folder_id, part_id, inner_folder_id, inner_part_id, staying_part_id, free_ball_id) = (10, 20, 11, 12, 13, 14, 15, 30);
    add_rigid_group(&mut world, first_id, root_id, Vec3::new(10.0, 0.0, 0.0), None)?;
    add_rigid_group(&mut world, second_id, root_id, Vec3::new(-5.0, 1.0, 0.0), None)?;
    add_local_group(&mut world, folder_id, first_id)?;
    add_ball(&mut world, part_id, folder_id, Vec3::new(12.0, 0.0, 0.0), false)?;
    add_local_group(&mut world, inner_folder_id, folder_id)?;
    add_brick(&mut world, inner_part_id, inner_folder_id, Vec3::new(10.0, 3.0, 0.0), false)?;
    add_cylinder(&mut world, staying_part_id, first_id, Vec3::new(8.0, 0.0, 0.0), false)?;
    add_ball(&mut world, free_ball_id, root_id, Vec3::new(0.0, 0.0, 9.0), false)?;
    let (part_place_before, inner_part_place_before) = (relative_location(&world, part_id), relative_location(&world, inner_part_id));
    world.editHistory = Some(WorldEditHistory::default());

    world.setParent(folder_id, second_id)?;

    assert_eq!(edit_history(&world).touched_object_ids, vec![folder_id], "Only the folder is marked");
    let retracked_parts = retracked_parts_by_id(&world);
    assert_eq!(
        retracked_parts.iter().map(|retracked_part| retracked_part.part_id).collect::<Vec<_>>(),
        vec![part_id, inner_part_id]
    );
    for (retracked_part, place_before) in retracked_parts.iter().zip([&part_place_before, &inner_part_place_before]) {
        assert_eq!(retracked_part.reparented_object_id, folder_id);
        assert_in_group(&retracked_part.place_before, first_id, place_before);
        assert_in_group(&retracked_part.place_after, second_id, &relative_location(&world, retracked_part.part_id));
    }
    Ok(())
}

// A rigid group at (3, 1.5, -7.25) turned 137 degrees holds a folder with two parts. Each reparent records every
// part under the moved object, whether or not its group changed, so each part's recorded places chain exactly from
// where it started to where it is now.
#[test]
fn every_part_a_reparent_re_tracks_is_recorded_and_its_places_chain() -> anyhow::Result<()> {
    let mut world = editor_world()?;
    let root_id = world.getRootObjectId();
    let (group_id, folder_id, part_id, inner_folder_id, inner_part_id, other_folder_id, free_ball_id) = (10, 11, 12, 13, 14, 16, 30);
    add_rigid_group(&mut world, group_id, root_id, Vec3::new(3.0, 1.5, -7.25), None)?;
    world.setPhysicsInfo3D(
        group_id,
        SetPhysicsInfo3D {
            rotation: Some(Quat::from_rotation_y(137f32.to_radians())),
            ..Default::default()
        },
        true,
    );
    add_local_group(&mut world, folder_id, group_id)?;
    add_ball(&mut world, part_id, folder_id, Vec3::new(4.2, 2.0, -6.1), false)?;
    add_local_group(&mut world, inner_folder_id, folder_id)?;
    add_brick(&mut world, inner_part_id, inner_folder_id, Vec3::new(1.3, 0.7, -9.9), false)?;
    add_local_group(&mut world, other_folder_id, group_id)?;
    add_ball(&mut world, free_ball_id, root_id, Vec3::new(0.0, 0.0, 9.0), false)?;
    let start_place_of_id: HashMap<ObjectId, PartPlace> = [part_id, inner_part_id, free_ball_id]
        .into_iter()
        .map(|id| (id, current_place(&world, id)))
        .collect();
    world.editHistory = Some(WorldEditHistory::default());

    world.setParent(folder_id, other_folder_id)?;
    world.setParent(folder_id, group_id)?;
    world.setParent(folder_id, other_folder_id)?;
    world.setParent(folder_id, root_id)?;
    world.setParent(free_ball_id, group_id)?;

    assert_eq!(
        edit_history(&world).touched_object_ids,
        vec![folder_id, free_ball_id],
        "Only the reparented objects are marked"
    );
    let groups_in_folder = vec![Some(group_id), Some(group_id), Some(group_id), Some(group_id), None];
    for (part_id, expected_causes, expected_groups) in [
        (part_id, vec![folder_id; 4], groups_in_folder.clone()),
        (inner_part_id, vec![folder_id; 4], groups_in_folder),
        (free_ball_id, vec![free_ball_id], vec![None, Some(group_id)]),
    ] {
        let retracked_parts: Vec<&RetrackedPart> = edit_history(&world)
            .retracked_parts
            .iter()
            .filter(|retracked_part| retracked_part.part_id == part_id)
            .collect();
        assert_eq!(
            retracked_parts
                .iter()
                .map(|retracked_part| retracked_part.reparented_object_id)
                .collect::<Vec<_>>(),
            expected_causes,
            "Part {part_id} is recorded once for each reparent above it"
        );
        let mut places = vec![place_bits(&start_place_of_id[&part_id])];
        for retracked_part in &retracked_parts {
            assert_eq!(
                place_bits(&retracked_part.place_before),
                *places.last().unwrap(),
                "Part {part_id}'s place before a reparent is its place after the last one"
            );
            places.push(place_bits(&retracked_part.place_after));
        }
        assert_eq!(
            *places.last().unwrap(),
            place_bits(&current_place(&world, part_id)),
            "Part {part_id}'s last recorded place is its place now"
        );
        assert_eq!(places.iter().map(|(group_id, _)| *group_id).collect::<Vec<_>>(), expected_groups);
    }
    Ok(())
}

// Removing the house marks the house, everything inside it, and every object outside it whose link into it was
// cleared. An object linked to something that stays is not marked.
#[test]
fn a_removal_marks_every_member_and_every_object_whose_link_it_cleared() -> anyhow::Result<()> {
    let mut world = editor_world()?;
    let root_id = world.getRootObjectId();
    let (house_id, material_id, inside_ball_id, clock_id, room_id, room_ball_id) = (40, 41, 42, 43, 44, 45);
    let (outside_ball_id, sound_player_id, animator_id, other_clock_id, other_sound_player_id) = (50, 51, 52, 53, 54);
    add_local_group(&mut world, house_id, root_id)?;
    add_material(&mut world, material_id, house_id)?;
    add_ball(&mut world, inside_ball_id, house_id, Vec3::ZERO, false)?;
    add_clock(&mut world, clock_id, house_id)?;
    add_local_group(&mut world, room_id, house_id)?;
    add_ball(&mut world, room_ball_id, room_id, Vec3::X, false)?;
    add_ball(&mut world, outside_ball_id, root_id, Vec3::Y, false)?;
    add_sound_player(&mut world, sound_player_id, root_id, Some(clock_id))?;
    add_property_animator(&mut world, animator_id, root_id)?;
    add_clock(&mut world, other_clock_id, root_id)?;
    add_sound_player(&mut world, other_sound_player_id, root_id, Some(other_clock_id))?;
    for ball_id in [inside_ball_id, outside_ball_id] {
        world.setBallInfo(
            ball_id,
            BallInfo {
                material: Some(BallMaterialSettings {
                    materialInfoId: ObjectIdPointer(material_id),
                    tileSizeU: 1.0,
                    tileSizeV: 1.0,
                    tileOffsetU: 0.0,
                    tileOffsetV: 0.0,
                }),
                visualSettings: VisualSettings::TYPICAL_OPAQUE,
            },
        )?;
    }
    let ObjectInfo::PropertyAnimator(animator) = world.getObject(animator_id)?.info.clone() else {
        anyhow::bail!("Expected a property animator");
    };
    world.setObjectInfo(
        animator_id,
        ObjectInfo::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: Some(ObjectIdPointer(room_ball_id)),
            ..animator
        }),
    )?;
    world.editHistory = Some(WorldEditHistory::default());

    world.removeObject(house_id)?;

    let touched_object_ids = &edit_history(&world).touched_object_ids;
    assert_eq!(touched_object_ids.first(), Some(&house_id), "The house is marked first");
    assert_eq!(
        touched_object_ids.iter().copied().collect::<HashSet<_>>(),
        HashSet::from([
            house_id,
            material_id,
            inside_ball_id,
            clock_id,
            room_id,
            room_ball_id,
            outside_ball_id,
            sound_player_id,
            animator_id
        ])
    );
    assert_eq!(touched_object_ids.len(), 9, "Each object is pushed once");
    let ObjectInfo::SoundPlayer(SoundPlayerInfo { clockId, .. }) = &world.getObject(other_sound_player_id)?.info else {
        anyhow::bail!("Expected a sound player");
    };
    assert_eq!(*clockId, Some(ObjectIdPointer(other_clock_id)), "A link to an object that stays is kept");
    Ok(())
}
