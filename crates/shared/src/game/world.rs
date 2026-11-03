use game_types::{
    GameId,
    consts::{DEFAULT_CAMERA_ORIENTATION, SERVER_PLAYER_HISTORY_MS, STORED_WORLD_CURRENT_VERSION},
    data_types::{DataId, DataIds},
    object_types::{
        BallInfo, BrickInfo, CameraLocation, ClockInfo, ClockState, CylinderInfo, InterpolationStyle, LabelImageInfo, LabelInfo, LiveObject, LocalGroupInfo,
        Location3D, MaterialInfo, MeshInfo, MeshMaterialSettings, Movement3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectInfoTrackedFields,
        ObjectKind, ObjectLocation, Orientation3D, PhysicsInfo3D, PhysicsToggles3D, PivotInfo, PlayerInfo, PropertyAnimationInfo, PropertyKeyframeInfo,
        RigidGroupInfo, ScriptInfo, SkAnimationInfo, SkAttachmentInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo, SoundPlayerInfo, TweenInfo,
        WorldLightingInfo,
        player::UserId,
        property_animator::PropertyAnimatorInfo,
        shared::{TOP_CLIENT_OBJECT_ID, euler_degrees_to_quat, is_client_object_id, quat_to_euler_degrees},
        sk_animator::SkAnimatorInfo,
    },
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings, PrimitiveSettings},
    stored_world_type::StoredWorld,
};
use render_core::{
    collect_animation_content::{
        collect_pivot_offsets, collect_property_keyframes, collect_property_overrides, collect_render_sk_animations, collect_sk_keyframes,
        computed_property_animation_of_tween,
    },
    computePropertyValue::{
        ComputedPropertyOverrides, applyLocationPropertyOverridesOfPath, canonicalRotationBaseDegrees, computePropertyOverridesOfObjectIdAtTime,
        getCurrentPropertyValue, isRotationAxisPropertyPath, rotationAxisOfPropertyPath,
    },
    computeSkeletonInfo::computeRenderSkeletonInfo,
    computeSkeletonPose::{SkeletonPose, computeFollowedSkJointMatrix, computeSkeletonPoseAtTime},
    renderer_types::{RenderSkAnimation, RenderSkKeyframe, RenderSkeletonInfo, SkJointToFollow},
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::game::physics::Physics;
use crate::game::{
    interpolate_physics::get_interpolated_pose,
    jsRustConversions::{ValueWithPath, jsonNumber, rustUpdateObject},
    physics::{SetOrForce, SetPhysicsInfo3D, SetPhysicsToggles3D},
    timeline::Timeline,
};
use crate::{
    game::{
        migrateStoredWorld,
        physics::{CastHit, CollisionPairEvent},
    },
    helpers::hashmap_with_reverse::HashMapWithReverse,
    protocol::messages::{NetworkedCoords, NetworkedPhysicsInfo3D, ObjectAddDelta, ObjectModifyDelta, ObjectModifyFlags, someIffTrue},
};
use glam::{Mat4, Quat, Vec3};
use serde::Serialize;
use tokio::sync::{Notify, mpsc};

// This identifies where an editor world is loaded from and where Publish should save it.
#[derive(Clone)]
pub enum WorldSource {
    Supabase { game_id: GameId },
}

#[derive(Clone, Copy)]
pub enum WorldControlMode {
    Server,
    Editor,
    Player(ObjectId), // the object id of the player that controls the world
}
impl WorldControlMode {
    // add/remove callbacks should happen on these
    fn should_call_collision_callbacks(&self) -> bool {
        match self {
            WorldControlMode::Server => true,
            WorldControlMode::Editor => false,
            WorldControlMode::Player(_) => true,
        }
    }

    fn should_call_object_callbacks(&self) -> bool {
        match self {
            WorldControlMode::Server => true,
            WorldControlMode::Editor => false,
            WorldControlMode::Player(_) => true,
        }
    }

    // modified is only used when networking
    pub fn should_mark_objects(&self) -> bool {
        match self {
            WorldControlMode::Server => true,
            WorldControlMode::Editor => true,
            WorldControlMode::Player(_) => false,
        }
    }

    // used when loading the world for the first time - generate new ids or not
    fn trusts_loaded_ids(&self) -> bool {
        match self {
            WorldControlMode::Server => false,
            WorldControlMode::Editor => true,
            WorldControlMode::Player(_) => true,
        }
    }
}

// Which scripts receive each event is decided by the emitter, not this enum:
// - ObjectCreated/ObjectDeleted: BOTH server and client scripts (should_call_object_callbacks). On the client
//   these mean "replicated into / removed from your world". They never fire for the initial world load, only
//   for changes after it (fromStoredWorldString attaches js_callback_inputs_tx after building the objects).
//   Player joins/leaves are these events with type "player".
// - CollisionStarted/CollisionStopped: BOTH server and client scripts (should_call_collision_callbacks).
// - Key/Mouse events: CLIENT scripts only. The world never fires these; the play window's input handler is the only producer.
// - BeforeCharacterStep/AfterCharacterStep: CLIENT scripts only, while the local player has a character. The client play
//   loop runs these right away around each physics step instead of queueing them.
#[derive(Clone, Debug, Serialize)]
pub enum JsCallbackInputs {
    // object_type is captured at deletion time (to_object_kind_name) because the object is already gone when JS runs.
    ObjectCreated {
        object_id: ObjectId,
    },
    ObjectDeleted {
        object_id: ObjectId,
        object_type: String,
    },
    CollisionStarted {
        object_id_1: ObjectId,
        object_id_2: ObjectId,
    },
    CollisionStopped {
        object_id_1: ObjectId,
        object_id_2: ObjectId,
    },
    // key names and button names must match the KeyName/MouseButtonName unions in clientInputService.ts.
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    MouseDown {
        button: String,
    },
    MouseUp {
        button: String,
    },

    GameChatMessageSent {
        userid: UserId,
        username: String,
        message: String,
        time_live_server_ms: u64,
    },

    BeforeCharacterStep {
        character_object_id: ObjectId,
        dt_secs: f32,
    },
    AfterCharacterStep {
        character_object_id: ObjectId,
        dt_secs: f32,
    },
}

// World (stateful, live)
pub struct World {
    pub physics: Physics,
    pub worldControlMode: WorldControlMode,
    // Editors wake their sync loop when dirty objects change; game networking keeps its own send loop.
    pub changes_notify_arc: Option<Arc<Notify>>,
    pub fallback_camera_location: CameraLocation,
    pub js_callback_inputs_tx: Option<mpsc::UnboundedSender<JsCallbackInputs>>, // can send typed JS callback inputs to run using this

    pub objectOfId: HashMap<ObjectId, LiveObject>,
    pub objectIdCounter: ObjectId,
    pub clientObjectIdCounter: ObjectId, // the next id for an object a client makes for itself; see TOP_CLIENT_OBJECT_ID
    pub serverObjectIdsDeletedOnClient: HashSet<ObjectId>,
    pub _objectIdsOfName: HashMapWithReverse<ObjectId, String>, // redundant, helps us look up names efficiently
    pub _kindOfObjectId: HashMapWithReverse<ObjectId, ObjectKind>, // redundant, useful for its reverse map

    playerMetadataOfPlayerObjectId: HashMap<ObjectId, String>, // what each player's client last shared with setLocalPlayerMetadata
    cameraOrientationOfPlayerObjectId: HashMap<ObjectId, Orientation3D>, // where each player's camera last looked, as sent with their character's movement

    // render cache stuff
    pub renderSkeletonInfoArcOfId: HashMap<ObjectId, Arc<RenderSkeletonInfo>>, // redundant from Skeletons, helps us efficiently look at Skeleton stuff we need
    pub renderSkKeyframesOfSkAnimationId: HashMap<ObjectId, Vec<RenderSkKeyframe>>, // redundant from SkAnimation children, avoids rebuilding animation frames while sampling
    pub propertyKeyframesArcOfPropertyAnimationId: HashMap<ObjectId, Arc<Vec<PropertyKeyframeInfo>>>, // redundant from PropertyAnimation children, avoids rebuilding animation frames while sampling
    animationIdsWithStaleKeyframes: Option<HashSet<ObjectId>>, // Some while a change of many objects runs: the animations it touched, rebuilt once when it ends

    pub parentOfId: HashMap<ObjectId, ObjectId>,
    pub _childrenOfId: HashMap<ObjectId, HashSet<ObjectId>>, // redundant, parentOfItem
    pub _rootObjectId: ObjectId,                             // redundant, the only item with parentId=None

    pub _objectsThatPointToId: HashMap<ObjectId, HashSet<ObjectId>>, // redundant, ObjectIdPointer

    // these live on both the server *and* the client, since we can have server and client scripts.
    pub scriptRawCodeOfId: HashMap<ObjectId, String>, // jsruntime reads this, and it doubles as storage
    pub readyToAddScriptIds: HashSet<ObjectId>,       // jsruntime flushes this - "ready to send"
    pub readyToRemoveScriptIds: HashSet<ObjectId>,    // jsruntime flushes this. could be named simpler but named after add

    // these only live on the server, and are flushed. they must have a parent, the root is not sent.
    pub modifiedObjectFlags: HashMap<ObjectId, ObjectModifyFlags>, // changed physics is merged with this last second before sending snapshots
    pub addedObjectIds: HashSet<ObjectId>,
    pub removedObjectIds: HashSet<ObjectId>,
    pub physicalObjectIdsMovedByAnimation: HashSet<ObjectId>, // we later ignore these ids when asking rapier for physics changes

    // Only the editor turns this on, so its undo history can read what changed.
    pub editHistory: Option<WorldEditHistory>,

    // The euler angles an object's rotation had when a PropertyAnimator first took one of its axes over. Only the
    // axes nobody animates are read back out of it; see updateRotationAnimationBases for why they can't come from
    // the object itself. Derived state - the server and every client rebuild it identically, so it is not replicated.
    pub rotationAnimationBaseDegreesOfObjectId: HashMap<ObjectId, Vec3>,

    // The x, y, z degrees last written to each object's rotation; see getObjectRotation and getRotationDegrees. Each machine keeps its own, and they are never saved or sent.
    pub writtenRotationDegreesOfObjectId: HashMap<ObjectId, Vec3>,

    // How many times each object jumped rather than moved. Every pose sent to another machine carries it, and a pose with a new count is jumped to instead of smoothed to.
    pub teleportCountOfObjectId: HashMap<ObjectId, u64>,

    // Objects that jumped since the last physics step. Until that step, every other move or turn they get jumps with them, so moving and
    // turning something in the same tick works in either order.
    pub teleportedObjectIdsSinceStep: HashSet<ObjectId>,

    // The objects whose render object may have changed since a drawing client last took them. Only a drawing client turns this on.
    pub objectIdsChangedForRender: Option<HashSet<ObjectId>>,

    // Server only: the poses each player sent for their character over the last SERVER_PLAYER_HISTORY_MS, at the stamps watchers got.
    pub characterPoseHistoryOfObjectId: HashMap<ObjectId, Timeline<NetworkedPhysicsInfo3D>>,
    // Server only: the metadata each player sent over the last SERVER_PLAYER_HISTORY_MS, at the stamps watchers got.
    pub playerMetadataHistoryOfObjectId: HashMap<ObjectId, Timeline<Option<String>>>,

    // The server and each client keep their own templates, which are never sent anywhere.
    pub templateOfName: HashMap<String, TemplateItem>, // These store the root item for each template.
}

#[derive(Clone, Debug)]
pub struct TemplateItem {
    // The source object id is only used to remap pointers when the template is pasted.
    pub sourceObjectId: ObjectId,
    pub name: String,
    pub objectBeingAdded: ObjectBeingAdded,
    pub scriptRawCode: Option<String>,
    pub children: Vec<TemplateItem>,
}
// ObjectWithChildren
pub struct ObjectWithChildren {
    pub id: ObjectId,
    pub name: String,
    pub parent_id: Option<ObjectId>,
    pub info: ObjectInfo,
    pub physics_info: Option<PhysicsInfo3D>,
    pub children: Vec<ObjectWithChildren>,
}

pub struct WorldDirtyObjectFlags {
    pub addedObjectIds: HashSet<ObjectId>,
    pub modifiedObjectFlags: HashMap<ObjectId, ObjectModifyFlags>,
    pub removedObjectIds: HashSet<ObjectId>,
}

// The objects the three serverMarkObjectAs* functions marked, in the order each was first marked. It also
// holds each part whose place was rewritten because it or an object above it was reparented.
#[derive(Default)]
pub struct WorldEditHistory {
    pub touched_object_ids: Vec<ObjectId>,
    pub _touched_object_id_set: HashSet<ObjectId>, // redundant, touched_object_ids
    pub retracked_parts: Vec<RetrackedPart>,
}

#[derive(Debug)]
pub struct RetrackedPart {
    pub part_id: ObjectId,
    pub reparented_object_id: ObjectId,
    pub place_before: PartPlace,
    pub place_after: PartPlace,
}

// A part of a rigid group is placed relative to its group. A part in no group is placed in the world.
#[derive(Debug)]
pub enum PartPlace {
    InGroup(ObjectId, Location3D),
    Free(PhysicsInfo3D),
}

// TODO: AI-PICKED-VALUE: A world may nest objects 1024 deep, about twice as deep as one model
// import can (a glTF has at most 512 nodes). Loading, copying, and removing a world recurse through it;
// a release build fits 1024 levels of skeleton joints, the costliest kind, in a 2 MiB thread stack and
// overflows near 1450. The request body limit bounds how many objects it holds.
const MAX_WORLD_DEPTH: usize = 1024;

// TODO: AI-PICKED-VALUE: Object names are capped characters so script module paths stay short, networking payloads bounded, etc.
const MAX_OBJECT_NAME_NUM_CHARS: usize = 64;

// ---------------------------- World --------------------------------
impl World {
    // ----------------- Add/Remove Object -----------------

    // run f in this control mode, then put back whatever was set before.
    // note this only changes what World does - Physics keeps the mode it was constructed with.
    pub fn run_f_in_control_mode<T>(&mut self, world_control_mode: WorldControlMode, f: impl FnOnce(&mut World) -> T) -> T {
        let original_world_control_mode = self.worldControlMode;
        self.worldControlMode = world_control_mode;
        let result = f(self);
        self.worldControlMode = original_world_control_mode;
        result
    }

    fn fireJsCallbacks(&self, js_callback_inputs: JsCallbackInputs) {
        let Some(js_callback_inputs_tx) = self.js_callback_inputs_tx.as_ref() else {
            return;
        };
        let _ = js_callback_inputs_tx.send(js_callback_inputs);
    }

    fn fireCollisionStartedJsCallbacks(&self, object_id_1: ObjectId, object_id_2: ObjectId) {
        self.fireJsCallbacks(JsCallbackInputs::CollisionStarted { object_id_1, object_id_2 });
    }

    fn fireCollisionStoppedJsCallbacks(&self, object_id_1: ObjectId, object_id_2: ObjectId) {
        self.fireJsCallbacks(JsCallbackInputs::CollisionStopped { object_id_1, object_id_2 });
    }

    fn normalizedObjectName(name: &str) -> String {
        if name.is_empty() {
            return "-".to_string();
        }

        // replace invalid characters with '-'
        let normalized_name = name
            .chars()
            .map(|character| {
                let is_allowed_symbol = character == '_' || character == '-';
                if character.is_ascii_alphanumeric() || is_allowed_symbol {
                    character
                } else {
                    '-'
                }
            })
            .take(MAX_OBJECT_NAME_NUM_CHARS)
            .collect::<String>();
        normalized_name
    }

    // create a new ObjectInfo out of info. It should have global initialPhysicsInfo3D coords, and "global" scale baked into its info.
    // all the metadata here is separate from the real world. for example, we never make sure the parent actually exists, etc. 100% independent metadata. it must be this way for out-of-order adding of objects.
    // this should really never fail, that's why it's a () and not anyhow::Result<()>.
    fn _addObjectWithId(
        &mut self,
        object_id: ObjectId,
        name: String,
        objectBeingAdded: ObjectBeingAdded,
        parentId: Option<ObjectId>,
        init_subitem_relative_location_3d: Option<Location3D>,
    ) {
        if self.objectOfId.contains_key(&object_id) {
            log::error!("Object id already exists: {}", object_id);
            return;
        }

        // update parent/children tracking
        self._childrenOfId.entry(object_id).or_insert_with(HashSet::new);
        if let Some(parentId) = parentId {
            // if they provided a parentId, add the parent -> child metadata. it's _or_insert in case out of order adds
            self.parentOfId.insert(object_id, parentId);
            let parent_children_entry = self._childrenOfId.entry(parentId).or_insert_with(HashSet::new);
            parent_children_entry.insert(object_id);
        } else {
            self._rootObjectId = object_id;
        }

        // physics
        self.addObjectToPhysicsTracking(object_id, &objectBeingAdded, init_subitem_relative_location_3d.as_ref());

        // script - just set content empty, don't mark as ready to run, that must manually happen in the place where script is added (just clone right now)
        if let ObjectBeingAdded::Script(_) = &objectBeingAdded {
            self.scriptRawCodeOfId.insert(object_id, "".to_string());
        }

        // track object kind
        let mut info = objectBeingAdded.to_object_info();
        if let ObjectInfo::Label(label) = &mut info {
            label.text = LabelInfo::text_without_emoji(&label.text);
        }
        log::info!("!!!!!Adding {:?} {} <- ({:?})", ObjectKind::from_object_info(&info), object_id, parentId);

        let old_tracked_fields = ObjectInfoTrackedFields::from_nonexistent(ObjectKind::from_object_info(&info));
        let new_tracked_fields = ObjectInfoTrackedFields::from_object_info(&info);

        // important to apply change before calling afterObjectChange
        let object_kind = ObjectKind::from_object_info(&info);

        let name = Self::normalizedObjectName(name.as_str());
        self._kindOfObjectId.insert(object_id, object_kind);
        self._objectIdsOfName.insert(object_id, name.clone());
        self.objectOfId.insert(object_id, LiveObject { id: object_id, name, info });

        // every client's camera starts looking the same way, so other machines know where a player looks before their first input arrives
        if object_kind == ObjectKind::Player {
            self.cameraOrientationOfPlayerObjectId.insert(object_id, DEFAULT_CAMERA_ORIENTATION);
        }

        self.afterObjectInfoChange(object_id, old_tracked_fields, new_tracked_fields);

        self.serverMarkObjectAsAdded(object_id);

        // queue the js callbacks when the object is created
        if self.worldControlMode.should_call_object_callbacks() {
            self.fireJsCallbacks(JsCallbackInputs::ObjectCreated { object_id });
        }
    }

    fn _addObjectAutoId(&mut self, name: String, objectBeingAdded: ObjectBeingAdded, parentId: Option<ObjectId>) -> ObjectId {
        let object_id = match self.worldControlMode {
            WorldControlMode::Player(_) => {
                self.clientObjectIdCounter -= 1;
                self.clientObjectIdCounter + 1
            }
            WorldControlMode::Server | WorldControlMode::Editor => {
                self.objectIdCounter += 1;
                self.objectIdCounter
            }
        };
        self._addObjectWithId(object_id, name, objectBeingAdded, parentId, None);
        object_id
    }

    // ----------------- add object fns we can use elsewhere -----------------

    pub fn addObject(&mut self, name: String, objectBeingAdded: ObjectBeingAdded, parentId: ObjectId) -> ObjectId {
        let objectId = self._addObjectAutoId(name, objectBeingAdded, Some(parentId));
        objectId
    }

    pub fn addObjectWithId(&mut self, object_id: ObjectId, name: String, objectBeingAdded: ObjectBeingAdded, parentId: ObjectId) {
        self._addObjectWithId(object_id, name, objectBeingAdded, Some(parentId), None);
    }

    pub fn addObjectWithIdFromDelta(&mut self, object_id: ObjectId, object_add_delta: ObjectAddDelta) {
        // a server object this client deleted stays deleted
        if self.serverObjectIdsDeletedOnClient.contains(&object_id) {
            return;
        }
        // so does a new object the server adds inside one, and recording it keeps anything later added inside it deleted too
        if self.serverObjectIdsDeletedOnClient.contains(&object_add_delta.parentId) {
            self.serverObjectIdsDeletedOnClient.insert(object_id);
            return;
        }
        let ObjectAddDelta {
            name,
            objectBeingAdded,
            parentId,
            initSubitemRelativeLocation3D,
            teleportCount,
        } = object_add_delta;
        self._addObjectWithId(object_id, name, objectBeingAdded, Some(parentId), initSubitemRelativeLocation3D);
        self.teleportCountOfObjectId.insert(object_id, teleportCount);
    }

    // Whether a client's script can delete this object from its own world.
    pub fn canDeleteOnClientOrErr(&self, object_id: ObjectId) -> anyhow::Result<()> {
        let WorldControlMode::Player(own_player_object_id) = self.worldControlMode else {
            return Err(anyhow::anyhow!("canDeleteOnClient: this world is not a client's"));
        };
        // Players belong to the server, and so does this client's own character, so a client can't delete any player or its own
        // character, or anything one sits inside. Other players' characters it can delete for itself.
        let own_character_object_id = self.getPlayerCharacterObject(own_player_object_id);
        let mut protected_object_ids = self
            ._kindOfObjectId
            .get_keys(&ObjectKind::Player)
            .into_iter()
            .flatten()
            .copied()
            .chain(own_character_object_id);

        if protected_object_ids.any(|protected_object_id| self.isAncestor(object_id, protected_object_id)) {
            return Err(anyhow::anyhow!("Only the server can delete a player or your own character"));
        }
        Ok(())
    }

    // is self or ancestor
    fn isAncestor(&self, object_id: ObjectId, check_if_ancestor: ObjectId) -> bool {
        let mut current_object_id = Some(check_if_ancestor);
        while let Some(checked_object_id) = current_object_id {
            if checked_object_id == object_id {
                return true;
            }
            current_object_id = self.parentOfId.get(&checked_object_id).copied();
        }
        false
    }

    // remove object and all its children.
    pub fn removeObject(&mut self, objectId: ObjectId) -> anyhow::Result<()> {
        // if the object was already removed, we should not do anything
        if !self.objectOfId.contains_key(&objectId) {
            return Ok(());
        }

        // a client remembers the server's objects it removes, so they stay removed when the server sends them again
        if let WorldControlMode::Player(_) = self.worldControlMode
            && !is_client_object_id(objectId)
        {
            self.serverObjectIdsDeletedOnClient.insert(objectId);
        }

        let skeleton_id = self.getSkeletonIdOfSkJoint(objectId);

        log::info!(
            "!!!!!Removing {:?} {:?}",
            ObjectKind::from_object_info(&self.getObject(objectId)?.info),
            objectId
        );

        // when the server removes a player, also remove its character (clients hear about both removals via snapshots)
        if matches!(self.worldControlMode, WorldControlMode::Server) {
            if let ObjectInfo::Player(player) = &self.getObject(objectId)?.info {
                // remove the player's character object
                if let Some(characterObjectId) = player.characterObjectId {
                    self.removeObject(characterObjectId.into())?;
                }
            }
        }

        // if anything points to it, update that object so it no longer points to it
        // only needs to be called when we remove an object, because it accounts for (DELETE).x = B, while other places always have A.x = B/(DELETE)/etc
        self.clearPointersToItem(objectId);
        self.playerMetadataOfPlayerObjectId.remove(&objectId);
        self.cameraOrientationOfPlayerObjectId.remove(&objectId);

        // update parent
        self.clearParent(objectId);

        // update _children
        let child_ids = self._childrenOfId.remove(&objectId);

        // TODO: distinguish between server and client scripts
        if let Some(ObjectKind::Script) = self._kindOfObjectId.get(&objectId) {
            match self.worldControlMode {
                WorldControlMode::Server | WorldControlMode::Player(_) => {
                    self.readyToRemoveScriptIds.insert(objectId);
                }
                WorldControlMode::Editor => {}
            }
        }

        let object = self.getObject(objectId)?;
        let kind = ObjectKind::from_object_info(&object.info);
        let old_tracked_fields = ObjectInfoTrackedFields::from_object_info(&object.info);

        // children recurse through removeObject below, so every deleted descendant fires its own event
        if self.worldControlMode.should_call_object_callbacks() {
            self.fireJsCallbacks(JsCallbackInputs::ObjectDeleted {
                object_id: objectId,
                object_type: kind.to_object_kind_name().to_string(),
            });
        }

        // remove object from physics
        self.physics.stopTrackingObject(objectId);

        // remove from object kind tracking
        self._kindOfObjectId.remove(&objectId);

        // remove object
        let new_tracked_fields = ObjectInfoTrackedFields::from_nonexistent(kind);
        self.afterObjectInfoChange(objectId, old_tracked_fields, new_tracked_fields);
        self.serverMarkObjectAsRemoved(objectId);
        if kind == ObjectKind::Script {
            self.scriptRawCodeOfId.remove(&objectId);
        }
        self._objectIdsOfName.remove(&objectId);
        self.renderSkeletonInfoArcOfId.remove(&objectId);
        self.renderSkKeyframesOfSkAnimationId.remove(&objectId);
        self.propertyKeyframesArcOfPropertyAnimationId.remove(&objectId);
        self.writtenRotationDegreesOfObjectId.remove(&objectId);
        self.teleportCountOfObjectId.remove(&objectId);
        self.teleportedObjectIdsSinceStep.remove(&objectId);
        self.characterPoseHistoryOfObjectId.remove(&objectId);
        self.playerMetadataHistoryOfObjectId.remove(&objectId);

        self.objectOfId.remove(&objectId);

        // remove all children now
        if let Some(child_ids) = child_ids {
            for child_id in child_ids {
                let res = self.removeObject(child_id);
                if let Err(e) = res {
                    log::warn!("removeObject: Error removing child object: {}", e);
                }
            }
        }

        if let Some(skeleton_id) = skeleton_id {
            self.refreshRenderSkeletonInfoArcOfId(skeleton_id);
        }

        // return
        Ok(())
    }

    // ----------------- Change Object In-Place - afterObjectChange + tons of Helpers -----------------

    fn clearParent(&mut self, objectId: ObjectId) {
        if let Some(parentId) = self.parentOfId.remove(&objectId) {
            if let Some(children) = self._childrenOfId.get_mut(&parentId) {
                children.remove(&objectId);
            }
            self.refreshRenderSkKeyframes(parentId);
            self.refreshPropertyKeyframes(parentId);
        }
    }

    pub fn canAddParent(&self, object_id: ObjectId, desired_parent_id: ObjectId) -> bool {
        // trace up from object_id->desired_parent_id->...->root, making sure we don't get a cycle (aka visit same thing twice)
        let mut visited_object_ids = HashSet::new();
        visited_object_ids.insert(object_id);

        let mut curr = desired_parent_id;
        loop {
            if curr == self.getRootObjectId() {
                return true; // if we reached the root, we're good, change is ready
            }

            if !self.hasObject(curr) {
                return false; // parent not found
            }
            let had_element = visited_object_ids.insert(curr);
            if !had_element {
                return false; // visited same object twice - cycle
            }

            // get next parent
            let next_parent = self.parentOfId.get(&curr).copied();
            if let Some(next_parent_id) = next_parent {
                curr = next_parent_id;
            } else {
                return false; // parent not found
            }
        }
    }

    // clear the field that points to object_id off of the pointer_object_id object
    pub fn clearPointerToItem(&mut self, object_id: ObjectId, pointer_object_id: ObjectId) {
        let Some(pointer_obj) = self.objectOfId.get_mut(&pointer_object_id) else {
            return;
        };
        let old_tracked_fields = ObjectInfoTrackedFields::from_object_info(&pointer_obj.info);
        match &mut pointer_obj.info {
            ObjectInfo::Ball(info) => {
                if let Some(material_settings_3d) = info.material.as_ref() {
                    let pointer_id: ObjectId = material_settings_3d.materialInfoId.into();
                    if pointer_id == object_id {
                        info.material = None;
                    }
                }
            }
            ObjectInfo::Brick(info) => {
                if let Some(material_settings_3d) = info.material.as_ref() {
                    let pointer_id: ObjectId = material_settings_3d.materialInfoId.into();
                    if pointer_id == object_id {
                        info.material = None;
                    }
                }
            }
            ObjectInfo::Cylinder(info) => {
                if let Some(material_settings_3d) = info.material.as_ref() {
                    let pointer_id: ObjectId = material_settings_3d.materialInfoId.into();
                    if pointer_id == object_id {
                        info.material = None;
                    }
                }
            }
            ObjectInfo::Label(info) => {
                if let Some(pointer_id) = info.physicalObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.physicalObjectId = None;
                    }
                }
            }
            ObjectInfo::LabelImage(info) => {
                if let Some(pointer_id) = info.physicalObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.physicalObjectId = None;
                    }
                }
            }
            ObjectInfo::Pivot(info) => {
                if let Some(pointer_id) = info.physicalObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.physicalObjectId = None;
                    }
                }
            }
            ObjectInfo::Mesh(info) => {
                if let Some(pointer_id) = info.followSkeletonId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.followSkeletonId = None;
                    }
                }
                if let Some(pointer_id) = info.physicalObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.physicalObjectId = None;
                    }
                }
                if let Some(mesh_material_settings) = info.material.as_ref() {
                    let pointer_id: ObjectId = mesh_material_settings.materialInfoId.into();
                    if pointer_id == object_id {
                        info.material = None;
                    }
                }
            }
            ObjectInfo::RigidGroup(info) => {
                if let Some(pointer_id) = info.followSkeletonId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.followSkeletonId = None;
                    }
                }
            }
            ObjectInfo::Player(info) => {
                if let Some(pointer_id) = info.headObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.headObjectId = None;
                    }
                }
                if let Some(pointer_id) = info.characterObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.characterObjectId = None;
                    }
                }
            }
            ObjectInfo::SoundPlayer(info) => {
                if let Some(pointer_id) = info.clockId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.clockId = None;
                    }
                }
            }
            ObjectInfo::SkAnimator(info) => {
                if let Some(pointer_id) = info.skeletonId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.skeletonId = None;
                    }
                }
                if let Some(pointer_id) = info.clockId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.clockId = None;
                    }
                }
                if let Some(pointer_id) = info.skAnimationId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.skAnimationId = None;
                    }
                }
            }
            ObjectInfo::SkAttachment(info) => {
                if let Some(pointer_id) = info.objectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.objectId = None;
                    }
                }
            }
            ObjectInfo::PropertyAnimator(info) => {
                if let Some(pointer_id) = info.targetObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.targetObjectId = None;
                    }
                }
                if let Some(pointer_id) = info.clockId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.clockId = None;
                    }
                }
                if let Some(pointer_id) = info.propertyAnimationId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.propertyAnimationId = None;
                    }
                }
            }
            ObjectInfo::Tween(info) => {
                if let Some(pointer_id) = info.targetObjectId.as_ref() {
                    let pointer_id: ObjectId = pointer_id.into();
                    if pointer_id == object_id {
                        info.targetObjectId = None;
                    }
                }
            }
            _ => {}
        }
        let new_tracked_fields = ObjectInfoTrackedFields::from_object_info(&pointer_obj.info);
        self.afterObjectInfoChange(pointer_object_id, old_tracked_fields, new_tracked_fields);
        self.serverMarkObjectAsModified(pointer_object_id, ObjectModifyFlags::from_only_object_info());
    }

    // finds all the objects that point to object_id and sets those fields to None. must only be called internally and used with afterObjectChange.
    fn clearPointersToItem(&mut self, object_id: ObjectId) {
        // get all objects that point to this object
        let Some(pointer_object_ids) = self._objectsThatPointToId.get(&object_id) else {
            return;
        };
        let pointer_object_ids = pointer_object_ids.iter().copied().collect::<Vec<ObjectId>>();
        for pointer_object_id in pointer_object_ids {
            self.clearPointerToItem(object_id, pointer_object_id);
        }
    }

    fn setPointerToItem(&mut self, oldPointerId: Option<ObjectId>, newPointerId: Option<ObjectId>, pointerObjectId: ObjectId) {
        // remove pointerObjectId as the object that points to the old id
        if let Some(oldId) = oldPointerId {
            if let Some(pointers) = self._objectsThatPointToId.get_mut(&oldId) {
                pointers.remove(&pointerObjectId);
            }
        }
        // add pointerObjectId as the object that points to the new id
        match newPointerId {
            Some(newPointerId) => self
                ._objectsThatPointToId
                .entry(newPointerId)
                .or_insert_with(HashSet::new)
                .insert(pointerObjectId),
            None => false,
        };
    }

    // Whether this machine created the object: the server creates every object outside the client id range, and a client
    // creates only the ones inside it.
    fn isObjectCreatedByUs(&self, object_id: ObjectId) -> bool {
        match self.worldControlMode {
            WorldControlMode::Server | WorldControlMode::Editor => !is_client_object_id(object_id),
            WorldControlMode::Player(_) => is_client_object_id(object_id),
        }
    }

    // this is independent of whether it is created by us
    fn shouldObjectBeControlledByUs(&self, object_id: ObjectId) -> bool {
        match &self.worldControlMode {
            // location = player's computer
            // return whether controlled_object_id equals object_id
            WorldControlMode::Player(player_obj_id) => {
                // a client also controls the objects it made for itself
                if is_client_object_id(object_id) {
                    return true;
                }
                let Some(character_object) = self.getPlayerCharacterObject(*player_obj_id) else {
                    return false;
                };
                let objIsPlayersCharacter = character_object == object_id;
                return objIsPlayersCharacter;
            }
            // location = server
            // everything should be controlled except object_ids controlled by any player
            WorldControlMode::Server => {
                let Some(player_object_ids) = self._kindOfObjectId.get_keys(&ObjectKind::Player) else {
                    return true;
                };
                for &player_object_id in player_object_ids {
                    let Some(character_object) = self.getPlayerCharacterObject(player_object_id) else {
                        continue;
                    };
                    let objIsPlayersCharacter = character_object == object_id;
                    if objIsPlayersCharacter {
                        return false;
                    }
                }
                return true;
            }
            // location = editor
            // trivially true
            WorldControlMode::Editor => {
                return true;
            }
        }
    }

    // always call this after changing an object in-place, regardless of server or client.
    // be sure to make the change/add/remove to the object *before* calling this.
    // this fn keeps pointer invariants and mesh data up to date.
    // it's fine to have recursive sandwiches that call this fn.
    // when we update any object types, we need to make sure they are accounted for here! don't forget any.
    pub fn afterObjectInfoChange(&mut self, objectId: ObjectId, old: ObjectInfoTrackedFields, new: ObjectInfoTrackedFields) {
        self.markObjectChangedForRender(objectId);

        // helper type + fn
        type O = ObjectInfoTrackedFields;
        fn map_option<T, F, I>(option: &Option<T>, f: F) -> Option<ObjectId>
        where
            F: FnOnce(&T) -> Option<I>,
            I: Into<ObjectId>,
        {
            option.as_ref().and_then(|s| f(s)).map(Into::into)
        }

        match (&old, &new) {
            (O::Ball(old), O::Ball(new)) => {
                let old_material_info_id = map_option(old, |s| s.material.map(|settings| settings.materialInfoId));
                let new_material_info_id = map_option(new, |s| s.material.map(|settings| settings.materialInfoId));
                if old_material_info_id != new_material_info_id {
                    self.setPointerToItem(old_material_info_id, new_material_info_id, objectId);
                }
            }
            (O::SkJoint(), O::SkJoint()) => {}
            (O::Skeleton(), O::Skeleton()) => {}
            (O::Brick(old), O::Brick(new)) => {
                let old_material_info_id = map_option(old, |s| s.material.map(|settings| settings.materialInfoId));
                let new_material_info_id = map_option(new, |s| s.material.map(|settings| settings.materialInfoId));
                if old_material_info_id != new_material_info_id {
                    self.setPointerToItem(old_material_info_id, new_material_info_id, objectId);
                }
            }
            (O::Cylinder(old), O::Cylinder(new)) => {
                let old_material_info_id = map_option(old, |s| s.material.map(|settings| settings.materialInfoId));
                let new_material_info_id = map_option(new, |s| s.material.map(|settings| settings.materialInfoId));
                if old_material_info_id != new_material_info_id {
                    self.setPointerToItem(old_material_info_id, new_material_info_id, objectId);
                }
            }
            (O::Clock(), O::Clock()) => {}
            (O::LocalGroup(), O::LocalGroup()) => {}
            (O::Material(), O::Material()) => {}
            (O::Label(old), O::Label(new)) => {
                let old_physical_object_id = map_option(old, |s| s.physicalObjectId);
                let new_physical_object_id = map_option(new, |s| s.physicalObjectId);
                if old_physical_object_id != new_physical_object_id {
                    self.setPointerToItem(old_physical_object_id, new_physical_object_id, objectId);
                }
            }
            (O::LabelImage(old), O::LabelImage(new)) => {
                let old_physical_object_id = map_option(old, |s| s.physicalObjectId);
                let new_physical_object_id = map_option(new, |s| s.physicalObjectId);
                if old_physical_object_id != new_physical_object_id {
                    self.setPointerToItem(old_physical_object_id, new_physical_object_id, objectId);
                }
            }
            (O::Pivot(old), O::Pivot(new)) => {
                let old_physical_object_id = map_option(old, |s| s.physicalObjectId);
                let new_physical_object_id = map_option(new, |s| s.physicalObjectId);
                if old_physical_object_id != new_physical_object_id {
                    self.setPointerToItem(old_physical_object_id, new_physical_object_id, objectId);
                }
            }
            (O::Mesh(old), O::Mesh(new)) => {
                let old_follow_skeleton_id = map_option(old, |s| s.followSkeletonId);
                let new_follow_skeleton_id = map_option(new, |s| s.followSkeletonId);
                if old_follow_skeleton_id != new_follow_skeleton_id {
                    self.setPointerToItem(old_follow_skeleton_id, new_follow_skeleton_id, objectId);
                }
                let old_material_info_id = map_option(old, |s| s.material.map(|mesh_material_settings| mesh_material_settings.materialInfoId));
                let new_material_info_id = map_option(new, |s| s.material.map(|mesh_material_settings| mesh_material_settings.materialInfoId));
                if old_material_info_id != new_material_info_id {
                    self.setPointerToItem(old_material_info_id, new_material_info_id, objectId);
                }
                let old_physical_object_id = map_option(old, |s| s.physicalObjectId);
                let new_physical_object_id = map_option(new, |s| s.physicalObjectId);
                if old_physical_object_id != new_physical_object_id {
                    self.setPointerToItem(old_physical_object_id, new_physical_object_id, objectId);
                }
            }
            (O::Player(old), O::Player(new)) => {
                let old_head_object_id = map_option(old, |s| s.headObjectId);
                let new_head_object_id = map_option(new, |s| s.headObjectId);
                if old_head_object_id != new_head_object_id {
                    self.setPointerToItem(old_head_object_id, new_head_object_id, objectId);
                }
                let old_character_object_id = map_option(old, |s| s.characterObjectId);
                let new_character_object_id = map_option(new, |s| s.characterObjectId);
                if old_character_object_id != new_character_object_id {
                    self.setPointerToItem(old_character_object_id, new_character_object_id, objectId);
                    // update physics for the old_character_object_id and new_character_object_id
                    if let Some(old_character_object_id) = old_character_object_id {
                        let isControlledByUs = self.shouldObjectBeControlledByUs(old_character_object_id); // should be true on server
                        self.physics.updateObjectIsControlledByUs(old_character_object_id, isControlledByUs);

                        // mark old obj position dirty for good measure so if the foreign character delta (which is best effort) didnt go through to all clients, reliably transmit it now
                        self.serverMarkObjectAsModified(old_character_object_id, ObjectModifyFlags::from_only_physics_info3D());
                    }
                    if let Some(new_character_object_id) = new_character_object_id {
                        let isControlledByUs = self.shouldObjectBeControlledByUs(new_character_object_id); // should be false on server
                        self.physics.updateObjectIsControlledByUs(new_character_object_id, isControlledByUs);

                        // mark obj position dirty so client who controls it will be ignored till they ACK its starter position
                        self.serverMarkObjectAsModified(new_character_object_id, ObjectModifyFlags::from_only_physics_info3D());
                    }
                }
            }
            (O::RigidGroup(old), O::RigidGroup(new)) => {
                let old_follow_skeleton_id = map_option(old, |s| s.followSkeletonId);
                let new_follow_skeleton_id = map_option(new, |s| s.followSkeletonId);
                if old_follow_skeleton_id != new_follow_skeleton_id {
                    self.setPointerToItem(old_follow_skeleton_id, new_follow_skeleton_id, objectId);
                }
            }
            (O::Script(), O::Script()) => {}
            (O::SoundPlayer(old), O::SoundPlayer(new)) => {
                let old_clock_id = map_option(old, |s| s.clockId);
                let new_clock_id = map_option(new, |s| s.clockId);
                if old_clock_id != new_clock_id {
                    self.setPointerToItem(old_clock_id, new_clock_id, objectId);
                }
            }
            (O::SkAnimation(), O::SkAnimation()) => {}
            (O::SkAttachment(old), O::SkAttachment(new)) => {
                let old_object_id = map_option(old, |s| s.objectId);
                let new_object_id = map_option(new, |s| s.objectId);
                if old_object_id != new_object_id {
                    self.setPointerToItem(old_object_id, new_object_id, objectId);
                }
            }
            (O::SkKeyframe(), O::SkKeyframe()) => {}
            (O::SkKeyframePose(), O::SkKeyframePose()) => {}
            (O::SkAnimator(old), O::SkAnimator(new)) => {
                let old_skeleton_id = map_option(old, |s| s.skeletonId);
                let new_skeleton_id = map_option(new, |s| s.skeletonId);
                if old_skeleton_id != new_skeleton_id {
                    self.setPointerToItem(old_skeleton_id, new_skeleton_id, objectId);
                }
                let old_clock_id = map_option(old, |s| s.clockId);
                let new_clock_id = map_option(new, |s| s.clockId);
                if old_clock_id != new_clock_id {
                    self.setPointerToItem(old_clock_id, new_clock_id, objectId);
                }
                let old_sk_animation_id = map_option(old, |s| s.skAnimationId);
                let new_sk_animation_id = map_option(new, |s| s.skAnimationId);
                if old_sk_animation_id != new_sk_animation_id {
                    self.setPointerToItem(old_sk_animation_id, new_sk_animation_id, objectId);
                }
            }
            (O::PropertyAnimation(), O::PropertyAnimation()) => {}
            (O::PropertyKeyframe(), O::PropertyKeyframe()) => {}
            (O::PropertyAnimator(old), O::PropertyAnimator(new)) => {
                let old_target_object_id = map_option(old, |s| s.targetObjectId);
                let new_target_object_id = map_option(new, |s| s.targetObjectId);
                if old_target_object_id != new_target_object_id {
                    self.setPointerToItem(old_target_object_id, new_target_object_id, objectId);
                }
                let old_clock_id = map_option(old, |s| s.clockId);
                let new_clock_id = map_option(new, |s| s.clockId);
                if old_clock_id != new_clock_id {
                    self.setPointerToItem(old_clock_id, new_clock_id, objectId);
                }
                let old_property_animation_id = map_option(old, |s| s.propertyAnimationId);
                let new_property_animation_id = map_option(new, |s| s.propertyAnimationId);
                if old_property_animation_id != new_property_animation_id {
                    self.setPointerToItem(old_property_animation_id, new_property_animation_id, objectId);
                }
            }
            (O::Tween(old), O::Tween(new)) => {
                let old_target_object_id = map_option(old, |s| s.targetObjectId);
                let new_target_object_id = map_option(new, |s| s.targetObjectId);
                if old_target_object_id != new_target_object_id {
                    self.setPointerToItem(old_target_object_id, new_target_object_id, objectId);
                }
            }
            (O::WorldLighting(), O::WorldLighting()) => {}
            _ => {
                log::error!("Object type somehow changed in afterObjectInfoChange for object {}", objectId,);
            }
        }

        self.refreshRenderSkeletonInfoArcForObject(objectId);
        self.refreshRenderSkKeyframes(objectId);
        self.refreshPropertyKeyframes(objectId);
    }

    // Editor saves and AI edits mark the script dirty so its source accompanies the next editor batch.
    pub fn setScriptRawCode(&mut self, object_id: ObjectId, raw_code: String) {
        self.scriptRawCodeOfId.insert(object_id, raw_code);
        self.serverMarkObjectAsModified(object_id, ObjectModifyFlags::default());
    }

    // always be sure this is called after the script's addObject is called on the server+editor, and after its contents are received on the client (client can't do addObject)
    pub fn markScriptAsReadyToRun(&mut self, object_id: ObjectId, raw_code: String) {
        // all scripts run everywhere. it is *crucial* that transpileUseServer works properly.
        self.scriptRawCodeOfId.insert(object_id, raw_code);
        self.readyToAddScriptIds.insert(object_id);
    }

    // one of these 3 functions should always accompany a change to an object on the server.
    // always be sure this is called when you call any of these on the server: afterObjectChange, self.set<anything>, etc
    pub fn serverMarkObjectAsModified(&mut self, objectId: ObjectId, objectModifyFlags: ObjectModifyFlags) {
        if !self.worldControlMode.should_mark_objects() {
            return;
        }

        // mark object as modified
        self.modifiedObjectFlags.entry(objectId).or_default().merge_with(&objectModifyFlags);
        self.afterObjectMarked(objectId);
    }

    fn serverMarkObjectAsAdded(&mut self, objectId: ObjectId) {
        if !self.worldControlMode.should_mark_objects() {
            return;
        }
        self.addedObjectIds.insert(objectId);
        self.afterObjectMarked(objectId);
    }

    fn serverMarkObjectAsRemoved(&mut self, objectId: ObjectId) {
        if !self.worldControlMode.should_mark_objects() {
            return;
        }
        self.removedObjectIds.insert(objectId);
        self.afterObjectMarked(objectId);
    }

    // Every change to an object has to come through here, or a drawing client keeps drawing it as it was. A change can also change
    // how everything under the object is drawn (parts follow their group, and attached parts follow the skeleton of the highest group
    // above them), and anything pointing at it (a mesh or label is only drawn while the object it's on exists).
    fn markObjectChangedForRender(&mut self, objectId: ObjectId) {
        if self.objectIdsChangedForRender.is_none() {
            return;
        }
        let mut object_ids = Vec::new();
        self.collectAllDescendants(&mut object_ids, objectId, true, &|_| true);
        object_ids.extend(self._objectsThatPointToId.get(&objectId).into_iter().flatten().copied());
        if let Some(object_ids_changed_for_render) = &mut self.objectIdsChangedForRender {
            object_ids_changed_for_render.extend(object_ids);
        }
    }

    // The objects whose render object may have changed since the last call: the marked ones, and everything on a body that can move.
    // The first call turns marking on.
    pub fn takeObjectIdsChangedForRender(&mut self) -> HashSet<ObjectId> {
        let mut object_ids = self.objectIdsChangedForRender.replace(HashSet::new()).unwrap_or_default();
        object_ids.extend(self.physics.getObjectIdsOnNonFixedBodies());
        object_ids
    }

    fn afterObjectMarked(&mut self, objectId: ObjectId) {
        self.recordTouchedObjectIdInEditHistory(objectId);
        if let Some(notify_arc) = &self.changes_notify_arc {
            notify_arc.notify_one();
        }
    }

    // ---------------------------- Edit history --------------------------------

    pub fn readPartPlace(&self, object_id: ObjectId) -> Option<PartPlace> {
        match (
            self.physics.getSubitemRigidGroupId(object_id),
            self.physics.getSubitemRelativeLocation3D(object_id),
        ) {
            (Some(group_id), Some(relative_location_3d)) => Some(PartPlace::InGroup(group_id, relative_location_3d)),
            _ => self.getObjectPhysicsInfo3D(object_id).map(PartPlace::Free),
        }
    }

    fn recordTouchedObjectIdInEditHistory(&mut self, objectId: ObjectId) {
        let Some(editHistory) = &mut self.editHistory else {
            return;
        };
        if editHistory._touched_object_id_set.insert(objectId) {
            editHistory.touched_object_ids.push(objectId);
        }
    }

    // The places of these parts before a refresh rewrites them, when the edit history is on.
    fn readPartPlacesForEditHistory(&self, part_ids: &[ObjectId]) -> Vec<(ObjectId, PartPlace)> {
        if self.editHistory.is_none() {
            return Vec::new();
        }
        part_ids.iter().filter_map(|part_id| Some((*part_id, self.readPartPlace(*part_id)?))).collect()
    }

    // The refresh rewrites parts' places without marking them, so the edit history records them here.
    fn recordRetrackedPartsInEditHistory(&mut self, reparented_object_id: ObjectId, places_before: Vec<(ObjectId, PartPlace)>) {
        let retracked_parts: Vec<RetrackedPart> = places_before
            .into_iter()
            .filter_map(|(part_id, place_before)| {
                Some(RetrackedPart {
                    part_id,
                    reparented_object_id,
                    place_before,
                    place_after: self.readPartPlace(part_id)?,
                })
            })
            .collect();
        if let Some(editHistory) = &mut self.editHistory {
            editHistory.retracked_parts.extend(retracked_parts);
        }
    }

    // ----------------- Getters -----------------
    // object get/set/add/remove
    pub fn getObject(&self, object_id: ObjectId) -> anyhow::Result<&LiveObject> {
        self.objectOfId.get(&object_id).ok_or_else(|| anyhow::anyhow!("getObject: Object not found"))
    }

    pub fn hasObject(&self, object_id: ObjectId) -> bool {
        self.objectOfId.contains_key(&object_id)
    }

    pub fn getObjectMut(&mut self, object_id: ObjectId) -> anyhow::Result<&mut LiveObject> {
        self.objectOfId
            .get_mut(&object_id)
            .ok_or_else(|| anyhow::anyhow!("getObjectMut: Object not found"))
    }

    pub fn getRootObjectId(&self) -> ObjectId {
        self._rootObjectId
    }

    pub fn getAllObjects(&self) -> Vec<(ObjectId, &LiveObject)> {
        self.objectOfId.iter().map(|(id, obj)| (*id, obj)).collect()
    }

    pub fn getObjectIdsByName(&self, name: &str) -> Vec<ObjectId> {
        match self._objectIdsOfName.get_keys(&name.to_string()) {
            Some(object_ids) => object_ids.iter().copied().collect(),
            None => Vec::new(),
        }
    }

    pub fn getRenderSkeletonInfoArc(&self, object_id: ObjectId) -> Option<&Arc<RenderSkeletonInfo>> {
        self.renderSkeletonInfoArcOfId.get(&object_id)
    }

    pub fn getDirectChildrenIds(&self, objectId: ObjectId) -> anyhow::Result<&HashSet<ObjectId>> {
        let children = self._childrenOfId.get(&objectId).ok_or_else(|| anyhow::anyhow!("Children not found"))?;
        Ok(children)
    }

    // This builds the slash-separated object path from the root, which is what the
    // script runtime and the editor both use when they refer to a script by path.
    pub fn getObjectNamePath(&self, object_id: ObjectId) -> anyhow::Result<String> {
        let mut name_path_reversed = Vec::new();
        let mut current_object_id = object_id;
        loop {
            let object = self.getObject(current_object_id)?;
            name_path_reversed.push(object.name.to_string());
            let Some(parent_id) = self.parentOfId.get(&current_object_id) else {
                break;
            };
            current_object_id = *parent_id;
        }

        name_path_reversed.reverse();
        Ok(name_path_reversed.join("/"))
    }

    // this should be the same exact statement as "player.characterObjectId == object_id" (where the player = the player_object_id that controls this world)
    // also, typically should be the same as shouldBeControlledByUs except for very recent changes
    pub fn getIsObjectControlledByUs(&self, object_id: ObjectId) -> Option<bool> {
        self.physics.isControlledByUs(object_id)
    }

    pub fn getObjectPrimitiveSettings(&self, objectId: ObjectId) -> Option<PrimitiveSettings> {
        self.physics.getPrimitiveSettings(objectId)
    }

    pub fn getObjectPhysicsInfo3D(&self, objectId: ObjectId) -> Option<PhysicsInfo3D> {
        self.physics.getObjectPhysicsInfo3D(objectId)
    }

    pub fn getObjectPhysicsToggles3D(&self, objectId: ObjectId) -> Option<PhysicsToggles3D> {
        self.physics.getObjectPhysicsToggles3D(objectId)
    }

    pub fn getIsObjectTransparent(&self, objectId: ObjectId) -> anyhow::Result<bool> {
        let obj = self.getObject(objectId)?;
        let isTransparent = match &obj.info {
            ObjectInfo::Ball(ball_info) => ball_info.visualSettings.opacity != 1.0,
            ObjectInfo::Brick(brick_info) => brick_info.visualSettings.opacity != 1.0,
            ObjectInfo::Cylinder(cylinder_info) => cylinder_info.visualSettings.opacity != 1.0,
            _ => false,
        };
        Ok(isTransparent)
    }

    pub fn collectAllMeshDataIds(&self) -> HashSet<DataId> {
        let mut data_ids: HashSet<DataId> = HashSet::new();
        let Some(object_ids) = self._kindOfObjectId.get_keys(&ObjectKind::Mesh) else {
            return data_ids;
        };
        for object_id in object_ids {
            let Some(object) = self.objectOfId.get(object_id) else {
                continue;
            };
            let ObjectInfo::Mesh(info) = &object.info else {
                continue;
            };
            // insert
            data_ids.insert(info.meshDataId);
        }
        data_ids
    }

    // Every data id that an object in the world uses, whether or not anything draws or plays it.
    pub fn collectAllDataIds(&self) -> DataIds {
        let mut data_ids = DataIds::default();
        for object in self.objectOfId.values() {
            data_ids.insert_data_ids_of_object(&object.info);
        }
        data_ids
    }

    // ----------------- Setters -----------------
    // Change Object fields which World / Physics controls

    pub fn setObjectName(&mut self, object_id: ObjectId, name: String) -> anyhow::Result<()> {
        let normalized_name = Self::normalizedObjectName(name.as_str());
        let object = self.getObjectMut(object_id)?;
        object.name = normalized_name.clone();

        self._objectIdsOfName.insert(object_id, normalized_name);
        self.refreshRenderSkeletonInfoArcForObject(object_id);

        self.serverMarkObjectAsModified(
            object_id,
            ObjectModifyFlags {
                modifiedName: true,
                ..Default::default()
            },
        );

        Ok(())
    }

    // All object-info replacements maintain references, derived state, and outgoing changes here.
    pub fn setObjectInfo(&mut self, object_id: ObjectId, mut info: ObjectInfo) -> anyhow::Result<()> {
        if let ObjectInfo::Label(label) = &mut info {
            label.text = LabelInfo::text_without_emoji(&label.text);
        }
        let object = self.getObjectMut(object_id)?;
        anyhow::ensure!(
            std::mem::discriminant(&object.info) == std::mem::discriminant(&info),
            "The object type cannot change."
        );
        let old_tracked_fields = ObjectInfoTrackedFields::from_object_info(&object.info);
        let new_tracked_fields = ObjectInfoTrackedFields::from_object_info(&info);
        object.info = info;
        self.afterObjectInfoChange(object_id, old_tracked_fields, new_tracked_fields);
        self.serverMarkObjectAsModified(object_id, ObjectModifyFlags::from_only_object_info());
        Ok(())
    }

    pub fn setBallInfo(&mut self, object_id: ObjectId, ball_info: BallInfo) -> anyhow::Result<()> {
        let ObjectInfo::Ball(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setBallInfo: object {} is not a ball", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Ball(ball_info))
    }

    pub fn setSkJointInfo(&mut self, object_id: ObjectId, sk_joint_info: SkJointInfo) -> anyhow::Result<()> {
        let ObjectInfo::SkJoint(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSkJointInfo: object {} is not a SkJoint", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SkJoint(sk_joint_info))
    }

    pub fn setSkAnimationInfo(&mut self, object_id: ObjectId, sk_animation_info: SkAnimationInfo) -> anyhow::Result<()> {
        let ObjectInfo::SkAnimation(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSkAnimationInfo: object {} is not a SkAnimation", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SkAnimation(sk_animation_info))
    }

    pub fn setSkAttachmentInfo(&mut self, object_id: ObjectId, sk_attachment_info: SkAttachmentInfo) -> anyhow::Result<()> {
        let ObjectInfo::SkAttachment(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSkAttachmentInfo: object {} is not a SkAttachment", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SkAttachment(sk_attachment_info))
    }

    pub fn setSkKeyframeInfo(&mut self, object_id: ObjectId, sk_keyframe_info: SkKeyframeInfo) -> anyhow::Result<()> {
        let ObjectInfo::SkKeyframe(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSkKeyframeInfo: object {} is not a SkKeyframe", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SkKeyframe(sk_keyframe_info))
    }

    pub fn setPropertyAnimationInfo(&mut self, object_id: ObjectId, property_animation_info: PropertyAnimationInfo) -> anyhow::Result<()> {
        let ObjectInfo::PropertyAnimation(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setPropertyAnimationInfo: object {} is not a PropertyAnimation", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::PropertyAnimation(property_animation_info))
    }

    pub fn setPropertyKeyframeInfo(&mut self, object_id: ObjectId, property_keyframe_info: PropertyKeyframeInfo) -> anyhow::Result<()> {
        let ObjectInfo::PropertyKeyframe(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setPropertyKeyframeInfo: object {} is not a PropertyKeyframe", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::PropertyKeyframe(property_keyframe_info))
    }

    pub fn setSkKeyframePoseInfo(&mut self, object_id: ObjectId, sk_keyframe_pose_info: SkKeyframePoseInfo) -> anyhow::Result<()> {
        let ObjectInfo::SkKeyframePose(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSkKeyframePoseInfo: object {} is not a SkKeyframePose", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SkKeyframePose(sk_keyframe_pose_info))
    }

    pub fn setBrickInfo(&mut self, object_id: ObjectId, brick_info: BrickInfo) -> anyhow::Result<()> {
        let ObjectInfo::Brick(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setBrickInfo: object {} is not a brick", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Brick(brick_info))
    }

    pub fn setCylinderInfo(&mut self, object_id: ObjectId, cylinder_info: CylinderInfo) -> anyhow::Result<()> {
        let ObjectInfo::Cylinder(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setCylinderInfo: object {} is not a cylinder", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Cylinder(cylinder_info))
    }

    pub fn setClockInfo(&mut self, object_id: ObjectId, clock_info: ClockInfo) -> anyhow::Result<()> {
        let ObjectInfo::Clock(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setClockInfo: object {} is not a clock", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Clock(clock_info))
    }

    pub fn setMaterialInfo(&mut self, object_id: ObjectId, material_info: MaterialInfo) -> anyhow::Result<()> {
        let ObjectInfo::Material(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setMaterialInfo: object {} is not a material", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Material(material_info))
    }

    pub fn setWorldLightingInfo(&mut self, object_id: ObjectId, world_lighting_info: WorldLightingInfo) -> anyhow::Result<()> {
        let ObjectInfo::WorldLighting(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setWorldLightingInfo: object {} is not world lighting", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::WorldLighting(world_lighting_info))
    }

    pub fn setLabelInfo(&mut self, object_id: ObjectId, label_info: LabelInfo) -> anyhow::Result<()> {
        let ObjectInfo::Label(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setLabelInfo: object {} is not a label", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Label(label_info))
    }

    pub fn setMeshInfo(&mut self, object_id: ObjectId, mesh_info: MeshInfo) -> anyhow::Result<()> {
        let ObjectInfo::Mesh(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setMeshInfo: object {} is not a mesh", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Mesh(mesh_info))
    }

    pub fn setRigidGroupInfo(&mut self, object_id: ObjectId, rigid_group_info: RigidGroupInfo) -> anyhow::Result<()> {
        let ObjectInfo::RigidGroup(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setRigidGroupInfo: object {} is not a rigid group", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::RigidGroup(rigid_group_info))
    }

    pub fn setPlayerInfo(&mut self, object_id: ObjectId, player_info: PlayerInfo) -> anyhow::Result<()> {
        let ObjectInfo::Player(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setPlayerInfo: object {} is not a player", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Player(player_info))
    }

    pub fn setScriptInfo(&mut self, object_id: ObjectId, script_info: ScriptInfo) -> anyhow::Result<()> {
        let ObjectInfo::Script(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setScriptInfo: object {} is not a script", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::Script(script_info))
    }

    pub fn setSoundPlayerInfo(&mut self, object_id: ObjectId, sound_player_info: SoundPlayerInfo) -> anyhow::Result<()> {
        let ObjectInfo::SoundPlayer(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSoundPlayerInfo: object {} is not a sound player", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SoundPlayer(sound_player_info))
    }

    pub fn setSkAnimatorInfo(&mut self, object_id: ObjectId, sk_animator_info: SkAnimatorInfo) -> anyhow::Result<()> {
        let ObjectInfo::SkAnimator(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setSkAnimatorInfo: object {} is not a SkAnimator", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::SkAnimator(sk_animator_info))
    }

    pub fn setPropertyAnimatorInfo(&mut self, object_id: ObjectId, property_animator_info: PropertyAnimatorInfo) -> anyhow::Result<()> {
        let ObjectInfo::PropertyAnimator(_) = &self.getObject(object_id)?.info else {
            return Err(anyhow::anyhow!("setPropertyAnimatorInfo: object {} is not a property animator", object_id));
        };
        self.setObjectInfo(object_id, ObjectInfo::PropertyAnimator(property_animator_info))
    }

    // updates the parent metadata and physics
    pub fn setParent(&mut self, object_id: ObjectId, parentId: ObjectId) -> anyhow::Result<()> {
        if !self.hasObject(object_id) {
            return Err(anyhow::anyhow!("setParent: object {} was not found", object_id));
        }
        if !self.hasObject(parentId) {
            return Err(anyhow::anyhow!("setParent: parent {} was not found", parentId));
        }
        if !self.canAddParent(object_id, parentId) {
            return Err(anyhow::anyhow!("setParent would cause cycle: object {} parent {}", object_id, parentId));
        }

        let old_skeleton_id = self.getSkeletonIdOfSkJoint(object_id);

        // clear + re-set parent relationships
        self.clearParent(object_id);
        self.parentOfId.insert(object_id, parentId);

        // clear + re-set children relationships
        let children_of_the_parent = self._childrenOfId.entry(parentId).or_insert_with(|| HashSet::new());
        children_of_the_parent.insert(object_id);

        // if it was a physical object, refresh physics
        self.refreshPhysicsAfterChangeParent(object_id);
        self.markObjectChangedForRender(object_id);

        // refresh render data
        let new_skeleton_id = self.getSkeletonIdOfSkJoint(object_id);
        if let Some(old_skeleton_id) = old_skeleton_id {
            if Some(old_skeleton_id) != new_skeleton_id {
                self.refreshRenderSkeletonInfoArcOfId(old_skeleton_id);
            }
        }
        if let Some(new_skeleton_id) = new_skeleton_id {
            self.refreshRenderSkeletonInfoArcOfId(new_skeleton_id);
        }
        self.refreshRenderSkKeyframes(parentId);
        self.refreshPropertyKeyframes(parentId);

        self.serverMarkObjectAsModified(
            object_id,
            ObjectModifyFlags {
                modifiedParent: true,
                ..Default::default()
            },
        );
        Ok(())
    }

    // this is what an animator or a script writing a position every tick wants
    pub fn setPhysicsInfo3D(&mut self, objectId: ObjectId, setPhysicsInfo3D: SetPhysicsInfo3D, should_server_mark_dirty: bool) {
        let isPose = setPhysicsInfo3D.position.is_some() || setPhysicsInfo3D.rotation.is_some();
        if isPose && self.teleportedObjectIdsSinceStep.contains(&objectId) {
            self.physics.forceObjectPhysicsInfo3D(objectId, setPhysicsInfo3D);
        } else {
            self.physics.setObjectPhysicsInfo3D(objectId, setPhysicsInfo3D);
        }
        self.markPhysicsInfo3DDirty(objectId, should_server_mark_dirty);
    }

    // this is what a teleport wants
    pub fn forcePhysicsInfo3D(&mut self, objectId: ObjectId, setPhysicsInfo3D: SetPhysicsInfo3D, should_server_mark_dirty: bool) {
        self.physics.forceObjectPhysicsInfo3D(objectId, setPhysicsInfo3D);
        self.markPhysicsInfo3DDirty(objectId, should_server_mark_dirty);
        // a new velocity is not a jump, so only a new position or rotation stops the object from being smoothed
        if setPhysicsInfo3D.position.is_some() || setPhysicsInfo3D.rotation.is_some() {
            *self.teleportCountOfObjectId.entry(objectId).or_insert(0) += 1;
            self.teleportedObjectIdsSinceStep.insert(objectId);
        }
    }

    pub fn getTeleportCount(&self, objectId: ObjectId) -> u64 {
        self.teleportCountOfObjectId.get(&objectId).copied().unwrap_or(0)
    }

    // this is what a pose from another machine wants
    pub fn setNetworkedPhysicsInfo3D(&mut self, objectId: ObjectId, networked_physics_info_3d: NetworkedPhysicsInfo3D) -> anyhow::Result<()> {
        let NetworkedPhysicsInfo3D {
            coords,
            teleportCount: teleport_count,
        } = networked_physics_info_3d;
        let did_teleport = self.getTeleportCount(objectId) != teleport_count;
        if did_teleport {
            self.teleportCountOfObjectId.insert(objectId, teleport_count);
        }
        match coords {
            NetworkedCoords::WorldCoords(physics_info_3d) if did_teleport => {
                self.physics.forceObjectPhysicsInfo3D(objectId, SetPhysicsInfo3D::from(physics_info_3d));
                self.markObjectChangedForRender(objectId);
                Ok(())
            }
            NetworkedCoords::WorldCoords(physics_info_3d) => self.setObjectNextPhysicsInfo3D(objectId, physics_info_3d),
            NetworkedCoords::SubitemRelativeCoords(relative_location_3d) => {
                // relative coords are only used for subitems
                self.setSubitemRelativeLocation3D(objectId, relative_location_3d)
            }
        }
    }

    fn markPhysicsInfo3DDirty(&mut self, objectId: ObjectId, should_server_mark_dirty: bool) {
        // a teleport can move a Fixed body without making it kinematic, and a part can move inside a group that stays put
        self.markObjectChangedForRender(objectId);
        if should_server_mark_dirty {
            self.serverMarkObjectAsModified(
                objectId,
                ObjectModifyFlags {
                    modifiedPhysicsInfo3D: true,
                    ..Default::default()
                },
            );
        }
    }

    pub fn setPhysicsToggles3D(&mut self, object_id: ObjectId, setPhysicsToggles3D: SetPhysicsToggles3D, should_server_mark_dirty: bool) {
        self.physics.updateObjectToHavePhysicsToggles3D(object_id, setPhysicsToggles3D);
        if should_server_mark_dirty {
            self.serverMarkObjectAsModified(
                object_id,
                ObjectModifyFlags {
                    modifiedPhysicsToggles3D: true,
                    ..Default::default()
                },
            );
        }
    }

    pub fn setPrimitiveSettings(&mut self, object_id: ObjectId, primitive_settings: PrimitiveSettings, should_server_mark_dirty: bool) -> anyhow::Result<()> {
        // The world never holds a size that loading refuses, so a saved world always opens again and players can always join.
        primitive_settings.validate()?;
        // surprisingly, there is no way to change this in rapier physics efficiently, we just refresh the tracking here
        self.refreshObjectPrimitivePhysics(object_id, None, None, Some(primitive_settings), None)?;
        if should_server_mark_dirty {
            self.serverMarkObjectAsModified(
                object_id,
                ObjectModifyFlags {
                    modifiedPrimitiveSettings: true,
                    ..Default::default()
                },
            );
        }
        Ok(())
    }

    // rewires all pointers on the object info object
    pub fn rewireObjectInfoPointers(
        object_info: &mut ObjectInfo,
        old_object_info: &ObjectInfo,
        cloned_object_id_of_source_object_id: &HashMap<ObjectId, ObjectId>,
        are_missing_object_ids_valid: bool,
    ) -> anyhow::Result<()> {
        // iterate thru every item, setting pointers appropriately
        let get_new_object_id = |source_object_id: Option<ObjectId>| -> Option<ObjectId> {
            let source_object_id = source_object_id?;
            if let Some(cloned_object_id) = cloned_object_id_of_source_object_id.get(&source_object_id).copied() {
                Some(cloned_object_id)
            } else {
                if are_missing_object_ids_valid { Some(source_object_id) } else { None }
            }
        };

        match (object_info, old_object_info) {
            (ObjectInfo::Ball(info), ObjectInfo::Ball(oldInfo)) => {
                info.material = oldInfo.material.and_then(|material| {
                    let old_material_info_id = ObjectId::from(material.materialInfoId);
                    get_new_object_id(Some(old_material_info_id)).map(|new_material_info_id| game_types::object_types::BallMaterialSettings {
                        materialInfoId: ObjectIdPointer(new_material_info_id),
                        tileSizeU: material.tileSizeU,
                        tileSizeV: material.tileSizeV,
                        tileOffsetU: material.tileOffsetU,
                        tileOffsetV: material.tileOffsetV,
                    })
                });
            }
            (ObjectInfo::Brick(info), ObjectInfo::Brick(oldInfo)) => {
                info.material = oldInfo.material.and_then(|material| {
                    let old_material_info_id = ObjectId::from(material.materialInfoId);
                    get_new_object_id(Some(old_material_info_id)).map(|new_material_info_id| game_types::object_types::BrickMaterialSettings {
                        materialInfoId: ObjectIdPointer(new_material_info_id),
                        tileSizeU: material.tileSizeU,
                        tileSizeV: material.tileSizeV,
                        tileOffsetU: material.tileOffsetU,
                        tileOffsetV: material.tileOffsetV,
                    })
                });
            }
            (ObjectInfo::Cylinder(info), ObjectInfo::Cylinder(oldInfo)) => {
                info.material = oldInfo.material.and_then(|material| {
                    let old_material_info_id = ObjectId::from(material.materialInfoId);
                    get_new_object_id(Some(old_material_info_id)).map(|new_material_info_id| game_types::object_types::CylinderMaterialSettings {
                        materialInfoId: ObjectIdPointer(new_material_info_id),
                        tileSizeU: material.tileSizeU,
                        tileSizeV: material.tileSizeV,
                        tileOffsetU: material.tileOffsetU,
                        tileOffsetV: material.tileOffsetV,
                    })
                });
            }
            (ObjectInfo::SkJoint(_), ObjectInfo::SkJoint(_)) => {}
            (ObjectInfo::Skeleton(_), ObjectInfo::Skeleton(_)) => {}
            (ObjectInfo::Clock(info), ObjectInfo::Clock(oldInfo)) => info.clockState = oldInfo.clockState,
            (ObjectInfo::LocalGroup(_), ObjectInfo::LocalGroup(_)) => {}
            (ObjectInfo::Material(_), ObjectInfo::Material(_)) => {}
            (ObjectInfo::Label(info), ObjectInfo::Label(oldInfo)) => {
                let old_physical_id = oldInfo.physicalObjectId.map(ObjectId::from);
                info.physicalObjectId = get_new_object_id(old_physical_id).map(ObjectIdPointer);
            }
            (ObjectInfo::LabelImage(info), ObjectInfo::LabelImage(oldInfo)) => {
                let old_physical_id = oldInfo.physicalObjectId.map(ObjectId::from);
                info.physicalObjectId = get_new_object_id(old_physical_id).map(ObjectIdPointer);
            }
            (ObjectInfo::Pivot(info), ObjectInfo::Pivot(oldInfo)) => {
                let old_physical_id = oldInfo.physicalObjectId.map(ObjectId::from);
                info.physicalObjectId = get_new_object_id(old_physical_id).map(ObjectIdPointer);
            }
            (ObjectInfo::Mesh(info), ObjectInfo::Mesh(oldInfo)) => {
                let oldSkeletonId = oldInfo.followSkeletonId.map(ObjectId::from);
                let old_physical_id = oldInfo.physicalObjectId.map(ObjectId::from);
                info.followSkeletonId = get_new_object_id(oldSkeletonId).map(ObjectIdPointer);
                info.physicalObjectId = get_new_object_id(old_physical_id).map(ObjectIdPointer);
                info.material = oldInfo.material.and_then(|mesh_material_settings| {
                    let old_material_info_id = ObjectId::from(mesh_material_settings.materialInfoId);
                    get_new_object_id(Some(old_material_info_id)).map(|new_material_info_id| MeshMaterialSettings {
                        materialInfoId: ObjectIdPointer(new_material_info_id),
                    })
                });
            }
            (ObjectInfo::Player(info), ObjectInfo::Player(oldInfo)) => {
                let oldHeadId = oldInfo.headObjectId.as_ref().map(ObjectId::from);
                let oldCharacterId = oldInfo.characterObjectId.as_ref().map(ObjectId::from);
                info.headObjectId = get_new_object_id(oldHeadId).map(ObjectIdPointer);
                info.characterObjectId = get_new_object_id(oldCharacterId).map(ObjectIdPointer);
            }
            (ObjectInfo::RigidGroup(info), ObjectInfo::RigidGroup(oldInfo)) => {
                let oldSkeletonId = oldInfo.followSkeletonId.as_ref().map(ObjectId::from);
                info.followSkeletonId = get_new_object_id(oldSkeletonId).map(ObjectIdPointer);
            }
            (ObjectInfo::Script(_), ObjectInfo::Script(_)) => {}
            (ObjectInfo::SkAnimation(_), ObjectInfo::SkAnimation(_)) => {}
            (ObjectInfo::SkAttachment(info), ObjectInfo::SkAttachment(oldInfo)) => {
                let old_attached_object_id = oldInfo.objectId.as_ref().map(ObjectId::from);
                info.objectId = get_new_object_id(old_attached_object_id).map(ObjectIdPointer);
            }
            (ObjectInfo::SkKeyframe(_), ObjectInfo::SkKeyframe(_)) => {}
            (ObjectInfo::SkKeyframePose(_), ObjectInfo::SkKeyframePose(_)) => {}
            (ObjectInfo::WorldLighting(_), ObjectInfo::WorldLighting(_)) => {}
            (ObjectInfo::SoundPlayer(info), ObjectInfo::SoundPlayer(oldInfo)) => {
                let oldClockId = oldInfo.clockId.as_ref().map(ObjectId::from);
                info.clockId = get_new_object_id(oldClockId).map(ObjectIdPointer);
            }
            (ObjectInfo::SkAnimator(info), ObjectInfo::SkAnimator(oldInfo)) => {
                let oldSkeletonId = oldInfo.skeletonId.as_ref().map(ObjectId::from);
                let oldClockId = oldInfo.clockId.as_ref().map(ObjectId::from);
                let oldSkAnimationId = oldInfo.skAnimationId.as_ref().map(ObjectId::from);
                info.skeletonId = get_new_object_id(oldSkeletonId).map(ObjectIdPointer);
                info.clockId = get_new_object_id(oldClockId).map(ObjectIdPointer);
                info.skAnimationId = get_new_object_id(oldSkAnimationId).map(ObjectIdPointer);
            }
            (ObjectInfo::PropertyAnimator(info), ObjectInfo::PropertyAnimator(oldInfo)) => {
                let oldTargetObjectId = oldInfo.targetObjectId.as_ref().map(ObjectId::from);
                let oldClockId = oldInfo.clockId.as_ref().map(ObjectId::from);
                let oldPropertyAnimationId = oldInfo.propertyAnimationId.as_ref().map(ObjectId::from);
                info.targetObjectId = get_new_object_id(oldTargetObjectId).map(ObjectIdPointer);
                info.clockId = get_new_object_id(oldClockId).map(ObjectIdPointer);
                info.propertyAnimationId = get_new_object_id(oldPropertyAnimationId).map(ObjectIdPointer);
            }
            (ObjectInfo::PropertyAnimation(_), ObjectInfo::PropertyAnimation(_)) => {}
            (ObjectInfo::PropertyKeyframe(_), ObjectInfo::PropertyKeyframe(_)) => {}
            (ObjectInfo::Tween(info), ObjectInfo::Tween(oldInfo)) => {
                let oldTargetObjectId = oldInfo.targetObjectId.as_ref().map(ObjectId::from);
                info.targetObjectId = get_new_object_id(oldTargetObjectId).map(ObjectIdPointer);
            }
            _ => {
                return Err(anyhow::anyhow!("Object type somehow changed while rewiring object pointers"));
            }
        }

        Ok(())
    }

    // This rewires pointers after addObjectFromTemplate creates the template objects.
    fn rewireTemplatePointers(
        &mut self,
        template_item: &TemplateItem,
        cloned_object_id_of_source_object_id: &HashMap<ObjectId, ObjectId>,
    ) -> anyhow::Result<()> {
        let Some(&new_object_id) = cloned_object_id_of_source_object_id.get(&template_item.sourceObjectId) else {
            return Ok(());
        };

        // before
        let old_tracked_fields = {
            let new_object = self.getObject(new_object_id)?;
            ObjectInfoTrackedFields::from_object_info(&new_object.info)
        };
        // rewire
        {
            let old_object_info = template_item.objectBeingAdded.clone().to_object_info();
            let new_object = self.getObjectMut(new_object_id)?;
            Self::rewireObjectInfoPointers(&mut new_object.info, &old_object_info, cloned_object_id_of_source_object_id, true)?;
        }
        // after
        let new_tracked_fields = {
            let new_object = self.getObject(new_object_id)?;
            ObjectInfoTrackedFields::from_object_info(&new_object.info)
        };
        self.afterObjectInfoChange(new_object_id, old_tracked_fields, new_tracked_fields);

        // rewire children too
        for child_template_item in template_item.children.iter() {
            self.rewireTemplatePointers(child_template_item, cloned_object_id_of_source_object_id)?;
        }
        Ok(())
    }

    // This creates template objects and fills the source-to-new id map before pointer rewiring.
    fn addObjectFromTemplateItem(
        &mut self,
        template_item: &TemplateItem,
        parent_id: ObjectId,
        cloned_object_id_of_source_object_id: &mut HashMap<ObjectId, ObjectId>,
    ) -> anyhow::Result<ObjectId> {
        let new_object_id = self.addObject(template_item.name.clone(), template_item.objectBeingAdded.clone(), parent_id);
        if let Some(script_raw_code) = template_item.scriptRawCode.as_ref() {
            self.markScriptAsReadyToRun(new_object_id, script_raw_code.clone());
        }
        cloned_object_id_of_source_object_id.insert(template_item.sourceObjectId, new_object_id);
        for child_template_item in template_item.children.iter() {
            self.addObjectFromTemplateItem(child_template_item, new_object_id, cloned_object_id_of_source_object_id)?;
        }
        Ok(new_object_id)
    }

    // This is used (1) when cloning objects and (2) when pasting from templates.
    pub fn addObjectFromTemplate(&mut self, root_template_item: &TemplateItem, parent_id: ObjectId) -> anyhow::Result<ObjectId> {
        if !self.hasObject(parent_id) {
            return Err(anyhow::anyhow!("pasteTemplate: parent object {} was not found", parent_id));
        }

        self.refreshAnimationKeyframesOnceAfter(|world| {
            let mut cloned_object_id_of_source_object_id = HashMap::new();
            let new_root_object_id = world.addObjectFromTemplateItem(root_template_item, parent_id, &mut cloned_object_id_of_source_object_id)?;
            world.rewireTemplatePointers(root_template_item, &cloned_object_id_of_source_object_id)?;
            Ok(new_root_object_id)
        })
    }

    pub fn createTemplateFrom(&self, object_id: ObjectId) -> anyhow::Result<TemplateItem> {
        let object_name = self.getObject(object_id)?.name.clone();
        let objectBeingAdded = self.cloneObjectAsObjectBeingAdded(object_id)?;
        let scriptRawCode = self.scriptRawCodeOfId.get(&object_id).cloned();
        let mut children = Vec::new();
        let child_object_ids = self.getDirectChildrenIds(object_id)?.iter().copied().collect::<Vec<_>>();
        for child_object_id in child_object_ids {
            children.push(self.createTemplateFrom(child_object_id)?);
        }
        Ok(TemplateItem {
            sourceObjectId: object_id,
            name: object_name,
            objectBeingAdded,
            scriptRawCode,
            children,
        })
    }

    // Cutting creates a template and deletes the object.
    pub fn cutObjectToTemplate(&mut self, object_id: ObjectId, template_name: String) -> anyhow::Result<()> {
        if template_name.is_empty() {
            return Err(anyhow::anyhow!("cutObjectToTemplate: template name cannot be empty"));
        }
        if !self.hasObject(object_id) {
            return Err(anyhow::anyhow!("cutObjectToTemplate: source object {} was not found", object_id));
        }
        if !self.parentOfId.contains_key(&object_id) {
            return Err(anyhow::anyhow!("cutObjectToTemplate: cannot cut the root object"));
        }

        let root_template_item = self.createTemplateFrom(object_id)?;
        self.removeObject(object_id)?;
        self.templateOfName.insert(template_name, root_template_item);
        Ok(())
    }

    // Pasting creates an object from a template.
    pub fn pasteTemplate(&mut self, template_name: &str, parent_id: ObjectId) -> anyhow::Result<ObjectId> {
        let root_template_item = self
            .templateOfName
            .get(template_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("pasteTemplate: template {} was not found", template_name))?;
        self.addObjectFromTemplate(&root_template_item, parent_id)
    }

    // Every data id that an object in a template uses. Templates are never sent to players, so this is how they learn about that data.
    pub fn getTemplateDataIds(&self) -> DataIds {
        fn insertDataIdsOfTemplateItem(data_ids: &mut DataIds, template_item: &TemplateItem) {
            data_ids.insert_data_ids_of_object(&template_item.objectBeingAdded.clone().to_object_info());
            for child_template_item in template_item.children.iter() {
                insertDataIdsOfTemplateItem(data_ids, child_template_item);
            }
        }
        let mut data_ids = DataIds::default();
        for template_item in self.templateOfName.values() {
            insertDataIdsOfTemplateItem(&mut data_ids, template_item);
        }
        data_ids
    }

    pub fn addClonedObjectRecursive(&mut self, source_object_id: ObjectId, clone_name: String) -> anyhow::Result<ObjectId> {
        if !self.hasObject(source_object_id) {
            return Err(anyhow::anyhow!("cloneObject: source object {} was not found", source_object_id));
        }
        let source_parent_id = self
            .parentOfId
            .get(&source_object_id)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("cloneObject: cannot clone the root object"))?;
        let mut root_template_item = self.createTemplateFrom(source_object_id)?;
        root_template_item.name = clone_name;
        self.addObjectFromTemplate(&root_template_item, source_parent_id)
    }

    // Move every physical object in the subtree by the same world-space position offset.
    pub fn setObjectPositionOffsetRecursive(&mut self, object_id: ObjectId, position_offset: Vec3, set_or_force: SetOrForce) -> anyhow::Result<()> {
        if position_offset.length_squared() <= f32::EPSILON {
            return Ok(());
        }
        // Snapshot the old world positions first so grouped children keep the same local offset after the parent body moves.
        let mut subtree_object_ids = Vec::new();
        self.collectAllDescendants(&mut subtree_object_ids, object_id, true, &Self::isPhysical);

        let mut old_position_of_object_id = Vec::new();
        for subtree_object_id in subtree_object_ids.iter().copied() {
            let Some(physics_info_3d) = self.getObjectPhysicsInfo3D(subtree_object_id) else {
                continue;
            };
            old_position_of_object_id.push((subtree_object_id, physics_info_3d.location.position));
        }

        for (subtree_object_id, old_position) in old_position_of_object_id {
            let set_physics_info_3d = SetPhysicsInfo3D {
                position: Some(old_position + position_offset),
                ..Default::default()
            };
            match set_or_force {
                SetOrForce::Set => self.setPhysicsInfo3D(subtree_object_id, set_physics_info_3d, true),
                SetOrForce::Force => self.forcePhysicsInfo3D(subtree_object_id, set_physics_info_3d, true),
            }
        }

        Ok(())
    }

    // Compute the global locations every physical object in the subtree would have after one shared pivot rotation.
    pub fn getObjectRotatedRecursive(
        &self,
        object_id: ObjectId,
        rotation_quaternion: Quat,
        center_point: Vec3,
    ) -> anyhow::Result<HashMap<ObjectId, Location3D>> {
        let rotation_quaternion_length = rotation_quaternion.length();
        if rotation_quaternion_length <= f32::EPSILON {
            return Err(anyhow::anyhow!("rotate failed: rotation quaternion must have magnitude greater than 0"));
        }
        let normalized_rotation_quaternion = rotation_quaternion / rotation_quaternion_length;

        // Snapshot the old subtree transforms first so every item gets the same delta from the same pivot.
        let mut object_ids = Vec::new();
        self.collectAllDescendants(&mut object_ids, object_id, true, &Self::isPhysical);
        if object_ids.is_empty() {
            return Err(anyhow::anyhow!("rotate failed: object {} has no physical root-or-descendants", object_id));
        }

        let mut rotated_location_of_object_id = HashMap::new();
        for physical_object_id in object_ids.iter().copied() {
            let Some(physics_info_3d) = self.getObjectPhysicsInfo3D(physical_object_id) else {
                continue;
            };
            let old_location = physics_info_3d.location;
            let rotated_location = Location3D {
                position: center_point + normalized_rotation_quaternion * (old_location.position - center_point),
                quaternion: normalized_rotation_quaternion * old_location.quaternion,
            };
            rotated_location_of_object_id.insert(physical_object_id, rotated_location);
        }

        Ok(rotated_location_of_object_id)
    }

    // Rotate every physical object in the subtree by the same world-space delta around one shared pivot point.
    pub fn setObjectRotationRecursive(
        &mut self,
        object_id: ObjectId,
        rotation_quaternion: Quat,
        center_point: Vec3,
        set_or_force: SetOrForce,
    ) -> anyhow::Result<()> {
        let rotated_location_of_object_id = self.getObjectRotatedRecursive(object_id, rotation_quaternion, center_point)?;
        let mut object_ids = Vec::new();
        self.collectAllDescendants(&mut object_ids, object_id, true, &Self::isPhysical);

        // Apply parent objects before descendants so rigid-group subitems keep the intended world-space transform.
        for physical_object_id in object_ids {
            let Some(new_location) = rotated_location_of_object_id.get(&physical_object_id) else {
                continue;
            };
            let set_physics_info_3d = SetPhysicsInfo3D {
                position: Some(new_location.position),
                rotation: Some(new_location.quaternion),
                ..Default::default()
            };
            match set_or_force {
                SetOrForce::Set => self.setPhysicsInfo3D(physical_object_id, set_physics_info_3d, true),
                SetOrForce::Force => self.forcePhysicsInfo3D(physical_object_id, set_physics_info_3d, true),
            }
        }
        Ok(())
    }

    // Scale every physical object in the subtree around one shared world-space pivot.
    pub fn setObjectScaleRecursive(&mut self, object_id: ObjectId, scale_multiplier: f32, center_point: Vec3, set_or_force: SetOrForce) -> anyhow::Result<()> {
        // Gather the physical subtree once so every target center comes from the same pre-scale world state.
        let mut object_ids = Vec::new();
        self.collectAllDescendants(&mut object_ids, object_id, true, &Self::isPhysical);

        // Snapshot the old subtree centers first so rigid-group children keep one shared world-space source position.
        let mut old_position_of_object_id = Vec::with_capacity(object_ids.len());
        for physical_object_id in object_ids.iter().copied() {
            let Some(physics_info_3d) = self.getObjectPhysicsInfo3D(physical_object_id) else {
                continue;
            };
            old_position_of_object_id.push((physical_object_id, physics_info_3d.location.position));
        }

        // Apply the scaled centers parent-first so rigid-group children can convert into the right local offset.
        for (physical_object_id, old_position) in &old_position_of_object_id {
            let set_physics_info_3d = SetPhysicsInfo3D {
                position: Some(center_point + (*old_position - center_point) * scale_multiplier),
                ..Default::default()
            };
            match set_or_force {
                SetOrForce::Set => self.setPhysicsInfo3D(*physical_object_id, set_physics_info_3d, true),
                SetOrForce::Force => self.forcePhysicsInfo3D(*physical_object_id, set_physics_info_3d, true),
            }
        }

        // Resize primitives after every center is already at its final world-space position.
        let primitive_scale_multiplier = scale_multiplier.abs();
        for (primitive_object_id, _) in &old_position_of_object_id {
            let Some(primitive_settings) = self.physics.getPrimitiveSettings(*primitive_object_id) else {
                continue;
            };
            match primitive_settings {
                PrimitiveSettings::Ball(ball_primitive_settings) => {
                    let new_radius = ball_primitive_settings.radius * primitive_scale_multiplier;
                    let res = self.setPrimitiveSettings(
                        *primitive_object_id,
                        PrimitiveSettings::Ball(BallPrimitiveSettings { radius: new_radius }),
                        true,
                    );
                    if let Err(err) = res {
                        log::error!("Error setting primitive info for ball {}: {}", primitive_object_id, err);
                    }
                }
                PrimitiveSettings::Brick(brick_primitive_settings) => {
                    let new_scale = brick_primitive_settings.size * primitive_scale_multiplier;
                    let res = self.setPrimitiveSettings(*primitive_object_id, PrimitiveSettings::Brick(BrickPrimitiveSettings { size: new_scale }), true);
                    if let Err(err) = res {
                        log::error!("Error setting primitive info for brick {}: {}", primitive_object_id, err);
                    }
                }
                PrimitiveSettings::Cylinder(cylinder_primitive_settings) => {
                    let new_radius = cylinder_primitive_settings.radius * primitive_scale_multiplier;
                    let new_length = cylinder_primitive_settings.length * primitive_scale_multiplier;
                    let res = self.setPrimitiveSettings(
                        *primitive_object_id,
                        PrimitiveSettings::Cylinder(CylinderPrimitiveSettings {
                            radius: new_radius,
                            length: new_length,
                        }),
                        true,
                    );
                    if let Err(err) = res {
                        log::error!("Error setting primitive info for cylinder {}: {}", primitive_object_id, err);
                    }
                }
            };
        }

        // The Meshes, Labels and Pivots on a scaled object are part of how it looks and turns, so they grow or shrink with it:
        // a model's mesh keeps matching its collider, a label stays on the surface it was on, and a hinge stays on its edge.
        // A constant-size label keeps its size on screen, so only its offset changes.
        let mut attached_object_ids = HashSet::new();
        for (scaled_object_id, _) in &old_position_of_object_id {
            if let Some(pointer_object_ids) = self._objectsThatPointToId.get(scaled_object_id) {
                attached_object_ids.extend(pointer_object_ids.iter().copied());
            }
        }
        for attached_object_id in attached_object_ids {
            let scaled_info = match &self.getObject(attached_object_id)?.info {
                ObjectInfo::Mesh(mesh) => ObjectInfo::Mesh(MeshInfo {
                    scale: mesh.scale * primitive_scale_multiplier,
                    ..mesh.clone()
                }),
                ObjectInfo::Label(label) => {
                    let text_scale_multiplier = if label.isConstantSize { 1.0 } else { primitive_scale_multiplier };
                    ObjectInfo::Label(LabelInfo {
                        offsetFromObject: label.offsetFromObject * primitive_scale_multiplier,
                        lineHeight: label.lineHeight * text_scale_multiplier,
                        maxWidth: label.maxWidth.map(|max_width| max_width * text_scale_multiplier),
                        ..label.clone()
                    })
                }
                ObjectInfo::LabelImage(image) => {
                    let image_scale_multiplier = if image.isConstantSize { 1.0 } else { primitive_scale_multiplier };
                    ObjectInfo::LabelImage(LabelImageInfo {
                        offsetFromObject: image.offsetFromObject * primitive_scale_multiplier,
                        width: image.width * image_scale_multiplier,
                        height: image.height * image_scale_multiplier,
                        ..image.clone()
                    })
                }
                ObjectInfo::Pivot(pivot) => ObjectInfo::Pivot(PivotInfo {
                    offsetFromObject: pivot.offsetFromObject * primitive_scale_multiplier,
                    ..pivot.clone()
                }),
                _ => continue,
            };
            self.setObjectInfo(attached_object_id, scaled_info)?;
        }

        // mark as dirty - the sets we call here already do that

        Ok(())
    }

    // Set the walkthrough state on every primitive in the subtree.
    pub fn setObjectWalkthroughRecursive(&mut self, object_id: ObjectId, is_walkthrough: bool) -> anyhow::Result<()> {
        let mut primitive_object_ids = Vec::new();
        self.collectAllDescendants(&mut primitive_object_ids, object_id, true, &Self::isPrimitive);

        for primitive_object_id in primitive_object_ids {
            self.setPhysicsToggles3D(
                primitive_object_id,
                SetPhysicsToggles3D {
                    isWalkthrough: Some(is_walkthrough),
                    ..Default::default()
                },
                true,
            );
        }

        Ok(())
    }

    // Set whether every primitive in the subtree is pushable.
    pub fn setObjectPushableRecursive(&mut self, object_id: ObjectId, is_pushable: bool) -> anyhow::Result<()> {
        let mut primitive_object_ids = Vec::new();
        self.collectAllDescendants(&mut primitive_object_ids, object_id, true, &Self::isPrimitive);

        for primitive_object_id in primitive_object_ids {
            self.setPhysicsToggles3D(
                primitive_object_id,
                SetPhysicsToggles3D {
                    isPushable: Some(is_pushable),
                    ..Default::default()
                },
                true,
            );
        }

        Ok(())
    }

    // Set the density on every primitive in the subtree.
    pub fn setObjectDensityRecursive(&mut self, object_id: ObjectId, density: f32) -> anyhow::Result<()> {
        let mut primitive_object_ids = Vec::new();
        self.collectAllDescendants(&mut primitive_object_ids, object_id, true, &Self::isPrimitive);

        for primitive_object_id in primitive_object_ids {
            self.setPhysicsToggles3D(
                primitive_object_id,
                SetPhysicsToggles3D {
                    density: Some(density),
                    ..Default::default()
                },
                true,
            );
        }

        Ok(())
    }

    pub fn setObjectNextPhysicsInfo3D(&mut self, objectId: ObjectId, physics_info_3d: PhysicsInfo3D) -> anyhow::Result<()> {
        self.physics.setObjectNextPhysicsInfo3D(objectId, physics_info_3d)?;
        self.markObjectChangedForRender(objectId);
        // mark as dirty - we are currently only using this fn for foreign-controlled, and we shouldnt mark those as modified. They go out as foreign character deltas instead, which happens automatically.
        Ok(())
    }

    pub fn setSubitemRelativeLocation3D(&mut self, objectId: ObjectId, relative_location_3d: Location3D) -> anyhow::Result<()> {
        self.physics.setSubitemRelativeLocation3D(objectId, relative_location_3d)?;
        self.markPhysicsInfo3DDirty(objectId, true);
        Ok(())
    }

    // ---------------------------- Player metadata and camera --------------------------------

    pub fn getPlayerMetadata(&self, player_object_id: ObjectId) -> Option<&str> {
        self.playerMetadataOfPlayerObjectId.get(&player_object_id).map(String::as_str)
    }

    pub fn setPlayerMetadata(&mut self, player_object_id: ObjectId, json: Option<String>) {
        match json {
            Some(json) => self.playerMetadataOfPlayerObjectId.insert(player_object_id, json),
            None => self.playerMetadataOfPlayerObjectId.remove(&player_object_id),
        };
    }

    pub fn setLocalPlayerMetadataJson(&mut self, json: String) -> anyhow::Result<()> {
        let WorldControlMode::Player(player_object_id) = self.worldControlMode else {
            return Err(anyhow::anyhow!("setLocalPlayerMetadata: only the playing client has a local player"));
        };
        self.playerMetadataOfPlayerObjectId.insert(player_object_id, json);
        Ok(())
    }

    pub fn getPlayerCameraOrientation(&self, player_object_id: ObjectId) -> Option<Orientation3D> {
        self.cameraOrientationOfPlayerObjectId.get(&player_object_id).copied()
    }

    pub fn setPlayerCameraOrientation(&mut self, player_object_id: ObjectId, camera_orientation: Orientation3D) {
        self.cameraOrientationOfPlayerObjectId.insert(player_object_id, camera_orientation);
    }

    // ---------------------------- Player history (server only) --------------------------------

    pub fn recordCharacterPose(&mut self, character_object_id: ObjectId, est_live_server_sim_ms: f64, physics_info_3d: PhysicsInfo3D, teleport_count: u64) {
        let history = self.characterPoseHistoryOfObjectId.entry(character_object_id).or_insert_with(Timeline::new);
        history.insert(
            est_live_server_sim_ms,
            NetworkedPhysicsInfo3D {
                coords: NetworkedCoords::WorldCoords(physics_info_3d),
                teleportCount: teleport_count,
            },
        );
        history.forget_entries_before(est_live_server_sim_ms - SERVER_PLAYER_HISTORY_MS as f64);
    }

    // Where watchers drew this character at a recent time. They read the same kind of timeline the same way.
    pub fn getCharacterLocationAtTime(&self, character_object_id: ObjectId, est_server_sim_ms: f64) -> Option<Location3D> {
        let history = self.characterPoseHistoryOfObjectId.get(&character_object_id)?;
        match get_interpolated_pose(history, est_server_sim_ms)?.coords {
            NetworkedCoords::WorldCoords(physics_info_3d) => Some(physics_info_3d.location),
            NetworkedCoords::SubitemRelativeCoords(_) => None,
        }
    }

    pub fn recordPlayerMetadata(&mut self, player_object_id: ObjectId, est_live_server_sim_ms: f64, json: Option<String>) {
        let history = self.playerMetadataHistoryOfObjectId.entry(player_object_id).or_insert_with(Timeline::new);
        history.insert(est_live_server_sim_ms, json);
        history.forget_entries_before(est_live_server_sim_ms - SERVER_PLAYER_HISTORY_MS as f64);
    }

    // The metadata watchers had for this player at a recent time
    pub fn getPlayerMetadataAtTime(&self, player_object_id: ObjectId, est_server_sim_ms: f64) -> Option<&str> {
        let history = self.playerMetadataHistoryOfObjectId.get(&player_object_id)?;
        let (_, json) = history.find_last_entry_at_or_before(est_server_sim_ms)?;
        json.as_deref()
    }

    // ---------------------------- Player Getters --------------------------------

    pub fn getPlayerCharacterObject(&self, player_object_id: ObjectId) -> Option<ObjectId> {
        let Some(player_obj) = self.getObject(player_object_id).ok() else {
            return None;
        };
        let ObjectInfo::Player(player) = &player_obj.info else {
            return None;
        };
        player.characterObjectId.as_ref().map(|id| (*id).into())
    }

    // TODO: could make this more efficient
    pub fn getPlayerIdWhoControlsObject(&self, object_id: ObjectId) -> Option<ObjectId> {
        let player_object_ids = self._kindOfObjectId.get_keys(&ObjectKind::Player)?;
        for &player_object_id in player_object_ids {
            if self.getPlayerCharacterObject(player_object_id) == Some(object_id) {
                return Some(player_object_id);
            }
        }
        None
    }

    pub fn getPlayerHeadVec3(&self, playerId: ObjectId) -> Option<Vec3> {
        let head_object_id = self.getPlayerHeadObjectId(playerId)?;
        let info3D = self.getObjectPhysicsInfo3D(head_object_id)?;
        Some(info3D.location.position)
    }

    pub fn getPlayerHeadObjectId(&self, playerId: ObjectId) -> Option<ObjectId> {
        let playerObj = self.getObject(playerId).ok()?;
        let ObjectInfo::Player(player) = &playerObj.info else {
            return None;
        };
        player.headObjectId.as_ref().map(|id| id.into())
    }

    pub fn getPlayerBodyVec3(&self, playerId: ObjectId) -> Option<Vec3> {
        let Some(characterObjectId) = self.getPlayerCharacterObject(playerId) else {
            return None;
        };
        let Some(info3D) = self.getObjectPhysicsInfo3D(characterObjectId.into()) else {
            return None;
        };
        Some(info3D.location.position)
    }

    // Read live object and physics state for templates and network object additions.
    pub fn cloneObjectAsObjectBeingAdded(&self, object_id: ObjectId) -> anyhow::Result<ObjectBeingAdded> {
        let live_object = self.getObject(object_id)?;

        let object_being_added = match &live_object.info {
            ObjectInfo::Ball(info) => {
                let ball_primitive_settings = self
                    .physics
                    .getBallPrimitiveSettings(object_id)
                    .ok_or_else(|| anyhow::anyhow!("BallPrimitiveSettings not found for ball {}", object_id))?;
                let physics_info = self
                    .getObjectPhysicsInfo3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics info not found for ball {}", object_id))?;
                let physics_toggles = self
                    .getObjectPhysicsToggles3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics toggles not found for ball {}", object_id))?;
                ObjectBeingAdded::Ball(info.clone(), ball_primitive_settings, physics_info, physics_toggles)
            }
            ObjectInfo::SkJoint(info) => ObjectBeingAdded::SkJoint(info.clone()),
            ObjectInfo::Skeleton(info) => ObjectBeingAdded::Skeleton(info.clone()),
            ObjectInfo::Brick(info) => {
                let brick_primitive_settings = self
                    .physics
                    .getBrickPrimitiveSettings(object_id)
                    .ok_or_else(|| anyhow::anyhow!("BrickPrimitiveSettings not found for brick {}", object_id))?;
                let physics_info = self
                    .getObjectPhysicsInfo3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics info not found for brick {}", object_id))?;
                let physics_toggles = self
                    .getObjectPhysicsToggles3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics toggles not found for brick {}", object_id))?;
                ObjectBeingAdded::Brick(info.clone(), brick_primitive_settings, physics_info, physics_toggles)
            }
            ObjectInfo::Cylinder(info) => {
                let cylinder_primitive_settings = self
                    .physics
                    .getCylinderPrimitiveSettings(object_id)
                    .ok_or_else(|| anyhow::anyhow!("CylinderPrimitiveSettings not found for cylinder {}", object_id))?;
                let physics_info = self
                    .getObjectPhysicsInfo3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics info not found for cylinder {}", object_id))?;
                let physics_toggles = self
                    .getObjectPhysicsToggles3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics toggles not found for cylinder {}", object_id))?;
                ObjectBeingAdded::Cylinder(info.clone(), cylinder_primitive_settings, physics_info, physics_toggles)
            }
            ObjectInfo::RigidGroup(info) => {
                let physics_info = self
                    .getObjectPhysicsInfo3D(object_id)
                    .ok_or_else(|| anyhow::anyhow!("Physics info not found for rigid group {}", object_id))?;
                ObjectBeingAdded::RigidGroup(info.clone(), physics_info)
            }
            ObjectInfo::Clock(info) => ObjectBeingAdded::Clock(info.clone()),
            ObjectInfo::Label(info) => ObjectBeingAdded::Label(info.clone()),
            ObjectInfo::LabelImage(info) => ObjectBeingAdded::LabelImage(info.clone()),
            ObjectInfo::LocalGroup(info) => ObjectBeingAdded::LocalGroup(info.clone()),
            ObjectInfo::Material(info) => ObjectBeingAdded::Material(info.clone()),
            ObjectInfo::Mesh(info) => ObjectBeingAdded::Mesh(info.clone()),
            ObjectInfo::Pivot(info) => ObjectBeingAdded::Pivot(info.clone()),
            ObjectInfo::Player(info) => ObjectBeingAdded::Player(info.clone()),
            ObjectInfo::Script(info) => ObjectBeingAdded::Script(info.clone()),
            ObjectInfo::SoundPlayer(info) => ObjectBeingAdded::SoundPlayer(info.clone()),
            ObjectInfo::SkAnimation(info) => ObjectBeingAdded::SkAnimation(info.clone()),
            ObjectInfo::SkAttachment(info) => ObjectBeingAdded::SkAttachment(info.clone()),
            ObjectInfo::SkKeyframe(info) => ObjectBeingAdded::SkKeyframe(info.clone()),
            ObjectInfo::SkKeyframePose(info) => ObjectBeingAdded::SkKeyframePose(info.clone()),
            ObjectInfo::SkAnimator(info) => ObjectBeingAdded::SkAnimator(info.clone()),
            ObjectInfo::PropertyAnimation(info) => ObjectBeingAdded::PropertyAnimation(info.clone()),
            ObjectInfo::PropertyKeyframe(info) => ObjectBeingAdded::PropertyKeyframe(info.clone()),
            ObjectInfo::PropertyAnimator(info) => ObjectBeingAdded::PropertyAnimator(info.clone()),
            ObjectInfo::Tween(info) => ObjectBeingAdded::Tween(info.clone()),
            ObjectInfo::WorldLighting(info) => ObjectBeingAdded::WorldLighting(*info),
        };
        Ok(object_being_added)
    }

    // ---------------------------- Physics --------------------------------

    // Rapier requires a zero-dt physics sync before scene queries when the editor changes objects without running simulation.
    pub fn call_this_before_editor_query(&mut self) {
        self.physics.update_collisions();
    }

    // // Editor callers sync physics once before collecting both placement warning lists.
    // pub fn getOverlapWarnings(&self, max_signed_distance: f32) -> Vec<(ObjectId, ObjectId, f32)> {
    //     self.physics.getOverlapWarnings(max_signed_distance)
    // }

    // pub fn getFloatingPlacementWarnings(&self, contact_tolerance: f32) -> Vec<ObjectId> {
    //     self.physics.getFloatingPlacementWarnings(contact_tolerance)
    // }

    // returns how many steps fit between two tick times; the caller then sets prev_physics_step_time_ms = step_physics_to_time_ms
    // rounded, because two ticks one period apart can measure a hair under dt_ms
    pub fn get_num_physics_steps_to_take(prev_physics_step_time_ms: f64, step_physics_to_time_ms: f64, dt_ms: f32, max_num_steps_per_tick: u64) -> u64 {
        let desired_total_ms_to_step = (step_physics_to_time_ms - prev_physics_step_time_ms).max(0.0);
        let num_steps_rounded = (desired_total_ms_to_step / dt_ms as f64).round() as u64;

        // If a frame stalls for a long time, we intentionally drop excess backlog when capped so simulation
        // does not run at an artificial fast-forward speed for the next few frames.
        num_steps_rounded.min(max_num_steps_per_tick)
    }

    pub fn getGroundSupport(&self, object_id: ObjectId, max_distance_to_ground: f32, max_ground_slope_degrees: f32) -> Option<CastHit> {
        self.physics.getGroundSupport(object_id, max_distance_to_ground, max_ground_slope_degrees)
    }

    pub fn getVelocityRelativeToGround(&self, object_id: ObjectId, max_distance_to_ground: f32, max_ground_slope_degrees: f32) -> Option<Vec3> {
        self.physics
            .getVelocityRelativeToGround(object_id, max_distance_to_ground, max_ground_slope_degrees)
    }

    pub fn castRay<ShouldExcludeObjectFn>(
        &self,
        ray_origin: Vec3,
        ray_direction: Vec3,
        max_num_ray_steps: f32,
        should_include_walkthrough: bool,
        shouldExcludeHitObject: &ShouldExcludeObjectFn,
    ) -> Option<CastHit>
    where
        ShouldExcludeObjectFn: Fn(ObjectId) -> bool,
    {
        self.physics.castRay(
            ray_origin,
            ray_direction,
            max_num_ray_steps,
            should_include_walkthrough,
            &shouldExcludeHitObject,
        )
    }

    fn use_or_compute_relative_location_3d(
        &self,
        object_id: ObjectId,
        group_object_id: ObjectId,
        initial_physics_info_3d: &PhysicsInfo3D,
        init_subitem_relative_location_3d: Option<&Location3D>,
    ) -> anyhow::Result<Location3D> {
        if let Some(init_subitem_relative_location_3d) = init_subitem_relative_location_3d {
            let Location3D { position, quaternion } = init_subitem_relative_location_3d;
            Ok(Location3D {
                position: *position,
                quaternion: *quaternion,
            })
        } else {
            match self
                .physics
                .computeSubitemRelativeLocation3DToGroup(&initial_physics_info_3d.location, group_object_id)
            {
                Ok(relative_location_3d) => Ok(relative_location_3d),
                Err(err) => Err(anyhow::anyhow!("Error computing relative subitem location for {}: {}", object_id, err)),
            }
        }
    }

    // helper - call this after a new object is created
    fn addObjectToPhysicsTracking(&mut self, object_id: ObjectId, objectBeingAdded: &ObjectBeingAdded, init_subitem_relative_location_3d: Option<&Location3D>) {
        // add to physics
        let inRigidGroupId = self.findHighestParentRigidGroup(object_id);
        if let Some(group_object_id) = inRigidGroupId {
            // if we are in a rigidgroup, this and all its children should be added as subitems of the rigidgroup
            match &objectBeingAdded {
                ObjectBeingAdded::Ball(_, ball_primitive_settings, initial_physics_info_3d, initial_physics_toggles_3d) => {
                    // if they gave a relative location, use it, otherwise compute one and use it
                    let relative_location_res =
                        self.use_or_compute_relative_location_3d(object_id, group_object_id, initial_physics_info_3d, init_subitem_relative_location_3d);
                    let relative_location_3d = match relative_location_res {
                        Ok(relative_location_3d) => relative_location_3d,
                        Err(err) => return log::error!("Error computing relative subitem location for ball {}: {}", object_id, err),
                    };

                    let res = self.physics.startTrackingSubitem(
                        object_id,
                        group_object_id,
                        PrimitiveSettings::Ball(*ball_primitive_settings),
                        &relative_location_3d,
                        initial_physics_toggles_3d,
                    );
                    if let Err(err) = res {
                        log::error!("Error starting tracking subitem for ball {}: {}", object_id, err);
                    }
                }
                ObjectBeingAdded::Brick(_, brick_primitive_settings, initial_physics_info_3d, initial_physics_toggles_3d) => {
                    // if they gave a relative location, use it, otherwise compute one and use it
                    let relative_location_res =
                        self.use_or_compute_relative_location_3d(object_id, group_object_id, initial_physics_info_3d, init_subitem_relative_location_3d);
                    let relative_location_3d = match relative_location_res {
                        Ok(relative_location_3d) => relative_location_3d,
                        Err(err) => return log::error!("Error computing relative subitem location for brick {}: {}", object_id, err),
                    };

                    let res = self.physics.startTrackingSubitem(
                        object_id,
                        group_object_id,
                        PrimitiveSettings::Brick(*brick_primitive_settings),
                        &relative_location_3d,
                        initial_physics_toggles_3d,
                    );
                    if let Err(err) = res {
                        log::error!("Error starting tracking subitem for brick {}: {}", object_id, err);
                    }
                }
                ObjectBeingAdded::Cylinder(_, cylinder_primitive_settings, initial_physics_info_3d, initial_physics_toggles_3d) => {
                    let relative_location_res =
                        self.use_or_compute_relative_location_3d(object_id, group_object_id, initial_physics_info_3d, init_subitem_relative_location_3d);
                    let relative_location_3d = match relative_location_res {
                        Ok(relative_location_3d) => relative_location_3d,
                        Err(err) => return log::error!("Error computing relative subitem location for cylinder {}: {}", object_id, err),
                    };

                    let res = self.physics.startTrackingSubitem(
                        object_id,
                        group_object_id,
                        PrimitiveSettings::Cylinder(*cylinder_primitive_settings),
                        &relative_location_3d,
                        initial_physics_toggles_3d,
                    );
                    if let Err(err) = res {
                        log::error!("Error starting tracking subitem for cylinder {}: {}", object_id, err);
                    }
                }
                ObjectBeingAdded::RigidGroup(_, initial_physics_info_3d) => {
                    self.physics
                        .startTrackingGroup(object_id, initial_physics_info_3d, self.shouldObjectBeControlledByUs(object_id));
                }
                _ => {}
            }
        } else {
            // if we are not in a rigidgroup, this is a standalone or group
            match objectBeingAdded {
                ObjectBeingAdded::Ball(_, ball_primitive_settings, initial_physics_info_3d, initial_physics_toggles_3d) => {
                    let isControlledByUs = self.shouldObjectBeControlledByUs(object_id);
                    self.physics.startTrackingStandalone(
                        object_id,
                        PrimitiveSettings::Ball(*ball_primitive_settings),
                        initial_physics_info_3d,
                        initial_physics_toggles_3d,
                        isControlledByUs,
                    );
                }
                ObjectBeingAdded::Brick(_, brick_primitive_settings, initial_physics_info_3d, initial_physics_toggles_3d) => {
                    let isControlledByUs = self.shouldObjectBeControlledByUs(object_id);
                    self.physics.startTrackingStandalone(
                        object_id,
                        PrimitiveSettings::Brick(*brick_primitive_settings),
                        initial_physics_info_3d,
                        initial_physics_toggles_3d,
                        isControlledByUs,
                    );
                }
                ObjectBeingAdded::Cylinder(_, cylinder_primitive_settings, initial_physics_info_3d, initial_physics_toggles_3d) => {
                    let isControlledByUs = self.shouldObjectBeControlledByUs(object_id);
                    self.physics.startTrackingStandalone(
                        object_id,
                        PrimitiveSettings::Cylinder(*cylinder_primitive_settings),
                        initial_physics_info_3d,
                        initial_physics_toggles_3d,
                        isControlledByUs,
                    );
                }
                ObjectBeingAdded::RigidGroup(_, initial_physics_info_3d) => {
                    let isControlledByUs = self.shouldObjectBeControlledByUs(object_id);
                    self.physics.startTrackingGroup(object_id, initial_physics_info_3d, isControlledByUs);
                }
                _ => {}
            }
        }
    }

    fn refreshPhysicsAfterChangeParent(&mut self, object_id: ObjectId) {
        let inRigidGroupId = self.findHighestParentRigidGroup(object_id);
        let Ok(object_kind) = self.getObject(object_id).map(|o| ObjectKind::from_object_info(&o.info)) else {
            return;
        };
        if let Some(group_object_id) = inRigidGroupId {
            // if we are in a rigidgroup, this and all descendants should track the rigid group = group_object_id
            self.refreshAllPrimitiveDescendantsToTrackGroup(object_id, Some(group_object_id));
        } else {
            // if we are not in a rigidgroup, set all descendants to either track this rigid group, or to standalone
            match object_kind {
                ObjectKind::RigidGroup => self.refreshAllPrimitiveDescendantsToTrackGroup(object_id, Some(object_id)),
                _ => self.refreshAllPrimitiveDescendantsToTrackGroup(object_id, None),
            }
        }
    }

    // helper for physics - rigid groups
    fn findHighestParentRigidGroup(self: &World, objectId: ObjectId) -> Option<ObjectId> {
        let mut curr = objectId;
        let mut found = None;
        while let Some(parentId) = self.parentOfId.get(&curr) {
            curr = *parentId;
            let Ok(parentObj) = self.getObject(curr) else {
                break; // should never happen, if an object has a parentid then the parent should exist
            };
            if let ObjectInfo::RigidGroup(_) = &parentObj.info {
                found = Some(curr);
            }
        }
        found
    }

    // refresh *primitive* physics tracking on 1 object. an object never changes from a primitive to a group so we can just update its primitive physics here
    fn refreshObjectPrimitivePhysics(
        &mut self,
        object_id: ObjectId,
        new_physics_info_3d: Option<PhysicsInfo3D>,        // None = keep current
        new_physics_toggles_3d: Option<PhysicsToggles3D>,  // None = keep current
        new_primitive_settings: Option<PrimitiveSettings>, // None = keep current
        new_group_object_id: Option<Option<ObjectId>>,     // Some(None | group_id) means go with inner val, None means go with current
    ) -> anyhow::Result<()> {
        let Some(physics_info_3d) = new_physics_info_3d.or_else(|| self.getObjectPhysicsInfo3D(object_id)) else {
            return Err(anyhow::anyhow!("Missing PhysicsInfo3D for eligible subitem {}", object_id));
        };
        let Some(physics_toggles_3d) = new_physics_toggles_3d.or_else(|| self.getObjectPhysicsToggles3D(object_id)) else {
            return Err(anyhow::anyhow!("Missing PhysicsToggles3D for eligible subitem {}", object_id));
        };
        let Some(primitive_settings) = new_primitive_settings.or_else(|| self.physics.getPrimitiveSettings(object_id)) else {
            return Err(anyhow::anyhow!("Missing PrimitiveSettings for eligible subitem {}", object_id));
        };

        let group_object_id = new_group_object_id.unwrap_or_else(|| self.findHighestParentRigidGroup(object_id));
        self.markObjectChangedForRender(object_id);
        self.physics.stopTrackingObject(object_id);
        if let Some(group_object_id) = group_object_id {
            let relative_location_3d = self.use_or_compute_relative_location_3d(object_id, group_object_id, &physics_info_3d, None)?;

            self.physics
                .startTrackingSubitem(object_id, group_object_id, primitive_settings, &relative_location_3d, &physics_toggles_3d)?;
        } else {
            let is_controlled_by_us = self.shouldObjectBeControlledByUs(object_id);
            self.physics
                .startTrackingStandalone(object_id, primitive_settings, &physics_info_3d, &physics_toggles_3d, is_controlled_by_us);
        }
        Ok(())
    }

    // helper only - if rigid group is None, it sets them to standalones, else sets them to subitems
    fn refreshAllPrimitiveDescendantsToTrackGroup(&mut self, object_id_and_all_descendants: ObjectId, rigid_group_object_id: Option<ObjectId>) {
        let object_id = object_id_and_all_descendants;
        let mut collect_ids_here = Vec::new();
        self.collectAllDescendants(&mut collect_ids_here, object_id, true, &Self::isPrimitive);
        let places_before = self.readPartPlacesForEditHistory(&collect_ids_here);
        for subitem_obj_id in &collect_ids_here {
            if let Err(err) = self.refreshObjectPrimitivePhysics(*subitem_obj_id, None, None, None, Some(rigid_group_object_id)) {
                log::error!("Error refreshing physics tracking for subitem {}: {}", subitem_obj_id, err);
            }
        }
        self.recordRetrackedPartsInEditHistory(object_id, places_before);
    }

    fn isPrimitive(info: &ObjectInfo) -> bool {
        match info {
            ObjectInfo::Ball(_) => true,
            ObjectInfo::Brick(_) => true,
            ObjectInfo::Cylinder(_) => true,
            _ => false,
        }
    }

    fn isPhysical(info: &ObjectInfo) -> bool {
        match info {
            ObjectInfo::Ball(_) => true,
            ObjectInfo::Brick(_) => true,
            ObjectInfo::Cylinder(_) => true,
            ObjectInfo::RigidGroup(_) => true,
            _ => false,
        }
    }

    fn collectAllDescendants(&self, collect_ids_here: &mut Vec<ObjectId>, objectId: ObjectId, includeRoot: bool, filter: &impl Fn(&ObjectInfo) -> bool) {
        if includeRoot {
            let Some(live_obj) = self.getObject(objectId).ok() else {
                return;
            };
            if filter(&live_obj.info) {
                collect_ids_here.push(objectId);
            }
        }
        let Ok(children) = self.getDirectChildrenIds(objectId) else {
            return;
        };
        for &childId in children {
            self.collectAllDescendants(collect_ids_here, childId, true, filter);
        }
    }

    // ---------------------------- Animations --------------------------------

    // This flushes finished weight transitions back into their animator objects.
    pub fn flushAndSetFinishedWeightTransitions(&mut self, est_live_server_sim_ms: f64, live_client_sim_ms: Option<f64>) {
        let mut finished_animator_ids = Vec::new();

        for object_kind in [ObjectKind::SkAnimator, ObjectKind::PropertyAnimator] {
            let Some(animator_ids) = self._kindOfObjectId.get_keys(&object_kind) else {
                continue;
            };
            for animator_id in animator_ids {
                let Some(object) = self.objectOfId.get(animator_id) else {
                    continue;
                };
                let weight_transition = match &object.info {
                    ObjectInfo::SkAnimator(animator) => animator.weightTransition,
                    ObjectInfo::PropertyAnimator(animator) => animator.weightTransition,
                    _ => None,
                };
                let Some(weight_transition) = weight_transition else {
                    continue;
                };
                let is_finished = weight_transition.isFinished(est_live_server_sim_ms, live_client_sim_ms);
                if is_finished {
                    finished_animator_ids.push(*animator_id);
                }
            }
        }

        // mark each finished transition as done - set its weight to the final value, and clear the transition
        for animator_id in finished_animator_ids {
            let Some(animator_object) = self.objectOfId.get_mut(&animator_id) else {
                continue;
            };
            let (weight, weight_transition) = match &mut animator_object.info {
                ObjectInfo::SkAnimator(animator) => (&mut animator.weight, &mut animator.weightTransition),
                ObjectInfo::PropertyAnimator(animator) => (&mut animator.weight, &mut animator.weightTransition),
                _ => continue,
            };
            let Some(weight_transition) = weight_transition.take() else {
                continue;
            };
            *weight = weight_transition.finalValue;
            self.serverMarkObjectAsModified(animator_id, ObjectModifyFlags::from_only_object_info());
        }
    }

    // This resolves the world objects that drive one Skeleton into the shared render Skeleton animations used by physics and redraw.
    pub fn collectRenderSkAnimationsForSkeleton(&self, skeletonId: ObjectId) -> Vec<RenderSkAnimation> {
        collect_render_sk_animations(
            &self.objectOfId,
            skeletonId,
            self._objectsThatPointToId.get(&skeletonId).into_iter().flatten().copied(),
            &self.renderSkKeyframesOfSkAnimationId,
        )
    }

    // get the joint the object should follow
    pub fn getSkJointThatObjectFollows(&self, object_id: ObjectId) -> Option<SkJointToFollow> {
        let rigid_group_id = self.findHighestParentRigidGroup(object_id)?;
        let rigid_group_object = self.objectOfId.get(&rigid_group_id)?;
        let ObjectInfo::RigidGroup(rigid_group) = &rigid_group_object.info else {
            return None;
        };
        let skeleton_id = rigid_group.followSkeletonId.map(|id| id.into())?;

        let pointer_object_ids = self._objectsThatPointToId.get(&object_id)?;
        // TODO: this could be more efficient
        // Use the lowest attachment id, matching render snapshot conversion regardless of hash iteration order.
        let (_, sk_attachment) = pointer_object_ids
            .iter()
            .filter_map(|id| {
                let ObjectInfo::SkAttachment(attachment) = &self.objectOfId.get(id)?.info else {
                    return None;
                };
                (attachment.objectId.map(ObjectId::from) == Some(object_id)).then_some((*id, attachment))
            })
            .min_by_key(|(id, _)| *id)?;

        // a rigid group without physics info cannot follow anything, matching the skip in setRigidGroupsToPoses
        self.getObjectPhysicsInfo3D(rigid_group_id)?;
        Some(SkJointToFollow {
            skeleton_id,
            joint_name: sk_attachment.jointName.clone(),
            offset_from_joint: sk_attachment.offsetFromJoint.clone(),
            follower_group_id: rigid_group_id,
        })
    }

    // get all the rigid group ids that follow a Skeleton
    fn collectRigidGroupIdsFollowingSkeletons(&self) -> HashSet<ObjectId> {
        let mut rigid_group_ids_following_an_skeleton = HashSet::new();
        let Some(rigid_group_ids) = self._kindOfObjectId.get_keys(&ObjectKind::RigidGroup) else {
            return rigid_group_ids_following_an_skeleton;
        };

        for object_id in rigid_group_ids.iter().copied() {
            let Some(object) = self.objectOfId.get(&object_id) else {
                continue;
            };
            let ObjectInfo::RigidGroup(rigid_group) = &object.info else {
                continue;
            };
            if rigid_group.followSkeletonId.is_some() {
                rigid_group_ids_following_an_skeleton.insert(object_id);
            }
        }

        rigid_group_ids_following_an_skeleton
    }

    // for each object following a Skeleton, compute its pose
    fn computePoseOfFollowingObjectId(
        &self,
        rigid_group_ids_following_an_skeleton: &HashSet<ObjectId>,
        est_server_sim_ms_for_animations: f64,
        live_client_sim_ms_for_animations: Option<f64>,
    ) -> anyhow::Result<HashMap<ObjectId, SkeletonPose>> {
        let mut skeleton_ids_followed_by_rigid_groups = HashSet::new();
        for object_id in rigid_group_ids_following_an_skeleton.iter().copied() {
            let rigid_group_obj = self.getObject(object_id)?;
            let ObjectInfo::RigidGroup(rigid_group) = &rigid_group_obj.info else {
                return Err(anyhow::anyhow!("computePoseOfFollowingObjectId: Object {} is not a rigid group", object_id));
            };
            let Some(skeletonId) = rigid_group.followSkeletonId.map(|id| id.into()) else {
                continue;
            };
            skeleton_ids_followed_by_rigid_groups.insert(skeletonId);
        }

        let mut skeleton_pose_of_id = HashMap::new();
        for skeletonId in skeleton_ids_followed_by_rigid_groups.iter().copied() {
            let Some(skeleton_obj) = self.objectOfId.get(&skeletonId) else {
                log::error!("computePoseOfFollowingObjectId: Skeleton {} was not found", skeletonId);
                continue;
            };
            let ObjectInfo::Skeleton(_) = &skeleton_obj.info else {
                log::error!("computePoseOfFollowingObjectId: Object {} is not a Skeleton", skeletonId);
                continue;
            };
            let Some(render_skeleton_info_arc) = self.getRenderSkeletonInfoArc(skeletonId) else {
                log::error!("computePoseOfFollowingObjectId: Render Skeleton info for {} was not found", skeletonId);
                continue;
            };
            let render_skeleton_info = render_skeleton_info_arc.as_ref();
            let render_sk_animations = self.collectRenderSkAnimationsForSkeleton(skeletonId);
            let pose = computeSkeletonPoseAtTime(
                &render_skeleton_info.jointOfIndex,
                &render_sk_animations,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?;
            skeleton_pose_of_id.insert(skeletonId, pose);
        }

        Ok(skeleton_pose_of_id)
    }

    // physically update rigid groups' subitems
    fn setRigidGroupsToPoses(
        &mut self,
        rigid_group_ids_following_an_skeleton: &HashSet<ObjectId>,
        skeleton_pose_of_id: &HashMap<ObjectId, SkeletonPose>,
    ) -> anyhow::Result<()> {
        let sk_attachment_ids = self
            ._kindOfObjectId
            .get_keys(&ObjectKind::SkAttachment)
            .map(|ids| ids.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        for rigid_group_id in rigid_group_ids_following_an_skeleton.iter().copied() {
            let skeletonId = {
                let rigid_group_obj = self.getObject(rigid_group_id)?;
                let ObjectInfo::RigidGroup(rigid_group) = &rigid_group_obj.info else {
                    continue;
                };
                rigid_group.followSkeletonId.map(|id| id.into())
            };
            let Some(skeletonId) = skeletonId else {
                continue;
            };
            let Some(pose) = skeleton_pose_of_id.get(&skeletonId) else {
                continue;
            };
            let joint_binding_of_child_id: Vec<(ObjectId, usize, Location3D)> = {
                let Some(render_skeleton_info_arc) = self.getRenderSkeletonInfoArc(skeletonId) else {
                    continue;
                };
                let jointIndexOfName = &render_skeleton_info_arc.as_ref().jointIndexOfName;

                let mut joint_binding_of_child_id: Vec<(ObjectId, usize, Location3D)> = Vec::new();
                for sk_attachment_id in &sk_attachment_ids {
                    let Some(sk_attachment_object) = self.objectOfId.get(sk_attachment_id) else {
                        continue;
                    };
                    let ObjectInfo::SkAttachment(sk_attachment) = &sk_attachment_object.info else {
                        continue;
                    };
                    let Some(childId) = sk_attachment.objectId.map(ObjectId::from) else {
                        continue;
                    };
                    let Some(child_object) = self.objectOfId.get(&childId) else {
                        continue;
                    };
                    if !Self::isPhysical(&child_object.info) {
                        continue;
                    }
                    if self.findHighestParentRigidGroup(childId) != Some(rigid_group_id) {
                        continue;
                    }
                    let Some(sk_joint_to_follow) = self.getSkJointThatObjectFollows(childId) else {
                        continue;
                    };
                    let Some(&joint_idx) = jointIndexOfName.get(sk_joint_to_follow.joint_name.as_str()) else {
                        continue;
                    };
                    joint_binding_of_child_id.push((childId, joint_idx, sk_joint_to_follow.offset_from_joint));
                }
                joint_binding_of_child_id
            };

            // SkKeyframePoses are relative to the group, so placing the parts needs the group's
            // current location (see computeFollowedSkJointMatrix).
            let Some(follower_group_location) = self.getObjectPhysicsInfo3D(rigid_group_id).map(|physics_info| physics_info.location) else {
                continue;
            };

            for (childId, joint_idx, offset_from_joint) in joint_binding_of_child_id {
                let Some(child_pose) = computeFollowedSkJointMatrix(pose, joint_idx, &offset_from_joint, &follower_group_location) else {
                    continue;
                };
                let (_childScale, childRotation, childTranslation) = child_pose.to_scale_rotation_translation();
                self.setObjectNextPhysicsInfo3D(
                    childId,
                    PhysicsInfo3D {
                        location: Location3D {
                            position: childTranslation,
                            quaternion: childRotation,
                        },
                        movement: Movement3D {
                            velocity: Vec3::ZERO,
                            angularVelocity: Vec3::ZERO,
                        },
                    },
                )?;
                self.physicalObjectIdsMovedByAnimation.insert(childId);
            }
        }

        Ok(())
    }

    // This groups every property animator and tween by the target object property it affects.
    pub fn collectComputedPropertyOverridesOfObjectId(&self) -> HashMap<ObjectId, ComputedPropertyOverrides> {
        collect_property_overrides(
            &self.objectOfId,
            self._kindOfObjectId.get_keys(&ObjectKind::PropertyAnimator).into_iter().flatten().copied(),
            &self.propertyKeyframesArcOfPropertyAnimationId,
            self._kindOfObjectId.get_keys(&ObjectKind::Tween).into_iter().flatten().copied(),
        )
    }

    // Where each object with a Pivot turns and scales, found the same way the renderer finds it.
    pub fn collectPivotOffsetOfObjectId(&self) -> HashMap<ObjectId, Vec3> {
        collect_pivot_offsets(
            &self.objectOfId,
            self._kindOfObjectId.get_keys(&ObjectKind::Pivot).into_iter().flatten().copied(),
        )
    }

    // rotateObjectBy, rotateObjectTo, scaleObjectBy and the rotate_recursively and scale_recursively tools turn and scale an
    // object around this point: its Pivot's point if it has one, otherwise its own position.
    pub fn getPivotPoint(&self, object_id: ObjectId) -> Option<Vec3> {
        let location = self.getObjectPhysicsInfo3D(object_id)?.location;
        // only the objects pointing at this one can be its Pivot
        let pointer_object_ids = self._objectsThatPointToId.get(&object_id).into_iter().flatten().copied();
        let pivot_offset = collect_pivot_offsets(&self.objectOfId, pointer_object_ids)
            .get(&object_id)
            .copied()
            .unwrap_or(Vec3::ZERO);
        Some(location.position + location.quaternion * pivot_offset)
    }

    // An object's rotation is its location.rotation, or a joint's restPose.rotation.
    pub fn getObjectRotation(&self, object_id: ObjectId) -> Option<Quat> {
        match &self.objectOfId.get(&object_id)?.info {
            ObjectInfo::SkJoint(sk_joint_info) => Some(sk_joint_info.restPose.quaternion),
            _ => Some(self.getObjectPhysicsInfo3D(object_id)?.location.quaternion),
        }
    }

    // Like Godot, an object's rotation reads back as the degrees last written to it for as long as they still describe the
    // object's rotation. Otherwise it reads as quat_to_euler_degrees, which can give different angles for the same turn.
    pub fn getRotationDegrees(&self, object_id: ObjectId) -> Option<Vec3> {
        let quaternion = self.getObjectRotation(object_id)?;
        if let Some(written_rotation_degrees) = self.writtenRotationDegreesOfObjectId.get(&object_id) {
            let written_quaternion = euler_degrees_to_quat(*written_rotation_degrees);
            // q and -q are the same turn, and physics hands a rotation back with float noise
            if written_quaternion.abs_diff_eq(quaternion, 1e-5) || written_quaternion.abs_diff_eq(-quaternion, 1e-5) {
                return Some(*written_rotation_degrees);
            }
        }
        Some(quat_to_euler_degrees(quaternion))
    }

    // x, y and z rotation are three property paths but one stored quaternion, and quat_to_euler_degrees only ever
    // reports an x in [-90, 90]. So once an animator tips a rotation past straight up, decomposing the quaternion it
    // just wrote hands back a different triple than the one that went in - with y and z turned by 180. An animator that
    // reads its own output back as its base therefore folds, and a lone pitch animator sweeping a full turn visibly
    // reverses.
    //
    // The fix is to never decompose animated rotation. The angles an object had when rotation animation started are
    // captured once here, and the axes nobody animates are taken from that instead of from the live quaternion.
    //
    // Capturing the base is itself a decompose, though, and it does not always happen while the object is still
    // untouched: a client joining a game in progress builds its own world and captures its own bases, against
    // whatever rotation each object has by then. So the capture has to pick the un-folded branch itself - see
    // canonicalRotationBaseDegrees.
    fn updateRotationAnimationBases(&mut self, computedPropertyOverridesOfObjectId: &HashMap<ObjectId, ComputedPropertyOverrides>) {
        let mut object_ids_with_animated_rotation: HashSet<ObjectId> = HashSet::new();
        for (object_id, computed_property_overrides) in computedPropertyOverridesOfObjectId {
            let has_animated_rotation = computed_property_overrides
                .computed_property_animations
                .iter()
                .any(|(property_path, _)| isRotationAxisPropertyPath(property_path));
            if has_animated_rotation {
                object_ids_with_animated_rotation.insert(*object_id);
            }
        }

        self.rotationAnimationBaseDegreesOfObjectId
            .retain(|object_id, _| object_ids_with_animated_rotation.contains(object_id));

        for object_id in object_ids_with_animated_rotation {
            if self.rotationAnimationBaseDegreesOfObjectId.contains_key(&object_id) {
                continue;
            }
            // only physical objects and joints have a rotation to animate; a location write on anything else is already a no-op.
            let Some(quaternion) = self.getObjectRotation(object_id) else {
                continue;
            };
            let rotation_degrees = canonicalRotationBaseDegrees(quat_to_euler_degrees(quaternion));
            self.rotationAnimationBaseDegreesOfObjectId.insert(object_id, rotation_degrees);
        }
    }

    // ---------------------------- Tweens --------------------------------

    // Starts a tween on one of an object's numbers; see TweenInfo. current_value is what a script reads that number as now.
    // A tween already playing on the same number is changed into the new one, which starts from wherever it had got to.
    // Returns the Tween object, which sits at the world root and deletes itself once it has played out.
    pub fn tweenTo(
        &mut self,
        object_id: ObjectId,
        property_path: Vec<String>,
        current_value: f32,
        target_value: f32,
        duration_ms: f32,
        interpolationStyle: InterpolationStyle,
        time_location: ObjectLocation,
        est_live_server_sim_ms: f64,
        live_client_sim_ms: Option<f64>,
    ) -> anyhow::Result<ObjectId> {
        if !target_value.is_finite() || !duration_ms.is_finite() || duration_ms < 0.0 {
            return Err(anyhow::anyhow!(
                "tweenTo: the target must be a number, and the duration a number that is not negative"
            ));
        }
        // Writing the number its own value checks that this object can write it, since a tween that can't would stop every
        // physics step.
        rustUpdateObject(
            self,
            object_id,
            ValueWithPath::new(&jsonNumber(current_value)?, &property_path),
            SetOrForce::Set,
        )?;

        let playing_tween = self._objectsThatPointToId.get(&object_id).into_iter().flatten().find_map(|pointer_object_id| {
            if !self.isObjectCreatedByUs(*pointer_object_id) {
                return None;
            }
            let ObjectInfo::Tween(tween) = &self.objectOfId.get(pointer_object_id)?.info else {
                return None;
            };
            if tween.propertyPath != property_path {
                return None;
            }
            let animation_time_secs = tween
                .clockState
                .readTimeSecs(1.0, false, tween.duration, est_live_server_sim_ms, live_client_sim_ms);
            let tween_keyframes_arc = computed_property_animation_of_tween(tween).property_keyframes_arc;
            let tween_value = getCurrentPropertyValue(&tween_keyframes_arc, tween.interpolationStyle, animation_time_secs).unwrap_or(current_value);
            Some((*pointer_object_id, tween_value))
        });
        let tween_info = TweenInfo {
            targetObjectId: Some(ObjectIdPointer(object_id)),
            propertyPath: property_path,
            fromValue: playing_tween.map(|(_, tween_value)| tween_value).unwrap_or(current_value),
            toValue: target_value,
            duration: duration_ms / 1000.0,
            interpolationStyle,
            clockState: ClockState::new_clock_state(0.0, 1.0, time_location, est_live_server_sim_ms, live_client_sim_ms)?,
        };
        match playing_tween {
            Some((tween_id, _)) => {
                self.setObjectInfo(tween_id, ObjectInfo::Tween(tween_info))?;
                Ok(tween_id)
            }
            None => {
                let root_object_id = self.getRootObjectId();
                Ok(self.addObject("Tween".to_string(), ObjectBeingAdded::Tween(tween_info), root_object_id))
            }
        }
    }

    // The editor never plays the game, so the tweens a temporary script starts there end at their targets right away.
    pub fn endTweensAtTheirTargets(&mut self) -> anyhow::Result<()> {
        let tween_ids: Vec<ObjectId> = self._kindOfObjectId.get_keys(&ObjectKind::Tween).into_iter().flatten().copied().collect();
        for tween_id in tween_ids {
            let ObjectInfo::Tween(TweenInfo {
                targetObjectId,
                propertyPath,
                toValue,
                ..
            }) = &self.getObject(tween_id)?.info
            else {
                continue;
            };
            let (target_object_id, property_path, to_value) = (targetObjectId.as_ref().map(ObjectId::from), propertyPath.clone(), *toValue);
            if let Some(target_object_id) = target_object_id {
                let objectModifyFlags = rustUpdateObject(
                    self,
                    target_object_id,
                    ValueWithPath::new(&jsonNumber(to_value)?, &property_path),
                    SetOrForce::Set,
                )?;
                self.serverMarkObjectAsModified(target_object_id, objectModifyFlags);
            }
            self.removeObject(tween_id)?;
        }
        Ok(())
    }

    // This writes computed property override values into the live world without marking them for replication.
    // It returns what it changed on each object, for a caller that does want to send those values.
    fn setObjectsToPropertyOverrides(
        &mut self,
        propertyOverrideOfPathOfObjectId: &HashMap<ObjectId, HashMap<Vec<String>, f32>>,
        pivot_offset_of_object_id: &HashMap<ObjectId, Vec3>,
    ) -> anyhow::Result<HashMap<ObjectId, ObjectModifyFlags>> {
        let mut objectModifyFlagsOfObjectId: HashMap<ObjectId, ObjectModifyFlags> = HashMap::new();
        for (object_id, propertyOverrideOfPath) in propertyOverrideOfPathOfObjectId {
            // Every rotation axis is written in one go, so a per-axis write can never build on an axis another one
            // just changed. computePropertyOverridesOfObjectIdAtTime has already filled in the axes nobody animates.
            let mut rotation_field: Option<&str> = None;
            let mut rotationOverrideOfAxis: HashMap<&str, f32> = HashMap::new();
            let mut restPositionOverrideOfAxis: HashMap<&str, f32> = HashMap::new();
            let pivot_offset = pivot_offset_of_object_id.get(object_id).copied();
            let mut is_position_overridden = false;
            for (property_path, property_override) in propertyOverrideOfPath {
                if let Some((field, rotation_axis)) = rotationAxisOfPropertyPath(property_path) {
                    rotation_field = Some(field);
                    rotationOverrideOfAxis.insert(rotation_axis, *property_override);
                    continue;
                }
                // A joint's rest position is written whole below, so its Skeleton is rebuilt once instead of once per axis.
                if let [field, subfield, axis] = property_path.as_slice()
                    && field == "restPose"
                    && subfield == "position"
                {
                    restPositionOverrideOfAxis.insert(axis.as_str(), *property_override);
                    continue;
                }
                // An object with a Pivot gets its whole position worked out below, from the pivot and the new rotation.
                let is_position_axis = match property_path.as_slice() {
                    [field, subfield, _] => field == "location" && subfield == "position",
                    _ => false,
                };
                if pivot_offset.is_some() && is_position_axis {
                    is_position_overridden = true;
                    continue;
                }
                // A path its target can't take only skips that animator, instead of stopping the whole world from stepping.
                match rustUpdateObject(
                    self,
                    *object_id,
                    ValueWithPath::new(&jsonNumber(*property_override)?, property_path),
                    SetOrForce::Set,
                ) {
                    Ok(objectModifyFlags) => objectModifyFlagsOfObjectId.entry(*object_id).or_default().merge_with(&objectModifyFlags),
                    Err(error) => log::warn!("Skipped an animator write to object {object_id} at {property_path:?}: {error}"),
                }
            }

            // Turning around a Pivot moves the object too, so its position comes from the pivot and the new rotation, by
            // the same rule the renderer uses, and is written whole. Its rotation is written below like any other object's.
            if let Some(pivot_offset) = pivot_offset
                && (is_position_overridden || !rotationOverrideOfAxis.is_empty())
                && let Some(physics_info_3d) = self.getObjectPhysicsInfo3D(*object_id)
            {
                let mut location = physics_info_3d.location;
                applyLocationPropertyOverridesOfPath(propertyOverrideOfPath, &mut location, pivot_offset);
                let position_property_path = ["location".to_owned(), "position".to_owned()];
                let valueOfAxis = Map::from_iter([
                    ("x".to_owned(), jsonNumber(location.position.x)?),
                    ("y".to_owned(), jsonNumber(location.position.y)?),
                    ("z".to_owned(), jsonNumber(location.position.z)?),
                ]);
                let objectModifyFlags = rustUpdateObject(
                    self,
                    *object_id,
                    ValueWithPath::new(&Value::Object(valueOfAxis), &position_property_path),
                    SetOrForce::Set,
                )?;
                objectModifyFlagsOfObjectId.entry(*object_id).or_default().merge_with(&objectModifyFlags);
            }

            if !restPositionOverrideOfAxis.is_empty()
                && let Some(ObjectInfo::SkJoint(sk_joint_info)) = self.objectOfId.get(object_id).map(|object| &object.info)
            {
                let rest_position = sk_joint_info.restPose.position;
                let valueOfAxis = Map::from_iter([
                    (
                        "x".to_owned(),
                        jsonNumber(restPositionOverrideOfAxis.get("x").copied().unwrap_or(rest_position.x))?,
                    ),
                    (
                        "y".to_owned(),
                        jsonNumber(restPositionOverrideOfAxis.get("y").copied().unwrap_or(rest_position.y))?,
                    ),
                    (
                        "z".to_owned(),
                        jsonNumber(restPositionOverrideOfAxis.get("z").copied().unwrap_or(rest_position.z))?,
                    ),
                ]);
                let rest_position_property_path = ["restPose".to_owned(), "position".to_owned()];
                let objectModifyFlags = rustUpdateObject(
                    self,
                    *object_id,
                    ValueWithPath::new(&Value::Object(valueOfAxis), &rest_position_property_path),
                    SetOrForce::Set,
                )?;
                objectModifyFlagsOfObjectId.entry(*object_id).or_default().merge_with(&objectModifyFlags);
            }

            // All three or none: an object with no rotation base is one with no physics to rotate, and a location
            // write on that is already a no-op - so there is nothing here worth reconstructing an angle for.
            let mut valueOfAxis = Map::new();
            for rotation_axis in ["x", "y", "z"] {
                let Some(rotation_degrees) = rotationOverrideOfAxis.get(rotation_axis) else {
                    break;
                };
                valueOfAxis.insert(rotation_axis.to_owned(), jsonNumber(*rotation_degrees)?);
            }
            if valueOfAxis.len() != 3 {
                continue;
            }
            let Some(rotation_field) = rotation_field else {
                continue;
            };
            let rotation_property_path = [rotation_field.to_owned(), "rotation".to_owned()];
            match rustUpdateObject(
                self,
                *object_id,
                ValueWithPath::new(&Value::Object(valueOfAxis), &rotation_property_path),
                SetOrForce::Set,
            ) {
                Ok(objectModifyFlags) => objectModifyFlagsOfObjectId.entry(*object_id).or_default().merge_with(&objectModifyFlags),
                Err(error) => log::warn!("Skipped an animator write to object {object_id} at {rotation_property_path:?}: {error}"),
            }
        }

        for (object_id, objectModifyFlags) in &objectModifyFlagsOfObjectId {
            if objectModifyFlags.modifiedPhysicsInfo3D || objectModifyFlags.modifiedPhysicsToggles3D || objectModifyFlags.modifiedPrimitiveSettings {
                self.physicalObjectIdsMovedByAnimation.insert(*object_id);
            }
        }
        Ok(objectModifyFlagsOfObjectId)
    }

    // Samples the skeletons for one step of a batch and poses the rigid groups following them. Shares the per-step
    // timing of applyPropertyOverridesForStep below, and for the same reason: these write kinematic targets too, so a
    // batch that posed them once left every step after the first deriving a velocity that described no motion.
    fn applySkeletonPosesForStep(
        &mut self,
        rigid_group_ids_following_an_skeleton: &HashSet<ObjectId>,
        steps_before_end: u64,
        dt_secs: f32,
        est_server_sim_ms_for_animations: f64,
        live_client_sim_ms_for_animations: Option<f64>,
    ) -> anyhow::Result<()> {
        let offset_ms = steps_before_end as f64 * dt_secs as f64 * 1000.0;
        let skeleton_pose_of_id = self.computePoseOfFollowingObjectId(
            rigid_group_ids_following_an_skeleton,
            est_server_sim_ms_for_animations - offset_ms,
            live_client_sim_ms_for_animations.map(|live_client_sim_ms| live_client_sim_ms - offset_ms),
        )?;
        self.setRigidGroupsToPoses(rigid_group_ids_following_an_skeleton, &skeleton_pose_of_id)
    }

    // Samples the animators for one step of a batch and writes the result into the world. `steps_before_end` counts
    // back from the batch's own time, so the last step of a batch lands exactly on it.
    //
    // A whole catch-up batch used to share one sample, which gave its first step all of the batch's motion and the
    // rest none. Rapier derives a kinematic body's velocity from how far it moves during a step, so the derived
    // velocity spiked and then fell to zero while the pose advanced evenly - and anything reading that velocity, from
    // a player standing on the platform to the renderer, was reading motion that did not happen.
    fn applyPropertyOverridesForStep(
        &mut self,
        computedPropertyOverridesOfObjectId: &HashMap<ObjectId, ComputedPropertyOverrides>,
        pivot_offset_of_object_id: &HashMap<ObjectId, Vec3>,
        steps_before_end: u64,
        dt_secs: f32,
        est_server_sim_ms_for_animations: f64,
        live_client_sim_ms_for_animations: Option<f64>,
    ) -> anyhow::Result<HashMap<ObjectId, ObjectModifyFlags>> {
        let offset_ms = steps_before_end as f64 * dt_secs as f64 * 1000.0;
        let propertyOverrideOfPathOfObjectId = computePropertyOverridesOfObjectIdAtTime(
            computedPropertyOverridesOfObjectId,
            &self.rotationAnimationBaseDegreesOfObjectId,
            est_server_sim_ms_for_animations - offset_ms,
            live_client_sim_ms_for_animations.map(|live_client_sim_ms| live_client_sim_ms - offset_ms),
        );
        self.setObjectsToPropertyOverrides(&propertyOverrideOfPathOfObjectId, pivot_offset_of_object_id)
    }

    // step physics and all Skeletons/blocks following them
    pub fn step_physics(
        &mut self,
        num_steps: u64,
        dt_secs: f32,
        est_server_sim_ms_for_animations: f64,
        live_client_sim_ms_for_animations: Option<f64>,
    ) -> anyhow::Result<()> {
        // Which rigid groups follow a skeleton, and which animators target which property, are both config rather
        // than time, so collect them once for the whole batch.
        let rigid_group_ids_following_an_skeleton = self.collectRigidGroupIdsFollowingSkeletons();
        let computedPropertyOverridesOfObjectId = self.collectComputedPropertyOverridesOfObjectId();
        let pivot_offset_of_object_id = self.collectPivotOffsetOfObjectId();

        // The base an animated rotation folds around is captured when the animation starts, so it belongs to the
        // whole batch - sampling it per step would let it drift with the very rotation it is the reference for.
        self.updateRotationAnimationBases(&computedPropertyOverridesOfObjectId);

        // A joint's animators write its rest pose before the Skeletons are posed, so the parts following it move in the same step.
        let (computedPropertyOverridesOfJointId, computedPropertyOverridesOfObjectId): (HashMap<_, _>, HashMap<_, _>) = computedPropertyOverridesOfObjectId
            .into_iter()
            .partition(|(object_id, _)| match self._kindOfObjectId.get(object_id) {
                Some(ObjectKind::SkJoint) => true,
                _ => false,
            });

        // what the last step's animations wrote on each object, which a tween that has played out sends
        let mut objectModifyFlagsOfObjectId: HashMap<ObjectId, ObjectModifyFlags> = HashMap::new();

        // Nothing steps, so there is no step to sample for. Hold the animations at the tick's own time.
        if num_steps == 0 {
            objectModifyFlagsOfObjectId = self.applyPropertyOverridesForStep(
                &computedPropertyOverridesOfJointId,
                &pivot_offset_of_object_id,
                0,
                dt_secs,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?;
            self.applySkeletonPosesForStep(
                &rigid_group_ids_following_an_skeleton,
                0,
                dt_secs,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?;
            objectModifyFlagsOfObjectId.extend(self.applyPropertyOverridesForStep(
                &computedPropertyOverridesOfObjectId,
                &pivot_offset_of_object_id,
                0,
                dt_secs,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?);
        }

        // Step physics by dt. Nothing may step with dt=0 first: rapier derives a kinematic body's velocity from how
        // far it moves toward its target during a step, and a dt=0 step consumes that move while dividing by an
        // inv_dt of 0. The target is reached, every kinematic velocity is zeroed, and the real step is left with
        // nothing to derive from - which is why animated platforms stopped carrying anything standing on them.
        let mut collision_pair_events = Vec::new();
        for step_index in 0..num_steps {
            let steps_before_end = num_steps - 1 - step_index;
            objectModifyFlagsOfObjectId = self.applyPropertyOverridesForStep(
                &computedPropertyOverridesOfJointId,
                &pivot_offset_of_object_id,
                steps_before_end,
                dt_secs,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?;
            self.applySkeletonPosesForStep(
                &rigid_group_ids_following_an_skeleton,
                steps_before_end,
                dt_secs,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?;
            // Keep kinematic objects moving. This has to land before the animators so that an axis an animator
            // writes lands on exactly the animated value, instead of that value plus one tick of velocity.
            self.physics.applyContinuedKinematicMovement(dt_secs);
            objectModifyFlagsOfObjectId.extend(self.applyPropertyOverridesForStep(
                &computedPropertyOverridesOfObjectId,
                &pivot_offset_of_object_id,
                steps_before_end,
                dt_secs,
                est_server_sim_ms_for_animations,
                live_client_sim_ms_for_animations,
            )?);
            collision_pair_events.extend(self.physics.step(dt_secs));
            self.teleportedObjectIdsSinceStep.clear();
        }

        // A tween that has played out has left its number at the target, and the machine that made it deletes it. The server
        // also sends that number like any other change: clients delete a server tween before they step, so one could otherwise
        // stop a step short of the target, and a client that is behind might never have got the tween at all.
        let finished_tweens: Vec<(ObjectId, Option<ObjectId>)> = self
            ._kindOfObjectId
            .get_keys(&ObjectKind::Tween)
            .into_iter()
            .flatten()
            .copied()
            .filter_map(|tween_id| match self.objectOfId.get(&tween_id).map(|tween_object| &tween_object.info) {
                Some(ObjectInfo::Tween(tween))
                    if self.isObjectCreatedByUs(tween_id)
                        && tween
                            .clockState
                            .readRawTimeMs(1.0, est_server_sim_ms_for_animations, live_client_sim_ms_for_animations)
                            >= tween.duration as f64 * 1000.0 =>
                {
                    Some((tween_id, tween.targetObjectId.as_ref().map(ObjectId::from)))
                }
                _ => None,
            })
            .collect();
        for (tween_id, target_object_id) in finished_tweens {
            if let Some(target_object_id) = target_object_id
                && let Some(objectModifyFlags) = objectModifyFlagsOfObjectId.remove(&target_object_id)
            {
                self.serverMarkObjectAsModified(target_object_id, objectModifyFlags);
            }
            self.removeObject(tween_id)?;
        }

        if self.worldControlMode.should_call_collision_callbacks() {
            for collision_pair_event in collision_pair_events {
                match collision_pair_event {
                    CollisionPairEvent::Started { object_id_1, object_id_2 } => {
                        self.fireCollisionStartedJsCallbacks(object_id_1, object_id_2);
                    }
                    CollisionPairEvent::Stopped { object_id_1, object_id_2 } => {
                        self.fireCollisionStoppedJsCallbacks(object_id_1, object_id_2);
                    }
                }
            }
        }

        Ok(())
    }

    // ---------------------------- Server --------------------------------

    // Game networking and the editor use the same builders to read delta values from the world.
    pub fn buildAddDelta(&self, id: ObjectId) -> anyhow::Result<ObjectAddDelta> {
        let obj = self.getObject(id)?;
        // should never happen - the object has no parent (is the root), since we filter this out when creating world
        let parent_id = *self.parentOfId.get(&id).ok_or_else(|| anyhow::anyhow!("Object {id} has no parent"))?;
        // should never happen if world data structure is working properly
        let object_being_added = self.cloneObjectAsObjectBeingAdded(id)?;
        Ok(ObjectAddDelta {
            name: obj.name.clone(),
            objectBeingAdded: object_being_added,
            parentId: parent_id,
            initSubitemRelativeLocation3D: self.physics.getSubitemRelativeLocation3D(id),
            teleportCount: self.getTeleportCount(id),
        })
    }

    // Build world-coordinate transforms; game networking handles its relative rigid-group poses.
    pub fn buildModifyDelta(&self, id: ObjectId, flags: &ObjectModifyFlags) -> anyhow::Result<ObjectModifyDelta> {
        let obj = self.getObject(id)?;
        Ok(ObjectModifyDelta {
            modifiedObjectInfo: someIffTrue(flags.modifiedObjectInfo, || obj.info.clone()),
            modifiedPrimitiveSettings: someIffTrue(flags.modifiedPrimitiveSettings, || self.getObjectPrimitiveSettings(id)).flatten(),
            modifiedPhysicsToggles3D: someIffTrue(flags.modifiedPhysicsToggles3D, || self.getObjectPhysicsToggles3D(id)).flatten(),
            modifiedPhysicsInfo3D: someIffTrue(flags.modifiedPhysicsInfo3D, || self.getObjectPhysicsInfo3D(id))
                .flatten()
                .map(|physics_info_3d| NetworkedPhysicsInfo3D {
                    coords: NetworkedCoords::WorldCoords(physics_info_3d),
                    teleportCount: self.getTeleportCount(id),
                }),
            modifiedParent: someIffTrue(flags.modifiedParent, || self.parentOfId.get(&id).copied()).flatten(),
            modifiedName: someIffTrue(flags.modifiedName, || obj.name.clone()),
        })
    }

    pub fn flushDirtyObjects(&mut self) -> WorldDirtyObjectFlags {
        let dirtyPhysicsObjectIds = self.physics.takeDirtyPhysicsObjectsControlledByUs();
        let mut modifiedObjectFlags = std::mem::take(&mut self.modifiedObjectFlags);
        let addedObjectIds = std::mem::take(&mut self.addedObjectIds);
        let removedObjectIds = std::mem::take(&mut self.removedObjectIds);

        // merge physics objects in - aka dirtyPhysicsObjectIds
        for object_id in dirtyPhysicsObjectIds {
            if self.physicalObjectIdsMovedByAnimation.contains(&object_id) {
                continue;
            }
            let entry = modifiedObjectFlags.entry(object_id).or_insert_with(ObjectModifyFlags::default);
            entry.modifiedPhysicsInfo3D = true;
        }
        self.physicalObjectIdsMovedByAnimation.clear();

        WorldDirtyObjectFlags {
            modifiedObjectFlags,
            addedObjectIds,
            removedObjectIds,
        }
    }

    // returns all the changed object ids
    pub fn step_server(&mut self, num_steps: u64, dt_secs: f32, live_server_sim_ms_for_animations: f64) -> anyhow::Result<()> {
        self.step_physics(num_steps, dt_secs, live_server_sim_ms_for_animations, None)
    }

    pub fn flushDirtyScripts(&mut self) -> (HashSet<ObjectId>, HashSet<ObjectId>) {
        let addedScriptIds = std::mem::take(&mut self.readyToAddScriptIds);
        let removedScriptIds = std::mem::take(&mut self.readyToRemoveScriptIds);
        (addedScriptIds, removedScriptIds)
    }

    // ---------------------------- World -> ObjectWithChildren --------------------------------
    pub fn toPrettyObjectWithChildren(&self) -> anyhow::Result<ObjectWithChildren> {
        fn build_node(world: &World, object_id: ObjectId, visiting: &mut HashSet<ObjectId>) -> anyhow::Result<ObjectWithChildren> {
            if !visiting.insert(object_id) {
                return Err(anyhow::anyhow!("Cycle detected while building ObjectWithChildren at object {}", object_id));
            }

            let obj = world.getObject(object_id)?;
            let physics_info = world.getObjectPhysicsInfo3D(object_id); // Option<PhysicsInfo3D>
            let parent_id = world.parentOfId.get(&object_id).copied();

            let mut children = Vec::new();
            let child_ids = world.getDirectChildrenIds(object_id)?;
            for child_id in child_ids.iter() {
                children.push(build_node(world, *child_id, visiting)?);
            }

            visiting.remove(&object_id);
            Ok(ObjectWithChildren {
                id: obj.id,
                name: obj.name.clone(),
                parent_id,
                info: obj.info.clone(),
                physics_info,
                children,
            })
        }

        let mut visiting = HashSet::new();
        build_node(self, self.getRootObjectId(), &mut visiting)
    }

    // ---------------------------- World <-> StoredWorld --------------------------------

    // Uploading a world file swaps out everything under the root. The root keeps its id and every other
    // object gets a new one, so pointers to the other world's root land on this root.
    pub fn loadNewWorld(&mut self, other: &World) -> anyhow::Result<()> {
        let root_template_item = other.createTemplateFrom(other.getRootObjectId())?;
        self.refreshAnimationKeyframesOnceAfter(|world| {
            let root_id = world.getRootObjectId();
            let old_child_ids = world.getDirectChildrenIds(root_id)?.iter().copied().collect::<Vec<_>>();
            for old_child_id in old_child_ids {
                world.removeObject(old_child_id)?;
            }
            let mut cloned_object_id_of_source_object_id = HashMap::from([(root_template_item.sourceObjectId, root_id)]);
            for child_template_item in root_template_item.children.iter() {
                world.addObjectFromTemplateItem(child_template_item, root_id, &mut cloned_object_id_of_source_object_id)?;
            }
            world.rewireTemplatePointers(&root_template_item, &cloned_object_id_of_source_object_id)
        })?;
        self.fallback_camera_location = other.fallback_camera_location;
        Ok(())
    }

    // we only need this for edit mode, not live mode
    pub fn toStoredWorld(&self) -> StoredWorld {
        let mut physicsInfo3DOfId = HashMap::new();
        let mut initPhysicsToggles3DOfId = HashMap::new();
        for (objectId, _) in self.objectOfId.iter() {
            let physicsInfo3D = self.getObjectPhysicsInfo3D(*objectId);
            if let Some(physicsInfo3D) = physicsInfo3D {
                physicsInfo3DOfId.insert(*objectId, physicsInfo3D);
            }
        }
        for (objectId, _) in self.objectOfId.iter() {
            let physicsToggles3D = self.getObjectPhysicsToggles3D(*objectId);
            if let Some(physicsToggles3D) = physicsToggles3D {
                initPhysicsToggles3DOfId.insert(*objectId, physicsToggles3D);
            }
        }

        StoredWorld {
            worldSchemaVersion: STORED_WORLD_CURRENT_VERSION,
            fallback_camera_location: self.fallback_camera_location,
            objectOfId: self.objectOfId.clone(),
            childrenOfId: self
                ._childrenOfId
                .iter()
                .map(|(objectId, children)| (*objectId, children.iter().map(|id| *id).collect()))
                .collect(),
            rootObjectId: self._rootObjectId,
            physicsInfo3DOfId,
            physicsToggles3DOfId: initPhysicsToggles3DOfId,
            scriptRawCodeOfId: self.scriptRawCodeOfId.clone(),
            primitiveSettingsOfId: self.physics.primitiveSettingsOfId.clone(),
        }
    }

    pub fn toStoredWorldString(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(&self.toStoredWorld())?)
    }

    // we need this for both edit mode and live mode. Every world is checked before anything loads it. The loader skips
    // objects it cannot build, so a world that loses any object on the way is refused instead.
    pub fn fromStoredWorldString(
        storedWorldString: &str,
        worldControlMode: WorldControlMode,
        js_callback_inputs_tx: Option<mpsc::UnboundedSender<JsCallbackInputs>>,
    ) -> anyhow::Result<Self> {
        let mut storedWorldJson = serde_json::from_str::<serde_json::Value>(storedWorldString)?;
        migrateStoredWorld::updateToVersion(
            &mut storedWorldJson,
            STORED_WORLD_CURRENT_VERSION,
            migrateStoredWorld::StoredWorldKind::GameWorld,
        )?;
        let storedWorld = serde_json::from_value::<StoredWorld>(storedWorldJson)?;

        // Walk the hierarchy without recursion, since its depth is exactly what is being checked.
        let mut reachable_ids = HashSet::from([storedWorld.rootObjectId]);
        let mut pending = vec![(storedWorld.rootObjectId, 0)];
        while let Some((id, depth)) = pending.pop() {
            anyhow::ensure!(depth <= MAX_WORLD_DEPTH, "Objects are nested more than {MAX_WORLD_DEPTH} deep.");
            for child_id in storedWorld.childrenOfId.get(&id).into_iter().flatten() {
                anyhow::ensure!(reachable_ids.insert(*child_id), "Object {child_id} appears more than once.");
                pending.push((*child_id, depth + 1));
            }
        }

        for (id, physics_info_3d) in &storedWorld.physicsInfo3DOfId {
            physics_info_3d.validate().map_err(|error| error.context(format!("Object {id}")))?;
        }
        for (id, physics_toggles_3d) in &storedWorld.physicsToggles3DOfId {
            physics_toggles_3d.validate().map_err(|error| error.context(format!("Object {id}")))?;
        }
        for (id, primitive_settings) in &storedWorld.primitiveSettingsOfId {
            primitive_settings.validate().map_err(|error| error.context(format!("Object {id}")))?;
        }
        let camera = storedWorld.fallback_camera_location;
        anyhow::ensure!(
            camera.position.is_finite() && camera.orientation.theta.is_finite() && camera.orientation.phi.is_finite(),
            "The camera must be finite."
        );

        // -------------------- visitAll helper to iterate thru all objects --------------------
        fn visitAll(
            oldObjectId: ObjectId,
            oldWorld: &StoredWorld,
            newParentId: ObjectId,
            world: &mut World,
            newOfOld: &mut HashMap<ObjectId, ObjectId>,
            trusts_loaded_ids: bool,
        ) -> anyhow::Result<()> {
            if newOfOld.contains_key(&oldObjectId) {
                return Ok(());
            }

            let oldObject = oldWorld.objectOfId.get(&oldObjectId);
            let Some(oldObject) = oldObject else {
                return Err(anyhow::anyhow!("visitAll: Object not found in stored world"));
            };

            let oldName = oldObject.name.clone();
            let oldObjInfo = oldObject.info.clone();

            let initialPhysicsInfo3D = oldWorld.physicsInfo3DOfId.get(&oldObjectId).cloned();
            let initialPhysicsToggles3D = oldWorld.physicsToggles3DOfId.get(&oldObjectId).cloned();
            let initialPrimitiveSettings = oldWorld.primitiveSettingsOfId.get(&oldObjectId).cloned();

            let initialRawCode = oldWorld.scriptRawCodeOfId.get(&oldObjectId).map(|code| code.clone());

            // add the object, except remove any old ids it points to. We fill them in later once we have the corresponding new ids
            let mut addObjectFn = |name: String, objectBeingAdded: ObjectBeingAdded| -> anyhow::Result<ObjectId> {
                if trusts_loaded_ids {
                    world._addObjectWithId(oldObjectId, name, objectBeingAdded, Some(newParentId), None);
                    let newObjectId = oldObjectId;
                    Ok(newObjectId)
                } else {
                    let newObjectId = world._addObjectAutoId(name, objectBeingAdded, Some(newParentId));
                    Ok(newObjectId)
                }
            };

            let newObjectId: ObjectId = match &oldObjInfo {
                ObjectInfo::Ball(oldInfo) => {
                    let initialPhysicsInfo3D = initialPhysicsInfo3D.ok_or_else(|| anyhow::anyhow!("Physics info not found for object {}", oldObjectId))?;
                    let initialPhysicsToggles3D =
                        initialPhysicsToggles3D.ok_or_else(|| anyhow::anyhow!("Physics toggles not found for object {}", oldObjectId))?;

                    let initialPrimitiveSettings =
                        initialPrimitiveSettings.ok_or_else(|| anyhow::anyhow!("Primitive info not found for object {}", oldObjectId))?;
                    let ball_primitive_settings = match initialPrimitiveSettings {
                        PrimitiveSettings::Ball(info) => info,
                        _ => return Err(anyhow::anyhow!("Ball primitive info not found for object {}", oldObjectId)),
                    };

                    let newInfo = BallInfo { ..oldInfo.clone() };
                    let newId = addObjectFn(
                        oldName,
                        ObjectBeingAdded::Ball(newInfo, ball_primitive_settings, initialPhysicsInfo3D, initialPhysicsToggles3D),
                    )?;
                    newId
                }
                ObjectInfo::SkJoint(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SkJoint(newInfo))?;
                    newId
                }
                ObjectInfo::Skeleton(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Skeleton(newInfo))?;
                    newId
                }
                ObjectInfo::Brick(oldInfo) => {
                    let initialPhysicsInfo3D = initialPhysicsInfo3D.ok_or_else(|| anyhow::anyhow!("Physics info not found for object {}", oldObjectId))?;
                    let initialPhysicsToggles3D =
                        initialPhysicsToggles3D.ok_or_else(|| anyhow::anyhow!("Physics toggles not found for object {}", oldObjectId))?;

                    let initialPrimitiveSettings =
                        initialPrimitiveSettings.ok_or_else(|| anyhow::anyhow!("Primitive info not found for object {}", oldObjectId))?;
                    let brick_primitive_settings = match initialPrimitiveSettings {
                        PrimitiveSettings::Brick(info) => info,
                        _ => return Err(anyhow::anyhow!("Brick primitive info not found for object {}", oldObjectId)),
                    };
                    let newInfo = BrickInfo { ..oldInfo.clone() };
                    let newId = addObjectFn(
                        oldName,
                        ObjectBeingAdded::Brick(newInfo, brick_primitive_settings, initialPhysicsInfo3D, initialPhysicsToggles3D),
                    )?;
                    newId
                }
                ObjectInfo::Cylinder(oldInfo) => {
                    let initialPhysicsInfo3D = initialPhysicsInfo3D.ok_or_else(|| anyhow::anyhow!("Physics info not found for object {}", oldObjectId))?;
                    let initialPhysicsToggles3D =
                        initialPhysicsToggles3D.ok_or_else(|| anyhow::anyhow!("Physics toggles not found for object {}", oldObjectId))?;

                    let initialPrimitiveSettings =
                        initialPrimitiveSettings.ok_or_else(|| anyhow::anyhow!("Primitive info not found for object {}", oldObjectId))?;
                    let cylinder_primitive_settings = match initialPrimitiveSettings {
                        PrimitiveSettings::Cylinder(info) => info,
                        _ => return Err(anyhow::anyhow!("Cylinder primitive info not found for object {}", oldObjectId)),
                    };
                    let newInfo = CylinderInfo { ..oldInfo.clone() };
                    let newId = addObjectFn(
                        oldName,
                        ObjectBeingAdded::Cylinder(newInfo, cylinder_primitive_settings, initialPhysicsInfo3D, initialPhysicsToggles3D),
                    )?;
                    newId
                }
                ObjectInfo::Clock(oldInfo) => {
                    let newInfo = ClockInfo { ..oldInfo.clone() };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Clock(newInfo))?;
                    newId
                }
                ObjectInfo::LocalGroup(oldInfo) => {
                    let newInfo = LocalGroupInfo { ..oldInfo.clone() };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::LocalGroup(newInfo))?;
                    newId
                }
                ObjectInfo::Material(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Material(newInfo))?;
                    newId
                }
                ObjectInfo::RigidGroup(oldInfo) => {
                    let initialPhysicsInfo3D = initialPhysicsInfo3D.ok_or_else(|| anyhow::anyhow!("Physics info not found for object {}", oldObjectId))?;
                    let newInfo = RigidGroupInfo {
                        followSkeletonId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::RigidGroup(newInfo, initialPhysicsInfo3D))?;
                    newId
                }
                ObjectInfo::Label(oldInfo) => {
                    let newInfo = LabelInfo {
                        physicalObjectId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Label(newInfo))?;
                    newId
                }
                ObjectInfo::LabelImage(oldInfo) => {
                    let newInfo = LabelImageInfo {
                        physicalObjectId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::LabelImage(newInfo))?;
                    newId
                }
                ObjectInfo::Pivot(oldInfo) => {
                    let newInfo = PivotInfo {
                        physicalObjectId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Pivot(newInfo))?;
                    newId
                }
                ObjectInfo::Mesh(oldInfo) => {
                    let newInfo = MeshInfo {
                        followSkeletonId: None,
                        physicalObjectId: None,
                        material: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Mesh(newInfo))?;
                    newId
                }
                ObjectInfo::Player(oldInfo) => {
                    let newInfo = PlayerInfo {
                        headObjectId: None,
                        characterObjectId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Player(newInfo))?;
                    newId
                }
                ObjectInfo::Script(oldInfo) => {
                    let newInfo = ScriptInfo { ..oldInfo.clone() };
                    let rawCode = initialRawCode.ok_or_else(|| anyhow::anyhow!("Raw code not found for object {}", oldObjectId))?;
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Script(newInfo))?;
                    world.markScriptAsReadyToRun(newId, rawCode);
                    newId
                }
                ObjectInfo::SkAnimation(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SkAnimation(newInfo))?;
                    newId
                }
                ObjectInfo::SkAttachment(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SkAttachment(newInfo))?;
                    newId
                }
                ObjectInfo::SkKeyframe(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SkKeyframe(newInfo))?;
                    newId
                }
                ObjectInfo::SkKeyframePose(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SkKeyframePose(newInfo))?;
                    newId
                }
                ObjectInfo::SkAnimator(oldInfo) => {
                    let newInfo = SkAnimatorInfo {
                        skeletonId: None,
                        clockId: None,
                        skAnimationId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SkAnimator(newInfo))?;
                    newId
                }
                ObjectInfo::PropertyAnimation(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::PropertyAnimation(newInfo))?;
                    newId
                }
                ObjectInfo::PropertyKeyframe(oldInfo) => {
                    let newInfo = oldInfo.clone();
                    let newId = addObjectFn(oldName, ObjectBeingAdded::PropertyKeyframe(newInfo))?;
                    newId
                }
                ObjectInfo::PropertyAnimator(oldInfo) => {
                    let newInfo = PropertyAnimatorInfo {
                        targetObjectId: None,
                        clockId: None,
                        propertyAnimationId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::PropertyAnimator(newInfo))?;
                    newId
                }
                ObjectInfo::Tween(oldInfo) => {
                    let newInfo = TweenInfo {
                        targetObjectId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::Tween(newInfo))?;
                    newId
                }
                ObjectInfo::SoundPlayer(oldInfo) => {
                    let newInfo = SoundPlayerInfo {
                        clockId: None,
                        ..oldInfo.clone()
                    };
                    let newId = addObjectFn(oldName, ObjectBeingAdded::SoundPlayer(newInfo))?;
                    newId
                }
                ObjectInfo::WorldLighting(oldInfo) => {
                    let newInfo = *oldInfo;
                    let newId = addObjectFn(oldName, ObjectBeingAdded::WorldLighting(newInfo))?;
                    newId
                }
            };

            newOfOld.insert(oldObjectId, newObjectId);

            // now, visitAll on children
            if let Some(oldChildren) = oldWorld.childrenOfId.get(&oldObjectId) {
                // iterate thru children calling visitAll
                for oldChildId in oldChildren {
                    let result = visitAll(*oldChildId, oldWorld, newObjectId, world, newOfOld, trusts_loaded_ids);
                    // log errors and continue
                    if let Err(e) = result {
                        log::error!("visitAll: Error visiting child {}: {}", oldChildId, e);
                        continue;
                    }
                }
            }

            Ok(())
        }

        // -------------------- Now, use visitAll to add all objects to the world --------------------
        let trusts_loaded_ids = worldControlMode.trusts_loaded_ids();
        let mut world = Self {
            physics: Physics::new(worldControlMode.clone()),
            worldControlMode,
            changes_notify_arc: None,
            fallback_camera_location: storedWorld.fallback_camera_location,
            // attached after the initial objects are built, so loading a world never queues ObjectCreated events
            js_callback_inputs_tx: None,

            objectOfId: HashMap::new(),
            templateOfName: HashMap::new(),
            _objectIdsOfName: HashMapWithReverse::new(),
            playerMetadataOfPlayerObjectId: HashMap::new(),
            cameraOrientationOfPlayerObjectId: HashMap::new(),
            renderSkeletonInfoArcOfId: HashMap::new(),
            renderSkKeyframesOfSkAnimationId: HashMap::new(),
            propertyKeyframesArcOfPropertyAnimationId: HashMap::new(),
            // Loading is one change, so each animation's keyframes are built once the whole world is in.
            animationIdsWithStaleKeyframes: Some(HashSet::new()),
            objectIdCounter: 0,
            clientObjectIdCounter: TOP_CLIENT_OBJECT_ID,
            serverObjectIdsDeletedOnClient: HashSet::new(),

            parentOfId: HashMap::new(),
            _childrenOfId: HashMap::new(),
            _rootObjectId: 404, // gets set when add first item to world

            scriptRawCodeOfId: HashMap::new(),
            readyToAddScriptIds: HashSet::new(),
            readyToRemoveScriptIds: HashSet::new(),
            _objectsThatPointToId: HashMap::new(),
            _kindOfObjectId: HashMapWithReverse::new(),

            modifiedObjectFlags: HashMap::new(),
            addedObjectIds: HashSet::new(),
            removedObjectIds: HashSet::new(),
            physicalObjectIdsMovedByAnimation: HashSet::new(),
            rotationAnimationBaseDegreesOfObjectId: HashMap::new(),
            writtenRotationDegreesOfObjectId: HashMap::new(),
            teleportCountOfObjectId: HashMap::new(),
            teleportedObjectIdsSinceStep: HashSet::new(),
            objectIdsChangedForRender: None,
            characterPoseHistoryOfObjectId: HashMap::new(),
            playerMetadataHistoryOfObjectId: HashMap::new(),
            editHistory: None,
        };

        // add the root object
        let adding_root_object = ObjectBeingAdded::LocalGroup(LocalGroupInfo {});
        let root_name = "root".to_string();
        let rootId = if trusts_loaded_ids {
            world._addObjectWithId(storedWorld.rootObjectId, root_name, adding_root_object, None, None);
            storedWorld.rootObjectId
        } else {
            let root_id = world._addObjectAutoId(root_name, adding_root_object, None);
            root_id
        };
        world.addedObjectIds.remove(&rootId); // do not mark the root as dirty on the server, since it is special and recreated on the client too
        let mut newOfOld = HashMap::new();
        newOfOld.insert(storedWorld.rootObjectId, rootId);

        // add all children of the root object by calling visitAll on each child
        if let Some(root_children) = storedWorld.childrenOfId.get(&storedWorld.rootObjectId) {
            for old_child_id in root_children {
                let result = visitAll(*old_child_id, &storedWorld, rootId, &mut world, &mut newOfOld, trusts_loaded_ids);
                if let Err(e) = result {
                    log::error!("fromStoredWorldString: Error visiting root child {}: {}", old_child_id, e);
                    continue;
                }
            }
        }

        // now act like we are now setting each pointed objectid right here. This is the best way to avoid a dependency tree (which wouldnt even work with cycles)
        // always make sure this is always up to date and contains every ObjectIdPointer field!
        for (old_object_id, old_object) in storedWorld.objectOfId.iter() {
            let Some(&new_object_id) = newOfOld.get(old_object_id) else {
                continue;
            };

            let old_tracked_fields = {
                // the object here is old, "new" here just indicates the object's new id
                let new_object = world.getObject(new_object_id)?;
                ObjectInfoTrackedFields::from_object_info(&new_object.info)
            };

            let new_object = world.getObjectMut(new_object_id)?;
            Self::rewireObjectInfoPointers(&mut new_object.info, &old_object.info, &newOfOld, false)?;

            let new_tracked_fields = ObjectInfoTrackedFields::from_object_info(&new_object.info);

            world.afterObjectInfoChange(new_object_id, old_tracked_fields, new_tracked_fields);
            // this is the first time we create the world so we don't mark anything modified here
        }

        if trusts_loaded_ids {
            world.objectIdCounter = world.objectOfId.keys().copied().max().unwrap_or(0);
        }

        anyhow::ensure!(world.objectOfId.len() == reachable_ids.len(), "Some objects in this world could not be loaded.");
        world.refreshStaleAnimationKeyframes();

        // Contacts the world loads with aren't new, so physics learns them now: they never fire a collision start, only a stop when they separate.
        if js_callback_inputs_tx.is_some() {
            world.physics.update_collisions();
        }

        // done!
        world.js_callback_inputs_tx = js_callback_inputs_tx;
        Ok(world)
    }

    // --------------------------------- special ops ---------------------------------
    // This is an authoring helper that turns the current rigid group pose into explicit SkAttachment objects.
    pub fn createSkAttachmentsInRigidGroup(&mut self, skeleton_id: ObjectId, rigid_group_id: ObjectId) -> anyhow::Result<Vec<ObjectId>> {
        fn offset_from_joint_matrix(joint_matrix: Mat4, object_location: Location3D) -> Location3D {
            let object_matrix = Mat4::from_rotation_translation(object_location.quaternion, object_location.position);
            let offset_matrix = joint_matrix.inverse() * object_matrix;
            let (_scale, quaternion, position) = offset_matrix.to_scale_rotation_translation();
            Location3D { position, quaternion }
        }

        fn create_or_update_sk_attachment(
            world: &mut World,
            sk_attachment_id_of_object_id: &mut HashMap<ObjectId, ObjectId>,
            object_id: ObjectId,
            object_name: &str,
            joint_name: String,
            offset_from_joint: Location3D,
        ) -> anyhow::Result<ObjectId> {
            if let Some(sk_attachment_id) = sk_attachment_id_of_object_id.get(&object_id).copied() {
                let old_tracked_fields = {
                    let sk_attachment_object = world.getObject(sk_attachment_id)?;
                    ObjectInfoTrackedFields::from_object_info(&sk_attachment_object.info)
                };
                {
                    let sk_attachment_object = world.getObjectMut(sk_attachment_id)?;
                    let ObjectInfo::SkAttachment(sk_attachment_info) = &mut sk_attachment_object.info else {
                        return Err(anyhow::anyhow!(
                            "createSkAttachmentsInRigidGroup: object {} is not a SkAttachment",
                            sk_attachment_id
                        ));
                    };
                    sk_attachment_info.jointName = joint_name;
                    sk_attachment_info.offsetFromJoint = offset_from_joint;
                }
                let new_tracked_fields = {
                    let sk_attachment_object = world.getObject(sk_attachment_id)?;
                    ObjectInfoTrackedFields::from_object_info(&sk_attachment_object.info)
                };
                world.afterObjectInfoChange(sk_attachment_id, old_tracked_fields, new_tracked_fields);
                world.serverMarkObjectAsModified(sk_attachment_id, ObjectModifyFlags::from_only_object_info());
                if world.parentOfId.get(&sk_attachment_id).copied() != Some(object_id) {
                    world.setParent(sk_attachment_id, object_id)?;
                }
                return Ok(sk_attachment_id);
            }

            let sk_attachment_id = world.addObject(
                format!("{object_name}SkAttachment"),
                ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
                    objectId: Some(ObjectIdPointer(object_id)),
                    jointName: joint_name,
                    offsetFromJoint: offset_from_joint,
                }),
                object_id,
            );
            sk_attachment_id_of_object_id.insert(object_id, sk_attachment_id);
            Ok(sk_attachment_id)
        }

        fn create_sk_attachments_in_children(
            world: &mut World,
            parent_id: ObjectId,
            inherited_joint_name: Option<String>,
            rigid_group_id: ObjectId,
            joint_matrix_of_name: &HashMap<String, Mat4>,
            sk_attachment_id_of_object_id: &mut HashMap<ObjectId, ObjectId>,
            sk_attachment_ids: &mut Vec<ObjectId>,
        ) -> anyhow::Result<()> {
            let mut child_ids = world.getDirectChildrenIds(parent_id)?.iter().copied().collect::<Vec<_>>();
            child_ids.sort_unstable();

            for child_id in child_ids {
                let (child_name, is_primitive) = {
                    let child_object = world.getObject(child_id)?;
                    (child_object.name.clone(), World::isPrimitive(&child_object.info))
                };

                let mut child_joint_name = inherited_joint_name.clone();
                if is_primitive {
                    let joint_name = inherited_joint_name.clone().unwrap_or_else(|| child_name.clone());
                    if let Some(joint_matrix) = joint_matrix_of_name.get(&joint_name).copied() {
                        let physics_info_3d = world
                            .getObjectPhysicsInfo3D(child_id)
                            .ok_or_else(|| anyhow::anyhow!("createSkAttachmentsInRigidGroup: object {} has no PhysicsInfo3D", child_id))?;
                        let offset_from_joint = offset_from_joint_matrix(joint_matrix, physics_info_3d.location);
                        let sk_attachment_id = create_or_update_sk_attachment(
                            world,
                            sk_attachment_id_of_object_id,
                            child_id,
                            &child_name,
                            joint_name.clone(),
                            offset_from_joint,
                        )?;
                        sk_attachment_ids.push(sk_attachment_id);
                        child_joint_name = Some(joint_name);
                    }
                }

                create_sk_attachments_in_children(
                    world,
                    child_id,
                    child_joint_name,
                    rigid_group_id,
                    joint_matrix_of_name,
                    sk_attachment_id_of_object_id,
                    sk_attachment_ids,
                )?;
            }

            Ok(())
        }

        let skeleton_object = self.getObject(skeleton_id)?;
        let ObjectInfo::Skeleton(_) = &skeleton_object.info else {
            return Err(anyhow::anyhow!("createSkAttachmentsInRigidGroup: object {} is not a Skeleton", skeleton_id));
        };
        let rigid_group_object = self.getObject(rigid_group_id)?;
        let ObjectInfo::RigidGroup(_) = &rigid_group_object.info else {
            return Err(anyhow::anyhow!(
                "createSkAttachmentsInRigidGroup: object {} is not a RigidGroup",
                rigid_group_id
            ));
        };

        let joint_matrix_of_name = {
            let render_skeleton_info_arc = self
                .getRenderSkeletonInfoArc(skeleton_id)
                .ok_or_else(|| anyhow::anyhow!("createSkAttachmentsInRigidGroup: Skeleton {} has no render info", skeleton_id))?;
            let render_skeleton_info = render_skeleton_info_arc.as_ref();
            let pose = computeSkeletonPoseAtTime(&render_skeleton_info.jointOfIndex, &[], 0.0, None)?;

            // SkKeyframePoses are relative to the group (see computeFollowedSkJointMatrix), so measure
            // the offsets against the joints' world positions: group location + joint position
            // within the group. The offsets then come out right no matter where the character is
            // currently standing.
            let follower_group_location = self
                .getObjectPhysicsInfo3D(rigid_group_id)
                .ok_or_else(|| anyhow::anyhow!("createSkAttachmentsInRigidGroup: rigid group {} has no PhysicsInfo3D", rigid_group_id))?
                .location;
            let follower_group_matrix = Mat4::from_rotation_translation(follower_group_location.quaternion, follower_group_location.position);

            let mut joint_matrix_of_name = HashMap::new();
            for (joint_index, joint) in render_skeleton_info.jointOfIndex.iter().enumerate() {
                let Some(joint_matrix) = pose.jointMatrixOfJointIndex.get(joint_index).copied() else {
                    continue;
                };
                joint_matrix_of_name.insert(joint.name.clone(), follower_group_matrix * joint_matrix);
            }
            joint_matrix_of_name
        };

        let mut sk_attachment_id_of_object_id = HashMap::new();
        if let Some(sk_attachment_ids) = self._kindOfObjectId.get_keys(&ObjectKind::SkAttachment) {
            for sk_attachment_id in sk_attachment_ids.iter().copied() {
                let sk_attachment_object = self.getObject(sk_attachment_id)?;
                let ObjectInfo::SkAttachment(sk_attachment_info) = &sk_attachment_object.info else {
                    continue;
                };
                if let Some(object_id) = sk_attachment_info.objectId.map(ObjectId::from) {
                    sk_attachment_id_of_object_id.insert(object_id, sk_attachment_id);
                }
            }
        }

        let mut sk_attachment_ids = Vec::new();
        create_sk_attachments_in_children(
            self,
            rigid_group_id,
            None,
            rigid_group_id,
            &joint_matrix_of_name,
            &mut sk_attachment_id_of_object_id,
            &mut sk_attachment_ids,
        )?;
        Ok(sk_attachment_ids)
    }

    // --------------------------------- render caching stuff ---------------------------------

    // This keeps the render animation frame cache in the world, which is the layer that owns the editable animation object tree.
    fn getSkAnimationIdThatOwnsAnimationFrameObject(&self, object_id: ObjectId) -> Option<ObjectId> {
        let object = self.objectOfId.get(&object_id)?;
        match &object.info {
            ObjectInfo::SkAnimation(_) => Some(object_id),
            ObjectInfo::SkKeyframe(_) => {
                let sk_animation_id = *self.parentOfId.get(&object_id)?;
                let sk_animation_object = self.objectOfId.get(&sk_animation_id)?;
                match &sk_animation_object.info {
                    ObjectInfo::SkAnimation(_) => Some(sk_animation_id),
                    _ => None,
                }
            }
            ObjectInfo::SkKeyframePose(_) => {
                let sk_keyframe_id = *self.parentOfId.get(&object_id)?;
                let sk_keyframe_object = self.objectOfId.get(&sk_keyframe_id)?;
                match &sk_keyframe_object.info {
                    ObjectInfo::SkKeyframe(_) => self.getSkAnimationIdThatOwnsAnimationFrameObject(sk_keyframe_id),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    // This keeps the property keyframe cache in the world, which is the layer that owns the editable animation object tree.
    fn getPropertyAnimationIdThatOwnsAnimationFrameObject(&self, object_id: ObjectId) -> Option<ObjectId> {
        let object = self.objectOfId.get(&object_id)?;
        match &object.info {
            ObjectInfo::PropertyAnimation(_) => Some(object_id),
            ObjectInfo::PropertyKeyframe(_) => {
                let property_animation_id = *self.parentOfId.get(&object_id)?;
                let property_animation_object = self.objectOfId.get(&property_animation_id)?;
                match &property_animation_object.info {
                    ObjectInfo::PropertyAnimation(_) => Some(property_animation_id),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // This flattens SkAnimation -> SkKeyframe -> SkKeyframePose objects into the data structure used by the renderer and animation sampler.
    fn collectRenderSkKeyframesFromObjectTree(&self, skAnimationId: ObjectId) -> Vec<RenderSkKeyframe> {
        collect_sk_keyframes(
            &self.objectOfId,
            &|id| self._childrenOfId.get(&id).into_iter().flatten().copied().collect(),
            skAnimationId,
        )
    }

    // This flattens PropertyAnimation -> PropertyKeyframe objects into the data structure used by the animation sampler.
    // Children live in a HashSet so they arrive in arbitrary order; getCurrentPropertyValue searches by time, not by index.
    fn collectPropertyKeyframesFromObjectTree(&self, propertyAnimationId: ObjectId) -> Vec<PropertyKeyframeInfo> {
        collect_property_keyframes(
            &self.objectOfId,
            &|id| self._childrenOfId.get(&id).into_iter().flatten().copied().collect(),
            propertyAnimationId,
        )
    }

    // Refresh one animation cache entry after a relevant object or parent relationship changes.
    fn refreshRenderSkKeyframesOfSkAnimationId(&mut self, skAnimationId: ObjectId) {
        let Some(sk_animation_object) = self.objectOfId.get(&skAnimationId) else {
            self.renderSkKeyframesOfSkAnimationId.remove(&skAnimationId);
            return;
        };
        let ObjectInfo::SkAnimation(_) = &sk_animation_object.info else {
            self.renderSkKeyframesOfSkAnimationId.remove(&skAnimationId);
            return;
        };

        let sk_keyframes = self.collectRenderSkKeyframesFromObjectTree(skAnimationId);
        self.renderSkKeyframesOfSkAnimationId.insert(skAnimationId, sk_keyframes);
    }

    // Refresh one property animation cache entry after a relevant object or parent relationship changes.
    fn refreshPropertyKeyframesOfPropertyAnimationId(&mut self, propertyAnimationId: ObjectId) {
        let Some(property_animation_object) = self.objectOfId.get(&propertyAnimationId) else {
            self.propertyKeyframesArcOfPropertyAnimationId.remove(&propertyAnimationId);
            return;
        };
        let ObjectInfo::PropertyAnimation(_) = &property_animation_object.info else {
            self.propertyKeyframesArcOfPropertyAnimationId.remove(&propertyAnimationId);
            return;
        };

        let property_keyframes_arc = Arc::new(self.collectPropertyKeyframesFromObjectTree(propertyAnimationId));
        self.propertyKeyframesArcOfPropertyAnimationId
            .insert(propertyAnimationId, property_keyframes_arc);
    }

    // ------- intended for use in the World functions -------

    fn refreshRenderSkeletonInfoArcOfId(&mut self, object_id: ObjectId) {
        let render_skeleton_info_arc = {
            let Some(object) = self.objectOfId.get(&object_id) else {
                self.renderSkeletonInfoArcOfId.remove(&object_id);
                return;
            };
            match &object.info {
                ObjectInfo::Skeleton(_) => Some(Arc::new(computeRenderSkeletonInfo(
                    &self.objectOfId,
                    &|id| {
                        self._childrenOfId
                            .get(&id)
                            .map(|children| children.iter().copied().collect())
                            .unwrap_or_default()
                    },
                    object_id,
                ))),
                _ => None,
            }
        };

        let Some(render_skeleton_info_arc) = render_skeleton_info_arc else {
            self.renderSkeletonInfoArcOfId.remove(&object_id);
            return;
        };
        self.renderSkeletonInfoArcOfId.insert(object_id, render_skeleton_info_arc);
    }

    fn refreshRenderSkeletonInfoArcForObject(&mut self, object_id: ObjectId) {
        let render_skeleton_owner_id = {
            let Some(object) = self.objectOfId.get(&object_id) else {
                return;
            };
            match &object.info {
                ObjectInfo::Skeleton(_) => Some(object_id),
                ObjectInfo::SkJoint(_) => self.getSkeletonIdOfSkJoint(object_id),
                _ => None,
            }
        };

        let Some(render_skeleton_owner_id) = render_skeleton_owner_id else {
            return;
        };
        self.refreshRenderSkeletonInfoArcOfId(render_skeleton_owner_id);
    }

    // Rebuilding an animation's keyframes reads every one of its poses, and one long animation has thousands, so a change
    // that adds or removes many objects rebuilds each animation it touched once, when it ends, instead of after every object.
    // Keyframes are only read while the world steps or renders, never partway through a change, so nothing reads them stale.
    pub fn refreshAnimationKeyframesOnceAfter<T>(&mut self, change: impl FnOnce(&mut World) -> T) -> T {
        if self.animationIdsWithStaleKeyframes.is_some() {
            return change(self);
        }
        self.animationIdsWithStaleKeyframes = Some(HashSet::new());
        let result = change(self);
        self.refreshStaleAnimationKeyframes();
        result
    }

    // Each id is either kind of animation, and refreshing the other kind's cache for it removes nothing that exists.
    fn refreshStaleAnimationKeyframes(&mut self) {
        for animation_id in self.animationIdsWithStaleKeyframes.take().into_iter().flatten() {
            self.refreshRenderSkKeyframesOfSkAnimationId(animation_id);
            self.refreshPropertyKeyframesOfPropertyAnimationId(animation_id);
        }
    }

    fn refreshRenderSkKeyframes(&mut self, object_id: ObjectId) {
        let Some(sk_animation_id) = self.getSkAnimationIdThatOwnsAnimationFrameObject(object_id) else {
            return;
        };
        if let Some(animation_ids_with_stale_keyframes) = &mut self.animationIdsWithStaleKeyframes {
            animation_ids_with_stale_keyframes.insert(sk_animation_id);
            return;
        }
        self.refreshRenderSkKeyframesOfSkAnimationId(sk_animation_id);
    }

    fn refreshPropertyKeyframes(&mut self, object_id: ObjectId) {
        let Some(property_animation_id) = self.getPropertyAnimationIdThatOwnsAnimationFrameObject(object_id) else {
            return;
        };
        if let Some(animation_ids_with_stale_keyframes) = &mut self.animationIdsWithStaleKeyframes {
            animation_ids_with_stale_keyframes.insert(property_animation_id);
            return;
        }
        self.refreshPropertyKeyframesOfPropertyAnimationId(property_animation_id);
    }

    // This keeps the render Skeleton cache in the world, which is the layer that owns the object tree and knows when it changed.
    fn getSkeletonIdOfSkJoint(&self, joint_id: ObjectId) -> Option<ObjectId> {
        let mut current_object_id = joint_id;
        loop {
            let object = self.objectOfId.get(&current_object_id)?;
            match &object.info {
                ObjectInfo::Skeleton(_) => return Some(current_object_id),
                ObjectInfo::SkJoint(_) => {
                    current_object_id = *self.parentOfId.get(&current_object_id)?;
                }
                _ => return None,
            }
        }
    }
}
