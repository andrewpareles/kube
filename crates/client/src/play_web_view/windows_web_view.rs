// On Windows the play webview uses WebView2's visual hosting: the page draws into a DirectComposition visual over the game
// window, instead of into windows of its own that belong to Edge's process. So the game window gets the mouse, touch and
// pen itself and decides the pointer (see windows_mouse_cursor.rs), and it hands the page every one of their messages.

use super::get_play_web_view_file;
use std::{ffi::c_void, sync::mpsc};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, CoreWebView2EnvironmentOptions, CreateCoreWebView2CompositionControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler, Microsoft::Web::WebView2::Win32::*, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, take_pwstr, wait_with_pump,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            DirectComposition::{DCompositionCreateDevice2, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual},
            Gdi::ScreenToClient,
        },
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::{
            Controls::WM_MOUSELEAVE,
            Input::{
                Pointer::{
                    GetPointerPenInfo, GetPointerTouchInfo, GetPointerType, POINTER_BUTTON_CHANGE_TYPE, POINTER_FLAGS, POINTER_INFO, POINTER_PEN_INFO,
                    POINTER_TOUCH_INFO,
                },
                Touch::UnregisterTouchWindow,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SHCreateMemStream, SetWindowSubclass},
            WindowsAndMessaging::{
                POINTER_INPUT_TYPE, PT_PEN, PT_TOUCH, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
                WM_MOVE, WM_MOVING, WM_POINTERACTIVATE, WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE, WM_RBUTTONDOWN,
                WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
            },
        },
    },
    core::{HSTRING, Interface, PCWSTR, PWSTR},
};
use winit::{
    dpi::PhysicalSize,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

// WebView2 can't serve a scheme of its own, so the page's files are answered for this address, which wry used too, so the
// page keeps what it saved in its storage.
const PLAY_WEB_VIEW_URL: &str = "http://kube-play.localhost/";
// The mini menu that pops up over selected text, and SmartScreen's checks of every address, the same as wry turned off.
const BROWSER_ARGUMENTS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

pub struct WindowsWebView {
    hwnd: HWND,
    controller: ICoreWebView2Controller,
    web_view: ICoreWebView2,
    // the window's subclass holds a pointer to this
    _page_input_target_box: Box<PageInputTarget>,
    // DirectComposition draws the page only while these live.
    _composition_device: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _composition_visual: IDCompositionVisual,
}

// What the window's subclass hands the page's input to.
struct PageInputTarget {
    environment: ICoreWebView2Environment3,
    composition_controller: ICoreWebView2CompositionController,
}

impl WindowsWebView {
    pub fn new(window: &Window, receive_page_message: impl Fn(&str) + 'static) -> anyhow::Result<Self> {
        let RawWindowHandle::Win32(win32_window_handle) = window.window_handle()?.as_raw() else {
            return Err(anyhow::anyhow!("The game window has no Win32 handle"));
        };
        let hwnd = HWND(win32_window_handle.hwnd.get() as *mut c_void);
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

        // WebView2 makes its environment and controller in the background, and the window's messages keep running until each is ready.
        let environment_options = CoreWebView2EnvironmentOptions::default();
        unsafe { environment_options.set_additional_browser_arguments(BROWSER_ARGUMENTS.to_string()) };
        let (environment_tx, environment_rx) = mpsc::channel();
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR::null(),
                &ICoreWebView2EnvironmentOptions::from(environment_options),
                &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |error_code, environment| {
                    let _ = environment_tx.send(error_code.and_then(|()| environment.ok_or_else(|| E_POINTER.into())));
                    Ok(())
                })),
            )
        }?;
        let environment: ICoreWebView2Environment3 = wait_with_pump(environment_rx)??.cast()?;
        let (composition_controller_tx, composition_controller_rx) = mpsc::channel();
        unsafe {
            environment.CreateCoreWebView2CompositionController(
                hwnd,
                &CreateCoreWebView2CompositionControllerCompletedHandler::create(Box::new(move |error_code, composition_controller| {
                    let _ = composition_controller_tx.send(error_code.and_then(|()| composition_controller.ok_or_else(|| E_POINTER.into())));
                    Ok(())
                })),
            )
        }?;
        let composition_controller = wait_with_pump(composition_controller_rx)??;

        // The page's visual sits on top of everything the game draws into the window.
        let composition_device: IDCompositionDevice = unsafe { DCompositionCreateDevice2(None) }?;
        let composition_target = unsafe { composition_device.CreateTargetForHwnd(hwnd, true) }?;
        let composition_visual = unsafe { composition_device.CreateVisual() }?;
        unsafe { composition_target.SetRoot(&composition_visual) }?;
        unsafe { composition_controller.SetRootVisualTarget(&composition_visual) }?;
        unsafe { composition_device.Commit() }?;

        let controller: ICoreWebView2Controller = composition_controller.cast()?;
        unsafe {
            controller
                .cast::<ICoreWebView2Controller2>()?
                .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 0, R: 0, G: 0, B: 0 })
        }?;
        let web_view = unsafe { controller.CoreWebView2() }?;
        let settings = unsafe { web_view.Settings() }?;
        unsafe {
            // Browser shortcuts like F5 reloading the page do not belong in a game.
            settings.cast::<ICoreWebView2Settings3>()?.SetAreBrowserAcceleratorKeysEnabled(false)?;
            settings.SetAreDefaultContextMenusEnabled(false)?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetIsZoomControlEnabled(false)?;
            settings.SetAreDevToolsEnabled(false)?;
            settings.cast::<ICoreWebView2Settings5>()?.SetIsPinchZoomEnabled(false)?;
            settings.cast::<ICoreWebView2Settings6>()?.SetIsSwipeNavigationEnabled(false)?;
        }

        // The page sends messages with ipc.postMessage, like it does through wry on macOS.
        let web_view_for_script = web_view.clone();
        AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| {
                unsafe {
                    web_view_for_script.AddScriptToExecuteOnDocumentCreated(
                        &HSTRING::from("Object.defineProperty(window, 'ipc', { value: Object.freeze({ postMessage: (message) => window.chrome.webview.postMessage(message) }) });"),
                        &handler,
                    )
                }
                .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(|error_code, _script_id| error_code),
        )?;
        let mut event_registration_token = 0;
        unsafe {
            web_view.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_web_view, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut page_message = PWSTR::null();
                    args.TryGetWebMessageAsString(&mut page_message)?;
                    receive_page_message(&take_pwstr(page_message));
                    Ok(())
                })),
                &mut event_registration_token,
            )
        }?;

        // The page and its files are answered from the app, never the network.
        let environment_for_page_files = environment.clone();
        unsafe { web_view.AddWebResourceRequestedFilter(&HSTRING::from(format!("{PLAY_WEB_VIEW_URL}*")), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL) }?;
        unsafe {
            web_view.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_web_view, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut uri = PWSTR::null();
                    args.Request()?.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    let path = uri.strip_prefix(PLAY_WEB_VIEW_URL).unwrap_or_default();
                    let response = match get_play_web_view_file(path) {
                        Some((content_type, bytes)) => environment_for_page_files.CreateWebResourceResponse(
                            SHCreateMemStream(Some(bytes)).as_ref(),
                            200,
                            &HSTRING::from("OK"),
                            &HSTRING::from(format!("Content-Type: {content_type}")),
                        )?,
                        None => environment_for_page_files.CreateWebResourceResponse(None, 404, &HSTRING::from("Not Found"), &HSTRING::new())?,
                    };
                    args.SetResponse(&response)
                })),
                &mut event_registration_token,
            )
        }?;
        unsafe { web_view.Navigate(&HSTRING::from(PLAY_WEB_VIEW_URL)) }?;
        unsafe { controller.SetBounds(full_window_rect(window.inner_size())) }?;
        unsafe { controller.SetIsVisible(true) }?;

        let page_input_target_box = Box::new(PageInputTarget {
            environment,
            composition_controller,
        });
        let page_input_target_pointer = page_input_target_box.as_ref() as *const PageInputTarget as usize;
        if !unsafe { SetWindowSubclass(hwnd, Some(hand_input_to_page), 0, page_input_target_pointer) }.as_bool() {
            log::warn!("Failed to hand the play webview its input");
        }
        // winit asks Windows for touch as WM_TOUCH messages, which the page can't take, so touch comes as pointer messages
        // like a pen's.
        let _ = unsafe { UnregisterTouchWindow(hwnd) };
        Ok(Self {
            hwnd,
            controller,
            web_view,
            _page_input_target_box: page_input_target_box,
            _composition_device: composition_device,
            _composition_target: composition_target,
            _composition_visual: composition_visual,
        })
    }

    pub fn resize(&self, window_size: PhysicalSize<u32>) {
        let _ = unsafe { self.controller.SetBounds(full_window_rect(window_size)) };
    }

    pub fn focus(&self) {
        let _ = unsafe { self.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
    }

    pub fn evaluate_script(&self, script: &str) -> anyhow::Result<()> {
        unsafe {
            self.web_view.ExecuteScript(
                &HSTRING::from(script),
                &ExecuteScriptCompletedHandler::create(Box::new(|_error_code, _result_json| Ok(()))),
            )
        }?;
        Ok(())
    }
}

impl Drop for WindowsWebView {
    fn drop(&mut self) {
        let _ = unsafe { RemoveWindowSubclass(self.hwnd, Some(hand_input_to_page), 0) };
        let _ = unsafe { self.controller.Close() };
    }
}

fn full_window_rect(window_size: PhysicalSize<u32>) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: window_size.width as i32,
        bottom: window_size.height as i32,
    }
}

// Nothing of Edge's covers the window, so it gets the mouse, touch and pen, and the page is handed each of their
// messages. WebView2 numbers its mouse and pointer inputs the same way Windows numbers these messages.
unsafe extern "system" fn hand_input_to_page(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    page_input_target_pointer: usize,
) -> LRESULT {
    // What Windows knows about one touch or pen pointer, with its spots in the window's pixels like the page's. None for
    // other pointers, which come as mouse messages.
    fn make_pointer_info(hwnd: HWND, pointer_id: u32, environment: &ICoreWebView2Environment3) -> windows::core::Result<Option<ICoreWebView2PointerInfo>> {
        let to_window_point = |mut point: POINT| {
            let _ = unsafe { ScreenToClient(hwnd, &mut point) };
            point
        };
        let to_window_rect = |RECT { left, top, right, bottom }: RECT| {
            let POINT { x: left, y: top } = to_window_point(POINT { x: left, y: top });
            let POINT { x: right, y: bottom } = to_window_point(POINT { x: right, y: bottom });
            RECT { left, top, right, bottom }
        };
        let mut pointer_type = POINTER_INPUT_TYPE::default();
        unsafe { GetPointerType(pointer_id, &mut pointer_type) }?;
        let pointer_info = unsafe { environment.CreateCoreWebView2PointerInfo() }?;
        let POINTER_INFO {
            pointerType: POINTER_INPUT_TYPE(pointer_kind),
            frameId,
            pointerFlags: POINTER_FLAGS(pointer_flags),
            ptPixelLocation,
            ptPixelLocationRaw,
            dwTime,
            historyCount,
            InputData,
            dwKeyStates,
            PerformanceCount,
            ButtonChangeType: POINTER_BUTTON_CHANGE_TYPE(button_change_kind),
            ..
        } = match pointer_type {
            PT_TOUCH => {
                let mut touch_info = POINTER_TOUCH_INFO::default();
                unsafe { GetPointerTouchInfo(pointer_id, &mut touch_info) }?;
                let POINTER_TOUCH_INFO {
                    pointerInfo,
                    touchFlags,
                    touchMask,
                    rcContact,
                    rcContactRaw,
                    orientation,
                    pressure,
                } = touch_info;
                unsafe {
                    pointer_info.SetTouchFlags(touchFlags)?;
                    pointer_info.SetTouchMask(touchMask)?;
                    pointer_info.SetTouchContact(to_window_rect(rcContact))?;
                    pointer_info.SetTouchContactRaw(to_window_rect(rcContactRaw))?;
                    pointer_info.SetTouchOrientation(orientation)?;
                    pointer_info.SetTouchPressure(pressure)?;
                }
                pointerInfo
            }
            PT_PEN => {
                let mut pen_info = POINTER_PEN_INFO::default();
                unsafe { GetPointerPenInfo(pointer_id, &mut pen_info) }?;
                let POINTER_PEN_INFO {
                    pointerInfo,
                    penFlags,
                    penMask,
                    pressure,
                    rotation,
                    tiltX,
                    tiltY,
                } = pen_info;
                unsafe {
                    pointer_info.SetPenFlags(penFlags)?;
                    pointer_info.SetPenMask(penMask)?;
                    pointer_info.SetPenPressure(pressure)?;
                    pointer_info.SetPenRotation(rotation)?;
                    pointer_info.SetPenTiltX(tiltX)?;
                    pointer_info.SetPenTiltY(tiltY)?;
                }
                pointerInfo
            }
            _ => return Ok(None),
        };
        unsafe {
            pointer_info.SetPointerKind(pointer_kind as u32)?;
            pointer_info.SetPointerId(pointer_id)?;
            pointer_info.SetFrameId(frameId)?;
            pointer_info.SetPointerFlags(pointer_flags)?;
            pointer_info.SetPixelLocation(to_window_point(ptPixelLocation))?;
            pointer_info.SetPixelLocationRaw(to_window_point(ptPixelLocationRaw))?;
            pointer_info.SetTime(dwTime)?;
            pointer_info.SetHistoryCount(historyCount)?;
            pointer_info.SetInputData(InputData)?;
            pointer_info.SetKeyStates(dwKeyStates)?;
            pointer_info.SetPerformanceCount(PerformanceCount)?;
            pointer_info.SetButtonChangeKind(button_change_kind)?;
        }
        Ok(Some(pointer_info))
    }

    let PageInputTarget {
        environment,
        composition_controller,
    } = unsafe { &*(page_input_target_pointer as *const PageInputTarget) };
    let get_point = || POINT {
        x: (lparam.0 & 0xffff) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xffff) as i16 as i32,
    };
    let virtual_keys = COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS((wparam.0 & 0xffff) as i32);
    let wparam_high_word = ((wparam.0 >> 16) & 0xffff) as u16;
    let mouse_input = match message {
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => Some((virtual_keys, 0, get_point())),
        // Which side button it was.
        WM_XBUTTONDOWN | WM_XBUTTONUP => Some((virtual_keys, wparam_high_word as u32, get_point())),
        // How far the wheel turned, with the point on the screen instead of on the window.
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let mut point = get_point();
            let _ = unsafe { ScreenToClient(hwnd, &mut point) };
            Some((virtual_keys, wparam_high_word as i16 as i32 as u32, point))
        }
        WM_MOUSELEAVE => Some((COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS_NONE, 0, POINT::default())),
        _ => None,
    };
    if let Some((virtual_keys, mouse_data, point)) = mouse_input {
        let _ = unsafe { composition_controller.SendMouseInput(COREWEBVIEW2_MOUSE_EVENT_KIND(message as i32), virtual_keys, mouse_data, point) };
    }
    // Windows makes mouse messages from the touch and pen messages that reach DefWindowProc, so the ones handed to the page
    // stop here, and the page gets each touch once. A touch still brings the window to the front, like a click.
    let is_pointer_message = match message {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERENTER | WM_POINTERLEAVE | WM_POINTERACTIVATE => true,
        _ => false,
    };
    if is_pointer_message && let Ok(Some(pointer_info)) = make_pointer_info(hwnd, (wparam.0 & 0xffff) as u32, environment) {
        let _ = unsafe { composition_controller.SendPointerInput(COREWEBVIEW2_POINTER_EVENT_KIND(message as i32), &pointer_info) };
        if message != WM_POINTERACTIVATE {
            return LRESULT(0);
        }
    }
    // Popups like the typing helper for other languages are placed from where the window is.
    if (message == WM_MOVE || message == WM_MOVING)
        && let Ok(controller) = composition_controller.cast::<ICoreWebView2Controller>()
    {
        let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
