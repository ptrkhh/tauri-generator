# Photopea Windows Open/Save Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove on Windows that a Tauri/WebView2 wrapper can automatically open a supplied local PSD in Photopea and that a real Ctrl+S causes replacement PSD bytes to be written to the original file.

**Architecture:** Build a self-contained, non-bundled Tauri spike under spikes/photopea-windows-writeback. The Tauri window loads a local Outer Environment (OE) HTML page at http://tauri.localhost, and that page embeds https://www.photopea.com in a full-window iframe. The native WebView2 bridge creates a read/write FileSystemFileHandle for the explicitly supplied path and posts it to the OE. The OE reads the bytes with handle.getFile().arrayBuffer(), sends them to Photopea using the documented Live Messaging API, sets app.activeDocument.source to a per-open source ID, and translates Photopea's custom-I/O save callback into a bounded base64 JSON message. Rust validates the message and PSD signature, then atomically replaces the original path with a same-directory temporary file and MoveFileExW.

**Tech Stack:** Rust 2021, Tauri 2.11.5, WebView2 COM 0.38.2, windows 0.61, serde_json 1.0.151, base64 0.22.1, plain HTML/JavaScript, Windows PowerShell, Photopea Live Messaging/custom I/O.

**Spec:** docs/superpowers/specs/2026-09-14-tauri-web-app-generator-design.md (Photopea adapter and delivery sequence).

## Global constraints

- This is a release-blocking Windows spike, not generator UI, packaging, Android, DNS, or file-association work.
- Accept exactly one existing file path. Canonicalize it before any WebView2 handle is created; reject missing paths, directories, and extra arguments.
- The native bridge may deliver a handle only to the local OE origin http://tauri.localhost and must re-read the live WebView2 source before every delivery/write action. The remote Photopea iframe never receives a native path.
- The OE must accept Photopea messages only from the iframe window and exact origin https://www.photopea.com; the native bridge must accept only the exact local-origin JSON protocol.
- Keep a 32 MiB decoded PSD cap for this spike. Reject malformed JSON, invalid base64, non-PSD bytes, and over-cap payloads without touching the original.
- Save through a same-directory temporary file, flush it, and replace the original with MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH). Any failure leaves the old file in place and is surfaced in the window title and stderr.
- Do not use a server save endpoint, data URL, browser picker, or silent fallback to downloads/system DNS.
- Every non-trivial parser or write path has a focused unit test. Native/Photopea acceptance is run on the Windows x64 toolchain only.

## Task 1: Scaffold the spike and the local Outer Environment

Files: spikes/photopea-windows-writeback/Cargo.toml, build.rs, tauri.conf.json, src/main.rs, src/webview2_bridge.rs, ui/photopea.html, ui/photopea.js, icons/icon.png (or the existing minimal icon-generation pattern).

- [ ] RED: Add src/main.rs tests for parse_request: one existing path selects Photopea, a missing path is rejected, directories are rejected, and zero/multiple/unknown arguments are rejected. Assert the stored path is canonical.
- [ ] GREEN: Copy the smallest working Tauri/WebView2 spike manifest from spikes/vscode-webview2-handle, changing the package/product/identifier to Photopea names and adding only direct serde_json and base64 dependencies needed by later tasks. Keep bundling disabled and the frontend distribution at ui.
- [ ] GREEN: Implement LaunchRequest { path: PathBuf }, parse_request, and run. Build one Tauri window titled Photopea Writeback Spike with WebviewUrl::App("photopea.html"); leave the WebView2 bridge module behind mod webview2_bridge and call install from setup.
- [ ] GREEN: Add a compile-only webview2_bridge::install stub returning tauri::Result<()> so the scaffold remains buildable; Task 2 replaces the stub with the native bridge.
- [ ] GREEN: Make ui/photopea.html a full-viewport OE page with one iframe and a small status element. The iframe source must be https://www.photopea.com/# followed by encodeURIComponent(JSON.stringify(config)), where config.environment.customIO.open and .save echo Open and Save, and localsave is false. Do not put the local path in the URL.
- [ ] GREEN: In ui/photopea.js, create the iframe and origin constants, listen for the native WebView2 message event, and listen for Photopea message events only when event.source === frame.contentWindow and event.origin === "https://www.photopea.com". Post one exact JSON-string {"type":"webwrap-photopea-shell-ready"} to the native bridge after the iframe load event; ignore all other native messages until Task 2 adds the handle protocol.
- [ ] VERIFY: Run node --check spikes/photopea-windows-writeback/ui/photopea.js; run the native Windows cargo test --workspace --target-dir D:/git/pwa_ganerator/target/photopea-x64 and cargo fmt --all -- --check from the configured MSVC environment.
- [ ] Commit as test(spike): define photopea request and shell (tests may be committed with their scaffold when they are the first useful change).

## Task 2: Deliver the explicit file handle and prove automatic Photopea open

Files: spikes/photopea-windows-writeback/src/webview2_bridge.rs, ui/photopea.js, and src/main.rs only if the request type needs a shared helper.

- [ ] RED: Add Rust tests for the exact WebView2 JSON-string envelope decoder, local-origin/live-source action checks, and save/open status message classification. Add a JavaScript self-check or deterministic helper assertion for source IDs (webwrap-local: prefix plus a filesystem-safe basename/token); keep the checks outside the network acceptance path.
- [ ] GREEN: Reuse the proven WebView2 code shape from the VS Code spike: ICoreWebView2Environment14::CreateWebFileSystemFileHandle with COREWEBVIEW2_FILE_SYSTEM_HANDLE_PERMISSION_READ_WRITE, an object collection, and ICoreWebView2_23::PostWebMessageAsJsonWithAdditionalObjects. Post the handle only after the exact shell-ready message and only after both event and live Source pass http://tauri.localhost origin validation. Preserve the existing failure-title behavior for missing callback data, failed source reads, and failed event registration.
- [ ] GREEN: Define exact native messages: shell-ready, handle-received, photopea-opened, save-requested, save-complete, and error. Decode WebView2's outer JSON string with serde_json; do not accept raw objects, whitespace variants, unknown types, or messages from another origin. Native state must allow only one handle delivery and one in-flight open/save operation.
- [ ] GREEN: On the OE's webwrap-file additional object, require a FileSystemFileHandle, call getFile()/arrayBuffer(), reject an absent handle or read failure, and retain the bytes only in page memory. Generate a source ID from the canonical file name plus a random/in-process token; retain the (source_id, handle) association in the OE.
- [ ] GREEN: Wait for Photopea's documented "done" readiness message, transfer the bytes with frame.contentWindow.postMessage(arrayBuffer, PHOTOPEA_ORIGIN, [arrayBuffer]), then send the script app.activeDocument.source=<JSON source_id>; app.echoToOE(JSON.stringify({kind:"opened",source:app.activeDocument.source}));. On the resulting message, post the exact native opened status and show PASS: Opened <name> in the page/title. Never use Photopea's server or browser file picker.
- [ ] VERIFY: Build and run on Windows with a valid PSD fixture. Confirm the app opens the supplied PSD without a picker, the Photopea canvas/document is visible, and the observed app.activeDocument.source equals the generated source ID. Capture the native stderr and a CDP/DevTools transcript sufficient to reproduce the readiness and opened messages.
- [ ] Commit as feat(spike): open psd through photopea live messaging.

## Task 3: Route Ctrl+S to PSD export and atomically write the original

Files: spikes/photopea-windows-writeback/src/webview2_bridge.rs, ui/photopea.js, and focused tests in the same modules.

- [ ] RED: Add unit tests for decode_save_message (valid envelope, wrong type, missing source/data, malformed base64, over-cap data, and non-8BPS bytes) and write_psd_atomically (success changes bytes, a failed validation/write leaves the original unchanged). Tests must use unique temp paths and clean up.
- [ ] GREEN: When the OE receives Photopea's exact Save echo, verify a current source ID is associated with the delivered native handle, mark save in flight, and post app.activeDocument.saveToOE("psd"); app.echoToOE(app.activeDocument.source); to the iframe. Treat the next ArrayBuffer as the export and reject a source mismatch or a second concurrent save.
- [ ] GREEN: In the OE, base64-encode only the returned ArrayBuffer, reject/announce payloads over 32 MiB before posting, and send JSON.stringify({type:"webwrap-photopea-save",source:sourceId,data:base64}) to the native bridge. On native success, post save-complete; on failure, post an error and keep the old file. Keep the original native path out of all page status strings and messages.
- [ ] GREEN: Implement write_psd_atomically(path, bytes): verify the decoded bytes begin with 8BPS, write them to a create_new temp file in the original directory, sync_all, close it, then call MoveFileExW with replace/write-through flags. Remove only the uniquely named temp file on an error. Do not delete or truncate the destination before the replacement call.
- [ ] GREEN: Mark the native window PASS: Saved original only after the replacement succeeds and log source ID, byte count, and elapsed time (not the absolute path). Reset save state after completion so a later Ctrl+S can repeat safely.
- [ ] VERIFY: Use the fixture from Task 2, make a visible edit through a Photopea script or UI action, send a real trusted Ctrl+S keydown/up through CDP, wait for save-complete, and compare the pre/post file hashes and 8BPS header. Assert the post-save file is not in Downloads and that a forced bad payload leaves the original hash unchanged. Run native tests, cargo fmt --all -- --check, cargo clippy --workspace --all-targets --target-dir D:/git/pwa_ganerator/target/photopea-x64 -- -D warnings, and node --check.
- [ ] Commit as feat(spike): atomically save photopea psd.

## Task 4: Produce the spike evidence and close the gate

Files: docs/spikes/2026-09-15-photopea-windows-writeback.md, plus minimal corrections to the spike source/tests if the evidence exposes a defect.

- [ ] Record exact commit, Windows/WSL versions, Rust/Tauri/WebView2 versions, Photopea URL/config hash, fixture bytes/hash, command lines, and all pass/fail observations. Include the exact origin/message policy and the 32 MiB ceiling.
- [ ] Include reproducible PowerShell fixture creation for a 43-byte 1x1 RGB PSD from this base64 literal: OEJQUwABAAAAAAAAAAMAAAABAAAAAQAIAAMAAAAAAAAAAAAAAAAAAP8AAA==. The acceptance commands must use a temp copy and print SHA-256 before and after.
- [ ] Document the live transcript: shell-ready -> handle-received -> Photopea done -> ArrayBuffer open -> source assignment/echo -> Ctrl+S Save -> saveToOE("psd") ArrayBuffer -> native save-complete. Explicitly distinguish verified original-file write-back from unsupported areas (generator UX, packaging, Android, associations, DNS, large-file transport).
- [ ] Add a hostile-origin check (navigate the OE or iframe to an untrusted origin) and show that no handle delivery or write occurs; add a malformed/oversize save-message check and show the old hash remains.
- [ ] Run the full native verification command and git diff --check; ensure the worktree contains only the planned spike and evidence. Commit as docs: record photopea windows writeback spike.

## Review checkpoints

After each implementation task, dispatch a fresh reviewer against the task checklist and its commit. The reviewer must inspect the diff, run the narrowest available tests, and report blockers before the next task. After Task 4, run one broad review for security/origin checks, atomic-write failure behavior, protocol exactness, and evidence reproducibility. Keep an SDD ledger at .superpowers/sdd/2026-09-15-photopea-windows-writeback-spike/progress.md with plan-scan rulings, task status, reviewer findings, and verification commands.

## Sources

- Photopea Live Messaging API: https://www.photopea.com/api/live
- Photopea environment/custom I/O API: https://www.photopea.com/api/environment
- Photopea configuration hash format: https://www.photopea.com/api/
- WebView2 host-created filesystem handles: https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2environment.createwebfilesystemfilehandle
