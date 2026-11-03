use super::get_play_web_view_file;
use std::borrow::Cow;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    window::Window,
};
use wry::{
    Rect, WebView, WebViewBuilder,
    http::{Request, Response, StatusCode, header::CONTENT_TYPE},
};

const PLAY_WEB_VIEW_PROTOCOL_NAME: &str = "kube-play";

pub struct MacosWebView {
    web_view: WebView,
}

impl MacosWebView {
    pub fn new(window: &Window, receive_page_message: impl Fn(&str) + 'static) -> anyhow::Result<Self> {
        let web_view = WebViewBuilder::new()
            .with_custom_protocol(PLAY_WEB_VIEW_PROTOCOL_NAME.into(), |_web_view_id, request| play_web_view_response(request))
            .with_ipc_handler(move |request: Request<String>| receive_page_message(request.body()))
            .with_transparent(true)
            .with_bounds(full_window_rect(window.inner_size()))
            .with_url(format!("{PLAY_WEB_VIEW_PROTOCOL_NAME}://localhost"))
            // The game window takes the first click when it is not focused, so the page does too.
            .with_accept_first_mouse(true)
            .with_hotkeys_zoom(false)
            // A file dropped on the window would otherwise replace the page with that file.
            .with_drag_drop_handler(|_drag_drop_event| true)
            .build_as_child(window)?;
        lift_60_fps_cap(&web_view);
        Ok(Self { web_view })
    }

    pub fn resize(&self, window_size: PhysicalSize<u32>) {
        let _ = self.web_view.set_bounds(full_window_rect(window_size));
    }

    pub fn focus(&self) {
        let _ = self.web_view.focus();
    }

    pub fn evaluate_script(&self, script: &str) -> anyhow::Result<()> {
        Ok(self.web_view.evaluate_script(script)?)
    }
}

fn full_window_rect(window_size: PhysicalSize<u32>) -> Rect {
    Rect {
        position: PhysicalPosition::new(0, 0).into(),
        size: window_size.into(),
    }
}

fn play_web_view_response(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let response = match get_play_web_view_file(request.uri().path()) {
        Some((content_type, bytes)) => Response::builder().header(CONTENT_TYPE, content_type).body(Cow::Borrowed(bytes)),
        None => Response::builder().status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&b"Not found"[..])),
    };
    response.expect("Play webview response should build")
}

// WebKit draws pages at most 60 times a second, even on 120 Hz screens, unless this private feature flag is turned off.
fn lift_60_fps_cap(web_view: &WebView) {
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject},
        sel,
    };
    use objc2_foundation::NSString;
    use wry::WebViewExtMacOS;

    let Some(preferences_class) = AnyClass::get(c"WKPreferences") else {
        log::warn!("Could not find WKPreferences to lift the play webview's 60 fps cap");
        return;
    };
    let wk_web_view = web_view.webview();
    let configuration: Retained<AnyObject> = unsafe { msg_send![&*wk_web_view, configuration] };
    let preferences: Retained<AnyObject> = unsafe { msg_send![&*configuration, preferences] };
    let has_features: bool = unsafe { msg_send![preferences_class, respondsToSelector: sel!(_features)] };
    let can_set_feature: bool = unsafe { msg_send![&*preferences, respondsToSelector: sel!(_setEnabled:forFeature:)] };
    if !has_features || !can_set_feature {
        log::warn!("WebKit has no feature flags to lift the play webview's 60 fps cap");
        return;
    }
    let features: Retained<AnyObject> = unsafe { msg_send![preferences_class, _features] };
    let feature_count: usize = unsafe { msg_send![&*features, count] };
    for feature_index in 0..feature_count {
        let feature: Retained<AnyObject> = unsafe { msg_send![&*features, objectAtIndex: feature_index] };
        let key: Retained<NSString> = unsafe { msg_send![&*feature, key] };
        if key.to_string() == "PreferPageRenderingUpdatesNear60FPSEnabled" {
            let _: () = unsafe { msg_send![&*preferences, _setEnabled: false, forFeature: &*feature] };
            return;
        }
    }
    log::warn!("WebKit has no 60 fps cap flag to lift for the play webview");
}
