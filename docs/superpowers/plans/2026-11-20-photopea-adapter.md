# Photopea Adapter Implementation Plan (sub-project D, phase D3)

Spec: [`docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md`](../specs/2026-11-20-file-bridge-and-adapters-design.md), section D3. It builds on D1's bridge and its page protocol, and follows D2's patterns: a local stand-in in CI, a live-site step, and a weekly run. Branch `feat/photopea-adapter`.

## Goal

An app generated with `--adapter photopea` opens a file it is given, at launch or from a later launch, as a document in Photopea. Photopea's File > Save (Ctrl+S) writes the original file, for PSDs of 150 MB and more. Ctrl+O opens a file through the native Windows dialog, and it saves back the same way.

This is the save half of gate 4; its "Open with" and double-click half stays with E.

## Global constraints

As D1 and D2. Two of them matter more here:

- **No new dependencies.** The open dialog uses `IFileOpenDialog` from the `windows` crate, which needs two more of its features: `Win32_System_Com` and `Win32_UI_Shell_Common`.
- **Every decision is a `webwrap: <event>` line.**

## What Photopea does, checked

These were checked against live Photopea in headless Chromium on 2026-11-20, from a page embedding `https://www.photopea.com/#<config>`:

- **Startup:** Photopea posts `"done"` when it is ready, and again after each message it processes.
- **Opening:** an `ArrayBuffer` posted to it opens as a new, active document, named `file`, with `source` `"file"`.
  - `app.activeDocument.name` and `.source` are writable.
  - A name of `x.psd` shows as `x.psd.psd`, so the shell sets the name without its extension.
  - A second file becomes a second document. `app.documents[0].source` keeps its earlier value.
- **Saving:** `app.activeDocument.saveToOE(f)` posts an `ArrayBuffer`, then `"done"`. For `psd`, `png`, `jpg:0.95`, and `webp`, the bytes begin `8BPS`, `\x89PNG`, `FF D8 FF`, and `RIFF`.
- **Scripts:** `artLayers.add()` and `activeLayer.invert()` work.
- **`customIO`:** `{open, save}` replaces File > Open and File > Save. Clicking those menu items posted `"Open"` and `"Save"`.
  - DevTools Ctrl+S and Ctrl+O did not reach the hooks in headless Chromium.
  - The spike's DevTools Ctrl+S did reach them in WebView2, so the live CI step decides. If Ctrl+S does not, it records that and uses the menu.
- **Its documentation:** `customIO` also knows `new`, `saveAsPSD`, `exportLayers` and others. `localsave: false` would remove Photopea's own download saves; D3 keeps them.

## Clarifications of the spec

1. **The shell page.**
   - The shell is a bundled asset, `crates/webwrap-runtime/ui/photopea.html`.
   - It is served at `https://tauri.localhost/photopea.html`, with `use_https_scheme(true)` for this adapter only. The navigation policy allows only `https` at the top level, and only `https` origins can be trusted.
   - Its script, `src/photopea.js`, is an initialization script like D2's. It is templated with Photopea's launch URL and origin from the spec, and runs only in the top-level document at `https://tauri.localhost/photopea.html`.
   - The session trusts only `https://tauri.localhost`. Photopea's frame never talks to the runtime.
   - The shell talks to Photopea only through `frame.contentWindow` and `spec.trustedOrigins[0]`, and ignores any other sender.
   - The runtime registers no Tauri commands and no capabilities, so the shell has no Tauri API.
2. **Open, through D1's shared buffers, not the spike's handles.**
   - D2 measured that WebView2 refuses handles in AppData, on shares, and for long paths. D1's shared buffers have none of these limits.
   - The shell copies each delivered buffer into an ordinary `ArrayBuffer`, releases the shared one, and posts the copy to Photopea. It opens one file at a time.
   - After Photopea's `"done"`, one script tags the new document with its `source` (`webwrap:<token>`) and name, but only if `app.documents.length` grew by one. Otherwise a file Photopea could not open would retag the document that was already active, and saving that document would overwrite the wrong file.
   - A file Photopea did not open is reported with D2's `webwrap-open-failed`, which this adapter also honours.
3. **Save.**
   - `customIO.save` posts `"Save"`. One script then reads the active document's `source`. If the source is a known token, it exports in that file's format; otherwise it reports the document as unbound. So the check and the export happen in one Photopea turn, never on a stale active document.
   - Format by extension: `.psd` exports `psd`, `.png` exports `png`, `.jpg` and `.jpeg` export `jpg:0.95`, and `.webp` exports `webp`. Any other extension exports `psd`, which the runtime then refuses with a message.
   - The bytes go through D1's save protocol (`webwrap-save`, then the save buffer, then `webwrap-save-commit`).
   - The runtime checks that the bytes' signature matches the target file's extension before `ReplaceFileW`. It never writes PSD bytes into a `.png`.
   - The shell runs one save at a time and shows a short "Saved" or "Not saved" notice. A failed write already shows D1's message box.
4. **An unbound document is downloaded.** A document made inside Photopea has no file. Its Save exports `psd` and downloads it as `<name>.psd`, as Photopea does in a browser. Photopea's own "Save as PSD" and "Export as" still download, because `localsave` keeps its default.
5. **Ctrl+O.**
   - `customIO.open` posts `"Open"`, and the shell sends a new message, `webwrap-pick-file`.
   - The runtime shows `IFileOpenDialog`, owned by the window. It allows several files, filtered to the app's file associations plus "All files".
   - The chosen paths go through `resolve` and `Bridge::open`, so they become tokens exactly like double-clicked files.
   - The dialog is opened from `run_on_main_thread`, not inside the message handler.
   - Only this adapter honours the message, and only from the trusted shell (`pick-file-refused` otherwise).
6. **Large files.** The transport limit is D1's 4 GiB. CI generates a 150 MB PSD with random pixels, which compression cannot shrink, and opens and saves it against live Photopea.
7. **The shell is a different storage partition** from today's top-level `www.photopea.com`. Photopea settings saved by earlier builds of a Photopea app do not carry over. Nothing has been released, so nothing is lost.

## Page protocol additions

| Direction | Message | Meaning |
| --- | --- | --- |
| page → host | `{"type":"webwrap-pick-file"}` | Show the native open dialog. `photopea` adapter only, from the trusted shell. |
| page → host | `{"type":"webwrap-open-failed","tokens":[…]}` | From D2, now also honoured for `photopea`. The box names the site. |

## Task 1: `webwrap-bridge`

- `PageMessage::PickFile`.
- `save_signature_ok(path, bytes) -> Result<(), String>` checks the bytes against the extension: `psd`, `png`, `jpg`/`jpeg`, or `webp`. Any other extension is refused with "Photopea saves back only PSD, PNG, JPEG and WebP files".
- Tests:
  - each format's signature, matched and mismatched;
  - extensions in upper case;
  - an unknown extension;
  - empty bytes.

## Task 2: runtime

**`main.rs`.** For `Adapter::Photopea`:

- the window uses `WebviewUrl::App("photopea.html")` with `use_https_scheme(true)`;
- the initialization script is `photopea::script(launch_url, origin)`;
- the session trusts `https://tauri.localhost`.

**`ui/photopea.html`.** A full-window iframe container, a status line, and no script: the initialization script does the work.

**`src/photopea.js`**:

- Builds the iframe with the `customIO` config.
- Says `webwrap-ready` once Photopea's first `"done"` arrives.
- Then handles opening, saving, unbound documents, and Ctrl+O as clarifications 2-5 describe.

**`src/photopea.rs`**:

- `script(url, origin)`.
- `pick_files(hwnd, associations) -> Vec<PathBuf>`, using `IFileOpenDialog`.

**`bridge.rs`**:

- `on_message(PickFile)`: check the adapter and both sources, then `run_on_main_thread`, then `pick_files`, then `resolve`, then `open`.
- `write()`: for `photopea`, `save_signature_ok` runs before `save::replace`.
- `OpenFailed` also for `photopea`, with the site's name in the box.

## Task 3: delete the spike

- Remove `spikes/photopea-windows-writeback`.
- Remove `spikes/*` from the workspace members, because no spike is left.

## Task 4: end to end in CI

**Local, deterministic** (`e2e/bridge.mjs`):

- A fourth app, `bridge-d`, uses `--adapter photopea --url https://localhost:8446/`.
- That port serves a stand-in Photopea that follows the behaviour above:
  - it posts `"done"`;
  - it opens `ArrayBuffer`s, but refuses ones starting `BAD!`, adding no document;
  - it runs scripts against a small fake `app`;
  - its `saveToOE` returns the document's bytes and records the format;
  - Ctrl+S and Ctrl+O run the `customIO` scripts.
- The test drives the stand-in through its own DevTools target. `cdp.mjs` learns to attach to `iframe` targets.
- Cases:
  1. **Launch with a 160 MiB PSD:** Photopea holds the same bytes, tagged with its token.
  2. **Ctrl+S after an edit:** the original file holds the edited bytes, byte for byte, and no `.webwrap-*` file is left.
  3. **A PNG from a second launch** opens as a second document. Saving each document writes only its own file, and the PNG is exported as `png`.
  4. **PSD bytes the extension does not match** are refused with a message box, and the file is unchanged.
  5. **A file Photopea refuses** (`BAD!`) is reported once, and the active document keeps its tag.
  6. **Ctrl+S on an unbound document** downloads `<name>.psd` to Downloads.
  7. **Ctrl+O:**
     - the native dialog appears;
     - PowerShell UI Automation types a path into it and presses Open;
     - the file opens and saves back.

     If UI Automation proves unreliable on the runner, this case records that, and Ctrl+O stays a PC check.
  8. **Gate 8:** on `https://127.0.0.1:8446/` (an untrusted top level), these are each refused, and no file reaches it:
     - `webwrap-ready`;
     - a save;
     - `webwrap-pick-file`.

**Live** (`e2e/photopea-live.mjs`, its own CI step, also weekly):

- An app is generated for `https://www.photopea.com/` with `--adapter photopea`, and launched with a generated 150 MB PSD.
- The test waits for the shell's "Opened" notice.
- It posts `app.activeDocument.artLayers.add()` into Photopea and presses Ctrl+S, falling back to File > Save if Ctrl+S does not reach the hook.
- The original must:
  - begin with `8BPS`;
  - still be at least 150 MB;
  - have changed;
  - have no copy in Downloads.

**Workflow:**

- Generate `bridge-d` and the live app.
- Raise the end-to-end steps' timeouts for the large files.

## Task 5: evidence, review, handover

- `docs/evidence/2026-11-20-photopea-adapter.md`.
- An independent review, with the findings fixed or recorded.
- Update `HANDOVER.md` and the spec status.
- Add D3's checks to `docs/windows-runs/NEXT.md`:
  - a real PSD the user has;
  - Ctrl+O through the dialog, by hand;
  - the "Saved" notice;
  - a document made in Photopea.
