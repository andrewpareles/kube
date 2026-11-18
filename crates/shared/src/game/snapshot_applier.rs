// Applying what the server changed to a client's world.
//
// A client does not simulate the whole game: the server owns every object it moves, and a client copies those changes
// into its world as they come due. The desktop client and a browser page both do that through this struct, so an object
// arrives, changes and leaves the world the same way on either.

use std::collections::HashSet;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectInfoTrackedFields};

use crate::game::physics::SetPhysicsToggles3D;
use crate::game::world::World;
use crate::protocol::messages::{ForeignPlayerDelta, NetworkedPhysicsInfo3D, ObjectAddDelta, ObjectModifyDelta};

#[derive(Default)]
pub struct SnapshotApplier {
    // A resent add can arrive after the object was removed, since neither order is guaranteed, and it must not come back.
    recently_removed_object_ids: HashSet<ObjectId>,
}

impl SnapshotApplier {
    // Objects a snapshot added, now that their parents are in the world. Returns the scripts among them, because only
    // the caller knows how to start one.
    pub fn apply_adds(&mut self, world: &mut World, valid_adds: Vec<(ObjectId, ObjectAddDelta)>) -> Vec<ObjectId> {
        let mut scripts_added = Vec::new();
        world.refreshAnimationKeyframesOnceAfter(|world| {
            for (object_id, object_add_delta) in valid_adds {
                if self.recently_removed_object_ids.contains(&object_id) {
                    continue;
                }
                if world.hasObject(object_id) {
                    // an add sent again before its ack arrived
                    continue;
                }
                if let ObjectBeingAdded::Script(_) = &object_add_delta.objectBeingAdded {
                    scripts_added.push(object_id);
                }
                world.addObjectWithIdFromDelta(object_id, object_add_delta);
            }
        });
        scripts_added
    }

    // Parent changes are only applied once both objects are in the world, so a whole character can land in one tick.
    pub fn apply_parent_changes(&mut self, world: &mut World, valid_parent_changes: Vec<(ObjectId, ObjectId)>) {
        for (object_id, desired_parent_id) in valid_parent_changes {
            if !world.hasObject(object_id) {
                continue;
            }
            if let Err(error) = world.setParent(object_id, desired_parent_id) {
                log::error!("Could not set parent of {object_id} to {desired_parent_id}: {error}");
            }
        }
    }

    pub fn apply_modifies(&mut self, world: &mut World, modifies: Vec<(ObjectId, ObjectModifyDelta)>) {
        for (object_id, modify_delta) in modifies {
            if !world.hasObject(object_id) {
                continue;
            }
            let ObjectModifyDelta {
                modifiedObjectInfo,
                modifiedPrimitiveSettings,
                modifiedPhysicsToggles3D,
                modifiedPhysicsInfo3D,
                modifiedName,
                ..
            } = modify_delta;

            if let Some(physics_toggles_3d) = modifiedPhysicsToggles3D {
                world.setPhysicsToggles3D(
                    object_id,
                    SetPhysicsToggles3D {
                        isWalkthrough: Some(physics_toggles_3d.isWalkthrough),
                        isPushable: Some(physics_toggles_3d.isPushable),
                        density: Some(physics_toggles_3d.density),
                    },
                    false,
                );
            }

            if let Some(primitive_settings) = modifiedPrimitiveSettings
                && let Err(error) = world.setPrimitiveSettings(object_id, primitive_settings, false)
            {
                log::warn!("Could not set primitive settings on client: {error}");
            }

            if let Some(networked_physics_info_3d) = modifiedPhysicsInfo3D
                && let Err(error) = world.setNetworkedPhysicsInfo3D(object_id, networked_physics_info_3d)
            {
                log::warn!("Could not apply a snapshot's physics on client: {error}");
            }

            if let Some(name) = modifiedName {
                let _ = world.setObjectName(object_id, name);
            }

            let Ok(object_mut) = world.getObjectMut(object_id) else {
                log::error!("Could not modify object {object_id}: it is not in the world");
                continue;
            };
            if let Some(object_info) = modifiedObjectInfo {
                let old_tracked_fields = ObjectInfoTrackedFields::from_object_info(&object_mut.info);
                object_mut.info = object_info;
                let new_tracked_fields = ObjectInfoTrackedFields::from_object_info(&object_mut.info);
                world.afterObjectInfoChange(object_id, old_tracked_fields, new_tracked_fields);
            }
        }
    }

    // Where an object was between the two snapshots around the delayed server time.
    pub fn apply_interpolated_poses(&mut self, world: &mut World, interpolated_poses: Vec<(ObjectId, NetworkedPhysicsInfo3D)>) {
        for (object_id, interpolated_pose) in interpolated_poses {
            if !world.hasObject(object_id) {
                continue;
            }
            if let Err(error) = world.setNetworkedPhysicsInfo3D(object_id, interpolated_pose) {
                log::warn!("Could not apply an interpolated pose on client: {error}");
            }
        }
    }

    // Another player's camera and metadata, which their client sent and the server passed on.
    pub fn apply_foreign_players(&mut self, world: &mut World, foreign_players: Vec<(ObjectId, ForeignPlayerDelta)>) {
        for (player_object_id, foreign_player_delta) in foreign_players {
            if !world.hasObject(player_object_id) {
                continue;
            }
            world.setPlayerCameraOrientation(player_object_id, foreign_player_delta.cameraOrientation);
            world.setPlayerMetadata(player_object_id, foreign_player_delta.playerMetadata);
        }
    }

    pub fn apply_removes(&mut self, world: &mut World, removes: Vec<ObjectId>) {
        for object_id in removes {
            self.recently_removed_object_ids.insert(object_id);
            if !world.hasObject(object_id) {
                // the server removed an object this client already deleted, so there is nothing left to keep deleted
                world.serverObjectIdsDeletedOnClient.remove(&object_id);
                continue;
            }
            let res = world.removeObject(object_id);
            // removeObject remembers it as removed on this client, but the server removed it for real
            world.serverObjectIdsDeletedOnClient.remove(&object_id);
            if let Err(error) = res {
                log::warn!("Could not remove object {object_id} on client: {error}");
            }
        }
    }
}
