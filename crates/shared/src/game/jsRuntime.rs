use crate::game::gui_types::{GuiRenderedRoot, ScriptGuiKey};
use crate::game::js_cpu_watchdog::{JS_CPU_BUDGET, JsCpuWatchdog, TEMPORARY_SCRIPT_CPU_BUDGET};
use crate::game::jsRuntimeOps::{denoWebRequiredImportsExtension, ourCustomJsOpsExtension};
use crate::game::transpileJsScript::transpileRuntimeScriptSource;
use crate::game::world::{JsCallbackInputs, World, WorldControlMode};
use crate::helpers::multi_thread_hashmap::MultiThreadHashMap;
use deno_core::{
    Extension, ExtensionFileSource, JsRuntime, ModuleCodeString, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleResolveResponse, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, OpState, PollEventLoopOptions, ResolutionKind, RuntimeOptions,
    error::JsError, resolve_import, resolve_url, serde_v8, v8,
};
use deno_error::JsErrorBox;
use deno_web::{BlobStore, InMemoryBroadcastChannel};
use futures::{FutureExt, future::LocalBoxFuture};
use game_types::consts::{CLIENT_WORLD_DELAY_BASE_MS, DEFAULT_CAMERA_ZOOM, MAX_CAMERA_ZOOM, MIN_CAMERA_ZOOM};
use game_types::data_types::DataId;
use game_types::object_types::{ObjectBeingAdded, ObjectId, ObjectInfo, ObjectKind, ObjectLocation, Orientation3D, ScriptInfo};
use glam::Vec3;
use indexmap::IndexMap;
use quinn::Connection;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Waker},
};
use tokio::sync::{mpsc, oneshot};

// -------------- Getters from JsOps --------------

pub fn get_world_from_opstate(state: &OpState) -> anyhow::Result<&World> {
    let ptr = state.try_borrow::<JsRuntimeWorldPtr>().ok_or_else(|| anyhow::anyhow!("World not found."))?;
    let world = ptr.get()?;
    Ok(world)
}
pub fn get_world_mut_from_opstate(state: &mut OpState) -> anyhow::Result<&mut World> {
    let ptr = state.try_borrow_mut::<JsRuntimeWorldPtr>().ok_or_else(|| anyhow::anyhow!("World not found."))?;
    let world = ptr.get_mut()?;
    Ok(world)
}
pub fn get_world_change_count_from_opstate(state: &OpState) -> anyhow::Result<u64> {
    let ptr = state.try_borrow::<JsRuntimeWorldPtr>().ok_or_else(|| anyhow::anyhow!("World not found."))?;
    Ok(ptr.world_change_count())
}
pub fn get_live_sim_ms_from_opstate(state: &OpState) -> anyhow::Result<(f64, Option<f64>)> {
    let js_runtime_sim_ms = state.try_borrow::<JsRuntimeSimMs>().ok_or_else(|| anyhow::anyhow!("Sim ms not found."))?;
    Ok((js_runtime_sim_ms.est_live_server_sim_ms, js_runtime_sim_ms.live_client_sim_ms))
}
pub fn get_server_fn_caller_object_id_from_opstate(state: &OpState) -> anyhow::Result<Option<ObjectId>> {
    let server_fn_caller = state
        .try_borrow::<JsRuntimeServerFnCaller>()
        .ok_or_else(|| anyhow::anyhow!("Server fn caller slot not found."))?;
    Ok(server_fn_caller.player_object_id)
}
pub fn get_camera_orientation_from_opstate(state: &OpState) -> anyhow::Result<Orientation3D> {
    let camera_orientation = state
        .try_borrow::<JsRuntimeCameraOrientation>()
        .ok_or_else(|| anyhow::anyhow!("Camera orientation slot not found."))?;
    Ok(camera_orientation.orientation)
}
pub fn get_mouse_ray_from_opstate(state: &OpState) -> anyhow::Result<Option<(Vec3, Vec3)>> {
    let mouse_ray = state
        .try_borrow::<JsRuntimeMouseRay>()
        .ok_or_else(|| anyhow::anyhow!("Mouse ray slot not found."))?;
    Ok(mouse_ray.ray)
}
pub fn get_camera_zoom_from_opstate(state: &OpState) -> anyhow::Result<f32> {
    let camera_zoom = state
        .try_borrow::<JsRuntimeCameraZoom>()
        .ok_or_else(|| anyhow::anyhow!("Camera zoom slot not found."))?;
    Ok(camera_zoom.zoom)
}
pub fn get_desired_camera_zoom_from_opstate(state: &OpState) -> anyhow::Result<f32> {
    let desired_camera_zoom = state
        .try_borrow::<JsRuntimeDesiredCameraZoom>()
        .ok_or_else(|| anyhow::anyhow!("Desired camera zoom slot not found."))?;
    Ok(desired_camera_zoom.desired_zoom)
}
pub fn get_client_world_delay_ms_from_opstate(state: &OpState) -> anyhow::Result<f32> {
    let client_world_delay = state
        .try_borrow::<JsRuntimeClientWorldDelay>()
        .ok_or_else(|| anyhow::anyhow!("Client world delay slot not found."))?;
    Ok(client_world_delay.delay_ms)
}
pub fn get_player_ping_ms_from_opstate(state: &OpState, player_object_id: ObjectId) -> anyhow::Result<Option<u16>> {
    let player_pings = state
        .try_borrow::<JsRuntimePlayerPings>()
        .ok_or_else(|| anyhow::anyhow!("Player pings slot not found."))?;
    Ok(player_pings.ping_ms_of_player_object_id.get(&player_object_id).copied())
}
pub fn get_script_manager_mut_from_opstate(state: &mut OpState) -> anyhow::Result<&mut Rc<RefCell<ScriptManager>>> {
    state
        .try_borrow_mut::<Rc<RefCell<ScriptManager>>>()
        .ok_or_else(|| anyhow::anyhow!("ScriptManager not found."))
}

// -------------- Helpers --------------

// ModuleSpecifier = Url
// ModuleCodeString = FastString

type ScriptGuiHandlerKey = (ObjectId, String, String); // (script_object_id, gui_key, handler_id)

#[derive(Clone, Debug)]
pub struct JsConsoleLogEntry {
    pub log_level: String,
    pub message: String,
}

// Called on the runtime's own thread the moment a script prints, so entries stay in the order they happened.
// Keep it quick: a slow callback slows the script down.
pub type JsConsoleLogCallback = Arc<dyn Fn(JsConsoleLogEntry) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ScriptRuntimeFailureEntry {
    pub script_object_id: ObjectId,
    pub script_path: String,
    pub error_text: String,
}

// Client scripts set these, and the play loop and renderer place the player's camera with them. Deleting the script that set them keeps them.
#[derive(Clone, Copy, Debug)]
pub struct CameraLimits {
    pub locked_orientation: Option<Orientation3D>, // None means the player turns the camera freely
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub fov_degrees: Option<f32>, // None means the default fov
    pub is_mouse_locked: bool,    // the pointer is hidden and held in the middle of the window, and moving the mouse turns the camera
    pub offset: Vec3,             // along the camera's own right, up and forward directions
}
impl CameraLimits {
    pub fn new() -> Self {
        Self {
            locked_orientation: None,
            min_zoom: MIN_CAMERA_ZOOM,
            max_zoom: MAX_CAMERA_ZOOM,
            fov_degrees: None,
            is_mouse_locked: false,
            offset: Vec3::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BuiltinMouseCursor {
    Default,
    Pointer,
    Crosshair,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct MouseCursorImage {
    pub textureDataId: DataId,
    pub width: f32, // in points, so it looks the same on any display
    pub height: f32,
    pub hotspotX: f32, // the point that clicks, as a fraction of the picture from its top left
    pub hotspotY: f32,
}

// What the local player's mouse cursor shows. Client scripts set it, and the desktop client makes it the window's cursor.
// Deleting the script that set it keeps it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum MouseCursor {
    Builtin(BuiltinMouseCursor),
    Image(MouseCursorImage),
}

pub struct ScriptManager {
    code_of_url: HashMap<ModuleSpecifier, String>,        // static, never changes once added
    url_of_script_id: HashMap<ObjectId, ModuleSpecifier>, // static, never changes once added, even if the script location changes. in our docs say "url = url of the script initially. 99% of the time this is not relevant"
    script_launcher_num: u64,

    server_fn_of_id: HashMap<String, v8::Global<v8::Function>>, // FN_ID -> the v8 fn to call. static, never changes once added. this is the registry of server fns that we call when the client calls a server fn.
    client_fn_of_id: HashMap<String, v8::Global<v8::Function>>, // FN_ID -> the v8 fn to call. static, never changes once added. this is the registry of client fns that we call when the server calls a client fn.
    js_callback_fn: Option<v8::Global<v8::Function>>, // internal JS callback fn for typed JS callback inputs queued by Rust-owned systems like physics and player join.
    gui_root_of_script_gui_key: IndexMap<ScriptGuiKey, GuiRenderedRoot>, // In the order the GUIs were shown, which is the order they stack in, newest on top.
    gui_handler_of_script_gui_handler_key: HashMap<ScriptGuiHandlerKey, v8::Global<v8::Function>>,
    camera_limits: CameraLimits,
    mouse_cursor: MouseCursor,
    // console_log_string_list: Vec<ConsoleLogStringEntry>, // (console_log_time, console_log_string, console_log_level)
    next_tick_txs_of_script_id: HashMap<ObjectId, Vec<oneshot::Sender<()>>>, // await tick()

    // for the temp script tool. only added to if not None. use start_recording to start and take_temp_script_console_log_entries to clear/stop.

    // optional. only used by run temp script.
    temp_script_console_log_entries: Option<Vec<JsConsoleLogEntry>>,

    // optional. this is given to us by whoever wants to be told about every console call as it happens.
    console_log_callback: Option<JsConsoleLogCallback>,
}

impl ScriptManager {
    fn new(console_log_callback: Option<JsConsoleLogCallback>) -> Self {
        Self {
            code_of_url: HashMap::new(),
            url_of_script_id: HashMap::new(),
            script_launcher_num: 0,
            server_fn_of_id: HashMap::new(),
            client_fn_of_id: HashMap::new(),
            js_callback_fn: None,
            gui_root_of_script_gui_key: IndexMap::new(),
            gui_handler_of_script_gui_handler_key: HashMap::new(),
            camera_limits: CameraLimits::new(),
            mouse_cursor: MouseCursor::Builtin(BuiltinMouseCursor::Default),
            temp_script_console_log_entries: None,
            console_log_callback,
            next_tick_txs_of_script_id: HashMap::new(),
        }
    }
    // user scripts
    // "game:/<script_path>?id=<script_object_id>" -> source
    fn add_user_script(&mut self, script_object_id: &ObjectId, world: &World) -> anyhow::Result<()> {
        let Some(raw_code) = world.scriptRawCodeOfId.get(script_object_id) else {
            log::error!("Script raw code not found for id: {}", script_object_id);
            return Err(anyhow::anyhow!("Script raw code not found for id: {}", script_object_id));
        };
        let script_live_object = world.getObject(*script_object_id)?;
        let objectLocation = match &script_live_object.info {
            ObjectInfo::Script(script_info) => script_info.objectLocation,
            _ => return Err(anyhow::anyhow!("Script object is not a script for id: {}", script_object_id)),
        };

        let script_path = createScriptPath(world, *script_object_id)?;
        let script_url = urlOfScript(script_path.as_str(), *script_object_id)?;
        let source_transpiled = transpileRuntimeScriptSource(script_path.as_str(), raw_code.as_str(), &world.worldControlMode, objectLocation)?;
        let source = injectedSource(script_object_id, source_transpiled.as_str());
        self.code_of_url.insert(script_url.clone(), source);
        self.url_of_script_id.insert(*script_object_id, script_url);
        Ok(())
    }

    // kube-ts-api
    // "kube-ts-api:bundle" -> source
    fn add_kube_ts_api(&mut self) -> anyhow::Result<()> {
        let kube_ts_api_url = resolve_url("kube-ts-api:bundle")?;
        let source = include_str!("../../../../packages/kube-ts-api/dist/kube-ts-api.bundle.js").to_string();
        self.code_of_url.insert(kube_ts_api_url, source);
        Ok(())
    }

    // kube-ts-api/jsx-runtime: `jsx-runtime` is a special automatic JSX entry name, so this module path stays literal.
    // "kube-ts-api:jsx-runtime.js" -> source
    fn add_kube_ts_api_jsx_runtime(&mut self) -> anyhow::Result<()> {
        let kube_ts_api_jsx_runtime_url = resolve_url("kube-ts-api:jsx-runtime.js")?;
        let source = include_str!("../../../../packages/kube-ts-api/dist/jsx-runtime.js").to_string();
        self.code_of_url.insert(kube_ts_api_jsx_runtime_url, source);
        Ok(())
    }

    fn remove_user_script(&mut self, script_object_id: &ObjectId) {
        if let Some(script_url) = self.url_of_script_id.remove(script_object_id) {
            self.code_of_url.remove(&script_url);
        }
        self.gui_root_of_script_gui_key
            .retain(|(next_script_object_id, _), _| next_script_object_id != script_object_id);
        self.gui_handler_of_script_gui_handler_key
            .retain(|(next_script_object_id, _, _), _| next_script_object_id != script_object_id);

        self.next_tick_txs_of_script_id.remove(script_object_id);
    }

    fn get_raw_code(&self, url: &ModuleSpecifier) -> Option<&String> {
        self.code_of_url.get(url)
    }
    fn get_script_url(&self, script_object_id: &ObjectId) -> Option<&ModuleSpecifier> {
        self.url_of_script_id.get(script_object_id)
    }

    // Scripts import each other by path, and a pasted copy shares its original's path. The newest script at the path
    // wins, so a copy's scripts import each other instead of the original's.
    fn get_newest_script_url_at_path(&self, path_url: &ModuleSpecifier) -> Option<&ModuleSpecifier> {
        self.url_of_script_id
            .iter()
            .filter(|(_, script_url)| script_url.path() == path_url.path())
            .max_by_key(|(script_object_id, _)| **script_object_id)
            .map(|(_, script_url)| script_url)
    }

    /// `remove_user_script` is the only thing that clears id, so it doubles as the liveness set with no extra bookkeeping.
    pub fn is_script_live(&self, script_object_id: &ObjectId) -> bool {
        self.url_of_script_id.contains_key(script_object_id)
    }

    pub fn set_as_awaiting_next_tick(&mut self, script_object_id: &ObjectId, next_tick_tx: oneshot::Sender<()>) {
        // Keep every concurrent wait in a script so they all resume on the next tick.
        self.next_tick_txs_of_script_id.entry(*script_object_id).or_default().push(next_tick_tx);
    }

    pub fn register_server_js_fn(&mut self, fn_id: String, server_js_fn: v8::Global<v8::Function>) {
        self.server_fn_of_id.insert(fn_id, server_js_fn);
    }

    pub fn register_client_js_fn(&mut self, fn_id: String, client_js_fn: v8::Global<v8::Function>) {
        self.client_fn_of_id.insert(fn_id, client_js_fn);
    }

    pub fn get_server_js_fn(&self, fn_id: &str) -> Option<&v8::Global<v8::Function>> {
        self.server_fn_of_id.get(fn_id)
    }

    pub fn get_client_js_fn(&self, fn_id: &str) -> Option<&v8::Global<v8::Function>> {
        self.client_fn_of_id.get(fn_id)
    }

    pub fn register_js_callback_fn(&mut self, js_callback_fn: v8::Global<v8::Function>) {
        self.js_callback_fn = Some(js_callback_fn);
    }

    // console log in temp scripts
    pub fn push_console_log_string(&mut self, console_log_string: String, console_log_level: String) -> anyhow::Result<()> {
        // let console_log_time = SystemTime::now().duration_since(UNIX_EPOCH)?;
        log::info!("[JS console.{}] {}", console_log_level, console_log_string);
        let console_log_entry = JsConsoleLogEntry {
            log_level: console_log_level,
            message: console_log_string,
        };

        // if there's a tx, send entry to it
        if let Some(console_log_callback) = self.console_log_callback.as_ref() {
            console_log_callback(console_log_entry.clone());
        }

        // if theres a temp script vector, add entry to it
        if let Some(temp_script_console_log_entries) = self.temp_script_console_log_entries.as_mut() {
            temp_script_console_log_entries.push(console_log_entry);
        }
        // self.console_log_string_list.push((console_log_time, console_log_string, console_log_level));
        Ok(())
    }

    pub fn record_script_runtime_failure(&mut self, error_text: String) {
        // just log the error like it's a console.error
        let _ = self.push_console_log_string(error_text, "error".to_string());
    }

    fn start_recording_temp_script_console_log_entries(&mut self) {
        self.temp_script_console_log_entries = Some(Vec::new());
    }

    fn take_temp_script_console_log_entries(&mut self) -> Vec<JsConsoleLogEntry> {
        self.temp_script_console_log_entries.take().unwrap_or_default()
    }

    pub fn gui_root_of_script_gui_key(&self) -> &IndexMap<ScriptGuiKey, GuiRenderedRoot> {
        &self.gui_root_of_script_gui_key
    }

    pub fn set_gui_root(&mut self, script_object_id: ObjectId, gui_key: String, gui_root: GuiRenderedRoot) {
        self.gui_root_of_script_gui_key.insert((script_object_id, gui_key), gui_root);
    }

    pub fn clear_gui_root(&mut self, script_object_id: &ObjectId, gui_key: &str) {
        self.gui_root_of_script_gui_key.shift_remove(&(*script_object_id, gui_key.to_string()));
        self.clear_gui_handlers(script_object_id, gui_key);
    }

    pub fn clear_gui_handlers(&mut self, script_object_id: &ObjectId, gui_key: &str) {
        self.gui_handler_of_script_gui_handler_key
            .retain(|(next_script_object_id, next_gui_key, _), _| next_script_object_id != script_object_id || next_gui_key != gui_key);
    }

    pub fn register_gui_handler(&mut self, script_object_id: ObjectId, gui_key: String, handler_id: String, handler: v8::Global<v8::Function>) {
        self.gui_handler_of_script_gui_handler_key
            .insert((script_object_id, gui_key, handler_id), handler);
    }

    pub fn gui_handler(&self, script_object_id: &ObjectId, gui_key: &str, handler_id: &str) -> Option<&v8::Global<v8::Function>> {
        self.gui_handler_of_script_gui_handler_key
            .get(&(*script_object_id, gui_key.to_string(), handler_id.to_string()))
    }

    pub fn camera_limits(&self) -> CameraLimits {
        self.camera_limits
    }

    pub fn camera_limits_mut(&mut self) -> &mut CameraLimits {
        &mut self.camera_limits
    }

    pub fn mouse_cursor(&self) -> MouseCursor {
        self.mouse_cursor
    }

    pub fn mouse_cursor_mut(&mut self) -> &mut MouseCursor {
        &mut self.mouse_cursor
    }

    // A script that left the world never resumes: its senders drop here, which fails its tick() await.
    fn signal_all_awaiting_next_ticks(&mut self, world: &World) {
        for (script_object_id, next_tick_txs) in std::mem::take(&mut self.next_tick_txs_of_script_id) {
            if !world.hasObject(script_object_id) {
                continue;
            }
            for next_tick_tx in next_tick_txs {
                let _ = next_tick_tx.send(());
            }
        }
    }
}

// -------------- JsRuntimeWorldPtr --------------

#[derive(Clone, Copy)] // it is copyable, just a ptr
pub struct JsRuntimeWorldPtr {
    world_ptr: *mut World,
    world_change_count: u64, // goes up whenever scripts are handed the world or an op takes it to change it, so a read made at the same count is still current
}
impl JsRuntimeWorldPtr {
    pub fn null() -> Self {
        Self {
            world_ptr: std::ptr::null_mut(),
            world_change_count: 0,
        }
    }
    pub fn set(&mut self, world: Option<&mut World>) {
        self.world_change_count += 1;
        self.world_ptr = match world {
            Some(world) => world as *mut World,
            None => std::ptr::null_mut(),
        };
    }
    pub fn get(&self) -> anyhow::Result<&World> {
        unsafe { self.world_ptr.as_ref() }.ok_or_else(|| anyhow::anyhow!("World ptr was null."))
    }
    pub fn get_mut(&mut self) -> anyhow::Result<&mut World> {
        self.world_change_count += 1;
        unsafe { self.world_ptr.as_mut() }.ok_or_else(|| anyhow::anyhow!("World ptr was null."))
    }
    pub fn world_change_count(&self) -> u64 {
        self.world_change_count
    }
}

// -------------- JsRuntimeSimMs --------------

pub struct JsRuntimeSimMs {
    est_live_server_sim_ms: f64,
    live_client_sim_ms: Option<f64>,
}
impl JsRuntimeSimMs {
    pub fn new() -> Self {
        Self {
            est_live_server_sim_ms: 0.0,
            live_client_sim_ms: None,
        }
    }
    pub fn set(&mut self, est_live_server_sim_ms: f64, live_client_sim_ms: Option<f64>) {
        self.est_live_server_sim_ms = est_live_server_sim_ms;
        self.live_client_sim_ms = live_client_sim_ms;
    }
}

// -------------- JsRuntimeServerFnCaller --------------

// The player whose call the server is running, set only for the length of a 'use server' function a client called.
pub struct JsRuntimeServerFnCaller {
    player_object_id: Option<ObjectId>,
}
impl JsRuntimeServerFnCaller {
    pub fn new() -> Self {
        Self { player_object_id: None }
    }

    pub fn set(&mut self, player_object_id: Option<ObjectId>) {
        self.player_object_id = player_object_id;
    }
}

// -------------- JsRuntimeCameraOrientation --------------

// The client play loop refreshes this every tick. It exists on every runtime but the camera op is client-only, so the server never reads it.
pub struct JsRuntimeCameraOrientation {
    orientation: Orientation3D,
}
impl JsRuntimeCameraOrientation {
    pub fn new() -> Self {
        Self {
            orientation: Orientation3D::Z(), // doesnt matter, immediately overwritten
        }
    }

    pub fn set(&mut self, orientation: Orientation3D) {
        self.orientation = orientation;
    }
}

// -------------- JsRuntimeMouseRay --------------

pub struct JsRuntimeMouseRay {
    // (origin, unit direction)
    ray: Option<(Vec3, Vec3)>,
}
impl JsRuntimeMouseRay {
    pub fn new() -> Self {
        Self { ray: None } // no cursor yet, immediately overwritten by the render loop
    }

    pub fn set(&mut self, ray: Option<(Vec3, Vec3)>) {
        self.ray = ray;
    }
}

// -------------- JsRuntimeCameraZoom --------------

// The client play loop refreshes this every tick with how far the camera is from the head, after anything in the way moves it closer. It exists on every runtime but getZoom is client-only, so the server never reads it.
pub struct JsRuntimeCameraZoom {
    zoom: f32,
}
impl JsRuntimeCameraZoom {
    pub fn new() -> Self {
        Self { zoom: DEFAULT_CAMERA_ZOOM }
    }

    pub fn set(&mut self, zoom: f32) {
        self.zoom = zoom;
    }
}

// -------------- JsRuntimeDesiredCameraZoom --------------

// The client play loop refreshes this every tick with the zoom the player picked, before anything in the way moves the camera closer. It exists on every runtime but getDesiredZoom is client-only, so the server never reads it.
pub struct JsRuntimeDesiredCameraZoom {
    desired_zoom: f32,
}
impl JsRuntimeDesiredCameraZoom {
    pub fn new() -> Self {
        Self {
            desired_zoom: DEFAULT_CAMERA_ZOOM,
        }
    }

    pub fn set(&mut self, desired_zoom: f32) {
        self.desired_zoom = desired_zoom;
    }
}

// -------------- JsRuntimeClientWorldDelay --------------

// The client play loop refreshes this every tick. It exists on every runtime but getDelayedWorldTime is client-only, so the server never reads it.
pub struct JsRuntimeClientWorldDelay {
    delay_ms: f32,
}
impl JsRuntimeClientWorldDelay {
    pub fn new() -> Self {
        Self {
            delay_ms: CLIENT_WORLD_DELAY_BASE_MS, // what ClientWorldDelay starts at, immediately overwritten
        }
    }

    pub fn set(&mut self, delay_ms: f32) {
        self.delay_ms = delay_ms;
    }
}

// -------------- JsRuntimePlayerPings --------------

// The client play loop sets this whenever the server's pings arrive. It exists on every runtime, but the server reads pings straight off the connections instead.
pub struct JsRuntimePlayerPings {
    ping_ms_of_player_object_id: HashMap<ObjectId, u16>,
}
impl JsRuntimePlayerPings {
    pub fn new() -> Self {
        Self {
            ping_ms_of_player_object_id: HashMap::new(),
        }
    }

    pub fn set(&mut self, ping_ms_of_player_object_id: HashMap<ObjectId, u16>) {
        self.ping_ms_of_player_object_id = ping_ms_of_player_object_id;
    }
}

// -------------- ScriptManager --------------

// INTERNAL HOOK for world pointer: call this before running any JS
fn beforeRunJS(jsRuntime: &mut JsRuntime, world: &mut World, est_live_server_sim_ms: f64, live_client_sim_ms: Option<f64>) {
    let opStateRc = jsRuntime.op_state();
    let mut opState = opStateRc.borrow_mut();
    opState.borrow_mut::<JsRuntimeWorldPtr>().set(Some(world));
    opState.borrow_mut::<JsRuntimeSimMs>().set(est_live_server_sim_ms, live_client_sim_ms);
}

// INTERNAL HOOK for world pointer: call this after running any JS
fn afterRunJS(jsRuntime: &mut JsRuntime) {
    let opStateRc = jsRuntime.op_state();
    let mut opState = opStateRc.borrow_mut();
    let jsRuntimeWorldPtr = opState.borrow_mut::<JsRuntimeWorldPtr>();
    jsRuntimeWorldPtr.set(None);
}

// -------------- Helpers to import scripts --------------

pub fn createScriptPath(world: &World, script_object_id: ObjectId) -> anyhow::Result<String> {
    world
        .getObjectNamePath(script_object_id)
        .map_err(|error| anyhow::anyhow!("Script object path not found for id {}: {}", script_object_id, error))
}

// script_path: path of object names from the world hierarchy, for example "Root/Folder/MyScript".
// "game:/<script_path>?id=<script_object_id>" <--> ModuleSpecifier used by deno_core. The id makes every Script object its own module, even when two scripts share a path.
fn urlOfScript(script_path: &str, script_object_id: ObjectId) -> anyhow::Result<ModuleSpecifier> {
    ModuleSpecifier::parse(&format!("game:/{script_path}?id={script_object_id}")).map_err(|e| anyhow::anyhow!("Error parsing module specifier: {}", e))
}

fn injectedSource(script_object_id: &ObjectId, script_source: &str) -> String {
    format!(
        "
import {{ __getGame__, stopScript as __stopScriptInternal__, tick as __tickInternal__, __getScript__, createState, useState, getTime, getDelayedWorldTime, vec3 }} from 'kube-ts-api';
const script = __getScript__({script_object_id});
const game   = __getGame__(script.id);
const tick   = () => __tickInternal__(script.id);
const stopScript = () => __stopScriptInternal__();
{script_source}
"
    )
}

// ---------------------- Helpers to build the runtime -------------------

struct OurCustomModuleLoader {
    script_manager_rc: Rc<RefCell<ScriptManager>>, // very important that this is updated by manager before trying to access
}
impl OurCustomModuleLoader {
    fn new(script_manager_rc: Rc<RefCell<ScriptManager>>) -> Self {
        Self { script_manager_rc }
    }
}
impl ModuleLoader for OurCustomModuleLoader {
    // re-maps imported names to the module URLs we gave them in rust
    fn resolve(&self, import_specifier: &str, referrer_module_url: &str, _kind: ResolutionKind) -> ModuleResolveResponse {
        // if the user imported 'kube-ts-api', resolve to the bundle
        if import_specifier == "kube-ts-api" {
            return resolve_url("kube-ts-api:bundle").map_err(JsErrorBox::from_err);
        }
        if import_specifier == "kube-ts-api/jsx-runtime" {
            return resolve_url("kube-ts-api:jsx-runtime.js").map_err(JsErrorBox::from_err);
        }
        // an import names a script by its path, without the id that its url has
        let module_url = resolve_import(import_specifier, referrer_module_url).map_err(JsErrorBox::from_err)?;
        if module_url.query().is_none() {
            if let Some(script_url) = self.script_manager_rc.borrow().get_newest_script_url_at_path(&module_url) {
                return Ok(script_url.clone());
            }
        }
        Ok(module_url)
    }

    // returns the code given the resolved rust module URL
    fn load(&self, module_url: &ModuleSpecifier, _maybe_referrer: Option<&ModuleLoadReferrer>, _options: ModuleLoadOptions) -> ModuleLoadResponse {
        let Some(code_str) = self.script_manager_rc.borrow().get_raw_code(module_url).cloned() else {
            return ModuleLoadResponse::Sync(Err(JsErrorBox::generic("Module not found")));
        };
        log::info!("Loading code for module url: {}", module_url);
        let code = ModuleSourceCode::String(ModuleCodeString::from(code_str));
        ModuleLoadResponse::Sync(Ok(ModuleSource::new(ModuleType::JavaScript, code, module_url, None)))
    }
}

// ---------------------- Helpers to sync scripts -------------------

async fn flushAndStartNewlyAddedScripts(jsRuntime: &mut JsRuntime, world: &mut World, script_manager_rc: &Rc<RefCell<ScriptManager>>) -> anyhow::Result<()> {
    let (added_script_ids, removed_script_ids) = world.flushDirtyScripts();
    let mut added_script_urls = Vec::new();

    // register every new script before loading any of them so they can depend on each other
    for script_object_id in added_script_ids {
        // a script removed before it started never runs
        if !world.hasObject(script_object_id) {
            continue;
        }
        let res = script_manager_rc.borrow_mut().add_user_script(&script_object_id, world);
        if let Err(err) = res {
            let script_path = createScriptPath(world, script_object_id)?;
            let script_err_message = format!("Error adding user script {}: {}: {}", script_object_id, script_path, err);
            log::error!("{script_err_message}");
            script_manager_rc.borrow_mut().record_script_runtime_failure(script_err_message);
            continue;
        }

        let Some(script_url) = script_manager_rc.borrow().get_script_url(&script_object_id).cloned() else {
            let script_path = createScriptPath(world, script_object_id)?;
            let script_err_message = format!("Script module url not found for script {}: {}", script_object_id, script_path);
            log::error!("{script_err_message}");
            script_manager_rc.borrow_mut().record_script_runtime_failure(script_err_message);
            continue;
        };
        added_script_urls.push((script_object_id, script_url));
    }

    // Start the scripts in a fixed order, by their path in the world.
    //
    // The ids arrive from a HashSet, and Rust seeds its hasher per process, so without this the same
    // world starts its scripts in a different order on every single boot. Which script starts first
    // decides which one is blamed when something goes wrong, so a game could behave differently on
    // two servers running byte-identical data.
    //
    // Sorting by path rather than by object id, because on the server the stored ids
    // are thrown away and reassigned by tree position on load, so adding a single object anywhere
    // earlier in the world renumbers everything after it. A path only changes when someone renames or
    // moves the script itself.
    added_script_urls.sort_by(|(_, left_script_url), (_, right_script_url)| left_script_url.as_str().cmp(right_script_url.as_str()));

    // we were having problems before treating scripts as modules, because when they'd import each other they could panic
    // now, each script is treated as a true module by having only one file that imports the script as the module, not the script itself.

    // load_side_es_module_from_code can be called as many times as you want and is recursive, it loads a whole module graph. it calls load() in our custom module loader.
    // mod_evaluate *must* be called only once on each module and was the thing causing the panics before, you get a panic if you call mod_evaluate on a script and then also on a script that is imported by that one.
    // the order is load module -> evaluate module to add all items to promise queue -> poll event loop to run code.

    // scriptManager does the conversion from import to actual code during CustomModuleLoader load(). the script_launcher_url can be anything, we never use it elsewhere.

    for (script_object_id, script_url) in added_script_urls {
        let script_launcher_num = {
            let mut script_manager = script_manager_rc.borrow_mut();
            let script_launcher_num = script_manager.script_launcher_num;
            script_manager.script_launcher_num += 1;
            script_launcher_num
        };
        let script_launcher_url = resolve_url(format!("game:/__internal__/script_launcher_{script_launcher_num}").as_str())?;

        let script_launcher_source = format!("import {};", serde_json::to_string(script_url.as_str())?);
        // Each new script gets its own budget, so a slow script does not exhaust its neighbour's.
        let result = JsCpuWatchdog::run(jsRuntime, JS_CPU_BUDGET, async |jsRuntime| {
            let script_launcher_module_id = jsRuntime.load_side_es_module_from_code(&script_launcher_url, script_launcher_source).await?;

            // Run this script's own code before loading the next one.
            let _ = jsRuntime.mod_evaluate(script_launcher_module_id);
            pollEventLoop(jsRuntime).await
        })
        .await;
        if let Err(err) = result {
            // im not sure this is perfect, maybe revisit
            let script_err_message = format!("Error running script {}: in file {}: {}", script_url, script_object_id, err);
            log::error!("{script_err_message}");
            script_manager_rc.borrow_mut().record_script_runtime_failure(script_err_message);
        }
    }

    // remove script from script manager, marking it as stopped
    for script_object_id in removed_script_ids {
        script_manager_rc.borrow_mut().remove_user_script(&script_object_id);
    }

    Ok(())
}

// ---------------------- Helpers to Run pending JS -------------------

// The registered JS callback fn never awaits, so this finishes without running the event loop. That lets the client play
// loop call it in the middle of a physics step.
fn callJsCallbacks(jsRuntime: &mut JsRuntime, script_manager_rc: &Rc<RefCell<ScriptManager>>, js_callback_inputs: Vec<JsCallbackInputs>) -> anyhow::Result<()> {
    if js_callback_inputs.is_empty() {
        return Ok(());
    }

    let js_callback_inputs_arg = {
        deno_core::scope!(scope, jsRuntime);
        let js_inputs = serde_v8::to_v8(scope, js_callback_inputs)?;
        v8::Global::new(scope, js_inputs)
    };

    let js_callback_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(js_callback_fn) = script_manager.js_callback_fn.as_ref() else {
            return Err(anyhow::anyhow!("JS callback fn was not registered"));
        };
        js_callback_fn.clone()
    };

    // the slice spreads the args
    JsCpuWatchdog::run_sync(jsRuntime, JS_CPU_BUDGET, |jsRuntime| {
        let call = jsRuntime.call_with_args(&js_callback_fn, std::slice::from_ref(&js_callback_inputs_arg));
        let Some(call_result) = call.now_or_never() else {
            return Err(anyhow::anyhow!("JS callback fn returned a promise, but it must finish right away"));
        };
        call_result?;
        Ok(())
    })
}

async fn pollEventLoop(jsRuntime: &mut JsRuntime) -> anyhow::Result<()> {
    // Step a single event loop step: re-queue every item and wait till they all reach an `await` they can't move past
    let poll_result = std::future::poll_fn(|cx| match jsRuntime.poll_event_loop(cx, Default::default()) {
        std::task::Poll::Ready(result) => std::task::Poll::Ready(Some(result)),
        std::task::Poll::Pending => std::task::Poll::Ready(None),
    })
    .await;

    if let Some(result) = poll_result {
        result?;
    }
    Ok(())
}

// loads kube-ts-api module
async fn loadKubeTsApi(jsRuntime: &mut JsRuntime) -> anyhow::Result<()> {
    // The bundled API must be evaluated before any user code can use the injected `game` and `script` globals.
    let kube_ts_api_url = resolve_url("kube-ts-api:bundle")?;
    let kube_ts_api_module_id = jsRuntime.load_side_es_module(&kube_ts_api_url).await?;
    let module_evaluation = jsRuntime.mod_evaluate(kube_ts_api_module_id);
    jsRuntime.with_event_loop_promise(module_evaluation, PollEventLoopOptions::default()).await?;
    // kube-ts-api has kept the ops it uses, so take every op away from user code, which runs next. Through Deno.core.ops or
    // __bootstrap.core.ops a script could call any op with any arguments, like op_panic, which stops the whole process.
    jsRuntime.execute_script("game:/__internal__/hide_ops", "delete globalThis.Deno; delete globalThis.__bootstrap;")?;
    Ok(())
}

// ---------------------- Temporary editor scripts -------------------

async fn evaluateTemporaryScriptModule(jsRuntime: &mut JsRuntime, script_launcher_module_id: deno_core::ModuleId) -> anyhow::Result<()> {
    // A temporary script should finish as a short editor command, so a few unresolved event-loop polls is enough to catch accidental waits without adding a wall-clock timer here.
    // this is the one place in the repo where i have no idea whats going on. oh well, lgtm.
    const TEMPORARY_SCRIPT_PENDING_POLL_LIMIT: usize = 8;

    let module_evaluation = jsRuntime.mod_evaluate(script_launcher_module_id);
    futures::pin_mut!(module_evaluation);
    let mut pending_poll_count = 0;
    loop {
        let module_evaluation_result = futures::future::poll_fn(|cx| {
            let mut ready_module_evaluation_result = None;
            if let Poll::Ready(module_evaluation_result) = module_evaluation.as_mut().poll(cx) {
                ready_module_evaluation_result = Some(module_evaluation_result);
            }
            match jsRuntime.poll_event_loop(cx, PollEventLoopOptions::default()) {
                Poll::Ready(Err(error)) => Poll::Ready(Err(anyhow::Error::new(error))),
                Poll::Ready(Ok(())) => {
                    if let Some(module_evaluation_result) = ready_module_evaluation_result {
                        return Poll::Ready(Ok(Some(module_evaluation_result)));
                    }
                    if let Poll::Ready(module_evaluation_result) = module_evaluation.as_mut().poll(cx) {
                        Poll::Ready(Ok(Some(module_evaluation_result)))
                    } else {
                        Poll::Ready(Ok(None))
                    }
                }
                Poll::Pending => Poll::Ready(Ok(ready_module_evaluation_result)),
            }
        })
        .await?;

        let Some(module_evaluation_result) = module_evaluation_result else {
            pending_poll_count += 1;
            if pending_poll_count >= TEMPORARY_SCRIPT_PENDING_POLL_LIMIT {
                return Err(anyhow::anyhow!(
                    "Temporary script did not finish. Keep temporary scripts as short commands and do not wait for tick(), timers, or long-running promises."
                ));
            }
            std::thread::yield_now();
            continue;
        };

        module_evaluation_result?;
        return Ok(());
    }
}

pub fn runTemporaryScript(world: &mut World, script_raw_code: String) -> anyhow::Result<Vec<JsConsoleLogEntry>> {
    // Editor mode so the temp script stays invisible: it is never marked for replication, and adding and
    // removing it never queues objectCreated/objectDeleted onto js_callback_inputs_tx, which outlives us.
    world.run_f_in_control_mode(WorldControlMode::Editor, |world| {
        // add the script to the world
        let temp_script_id = world.addObject(
            "__temporary_ai_script".to_string(),
            ObjectBeingAdded::Script(ScriptInfo {
                objectLocation: ObjectLocation::Server,
            }),
            world.getRootObjectId(),
        );
        world.scriptRawCodeOfId.insert(temp_script_id, script_raw_code);

        // run the script. Server mode only while it runs, because it is transpiled as a Server script and
        // its assertServer ops reject in Editor mode.
        let run_result = world.run_f_in_control_mode(WorldControlMode::Server, |world| {
            std::thread::scope(
                |thread_scope| match thread_scope.spawn(|| runTemporaryScriptOnCurrentThread(world, temp_script_id)).join() {
                    Ok(run_result) => run_result,
                    Err(_) => Err(anyhow::anyhow!("Temporary script thread panicked.")),
                },
            )
        });

        // remove script from world
        world.removeObject(temp_script_id)?;
        world.endTweensAtTheirTargets()?;
        run_result
    })
}

#[tokio::main(flavor = "current_thread")]
async fn runTemporaryScriptOnCurrentThread(world: &mut World, temp_script_id: ObjectId) -> anyhow::Result<Vec<JsConsoleLogEntry>> {
    fn register_world_scripts(script_manager_rc: &Rc<RefCell<ScriptManager>>, world: &World) -> anyhow::Result<()> {
        let mut script_object_ids = world
            ._kindOfObjectId
            .get_keys(&ObjectKind::Script)
            .map(|object_ids| object_ids.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        script_object_ids.sort_unstable();

        for script_object_id in script_object_ids {
            script_manager_rc.borrow_mut().add_user_script(&script_object_id, world)?;
        }

        Ok(())
    }

    let run_result = async {
        let (mut jsRuntime, script_manager_rc) = initializeJsRuntime(JsRuntimeInitialization::NoNetworking, None).await?;
        script_manager_rc.borrow_mut().start_recording_temp_script_console_log_entries();

        register_world_scripts(&script_manager_rc, world)?;

        beforeRunJS(&mut jsRuntime, world, 0.0, Some(0.0));

        let run_result = JsCpuWatchdog::run(&mut jsRuntime, TEMPORARY_SCRIPT_CPU_BUDGET, async |jsRuntime| {
            loadKubeTsApi(jsRuntime).await?;
            let script_launcher_source = {
                let script_manager = script_manager_rc.borrow();
                let script_url = script_manager
                    .get_script_url(&temp_script_id)
                    .ok_or_else(|| anyhow::anyhow!("Temporary script module url was not registered."))?;
                format!("import {};", serde_json::to_string(script_url.as_str())?)
            };
            let script_launcher_url = resolve_url("game:/__internal__/temporary_script_launcher")?;
            let script_launcher_module_id = jsRuntime.load_side_es_module_from_code(&script_launcher_url, script_launcher_source).await?;
            evaluateTemporaryScriptModule(jsRuntime, script_launcher_module_id).await?;
            Ok(())
        })
        .await;

        afterRunJS(&mut jsRuntime);

        let console_log_entries = script_manager_rc.borrow_mut().take_temp_script_console_log_entries();
        for console_log_entry in &console_log_entries {
            if console_log_entry.log_level == "error" && console_log_entry.message.contains("[Unhandled JS error]") {
                return Err(anyhow::anyhow!("Temporary script failed: {}", console_log_entry.message));
            }
        }

        run_result?;
        Ok(console_log_entries)
    }
    .await;

    run_result
}

async fn stepEventLoop(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: &mut mpsc::UnboundedReceiver<JsCallbackInputs>,
) -> anyhow::Result<()> {
    // Signal all scripts that were waiting to be resolved in the next poll, so they can continue.
    script_manager_rc.borrow_mut().signal_all_awaiting_next_ticks(world);

    // after we signal, yield so that the awaiting items can run
    tokio::task::yield_now().await;

    // Dispatch queued JS callback inputs first so Rust-owned systems can notify JS before the next event-loop step.
    let queued_js_callback_inputs = std::iter::from_fn(|| js_callback_inputs_rx.try_recv().ok()).collect();
    callJsCallbacks(jsRuntime, script_manager_rc, queued_js_callback_inputs)?;

    // Start new scripts
    let res = flushAndStartNewlyAddedScripts(jsRuntime, world, script_manager_rc).await;
    if let Err(err) = res {
        log::error!("stepEventLoop: error starting newly added scripts: {}", err);
    }

    JsCpuWatchdog::run(jsRuntime, JS_CPU_BUDGET, async |jsRuntime| pollEventLoop(jsRuntime).await).await?;

    Ok(())
}

// -------------- Entrypoints --------------

// On Intel and AMD Linux machines with memory protection keys, V8 locks its memory to the thread that set it up and to threads
// started from that thread afterwards. Any other thread that makes a runtime crashes with a segfault. A program that runs scripts
// on several threads calls this before it starts any thread, so every thread it starts can use V8.
pub fn initializeV8() {
    JsRuntime::init_platform(None);
}

pub enum JsRuntimeInitialization {
    NoNetworking,
    ServerFnConnection(Connection),
    ClientFnConnections(Arc<MultiThreadHashMap<ObjectId, Connection>>),
}

include!(concat!(env!("OUT_DIR"), "/code_of_deno_js_file.rs"));

// deno_core only keeps the paths of an extension's JS files on the machine that built it, for making a startup snapshot. We
// don't use one, so it would read them from those paths when a runtime starts. build.rs puts the files in our binary instead.
fn embedDenoJsFiles(mut extension: Extension) -> anyhow::Result<Extension> {
    let embed = |files: &[ExtensionFileSource]| {
        files
            .iter()
            .map(|file| {
                let (_, code) = CODE_OF_DENO_JS_FILE
                    .iter()
                    .find(|(specifier, _)| *specifier == file.specifier)
                    .ok_or_else(|| anyhow::anyhow!("build.rs did not embed {}", file.specifier))?;
                Ok(ExtensionFileSource::new(file.specifier, *code))
            })
            .collect::<anyhow::Result<Vec<_>>>()
    };
    extension.lazy_loaded_esm_files = embed(&extension.lazy_loaded_esm_files)?.into();
    extension.lazy_loaded_js_files = embed(&extension.lazy_loaded_js_files)?.into();
    Ok(extension)
}

pub fn createJsExtensions() -> anyhow::Result<Vec<Extension>> {
    let blob_store_arc = Arc::new(BlobStore::default());
    Ok(vec![
        embedDenoJsFiles(deno_webidl::deno_webidl::init())?,
        embedDenoJsFiles(deno_web::deno_web::init(blob_store_arc, None, false, InMemoryBroadcastChannel::default()))?,
        ourCustomJsOpsExtension(),
        denoWebRequiredImportsExtension(),
    ])
}

pub async fn initializeJsRuntime(
    js_inputs: JsRuntimeInitialization,
    console_log_callback: Option<JsConsoleLogCallback>,
) -> anyhow::Result<(JsRuntime, Rc<RefCell<ScriptManager>>)> {
    // deno_core's op driver spawns unfinished async ops through deno_unsync, which is only sound on a
    // current-thread runtime. On a multi-thread one the failure is a panic inside an `extern "C"` v8
    // callback, i.e. an un-unwindable abort that takes the process down with no chance to save. Refuse
    // here instead, where it is still an ordinary error.
    if let Ok(runtime_handle) = tokio::runtime::Handle::try_current() {
        if runtime_handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::CurrentThread {
            return Err(anyhow::anyhow!(
                "The JS runtime must be built on a current-thread tokio runtime, but this one is {:?}. \
                 Give it its own thread, the way runTemporaryScriptOnCurrentThread does.",
                runtime_handle.runtime_flavor()
            ));
        }
    }

    let mut script_manager = ScriptManager::new(console_log_callback);
    script_manager.add_kube_ts_api()?;
    script_manager.add_kube_ts_api_jsx_runtime()?;
    let script_manager_rc = Rc::new(RefCell::new(script_manager)); // rc lets code of url be sent from manager to moduleloader
    let module_loader_rc = Rc::new(OurCustomModuleLoader::new(script_manager_rc.clone()));

    // 1GB in-memory storage limit
    let deno_params = deno_core::v8::CreateParams::default().heap_limits(0, 1 * 1024 * 1024 * 1024);

    let mut jsRuntime = JsRuntime::new(RuntimeOptions {
        module_loader: Some(module_loader_rc),
        extensions: createJsExtensions()?,
        create_params: Some(deno_params),
        ..Default::default()
    });

    jsRuntime.op_state().borrow_mut().put(JsCpuWatchdog::new(&mut jsRuntime)?);
    // very important: allows ops to access these structs
    jsRuntime.op_state().borrow_mut().put(JsRuntimeWorldPtr::null());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeSimMs::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeServerFnCaller::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeCameraOrientation::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeMouseRay::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeCameraZoom::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeDesiredCameraZoom::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimeClientWorldDelay::new());
    jsRuntime.op_state().borrow_mut().put(JsRuntimePlayerPings::new());
    jsRuntime.op_state().borrow_mut().put(script_manager_rc.clone()); // ops (like __tickInternal__) access ScriptManager through OpState
    match js_inputs {
        JsRuntimeInitialization::NoNetworking => {}
        JsRuntimeInitialization::ServerFnConnection(server_fn_connection) => {
            jsRuntime.op_state().borrow_mut().put(server_fn_connection);
        }
        JsRuntimeInitialization::ClientFnConnections(connection_of_player_object_id_arc) => {
            jsRuntime.op_state().borrow_mut().put(connection_of_player_object_id_arc);
        }
    }

    // Route top-level module throws through one handler so script failures do not tear down Play mode.
    // Timer, microtask, and other uncaught callback errors use a separate exception lane in this embedder, so catch that through Deno.core too.
    // this apparently catches top level errors and lets other scripts run even when this script failed.
    jsRuntime.execute_script(
        "game:/__internal__/runtime_unhandled_error_handlers",
        r#"
const formatUnhandledJsError = (value) => {
    if (value instanceof Error) {
        return value.stack ?? `${value.name}: ${value.message}`;
    }
    return String(value);
};

Deno.core.setReportExceptionCallback((error) => {
    console.error("[Unhandled JS error]", formatUnhandledJsError(error));
});

Deno.core.setUnhandledPromiseRejectionHandler((_promise, rejection) => {
    console.error("[Unhandled JS error]", formatUnhandledJsError(rejection));
    return true;
});
"#,
    )?;

    // deno_core stops the event loop with an error when a top-level await can't finish and no timer or op is pending. A script
    // can wait on a game event for as long as it likes, so keep one timer that never fires. It is unref'd, so it never keeps the
    // event loop busy.
    jsRuntime.execute_script(
        "game:/__internal__/keep_one_timer",
        "Deno.core.createSystemInterval(() => {}, 2147483647, false);",
    )?;

    Ok((jsRuntime, script_manager_rc))
}

// run all initial scripts (kube-ts-api and initial user scripts)
pub async fn initializeJsAndRunInitialScripts(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    live_client_sim_ms: Option<f64>,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
) -> anyhow::Result<()> {
    beforeRunJS(jsRuntime, world, est_live_server_sim_ms, live_client_sim_ms);

    let result = async {
        // we need to load the kube-ts-api first because it initializes some global stuff
        loadKubeTsApi(jsRuntime).await?;
        flushAndStartNewlyAddedScripts(jsRuntime, world, script_manager_rc).await?;
        JsCpuWatchdog::run(jsRuntime, JS_CPU_BUDGET, async |jsRuntime| pollEventLoop(jsRuntime).await).await?;
        Ok(())
    }
    .await;

    // Startup reports each script's failure above and continues starting the other scripts.
    afterRunJS(jsRuntime);

    result
}

pub async fn runPendingJS(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    live_client_sim_ms: Option<f64>,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    js_callback_inputs_rx: &mut mpsc::UnboundedReceiver<JsCallbackInputs>,
) -> anyhow::Result<()> {
    beforeRunJS(jsRuntime, world, est_live_server_sim_ms, live_client_sim_ms);
    let result = stepEventLoop(jsRuntime, world, script_manager_rc, js_callback_inputs_rx).await;
    afterRunJS(jsRuntime);
    result
}

// Runs JS callbacks right away instead of in the next runPendingJS. The client play loop uses this to run
// clientCharacterService's step callbacks around each physics step.
pub fn runJsCallbacks(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    live_client_sim_ms: Option<f64>,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    js_callback_inputs: Vec<JsCallbackInputs>,
) -> anyhow::Result<()> {
    beforeRunJS(jsRuntime, world, est_live_server_sim_ms, live_client_sim_ms);
    let result = callJsCallbacks(jsRuntime, script_manager_rc, js_callback_inputs);
    afterRunJS(jsRuntime);
    result
}

pub async fn callJsFn(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    live_client_sim_ms: Option<f64>,
    js_fn: &v8::Global<v8::Function>,
    js_args: &str,
) -> anyhow::Result<String> {
    beforeRunJS(jsRuntime, world, est_live_server_sim_ms, live_client_sim_ms);

    let result = JsCpuWatchdog::run(jsRuntime, JS_CPU_BUDGET, async |jsRuntime| {
        // Registered JS handlers decode the arguments and encode the result with SuperJSON.
        let args = {
            deno_core::scope!(scope, jsRuntime);
            let args: v8::Local<v8::Value> = v8::String::new(scope, js_args)
                .ok_or_else(|| anyhow::anyhow!("Could not create remote argument string"))?
                .into();
            [v8::Global::new(scope, args)]
        };

        let call = jsRuntime.call_with_args(js_fn, &args);
        let js_result = jsRuntime.with_event_loop_promise(call, PollEventLoopOptions::default()).await?;
        readRemoteFnResult(jsRuntime, js_result)
    })
    .await;

    afterRunJS(jsRuntime);
    result
}

fn readRemoteFnResult(jsRuntime: &mut JsRuntime, js_result: v8::Global<v8::Value>) -> anyhow::Result<String> {
    deno_core::scope!(scope, jsRuntime);
    let js_result =
        v8::Local::<v8::String>::try_from(v8::Local::new(scope, js_result)).map_err(|_| anyhow::anyhow!("Expected an encoded remote function result"))?;
    Ok(js_result.to_rust_string_lossy(scope))
}

// A 'use server' call from when it starts until its promise settles.
pub type ServerFnCall = LocalBoxFuture<'static, Result<v8::Global<v8::Value>, Box<JsError>>>;

// Runs a 'use server' call up to its first await. The rest runs on the event loop with the other scripts, so a call that
// waits never holds up the tick.
pub fn startRegisteredServerFn(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    server_fn_caller_object_id: ObjectId,
    fn_id: &str,
    js_args: &str,
) -> anyhow::Result<ServerFnCall> {
    let server_fn = {
        let script_manager = script_manager_rc.borrow();
        let Some(server_fn) = script_manager.get_server_js_fn(fn_id) else {
            return Err(anyhow::anyhow!("Server function not found: {}", fn_id));
        };
        server_fn.clone()
    };

    let opStateRc = jsRuntime.op_state();
    opStateRc
        .borrow_mut()
        .borrow_mut::<JsRuntimeServerFnCaller>()
        .set(Some(server_fn_caller_object_id));
    beforeRunJS(jsRuntime, world, est_live_server_sim_ms, None);
    let result = JsCpuWatchdog::run_sync(jsRuntime, JS_CPU_BUDGET, |jsRuntime| {
        let args = {
            deno_core::scope!(scope, jsRuntime);
            let args: v8::Local<v8::Value> = v8::String::new(scope, js_args)
                .ok_or_else(|| anyhow::anyhow!("Could not create remote argument string"))?
                .into();
            [v8::Global::new(scope, args)]
        };
        Ok(jsRuntime.call_with_args(&server_fn, &args).boxed_local())
    });
    afterRunJS(jsRuntime);
    opStateRc.borrow_mut().borrow_mut::<JsRuntimeServerFnCaller>().set(None);
    result
}

// The event loop settles calls, so the result of a call shows up after a runPendingJS.
pub fn pollServerFnCall(jsRuntime: &mut JsRuntime, server_fn_call: &mut ServerFnCall) -> Poll<anyhow::Result<String>> {
    let Poll::Ready(js_result) = server_fn_call.poll_unpin(&mut Context::from_waker(Waker::noop())) else {
        return Poll::Pending;
    };
    Poll::Ready(
        js_result
            .map_err(anyhow::Error::from)
            .and_then(|js_result| readRemoteFnResult(jsRuntime, js_result)),
    )
}

pub async fn callRegisteredGuiHandler(
    jsRuntime: &mut JsRuntime,
    world: &mut World,
    est_live_server_sim_ms: f64,
    live_client_sim_ms: Option<f64>,
    script_manager_rc: &Rc<RefCell<ScriptManager>>,
    script_object_id: ObjectId,
    gui_key: &str,
    handler_id: &str,
    button: Option<&str>,
) -> anyhow::Result<()> {
    let handler = {
        let script_manager = script_manager_rc.borrow();
        let Some(handler) = script_manager.gui_handler(&script_object_id, gui_key, handler_id) else {
            return Err(anyhow::anyhow!(
                "GUI handler not found for script {} gui {} handler {}",
                script_object_id,
                gui_key,
                handler_id
            ));
        };
        handler.clone()
    };

    beforeRunJS(jsRuntime, world, est_live_server_sim_ms, live_client_sim_ms);

    let result = JsCpuWatchdog::run(jsRuntime, JS_CPU_BUDGET, async |jsRuntime| {
        // onMouseDown and onMouseUp handlers get the button, and the others get nothing.
        let args = match button {
            Some(button) => {
                deno_core::scope!(scope, jsRuntime);
                let button: v8::Local<v8::Value> = v8::String::new(scope, button)
                    .ok_or_else(|| anyhow::anyhow!("Could not create GUI handler button string"))?
                    .into();
                vec![v8::Global::new(scope, button)]
            }
            None => Vec::new(),
        };
        let call = jsRuntime.call_with_args(&handler, &args);
        let _ = jsRuntime.with_event_loop_promise(call, PollEventLoopOptions::default()).await?;
        Ok(())
    })
    .await;

    afterRunJS(jsRuntime);
    result
}
