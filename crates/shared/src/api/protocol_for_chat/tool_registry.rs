use super::{ClientEditorToolHandlers, ToolCall, ToolImage, ToolResultFinal};
use crate::{
    api::protocol_for_assets::{AssetPublishData, AssetSearchRequest},
    conversions::float_conversions::with_short_floats,
    game::{
        assets::{
            AssetData, AssetType as ApiAssetType, StoredAssetObject, add_asset_to_world, add_picture_to_world, download_asset, picture_size_of_texture,
            publish_to_assets, record_asset_import, search_assets,
        },
        data_fetcher::DataFetcherArc,
        jsRuntime::runTemporaryScript,
        jsRustConversions::{ValueWithPath, asJSON, rustObjectBeingAddedOfJSObjectInfo, rustUpdateObject},
        physics::SetOrForce,
        world::World,
    },
};
use async_stream::stream;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{Stream, StreamExt, stream::FuturesUnordered};
use game_types::{
    consts::MAX_PLAYTEST_CLIENTS,
    data_types::{DataId, MeshData, MeshVertexInternal},
    object_types::{
        BallInfo, BrickInfo, CylinderInfo, LabelHorizontalAlign, LabelImageInfo, LabelInfo, Location3D, Movement3D, ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo,
        ObjectKind, Orientation3D, PhysicsInfo3D, PhysicsToggles3D, VisualSettings, label::BillboardMode,
        shared::{CameraLocation, Color3, euler_degrees_to_quat},
    },
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings},
};
use glam::{Mat3, Quat, Vec3};
use render_core::dehydrated_gltf::DehydratedGltf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    pin::Pin,
    str::FromStr,
    sync::{Arc, LazyLock},
};
use strum::{AsRefStr, EnumString};
use tokio::{
    sync::RwLock,
    time::{Duration, Instant, timeout_at},
};
use ts_rs::TS;

// -------------------------- chat tools --------------------------

#[derive(AsRefStr, Clone, Copy, Debug, Deserialize, EnumString, Eq, PartialEq, Serialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ToolName {
    CreateObject,
    AssetSearch,
    AssetPublishWorldObject,
    AssetDownload,
    CatalogItemPublish,
    CatalogItemEdit,
    DeleteObject,
    ReadDirtyScriptsInWorkspace, // read every script's live text out of the editor, including unsaved edits
    ReadAllScriptWarnings,       // read all script errors from monaco
    ReadScriptRawCode,           // read a script object_id's raw code
    WriteScriptRawCode,          // write a script object_id's raw code
    EditScriptRawCode,           // replace exact text in a script object_id's raw code
    // GetPlacementWarnings,
    CaptureScreenshot,
    Ls,
    Inspect,
    Grep,
    GetPath,
    GetGameSettings,
    CopyAndPaste,
    EditObject,
    SetGameSettings,
    SetFallbackCamera,
    Publish,
    LsData,
    InspectData,
    MoveByRecursively,
    MoveToRecursively,
    ScaleRecursively,
    RotateRecursively,
    RunTemporaryScript,
    CreateBrick,
    CreateBoxBetweenCorners,
    CreateWall,
    CreateBrickBetweenPoints,
    CreateBall,
    CreateCylinder,
    CreateCylinderBetweenPoints,
    CreateLabel,
    CreateLabelImage,
    ReportIssue,
    RunParallel,
    StartAiHeadlessPlaytestServer,
    StartAiHeadlessPlaytestClient,
    StopAiHeadlessPlaytestClient,
    ReadAiHeadlessPlaytestLogs,
    CaptureAiHeadlessPlaytestScreenshot,
    StopAiHeadlessPlaytestAll,
}

// -------------------------- TOOL IMPLEMENTATIONS --------------------------

pub type ToolResultStream = Pin<Box<dyn Stream<Item = ToolResultFinal> + Send>>;
type EditorToolRunner = Box<dyn Fn(Arc<RwLock<World>>, DataFetcherArc, Option<String>, Value) -> ToolResultStream + Send + Sync>;
pub struct EditorToolSpec {
    pub tool_name: ToolName,
    pub description: String,
    pub parameters_json: Value,
    pub run: EditorToolRunner,
}

// makes sure LLM does not include extra fields by adding "additionalProperties": false to the schema
fn ensure_no_additional_properties(schema: serde_json::Value) -> serde_json::Value {
    fn visit(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                let is_object = map.get("type").and_then(|value| value.as_str()).is_some_and(|value| value == "object");
                if is_object {
                    map.entry("properties".to_string()).or_insert(serde_json::Value::Object(Default::default()));
                    map.entry("additionalProperties".to_string()).or_insert(serde_json::Value::Bool(false));
                }
                for value in map.values_mut() {
                    visit(value);
                }
            }
            serde_json::Value::Array(items) => {
                for value in items {
                    visit(value);
                }
            }
            _ => {}
        }
    }

    let mut schema = schema;
    visit(&mut schema);
    schema
}

fn tool_schema_for<T: JsonSchema>() -> serde_json::Value {
    // unwrap is used here, hard to miss if it panics because the api uses it
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    ensure_no_additional_properties(schema)
}

fn make_editor_tool_spec<
    Args: serde::de::DeserializeOwned + JsonSchema + Send + 'static + TS,
    ToolCallFutureType: std::future::Future<Output = anyhow::Result<ToolResultFinal>> + Send + 'static,
>(
    tool_name: ToolName,
    description: String,
    apply_tool_call: fn(Arc<RwLock<World>>, DataFetcherArc, Option<String>, Args) -> ToolCallFutureType,
) -> EditorToolSpec {
    let ts_config = ts_rs::Config::default();
    let description_with_input_ts_type = format!("{}\nInput type: {}", description, Args::inline(&ts_config));
    log::debug!("description_with_input_ts_type: {}", description_with_input_ts_type);
    EditorToolSpec {
        tool_name,
        description: description_with_input_ts_type,
        parameters_json: tool_schema_for::<Args>(),
        run: Box::new(move |world_arc, data_fetcher, editor_server_token, args_json| {
            Box::pin(stream! {
                let args = match serde_json::from_value::<Args>(args_json) {
                    Ok(args) => args,
                    Err(error) => {
                        yield tool_failed(format!("Tool args error: {}", error));
                        return;
                    }
                };
                let tool_run_final = match apply_tool_call(world_arc, data_fetcher, editor_server_token, args).await {
                    Ok(tool_run_final) => tool_run_final,
                    Err(error) => tool_failed(format!("Tool error: {}", error)),
                };
                yield tool_run_final;
            })
        }),
    }
}

pub struct EditorToolRegistry {
    tool_specs: Vec<EditorToolSpec>,
}
impl EditorToolRegistry {
    pub fn tool_specs(&self) -> &[EditorToolSpec] {
        self.tool_specs.as_slice()
    }

    pub fn get_tool(&self, tool_name: ToolName) -> Option<&EditorToolSpec> {
        self.tool_specs.iter().find(|tool_spec| tool_spec.tool_name == tool_name)
    }
}
pub static EDITOR_TOOL_REGISTRY: LazyLock<EditorToolRegistry> = LazyLock::new(|| {
    EditorToolRegistry {
    tool_specs: vec![

        // -------------------------- READS: --------------------------

        make_editor_tool_spec(
            ToolName::AssetSearch,
            "Search assets by name. Filter by Mesh, Texture, Sound (music and other sounds 10 seconds or longer), Sfx (sound effects under 10 seconds), Object, Material, or Skybox, or use null for all types. Use offset and limit to page through results (limit 1-100).".to_string(),
            apply_asset_search,
        ),
        make_editor_tool_spec(
            ToolName::ReadDirtyScriptsInWorkspace,
            "Read every script's text as it is in the editor right now, including edits the user has not saved yet, with optional TypeScript diagnostics."
                .to_string(),
            apply_read_dirty_scripts_in_workspace,
        ),
        make_editor_tool_spec(
            ToolName::ReadAllScriptWarnings,
            "Read TypeScript diagnostics for every script in the editor.".to_string(),
            apply_read_all_script_warnings,
        ),
        make_editor_tool_spec(
            ToolName::ReadScriptRawCode,
            "Read the stored raw code for one script object, with each line numbered like `cat -n` so the numbers match TypeScript warnings. Use start_line and line_count to read part of a long script.".to_string(),
            apply_read_script_raw_code,
        ),
        make_editor_tool_spec(
            ToolName::Ls,
            "List the children under `in_object_id`, expanding them up to `depth` levels. `limit` caps the objects in the whole answer, counting every level, and fills the top levels first; `offset` pages through the direct children. Every object gives its `child_count`, so ls into one whose children were cut off.".to_string(),
            apply_ls,
        ),
        make_editor_tool_spec(
            ToolName::LsData,
            "List referenced mesh, animation, and texture data ids in the world, with optional mode filtering and pagination.".to_string(),
            apply_ls_data,
        ),
        make_editor_tool_spec(
            ToolName::Inspect,
            "Inspect one object. Returns object summary data plus parent, children, incoming pointers, referenced data ids, and info or physics fields based on `mode`. Children and incoming pointers list the first 100 of each, with their counts. Use read_script_raw_code for a script's code."
                .to_string(),
            apply_inspect,
        ),
        make_editor_tool_spec(
            ToolName::InspectData,
            "Inspect cached mesh, animation, or texture data by `data_id`. Texture results are metadata-only in this text response, and uncached data returns a failure payload describing the referenced types."
                .to_string(),
            apply_inspect_data,
        ),
        make_editor_tool_spec(
            ToolName::Grep,
            "Search objects by AND-ing all provided filters. Supports name, object_type, parent, pointer relationships, referenced data ids, and substring search over serialized object info."
                .to_string(),
            apply_grep,
        ),
        make_editor_tool_spec(
            ToolName::GetPath,
            "Get the parent path from the root object down to one object, with optional pagination over the path entries.".to_string(),
            apply_get_path,
        ),
        make_editor_tool_spec(
            ToolName::GetGameSettings,
            "Get the current game title, description, suggested min players, max players, and visibility.".to_string(),
            apply_get_game_settings,
        ),

        // make_editor_tool_spec(
        //     ToolName::GetPlacementWarnings,
        //     "Report geometry overlaps deeper than 0.001 units and objects without support below. Intentional floating or overlapping geometry may be valid."
        //         .to_string(),
        //     apply_get_placement_warnings,
        // ),
        make_editor_tool_spec(
            ToolName::CaptureScreenshot,
            "Capture a screenshot of the edit-mode world from a camera position and look_at point in world coordinates, and return the image for you to inspect. This does not capture UI. Use it to check proportions, colors, visibility, and placement after building or changing a scene.".to_string(),
            apply_capture_screenshot,
        ),
        // -------------------------- WRITES: --------------------------

        make_editor_tool_spec(
            ToolName::AssetDownload,
            "Download the latest revision of an asset into the root of this world. Mesh imports the complete glTF model with materials and animations; Object inserts its hierarchy with fresh ids; Texture creates a LabelImage of the picture on the front of a thin board its own shape; Material creates a Material with all of its texture maps; Sound and Sfx create a SoundPlayer with a paused child Clock; Skybox replaces the gradient sky with its panorama by setting the world lighting's skybox, creating the world lighting object if there is none. Returns the created or changed object id.".to_string(),
            apply_asset_download,
        ),
        make_editor_tool_spec(
            ToolName::AssetPublishWorldObject,
            "Publish an object and its children as an Object asset on the Assets page, where anyone can find and use it. An object's references must stay inside its hierarchy. Supply asset_id to append a revision to an asset you published, or null to create an asset. Publishing it publicly shares all included scripts. A model file is published as a Mesh asset by importing it with should_publish.".to_string(),
            apply_asset_publish_world_object,
        ),
        make_editor_tool_spec(
            ToolName::CatalogItemPublish,
            format!("Publish a model as a new item in the kube.new catalog, where people get hats for free. Give `asset_id`, a Mesh asset you or the game's creator uploaded to assets. Set is_listed false to publish it unlisted. Returns the item's catalog id and URL."),
            apply_catalog_item_publish,
        ),
        make_editor_tool_spec(
            ToolName::CatalogItemEdit,
            "Change the name, description, or listing of a catalog item this game's creator published. Omitted fields are left unchanged, and an empty description clears it. An unlisted item (is_listed false) stays off the Catalog page and can't be bought, but anyone with its id can still use it, and anyone who already bought it keeps it.".to_string(),
            apply_catalog_item_edit,
        ),
        make_editor_tool_spec(
            ToolName::CreateObject,
            format!(
                "Create a world object from the JSON string in `object`. If `parent_id` is omitted, the object is created under the root.",
            ),
            apply_create_object,
        ),
        make_editor_tool_spec(
            ToolName::DeleteObject,
            "Delete an object, recursively removing its children and clearing pointer links that point to it.".to_string(),
            apply_delete_object,
        ),
        make_editor_tool_spec(
            ToolName::CopyAndPaste,
            "Duplicate an object and its children under the same parent. `offset` moves the copied subtree in world space, and the copied root name is auto-suffixed with `_Copy` if `clone_name` is omitted."
                .to_string(),
            apply_copy_and_paste,
        ),
        make_editor_tool_spec(
            ToolName::EditObject,
            "Edit one object at `jspath` using the JSON string in `jsvalue`. `jspath` can be a dot-separated path like `a.b.c` or a JSON-encoded array of path segments."
                .to_string(),
            apply_edit_object,
        ),
        make_editor_tool_spec(
            ToolName::SetGameSettings,
            "Set one or more current game settings. Omitted fields are left unchanged. Setting visibility to Published only changes visibility; it does not save or publish the current editor world. suggested_min_players is the suggested number of players the game needs to start: 1 if it can be played alone, 2 if it needs at least two people to start, and so on. It is shown on the game's card and page, is never enforced, and can't be more than max_players."
                .to_string(),
            apply_set_game_settings,
        ),
        make_editor_tool_spec(
            ToolName::SetFallbackCamera,
            "Set the game's fallback camera from a position and look_at point in world coordinates, the same camera capture_screenshot takes. Publishing captures the game's thumbnail from this camera, the editor opens at it, and the published game shows it when there is no player to follow."
                .to_string(),
            apply_set_fallback_camera,
        ),
        make_editor_tool_spec(
            ToolName::Publish,
            "Publish the current world to the website, overriding the existing published game for all new servers that start. This publishes the current version and captures its thumbnail, and returns the URL the published game is live at. Call this only after all edits and checks are complete, and do not call it inside run_parallel."
                .to_string(),
            apply_publish,
        ),
        make_editor_tool_spec(
            ToolName::WriteScriptRawCode,
            "Write raw code for one script object, replacing all of it. Returns TypeScript warnings for the script. To change part of a script, use edit_script_raw_code.".to_string(),
            apply_write_script_raw_code,
        ),
        make_editor_tool_spec(
            ToolName::EditScriptRawCode,
            "Replace `old_string` with `new_string` in one script object's raw code. `old_string` must match the code exactly, including indentation and without the line numbers read_script_raw_code adds, and must appear exactly once unless `replace_all` is true. Returns TypeScript warnings for the script. Prefer this to write_script_raw_code when changing part of a script.".to_string(),
            apply_edit_script_raw_code,
        ),
        make_editor_tool_spec(
            ToolName::RunTemporaryScript,
            "Run one temporary server script against the editor world, then remove the temporary Script object. Use this for short command-style edits that are easier in TypeScript than individual object tools. Returns console logs plus how many objects of each type were created, deleted, and modified, so log any object ids you need. Execution has a 30-second limit. On failure, changes already applied remain and are counted in the result. Do not use tick() or timers here."
                .to_string(),
            apply_run_temporary_script,
        ),
        make_editor_tool_spec(
            ToolName::MoveByRecursively,
            "Move an object and its physical descendants by `translation` in world space."
                .to_string(),
            apply_move_by_recursively,
        ),
        make_editor_tool_spec(
            ToolName::MoveToRecursively,
            "Move an object and its physical descendants so the root object ends up at `target_point`. This fails if the root object has no direct physics position."
                .to_string(),
            apply_move_to_recursively,
        ),
        make_editor_tool_spec(
            ToolName::ScaleRecursively,
            "Scale an object and its physical descendants by `scale_multiplier` around `center_point`. If `center_point` is omitted, the tool uses the point of the object's Pivot if it has one, otherwise the object's current physics position, and fails if the object has no direct physics position."
                .to_string(),
            apply_scale_recursively,
        ),
        make_editor_tool_spec(
            ToolName::RotateRecursively,
            "Rotate an object and its physical descendants by `rotation_magnitude` radians around `rotation_axis` and `center_point`. If `center_point` is omitted, the tool uses the point of the object's Pivot if it has one, otherwise the object's current physics position, and fails if the object has no direct physics position."
                .to_string(),
            apply_rotate_recursively,
        ),
        make_editor_tool_spec(
            ToolName::CreateBrick,
            "Create a brick from a center point, size, and optional rotation in degrees, applied like `location.rotation`."
                .to_string(),
            apply_create_brick,
        ),
        make_editor_tool_spec(
            ToolName::CreateBoxBetweenCorners,
            "Create an axis-aligned brick from two opposite corners."
                .to_string(),
            apply_create_box_between_corners,
        ),
        make_editor_tool_spec(
            ToolName::CreateWall,
            "Create a vertical wall from two floor-level centerline endpoints, plus height and thickness."
                .to_string(),
            apply_create_wall,
        ),
        make_editor_tool_spec(
            ToolName::CreateBrickBetweenPoints,
            "Create a brick from two endpoint centers, width, and height. The brick length runs along local Z from start to end, and local Y stays as close to world up as possible."
                .to_string(),
            apply_create_brick_between_points,
        ),
        make_editor_tool_spec(
            ToolName::CreateBall,
            "Create a ball from a center point and radius.".to_string(),
            apply_create_ball,
        ),
        make_editor_tool_spec(
            ToolName::CreateCylinder,
            "Create a cylinder from a center point, radius, length, and optional rotation in degrees, applied like `location.rotation`. The unrotated cylinder length runs along local Z."
                .to_string(),
            apply_create_cylinder,
        ),
        make_editor_tool_spec(
            ToolName::CreateCylinderBetweenPoints,
            "Create a cylinder from two endpoint centers and a radius.".to_string(),
            apply_create_cylinder_between_points,
        ),
        make_editor_tool_spec(
            ToolName::CreateLabel,
            "Create a text label in the world. It has no body of its own: it follows the object given by follows_object_id, floating at an offset above it, and always turns to face the camera.".to_string(),
            apply_create_label,
        ),
        make_editor_tool_spec(
            ToolName::CreateLabelImage,
            "Create a picture in the world from a texture. Like a label it has no body of its own: it follows the object given by follows_object_id, floating at an offset above it, and always turns to face the camera. To stick it flat on a surface instead, set its billboardMode to 'none' and turn it with rotationFromObject; with no rotation it faces the followed object's +Z side.".to_string(),
            apply_create_label_image,
        ),

        // -------------------------- PLAYTEST: --------------------------

        make_editor_tool_spec(
            ToolName::StartAiHeadlessPlaytestServer,
            "Start a new \"playtest\" testing server that runs exactly like a normal server. All scripts execute, etc. The world is a snapshot taken now, so start a new one after editing to test the change. Only one playtest runs at a time; starting one always replaces the previous one and clears its logs.".to_string(),
            apply_start_ai_headless_playtest_server,
        ),
        make_editor_tool_spec(
            ToolName::StartAiHeadlessPlaytestClient,
            format!(
                "Make one player join the running playtest server, the way a real player joining would. Returns their client_id. Call this again for each extra player, up to {}.",
                MAX_PLAYTEST_CLIENTS,
            ),
            apply_start_ai_headless_playtest_client,
        ),
        make_editor_tool_spec(
            ToolName::StopAiHeadlessPlaytestClient,
            "Make one player leave the playtest server, the way a real player quitting would. The server and every other player keep running.".to_string(),
            apply_stop_ai_headless_playtest_client,
        ),
        make_editor_tool_spec(
            ToolName::ReadAiHeadlessPlaytestLogs,
            format!(
                "Read console output from the running playtest server or client, oldest first. Pass the `next_seq` from the previous call as `after_seq` to continue where you left off, and `wait_ms` to sleep that long before reading so the game has time to print more. Each entry is one console call and can span several lines. Waits longer than {}ms are shortened to that.",
                PLAYTEST_MAX_WAIT_MS,
            ),
            apply_read_ai_headless_playtest_logs,
        ),
        make_editor_tool_spec(
            ToolName::CaptureAiHeadlessPlaytestScreenshot,
            "Take a screenshot of what one playtest player sees right now: the world from their own camera, with the UI their client scripts show on top. Nothing steers the player, so this shows the game running on its own.".to_string(),
            apply_capture_ai_headless_playtest_screenshot,
        ),
        make_editor_tool_spec(
            ToolName::StopAiHeadlessPlaytestAll,
            "Stop and cleanup the whole playtest server and all its clients. This happens automatically when your turn ends (unless you are in MCP mode (if so you will be told)), so only call it to free things up early.".to_string(),
            apply_stop_ai_headless_playtest_all,
        ),

        // -------------------------- META: --------------------------

        make_editor_tool_spec(
            ToolName::ReportIssue,
            "Report that tool behavior or the current system instructions or tool definitions are missing, conflicting, or unclear for the task. Use this instead of guessing when tool calling does not go to plan.".to_string(),
            apply_report_issue,
        ),
        make_editor_tool_spec(
            ToolName::RunParallel,
            "Run multiple tool calls concurrently and return one status per subcall. Each `tool_calls[].args` value must be a JSON string for that subtool, and `timeout_ms` applies to the whole batch."
                .to_string(),
            apply_run_parallel,
        ),
    ],
}
});

pub fn stream_editor_tool_call(
    world_arc: Arc<RwLock<World>>,
    data_fetcher: DataFetcherArc,
    editor_server_token: Option<String>,
    call: ToolCall,
) -> ToolResultStream {
    let ToolCall {
        name: tool_name,
        arguments: args,
    } = call;

    match ToolName::from_str(&tool_name) {
        Ok(tool_name) => match EDITOR_TOOL_REGISTRY.get_tool(tool_name) {
            Some(tool_spec) => (tool_spec.run)(world_arc, data_fetcher, editor_server_token, args),
            None => Box::pin(stream! {
                yield tool_failed(format!("Tool error: Unknown tool name: {}", tool_name.as_ref()));
            }),
        },
        Err(_) => Box::pin(stream! {
            yield tool_failed(format!("Tool error: Unknown tool name: {}", tool_name));
        }),
    }
}

// TODO: AI-PICKED-VALUE: A tool result is at most 100,000 characters, about 25,000 tokens, so no single result can fill an AI's
// conversation. A longer one keeps its start and its end, since results put their summary first and console output last.
const MAX_TOOL_RESULT_CHARS: usize = 100_000;

pub async fn apply_editor_tool_call(
    world_arc: Arc<RwLock<World>>,
    data_fetcher: DataFetcherArc,
    editor_server_token: Option<String>,
    call: ToolCall,
) -> ToolResultFinal {
    let mut tool_run_stream = stream_editor_tool_call(world_arc, data_fetcher, editor_server_token, call);
    while let Some(mut tool_run_final) = tool_run_stream.next().await {
        let char_count = tool_run_final.text.chars().count();
        if char_count > MAX_TOOL_RESULT_CHARS {
            let kept_char_count = MAX_TOOL_RESULT_CHARS / 2;
            let start: String = tool_run_final.text.chars().take(kept_char_count).collect();
            let end: String = tool_run_final.text.chars().skip(char_count - kept_char_count).collect();
            tool_run_final.text = format!(
                "{start}\n\n[{} characters were cut from the middle of this result, since a tool result shows at most {MAX_TOOL_RESULT_CHARS}. Ask for less at a time, for example with a smaller limit or a line range, or by logging less.]\n\n{end}",
                char_count - 2 * kept_char_count
            );
        }
        return tool_run_final;
    }

    tool_failed("Tool stream ended before a final result.".to_string())
}

// -------------------------- utility helpers --------------------------

const EPSILON: f32 = 0.00001;
// const PLACEMENT_CONTACT_TOLERANCE: f32 = 0.001;

const TOOL_DEFAULT_OFFSET: usize = 0;
const TOOL_DEFAULT_LIMIT: usize = 100;
const TOOL_DEFAULT_DEPTH: usize = 3;
const TOOL_RUN_PARALLEL_DEFAULT_TIMEOUT_MS: u64 = 30_000;
const PLAYTEST_MAX_WAIT_MS: u64 = 10_000;

fn paginate_vec<T>(items: Vec<T>, offset: usize, limit: usize) -> (Vec<T>, bool) {
    if limit == 0 {
        let has_more = offset < items.len();
        return (Vec::new(), has_more);
    }

    let total_len = items.len();
    let paged_items = items.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
    let has_more = offset.saturating_add(paged_items.len()) < total_len;
    (paged_items, has_more)
}

fn object_kind_name_of_object_info(info: &ObjectInfo) -> &'static str {
    ObjectKind::from_object_info(info).to_object_kind_name()
}

fn object_summary_json(world: &World, object_id: ObjectId) -> anyhow::Result<Value> {
    let object = world.getObject(object_id)?;
    Ok(json!({
        "object_id": object_id,
        "name": object.name,
        "type": object_kind_name_of_object_info(&object.info),
    }))
}

fn data_ids_of_object_info(info: &ObjectInfo) -> Vec<DataId> {
    let mut data_ids = Vec::new();

    match info {
        ObjectInfo::Material(material_info) => {
            if let Some(data_id) = material_info.albedoTextureDataId {
                data_ids.push(data_id);
            }
            if let Some(data_id) = material_info.metallicTextureDataId {
                data_ids.push(data_id);
            }
            if let Some(data_id) = material_info.roughnessTextureDataId {
                data_ids.push(data_id);
            }
            if let Some(data_id) = material_info.normalTextureDataId {
                data_ids.push(data_id);
            }
        }
        ObjectInfo::Mesh(mesh_info) => {
            data_ids.push(mesh_info.meshDataId);
        }
        ObjectInfo::SoundPlayer(sound_player_info) => {
            data_ids.push(sound_player_info.soundDataId);
        }
        ObjectInfo::LabelImage(label_image_info) => {
            data_ids.push(label_image_info.textureDataId);
        }
        _ => {}
    }

    data_ids.sort_unstable();
    data_ids.dedup();
    data_ids
}

fn object_uses_data_id(info: &ObjectInfo, data_id: DataId) -> bool {
    data_ids_of_object_info(info).into_iter().any(|candidate_data_id| candidate_data_id == data_id)
}

fn tool_succeeded(text: String) -> ToolResultFinal {
    ToolResultFinal {
        is_error: false,
        text,
        images: Vec::new(),
    }
}

fn tool_failed(text: String) -> ToolResultFinal {
    ToolResultFinal {
        is_error: true,
        text,
        images: Vec::new(),
    }
}

// TODO: AI-PICKED-VALUE: 768x432 keeps scene details readable at about half the image tokens of 1024x576, and draws in about 0.4s on the container's CPU renderer.
const AI_SCREENSHOT_WIDTH: u32 = 768;
const AI_SCREENSHOT_HEIGHT: u32 = 432;
// TODO: AI-PICKED-VALUE: At quality 92 a screenshot looks the same as the PNG at about a sixth of the bytes (61 KB against 367 KB
// on game 319's), so a chat fits about 380 of them under Claude's 32 MB request limit instead of about 60.
const AI_SCREENSHOT_JPEG_QUALITY: u8 = 92;

// Screenshots are drawn as PNG and sent to the AI as JPEG.
fn screenshot_for_ai(png_bytes: &[u8]) -> anyhow::Result<ToolImage> {
    let image = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)?.into_rgb8();
    let mut jpeg_bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, AI_SCREENSHOT_JPEG_QUALITY).encode_image(&image)?;
    Ok(ToolImage {
        media_type: "image/jpeg".to_string(),
        base64: BASE64_STANDARD.encode(jpeg_bytes),
    })
}

// Capture uses the renderer on the editor thread; only the resulting image crosses the API.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct ScreenshotCameraArgs {
    pub position: Vec3Args,
    pub look_at: Vec3Args,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CaptureScreenshotArgs {
    pub camera: ScreenshotCameraArgs,
}

fn read_camera_points(camera: &ScreenshotCameraArgs, tool_name: ToolName) -> anyhow::Result<(Vec3, Vec3)> {
    let position = Vec3::from(camera.position);
    let look_at = Vec3::from(camera.look_at);
    let direction = look_at - position;
    if !position.is_finite() || !look_at.is_finite() || !direction.is_finite() || direction.length_squared() < f32::EPSILON {
        anyhow::bail!("{} camera points must be finite and different.", tool_name.as_ref());
    }
    if direction.normalize().cross(Vec3::Y).length_squared() < f32::EPSILON {
        anyhow::bail!("{} camera must not point exactly straight up or down.", tool_name.as_ref());
    }
    Ok((position, look_at))
}

async fn apply_capture_screenshot(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CaptureScreenshotArgs,
) -> anyhow::Result<ToolResultFinal> {
    let (position, look_at) = read_camera_points(&args.camera, ToolName::CaptureScreenshot)?;
    let handler_arc = ClientEditorToolHandlers::get_curr()?;
    let png_bytes = handler_arc
        .screenshot_handler
        .capture(position, look_at, AI_SCREENSHOT_WIDTH, AI_SCREENSHOT_HEIGHT)
        .await?;
    Ok(ToolResultFinal {
        is_error: false,
        text: "Screenshot of the edit-mode world from the requested camera.".to_string(),
        images: vec![screenshot_for_ai(&png_bytes)?],
    })
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct ReadScriptFilesArgs {
    pub path_substring: Option<String>,
    pub include_text: Option<bool>,
    pub include_diagnostics: Option<bool>,
    pub only_files_with_diagnostics: Option<bool>,
}

async fn apply_read_dirty_scripts_in_workspace(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: ReadScriptFilesArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;

    let script_files_text = client_editor_tool_handlers.script_files_handler.read_text(args).await?;
    Ok(tool_succeeded(script_files_text))
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct ReadAllDiagnosticsArgs {}

async fn apply_read_all_script_warnings(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    _args: ReadAllDiagnosticsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;

    let script_files_text = client_editor_tool_handlers
        .script_files_handler
        .read_text(ReadScriptFilesArgs {
            path_substring: None,
            include_text: Some(false),
            include_diagnostics: Some(true),
            only_files_with_diagnostics: Some(true),
        })
        .await?;
    Ok(tool_succeeded(message_of_json(json!({
        "script_warnings": script_warnings_of_script_files_text(&script_files_text)?,
    }))?))
}

fn message_of_json(value: Value) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(&with_short_floats(value))?)
}

fn parse_json_value_from_string(value: &str, field_name: &str, tool_name: &str) -> anyhow::Result<Value> {
    serde_json::from_str::<Value>(value).map_err(|error| anyhow::anyhow!("{} {} must be a valid JSON string. parse error: {}", tool_name, field_name, error))
}

// some common structs
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct Vec3Args {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl From<Vec3Args> for Vec3 {
    fn from(value: Vec3Args) -> Self {
        Vec3::new(value.x, value.y, value.z)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct Color3Args {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}
impl From<Color3Args> for Color3 {
    fn from(value: Color3Args) -> Self {
        Color3 {
            r: value.r,
            g: value.g,
            b: value.b,
        }
    }
}

// -------------------------- create object --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateObjectArgs {
    pub object: String,
    pub parent_id: Option<ObjectId>,
}

async fn apply_create_object(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateObjectArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let object_json = parse_json_value_from_string(&args.object, "object", "create_object")?;
    let object = asJSON(&object_json, "create_object")?;
    let object_being_added = rustObjectBeingAddedOfJSObjectInfo(&object_json)?;
    let parent_id = args.parent_id.unwrap_or_else(|| world.getRootObjectId());
    let name = object
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "object".to_string());
    // let should_check_placement_warnings = match &object_being_added {
    //     ObjectBeingAdded::Ball(..) | ObjectBeingAdded::Brick(..) | ObjectBeingAdded::Cylinder(..) => true,
    //     _ => false,
    // };
    let object_id = world.addObject(name, object_being_added, parent_id);
    let result_json = json!({
        "message": format!("Successfully created object {}", object_id),
        "object": object_summary_json(world, object_id)?,
        "parent_id": parent_id,
    });
    // if should_check_placement_warnings {
    //     let warning_text = compute_placement_warnings_text(world)?;
    //     if let Some(warning_text) = warning_text {
    //         result_json["warning"] = json!(warning_text);
    //     }
    // }

    Ok(tool_succeeded(message_of_json(result_json)?))
}

// -------------------------- asset search --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub enum AssetType {
    Mesh,
    Texture,
    Sound,
    Object,
    Material,
    Skybox,
    Sfx,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct AssetSearchArgs {
    pub query: String,
    pub asset_type: Option<AssetType>,
    pub offset: usize,
    pub limit: usize,
}

async fn apply_asset_search(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: AssetSearchArgs,
) -> anyhow::Result<ToolResultFinal> {
    let AssetSearchArgs {
        query,
        asset_type,
        offset,
        limit,
    } = args;
    let asset_type = match asset_type {
        Some(AssetType::Mesh) => Some(ApiAssetType::Mesh),
        Some(AssetType::Texture) => Some(ApiAssetType::Texture),
        Some(AssetType::Sound) => Some(ApiAssetType::Sound),
        Some(AssetType::Object) => Some(ApiAssetType::Object),
        Some(AssetType::Material) => Some(ApiAssetType::Material),
        Some(AssetType::Skybox) => Some(ApiAssetType::Skybox),
        Some(AssetType::Sfx) => Some(ApiAssetType::Sfx),
        None => None,
    };
    let request = AssetSearchRequest {
        query,
        asset_type,
        offset,
        limit,
    };
    Ok(tool_succeeded(serde_json::to_string(&search_assets(&request).await?)?))
}

// -------------------------- asset publish world object --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct AssetPublishWorldObjectArgs {
    /// The object to publish, with everything inside it, as an Object asset.
    pub object_id: ObjectId,
    pub asset_id: Option<i64>,
    pub name: String,
}

async fn apply_asset_publish_world_object(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: AssetPublishWorldObjectArgs,
) -> anyhow::Result<ToolResultFinal> {
    let world_lock = world_arc.read().await;
    let object = StoredAssetObject::from_world_object(&world_lock, args.object_id)?;
    drop(world_lock);
    let data = AssetPublishData::Object(serde_json::to_string(&object)?);
    let asset = publish_to_assets(args.asset_id, &args.name, data).await?;
    Ok(tool_succeeded(serde_json::to_string(&serde_json::json!({
        "asset_id": asset.asset_id,
        "version_num": asset.version_num,
        "name": asset.name,
        "created_at": asset.created_at,
        "asset_type": asset.asset_type,
    }))?))
}

// -------------------------- asset download --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct AssetDownloadArgs {
    pub asset_id: i64,
}

async fn apply_asset_download(
    world_arc: Arc<RwLock<World>>,
    data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: AssetDownloadArgs,
) -> anyhow::Result<ToolResultFinal> {
    let asset = download_asset(args.asset_id).await?;
    // A picture is sized to its texture, which has to download before the world is locked.
    let object_id = if let AssetData::Texture(texture_data_id) = asset.data_json {
        let picture_size = picture_size_of_texture(&data_fetcher, texture_data_id).await;
        let mut world_lock = world_arc.write().await;
        let object_id = add_picture_to_world(&mut world_lock, asset.name, texture_data_id, picture_size);
        drop(world_lock);
        object_id
    } else {
        let mut world_lock = world_arc.write().await;
        let object_id = add_asset_to_world(&mut world_lock, asset.name, asset.data_json)?;
        drop(world_lock);
        object_id
    };
    record_asset_import(args.asset_id, asset.version_num).await?;
    Ok(tool_succeeded(
        serde_json::json!({"object_id": object_id, "asset_id": args.asset_id}).to_string(),
    ))
}

// -------------------------- catalog --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CatalogItemPublishArgs {
    pub asset_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_listed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CatalogItemEditArgs {
    pub catalog_item_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_listed: Option<bool>,
}

// Both catalog tools hold names and descriptions to the same limits as the website.
fn validate_catalog_item_fields(name: Option<&mut String>, description: Option<&mut String>) -> anyhow::Result<()> {
    if let Some(name) = name {
        *name = name.trim().to_string();
        anyhow::ensure!(!name.is_empty() && name.chars().count() <= 100, "The name must be 1 to 100 characters.");
    }
    if let Some(description) = description {
        *description = description.trim().to_string();
        anyhow::ensure!(description.chars().count() <= 1000, "The description can be at most 1000 characters.");
    }
    Ok(())
}

impl CatalogItemPublishArgs {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        validate_catalog_item_fields(Some(&mut self.name), self.description.as_mut())?;
        anyhow::ensure!(self.asset_id > 0, "Asset id must be positive.");
        Ok(())
    }
}

// A catalog item is a static model, but a rigged mesh's geometry lists for every vertex the joints that move it, and is
// only drawn with a skeleton. The item is drawn from this static copy of that geometry instead.
pub fn static_copy_of_rigged_mesh_data(mesh_data: &MeshData) -> Option<MeshData> {
    let MeshData::SkinnedMeshData { vertices, indices, .. } = mesh_data else {
        return None;
    };
    Some(MeshData::StaticMeshData {
        vertices: vertices
            .iter()
            .map(|vertex| MeshVertexInternal {
                position: vertex.position,
                normal: vertex.normal,
                uv: vertex.uv,
                tangent: vertex.tangent,
                skinning: (),
            })
            .collect(),
        indices: indices.clone(),
    })
}

// if these are updated be sure to update them in the system prompt too
const MIN_HAT_WIDEST_SIDE: f32 = 0.5;
const MAX_HAT_WIDEST_SIDE: f32 = 2.0;
const MAX_HAT_CENTER_OFFSET: f32 = 0.25;
const MIN_HAT_BOTTOM: f32 = -0.6;
const MAX_HAT_BOTTOM: f32 = 0.1;
// Every player can wear a hat, so a hat has half the triangles a model may have, and 3 vertices for each.
const MAX_HAT_TRIANGLES: usize = 10_000;

// Every catalog item is a hat for the default character. Its origin is the point that sits on the top middle of the head,
// so the model has to be a hat's size and sit on that point.
pub fn check_catalog_item_is_a_hat(mesh: &DehydratedGltf, mesh_data_of_id: &HashMap<DataId, Arc<MeshData>>) -> anyhow::Result<()> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut vertex_count = 0;
    let mut triangle_count = 0;
    for dehydrated_mesh in &mesh.dehydrated_meshes {
        let mesh_data = mesh_data_of_id
            .get(&dehydrated_mesh.mesh_data_id)
            .ok_or_else(|| anyhow::anyhow!("Mesh data {} could not be read.", dehydrated_mesh.mesh_data_id))?;
        let positions = match mesh_data.as_ref() {
            MeshData::StaticMeshData { vertices, .. } => vertices.iter().map(|vertex| vertex.position).collect::<Vec<_>>(),
            MeshData::SkinnedMeshData { vertices, .. } => vertices.iter().map(|vertex| vertex.position).collect::<Vec<_>>(),
        };
        vertex_count += positions.len();
        triangle_count += match mesh_data.as_ref() {
            MeshData::StaticMeshData { indices, .. } | MeshData::SkinnedMeshData { indices, .. } => indices.len() / 3,
        };
        for position in positions {
            min = min.min(position * mesh.import_scale);
            max = max.max(position * mesh.import_scale);
        }
    }
    anyhow::ensure!(min.is_finite() && max.is_finite(), "The model has no vertices.");
    anyhow::ensure!(
        triangle_count <= MAX_HAT_TRIANGLES && vertex_count <= 3 * MAX_HAT_TRIANGLES,
        "Every player can wear a hat, so a hat can have at most {MAX_HAT_TRIANGLES} triangles and {} vertices. This model has {triangle_count} triangles and {vertex_count} vertices.",
        3 * MAX_HAT_TRIANGLES
    );
    let size = max - min;
    let widest_side = size.max_element();
    anyhow::ensure!(
        (MIN_HAT_WIDEST_SIDE..=MAX_HAT_WIDEST_SIDE).contains(&widest_side),
        "Catalog items are hats for the default character, whose head is a 1-unit cube. This model's widest side is {widest_side:.2} units, and a hat's must be from {MIN_HAT_WIDEST_SIDE} to {MAX_HAT_WIDEST_SIDE}."
    );
    let center = (min + max) * 0.5;
    anyhow::ensure!(
        center.x.abs() <= MAX_HAT_CENTER_OFFSET && center.z.abs() <= MAX_HAT_CENTER_OFFSET,
        "A hat's origin is the top middle of the head, so the model must be centered over it. Its middle is {:.2} units across and {:.2} deep from the origin, and may be at most {MAX_HAT_CENTER_OFFSET} each way.",
        center.x,
        center.z
    );
    anyhow::ensure!(
        (MIN_HAT_BOTTOM..=MAX_HAT_BOTTOM).contains(&min.y),
        "A hat's origin is the top of the head, so the model's lowest point must be from {MIN_HAT_BOTTOM} to {MAX_HAT_BOTTOM} units up from it. This model's lowest point is at {:.2}.",
        min.y
    );
    Ok(())
}

impl CatalogItemEditArgs {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.catalog_item_id > 0, "Catalog item id must be positive.");
        anyhow::ensure!(
            self.name.is_some() || self.description.is_some() || self.is_listed.is_some(),
            "catalog_item_edit requires at least one change."
        );
        validate_catalog_item_fields(self.name.as_mut(), self.description.as_mut())
    }
}

async fn apply_catalog_item_publish(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CatalogItemPublishArgs,
) -> anyhow::Result<ToolResultFinal> {
    let handler_arc = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(handler_arc.catalog_handler.publish(args).await?))
}

async fn apply_catalog_item_edit(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CatalogItemEditArgs,
) -> anyhow::Result<ToolResultFinal> {
    let handler_arc = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(handler_arc.catalog_handler.edit(args).await?))
}

// -------------------------- brick creation --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateBrickArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub center: Vec3Args,
    pub size: Vec3Args,
    pub rotation_degrees: Option<Vec3Args>,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateBoxBetweenCornersArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub corner_a: Vec3Args,
    pub corner_b: Vec3Args,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateWallArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub start: Vec3Args,
    pub end: Vec3Args,
    pub height: f32,
    pub thickness: f32,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateBrickBetweenPointsArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub start: Vec3Args,
    pub end: Vec3Args,
    pub width: f32,
    pub height: f32,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateBallArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub center: Vec3Args,
    pub radius: f32,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateCylinderArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub center: Vec3Args,
    pub radius: f32,
    pub length: f32,
    pub rotation_degrees: Option<Vec3Args>,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateLabelArgs {
    pub parent_id: ObjectId,
    pub name: String,
    /// The text to show. Newlines start a new row.
    pub text: String,
    /// The objectId of the Ball, Brick, Cylinder, or RigidGroup the label follows. A label needs one to be drawn.
    pub follows_object_id: ObjectId,
    /// Blocks from the followed object's center to the label's center. Defaults to 2 blocks straight up.
    pub offset: Option<Vec3Args>,
    /// How many blocks tall one row of text is. Defaults to 0.5.
    pub line_height: Option<f32>,
    /// Wrap the text at this many blocks wide. Leave unset to only break on newlines.
    pub max_width: Option<f32>,
    /// Color of a panel drawn behind the text. Leave unset for no panel.
    pub background_color: Option<Color3Args>,
    /// Draw the label even when world geometry is in front of it. Useful for nameplates.
    pub is_always_on_top: Option<bool>,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateLabelImageArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub texture_data_id: DataId,
    /// Blocks wide. The picture stretches to fill width by height, so match its aspect ratio.
    pub width: f32,
    /// Blocks tall.
    pub height: f32,
    /// The objectId of the Ball, Brick, Cylinder, or RigidGroup the picture follows. A picture needs one to be drawn.
    pub follows_object_id: ObjectId,
    /// Blocks from the followed object's center to the picture's center. Defaults to 2 blocks straight up.
    pub offset: Option<Vec3Args>,
    /// Draw the picture even when world geometry is in front of it.
    pub is_always_on_top: Option<bool>,
    pub opacity: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CreateCylinderBetweenPointsArgs {
    pub parent_id: ObjectId,
    pub name: String,
    pub start: Vec3Args,
    pub end: Vec3Args,
    pub radius: f32,
    pub color: Option<Color3Args>,
    pub opacity: Option<f32>,
}

type Bounds3D = ([f32; 2], [f32; 2], [f32; 2]);

fn bounds_of_brick(position: Vec3, rotation: Quat, size: Vec3) -> ([f32; 2], [f32; 2], [f32; 2]) {
    let axis_x = rotation * Vec3::X;
    let axis_y = rotation * Vec3::Y;
    let axis_z = rotation * Vec3::Z;
    let half_size = 0.5 * size;
    let corner_offset_x = axis_x * half_size.x;
    let corner_offset_y = axis_y * half_size.y;
    let corner_offset_z = axis_z * half_size.z;

    let vertices = [
        position - corner_offset_x - corner_offset_y - corner_offset_z,
        position + corner_offset_x - corner_offset_y - corner_offset_z,
        position + corner_offset_x + corner_offset_y - corner_offset_z,
        position - corner_offset_x + corner_offset_y - corner_offset_z,
        position - corner_offset_x - corner_offset_y + corner_offset_z,
        position + corner_offset_x - corner_offset_y + corner_offset_z,
        position + corner_offset_x + corner_offset_y + corner_offset_z,
        position - corner_offset_x + corner_offset_y + corner_offset_z,
    ];

    let mut min_x = vertices[0].x;
    let mut max_x = vertices[0].x;
    let mut min_y = vertices[0].y;
    let mut max_y = vertices[0].y;
    let mut min_z = vertices[0].z;
    let mut max_z = vertices[0].z;

    for vertex in vertices.iter().skip(1) {
        min_x = min_x.min(vertex.x);
        max_x = max_x.max(vertex.x);
        min_y = min_y.min(vertex.y);
        max_y = max_y.max(vertex.y);
        min_z = min_z.min(vertex.z);
        max_z = max_z.max(vertex.z);
    }

    ([min_x, max_x], [min_y, max_y], [min_z, max_z])
}

fn bounds_of_ball(center: Vec3, radius: f32) -> Bounds3D {
    (
        [center.x - radius, center.x + radius],
        [center.y - radius, center.y + radius],
        [center.z - radius, center.z + radius],
    )
}

fn bounds_of_cylinder(position: Vec3, rotation: Quat, radius: f32, length: f32) -> Bounds3D {
    let axis_z = rotation * Vec3::Z;
    let half_length = 0.5 * length;
    let axis_x_offset = axis_z.x.abs() * half_length + radius * (1.0 - axis_z.x * axis_z.x).max(0.0).sqrt();
    let axis_y_offset = axis_z.y.abs() * half_length + radius * (1.0 - axis_z.y * axis_z.y).max(0.0).sqrt();
    let axis_z_offset = axis_z.z.abs() * half_length + radius * (1.0 - axis_z.z * axis_z.z).max(0.0).sqrt();

    (
        [position.x - axis_x_offset, position.x + axis_x_offset],
        [position.y - axis_y_offset, position.y + axis_y_offset],
        [position.z - axis_z_offset, position.z + axis_z_offset],
    )
}

fn rotation_from_degrees(rotation_degrees: Option<Vec3Args>) -> Quat {
    euler_degrees_to_quat(rotation_degrees.map(Vec3::from).unwrap_or(Vec3::ZERO))
}

fn physical_visual_settings(color: Option<Color3Args>, opacity: Option<f32>, tool_name: &str) -> anyhow::Result<VisualSettings> {
    let color = color.map(Color3::from).unwrap_or(Color3::DEFAULT);
    if color.r < 0.0 || color.r > 1.0 || color.g < 0.0 || color.g > 1.0 || color.b < 0.0 || color.b > 1.0 {
        return Err(anyhow::anyhow!("{tool_name} color values must each be within [0, 1]"));
    }

    let opacity = opacity.unwrap_or(1.0);
    if opacity < 0.0 || opacity > 1.0 {
        return Err(anyhow::anyhow!("{tool_name} opacity must be within [0, 1]"));
    }

    Ok(VisualSettings {
        opacity,
        color,
        glow: 0.0,
        isAlwaysOnTop: false,
    })
}

fn physical_info(position: Vec3, rotation: Quat) -> PhysicsInfo3D {
    PhysicsInfo3D {
        location: Location3D {
            position,
            quaternion: rotation,
        },
        movement: Movement3D {
            velocity: Vec3::ZERO,
            angularVelocity: Vec3::ZERO,
        },
    }
}

fn physical_toggles() -> PhysicsToggles3D {
    PhysicsToggles3D {
        isWalkthrough: false,
        isPushable: false,
        density: 1.0,
    }
}

fn created_physical_object_result_json(world: &mut World, object_id: ObjectId, message: String, bounds: Bounds3D) -> anyhow::Result<Value> {
    let (bounds_x, bounds_y, bounds_z) = bounds;
    let result_json = json!({
        "message": message,
        "object": object_summary_json(world, object_id)?,
        "bounds": {
            "x": bounds_x,
            "y": bounds_y,
            "z": bounds_z,
        },
    });
    // if let Some(warning_text) = compute_placement_warnings_text(world)? {
    //     result_json["warning"] = json!(warning_text);
    // }

    Ok(result_json)
}

fn create_brick_object(
    world: &mut World,
    parent_id: ObjectId,
    part_name: String,
    part_position: Vec3,
    part_rotation: Quat,
    part_size: Vec3,
    visual_settings: VisualSettings,
) -> anyhow::Result<Value> {
    if part_size.x <= EPSILON || part_size.y <= EPSILON || part_size.z <= EPSILON {
        return Err(anyhow::anyhow!("brick size values must each be > 0"));
    }

    let brick_info = BrickInfo {
        material: None,
        visualSettings: visual_settings,
    };
    let brick_primitive_settings = BrickPrimitiveSettings { size: part_size };

    let brick_object_id = world.addObject(
        part_name,
        ObjectBeingAdded::Brick(
            brick_info,
            brick_primitive_settings,
            physical_info(part_position, part_rotation),
            physical_toggles(),
        ),
        parent_id,
    );

    created_physical_object_result_json(
        world,
        brick_object_id,
        format!("Created brick {}", brick_object_id),
        bounds_of_brick(part_position, part_rotation, part_size),
    )
}

fn create_ball_object(
    world: &mut World,
    parent_id: ObjectId,
    part_name: String,
    part_position: Vec3,
    radius: f32,
    visual_settings: VisualSettings,
) -> anyhow::Result<Value> {
    if radius <= EPSILON {
        return Err(anyhow::anyhow!("ball radius must be > 0"));
    }

    let ball_info = BallInfo {
        material: None,
        visualSettings: visual_settings,
    };
    let ball_primitive_settings = BallPrimitiveSettings { radius };

    let ball_object_id = world.addObject(
        part_name,
        ObjectBeingAdded::Ball(
            ball_info,
            ball_primitive_settings,
            physical_info(part_position, Quat::IDENTITY),
            physical_toggles(),
        ),
        parent_id,
    );

    created_physical_object_result_json(
        world,
        ball_object_id,
        format!("Created ball {}", ball_object_id),
        bounds_of_ball(part_position, radius),
    )
}

const DEFAULT_LABEL_OFFSET: Vec3 = Vec3::new(0.0, 2.0, 0.0);
const DEFAULT_LABEL_BACKGROUND_OPACITY: f32 = 0.65;

fn create_cylinder_object(
    world: &mut World,
    parent_id: ObjectId,
    part_name: String,
    part_position: Vec3,
    part_rotation: Quat,
    radius: f32,
    length: f32,
    visual_settings: VisualSettings,
) -> anyhow::Result<Value> {
    if radius <= EPSILON {
        return Err(anyhow::anyhow!("cylinder radius must be > 0"));
    }
    if length <= EPSILON {
        return Err(anyhow::anyhow!("cylinder length must be > 0"));
    }

    let cylinder_info = CylinderInfo {
        material: None,
        visualSettings: visual_settings,
    };
    let cylinder_primitive_settings = CylinderPrimitiveSettings { radius, length };

    let cylinder_object_id = world.addObject(
        part_name,
        ObjectBeingAdded::Cylinder(
            cylinder_info,
            cylinder_primitive_settings,
            physical_info(part_position, part_rotation),
            physical_toggles(),
        ),
        parent_id,
    );

    created_physical_object_result_json(
        world,
        cylinder_object_id,
        format!("Created cylinder {}", cylinder_object_id),
        bounds_of_cylinder(part_position, part_rotation, radius, length),
    )
}

async fn apply_create_brick(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateBrickArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateBrickArgs {
        parent_id,
        name: part_name,
        center,
        size,
        rotation_degrees,
        color,
        opacity,
    } = args;

    let part_size: Vec3 = size.into();
    if part_size.x <= EPSILON || part_size.y <= EPSILON || part_size.z <= EPSILON {
        return Err(anyhow::anyhow!("create_brick size values must each be > 0"));
    }
    let part_rotation = rotation_from_degrees(rotation_degrees);
    let visual_settings = physical_visual_settings(color, opacity, "create_brick")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_brick_object(world, parent_id, part_name, center.into(), part_rotation, part_size, visual_settings)?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_box_between_corners(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateBoxBetweenCornersArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateBoxBetweenCornersArgs {
        parent_id,
        name: part_name,
        corner_a,
        corner_b,
        color,
        opacity,
    } = args;

    let corner_a: Vec3 = corner_a.into();
    let corner_b: Vec3 = corner_b.into();
    let min_corner = corner_a.min(corner_b);
    let max_corner = corner_a.max(corner_b);
    let part_size = max_corner - min_corner;
    if part_size.x <= EPSILON || part_size.y <= EPSILON || part_size.z <= EPSILON {
        return Err(anyhow::anyhow!(
            "create_box_between_corners corners must define a non-zero brick size on x, y, and z axes"
        ));
    }
    let visual_settings = physical_visual_settings(color, opacity, "create_box_between_corners")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_brick_object(
        world,
        parent_id,
        part_name,
        0.5 * (corner_a + corner_b),
        Quat::IDENTITY,
        part_size,
        visual_settings,
    )?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_wall(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateWallArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateWallArgs {
        parent_id,
        name: part_name,
        start,
        end,
        height,
        thickness,
        color,
        opacity,
    } = args;

    if height <= EPSILON {
        return Err(anyhow::anyhow!("create_wall height must be > 0"));
    }
    if thickness <= EPSILON {
        return Err(anyhow::anyhow!("create_wall thickness must be > 0"));
    }

    let start: Vec3 = start.into();
    let end: Vec3 = end.into();
    if (start.y - end.y).abs() > EPSILON {
        return Err(anyhow::anyhow!("create_wall start.y and end.y must match because walls are vertical"));
    }

    let wall_delta = end - start;
    let wall_direction = Vec3::new(wall_delta.x, 0.0, wall_delta.z);
    let wall_length = wall_direction.length();
    if wall_length <= EPSILON {
        return Err(anyhow::anyhow!("create_wall start and end must be different in the XZ plane"));
    }

    let axis_x = wall_direction / wall_length;
    let axis_y = Vec3::Y;
    let axis_z = axis_x.cross(axis_y);
    let part_rotation = Quat::from_mat3(&Mat3::from_cols(axis_x, axis_y, axis_z)).normalize();
    let part_position = 0.5 * (start + end) + Vec3::Y * (0.5 * height);
    let part_size = Vec3::new(wall_length, height, thickness);
    let visual_settings = physical_visual_settings(color, opacity, "create_wall")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_brick_object(world, parent_id, part_name, part_position, part_rotation, part_size, visual_settings)?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_brick_between_points(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateBrickBetweenPointsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateBrickBetweenPointsArgs {
        parent_id,
        name: part_name,
        start,
        end,
        width,
        height,
        color,
        opacity,
    } = args;

    if width <= EPSILON {
        return Err(anyhow::anyhow!("create_brick_between_points width must be > 0"));
    }
    if height <= EPSILON {
        return Err(anyhow::anyhow!("create_brick_between_points height must be > 0"));
    }

    let start: Vec3 = start.into();
    let end: Vec3 = end.into();
    let part_delta = end - start;
    let length = part_delta.length();
    if length <= EPSILON {
        return Err(anyhow::anyhow!("create_brick_between_points start and end must be different"));
    }

    let axis_z = part_delta / length;
    let up_hint = if axis_z.cross(Vec3::Y).length_squared() > EPSILON * EPSILON {
        Vec3::Y
    } else {
        Vec3::X
    };
    let axis_x = up_hint.cross(axis_z).normalize();
    let axis_y = axis_z.cross(axis_x).normalize();
    let part_rotation = Quat::from_mat3(&Mat3::from_cols(axis_x, axis_y, axis_z)).normalize();
    let part_position = 0.5 * (start + end);
    let part_size = Vec3::new(width, height, length);
    let visual_settings = physical_visual_settings(color, opacity, "create_brick_between_points")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_brick_object(world, parent_id, part_name, part_position, part_rotation, part_size, visual_settings)?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_ball(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateBallArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateBallArgs {
        parent_id,
        name: part_name,
        center,
        radius,
        color,
        opacity,
    } = args;

    if radius <= EPSILON {
        return Err(anyhow::anyhow!("create_ball radius must be > 0"));
    }
    let visual_settings = physical_visual_settings(color, opacity, "create_ball")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_ball_object(world, parent_id, part_name, center.into(), radius, visual_settings)?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_cylinder(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateCylinderArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateCylinderArgs {
        parent_id,
        name: part_name,
        center,
        radius,
        length,
        rotation_degrees,
        color,
        opacity,
    } = args;

    if radius <= EPSILON {
        return Err(anyhow::anyhow!("create_cylinder radius must be > 0"));
    }
    if length <= EPSILON {
        return Err(anyhow::anyhow!("create_cylinder length must be > 0"));
    }
    let part_rotation = rotation_from_degrees(rotation_degrees);
    let visual_settings = physical_visual_settings(color, opacity, "create_cylinder")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_cylinder_object(world, parent_id, part_name, center.into(), part_rotation, radius, length, visual_settings)?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_label(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateLabelArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateLabelArgs {
        parent_id,
        name: label_name,
        text,
        follows_object_id,
        offset,
        line_height,
        max_width,
        background_color,
        is_always_on_top,
        color,
        opacity,
    } = args;

    if text.is_empty() {
        return Err(anyhow::anyhow!("create_label text must not be empty"));
    }
    let line_height = line_height.unwrap_or(LabelInfo::DEFAULT_LINE_HEIGHT);
    if line_height <= EPSILON {
        return Err(anyhow::anyhow!("create_label line_height must be > 0"));
    }
    if let Some(max_width) = max_width {
        if max_width <= EPSILON {
            return Err(anyhow::anyhow!("create_label max_width must be > 0 when set"));
        }
    }

    let mut visual_settings = physical_visual_settings(color, opacity, "create_label")?;
    visual_settings.isAlwaysOnTop = is_always_on_top.unwrap_or(false);

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    // Fail loudly rather than creating a label that never draws.
    let followed_object = world.getObject(follows_object_id)?;
    if world.getObjectPhysicsInfo3D(follows_object_id).is_none() {
        return Err(anyhow::anyhow!(
            "create_label follows_object_id {} is a {}, which has no position. Labels can only follow a ball, brick, cylinder, or rigid group.",
            follows_object_id,
            ObjectKind::from_object_info(&followed_object.info).to_object_kind_name(),
        ));
    }

    let label_info = LabelInfo {
        text,
        lineHeight: line_height,
        maxWidth: max_width,
        horizontalAlign: LabelHorizontalAlign::Center,
        backgroundColor: background_color.map(|background_color| background_color.into()),
        backgroundOpacity: DEFAULT_LABEL_BACKGROUND_OPACITY,
        offsetFromObject: offset.map(Vec3::from).unwrap_or(DEFAULT_LABEL_OFFSET),
        rotationFromObject: Quat::IDENTITY,
        // Text that does not face the camera is almost never what create_label was asked for.
        billboardMode: BillboardMode::Full,
        isConstantSize: false,
        physicalObjectId: Some(ObjectIdPointer(follows_object_id)),
        visualSettings: visual_settings,
    };
    let label_object_id = world.addObject(label_name, ObjectBeingAdded::Label(label_info), parent_id);

    let result_json = json!({
        "message": format!("Created label {}", label_object_id),
        "object": object_summary_json(world, label_object_id)?,
    });
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_label_image(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateLabelImageArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateLabelImageArgs {
        parent_id,
        name: label_image_name,
        texture_data_id,
        width,
        height,
        follows_object_id,
        offset,
        is_always_on_top,
        opacity,
    } = args;

    if width <= EPSILON || height <= EPSILON {
        return Err(anyhow::anyhow!("create_label_image width and height must be > 0"));
    }

    let mut visual_settings = physical_visual_settings(None, opacity, "create_label_image")?;
    visual_settings.isAlwaysOnTop = is_always_on_top.unwrap_or(false);

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    // Fail loudly rather than creating a picture that never draws.
    let followed_object = world.getObject(follows_object_id)?;
    if world.getObjectPhysicsInfo3D(follows_object_id).is_none() {
        return Err(anyhow::anyhow!(
            "create_label_image follows_object_id {} is a {}, which has no position. Pictures can only follow a ball, brick, cylinder, or rigid group.",
            follows_object_id,
            ObjectKind::from_object_info(&followed_object.info).to_object_kind_name(),
        ));
    }

    let label_image_info = LabelImageInfo {
        textureDataId: texture_data_id,
        width,
        height,
        offsetFromObject: offset.map(Vec3::from).unwrap_or(DEFAULT_LABEL_OFFSET),
        rotationFromObject: Quat::IDENTITY,
        physicalObjectId: Some(ObjectIdPointer(follows_object_id)),
        billboardMode: BillboardMode::Full,
        isConstantSize: false,
        visualSettings: visual_settings,
    };
    let label_image_object_id = world.addObject(label_image_name, ObjectBeingAdded::LabelImage(label_image_info), parent_id);

    let result_json = json!({
        "message": format!("Created label image {}", label_image_object_id),
        "object": object_summary_json(world, label_image_object_id)?,
    });
    Ok(tool_succeeded(message_of_json(result_json)?))
}

async fn apply_create_cylinder_between_points(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CreateCylinderBetweenPointsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let CreateCylinderBetweenPointsArgs {
        parent_id,
        name: part_name,
        start,
        end,
        radius,
        color,
        opacity,
    } = args;

    if radius <= EPSILON {
        return Err(anyhow::anyhow!("create_cylinder_between_points radius must be > 0"));
    }

    let start: Vec3 = start.into();
    let end: Vec3 = end.into();
    let part_delta = end - start;
    let length = part_delta.length();
    if length <= EPSILON {
        return Err(anyhow::anyhow!("create_cylinder_between_points start and end must be different"));
    }
    let part_position = 0.5 * (start + end);
    let part_rotation = Quat::from_rotation_arc(Vec3::Z, part_delta / length);
    let visual_settings = physical_visual_settings(color, opacity, "create_cylinder_between_points")?;

    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let result_json = create_cylinder_object(world, parent_id, part_name, part_position, part_rotation, radius, length, visual_settings)?;
    Ok(tool_succeeded(message_of_json(result_json)?))
}

// -------------------------- delete object --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct DeleteObjectArgs {
    pub object_id: ObjectId,
}

async fn apply_delete_object(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: DeleteObjectArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let (deleted_object_name, deleted_object_type, deleted_object_info) = {
        let object = world.getObject(args.object_id)?;
        (
            object.name.clone(),
            object_kind_name_of_object_info(&object.info).to_string(),
            serde_json::to_value(&object.info)?,
        )
    };

    world.removeObject(args.object_id)?;

    Ok(tool_succeeded(message_of_json(json!({
        "message": format!("Successfully deleted object {}", args.object_id),
        "object_id": args.object_id,
        "name": deleted_object_name,
        "type": deleted_object_type,
        "info": deleted_object_info,
    }))?))
}

// -------------------------- ls --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct LsArgs {
    pub in_object_id: ObjectId,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub depth: Option<usize>,
}

async fn apply_ls(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: LsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let in_object_id = args.in_object_id;
    let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
    let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);
    let depth = args.depth.unwrap_or(TOOL_DEFAULT_DEPTH);

    world.getObject(in_object_id)?;

    fn sorted_child_ids(world: &World, object_id: ObjectId) -> anyhow::Result<Vec<ObjectId>> {
        let mut child_ids = world.getDirectChildrenIds(object_id)?.iter().copied().collect::<Vec<_>>();
        child_ids.sort_unstable();
        Ok(child_ids)
    }

    fn build_ls_node(world: &World, object_id: ObjectId, shown_child_ids_of_id: &HashMap<ObjectId, Vec<ObjectId>>) -> anyhow::Result<Value> {
        let object = world.getObject(object_id)?;
        let children = shown_child_ids_of_id
            .get(&object_id)
            .into_iter()
            .flatten()
            .map(|child_id| build_ls_node(world, *child_id, shown_child_ids_of_id))
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(json!({
            "object_id": object_id,
            "name": object.name,
            "type": object_kind_name_of_object_info(&object.info),
            "child_count": world.getDirectChildrenIds(object_id)?.len(),
            "children": children,
        }))
    }

    let root_child_ids = sorted_child_ids(world, in_object_id)?;

    let total = root_child_ids.len();
    let page_ids = root_child_ids.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
    let has_more = offset.saturating_add(page_ids.len()) < total;

    // `limit` counts every object in the answer, so each level fills before the one below it, until the limit runs out.
    let mut shown_child_ids_of_id = HashMap::<ObjectId, Vec<ObjectId>>::new();
    let mut shown_count = page_ids.len();
    let mut level_ids = page_ids.clone();
    'levels: for _ in 1..depth {
        let mut next_level_ids = Vec::new();
        for object_id in level_ids {
            for child_id in sorted_child_ids(world, object_id)? {
                if shown_count >= limit {
                    break 'levels;
                }
                shown_count += 1;
                shown_child_ids_of_id.entry(object_id).or_default().push(child_id);
                next_level_ids.push(child_id);
            }
        }
        level_ids = next_level_ids;
    }

    let children = page_ids
        .into_iter()
        .map(|child_id| build_ls_node(world, child_id, &shown_child_ids_of_id))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let message = message_of_json(json!({
        "in_object_id": in_object_id,
        "offset": offset,
        "limit": limit,
        "depth": depth,
        "results": children,
        "more_than_limit": has_more,
        "total_in_parent": total,
    }))?;

    Ok(tool_succeeded(message))
}

// -------------------------- inspect --------------------------

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
pub enum InspectMode {
    Info,
    Physics,
    All,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct InspectArgs {
    pub object_id: ObjectId,
    pub mode: Option<InspectMode>,
}

async fn apply_inspect(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: InspectArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let mode = args.mode.unwrap_or(InspectMode::All);
    let object = world.getObject(args.object_id)?;

    // parent
    let parent = match world.parentOfId.get(&args.object_id).copied() {
        Some(parent_id) => Some(object_summary_json(world, parent_id)?),
        None => None,
    };

    // all children
    let mut child_ids = world.getDirectChildrenIds(args.object_id)?.iter().copied().collect::<Vec<_>>();
    child_ids.sort_unstable();
    let child_count = child_ids.len();
    let children = child_ids
        .into_iter()
        .take(TOOL_DEFAULT_LIMIT)
        .map(|child_id| object_summary_json(world, child_id))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // pointers to this
    let mut pointer_ids = world
        ._objectsThatPointToId
        .get(&args.object_id)
        .map(|object_ids| object_ids.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    pointer_ids.sort_unstable();
    let objects_that_point_to_this_count = pointer_ids.len();
    let objects_that_point_to_this = pointer_ids
        .into_iter()
        .take(TOOL_DEFAULT_LIMIT)
        .map(|pointer_id| object_summary_json(world, pointer_id))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // data ids that this object points to if it's the right type
    let data_ids = data_ids_of_object_info(&object.info);

    // type
    let object_kind = object_kind_name_of_object_info(&object.info);

    let mut inspect_json = json!({
        "object_id": args.object_id,
        "name": object.name,
        "type": object_kind,
        "parent": parent,
        "child_count": child_count,
        "children": children,
        "objects_that_point_to_this_count": objects_that_point_to_this_count,
        "objects_that_point_to_this": objects_that_point_to_this,
        "data_ids_this_points_to": data_ids,
    });

    let should_include_info = match mode {
        InspectMode::Info | InspectMode::All => true,
        InspectMode::Physics => false,
    };
    if should_include_info {
        inspect_json["info"] = serde_json::to_value(&object.info)?;
    }

    let should_include_physics = match mode {
        InspectMode::Physics | InspectMode::All => true,
        InspectMode::Info => false,
    };
    if should_include_physics {
        inspect_json["physics"] = serde_json::to_value(world.getObjectPhysicsInfo3D(args.object_id))?;
    }

    Ok(tool_succeeded(message_of_json(inspect_json)?))
}

// -------------------------- script raw code --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct ReadScriptRawCodeArgs {
    pub object_id: ObjectId,
    pub start_line: Option<usize>,
    pub line_count: Option<usize>,
}

async fn apply_read_script_raw_code(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: ReadScriptRawCodeArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;

    let ObjectInfo::Script(_) = &world.getObject(args.object_id)?.info else {
        return Err(anyhow::anyhow!("read_script_raw_code requires a script object"));
    };
    let script_raw_code = world
        .scriptRawCodeOfId
        .get(&args.object_id)
        .ok_or_else(|| anyhow::anyhow!("Script raw code not found for object {}", args.object_id))?;
    let name = &world.getObject(args.object_id)?.name;

    // Lines are numbered like `cat -n`, the same numbers TypeScript warnings use.
    let lines = script_raw_code.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Ok(tool_succeeded(format!("Script {} \"{name}\" is empty.", args.object_id)));
    }
    let first_line = args.start_line.unwrap_or(1);
    anyhow::ensure!(
        (1..=lines.len()).contains(&first_line),
        "start_line must be from 1 to {}, the script's last line.",
        lines.len()
    );
    let line_count = args.line_count.unwrap_or(lines.len());
    anyhow::ensure!(line_count >= 1, "line_count must be at least 1.");
    let last_line = (first_line - 1).saturating_add(line_count).min(lines.len());
    let number_width = last_line.to_string().len();
    let mut text = format!("Script {} \"{name}\", lines {first_line}-{last_line} of {}:\n", args.object_id, lines.len());
    for (line_index, line) in lines.iter().enumerate().take(last_line).skip(first_line - 1) {
        text.push_str(&format!("{:>number_width$}\t{line}\n", line_index + 1));
    }
    Ok(tool_succeeded(text))
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct WriteScriptRawCodeArgs {
    pub object_id: ObjectId,
    pub script_raw_code: String,
}

async fn apply_write_script_raw_code(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    editor_server_token: Option<String>,
    args: WriteScriptRawCodeArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let WriteScriptRawCodeArgs { object_id, script_raw_code } = args;

    let ObjectInfo::Script(_) = &world.getObject(object_id)?.info else {
        return Err(anyhow::anyhow!("write_script_raw_code requires a script object"));
    };
    world.setScriptRawCode(object_id, script_raw_code);

    let mut result_json = json!({
        "object": object_summary_json(world, object_id)?,
    });
    // check script warnings
    let script_path = world.getObjectNamePath(object_id)?;
    drop(world_lock);

    let script_warnings = if editor_server_token.is_some() {
        compute_script_warnings(&script_path).await?
    } else {
        None
    };
    if let Some(script_warnings) = script_warnings {
        result_json["script_warnings"] = script_warnings;
    }

    Ok(tool_succeeded(message_of_json(result_json)?))
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct EditScriptRawCodeArgs {
    pub object_id: ObjectId,
    pub old_string: String,
    pub new_string: String,
    pub replace_all: Option<bool>,
}

async fn apply_edit_script_raw_code(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    editor_server_token: Option<String>,
    args: EditScriptRawCodeArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let EditScriptRawCodeArgs {
        object_id,
        old_string,
        new_string,
        replace_all,
    } = args;

    let ObjectInfo::Script(_) = &world.getObject(object_id)?.info else {
        return Err(anyhow::anyhow!("edit_script_raw_code requires a script object"));
    };
    anyhow::ensure!(!old_string.is_empty(), "old_string must not be empty.");
    anyhow::ensure!(old_string != new_string, "old_string and new_string are the same.");
    let script_raw_code = world.scriptRawCodeOfId.get(&object_id).cloned().unwrap_or_default();
    let replacement_count = script_raw_code.matches(old_string.as_str()).count();
    anyhow::ensure!(
        replacement_count > 0,
        "old_string was not found in the script. It must match the code exactly, including indentation, without line numbers."
    );
    anyhow::ensure!(
        replacement_count == 1 || replace_all == Some(true),
        "old_string appears {replacement_count} times. Include more of the code around it so it appears once, or set replace_all to replace every one."
    );
    world.setScriptRawCode(object_id, script_raw_code.replace(old_string.as_str(), &new_string));

    let mut result_json = json!({
        "object": object_summary_json(world, object_id)?,
        "replacement_count": replacement_count,
    });
    let script_path = world.getObjectNamePath(object_id)?;
    drop(world_lock);

    let script_warnings = if editor_server_token.is_some() {
        compute_script_warnings(&script_path).await?
    } else {
        None
    };
    if let Some(script_warnings) = script_warnings {
        result_json["script_warnings"] = script_warnings;
    }

    Ok(tool_succeeded(message_of_json(result_json)?))
}

// -------------------------- run temporary script --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct RunTemporaryScriptArgs {
    pub script: String,
}

async fn apply_run_temporary_script(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: RunTemporaryScriptArgs,
) -> anyhow::Result<ToolResultFinal> {
    fn snapshot_of_object_id(world: &World, object_id: ObjectId) -> anyhow::Result<Value> {
        let object = world.getObject(object_id)?;
        Ok(json!({
            "summary": object_summary_json(world, object_id)?,
            "parent_id": world.parentOfId.get(&object_id),
            "info": &object.info,
            "physics": world.getObjectPhysicsInfo3D(object_id),
            "physics_toggles": world.getObjectPhysicsToggles3D(object_id),
            "primitive_settings": world.getObjectPrimitiveSettings(object_id),
            "script_raw_code": world.scriptRawCodeOfId.get(&object_id),
        }))
    }

    fn snapshot_of_world(world: &World) -> anyhow::Result<HashMap<ObjectId, Value>> {
        let mut snapshotOfObjectId = HashMap::new();
        for object_id in world.objectOfId.keys() {
            snapshotOfObjectId.insert(*object_id, snapshot_of_object_id(world, *object_id)?);
        }
        Ok(snapshotOfObjectId)
    }

    // A script can make thousands of objects, so the result only counts them by type, and the script logs any ids the AI needs.
    fn count_of_type(snapshotOfObjectId: &HashMap<ObjectId, Value>, object_ids: &[ObjectId]) -> BTreeMap<String, usize> {
        let mut count_of_type = BTreeMap::new();
        for object_id in object_ids {
            if let Some(object_type) = snapshotOfObjectId.get(object_id).and_then(|snapshot| snapshot["summary"]["type"].as_str()) {
                *count_of_type.entry(object_type.to_string()).or_insert(0) += 1;
            }
        }
        count_of_type
    }

    let script_raw_code = args.script;
    if script_raw_code.trim().is_empty() {
        return Err(anyhow::anyhow!("run_temporary_script script must not be empty"));
    }

    let mut world_lock = world_arc.write().await;
    // Placement warnings used to sync physics after every edit. Scene queries in the script need it, so it happens here now.
    world_lock.call_this_before_editor_query();
    let beforeSnapshotOfObjectId = snapshot_of_world(&world_lock)?;
    let temporary_script_result = runTemporaryScript(&mut world_lock, script_raw_code);
    let afterSnapshotOfObjectId = snapshot_of_world(&world_lock)?;

    let created_object_ids = afterSnapshotOfObjectId
        .keys()
        .filter(|object_id| !beforeSnapshotOfObjectId.contains_key(object_id))
        .copied()
        .collect::<Vec<_>>();
    let deleted_object_ids = beforeSnapshotOfObjectId
        .keys()
        .filter(|object_id| !afterSnapshotOfObjectId.contains_key(object_id))
        .copied()
        .collect::<Vec<_>>();
    let modified_object_ids = beforeSnapshotOfObjectId
        .iter()
        .filter_map(|(object_id, before_snapshot)| {
            let after_snapshot = afterSnapshotOfObjectId.get(object_id)?;
            if before_snapshot == after_snapshot {
                return None;
            }
            Some(*object_id)
        })
        .collect::<Vec<_>>();

    let mut result_json = json!({
        "created": count_of_type(&afterSnapshotOfObjectId, &created_object_ids),
        "deleted": count_of_type(&beforeSnapshotOfObjectId, &deleted_object_ids),
        "modified": count_of_type(&afterSnapshotOfObjectId, &modified_object_ids),
    });

    // if let Some(warning_text) = compute_placement_warnings_text(&mut world_lock)? {
    //     result_json["warning"] = json!(warning_text);
    // }
    drop(world_lock);

    match temporary_script_result {
        Ok(console_log_entries) => {
            result_json["message"] = json!("Temporary script finished.");
            result_json["console_logs"] = Value::Array(
                console_log_entries
                    .into_iter()
                    .map(|console_log_entry| {
                        json!({
                            "level": console_log_entry.log_level,
                            "message": console_log_entry.message,
                        })
                    })
                    .collect(),
            );
            Ok(tool_succeeded(message_of_json(result_json)?))
        }
        Err(error) => {
            result_json["message"] = json!("Temporary script failed after applying the changes counted in this result.");
            result_json["error"] = json!(error.to_string());
            Ok(tool_failed(message_of_json(result_json)?))
        }
    }
}

// -------------------------- grep --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct GrepArgs {
    pub name: Option<String>,
    pub object_type: Option<String>,
    pub parent_id: Option<ObjectId>,
    pub object_id_that_points: Option<ObjectId>,
    pub has_data_id: Option<DataId>,
    pub object_info_string_contains: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

async fn apply_grep(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: GrepArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let mut candidate_object_ids: Option<HashSet<ObjectId>> = None;

    let intersect_candidate_ids = |current_candidates: &mut Option<HashSet<ObjectId>>, next_candidates: HashSet<ObjectId>| {
        if let Some(current_candidates) = current_candidates {
            current_candidates.retain(|object_id| next_candidates.contains(object_id));
        } else {
            *current_candidates = Some(next_candidates);
        }
    };

    if let Some(object_type) = &args.object_type {
        let parsed_kind = ObjectKind::from_object_kind_name(object_type.as_str());
        let object_ids = if let Some(parsed_kind) = parsed_kind {
            world
                ._kindOfObjectId
                .get_keys(&parsed_kind)
                .map(|ids| ids.iter().copied().collect::<HashSet<_>>())
                .unwrap_or_default()
        } else {
            HashSet::new()
        };
        intersect_candidate_ids(&mut candidate_object_ids, object_ids);
    }

    if let Some(parent_id) = args.parent_id {
        let object_ids = world._childrenOfId.get(&parent_id).cloned().unwrap_or_default();
        intersect_candidate_ids(&mut candidate_object_ids, object_ids);
    }

    if let Some(target_id) = args.object_id_that_points {
        let object_ids = world._objectsThatPointToId.get(&target_id).cloned().unwrap_or_default();
        intersect_candidate_ids(&mut candidate_object_ids, object_ids);
    }

    let mut object_ids = if let Some(candidate_object_ids) = candidate_object_ids {
        candidate_object_ids.into_iter().collect::<Vec<_>>()
    } else {
        world.objectOfId.keys().copied().collect::<Vec<_>>()
    };

    if let Some(name_filter) = args.name {
        let name_filter = name_filter.to_lowercase();
        object_ids.retain(|object_id| {
            let Ok(object) = world.getObject(*object_id) else {
                return false;
            };
            object.name.to_lowercase().contains(&name_filter)
        });
    }

    if let Some(data_id_filter) = args.has_data_id {
        object_ids.retain(|object_id| {
            let Ok(object) = world.getObject(*object_id) else {
                return false;
            };
            object_uses_data_id(&object.info, data_id_filter)
        });
    }

    if let Some(info_string_filter) = args.object_info_string_contains {
        object_ids.retain(|object_id| {
            let Ok(object) = world.getObject(*object_id) else {
                return false;
            };
            let info_string = serde_json::to_string(&object.info).unwrap_or_default();
            info_string.contains(&info_string_filter)
        });
    }

    object_ids.sort_unstable();

    let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
    let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);
    let (paged_object_ids, has_more) = paginate_vec(object_ids, offset, limit);

    let results = paged_object_ids
        .into_iter()
        .map(|object_id| object_summary_json(world, object_id))
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(tool_succeeded(message_of_json(json!({
        "offset": offset,
        "limit": limit,
        "results": results,
        "more_than_limit": has_more,
    }))?))
}

// -------------------------- get path --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct GetPathArgs {
    pub object_id: ObjectId,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

async fn apply_get_path(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: GetPathArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let object = object_summary_json(world, args.object_id)?;

    let mut object_path = Vec::new();
    let mut object_id = args.object_id;
    object_path.push(object_id);

    while let Some(parent_id) = world.parentOfId.get(&object_id).copied() {
        object_path.push(parent_id);
        object_id = parent_id;
    }

    object_path.reverse();

    let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
    let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);
    let (paged_path, has_more) = paginate_vec(object_path, offset, limit);
    let path = paged_path
        .into_iter()
        .map(|path_object_id| object_summary_json(world, path_object_id))
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(tool_succeeded(message_of_json(json!({
        "object": object,
        "offset": offset,
        "limit": limit,
        "path": path,
        "more_than_limit": has_more,
    }))?))
}

// -------------------------- game settings --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct GetGameSettingsArgs {}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub enum GameVisibility {
    Private,
    Published,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct SetGameSettingsArgs {
    pub title: Option<String>,
    pub description: Option<String>,
    pub suggested_min_players: Option<i64>,
    pub max_players: Option<i64>,
    pub visibility: Option<GameVisibility>,
}

impl SetGameSettingsArgs {
    // Preserve partial updates: omitted/null fields stay unchanged, and an empty description clears it.
    pub fn validate(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.title.is_some()
                || self.description.is_some()
                || self.suggested_min_players.is_some()
                || self.max_players.is_some()
                || self.visibility.is_some(),
            "set_game_settings requires at least one setting."
        );
        if let Some(title) = &mut self.title {
            *title = title.trim().to_string();
            anyhow::ensure!(!title.is_empty(), "Game title is required.");
        }
        if let Some(description) = &mut self.description {
            *description = description.trim().to_string();
        }
        if let Some(suggested_min_players) = self.suggested_min_players {
            anyhow::ensure!(
                suggested_min_players > 0 && suggested_min_players <= i64::from(i32::MAX),
                "Suggested min players must be a positive database integer."
            );
        }
        if let Some(max_players) = self.max_players {
            anyhow::ensure!(
                max_players > 0 && max_players <= i64::from(i32::MAX),
                "Max players must be a positive database integer."
            );
        }
        if let (Some(suggested_min_players), Some(max_players)) = (self.suggested_min_players, self.max_players) {
            anyhow::ensure!(suggested_min_players <= max_players, "suggested_min_players can't be more than max_players.");
        }
        Ok(())
    }
}

async fn apply_get_game_settings(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    _args: GetGameSettingsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let handler_arc = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(handler_arc.game_settings_handler.get().await?))
}

async fn apply_set_game_settings(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: SetGameSettingsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let handler_arc = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(handler_arc.game_settings_handler.set(args).await?))
}

// -------------------------- fallback camera --------------------------

async fn apply_set_fallback_camera(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CaptureScreenshotArgs,
) -> anyhow::Result<ToolResultFinal> {
    let (position, look_at) = read_camera_points(&args.camera, ToolName::SetFallbackCamera)?;
    let direction = (look_at - position).normalize();
    let orientation = Orientation3D {
        theta: direction.x.atan2(direction.z),
        phi: (-direction.y).asin(),
    };
    world_arc.write().await.fallback_camera_location = CameraLocation { position, orientation };
    Ok(tool_succeeded("Set the fallback camera.".to_string()))
}

// -------------------------- publish --------------------------

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct PublishArgs {}

async fn apply_publish(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    _args: PublishArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(client_editor_tool_handlers.publish_handler.publish().await?))
}

// -------------------------- copy and paste --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
struct CopyAndPasteArgs {
    pub object_id: ObjectId,
    pub offset: Option<Vec3Args>,
    pub clone_name: Option<String>,
}

async fn apply_copy_and_paste(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CopyAndPasteArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let source_object_id = args.object_id;
    let offset = args.offset.map(Vec3::from).unwrap_or(Vec3::ZERO);
    let source_object = world.getObject(source_object_id)?;

    let new_name = args.clone_name.unwrap_or_else(|| format!("{}_Copy", source_object.name));
    let new_root_object_id = world.addClonedObjectRecursive(source_object_id, new_name)?;
    if offset.length_squared() > f32::EPSILON {
        world.setObjectPositionOffsetRecursive(new_root_object_id, offset, SetOrForce::Force)?;
    }
    let pasted_position = world
        .getObjectPhysicsInfo3D(new_root_object_id)
        .map(|physics_info| physics_info.location.position);

    Ok(tool_succeeded(message_of_json(json!({
        "message": format!("Successfully copied and pasted object {}", new_root_object_id),
        "source_object": object_summary_json(world, source_object_id)?,
        "object": object_summary_json(world, new_root_object_id)?,
        "offset": offset,
        "pasted_position": pasted_position,
    }))?))
}

// -------------------------- edit object --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct EditObjectArgs {
    pub object_id: ObjectId,
    pub jspath: String,
    /// A JSON string containing the value to write at `jspath`.
    pub jsvalue: String,
}

async fn apply_edit_object(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: EditObjectArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let EditObjectArgs { object_id, jspath, jsvalue } = args;

    fn path_of_jspath(jspath: &str) -> anyhow::Result<Vec<String>> {
        let parsed_jspath = match serde_json::from_str::<Value>(jspath) {
            Ok(parsed_jspath) => parsed_jspath,
            Err(_) => Value::String(jspath.to_string()),
        };

        match parsed_jspath {
            Value::Array(path_segments) => {
                let mut parsed_path = Vec::new();
                for path_segment in path_segments {
                    let Some(path_segment) = path_segment.as_str() else {
                        return Err(anyhow::anyhow!("edit_object jspath array must contain only strings"));
                    };
                    parsed_path.push(path_segment.to_string());
                }
                Ok(parsed_path)
            }
            Value::String(jspath_string) => {
                let parsed_path = jspath_string
                    .split('.')
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| segment.to_string())
                    .collect::<Vec<_>>();
                Ok(parsed_path)
            }
            _ => Err(anyhow::anyhow!("edit_object jspath must be an array of strings or a dot-separated string")),
        }
    }

    world.getObject(object_id)?;

    let path_segments = path_of_jspath(&jspath)?;
    if path_segments.is_empty() {
        return Err(anyhow::anyhow!("edit_object jspath cannot be empty"));
    }

    let applied_value = parse_json_value_from_string(&jsvalue, "jsvalue", "edit_object")?;
    let value_with_path = ValueWithPath::new(&applied_value, &path_segments);
    let objectModifyFlags = rustUpdateObject(world, object_id, value_with_path, SetOrForce::Force)?;
    world.serverMarkObjectAsModified(object_id, objectModifyFlags);

    let result_json = json!({
        "message": format!("Successfully edited object {}", object_id),
        "object": object_summary_json(world, object_id)?,
        "path": path_segments,
        "value": applied_value,
    });
    // let warning_text = match &world.getObject(object_id)?.info {
    //     ObjectInfo::Ball(_) | ObjectInfo::Brick(_) | ObjectInfo::Cylinder(_) => compute_placement_warnings_text(world)?,
    //     _ => None,
    // };
    // if let Some(warning_text) = warning_text {
    //     result_json["warning"] = json!(warning_text);
    // }

    Ok(tool_succeeded(message_of_json(result_json)?))
}

// -------------------------- ls data --------------------------
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
pub enum DataMode {
    Mesh,
    Sound,
    Texture,
    All,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct LsDataArgs {
    pub mode: Option<DataMode>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

async fn apply_ls_data(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: LsDataArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let mode = args.mode.unwrap_or(DataMode::All);
    let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
    let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);

    let mut data_entries = HashSet::new();
    for (_, object) in world.getAllObjects() {
        match &object.info {
            ObjectInfo::Material(material_info) => {
                if let Some(data_id) = material_info.albedoTextureDataId {
                    data_entries.insert((data_id, "texture"));
                }
                if let Some(data_id) = material_info.metallicTextureDataId {
                    data_entries.insert((data_id, "texture"));
                }
                if let Some(data_id) = material_info.roughnessTextureDataId {
                    data_entries.insert((data_id, "texture"));
                }
                if let Some(data_id) = material_info.normalTextureDataId {
                    data_entries.insert((data_id, "texture"));
                }
            }
            ObjectInfo::Mesh(mesh_info) => {
                data_entries.insert((mesh_info.meshDataId, "mesh"));
            }
            ObjectInfo::SoundPlayer(sound_player_info) => {
                data_entries.insert((sound_player_info.soundDataId, "sound"));
            }
            ObjectInfo::LabelImage(label_image_info) => {
                data_entries.insert((label_image_info.textureDataId, "texture"));
            }
            _ => {}
        }
    }

    let mut data_entries = data_entries.into_iter().collect::<Vec<_>>();
    data_entries.retain(|(_, data_type)| match (mode, *data_type) {
        (DataMode::All, _) | (DataMode::Mesh, "mesh") | (DataMode::Sound, "sound") | (DataMode::Texture, "texture") => true,
        _ => false,
    });

    data_entries.sort_unstable_by(|(data_id_a, data_type_a), (data_id_b, data_type_b)| data_id_a.cmp(data_id_b).then(data_type_a.cmp(data_type_b)));

    let (paged_entries, has_more) = paginate_vec(data_entries, offset, limit);
    let results = paged_entries
        .into_iter()
        .map(|(data_id, data_type)| {
            json!({
                "data_id": data_id,
                "type": data_type,
            })
        })
        .collect::<Vec<_>>();

    Ok(tool_succeeded(message_of_json(json!({
        "mode": mode,
        "offset": offset,
        "limit": limit,
        "results": results,
        "more_than_limit": has_more,
    }))?))
}

// -------------------------- inspect data --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct InspectDataArgs {
    pub data_id: DataId,
}

async fn apply_inspect_data(
    world_arc: Arc<RwLock<World>>,
    data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: InspectDataArgs,
) -> anyhow::Result<ToolResultFinal> {
    let data_id = args.data_id;
    let mut results = Vec::new();

    if let Some(mesh_data) = data_fetcher.getOrFetchMeshData(data_id) {
        let mesh_summary = match mesh_data.as_ref() {
            MeshData::SkinnedMeshData {
                vertices,
                indices,
                jointNameOfIdx,
                ..
            } => {
                json!({
                    "type": "mesh",
                    "is_skinned": true,
                    "num_vertices": vertices.len(),
                    "num_indices": indices.len(),
                    "num_joints": jointNameOfIdx.len(),
                })
            }
            MeshData::StaticMeshData { vertices, indices } => {
                json!({
                    "type": "mesh",
                    "is_skinned": false,
                    "num_vertices": vertices.len(),
                    "num_indices": indices.len(),
                    "num_joints": 0,
                })
            }
        };
        results.push(mesh_summary);
    }

    if let Some(texture_data) = data_fetcher.getOrFetchTextureData(data_id) {
        results.push(json!({
            "type": "texture",
            "width": texture_data.width,
            "height": texture_data.height,
            "mip_level_count": texture_data.mipLevelCount,
            "is_srgb": texture_data.isSrgb,
            "has_alpha": texture_data.hasAlpha,
            "byte_len": texture_data.ktx2Data.len(),
            "note": "Texture image preview is not available in this text tool result.",
        }));
    }

    if let Some(sound_data) = data_fetcher.getOrFetchSoundData(data_id) {
        results.push(json!({
            "type": "sound",
            "byte_len": sound_data.oggVorbisData.len(),
        }));
    }

    if results.is_empty() {
        let world_lock = world_arc.read().await;
        let world = &*world_lock;
        let mut referenced_types = HashSet::new();
        for (_, object) in world.getAllObjects() {
            match &object.info {
                ObjectInfo::Mesh(mesh_info) if mesh_info.meshDataId == data_id => {
                    referenced_types.insert("mesh");
                }
                ObjectInfo::Material(material_info) => {
                    let uses_texture = material_info.albedoTextureDataId == Some(data_id)
                        || material_info.metallicTextureDataId == Some(data_id)
                        || material_info.roughnessTextureDataId == Some(data_id)
                        || material_info.normalTextureDataId == Some(data_id);
                    if uses_texture {
                        referenced_types.insert("texture");
                    }
                }
                ObjectInfo::SoundPlayer(sound_player_info) if sound_player_info.soundDataId == data_id => {
                    referenced_types.insert("sound");
                }
                ObjectInfo::LabelImage(label_image_info) if label_image_info.textureDataId == data_id => {
                    referenced_types.insert("texture");
                }
                _ => {}
            }
        }

        let mut referenced_types = referenced_types.into_iter().collect::<Vec<_>>();
        referenced_types.sort_unstable();

        return Ok(tool_failed(message_of_json(json!({
            "data_id": data_id,
            "result": null,
            "referenced_types": referenced_types,
            "message": "Data is not currently cached in memory.",
        }))?));
    }

    Ok(tool_succeeded(message_of_json(json!({
        "data_id": data_id,
        "results": results,
    }))?))
}

// -------------------------- get warnings --------------------------

// // placement warnings = collision warnings + floating warnings
// fn compute_placement_warnings(world: &mut World) -> anyhow::Result<(Vec<Value>, Vec<Value>)> {
//     world.call_this_before_editor_query();
//     let overlap_warnings = world.getOverlapWarnings(-PLACEMENT_CONTACT_TOLERANCE);

//     let mut collision_warnings = Vec::new();
//     for (object_id_1, object_id_2, signed_distance) in overlap_warnings {
//         let collision_depth = (-signed_distance).max(0.0);
//         collision_warnings.push(json!({
//             "object": object_summary_json(world, object_id_1)?,
//             "colliding_with_object": object_summary_json(world, object_id_2)?,
//             "collision_depth": collision_depth,
//             "signed_distance": signed_distance,
//         }));
//     }

//     let mut floating_warnings = Vec::new();
//     for warning_object_id in world.getFloatingPlacementWarnings(PLACEMENT_CONTACT_TOLERANCE) {
//         floating_warnings.push(object_summary_json(world, warning_object_id)?);
//     }

//     Ok((collision_warnings, floating_warnings))
// }

// fn compute_placement_warnings_text(world: &mut World) -> anyhow::Result<Option<String>> {
//     let (collision_warnings, floating_warnings) = compute_placement_warnings(world)?;
//     let mut warning_sections = Vec::new();

//     if !collision_warnings.is_empty() {
//         warning_sections.push(format!("Placement collision warnings:\n{}", message_of_json(Value::Array(collision_warnings))?));
//     }
//     if !floating_warnings.is_empty() {
//         warning_sections.push(format!("Placement floating warnings:\n{}", message_of_json(Value::Array(floating_warnings))?));
//     }

//     if warning_sections.is_empty() {
//         return Ok(None);
//     }

//     Ok(Some(warning_sections.join("\n\n")))
// }

// TODO: AI-PICKED-VALUE: A script's warnings show its first 20 problems with their total, since one typo can set off hundreds of knock-on errors.
pub const MAX_LISTED_SCRIPT_WARNINGS: usize = 20;

// Each warning is one line like "22:15 error Cannot find name 'T'.", using the line numbers read_script_raw_code shows.
pub fn script_warnings_of_script_files_text(script_files_text: &str) -> anyhow::Result<Vec<Value>> {
    let script_files = serde_json::from_str::<Value>(script_files_text)?;
    let files = script_files["files"].as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(files
        .iter()
        .map(|file| {
            let diagnostics = file["diagnostics"].as_array().map(Vec::as_slice).unwrap_or_default();
            let warnings = diagnostics
                .iter()
                .take(MAX_LISTED_SCRIPT_WARNINGS)
                .map(|diagnostic| {
                    let message = diagnostic["message"]
                        .as_str()
                        .unwrap_or_default()
                        .lines()
                        .map(str::trim)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let severity = diagnostic["severity"].as_str().unwrap_or("error");
                    format!("{}:{} {severity} {message}", diagnostic["startLineNumber"], diagnostic["startColumn"])
                })
                .collect::<Vec<_>>();
            json!({
                "object_id": file["objectId"],
                "path": file["path"],
                "warning_count": diagnostics.len(),
                "warnings": warnings,
            })
        })
        .collect())
}

async fn compute_script_warnings(script_path: &str) -> anyhow::Result<Option<Value>> {
    // these are always attached automatically, so having no editor client just means none
    let Ok(client_editor_tool_handlers) = ClientEditorToolHandlers::get_curr() else {
        return Ok(None);
    };

    let script_warning_text = match client_editor_tool_handlers
        .script_files_handler
        .read_text(ReadScriptFilesArgs {
            path_substring: Some(script_path.to_string()),
            include_text: Some(false),
            include_diagnostics: Some(true),
            only_files_with_diagnostics: Some(true),
        })
        .await
    {
        Ok(script_warning_text) => script_warning_text,
        Err(error) => {
            log::warn!("Skipping automatic script warnings for {}: {}", script_path, error);
            return Ok(None);
        }
    };
    let script_warnings = match script_warnings_of_script_files_text(&script_warning_text) {
        Ok(script_warnings) => script_warnings,
        Err(error) => {
            log::warn!(
                "Skipping automatic script warnings for {} because the editor returned invalid JSON: {}",
                script_path,
                error
            );
            return Ok(None);
        }
    };
    if script_warnings.is_empty() {
        return Ok(None);
    }

    Ok(Some(Value::Array(script_warnings)))
}

// #[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
// pub struct GetPlacementWarningsArgs {}

// async fn apply_get_placement_warnings(
//     world_arc: Arc<RwLock<World>>,
//     _data_fetcher: DataFetcherArc,
//     _editor_server_token: Option<String>,
//     _args: GetPlacementWarningsArgs,
// ) -> anyhow::Result<ToolResultFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;
//     let (collision_warnings, floating_warnings) = compute_placement_warnings(world)?;
//     Ok(tool_succeeded(message_of_json(json!({
//         "collision_warnings": collision_warnings,
//         "floating_warnings": floating_warnings,
//     }))?))
// }

// -------------------------- move by recursively --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct MoveByRecursivelyArgs {
    pub object_id: ObjectId,
    pub translation: Vec3Args,
}

async fn apply_move_by_recursively(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: MoveByRecursivelyArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let translation = Vec3::from(args.translation);

    world.setObjectPositionOffsetRecursive(args.object_id, translation, SetOrForce::Force)?;

    Ok(tool_succeeded(message_of_json(json!({
        "message": format!("Successfully moved object {}", args.object_id),
        "object": object_summary_json(world, args.object_id)?,
        "translation": translation,
    }))?))
}

// -------------------------- move to recursively --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct MoveToRecursivelyArgs {
    pub object_id: ObjectId,
    pub target_point: Vec3Args,
}

async fn apply_move_to_recursively(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: MoveToRecursivelyArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let target_point = Vec3::from(args.target_point);
    let physics_info = world
        .getObjectPhysicsInfo3D(args.object_id)
        .ok_or_else(|| anyhow::anyhow!("move_to_recursively needs object {} to have a direct physics position", args.object_id))?;
    let current_position = physics_info.location.position;
    let translation = target_point - current_position;

    world.setObjectPositionOffsetRecursive(args.object_id, translation, SetOrForce::Force)?;

    Ok(tool_succeeded(message_of_json(json!({
        "message": format!("Successfully moved object {} to target point", args.object_id),
        "object": object_summary_json(world, args.object_id)?,
        "target_point": target_point,
        "translation": translation,
    }))?))
}

// -------------------------- scale recursively --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct ScaleRecursivelyArgs {
    pub object_id: ObjectId,
    pub scale_multiplier: f32,
    pub center_point: Option<Vec3Args>,
}

async fn apply_scale_recursively(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: ScaleRecursivelyArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let scale_multiplier = args.scale_multiplier;
    if scale_multiplier.abs() <= EPSILON {
        return Err(anyhow::anyhow!("scale_recursively scale_multiplier must have magnitude greater than 0"));
    }

    let center_point = match args.center_point {
        Some(center_point) => Vec3::from(center_point),
        None => world.getPivotPoint(args.object_id).ok_or_else(|| {
            anyhow::anyhow!(
                "scale_recursively needs center_point when object {} has no direct physics position",
                args.object_id
            )
        })?,
    };

    world.setObjectScaleRecursive(args.object_id, scale_multiplier, center_point, SetOrForce::Force)?;

    Ok(tool_succeeded(message_of_json(json!({
        "message": format!("Successfully scaled object {}", args.object_id),
        "object": object_summary_json(world, args.object_id)?,
        "scale_multiplier": scale_multiplier,
        "center_point": center_point,
    }))?))
}

// -------------------------- rotate recursively --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct RotateRecursivelyArgs {
    pub object_id: ObjectId,
    pub rotation_axis: Vec3Args,
    pub rotation_magnitude: f32,
    pub center_point: Option<Vec3Args>,
}

async fn apply_rotate_recursively(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: RotateRecursivelyArgs,
) -> anyhow::Result<ToolResultFinal> {
    let mut world_lock = world_arc.write().await;
    let world = &mut *world_lock;
    let rotation_axis = Vec3::from(args.rotation_axis);
    let rotation_axis_length = rotation_axis.length();
    if rotation_axis_length <= f32::EPSILON {
        return Err(anyhow::anyhow!("rotate_recursively rotation_axis must have magnitude greater than 0"));
    }
    let normalized_rotation_axis = rotation_axis / rotation_axis_length;
    let rotation_quaternion = Quat::from_axis_angle(normalized_rotation_axis, args.rotation_magnitude);

    let center_point = match args.center_point {
        Some(center_point) => Vec3::from(center_point),
        None => world.getPivotPoint(args.object_id).ok_or_else(|| {
            anyhow::anyhow!(
                "rotate_recursively needs center_point when object {} has no direct physics position",
                args.object_id
            )
        })?,
    };

    world.setObjectRotationRecursive(args.object_id, rotation_quaternion, center_point, SetOrForce::Force)?;

    Ok(tool_succeeded(message_of_json(json!({
        "message": format!("Successfully rotated object {}", args.object_id),
        "object": object_summary_json(world, args.object_id)?,
        "rotation_axis": normalized_rotation_axis,
        "rotation_magnitude": args.rotation_magnitude,
        "center_point": center_point,
    }))?))
}

// -------------------------- ai headless playtest --------------------------

// start server
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct StartAiHeadlessPlaytestServerArgs {}
async fn apply_start_ai_headless_playtest_server(
    world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    _args: StartAiHeadlessPlaytestServerArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;

    // the test server runs a snapshot of the world as it is right now, so take it before starting anything
    let world_lock = world_arc.read().await;
    let stored_world_string = world_lock.toStoredWorldString()?;
    drop(world_lock);

    Ok(tool_succeeded(
        client_editor_tool_handlers.headless_playtest_handler.start_server(stored_world_string).await?,
    ))
}

// start client
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct StartAiHeadlessPlaytestClientArgs {}
async fn apply_start_ai_headless_playtest_client(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    _args: StartAiHeadlessPlaytestClientArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(client_editor_tool_handlers.headless_playtest_handler.start_client().await?))
}

// stop client
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct StopAiHeadlessPlaytestClientArgs {
    pub client_id: u8,
}
async fn apply_stop_ai_headless_playtest_client(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: StopAiHeadlessPlaytestClientArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(
        client_editor_tool_handlers.headless_playtest_handler.stop_client(args.client_id).await?,
    ))
}

// read logs on server | client
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct ReadAiHeadlessPlaytestLogsArgs {
    pub after_seq: Option<usize>,
    pub max_bytes: Option<usize>,
    /// Sleep this long before reading, so the game has time to print more. This is a plain delay from now, not a point in the game's life. Max 10s.
    pub wait_ms: Option<u64>,
    /// Which runtime to read, written as "server" or "client_<client_id>" (for example "client_1"). Omit to read all.
    pub source: Option<String>,
}
async fn apply_read_ai_headless_playtest_logs(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: ReadAiHeadlessPlaytestLogsArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;

    let wait_ms = args.wait_ms.unwrap_or(0).min(PLAYTEST_MAX_WAIT_MS);
    if wait_ms > 0 {
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    }

    Ok(tool_succeeded(client_editor_tool_handlers.headless_playtest_handler.read_logs(args)?))
}

// screenshot a client
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct CaptureAiHeadlessPlaytestScreenshotArgs {
    pub client_id: u8,
}
async fn apply_capture_ai_headless_playtest_screenshot(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: CaptureAiHeadlessPlaytestScreenshotArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;
    let png_bytes = client_editor_tool_handlers
        .headless_playtest_handler
        .capture_screenshot(args.client_id, AI_SCREENSHOT_WIDTH, AI_SCREENSHOT_HEIGHT)
        .await?;
    Ok(ToolResultFinal {
        is_error: false,
        text: format!("What client_{} sees right now, including the UI its client scripts show.", args.client_id),
        images: vec![screenshot_for_ai(&png_bytes)?],
    })
}

// stop server (and all clients)
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, TS)]
pub struct StopAiHeadlessPlaytestAllArgs {}
async fn apply_stop_ai_headless_playtest_all(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    _args: StopAiHeadlessPlaytestAllArgs,
) -> anyhow::Result<ToolResultFinal> {
    let client_editor_tool_handlers = ClientEditorToolHandlers::get_curr()?;
    Ok(tool_succeeded(client_editor_tool_handlers.headless_playtest_handler.stop_all()))
}

// -------------------------- report issue --------------------------

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct ReportIssueArgs {
    pub description: String,
}

async fn apply_report_issue(
    _world_arc: Arc<RwLock<World>>,
    _data_fetcher: DataFetcherArc,
    _editor_server_token: Option<String>,
    args: ReportIssueArgs,
) -> anyhow::Result<ToolResultFinal> {
    let description = args.description;
    if description.trim().is_empty() {
        return Err(anyhow::anyhow!("report_issue description must not be empty"));
    }

    Ok(tool_succeeded(message_of_json(json!({
        "status": "reported",
        "message": "Issue reported. Tell the user what is missing or unclear before making more tool calls.",
        "description": description,
    }))?))
}

// -------------------------- run parallel --------------------------
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct RunParallelToolCallArgs {
    pub call_id: Option<String>,
    #[serde(alias = "tool")]
    pub tool_name: ToolName,
    pub args: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, TS)]
pub struct RunParallelArgs {
    pub tool_calls: Vec<RunParallelToolCallArgs>,
    pub timeout_ms: Option<u64>,
}

async fn apply_run_parallel(
    world_arc: Arc<RwLock<World>>,
    data_fetcher: DataFetcherArc,
    editor_server_token: Option<String>,
    args: RunParallelArgs,
) -> anyhow::Result<ToolResultFinal> {
    let num_total = args.tool_calls.len();
    let timeout_ms = args.timeout_ms.unwrap_or(TOOL_RUN_PARALLEL_DEFAULT_TIMEOUT_MS);
    let timeout_deadline = Instant::now() + Duration::from_millis(timeout_ms);

    let mut num_success = 0usize;
    let mut images = Vec::new();
    let mut result_of_subcall_index: Vec<Option<Value>> = vec![None; num_total];
    let mut pending_subcall_of_index = HashMap::new();
    let mut subtool_futures = FuturesUnordered::new();

    for (subcall_index, subcall) in args.tool_calls.into_iter().enumerate() {
        // TODO: AI-PICKED-VALUE: fallback call ids use run_parallel_{index} so each subcall result has a stable local identifier when omitted by the model.
        let call_id = subcall
            .call_id
            .and_then(|call_id| if call_id.trim().is_empty() { None } else { Some(call_id) })
            .unwrap_or_else(|| format!("run_parallel_{}", subcall_index));
        let tool_name = subcall.tool_name;
        if EDITOR_TOOL_REGISTRY.get_tool(tool_name).is_none() {
            result_of_subcall_index[subcall_index] = Some(json!({
                "call_id": call_id,
                "tool_name": tool_name.as_ref(),
                "status": "failed",
                "error": format!("Unknown tool name: {}", tool_name.as_ref()),
            }));
            continue;
        }

        let parsed_args = match parse_json_value_from_string(&subcall.args, "args", "run_parallel") {
            Ok(parsed_args) => parsed_args,
            Err(error) => {
                result_of_subcall_index[subcall_index] = Some(json!({
                    "call_id": call_id,
                    "tool_name": tool_name.as_ref(),
                    "status": "failed",
                    "error": error.to_string(),
                }));
                continue;
            }
        };
        let parsed_tool_call = ToolCall {
            name: tool_name.as_ref().to_string(),
            arguments: parsed_args,
        };

        pending_subcall_of_index.insert(subcall_index, (call_id.clone(), tool_name));
        let world_arc = world_arc.clone();
        let data_fetcher = data_fetcher.clone();
        let editor_server_token = editor_server_token.clone();
        subtool_futures.push(async move {
            let subtool_result = apply_editor_tool_call(world_arc, data_fetcher, editor_server_token, parsed_tool_call).await;
            (subcall_index, call_id, tool_name, subtool_result)
        });
    }

    // This section collects subtool results until every call finishes or the batch timeout expires.
    while !subtool_futures.is_empty() {
        let next_subtool = match timeout_at(timeout_deadline, subtool_futures.next()).await {
            Ok(next_subtool) => next_subtool,
            Err(_) => break,
        };
        let Some((subcall_index, call_id, tool_name, subtool_result)) = next_subtool else {
            break;
        };

        pending_subcall_of_index.remove(&subcall_index);
        let mut result = if subtool_result.is_error {
            json!({ "call_id": call_id, "tool_name": tool_name.as_ref(), "status": "failed", "error": subtool_result.text })
        } else {
            num_success += 1;
            json!({ "call_id": call_id, "tool_name": tool_name.as_ref(), "status": "success", "result": subtool_result.text })
        };
        if !subtool_result.images.is_empty() {
            let first_image = images.len() + 1;
            images.extend(subtool_result.images);
            result["image_positions"] = json!((first_image..=images.len()).collect::<Vec<_>>());
        }
        result_of_subcall_index[subcall_index] = Some(result);
    }

    let did_timeout = !pending_subcall_of_index.is_empty();
    for (subcall_index, (call_id, tool_name)) in pending_subcall_of_index.into_iter() {
        result_of_subcall_index[subcall_index] = Some(json!({
            "call_id": call_id,
            "tool_name": tool_name.as_ref(),
            "status": "failed",
            "error": format!("run_parallel timed out after {} ms", timeout_ms),
        }));
    }

    let text = message_of_json(json!({
        "results": result_of_subcall_index.into_iter().flatten().collect::<Vec<_>>(),
        "summary": format!("Completed {}/{} tool calls.", num_success, num_total),
        "timeout_ms": timeout_ms,
        "did_timeout": did_timeout,
    }))?;
    Ok(ToolResultFinal { is_error: false, text, images })
}

// tools ai doesnt really use (we might need to add physics shape casting to make these work):
// run_temporary_script
// get_in_region
// get_in_ray
// distance_to_surroundings_cartesian
// distance_between_objects // returns closest distance between objects. 0 if touching/colliding, closest possible distance from first object vertex to second object vertex.
// move_object_to_touch
// get_in_radius

// make_editor_tool_spec(
//     ToolName::RunTemporaryScript,
//     "Run a temporary editor script. This is currently safety-restricted on client.".to_string(),
//     apply_run_temporary_script,
// ),

// make_editor_tool_spec(ToolName::GetInRegion, "Find physical objects in an axis-aligned region.".to_string(), apply_get_in_region),

// make_editor_tool_spec(ToolName::GetInRay, "Find physical objects along a ray segment.".to_string(), apply_get_in_ray),

// make_editor_tool_spec(
//     ToolName::DistanceToSurroundingsCartesian,
//     "Shape-cast an object in Cartesian directions to find nearby surfaces.".to_string(),
//     apply_distance_to_surroundings_cartesian,
// ),

// make_editor_tool_spec(
//     ToolName::DistanceAndDirectionBetweenObjects,
//     "Measure distance and direction between two objects.".to_string(),
//     apply_distance_and_direction_between_objects,
// ),

// make_editor_tool_spec(
//     ToolName::MoveObjectToTouch,
//     "Move an object and its physical descendants so it touches another object. This tool fails if the objects already overlap.".to_string(),
//     apply_move_object_to_touch,
// ),

// make_editor_tool_spec(ToolName::GetInRadius, "Find physical objects inside a radius.".to_string(), apply_get_in_radius),

// #[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
// #[serde(rename_all = "lowercase")]
// enum AxisDirection {
//     X,
//     Y,
//     Z,
//     All,
// }
// // -------------------------- helpers  --------------------------

// fn object_center_and_radius(world: &World, object_id: ObjectId) -> Option<(Vec3, f32)> {
//     let object = world.getObject(object_id).ok()?;
//     let physics_info = world.getObjectPhysicsInfo3D(object_id)?;

//     let radius = match &object.info {
//         ObjectInfo::Ball(_) => world.physics.getBallPrimitiveInfo(object_id)?.radius,
//         ObjectInfo::Brick(_) => 0.5 * world.physics.getBrickPrimitiveInfo(object_id)?.size.length(),
//         // TODO: AI-PICKED-VALUE: RigidGroup currently uses a 0-radius center point for spatial query approximations.
//         ObjectInfo::RigidGroup(_) => 0.0,
//         _ => return None,
//     };

//     Some((physics_info.location.position, radius))
// }

// fn axis_direction_vectors(direction: AxisDirection) -> Vec<(&'static str, Vec3)> {
//     match direction {
//         AxisDirection::All => vec![
//             ("+z", Vec3::new(0.0, 0.0, 1.0)),
//             ("-z", Vec3::new(0.0, 0.0, -1.0)),
//             ("+y", Vec3::new(0.0, 1.0, 0.0)),
//             ("-y", Vec3::new(0.0, -1.0, 0.0)),
//             ("+x", Vec3::new(1.0, 0.0, 0.0)),
//             ("-x", Vec3::new(-1.0, 0.0, 0.0)),
//         ],
//         AxisDirection::X => vec![("+x", Vec3::new(1.0, 0.0, 0.0)), ("-x", Vec3::new(-1.0, 0.0, 0.0))],
//         AxisDirection::Y => vec![("+y", Vec3::new(0.0, 1.0, 0.0)), ("-y", Vec3::new(0.0, -1.0, 0.0))],
//         AxisDirection::Z => vec![("+z", Vec3::new(0.0, 0.0, 1.0)), ("-z", Vec3::new(0.0, 0.0, -1.0))],
//     }
// }

// // -------------------------- run temporary script --------------------------
// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// struct RunTemporaryScriptArgs {
//     pub script: String,
// }
// async fn apply_run_temporary_script(_world_arc: Arc<RwLock<World>>, args: RunTemporaryScriptArgs) -> anyhow::Result<ToolRunFinal> {
//     Ok(tool_failure_result(
//         format!(
//             "run_temporary_script is disabled for safety. Script execution is not enabled. Script preview: {}",
//             args.script.chars().take(120).collect::<String>()
//         ),
//         None,
//     ))
// }

// // -------------------------- get in region --------------------------

// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// pub struct GetInRegionArgs {
//     pub x0: Vec3Args,
//     pub x1: Vec3Args,
//     pub offset: Option<usize>,
//     pub limit: Option<usize>,
// }

// async fn apply_get_in_region(world_arc: Arc<RwLock<World>>, args: GetInRegionArgs) -> anyhow::Result<ToolRunFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;
//     let min_x = args.x0.x.min(args.x1.x);
//     let max_x = args.x0.x.max(args.x1.x);
//     let min_y = args.x0.y.min(args.x1.y);
//     let max_y = args.x0.y.max(args.x1.y);
//     let min_z = args.x0.z.min(args.x1.z);
//     let max_z = args.x0.z.max(args.x1.z);

//     let mut results = Vec::new();
//     for (object_id, object) in world.getAllObjects() {
//         let Some((position, radius)) = object_center_and_radius(world, object_id) else {
//             continue;
//         };

//         let is_contained = position.x - radius >= min_x
//             && position.x + radius <= max_x
//             && position.y - radius >= min_y
//             && position.y + radius <= max_y
//             && position.z - radius >= min_z
//             && position.z + radius <= max_z;

//         let is_partial = position.x + radius >= min_x
//             && position.x - radius <= max_x
//             && position.y + radius >= min_y
//             && position.y - radius <= max_y
//             && position.z + radius >= min_z
//             && position.z - radius <= max_z;

//         if !is_partial {
//             continue;
//         }

//         results.push(json!({
//             "object_id": object_id,
//             "name": object.name,
//             "type": object_kind_name_of_object_info(&object.info),
//             "contained": if is_contained { "contained" } else { "partial" },
//         }));
//     }

//     results.sort_by(|a, b| {
//         let object_id_a = a["object_id"].as_i64().unwrap_or_default();
//         let object_id_b = b["object_id"].as_i64().unwrap_or_default();
//         object_id_a.cmp(&object_id_b)
//     });

//     let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
//     let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);
//     let (paged_results, has_more) = paginate_vec(results, offset, limit);

//     Ok(tool_success_result(
//         message_of_json(json!({
//             "offset": offset,
//             "limit": limit,
//             "results": paged_results,
//             "more_than_limit": has_more,
//         }))?,
//         None,
//     ))
// }

// // -------------------------- get in ray --------------------------

// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// pub struct GetInRayArgs {
//     pub x0: Vec3Args,
//     pub x1: Vec3Args,
//     pub offset: Option<usize>,
//     pub limit: Option<usize>,
// }

// async fn apply_get_in_ray(world_arc: Arc<RwLock<World>>, args: GetInRayArgs) -> anyhow::Result<ToolRunFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;
//     let ray_origin = Vec3::from(args.x0);
//     let ray_endpoint = Vec3::from(args.x1);
//     let ray_vector = ray_endpoint - ray_origin;
//     let ray_length = ray_vector.length();

//     if ray_length <= f32::EPSILON {
//         return Err(anyhow::anyhow!("get_in_ray requires x0 and x1 to be different points"));
//     }

//     let ray_direction = ray_vector / ray_length;
//     let mut excluded_object_ids = HashSet::new();

//     let mut hits = Vec::new();
//     loop {
//         let hit = world.castRay(ray_origin, ray_direction, ray_length, &|object_id| excluded_object_ids.contains(&object_id));
//         let Some((hit_object_id, distance)) = hit else {
//             break;
//         };

//         if !excluded_object_ids.insert(hit_object_id) {
//             break;
//         }

//         let hit_object = world.getObject(hit_object_id)?;
//         hits.push(json!({
//             "object_id": hit_object_id,
//             "name": hit_object.name,
//             "type": object_kind_name_of_object_info(&hit_object.info),
//             "distance": distance,
//         }));
//     }

//     let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
//     let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);
//     let (paged_hits, has_more) = paginate_vec(hits, offset, limit);

//     Ok(tool_success_result(
//         message_of_json(json!({
//             "offset": offset,
//             "limit": limit,
//             "results": paged_hits,
//             "more_than_limit": has_more,
//         }))?,
//         None,
//     ))
// }

// // -------------------------- distance to surroundings cartesian --------------------------

// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// pub struct DistanceToSurroundingsCartesianArgs {
//     pub object_id: ObjectId,
//     pub direction: Option<AxisDirection>,
//     pub max_num_ray_steps: f32,
// }

// async fn apply_distance_to_surroundings_cartesian(
//     world_arc: Arc<RwLock<World>>,
//     args: DistanceToSurroundingsCartesianArgs,
// ) -> anyhow::Result<ToolRunFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;
//     world.getObject(args.object_id)?;

//     let direction = args.direction.unwrap_or(AxisDirection::All);
//     let mut result = serde_json::Map::new();

//     for (direction_label, ray_direction) in axis_direction_vectors(direction) {
//         let cast = world.physics.castShape(args.object_id, ray_direction, args.max_num_ray_steps, true);

//         let value = if let Some((hit_object_id, hit)) = cast {
//             let hit_object = world.getObject(hit_object_id)?;
//             json!({
//                 "object_id": hit_object_id,
//                 "name": hit_object.name,
//                 "type": object_kind_name_of_object_info(&hit_object.info),
//                 "distance": hit.time_of_impact * ray_direction.length(),
//             })
//         } else {
//             Value::Null
//         };

//         result.insert(direction_label.to_string(), value);
//     }

//     Ok(tool_success_result(message_of_json(Value::Object(result))?, Some(args.object_id)))
// }

// // -------------------------- distance and direction between objects --------------------------

// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// pub struct DistanceAndDirectionBetweenObjectsArgs {
//     pub object_id_1: ObjectId,
//     pub object_id_2: ObjectId,
//     pub direction_only: Option<AxisDirection>,
// }

// async fn apply_distance_and_direction_between_objects(
//     world_arc: Arc<RwLock<World>>,
//     args: DistanceAndDirectionBetweenObjectsArgs,
// ) -> anyhow::Result<ToolRunFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;
//     let Some(object_1_physics) = world.getObjectPhysicsInfo3D(args.object_id_1) else {
//         return Ok(tool_failure_result(
//             format!("Object {} does not have physics information.", args.object_id_1),
//             Some(args.object_id_1),
//         ));
//     };

//     let Some(object_2_physics) = world.getObjectPhysicsInfo3D(args.object_id_2) else {
//         return Ok(tool_failure_result(
//             format!("Object {} does not have physics information.", args.object_id_2),
//             Some(args.object_id_2),
//         ));
//     };

//     let position_1 = object_1_physics.location.position;
//     let position_2 = object_2_physics.location.position;
//     let delta = position_2 - position_1;

//     let direction_only = args.direction_only.unwrap_or(AxisDirection::All);
//     let result = match direction_only {
//         AxisDirection::All => {
//             let distance = delta.length();
//             if distance <= f32::EPSILON {
//                 Value::Null
//             } else {
//                 let direction = delta / distance;
//                 json!({
//                     "distance": distance,
//                     "direction": {
//                         "x": direction.x,
//                         "y": direction.y,
//                         "z": direction.z,
//                     }
//                 })
//             }
//         }
//         AxisDirection::X => {
//             let sign = delta.x.signum();
//             json!({
//                 "distance": delta.x.abs(),
//                 "direction": {
//                     "x": sign,
//                     "y": 0.0,
//                     "z": 0.0,
//                 }
//             })
//         }
//         AxisDirection::Y => {
//             let sign = delta.y.signum();
//             json!({
//                 "distance": delta.y.abs(),
//                 "direction": {
//                     "x": 0.0,
//                     "y": sign,
//                     "z": 0.0,
//                 }
//             })
//         }
//         AxisDirection::Z => {
//             let sign = delta.z.signum();
//             json!({
//                 "distance": delta.z.abs(),
//                 "direction": {
//                     "x": 0.0,
//                     "y": 0.0,
//                     "z": sign,
//                 }
//             })
//         }
//     };

//     Ok(tool_success_result(message_of_json(result)?, None))
// }

// // -------------------------- move object to touch --------------------------

// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// pub struct MoveObjectToTouchArgs {
//     pub object_id_1: ObjectId,
//     pub object_id_2: ObjectId,
//     pub direction_only: Option<AxisDirection>,
// }

// async fn apply_move_object_to_touch(world_arc: Arc<RwLock<World>>, args: MoveObjectToTouchArgs) -> anyhow::Result<ToolRunFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;

//     let Some(object_1_physics) = world.getObjectPhysicsInfo3D(args.object_id_1) else {
//         return Ok(tool_failure_result(
//             format!("Object {} does not have physics information.", args.object_id_1),
//             Some(args.object_id_1),
//         ));
//     };

//     let Some(object_2_physics) = world.getObjectPhysicsInfo3D(args.object_id_2) else {
//         return Ok(tool_failure_result(
//             format!("Object {} does not have physics information.", args.object_id_2),
//             Some(args.object_id_2),
//         ));
//     };

//     let object_1_position = object_1_physics.location.position;
//     let object_2_position = object_2_physics.location.position;

//     let direction_only = args.direction_only.unwrap_or(AxisDirection::All);
//     let center_delta = object_2_position - object_1_position;

//     let touch_direction = match direction_only {
//         AxisDirection::All => {
//             let distance = center_delta.length();
//             if distance <= f32::EPSILON { Vec3::ZERO } else { center_delta / distance }
//         }
//         AxisDirection::X => Vec3::new(center_delta.x.signum(), 0.0, 0.0),
//         AxisDirection::Y => Vec3::new(0.0, center_delta.y.signum(), 0.0),
//         AxisDirection::Z => Vec3::new(0.0, 0.0, center_delta.z.signum()),
//     };

//     let Some(translation_to_touch_object_result) = world
//         .physics
//         .getTranslationToTouchObjectInDirection(args.object_id_1, args.object_id_2, touch_direction)
//     else {
//         let error_message = match direction_only {
//             AxisDirection::All => "move_object_to_touch could not compute direction because both objects are at the same position".to_string(),
//             AxisDirection::X => "move_object_to_touch direction_only=x requires the objects to have different x positions".to_string(),
//             AxisDirection::Y => "move_object_to_touch direction_only=y requires the objects to have different y positions".to_string(),
//             AxisDirection::Z => "move_object_to_touch direction_only=z requires the objects to have different z positions".to_string(),
//         };
//         return Err(anyhow::anyhow!(error_message));
//     };

//     let translation = match translation_to_touch_object_result {
//         TranslationToTouchObjectResult::AlreadyOverlapping => {
//             return Ok(tool_failure_result(
//                 format!(
//                     "Object {} already intersects with object {}. move_object_to_touch only works when the objects start separated or already touching.",
//                     args.object_id_1, args.object_id_2
//                 ),
//                 Some(args.object_id_1),
//             ));
//         }
//         TranslationToTouchObjectResult::Translation(translation) => translation,
//     };

//     world.setObjectPositionOffsetRecursive(args.object_id_1, translation)?;

//     let new_position = object_1_position + translation;
//     let distance_moved = translation.length();

//     Ok(tool_success_result(
//         format!(
//             "Successfully placed object {} to touch object {}. It is now at {:?}, and moved {} distance.",
//             args.object_id_1, args.object_id_2, new_position, distance_moved
//         ),
//         Some(args.object_id_1),
//     ))
// }

// // -------------------------- get in radius --------------------------

// #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
// pub struct GetInRadiusArgs {
//     pub x0: Vec3Args,
//     pub radius: f32,
//     pub offset: Option<usize>,
//     pub limit: Option<usize>,
// }

// async fn apply_get_in_radius(world_arc: Arc<RwLock<World>>, args: GetInRadiusArgs) -> anyhow::Result<ToolRunFinal> {
//     let mut world_lock = world_arc.write().await;
//     let world = &mut *world_lock;
//     let center = Vec3::from(args.x0);
//     let radius = args.radius;

//     let mut results = Vec::new();
//     for (object_id, object) in world.getAllObjects() {
//         let Some((position, object_radius)) = object_center_and_radius(world, object_id) else {
//             continue;
//         };

//         let distance = position.distance(center);
//         let is_contained = distance + object_radius <= radius;
//         let is_partial = distance - object_radius <= radius;

//         if !is_partial {
//             continue;
//         }

//         results.push(json!({
//             "object_id": object_id,
//             "name": object.name,
//             "type": object_kind_name_of_object_info(&object.info),
//             "contained": if is_contained { "contained" } else { "partial" },
//         }));
//     }

//     results.sort_by(|a, b| {
//         let object_id_a = a["object_id"].as_i64().unwrap_or_default();
//         let object_id_b = b["object_id"].as_i64().unwrap_or_default();
//         object_id_a.cmp(&object_id_b)
//     });

//     let offset = args.offset.unwrap_or(TOOL_DEFAULT_OFFSET);
//     let limit = args.limit.unwrap_or(TOOL_DEFAULT_LIMIT);
//     let (paged_results, has_more) = paginate_vec(results, offset, limit);

//     Ok(tool_success_result(
//         message_of_json(json!({
//             "offset": offset,
//             "limit": limit,
//             "results": paged_results,
//             "more_than_limit": has_more,
//         }))?,
//         None,
//     ))
// }
