use std::{os::windows::ffi::OsStrExt, path::Path};
use tauri::Manager;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment14,
    ICoreWebView2ObjectCollection, ICoreWebView2ObjectCollectionView, ICoreWebView2_23,
    COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
use windows::core::{IUnknown, Interface, PCWSTR, PWSTR};

use crate::{LaunchRequest, Mode};

const PROBE_READY: &str = r#"{"type":"webwrap-probe-ready"}"#;
const PROBE_COMPLETE: &str = r#"{"type":"webwrap-probe-complete"}"#;
const VSCODE_READY: &str = r#"{"type":"webwrap-vscode-ready"}"#;
const VSCODE_HANDLE_READY: &str = r#"{"type":"webwrap-vscode-handle-ready"}"#;

fn decode_message(message: &str) -> Option<&'static str> {
    match message {
        r#""{\"type\":\"webwrap-probe-ready\"}""# => Some(PROBE_READY),
        r#""{\"type\":\"webwrap-probe-complete\"}""# => Some(PROBE_COMPLETE),
        r#""{\"type\":\"webwrap-vscode-ready\"}""# => Some(VSCODE_READY),
        r#""{\"type\":\"webwrap-vscode-handle-ready\"}""# => Some(VSCODE_HANDLE_READY),
        _ => None,
    }
}

fn source_is_allowed(source: &str, mode: Mode) -> bool {
    let Ok(url) = tauri::Url::parse(source) else {
        return false;
    };
    match mode {
        Mode::Vscode => {
            url.scheme() == "https"
                && url.host_str() == Some("vscode.dev")
                && url.port_or_known_default() == Some(443)
        }
        Mode::Probe => {
            url.scheme() == "http"
                && url.host_str() == Some("tauri.localhost")
                && url.port_or_known_default() == Some(80)
        }
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

pub fn install(window: &tauri::WebviewWindow, request: LaunchRequest) -> tauri::Result<()> {
    let app_handle = window.app_handle().clone();
    window.with_webview(move |platform| unsafe {
        let environment = platform.environment();
        let webview = match platform.controller().CoreWebView2() {
            Ok(webview) => webview,
            Err(error) => {
                eprintln!("webview2 bridge: CoreWebView2 unavailable: {error}");
                return;
            }
        };
        let delivery_environment = environment.clone();
        let delivery_webview = webview.clone();
        let mut handle_sent = false;
        let mut open_triggered = false;
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |_sender, args| {
            let Some(args) = args else {
                return Ok(());
            };

            let mut message = PWSTR::null();
            if let Err(error) = args.WebMessageAsJson(&mut message) {
                eprintln!("webview2 bridge: WebMessageAsJson failed: {error}");
                return Ok(());
            }
            let message = take_pwstr(message);
            let Some(message) = decode_message(&message) else {
                return Ok(());
            };

            let mut source = PWSTR::null();
            if let Err(error) = args.Source(&mut source) {
                eprintln!("webview2 bridge: Source failed: {error}");
                return Ok(());
            }
            let source = take_pwstr(source);
            if !source_is_allowed(&source, request.mode) {
                return Ok(());
            }

            match (request.mode, message) {
                (Mode::Probe, PROBE_READY) if !handle_sent => {
                    if let Err(error) =
                        post_file_handle(&delivery_environment, &delivery_webview, &request.path)
                    {
                        eprintln!("webview2 bridge: handle delivery failed: {error}");
                    } else {
                        handle_sent = true;
                    }
                }
                (Mode::Probe, PROBE_COMPLETE) if handle_sent => app_handle.exit(0),
                (Mode::Vscode, VSCODE_READY | VSCODE_HANDLE_READY) => {
                    // Task 3 replaces this no-op with Ctrl+O and handle delivery.
                    let _ = &mut open_triggered;
                }
                _ => {}
            }
            Ok(())
        }));
        let mut token = 0;
        if let Err(error) = webview.add_WebMessageReceived(&handler, &mut token) {
            eprintln!("webview2 bridge: add_WebMessageReceived failed: {error}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        decode_message, source_is_allowed, PROBE_COMPLETE, PROBE_READY, VSCODE_HANDLE_READY,
        VSCODE_READY,
    };
    use crate::Mode;

    #[test]
    fn origin_policy_is_exact_and_mode_specific() {
        assert!(source_is_allowed("https://vscode.dev/", Mode::Vscode));
        assert!(source_is_allowed(
            "https://vscode.dev/github/repo",
            Mode::Vscode
        ));
        assert!(!source_is_allowed(
            "https://vscode.dev.evil.test/",
            Mode::Vscode
        ));
        assert!(!source_is_allowed("http://vscode.dev/", Mode::Vscode));
        assert!(source_is_allowed(
            "http://tauri.localhost/probe.html",
            Mode::Probe
        ));
        assert!(!source_is_allowed(
            "http://tauri.localhost/probe.html",
            Mode::Vscode
        ));
    }

    #[test]
    fn message_wire_requires_exact_json_string_envelope() {
        for (wire, decoded) in [
            (r#""{\"type\":\"webwrap-probe-ready\"}""#, PROBE_READY),
            (r#""{\"type\":\"webwrap-probe-complete\"}""#, PROBE_COMPLETE),
            (r#""{\"type\":\"webwrap-vscode-ready\"}""#, VSCODE_READY),
            (
                r#""{\"type\":\"webwrap-vscode-handle-ready\"}""#,
                VSCODE_HANDLE_READY,
            ),
        ] {
            assert_eq!(decode_message(wire), Some(decoded));
        }
        for rejected in [
            PROBE_READY,
            r#""{\"type\": \"webwrap-probe-ready\"}""#,
            r#""{\"type\":\"unknown\"}""#,
            r#""not json""#,
        ] {
            assert_eq!(decode_message(rejected), None);
        }
    }
}
