use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment14,
    ICoreWebView2ObjectCollection, ICoreWebView2ObjectCollectionView, ICoreWebView2_23,
    COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
use windows::core::{IUnknown, Interface, PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

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
const SAVE_IDLE: u8 = 0;
const SAVE_IN_FLIGHT: u8 = 1;
const MAX_PSD_BYTES: usize = 32 * 1024 * 1024;
const MAX_PSD_BASE64_BYTES: usize = MAX_PSD_BYTES.div_ceil(3) * 4;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BridgeMessage {
    ShellReady,
    HandleReceived,
    PhotopeaOpened,
    SaveRequested,
    SaveComplete,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
struct SavePayload {
    source_id: String,
    bytes: Vec<u8>,
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

fn decode_inner_message(wire: &str) -> Option<String> {
    let inner = serde_json::from_str::<String>(wire).ok()?;
    (serde_json::to_string(&inner).ok()? == wire).then_some(inner)
}

fn decode_save_message(wire: &str) -> Option<SavePayload> {
    let inner = decode_inner_message(wire)?;
    let value = serde_json::from_str::<serde_json::Value>(&inner).ok()?;
    let object = value.as_object()?;
    if object.get("type")?.as_str()? != "webwrap-photopea-save" {
        return None;
    }
    let source_id = object.get("source")?.as_str()?;
    if !source_id.starts_with("webwrap-local:")
        || source_id.len() > 256
        || source_id.bytes().any(|byte| byte == 0 || byte < 0x20)
    {
        return None;
    }
    let encoded = object.get("data")?.as_str()?;
    if encoded.is_empty() || encoded.len() > MAX_PSD_BASE64_BYTES {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_PSD_BYTES || !bytes.starts_with(b"8BPS") {
        return None;
    }
    Some(SavePayload {
        source_id: source_id.to_string(),
        bytes,
    })
}

fn looks_like_save_message(wire: &str) -> bool {
    let Some(inner) = decode_inner_message(wire) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&inner)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|kind| kind == "webwrap-photopea-save")
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

fn mark_save_failed(
    window: &tauri::WebviewWindow,
    save_state: &AtomicU8,
    webview: &ICoreWebView2,
    reason: impl std::fmt::Display,
) {
    save_state.store(SAVE_IDLE, Ordering::Release);
    eprintln!("webview2 bridge: Photopea save failed: {reason}");
    if let Err(error) =
        unsafe { webview.PostWebMessageAsJson(windows::core::w!(r#"{"type":"webwrap-error"}"#)) }
    {
        eprintln!("webview2 bridge: save error delivery failed: {error}");
    }
    if let Err(error) = window.set_title("FAIL: Photopea save failed") {
        eprintln!("webview2 bridge: failure title update failed: {error}");
    }
}

unsafe fn post_file_handle(
    environment: &ICoreWebView2Environment,
    webview: &ICoreWebView2,
    path: &Path,
    source_id: &str,
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
    let message = serde_json::json!({
        "type": "webwrap-file",
        "source": source_id,
    });
    let message: Vec<u16> = serde_json::to_string(&message)
        .expect("file-handle message is serializable")
        .encode_utf16()
        .chain([0])
        .collect();
    webview
        .cast::<ICoreWebView2_23>()?
        .PostWebMessageAsJsonWithAdditionalObjects(PCWSTR(message.as_ptr()), &collection)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().replace(['\r', '\n'], "_"))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "file".into())
}

fn source_id_for(name: &str) -> String {
    let mut safe = String::with_capacity(name.len().min(80));
    let mut previous_dash = false;
    for character in name.chars() {
        if safe.len() >= 80 {
            break;
        }
        let allowed = character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-');
        if allowed {
            safe.push(character);
            previous_dash = false;
        } else if !previous_dash {
            safe.push('-');
            previous_dash = true;
        }
    }
    let safe = safe.trim_matches(['.', '-']).to_string();
    let safe = if safe.is_empty() { "file" } else { &safe };
    let token = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    format!("webwrap-local:{safe}:{}-{token}", std::process::id())
}

fn psd_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn replace_file_atomically(temp: &Path, destination: &Path) -> io::Result<()> {
    let temp: Vec<u16> = temp.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(temp.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| io::Error::other(error.to_string()))
}

fn write_psd_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() < 4 || !bytes.starts_with(b"8BPS") {
        return Err(psd_error("replacement is not a PSD"));
    }
    if bytes.len() > MAX_PSD_BYTES {
        return Err(psd_error("replacement exceeds the 32 MiB spike limit"));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;

    for _ in 0..16 {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".webwrap-photopea-{}-{id}.tmp", std::process::id()));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok::<(), io::Error>(())
        })();
        drop(file);
        let result = result.and_then(|()| replace_file_atomically(&temp, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary PSD path",
    ))
}

pub fn install(window: &tauri::WebviewWindow, request: LaunchRequest) -> tauri::Result<()> {
    let failure_window = window.clone();
    let open_state = Arc::new(AtomicU8::new(OPEN_IDLE));
    let save_state = Arc::new(AtomicU8::new(SAVE_IDLE));
    let path = request.path.clone();
    let name = display_name(&request.path);
    let source_id = source_id_for(&name);

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
        let event_save_state = Arc::clone(&save_state);
        let event_source_id = source_id.clone();
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
            let save_payload = decode_save_message(&message);
            let decoded_message = decode_message(&message);
            if save_payload.is_none() && decoded_message.is_none() {
                return Ok(());
            }

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

            if looks_like_save_message(&message) {
                match live_source_allows_action(&sender, &source) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => return Ok(()),
                }
                let Some(payload) = save_payload else {
                    mark_save_failed(
                        &event_window,
                        &event_save_state,
                        &sender,
                        "rejected malformed Photopea save payload",
                    );
                    return Ok(());
                };
                if event_open_state.load(Ordering::Acquire) != OPEN_OPENED
                    || event_save_state.load(Ordering::Acquire) != SAVE_IN_FLIGHT
                    || payload.source_id != event_source_id
                {
                    mark_save_failed(
                        &event_window,
                        &event_save_state,
                        &sender,
                        "rejected out-of-order or mismatched Photopea save",
                    );
                    return Ok(());
                }
                let started = Instant::now();
                let result = write_psd_atomically(&path, &payload.bytes);
                event_save_state.store(SAVE_IDLE, Ordering::Release);
                match result {
                    Ok(()) => {
                        eprintln!(
                            "webview2 bridge: saved Photopea PSD source={} bytes={} elapsed_ms={}",
                            payload.source_id,
                            payload.bytes.len(),
                            started.elapsed().as_millis()
                        );
                        if let Err(error) = sender.PostWebMessageAsJson(windows::core::w!(
                            r#"{"type":"webwrap-photopea-save-complete"}"#
                        )) {
                            eprintln!("webview2 bridge: save-complete delivery failed: {error}");
                        }
                        let title = format!("PASS: Saved {name}");
                        if let Err(error) = event_window.set_title(&title) {
                            eprintln!("webview2 bridge: success title update failed: {error}");
                        }
                    }
                    Err(error) => {
                        mark_save_failed(&event_window, &event_save_state, &sender, error);
                    }
                }
                return Ok(());
            }

            let Some(message) = decoded_message else {
                return Ok(());
            };

            if message == BridgeMessage::Error {
                if event_open_state.load(Ordering::Acquire) == OPEN_OPENED {
                    event_save_state.store(SAVE_IDLE, Ordering::Release);
                    eprintln!("webview2 bridge: Outer Environment reported a save/open failure");
                    if let Err(error) = event_window.set_title("FAIL: Photopea operation failed") {
                        eprintln!("webview2 bridge: failure title update failed: {error}");
                    }
                } else if let Err(error) = live_source_allows_action(&sender, &source) {
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

            if message == BridgeMessage::SaveRequested {
                if live_source_allows_action(&sender, &source).unwrap_or(false)
                    && event_open_state.load(Ordering::Acquire) == OPEN_OPENED
                    && event_save_state
                        .compare_exchange(
                            SAVE_IDLE,
                            SAVE_IN_FLIGHT,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                {
                    eprintln!("webview2 bridge: Photopea save requested");
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
                    if let Err(error) =
                        post_file_handle(&delivery_environment, &sender, &path, &event_source_id)
                    {
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use base64::{engine::general_purpose::STANDARD, Engine as _};

    use super::{
        classify_message, decode_message, decode_save_message, source_is_allowed,
        sources_allow_action, write_psd_atomically, BridgeMessage, MAX_PSD_BASE64_BYTES,
        OPEN_HANDLE_RECEIVED, OPEN_HANDLE_SENT, OPEN_IDLE, OPEN_OPENED,
    };

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "webwrap-photopea-save-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    fn save_wire(source: &str, data: &str) -> String {
        let inner = serde_json::json!({
            "type": "webwrap-photopea-save",
            "source": source,
            "data": data,
        });
        serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap()
    }

    fn valid_psd(suffix: &[u8]) -> Vec<u8> {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend_from_slice(suffix);
        bytes
    }

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

    #[test]
    fn valid_save_envelope_decodes_source_and_psd_bytes() {
        let bytes = valid_psd(b"fixture");
        let wire = save_wire("webwrap-local:image.psd:token", &STANDARD.encode(&bytes));

        let payload = decode_save_message(&wire).unwrap();

        assert_eq!(payload.source_id, "webwrap-local:image.psd:token");
        assert_eq!(payload.bytes, bytes);
    }

    #[test]
    fn save_envelope_rejects_wrong_type() {
        let inner = serde_json::json!({
            "type": "webwrap-photopea-save-complete",
            "source": "webwrap-local:image.psd:token",
            "data": STANDARD.encode(valid_psd(b"fixture")),
        });
        let wire = serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap();

        assert!(decode_save_message(&wire).is_none());
    }

    #[test]
    fn save_envelope_rejects_missing_source_or_data() {
        for inner in [
            serde_json::json!({
                "type": "webwrap-photopea-save",
                "data": STANDARD.encode(valid_psd(b"fixture")),
            }),
            serde_json::json!({
                "type": "webwrap-photopea-save",
                "source": "webwrap-local:image.psd:token",
            }),
        ] {
            let wire = serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap();
            assert!(decode_save_message(&wire).is_none());
        }
    }

    #[test]
    fn save_envelope_rejects_malformed_base64() {
        let wire = save_wire("webwrap-local:image.psd:token", "not base64");

        assert!(decode_save_message(&wire).is_none());
    }

    #[test]
    fn save_envelope_rejects_over_cap_data_before_decoding() {
        let oversized = "A".repeat(MAX_PSD_BASE64_BYTES + 1);
        let wire = save_wire("webwrap-local:image.psd:token", &oversized);

        assert!(decode_save_message(&wire).is_none());
    }

    #[test]
    fn save_envelope_rejects_non_psd_bytes() {
        let wire = save_wire(
            "webwrap-local:image.psd:token",
            &STANDARD.encode(b"not a PSD"),
        );

        assert!(decode_save_message(&wire).is_none());
    }

    #[test]
    fn atomic_psd_write_replaces_destination_bytes() {
        let dir = temp_dir("success");
        let path = dir.join("image.psd");
        let replacement = valid_psd(b"replacement");
        std::fs::write(&path, b"8BPS original").unwrap();

        write_psd_atomically(&path, &replacement).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), replacement);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_psd_write_leaves_original_unchanged() {
        let dir = temp_dir("invalid");
        let path = dir.join("image.psd");
        let original = b"8BPS original";
        std::fs::write(&path, original).unwrap();

        assert!(write_psd_atomically(&path, b"not a PSD").is_err());

        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_destination_replace_leaves_existing_destination_untouched() {
        let dir = temp_dir("replace-failure");
        let path = dir.join("image.psd");
        std::fs::create_dir(&path).unwrap();

        assert!(write_psd_atomically(&path, &valid_psd(b"replacement")).is_err());

        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
