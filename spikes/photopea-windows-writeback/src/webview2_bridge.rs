use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment14,
    ICoreWebView2ObjectCollection, ICoreWebView2ObjectCollectionView, ICoreWebView2_23,
    COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
use windows::core::{IUnknown, Interface, PCWSTR, PWSTR};

use crate::LaunchRequest;

const SHELL_READY: &str = r#"{"type":"webwrap-photopea-shell-ready"}"#;
const HANDLE_RECEIVED: &str = r#"{"type":"webwrap-photopea-handle-received"}"#;
const PHOTOPEA_OPENED: &str = r#"{"type":"webwrap-photopea-opened"}"#;
const SAVE_REQUESTED: &str = r#"{"type":"webwrap-photopea-save-requested"}"#;
const SAVE_COMPLETE: &str = r#"{"type":"webwrap-photopea-save-complete"}"#;
const ERROR: &str = r#"{"type":"webwrap-error"}"#;

const OPEN_IDLE: u8 = 0;
const OPEN_HANDLE_SENT: u8 = 1;
const OPEN_HANDLE_RECEIVED: u8 = 2;
const OPEN_OPENED: u8 = 3;
const OPEN_FAILED: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BridgeMessage {
    ShellReady,
    HandleReceived,
    PhotopeaOpened,
    SaveRequested,
    SaveComplete,
    Error,
}

fn decode_message(wire: &str) -> Option<BridgeMessage> {
    let inner = serde_json::from_str::<String>(wire).ok()?;
    let canonical = serde_json::to_string(&inner).ok()?;
    if canonical != wire {
        return None;
    }
    match inner.as_str() {
        SHELL_READY => Some(BridgeMessage::ShellReady),
        HANDLE_RECEIVED => Some(BridgeMessage::HandleReceived),
        PHOTOPEA_OPENED => Some(BridgeMessage::PhotopeaOpened),
        SAVE_REQUESTED => Some(BridgeMessage::SaveRequested),
        SAVE_COMPLETE => Some(BridgeMessage::SaveComplete),
        ERROR => Some(BridgeMessage::Error),
        _ => None,
    }
}

fn source_is_allowed(source: &str) -> bool {
    let Ok(url) = tauri::Url::parse(source) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str() == Some("tauri.localhost")
        && url.port_or_known_default() == Some(80)
}

fn sources_allow_action(event_source: &str, live_source: &str) -> bool {
    source_is_allowed(event_source) && source_is_allowed(live_source)
}

unsafe fn live_source_allows_action(
    sender: &ICoreWebView2,
    event_source: &str,
) -> windows::core::Result<bool> {
    let mut live_source = PWSTR::null();
    sender.Source(&mut live_source)?;
    Ok(sources_allow_action(event_source, &take_pwstr(live_source)))
}

fn classify_message(state: u8, message: BridgeMessage) -> Option<u8> {
    match (state, message) {
        (OPEN_IDLE, BridgeMessage::ShellReady) => Some(OPEN_HANDLE_SENT),
        (OPEN_HANDLE_SENT, BridgeMessage::HandleReceived) => Some(OPEN_HANDLE_RECEIVED),
        (OPEN_HANDLE_RECEIVED, BridgeMessage::PhotopeaOpened) => Some(OPEN_OPENED),
        _ => None,
    }
}

fn mark_failed(
    window: &tauri::WebviewWindow,
    open_state: &AtomicU8,
    webview: Option<&ICoreWebView2>,
    reason: impl std::fmt::Display,
) {
    let mut state = open_state.load(Ordering::Acquire);
    loop {
        if matches!(state, OPEN_OPENED | OPEN_FAILED) {
            return;
        }
        match open_state.compare_exchange(state, OPEN_FAILED, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(next) => state = next,
        }
    }

    eprintln!("webview2 bridge: Photopea open failed: {reason}");
    if let Some(webview) = webview {
        if let Err(error) = unsafe {
            webview.PostWebMessageAsJson(windows::core::w!(r#"{"type":"webwrap-error"}"#))
        } {
            eprintln!("webview2 bridge: error message delivery failed: {error}");
        }
    }
    if let Err(error) = window.set_title("FAIL: Native bridge failed") {
        eprintln!("webview2 bridge: failure title update failed: {error}");
    }
}

unsafe fn post_file_handle(
    environment: &ICoreWebView2Environment,
    webview: &ICoreWebView2,
    path: &Path,
) -> windows::core::Result<()> {
    let environment = environment.cast::<ICoreWebView2Environment14>()?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let handle = environment.CreateWebFileSystemFileHandle(
        PCWSTR(path.as_ptr()),
        COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
    )?;
    let mut items = [Some(handle.cast::<IUnknown>()?)];
    let mut collection: Option<ICoreWebView2ObjectCollection> = None;
    environment.CreateObjectCollection(1, items.as_mut_ptr(), &mut collection)?;
    let collection = collection
        .ok_or_else(windows::core::Error::from_win32)?
        .cast::<ICoreWebView2ObjectCollectionView>()?;
    webview
        .cast::<ICoreWebView2_23>()?
        .PostWebMessageAsJsonWithAdditionalObjects(
            windows::core::w!(r#"{"type":"webwrap-file"}"#),
            &collection,
        )
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().replace(['\r', '\n'], "_"))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "file".into())
}

pub fn install(window: &tauri::WebviewWindow, request: LaunchRequest) -> tauri::Result<()> {
    let failure_window = window.clone();
    let open_state = Arc::new(AtomicU8::new(OPEN_IDLE));
    let path = request.path.clone();
    let name = display_name(&request.path);

    window.with_webview(move |platform| unsafe {
        let environment = platform.environment();
        let webview = match platform.controller().CoreWebView2() {
            Ok(webview) => webview,
            Err(error) => {
                eprintln!("webview2 bridge: CoreWebView2 unavailable: {error}");
                mark_failed(&failure_window, &open_state, None, error);
                return;
            }
        };

        let delivery_environment = environment.clone();
        let event_window = failure_window.clone();
        let event_open_state = Arc::clone(&open_state);
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
            let (Some(sender), Some(args)) = (sender, args) else {
                mark_failed(
                    &event_window,
                    &event_open_state,
                    None,
                    "WebView2 event did not include sender and arguments",
                );
                return Ok(());
            };

            let mut message = PWSTR::null();
            if let Err(error) = args.WebMessageAsJson(&mut message) {
                eprintln!("webview2 bridge: WebMessageAsJson failed: {error}");
                mark_failed(&event_window, &event_open_state, Some(&sender), error);
                return Ok(());
            }
            let message = take_pwstr(message);
            let Some(message) = decode_message(&message) else {
                return Ok(());
            };

            let mut source = PWSTR::null();
            if let Err(error) = args.Source(&mut source) {
                eprintln!("webview2 bridge: Source failed: {error}");
                mark_failed(&event_window, &event_open_state, Some(&sender), error);
                return Ok(());
            }
            let source = take_pwstr(source);
            if !source_is_allowed(&source) {
                return Ok(());
            }

            if message == BridgeMessage::Error {
                if let Err(error) = live_source_allows_action(&sender, &source) {
                    mark_failed(&event_window, &event_open_state, Some(&sender), error);
                } else {
                    mark_failed(
                        &event_window,
                        &event_open_state,
                        Some(&sender),
                        "the Outer Environment reported a failure",
                    );
                }
                return Ok(());
            }

            let Some(next_state) =
                classify_message(event_open_state.load(Ordering::Acquire), message)
            else {
                return Ok(());
            };

            match message {
                BridgeMessage::ShellReady => {
                    match live_source_allows_action(&sender, &source) {
                        Ok(true) => {}
                        Ok(false) => {
                            mark_failed(
                                &event_window,
                                &event_open_state,
                                Some(&sender),
                                "the WebView2 source changed before handle delivery",
                            );
                            return Ok(());
                        }
                        Err(error) => {
                            mark_failed(&event_window, &event_open_state, Some(&sender), error);
                            return Ok(());
                        }
                    }
                    if event_open_state
                        .compare_exchange(
                            OPEN_IDLE,
                            next_state,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        return Ok(());
                    }
                    if let Err(error) = post_file_handle(&delivery_environment, &sender, &path) {
                        mark_failed(&event_window, &event_open_state, Some(&sender), error);
                    } else {
                        eprintln!("webview2 bridge: delivered read/write file handle");
                    }
                }
                BridgeMessage::HandleReceived | BridgeMessage::PhotopeaOpened => {
                    match live_source_allows_action(&sender, &source) {
                        Ok(true) => {}
                        Ok(false) => {
                            mark_failed(
                                &event_window,
                                &event_open_state,
                                Some(&sender),
                                "the WebView2 source changed during Photopea open",
                            );
                            return Ok(());
                        }
                        Err(error) => {
                            mark_failed(&event_window, &event_open_state, Some(&sender), error);
                            return Ok(());
                        }
                    }
                    let expected = match message {
                        BridgeMessage::HandleReceived => OPEN_HANDLE_SENT,
                        BridgeMessage::PhotopeaOpened => OPEN_HANDLE_RECEIVED,
                        _ => unreachable!(),
                    };
                    if event_open_state
                        .compare_exchange(expected, next_state, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                    {
                        return Ok(());
                    }
                    if message == BridgeMessage::PhotopeaOpened {
                        eprintln!("webview2 bridge: Photopea opened {name}");
                        let title = format!("PASS: Opened {name}");
                        if let Err(error) = event_window.set_title(&title) {
                            eprintln!("webview2 bridge: success title update failed: {error}");
                        }
                    }
                }
                BridgeMessage::SaveRequested | BridgeMessage::SaveComplete => {}
                BridgeMessage::Error => unreachable!(),
            }
            Ok(())
        }));
        let mut token = 0;
        if let Err(error) = webview.add_WebMessageReceived(&handler, &mut token) {
            eprintln!("webview2 bridge: add_WebMessageReceived failed: {error}");
            mark_failed(&failure_window, &open_state, Some(&webview), error);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        classify_message, decode_message, source_is_allowed, sources_allow_action, BridgeMessage,
        OPEN_HANDLE_RECEIVED, OPEN_HANDLE_SENT, OPEN_IDLE, OPEN_OPENED,
    };

    #[test]
    fn message_wire_requires_exact_json_string_envelope() {
        for (wire, message) in [
            (
                r#""{\"type\":\"webwrap-photopea-shell-ready\"}""#,
                BridgeMessage::ShellReady,
            ),
            (
                r#""{\"type\":\"webwrap-photopea-handle-received\"}""#,
                BridgeMessage::HandleReceived,
            ),
            (
                r#""{\"type\":\"webwrap-photopea-opened\"}""#,
                BridgeMessage::PhotopeaOpened,
            ),
            (
                r#""{\"type\":\"webwrap-photopea-save-requested\"}""#,
                BridgeMessage::SaveRequested,
            ),
            (
                r#""{\"type\":\"webwrap-photopea-save-complete\"}""#,
                BridgeMessage::SaveComplete,
            ),
            (r#""{\"type\":\"webwrap-error\"}""#, BridgeMessage::Error),
        ] {
            assert_eq!(decode_message(wire), Some(message));
        }

        for rejected in [
            r#"{"type":"webwrap-photopea-shell-ready"}"#,
            r#" "{\"type\":\"webwrap-photopea-shell-ready\"}""#,
            r#""{\"type\": \"webwrap-photopea-shell-ready\"}""#,
            r#""{\"type\":\"unknown\"}""#,
            r#""not json""#,
        ] {
            assert_eq!(decode_message(rejected), None);
        }
    }

    #[test]
    fn local_origin_and_live_source_must_both_match() {
        assert!(source_is_allowed("http://tauri.localhost/"));
        assert!(source_is_allowed("http://tauri.localhost/photopea.html"));
        assert!(!source_is_allowed("https://tauri.localhost/"));
        assert!(!source_is_allowed("http://tauri.localhost.evil.test/"));
        assert!(!source_is_allowed("http://evil.test/"));

        assert!(sources_allow_action(
            "http://tauri.localhost/photopea.html",
            "http://tauri.localhost/photopea.html",
        ));
        assert!(!sources_allow_action(
            "http://tauri.localhost/photopea.html",
            "https://evil.test/",
        ));
        assert!(!sources_allow_action(
            "https://evil.test/",
            "http://tauri.localhost/photopea.html",
        ));
    }

    #[test]
    fn status_classification_allows_only_the_open_sequence() {
        assert_eq!(
            classify_message(OPEN_IDLE, BridgeMessage::ShellReady),
            Some(OPEN_HANDLE_SENT),
        );
        assert_eq!(
            classify_message(OPEN_HANDLE_SENT, BridgeMessage::HandleReceived),
            Some(OPEN_HANDLE_RECEIVED),
        );
        assert_eq!(
            classify_message(OPEN_HANDLE_RECEIVED, BridgeMessage::PhotopeaOpened),
            Some(OPEN_OPENED),
        );
        assert_eq!(
            classify_message(OPEN_HANDLE_SENT, BridgeMessage::PhotopeaOpened),
            None,
        );
        assert_eq!(
            classify_message(OPEN_IDLE, BridgeMessage::HandleReceived),
            None,
        );
        assert_eq!(
            classify_message(OPEN_OPENED, BridgeMessage::ShellReady),
            None,
        );
    }
}
