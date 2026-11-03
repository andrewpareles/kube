mod helpers;

use deno_core::JsRuntime;
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use serde::de::DeserializeOwned;
use shared::game::gui_types::GuiSerializedNode;
use shared::game::jsRuntime::{JsRuntimeInitialization, ScriptManager, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime};
use shared::game::world::World;
use std::{cell::RefCell, rc::Rc};

const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;
const SCRIPT_NAME: &str = "create_state_script";
const SCRIPT_PATH: &str = "root/create_state_script";

// These tests use the real player runtime so createState subscriptions, rerenders, and cleanup all run through the same path the game uses.
fn add_named_client_script(world: &mut World, object_id: ObjectId, parent_id: ObjectId, name: &str, raw_code: &str) {
    world.addObjectWithId(
        object_id,
        name.to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        parent_id,
    );
    world.markScriptAsReadyToRun(object_id, raw_code.to_string());
}

// This keeps each test focused on the state behavior instead of repeating the same runtime startup.
async fn initialize_player_runtime_with_client_script(raw_code: &str) -> anyhow::Result<(World, JsRuntime, Rc<RefCell<ScriptManager>>)> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    add_named_client_script(&mut world, SCRIPT_OBJECT_ID, root_object_id, SCRIPT_NAME, raw_code);

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;

    Ok((world, js_runtime, script_manager_rc))
}

// This goes through the exported client-function registry so the test hits the same function-calling path as production.
async fn call_registered_client_fn<ResultValue: DeserializeOwned>(
    js_runtime: &mut JsRuntime,
    world: &mut World,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    export_name: &str,
    js_args: &str,
) -> anyhow::Result<ResultValue> {
    let fn_id = format!("{export_name}#{SCRIPT_PATH}");
    let client_js_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(client_js_fn) = script_manager.get_client_js_fn(fn_id.as_str()) else {
            return Err(anyhow::anyhow!("Client function not found: {}", fn_id));
        };
        client_js_fn.clone()
    };

    let result_string = callJsFn(js_runtime, world, 0.0, Some(0.0), &client_js_fn, &format!(r#"{{"json":{js_args}}}"#)).await?;
    helpers::remote_json_result(&result_string)
}

fn collect_gui_texts(children: &[GuiSerializedNode], text_values: &mut Vec<String>) {
    for child in children {
        match child {
            GuiSerializedNode::Element(element_node) => {
                collect_gui_texts(&element_node.children, text_values);
            }
            GuiSerializedNode::Text(text_node) => {
                text_values.push(text_node.text.clone());
            }
        }
    }
}

fn gui_texts(script_manager_rc: &Rc<RefCell<ScriptManager>>, script_object_id: ObjectId, gui_key: &str) -> anyhow::Result<Vec<String>> {
    let script_manager = script_manager_rc.borrow();
    let Some(gui_root) = script_manager
        .gui_root_of_script_gui_key()
        .iter()
        .find(|((next_script_object_id, next_gui_key), _)| *next_script_object_id == script_object_id && next_gui_key == gui_key)
        .map(|(_, gui_root)| gui_root)
    else {
        return Err(anyhow::anyhow!("GUI root not found for script {} gui {}", script_object_id, gui_key));
    };

    let mut text_values = Vec::new();
    collect_gui_texts(&gui_root.children, &mut text_values);
    Ok(text_values)
}

fn has_gui_root(script_manager_rc: &Rc<RefCell<ScriptManager>>, script_object_id: ObjectId, gui_key: &str) -> bool {
    let script_manager = script_manager_rc.borrow();
    script_manager
        .gui_root_of_script_gui_key()
        .keys()
        .any(|(next_script_object_id, next_gui_key)| *next_script_object_id == script_object_id && next_gui_key == gui_key)
}

#[tokio::test]
async fn create_state_set_rerenders_every_mounted_root_that_read_it() -> anyhow::Result<()> {
    let (mut world, mut js_runtime, script_manager_rc) = initialize_player_runtime_with_client_script(
        r#"'use client'
const scoreState = createState(0);
let alphaRenderCount = 0;
let betaRenderCount = 0;
let gammaRenderCount = 0;

const Alpha = () => {
    alphaRenderCount += 1;
    return <div>{`alpha_${scoreState.get()}`}</div>;
};

const Beta = () => {
    betaRenderCount += 1;
    return <div>{`beta_${scoreState.get()}`}</div>;
};

const Gamma = () => {
    gammaRenderCount += 1;
    return <div>gamma_static</div>;
};

game.guiService.show('alpha', Alpha);
game.guiService.show('beta', Beta);
game.guiService.show('gamma', Gamma);

export const getRenderCounts = () => [alphaRenderCount, betaRenderCount, gammaRenderCount];
export const setScore = (nextScore) => {
    scoreState.set(nextScore);
    return [alphaRenderCount, betaRenderCount, gammaRenderCount];
};
"#,
    )
    .await?;

    let initial_render_counts: [u64; 3] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "getRenderCounts", "[]").await?;
    assert_eq!(initial_render_counts, [1, 1, 1]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "alpha")?, vec!["alpha_0".to_string()]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "beta")?, vec!["beta_0".to_string()]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "gamma")?, vec!["gamma_static".to_string()]);

    let render_counts_after_score_change: [u64; 3] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "setScore", "[1]").await?;
    assert_eq!(render_counts_after_score_change, [2, 2, 1]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "alpha")?, vec!["alpha_1".to_string()]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "beta")?, vec!["beta_1".to_string()]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "gamma")?, vec!["gamma_static".to_string()]);

    Ok(())
}

#[tokio::test]
async fn create_state_unsubscribes_root_when_its_next_render_stops_reading_that_state() -> anyhow::Result<()> {
    let (mut world, mut js_runtime, script_manager_rc) = initialize_player_runtime_with_client_script(
        r#"'use client'
const scoreState = createState(0);
const shouldReadScoreState = createState(true);
let alwaysRenderCount = 0;
let conditionalRenderCount = 0;

const Always = () => {
    alwaysRenderCount += 1;
    return <div>{`always_${scoreState.get()}`}</div>;
};

const Conditional = () => {
    conditionalRenderCount += 1;
    const shouldReadScore = shouldReadScoreState.get();
    if (!shouldReadScore) {
        return <div>conditional_off</div>;
    }
    return <div>{`conditional_${scoreState.get()}`}</div>;
};

game.guiService.show('always', Always);
game.guiService.show('conditional', Conditional);

export const getRenderCounts = () => [alwaysRenderCount, conditionalRenderCount];
export const setShouldReadScore = (shouldReadScore) => {
    shouldReadScoreState.set(shouldReadScore);
    return [alwaysRenderCount, conditionalRenderCount];
};
export const setScore = (nextScore) => {
    scoreState.set(nextScore);
    return [alwaysRenderCount, conditionalRenderCount];
};
"#,
    )
    .await?;

    let initial_render_counts: [u64; 2] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "getRenderCounts", "[]").await?;
    assert_eq!(initial_render_counts, [1, 1]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "always")?, vec!["always_0".to_string()]);
    assert_eq!(
        gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "conditional")?,
        vec!["conditional_0".to_string()]
    );

    let render_counts_after_unsubscribe: [u64; 2] =
        call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "setShouldReadScore", "[false]").await?;
    assert_eq!(render_counts_after_unsubscribe, [1, 2]);
    assert_eq!(
        gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "conditional")?,
        vec!["conditional_off".to_string()]
    );

    let render_counts_after_score_change: [u64; 2] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "setScore", "[1]").await?;
    assert_eq!(render_counts_after_score_change, [2, 2]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "always")?, vec!["always_1".to_string()]);
    assert_eq!(
        gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "conditional")?,
        vec!["conditional_off".to_string()]
    );

    Ok(())
}

#[tokio::test]
async fn create_state_cleans_up_subscriptions_when_a_root_is_hidden() -> anyhow::Result<()> {
    let (mut world, mut js_runtime, script_manager_rc) = initialize_player_runtime_with_client_script(
        r#"'use client'
const scoreState = createState(0);
let visibleRenderCount = 0;
let observerRenderCount = 0;

const Visible = () => {
    visibleRenderCount += 1;
    return <div>{`visible_${scoreState.get()}`}</div>;
};

const Observer = () => {
    observerRenderCount += 1;
    return <div>{`observer_${scoreState.get()}`}</div>;
};

game.guiService.show('visible', Visible);
game.guiService.show('observer', Observer);

export const getRenderCounts = () => [visibleRenderCount, observerRenderCount];
export const hideVisible = () => {
    game.guiService.hide('visible');
    return [visibleRenderCount, observerRenderCount];
};
export const setScore = (nextScore) => {
    scoreState.set(nextScore);
    return [visibleRenderCount, observerRenderCount];
};
"#,
    )
    .await?;

    let initial_render_counts: [u64; 2] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "getRenderCounts", "[]").await?;
    assert_eq!(initial_render_counts, [1, 1]);
    assert!(has_gui_root(&script_manager_rc, SCRIPT_OBJECT_ID, "visible"));
    assert!(has_gui_root(&script_manager_rc, SCRIPT_OBJECT_ID, "observer"));

    let render_counts_after_hide: [u64; 2] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "hideVisible", "[]").await?;
    assert_eq!(render_counts_after_hide, [1, 1]);
    assert!(!has_gui_root(&script_manager_rc, SCRIPT_OBJECT_ID, "visible"));
    assert!(has_gui_root(&script_manager_rc, SCRIPT_OBJECT_ID, "observer"));

    let render_counts_after_score_change: [u64; 2] = call_registered_client_fn(&mut js_runtime, &mut world, &script_manager_rc, "setScore", "[1]").await?;
    assert_eq!(render_counts_after_score_change, [1, 2]);
    assert_eq!(gui_texts(&script_manager_rc, SCRIPT_OBJECT_ID, "observer")?, vec!["observer_1".to_string()]);

    Ok(())
}
