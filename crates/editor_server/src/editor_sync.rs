use anyhow::{Result, ensure};
use axum::body::Bytes;
use game_types::{
    consts::EDITOR_WORLD_PROTOCOL_VERSION,
    editor_sync::{EditorClientDelta, EditorSection, EditorServerDelta, EditorState, EditorStatus},
    object_delta::ObjectDelta,
    object_types::{LiveObject, ObjectId, ObjectInfo, ObjectKind, PhysicsInfo3D, PhysicsToggles3D},
    physics_primitive_types::PrimitiveSettings,
};
use serde_json::Value;
use shared::game::{
    physics::{SetPhysicsInfo3D, SetPhysicsToggles3D},
    world::World,
};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, watch};

// andrew note:
// - EditorState is the shared state between browser and server.
// - EditorSync runs on the editor_server and syncs them as the authority.

// The world lock orders snapshot/subscription and publication. Each batch includes
// current status, so EditorSync needs no mutable bookkeeping or separate mutex.
pub struct EditorSync {
    editor_status_rx: watch::Receiver<EditorStatus>,
    changes_tx: broadcast::Sender<Bytes>,
    // Only ever moved under the world lock, which is what orders batches against edits.
    seq: AtomicU64,
}

impl EditorSync {
    pub fn new(editor_status_rx: watch::Receiver<EditorStatus>) -> Self {
        // TODO: AI-PICKED-VALUE: Retain up to 64 editor delta batches for connected browsers.
        let (changes_tx, _) = broadcast::channel(64);
        Self { editor_status_rx, changes_tx, seq: AtomicU64::new(0) }
    }

    fn read_state(&self, world: &World) -> EditorState {
        EditorState {
            protocol_version: EDITOR_WORLD_PROTOCOL_VERSION,
            seq: self.seq.load(Ordering::SeqCst),
            world: world.toStoredWorld(),
            editor_status: self.editor_status_rx.borrow().clone(),
        }
    }

    // Drain once, then broadcast the same batch to every browser. The runtime world
    // already records dirty objects; each batch carries the current running flags.
    fn take_changes(&self, world: &mut World) -> Result<EditorServerDelta> {
        let mut dirty = world.flushDirtyObjects();
        // Special case for RigidGroups - the editor moves all rigid group children, so we need to mark them as dirty so we send them too.
        let moved_ids: Vec<ObjectId> = dirty.modifiedObjectFlags.iter().filter(|(_, flags)| flags.modifiedPhysicsInfo3D).map(|(id, _)| *id).collect();
        for part_id in moved_ids.into_iter().flat_map(|id| world.physics.getRigidGroupSubitemIds(id)) {
            dirty.modifiedObjectFlags.entry(part_id).or_default().modifiedPhysicsInfo3D = true;
        }
        let mut object_deltas = Vec::new();
        // An object taken away and brought back since the last batch is removed first, so browsers rebuild
        // it instead of losing it.
        let readded_ids: Vec<_> = dirty.removedObjectIds.iter().copied().filter(|id| dirty.addedObjectIds.contains(id) && world.hasObject(*id)).collect();
        for id in &readded_ids {
            object_deltas.push((*id, ObjectDelta::Remove(())));
        }
        // Parents precede their children even when both were added during one tool call.
        let mut added_ids: Vec<_> = dirty.addedObjectIds.iter().copied().filter(|id| world.hasObject(*id)).collect();
        added_ids.sort_by_cached_key(|id| {
            let mut depth = 0;
            let mut current = *id;
            while let Some(parent) = world.parentOfId.get(&current) {
                depth += 1;
                current = *parent;
            }
            depth
        });
        for id in added_ids {
            object_deltas.push((id, ObjectDelta::Add(world.buildAddDelta(id)?)));
        }
        for (id, flags) in dirty.modifiedObjectFlags {
            if world.hasObject(id) && !dirty.addedObjectIds.contains(&id) {
                object_deltas.push((id, ObjectDelta::Modify(world.buildModifyDelta(id, &flags)?)));
            }
        }
        for id in dirty.removedObjectIds {
            if !readded_ids.contains(&id) {
                object_deltas.push((id, ObjectDelta::Remove(())));
            }
        }
        let script_raw_code_of_id = object_deltas
            .iter()
            .filter_map(|(id, _)| world.scriptRawCodeOfId.get(id).map(|source| (*id, source.clone())))
            .collect();
        Ok(EditorServerDelta {
            seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            object_deltas,
            script_raw_code_of_id,
            editor_status: self.editor_status_rx.borrow().clone(),
        })
    }

    pub fn broadcast_changes(&self, world: &mut World) -> Result<()> {
        let delta = self.take_changes(world)?;
        // No receivers is normal when no browser is connected; a new connection starts with the full world.
        let _ = self.changes_tx.send(bincode::serialize(&delta)?.into());
        Ok(())
    }

    // Called under the world lock right after an edit: publication needs that lock too, so the next
    // batch is the first one that can carry the edit.
    pub fn next_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst) + 1
    }

    pub fn connect_client(&self, world: &mut World) -> Result<(EditorState, broadcast::Receiver<Bytes>)> {
        // Flush to existing browsers, subscribe, and capture the world under one lock so
        // the new browser receives exactly the changes made after its initial world.
        self.broadcast_changes(world)?;
        let changes_rx = self.changes_tx.subscribe();
        Ok((self.read_state(world), changes_rx))
    }

    // The browser uses StoredWorld paths. Deserialize the edited section before touching
    // World, then use its setters and reference bookkeeping. The last applied edit wins.
    pub fn apply_client_delta(world: &mut World, delta: EditorClientDelta) -> Result<()> {
        fn replace_field(mut current: Value, path: &[String], value: Value) -> Result<Value> {
            let mut field = &mut current;
            for name in path {
                field = field.get_mut(name).ok_or_else(|| anyhow::anyhow!("This property no longer exists."))?;
            }
            *field = value;
            Ok(current)
        }
        let EditorClientDelta {
            objectId: id,
            section,
            path,
            value,
        } = delta;
        let object = world.getObject(id)?;
        match section {
            EditorSection::objectOfId => {
                let kind = format!("{:?}", ObjectKind::from_object_info(&object.info));
                ensure!(
                    path == ["name"] || (path.len() >= 3 && path[0] == "info" && path[1] == kind),
                    "This property cannot be edited."
                );
                let edited: LiveObject = serde_json::from_value(replace_field(serde_json::to_value(object)?, &path, value)?)?;
                if path == ["name"] {
                    world.setObjectName(id, edited.name)?;
                } else {
                    world.setObjectInfo(id, edited.info)?;
                }
            }
            EditorSection::physicsInfo3DOfId => {
                let physics = world.getObjectPhysicsInfo3D(id).ok_or_else(|| anyhow::anyhow!("This object has no physics."))?;
                let edited: PhysicsInfo3D = serde_json::from_value(replace_field(serde_json::to_value(&physics)?, &path, value)?)?;
                edited.validate()?;
                // A part of a rigid group moves with its group and has no motion of its own, so only a change
                // to where it sits is written, and an edit that changes nothing writes nothing.
                let is_part = world.physics.getSubitemRigidGroupId(id).is_some();
                if serde_json::to_value(&edited)? != serde_json::to_value(&physics)? && (!is_part || path.first().is_some_and(|name| name == "location")) {
                    world.forcePhysicsInfo3D(id, SetPhysicsInfo3D::from(edited), true);
                }
            }
            EditorSection::physicsToggles3DOfId => {
                let toggles = world
                    .getObjectPhysicsToggles3D(id)
                    .ok_or_else(|| anyhow::anyhow!("This object has no physics settings."))?;
                let edited: PhysicsToggles3D = serde_json::from_value(replace_field(serde_json::to_value(toggles)?, &path, value)?)?;
                edited.validate()?;
                world.setPhysicsToggles3D(
                    id,
                    SetPhysicsToggles3D {
                        isWalkthrough: Some(edited.isWalkthrough),
                        isPushable: Some(edited.isPushable),
                        density: Some(edited.density),
                    },
                    true,
                );
            }
            EditorSection::primitiveSettingsOfId => {
                let primitive = world
                    .getObjectPrimitiveSettings(id)
                    .ok_or_else(|| anyhow::anyhow!("This object has no shape settings."))?;
                let edited: PrimitiveSettings = serde_json::from_value(replace_field(serde_json::to_value(primitive)?, &path, value)?)?;
                ensure!(
                    std::mem::discriminant(&primitive) == std::mem::discriminant(&edited),
                    "The shape type cannot change."
                );
                edited.validate()?;
                world.setPrimitiveSettings(id, edited, true)?;
            }
            EditorSection::scriptRawCodeOfId => {
                ensure!(path.is_empty(), "Script source has no nested fields.");
                let ObjectInfo::Script(_) = &object.info else {
                    anyhow::bail!("This object is not a script.");
                };
                world.setScriptRawCode(id, serde_json::from_value(value)?);
            }
        }
        Ok(())
    }
}
