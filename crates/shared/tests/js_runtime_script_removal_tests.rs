mod helpers;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deno_core::JsRuntime;
use game_types::object_types::{LocalGroupInfo, ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::{make_world, make_world_controlled_by_player};
use shared::game::jsRuntime::{
    JsConsoleLogCallback, JsConsoleLogEntry, JsRuntimeInitialization, ScriptManager, callJsFn, initializeJsAndRunInitialScripts, initializeJsRuntime,
    runPendingJS,
};
use shared::game::world::{JsCallbackInputs, World, WorldControlMode};
use tokio::sync::mpsc;

const PLAYER_OBJECT_ID: ObjectId = 99;

// A world with a running JS runtime. Scripts log with console.log, and the tests read those lines.
struct TestRuntime {
    world: World,
    js_runtime: JsRuntime,
    script_manager_rc: Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: mpsc::UnboundedReceiver<JsCallbackInputs>,
    logs_arc: Arc<Mutex<Vec<String>>>,
    now_ms: f64,
}

impl TestRuntime {
    async fn new(mut world: World) -> anyhow::Result<Self> {
        let logs_arc = Arc::new(Mutex::new(Vec::new()));
        let console_log_callback: JsConsoleLogCallback = {
            let logs_arc = logs_arc.clone();
            Arc::new(move |entry: JsConsoleLogEntry| logs_arc.lock().unwrap().push(entry.message.trim().to_string()))
        };
        let (js_callback_inputs_tx, js_callback_inputs_rx) = mpsc::unbounded_channel();
        world.js_callback_inputs_tx = Some(js_callback_inputs_tx);
        let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, Some(console_log_callback)).await?;
        initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
        Ok(Self {
            world,
            js_runtime,
            script_manager_rc,
            js_callback_inputs_rx,
            logs_arc,
            now_ms: 0.0,
        })
    }

    // Server scripts on the server, client scripts on a client.
    fn add_script(&mut self, name: &str, parent_id: ObjectId, code: &str) -> ObjectId {
        let object_location = match self.world.worldControlMode {
            WorldControlMode::Player(_) => ObjectLocation::Client,
            WorldControlMode::Server | WorldControlMode::Editor => ObjectLocation::Server,
        };
        let script_object_id = self.world.addObject(
            name.to_string(),
            ObjectBeingAdded::Script(ScriptInfo {
                objectLocation: object_location,
            }),
            parent_id,
        );
        self.world.markScriptAsReadyToRun(script_object_id, code.to_string());
        script_object_id
    }

    // Adding an object also fires every object-created listener.
    fn add_folder(&mut self, name: &str, parent_id: ObjectId) -> ObjectId {
        self.world
            .addObject(name.to_string(), ObjectBeingAdded::LocalGroup(LocalGroupInfo {}), parent_id)
    }

    // Waits first, so timers that come due in the meantime fire during the tick.
    async fn run_tick(&mut self, wait_ms: u64) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        self.now_ms += 16.0;
        runPendingJS(
            &mut self.js_runtime,
            &mut self.world,
            self.now_ms,
            Some(self.now_ms),
            &self.script_manager_rc,
            &mut self.js_callback_inputs_rx,
        )
        .await
    }

    fn take_logs(&self) -> Vec<String> {
        std::mem::take(&mut *self.logs_arc.lock().unwrap())
    }
}

const SCRIPT_THAT_KEEPS_RUNNING: &str = r#"
game.objectService.addObjectCreatedListener(() => console.log("listener"));
setInterval(() => console.log("interval"), 1);
setTimeout(() => console.log("timeout"), 60);
while (true) {
    console.log("tick");
    await tick();
}
"#;

async fn assert_removing_a_script_stops_everything_it_started(world: World) -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(world).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let script_object_id = test_runtime.add_script("keepsRunning", root_object_id, SCRIPT_THAT_KEEPS_RUNNING);
    test_runtime.run_tick(0).await?;
    test_runtime.add_folder("trigger", root_object_id);
    test_runtime.run_tick(5).await?;
    let logs_before_removal = test_runtime.take_logs();
    for expected_log in ["tick", "listener", "interval"] {
        assert!(
            logs_before_removal.iter().any(|log| log == expected_log),
            "the script should have logged {expected_log} before it was removed, got {logs_before_removal:?}"
        );
    }

    // The timeout is still pending when the script goes, and comes due during the ticks below.
    test_runtime.world.removeObject(script_object_id)?;
    for _ in 0..3 {
        test_runtime.add_folder("trigger", root_object_id);
        test_runtime.run_tick(30).await?;
    }
    assert_eq!(test_runtime.take_logs(), Vec::<String>::new(), "a removed script must not run again");
    Ok(())
}

#[tokio::test]
async fn removing_a_script_on_the_server_stops_its_tick_loops_listeners_and_timers() -> anyhow::Result<()> {
    assert_removing_a_script_stops_everything_it_started(make_world()?).await
}

#[tokio::test]
async fn removing_a_script_on_a_client_stops_its_tick_loops_listeners_and_timers() -> anyhow::Result<()> {
    assert_removing_a_script_stops_everything_it_started(make_world_controlled_by_player(PLAYER_OBJECT_ID)?).await
}

#[tokio::test]
async fn removing_a_folder_stops_the_scripts_inside_it() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let folder_object_id = test_runtime.add_folder("folder", root_object_id);
    test_runtime.add_script("keepsRunning", folder_object_id, SCRIPT_THAT_KEEPS_RUNNING);
    test_runtime.run_tick(0).await?;
    assert!(test_runtime.take_logs().contains(&"tick".to_string()));

    test_runtime.world.removeObject(folder_object_id)?;
    for _ in 0..3 {
        test_runtime.add_folder("trigger", root_object_id);
        test_runtime.run_tick(30).await?;
    }
    assert_eq!(test_runtime.take_logs(), Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn removing_a_script_on_a_client_removes_its_gui() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world_controlled_by_player(PLAYER_OBJECT_ID)?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let script_object_id = test_runtime.add_script(
        "hud",
        root_object_id,
        "'use client'\ngame.guiService.show('score', () => <div>0 points</div>);\n",
    );
    test_runtime.run_tick(0).await?;
    let gui_key = (script_object_id, "score".to_string());
    assert!(test_runtime.script_manager_rc.borrow().gui_root_of_script_gui_key().contains_key(&gui_key));

    test_runtime.world.removeObject(script_object_id)?;
    test_runtime.run_tick(0).await?;
    assert!(!test_runtime.script_manager_rc.borrow().gui_root_of_script_gui_key().contains_key(&gui_key));
    Ok(())
}

#[tokio::test]
async fn a_removed_scripts_server_and_client_functions_can_no_longer_be_called() -> anyhow::Result<()> {
    async fn call_ping(test_runtime: &mut TestRuntime, is_server: bool) -> anyhow::Result<String> {
        let ping_fn = {
            let script_manager = test_runtime.script_manager_rc.borrow();
            let ping_fn = match is_server {
                true => script_manager.get_server_js_fn("ping#root/remote"),
                false => script_manager.get_client_js_fn("ping#root/remote"),
            };
            ping_fn.ok_or_else(|| anyhow::anyhow!("ping was not registered"))?.clone()
        };
        let now_ms = test_runtime.now_ms;
        callJsFn(
            &mut test_runtime.js_runtime,
            &mut test_runtime.world,
            now_ms,
            Some(now_ms),
            &ping_fn,
            r#"{"json":[]}"#,
        )
        .await
    }

    for world in [make_world()?, make_world_controlled_by_player(PLAYER_OBJECT_ID)?] {
        let is_server = match world.worldControlMode {
            WorldControlMode::Server => true,
            WorldControlMode::Editor | WorldControlMode::Player(_) => false,
        };
        let mut test_runtime = TestRuntime::new(world).await?;
        let root_object_id = test_runtime.world.getRootObjectId();
        let directive = if is_server { "'use server'" } else { "'use client'" };
        let script_object_id = test_runtime.add_script("remote", root_object_id, &format!("{directive}\nexport const ping = async () => 'pong';\n"));
        test_runtime.run_tick(0).await?;
        let pong: String = helpers::remote_json_result(&call_ping(&mut test_runtime, is_server).await?)?;
        assert_eq!(pong, "pong");

        test_runtime.world.removeObject(script_object_id)?;
        let Err(error) = call_ping(&mut test_runtime, is_server).await else {
            return Err(anyhow::anyhow!("{directive} ping still ran after its script was removed"));
        };
        assert!(error.to_string().contains("This function's script was deleted."), "{directive}: {error}");
    }
    Ok(())
}

// A listener belongs to the script it was written in, even when another script registered it.
#[tokio::test]
async fn a_listener_keeps_working_after_the_script_that_registered_it_is_removed() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let helper_object_id = test_runtime.add_script(
        "helper",
        root_object_id,
        "export const listenForCreates = (callback) => game.objectService.addObjectCreatedListener(callback);\n",
    );
    test_runtime.add_script(
        "user",
        root_object_id,
        "import { listenForCreates } from './helper';\nlistenForCreates(() => console.log('user heard'));\n",
    );
    test_runtime.run_tick(0).await?;

    test_runtime.world.removeObject(helper_object_id)?;
    test_runtime.add_folder("trigger", root_object_id);
    test_runtime.run_tick(0).await?;
    assert_eq!(test_runtime.take_logs(), vec!["user heard"]);
    Ok(())
}

// Every script used to run as the module at its path, so a script at a path that had already run never ran.
#[tokio::test]
async fn a_new_script_at_the_path_of_a_removed_one_runs() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let script_object_id = test_runtime.add_script("greeter", root_object_id, "console.log('first greeter ran');");
    test_runtime.run_tick(0).await?;
    test_runtime.world.removeObject(script_object_id)?;
    test_runtime.add_script("greeter", root_object_id, "console.log('second greeter ran');");
    test_runtime.run_tick(0).await?;
    assert_eq!(test_runtime.take_logs(), vec!["first greeter ran", "second greeter ran"]);
    Ok(())
}

// A pasted copy sits at its original's path. Its scripts run too, and an import picks the copy's own sibling.
#[tokio::test]
async fn a_pasted_copy_runs_its_scripts_and_imports_its_own_siblings() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let enemy_object_id = test_runtime.add_folder("enemy", root_object_id);
    test_runtime.add_script("helper", enemy_object_id, "export const helperScriptId = script.id;\n");
    test_runtime.add_script(
        "brain",
        enemy_object_id,
        "import { helperScriptId } from './helper';\nconsole.log(`brain ${script.id} uses helper ${helperScriptId}`);\n",
    );
    test_runtime.run_tick(0).await?;

    let template = test_runtime.world.createTemplateFrom(enemy_object_id)?;
    let copy_object_id = test_runtime.world.addObjectFromTemplate(&template, root_object_id)?;
    test_runtime.run_tick(0).await?;

    let mut expected_logs = Vec::new();
    for enemy_id in [enemy_object_id, copy_object_id] {
        let script_id_of_name = |name: &str| -> anyhow::Result<ObjectId> {
            let child_ids = test_runtime.world.getDirectChildrenIds(enemy_id)?;
            child_ids
                .iter()
                .copied()
                .find(|child_id| test_runtime.world.getObject(*child_id).is_ok_and(|child| child.name == name))
                .ok_or_else(|| anyhow::anyhow!("{name} not found under {enemy_id}"))
        };
        expected_logs.push(format!("brain {} uses helper {}", script_id_of_name("brain")?, script_id_of_name("helper")?));
    }
    assert_eq!(test_runtime.take_logs(), expected_logs);
    Ok(())
}

// Code in a removed script can still resume from an await it was in. It must not be able to start anything new.
#[tokio::test]
async fn a_removed_script_that_resumes_cannot_add_listeners_or_timers() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let sleeper_object_id = test_runtime.add_script(
        "sleeper",
        root_object_id,
        r#"
await new Promise((resolve) => { globalThis.wakeSleeper = resolve; });
game.objectService.addObjectCreatedListener(() => console.log("late listener"));
setInterval(() => console.log("late interval"), 1);
setTimeout(() => console.log("late timeout"), 1);
console.log("sleeper resumed");
"#,
    );
    test_runtime.run_tick(0).await?;
    test_runtime.world.removeObject(sleeper_object_id)?;
    test_runtime.run_tick(0).await?;

    test_runtime.add_script("waker", root_object_id, "globalThis.wakeSleeper();");
    test_runtime.run_tick(0).await?;
    for _ in 0..2 {
        test_runtime.add_folder("trigger", root_object_id);
        test_runtime.run_tick(5).await?;
    }
    assert_eq!(test_runtime.take_logs(), vec!["sleeper resumed"]);
    Ok(())
}

// This used to stop every other script added in the same tick from starting.
#[tokio::test]
async fn a_script_removed_before_it_starts_never_runs_and_does_not_block_others() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    let vanishing_object_id = test_runtime.add_script("vanishing", root_object_id, "console.log('vanishing ran');");
    test_runtime.add_script("survivor", root_object_id, "console.log('survivor ran');");
    test_runtime.world.removeObject(vanishing_object_id)?;
    test_runtime.run_tick(0).await?;
    assert_eq!(test_runtime.take_logs(), vec!["survivor ran"]);
    Ok(())
}

// deno_core 0.352 panicked in debug builds when the last timer was cleared after it came due but before it ran.
#[tokio::test]
async fn clearing_the_only_timer_after_it_comes_due_does_not_crash() -> anyhow::Result<()> {
    let mut test_runtime = TestRuntime::new(make_world()?).await?;
    let root_object_id = test_runtime.world.getRootObjectId();
    test_runtime.add_script(
        "clearer",
        root_object_id,
        r#"
const intervalId = setInterval(() => {}, 1);
game.objectService.addObjectCreatedListener(() => {
    clearInterval(intervalId);
    console.log("cleared");
});
"#,
    );
    test_runtime.run_tick(0).await?;
    test_runtime.add_folder("trigger", root_object_id);
    test_runtime.run_tick(5).await?;
    test_runtime.run_tick(5).await?;
    assert_eq!(test_runtime.take_logs(), vec!["cleared"]);
    Ok(())
}
