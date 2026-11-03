use crate::play_web_view::{HostToWebViewMessage, PlayWebView};
use crate::simple_renderer::SimpleRenderer;
use render_core::renderer::RenderFrame;
use serde::Serialize;

// The splash page covers the window, but it takes a moment to draw after the window opens. The window
// is cleared to the white the page starts from, so opening the app does not flash another colour first.
const SPLASH_CLEAR_COLOR: wgpu::Color = wgpu::Color::WHITE;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplashScreenContent {
    pub title_text: String, // <----- TODO: add
    pub sub_text: Option<String>,
    pub thumbnailUrl: Option<String>,
    pub is_loading: bool, // the K only tumbles while loading, not on a disconnect or error
}

// The play webview draws the splash screen itself, down to fetching the thumbnail, so this only hands it the content.
pub struct SplashScreenRunner {}

impl SplashScreenRunner {
    pub fn new(splash_screen_content: SplashScreenContent, play_web_view: &mut PlayWebView) -> Self {
        play_web_view.send(&HostToWebViewMessage::ShowSplashScreen(&splash_screen_content));
        Self {}
    }

    pub fn onRequestRedraw(&mut self, renderer: &mut SimpleRenderer, render_frame: &mut RenderFrame) {
        renderer.gpu.clear_frame(render_frame, SPLASH_CLEAR_COLOR);
    }
}
