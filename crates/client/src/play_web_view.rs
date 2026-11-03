// The play webview (packages/play-webview) draws the GUI over the whole game window. The game reads keys and
// the mouse from the OS itself (see WindowInput), and the page tells it where its UI is and whether chat has
// focus, so the game never waits on the page. The matching TS types are in its src/hostBridge.ts.

use crate::play_runner::game_chat::GameChatState;
use crate::play_runner::play::GuiTriggeredHandler;
use crate::splash_runner::SplashScreenContent;
use game_types::object_types::ObjectId;
use indexmap::IndexMap;
use render_core::fonts::INTER_VARIABLE_FONT_BYTES;
use serde::{Deserialize, Serialize};
use shared::game::gui_types::{GuiRenderedRoot, ScriptGuiKey};
use tokio::sync::mpsc;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    window::Window,
};

// Every file the page loads, as (path, content type, bytes). The editor server draws players' GUI from these too.
pub const PLAY_WEB_VIEW_FILES: [(&str, &str, &[u8]); 5] = [
    (
        "index.html",
        "text/html; charset=utf-8",
        include_bytes!("../../../packages/play-webview/index.html"),
    ),
    (
        "play-webview.bundle.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../../packages/play-webview/dist/play-webview.bundle.js"),
    ),
    ("fonts/InterVariable.ttf", "font/ttf", INTER_VARIABLE_FONT_BYTES),
    (
        "fonts/InterVariable-Italic.woff2",
        "font/woff2",
        include_bytes!("../../../assets/fonts/InterVariable-Italic.woff2"),
    ),
    (
        "fonts/GeistMono-Regular.ttf",
        "font/ttf",
        include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf"),
    ),
];

// The file a page request asks for, as (content type, bytes). The page itself is at the root.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn get_play_web_view_file(path: &str) -> Option<(&'static str, &'static [u8])> {
    let path = match path.trim_start_matches('/') {
        "" => "index.html",
        path => path,
    };
    PLAY_WEB_VIEW_FILES
        .iter()
        .find(|(file_path, _, _)| *file_path == path)
        .map(|(_, content_type, bytes)| (*content_type, *bytes))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostToWebViewMessage<'a> {
    ShowSplashScreen(&'a SplashScreenContent),
    ShowPlayScreen,
    SetScriptGuiRoots(Vec<ScriptGuiRoot<'a>>),
    SetGameChat(&'a GameChatState),
    SetPlayersList(&'a [PlayersListEntry]),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptGuiRoot<'a> {
    pub script_object_id: ObjectId,
    pub gui_key: &'a str,
    pub root: &'a GuiRenderedRoot,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayersListEntry {
    pub username: String,
    pub is_local_player: bool,
    pub ping_ms: Option<u16>,
}

// The page stacks roots in this order, so the GUI shown last is drawn on top.
pub fn get_script_gui_roots(gui_root_of_script_gui_key: &IndexMap<ScriptGuiKey, GuiRenderedRoot>) -> Vec<ScriptGuiRoot<'_>> {
    gui_root_of_script_gui_key
        .iter()
        .map(|((script_object_id, gui_key), root)| ScriptGuiRoot {
            script_object_id: *script_object_id,
            gui_key,
            root,
        })
        .collect()
}

// A rectangle of the page in physical pixels from the top left of the window, the same as window positions.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct UiRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl UiRect {
    pub fn contains(&self, position: PhysicalPosition<f64>) -> bool {
        let UiRect { x, y, width, height } = *self;
        (x..x + width).contains(&position.x) && (y..y + height).contains(&position.y)
    }
}

// Where the page's UI is, so the game can tell right away which presses and scrolls are its own.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiRects {
    pub ui_only_rects: Vec<UiRect>,    // buttons, inputs and the chat
    pub scrollable_rects: Vec<UiRect>, // panels that can scroll
}

impl UiRects {
    // The page keeps presses of every button, so a right-drag that starts on a button does not turn the camera.
    pub fn should_game_get_press(&self, position: PhysicalPosition<f64>) -> bool {
        !self.ui_only_rects.iter().any(|ui_only_rect| ui_only_rect.contains(position))
    }

    pub fn should_game_get_scroll(&self, position: PhysicalPosition<f64>) -> bool {
        !self
            .ui_only_rects
            .iter()
            .chain(&self.scrollable_rects)
            .any(|ui_rect| ui_rect.contains(position))
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WebViewToHostMessage {
    Ready,
    SetUiRects(UiRects),
    SetIsChatFocused(bool),
    SetIsMenuOpen(bool),
    GuiHandler(GuiTriggeredHandler),
    SendGameChatMessage { message: String },
    SetVolume(f32),
    SetGraphicsQuality(GraphicsQuality),
    LeaveGame,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphicsQuality {
    Low,
    High,
}

#[cfg(target_os = "macos")]
mod macos_web_view;
#[cfg(target_os = "macos")]
use self::macos_web_view::MacosWebView as OsWebView;
#[cfg(target_os = "windows")]
mod windows_web_view;
#[cfg(target_os = "windows")]
use self::windows_web_view::WindowsWebView as OsWebView;

pub struct PlayWebView {
    web_view: OsWebView,
    web_view_to_host_message_rx: mpsc::UnboundedReceiver<WebViewToHostMessage>,
    // A script run before the page has loaded runs in the blank page it replaces, so scripts wait here until the page says it is ready.
    is_page_ready: bool,
    scripts_waiting_for_page: Vec<String>,
}

impl PlayWebView {
    pub fn new(window: &Window) -> anyhow::Result<Self> {
        let (web_view_to_host_message_tx, web_view_to_host_message_rx) = mpsc::unbounded_channel();
        // this is the receiver that lets the page send messages to rust. in ts write ipc.postMessage to send a message here
        let receive_page_message = move |page_message: &str| match serde_json::from_str::<WebViewToHostMessage>(page_message) {
            Ok(web_view_to_host_message) => {
                let _ = web_view_to_host_message_tx.send(web_view_to_host_message);
            }
            Err(error) => log::debug!("Dropping play webview message {}: {}", page_message, error),
        };
        let web_view = OsWebView::new(window, receive_page_message)?;
        web_view.focus();
        Ok(Self {
            web_view,
            web_view_to_host_message_rx,
            is_page_ready: false,
            scripts_waiting_for_page: Vec::new(),
        })
    }

    pub fn resize(&self, window_size: PhysicalSize<u32>) {
        self.web_view.resize(window_size);
    }

    // Keyboard input only reaches the page while it has focus, and on Windows the window takes it back whenever it is focused.
    pub fn focus(&self) {
        self.web_view.focus();
    }

    pub fn send(&mut self, host_to_web_view_message: &HostToWebViewMessage) {
        let host_to_web_view_message_json = match serde_json::to_string(host_to_web_view_message) {
            Ok(host_to_web_view_message_json) => host_to_web_view_message_json,
            Err(error) => {
                log::error!("Failed to encode play webview message: {}", error);
                return;
            }
        };
        let script = format!("receiveFromHost({host_to_web_view_message_json})");
        if !self.is_page_ready {
            self.scripts_waiting_for_page.push(script);
            return;
        }
        if let Err(error) = self.web_view.evaluate_script(script.as_str()) {
            log::warn!("Failed to run play webview script: {}", error);
        }
    }

    // The page saying it is ready is handled here, so callers only ever see the page's other messages.
    pub fn try_recv(&mut self) -> Option<WebViewToHostMessage> {
        loop {
            let web_view_to_host_message = self.web_view_to_host_message_rx.try_recv().ok()?;
            let WebViewToHostMessage::Ready = web_view_to_host_message else {
                return Some(web_view_to_host_message);
            };
            self.is_page_ready = true;
            for script in std::mem::take(&mut self.scripts_waiting_for_page) {
                if let Err(error) = self.web_view.evaluate_script(script.as_str()) {
                    log::warn!("Failed to run play webview script: {}", error);
                }
            }
        }
    }
}

// The desktop client only ships on macOS and Windows. Other systems build the client for headless
// playtest players, which have no window and so never make one of these.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct OsWebView {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl OsWebView {
    fn new(_window: &Window, _receive_page_message: impl Fn(&str) + 'static) -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("The play webview only runs on macOS and Windows."))
    }

    fn resize(&self, _window_size: PhysicalSize<u32>) {}

    fn focus(&self) {}

    fn evaluate_script(&self, _script: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
