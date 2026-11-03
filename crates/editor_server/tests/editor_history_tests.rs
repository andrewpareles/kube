use editor_server::{
    editor_history::{EditorHistory, EditorHistoryDirection, EditorHistoryStep, EditorTurnChange, EditorTurnObjectChange, EditorTypingTarget},
    editor_sync::EditorSync,
};
use game_types::{
    editor_sync::{EditorClientDelta, EditorSection},
    object_types::{
        InterpolationStyle, LocalGroupInfo, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectKind, ObjectLocation, PropertyAnimationInfo,
        PropertyAnimatorInfo, PropertyKeyframeInfo, ScriptInfo,
    },
};
use render_core::collect_animation_content::{collect_property_keyframes, collect_sk_keyframes};
use serde_json::{Value, json};
use shared::game::{
    assets::{AssetData, add_asset_to_world},
    world::{World, WorldControlMode},
};
use std::ops::{Deref, DerefMut};

// Use a real authored world and the real browser mutation path, so the history walks the same
// structures and setters the editor server uses in production.
fn world() -> anyhow::Result<World> {
    let asset: Value = serde_json::from_str(include_str!("../../../tests/fixtures/asset-preview-animated-object.json"))?;
    World::fromStoredWorldString(&asset["Object"].to_string(), WorldControlMode::Editor, None)
}

fn ball_id(world: &World) -> ObjectId {
    *world._kindOfObjectId.get_keys(&ObjectKind::Ball).unwrap().iter().next().unwrap()
}

// The history drains the World's edit history before and after every action and press and at both ends of a reply. After
// each of them what the world tracker last saw must equal the world, so every test also checks that the World marked every change.
struct History {
    history: EditorHistory,
}

impl History {
    fn new(world: &mut World) -> Self {
        Self {
            history: EditorHistory::new(world),
        }
    }

    fn check_marked(&self, world: &World) {
        assert_eq!(
            self.history.read_unmarked_object_ids(world),
            Vec::<ObjectId>::new(),
            "The World marked every change"
        );
    }

    fn begin_chat_turn(&mut self, world: &mut World) {
        self.history.begin_chat_turn(world);
        self.check_marked(world);
    }

    fn end_chat_turn(&mut self, world: &mut World) -> Vec<EditorTurnObjectChange> {
        let object_changes = self.history.end_chat_turn(world);
        self.check_marked(world);
        object_changes
    }

    // A call reads the world only when it begins or ends a reply of its own; one inside another reply leaves that to the reply.
    fn begin_tool_call(&mut self, world: &mut World) -> Option<u64> {
        let reply_number = self.history.begin_tool_call(world);
        if reply_number.is_some() {
            self.check_marked(world);
        }
        reply_number
    }

    fn end_tool_call(&mut self, world: &mut World, reply_number: Option<u64>) {
        self.history.end_tool_call(world, reply_number);
        if !self.history.is_chat_turn_open() {
            self.check_marked(world);
        }
    }

    fn take_step(&mut self, world: &mut World, author: &str, direction: EditorHistoryDirection) -> anyhow::Result<Option<EditorHistoryStep>> {
        let step = self.history.take_step(world, author, direction);
        self.check_marked(world);
        step
    }
}

impl Deref for History {
    type Target = EditorHistory;
    fn deref(&self) -> &EditorHistory {
        &self.history
    }
}

impl DerefMut for History {
    fn deref_mut(&mut self) -> &mut EditorHistory {
        &mut self.history
    }
}

// Apply one browser action the way EditorServer does, and record it as one step.
fn apply_and_record(
    world: &mut World,
    history: &mut History,
    author: &str,
    description: &str,
    deltas: Vec<EditorClientDelta>,
    typing: Option<EditorTypingTarget>,
) -> anyhow::Result<()> {
    history.start_person_action(world);
    for delta in deltas {
        EditorSync::apply_client_delta(world, delta)?;
    }
    history.record_person_action(world, author.into(), description.into(), typing);
    history.check_marked(world);
    Ok(())
}

// Add objects the way EditorServer::insert_asset does.
fn add_and_record(world: &mut World, history: &mut History, author: &str, description: &str, add: impl FnOnce(&mut World)) {
    history.start_person_action(world);
    add(world);
    history.record_person_action(world, author.into(), description.into(), None);
    history.check_marked(world);
}

// What publication does between the AI's tool calls. The history reads the World's edit history, not what was published.
fn publish(world: &mut World) {
    world.flushDirtyObjects();
}

fn add_group(world: &mut World, name: &str, parent_id: ObjectId) -> ObjectId {
    world.addObject(name.into(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), parent_id)
}

fn set_name(object_id: ObjectId, name: &str) -> anyhow::Result<EditorClientDelta> {
    Ok(serde_json::from_value(
        json!({ "objectId": object_id, "section": "objectOfId", "path": ["name"], "value": name }),
    )?)
}

fn set_radius(object_id: ObjectId, radius: f64) -> anyhow::Result<EditorClientDelta> {
    Ok(serde_json::from_value(
        json!({ "objectId": object_id, "section": "primitiveSettingsOfId", "path": ["Ball", "radius"], "value": radius }),
    )?)
}

fn set_code(script: ObjectId, code: &str) -> anyhow::Result<EditorClientDelta> {
    Ok(serde_json::from_value(
        json!({ "objectId": script, "section": "scriptRawCodeOfId", "path": [], "value": code }),
    )?)
}

fn read_radius(world: &World, object_id: ObjectId) -> f64 {
    serde_json::to_value(world.getObjectPrimitiveSettings(object_id)).unwrap()["Ball"]["radius"].as_f64().unwrap()
}

fn name(world: &World, object_id: ObjectId) -> String {
    world.getObject(object_id).unwrap().name.clone()
}

fn undo(world: &mut World, history: &mut History, author: &str) -> anyhow::Result<Option<String>> {
    Ok(history.take_step(world, author, EditorHistoryDirection::undo)?.map(|step| step.description))
}

fn redo(world: &mut World, history: &mut History, author: &str) -> anyhow::Result<Option<String>> {
    Ok(history.take_step(world, author, EditorHistoryDirection::redo)?.map(|step| step.description))
}

fn animator_pointing_at(world: &mut World, target: ObjectId) -> ObjectId {
    let root = world.getRootObjectId();
    world.addObject(
        "Animator".into(),
        ObjectBeingAdded::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: Some(ObjectIdPointer(target)),
            propertyPath: vec![],
            propertyAnimationId: None,
            clockId: None,
            priority: 0,
            weight: 1.0,
            weightTransition: None,
        }),
        root,
    )
}

fn animator_target(world: &World, animator: ObjectId) -> Value {
    serde_json::to_value(&world.getObject(animator).unwrap().info).unwrap()["PropertyAnimator"]["targetObjectId"].clone()
}

#[tokio::test]
async fn undo_restores_the_previous_value_and_redo_applies_it_again() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Renamed")?], None)?;
    assert_eq!(name(&world, ball), "Renamed");

    let step = history.take_step(&mut world, "tab-a", EditorHistoryDirection::undo)?.expect("The rename can be undone");
    assert_eq!(step.description, "Name on Ball");
    assert_eq!(step.object_ids, vec![ball], "The undo names the object it changed");
    assert_eq!(name(&world, ball), original);

    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Renamed");
    assert!(redo(&mut world, &mut history, "tab-a")?.is_none());
    Ok(())
}

#[tokio::test]
async fn a_press_names_each_object_it_put_back_once() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Edit Ball", vec![set_name(ball, "Mine")?, set_radius(ball, 4.0)?], None)?;
    let step = history.take_step(&mut world, "tab-a", EditorHistoryDirection::undo)?.expect("The edit can be undone");
    assert_eq!(step.object_ids, vec![ball]);
    let step = history.take_step(&mut world, "tab-a", EditorHistoryDirection::redo)?.expect("The edit can be redone");
    assert_eq!(step.object_ids, vec![ball]);
    Ok(())
}

// Each editing session walks its own steps. Nobody's shortcut reaches anybody else's work.
#[tokio::test]
async fn one_session_never_undoes_another_session_step() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let group = add_group(&mut world, "Group", root);
    let original_ball = name(&world, ball);

    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "A-renamed")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Group", vec![set_name(group, "B-renamed")?], None)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original_ball, "Tab A takes back its own rename");
    assert_eq!(name(&world, group), "B-renamed", "Tab B's rename is untouched");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    Ok(())
}

// A step is measured on the objects the action touched, so a change nobody recorded, made to another
// object at the same moment, never ends up in it.
#[tokio::test]
async fn a_step_holds_only_what_the_action_changed() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let group = add_group(&mut world, "Group", root);
    let mut history = History::new(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(group, "Unrecorded")?)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, group), "Unrecorded", "The other change is not part of the step");
    Ok(())
}

// Undo puts the step's own field back even when somebody changed it since.
#[tokio::test]
async fn undo_puts_its_field_back_over_a_newer_change() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original_name = name(&world, ball);
    let original_radius = read_radius(&world, ball);

    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Radius on Ball", vec![set_radius(ball, 4.0)?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Radius on Ball", vec![set_radius(ball, 9.0)?], None)?;

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Radius on Ball"));
    assert_eq!(read_radius(&world, ball), original_radius, "Undo wins over tab B's radius");
    assert_eq!(name(&world, ball), "Mine");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original_name);
    Ok(())
}

// A neighbouring field belongs to whoever set it; undoing one field never drags the other back.
#[tokio::test]
async fn a_change_to_a_neighbouring_field_leaves_this_session_step_alone() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original_name = name(&world, ball);

    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 9.0)?)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original_name);
    assert_eq!(read_radius(&world, ball), 9.0, "The radius change survives the undo");
    Ok(())
}

// Text fields send one delta per keystroke, so a run of typing in one field is a single step.
#[tokio::test]
async fn a_run_of_typing_in_one_field_undoes_in_a_single_step() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let typing = Some(EditorTypingTarget { object_id: ball, section: EditorSection::objectOfId, path: vec!["name".into()] });
    let mut history = History::new(&mut world);
    for name in ["R", "Ra", "Ram"] {
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, name)?], typing.clone())?;
    }
    assert_eq!(name(&world, ball), "Ram");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original, "One undo takes back everything that was typed");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    Ok(())
}

// A run of typing counts what it grew to toward what the log holds.
#[tokio::test]
async fn a_typing_run_counts_what_it_grew_to() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let script = world.addObject("Script".into(), ObjectBeingAdded::Script(ScriptInfo { objectLocation: ObjectLocation::Server }), root);
    world.setScriptRawCode(script, "a".into());
    let typing = Some(EditorTypingTarget { object_id: script, section: EditorSection::scriptRawCodeOfId, path: vec![] });
    let mut history = History::new(&mut world);
    history.byte_limit = 1 << 15;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Theirs")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Code on Script", vec![set_code(script, "ab")?], typing.clone())?;
    apply_and_record(&mut world, &mut history, "tab-a", "Code on Script", vec![set_code(script, &format!("ab{}", "c".repeat(1 << 16)))?], typing)?;

    assert!(undo(&mut world, &mut history, "tab-b")?.is_none(), "The run grew past the budget and pushed out tab B's older step");
    Ok(())
}

// Recording a new step drops whatever that session had undone.
#[tokio::test]
async fn a_new_step_drops_what_was_undone() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "A")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "B")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-a", "Radius on Ball", vec![set_radius(ball, 5.0)?], None)?;

    assert!(redo(&mut world, &mut history, "tab-a")?.is_none(), "B can never come back");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Radius on Ball"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// Recording a new step drops only that session's undone steps, never another session's.
#[tokio::test]
async fn a_new_step_drops_only_your_own_undone_steps() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "Door", root);
    let lamp = add_group(&mut world, "Lamp", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "A1")?], None)?;
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Lamp", vec![set_name(lamp, "B1")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Lamp", vec![set_name(lamp, "B2")?], None)?;
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Name on Lamp"));
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "A2")?], None)?;

    assert_eq!(redo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Name on Lamp"));
    assert_eq!(name(&world, lamp), "B2");
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Name on Lamp"));
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Name on Lamp"));
    assert_eq!(name(&world, lamp), "Lamp");
    assert!(redo(&mut world, &mut history, "tab-a")?.is_none(), "A1 can never come back");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    assert_eq!(name(&world, door), "Door");
    Ok(())
}

#[tokio::test]
async fn undo_takes_back_an_added_object_and_redo_brings_it_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut added = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Added a group", |world| added = add_group(world, "Added", root));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(!world.hasObject(added), "Undoing an add removes the object");
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(added), "Redoing an add brings it back with the same id");
    assert_eq!(world.parentOfId.get(&added), Some(&root), "It goes back to the parent it had");
    Ok(())
}

// A press names the objects it put back, so the browser can select them, and leaves out what it removed.
#[tokio::test]
async fn a_press_leaves_out_what_it_removed() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut added = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Added a group", |world| added = add_group(world, "Added", root));
    let step = history.take_step(&mut world, "tab-a", EditorHistoryDirection::undo)?.expect("The add can be undone");
    assert_eq!(step.object_ids, Vec::<ObjectId>::new());
    let step = history.take_step(&mut world, "tab-a", EditorHistoryDirection::redo)?.expect("The add can be redone");
    assert_eq!(step.object_ids, vec![added]);
    Ok(())
}

// An asset added from the browser can be several objects, like a sound and its clock, and is
// one step that takes all of them back.
#[tokio::test]
async fn adding_a_asset_undoes_in_a_single_step() -> anyhow::Result<()> {
    let mut world = world()?;
    let object_count = world.objectOfId.len();
    let mut history = History::new(&mut world);
    let mut sound = None;
    add_and_record(&mut world, &mut history, "tab-a", "Add Beep", |world| {
        sound = add_asset_to_world(world, "Beep".into(), AssetData::Sound(7)).ok()
    });
    assert_eq!(world.objectOfId.len(), object_count + 2, "The sound comes with its own clock");

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add Beep"));
    assert_eq!(world.objectOfId.len(), object_count, "One undo takes back the sound and its clock");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(sound.unwrap()), "Redo brings the sound back under its own id");
    assert_eq!(world.objectOfId.len(), object_count + 2);
    Ok(())
}

// White door, you set it blue, the AI sets it red along with other changes. Undo gives blue and leaves
// the AI's other changes, redo gives red, and one more undo gives white.
#[tokio::test]
async fn undo_goes_back_through_the_ais_changes_to_your_objects() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let other = add_group(&mut world, "Other", root);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;

    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    EditorSync::apply_client_delta(&mut world, set_name(other, "AI-renamed")?)?;
    let added = add_group(&mut world, "AI-added", root);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Blue");
    assert_eq!(name(&world, other), "AI-renamed", "The AI's changes to other objects stay");
    assert!(world.hasObject(added));
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Red");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original);
    assert_eq!(name(&world, other), "AI-renamed");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none(), "The AI's own objects are nobody's to undo");
    Ok(())
}

// A reply that changed nothing a person edited adds nothing to their history.
#[tokio::test]
async fn a_reply_that_touched_nothing_of_yours_adds_no_step() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let other = add_group(&mut world, "Other", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(other, "AI-renamed")?)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, other), "AI-renamed");
    Ok(())
}

// A tool call over MCP arrives with no reply open, so it is a reply of its own, and undo goes back through its
// change to your object the same way it does for the built-in AI.
#[tokio::test]
async fn a_tool_call_with_no_reply_open_is_a_reply_of_its_own() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;

    let reply_number = history.begin_tool_call(&mut world);
    assert!(reply_number.is_some());
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    history.end_tool_call(&mut world, reply_number);
    assert!(!history.is_chat_turn_open(), "The call closes the reply it began");

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Blue");
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Red");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// Each call over MCP is its own reply, so undo walks its changes back one call at a time.
#[tokio::test]
async fn undo_walks_back_one_mcp_call_at_a_time() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;
    for call_name in ["Red", "Green"] {
        let reply_number = history.begin_tool_call(&mut world);
        EditorSync::apply_client_delta(&mut world, set_name(ball, call_name)?)?;
        history.end_tool_call(&mut world, reply_number);
    }

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Red");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Blue");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    Ok(())
}

// A tool call made while a chat reply is open belongs to that reply and leaves it open for the rest of the reply.
#[tokio::test]
async fn a_tool_call_inside_a_reply_leaves_the_reply_open() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;

    history.begin_chat_turn(&mut world);
    let reply_number = history.begin_tool_call(&mut world);
    assert_eq!(reply_number, None, "The call joins the open reply");
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    history.end_tool_call(&mut world, reply_number);
    assert!(history.is_chat_turn_open(), "The call leaves the chat's reply open");
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 3.0)?)?;
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|change| change.object_id).collect::<Vec<_>>(), vec![ball]);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Blue");
    Ok(())
}

// A chat reply that begins while an MCP call runs cuts that call's reply short. The rest of the call's changes go to
// the chat's reply, which the call then leaves open.
#[tokio::test]
async fn a_chat_reply_that_begins_during_a_tool_call_stays_open_after_it() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;

    let reply_number = history.begin_tool_call(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Green")?)?;
    history.end_tool_call(&mut world, reply_number);
    assert!(history.is_chat_turn_open(), "The call does not close the chat's reply");
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Red");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Blue");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// Everyone who edited an object gets their own step for the AI's change to it.
#[tokio::test]
async fn each_person_who_edited_an_object_gets_the_ais_change_to_it() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "A")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Radius on Ball", vec![set_radius(ball, 3.0)?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "AI")?)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "A");
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Radius on Ball"));
    assert_eq!(
        undo(&mut world, &mut history, "tab-a")?.as_deref(),
        Some("Name on Ball"),
        "Tab A's step for the reply already shows what it would put back, so the press goes on to their rename"
    );
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// An edit made while the AI runs counts as made before the reply, and the reply's change to the same
// object comes after it.
#[tokio::test]
async fn an_edit_made_during_a_reply_counts_before_it() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    history.begin_chat_turn(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| (row.object_id, row.change)).collect::<Vec<_>>(), vec![(ball, EditorTurnChange::modified)]);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Blue");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Red");
    Ok(())
}

// When the AI changed a field before the person overwrote it during the same reply, the person's step
// Changes during a reply are ordered as they happened: when the AI changed a field before the person
// overwrote it, the person's undo puts back the AI's value, which is what they saw when they edited.
#[tokio::test]
async fn an_edit_during_a_reply_comes_after_what_the_ai_had_done() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 7.0)?)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![ball]);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Red");
    assert_eq!(read_radius(&world, ball), 7.0, "The AI changed the ball before this session had edited it");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    Ok(())
}

// Undo works while the AI is running, and what it changes is the person's, not the reply's.
#[tokio::test]
async fn undo_works_while_a_reply_is_running() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    let added = add_group(&mut world, "AI-added", root);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original);
    assert_eq!(
        history.read_chat_turn_changes(&world).iter().map(|row| row.object_id).collect::<Vec<_>>(),
        vec![added],
        "The live list leaves the undo out"
    );
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![added]);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Mine");
    Ok(())
}

// An edit to an object the AI made during the reply is the person's, and the object stays the AI's.
#[tokio::test]
async fn an_edit_to_an_object_the_ai_made_leaves_the_object_with_the_ai() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    history.begin_chat_turn(&mut world);
    let tower = add_group(&mut world, "Tower", root);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Tower", vec![set_name(tower, "Mine")?], None)?;
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| (row.object_id, row.change)).collect::<Vec<_>>(), vec![(tower, EditorTurnChange::created)]);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Tower"));
    assert_eq!(name(&world, tower), "Tower");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    assert!(world.hasObject(tower));
    Ok(())
}

// The AI deletes the house with your door in it. Undo brings back the whole house, the door as you left
// it, and the links to the house, while something else the AI deleted stays deleted.
#[tokio::test]
async fn an_object_the_ai_deleted_comes_back_with_its_whole_group() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let window = add_group(&mut world, "Window", house);
    let tree = add_group(&mut world, "Tree", root);
    let animator = animator_pointing_at(&mut world, house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue-door")?], None)?;

    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    world.removeObject(tree)?;
    history.end_chat_turn(&mut world);
    assert_eq!(animator_target(&world, animator), Value::Null);

    let step = history.take_step(&mut world, "tab-a", EditorHistoryDirection::undo)?.expect("The AI's changes to the door");
    assert!(world.hasObject(house) && world.hasObject(door) && world.hasObject(window), "The whole house comes back");
    assert_eq!(world.parentOfId.get(&door), Some(&house));
    assert_eq!(name(&world, door), "Blue-door");
    assert_eq!(animator_target(&world, animator), json!(house), "The link to the house comes back");
    assert!(!world.hasObject(tree), "Other things the AI deleted stay deleted");
    assert!(step.object_ids.contains(&house) && step.object_ids.contains(&door));

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(name(&world, door), "Door");
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(!world.hasObject(house) && !world.hasObject(door) && !world.hasObject(window), "Redo deletes the house again");
    assert_eq!(animator_target(&world, animator), Value::Null);
    Ok(())
}

// An add delta holds no script code, so a script the AI deleted comes back with its code.
#[tokio::test]
async fn a_deleted_script_comes_back_with_its_code() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let script = world.addObject("Script".into(), ObjectBeingAdded::Script(ScriptInfo { objectLocation: ObjectLocation::Server }), house);
    world.setScriptRawCode(script, "export const a = 1;\n".into());
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(script));
    assert_eq!(world.scriptRawCodeOfId.get(&script).map(String::as_str), Some("export const a = 1;\n"));
    Ok(())
}

// Undoing an add takes away what the AI put inside it and the links to it, and redo brings them back.
#[tokio::test]
async fn undoing_an_add_keeps_what_went_with_it_for_redo() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let chimney = add_group(&mut world, "Chimney", house);
    let animator = animator_pointing_at(&mut world, house);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert!(!world.hasObject(house) && !world.hasObject(chimney));
    assert_eq!(animator_target(&world, animator), Value::Null);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(house) && world.hasObject(chimney), "The chimney comes back with the house");
    assert_eq!(world.parentOfId.get(&chimney), Some(&house));
    assert_eq!(animator_target(&world, animator), json!(house), "So does the link");
    Ok(())
}

// Someone else took away the object a step changed. There is nothing left for that step to do, so the
// same press goes on to the step before it.
#[tokio::test]
async fn a_step_with_nothing_left_to_do_is_passed_over() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    let mut box_id = 0;
    add_and_record(&mut world, &mut history, "tab-b", "Add Box", |world| box_id = add_group(world, "Box", root));
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Box", vec![set_name(box_id, "My-box")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-b")?.is_some());

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original);
    assert!(!world.hasObject(box_id));
    Ok(())
}

// A step that can still do part of its work does it, rather than being passed over.
#[tokio::test]
async fn a_step_with_one_object_gone_still_puts_back_the_rest() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    let mut box_id = 0;
    add_and_record(&mut world, &mut history, "tab-b", "Add Box", |world| box_id = add_group(world, "Box", root));
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(ball, "Mine")?, set_name(box_id, "My-box")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-b")?.is_some());

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Rename both"));
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// A typing run stops at an AI change, so one undo takes back the typing after it, the next the AI's
// change, and the next the typing before it.
#[tokio::test]
async fn a_typing_run_does_not_continue_across_an_ai_change() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original_name = name(&world, ball);
    let original_radius = read_radius(&world, ball);
    let typing = Some(EditorTypingTarget { object_id: ball, section: EditorSection::objectOfId, path: vec!["name".into()] });
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "A")?], typing.clone())?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 7.0)?)?;
    history.end_chat_turn(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "AB")?], typing.clone())?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!((name(&world, ball), read_radius(&world, ball)), ("A".to_string(), 7.0));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!((name(&world, ball), read_radius(&world, ball)), ("A".to_string(), original_radius));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original_name);
    Ok(())
}

// Moving a rigid group moves every part inside it. Those moves are the group's, so a part someone
// edited gets no step for them and the chat lists only the group.
#[tokio::test]
async fn parts_that_move_with_their_group_are_not_changed_themselves() -> anyhow::Result<()> {
    let mut world = world()?;
    let group = *world._kindOfObjectId.get_keys(&ObjectKind::RigidGroup).unwrap().iter().next().unwrap();
    let part = *world.objectOfId.keys().find(|object_id| world.physics.getSubitemRigidGroupId(**object_id) == Some(group)).expect("The group has a part");
    let original = name(&world, part);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    let position: Vec<f64> = serde_json::from_value(serde_json::to_value(world.getObjectPhysicsInfo3D(group))?["location"]["position"].clone())?;
    EditorSync::apply_client_delta(
        &mut world,
        EditorClientDelta { objectId: group, section: EditorSection::physicsInfo3DOfId, path: vec!["location".into(), "position".into()], value: json!([position[0] + 3.0, position[1], position[2]]) },
    )?;
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![group]);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Part"));
    assert_eq!(name(&world, part), original);
    Ok(())
}

// Placement warnings bring collisions up to date before every AI edit. That check must not change
// anything a reply records, or a free-moving object the AI never touched is listed as modified.
#[tokio::test]
async fn an_editor_query_during_a_turn_changes_no_object() -> anyhow::Result<()> {
    let mut world = world()?;
    let group = *world._kindOfObjectId.get_keys(&ObjectKind::RigidGroup).unwrap().iter().next().unwrap();
    let skeleton = *world._kindOfObjectId.get_keys(&ObjectKind::Skeleton).unwrap().iter().next().unwrap();
    let delta = |section: EditorSection, path: &[&str], value: Value| EditorClientDelta { objectId: group, section, path: path.iter().map(|name| name.to_string()).collect(), value };
    EditorSync::apply_client_delta(&mut world, delta(EditorSection::objectOfId, &["info", "RigidGroup", "followSkeletonId"], json!(skeleton)))?;
    EditorSync::apply_client_delta(&mut world, delta(EditorSection::physicsInfo3DOfId, &["movement", "angularVelocity"], json!([-1.2e-7, 0.0, 2.5e-6])))?;
    let mut history = History::new(&mut world);
    history.begin_chat_turn(&mut world);
    world.call_this_before_editor_query();
    assert_eq!(history.end_chat_turn(&mut world), vec![], "The check leaves the group's stored spin alone");
    Ok(())
}

// The reply's list reads in the order the AI first touched each object, whatever their ids.
#[tokio::test]
async fn a_turn_lists_objects_in_the_order_they_were_first_touched() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    let first = add_group(&mut world, "First", root);
    publish(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Second")?)?;
    publish(&mut world);
    let unpublished = add_group(&mut world, "Unpublished", root);
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![first, ball, unpublished], "Anything not yet published comes last");
    Ok(())
}

// The chat lists a reply's changes while it runs. Reading them leaves the reply open, and ending it
// reports the same objects. An object made and removed within the reply never happened.
#[tokio::test]
async fn a_running_turn_lists_its_changes_without_ending() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let doomed = add_group(&mut world, "Doomed", root);
    let mut history = History::new(&mut world);
    assert_eq!(history.read_chat_turn_changes(&world), vec![], "No reply, no list");
    history.begin_chat_turn(&mut world);
    let added = add_group(&mut world, "Added", root);
    publish(&mut world);
    let so_far = history.read_chat_turn_changes(&world);
    assert_eq!(so_far.iter().map(|row| (row.object_id, row.change)).collect::<Vec<_>>(), vec![(added, EditorTurnChange::created)]);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Renamed")?)?;
    world.removeObject(doomed)?;
    let fleeting = add_group(&mut world, "Fleeting", root);
    world.removeObject(fleeting)?;
    publish(&mut world);
    let object_changes = history.end_chat_turn(&mut world);
    let row_of = |object_id| object_changes.iter().find(|row| row.object_id == object_id).map(|row| (row.name.as_str(), row.kind.as_str(), row.change));
    assert_eq!(row_of(added), Some(("Added", "LocalGroup", EditorTurnChange::created)));
    assert_eq!(row_of(ball), Some(("Renamed", "Ball", EditorTurnChange::modified)));
    assert_eq!(row_of(doomed), Some(("Doomed", "LocalGroup", EditorTurnChange::deleted)), "A deleted object keeps the name it had");
    assert_eq!(row_of(fleeting), None);
    Ok(())
}

// A reply whose end is never reported is still closed when the next one begins, so each reply is its
// own step.
#[tokio::test]
async fn a_turn_that_never_reported_its_end_is_still_its_own_step() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let group = add_group(&mut world, "Group", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(ball, "Mine")?, set_name(group, "My-group")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "First-turn")?)?;
    // Node never reports this reply finishing; the next prompt arrives instead.
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(group, "Second-turn")?)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!((name(&world, ball), name(&world, group)), ("First-turn".to_string(), "My-group".to_string()));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Mine");
    Ok(())
}

#[tokio::test]
async fn a_turn_is_open_from_its_beginning_until_its_end_is_reported() -> anyhow::Result<()> {
    let mut world = world()?;
    let mut history = History::new(&mut world);
    assert!(!history.is_chat_turn_open());
    history.begin_chat_turn(&mut world);
    assert!(history.is_chat_turn_open());
    history.end_chat_turn(&mut world);
    assert!(!history.is_chat_turn_open());
    Ok(())
}

// The log is bounded, and a step that falls out of it is simply gone.
#[tokio::test]
async fn the_oldest_steps_are_forgotten() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut object_ids = Vec::new();
    for index in 0..260 {
        add_and_record(&mut world, &mut history, "tab-a", &format!("Added {index}"), |world| object_ids.push(add_group(world, &format!("Group-{index}"), root)));
    }
    let mut taken = 0;
    while undo(&mut world, &mut history, "tab-a")?.is_some() {
        taken += 1;
    }
    assert_eq!(taken, 200, "Undo walks back through every entry the log still holds, and no further");
    // The first sixty fell out of the log, so nothing can take them back; the last two hundred went.
    for (index, object_id) in object_ids.iter().enumerate() {
        assert_eq!(
            world.hasObject(*object_id),
            index < 260 - 200,
            "Group-{index} is on the wrong side of where the log was trimmed"
        );
    }
    Ok(())
}

fn add_material(world: &mut World, name: &str, parent_id: ObjectId) -> anyhow::Result<ObjectId> {
    let material = serde_json::from_value(json!({ "Material": {
        "albedoTextureDataId": null, "metallicTextureDataId": null, "roughnessTextureDataId": null, "normalTextureDataId": null,
        "alphaMode": "Opaque", "isDoubleSided": false,
    } }))?;
    Ok(world.addObject(name.into(), material, parent_id))
}

fn set_ball_material(ball: ObjectId, material: ObjectId, tile_size: f64) -> anyhow::Result<EditorClientDelta> {
    Ok(serde_json::from_value(json!({
        "objectId": ball, "section": "objectOfId", "path": ["info", "Ball", "material"],
        "value": { "materialInfoId": material, "tileSizeU": tile_size, "tileSizeV": 1.0, "tileOffsetU": 0.0, "tileOffsetV": 0.0 },
    }))?)
}

fn set_tile_size(ball: ObjectId, tile_size: f64) -> anyhow::Result<EditorClientDelta> {
    Ok(serde_json::from_value(json!({ "objectId": ball, "section": "objectOfId", "path": ["info", "Ball", "material", "tileSizeU"], "value": tile_size }))?)
}

fn ball_material(world: &World, ball: ObjectId) -> Value {
    serde_json::to_value(&world.getObject(ball).unwrap().info).unwrap()["Ball"]["material"].clone()
}

fn move_by(world: &World, object_id: ObjectId, x: f64) -> anyhow::Result<EditorClientDelta> {
    let position: Vec<f64> = serde_json::from_value(serde_json::to_value(world.getObjectPhysicsInfo3D(object_id))?["location"]["position"].clone())?;
    Ok(EditorClientDelta {
        objectId: object_id,
        section: EditorSection::physicsInfo3DOfId,
        path: vec!["location".into(), "position".into()],
        value: json!([position[0] + x, position[1], position[2]]),
    })
}

fn shift(world: &mut World, object_id: ObjectId, x: f64) -> anyhow::Result<()> {
    let delta = move_by(world, object_id, x)?;
    EditorSync::apply_client_delta(world, delta)?;
    Ok(())
}

fn relative_x(world: &World, part: ObjectId) -> f64 {
    serde_json::to_value(world.physics.getSubitemRelativeLocation3D(part)).unwrap()["position"][0].as_f64().unwrap()
}

fn rigid_part(world: &World) -> (ObjectId, ObjectId) {
    let group = *world._kindOfObjectId.get_keys(&ObjectKind::RigidGroup).unwrap().iter().min().unwrap();
    let part = *world.objectOfId.keys().find(|object_id| world.physics.getSubitemRigidGroupId(**object_id) == Some(group)).unwrap();
    (group, part)
}

// When the AI moves an object and then deletes what it moved it into, undo goes back through both in
// order: first the deleted group comes back with the object in it, then the object goes back where it was.
#[tokio::test]
async fn undo_goes_back_through_a_move_and_then_a_delete_in_order() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let garage = add_group(&mut world, "Garage", root);
    let car = add_group(&mut world, "Car", garage);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, garage)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Door", vec![set_name(door, "Theirs")?], None)?;
    world.removeObject(garage)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(garage) && world.hasObject(car), "The garage comes back with the door that was in it");
    assert_eq!(world.parentOfId.get(&door), Some(&garage));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(world.parentOfId.get(&door), Some(&house));
    Ok(())
}

// When the reply took away the parent or the link target of something a person edited, undoing the
// reply's changes to it brings those back too.
#[tokio::test]
async fn undo_brings_back_the_parent_and_link_target_the_reply_deleted() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let target = add_group(&mut world, "Target", root);
    let animator = animator_pointing_at(&mut world, target);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(door, "Mine")?, set_name(animator, "My-animator")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;
    world.removeObject(house)?;
    world.removeObject(target)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(world.parentOfId.get(&door), Some(&house));
    assert_eq!(animator_target(&world, animator), json!(target));
    assert!(world.hasObject(target));
    Ok(())
}

// A parent that is gone is left alone, and the rest of the step still applies.
#[tokio::test]
async fn a_step_whose_parent_is_gone_still_puts_back_the_rest() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-b", "Add House", |world| house = add_group(world, "House", root));
    let door = add_group(&mut world, "Door", house);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(door, "AI-door")?)?;
    world.setParent(door, root)?;
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-b")?.is_some());
    assert!(!world.hasObject(house));

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "Mine");
    assert_eq!(world.parentOfId.get(&door), Some(&root));
    Ok(())
}

// Undoing a reply that swapped two objects' nesting puts the outer one back first.
#[tokio::test]
async fn undo_puts_back_swapped_nesting() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(house, "My-house")?, set_name(door, "My-door")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;
    world.setParent(house, door)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(world.parentOfId.get(&house), Some(&root));
    assert_eq!(world.parentOfId.get(&door), Some(&house));
    Ok(())
}

// An edit inside something the AI added during the reply, like the tiling of a material it just put
// on, is the person's own step.
#[tokio::test]
async fn an_edit_inside_something_the_ai_just_added_is_the_persons() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let material = add_material(&mut world, "Mat", root)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_ball_material(ball, material, 1.0)?)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Tile on Ball", vec![set_tile_size(ball, 5.0)?], None)?;

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Tile on Ball"));
    assert_eq!(ball_material(&world, ball)["tileSizeU"], json!(1.0), "The AI's material stays, with its own tiling");
    history.end_chat_turn(&mut world);
    Ok(())
}

// The world clears a whole material when the material object goes, and it comes back with it.
#[tokio::test]
async fn a_material_link_comes_back_with_the_deleted_group() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let material = add_material(&mut world, "Mat", house)?;
    EditorSync::apply_client_delta(&mut world, set_ball_material(ball, material, 2.0)?)?;
    let linked = ball_material(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    assert_eq!(ball_material(&world, ball), Value::Null);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(material));
    assert_eq!(ball_material(&world, ball), linked);
    Ok(())
}

// A number that happens to equal a restored object's id is not a link unless the object linked to it.
#[tokio::test]
async fn only_real_links_are_put_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let material = add_material(&mut world, "Mat", root)?;
    let set_texture = |value: Value| -> anyhow::Result<EditorClientDelta> {
        Ok(serde_json::from_value(json!({ "objectId": material, "section": "objectOfId", "path": ["info", "Material", "albedoTextureDataId"], "value": value }))?)
    };
    EditorSync::apply_client_delta(&mut world, set_texture(json!(house))?)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_texture(Value::Null)?)?;
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(house));
    let texture = serde_json::to_value(&world.getObject(material)?.info)?["Material"]["albedoTextureDataId"].clone();
    assert_eq!(texture, Value::Null, "The AI's change to a material nobody edited stays");
    Ok(())
}

// Undoing a part's move puts it back in its place in the group, even after the group moved.
#[tokio::test]
async fn undoing_a_parts_move_puts_it_back_in_its_group() -> anyhow::Result<()> {
    let mut world = world()?;
    let (group, part) = rigid_part(&world);
    let original = relative_x(&world, part);
    let mut history = History::new(&mut world);
    let move_part = move_by(&world, part, 1.0)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Move Part", vec![move_part], None)?;
    let move_group = move_by(&world, group, 3.0)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Move Group", vec![move_group], None)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!((relative_x(&world, part) - original).abs() < 1e-4, "The part is back where it was in the group");
    Ok(())
}

// A person's edit to a part during a reply does not hide the AI's move of that part within its group.
#[tokio::test]
async fn an_edit_to_a_part_during_a_reply_keeps_the_ais_move_of_it() -> anyhow::Result<()> {
    let mut world = world()?;
    let (_, part) = rigid_part(&world);
    let original = relative_x(&world, part);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Part", vec![set_name(part, "Theirs")?], None)?;
    history.begin_chat_turn(&mut world);
    let move_part = move_by(&world, part, 1.0)?;
    EditorSync::apply_client_delta(&mut world, move_part)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![part]);

    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert!((relative_x(&world, part) - original).abs() < 1e-4);
    Ok(())
}

// What came back with an undone add is not the person's, so the AI's later changes to it are nobody's.
#[tokio::test]
async fn what_comes_back_with_an_add_stays_nobodys() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let chimney = add_group(&mut world, "Chimney", house);
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(chimney, "AI-chimney")?)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    Ok(())
}

// Parents saved at different moments of a reply can point at each other; closing the reply still ends.
#[tokio::test]
async fn a_reply_closes_when_its_start_holds_a_parent_cycle() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let a = add_group(&mut world, "A", root);
    let b = add_group(&mut world, "B", a);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on A", vec![set_name(a, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(b, root)?;
    world.setParent(a, b)?;
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    world.setParent(b, root)?;
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    world.removeObject(b)?;
    history.end_chat_turn(&mut world);
    Ok(())
}

fn clock_id(world: &World) -> ObjectId {
    *world._kindOfObjectId.get_keys(&ObjectKind::Clock).unwrap().iter().min().unwrap()
}

fn clock_state(world: &World, clock: ObjectId) -> Value {
    serde_json::to_value(&world.getObject(clock).unwrap().info).unwrap()["Clock"]["clockState"].clone()
}

fn set_clock_state(clock: ObjectId, state: Value) -> anyhow::Result<EditorClientDelta> {
    Ok(serde_json::from_value(json!({ "objectId": clock, "section": "objectOfId", "path": ["info", "Clock", "clockState"], "value": state }))?)
}

fn world_x(world: &World, object_id: ObjectId) -> f64 {
    serde_json::to_value(world.getObjectPhysicsInfo3D(object_id)).unwrap()["location"]["position"][0].as_f64().unwrap()
}

fn add_rigid_group(world: &mut World, name: &str, x: f64) -> anyhow::Result<ObjectId> {
    let root = world.getRootObjectId();
    let (existing, _) = rigid_part(world);
    let copy = world.cloneObjectAsObjectBeingAdded(existing)?;
    let group = world.addObject(name.into(), copy, root);
    let position = move_by(world, group, x - world_x(world, group))?;
    EditorSync::apply_client_delta(world, position)?;
    Ok(group)
}

fn add_ball(world: &mut World, name: &str, parent_id: ObjectId, x: f64) -> anyhow::Result<ObjectId> {
    let ball = ball_id(world);
    let copy = world.cloneObjectAsObjectBeingAdded(ball)?;
    let added = world.addObject(name.into(), copy, parent_id);
    let x = x - world_x(world, added);
    let position = move_by(world, added, x)?;
    EditorSync::apply_client_delta(world, position)?;
    Ok(added)
}

// A clock switching between paused and playing is one field, so undo and redo put back the whole state.
#[tokio::test]
async fn a_switch_between_clock_states_undoes_and_redoes() -> anyhow::Result<()> {
    let mut world = world()?;
    let clock = clock_id(&world);
    let original = clock_state(&world, clock);
    let other = match original.get("Paused") {
        Some(_) => json!({ "Playing": { "localStartMs": 0.0, "timeLocation": "Client" } }),
        None => json!({ "Paused": { "animationTimeSecs": 0.5 } }),
    };
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "State on Clock", vec![set_clock_state(clock, other.clone())?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(clock_state(&world, clock), original);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(clock_state(&world, clock), other);

    history.begin_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(history.end_chat_turn(&mut world), vec![], "The person's switch is not the AI's");
    Ok(())
}

// Undoing the AI putting a free ball into a rigid group puts it back where it was in the world.
#[tokio::test]
async fn undo_takes_an_object_back_out_of_a_rigid_group_to_where_it_was() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let group = add_rigid_group(&mut world, "Cart", 10.0)?;
    let ball = add_ball(&mut world, "Loose", root, 2.0)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Loose", vec![set_name(ball, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(ball, group)?;
    let position = move_by(&world, ball, 11.0 - world_x(&world, ball))?;
    EditorSync::apply_client_delta(&mut world, position)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(world.parentOfId.get(&ball), Some(&root));
    assert!((world_x(&world, ball) - 2.0).abs() < 1e-4);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(world.parentOfId.get(&ball), Some(&group));
    assert!((world_x(&world, ball) - 11.0).abs() < 1e-4);
    Ok(())
}

// A part's place belongs to its group. When the AI moves a folder holding it into another group, undo
// does not write the old group's place into the new one.
#[tokio::test]
async fn a_part_moved_into_another_group_with_its_folder_stays_where_it_is() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(folder, second)?;
    history.end_chat_turn(&mut world);
    let x = world_x(&world, part);

    undo(&mut world, &mut history, "tab-a")?;
    assert!((world_x(&world, part) - x).abs() < 1e-4, "The part does not jump");
    Ok(())
}

// What went away with a removed object is saved again on every removal, so redo brings back only what
// went away the last time.
#[tokio::test]
async fn what_goes_away_with_an_add_is_saved_again_each_time() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let tree = add_group(&mut world, "Tree", root);
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let chimney = add_group(&mut world, "Chimney", house);
    let animator = animator_pointing_at(&mut world, house);
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    world.setParent(chimney, root)?;
    let repoint: EditorClientDelta = serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": tree }))?;
    EditorSync::apply_client_delta(&mut world, repoint)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert!(world.hasObject(chimney), "The chimney had left the house");
    assert_eq!(animator_target(&world, animator), json!(tree), "The animator points elsewhere now");
    Ok(())
}

// An object the step already holds that goes away with a removed object is remembered too.
#[tokio::test]
async fn an_object_of_the_step_that_goes_away_with_a_removed_one_comes_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let animator = animator_pointing_at(&mut world, house);
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    world.setParent(animator, house)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(!world.hasObject(animator));
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(animator), "The animator comes back inside the house");
    assert_eq!(world.parentOfId.get(&animator), Some(&house));
    Ok(())
}

// An object whose parent cannot come back still gets its other fields back.
#[tokio::test]
async fn an_object_whose_parent_cannot_come_back_still_gets_its_other_fields() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let outer = add_group(&mut world, "Outer", root);
    let inner = add_group(&mut world, "Inner", outer);
    let item = add_group(&mut world, "Item", inner);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Item", vec![set_name(item, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(item, root)?;
    EditorSync::apply_client_delta(&mut world, set_name(item, "AI-item")?)?;
    world.removeObject(inner)?;
    history.end_chat_turn(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(outer)?;
    history.end_chat_turn(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, item), "Mine");
    Ok(())
}

// A step that can write nothing, because the only thing left is a parent that is gone, is passed over.
#[tokio::test]
async fn a_step_that_can_write_nothing_is_passed_over() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let garage = add_group(&mut world, "Garage", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, garage)?;
    history.end_chat_turn(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(name(&world, door), "Door");
    Ok(())
}

// A link a person makes while the AI runs counts as made before the reply, so its target comes back
// with it when the AI deletes it.
#[tokio::test]
async fn a_link_made_during_a_reply_comes_back_with_its_target() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let first = add_group(&mut world, "First", root);
    let target = add_group(&mut world, "Target", root);
    let animator = animator_pointing_at(&mut world, first);
    let mut history = History::new(&mut world);
    history.begin_chat_turn(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": target }))?;
    apply_and_record(&mut world, &mut history, "tab-a", "Target on Animator", vec![repoint], None)?;
    world.removeObject(target)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(target));
    assert_eq!(animator_target(&world, animator), json!(target));
    Ok(())
}

// Taking away and bringing back what the AI made during the same reply leaves it the AI's.
#[tokio::test]
async fn what_the_ai_made_stays_the_ais_through_an_undo_and_redo() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let chimney = add_group(&mut world, "Chimney", house);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| (row.object_id, row.change)).collect::<Vec<_>>(), vec![(chimney, EditorTurnChange::created)]);
    Ok(())
}

// A part has no motion of its own, so setting a part's velocity changes nothing and records nothing.
#[tokio::test]
async fn setting_a_parts_velocity_records_nothing() -> anyhow::Result<()> {
    let mut world = world()?;
    let (_, part) = rigid_part(&world);
    let mut history = History::new(&mut world);
    let velocity: EditorClientDelta = serde_json::from_value(json!({ "objectId": part, "section": "physicsInfo3DOfId", "path": ["movement", "velocity"], "value": [1.0, 0.0, 0.0] }))?;
    apply_and_record(&mut world, &mut history, "tab-a", "Velocity on Part", vec![velocity], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    Ok(())
}

// Undoing a part's move after the AI took its folder into another group does not write the old
// group's place into the new one.
#[tokio::test]
async fn undoing_a_parts_move_after_it_changed_groups_does_not_jump() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    let move_part = move_by(&world, part, 0.5)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Move Part", vec![move_part], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(folder, second)?;
    history.end_chat_turn(&mut world);
    let x = world_x(&world, part);

    undo(&mut world, &mut history, "tab-a")?;
    assert!((world_x(&world, part) - x).abs() < 1e-4, "The part does not jump");
    Ok(())
}

// A person's undo during a reply that switches a clock back counts as the person's, even when the AI
// Undo during a reply goes back through the AI's newest change to the person's object first.
#[tokio::test]
async fn undo_during_a_reply_takes_back_the_ais_newer_change_first() -> anyhow::Result<()> {
    let mut world = world()?;
    let clock = clock_id(&world);
    let playing = json!({ "Playing": { "localStartMs": 0.0, "timeLocation": "Client" } });
    let paused = json!({ "Paused": { "animationTimeSecs": 0.5 } });
    EditorSync::apply_client_delta(&mut world, set_clock_state(clock, playing.clone())?)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "State on Clock", vec![set_clock_state(clock, paused.clone())?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_clock_state(clock, json!({ "Playing": { "localStartMs": 9.0, "timeLocation": "Client" } }))?)?;

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(clock_state(&world, clock), paused);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("State on Clock"));
    assert_eq!(clock_state(&world, clock), playing);
    assert_eq!(history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![clock], "The AI did change the clock");
    Ok(())
}

// A step's fields inside a clock state the clock has since left are left alone.
#[tokio::test]
async fn an_edit_inside_a_state_the_clock_has_left_is_left_alone() -> anyhow::Result<()> {
    let mut world = world()?;
    let clock = clock_id(&world);
    EditorSync::apply_client_delta(&mut world, set_clock_state(clock, json!({ "Paused": { "animationTimeSecs": 0.5 } }))?)?;
    let playing = json!({ "Playing": { "localStartMs": 0.0, "timeLocation": "Client" } });
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "State on Clock", vec![set_clock_state(clock, json!({ "Paused": { "animationTimeSecs": 0.75 } }))?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "State on Clock", vec![set_clock_state(clock, playing.clone())?], None)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_none());
    assert_eq!(clock_state(&world, clock), playing);
    Ok(())
}

// An object of the step's own that went away inside something the step removed comes back with it.
#[tokio::test]
async fn an_object_of_the_step_that_went_away_inside_a_removed_group_comes_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let lamp = animator_pointing_at(&mut world, house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    world.setParent(lamp, house)?;
    history.end_chat_turn(&mut world);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(!world.hasObject(lamp));

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(world.hasObject(lamp), "The lamp comes back inside the house");
    assert_eq!(world.parentOfId.get(&lamp), Some(&house));
    assert_eq!(animator_target(&world, lamp), json!(house));
    Ok(())
}

// An object of the step's own that linked to something the step removed gets that link back.
#[tokio::test]
async fn an_object_of_the_step_gets_back_its_link_to_what_the_step_removed() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let tree = add_group(&mut world, "Tree", root);
    let house = add_group(&mut world, "House", root);
    let lamp = animator_pointing_at(&mut world, tree);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(lamp, "Mine")?, set_name(house, "My-house")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(lamp, "AI-lamp")?)?;
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(json!({ "objectId": lamp, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": house }))?;
    EditorSync::apply_client_delta(&mut world, repoint)?;
    history.end_chat_turn(&mut world);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(animator_target(&world, lamp), Value::Null);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(animator_target(&world, lamp), json!(house));
    Ok(())
}

// Undo never leaves a link to an object that is gone.
#[tokio::test]
async fn undo_never_leaves_a_link_to_an_object_that_is_gone() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let first = add_group(&mut world, "First", root);
    let second = add_group(&mut world, "Second", root);
    let animator = animator_pointing_at(&mut world, first);
    let mut history = History::new(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": second }))?;
    apply_and_record(&mut world, &mut history, "tab-a", "Target on Animator", vec![repoint], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(first)?;
    history.end_chat_turn(&mut world);

    undo(&mut world, &mut history, "tab-a")?;
    assert_ne!(animator_target(&world, animator), json!(first));
    Ok(())
}

// Undoing and redoing a removal while the AI runs leaves the AI's changes to what came back with the AI.
#[tokio::test]
async fn an_undo_and_redo_during_a_reply_leaves_the_ais_changes_the_ais() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let lamp = add_group(&mut world, "Lamp", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Lamp", vec![set_name(lamp, "Theirs")?], None)?;
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    world.setParent(lamp, house)?;
    history.end_chat_turn(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(lamp, "AI-lamp")?)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![lamp]);

    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, lamp), "Theirs");
    Ok(())
}

// A field an undo cannot write, like a parent that is gone, is left alone, and the rest still applies.
#[tokio::test]
async fn an_undo_during_a_reply_skips_a_parent_that_is_gone() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let garage = add_group(&mut world, "Garage", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, garage)?;
    EditorSync::apply_client_delta(&mut world, set_name(door, "AI-door")?)?;
    history.end_chat_turn(&mut world);
    world.removeObject(house)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(world.parentOfId.get(&door), Some(&garage), "The newest AI change, the move to the root, goes first");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, door), "Mine");
    assert_eq!(world.parentOfId.get(&door), Some(&garage), "The house is gone, so the door stays where it is");
    history.end_chat_turn(&mut world);
    Ok(())
}

// Undo only clears links on the objects it writes.
#[tokio::test]
async fn undo_leaves_other_objects_links_alone() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let target = add_group(&mut world, "Target", root);
    let animator = animator_pointing_at(&mut world, target);
    let dangling: EditorClientDelta = serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": 999999 }))?;
    EditorSync::apply_client_delta(&mut world, dangling)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(animator_target(&world, animator), json!(999999));
    Ok(())
}

// A reply is cut into steps wherever someone acts in the middle of it, and they undo in order.
#[tokio::test]
async fn a_reply_cut_by_someone_elses_edit_undoes_in_order() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original_radius = read_radius(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Theirs")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "AI-name")?)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Radius on Ball", vec![set_radius(ball, 3.0)?], None)?;
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 7.0)?)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert_eq!((name(&world, ball), read_radius(&world, ball)), ("AI-name".to_string(), 3.0), "The AI's later radius goes first");
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Theirs");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Radius on Ball"));
    assert_eq!(read_radius(&world, ball), original_radius);
    Ok(())
}

// Undo walks back in the order things happened, across objects too: the AI's change to one of your
// objects that came before your edit to another is undone after it.
#[tokio::test]
async fn undo_follows_the_order_things_happened_across_objects() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let group = add_group(&mut world, "Group", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(ball, "My-ball")?, set_name(group, "My-group")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "AI-ball")?)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Group", vec![set_name(group, "Group-again")?], None)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Group"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "My-ball");
    Ok(())
}

// Someone acting in the middle of a reply, after the AI took away the parent and the link target of
// an object a person edited, still leaves both to come back with the AI's changes.
#[tokio::test]
async fn a_parent_and_link_target_deleted_before_a_cut_still_come_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let target = add_group(&mut world, "Target", root);
    let animator = animator_pointing_at(&mut world, target);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(door, "Mine")?, set_name(animator, "My-animator")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;
    world.removeObject(house)?;
    world.removeObject(target)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(world.parentOfId.get(&door), Some(&house));
    assert_eq!(animator_target(&world, animator), json!(target));
    Ok(())
}

// An object linking to a deleted group that someone else edits mid-reply still gets its link back.
#[tokio::test]
async fn a_link_to_a_deleted_group_comes_back_after_the_linking_object_is_cut() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let animator = animator_pointing_at(&mut world, house);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-c", "Name on Animator", vec![set_name(animator, "Theirs")?], None)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house) && world.hasObject(door));
    assert_eq!(animator_target(&world, animator), json!(house));
    Ok(())
}

// An object the AI moved out of a group before deleting that group, and later deleted too, still comes
// back after someone acted in between.
#[tokio::test]
async fn an_object_moved_out_of_a_deleted_group_and_deleted_later_comes_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let garage = add_group(&mut world, "Garage", root);
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", garage);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Garage", vec![set_name(garage, "Their-garage")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(door, house)?;
    world.removeObject(garage)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
    world.removeObject(house)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(door), "The door comes back");
    Ok(())
}

// Something a person takes away during a reply is not listed as the AI's deletion.
#[tokio::test]
async fn what_a_person_takes_away_during_a_reply_is_not_listed_as_deleted() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(house, "AI-house")?)?;
    publish(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert_eq!(history.end_chat_turn(&mut world), vec![]);
    Ok(())
}

// Something a person's own action takes away during a reply is not listed as the AI's deletion either.
#[tokio::test]
async fn what_a_persons_action_takes_away_during_a_reply_is_not_listed() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(door, "AI-door")?)?;
    publish(&mut world);
    add_and_record(&mut world, &mut history, "tab-a", "Delete House", |world| world.removeObject(house).unwrap());
    assert_eq!(history.read_chat_turn_changes(&world), vec![]);
    assert_eq!(history.end_chat_turn(&mut world), vec![]);
    Ok(())
}

// The AI changing one of your objects does not take away what you can redo: its change goes below the
// steps you undid.
#[tokio::test]
async fn an_ai_change_to_your_object_keeps_your_redo() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "Door", root);
    let window = add_group(&mut world, "Window", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue-door")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Window", vec![set_name(window, "Green-window")?], None)?;
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Window"));
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(door, "AI-door")?)?;
    publish(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Window"));
    assert_eq!(name(&world, window), "Green-window");
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Window"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "Blue-door");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    Ok(())
}

// Many AI steps slotted below the same undone step still keep their order and the redo.
#[tokio::test]
async fn many_ai_steps_below_an_undone_step_keep_their_order() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "Door", root);
    let window = add_group(&mut world, "Window", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Window", vec![set_name(window, "My-window")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    for index in 0..30 {
        EditorSync::apply_client_delta(&mut world, set_name(door, &format!("AI-{index}"))?)?;
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, &format!("B-{index}"))?], None)?;
    }
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Window"));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    for index in (0..29).rev() {
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
        assert_eq!(name(&world, door), format!("AI-{index}"));
    }
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "Mine");
    Ok(())
}

// Moving a folder from one rigid group to another leaves the parts inside where they are, so only the
// folder is listed and nobody gets a step for the parts. A part the AI also moved still counts.
#[tokio::test]
async fn parts_that_only_switched_groups_with_their_folder_are_not_changed() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let nudged = add_ball(&mut world, "Nudged", folder, 4.0)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(folder, second)?;
    let nudge = move_by(&world, nudged, 1.0)?;
    EditorSync::apply_client_delta(&mut world, nudge)?;
    publish(&mut world);
    let mut rows: Vec<ObjectId> = history.read_chat_turn_changes(&world).iter().map(|row| row.object_id).collect();
    rows.sort_unstable();
    assert_eq!(rows, vec![folder, nudged]);
    let mut object_changes: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
    object_changes.sort_unstable();
    assert_eq!(object_changes, vec![folder, nudged]);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Part"), "No step for a part that only switched groups");
    Ok(())
}

// Each session knows whether it has anything to undo or redo, including steps the AI's work gives it.
#[tokio::test]
async fn each_session_knows_whether_it_can_undo_or_redo() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    let availability = |history: &EditorHistory, author: &str| history.read_availability().get(author).map(|availability| (availability.can_undo, availability.can_redo)).unwrap_or_default();
    assert_eq!(availability(&history, "tab-a"), (false, false));
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    assert_eq!(availability(&history, "tab-a"), (true, false));
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(availability(&history, "tab-a"), (false, true));
    apply_and_record(&mut world, &mut history, "tab-a", "Radius on Ball", vec![set_radius(ball, 3.0)?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 5.0)?)?;
    history.end_chat_turn(&mut world);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(availability(&history, "tab-a"), (true, true), "The AI's step was undone and the person's own is left");
    assert_eq!(availability(&history, "tab-b"), (false, false));
    Ok(())
}

// An AI step that lands after a redo is still one to undo, before the steps it came after.
#[tokio::test]
async fn an_ai_change_after_a_redo_is_undone_first() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "Door", root);
    let window = add_group(&mut world, "Window", root);
    let lamp = add_group(&mut world, "Lamp", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue-door")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Window", vec![set_name(window, "Green-window")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Lamp", vec![set_name(lamp, "Red-lamp")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(door, "AI-door")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "Blue-door");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Window"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Window"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Lamp"), "The lamp can still be redone");
    Ok(())
}

// Redoing a step over a field the AI changed meanwhile, then undoing it, shows the AI's value again.
#[tokio::test]
async fn undoing_a_redo_over_the_ais_change_shows_the_ais_value() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let window = add_group(&mut world, "Window", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Window", vec![set_name(window, "Green")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Window", vec![set_name(window, "Blue")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(window, "Red")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, window), "Blue");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, window), "Red");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, window), "Green");
    Ok(())
}

// A part whose group moved earlier in the reply, then only switched groups with its folder, is still not
// counted as changed.
#[tokio::test]
async fn a_part_that_only_switched_groups_after_its_group_moved_is_not_changed() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    let move_first = move_by(&world, first, 3.0)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Move First", vec![move_first], None)?;
    publish(&mut world);
    world.setParent(folder, second)?;
    publish(&mut world);
    let object_changes = history.end_chat_turn(&mut world);
    assert_eq!(object_changes.iter().map(|row| row.object_id).collect::<Vec<_>>(), vec![folder]);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Part"));
    Ok(())
}

// A part that really turned while switching groups is still counted.
#[tokio::test]
async fn a_part_that_turned_while_switching_groups_is_changed() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(folder, second)?;
    let turn: EditorClientDelta = serde_json::from_value(json!({ "objectId": part, "section": "physicsInfo3DOfId", "path": ["location", "quaternion"], "value": [0.0, 0.0017453, 0.0, 0.9999985] }))?;
    EditorSync::apply_client_delta(&mut world, turn)?;
    publish(&mut world);
    let mut object_changes: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
    object_changes.sort_unstable();
    assert_eq!(object_changes, vec![folder, part]);
    Ok(())
}

// The AI moves the door out of the house, someone acts, then the AI deletes the house: undo still brings
// the house back and then puts the door back in it.
#[tokio::test]
async fn a_group_deleted_after_a_cut_still_comes_back_for_what_left_it() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let first = add_group(&mut world, "First", root);
    let second = add_group(&mut world, "Second", root);
    let animator = animator_pointing_at(&mut world, first);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(door, "Mine")?, set_name(animator, "My-animator")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;
    let repoint: EditorClientDelta = serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": second }))?;
    EditorSync::apply_client_delta(&mut world, repoint)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
    world.removeObject(house)?;
    world.removeObject(first)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house) && world.hasObject(first));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(world.parentOfId.get(&door), Some(&house));
    assert_eq!(animator_target(&world, animator), json!(first));
    Ok(())
}

// White door, you set it blue and undo it, the AI sets it yellow, you redo: blue. Undo then shows the
// AI's yellow before going back to white, so the AI's change is never lost.
#[tokio::test]
async fn an_ai_change_to_something_you_undid_comes_back_in_order() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "White", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(door, "Yellow")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(name(&world, door), "Blue");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(name(&world, door), "Yellow");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "White");
    Ok(())
}

// Redoing a step leaves alone what the AI changed on fields the step never touched.
#[tokio::test]
async fn a_redo_keeps_the_ais_change_to_fields_the_step_never_touched() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Green")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    let x = world_x(&world, ball);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 5.0)?)?;
    shift(&mut world, ball, 10.0)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!((name(&world, ball), read_radius(&world, ball)), ("Green".into(), 5.0));
    assert!((world_x(&world, ball) - x - 10.0).abs() < 1e-3);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!((name(&world, ball), read_radius(&world, ball)), ("Blue".into(), 5.0));
    Ok(())
}

// Only the first undone step on a field starts from the AI's value, so walking back down after redoing
// them all shows each of the person's values before the AI's.
#[tokio::test]
async fn walking_back_after_redoing_over_an_ai_change_shows_every_value() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    publish(&mut world);
    for color in ["Blue", "Green", "Yellow"] {
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, color)?], None)?;
    }
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    let mut names = Vec::new();
    for direction in [EditorHistoryDirection::redo, EditorHistoryDirection::redo, EditorHistoryDirection::undo, EditorHistoryDirection::undo, EditorHistoryDirection::undo] {
        assert!(history.take_step(&mut world, "tab-a", direction)?.is_some());
        names.push(name(&world, ball));
    }
    assert_eq!(names, ["Green", "Yellow", "Green", "Red", "Blue"]);
    Ok(())
}

// An undone AI step stays when the AI changes its object again, starting from the newer value, so every
// value comes back in order.
#[tokio::test]
async fn an_undone_ai_step_stays_when_the_ai_changes_its_object_again() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blue")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "Red")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Both on Ball", vec![set_name(ball, "Green")?, set_radius(ball, 2.0)?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    let radius = read_radius(&world, ball);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_radius(ball, 5.0)?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    let mut states = Vec::new();
    for direction in [EditorHistoryDirection::redo, EditorHistoryDirection::redo, EditorHistoryDirection::undo, EditorHistoryDirection::undo, EditorHistoryDirection::undo] {
        assert!(history.take_step(&mut world, "tab-a", direction)?.is_some());
        states.push((name(&world, ball), read_radius(&world, ball)));
    }
    let expected = [("Red", 5.0), ("Green", 2.0), ("Red", 5.0), ("Blue", 5.0), ("Blue", radius)];
    assert_eq!(states, expected.map(|(name, radius)| (name.to_string(), radius)));
    Ok(())
}

// Every step from the latest reply survives a full log, so the whole reply can be taken back even when
// someone acting in the middle split it.
#[tokio::test]
async fn every_step_of_the_latest_reply_survives_a_full_log() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "Door", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    for index in 0..200 {
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, &format!("Door-{index}"))?], None)?;
    }
    while undo(&mut world, &mut history, "tab-a")?.is_some() {}
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(door, "Red")?)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
    EditorSync::apply_client_delta(&mut world, set_name(door, "Yellow")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "Red");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, door), "Door");
    Ok(())
}

// A part the AI also moved while its folder switched groups still counts, whichever group moved and when,
// so undoing the switch puts the part back in its place.
#[tokio::test]
async fn a_part_the_ai_moved_while_switching_groups_is_changed() -> anyhow::Result<()> {
    for moved in ["old group before", "new group after", "old group after"] {
        let mut world = world()?;
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let second = add_rigid_group(&mut world, "Second", 10.0)?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(part, "Mine")?, set_name(folder, "My-folder")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        if moved == "old group before" {
            shift(&mut world, first, 3.0)?;
        }
        world.setParent(folder, second)?;
        shift(&mut world, part, 1.0)?;
        match moved {
            "new group after" => shift(&mut world, second, 3.0)?,
            "old group after" => shift(&mut world, first, 3.0)?,
            _ => {}
        }
        publish(&mut world);
        let listed: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
        assert!(listed.contains(&part), "{moved}");

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{moved}");
        assert_eq!(world.parentOfId.get(&folder), Some(&first), "{moved}");
        assert!((relative_x(&world, part) - 2.0).abs() < 1e-3, "{moved}");
    }
    Ok(())
}

// A part that only went along with its new group after switching to it is not changed either, while the
// reply runs and once it ends.
#[tokio::test]
async fn a_part_that_only_moved_with_its_new_group_is_not_changed() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(folder, second)?;
    shift(&mut world, second, 3.0)?;
    publish(&mut world);
    let live: Vec<ObjectId> = history.read_chat_turn_changes(&world).iter().map(|row| row.object_id).collect();
    let listed: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
    assert!(!listed.contains(&part));
    assert_eq!(live, listed);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Part"));
    Ok(())
}

// A group's move carries its parts, so a part a group the AI made carried off is not changed, whether or not that
// group moved. Undoing the switch still puts the part back in its place.
#[tokio::test]
async fn a_part_carried_off_by_a_group_the_ai_made_is_not_changed() -> anyhow::Result<()> {
    for is_cart_moved in [false, true] {
        let mut world = world()?;
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(part, "Mine")?, set_name(folder, "My-folder")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        let cart = add_rigid_group(&mut world, "Cart", 20.0)?;
        world.setParent(folder, cart)?;
        if is_cart_moved {
            shift(&mut world, cart, 3.0)?;
        }
        publish(&mut world);
        let live: Vec<ObjectId> = history.read_chat_turn_changes(&world).iter().map(|row| row.object_id).collect();
        let listed: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
        assert!(!listed.contains(&part), "{is_cart_moved}");
        assert_eq!(live, listed, "{is_cart_moved}");

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_cart_moved}");
        assert_eq!(world.parentOfId.get(&folder), Some(&first), "{is_cart_moved}");
        assert!((relative_x(&world, part) - 2.0).abs() < 1e-3, "{is_cart_moved}");
    }
    Ok(())
}

// A deletion of the group an object left earlier in the reply comes back only for whoever got that move.
#[tokio::test]
async fn a_later_deletion_comes_back_only_for_whoever_got_the_earlier_move() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    world.removeObject(house)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert!(!world.hasObject(house));
    Ok(())
}

// A change that kept an object's link does not bring back the object it links to when that is deleted later.
#[tokio::test]
async fn a_later_deletion_of_a_link_the_change_kept_does_not_come_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let first = add_group(&mut world, "First", root);
    let second = add_group(&mut world, "Second", root);
    let animator = animator_pointing_at(&mut world, first);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Animator", vec![set_name(animator, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(animator, "AI-animator")?)?;
    publish(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": second }))?;
    apply_and_record(&mut world, &mut history, "tab-b", "Target on Animator", vec![repoint], None)?;
    world.removeObject(first)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, animator), "Mine");
    assert!(!world.hasObject(first));
    Ok(())
}

fn place(world: &mut World, object_id: ObjectId, position: [f64; 3], quaternion: [f64; 4]) -> anyhow::Result<()> {
    let delta: EditorClientDelta = serde_json::from_value(json!({ "objectId": object_id, "section": "physicsInfo3DOfId", "path": ["location"], "value": { "position": position, "quaternion": quaternion } }))?;
    EditorSync::apply_client_delta(world, delta)?;
    Ok(())
}

// A deletion later in the reply only comes back for someone whose own objects left or pointed at it: an object
// that came back only with a deleted group brings back nothing else it pointed at or sat in.
#[tokio::test]
async fn a_later_deletion_does_not_come_back_through_what_only_returned_with_a_group() -> anyhow::Result<()> {
    for is_linked in [true, false] {
        let mut world = world()?;
        let ball = ball_id(&world);
        let root = world.getRootObjectId();
        let house = add_group(&mut world, "House", root);
        let door = add_group(&mut world, "Door", house);
        let outer = add_group(&mut world, "Outer", root);
        let lamp = animator_pointing_at(&mut world, if is_linked { outer } else { house });
        world.setParent(lamp, if is_linked { house } else { outer })?;
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        if !is_linked {
            world.setParent(lamp, root)?;
        }
        world.removeObject(house)?;
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
        world.removeObject(outer)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_linked}");
        assert!(world.hasObject(house) && world.hasObject(door), "{is_linked}");
        assert!(!world.hasObject(outer), "{is_linked}: nobody's step needs Outer");
    }
    Ok(())
}

// Undoing and redoing the running reply's own deletion leaves it listed as the AI's.
#[tokio::test]
async fn a_deletion_undone_and_redone_during_its_reply_stays_listed() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let crate_id = add_group(&mut world, "Crate", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Crate", vec![set_name(crate_id, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(crate_id)?;
    publish(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    publish(&mut world);
    let listed = history.end_chat_turn(&mut world);

    assert!(listed.iter().any(|row| row.object_id == crate_id && row.change == EditorTurnChange::deleted));
    Ok(())
}

// A part whose folder switched groups is compared with where its groups were at that same moment, even when
// someone acts or moves one of the groups in between, so it is never an AI change and a redo of the person's own
// move never throws it.
#[tokio::test]
async fn a_part_that_only_switched_groups_stays_put_whoever_acts_in_between() -> anyhow::Result<()> {
    for then in ["AI moves old group", "AI moves new group", "AI deletes old group", "person moves old group", "person moves new group"] {
        let mut world = world()?;
        let ball = ball_id(&world);
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let second = add_rigid_group(&mut world, "Second", 10.0)?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let mut history = History::new(&mut world);
        publish(&mut world);
        let nudge = move_by(&world, part, 0.5)?;
        apply_and_record(&mut world, &mut history, "tab-a", "Move Part", vec![nudge], None)?;
        assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        world.setParent(folder, second)?;
        publish(&mut world);
        match then {
            "AI moves old group" => shift(&mut world, first, 3.0)?,
            "AI moves new group" => shift(&mut world, second, 3.0)?,
            "AI deletes old group" => world.removeObject(first)?,
            _ => {}
        }
        publish(&mut world);
        let person_action = match then {
            "person moves old group" => move_by(&world, first, 3.0)?,
            "person moves new group" => move_by(&world, second, 3.0)?,
            _ => set_name(ball, "Other")?,
        };
        apply_and_record(&mut world, &mut history, "tab-b", "Person", vec![person_action], None)?;
        publish(&mut world);
        let live: Vec<ObjectId> = history.read_chat_turn_changes(&world).iter().map(|row| row.object_id).collect();
        let listed: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
        assert!(!live.contains(&part) && !listed.contains(&part), "{then}");

        let x = world_x(&world, part);
        assert_eq!(redo(&mut world, &mut history, "tab-a")?, None, "{then}");
        assert!((world_x(&world, part) - x).abs() < 1e-3, "{then}");
    }
    Ok(())
}

// A person's undone move of a part is measured in its old group, so once the AI moves the part to another group
// the redo has nothing left to put back there.
#[tokio::test]
async fn a_redo_never_writes_a_part_place_into_another_group() -> anyhow::Result<()> {
    for is_nudged in [false, true] {
        let mut world = world()?;
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let second = add_rigid_group(&mut world, "Second", 10.0)?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let mut history = History::new(&mut world);
        publish(&mut world);
        let nudge = move_by(&world, part, 0.5)?;
        apply_and_record(&mut world, &mut history, "tab-a", "Move Part", vec![nudge], None)?;
        assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        world.setParent(part, second)?;
        if is_nudged {
            shift(&mut world, part, 1.0)?;
        }
        publish(&mut world);
        history.end_chat_turn(&mut world);

        let x = world_x(&world, part);
        redo(&mut world, &mut history, "tab-a")?;
        assert!((world_x(&world, part) - x).abs() < 1e-3, "{is_nudged}: the part stays where the AI left it");
    }
    Ok(())
}

// The chat list while the reply runs agrees with the one it ends with after someone acts in between.
#[tokio::test]
async fn the_live_list_agrees_with_the_final_one_after_a_cut() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    let part = add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    shift(&mut world, first, 3.0)?;
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
    world.setParent(folder, second)?;
    shift(&mut world, first, 3.0)?;
    publish(&mut world);
    let mut live: Vec<ObjectId> = history.read_chat_turn_changes(&world).iter().map(|row| row.object_id).collect();
    let mut listed: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();
    live.sort_unstable();
    listed.sort_unstable();
    assert_eq!(live, listed);
    assert!(!listed.contains(&part));
    Ok(())
}

// Far from the origin, with turned groups, a part that only switched groups with its folder is still not changed.
#[tokio::test]
async fn a_part_that_only_switched_groups_far_away_is_not_changed() -> anyhow::Result<()> {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    for scale in [1e4, 3e4] {
        let mut world = world()?;
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let second = add_rigid_group(&mut world, "Second", 0.0)?;
        place(&mut world, first, [3.0 * scale + 0.37, 5.0, 2.7 * scale + 0.11], [0.0, half, 0.0, half])?;
        place(&mut world, second, [3.3 * scale + 0.53, 2.0, -3.0 * scale + 0.29], [0.0, 0.0, half, half])?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 0.0)?;
        place(&mut world, part, [3.0 * scale + 2.37, 5.5, 2.7 * scale + 1.11], [0.0, 0.0, 0.0, 1.0])?;
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        place(&mut world, first, [3.0 * scale + 10.0, 5.0, 2.7 * scale], [0.0, 0.0, 0.0, 1.0])?;
        world.setParent(folder, second)?;
        place(&mut world, second, [3.3 * scale - 10.0, 2.0, -3.0 * scale + 9.0], [0.38268343, 0.0, 0.0, 0.9238795])?;
        publish(&mut world);
        let listed: Vec<ObjectId> = history.end_chat_turn(&mut world).iter().map(|row| row.object_id).collect();

        assert!(!listed.contains(&part), "{scale}");
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Part"), "{scale}");
    }
    Ok(())
}

// The AI deletes the house holding your door and then the street holding the house. Undo brings the door back
// inside both, whether or not someone acted between the two deletions.
#[tokio::test]
async fn the_door_comes_back_when_the_ai_deletes_its_house_and_then_its_street() -> anyhow::Result<()> {
    for is_cut in [false, true] {
        let mut world = world()?;
        let ball = ball_id(&world);
        let root = world.getRootObjectId();
        let street = add_group(&mut world, "Street", root);
        let house = add_group(&mut world, "House", street);
        let door = add_group(&mut world, "Door", house);
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        world.removeObject(house)?;
        publish(&mut world);
        if is_cut {
            apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
        }
        world.removeObject(street)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_cut}");
        assert!(world.hasObject(street), "{is_cut}");
        if is_cut {
            assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_cut}: the house comes back after the street");
        }
        assert_eq!(world.parentOfId.get(&house), Some(&street), "{is_cut}");
        assert_eq!(world.parentOfId.get(&door), Some(&house), "{is_cut}");
        assert_eq!(name(&world, door), "Mine", "{is_cut}");
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"), "{is_cut}");
    }
    Ok(())
}

// The AI deletes the house your ball is in and the paint the ball uses, which is outside the house. Undo brings the
// paint back too, whether the AI deleted it first or after someone acted.
#[tokio::test]
async fn an_object_that_comes_back_gets_back_its_link_the_reply_deleted() -> anyhow::Result<()> {
    for is_cut in [false, true] {
        let mut world = world()?;
        let other = ball_id(&world);
        let root = world.getRootObjectId();
        let house = add_group(&mut world, "House", root);
        let ball = add_ball(&mut world, "Ball", house, 0.0)?;
        let paint = add_material(&mut world, "Paint", root)?;
        EditorSync::apply_client_delta(&mut world, set_ball_material(ball, paint, 2.0)?)?;
        let linked = ball_material(&world, ball);
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        if !is_cut {
            world.removeObject(paint)?;
        }
        world.removeObject(house)?;
        publish(&mut world);
        if is_cut {
            apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(other, "Other")?], None)?;
            world.removeObject(paint)?;
            publish(&mut world);
        }
        history.end_chat_turn(&mut world);

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_cut}");
        assert!(world.hasObject(paint), "{is_cut}");
        if is_cut {
            assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_cut}");
        }
        assert!(world.hasObject(house) && world.hasObject(ball), "{is_cut}");
        assert_eq!(ball_material(&world, ball), linked, "{is_cut}");
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"), "{is_cut}");
    }
    Ok(())
}

// Another session renaming your door, before or after your undo, does not change what your redo puts back.
#[tokio::test]
async fn a_redo_puts_back_your_own_value_over_another_sessions_edit() -> anyhow::Result<()> {
    for is_edited_before_undo in [true, false] {
        let mut world = world()?;
        let root = world.getRootObjectId();
        let door = add_group(&mut world, "White", root);
        let mut history = History::new(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue")?], None)?;
        if is_edited_before_undo {
            apply_and_record(&mut world, &mut history, "tab-b", "Name on Door", vec![set_name(door, "Green")?], None)?;
        }
        assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
        assert_eq!(name(&world, door), "White", "{is_edited_before_undo}");
        if !is_edited_before_undo {
            apply_and_record(&mut world, &mut history, "tab-b", "Name on Door", vec![set_name(door, "Green")?], None)?;
        }
        assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
        assert_eq!(name(&world, door), "Blue", "{is_edited_before_undo}");
    }
    Ok(())
}

// A part made before its rigid group has the smaller id, and still comes back inside the group when the group
// the AI deleted comes back.
#[tokio::test]
async fn a_part_made_before_its_group_comes_back_inside_it() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let part = add_ball(&mut world, "Part", root, 12.0)?;
    let cart = add_rigid_group(&mut world, "Cart", 10.0)?;
    world.setParent(part, cart)?;
    assert!(part < cart);
    let relative = relative_x(&world, part);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(cart)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(world.parentOfId.get(&part), Some(&cart));
    assert_eq!(world.physics.getSubitemRigidGroupId(part), Some(cart));
    assert!((relative_x(&world, part) - relative).abs() < 1e-3);
    assert_eq!(name(&world, part), "Mine");
    Ok(())
}

// A typing run only goes on while nobody else has written the field since, so another session's edit in the
// middle of it splits it.
#[tokio::test]
async fn a_typing_run_does_not_continue_over_another_sessions_edit() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let typing = Some(EditorTypingTarget { object_id: ball, section: EditorSection::objectOfId, path: vec!["name".into()] });
    let mut history = History::new(&mut world);
    for name in ["B", "Bl"] {
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, name)?], typing.clone())?;
    }
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Green")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Blu")?], typing.clone())?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Green", "Only the typing after tab B's edit is taken back");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// What came back inside a group with the AI's step is not the person's, so the AI's later change to it is nobody's.
#[tokio::test]
async fn what_comes_back_with_a_deleted_group_stays_nobodys() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let window = add_group(&mut world, "Window", house);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(window));
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(window, "AI-window")?)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(name(&world, window), "AI-window");
    Ok(())
}

// The log is bounded by how much its steps hold as well. One edit to a script as long as the budget is over it on its
// own, so it pushes out every older step but the latest reply's, and as the newest step it is kept itself.
#[tokio::test]
async fn the_oldest_steps_are_forgotten_when_the_log_holds_too_much() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let script = world.addObject("Script".into(), ObjectBeingAdded::Script(ScriptInfo { objectLocation: ObjectLocation::Server }), root);
    world.setScriptRawCode(script, "a".repeat(1 << 16));
    let mut history = History::new(&mut world);
    history.byte_limit = 1 << 16;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Theirs")?], None)?;
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(ball, "AI-name")?)?;
    history.end_chat_turn(&mut world);
    let set_code: EditorClientDelta =
        serde_json::from_value(json!({ "objectId": script, "section": "scriptRawCodeOfId", "path": [], "value": "b".repeat(1 << 16) }))?;
    apply_and_record(&mut world, &mut history, "tab-a", "Code on Script", vec![set_code], None)?;

    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert!(undo(&mut world, &mut history, "tab-b")?.is_none(), "Tab B's older step was forgotten");
    assert_eq!(name(&world, ball), "Theirs");
    let availability = |history: &EditorHistory, author: &str| history.read_availability().get(author).map(|availability| (availability.can_undo, availability.can_redo)).unwrap_or_default();
    assert_eq!(availability(&history, "tab-a"), (true, false));
    Ok(())
}

// What a redo replaced counts toward what the log holds, and forgetting stops once the log fits.
#[tokio::test]
async fn what_a_redo_replaced_counts_toward_what_the_log_holds() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let script = world.addObject("Script".into(), ObjectBeingAdded::Script(ScriptInfo { objectLocation: ObjectLocation::Server }), root);
    world.setScriptRawCode(script, "a".into());
    let mut history = History::new(&mut world);
    history.byte_limit = 3 << 15;
    apply_and_record(&mut world, &mut history, "tab-a", "Code on Script", vec![set_code(script, "b")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-b", "Code on Script", vec![set_code(script, &"c".repeat(1 << 16))?], None)?;
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Code on Script"));
    apply_and_record(&mut world, &mut history, "tab-c", "Name on Ball", vec![set_name(ball, "C")?], None)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_none(), "Tab A's step now holds tab B's code and is the oldest");
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Code on Script"), "Forgetting stops once the log fits");
    Ok(())
}

// A redo over another session's edit replaced their value, so undoing it gives their value back. When they had already
// set your value, the redo is passed over and there is nothing to undo.
#[tokio::test]
async fn undoing_a_redo_over_another_sessions_edit_gives_back_their_value() -> anyhow::Result<()> {
    for their_name in ["Green", "Blue"] {
        let mut world = world()?;
        let root = world.getRootObjectId();
        let door = add_group(&mut world, "White", root);
        let mut history = History::new(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue")?], None)?;
        assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
        apply_and_record(&mut world, &mut history, "tab-b", "Name on Door", vec![set_name(door, their_name)?], None)?;

        let is_passed_over = their_name == "Blue";
        assert_eq!(redo(&mut world, &mut history, "tab-a")?.is_none(), is_passed_over, "{their_name}");
        assert_eq!(name(&world, door), "Blue", "{their_name}");
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.is_none(), is_passed_over, "{their_name}");
        assert_eq!(name(&world, door), their_name);
    }
    Ok(())
}

// White door, you set it blue and undo it, the AI sets it blue too and later red: redo still gives your blue and undo
// then gives the AI's red, whether the AI's two changes were split by someone else's edit or came in two replies.
#[tokio::test]
async fn a_redo_gives_your_value_after_the_ai_set_it_and_then_changed_it() -> anyhow::Result<()> {
    for is_same_reply in [true, false] {
        let mut world = world()?;
        let ball = ball_id(&world);
        let root = world.getRootObjectId();
        let door = add_group(&mut world, "White", root);
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue")?], None)?;
        assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        EditorSync::apply_client_delta(&mut world, set_name(door, "Blue")?)?;
        publish(&mut world);
        match is_same_reply {
            true => apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?,
            false => {
                history.end_chat_turn(&mut world);
                history.begin_chat_turn(&mut world);
            }
        }
        EditorSync::apply_client_delta(&mut world, set_name(door, "Red")?)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);

        let mut names = Vec::new();
        for (direction, description) in [(EditorHistoryDirection::redo, "Name on Door"), (EditorHistoryDirection::undo, "Name on Door"), (EditorHistoryDirection::undo, "the AI's changes"), (EditorHistoryDirection::undo, "the AI's changes")] {
            assert_eq!(history.take_step(&mut world, "tab-a", direction)?.map(|step| step.description).as_deref(), Some(description), "{is_same_reply}");
            names.push(name(&world, door));
        }
        assert_eq!(names, ["Blue", "Red", "Blue", "White"], "{is_same_reply}");
    }
    Ok(())
}

// You set the door blue, the AI sets it red and you undo that. The AI sets it red again and later green: redoing the
// AI's first change still gives red, and undo then gives green, whether the AI's two changes were split by someone
// else's edit or came in two replies.
#[tokio::test]
async fn a_redo_of_an_undone_ai_step_gives_its_value_after_the_ai_set_it_again_and_changed_it() -> anyhow::Result<()> {
    for is_same_reply in [true, false] {
        let mut world = world()?;
        let ball = ball_id(&world);
        let root = world.getRootObjectId();
        let door = add_group(&mut world, "White", root);
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        EditorSync::apply_client_delta(&mut world, set_name(door, "Red")?)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
        assert_eq!(name(&world, door), "Blue");
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        EditorSync::apply_client_delta(&mut world, set_name(door, "Red")?)?;
        publish(&mut world);
        match is_same_reply {
            true => apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?,
            false => {
                history.end_chat_turn(&mut world);
                history.begin_chat_turn(&mut world);
            }
        }
        EditorSync::apply_client_delta(&mut world, set_name(door, "Green")?)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);

        let mut names = Vec::new();
        for direction in [EditorHistoryDirection::redo, EditorHistoryDirection::undo, EditorHistoryDirection::undo, EditorHistoryDirection::undo, EditorHistoryDirection::undo] {
            assert!(history.take_step(&mut world, "tab-a", direction)?.is_some(), "{is_same_reply}");
            names.push(name(&world, door));
        }
        assert_eq!(names, ["Red", "Green", "Red", "Blue", "White"], "{is_same_reply}");
    }
    Ok(())
}

// A part's place only means something in its group. The AI moved your part into another group and its old folder is
// gone later, so undo cannot put it back there and leaves the part where it is instead of writing its old place into
// the new group.
#[tokio::test]
async fn a_part_place_is_only_written_in_its_own_group() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let old_folder = add_group(&mut world, "Old", first);
    let new_folder = add_group(&mut world, "New", second);
    let part = add_ball(&mut world, "Part", old_folder, 2.0)?;
    let original = name(&world, part);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.setParent(part, new_folder)?;
    shift(&mut world, part, 1.0)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(old_folder)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    let x = world_x(&world, part);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Part"), "The AI's move has nothing left it can put back");
    assert_eq!(world.physics.getSubitemRigidGroupId(part), Some(second));
    assert!((world_x(&world, part) - x).abs() < 1e-3, "The part does not jump");
    assert_eq!(name(&world, part), original);
    Ok(())
}

// A step you undid and then replaced with a new one is gone, so it never writes its value again.
#[tokio::test]
async fn a_dropped_step_never_writes_again() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "A")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "B")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-a", "Radius on Ball", vec![set_radius(ball, 5.0)?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Theirs")?], None)?;

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Radius on Ball"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), original, "The dropped step never writes A over their name");
    Ok(())
}

// Objects a step holds that went away together come back together, whichever of them the step names first. You added
// a box and then a shelf, and the box was put on the shelf later, so redo puts the box back on the shelf.
#[tokio::test]
async fn objects_of_a_step_that_went_away_together_come_back_together() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let (mut box_id, mut shelf) = (0, 0);
    add_and_record(&mut world, &mut history, "tab-a", "Add two", |world| {
        box_id = add_group(world, "Box", root);
        shelf = add_group(world, "Shelf", root);
    });
    publish(&mut world);
    world.setParent(box_id, shelf)?;
    publish(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(!world.hasObject(box_id) && !world.hasObject(shelf));
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(world.parentOfId.get(&box_id), Some(&shelf), "The box comes back on the shelf");
    Ok(())
}

// A redo that takes objects away keeps them as it took them. The AI deleted a box and a shelf you had named, and you
// undid that. The box was put on the shelf, and after you redo the deletion and undo it again, the box is on the shelf.
#[tokio::test]
async fn an_undo_after_a_redo_brings_objects_back_as_the_redo_took_them() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let box_id = add_group(&mut world, "Box", root);
    let shelf = add_group(&mut world, "Shelf", root);
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Rename both", vec![set_name(box_id, "My-box")?, set_name(shelf, "My-shelf")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(box_id)?;
    world.removeObject(shelf)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    publish(&mut world);
    world.setParent(box_id, shelf)?;
    publish(&mut world);
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(!world.hasObject(box_id) && !world.hasObject(shelf));

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(world.parentOfId.get(&box_id), Some(&shelf), "The box comes back on the shelf");
    assert_eq!(name(&world, box_id), "My-box");
    Ok(())
}

// An object that comes back keeps no link to something that is gone. The AI deleted the house your ball was in, and in
// a later reply the paint the ball used, which was outside the house. Undo brings back the ball without the paint.
#[tokio::test]
async fn an_object_that_comes_back_drops_its_link_to_something_deleted_since() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let ball = add_ball(&mut world, "Ball", house, 0.0)?;
    let paint = add_material(&mut world, "Paint", root)?;
    EditorSync::apply_client_delta(&mut world, set_ball_material(ball, paint, 2.0)?)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(paint)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(ball) && !world.hasObject(paint));
    assert_eq!(ball_material(&world, ball), Value::Null);
    assert!(!world._objectsThatPointToId.get(&paint).is_some_and(|linking_ids| linking_ids.contains(&ball)), "The World holds no link to the paint");
    Ok(())
}

// Objects a step added come back with the links between them, whichever the step names first. You added a ball and
// then the paint it uses, so redo brings the ball back before its paint and the ball still uses it.
#[tokio::test]
async fn objects_a_step_added_come_back_with_the_links_between_them() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let (mut ball, mut paint) = (0, 0);
    add_and_record(&mut world, &mut history, "tab-a", "Add a painted ball", |world| {
        ball = add_ball(world, "Ball", root, 0.0).unwrap();
        paint = add_material(world, "Paint", root).unwrap();
        EditorSync::apply_client_delta(world, set_ball_material(ball, paint, 2.0).unwrap()).unwrap();
    });
    let linked = ball_material(&world, ball);
    publish(&mut world);

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(!world.hasObject(ball) && !world.hasObject(paint));
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(ball_material(&world, ball), linked, "The ball still uses its paint");
    Ok(())
}

// A redo that finds its object already gone still replaced something, so the undo after it leaves the object gone. Tab
// B added a house and you named its door. The AI deleted the house, you undid that, and tab B's undo took the house
// away again. Your redo has nothing to do, and neither has your undo.
#[tokio::test]
async fn an_undo_after_a_redo_that_found_its_object_gone_leaves_it_gone() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let (mut house, mut door) = (0, 0);
    add_and_record(&mut world, &mut history, "tab-b", "Add House", |world| {
        house = add_group(world, "House", root);
        door = add_group(world, "Door", house);
    });
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    publish(&mut world);
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house) && world.hasObject(door));
    publish(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Add House"));
    assert!(!world.hasObject(house));
    publish(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None, "The house is already gone");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?, None);
    assert!(!world.hasObject(house) && !world.hasObject(door), "Your undo does not bring back what tab B took away");
    Ok(())
}

// A redo that finds its object already there still replaced something, so the undo after it leaves the object there.
// You added a house, tab B named it and the AI deleted it. You undid both, and tab B's undo brought the house back, which
// tab C may have renamed since. Your redo passes over the add and deletes the house again, and after you undo that there
// is nothing left to undo.
#[tokio::test]
async fn an_undo_after_a_redo_that_found_its_object_there_leaves_it_there() -> anyhow::Result<()> {
    for is_renamed_since in [false, true] {
        let mut world = world()?;
        let root = world.getRootObjectId();
        let mut history = History::new(&mut world);
        let mut house = 0;
        add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-b", "Name on House", vec![set_name(house, "Theirs")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        world.removeObject(house)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
        assert!(!world.hasObject(house));
        publish(&mut world);
        assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
        assert!(world.hasObject(house));
        publish(&mut world);
        let later_name = match is_renamed_since {
            true => {
                apply_and_record(&mut world, &mut history, "tab-c", "Name on House", vec![set_name(house, "Later")?], None)?;
                "Later"
            }
            false => "Theirs",
        };

        assert_eq!(
            redo(&mut world, &mut history, "tab-a")?.as_deref(),
            Some("the AI's changes"),
            "The add is passed over, since the house is already there"
        );
        assert!(!world.hasObject(house));
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
        assert_eq!(name(&world, house), later_name);
        assert_eq!(undo(&mut world, &mut history, "tab-a")?, None);
        assert!(world.hasObject(house), "Your undo does not take away what tab B brought back");
    }
    Ok(())
}

// A part the AI moved in the same reply that its folder moved to another folder of the same group is still changed,
// so undo puts it back in its place, turned group or not. The folder's move is not yours and stays.
#[tokio::test]
async fn a_part_the_ai_moved_while_its_folder_moved_inside_its_group_is_changed() -> anyhow::Result<()> {
    for is_turned in [false, true] {
        let mut world = world()?;
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        if is_turned {
            place(&mut world, first, [0.37, 1.5, -7.25], [0.0, 0.3826834, 0.0, 0.9238795])?;
        }
        let old_folder = add_group(&mut world, "Old", first);
        let new_folder = add_group(&mut world, "New", first);
        let part = add_ball(&mut world, "Part", old_folder, 2.0)?;
        let original = relative_x(&world, part);
        let mut history = History::new(&mut world);
        publish(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Part", vec![set_name(part, "Mine")?], None)?;
        publish(&mut world);
        history.begin_chat_turn(&mut world);
        world.setParent(old_folder, new_folder)?;
        shift(&mut world, part, 1.0)?;
        publish(&mut world);
        history.end_chat_turn(&mut world);

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_turned}");
        assert!((relative_x(&world, part) - original).abs() < 1e-3, "{is_turned}");
        assert_eq!(world.parentOfId.get(&old_folder), Some(&new_folder), "{is_turned}");
    }
    Ok(())
}

// Whoever took an object away last decides whether the chat lists it. You added a house and the AI renamed it. You undid
// both, which took the house away, and redid your add. The AI then deleted the house, so it is listed as deleted, while the
// reply runs and once it ends.
#[tokio::test]
async fn an_object_the_ai_deletes_after_a_person_brought_it_back_is_listed_as_deleted() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(house, "AI-house")?)?;
    publish(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    world.removeObject(house)?;
    publish(&mut world);

    let live = history.read_chat_turn_changes(&world);
    let listed = history.end_chat_turn(&mut world);
    assert_eq!(live, listed);
    let rows: Vec<_> = listed.iter().map(|row| (row.object_id, row.name.as_str(), row.change)).collect();
    assert_eq!(rows, vec![(house, "House", EditorTurnChange::deleted)]);
    Ok(())
}

// An object whose link the AI's deletion cleared is listed as modified. The AI deleted the paint your ball uses, so the ball
// is listed with the paint, while the reply runs and once it ends.
#[tokio::test]
async fn an_object_whose_link_the_ais_deletion_cleared_is_listed() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let paint = add_material(&mut world, "Paint", root)?;
    EditorSync::apply_client_delta(&mut world, set_ball_material(ball, paint, 2.0)?)?;
    let mut history = History::new(&mut world);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    world.removeObject(paint)?;
    publish(&mut world);

    let mut live: Vec<_> = history.read_chat_turn_changes(&world).iter().map(|row| (row.object_id, row.change)).collect();
    let mut listed: Vec<_> = history.end_chat_turn(&mut world).iter().map(|row| (row.object_id, row.change)).collect();
    live.sort_unstable_by_key(|(object_id, _)| *object_id);
    listed.sort_unstable_by_key(|(object_id, _)| *object_id);
    assert_eq!(live, listed);
    let mut expected = vec![(ball, EditorTurnChange::modified), (paint, EditorTurnChange::deleted)];
    expected.sort_unstable_by_key(|(object_id, _)| *object_id);
    assert_eq!(listed, expected);
    Ok(())
}

// An object the AI made and took away again is not listed, even when someone acted in between.
#[tokio::test]
async fn an_object_the_ai_made_and_took_away_after_a_cut_is_not_listed() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    publish(&mut world);
    history.begin_chat_turn(&mut world);
    let fleeting = add_group(&mut world, "Fleeting", root);
    publish(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Other")?], None)?;
    world.removeObject(fleeting)?;
    publish(&mut world);

    assert_eq!(history.read_chat_turn_changes(&world), vec![]);
    assert_eq!(history.end_chat_turn(&mut world), vec![]);
    Ok(())
}

// A step keeps its field after a redo found the step's value already there. Tab B had set your blue, so your redo is
// passed over, and it replaced blue. Once tab C sets red, your undo gives that blue back.
#[tokio::test]
async fn a_step_keeps_its_field_after_a_redo_found_its_value_already_there() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let door = add_group(&mut world, "White", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Blue")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Door", vec![set_name(door, "Blue")?], None)?;
    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None);
    apply_and_record(&mut world, &mut history, "tab-c", "Name on Door", vec![set_name(door, "Red")?], None)?;

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Door"));
    assert_eq!(name(&world, door), "Blue");
    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None, "Blue is already there");
    assert_eq!(name(&world, door), "Blue");
    Ok(())
}

// A link a removal cleared comes back only while its field is still empty. You added a house, the AI pointed an
// animator at it, and you undid the add. The AI then pointed the animator at the tree, so your redo and the undo after
// it leave the tree.
#[tokio::test]
async fn a_redo_leaves_a_link_the_ai_pointed_elsewhere_since() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let tree = add_group(&mut world, "Tree", root);
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let animator = animator_pointing_at(&mut world, house);
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert_eq!(animator_target(&world, animator), Value::Null);
    history.begin_chat_turn(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(
        json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": tree }),
    )?;
    EditorSync::apply_client_delta(&mut world, repoint)?;
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert!(world.hasObject(house));
    assert_eq!(animator_target(&world, animator), json!(tree), "The redo leaves the AI's newer target");
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert_eq!(animator_target(&world, animator), json!(tree));
    Ok(())
}

// The AI deleted the house with your door in it and an animator pointing at it, and in the next reply pointed the
// animator at the tree. Undo brings back the house and leaves the tree, and so do the redo and the undo after it.
#[tokio::test]
async fn an_undo_leaves_a_link_the_ai_pointed_elsewhere_since() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let tree = add_group(&mut world, "Tree", root);
    let animator = animator_pointing_at(&mut world, house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    history.begin_chat_turn(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(
        json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": tree }),
    )?;
    EditorSync::apply_client_delta(&mut world, repoint)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house) && world.hasObject(door));
    assert_eq!(animator_target(&world, animator), json!(tree), "The undo leaves the AI's newer target");
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(!world.hasObject(house));
    assert_eq!(animator_target(&world, animator), json!(tree));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(animator_target(&world, animator), json!(tree));
    Ok(())
}

// Undoing the move of the folder holding a part writes back the part's place in its group. The AI moved your folder
// from First to Second and Second moved after that, so undo puts the part back at its place in First instead of
// keeping the place Second's move gave it.
#[tokio::test]
async fn undoing_a_folders_move_puts_its_part_back_in_its_place_after_the_new_group_moved() -> anyhow::Result<()> {
    for then in [
        "the AI moves Second",
        "the AI moves Second in the next reply",
        "a person moves Second during the reply",
        "a person moves Second after the reply",
    ] {
        let mut world = world()?;
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let second = add_rigid_group(&mut world, "Second", 10.0)?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let mut history = History::new(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Folder", vec![set_name(folder, "Mine")?], None)?;
        history.begin_chat_turn(&mut world);
        world.setParent(folder, second)?;
        match then {
            "the AI moves Second" => shift(&mut world, second, 3.0)?,
            "the AI moves Second in the next reply" => {
                history.end_chat_turn(&mut world);
                history.begin_chat_turn(&mut world);
                shift(&mut world, second, 3.0)?;
            }
            "a person moves Second during the reply" => {
                let move_second = move_by(&world, second, 3.0)?;
                apply_and_record(&mut world, &mut history, "tab-b", "Move Second", vec![move_second], None)?;
            }
            _ => {}
        }
        history.end_chat_turn(&mut world);
        if then == "a person moves Second after the reply" {
            let move_second = move_by(&world, second, 3.0)?;
            apply_and_record(&mut world, &mut history, "tab-b", "Move Second", vec![move_second], None)?;
        }
        assert!((world_x(&world, part) - 5.0).abs() < 1e-3, "{then}");

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{then}");
        assert_eq!(world.parentOfId.get(&folder), Some(&first), "{then}");
        assert!((relative_x(&world, part) - 2.0).abs() < 1e-3, "{then}");
        assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{then}");
        assert_eq!(world.parentOfId.get(&folder), Some(&second), "{then}");
        assert!((relative_x(&world, part) + 8.0).abs() < 1e-3, "{then}");
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{then}");
        assert!((relative_x(&world, part) - 2.0).abs() < 1e-3, "{then}");
    }
    Ok(())
}

// A part's place rewritten because its folder moved rides with the folder's change, and a folder the AI deleted has
// its change in that deletion, whether it was deleted on its own or inside its house. The AI took your folder out of
// First, moved First, put the part back in First and deleted the folder, so undo brings the folder back and puts the
// part back at its place in First. The part's own move is not yours and stays.
#[tokio::test]
async fn a_part_goes_back_to_its_place_when_the_deleted_folder_that_took_it_out_comes_back() -> anyhow::Result<()> {
    for is_in_house in [false, true] {
        let mut world = world()?;
        let root = world.getRootObjectId();
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let house = add_group(&mut world, "House", first);
        let folder = add_group(&mut world, "Folder", house);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let mut history = History::new(&mut world);
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Folder", vec![set_name(folder, "Mine")?], None)?;
        history.begin_chat_turn(&mut world);
        world.setParent(folder, root)?;
        shift(&mut world, first, 3.0)?;
        world.setParent(part, first)?;
        if is_in_house {
            world.removeObject(house)?;
        }
        world.removeObject(folder)?;
        history.end_chat_turn(&mut world);
        assert!((relative_x(&world, part) + 1.0).abs() < 1e-3, "{is_in_house}");

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_in_house}");
        assert_eq!(world.parentOfId.get(&folder), Some(&house), "{is_in_house}");
        assert_eq!(world.parentOfId.get(&part), Some(&first), "{is_in_house}");
        assert!((relative_x(&world, part) - 2.0).abs() < 1e-3, "{is_in_house}");
        assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_in_house}");
        assert!(!world.hasObject(folder), "{is_in_house}");
        assert!((relative_x(&world, part) + 1.0).abs() < 1e-3, "{is_in_house}");
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"), "{is_in_house}");
        assert!((relative_x(&world, part) - 2.0).abs() < 1e-3, "{is_in_house}");
    }
    Ok(())
}

// A parent that would put an object inside itself is left alone, the same as a parent that is gone. The AI moved your
// house into the shed, you undid that, and then the AI moved the shed into the house.
#[tokio::test]
async fn a_redo_never_puts_an_object_inside_itself() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let shed = add_group(&mut world, "Shed", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on House", vec![set_name(house, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(house, shed)?;
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    history.begin_chat_turn(&mut world);
    world.setParent(shed, house)?;
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None);
    assert_eq!(world.parentOfId.get(&house), Some(&root));
    assert_eq!(world.parentOfId.get(&shed), Some(&house));
    Ok(())
}

// An object comes back only under a parent that is there. You added a house to the street and undid that, and the AI
// deleted the street, so your redo has nothing it can bring back.
#[tokio::test]
async fn a_redo_never_brings_an_object_back_under_a_parent_that_is_gone() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let street = add_group(&mut world, "Street", root);
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| {
        house = add_group(world, "House", street)
    });
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    history.begin_chat_turn(&mut world);
    world.removeObject(street)?;
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None);
    assert!(!world.hasObject(house));
    Ok(())
}

// A number in a step that only equals the id of something gone is not a new link, so it still comes back while the
// object keeps an older link to that id.
#[tokio::test]
async fn a_number_equal_to_an_older_link_to_something_gone_still_comes_back() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let target = add_group(&mut world, "Target", root);
    let animator = animator_pointing_at(&mut world, target);
    let gone = (0..=u8::MAX as ObjectId).rev().find(|object_id| !world.hasObject(*object_id)).unwrap();
    let dangling: EditorClientDelta = serde_json::from_value(
        json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": gone }),
    )?;
    EditorSync::apply_client_delta(&mut world, dangling)?;
    let mut history = History::new(&mut world);
    let set_priority: EditorClientDelta =
        serde_json::from_value(json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "priority"], "value": gone }))?;
    apply_and_record(&mut world, &mut history, "tab-a", "Priority on Animator", vec![set_priority], None)?;
    let priority = |world: &World| serde_json::to_value(&world.getObject(animator).unwrap().info).unwrap()["PropertyAnimator"]["priority"].clone();

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(priority(&world), json!(0));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Priority on Animator"));
    assert_eq!(priority(&world), json!(gone));
    assert_eq!(animator_target(&world, animator), json!(gone));
    Ok(())
}

// A press removes objects last, so what its step moved out of them is out by then. The AI moved your door out of the
// house and deleted the house, and a redo after your undo does both again.
#[tokio::test]
async fn a_redo_moves_an_object_out_before_it_removes_the_group_it_left() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(door, root)?;
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(world.parentOfId.get(&door), Some(&house));

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(!world.hasObject(house));
    assert_eq!(world.parentOfId.get(&door), Some(&root));
    Ok(())
}

// A press brings back every removed group of its step before the links into them, so a link comes back even from an
// object that returns inside another group of the same step. Your step added a house to tab B's street and a shed. The
// house could not come back without the street, the AI put its animator of the house into the shed, and your undo
// took the shed away with it.
#[tokio::test]
async fn a_link_comes_back_from_an_object_in_another_group_of_the_step() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut street = 0;
    add_and_record(&mut world, &mut history, "tab-b", "Add Street", |world| {
        street = add_group(world, "Street", root)
    });
    let (mut house, mut shed) = (0, 0);
    add_and_record(&mut world, &mut history, "tab-a", "Add House and Shed", |world| {
        house = add_group(world, "House", street);
        shed = add_group(world, "Shed", root);
    });
    history.begin_chat_turn(&mut world);
    let animator = animator_pointing_at(&mut world, house);
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House and Shed"));
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Add Street"));
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House and Shed"));
    assert!(!world.hasObject(house) && world.hasObject(shed));
    history.begin_chat_turn(&mut world);
    world.setParent(animator, shed)?;
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House and Shed"));
    assert!(!world.hasObject(animator));
    assert_eq!(redo(&mut world, &mut history, "tab-b")?.as_deref(), Some("Add Street"));

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House and Shed"));
    assert!(world.hasObject(house) && world.hasObject(animator));
    assert_eq!(animator_target(&world, animator), json!(house));
    Ok(())
}

// A removed group counts once toward how much the log holds, however many sessions' steps share it. The AI deleted a
// house holding your door, tab B's window and a long script.
#[tokio::test]
async fn a_deleted_group_counts_once_toward_what_the_log_holds() -> anyhow::Result<()> {
    for (byte_limit, is_kept) in [(3 << 16, true), (1 << 16, false)] {
        let mut world = world()?;
        let root = world.getRootObjectId();
        let house = add_group(&mut world, "House", root);
        let door = add_group(&mut world, "Door", house);
        let window = add_group(&mut world, "Window", house);
        let script = world.addObject(
            "Script".into(),
            ObjectBeingAdded::Script(ScriptInfo {
                objectLocation: ObjectLocation::Server,
            }),
            house,
        );
        world.setScriptRawCode(script, "a".repeat(1 << 16));
        let mut history = History::new(&mut world);
        history.byte_limit = byte_limit;
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "Mine")?], None)?;
        apply_and_record(&mut world, &mut history, "tab-b", "Name on Window", vec![set_name(window, "Theirs")?], None)?;
        history.begin_chat_turn(&mut world);
        world.removeObject(house)?;
        history.end_chat_turn(&mut world);

        assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
        assert_eq!(undo(&mut world, &mut history, "tab-a")?.is_some(), is_kept, "{byte_limit}");
    }
    Ok(())
}

// A press ends a typing run, so what is typed after an undo and a redo is a step of its own.
#[tokio::test]
async fn a_typing_run_does_not_continue_across_an_undo_and_redo() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let typing = Some(EditorTypingTarget {
        object_id: ball,
        section: EditorSection::objectOfId,
        path: vec!["name".into()],
    });
    let mut history = History::new(&mut world);
    for name in ["R", "Ra"] {
        apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, name)?], typing.clone())?;
    }
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Ram")?], typing)?;

    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), "Ra");
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(name(&world, ball), original);
    Ok(())
}

// The end of a reply reads every object, so a change the World never marked is still the AI's.
#[tokio::test]
async fn a_change_the_world_never_marked_during_a_reply_is_the_ais() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.getObjectMut(ball)?.name = "Unmarked".into();
    let listed = history.end_chat_turn(&mut world);

    assert_eq!(
        listed
            .iter()
            .map(|object_change| (object_change.object_id, object_change.change))
            .collect::<Vec<_>>(),
        vec![(ball, EditorTurnChange::modified)]
    );
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(name(&world, ball), "Mine");
    Ok(())
}

// Undoing or redoing a folder's move names only the folder for selecting, not the parts that moved along with it.
#[tokio::test]
async fn a_press_on_a_folders_move_names_only_the_folder() -> anyhow::Result<()> {
    let mut world = world()?;
    let first = add_rigid_group(&mut world, "First", 0.0)?;
    let second = add_rigid_group(&mut world, "Second", 10.0)?;
    let folder = add_group(&mut world, "Folder", first);
    add_ball(&mut world, "Part", folder, 2.0)?;
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Folder", vec![set_name(folder, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.setParent(folder, second)?;
    history.end_chat_turn(&mut world);

    for direction in [EditorHistoryDirection::undo, EditorHistoryDirection::redo] {
        let step = history
            .take_step(&mut world, "tab-a", direction)?
            .expect("The folder's move can be taken back and forth");
        assert_eq!(step.object_ids, vec![folder], "{direction:?}");
    }
    Ok(())
}

// A reply that touched nothing of yours leaves you nothing new to undo, even when all you have is undone.
#[tokio::test]
async fn a_reply_that_touched_nothing_of_yours_leaves_nothing_to_undo() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let root = world.getRootObjectId();
    let other = add_group(&mut world, "Other", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(other, "AI-renamed")?)?;
    history.end_chat_turn(&mut world);

    let availability = history
        .read_availability()
        .get("tab-a")
        .map(|availability| (availability.can_undo, availability.can_redo));
    assert_eq!(availability, Some((false, true)));
    Ok(())
}

// An action that changes nothing is not a step, so it keeps what you had undone.
#[tokio::test]
async fn an_action_that_changes_nothing_keeps_what_was_undone() -> anyhow::Result<()> {
    let mut world = world()?;
    let ball = ball_id(&world);
    let original = name(&world, ball);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, "Mine")?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Ball", vec![set_name(ball, &original)?], None)?;

    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Name on Ball"));
    assert_eq!(name(&world, ball), "Mine");
    Ok(())
}

// A redo that could not write a value replaced nothing, so the undo after it still puts back the step's own value. You
// pointed the animator from First to Second and undid that, tab B pointed it at Third, and the AI deleted Second.
#[tokio::test]
async fn an_undo_after_a_redo_that_could_not_write_puts_back_the_steps_value() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let first = add_group(&mut world, "First", root);
    let second = add_group(&mut world, "Second", root);
    let third = add_group(&mut world, "Third", root);
    let animator = animator_pointing_at(&mut world, first);
    let point_at = |target: ObjectId| -> anyhow::Result<EditorClientDelta> {
        Ok(serde_json::from_value(
            json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": target }),
        )?)
    };
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Target on Animator", vec![point_at(second)?], None)?;
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    apply_and_record(&mut world, &mut history, "tab-b", "Target on Animator", vec![point_at(third)?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(second)?;
    history.end_chat_turn(&mut world);

    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None);
    assert_eq!(animator_target(&world, animator), json!(third));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Target on Animator"));
    assert_eq!(animator_target(&world, animator), json!(first));
    Ok(())
}

// A link the AI pointed somewhere else while it deleted the old target is a change of the linking object's own, so undo
// points it back.
#[tokio::test]
async fn undo_points_back_a_link_the_ai_moved_off_what_it_deleted() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let tree = add_group(&mut world, "Tree", root);
    let animator = animator_pointing_at(&mut world, house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Animator", vec![set_name(animator, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    let repoint: EditorClientDelta = serde_json::from_value(
        json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": tree }),
    )?;
    EditorSync::apply_client_delta(&mut world, repoint)?;
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);

    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house));
    assert_eq!(animator_target(&world, animator), json!(house));
    Ok(())
}

// Links into a group come back only with the group, so a redo that finds its object already there leaves a link someone
// cleared since alone. The AI's animator pointed at the house you added, tab B's undo brought the house back after you
// undid adding it, and then the AI cleared the animator.
#[tokio::test]
async fn a_redo_that_finds_its_object_there_leaves_the_links_into_it_alone() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let mut history = History::new(&mut world);
    let mut house = 0;
    add_and_record(&mut world, &mut history, "tab-a", "Add House", |world| house = add_group(world, "House", root));
    history.begin_chat_turn(&mut world);
    let animator = animator_pointing_at(&mut world, house);
    history.end_chat_turn(&mut world);
    apply_and_record(&mut world, &mut history, "tab-b", "Name on House", vec![set_name(house, "Theirs")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("Add House"));
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert_eq!(animator_target(&world, animator), json!(house));
    history.begin_chat_turn(&mut world);
    let clear: EditorClientDelta = serde_json::from_value(
        json!({ "objectId": animator, "section": "objectOfId", "path": ["info", "PropertyAnimator", "targetObjectId"], "value": null }),
    )?;
    EditorSync::apply_client_delta(&mut world, clear)?;
    history.end_chat_turn(&mut world);

    assert_eq!(
        redo(&mut world, &mut history, "tab-a")?.as_deref(),
        Some("the AI's changes"),
        "The add is passed over, since the house is already there"
    );
    assert!(!world.hasObject(house));
    Ok(())
}

// Only a redo of the running reply's own step counts as the AI taking something away. Redoing an earlier reply's
// deletion while a reply runs is the person's, so this reply does not list it as deleted.
#[tokio::test]
async fn a_redo_of_an_earlier_replys_deletion_is_not_listed_as_deleted() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on House", vec![set_name(house, "Mine")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    history.end_chat_turn(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    history.begin_chat_turn(&mut world);
    EditorSync::apply_client_delta(&mut world, set_name(house, "AI-house")?)?;
    assert_eq!(redo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(!world.hasObject(house));

    assert_eq!(history.read_chat_turn_changes(&world), Vec::new());
    assert_eq!(history.end_chat_turn(&mut world), Vec::new());
    Ok(())
}

// An undo of the running reply's own step is the person's. Your door is in the house and tab B named it too. The AI
// deleted the house, you undid that, and the AI deleted it again, so your redo found it gone. Tab B's undo brought the
// house back and yours took it away again, so the reply does not list it as deleted.
#[tokio::test]
async fn an_undo_of_the_running_replys_own_step_is_not_listed_as_deleted() -> anyhow::Result<()> {
    let mut world = world()?;
    let root = world.getRootObjectId();
    let house = add_group(&mut world, "House", root);
    let door = add_group(&mut world, "Door", house);
    let mut history = History::new(&mut world);
    apply_and_record(&mut world, &mut history, "tab-a", "Name on Door", vec![set_name(door, "A-door")?], None)?;
    apply_and_record(&mut world, &mut history, "tab-b", "Name on Door", vec![set_name(door, "B-door")?], None)?;
    history.begin_chat_turn(&mut world);
    world.removeObject(house)?;
    publish(&mut world);
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house));
    world.removeObject(house)?;
    publish(&mut world);
    assert_eq!(redo(&mut world, &mut history, "tab-a")?, None, "The house is already gone");
    assert_eq!(undo(&mut world, &mut history, "tab-b")?.as_deref(), Some("the AI's changes"));
    assert!(world.hasObject(house));
    assert_eq!(undo(&mut world, &mut history, "tab-a")?.as_deref(), Some("the AI's changes"));
    assert!(!world.hasObject(house));

    assert_eq!(history.read_chat_turn_changes(&world), Vec::new());
    assert_eq!(history.end_chat_turn(&mut world), Vec::new());
    Ok(())
}

// Where a person's action cuts a reply does not change what the reply lists. The AI renamed a part, moved its folder to
// another group and named the part back, and tab B's edit cut the reply after the rename or after the move.
#[tokio::test]
async fn where_a_reply_is_cut_does_not_change_what_it_lists() -> anyhow::Result<()> {
    let mut is_part_listed = Vec::new();
    for is_cut_after_the_move in [false, true] {
        let mut world = world()?;
        let ball = ball_id(&world);
        let first = add_rigid_group(&mut world, "First", 0.0)?;
        let second = add_rigid_group(&mut world, "Second", 10.0)?;
        let folder = add_group(&mut world, "Folder", first);
        let part = add_ball(&mut world, "Part", folder, 2.0)?;
        let original = name(&world, part);
        let mut history = History::new(&mut world);
        history.begin_chat_turn(&mut world);
        EditorSync::apply_client_delta(&mut world, set_name(part, "AI-part")?)?;
        if !is_cut_after_the_move {
            apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Theirs")?], None)?;
        }
        world.setParent(folder, second)?;
        if is_cut_after_the_move {
            apply_and_record(&mut world, &mut history, "tab-b", "Name on Ball", vec![set_name(ball, "Theirs")?], None)?;
        }
        EditorSync::apply_client_delta(&mut world, set_name(part, &original)?)?;
        is_part_listed.push(history.end_chat_turn(&mut world).iter().any(|object_change| object_change.object_id == part));
    }
    assert_eq!(is_part_listed[0], is_part_listed[1], "The part is listed the same after either cut");
    Ok(())
}

// Undo and redo put back and take away animations and their keyframes through the same setters as any other change of many
// objects, so every animation's cached keyframes still have to match its objects afterwards, or the editor and playtests would
// play poses that are gone.
#[tokio::test]
async fn undo_and_redo_leave_every_animation_with_current_keyframes() -> anyhow::Result<()> {
    // Every animation's cached keyframes must match a rebuild from its objects as they are now. Returns how many were checked.
    fn check_keyframe_caches_are_current(world: &World) -> anyhow::Result<usize> {
        let children = |id| world._childrenOfId.get(&id).into_iter().flatten().copied().collect();
        let mut checked_count = 0;
        for (id, object) in &world.objectOfId {
            let (cached, rebuilt) = match &object.info {
                ObjectInfo::SkAnimation(_) => (
                    serde_json::to_value(world.renderSkKeyframesOfSkAnimationId.get(id))?,
                    serde_json::to_value(Some(collect_sk_keyframes(&world.objectOfId, &children, *id)))?,
                ),
                ObjectInfo::PropertyAnimation(_) => (
                    serde_json::to_value(
                        world
                            .propertyKeyframesArcOfPropertyAnimationId
                            .get(id)
                            .map(|keyframes_arc| keyframes_arc.as_ref()),
                    )?,
                    serde_json::to_value(Some(collect_property_keyframes(&world.objectOfId, &children, *id)))?,
                ),
                _ => continue,
            };
            assert_eq!(cached, rebuilt, "Animation {id} ({}) has stale keyframes", object.name);
            checked_count += 1;
        }
        Ok(checked_count)
    }

    let mut world = world()?;
    let root = world.getRootObjectId();
    let bob = world.addObject(
        "Bob".into(),
        ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
            duration: 1.0,
            interpolationStyle: InterpolationStyle::Linear,
        }),
        root,
    );
    for index in 0..3 {
        world.addObject(
            format!("Bob key {index}"),
            ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo {
                time: index as f32,
                value: index as f32,
            }),
            bob,
        );
    }
    let animation_ids: Vec<ObjectId> = world
        .objectOfId
        .values()
        .filter_map(|object| match object.info {
            ObjectInfo::SkAnimation(_) | ObjectInfo::PropertyAnimation(_) => Some(object.id),
            _ => None,
        })
        .collect();
    assert_eq!(check_keyframe_caches_are_current(&world)?, 5);
    let mut history = History::new(&mut world);

    // A keyframe's time changes in place, and undo moves it back.
    let keyframe = world
        .objectOfId
        .values()
        .find_map(|object| match object.info {
            ObjectInfo::SkKeyframe(_) => Some(object.id),
            _ => None,
        })
        .unwrap();
    let set_time = json!({ "objectId": keyframe, "section": "objectOfId", "path": ["info", "SkKeyframe", "time"], "value": 0.9 });
    apply_and_record(&mut world, &mut history, "tab-a", "Time on Key", vec![serde_json::from_value(set_time)?], None)?;
    assert_eq!(check_keyframe_caches_are_current(&world)?, 5);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(check_keyframe_caches_are_current(&world)?, 5);

    // Deleting every animation, keyframes and all, is one step that undo puts back and redo takes away again.
    history.start_person_action(&mut world);
    for animation_id in &animation_ids {
        world.removeObject(*animation_id)?;
    }
    history.record_person_action(&mut world, "tab-a".into(), "Delete animations".into(), None);
    history.check_marked(&world);
    assert_eq!(check_keyframe_caches_are_current(&world)?, 0);
    assert!(undo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(check_keyframe_caches_are_current(&world)?, 5);
    assert!(redo(&mut world, &mut history, "tab-a")?.is_some());
    assert_eq!(check_keyframe_caches_are_current(&world)?, 0);
    Ok(())
}
