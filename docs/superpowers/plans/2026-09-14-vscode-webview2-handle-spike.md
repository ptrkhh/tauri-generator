# VS Code WebView2 Writable-Handle Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove on Windows that a Tauri/WebView2 wrapper can automatically open an OS-supplied local Markdown file in vscode.dev and that Ctrl+S changes the original file.

**Architecture:** A minimal Windows-only Tauri executable validates one local path, injects a narrow showOpenFilePicker adapter into the top-level https://vscode.dev document, creates a genuine read/write FileSystemFileHandle through WebView2 COM, and delivers it as an additional web-message object. A trusted DevTools Protocol Ctrl+O event invokes VS Code's normal browser file-open path. A local probe page isolates WebView2 handle transfer from hosted VS Code behavior.

**Tech Stack:** Rust 2021, Tauri 2.11.5, WebView2 COM 0.38.2, plain JavaScript/HTML, Windows PowerShell.

**Spec:** [Tauri Web App Generator Design](../specs/2026-09-14-tauri-web-app-generator-design.md), especially “vscode.dev Windows adapter,” “Security boundaries,” and delivery-sequence item 1.

## Global Constraints

- Run build and GUI checks in native Windows with the MSVC Rust toolchain. WSL is not a Windows GUI build environment.
- Do not add a frontend package manager, framework, plugin, Tauri command, or capability file.
- The remote page receives only the explicitly opened file handle. It never receives a native path or general Tauri IPC API.
- Recheck the live top-level origin in Rust immediately before handle delivery and before synthetic input.
- Manual Ctrl+O does not pass the spike. The supplied file must open automatically.
- Opening a file does not pass the spike. Ctrl+S must change the original file bytes without creating a download.
- Commit Cargo.lock; the COM surface is version-sensitive.
- If automatic open or original-file write-back fails, record a failed spike and stop. Do not substitute Monaco, upload bytes, or download-and-replace behavior.

## Scope

This plan covers only the release-blocking vscode.dev feasibility spike. It excludes Windows file-association registration, single-instance routing, PE resources, Photopea, Android, custom DNS, and generator UI work. Those receive separate plans only after this spike passes.

## File Map

- Create: .gitignore — ignore Rust build output.
- Create: Cargo.toml — workspace containing only the spike.
- Create: rust-toolchain.toml — stable Rust plus formatting and lint components.
- Create: spikes/vscode-webview2-handle/Cargo.toml — pinned spike dependencies.
- Create: spikes/vscode-webview2-handle/build.rs — Tauri build hook.
- Create: spikes/vscode-webview2-handle/tauri.conf.json — no default window, bundling, commands, or capabilities.
- Create: spikes/vscode-webview2-handle/icons/icon.png — static source icon required by Tauri's context macro.
- Create: spikes/vscode-webview2-handle/src/main.rs — path validation and window creation.
- Create: spikes/vscode-webview2-handle/src/webview2_bridge.rs — origin checks, COM handle delivery, and Ctrl+O dispatch.
- Create: spikes/vscode-webview2-handle/src/vscode.js — top-frame picker adapter and readiness handshake.
- Create/modify: spikes/vscode-webview2-handle/ui/probe.html — local write-through probe.
- Create after measurement: docs/spikes/2026-09-14-vscode-webview2-handle.md — pass/fail evidence.

---

### Task 1: Scaffold the smallest Windows-only spike

**Files:**

- Create: .gitignore
- Create: Cargo.toml
- Create: rust-toolchain.toml
- Create: spikes/vscode-webview2-handle/Cargo.toml
- Create: spikes/vscode-webview2-handle/build.rs
- Create: spikes/vscode-webview2-handle/tauri.conf.json
- Create: spikes/vscode-webview2-handle/icons/icon.png
- Create: spikes/vscode-webview2-handle/src/main.rs
- Create: spikes/vscode-webview2-handle/ui/probe.html

- [ ] **Step 1: Add workspace files**

~~~gitignore
/target/
~~~

~~~toml
# Cargo.toml
[workspace]
members = ["spikes/vscode-webview2-handle"]
resolver = "2"
~~~

~~~toml
# rust-toolchain.toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
~~~

- [ ] **Step 2: Add the crate and Tauri configuration**

~~~toml
# spikes/vscode-webview2-handle/Cargo.toml
[package]
name = "vscode-webview2-handle-spike"
version = "0.1.0"
edition = "2021"
build = "build.rs"

[build-dependencies]
tauri-build = "=2.6.3"

[dependencies]
tauri = "=2.11.5"

[target.'cfg(windows)'.dependencies]
webview2-com = "=0.38.2"
windows = "0.61"
~~~

~~~rust
// spikes/vscode-webview2-handle/build.rs
fn main() {
    tauri_build::build();
}
~~~

~~~json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "VS Code Handle Spike",
  "version": "0.1.0",
  "identifier": "dev.webwrap.vscode-handle-spike",
  "build": { "frontendDist": "ui" },
  "app": { "withGlobalTauri": false, "windows": [] },
  "bundle": { "active": false }
}
~~~

Create ui/probe.html as a minimal asset so Tauri's frontendDist exists while the crate is first compiled:

~~~html
<!doctype html><meta charset="utf-8"><title>Spike</title>
~~~

Add a small valid static PNG at icons/icon.png. It is only the development spike icon; the production generator later replaces embedded icons during packaging.

- [ ] **Step 3: Write failing argument tests**

Add tests in src/main.rs for:

- one path selects Mode::Vscode and returns its canonical path
- --probe plus one path selects Mode::Probe
- a missing file is rejected
- zero, extra, and unknown-flag arguments are rejected

Use only std::env::temp_dir, std::fs::write, and std::fs::remove_file; do not add tempfile.

~~~rust
fn test_file() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "webwrap-vscode-spike-{}-{:?}.md",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, b"before\n").unwrap();
    path
}

#[test]
fn one_path_selects_vscode_mode_and_canonicalizes() {
    let path = test_file();
    let request = parse_request([path.clone().into_os_string()]).unwrap();
    assert_eq!(request.mode, Mode::Vscode);
    assert_eq!(request.path, std::fs::canonicalize(&path).unwrap());
    std::fs::remove_file(path).unwrap();
}
~~~

- [ ] **Step 4: Run the tests and confirm RED**

Run in Windows PowerShell:

~~~powershell
cargo test -p vscode-webview2-handle-spike
~~~

Expected: compilation fails because Mode, LaunchRequest, and parse_request do not exist.

- [ ] **Step 5: Implement only the validated launch request**

~~~rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("the vscode WebView2 spike must be built and run on Windows");

use std::{ffi::OsString, fs, path::PathBuf};
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Probe,
    Vscode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchRequest {
    mode: Mode,
    path: PathBuf,
}

fn parse_request(args: impl IntoIterator<Item = OsString>) -> Result<LaunchRequest, String> {
    let args: Vec<_> = args.into_iter().collect();
    let (mode, path) = match args.as_slice() {
        [path] => (Mode::Vscode, path),
        [flag, path] if flag == "--probe" => (Mode::Probe, path),
        _ => return Err("usage: vscode-webview2-handle-spike [--probe] <file>".into()),
    };
    let path = fs::canonicalize(path)
        .map_err(|_| "the selected path is not an existing file".to_string())?;
    if !path.is_file() {
        return Err("the selected path is not an existing file".into());
    }
    Ok(LaunchRequest { mode, path })
}
~~~

Create the requested window without a bridge yet:

~~~rust
fn run(request: LaunchRequest) {
    tauri::Builder::default()
        .setup(move |app| {
            let url = match request.mode {
                Mode::Probe => WebviewUrl::App("probe.html".into()),
                Mode::Vscode => WebviewUrl::External("https://vscode.dev/".parse().unwrap()),
            };
            WebviewWindowBuilder::new(app, "main", url)
                .title("VS Code Handle Spike")
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run vscode WebView2 spike");
}

fn main() {
    let request = parse_request(std::env::args_os().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    run(request);
}
~~~

- [ ] **Step 6: Confirm GREEN**

~~~powershell
cargo test -p vscode-webview2-handle-spike
cargo fmt --all -- --check
cargo clippy -p vscode-webview2-handle-spike --all-targets -- -D warnings
~~~

Expected: all argument tests pass. Commit the generated Cargo.lock.

- [ ] **Step 7: Commit**

~~~powershell
git add .gitignore Cargo.toml Cargo.lock rust-toolchain.toml spikes/vscode-webview2-handle
git commit -m "spike: scaffold vscode handle test"
~~~

---

### Task 2: Prove genuine writable-handle delivery locally

**Files:**

- Create: spikes/vscode-webview2-handle/src/webview2_bridge.rs
- Create: spikes/vscode-webview2-handle/ui/probe.html
- Modify: spikes/vscode-webview2-handle/src/main.rs

- [ ] **Step 1: Write the origin-policy test before implementation**

~~~rust
#[test]
fn origin_policy_is_exact_and_mode_specific() {
    assert!(source_is_allowed("https://vscode.dev/", Mode::Vscode));
    assert!(source_is_allowed("https://vscode.dev/github/repo", Mode::Vscode));
    assert!(!source_is_allowed("https://vscode.dev.evil.test/", Mode::Vscode));
    assert!(!source_is_allowed("http://vscode.dev/", Mode::Vscode));
    assert!(source_is_allowed("http://tauri.localhost/probe.html", Mode::Probe));
    assert!(!source_is_allowed("http://tauri.localhost/probe.html", Mode::Vscode));
}
~~~

Add mod webview2_bridge; to main.rs. Run the test and confirm RED because source_is_allowed is missing.

- [ ] **Step 2: Implement exact source validation**

~~~rust
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
~~~

Run the test again and confirm GREEN.

- [ ] **Step 3: Add the local probe**

Create ui/probe.html:

~~~html
<!doctype html>
<meta charset="utf-8">
<title>Waiting for handle</title>
<p id="status">Waiting for handle</p>
<script>
  const bridge = window.chrome.webview;
  const status = document.querySelector("#status");
  const announce = () => bridge.postMessage({ type: "webwrap-probe-ready" });
  const timer = setInterval(announce, 250);
  announce();

  bridge.addEventListener("message", async (event) => {
    if (event.data?.type !== "webwrap-file") return;
    const handle = event.additionalObjects?.[0];
    if (!(handle instanceof FileSystemFileHandle)) {
      status.textContent = "FAIL: object is not a FileSystemFileHandle";
      return;
    }
    clearInterval(timer);
    try {
      const writable = await handle.createWritable();
      await writable.write(new TextEncoder().encode("webwrap-probe-ok\n"));
      await writable.close();
      status.textContent = "PASS";
      document.title = "PASS";
      bridge.postMessage({ type: "webwrap-probe-complete" });
    } catch (error) {
      status.textContent = "FAIL: " + error;
      document.title = "FAIL";
    }
  });
</script>
~~~

The 250 ms loop is a condition-based readiness handshake. It stops after delivery.

- [ ] **Step 4: Implement the WebView2 handle primitive**

Use these exact interfaces in webview2_bridge.rs:

~~~rust
use std::{os::windows::ffi::OsStrExt, path::Path};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment14,
    ICoreWebView2ObjectCollection, ICoreWebView2ObjectCollectionView, ICoreWebView2_23,
    COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE,
};
use windows::core::{Interface, IUnknown, PCWSTR};

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
~~~

- [ ] **Step 5: Register the native-only message handler**

Implement install(window, request) with:

- window.with_webview
- platform.environment
- platform.controller().CoreWebView2
- WebMessageReceivedEventHandler::create
- ICoreWebView2::add_WebMessageReceived
- WebMessageAsJson plus webview2_com::pwstr::take_pwstr

Accept only these exact JSON messages:

~~~text
{"type":"webwrap-probe-ready"}
{"type":"webwrap-probe-complete"}
{"type":"webwrap-vscode-ready"}
{"type":"webwrap-vscode-handle-ready"}
~~~

For every accepted message, read sender.Source, convert it with take_pwstr, and call source_is_allowed before acting. Keep handle_sent and open_triggered booleans inside the callback.

In probe mode:

- webwrap-probe-ready posts the handle once.
- webwrap-probe-complete exits through app_handle.exit(0).

Ignore malformed, unknown, wrong-mode, and wrong-origin messages. Log COM failures to stderr and leave the window open showing failure. Never loosen the origin check.

Keep the Task 1 window builder, then call webview2_bridge::install(&window, request.clone()) after build. Task 3 adds the vscode.js initialization script to this builder; do not reference that file before Task 3 creates it.

- [ ] **Step 6: Run the native probe**

~~~powershell
$probe = Join-Path $env:TEMP "webwrap-handle-probe.txt"
[IO.File]::WriteAllText($probe, "before", [Text.UTF8Encoding]::new($false))
cargo run -p vscode-webview2-handle-spike -- --probe $probe
$actual = [IO.File]::ReadAllText($probe)
if ($actual -ne ("webwrap-probe-ok" + [Environment]::NewLine)) { throw "native handle write-through failed: $actual" }
~~~

Expected: the window closes after writing and PowerShell throws no exception. This proves a genuine writable handle reaches the original file independently of vscode.dev.

- [ ] **Step 7: Check and commit**

~~~powershell
cargo test -p vscode-webview2-handle-spike
cargo fmt --all -- --check
cargo clippy -p vscode-webview2-handle-spike --all-targets -- -D warnings
git add spikes/vscode-webview2-handle
git commit -m "spike: prove WebView2 handle transfer"
~~~

---

### Task 3: Open the supplied handle automatically in vscode.dev

**Files:**

- Create: spikes/vscode-webview2-handle/src/vscode.js
- Modify: spikes/vscode-webview2-handle/src/main.rs
- Modify: spikes/vscode-webview2-handle/src/webview2_bridge.rs

- [ ] **Step 1: Add the top-frame picker adapter**

~~~javascript
(() => {
  if (window.top !== window || location.origin !== "https://vscode.dev") return;
  const bridge = window.chrome?.webview;
  if (!bridge) return;

  const browserPicker = window.showOpenFilePicker?.bind(window);
  let pendingHandle;
  let readyTimer;

  window.showOpenFilePicker = async (...args) => {
    if (pendingHandle) {
      const handle = pendingHandle;
      pendingHandle = undefined;
      return [handle];
    }
    if (browserPicker) return browserPicker(...args);
    throw new DOMException("No file was supplied", "NotFoundError");
  };

  bridge.addEventListener("message", (event) => {
    if (event.data?.type !== "webwrap-file") return;
    const handle = event.additionalObjects?.[0];
    if (!(handle instanceof FileSystemFileHandle)) return;
    pendingHandle = handle;
    clearInterval(readyTimer);
    bridge.postMessage({ type: "webwrap-vscode-handle-ready" });
  });

  const announceWhenWorkbenchExists = () => {
    if (!document.querySelector(".monaco-workbench")) {
      requestAnimationFrame(announceWhenWorkbenchExists);
      return;
    }
    const announce = () => bridge.postMessage({ type: "webwrap-vscode-ready" });
    readyTimer = setInterval(announce, 250);
    announce();
  };
  requestAnimationFrame(announceWhenWorkbenchExists);
})();
~~~

This queues one native handle and otherwise delegates to the browser picker.

- [ ] **Step 2: Confirm RED before trusted input exists**

~~~powershell
$proof = Join-Path $env:TEMP "webwrap-vscode-proof.md"
[IO.File]::WriteAllText($proof, ("before" + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
cargo run -p vscode-webview2-handle-spike -- $proof
~~~

Expected: vscode.dev loads and queues a handle, but the file does not open automatically. Close the window.

- [ ] **Step 3: Dispatch trusted Ctrl+O through WebView2**

Add:

~~~rust
use webview2_com::callback::CallDevToolsProtocolMethodCompletedHandler;

unsafe fn dispatch_ctrl_o(webview: &ICoreWebView2) -> windows::core::Result<()> {
    let down = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|status, _| status));
    webview.CallDevToolsProtocolMethod(
        windows::core::w!("Input.dispatchKeyEvent"),
        windows::core::w!(
            r#"{"type":"rawKeyDown","modifiers":2,"windowsVirtualKeyCode":79,"nativeVirtualKeyCode":79,"key":"o","code":"KeyO"}"#
        ),
        &down,
    )?;
    let up = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|status, _| status));
    webview.CallDevToolsProtocolMethod(
        windows::core::w!("Input.dispatchKeyEvent"),
        windows::core::w!(
            r#"{"type":"keyUp","modifiers":2,"windowsVirtualKeyCode":79,"nativeVirtualKeyCode":79,"key":"o","code":"KeyO"}"#
        ),
        &up,
    )
}
~~~

Handle webwrap-vscode-handle-ready only when:

- mode is Mode::Vscode
- handle_sent is true
- open_triggered is false
- a fresh sender.Source check still passes

Set open_triggered only after dispatch_ctrl_o returns Ok.

- [ ] **Step 4: Confirm automatic open**

Run the same PowerShell command. Do not press Ctrl+O or click Open File.

Expected: a tab named webwrap-vscode-proof.md opens automatically and contains before. A browser picker, unopened tab, or ignored event fails the spike. Do not add DOM selectors or fixed timing delays.

- [ ] **Step 5: Check and commit**

~~~powershell
cargo test -p vscode-webview2-handle-spike
cargo fmt --all -- --check
cargo clippy -p vscode-webview2-handle-spike --all-targets -- -D warnings
git add spikes/vscode-webview2-handle
git commit -m "spike: open local file in vscode.dev"
~~~

---

### Task 4: Verify Ctrl+S write-back and record the gate

**Files:**

- Create: docs/spikes/2026-09-14-vscode-webview2-handle.md

- [ ] **Step 1: Prepare exact test bytes**

~~~powershell
$proof = Join-Path $env:TEMP "webwrap-vscode-proof.md"
[IO.File]::WriteAllText($proof, ("before" + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
$beforeHash = (Get-FileHash -Algorithm SHA256 $proof).Hash
cargo run -p vscode-webview2-handle-spike -- $proof
~~~

- [ ] **Step 2: Perform the visible acceptance test**

Without pressing Ctrl+O or choosing a file:

1. Confirm webwrap-vscode-proof.md opens automatically and shows before.
2. Replace before with after.
3. Press Ctrl+S once.
4. Wait until VS Code's dirty indicator disappears.
5. Close the wrapper so PowerShell continues.

- [ ] **Step 3: Verify original bytes**

~~~powershell
$afterHash = (Get-FileHash -Algorithm SHA256 $proof).Hash
$actual = [IO.File]::ReadAllText($proof)
if ($afterHash -eq $beforeHash) { throw "Ctrl+S did not change the original file" }
if ($actual -ne ("after" + [Environment]::NewLine)) { throw "unexpected original-file bytes: $actual" }
~~~

Expected: neither assertion throws. Confirm no replacement Markdown file appeared in Downloads.

- [ ] **Step 4: Test fail-closed origin handling**

Navigate the top-level WebView away from https://vscode.dev using DevTools, then emit every recognized bridge message from that document's console.

Expected: the fresh Rust source check rejects every message; no handle is posted and no Ctrl+O event is dispatched.

- [ ] **Step 5: Record measured evidence**

Create docs/spikes/2026-09-14-vscode-webview2-handle.md containing:

- exact git commit tested
- rustc -Vv output
- installed WebView2 runtime version
- Windows version and architecture
- commands run
- PASS or FAIL for native probe, automatic open, Ctrl+S original-byte write-back, and wrong-origin denial
- observed file hashes before and after
- console or COM errors
- one conclusion: gate satisfied, or product work blocked pending redesign

Collect environment values with:

~~~powershell
git rev-parse HEAD
rustc -Vv
[Environment]::OSVersion.VersionString
$env:PROCESSOR_ARCHITECTURE
$programFilesX86 = [Environment]::GetFolderPath("ProgramFilesX86")
Get-ChildItem "$programFilesX86\Microsoft\EdgeWebView\Application" -Directory |
  Sort-Object { [version]$_.Name } -Descending |
  Select-Object -First 1 -ExpandProperty Name
~~~

Record actual observations; never infer PASS from code inspection.

- [ ] **Step 6: Final verification**

~~~powershell
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
git status --short
~~~

Expected: tests, formatting, clippy, and diff checks pass. Before commit, status lists only the result document and intentional spike fixes from the acceptance run.

- [ ] **Step 7: Commit the measured result**

Passing run:

~~~powershell
git add spikes/vscode-webview2-handle docs/spikes/2026-09-14-vscode-webview2-handle.md Cargo.lock
git commit -m "test: prove vscode original-file saves"
~~~

Failing run:

~~~powershell
git add spikes/vscode-webview2-handle docs/spikes/2026-09-14-vscode-webview2-handle.md Cargo.lock
git commit -m "docs: record vscode spike failure"
~~~

Do not begin Photopea or generator plans unless every gate above passes.
