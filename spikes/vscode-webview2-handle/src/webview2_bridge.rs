use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};
use tauri::Manager;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment14,
    ICoreWebView2ObjectCollection, ICoreWebView2ObjectCollectionView, ICoreWebView2_23,
    COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use webview2_com::{
    take_pwstr, CallDevToolsProtocolMethodCompletedHandler, WebMessageReceivedEventHandler,
};
use windows::core::{IUnknown, Interface, PCWSTR, PWSTR};

use crate::{LaunchRequest, Mode};

const PROBE_READY: &str = r#"{"type":"webwrap-probe-ready"}"#;
const PROBE_COMPLETE: &str = r#"{"type":"webwrap-probe-complete"}"#;
const VSCODE_READY: &str = r#"{"type":"webwrap-vscode-ready"}"#;
const VSCODE_HANDLE_READY: &str = r#"{"type":"webwrap-vscode-handle-ready"}"#;

const OPEN_IDLE: u8 = 0;
const OPEN_IN_FLIGHT: u8 = 1;
const OPEN_COMPLETED: u8 = 2;
const OPEN_FAILED: u8 = 3;

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

fn sources_allow_action(event_source: &str, live_source: &str, mode: Mode) -> bool {
    source_is_allowed(event_source, mode) && source_is_allowed(live_source, mode)
}

unsafe fn live_source_allows_action(
    sender: &ICoreWebView2,
    event_source: &str,
    mode: Mode,
) -> windows::core::Result<bool> {
    let mut live_source = PWSTR::null();
    sender.Source(&mut live_source)?;
    Ok(sources_allow_action(
        event_source,
        &take_pwstr(live_source),
        mode,
    ))
}

unsafe fn mark_probe_failed(window: &tauri::WebviewWindow, webview: Option<&ICoreWebView2>) {
    if let Some(webview) = webview {
        if let Err(error) = webview.PostWebMessageAsJson(windows::core::w!(
            r#"{"type":"webwrap-error","message":"Native bridge failed"}"#
        )) {
            eprintln!("webview2 bridge: error message delivery failed: {error}");
        }
    }
    if let Err(error) = window.set_title("FAIL: Native bridge failed") {
        eprintln!("webview2 bridge: failure title update failed: {error}");
    }
}

fn mark_vscode_failed(
    window: &tauri::WebviewWindow,
    open_state: &AtomicU8,
    reason: impl std::fmt::Display,
) {
    if open_state.swap(OPEN_FAILED, Ordering::AcqRel) == OPEN_FAILED {
        return;
    }
    eprintln!("webview2 bridge: VS Code open failed: {reason}");
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

unsafe fn dispatch_open_file_step(
    webview: ICoreWebView2,
    expected_source: String,
    failure_window: tauri::WebviewWindow,
    open_state: Arc<AtomicU8>,
    step: usize,
) -> windows::core::Result<()> {
    if open_state.load(Ordering::Acquire) != OPEN_IN_FLIGHT {
        return Ok(());
    }
    match live_source_allows_action(&webview, &expected_source, Mode::Vscode) {
        Ok(true) => {}
        Ok(false) => {
            mark_vscode_failed(
                &failure_window,
                &open_state,
                "WebView2 source changed during Open File dispatch",
            );
            return Ok(());
        }
        Err(error) => {
            mark_vscode_failed(
                &failure_window,
                &open_state,
                format!("live Source failed during Open File dispatch: {error}"),
            );
            return Ok(());
        }
    }

    let (method, parameters) = match step {
        0 => (
            windows::core::w!("Input.dispatchKeyEvent"),
            windows::core::w!(
                r#"{"type":"rawKeyDown","modifiers":10,"windowsVirtualKeyCode":80,"nativeVirtualKeyCode":80,"key":"p","code":"KeyP"}"#
            ),
        ),
        1 => (
            windows::core::w!("Input.dispatchKeyEvent"),
            windows::core::w!(
                r#"{"type":"keyUp","modifiers":10,"windowsVirtualKeyCode":80,"nativeVirtualKeyCode":80,"key":"p","code":"KeyP"}"#
            ),
        ),
        2 => (
            windows::core::w!("Input.insertText"),
            windows::core::w!(r#"{"text":"Open File..."}"#),
        ),
        3 => (
            windows::core::w!("Runtime.evaluate"),
            windows::core::w!(
                r#"{"expression":"new Promise(resolve=>{const deadline=performance.now()+5000;const ready=()=>{const active=document.activeElement;const id=active?.getAttribute('aria-activedescendant');const selected=id?document.getElementById(id):null;const isReady=typeof active?.value==='string'&&active.value.endsWith('Open File...')&&selected?.textContent?.includes('Open File...');if(isReady)resolve(true);else if(performance.now()>=deadline)resolve(false);else requestAnimationFrame(ready)};ready()})","awaitPromise":true,"returnByValue":true}"#
            ),
        ),
        4 => (
            windows::core::w!("Input.dispatchKeyEvent"),
            windows::core::w!(
                r#"{"type":"rawKeyDown","modifiers":0,"windowsVirtualKeyCode":13,"nativeVirtualKeyCode":13,"key":"Enter","code":"Enter"}"#
            ),
        ),
        5 => (
            windows::core::w!("Input.dispatchKeyEvent"),
            windows::core::w!(
                r#"{"type":"keyUp","modifiers":0,"windowsVirtualKeyCode":13,"nativeVirtualKeyCode":13,"key":"Enter","code":"Enter"}"#
            ),
        ),
        _ => return Ok(()),
    };
    let next_webview = webview.clone();
    let next_source = expected_source.clone();
    let next_window = failure_window.clone();
    let next_state = Arc::clone(&open_state);
    let completed =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, response| {
            let result = status.and_then(|_| {
                if next_state.load(Ordering::Acquire) != OPEN_IN_FLIGHT {
                    return Ok(());
                }
                if step == 3 {
                    let compact: String = response
                        .chars()
                        .filter(|character| !character.is_whitespace())
                        .collect();
                    if !compact.contains(r#""value":true"#) {
                        return Err(windows::core::Error::new(
                            windows::core::HRESULT(0x80004004u32 as i32),
                            "Open File command did not become ready",
                        ));
                    }
                }
                if step == 5 {
                    next_state.store(OPEN_COMPLETED, Ordering::Release);
                    eprintln!("webview2 bridge: VS Code Open File command completed");
                    return Ok(());
                }
                unsafe {
                    dispatch_open_file_step(
                        next_webview,
                        next_source,
                        next_window,
                        next_state,
                        step + 1,
                    )
                }
            });
            if let Err(error) = &result {
                mark_vscode_failed(
                    &failure_window,
                    &open_state,
                    format!("Open File dispatch step {}: {error}", step + 1),
                );
            }
            result
        }));
    webview.CallDevToolsProtocolMethod(method, parameters, &completed)
}

unsafe fn dispatch_open_file(
    webview: &ICoreWebView2,
    expected_source: String,
    failure_window: tauri::WebviewWindow,
    open_state: Arc<AtomicU8>,
) -> windows::core::Result<()> {
    dispatch_open_file_step(
        webview.clone(),
        expected_source,
        failure_window,
        open_state,
        0,
    )
}

pub fn install(window: &tauri::WebviewWindow, request: LaunchRequest) -> tauri::Result<()> {
    let app_handle = window.app_handle().clone();
    let failure_window = window.clone();
    let open_state = Arc::new(AtomicU8::new(OPEN_IDLE));
    window.with_webview(move |platform| unsafe {
        let environment = platform.environment();
        let webview = match platform.controller().CoreWebView2() {
            Ok(webview) => webview,
            Err(error) => {
                eprintln!("webview2 bridge: CoreWebView2 unavailable: {error}");
                if request.mode == Mode::Probe {
                    mark_probe_failed(&failure_window, None);
                } else {
                    mark_vscode_failed(&failure_window, &open_state, error);
                }
                return;
            }
        };
        let delivery_environment = environment.clone();
        let mode = request.mode;
        let event_window = failure_window.clone();
        let event_open_state = Arc::clone(&open_state);
        let mut handle_sent = false;
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
            let (Some(sender), Some(args)) = (sender, args) else {
                if request.mode == Mode::Vscode {
                    mark_vscode_failed(
                        &event_window,
                        &event_open_state,
                        "WebView2 event did not include sender and arguments",
                    );
                }
                return Ok(());
            };

            let mut message = PWSTR::null();
            if let Err(error) = args.WebMessageAsJson(&mut message) {
                eprintln!("webview2 bridge: WebMessageAsJson failed: {error}");
                if request.mode == Mode::Probe {
                    mark_probe_failed(&event_window, Some(&sender));
                } else {
                    mark_vscode_failed(&event_window, &event_open_state, error);
                }
                return Ok(());
            }
            let message = take_pwstr(message);
            let Some(message) = decode_message(&message) else {
                return Ok(());
            };

            let mut source = PWSTR::null();
            if let Err(error) = args.Source(&mut source) {
                eprintln!("webview2 bridge: Source failed: {error}");
                if request.mode == Mode::Probe {
                    mark_probe_failed(&event_window, Some(&sender));
                } else {
                    mark_vscode_failed(&event_window, &event_open_state, error);
                }
                return Ok(());
            }
            let source = take_pwstr(source);
            if !source_is_allowed(&source, request.mode) {
                if request.mode == Mode::Vscode {
                    mark_vscode_failed(
                        &event_window,
                        &event_open_state,
                        "event source is not an allowed vscode.dev origin",
                    );
                }
                return Ok(());
            }

            match (request.mode, message) {
                (Mode::Probe, PROBE_READY) if !handle_sent => {
                    match live_source_allows_action(&sender, &source, request.mode) {
                        Ok(true) => {}
                        Ok(false) => return Ok(()),
                        Err(error) => {
                            eprintln!("webview2 bridge: live Source failed: {error}");
                            mark_probe_failed(&event_window, Some(&sender));
                            return Ok(());
                        }
                    }
                    if let Err(error) =
                        post_file_handle(&delivery_environment, &sender, &request.path)
                    {
                        eprintln!("webview2 bridge: handle delivery failed: {error}");
                        mark_probe_failed(&event_window, Some(&sender));
                    } else {
                        handle_sent = true;
                    }
                }
                (Mode::Probe, PROBE_COMPLETE) if handle_sent => {
                    match live_source_allows_action(&sender, &source, request.mode) {
                        Ok(true) => app_handle.exit(0),
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("webview2 bridge: live Source failed: {error}");
                            mark_probe_failed(&event_window, Some(&sender));
                        }
                    }
                }
                (Mode::Vscode, VSCODE_READY) if !handle_sent => {
                    match live_source_allows_action(&sender, &source, request.mode) {
                        Ok(true) => {}
                        Ok(false) => {
                            mark_vscode_failed(
                                &event_window,
                                &event_open_state,
                                "live source is not an allowed vscode.dev origin",
                            );
                            return Ok(());
                        }
                        Err(error) => {
                            eprintln!("webview2 bridge: live Source failed: {error}");
                            mark_vscode_failed(&event_window, &event_open_state, error);
                            return Ok(());
                        }
                    }
                    if let Err(error) =
                        post_file_handle(&delivery_environment, &sender, &request.path)
                    {
                        eprintln!("webview2 bridge: handle delivery failed: {error}");
                        mark_vscode_failed(&event_window, &event_open_state, error);
                    } else {
                        handle_sent = true;
                    }
                }
                (Mode::Vscode, VSCODE_HANDLE_READY) if handle_sent => {
                    if event_open_state
                        .compare_exchange(
                            OPEN_IDLE,
                            OPEN_IN_FLIGHT,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        return Ok(());
                    }
                    if let Err(error) = dispatch_open_file(
                        &sender,
                        source,
                        event_window.clone(),
                        Arc::clone(&event_open_state),
                    ) {
                        eprintln!("webview2 bridge: Open File dispatch failed: {error}");
                        mark_vscode_failed(&event_window, &event_open_state, error);
                    }
                }
                _ => {}
            }
            Ok(())
        }));
        let mut token = 0;
        if let Err(error) = webview.add_WebMessageReceived(&handler, &mut token) {
            eprintln!("webview2 bridge: add_WebMessageReceived failed: {error}");
            if mode == Mode::Probe {
                mark_probe_failed(&failure_window, Some(&webview));
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        decode_message, source_is_allowed, sources_allow_action, PROBE_COMPLETE, PROBE_READY,
        VSCODE_HANDLE_READY, VSCODE_READY,
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

    #[test]
    fn action_requires_event_and_live_sources() {
        assert!(sources_allow_action(
            "http://tauri.localhost/probe.html",
            "http://tauri.localhost/probe.html",
            Mode::Probe
        ));
        assert!(!sources_allow_action(
            "http://tauri.localhost/probe.html",
            "https://evil.test/",
            Mode::Probe
        ));
        assert!(!sources_allow_action(
            "https://evil.test/",
            "http://tauri.localhost/probe.html",
            Mode::Probe
        ));
    }
}
