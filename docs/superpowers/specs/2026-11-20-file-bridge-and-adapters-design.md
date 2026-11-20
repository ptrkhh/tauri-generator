# File Bridge and Adapters Design (sub-project D)

Date: 2026-11-20. Status: decisions agreed with the user; awaiting their review before the first phase plan is written.

## Summary

D lets a generated app work with local files.

- **Opening:** double-click a file, or Ctrl+O, and it opens in the site.
- **Saving:** Ctrl+S writes back to the original file wherever that can be verified.
- **Second file:** a file opened while the app runs reaches the running window.

D delivers this for Photopea and `vscode.dev`, which are tested adapters with verified write-back. It also delivers a generic approach for every other site, labelled honestly: write-back where the site supports it, open-only otherwise.

It ports the two WebView2 spikes into `crates/webwrap-runtime` and deletes them. It also closes the four review findings B handed over.

D is too large for one plan. It ships in four phases (D1-D4). Each has its own plan, evidence, and merge, and each leaves `main` working.

## Decisions

| Question | Decision | Source |
| --- | --- | --- |
| Which sites get file support | Photopea and `vscode.dev` as tested adapters, **and** a generic approach for all other sites now | User, 2026-11-20 |
| File size | Large files (100 MB+) must work. No small transport cap: bytes move through WebView2 shared memory, not text encoding | User, 2026-11-20 |
| Ctrl+O write-back | Ctrl+O then Ctrl+S saves back to the original in **both** adapters, exactly like a double-clicked file | User, 2026-11-20 |
| Second file while running | Opens in the running window as a new document or tab, and the window comes to the front | This design; matches both sites' own behaviour |
| Downloads replacing the opened file | Out of D. The base design allows it only after explicit confirmation; it is a later, separate feature | This design, to keep D bounded |
| How save writes | `ReplaceFileW` from a same-folder temporary file. It keeps the original's creation time, ACLs, attributes, and alternate streams, which a plain rename loses. The original is untouched on any failure | This design |
| Save feedback | A failed save always shows a message box naming the file and the reason. A successful save is silent, like any editor | Base design ("report it prominently") |

## Phases

| Phase | Content | Gates |
| --- | --- | --- |
| **D1** Runtime file plumbing | File arguments, single instance and IPC, the file-token model, origin-gated delivery, shared-buffer transport, atomic save, B's four findings | 6, 7, 8 |
| **D2** `vscode.dev` adapter | Port the handle spike; double-click and Ctrl+O both write back | Save half of 5 |
| **D3** Photopea adapter | Port the shell-page spike; large-file transport; Ctrl+O through Photopea's custom I/O | Save half of 4 |
| **D4** Generic sites | `launchQueue` bridge, native-picker passthrough, upload-box and drop fallback, an unsupported message, and tier labels | — |

The double-click halves of gates 4 and 5 need registration, which is sub-project E.

## D1: runtime file plumbing

### Arguments

The runtime accepts:

```
app.exe [<file>...]
app.exe --inspect-self [--check-url <url>]...
```

- It takes up to 16 file paths.
- Each must exist and be a regular file. It is canonicalized and turned into a file token (below).
- A bad path shows a message box naming it. The app still opens without it.
- `--inspect-self` gains `fileTier`: the tier the app would use, `adapter`, `pwa`, `picker`, `upload`, or `unsupported`. Tests assert it.

### Single instance and IPC (gates 6 and 7)

Every generated executable shares one compiled runtime. So uniqueness comes from the embedded identity, never from Tauri's bundle identifier.

- **Lock:** the named mutex `Local\WebWrap.<appId>`. Both architecture builds of one app share the `appId`, so they share the lock (C's open item).
- **Endpoint:** the named pipe `\\.\pipe\WebWrap.<appId>.<user SID>`. Its security descriptor admits only the current user, and it is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`.
- **Message:** a 4-byte little-endian length, then JSON `{ "v": 1, "appId": "...", "files": ["..."] }`, at most 64 KiB.
- **What the receiver rejects, before touching any path:**
  - a wrong `v`;
  - a wrong `appId`;
  - an oversized or malformed payload;
  - zero files, or more than 16.
  
  Only then does it canonicalize and validate each path, as for arguments.
- **The second instance** calls `AllowSetForegroundWindow` for the first, sends, waits for a one-byte acknowledgement, and exits 0. If no acknowledgement arrives within 5 s, it says so in a message box and exits 1.
- **Unrelated apps** use different `appId`s, so they never see each other's files (gate 7).

The message codec and its validation live in a new cross-platform crate, `crates/webwrap-bridge` (`forbid(unsafe_code)`), so they are tested on Linux. The runtime keeps only the Win32 calls.

### File tokens and origin gating (gate 8)

- The native side keeps a table of tokens to canonical paths. A path enters it only from:
  - an OS open (arguments or IPC), or
  - a native picker the user drove.
- The page only ever sees opaque tokens and the file's display name. It can never submit a path.
- Before delivering a handle or bytes, the runtime checks two things against the adapter's exact origin: the top-level `Source`, and for Photopea the shell's frame. This is checked immediately before delivery, as in both spikes.
- A top-level navigation to any other origin cancels pending deliveries and revokes every token, so a later save message is refused (gate 8).

### Shared-buffer transport

This is the large-file path.

- **To the page:** `CreateSharedBuffer(size)` plus `PostSharedBufferToScript`, read-only, with JSON `{token, name, size}`. The page releases the buffer when it is done.
- **From the page (save):**
  1. The page asks to save a token with a byte size.
  2. The runtime refuses a revoked token, a size of 0, or more than 4 GiB.
  3. Otherwise it creates a read-write buffer and posts it.
  4. The page fills the buffer and commits.
  5. The runtime writes the bytes to a temporary file in the destination's folder, `fsync`s it, then calls `ReplaceFileW`.
- One save per token is in flight at a time. A second request while one is in flight is refused.

### Hardening carried from B (B evidence, section 6)

The runtime installs its own WebView2 handlers through `with_webview`. They act as extra vetoes: a cancel from any handler wins.

1. **`NewWindowRequested`.**
   - Handled entirely by the runtime, which replaces the Tauri `on_new_window` wiring.
   - A pop-up opens in the system browser only when it is `IsUserInitiated` **and** its verdict is `OpenExternally`.
   - Everything else is denied.
2. **`NavigationStarting`.** Cancelled unless `nav_verdict` is `Allow`. A URL that fails to parse is therefore blocked, which closes wry's fail-open.
3. **Frames and external schemes.**
   - `FrameNavigationStarting` cancels anything except `http`, `https`, `about:blank`, and `about:srcdoc`.
   - `LaunchingExternalUriScheme` is always cancelled, so `search-ms:` and similar never reach the OS prompt.
4. **A dropped or positional file** is now an open request, not a silent exit 2.

## D2: `vscode.dev` adapter

This ports `spikes/vscode-webview2-handle`.

- **Double-click.**
  1. The runtime creates a read-write `FileSystemFileHandle` (`CreateWebFileSystemFileHandle`) for the token's path.
  2. It posts the handle with `PostWebMessageAsJsonWithAdditionalObjects`.
  3. The initialization script holds the handle for the next `showOpenFilePicker` call.
  4. The runtime drives vscode.dev's own Open File command with trusted DevTools-protocol key events, as the spike does.
- **Save.** Ctrl+S uses vscode.dev's normal `createWritable()` on that handle, so the original file changes. No shared buffer is involved.
- **Ctrl+O.** vscode.dev's own `showOpenFilePicker` reaches WebView2's native picker, which returns genuine writable handles. D2 verifies this writes back; nothing is injected.
- **Fragility.** The command dispatch depends on hosted vscode.dev. A weekly scheduled CI run exercises it against the live site, so an upstream change is caught by a red run, not by the user.

## D3: Photopea adapter

This ports `spikes/photopea-windows-writeback`.

- **Shell page.**
  - The app loads a local shell page (`http://tauri.localhost`, from the runtime's assets) that embeds `https://www.photopea.com` in an iframe and speaks Photopea's Live Messaging API.
  - The shell, not Photopea, is the top-level document, so the navigation policy treats the shell origin as trusted for this adapter. The iframe origin must equal the spec's `trustedOrigins` entry.
- **Open.** The file's bytes arrive in the shell through a shared buffer. The shell transfers the `ArrayBuffer` into the iframe, and the document's Photopea source is bound to the token, as in the spike.
- **Save.**
  1. Photopea's custom-I/O `save` hook makes Photopea export PSD bytes to the shell.
  2. The shell copies them into a save buffer (see the transport above).
  3. The runtime checks the `8BPS` signature and writes back with `ReplaceFileW`.
- **Ctrl+O.**
  1. Photopea's custom-I/O `open` hook tells the shell, which the spike already does.
  2. The runtime shows the native Windows open dialog, filtered to the app's associations plus "All files".
  3. The chosen file becomes a token, and it opens and saves exactly like a double-clicked one.
- **Large files.** The 32 MiB spike cap is gone. The limit is the transport's 4 GiB. D3's evidence must include a real PSD of at least 150 MB opening and saving correctly.

## D4: generic sites

For `adapter: none`, a file request goes through tiers in order. The tier that handled it is what the user is told.

1. **PWA `launchQueue` (best-effort write-back).**
   - On trusted origins, the initialization script installs `window.launchQueue` before site scripts run.
   - If the site calls `setConsumer` within 10 s of load, it receives `LaunchParams` with read-write `FileSystemFileHandle`s.
   - Saving works if the site writes through the handle.
2. **Native picker (best-effort write-back).** A site that opens files with `showOpenFilePicker` already gets WebView2's native picker and writable handles for Ctrl+O. D4 only verifies this; nothing is injected.
3. **Upload box or drop (open-only).** Otherwise the runtime fills the page's single visible `<input type=file>` with a `File` built from a shared buffer and fires `change`. Failing that, it dispatches a synthetic drop on the page.
4. **Unsupported.** Otherwise a message says the site did not accept the file. Nothing is claimed.

**Labels.** "Verified write-back" is reserved for the D2 and D3 adapters. Tiers 1 and 2 are "best effort", tier 3 is "open only", and tier 4 is "unsupported".

The first time an app uses tier 3 or 4, the runtime shows a one-time notice explaining that saving creates a download. It remembers this in its data folder. F shows the same label at generation time.

## Crates and code moved

| Where | What |
| --- | --- |
| `crates/webwrap-bridge` (new, cross-platform) | IPC message codec and validation; argument and path rules; the token table; save-request validation; tier selection logic |
| `crates/webwrap-runtime/src/` | Win32 and WebView2 code: mutex, pipe, handlers, shared buffers, `CreateWebFileSystemFileHandle`, `ReplaceFileW`, native picker, adapters |
| `crates/webwrap-runtime/ui/` | The Photopea shell page (from the spike's `ui/`) |
| Deleted | `spikes/vscode-webview2-handle` with D2, `spikes/photopea-windows-writeback` with D3 |

## Verification

- **Linux:** `webwrap-bridge` unit tests. They cover:
  - IPC rejections: version, `appId`, size, count, malformed input;
  - token revocation;
  - save-size limits;
  - argument validation;
  - tier selection.
- **Windows CI, automated end to end.**
  - Generated apps run with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`.
  - They are driven by a dependency-free Node 22 script, using the built-in `WebSocket` to speak the DevTools protocol.
  - The script drives only the page, never the native side under test.

  Each phase adds its cases:
  - **D1:**
    - a second launch reaches the first instance (gate 6);
    - Photopea and Twitter running together each receive only their own files (gate 7);
    - after navigating to an untrusted origin, a delivery and a save are refused (gate 8);
    - a script-initiated pop-up is denied while a user-initiated one opens externally;
    - an iframe `search-ms:` never prompts;
    - a malformed pipe message is rejected.
  - **D2:** open, Ctrl+S, and a byte-level change, against live `vscode.dev`.
  - **D3:** open and save a 150 MB PSD against live Photopea; byte-level change, and the original preserved on a forced failure.
  - **D4:** a local test site per tier (served by the test), checking the delivered tier and, for tier 1, a write-back.
- **Weekly scheduled CI run** of the D2 and D3 live-site tests, to catch upstream changes.
- **Hands-on PC run (`docs/windows-runs/`)** for what automation cannot drive:
  - Ctrl+O through the native Windows dialog in both adapters, then Ctrl+S;
  - a real large PSD;
  - the save-failure and tier notices as they appear on screen.

## Release gates

- **D1:** 6 (a second file reaches the running instance), 7 (concurrent apps, files reach only the matching app), and 8 (no file access after navigating to an untrusted origin).
- **D2 and D3:** the save halves of 5 and 4. Each gate's "appears in Open with" and double-click half stays with E.
- **Not proven by D:** 1, 2, 9, 10, 12, 13.

## Open items for later sub-projects

- **E.**
  - Register with `"%1"` (one process per file) or a multi-file form. D1 accepts up to 16 files either way.
  - Decide whether registering one architecture build covers both.
- **F.**
  - Show the tier label at generation time. Read the manifest's `file_handlers` to propose associations.
  - Offer the Photopea and `vscode.dev` presets.
- **Later feature:** replacing the opened file from a site's download, only after explicit confirmation.
