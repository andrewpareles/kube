mod helpers;

use helpers::world_test_helpers::{add_script, make_world};
use shared::game::jsRuntime::{JsConsoleLogCallback, JsConsoleLogEntry, JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime};
use std::sync::{Arc, Mutex};

async fn run_and_collect_console(world: &mut shared::game::world::World) -> anyhow::Result<Vec<String>> {
    let console_log_entries: Arc<Mutex<Vec<JsConsoleLogEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let console_log_entries_for_callback = console_log_entries.clone();
    let console_log_callback: JsConsoleLogCallback = Arc::new(move |entry: JsConsoleLogEntry| {
        console_log_entries_for_callback.lock().unwrap().push(entry);
    });
    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, Some(console_log_callback)).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, world, 0.0, Some(0.0), &script_manager_rc).await?;
    let entries = console_log_entries.lock().unwrap().clone();
    Ok(entries.iter().map(|entry| entry.message.trim().to_string()).collect())
}

// A script written before the PropertyAnimation refactor creates its animators with the curve
// inline. This pins down what such a script experiences now, because 45 published games are
// written that way and every one of them stopped animating.
#[tokio::test]
async fn creating_an_animator_with_the_old_inline_curve_throws_and_names_the_missing_field() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_script(&mut world, 2, root_object_id)?;
    // addObjectWithId does not move the id counter, so without this the first object the script
    // creates is handed the script's own id.
    world.objectIdCounter = world.objectOfId.keys().copied().max().unwrap_or(root_object_id);
    world.markScriptAsReadyToRun(
        2,
        r#"
let outcome = 'no outcome';
try {
    game.objectService.createObject('old_shape_animator', {
        type: 'propertyAnimator',
        targetObjectId: null,
        propertyPath: ['location', 'position', 'y'],
        animation: { duration: 1, interpolationStyle: 'Linear', keyframes: [{ time: 0, value: 0 }, { time: 1, value: 5 }] },
        clockId: null, priority: 5, weight: 100, weightTransition: null,
    }, game.world.id);
    outcome = 'created';
} catch (error) {
    outcome = 'threw:' + error.message;
}
console.log('[old-shape] ' + outcome);
"#
        .to_string(),
    );

    let messages = run_and_collect_console(&mut world).await?;
    assert!(
        messages.iter().any(|m| m.contains("threw:") && m.contains("missing field propertyAnimationId")),
        "expected the old inline shape to be refused by name, got: {messages:?}"
    );
    Ok(())
}

// The conversion applied to the published games: the caller keeps passing one object literal with
// the curve inline, and a helper turns it into a PropertyAnimation with PropertyKeyframe children.
// This is the exact helper source that gets injected, so if it stops working the games do too.
#[tokio::test]
async fn the_injected_helper_builds_a_working_animator_from_an_inline_curve() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_object_id = world.getRootObjectId();
    add_script(&mut world, 2, root_object_id)?;
    // addObjectWithId does not move the id counter, so without this the first object the script
    // creates is handed the script's own id.
    world.objectIdCounter = world.objectOfId.keys().copied().max().unwrap_or(root_object_id);
    world.markScriptAsReadyToRun(
        2,
        r#"
function createAnimatorWithCurve(animatorName: string, animatorSpec: any, parentId: number) {
  const curve = animatorSpec.animation;
  const animation = game.objectService.createObject(animatorName + '__curve', {
    type: 'propertyAnimation',
    duration: curve.duration,
    interpolationStyle: curve.interpolationStyle,
  }, parentId);
  if (animation === null) return null;
  for (let keyframeIndex = 0; keyframeIndex < curve.keyframes.length; keyframeIndex++) {
    const keyframe = curve.keyframes[keyframeIndex];
    game.objectService.createObject(animatorName + '__kf' + keyframeIndex, {
      type: 'propertyKeyframe', time: keyframe.time, value: keyframe.value,
    }, animation.id);
  }
  const withAnimationId: any = {};
  for (const field of Object.keys(animatorSpec)) {
    if (field !== 'animation') withAnimationId[field] = animatorSpec[field];
  }
  withAnimationId.propertyAnimationId = animation.id;
  return game.objectService.createObject(animatorName, withAnimationId, parentId);
}

const made = createAnimatorWithCurve('wall_slide', {
    type: 'propertyAnimator',
    targetObjectId: null,
    propertyPath: ['location', 'position', 'y'],
    animation: { duration: 2, interpolationStyle: 'Linear', keyframes: [{ time: 0, value: 0 }, { time: 1, value: 5 }, { time: 2, value: 0 }] },
    clockId: null, priority: 5, weight: 100, weightTransition: null,
}, game.world.id);

console.log('[converted] animator=' + (made === null ? 'null' : made.type + ' id=' + made.id + ' paId=' + made.propertyAnimationId));
const animationId = made === null ? null : made.propertyAnimationId;
const curve = animationId === null ? null : game.objectService.getObjectById(animationId);
console.log('[converted] curve=' + (curve === null ? 'null' : curve.type + ':' + curve.duration + ':' + curve.interpolationStyle));
// Keyframe children come back in no particular order; each one carries its own time, so sort
// by time before reporting to make the comparison stable.
const keyframes = curve === null ? [] : curve.getChildren().slice().sort((a, b) => a.time - b.time);
console.log('[converted] keyframes=' + keyframes.length + ' values=' + keyframes.map((k) => k.time + '@' + k.value).join(','));
"#
        .to_string(),
    );

    let messages = run_and_collect_console(&mut world).await?;
    println!("console: {messages:?}");
    let joined = messages.join(" | ");
    assert!(joined.contains("[converted] animator=propertyAnimator"), "animator was not created: {joined}");
    assert!(
        joined.contains("curve=propertyAnimation:2:"),
        "PropertyAnimation was not created with its duration: {joined}"
    );
    assert!(joined.contains("keyframes=3"), "expected 3 PropertyKeyframe children: {joined}");
    assert!(joined.contains("values=0@0,1@5,2@0"), "keyframe times/values did not survive: {joined}");
    Ok(())
}
