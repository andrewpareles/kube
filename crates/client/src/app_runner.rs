mod macos_input_monitor;
mod window_input;
mod windows_input_monitor;

pub use self::macos_input_monitor::MacosInputMonitor;
#[cfg(target_os = "macos")]
pub use self::macos_input_monitor::{MacosEventRoute, MacosInputState, route_macos_event};
pub use self::window_input::WindowInput;
pub use self::windows_input_monitor::{TouchpadFinger, TouchpadFrame, TouchpadGestures, WindowsInputMonitor, is_window_in_front};
use crate::app_updater::quit_and_install_update_if_available;
use crate::deep_link_receiver::DeepLinkReceiver;
use crate::play_runner::{PlayRunner, PlayRunnerInputs};
use crate::play_web_view::{GraphicsQuality, PlayWebView, WebViewToHostMessage};
use crate::simple_renderer::SimpleRenderer;
use crate::splash_runner::{SplashScreenContent, SplashScreenRunner};
use crate::{
    parse_client_mode::{DeepLinkParams, DeepLinkParseError, parse_deep_link},
    play_runner::{get_play_app_runner_inputs_and_start_bg_tasks_to_official_server, get_play_app_runner_inputs_and_start_bg_tasks_to_playtest_server},
};
use render_core::renderer::{RenderFrame, SimpleViewportRegion};
use shared::env::env_vars::SITE_URL;
use shared::s3_storage::PublicFile;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{DeviceEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents},
    window::{UserAttentionType, Window, WindowId},
};

pub enum AppRunnerInputs {
    Splash(SplashScreenContent),
    PlayMode(PlayRunnerInputs),
}
impl AppRunnerInputs {
    // the plain wait shown while a world downloads or a play tab connects. The game's own name is
    // shown when the link carried it, so the screen says what is opening.
    pub fn from_loading(thumbnail_game_name: Option<String>, thumbnail_url: Option<String>) -> Self {
        Self::Splash(SplashScreenContent {
            title_text: thumbnail_game_name.unwrap_or_else(|| "Loading".to_string()),
            sub_text: None,
            thumbnailUrl: thumbnail_url,
            is_loading: true,
        })
    }
    pub fn from_error(title: &'static str, sub_text: String) -> Self {
        Self::Splash(SplashScreenContent {
            title_text: title.to_string(),
            sub_text: Some(sub_text),
            thumbnailUrl: None,
            is_loading: false,
        })
    }
}

// inputs are converted into a runner eventually
enum CurrentAppRunner {
    Splash(SplashScreenRunner),
    PlayMode(PlayRunner),
}
impl CurrentAppRunner {
    pub fn new(inputs: AppRunnerInputs, play_web_view: &mut PlayWebView) -> Self {
        let app_runner = match inputs {
            AppRunnerInputs::Splash(splash_screen_content) => {
                let runner = SplashScreenRunner::new(splash_screen_content, play_web_view);
                CurrentAppRunner::Splash(runner)
            }
            AppRunnerInputs::PlayMode(play_mode_runner_inputs) => match PlayRunner::new(play_mode_runner_inputs, play_web_view) {
                Ok(runner) => CurrentAppRunner::PlayMode(runner),
                Err(error) => {
                    let runner = SplashScreenRunner::new(
                        SplashScreenContent {
                            title_text: "Sorry, something unexpected happened".to_string(),
                            sub_text: Some(error.to_string()),
                            thumbnailUrl: None,
                            is_loading: false,
                        },
                        play_web_view,
                    );
                    CurrentAppRunner::Splash(runner)
                }
            },
        };
        app_runner
    }
}

enum AppWindowState {
    HasWindow {
        current_runner: CurrentAppRunner,
        window_arc: Arc<Window>,
        renderer: SimpleRenderer,
        play_web_view: PlayWebView,
    },
    AwaitingWindow {},
}

pub struct AppRunner {
    window_state: AppWindowState,
    app_runner_inputs_rx: mpsc::UnboundedReceiver<AppRunnerInputs>,
    macos_input_monitor: Option<MacosInputMonitor>,
    windows_input_monitor: Option<WindowsInputMonitor>,
}

impl AppRunner {
    // create this initially, then send it updates using runner_inputs_tx
    pub fn new_window(runner_inputs_rx: mpsc::UnboundedReceiver<AppRunnerInputs>) -> Self {
        Self {
            window_state: AppWindowState::AwaitingWindow {},
            app_runner_inputs_rx: runner_inputs_rx,
            macos_input_monitor: None,
            windows_input_monitor: None,
        }
    }

    // internal fn because of annoying borrow rules
    fn cleanup_current_runner(current_runner: &mut CurrentAppRunner, window: &Window) {
        match current_runner {
            CurrentAppRunner::PlayMode(play_mode_runner) => {
                play_mode_runner.cleanup(window);
            }
            CurrentAppRunner::Splash(_) => {}
        }
    }

    pub fn cleanup(&mut self) {
        match &mut self.window_state {
            AppWindowState::AwaitingWindow {} => {}
            AppWindowState::HasWindow {
                current_runner, window_arc, ..
            } => {
                Self::cleanup_current_runner(current_runner, window_arc.as_ref());
            }
        }
    }

    pub fn focus_window_if_exists(&mut self) {
        let AppWindowState::HasWindow { window_arc, .. } = &mut self.window_state else {
            return;
        };
        window_arc.set_minimized(false);
        window_arc.focus_window();
        window_arc.request_user_attention(Some(UserAttentionType::Critical));
    }

    pub fn create_and_run_event_loop_forever(&mut self) {
        fn start_event_loop_forever_internal(app_client: &mut AppRunner) -> anyhow::Result<()> {
            let event_loop = winit::event_loop::EventLoop::new()?;
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.run_app(app_client)?;
            Ok(())
        }
        let exit_res = start_event_loop_forever_internal(self);
        if let Err(e) = exit_res {
            log::error!("Client runtime exited with error: {}", e);
        }
    }

    // internal helpers:
    fn try_process_rx_updates(&mut self) {
        if self.app_runner_inputs_rx.is_empty() {
            return;
        }

        let AppWindowState::HasWindow {
            window_arc,
            play_web_view,
            current_runner: runner,
            ..
        } = &mut self.window_state
        else {
            return;
        };

        let Ok(received_runner_inputs) = self.app_runner_inputs_rx.try_recv() else {
            return;
        };

        Self::cleanup_current_runner(runner, window_arc.as_ref());
        *runner = CurrentAppRunner::new(received_runner_inputs, play_web_view);
        if let Some(macos_input_monitor) = self.macos_input_monitor.as_ref() {
            macos_input_monitor.set_is_chat_focused(false);
        }
        window_arc.request_redraw();
        self.focus_window_if_exists();
    }

    // internal fn because of annoying borrow rules
    fn onRequestRedrawCurrentRunner(
        current_runner: &mut CurrentAppRunner,
        renderer: &mut SimpleRenderer,
        viewport_region: SimpleViewportRegion,
        render_frame: &mut RenderFrame,
        play_web_view: &mut PlayWebView,
        window: &Window,
    ) {
        match current_runner {
            CurrentAppRunner::PlayMode(play_mode_runner) => {
                play_mode_runner.onRequestRedraw(renderer, viewport_region, render_frame, play_web_view, window);
            }
            CurrentAppRunner::Splash(splash_screen_runner) => {
                splash_screen_runner.onRequestRedraw(renderer, render_frame);
            }
        }
    }
    // internal fn because of annoying borrow rules
    fn onInputWindowEventCurrentRunner(current_runner: &mut CurrentAppRunner, event: &WindowEvent, window: &Window) {
        match current_runner {
            CurrentAppRunner::PlayMode(play_mode_runner) => {
                play_mode_runner.onInputWindowEvent(&event, window);
            }
            CurrentAppRunner::Splash(_) => {}
        }
    }
    fn onInputDeviceEventCurrentRunner(current_runner: &mut CurrentAppRunner, event: &DeviceEvent) {
        match current_runner {
            CurrentAppRunner::PlayMode(play_mode_runner) => {
                play_mode_runner.onInputDeviceEvent(event);
            }
            CurrentAppRunner::Splash(_) => {}
        }
    }

    // The page reports where its UI is, whether chat has focus, button clicks and chat messages.
    fn handle_play_web_view_messages(&mut self, event_loop: &ActiveEventLoop) {
        let AppWindowState::HasWindow {
            current_runner,
            renderer,
            play_web_view,
            ..
        } = &mut self.window_state
        else {
            return;
        };
        while let Some(web_view_to_host_message) = play_web_view.try_recv() {
            if let WebViewToHostMessage::LeaveGame = web_view_to_host_message {
                event_loop.exit();
            }
            if let WebViewToHostMessage::SetGraphicsQuality(graphics_quality) = web_view_to_host_message {
                renderer.set_render_scale(match graphics_quality {
                    GraphicsQuality::Low => 0.5,
                    GraphicsQuality::High => 1.0,
                });
            }
            // On macOS the input monitor decides which key presses the page gets, so it follows the chat's focus too.
            if let (WebViewToHostMessage::SetIsChatFocused(is_chat_focused), Some(macos_input_monitor)) =
                (&web_view_to_host_message, self.macos_input_monitor.as_ref())
            {
                macos_input_monitor.set_is_chat_focused(*is_chat_focused);
            }
            match current_runner {
                CurrentAppRunner::PlayMode(play_mode_runner) => {
                    play_mode_runner.onWebViewMessage(web_view_to_host_message);
                }
                CurrentAppRunner::Splash(_) => {}
            }
        }
    }

    // The game reads keys and the mouse from the OS, since the play webview gets them from the window.
    fn handle_window_inputs(&mut self) {
        let AppWindowState::HasWindow {
            current_runner, window_arc, ..
        } = &mut self.window_state
        else {
            return;
        };
        let mut window_inputs = match self.macos_input_monitor.as_ref() {
            Some(macos_input_monitor) => macos_input_monitor.take_window_inputs(),
            None => Vec::new(),
        };
        if let Some(windows_input_monitor) = self.windows_input_monitor.as_ref() {
            window_inputs.extend(windows_input_monitor.take_window_inputs());
        }
        let CurrentAppRunner::PlayMode(play_mode_runner) = current_runner else {
            return;
        };
        for window_input in window_inputs {
            play_mode_runner.onWindowInput(window_input, window_arc.as_ref());
        }
    }
}

impl ApplicationHandler for AppRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        fn get_startup_window_size(startup_monitor_size: PhysicalSize<u32>) -> PhysicalSize<u32> {
            const INITIAL_WINDOW_WIDTH_PORTION_OF_MONITOR: f64 = 0.8;
            const INITIAL_WINDOW_ASPECT_RATIO_WIDTH: u32 = 2;
            const INITIAL_WINDOW_ASPECT_RATIO_HEIGHT: u32 = 1;

            // Pick a large startup size that keeps the requested aspect ratio and still fits on the monitor.
            let target_window_width = (startup_monitor_size.width as f64 * INITIAL_WINDOW_WIDTH_PORTION_OF_MONITOR).round() as u32;
            let max_window_width_from_height =
                startup_monitor_size.height.saturating_mul(INITIAL_WINDOW_ASPECT_RATIO_WIDTH) / INITIAL_WINDOW_ASPECT_RATIO_HEIGHT;
            let startup_window_width = target_window_width
                .min(startup_monitor_size.width)
                .min(max_window_width_from_height)
                .max(INITIAL_WINDOW_ASPECT_RATIO_WIDTH);
            let startup_window_width = startup_window_width - startup_window_width % INITIAL_WINDOW_ASPECT_RATIO_WIDTH;
            let startup_window_height = startup_window_width * INITIAL_WINDOW_ASPECT_RATIO_HEIGHT / INITIAL_WINDOW_ASPECT_RATIO_WIDTH;

            PhysicalSize::new(startup_window_width, startup_window_height)
        }

        // self.try_recv_updates();

        // if they're awaiting window for the first time, create the window and set the app state
        let AppWindowState::AwaitingWindow {} = &mut self.window_state else {
            return;
        };

        // create window
        let mut window_attributes = Window::default_attributes()
            .with_title("Kube.new")
            .with_min_inner_size(LogicalSize::new(800, 450));
        if let Some(startup_monitor) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()) {
            let startup_window_size = get_startup_window_size(startup_monitor.size());
            window_attributes = window_attributes.with_inner_size(startup_window_size);
        }
        let window = match event_loop.create_window(window_attributes) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to create window: {}", e);
                event_loop.exit();
                return;
            }
        };
        let window_arc = Arc::new(window);
        self.macos_input_monitor = MacosInputMonitor::new(window_arc.clone());
        self.windows_input_monitor = WindowsInputMonitor::new(window_arc.clone());
        // Windows does not count mouse input over the play webview as this app's, so the app asks for raw input even
        // while it is not in front, and the Windows monitor keeps what arrives while the window is.
        if self.windows_input_monitor.is_some() {
            event_loop.listen_device_events(DeviceEvents::Always);
        }

        // block on the renderer. we cant render anything without a renderer so theres nothing we can do till we get it.
        let renderer = match pollster::block_on(SimpleRenderer::new(window_arc.clone())) {
            Ok(renderer) => renderer,
            Err(error) => {
                log::error!("Failed to create renderer: {}", error);
                event_loop.exit();
                return;
            }
        };
        let mut play_web_view = match PlayWebView::new(window_arc.as_ref()) {
            Ok(play_web_view) => play_web_view,
            Err(error) => {
                log::error!("Failed to create the play webview: {}", error);
                event_loop.exit();
                return;
            }
        };
        let initial_runner_inputs = match self.app_runner_inputs_rx.try_recv() {
            Ok(initial_runner_inputs) => initial_runner_inputs,
            Err(_) => AppRunnerInputs::from_loading(None, None), // default to loading, we will recv later
        };

        let initial_runner = CurrentAppRunner::new(initial_runner_inputs, &mut play_web_view);
        self.window_state = AppWindowState::HasWindow {
            current_runner: initial_runner,
            window_arc: window_arc.clone(),
            renderer,
            play_web_view,
        };

        self.focus_window_if_exists();
        window_arc.request_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.try_process_rx_updates();
        self.handle_play_web_view_messages(event_loop);
        self.handle_window_inputs();
        // Keep requesting frames from the event loop; child webview paints can interrupt requests made inside a paint callback.
        if let AppWindowState::HasWindow { window_arc, .. } = &self.window_state {
            window_arc.request_redraw();
        }
    }

    // call the runner's onWindowEvent
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        self.try_process_rx_updates();
        // winit on macOS redraws before about_to_wait, so input is also read here to reach the game before the frame
        // instead of after it. The page's messages go first, so presses are checked against its newest UI rects.
        if let WindowEvent::RedrawRequested = event {
            self.handle_play_web_view_messages(event_loop);
            self.handle_window_inputs();
        }

        let AppWindowState::HasWindow {
            current_runner,
            window_arc,
            renderer,
            play_web_view,
        } = &mut self.window_state
        else {
            return;
        };

        // always allow closing the window
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
            return;
        }

        // always allow resizing the window
        if let WindowEvent::Resized(size) = event {
            renderer.resize_wgpu_surface(&size);
            play_web_view.resize(size);
            window_arc.request_redraw();
            return;
        }

        if let WindowEvent::Focused(true) = event {
            play_web_view.focus();
        }

        if let WindowEvent::RedrawRequested = event {
            let window_size = window_arc.inner_size();
            let viewport_region = SimpleViewportRegion {
                x: 0,
                y: 0,
                width: window_size.width,
                height: window_size.height,
            };
            let begin_frame = renderer.begin_frame();
            let (mut render_frame, surface_texture) = match begin_frame {
                Ok((render_frame, surface_texture)) => (render_frame, surface_texture),
                Err(err) => {
                    // this is a normal hiccup (monitor change, wake from sleep, resize)
                    // ask for the next frame so we retry instead of freezing the screen
                    log::warn!("Error beginning frame: {}", err);
                    window_arc.request_redraw();
                    return;
                }
            };
            Self::onRequestRedrawCurrentRunner(current_runner, renderer, viewport_region, &mut render_frame, play_web_view, window_arc.as_ref());

            window_arc.pre_present_notify();
            renderer.finish_frame(render_frame, &surface_texture);
            surface_texture.present();
        } else {
            Self::onInputWindowEventCurrentRunner(current_runner, &event, window_arc.as_ref());
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: winit::event::DeviceId, event: DeviceEvent) {
        // Windows reports keys and buttons through raw input while the play webview has them. The monitor drops what
        // arrives while another app is in front, and what Windows makes up from fingers on the touchpad.
        if let Some(windows_input_monitor) = self.windows_input_monitor.as_ref() {
            if !windows_input_monitor.should_game_get_device_event(&event) {
                return;
            }
            if let AppWindowState::HasWindow {
                current_runner: CurrentAppRunner::PlayMode(play_mode_runner),
                window_arc,
                ..
            } = &mut self.window_state
                && let Some(window_input) = windows_input_monitor.get_window_input_for_device_event(&event)
            {
                play_mode_runner.onWindowInput(window_input, window_arc.as_ref());
            }
        }
        let AppWindowState::HasWindow { current_runner, .. } = &mut self.window_state else {
            return;
        };
        Self::onInputDeviceEventCurrentRunner(current_runner, &event);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.cleanup();
    }
}
// Start the desktop play-link listener.
pub async fn start_txing_app_runner_inputs(app_runner_inputs_tx: mpsc::UnboundedSender<AppRunnerInputs>, should_open_separately: bool) {
    let mut deep_link_receiver = DeepLinkReceiver::new();
    if let Err(error) = deep_link_receiver.start_receiving_links(should_open_separately).await {
        let _ = app_runner_inputs_tx.send(AppRunnerInputs::from_error("Error getting started", error.to_string()));
        return;
    }
    tokio::spawn(async move {
        start_handling_deep_links_forever(app_runner_inputs_tx, &mut deep_link_receiver, should_open_separately).await;
    });
}

// ---------------------------------------------- Deep Link Updater Helpers ----------------------------------------------

// A separate copy registers as nobody's link receiver, so it can always be started alongside the others.
fn open_link_in_new_copy(deep_link: &url::Url, runner_inputs_tx: &mpsc::UnboundedSender<AppRunnerInputs>) {
    match std::env::current_exe().and_then(|executable| std::process::Command::new(executable).arg("--open-separately").arg(deep_link.as_str()).spawn()) {
        Ok(mut child) => {
            log::info!("Opened the link in a new copy of the app (process {})", child.id());
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => {
            let _ = runner_inputs_tx.send(AppRunnerInputs::from_error("Could not open link", error.to_string()));
        }
    }
}
// Distributed app links can start play sessions or return to the website.
async fn start_handling_deep_links_forever(
    runner_inputs_tx: mpsc::UnboundedSender<AppRunnerInputs>,
    deep_link_receiver: &mut DeepLinkReceiver,
    should_open_separately: bool,
) {
    // Blank launches open the homepage directly and close the app, since there is nothing for it to show.
    fn open_homepage_for_blank_app(did_show_page: bool) {
        if let Err(error) = open::that(SITE_URL.as_str()) {
            log::error!("Failed to open homepage: {}", error);
        }
        if !did_show_page {
            std::process::exit(0);
        }
    }

    {
        // Allow a launch link to arrive before treating the initial launch as blank.
        let mut did_check_initial_link = false;
        let mut did_show_page = false;
        let mut did_receive_launch_link = false;

        loop {
            let deep_link = if !did_check_initial_link {
                did_check_initial_link = true;
                let launch_link = tokio::select! {
                    _ = tokio::time::sleep_until(Instant::now() + Duration::from_secs(1)) => None,
                    deep_link = deep_link_receiver.recv() => deep_link,
                };
                // macOS only hands over the launch link once the app is running, so the update waits for it and passes it on.
                if !should_open_separately {
                    match quit_and_install_update_if_available(launch_link.clone()).await {
                        Ok(true) => std::process::exit(0),
                        Ok(false) => {}
                        Err(error) => log::error!("Failed running client updater: {}", error),
                    }
                }
                if launch_link.is_some() {
                    launch_link
                } else {
                    open_homepage_for_blank_app(did_show_page);
                    deep_link_receiver.recv().await
                }
            } else {
                deep_link_receiver.recv().await
            };

            // we received a deeplink
            let Some(deep_link) = deep_link else {
                break;
            };

            // parse the deeplink
            let deep_link_params = match parse_deep_link(&deep_link) {
                Ok(deep_link_params) => deep_link_params,
                Err(e) => {
                    let error_message = match e {
                        DeepLinkParseError::MissingMode => "Missing mode in deep link.".to_string(),
                        DeepLinkParseError::InvalidMode => "Invalid mode in deep link.".to_string(),
                        DeepLinkParseError::MissingField(string) => format!("Missing {string} in deep link."),
                        DeepLinkParseError::InvalidField(string) => format!("Invalid {string} in deep link."),
                    };
                    log::error!("Failed to parse deep link: {error_message}");
                    // error page would be very frustrating here if they were already looking at a page
                    if !did_show_page {
                        let _ = runner_inputs_tx.send(AppRunnerInputs::from_error("Error reading link", error_message));
                    }
                    continue;
                }
            };

            // Links carry play tokens, so only their kind is logged.
            let deep_link_kind = match &deep_link_params {
                DeepLinkParams::Play { .. } => "play",
                DeepLinkParams::Playtest { .. } => "playtest",
                DeepLinkParams::Blank => "blank",
            };
            let is_launch_link = !did_receive_launch_link;
            did_receive_launch_link = true;
            log::info!("Received a {deep_link_kind} link (separate copy: {should_open_separately}, launch link: {is_launch_link})");

            // A separate copy shows only the link it was launched with. macOS can hand a later link to any running
            // copy, so one arriving here opens a copy of its own instead of replacing this window.
            if should_open_separately && !is_launch_link {
                open_link_in_new_copy(&deep_link, &runner_inputs_tx);
                continue;
            }

            // update app state
            let app_runner_inputs = match deep_link_params {
                DeepLinkParams::Play {
                    request_id,
                    play_auth,
                    thumbnail_uuid,
                    thumbnail_game_name,
                } => {
                    let _ = runner_inputs_tx.send(AppRunnerInputs::from_loading(
                        thumbnail_game_name,
                        thumbnail_uuid.map(|thumbnail_uuid| PublicFile::GameThumbnail { thumbnail_uuid }.url()),
                    ));

                    get_play_app_runner_inputs_and_start_bg_tasks_to_official_server(request_id, play_auth, runner_inputs_tx.clone()).await
                }
                // The link that launched this copy plays here. Each later playtest player is its own copy with one window.
                DeepLinkParams::Playtest { .. } if !should_open_separately && !is_launch_link => {
                    open_link_in_new_copy(&deep_link, &runner_inputs_tx);
                    continue;
                }
                DeepLinkParams::Playtest { public_ip, port, join_token } => {
                    let _ = runner_inputs_tx.send(AppRunnerInputs::from_loading(None, None));
                    get_play_app_runner_inputs_and_start_bg_tasks_to_playtest_server(public_ip, port, join_token, runner_inputs_tx.clone()).await
                }
                DeepLinkParams::Blank => {
                    open_homepage_for_blank_app(did_show_page);
                    continue;
                }
            };
            let _ = runner_inputs_tx.send(app_runner_inputs);
            did_show_page = true;
        }
    }
}
