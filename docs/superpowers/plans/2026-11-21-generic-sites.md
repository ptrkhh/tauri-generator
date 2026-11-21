# Generic Sites Implementation Plan (sub-project D, phase D4)

Spec: [`docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md`](../specs/2026-11-20-file-bridge-and-adapters-design.md), section D4. It builds on D1's bridge and page protocol, reuses D2's handle posting and D3's dialog automation, and follows their pattern: a local test site in CI, a live-site step, and a review before merging. Branch `feat/generic-sites`.

## Status

Not started. Written 2026-11-21.

## Goal

An app generated with `adapter: none` (the default) does something sensible with a file it is given, at launch or from a later launch, and says which tier handled it:

1. **`launchQueue`:** the site's consumer gets a read-write handle, and saves through it reach the original file ("best effort").
2. **Native picker:** a site's own `showOpenFilePicker` works with writable handles. D4 only verifies this ("best effort").
3. **Upload box or drop:** the site gets a `File` and opens it, but cannot save back ("open only").
4. **Unsupported:** a message box says the site did not accept the file ("unsupported").

D4 closes no release gate.

## Today's gap, checked

For `Adapter::None` the runtime installs no initialization script (`main.rs`, `Adapter::None => {}`), so no real site ever posts `webwrap-ready`. A file given to such an app (for example `twitter-x64.exe some.png`) is queued and never delivered, and the user is told nothing. D1's CI pages for `bridge-a` and `bridge-b` post `webwrap-ready` themselves, which is why the tests never saw this.

## Global constraints

As D1-D3:

- **No new dependencies.**
- **Every decision is a `webwrap: <event>` line.** The tier is logged as `file-tier <n> <name>`.
- **Origin gating does not change.** The script does nothing outside trusted origins. Every delivery still goes through `Session`.

## Clarifications of the spec

1. **One page script, `src/generic.js`,** templated with the trusted origins like `vscode.js`. On a trusted top-level document it:
   - defines `window.launchQueue` with `setConsumer(fn)` before site scripts run, unless the browser already has one;
   - waits for whichever comes first: `setConsumer` is called, or 10 s after `load` (or 10 s from now if the document has already loaded);
   - then posts `webwrap-ready` once, with `"want":"handle"` if a consumer was set and `"want":"buffer"` otherwise. A document keeps its choice: a consumer set after 10 s is not used.
   - A document whose `load` never fires never becomes ready and its files stay queued. That is the same silence as today, and accepted for D4.
2. **Ready says which transport the page wants.** `PageMessage::Ready` gets an optional `want`. If it is missing, the page wants a buffer, so D1's pages and both adapters are unchanged. `want` is honoured only for `adapter: none`.
3. **Tier 1: handles.**
   - For a handle-wanting document, the runtime posts a handle using D2's `post_handle` and its `webwrap-file-handle` message. `vscode::post_handle` moves to `bridge.rs` or a small shared module, unchanged.
   - The script calls the consumer with a `LaunchParams`-like object `{ files: [handle], targetURL: location.href }`, one file per call, as a browser does for each launch.
   - The script then posts `{"type":"webwrap-file-tier","token","tier":1}`.
   - **Handle limits from D2:** files in AppData, on shares, or with long paths are refused by WebView2. For those, the runtime falls back to a shared buffer, so the file goes to tier 3. The choice is a pure function in `webwrap-bridge`, so it is unit-tested. If creating the handle still fails when it is posted, `adapter: none` falls back to the shared buffer like this, rather than reporting a failure; `vscodeDev` keeps its error.
4. **Tier 3: upload box, then drop.** On `webwrap-file` (a shared buffer), the script copies the bytes into a `File` with the file's name, releases the buffer at once, and then:
   - if the page has exactly one *visible* `<input type=file>` (it has client rects and is not `disabled`), sets its `files` through a `DataTransfer` and fires `input` and `change`, both bubbling;
   - otherwise dispatches `dragenter`, `dragover` and `drop` with that `DataTransfer` on the element at the centre of the viewport (or `document.body`). The drop counts as accepted only if the site called `preventDefault()` on the `drop` itself: a site that handles a dropped file cancels `drop`, while cancelling only `dragover` is what a site does to stop the browser navigating to a dropped file, not to read it. `dragenter` and `dragover` are still dispatched, because some sites set state on them;
   - posts `webwrap-file-tier` with `tier: 3` if either worked.

   A page with several visible upload boxes is not guessed at: it goes to the drop.
5. **Tier 4: unsupported.** If neither worked, the script posts D2's `webwrap-open-failed`, which is now honoured for `adapter: none`. The box says: "<App> did not accept these files: … This website may not support opening files from Windows." It shows every time, because it reports a failure.
6. **Tier 2 is not touched.** WebView2 already shows its native picker for `showOpenFilePicker`. CI checks that a site gets a writable handle that way, using D3's `pickInDialog`, and that nothing from WebView2 or the runtime gets in its way. That helper finds the dialog among the windows of the app's `msedgewebview2.exe` processes and records its title rather than assume it: unlike D3's dialog, which the runtime itself shows in the app's process, WebView2's picker is a window of WebView2's browser process. If the runner cannot drive it, tier 2 stays a PC check (D2's known limit 6) and the step records that. That outcome is evidence, not a failure to hide.
7. **The one-time notice for open-only.** The first `webwrap-file-tier` with tier 3 shows an information box: "<App> opened <name> as an upload. Saving in <App> downloads a copy; it does not change the original file. This notice won't show again." The runtime then writes `open-only-notice` into its data folder (`%LOCALAPPDATA%\WebWrap\<appId>`). If that file exists, the notice is not shown. If the file cannot be written, the notice shows again next time; that is harmless. Tier 4 needs no notice, because its own box already says the file was not opened.
8. **`webwrap-file-tier` is only accepted for a delivered token**, from a trusted source, for `adapter: none`, and with a tier of 1 or 3 that matches how the file was delivered: 1 only after a handle, 3 only after a buffer. Anything else is refused and logged (`file-tier-refused`).
9. **`fileTier` in `--inspect-self`** is `"verified"` for `vscodeDev` and `photopea`, and `"generic"` for `none`. It describes the app, not a file, so F can show it at generation time. The per-file labels are what the runtime logs. (This replaces the spec's D1 sentence, where `fileTier` named the tier the app would use. Which tier took a file is per-file, and only the page can report it.)

## Page protocol additions

| Direction | Message | Meaning |
| --- | --- | --- |
| page → host | `{"type":"webwrap-ready","want":"handle"\|"buffer"}` | `want` is new and optional (default `buffer`). It is honoured only for `adapter: none`. |
| host → page | `webwrap-file-handle`, from D2 | Now also for `adapter: none`, when the page wants handles and the path allows one. |
| page → host | `{"type":"webwrap-file-tier","token","tier":1\|3}` | Which tier took a delivered file. `adapter: none` only. |
| page → host | `webwrap-open-failed`, from D2 | Now also for `adapter: none`: tier 4. |

## Task 1: `webwrap-bridge`

- `PageMessage::Ready { want: Option<Want> }` with `Want::{Handle, Buffer}`. `PageMessage::FileTier { token, tier }`, where serde accepts only 1 or 3.
- `Session` remembers how each token was delivered (handle or buffer) and gets `file_tier(source, live, token, tier) -> Result<&str, Refusal>`, which accepts tier 1 only after a handle and tier 3 only after a buffer (clarification 8). It returns the file's name.
- `Tier::{Verified, LaunchQueue, NativePicker, Upload, Unsupported}` with `label()`: "verified write-back", "best effort", "best effort", "open only", "unsupported".
- `transport(adapter, want, handle_refusal: Option<&str>) -> Transport::{Handle, Buffer}`: `vscodeDev` always uses a handle (its refusal stays an error, as today), `photopea` always a buffer, and `none` uses a handle only if it was wanted and the path allows one.
- Tests:
  - `ready` with and without `want`, and with an unknown `want`;
  - `file-tier` with tiers 1 and 3, and refusing 0, 2, 4 and a non-number;
  - every row of `transport`;
  - `file_tier`: matched, mismatched, undelivered and revoked tokens;
  - each label.

## Task 2: runtime

**`main.rs`:** `Adapter::None` gets `generic::script(&trusted_origins)` as its initialization script.

**`src/generic.js` and `src/generic.rs`:** clarifications 1, 3, 4 and 5. `generic.rs` holds only `script()` and its template test, like `vscode.rs`.

**`bridge.rs`:**

- Remember each document's `want`, cleared when the document changes, as `Session` already clears `ready`.
- `start()` chooses by `transport(...)`.
- `on_message(FileTier)`: clarification 8, including that the tier matches how this token was delivered, then log `file-tier`, and for tier 3 the one-time notice (clarification 7). The bridge learns the data folder from `install`.
- `on_message(OpenFailed)` for `Adapter::None`, with the box from clarification 5.

**`inspect.rs`:** `fileTier`.

## Task 3: end to end in CI

**Local, deterministic** (`e2e/generic.mjs`, called from the `bridge.mjs` step like `photopea.mjs`):

- A fifth app, `bridge-e`, uses `adapter: none` and `--url https://localhost:8447/`. That port serves one page per tier, all on one origin. The test navigates the app between them and forwards a file with a second launch. Each page records what it received in `window.__got`.
- Cases:
  1. **`/pwa`** calls `setConsumer` at once. A file from `outside` reaches the consumer as a handle. The page writes `edited` through `createWritable()`, and the original holds `edited`. Log: `file-tier 1`.
  2. **`/pwa` with a file in `%TEMP%`** falls back to a buffer and reaches the page's upload box. Log: `file-tier 3`, the notice shown once, and `open-only-notice` exists afterwards.
  3. **`/late`** calls `setConsumer` after 12 s. The file arrives as an upload, not to the consumer.
  4. **`/upload`** has one visible and one hidden upload box. The visible one gets a `File` with the right name and SHA-256, and gets `change`. Log: `file-tier 3`. No second notice.
  5. **`/drop`** has no upload box and a drop zone that calls `preventDefault()` on `dragover` and on `drop`, as a site that handles drops does (clarification 4). It gets the file. Log: `file-tier 3`.
  6. **`/none`** has neither. Log: `open-failed` and a box naming the file, with no `file-tier`.
  7. **`/picker`** has a button that calls `showOpenFilePicker`. The test clicks it with DevTools mouse events (a user gesture), picks a file from `outside` with D3's `pickInDialog`, extended to find WebView2's own picker window among the app's `msedgewebview2.exe` processes (clarification 6), and the page writes through the handle. The file changes, and the runtime delivered nothing (tier 2 is untouched).
  8. **Gate 8 still holds:** on `https://127.0.0.1:8447/pwa` (untrusted), `'launchQueue' in window` is false, no `webwrap-ready` is accepted, and a forwarded file is not delivered.
- **D1's `bridge-a` and `bridge-b` pages change** (check before Task 2 lands). They are `adapter: none`, so the generic script now runs there too and also receives their `webwrap-file` buffers. Their listener must copy the bytes synchronously before hashing, because the script releases the buffer. Their page must also have a drop handler, so files there take tier 3 and do not raise tier 4 boxes. The one-time open-only notice then fires once per app in this suite (clarification 7). The box is shown on its own thread, as the save-failure box already is, and the evidence records that it appeared. Their save-protocol checks still post messages directly and are unaffected.

**Live** (`e2e/generic-live.mjs`, its own step, also weekly): an app for `https://excalidraw.com/` launched with a small `.excalidraw` file from `outside`.

- Excalidraw declares `file_handlers` and consumes `launchQueue`. Expected: `file-tier 1`, and the scene shows the file's element (read through DevTools).
- Then an edit and Ctrl+S, if Excalidraw saves through the handle. The file changes, and nothing lands in Downloads.
- If Excalidraw does not set its consumer within 10 s in WebView2, the step records what happened, and tier 1 rests on the local test. That outcome is evidence, not a failure to hide.

**Workflow:** generate `bridge-e` and the live app; serve port 8447 from the test.

## Task 4: evidence, review, handover

- `docs/evidence/2026-11-21-generic-sites.md`.
- An independent review, with the findings fixed or recorded.
- Update `HANDOVER.md` (D done; E next) and the spec status, and rewrite the spec's `fileTier` sentence (D1 section, `--inspect-self`) to match clarification 9.
- Add D4's checks to `docs/windows-runs/NEXT.md`:
  - the open-only notice and the unsupported box as they appear on screen;
  - dropping an image on `twitter-<arch>.exe` (expected: tier 3 if Twitter has a visible upload box on the current page, otherwise the unsupported box);
  - the live Excalidraw open and save by hand.

## Not in D4

- Upload boxes with no client rects — hidden with `display:none` or `hidden`, often behind a styled button, common on real sites — are not filled. The spec says "single visible", and guessing which hidden input matters could upload a file somewhere the user did not intend. An input styled invisible with `opacity` or clipping does have client rects and counts as visible. This can be reconsidered after the PC run.
- Replacing the original from a site's download stays a later feature, as the spec says.
