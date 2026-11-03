//! Every object as the undo history last saw it, and the drain that turns what the World recorded in its edit history
//! since then into changes.
use crate::editor_history::{EditorObjectState, read_object_value, read_parent_id, read_place_value};
use game_types::object_types::ObjectId;
use serde_json::Value;
use shared::game::world::{RetrackedPart, World, WorldEditHistory};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub enum EditorLeafValue {
    Json(Value),
    Absent,
    Group(Arc<EditorObjectGroup>),
}

// Objects added or removed as one, parent first, and the links into them the removal cleared: the linking
// object, the path of its link and the value that link held.
#[derive(Debug)]
pub struct EditorObjectGroup {
    pub objects: Vec<(ObjectId, EditorObjectState)>,
    pub links: Vec<(ObjectId, Vec<String>, Value)>,
}

#[derive(Clone, Debug)]
pub struct EditorLeaf {
    pub object_id: ObjectId,
    pub path: Vec<String>,
    pub old: EditorLeafValue,
    pub new: EditorLeafValue,
    // A part's place the World rewrote only because an object above it changed parent.
    pub is_hidden: bool,
}

// What one object's change did. A removal or an add is one leaf on its top object, at the path "exists", and a
// hidden leaf sits on the change of each object whose parent change caused it.
#[derive(Debug)]
pub struct EditorChange {
    pub leaves: Vec<EditorLeaf>,
    // Each object of the change with a parent or link target it no longer has.
    pub left_ids: Vec<(ObjectId, ObjectId)>,
}

struct EditorTrackedObject {
    state: EditorObjectState,
    link_ids: HashSet<ObjectId>,
}

pub struct EditorWorldTracker {
    object_of_id: HashMap<ObjectId, EditorTrackedObject>,
}

fn read_tracked_objects(world: &World, object_ids: &HashSet<ObjectId>) -> HashMap<ObjectId, EditorTrackedObject> {
    let mut link_ids_of_id: HashMap<ObjectId, HashSet<ObjectId>> = HashMap::new();
    for (link_id, linking_ids) in &world._objectsThatPointToId {
        for linking_id in linking_ids.iter().filter(|linking_id| object_ids.contains(linking_id)) {
            link_ids_of_id.entry(*linking_id).or_default().insert(*link_id);
        }
    }
    object_ids
        .iter()
        .map(|object_id| {
            let state = EditorObjectState {
                value: read_object_value(world, *object_id),
                add: world.buildAddDelta(*object_id).ok(),
            };
            let link_ids = link_ids_of_id.remove(object_id).unwrap_or_default();
            (*object_id, EditorTrackedObject { state, link_ids })
        })
        .collect()
}

fn holds_link(value: &Value, link_id: ObjectId) -> bool {
    match value {
        Value::Number(number) => number.as_u64().is_some_and(|object_id| object_id as ObjectId == link_id),
        Value::Object(fields) => fields.values().any(|value| holds_link(value, link_id)),
        _ => false,
    }
}

// Objects with the same fields are compared inside. Anything else that differs, like a scalar, an array, an
// enum variant or a part's group, is one leaf.
fn read_leaves(old: &Value, new: &Value, path: &mut Vec<String>, leaves: &mut Vec<(Vec<String>, Value, Value)>) {
    match (old, new) {
        _ if old == new => {}
        (Value::Object(old_fields), Value::Object(new_fields))
            if old_fields.len() == new_fields.len() && old_fields.keys().all(|name| new_fields.contains_key(name)) =>
        {
            for (name, new_value) in new_fields {
                path.push(name.clone());
                read_leaves(&old_fields[name], new_value, path, leaves);
                path.pop();
            }
        }
        _ => leaves.push((path.clone(), old.clone(), new.clone())),
    }
}

// Each object's top, the highest of these objects above it, and how far below the top it sits.
fn read_top_of_id(object_ids: &HashSet<ObjectId>, read_parent_id: impl Fn(ObjectId) -> Option<ObjectId>) -> HashMap<ObjectId, (ObjectId, usize)> {
    object_ids
        .iter()
        .map(|object_id| {
            let (mut top_id, mut depth) = (*object_id, 0);
            while let Some(parent_id) = read_parent_id(top_id).filter(|parent_id| object_ids.contains(parent_id)) {
                top_id = parent_id;
                depth += 1;
            }
            (*object_id, (top_id, depth))
        })
        .collect()
}

fn read_member_ids_of_top(top_of_id: &HashMap<ObjectId, (ObjectId, usize)>) -> HashMap<ObjectId, Vec<ObjectId>> {
    let mut members: Vec<(ObjectId, ObjectId, usize)> = top_of_id.iter().map(|(object_id, (top_id, depth))| (*object_id, *top_id, *depth)).collect();
    members.sort_unstable_by_key(|(object_id, _, depth)| (*depth, *object_id));
    let mut member_ids_of_top: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (object_id, top_id, _) in members {
        member_ids_of_top.entry(top_id).or_default().push(object_id);
    }
    member_ids_of_top
}

impl EditorWorldTracker {
    pub fn new(world: &mut World) -> Self {
        world.editHistory = Some(WorldEditHistory::default());
        Self {
            object_of_id: read_tracked_objects(world, &world.objectOfId.keys().copied().collect()),
        }
    }

    // What changed since the last drain, in the order the World first touched each object. Reading every object
    // also catches a change the World never marked.
    pub fn drain(&mut self, world: &mut World, is_every_object: bool) -> Vec<EditorChange> {
        let Some(edit_history) = world.editHistory.as_mut().map(std::mem::take) else {
            return Vec::new();
        };
        let (changes, now_of_id) = self.read_changes(world, &edit_history, is_every_object);
        for (object_id, now) in now_of_id {
            match now.state.value.is_null() {
                true => self.object_of_id.remove(&object_id),
                false => self.object_of_id.insert(object_id, now),
            };
        }
        changes
    }

    // What the next drain returns, leaving it to that drain.
    pub fn read_pending_changes(&self, world: &World) -> Vec<EditorChange> {
        let Some(edit_history) = &world.editHistory else {
            return Vec::new();
        };
        let (changes, _) = self.read_changes(world, edit_history, false);
        changes
    }

    // The changes in an edit history, and every object they read as it is now.
    fn read_changes(&self, world: &World, edit_history: &WorldEditHistory, is_every_object: bool) -> (Vec<EditorChange>, HashMap<ObjectId, EditorTrackedObject>) {
        let mut read_ids = edit_history.touched_object_ids.clone();
        let mut edit_history_ids: HashSet<ObjectId> = read_ids.iter().copied().collect();
        read_ids.extend(
            edit_history
                .retracked_parts
                .iter()
                .map(|part| part.part_id)
                .filter(|part_id| edit_history_ids.insert(*part_id)),
        );
        if is_every_object {
            let mut other_ids: Vec<ObjectId> = world
                .objectOfId
                .keys()
                .chain(self.object_of_id.keys())
                .copied()
                .filter(|object_id| !edit_history_ids.contains(object_id))
                .collect();
            other_ids.sort_unstable();
            other_ids.dedup();
            read_ids.extend(other_ids);
        }
        let now_of_id = read_tracked_objects(world, &read_ids.iter().copied().collect());
        let removed_ids: HashSet<ObjectId> = read_ids
            .iter()
            .copied()
            .filter(|object_id| self.object_of_id.contains_key(object_id) && now_of_id[object_id].state.value.is_null())
            .collect();
        let added_ids: HashSet<ObjectId> = read_ids
            .iter()
            .copied()
            .filter(|object_id| !self.object_of_id.contains_key(object_id) && !now_of_id[object_id].state.value.is_null())
            .collect();
        let removed_top_of_id = read_top_of_id(&removed_ids, |object_id| read_parent_id(&self.object_of_id[&object_id].state.value));
        let added_top_of_id = read_top_of_id(&added_ids, |object_id| read_parent_id(&now_of_id[&object_id].state.value));
        // A removed object's change is the removal on its top.
        let read_change_id = |object_id: ObjectId| removed_top_of_id.get(&object_id).map_or(object_id, |(top_id, _)| *top_id);

        let mut events_of_part: HashMap<ObjectId, Vec<&RetrackedPart>> = HashMap::new();
        for part in &edit_history.retracked_parts {
            events_of_part.entry(part.part_id).or_default().push(part);
        }
        let mut leaves_of_id: HashMap<ObjectId, Vec<(Vec<String>, Value, Value)>> = HashMap::new();
        let mut links_of_top: HashMap<ObjectId, Vec<(ObjectId, Vec<String>, Value)>> = HashMap::new();
        let mut hidden_leaves: Vec<(ObjectId, EditorLeaf)> = Vec::new();
        for object_id in &read_ids {
            let now = &now_of_id[object_id].state.value;
            let Some(last_seen) = self.object_of_id.get(object_id).filter(|_| !now.is_null()) else {
                continue;
            };
            let mut leaves = Vec::new();
            read_leaves(&last_seen.state.value, now, &mut Vec::new(), &mut leaves);
            let cleared_ids: Vec<ObjectId> = last_seen.link_ids.iter().copied().filter(|link_id| removed_ids.contains(link_id)).collect();
            let mut change_ids: Vec<ObjectId> = Vec::new();
            if let Some(events) = events_of_part.get(object_id) {
                let places: Vec<Value> = std::iter::once(last_seen.state.value["physicsInfo3D"].clone())
                    .chain(
                        events
                            .iter()
                            .flat_map(|event| [read_place_value(&event.place_before), read_place_value(&event.place_after)]),
                    )
                    .chain([now["physicsInfo3D"].clone()])
                    .collect();
                if places.chunks(2).all(|pair| pair[0] == pair[1]) {
                    for event in events {
                        let change_id = read_change_id(event.reparented_object_id);
                        if !change_ids.contains(&change_id) {
                            change_ids.push(change_id);
                        }
                    }
                }
            }
            let mut own_leaves = Vec::new();
            for (path, old, new) in leaves {
                if let Some(link_id) = cleared_ids.iter().find(|link_id| new.is_null() && holds_link(&old, **link_id)) {
                    // A link the removal cleared goes with the removed group.
                    links_of_top.entry(removed_top_of_id[link_id].0).or_default().push((*object_id, path, old));
                } else if !change_ids.is_empty() && path.first().is_some_and(|section| section == "physicsInfo3D") {
                    // A part's place that went exactly from what the tracker last saw to now through the World's own rewrites rides with
                    // the parent changes that caused them.
                    hidden_leaves.extend(change_ids.iter().map(|change_id| {
                        let leaf = EditorLeaf {
                            object_id: *object_id,
                            path: path.clone(),
                            old: EditorLeafValue::Json(old.clone()),
                            new: EditorLeafValue::Json(new.clone()),
                            is_hidden: true,
                        };
                        (*change_id, leaf)
                    }));
                } else {
                    own_leaves.push((path, old, new));
                }
            }
            leaves_of_id.insert(*object_id, own_leaves);
        }

        let mut group_of_top: HashMap<ObjectId, Arc<EditorObjectGroup>> = HashMap::new();
        for (top_id, member_ids) in read_member_ids_of_top(&removed_top_of_id) {
            let objects = member_ids
                .iter()
                .map(|object_id| (*object_id, self.object_of_id[object_id].state.clone()))
                .collect();
            group_of_top.insert(
                top_id,
                Arc::new(EditorObjectGroup {
                    objects,
                    links: links_of_top.remove(&top_id).unwrap_or_default(),
                }),
            );
        }
        for (top_id, member_ids) in read_member_ids_of_top(&added_top_of_id) {
            let objects = member_ids.iter().map(|object_id| (*object_id, now_of_id[object_id].state.clone())).collect();
            group_of_top.insert(top_id, Arc::new(EditorObjectGroup { objects, links: Vec::new() }));
        }

        let mut left_ids_of_id: HashMap<ObjectId, Vec<(ObjectId, ObjectId)>> = HashMap::new();
        for object_id in &read_ids {
            let Some(last_seen) = self.object_of_id.get(object_id) else {
                continue;
            };
            let now = &now_of_id[object_id];
            let parent_id = read_parent_id(&now.state.value);
            let left_ids = read_parent_id(&last_seen.state.value)
                .filter(|left_id| parent_id != Some(*left_id))
                .into_iter()
                .chain(last_seen.link_ids.iter().copied().filter(|left_id| !now.link_ids.contains(left_id)));
            let change_id = read_change_id(*object_id);
            for left_id in left_ids {
                left_ids_of_id.entry(change_id).or_default().push((*object_id, left_id));
            }
        }

        let mut changes: Vec<EditorChange> = Vec::new();
        let mut index_of_id: HashMap<ObjectId, usize> = HashMap::new();
        let mut push_leaf = |subject_id: ObjectId, leaf: EditorLeaf| {
            let index = *index_of_id.entry(subject_id).or_insert_with(|| {
                changes.push(EditorChange {
                    leaves: Vec::new(),
                    left_ids: left_ids_of_id.remove(&subject_id).unwrap_or_default(),
                });
                changes.len() - 1
            });
            changes[index].leaves.push(leaf);
        };
        for object_id in &read_ids {
            let leaves = leaves_of_id.remove(object_id).unwrap_or_default();
            let group = group_of_top.remove(object_id);
            if cfg!(debug_assertions)
                && !edit_history_ids.contains(object_id)
                && (!leaves.is_empty() || removed_ids.contains(object_id) || added_ids.contains(object_id))
            {
                log::warn!("Object {object_id} changed without the World marking it.");
            }
            if let Some(group) = group {
                let (old, new) = match removed_ids.contains(object_id) {
                    true => (EditorLeafValue::Group(group), EditorLeafValue::Absent),
                    false => (EditorLeafValue::Absent, EditorLeafValue::Group(group)),
                };
                push_leaf(
                    *object_id,
                    EditorLeaf {
                        object_id: *object_id,
                        path: vec!["exists".into()],
                        old,
                        new,
                        is_hidden: false,
                    },
                );
            }
            for (path, old, new) in leaves {
                push_leaf(
                    *object_id,
                    EditorLeaf {
                        object_id: *object_id,
                        path,
                        old: EditorLeafValue::Json(old),
                        new: EditorLeafValue::Json(new),
                        is_hidden: false,
                    },
                );
            }
        }
        for (cause_id, leaf) in hidden_leaves {
            push_leaf(cause_id, leaf);
        }
        (changes, now_of_id)
    }

    // Every object the tracker last saw differently from the world. Right after a drain these are changes the World
    // never marked.
    pub fn read_unmarked_object_ids(&self, world: &World) -> Vec<ObjectId> {
        let mut object_ids: Vec<ObjectId> = world.objectOfId.keys().chain(self.object_of_id.keys()).copied().collect();
        object_ids.sort_unstable();
        object_ids.dedup();
        object_ids
            .retain(|object_id| self.object_of_id.get(object_id).map_or(&Value::Null, |object| &object.state.value) != &read_object_value(world, *object_id));
        object_ids
    }
}
