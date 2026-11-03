use super::{CatalogItemEditArgs, CatalogItemPublishArgs, ReadAiHeadlessPlaytestLogsArgs, ReadScriptFilesArgs, SetGameSettingsArgs};
use crate::api::protocol_for_assets::{Asset, AssetSummary, AssetPublishRequest, AssetSearchRequest};
use glam::Vec3;
use std::{
    pin::Pin,
    sync::{Arc, LazyLock, RwLock as StdRwLock},
};

// the tool registry uses the CLIENT_EDITOR_TOOL_HANDLERS.
// it's so the tools can access some client (editor) functionality from the shared crate.
// the client registers the functions and the shared crate calls them.

pub type ClientToolFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>>;
pub type ClientToolTextFuture<'a> = ClientToolFuture<'a, String>;

// Shared tools call the editor's asset implementation directly, without a local HTTP request.
pub trait AssetsHandler: Send + Sync {
    fn publish<'a>(&'a self, request: AssetPublishRequest) -> ClientToolFuture<'a, Asset>;
    fn search<'a>(&'a self, request: &'a AssetSearchRequest) -> ClientToolFuture<'a, Vec<AssetSummary>>;
    fn download<'a>(&'a self, asset_id: i64) -> ClientToolFuture<'a, Asset>;
    fn record_import<'a>(&'a self, asset_id: i64, version_num: i64) -> ClientToolFuture<'a, ()>;
}

pub trait ScreenshotHandler: Send + Sync {
    fn capture<'a>(&'a self, camera_position: Vec3, target: Vec3, width: u32, height: u32) -> ClientToolFuture<'a, Vec<u8>>;
}

pub trait PublishHandler: Send + Sync {
    fn publish<'a>(&'a self) -> ClientToolTextFuture<'a>;
}

// Settings tools call the editor's database operations directly, like asset and publishing tools.
pub trait GameSettingsHandler: Send + Sync {
    fn get<'a>(&'a self) -> ClientToolTextFuture<'a>;
    fn set<'a>(&'a self, args: SetGameSettingsArgs) -> ClientToolTextFuture<'a>;
}

// Catalog tools call the editor's database operations directly, like the settings tools.
pub trait CatalogHandler: Send + Sync {
    fn publish<'a>(&'a self, args: CatalogItemPublishArgs) -> ClientToolTextFuture<'a>;
    fn edit<'a>(&'a self, args: CatalogItemEditArgs) -> ClientToolTextFuture<'a>;
}

// something needs to implement both these things:
pub trait ScriptFilesHandler: Send + Sync {
    fn read_text<'a>(&'a self, args: ReadScriptFilesArgs) -> ClientToolTextFuture<'a>;
}

pub trait HeadlessPlaytestHandler: Send + Sync {
    fn start_server<'a>(&'a self, stored_world_string: String) -> ClientToolTextFuture<'a>;
    fn start_client<'a>(&'a self) -> ClientToolTextFuture<'a>;
    fn stop_client<'a>(&'a self, client_id: u8) -> ClientToolTextFuture<'a>;
    fn read_logs(&self, args: ReadAiHeadlessPlaytestLogsArgs) -> anyhow::Result<String>;
    fn capture_screenshot<'a>(&'a self, client_id: u8, width: u32, height: u32) -> ClientToolFuture<'a, Vec<u8>>;
    fn stop_all(&self) -> String;
}

// this contains the handlers above
pub struct ClientEditorToolHandlers {
    pub screenshot_handler: Arc<dyn ScreenshotHandler>,
    pub assets_handler: Arc<dyn AssetsHandler>,
    pub publish_handler: Arc<dyn PublishHandler>,
    pub game_settings_handler: Arc<dyn GameSettingsHandler>,
    pub catalog_handler: Arc<dyn CatalogHandler>,
    pub script_files_handler: Arc<dyn ScriptFilesHandler>,
    pub headless_playtest_handler: Arc<dyn HeadlessPlaytestHandler>,
}

static CLIENT_EDITOR_TOOL_HANDLERS: LazyLock<StdRwLock<Option<Arc<ClientEditorToolHandlers>>>> = LazyLock::new(|| StdRwLock::new(None));

impl ClientEditorToolHandlers {
    // the client should call this every time one of the handlers changes
    pub fn set_curr(client_editor_tool_handlers: Option<Arc<ClientEditorToolHandlers>>) {
        let Ok(mut client_editor_tool_handlers_lock) = CLIENT_EDITOR_TOOL_HANDLERS.write() else {
            log::error!("set_curr: Failed to set client editor tool handler: lock is poisoned");
            return;
        };
        *client_editor_tool_handlers_lock = client_editor_tool_handlers;
    }

    pub fn get_curr() -> anyhow::Result<Arc<ClientEditorToolHandlers>> {
        let Ok(client_editor_tool_handlers_lock) = CLIENT_EDITOR_TOOL_HANDLERS.read() else {
            anyhow::bail!("get_curr: Failed to read client editor tool handler: lock is poisoned");
        };
        let client_editor_tool_handlers = client_editor_tool_handlers_lock.clone();
        drop(client_editor_tool_handlers_lock);
        client_editor_tool_handlers.ok_or_else(|| anyhow::anyhow!("this tool is only available inside the editor client."))
    }
}
