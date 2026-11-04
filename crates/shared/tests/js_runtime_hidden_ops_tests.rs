mod helpers;

use helpers::world_test_helpers::{add_script, make_world};
use shared::game::jsRuntime::{JsRuntimeInitialization, initializeJsAndRunInitialScripts, initializeJsRuntime, runTemporaryScript};
use std::sync::{Arc, Mutex};

// Scripts reach the engine only through kube-ts-api. Through Deno.core.ops, __bootstrap.core.ops or an ext: import, a script
// could call any op with any arguments, like op_panic, which stops the whole process.
#[tokio::test]
async fn game_scripts_cannot_reach_the_ops() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let root_id = world.getRootObjectId();
    add_script(&mut world, 2, root_id)?;
    world.markScriptAsReadyToRun(
        2,
        r#"'use server'
export const probe = async () => {
    let importError = "";
    try { await import("ext:core/ops"); } catch (error) { importError = String(error); }
    console.log("the console still works");
    return [typeof Deno, typeof __bootstrap, importError];
};
"#
        .to_string(),
    );
    let messages_arc = Arc::new(Mutex::new(Vec::new()));
    let (mut runtime, manager_rc) = initializeJsRuntime(
        JsRuntimeInitialization::NoNetworking,
        Some(Arc::new({
            let messages_arc = messages_arc.clone();
            move |entry| messages_arc.lock().unwrap().push(entry.message)
        })),
    )
    .await?;
    initializeJsAndRunInitialScripts(&mut runtime, &mut world, 0.0, None, &manager_rc).await?;

    let result = helpers::call_server_fn(&mut runtime, &mut world, 0.0, &manager_rc, 0, "probe#root/script-2", r#"{"json":[]}"#).await?;
    let [deno_type, bootstrap_type, import_error] = helpers::remote_json_result::<[String; 3]>(&result)?;
    assert_eq!(deno_type, "undefined");
    assert_eq!(bootstrap_type, "undefined");
    assert!(import_error.contains("Importing ext: modules is only allowed"), "{import_error}");
    let messages = messages_arc.lock().unwrap();
    assert!(messages.iter().any(|message| message.contains("the console still works")), "{messages:?}");
    Ok(())
}

#[test]
fn temporary_scripts_cannot_reach_the_ops() -> anyhow::Result<()> {
    let mut world = make_world()?;
    let console_log_entries = runTemporaryScript(&mut world, "console.log(typeof Deno, typeof __bootstrap);".to_string())?;
    let messages: Vec<&str> = console_log_entries.iter().map(|entry| entry.message.trim()).collect();
    assert_eq!(messages, ["undefined undefined"]);
    Ok(())
}
