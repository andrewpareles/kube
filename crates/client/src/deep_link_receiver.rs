use anyhow::Result;
use game_types::consts::DEEP_LINK_SCHEME;
use std::path::PathBuf;
use tokio::sync::mpsc;
use url::Url;

// -------------------------------------------- entry points --------------------------------------------
// the player is launched via deep link "scheme://?mode=play&request_id=...&play_auth=...&thumbnail_uuid=...".

pub struct DeepLinkReceiver {
    deep_link_rx: mpsc::UnboundedReceiver<Url>,
    deep_link_tx: mpsc::UnboundedSender<Url>,
    did_start_receiving_links: bool,
    #[cfg(target_os = "macos")]
    mac_open_urls_handler: Option<mac_open_urls_handler::MacOpenUrlsHandler>,
}

impl DeepLinkReceiver {
    pub fn new() -> Self {
        let (deep_link_tx, deep_link_rx) = mpsc::unbounded_channel();
        Self {
            deep_link_rx,
            deep_link_tx,
            did_start_receiving_links: false,
            #[cfg(target_os = "macos")]
            mac_open_urls_handler: None,
        }
    }
    pub async fn recv(&mut self) -> Option<Url> {
        self.deep_link_rx.recv().await
    }

    pub async fn start_receiving_links(&mut self, should_open_separately: bool) -> Result<()> {
        // Run this once when the app starts.
        // First, check launch arguments for a link.
        // Next, set up link input for this OS:
        // Windows uses a local port between app instances.
        // macOS uses Apple URL events.
        // If we already got a launch link, send it into this channel.
        // On Windows, if this is the second app instance, send the link to the first one and return true.
        if self.did_start_receiving_links {
            return Ok(()); // already started
        }
        self.did_start_receiving_links = true;

        let initial_deep_link = find_deeplink_in_args_list(std::env::args().skip(1));

        // Separate windows consume their launch link without becoming the app's link receiver. macOS can still hand
        // them a later link, since every copy counts as the running app there, so they listen for those too.
        if should_open_separately {
            #[cfg(target_os = "macos")]
            {
                let mac_open_urls_handler = mac_open_urls_handler::start_macos_open_urls_handler(&self.deep_link_tx)?;
                self.mac_open_urls_handler = Some(mac_open_urls_handler);
            }
            if let Some(initial_deep_link) = initial_deep_link {
                let _ = self.deep_link_tx.send(initial_deep_link);
            }
            return Ok(());
        }

        // A link in the arguments is newer than one an update saved, so it wins.
        let saved_launch_link = take_saved_launch_link();
        let initial_deep_link = initial_deep_link.or(saved_launch_link);

        #[cfg(target_os = "windows")]
        {
            // on windows, a new app (this) is always opened, and then the message is forwarded to an existing "primary" app, in which case we should close the new app immediately
            let did_forward_to_primary =
                windows_deep_link_ipc::setup_windows_deep_link_ipc_and_maybe_forward_to_primary(&self.deep_link_tx, initial_deep_link.as_ref())?;
            if did_forward_to_primary {
                std::process::exit(0);
            }
        }

        #[cfg(target_os = "macos")]
        {
            let mac_open_urls_handler = mac_open_urls_handler::start_macos_open_urls_handler(&self.deep_link_tx)?;
            self.mac_open_urls_handler = Some(mac_open_urls_handler);
        }

        if let Some(initial_deep_link) = initial_deep_link {
            let _ = self.deep_link_tx.send(initial_deep_link);
        }

        Ok(())
    }
}

// vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv AI SLOP vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv

// -------------------------------------------- Cold and Warm Startup Orchestration --------------------------------------------

// Read a link from launch arguments.
// get the first arg that looks like a deeplink url
// windows shoves in the url this way. mac does something different
pub fn find_deeplink_in_args_list(args: impl Iterator<Item = String>) -> Option<Url> {
    for arg in args {
        let Ok(parsed_deep_link) = Url::parse(&arg) else {
            continue;
        };
        if parsed_deep_link.scheme() != DEEP_LINK_SCHEME {
            continue;
        }
        return Some(parsed_deep_link);
    }

    None
}

// The link that started the app, kept for the updated app. The updater writes it after downloading an update and just
// before installing, since the updated app opens without it. The next start of the main app reads and deletes it, and
// plays it unless that start came with a link of its own.
fn saved_launch_link_path() -> PathBuf {
    std::env::temp_dir().join("kube-new-launch-link")
}

pub fn save_launch_link(launch_link: &Url) -> std::io::Result<()> {
    std::fs::write(saved_launch_link_path(), launch_link.as_str())
}

fn take_saved_launch_link() -> Option<Url> {
    let saved_launch_link = std::fs::read_to_string(saved_launch_link_path()).ok();
    let _ = std::fs::remove_file(saved_launch_link_path());
    Url::parse(&saved_launch_link?).ok()
}

// -------------------------------------------- Windows Warm Deep-link IPC --------------------------------------------

#[cfg(target_os = "windows")]
mod windows_deep_link_ipc {
    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpListener, TcpStream},
        time::Duration,
    };

    use super::*;

    const WINDOWS_DEEP_LINK_IPC_ADDR: &str = "127.0.0.1:47747";
    const WINDOWS_DEEP_LINK_MAX_NUM_BYTES: usize = 16 * 1024;
    const WINDOWS_DEEP_LINK_CONNECT_TIMEOUT_MS: u64 = 150;

    // On Windows: either listen for links from other instances, or send this link to the running instance.
    pub fn setup_windows_deep_link_ipc_and_maybe_forward_to_primary(
        deep_link_tx: &mpsc::UnboundedSender<Url>,
        initial_deep_link_or_none: Option<&Url>,
    ) -> Result<bool> {
        // Attempt to start the local Windows IPC listener first. If this fails and this process has a deep-link, forward it.
        let windows_deep_link_ipc_listener_or_none = match TcpListener::bind(WINDOWS_DEEP_LINK_IPC_ADDR) {
            Ok(windows_deep_link_ipc_listener) => Some(windows_deep_link_ipc_listener),
            Err(bind_error) => {
                log::info!("Windows deep-link IPC listener bind failed on {}: {}", WINDOWS_DEEP_LINK_IPC_ADDR, bind_error);
                None
            }
        };

        if windows_deep_link_ipc_listener_or_none.is_none() {
            let Some(initial_deep_link) = initial_deep_link_or_none else {
                return Ok(false);
            };
            let did_forward_to_primary = try_forward_deep_link_to_primary_on_windows(initial_deep_link)?;
            return Ok(did_forward_to_primary);
        }

        let Some(windows_deep_link_ipc_listener) = windows_deep_link_ipc_listener_or_none else {
            return Ok(false);
        };
        spawn_windows_deep_link_ipc_listener(deep_link_tx.clone(), windows_deep_link_ipc_listener);

        Ok(false)
    }

    // -------------------------------------------- Windows IPC Helpers --------------------------------------------

    fn try_forward_deep_link_to_primary_on_windows(deep_link: &Url) -> Result<bool> {
        let windows_deep_link_ipc_addr: SocketAddr = WINDOWS_DEEP_LINK_IPC_ADDR
            .parse()
            .map_err(|parse_error| anyhow::anyhow!("Windows deep-link IPC address parse error: {}", parse_error))?;

        let connect_timeout = Duration::from_millis(WINDOWS_DEEP_LINK_CONNECT_TIMEOUT_MS);
        let mut windows_deep_link_stream = match TcpStream::connect_timeout(&windows_deep_link_ipc_addr, connect_timeout) {
            Ok(windows_deep_link_stream) => windows_deep_link_stream,
            Err(_) => {
                return Ok(false);
            }
        };

        let _ = windows_deep_link_stream.set_write_timeout(Some(connect_timeout));
        let deep_link_bytes = deep_link.as_str().as_bytes();
        if deep_link_bytes.len() > WINDOWS_DEEP_LINK_MAX_NUM_BYTES {
            return Err(anyhow::anyhow!("Deep-link is too large to forward"));
        }

        let deep_link_num_bytes_u32 = deep_link_bytes.len() as u32;
        windows_deep_link_stream.write_all(&deep_link_num_bytes_u32.to_le_bytes())?;
        windows_deep_link_stream.write_all(deep_link_bytes)?;

        Ok(true)
    }

    fn spawn_windows_deep_link_ipc_listener(deep_link_tx: mpsc::UnboundedSender<Url>, windows_deep_link_ipc_listener: TcpListener) {
        std::thread::spawn(move || {
            loop {
                let accepted_stream = windows_deep_link_ipc_listener.accept();
                let (mut windows_deep_link_stream, _) = match accepted_stream {
                    Ok(stream_data) => stream_data,
                    Err(accept_error) => {
                        log::warn!("Windows deep-link IPC accept failed: {}", accept_error);
                        continue;
                    }
                };

                let deep_link_or_none = read_deep_link_from_windows_ipc_stream(&mut windows_deep_link_stream);
                let Some(deep_link) = deep_link_or_none else {
                    continue;
                };

                if deep_link_tx.send(deep_link).is_err() {
                    break;
                }
            }
        });
    }

    fn read_deep_link_from_windows_ipc_stream(windows_deep_link_stream: &mut TcpStream) -> Option<Url> {
        let mut deep_link_num_bytes_buffer = [0_u8; 4];
        windows_deep_link_stream.read_exact(&mut deep_link_num_bytes_buffer).ok()?;
        let deep_link_num_bytes = u32::from_le_bytes(deep_link_num_bytes_buffer) as usize;
        if deep_link_num_bytes == 0 || deep_link_num_bytes > WINDOWS_DEEP_LINK_MAX_NUM_BYTES {
            return None;
        }

        let mut deep_link_bytes = vec![0_u8; deep_link_num_bytes];
        windows_deep_link_stream.read_exact(&mut deep_link_bytes).ok()?;
        let deep_link_string = String::from_utf8(deep_link_bytes).ok()?;
        let deep_link_parse_result = Url::parse(&deep_link_string);
        let Ok(deep_link) = deep_link_parse_result else {
            return None;
        };
        if deep_link.scheme() != DEEP_LINK_SCHEME {
            return None;
        }

        Some(deep_link)
    }
}

// -------------------------------------------- macOS Apple Event Bridge (Cold + Warm) --------------------------------------------

#[cfg(target_os = "macos")]
mod mac_open_urls_handler {
    use std::cell::RefCell;

    use super::*;
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSObject, NSObjectProtocol};

    #[derive(Debug)]
    struct DeepLinkAppleEventHandlerIvars {
        deep_link_tx: mpsc::UnboundedSender<Url>,
    }

    const INTERNET_EVENT_CLASS_GET_URL: u32 = 0x4755_524c;
    const INTERNET_EVENT_ID_GET_URL: u32 = 0x4755_524c;
    const KEYWORD_DIRECT_OBJECT: u32 = 0x2d2d_2d2d;

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = DeepLinkAppleEventHandlerIvars]
        struct DeepLinkAppleEventHandler;

        unsafe impl NSObjectProtocol for DeepLinkAppleEventHandler {}

        impl DeepLinkAppleEventHandler {
            #[unsafe(method(handleAppleEvent:withReplyEvent:))]
            fn handle_apple_event_with_reply_event(&self, apple_event: &NSAppleEventDescriptor, _reply_event: &NSAppleEventDescriptor) {
                let direct_object_descriptor_or_none = apple_event.paramDescriptorForKeyword(KEYWORD_DIRECT_OBJECT);
                let Some(direct_object_descriptor) = direct_object_descriptor_or_none else {
                    return;
                };
                let url_string_or_none = direct_object_descriptor.stringValue();
                let Some(url_string) = url_string_or_none else {
                    return;
                };
                let deep_link_parse_result = Url::parse(&url_string.to_string());
                let Ok(deep_link) = deep_link_parse_result else {
                    return;
                };
                if deep_link.scheme() != DEEP_LINK_SCHEME {
                    return;
                }
                let _ = self.ivars().deep_link_tx.send(deep_link);
            }
        }
    );

    impl DeepLinkAppleEventHandler {
        fn new(mtm: MainThreadMarker, deep_link_tx: mpsc::UnboundedSender<Url>) -> Retained<Self> {
            let deep_link_apple_event_handler = Self::alloc(mtm).set_ivars(DeepLinkAppleEventHandlerIvars { deep_link_tx });
            unsafe { msg_send![super(deep_link_apple_event_handler), init] }
        }
    }

    thread_local! {
        static DEEP_LINK_APPLE_EVENT_HANDLER: RefCell<Option<Retained<DeepLinkAppleEventHandler>>> = const { RefCell::new(None) };
    }

    pub struct MacOpenUrlsHandler;

    // On macOS: register Apple URL events and push each received link into the channel.
    pub fn start_macos_open_urls_handler(deep_link_tx: &mpsc::UnboundedSender<Url>) -> Result<MacOpenUrlsHandler> {
        let mtm = MainThreadMarker::new().ok_or_else(|| anyhow::anyhow!("Expected main thread while installing macOS deep-link handler"))?;
        let deep_link_apple_event_handler = DeepLinkAppleEventHandler::new(mtm, deep_link_tx.clone());
        let apple_event_manager = NSAppleEventManager::sharedAppleEventManager();
        unsafe {
            apple_event_manager.setEventHandler_andSelector_forEventClass_andEventID(
                &*deep_link_apple_event_handler,
                sel!(handleAppleEvent:withReplyEvent:),
                INTERNET_EVENT_CLASS_GET_URL,
                INTERNET_EVENT_ID_GET_URL,
            );
        }
        DEEP_LINK_APPLE_EVENT_HANDLER.with(|deep_link_apple_event_handler_slot| {
            *deep_link_apple_event_handler_slot.borrow_mut() = Some(deep_link_apple_event_handler);
        });
        Ok(MacOpenUrlsHandler)
    }
}
