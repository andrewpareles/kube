mod helpers;

use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectLocation, ScriptInfo};
use helpers::world_test_helpers::make_world_controlled_by_player;
use shared::game::jsRuntime::{JsRuntimeInitialization, callJsFn, createJsExtensions, initializeJsAndRunInitialScripts, initializeJsRuntime};

const PLAYER_OBJECT_ID: ObjectId = 99;
const SCRIPT_OBJECT_ID: ObjectId = 2;

// A released binary runs on machines without the crates it was built from. Any file left out here works in every test on a
// machine that built the crates, then stops every game on the servers, which is what happened in v0.1.133.
#[test]
fn every_js_file_of_every_extension_is_in_the_binary() -> anyhow::Result<()> {
    for extension in createJsExtensions()? {
        let files_of_kind = [
            &extension.js_files,
            &extension.esm_files,
            &extension.lazy_loaded_esm_files,
            &extension.lazy_loaded_js_files,
        ];
        for file in files_of_kind.into_iter().flat_map(|files| files.iter()) {
            assert!(file.is_runtime_loadable(), "{} would be read from the machine that built it", file.specifier);
        }
    }
    Ok(())
}

// deno_core stopped supplying locale data by default after 0.352, so this data now comes from V8's own library.
#[tokio::test]
async fn scripts_can_format_numbers_and_dates_for_a_locale() -> anyhow::Result<()> {
    let mut world = make_world_controlled_by_player(PLAYER_OBJECT_ID)?;
    let root_object_id = world.getRootObjectId();
    world.addObjectWithId(
        SCRIPT_OBJECT_ID,
        "locale_script".to_string(),
        ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: ObjectLocation::Client,
        }),
        root_object_id,
    );
    world.markScriptAsReadyToRun(
        SCRIPT_OBJECT_ID,
        r#"'use client'
export const readLocaleText = () => [
    (1234.5).toLocaleString("en-US"),
    new Intl.NumberFormat("de-DE").format(1234.5),
    new Date(0).toLocaleDateString("en-US", { timeZone: "UTC" }),
    ["b", "a"].sort((first, second) => first.localeCompare(second)).join(""),
    "é".normalize("NFC").length.toString(),
];
"#
        .to_string(),
    );

    let (mut js_runtime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
    initializeJsAndRunInitialScripts(&mut js_runtime, &mut world, 0.0, Some(0.0), &script_manager_rc).await?;
    let read_locale_text = script_manager_rc
        .borrow()
        .get_client_js_fn("readLocaleText#root/locale_script")
        .expect("Expected the registered function")
        .clone();
    let result = callJsFn(&mut js_runtime, &mut world, 0.0, Some(0.0), &read_locale_text, r#"{"json":[]}"#).await?;
    assert_eq!(
        helpers::remote_json_result::<Vec<String>>(&result)?,
        ["1,234.5", "1.234,5", "1/1/1970", "ab", "1"]
    );
    Ok(())
}
