//! The editor's undo history. The editor server owns the world, so it owns the history too: one
//! ordered log of steps, each belonging to the browser session that made it, plus a position per
//! session. A session's steps before its position are done and its steps after are undone. Applying
//! a step is an ordinary world change, so every browser sees it through the normal broadcast.
//!
//! A step is the values its cause changed, each paired with the value it replaced. Redo writes the
//! step's values, and undo writes back what its last application replaced.
use crate::editor_world_tracker::{EditorChange, EditorLeaf, EditorLeafValue, EditorObjectGroup, EditorWorldTracker};
use anyhow::Result;
use game_types::{
    editor_sync::{EditorHistoryAvailability, EditorSection},
    object_delta::ObjectAddDelta,
    object_types::{ObjectId, PhysicsInfo3D, PhysicsToggles3D},
    physics_primitive_types::PrimitiveSettings,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::game::{
    physics::{SetPhysicsInfo3D, SetPhysicsToggles3D},
    world::{PartPlace, World},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

// TODO: AI-PICKED-VALUE: Keep the last 200 steps across every session, far more than one editing
// session walks back through.
const EDITOR_HISTORY_LIMIT: usize = 200;
// Counting steps is not enough on its own: one property edit holds a single object, while one AI
// reply can hold much of the world. What a long-lived editor keeps is bounded by how much the steps
// hold, not by how many there are.
// TODO: AI-PICKED-VALUE: Keep about 128 MiB of history, which is thousands of ordinary edits and
// still tens of large AI replies on a big world.
const EDITOR_HISTORY_BYTE_LIMIT: usize = 128 * 1024 * 1024;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub enum EditorHistoryDirection {
    undo,
    redo,
}

// One object as data, plus the one thing the world cannot rebuild it from data alone. The value is
// what gets compared; the add delta is only ever used to put back an object that is not there.
#[derive(Clone, Debug)]
pub struct EditorObjectState {
    pub value: Value,
    pub add: Option<ObjectAddDelta>,
}

// Every per-object section of the stored world. A new section needs a case here, in `read_object_value`
// and in `write_section`.
const EDITOR_OBJECT_SECTIONS: [&str; 7] = [
    "parentId",
    "name",
    "physicsInfo3D",
    "physicsToggles3D",
    "primitiveSettings",
    "info",
    "scriptRawCode",
];

pub struct EditorHistoryStep {
    pub description: String,
    pub object_ids: Vec<ObjectId>,
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum EditorTurnChange {
    created,
    modified,
    deleted,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTurnObjectChange {
    pub object_id: ObjectId,
    pub name: String,
    pub kind: String,
    pub change: EditorTurnChange,
}

// Roughly how much memory a value takes, counting what serde_json allocates for every node and
// field. This only has to be close enough to bound the history.
fn count_value_bytes(value: &Value) -> usize {
    size_of::<Value>()
        + match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => 0,
            Value::String(text) => text.len(),
            Value::Array(items) => items.iter().map(count_value_bytes).sum::<usize>(),
            Value::Object(fields) => fields
                .iter()
                .map(|(name, value)| size_of::<String>() + name.len() + 16 + count_value_bytes(value))
                .sum::<usize>(),
        }
}

// Text fields send one edit per keystroke, so a run of typing in one field stays a single step.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTypingTarget {
    pub object_id: ObjectId,
    pub section: EditorSection,
    pub path: Vec<String>,
}

struct EditorStep {
    key: u64,
    author: String,
    description: String,
    reply_number: Option<u64>,
    leaves: Vec<EditorLeaf>,
    own_ids: HashSet<ObjectId>,
    typing: Option<EditorTypingTarget>,
    bytes: usize,
}

#[derive(Default)]
struct EditorReply {
    number: u64,
    needed_ids_of_author: HashMap<String, HashSet<ObjectId>>,
    // For the chat: each object the AI visibly touched, with its value just before it first did.
    first_values: Vec<(ObjectId, Value)>,
    touched_ids: HashSet<ObjectId>,
    removed_by_person_ids: HashSet<ObjectId>,
}

pub struct EditorHistory {
    world_tracker: EditorWorldTracker,
    steps: Vec<EditorStep>,
    first_undone_of_author: HashMap<String, Option<u64>>,
    next_key: u64,
    reply: Option<EditorReply>,
    replies_begun: u64,
    // Only tests lower it.
    pub byte_limit: usize,
}

// ---------------------------- one object, as data ----------------------------

pub fn read_object_value(world: &World, object_id: ObjectId) -> Value {
    let Ok(object) = world.getObject(object_id) else {
        return Value::Null;
    };
    json!({
        "parentId": world.parentOfId.get(&object_id),
        "name": object.name,
        "info": object.info,
        "physicsInfo3D": world.readPartPlace(object_id).map_or(Value::Null, |place| read_place_value(&place)),
        "physicsToggles3D": world.getObjectPhysicsToggles3D(object_id),
        "primitiveSettings": world.getObjectPrimitiveSettings(object_id),
        "scriptRawCode": world.scriptRawCodeOfId.get(&object_id),
    })
}

// A part's place is kept under its group, so moving to another group is one leaf.
pub fn read_place_value(place: &PartPlace) -> Value {
    match place {
        PartPlace::InGroup(group_id, relative_location) => json!({ "inGroup": { group_id.to_string(): relative_location } }),
        PartPlace::Free(physics_info_3d) => json!(physics_info_3d),
    }
}

pub fn read_parent_id(value: &Value) -> Option<ObjectId> {
    value["parentId"].as_u64().map(|parent_id| parent_id as ObjectId)
}

fn read_at<'v>(value: &'v Value, path: &[String]) -> Option<&'v Value> {
    path.iter().try_fold(value, |value, name| value.as_object()?.get(name))
}

fn write_at(value: &mut Value, path: &[String], new: Value) {
    if let Some(place) = path.iter().try_fold(value, |value, name| value.as_object_mut()?.get_mut(name)) {
        *place = new;
    }
}

fn read_group_id(physics: &Value) -> Option<&String> {
    physics.get("inGroup")?.as_object()?.keys().next()
}

fn write_section(world: &mut World, object_id: ObjectId, section: &str, value: &Value) -> Result<()> {
    match section {
        "parentId" => {
            if let Some(parent_id) = value.as_u64() {
                world.setParent(object_id, parent_id as ObjectId)?;
            }
        }
        "name" => world.setObjectName(object_id, Deserialize::deserialize(value)?)?,
        "info" => world.setObjectInfo(object_id, Deserialize::deserialize(value)?)?,
        "physicsInfo3D" => match value
            .get("inGroup")
            .and_then(Value::as_object)
            .and_then(|place_of_group| place_of_group.values().next())
        {
            Some(relative_location) => world.setSubitemRelativeLocation3D(object_id, Deserialize::deserialize(relative_location)?)?,
            None => {
                if let Some(physics_info_3d) = Option::<PhysicsInfo3D>::deserialize(value)? {
                    world.forcePhysicsInfo3D(object_id, SetPhysicsInfo3D::from(physics_info_3d), true);
                }
            }
        },
        "physicsToggles3D" => {
            if let Some(PhysicsToggles3D {
                isWalkthrough,
                isPushable,
                density,
            }) = Option::<PhysicsToggles3D>::deserialize(value)?
            {
                let toggles = SetPhysicsToggles3D {
                    isWalkthrough: Some(isWalkthrough),
                    isPushable: Some(isPushable),
                    density: Some(density),
                };
                world.setPhysicsToggles3D(object_id, toggles, true);
            }
        }
        "primitiveSettings" => {
            if let Some(primitive_settings) = Option::<PrimitiveSettings>::deserialize(value)? {
                world.setPrimitiveSettings(object_id, primitive_settings, true)?;
            }
        }
        "scriptRawCode" => {
            if let Some(script_raw_code) = Option::<String>::deserialize(value)? {
                world.setScriptRawCode(object_id, script_raw_code);
            }
        }
        _ => anyhow::bail!("The history does not know how to write {section}."),
    }
    Ok(())
}

// ---------------------------- a step's leaves ----------------------------

fn read_groups(leaves: &[EditorLeaf]) -> impl Iterator<Item = &Arc<EditorObjectGroup>> {
    leaves.iter().flat_map(|leaf| [&leaf.old, &leaf.new]).filter_map(|value| match value {
        EditorLeafValue::Group(group) => Some(group),
        _ => None,
    })
}

fn read_removed_groups(leaves: &[EditorLeaf]) -> impl Iterator<Item = &Arc<EditorObjectGroup>> {
    leaves.iter().filter_map(|leaf| match (&leaf.old, &leaf.new) {
        (EditorLeafValue::Group(group), EditorLeafValue::Absent) => Some(group),
        _ => None,
    })
}

// Groups are counted apart, once however many steps share them.
fn count_leaf_bytes(leaves: &[EditorLeaf]) -> usize {
    leaves
        .iter()
        .flat_map(|leaf| [&leaf.old, &leaf.new])
        .map(|value| match value {
            EditorLeafValue::Json(value) => count_value_bytes(value),
            _ => 0,
        })
        .sum()
}

// The add delta mirrors the data beside it, so an object counts about twice.
fn count_group_bytes(group: &EditorObjectGroup) -> usize {
    group.objects.iter().map(|(_, state)| count_value_bytes(&state.value) * 2).sum::<usize>()
        + group.links.iter().map(|(_, _, value)| count_value_bytes(value)).sum::<usize>()
}

fn read_visible_ids(leaves: &[EditorLeaf]) -> Vec<ObjectId> {
    let mut object_ids = Vec::new();
    for leaf in leaves.iter().filter(|leaf| !leaf.is_hidden) {
        match (&leaf.old, &leaf.new) {
            (EditorLeafValue::Group(group), _) | (_, EditorLeafValue::Group(group)) => object_ids.extend(
                group
                    .objects
                    .iter()
                    .map(|(object_id, _)| *object_id)
                    .chain(group.links.iter().map(|(linking_id, _, _)| *linking_id)),
            ),
            _ => object_ids.push(leaf.object_id),
        }
    }
    object_ids
}

fn read_removed_ids(changes: &[EditorChange]) -> impl Iterator<Item = ObjectId> + '_ {
    changes
        .iter()
        .flat_map(|change| read_removed_groups(&change.leaves))
        .flat_map(|group| group.objects.iter().map(|(object_id, _)| *object_id))
}

// ---------------------------- writing a step's values ----------------------------

fn read_target_depth(world: &World, parent_of_target: &HashMap<ObjectId, ObjectId>, object_id: ObjectId) -> usize {
    let mut seen_ids = HashSet::new();
    let mut current_id = object_id;
    while seen_ids.insert(current_id) {
        let Some(parent_id) = parent_of_target.get(&current_id).or_else(|| world.parentOfId.get(&current_id)) else {
            break;
        };
        current_id = *parent_id;
    }
    seen_ids.len() - 1
}

// The objects a value links to that are gone while the World still holds the object's link to them.
fn read_dangling_ids(world: &World, object_id: ObjectId, value: &Value) -> Vec<ObjectId> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(|link_id| link_id as ObjectId)
            .filter(|link_id| {
                !world.hasObject(*link_id)
                    && world
                        ._objectsThatPointToId
                        .get(link_id)
                        .is_some_and(|linking_ids| linking_ids.contains(&object_id))
            })
            .into_iter()
            .collect(),
        Value::Object(fields) => fields.values().flat_map(|value| read_dangling_ids(world, object_id, value)).collect(),
        _ => Vec::new(),
    }
}

// A value lands only where its place exists now: the object, every field on its path, the group of a part's place,
// a parent that exists without making a cycle, and every object a link names.
fn write_leaf(world: &mut World, object_id: ObjectId, path: &[String], value: &Value) -> Result<bool> {
    let current = read_object_value(world, object_id);
    let Some((section, inside)) = path.split_first() else {
        return Ok(false);
    };
    let Some(current_section) = current.get(section) else {
        return Ok(false);
    };
    let Some(current_value) = read_at(current_section, inside) else {
        return Ok(false);
    };
    if current_value == value {
        return Ok(true);
    }
    let mut section_value = current_section.clone();
    write_at(&mut section_value, inside, value.clone());
    let is_placed = match section.as_str() {
        "parentId" => value.as_u64().is_some_and(|parent_id| world.canAddParent(object_id, parent_id as ObjectId)),
        "physicsInfo3D" => read_group_id(&section_value) == read_group_id(current_section),
        _ => true,
    };
    if !is_placed {
        return Ok(false);
    }
    let was_dangling = !read_dangling_ids(world, object_id, value).is_empty();
    write_section(world, object_id, section, &section_value)?;
    if !was_dangling && !read_dangling_ids(world, object_id, value).is_empty() {
        write_section(world, object_id, section, current_section)?;
        return Ok(false);
    }
    Ok(true)
}

// Removed groups come back first, then the links into them, then parents from the outside in, then every other value,
// and removals go last.
fn write_leaves(world: &mut World, leaves: &[EditorLeaf], direction: EditorHistoryDirection) -> (Vec<bool>, Result<()>) {
    fn read_target(leaf: &EditorLeaf, direction: EditorHistoryDirection) -> &EditorLeafValue {
        match direction {
            EditorHistoryDirection::undo => &leaf.old,
            EditorHistoryDirection::redo => &leaf.new,
        }
    }
    let is_exists = |leaf: &EditorLeaf| leaf.path == ["exists"];
    let mut is_landed = vec![false; leaves.len()];
    let mut first_error = None;
    let mut added_ids = Vec::new();
    let mut links = Vec::new();
    for leaf in leaves.iter().filter(|leaf| is_exists(leaf)) {
        let EditorLeafValue::Group(group) = read_target(leaf, direction) else {
            continue;
        };
        let added_count = added_ids.len();
        for (object_id, state) in &group.objects {
            let Some(add) = state
                .add
                .as_ref()
                .filter(|_| read_parent_id(&state.value).is_some_and(|parent_id| world.hasObject(parent_id)) && !world.hasObject(*object_id))
            else {
                continue;
            };
            world.addObjectWithIdFromDelta(*object_id, add.clone());
            added_ids.push(*object_id);
            // An add delta does not hold everything, like a script's code, so each object is written whole after its add.
            for section in EDITOR_OBJECT_SECTIONS {
                if let Some(value) = state.value.get(section)
                    && let Err(error) = write_leaf(world, *object_id, &[section.to_string()], value)
                {
                    first_error.get_or_insert(error);
                }
            }
        }
        if added_ids.len() > added_count {
            links.extend(group.links.iter());
        }
    }
    // An add delta can link to an object that did not come back.
    for object_id in &added_ids {
        for link_id in read_dangling_ids(world, *object_id, &read_object_value(world, *object_id)) {
            world.clearPointerToItem(link_id, *object_id);
        }
    }
    // A link a removal cleared comes back only while its field is still empty.
    for (linking_id, path, value) in links {
        if read_at(&read_object_value(world, *linking_id), path).is_some_and(Value::is_null)
            && let Err(error) = write_leaf(world, *linking_id, path, value)
        {
            first_error.get_or_insert(error);
        }
    }
    let parent_of_target: HashMap<ObjectId, ObjectId> = leaves
        .iter()
        .filter(|leaf| leaf.path == ["parentId"])
        .filter_map(|leaf| match read_target(leaf, direction) {
            EditorLeafValue::Json(Value::Number(parent_id)) => Some((leaf.object_id, parent_id.as_u64()? as ObjectId)),
            _ => None,
        })
        .collect();
    let mut order: Vec<usize> = (0..leaves.len()).filter(|index| !is_exists(&leaves[*index])).collect();
    order.sort_by_cached_key(|index| match leaves[*index].path == ["parentId"] {
        true => (0, read_target_depth(world, &parent_of_target, leaves[*index].object_id)),
        false => (1, 0),
    });
    for index in order {
        if let EditorLeafValue::Json(value) = read_target(&leaves[index], direction) {
            match write_leaf(world, leaves[index].object_id, &leaves[index].path, value) {
                Ok(landed) => is_landed[index] = landed,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
    }
    for leaf in leaves.iter().filter(|leaf| is_exists(leaf)) {
        if let EditorLeafValue::Absent = read_target(leaf, direction)
            && let Err(error) = world.removeObject(leaf.object_id)
        {
            first_error.get_or_insert(error);
        }
    }
    for (index, leaf) in leaves.iter().enumerate().filter(|(_, leaf)| is_exists(leaf)) {
        is_landed[index] = match read_target(leaf, direction) {
            EditorLeafValue::Group(_) => world.hasObject(leaf.object_id),
            _ => !world.hasObject(leaf.object_id),
        };
    }
    (is_landed, first_error.map_or(Ok(()), Err))
}

// ---------------------------- the chat list ----------------------------

impl EditorReply {
    fn read_first_values(&self, world: &World, changes: &[EditorChange]) -> Vec<(ObjectId, Value)> {
        let mut object_ids = Vec::new();
        let mut value_of_id: HashMap<ObjectId, Value> = HashMap::new();
        for object_id in changes.iter().flat_map(|change| read_visible_ids(&change.leaves)) {
            if !self.touched_ids.contains(&object_id) && !value_of_id.contains_key(&object_id) {
                value_of_id.insert(object_id, read_object_value(world, object_id));
                object_ids.push(object_id);
            }
        }
        for leaf in changes.iter().flat_map(|change| &change.leaves) {
            match (&leaf.old, &leaf.new) {
                (EditorLeafValue::Json(old), _) => {
                    if let Some(value) = value_of_id.get_mut(&leaf.object_id) {
                        write_at(value, &leaf.path, old.clone());
                    }
                }
                (EditorLeafValue::Group(group), EditorLeafValue::Absent) => {
                    for (object_id, state) in &group.objects {
                        if let Some(value) = value_of_id.get_mut(object_id) {
                            *value = state.value.clone();
                        }
                    }
                    for (linking_id, path, old) in &group.links {
                        if let Some(value) = value_of_id.get_mut(linking_id) {
                            write_at(value, path, old.clone());
                        }
                    }
                }
                (EditorLeafValue::Absent, EditorLeafValue::Group(group)) => {
                    for (object_id, _) in &group.objects {
                        if let Some(value) = value_of_id.get_mut(object_id) {
                            *value = Value::Null;
                        }
                    }
                }
                _ => {}
            }
        }
        object_ids
            .into_iter()
            .filter_map(|object_id| Some((object_id, value_of_id.remove(&object_id)?)))
            .collect()
    }

    // Whoever took an object away last decides whether the chat lists it as deleted.
    fn note_removals(&mut self, changes: &[EditorChange], is_by_person: bool) {
        for object_id in read_removed_ids(changes) {
            match is_by_person {
                true => self.removed_by_person_ids.insert(object_id),
                false => self.removed_by_person_ids.remove(&object_id),
            };
        }
    }
}

// ---------------------------- the log and the per-session positions ----------------------------

impl EditorHistory {
    pub fn new(world: &mut World) -> Self {
        Self {
            world_tracker: EditorWorldTracker::new(world),
            steps: Vec::new(),
            first_undone_of_author: HashMap::new(),
            next_key: 0,
            reply: None,
            replies_begun: 0,
            byte_limit: EDITOR_HISTORY_BYTE_LIMIT,
        }
    }

    fn new_step(
        &mut self,
        author: String,
        description: String,
        reply_number: Option<u64>,
        leaves: Vec<EditorLeaf>,
        own_ids: HashSet<ObjectId>,
        typing: Option<EditorTypingTarget>,
    ) -> EditorStep {
        self.next_key += 1;
        EditorStep {
            key: self.next_key,
            author,
            description,
            reply_number,
            bytes: count_leaf_bytes(&leaves),
            leaves,
            own_ids,
            typing,
        }
    }

    fn read_first_undone_index(&self, author: &str) -> usize {
        match self.first_undone_of_author.get(author) {
            Some(Some(key)) => self.steps.iter().position(|step| step.key == *key).unwrap_or(self.steps.len()),
            _ => self.steps.len(),
        }
    }

    // An AI change is yours if it visibly changes your objects or removes what your AI steps from this reply need to be
    // undone. It goes just before your undone steps. Outside a reply, what changed is nobody's.
    fn cut(&mut self, world: &mut World, is_every_object: bool) {
        let changes = self.world_tracker.drain(world, is_every_object);
        let Some(mut reply) = self.reply.take_if(|_| !changes.is_empty()) else {
            return;
        };
        for (object_id, value) in reply.read_first_values(world, &changes) {
            reply.touched_ids.insert(object_id);
            reply.first_values.push((object_id, value));
        }
        reply.note_removals(&changes, false);
        let visible_ids: Vec<Vec<ObjectId>> = changes.iter().map(|change| read_visible_ids(&change.leaves)).collect();
        let mut own_ids_of_author: BTreeMap<String, HashSet<ObjectId>> = BTreeMap::new();
        for step in &self.steps {
            own_ids_of_author.entry(step.author.clone()).or_default().extend(&step.own_ids);
        }
        for (author, own_ids) in own_ids_of_author {
            let needed_ids = reply.needed_ids_of_author.entry(author.clone()).or_default();
            let mut is_taken = vec![false; changes.len()];
            let mut is_growing = true;
            while is_growing {
                is_growing = false;
                for (index, change) in changes.iter().enumerate() {
                    if is_taken[index]
                        || !(visible_ids[index].iter().any(|object_id| own_ids.contains(object_id))
                            || read_removed_groups(&change.leaves)
                                .flat_map(|group| &group.objects)
                                .any(|(object_id, _)| needed_ids.contains(object_id)))
                    {
                        continue;
                    }
                    is_taken[index] = true;
                    is_growing = true;
                    needed_ids.extend(
                        change
                            .left_ids
                            .iter()
                            .filter(|(object_id, _)| own_ids.contains(object_id))
                            .map(|(_, left_id)| *left_id),
                    );
                    needed_ids.extend(read_removed_groups(&change.leaves).filter_map(|group| read_parent_id(&group.objects.first()?.1.value)));
                }
            }
            let leaves: Vec<EditorLeaf> = changes
                .iter()
                .zip(is_taken)
                .filter(|(_, is_taken)| *is_taken)
                .flat_map(|(change, _)| change.leaves.iter().cloned())
                .collect();
            if leaves.is_empty() {
                continue;
            }
            let index = self.read_first_undone_index(&author);
            let step = self.new_step(author, "the AI's changes".into(), Some(reply.number), leaves, HashSet::new(), None);
            self.steps.insert(index, step);
        }
        self.reply = Some(reply);
    }

    pub fn start_person_action(&mut self, world: &mut World) {
        self.cut(world, false);
    }

    // A keystroke continues its field's step while the field still holds what that step wrote.
    pub fn record_person_action(&mut self, world: &mut World, author: String, description: String, typing: Option<EditorTypingTarget>) {
        let changes = self.world_tracker.drain(world, false);
        if let Some(reply) = self.reply.as_mut() {
            reply.note_removals(&changes, true);
        }
        let leaves: Vec<EditorLeaf> = changes.into_iter().flat_map(|change| change.leaves).collect();
        if leaves.is_empty() {
            return;
        }
        let first_undone_index = self.read_first_undone_index(&author);
        let mut index = 0;
        self.steps.retain(|step| {
            index += 1;
            index <= first_undone_index || step.author != author
        });
        self.first_undone_of_author.insert(author.clone(), None);
        let own_ids: HashSet<ObjectId> = read_visible_ids(&leaves).into_iter().collect();
        let previous = self.steps.iter_mut().rev().find(|step| step.author == author).filter(|previous| {
            typing.is_some()
                && previous.typing == typing
                && leaves.iter().all(|leaf| {
                    previous
                        .leaves
                        .iter()
                        .find(|existing| existing.object_id == leaf.object_id && existing.path == leaf.path)
                        .is_none_or(|existing| match (&existing.new, &leaf.old) {
                            (EditorLeafValue::Json(written), EditorLeafValue::Json(replaced)) => written == replaced,
                            _ => false,
                        })
                })
        });
        match previous {
            Some(previous) => {
                for leaf in leaves {
                    match previous
                        .leaves
                        .iter_mut()
                        .find(|existing| existing.object_id == leaf.object_id && existing.path == leaf.path)
                    {
                        Some(existing) => existing.new = leaf.new,
                        None => previous.leaves.push(leaf),
                    }
                }
                previous.description = description;
                previous.own_ids.extend(own_ids);
                previous.bytes = count_leaf_bytes(&previous.leaves);
            }
            None => {
                let step = self.new_step(author, description, None, leaves, own_ids, typing);
                self.steps.push(step);
            }
        }
        self.forget_oldest_steps();
    }

    pub fn begin_chat_turn(&mut self, world: &mut World) {
        self.end_chat_turn(world);
        self.replies_begun += 1;
        self.reply = Some(EditorReply {
            number: self.replies_begun,
            ..Default::default()
        });
    }

    pub fn is_chat_turn_open(&self) -> bool {
        self.reply.is_some()
    }

    // A tool always runs inside a reply, so what it changes is the AI's. A call that arrives with no reply open,
    // such as one over MCP, gets a reply of its own, and this answers with that reply's number.
    pub fn begin_tool_call(&mut self, world: &mut World) -> Option<u64> {
        if self.reply.is_some() {
            return None;
        }
        self.begin_chat_turn(world);
        Some(self.replies_begun)
    }

    // A call closes only the reply it began. A chat reply that began while it ran has already closed that one.
    pub fn end_tool_call(&mut self, world: &mut World, reply_number: Option<u64>) {
        if reply_number.is_some() && self.reply.as_ref().map(|reply| reply.number) == reply_number {
            self.end_chat_turn(world);
        }
    }

    // The AI's work since the last cut is read without taking it, so the live list is the one the reply ends with.
    pub fn read_chat_turn_changes(&self, world: &World) -> Vec<EditorTurnObjectChange> {
        let Some(reply) = &self.reply else {
            return Vec::new();
        };
        let pending = self.world_tracker.read_pending_changes(world);
        let pending_removed_ids: HashSet<ObjectId> = read_removed_ids(&pending).collect();
        let pending_first_values = reply.read_first_values(world, &pending);
        reply
            .first_values
            .iter()
            .chain(&pending_first_values)
            .filter_map(|(object_id, before)| {
                let now = read_object_value(world, *object_id);
                let is_removed_by_person = now.is_null() && reply.removed_by_person_ids.contains(object_id) && !pending_removed_ids.contains(object_id);
                if *before == now || is_removed_by_person {
                    return None;
                }
                let (change, value) = match (before.is_null(), now.is_null()) {
                    (true, _) => (EditorTurnChange::created, &now),
                    (_, true) => (EditorTurnChange::deleted, before),
                    _ => (EditorTurnChange::modified, &now),
                };
                Some(EditorTurnObjectChange {
                    object_id: *object_id,
                    name: value["name"].as_str().unwrap_or_default().to_string(),
                    kind: match &value["info"] {
                        Value::Object(info) => info.keys().next().cloned().unwrap_or_default(),
                        Value::String(kind) => kind.clone(),
                        _ => String::new(),
                    },
                    change,
                })
            })
            .collect()
    }

    // The reply's end reads every object, so a change the World never marked is still the AI's.
    pub fn end_chat_turn(&mut self, world: &mut World) -> Vec<EditorTurnObjectChange> {
        self.cut(world, self.reply.is_some());
        let object_changes = self.read_chat_turn_changes(world);
        if self.reply.take().is_some() {
            self.forget_oldest_steps();
        }
        object_changes
    }

    pub fn read_availability(&self) -> HashMap<String, EditorHistoryAvailability> {
        let first_undone_index_of_author: HashMap<&String, usize> = self
            .first_undone_of_author
            .keys()
            .map(|author| (author, self.read_first_undone_index(author)))
            .collect();
        let mut availability_of_author: HashMap<String, EditorHistoryAvailability> = HashMap::new();
        for (index, step) in self.steps.iter().enumerate() {
            let availability = availability_of_author.entry(step.author.clone()).or_default();
            match index < first_undone_index_of_author.get(&step.author).copied().unwrap_or(self.steps.len()) {
                true => availability.can_undo = true,
                false => availability.can_redo = true,
            }
        }
        availability_of_author
    }

    fn forget_oldest_steps(&mut self) {
        let mut holding_of_group: HashMap<*const EditorObjectGroup, (usize, usize)> = HashMap::new();
        let mut total = 0;
        for step in &self.steps {
            total += step.bytes;
            for group in read_groups(&step.leaves) {
                let (bytes, holders) = holding_of_group.entry(Arc::as_ptr(group)).or_insert_with(|| (count_group_bytes(group), 0));
                if *holders == 0 {
                    total += *bytes;
                }
                *holders += 1;
            }
        }
        let newest_key = self.steps.last().map(|step| step.key);
        let latest_reply_number = Some(self.replies_begun);
        let mut is_forgotten = false;
        while self.steps.len() > EDITOR_HISTORY_LIMIT || total > self.byte_limit {
            let Some(index) = self
                .steps
                .iter()
                .position(|step| Some(step.key) != newest_key && step.reply_number != latest_reply_number)
            else {
                break;
            };
            let step = self.steps.remove(index);
            total -= step.bytes;
            for group in read_groups(&step.leaves) {
                if let Some((bytes, holders)) = holding_of_group.get_mut(&Arc::as_ptr(group)) {
                    *holders -= 1;
                    if *holders == 0 {
                        total -= *bytes;
                    }
                }
            }
            if self.first_undone_of_author.get(&step.author) == Some(&Some(step.key)) {
                let next_key = self.steps[index..].iter().find(|other| other.author == step.author).map(|other| other.key);
                self.first_undone_of_author.insert(step.author.clone(), next_key);
            }
            is_forgotten = true;
        }
        if is_forgotten {
            let authors: HashSet<&String> = self.steps.iter().map(|step| &step.author).collect();
            self.first_undone_of_author.retain(|author, _| authors.contains(author));
        }
    }

    // A step whose press changed nothing is passed over in the same press.
    pub fn take_step(&mut self, world: &mut World, author: &str, direction: EditorHistoryDirection) -> Result<Option<EditorHistoryStep>> {
        self.cut(world, false);
        loop {
            let first_undone_index = self.read_first_undone_index(author);
            let index = match direction {
                EditorHistoryDirection::undo => self.steps[..first_undone_index].iter().rposition(|step| step.author == author),
                EditorHistoryDirection::redo => (first_undone_index < self.steps.len()).then_some(first_undone_index),
            };
            let Some(index) = index else {
                return Ok(None);
            };
            let first_undone_key = match direction {
                EditorHistoryDirection::undo => Some(self.steps[index].key),
                EditorHistoryDirection::redo => self.steps[index + 1..].iter().find(|step| step.author == author).map(|step| step.key),
            };
            self.first_undone_of_author.insert(author.to_string(), first_undone_key);
            let mut value_of_id: HashMap<ObjectId, Value> = HashMap::new();
            let starts: Vec<Option<EditorLeafValue>> = self.steps[index]
                .leaves
                .iter()
                .map(|leaf| match direction {
                    EditorHistoryDirection::undo => None,
                    EditorHistoryDirection::redo if leaf.path == ["exists"] => Some(match world.hasObject(leaf.object_id) {
                        true => leaf.new.clone(),
                        false => EditorLeafValue::Absent,
                    }),
                    EditorHistoryDirection::redo => read_at(
                        value_of_id.entry(leaf.object_id).or_insert_with(|| read_object_value(world, leaf.object_id)),
                        &leaf.path,
                    )
                    .cloned()
                    .map(EditorLeafValue::Json),
                })
                .collect();
            let (is_landed, written) = world.refreshAnimationKeyframesOnceAfter(|world| write_leaves(world, &self.steps[index].leaves, direction));
            let changes = self.world_tracker.drain(world, false);
            // What a press takes away is the person's, unless it redoes the running reply's own step.
            if let Some(reply) = self.reply.as_mut() {
                reply.note_removals(
                    &changes,
                    direction == EditorHistoryDirection::undo || self.steps[index].reply_number != Some(reply.number),
                );
            }
            let taken_of_id: HashMap<ObjectId, &Arc<EditorObjectGroup>> = changes
                .iter()
                .flat_map(|change| read_removed_groups(&change.leaves))
                .flat_map(|group| group.objects.iter().map(move |(object_id, _)| (*object_id, group)))
                .collect();
            let step = &mut self.steps[index];
            step.typing = None;
            for ((leaf, is_landed), start) in step.leaves.iter_mut().zip(is_landed).zip(starts) {
                let (target, other) = match direction {
                    EditorHistoryDirection::undo => (&leaf.old, &mut leaf.new),
                    EditorHistoryDirection::redo => (&leaf.new, &mut leaf.old),
                };
                match (target, taken_of_id.get(&leaf.object_id), start) {
                    (EditorLeafValue::Absent, Some(taken), _) => *other = EditorLeafValue::Group((*taken).clone()),
                    (_, _, Some(start)) if is_landed => *other = start,
                    _ => {}
                }
            }
            step.bytes = count_leaf_bytes(&step.leaves);
            written?;
            if changes.is_empty() {
                continue;
            }
            let mut seen_ids = HashSet::new();
            let object_ids = changes
                .iter()
                .flat_map(|change| &change.leaves)
                .filter(|leaf| !leaf.is_hidden)
                .flat_map(|leaf| match (&leaf.old, &leaf.new) {
                    (_, EditorLeafValue::Group(group)) => group.objects.iter().map(|(object_id, _)| *object_id).collect(),
                    (EditorLeafValue::Group(_), _) => Vec::new(),
                    _ => vec![leaf.object_id],
                })
                .filter(|object_id| seen_ids.insert(*object_id))
                .collect();
            return Ok(Some(EditorHistoryStep {
                description: step.description.clone(),
                object_ids,
            }));
        }
    }

    // Test-only, and O(world): right after any of the calls above, a change the World never marked.
    #[allow(dead_code)]
    pub fn read_unmarked_object_ids(&self, world: &World) -> Vec<ObjectId> {
        self.world_tracker.read_unmarked_object_ids(world)
    }
}
