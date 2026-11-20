# File Bridge Runtime Plumbing Implementation Plan (sub-project D, phase D1)

Spec: [`docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md`](../specs/2026-11-20-file-bridge-and-adapters-design.md), section D1. Branch `feat/file-bridge-plumbing`.

## Status

Not started.

## Global constraints

- **Where code runs.** Same as C: written and cross-built on Linux (`cargo xwin`, Wine 11 for runtime unit tests). Windows behaviour is decided by `.github/workflows/windows.yml` on x64 and ARM64. The PC only does the hands-on checks queued in `docs/windows-runs/NEXT.md`.
- **No new dependency trees.** `webview2-com =0.38.2` is already in `Cargo.lock` through wry; the runtime now names it directly. `webwrap-bridge` uses only `serde =1.0.229`, `serde_json =1.0.151`, and `webwrap-spec`.
- **`webwrap-bridge` is `#![forbid(unsafe_code)]`** and builds everywhere. Everything that can be decided without Win32 lives there and is tested on Linux.
- **The runtime's handlers only ever veto.** wry's own `NavigationStarting` handler calls `SetCancel(!allow)` and Tauri allows a URL it cannot parse, so it can set `Cancel(false)`. The runtime's handlers are added after wry's (through `with_webview`), run after it, and only ever set `Cancel(true)` or `Handled(true)`.
- **Diagnostics.** Every security decision and bridge refusal prints one `webwrap: <event> <detail>` line to stderr. A release build has no console, so these lines reach nobody unless a parent process captured stderr. The CI end-to-end script does exactly that, which lets it observe native decisions without test hooks in the product.

## Clarifications of the spec

These are decided here; the evidence will list them as deviations.

1. **No mutex.** The named pipe is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, which already fails atomically for every process but the first. So the pipe is both the lock and the endpoint. Both architecture builds still share it, because the name comes from `appId`.
2. **A second launch with no files brings the running window to the front** and exits. It sends an empty `files` list, which the receiver accepts as "activate". Otherwise a no-file launch would open a second window that owns nothing.
3. **The acknowledgement byte is `1` for accepted and `0` for rejected.** So a rejected request is reported at once, not after the 5 s timeout.
4. **Files not yet delivered survive navigation to an untrusted origin.** They were never exposed to any page, and a sign-in redirect would otherwise drop the file the user just opened. They are still delivered only to a trusted document. Every **delivered** token, and every save not yet committed, is revoked as the spec says. A write the trusted page committed before it left still completes: those bytes came from the trusted page.
5. **The frame scheme allowlist also admits `blob:` and `data:`.** Neither can launch an external application, and sites embed such frames. `LaunchingExternalUriScheme` is still always cancelled.
6. **`fileTier` in `--inspect-self` moves to D4**, where the tiers exist.
7. **Files dropped on the window reach the site.** Tauri's drag-and-drop interception is switched off, so a site receives the drop exactly as in a browser, open-only. Today a drop does nothing at all.
8. **A second launch checks who owns the pipe before sending anything.** The pipe's owner is set explicitly to the user, which another user cannot forge. If the owner differs, the launch sends nothing and runs on its own. This closes a pipe pre-created by another user (review finding).
9. **Opened paths are canonical** (`dunce::canonicalize`), so saving a symlinked file replaces the target, not the link, and paths over 260 characters keep the `\\?\` prefix `ReplaceFileW` needs.

## The page protocol

This is the contract D2-D4 build on. Messages go through `chrome.webview`. Every host-to-page message is sent only after checking that the live top-level `Source` is a trusted origin. Every page-to-host message is ignored unless both its own `Source` and the live `Source` are trusted.

| Direction | Message | Meaning |
| --- | --- | --- |
| page → host | `{"type":"webwrap-ready"}` | This document is listening. Deliver every queued file now and later ones as they arrive. |
| host → page | shared buffer, read-only, `{"type":"webwrap-file","token","name","size"}` | One opened file |
| page → host | `{"type":"webwrap-save","token","size"}` | Asks to save `size` bytes to a token's file |
| host → page | shared buffer, read-write, `{"type":"webwrap-save-buffer","token","size","save"}` | Fill this, then commit. `save` identifies this save |
| page → host | `{"type":"webwrap-save-commit","token","save"}` | The buffer is filled. `save` must match, so a late commit can never fill in for a different save of the same file |
| host → page | message `{"type":"webwrap-save-result","token","ok","error"?}` | The outcome. `error` never contains a path; a failed write shows the user a message box naming the file |

Rules enforced by the bridge state machine:

- A token is created only for a path from the command line or the pipe (later, a native picker).
- A document must say `webwrap-ready` before anything is delivered to it. Every new top-level document (`ContentLoading`, before its scripts run) resets this, so nothing is posted into a document that is not listening. Keying this on `NavigationStarting` would be wrong: a navigation that becomes a download, or that another handler cancels, leaves the trusted document in place.
- A save is refused for a token that is unknown, revoked, or not yet delivered; for a size of 0 or over 4 GiB; and while another save of the same token is in flight.
- A save buffer that was never committed is dropped on the next top-level navigation.
- Nothing is posted while an allowed top-level navigation to an untrusted origin is pending. WebView2 holds a navigation until its `NavigationStarting` handlers return, so this closes the gap between reading the live `Source` and the new document existing. The hold ends with that navigation's new document or its `NavigationCompleted`.
- A write the trusted page committed before leaving still completes; the page that follows learns nothing about it.
- Closing the window waits for a save being written, then closes.
- Reading and writing files happens on a worker thread. The UI thread only creates, posts, and closes buffers.

## Task 1: `crates/webwrap-bridge`

A new cross-platform crate holding everything decidable without Win32.

**`ipc`**
- `encode_request(app_id, files) -> Vec<u8>`: a 4-byte little-endian length, then the JSON `{ "v": 1, "appId": "...", "files": [...] }`.
- `read_request(reader, app_id) -> Result<Vec<PathBuf>, IpcError>`. It reads the length and refuses anything over 64 KiB **before** allocating. It then refuses, before touching any path:
  - a `v` other than 1;
  - a different `appId`;
  - unknown fields;
  - more than 16 files;
  - a path that is not absolute or contains NUL.

**`paths`**
- `resolve(arg, cwd) -> Result<PathBuf, String>`: absolute first, then checked.
- `check_file(path)`: absolute, exists, regular file.
- `display_name(path)`.
- Constants `MAX_FILES = 16` and `MAX_FILE_BYTES = 4 GiB`.

**`session`**: the token state machine above, plus `PageMessage` parsing. Page messages over 4 KiB are ignored unparsed. Trust is checked with `webwrap_spec::is_trusted_origin`.

Tests (Linux):
- each IPC rejection;
- a round trip;
- the over-size length refused without reading the body;
- path rules;
- delivery only after `ready`, only to trusted sources, and only for the current document;
- revocation of delivered tokens on an untrusted navigation, with queued files kept;
- a trusted navigation keeps tokens but drops uncommitted saves;
- every save refusal;
- one save in flight per token.

## Task 2: arguments and single instance

**`main.rs`**
- `--inspect-self` as before. Otherwise every argument is a file, so a file named `--x` is a missing file, not a usage error.
- Arguments are read as `OsString`, so a non-Unicode file name is reported as a bad path, not a silent exit 2.
- Files are resolved against the current directory, up to 16. The rest are reported as "too many files at once".

**`single_instance.rs`**
- The pipe name is `\\.\pipe\WebWrap.<appId>.<user SID>`. Its security descriptor is `D:P(A;;GA;;;<SID>)`, and it is created with `PIPE_REJECT_REMOTE_CLIENTS`.
- `become_primary` creates the pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE`. `ERROR_ACCESS_DENIED` or `ERROR_PIPE_BUSY` means another instance owns it.
- `serve`, on its own thread: connect, read a request with `webwrap_bridge::ipc`, write the acknowledgement, hand the paths to the UI thread, disconnect, repeat.
- `forward`, used by a second instance:
  1. Open the pipe, retrying `ERROR_FILE_NOT_FOUND` and `ERROR_PIPE_BUSY` until 5 s have passed.
  2. Call `AllowSetForegroundWindow` for the server's process ID (`GetNamedPipeServerProcessId`).
  3. Send the request and wait up to 5 s for the acknowledgement.
- **A second instance** forwards its good files, then shows one blocking message box for any bad paths or a failed forward. It exits 0 when acknowledged and 1 otherwise.
- **The first instance** shows bad paths in a message box on a separate thread, so the window still opens.
- Received files are re-checked; one that vanished meanwhile is reported the same way. The window is restored and focused.

Tests (Wine and CI): a second `become_primary` for the same app is refused; a different `appId` is not.

## Task 3: hardening handlers (B's four findings)

**`hardening.rs`** is installed through `with_webview` right after the window is built. A failure to install it aborts, because without it the app would run with B's known gaps.

| Handler | Rule |
| --- | --- |
| `NavigationStarting` | `SetCancel(true)` unless `nav_verdict(uri, TopLevel)` is `Allow`. An unparseable URI is therefore blocked |
| `ContentLoading` | Tells the session a new document is loading at the live `Source`. This resets readiness, and an untrusted origin revokes delivered tokens |
| `FrameNavigationStarting` | Cancel unless the scheme is `http`, `https`, `blob`, `data`, or the URL is `about:blank`/`about:srcdoc` |
| `LaunchingExternalUriScheme` | Always cancel |
| `NewWindowRequested` | wry already sets `Handled(true)` (deny) when Tauri has no pop-up hook, so `.on_new_window` is removed. The runtime opens the URL with `ShellExecuteW` only when `IsUserInitiated` **and** the verdict is `OpenExternally` |

Also:
- `.disable_drag_drop_handler()` (clarification 7);
- the scheme and frame rules are pure functions in `webwrap-bridge`, tested on Linux.

## Task 4: delivery and save

**`bridge.rs`**
- A `WebMessageReceived` handler feeds `PageMessage`s to the session.
- Shared buffers: `ICoreWebView2Environment12::CreateSharedBuffer` and `ICoreWebView2_17::PostSharedBufferToScript`. The UI thread keeps the COM buffers in a thread-local map keyed by token. Workers receive only the pointer and the length.
- **Open.**
  1. Create a buffer of the file's size; refuse a file over 4 GiB.
  2. A worker reads the file into it. Reading must hit end-of-file exactly at `size`, so a file that grew is refused rather than delivered truncated.
  3. Back on the UI thread, the session decides whether to post it (same document, trusted). Otherwise the file goes back to the queue.
  4. After posting, the host drops its reference.
- **Save.**
  1. Create a read-write buffer and post it.
  2. On commit, a worker calls `save::replace(path, bytes)`.
  3. The UI thread posts the result, closes the buffer, and shows a message box on failure.
- **`save.rs`.** `replace(path, bytes)`:
  1. Write and `fsync` a same-folder temporary file.
  2. Call `ReplaceFileW(path, temp, backup, REPLACEFILE_IGNORE_MERGE_ERRORS)` with a same-folder backup name. Without a backup, `ERROR_UNABLE_TO_MOVE_REPLACEMENT` leaves no file at the original name.
  3. On success, delete the backup.
  4. On `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`, rename the backup back.
  5. Always remove the temporary file.
  6. A missing original is an error; it is never recreated.

Tests (Wine and CI):
- the content is replaced and the creation time kept;
- a read-only original is untouched, with no stray files;
- a missing original fails and nothing is created.

## Task 5: end-to-end in CI

- **`e2e/cdp.mjs`.** A minimal DevTools-protocol client over Node 22's global `WebSocket`. It finds the page target from `/json/list`, then provides `send`, `evaluate`, and an event wait.
- **`e2e/bridge.mjs`** serves one test page over HTTPS on ports 8443 and 8444. The PFX is made by the workflow with `New-SelfSignedCertificate`, and the apps run with `--ignore-certificate-errors` in `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, test only. It launches the generated apps `bridge-a` (`https://localhost:8443/`) and `bridge-b` (`https://localhost:8444/`), each with its own `--remote-debugging-port`, and captures stderr. Cases:
  1. **Startup file:** A launched with a file delivers it after `ready`, with the bytes intact.
  2. **Gate 6:** a second launch of A with a file exits 0, and the running A receives it.
  3. **Gate 7:** with A and B running, B's file reaches only B.
  4. **Large file:** a 160 MB file opens with a matching SHA-256, saves new content, and the file on disk matches.
  5. **Save failure:** a read-only target gives `ok:false`, and the file is unchanged.
  6. **Pipe:** an oversized length, malformed JSON, and a wrong `appId` are each rejected (ack `0` or close), and a normal launch still works afterwards.
  7. **Gate 8:**
     - A navigates to `https://127.0.0.1:8443/`, an untrusted origin. There, `ready` delivers nothing, a save of the old token is refused (stderr), and a newly opened file is not delivered.
     - Back on `https://localhost:8443/`, the old token's save is refused, and `ready` delivers the queued file.
  8. **Pop-ups:** a script `window.open` is denied (stderr), and a clicked `target=_blank` link opens externally (stderr).
  9. **Schemes:** an iframe `search-ms:` is blocked (stderr), and a top-level `search-ms:` navigation is cancelled with the page unchanged.
- **Workflow:** add `webwrap-bridge` to the test and clippy steps. After the acceptance build: `actions/setup-node` 22, the PFX, generating the bridge apps with the CI-built generator, then `node e2e/bridge.mjs`.

## Task 6: evidence, review, handover

- `docs/evidence/2026-11-20-file-bridge-plumbing.md`.
- An independent review of the branch diff, with the findings fixed or recorded.
- Update `HANDOVER.md` and the spec's status.
- Queue in `docs/windows-runs/NEXT.md`:
  - dragging a PSD onto the open Photopea window opens it;
  - launching the app again by dragging a file onto the exe brings the running window to the front.
