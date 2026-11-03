//! Saved objects use the live renderer's conversion and animation samplers without starting physics or scripts.
use anyhow::{Result, ensure};
use game_types::{
    consts::STORED_WORLD_CURRENT_VERSION,
    object_delta::{NetworkedCoords, ObjectDelta},
    object_types::{
        ClockInfo, ClockState, InterpolationStyle, LiveObject, Location3D, ObjectBeingAdded, ObjectId, ObjectInfo, ObjectLocation, PhysicsInfo3D,
        PropertyKeyframeInfo, WeightTransition, shared::quat_to_euler_degrees,
    },
    physics_primitive_types::PrimitiveSettings,
    stored_world_type::StoredWorld,
};
use glam::Vec3;
use render_core::{
    collect_animation_content::{collect_property_keyframes, collect_sk_keyframes},
    computePropertyValue::canonicalRotationBaseDegrees,
    computeSkeletonInfo::computeRenderSkeletonInfo,
    renderable::{RenderObjectKindLists, Renderable, preview_timeline_start_ms, scan_render_object_kind_lists},
    renderer_types::{RenderSkKeyframe, RenderSkeletonInfo},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

// The stored world and its parent lookup, plus each skeleton's and animation's drawing data, which only an object change can make stale.
// Other derived drawing data belongs to RenderSnapshot.
pub struct StoredWorldRenderable {
    stored: StoredWorld,
    parent_of_id: HashMap<ObjectId, ObjectId>,
    skeleton_info_arc_of_id: HashMap<ObjectId, Arc<RenderSkeletonInfo>>,
    sk_keyframes_of_animation_id: HashMap<ObjectId, Vec<RenderSkKeyframe>>,
    property_keyframes_arc_of_animation_id: HashMap<ObjectId, Arc<Vec<PropertyKeyframeInfo>>>,
}

impl StoredWorldRenderable {
    // React receives authored state, before preview clock normalization or local property edits.
    pub fn stored_world(&self) -> &StoredWorld {
        &self.stored
    }

    // -------------------------- Applying object deltas --------------------------
    // This applies received deltas to StoredWorld. The desktop counterparts are the update_world_using_* methods in client/play_runner/play.rs.

    // Apply the complete batch before resolving references, so they can point to objects added in the same batch.
    pub fn apply_object_deltas(&mut self, deltas: Vec<(ObjectId, ObjectDelta)>, script_raw_code_of_id: HashMap<ObjectId, String>) -> Result<()> {
        // A removed joint, keyframe or pose only knows its skeleton or animation before the batch, and an added one only after it.
        let changed_ids: Vec<ObjectId> = deltas.iter().map(|(id, _)| *id).collect();
        let mut stale_owner_ids: HashSet<ObjectId> = changed_ids.iter().filter_map(|id| self.drawing_data_owner_id(*id)).collect();
        let stored = &mut self.stored;
        let mut relative_location_of_id = HashMap::new();
        for (id, delta) in deltas {
            match delta {
                ObjectDelta::Add(delta) => {
                    let object = delta.objectBeingAdded;
                    if let Some(physics) = object.get_physics_info() {
                        stored.physicsInfo3DOfId.insert(id, physics.clone());
                    }
                    let primitive = match &object {
                        ObjectBeingAdded::Ball(_, shape, _, toggles) => Some((PrimitiveSettings::Ball(*shape), toggles)),
                        ObjectBeingAdded::Brick(_, shape, _, toggles) => Some((PrimitiveSettings::Brick(*shape), toggles)),
                        ObjectBeingAdded::Cylinder(_, shape, _, toggles) => Some((PrimitiveSettings::Cylinder(*shape), toggles)),
                        _ => None,
                    };
                    if let Some((shape, toggles)) = primitive {
                        stored.primitiveSettingsOfId.insert(id, shape);
                        stored.physicsToggles3DOfId.insert(id, toggles.clone());
                    }
                    let info = object.to_object_info();
                    if let ObjectInfo::Script(_) = &info {
                        stored.scriptRawCodeOfId.insert(id, String::new());
                    }
                    stored.objectOfId.insert(id, LiveObject { id, name: delta.name, info });
                    stored.childrenOfId.entry(id).or_default();
                    stored.childrenOfId.entry(delta.parentId).or_default().push(id);
                    self.parent_of_id.insert(id, delta.parentId);
                    if let Some(relative_location) = delta.initSubitemRelativeLocation3D {
                        relative_location_of_id.insert(id, relative_location);
                    }
                }
                ObjectDelta::Modify(delta) => {
                    let object = stored.objectOfId.get_mut(&id).ok_or_else(|| anyhow::anyhow!("Object {id} was not found"))?;
                    if let Some(info) = delta.modifiedObjectInfo {
                        object.info = info;
                    }
                    if let Some(name) = delta.modifiedName {
                        object.name = name;
                    }
                    if let Some(physics) = delta.modifiedPhysicsInfo3D {
                        match physics.coords {
                            NetworkedCoords::WorldCoords(physics) => {
                                stored.physicsInfo3DOfId.insert(id, physics);
                                relative_location_of_id.remove(&id);
                            }
                            NetworkedCoords::SubitemRelativeCoords(location) => {
                                relative_location_of_id.insert(id, location);
                            }
                        }
                    }
                    if let Some(toggles) = delta.modifiedPhysicsToggles3D {
                        stored.physicsToggles3DOfId.insert(id, toggles);
                    }
                    if let Some(shape) = delta.modifiedPrimitiveSettings {
                        stored.primitiveSettingsOfId.insert(id, shape);
                    }
                    if let Some(parent) = delta.modifiedParent {
                        if let Some(old_parent) = self.parent_of_id.insert(id, parent) {
                            if let Some(children) = stored.childrenOfId.get_mut(&old_parent) {
                                children.retain(|child| *child != id);
                            }
                        }
                        stored.childrenOfId.entry(parent).or_default().push(id);
                    }
                }
                ObjectDelta::Remove(()) => {
                    stored.objectOfId.remove(&id);
                    stored.childrenOfId.remove(&id);
                    stored.physicsInfo3DOfId.remove(&id);
                    stored.physicsToggles3DOfId.remove(&id);
                    stored.primitiveSettingsOfId.remove(&id);
                    stored.scriptRawCodeOfId.remove(&id);
                    relative_location_of_id.remove(&id);
                    if let Some(parent) = self.parent_of_id.remove(&id) {
                        if let Some(children) = stored.childrenOfId.get_mut(&parent) {
                            children.retain(|child| *child != id);
                        }
                    }
                }
                ObjectDelta::ForeignCharacter(delta) => {
                    stored.physicsInfo3DOfId.insert(id, delta.physicsInfo3D);
                    relative_location_of_id.remove(&id);
                }
                ObjectDelta::ForeignPlayer(_) => {}
            }
        }

        // Resolve streamed relative poses after the whole hierarchy is available, including newly added parents.
        for (id, relative) in relative_location_of_id {
            let group_id = self.highest_rigid_group(id).ok_or_else(|| anyhow::anyhow!("Object {id} has no rigid group"))?;
            let group = self
                .stored
                .physicsInfo3DOfId
                .get(&group_id)
                .ok_or_else(|| anyhow::anyhow!("Group {group_id} has no physics"))?;
            let location = Location3D {
                position: group.location.position + group.location.quaternion * relative.position,
                quaternion: group.location.quaternion * relative.quaternion,
            };
            self.stored
                .physicsInfo3DOfId
                .get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("Object {id} has no physics"))?
                .location = location;
        }

        // Sources arrive with object changes, so newly added scripts exist before their text is applied.
        for (id, source) in script_raw_code_of_id {
            let script = self
                .stored
                .scriptRawCodeOfId
                .get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("Script {id} was not found"))?;
            *script = source;
        }

        stale_owner_ids.extend(changed_ids.into_iter().filter_map(|id| self.drawing_data_owner_id(id)));
        for owner_id in stale_owner_ids {
            self.refresh_drawing_data(owner_id);
        }
        Ok(())
    }

    // The skeleton or animation whose drawing data an object is part of: a joint's skeleton, or a keyframe's or pose's animation.
    fn drawing_data_owner_id(&self, object_id: ObjectId) -> Option<ObjectId> {
        let mut current_id = object_id;
        loop {
            match &self.stored.objectOfId.get(&current_id)?.info {
                ObjectInfo::Skeleton(_) | ObjectInfo::SkAnimation(_) | ObjectInfo::PropertyAnimation(_) => return Some(current_id),
                ObjectInfo::SkJoint(_) | ObjectInfo::SkKeyframe(_) | ObjectInfo::SkKeyframePose(_) | ObjectInfo::PropertyKeyframe(_) => {
                    current_id = *self.parent_of_id.get(&current_id)?;
                }
                _ => return None,
            }
        }
    }

    // Rebuild one skeleton's or animation's drawing data from its objects, or forget it once that object is gone.
    fn refresh_drawing_data(&mut self, owner_id: ObjectId) {
        let stored = &self.stored;
        let children = |id| stored.childrenOfId.get(&id).cloned().unwrap_or_default();
        self.skeleton_info_arc_of_id.remove(&owner_id);
        self.sk_keyframes_of_animation_id.remove(&owner_id);
        self.property_keyframes_arc_of_animation_id.remove(&owner_id);
        match stored.objectOfId.get(&owner_id).map(|object| &object.info) {
            Some(ObjectInfo::Skeleton(_)) => {
                self.skeleton_info_arc_of_id
                    .insert(owner_id, Arc::new(computeRenderSkeletonInfo(&stored.objectOfId, &children, owner_id)));
            }
            Some(ObjectInfo::SkAnimation(_)) => {
                self.sk_keyframes_of_animation_id
                    .insert(owner_id, collect_sk_keyframes(&stored.objectOfId, &children, owner_id));
            }
            Some(ObjectInfo::PropertyAnimation(_)) => {
                self.property_keyframes_arc_of_animation_id
                    .insert(owner_id, Arc::new(collect_property_keyframes(&stored.objectOfId, &children, owner_id)));
            }
            _ => {}
        }
    }

    // -------------------------- Stored world --------------------------

    // Preview controls read the authored animator names and object references.
    pub fn objects(&self) -> &HashMap<ObjectId, LiveObject> {
        &self.stored.objectOfId
    }

    // Editor groups outline their rendered descendants too, using only the authored hierarchy.
    pub fn object_and_descendant_ids(&self, object_id: ObjectId) -> HashSet<ObjectId> {
        let mut ids = HashSet::new();
        let mut pending = vec![object_id];
        while let Some(id) = pending.pop() {
            if !self.stored.objectOfId.contains_key(&id) || !ids.insert(id) {
                continue;
            }
            if let Some(children) = self.stored.childrenOfId.get(&id) {
                pending.extend(children);
            }
        }
        ids
    }

    // An asset has no running source world or saved publish-time clock sample. Start each timeline
    // at its earliest playing clock/transition, preserving offsets within that domain. Paused clocks
    // keep their saved phase. The renderer then receives elapsed preview time for both domains.
    pub fn restart_preview_timeline(stored: &mut StoredWorld) -> Result<()> {
        let (server_start_ms, client_start_ms) = preview_timeline_start_ms(stored.objectOfId.values())?;
        for object in stored.objectOfId.values_mut() {
            let start = match &mut object.info {
                ObjectInfo::Clock(clock) => match &mut clock.clockState {
                    ClockState::Playing { localStartMs, timeLocation } => Some((*timeLocation, localStartMs)),
                    ClockState::Paused { .. } => None,
                },
                ObjectInfo::SkAnimator(animator) => animator
                    .weightTransition
                    .as_mut()
                    .map(|transition| (transition.timeLocation, &mut transition.startedAtMs)),
                ObjectInfo::PropertyAnimator(animator) => animator
                    .weightTransition
                    .as_mut()
                    .map(|transition| (transition.timeLocation, &mut transition.startedAtMs)),
                _ => None,
            };
            if let Some((location, start)) = start {
                *start -= match location {
                    ObjectLocation::Server => server_start_ms,
                    ObjectLocation::Client => client_start_ms,
                };
            }
        }
        Ok(())
    }

    // Keep the stored world untouched by default, so callers can sample the original server/client timelines.
    pub fn new(stored: StoredWorld) -> Result<Self> {
        ensure!(
            stored.worldSchemaVersion == STORED_WORLD_CURRENT_VERSION,
            "Unsupported asset object world schema version"
        );
        let mut parent_of_id = HashMap::new();
        let mut visited = HashSet::new();
        let mut pending = vec![stored.rootObjectId];
        while let Some(id) = pending.pop() {
            ensure!(visited.insert(id), "Asset object contains a cycle or repeated child {id}");
            let object = stored.objectOfId.get(&id).ok_or_else(|| anyhow::anyhow!("Asset child {id} is missing"))?;
            ensure!(object.id == id, "Asset object id does not match its map key");
            for child_id in stored.childrenOfId.get(&id).into_iter().flatten() {
                parent_of_id.insert(*child_id, id);
                pending.push(*child_id);
            }
        }
        ensure!(visited.len() == stored.objectOfId.len(), "Asset object contains objects outside its root");
        let mut stored_world_renderable = Self {
            stored,
            parent_of_id,
            skeleton_info_arc_of_id: HashMap::new(),
            sk_keyframes_of_animation_id: HashMap::new(),
            property_keyframes_arc_of_animation_id: HashMap::new(),
        };
        let object_ids: Vec<ObjectId> = stored_world_renderable.stored.objectOfId.keys().copied().collect();
        for object_id in object_ids {
            stored_world_renderable.refresh_drawing_data(object_id);
        }
        Ok(stored_world_renderable)
    }

    fn highest_rigid_group(&self, id: ObjectId) -> Option<ObjectId> {
        let mut parent = self.parent_of_id.get(&id);
        let mut highest_group = None;
        while let Some(id) = parent {
            if let Some(ObjectInfo::RigidGroup(_)) = self.stored.objectOfId.get(id).map(|object| &object.info) {
                highest_group = Some(*id);
            }
            parent = self.parent_of_id.get(id);
        }
        highest_group
    }

    // -------------------------- Preview playback controls --------------------------

    fn update_clock(&mut self, id: ObjectId, update: impl FnOnce(&ClockInfo) -> Result<ClockInfo>) -> Result<()> {
        let object = self.stored.objectOfId.get_mut(&id).ok_or_else(|| anyhow::anyhow!("Clock {id} was not found"))?;
        let ObjectInfo::Clock(clock) = &mut object.info else {
            anyhow::bail!("Object {id} is not a Clock");
        };
        *clock = update(clock)?;
        Ok(())
    }

    // Controls change the source objects too, so camera changes cannot restore an older clock or weight.
    pub fn pause_clock(&mut self, id: ObjectId, live_server_wall_ms: f64, live_client_wall_ms: Option<f64>) -> Result<()> {
        self.update_clock(id, |clock| clock.withPausedClock(live_server_wall_ms, live_client_wall_ms))
    }

    pub fn play_clock(
        &mut self,
        id: ObjectId,
        animation_time_secs: f32,
        time_location: ObjectLocation,
        live_server_wall_ms: f64,
        live_client_wall_ms: Option<f64>,
    ) -> Result<()> {
        self.update_clock(id, |clock| {
            clock.withPlayingClock(animation_time_secs, time_location, live_server_wall_ms, live_client_wall_ms)
        })
    }

    pub fn set_clock_speed(&mut self, id: ObjectId, speed: f32, live_server_wall_ms: f64, live_client_wall_ms: Option<f64>) -> Result<()> {
        self.update_clock(id, |clock| clock.withNewSpeed(speed, live_server_wall_ms, live_client_wall_ms))
    }

    pub fn smoothly_change_weights(
        &mut self,
        weights: impl IntoIterator<Item = (ObjectId, f32)>,
        duration_ms: f32,
        easing: InterpolationStyle,
        time_location: ObjectLocation,
        live_server_wall_ms: f64,
        live_client_wall_ms: Option<f64>,
    ) -> Result<()> {
        // Calculate the whole change before applying it, so an invalid animator leaves the batch unchanged.
        let transitions = weights
            .into_iter()
            .map(|(id, target_weight)| {
                let object = self.stored.objectOfId.get(&id).ok_or_else(|| anyhow::anyhow!("Animator {id} was not found"))?;
                let (weight, transition) = match &object.info {
                    ObjectInfo::SkAnimator(animator) => (animator.weight, animator.weightTransition),
                    ObjectInfo::PropertyAnimator(animator) => (animator.weight, animator.weightTransition),
                    _ => anyhow::bail!("Object {id} is not an animator"),
                };
                Ok((
                    id,
                    WeightTransition::fromCurrentWeight(
                        time_location,
                        live_server_wall_ms,
                        live_client_wall_ms,
                        weight,
                        transition,
                        target_weight,
                        duration_ms,
                        easing,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (id, transition) in transitions {
            match &mut self.stored.objectOfId.get_mut(&id).unwrap().info {
                ObjectInfo::SkAnimator(animator) => animator.weightTransition = Some(transition),
                ObjectInfo::PropertyAnimator(animator) => animator.weightTransition = Some(transition),
                _ => unreachable!(),
            }
        }
        Ok(())
    }
}

// -------------------------- Creating the render snapshot --------------------------

impl Renderable for StoredWorldRenderable {
    fn object_of_id(&self) -> &HashMap<ObjectId, LiveObject> {
        &self.stored.objectOfId
    }

    fn parent_of_id(&self) -> &HashMap<ObjectId, ObjectId> {
        &self.parent_of_id
    }

    fn primitive_settings_of_id(&self) -> &HashMap<ObjectId, PrimitiveSettings> {
        &self.stored.primitiveSettingsOfId
    }

    fn skeleton_info_arc_of_id(&self) -> &HashMap<ObjectId, Arc<RenderSkeletonInfo>> {
        &self.skeleton_info_arc_of_id
    }

    fn sk_keyframes_of_animation_id(&self) -> &HashMap<ObjectId, Vec<RenderSkKeyframe>> {
        &self.sk_keyframes_of_animation_id
    }

    fn property_keyframes_arc_of_animation_id(&self) -> &HashMap<ObjectId, Arc<Vec<PropertyKeyframeInfo>>> {
        &self.property_keyframes_arc_of_animation_id
    }

    // Derive the other drawing data from the complete stored world whenever the snapshot is rebuilt.
    fn rotation_base_degrees_of_id(&self) -> HashMap<ObjectId, Vec3> {
        let mut rotation_base_degrees_of_id = HashMap::new();
        for (id, physics) in &self.stored.physicsInfo3DOfId {
            rotation_base_degrees_of_id.insert(*id, canonicalRotationBaseDegrees(quat_to_euler_degrees(physics.location.quaternion)));
        }
        rotation_base_degrees_of_id
    }

    fn render_object_kind_lists(&self) -> RenderObjectKindLists {
        scan_render_object_kind_lists(&self.stored.objectOfId)
    }

    fn physics_info(&self, object_id: ObjectId) -> Option<PhysicsInfo3D> {
        self.stored.physicsInfo3DOfId.get(&object_id).cloned()
    }

    fn rigid_group_to_follow(&self, object_id: ObjectId) -> Option<(ObjectId, Location3D)> {
        let stored = &self.stored;
        // Saved physics locations are world-space, just like the live snapshots. Welded primitives
        // need their local pose too, so animating a group carries its children with it.
        stored.primitiveSettingsOfId.get(&object_id)?;
        let group_id = self.highest_rigid_group(object_id)?;
        let physics = stored.physicsInfo3DOfId.get(&object_id)?;
        let group = stored.physicsInfo3DOfId.get(&group_id)?;
        let inverse = group.location.quaternion.inverse();
        Some((
            group_id,
            Location3D {
                position: inverse * (physics.location.position - group.location.position),
                quaternion: inverse * physics.location.quaternion,
            },
        ))
    }

    // Nothing in a stored world has jumped.
    fn teleport_count(&self, _object_id: ObjectId) -> u64 {
        0
    }
}

#[cfg(test)]
#[path = "../tests/unit/preview_timeline_tests.rs"]
mod tests;
