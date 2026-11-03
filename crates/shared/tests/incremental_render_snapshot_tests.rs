mod helpers;

use game_types::object_delta::{NetworkedCoords, NetworkedPhysicsInfo3D};
use game_types::object_types::{Location3D, ObjectId, ObjectIdPointer, ObjectInfo};
use game_types::physics_primitive_types::{CylinderPrimitiveSettings, PrimitiveSettings};
use glam::{Quat, Vec3};
use helpers::world_test_helpers::{
    add_ball, add_brick, add_cylinder, add_label, add_label_image, add_material, add_mesh, add_player, add_rigid_group, add_sk_attachment, add_skeleton,
    add_world_lighting, make_world_controlled_by_player, physics_info,
};
use render_core::renderable::{RenderObjectCache, Renderable};
use shared::game::physics::{SetPhysicsInfo3D, SetPhysicsToggles3D};
use shared::game::world::World;
use std::collections::BTreeMap;

const PLAYER_OBJECT_ID: ObjectId = 999;
const ROOT_OBJECT_ID: ObjectId = 1;
const GROUP_OBJECT_ID: ObjectId = 2;
const GROUP_BRICK_OBJECT_ID: ObjectId = 3;
const GROUP_CYLINDER_OBJECT_ID: ObjectId = 4;
const STANDALONE_BRICK_OBJECT_ID: ObjectId = 5;
const PUSHABLE_BALL_OBJECT_ID: ObjectId = 6;
const MESH_OBJECT_ID: ObjectId = 7;
const LABEL_OBJECT_ID: ObjectId = 8;
const LABEL_IMAGE_OBJECT_ID: ObjectId = 9;
const SKELETON_OBJECT_ID: ObjectId = 10;
const SK_ATTACHMENT_OBJECT_ID: ObjectId = 11;
const MATERIAL_OBJECT_ID: ObjectId = 12;
const WORLD_LIGHTING_OBJECT_ID: ObjectId = 13;
const LATE_BRICK_OBJECT_ID: ObjectId = 14;
const OUTER_GROUP_OBJECT_ID: ObjectId = 15;
const SECOND_LABEL_OBJECT_ID: ObjectId = 16;
const GROUP_SK_ATTACHMENT_OBJECT_ID: ObjectId = 17;

// A client's world with one of each kind of drawn object: a still RigidGroup with two parts, a still brick, a pushable ball that
// is this player's character, a mesh, two labels and a label image on still objects, and a part attached to a joint of the
// skeleton the group follows.
fn make_drawing_world() -> anyhow::Result<World> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    add_skeleton(&mut world, SKELETON_OBJECT_ID, ROOT_OBJECT_ID)?;
    add_rigid_group(&mut world, GROUP_OBJECT_ID, ROOT_OBJECT_ID, Vec3::new(0.0, 5.0, 0.0), Some(SKELETON_OBJECT_ID))?;
    add_brick(&mut world, GROUP_BRICK_OBJECT_ID, GROUP_OBJECT_ID, Vec3::new(2.0, 5.0, 0.0), false)?;
    add_cylinder(&mut world, GROUP_CYLINDER_OBJECT_ID, GROUP_OBJECT_ID, Vec3::new(-2.0, 5.0, 0.0), false)?;
    add_brick(&mut world, STANDALONE_BRICK_OBJECT_ID, ROOT_OBJECT_ID, Vec3::new(10.0, 0.0, 0.0), false)?;
    add_ball(&mut world, PUSHABLE_BALL_OBJECT_ID, ROOT_OBJECT_ID, Vec3::new(-10.0, 3.0, 0.0), true)?;
    add_player(&mut world, PLAYER_OBJECT_ID, ROOT_OBJECT_ID, Some(PUSHABLE_BALL_OBJECT_ID))?;
    add_mesh(&mut world, MESH_OBJECT_ID, ROOT_OBJECT_ID)?;
    set_physical_object_id(&mut world, MESH_OBJECT_ID, STANDALONE_BRICK_OBJECT_ID)?;
    add_label(&mut world, LABEL_OBJECT_ID, ROOT_OBJECT_ID)?;
    set_physical_object_id(&mut world, LABEL_OBJECT_ID, STANDALONE_BRICK_OBJECT_ID)?;
    add_label(&mut world, SECOND_LABEL_OBJECT_ID, ROOT_OBJECT_ID)?;
    set_physical_object_id(&mut world, SECOND_LABEL_OBJECT_ID, GROUP_CYLINDER_OBJECT_ID)?;
    add_label_image(&mut world, LABEL_IMAGE_OBJECT_ID, ROOT_OBJECT_ID)?;
    set_physical_object_id(&mut world, LABEL_IMAGE_OBJECT_ID, GROUP_BRICK_OBJECT_ID)?;
    add_sk_attachment(&mut world, SK_ATTACHMENT_OBJECT_ID, GROUP_OBJECT_ID, Some(GROUP_BRICK_OBJECT_ID))?;
    add_material(&mut world, MATERIAL_OBJECT_ID, ROOT_OBJECT_ID)?;
    add_world_lighting(&mut world, WORLD_LIGHTING_OBJECT_ID, ROOT_OBJECT_ID)?;
    Ok(world)
}

fn set_physical_object_id(world: &mut World, object_id: ObjectId, physical_object_id: ObjectId) -> anyhow::Result<()> {
    let physical_object_id = Some(ObjectIdPointer(physical_object_id));
    let info = match world.getObject(object_id)?.info.clone() {
        ObjectInfo::Mesh(mut mesh) => {
            mesh.physicalObjectId = physical_object_id;
            ObjectInfo::Mesh(mesh)
        }
        ObjectInfo::Label(mut label) => {
            label.physicalObjectId = physical_object_id;
            ObjectInfo::Label(label)
        }
        ObjectInfo::LabelImage(mut label_image) => {
            label_image.physicalObjectId = physical_object_id;
            ObjectInfo::LabelImage(label_image)
        }
        _ => anyhow::bail!("object {object_id} is not drawn on another object"),
    };
    world.setObjectInfo(object_id, info)
}

// The snapshot the client built from what changed, and one built from scratch, as JSON so every field is compared.
fn assert_built_render_snapshot_matches_full_rebuild(world: &mut World, render_object_cache: &mut RenderObjectCache, change: &str) -> anyhow::Result<()> {
    let changed_object_ids = world.takeObjectIdsChangedForRender();
    let built_render_object_of_id = render_object_cache.build_render_snapshot(world, changed_object_ids).render_object_of_id;
    let full_render_object_of_id = world.getRenderSnapshot().render_object_of_id;
    let json_of_id = |render_object_of_id: std::collections::HashMap<ObjectId, render_core::renderer_types::RenderObject>| {
        render_object_of_id
            .into_iter()
            .map(|(object_id, render_object)| Ok((object_id, serde_json::to_value(render_object)?)))
            .collect::<anyhow::Result<BTreeMap<ObjectId, serde_json::Value>>>()
    };
    assert_eq!(
        json_of_id(built_render_object_of_id)?,
        json_of_id(full_render_object_of_id)?,
        "the built render snapshot differs from a full rebuild after: {change}"
    );
    Ok(())
}

#[test]
fn a_built_render_snapshot_matches_a_full_rebuild_after_every_kind_of_change() -> anyhow::Result<()> {
    let mut world = make_drawing_world()?;
    let mut render_object_cache = RenderObjectCache::default();
    let mut sim_ms = 0.0;
    let mut step_and_check = |world: &mut World, change: &str| -> anyhow::Result<()> {
        sim_ms += 1000.0 / 60.0;
        world.step_physics(1, 1.0 / 60.0, sim_ms, Some(sim_ms))?;
        assert_built_render_snapshot_matches_full_rebuild(world, &mut render_object_cache, change)
    };
    step_and_check(&mut world, "the first snapshot")?;
    step_and_check(&mut world, "a step where only our character falls, with nothing marked")?;

    let ObjectInfo::Brick(mut brick) = world.getObject(STANDALONE_BRICK_OBJECT_ID)?.info.clone() else {
        anyhow::bail!("expected a brick");
    };
    brick.visualSettings.opacity = 0.5;
    world.setObjectInfo(STANDALONE_BRICK_OBJECT_ID, ObjectInfo::Brick(brick))?;
    step_and_check(&mut world, "making a still brick see-through")?;

    world.forcePhysicsInfo3D(
        GROUP_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(0.0, 8.0, 3.0)),
            ..Default::default()
        },
        false,
    );
    step_and_check(&mut world, "teleporting the still group, which stays Fixed")?;

    world.setNetworkedPhysicsInfo3D(
        STANDALONE_BRICK_OBJECT_ID,
        NetworkedPhysicsInfo3D {
            coords: NetworkedCoords::WorldCoords(physics_info(Vec3::new(12.0, 1.0, 0.0))),
            teleportCount: 7,
        },
    )?;
    step_and_check(&mut world, "a pose from the server that jumps the still brick")?;

    world.setSubitemRelativeLocation3D(
        GROUP_CYLINDER_OBJECT_ID,
        Location3D {
            position: Vec3::new(-3.0, 1.0, 0.0),
            quaternion: Quat::from_rotation_y(0.5),
        },
    )?;
    step_and_check(&mut world, "moving a part inside the still group")?;

    world.setPrimitiveSettings(
        GROUP_CYLINDER_OBJECT_ID,
        PrimitiveSettings::Cylinder(CylinderPrimitiveSettings { radius: 2.0, length: 4.0 }),
        false,
    )?;
    step_and_check(&mut world, "resizing a part")?;

    world.setParent(STANDALONE_BRICK_OBJECT_ID, GROUP_OBJECT_ID)?;
    step_and_check(&mut world, "welding the still brick into the group")?;
    world.setParent(STANDALONE_BRICK_OBJECT_ID, ROOT_OBJECT_ID)?;
    step_and_check(&mut world, "taking the brick back out of the group")?;

    let ObjectInfo::SkAttachment(mut sk_attachment) = world.getObject(SK_ATTACHMENT_OBJECT_ID)?.info.clone() else {
        anyhow::bail!("expected an attachment");
    };
    sk_attachment.jointName = "other_joint".to_string();
    world.setObjectInfo(SK_ATTACHMENT_OBJECT_ID, ObjectInfo::SkAttachment(sk_attachment))?;
    step_and_check(&mut world, "attaching the part to another joint")?;

    world.setObjectInfo(
        GROUP_OBJECT_ID,
        ObjectInfo::RigidGroup(game_types::object_types::RigidGroupInfo { followSkeletonId: None }),
    )?;
    step_and_check(&mut world, "the group no longer following the skeleton")?;

    // An attachment on the group itself only takes effect once a group above it follows a skeleton.
    add_sk_attachment(&mut world, GROUP_SK_ATTACHMENT_OBJECT_ID, ROOT_OBJECT_ID, Some(GROUP_OBJECT_ID))?;
    add_rigid_group(
        &mut world,
        OUTER_GROUP_OBJECT_ID,
        ROOT_OBJECT_ID,
        Vec3::new(0.0, 5.0, -5.0),
        Some(SKELETON_OBJECT_ID),
    )?;
    step_and_check(&mut world, "attaching the group, with no group above it")?;
    world.setParent(GROUP_OBJECT_ID, OUTER_GROUP_OBJECT_ID)?;
    step_and_check(&mut world, "putting the attached group inside a group that follows the skeleton")?;

    set_physical_object_id(&mut world, MESH_OBJECT_ID, GROUP_BRICK_OBJECT_ID)?;
    step_and_check(&mut world, "moving the mesh onto a part")?;

    let ObjectInfo::Label(mut label) = world.getObject(LABEL_OBJECT_ID)?.info.clone() else {
        anyhow::bail!("expected a label");
    };
    label.text = "new words".to_string();
    world.setObjectInfo(LABEL_OBJECT_ID, ObjectInfo::Label(label))?;
    step_and_check(&mut world, "changing what one label says, with letters the other label's atlas didn't have")?;

    world.setPhysicsToggles3D(
        STANDALONE_BRICK_OBJECT_ID,
        SetPhysicsToggles3D {
            isPushable: Some(true),
            ..Default::default()
        },
        false,
    );
    step_and_check(&mut world, "making the still brick pushable")?;
    step_and_check(&mut world, "a step after making it pushable")?;
    world.setPhysicsToggles3D(
        STANDALONE_BRICK_OBJECT_ID,
        SetPhysicsToggles3D {
            isPushable: Some(false),
            ..Default::default()
        },
        false,
    );
    step_and_check(&mut world, "anchoring the brick again")?;

    world.setPhysicsInfo3D(
        STANDALONE_BRICK_OBJECT_ID,
        SetPhysicsInfo3D {
            position: Some(Vec3::new(15.0, 0.0, 0.0)),
            ..Default::default()
        },
        false,
    );
    step_and_check(&mut world, "a smooth move of the brick")?;

    // The label image's object is removed, then a mesh is pointed at an object that doesn't exist yet, which is added after.
    world.removeObject(GROUP_BRICK_OBJECT_ID)?;
    step_and_check(&mut world, "removing the part the label image and mesh were on")?;
    set_physical_object_id(&mut world, MESH_OBJECT_ID, LATE_BRICK_OBJECT_ID)?;
    step_and_check(&mut world, "pointing the mesh at an object that doesn't exist yet")?;
    add_brick(&mut world, LATE_BRICK_OBJECT_ID, ROOT_OBJECT_ID, Vec3::new(0.0, 0.0, 20.0), false)?;
    step_and_check(&mut world, "adding the object the mesh points at")?;

    world.removeObject(GROUP_OBJECT_ID)?;
    step_and_check(&mut world, "removing the whole group")?;
    Ok(())
}
