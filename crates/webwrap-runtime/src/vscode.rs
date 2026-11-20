//! The `vscodeDev` adapter. A file opens as a read-write handle that
//! vscode.dev's own "Open File..." command asks for, so vscode.dev's Ctrl+S
//! writes the original file. Ported from `spikes/vscode-webview2-handle`.
//!
//! vscode.dev has no `launchQueue` consumer, and its Open File command calls
//! `showOpenFilePicker({multiple:false})`: one file per run of the command.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment14, ICoreWebView2ObjectCollection,
    ICoreWebView2ObjectCollectionView, ICoreWebView2_23,
    COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use windows::core::{IUnknown, Interface, HSTRING};

/// The adapter's page script for an app that trusts `origins`.
pub fn script(origins: &[String]) -> String {
    let origins = serde_json::to_string(origins).expect("strings always serialize");
    include_str!("vscode.js").replace("__WEBWRAP_TRUSTED__", &origins)
}

/// Post `message` with a read-write handle to `path` as its one additional object.
pub fn post_handle(
    environment: &ICoreWebView2Environment14,
    webview: &ICoreWebView2_23,
    path: &Path,
    message: &str,
) -> windows::core::Result<()> {
    unsafe {
        let handle = environment.CreateWebFileSystemFileHandle(
            &HSTRING::from(path),
            COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
        )?;
        let mut items = [Some(handle.cast::<IUnknown>()?)];
        let mut collection: Option<ICoreWebView2ObjectCollection> = None;
        environment.CreateObjectCollection(1, items.as_mut_ptr(), &mut collection)?;
        let collection = collection
            .ok_or_else(windows::core::Error::empty)?
            .cast::<ICoreWebView2ObjectCollectionView>()?;
        webview.PostWebMessageAsJsonWithAdditionalObjects(&HSTRING::from(message), &collection)
    }
}

/// How a run of the Open File command ended.
#[derive(Debug)]
pub enum Outcome {
    Done,
    /// The document changed or became untrusted, so the run stopped. Its
    /// handles went with the old document; nothing needs telling.
    PageChanged,
    Failed(String),
}

/// Waits up to 5 s for `ready()`, written as an expression, to be true. A
/// timer rather than requestAnimationFrame, which stops while minimized.
macro_rules! wait_until {
    ($ready:literal) => {
        concat!(
            r#"{"expression":"new Promise(resolve=>{const deadline=performance.now()+5000;const ready=()=>"#,
            $ready,
            r#";const poll=()=>{if(ready())resolve(true);else if(performance.now()>=deadline)resolve(false);else setTimeout(poll,50)};poll()})","awaitPromise":true,"returnByValue":true}"#
        )
    };
}

/// What a user types to run "File: Open File...". The two checks make sure
/// the text goes into the command palette, never into an open document, and
/// that Enter runs Open File and nothing else.
const STEPS: [(&str, &str); 7] = [
    (
        "Input.dispatchKeyEvent",
        r#"{"type":"rawKeyDown","modifiers":10,"windowsVirtualKeyCode":80,"nativeVirtualKeyCode":80,"key":"p","code":"KeyP"}"#,
    ),
    (
        "Input.dispatchKeyEvent",
        r#"{"type":"keyUp","modifiers":10,"windowsVirtualKeyCode":80,"nativeVirtualKeyCode":80,"key":"p","code":"KeyP"}"#,
    ),
    // The palette's input is focused and in command mode.
    (
        "Runtime.evaluate",
        wait_until!(
            r#"{const a=document.activeElement;return a?.tagName==='INPUT'&&a.value.startsWith('>')}"#
        ),
    ),
    ("Input.insertText", r#"{"text":"Open File..."}"#),
    // The selected entry is Open File.
    (
        "Runtime.evaluate",
        wait_until!(
            r#"{const a=document.activeElement;const id=a?.getAttribute('aria-activedescendant');const s=id?document.getElementById(id):null;return a?.tagName==='INPUT'&&a.value.endsWith('Open File...')&&!!s?.textContent?.includes('Open File...')}"#
        ),
    ),
    (
        "Input.dispatchKeyEvent",
        r#"{"type":"rawKeyDown","modifiers":0,"windowsVirtualKeyCode":13,"nativeVirtualKeyCode":13,"key":"Enter","code":"Enter"}"#,
    ),
    (
        "Input.dispatchKeyEvent",
        r#"{"type":"keyUp","modifiers":0,"windowsVirtualKeyCode":13,"nativeVirtualKeyCode":13,"key":"Enter","code":"Enter"}"#,
    ),
];

type Done = Rc<RefCell<Option<Box<dyn FnOnce(Outcome)>>>>;

fn finish(done: &Done, outcome: Outcome) {
    let done = done.borrow_mut().take();
    if let Some(done) = done {
        done(outcome);
    }
}

/// Run vscode.dev's Open File command once through trusted DevTools input.
/// `may_continue` is checked before every step. `done` is called exactly
/// once, on the UI thread.
pub fn run_open_command(
    webview: ICoreWebView2,
    may_continue: Rc<dyn Fn() -> bool>,
    done: impl FnOnce(Outcome) + 'static,
) {
    let done: Done = Rc::new(RefCell::new(Some(Box::new(done))));
    step(webview, 0, may_continue, done);
}

fn step(webview: ICoreWebView2, index: usize, may_continue: Rc<dyn Fn() -> bool>, done: Done) {
    let Some(&(method, parameters)) = STEPS.get(index) else {
        return finish(&done, Outcome::Done);
    };
    if !may_continue() {
        return finish(&done, Outcome::PageChanged);
    }
    let next = (webview.clone(), Rc::clone(&may_continue), Rc::clone(&done));
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, response| {
            let (webview, may_continue, done) = next;
            let checked = status.map_err(|error| error.to_string()).and_then(|()| {
                if method != "Runtime.evaluate" || evaluated_true(&response) {
                    Ok(())
                } else {
                    Err(match index {
                        2 => "the command palette did not open".to_string(),
                        _ => "the Open File command was not found".to_string(),
                    })
                }
            });
            match checked {
                Ok(()) => step(webview, index + 1, may_continue, done),
                Err(reason) => finish(&done, Outcome::Failed(reason)),
            }
            Ok(())
        }));
    let called = unsafe {
        webview.CallDevToolsProtocolMethod(
            &HSTRING::from(method),
            &HSTRING::from(parameters),
            &handler,
        )
    };
    if let Err(error) = called {
        finish(&done, Outcome::Failed(error.to_string()));
    }
}

fn evaluated_true(response: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(response)
        .is_ok_and(|value| value["result"]["value"] == serde_json::Value::Bool(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_gets_the_origins_as_a_javascript_array() {
        let script = script(&["https://vscode.dev".into()]);
        assert!(script.contains(r#"const trusted = ["https://vscode.dev"];"#));
        assert!(!script.contains("__WEBWRAP_TRUSTED__"));
    }

    #[test]
    fn every_step_is_valid_json_and_only_a_true_check_passes() {
        for (_, parameters) in STEPS {
            serde_json::from_str::<serde_json::Value>(parameters).expect(parameters);
        }
        assert!(evaluated_true(
            r#"{"result":{"type":"boolean","value":true}}"#
        ));
        assert!(!evaluated_true(
            r#"{"result":{"type":"boolean","value":false}}"#
        ));
        assert!(!evaluated_true(
            r#"{"result":{"type":"string","value":"true"}}"#
        ));
        assert!(!evaluated_true("not json"));
    }
}
