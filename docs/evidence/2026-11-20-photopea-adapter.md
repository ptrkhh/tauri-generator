# Evidence: Photopea adapter (sub-project D, phase D3)

Date: 2026-11-20. Branch `feat/photopea-adapter`. Spec: `docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md` (section D3). Plan: `docs/superpowers/plans/2026-11-20-photopea-adapter.md`.

**What it closes:** the save half of **gate 4**. A 150 MB PSD handed to an app generated with `--adapter photopea` opens in live Photopea. After an edit, Photopea's File > Save writes the edited document back over the original file, in place and in PSD format, with no download.

**What it does not prove:**

- **Gate 4's "Open with" and double-click half** waits for registration (E).
- **Ctrl+S from a real keyboard.** DevTools-simulated Ctrl+S never reached Photopea's Save hook, in headless Chromium or in WebView2, while File > Save always did. Both run the same hook, so this is a hands-on PC check (section 7).

Ctrl+O, by contrast, is proven in CI through the real Windows Open dialog (stand-in case 8).

## 1. Environments

As D2 (D2 evidence, section 1), plus:

- **Live Photopea:** the version served on 2026-11-20.
- **Before CI:** the shell script and the stand-in were run against live Photopea in local headless Chromium 1255, with a stand-in for `chrome.webview` that hands over shared buffers as plain `ArrayBuffer`s.

## 2. Automated results

| Suite | Linux | x64 CI | ARM64 CI |
| --- | --- | --- | --- |
| `webwrap-bridge` (adds: the pick-file message, save signatures) | 27 | 26 | 26 |
| `webwrap-spec` | 39 | 39 | 39 |
| `webwrap-runtime` unit (adds: the shell script's template) | 16 (Wine) | 16 | 16 |
| `webwrap-runtime` integration | 9 (Wine) | 9 | 9 |
| `webwrap-pack`, `webwrap-generate` | 24 | 24 + probe | 24 + probe |
| clippy `-D warnings`, all crates, both targets | pass (cross) | pass | pass |
| End to end, D1 and D2 (`e2e/bridge.mjs`) | n/a | 15 of 15 | 15 of 15 |
| End to end, `e2e/photopea.mjs`, the stand-in Photopea | n/a | 10 of 10 | 10 of 10 |
| End to end, `e2e/vscode-live.mjs` | n/a | 3 of 3 | 3 of 3 |
| End to end, `e2e/photopea-live.mjs`, against live Photopea | n/a | 2 of 2 | 2 of 2 |

**CI run:** `44800372868` at `0cd320d`, the final code commit. Every row above passed on both runners.

**Stand-in cases.** `bridge-d` is generated with `--adapter photopea` for a local stand-in Photopea. The stand-in matches live Photopea where the shell relies on it:

- it handles messages strictly in order;
- an open takes a moment, and an unreadable file gets no answer at all;
- a file becomes the last document, named and sourced `file`;
- `saveToOE` exports only the **active** document.

The cases:

1. **Open:** a 160 MiB PSD given at launch reaches Photopea byte for byte, tagged with its token and named `big`.
2. **Save after an edit** writes the edited 160 MiB to the original, byte for byte, as PSD, and leaves no temporary file.
3. **A PNG from a second launch.** The user switches to the first tab while the PNG opens, yet only the PNG's document is tagged. It saves as PNG, and only its own file changes.
4. **Save follows the pressed document.** Save is pressed on `big.psd` and then another tab becomes active. Still `big.psd`, not the PNG, is written.
5. **Format refusal.** PSD bytes for `image.png` are refused (`save-failed … not a .png file`), and the file is unchanged.
6. **An unreadable file** is reported once (`open-failed bad.psd`), and no document is retagged.
7. **A document made in Photopea** downloads as `Untitled-1.psd`.
8. **Ctrl+O.** The native dialog appears (`pick-file shown`). The test chooses a file in it as a person would: it types the path into the file-name box and presses Open. The file opens and saves back.
   - UI Automation does not expose that box or the Open button on the runners. The test reaches them as the dialog's classic child windows, ids 1148 and 1.
   - If that ever fails, the check records it and closes the dialog. It then still requires the app to report the dialog closed.
9. **Photopea's frame cannot reach the app.** `chrome.webview` does exist inside the frame. The frame posted `webwrap-ready` and `webwrap-pick-file` through it, and no dialog opened.
10. **Gate 8.** On an untrusted top level these are all refused, and a file forwarded meanwhile arrives only after returning to the shell:
    - `webwrap-ready`;
    - a save;
    - `webwrap-pick-file`.

**Live cases.** `photopea-live` is generated for `https://www.photopea.com/` and launched with a 151 MB PSD of random pixels.

1. **Open:** it opens (the shell's "Opened live.psd" notice).
2. **Save after an edit.** A new layer is added, then File > Save. The original then:
   - begins `8BPS`;
   - is at least 150 MB;
   - has changed;
   - holds two layers, so the edit was saved;
   - has no copy in Downloads.

## 3. What Photopea does, checked

Against live Photopea, in headless Chromium:

- **Startup and messages:** it posts `"done"` when ready, and handles messages in order.
- **Unreadable buffers:** an `ArrayBuffer` it cannot read gets no reply at all, so a shell waiting for its `"done"` would wait forever. The shell's later scripts still answer.
- **Opened files:** a buffer it opens becomes the last document, named and sourced `file`. A document from `app.documents.add()` is sourced `local,0,…`.
- **Names:** `name` and `source` are writable. A name like `x.psd` is stored as `x`.
- **Exports:**
  - `saveToOE("psd"|"png"|"jpg:0.95"|"webp")` gives correct bytes.
  - **`saveToOE` exports the active document, whichever document object it is called on.** Setting `app.activeDocument` first exports the right one.
- **Layers:** a saved PSD lists one layer as opened, and two after `artLayers.add()`.
- **The `customIO` hooks:** File > Save and File > Open run them. DevTools Ctrl+S and Ctrl+O do not, in headless Chromium or WebView2.

## 4. Deviations from the spec

The plan's clarifications record each with its reason:

1. **The shell is `https://tauri.localhost/photopea.html`.** It uses `use_https_scheme(true)`, and the session trusts only it.
2. **Files arrive through D1's shared buffers,** not the spike's handles, so D2's folder and share limits do not apply. For example, a file in `%TEMP%` opens and saves.
3. **Save exports the file's own format.** The runtime checks the signature against the extension before writing.
4. **A document made in Photopea downloads,** as in a browser.
5. **Ctrl+O uses `IFileOpenDialog`,** opened after the WebView2 event returns, one dialog at a time.

## 5. Review

An independent reviewer read the branch.

| Finding | Outcome |
| --- | --- |
| A request's 5-minute timeout, never cleared, could cancel a later request, losing a save or leaving an open untagged | Fixed: each request owns and clears its timer, and only clears `pending` if it is still its own |
| Replies before a request's echo were collected, so a late export could be written into another file | Fixed: replies count only after the request's own echo |
| Tagging used the active document, so a tab switch or a new document mid-open bound the wrong document to the file | Fixed: only the newest document, still `file`/`file`. Stand-in case 3 |
| A queued Save exported whatever was active when it ran | Fixed: Save carries the pressed document, which the shell activates before exporting. Stand-in case 4 |
| The open dialog ran inside the WebView2 event, and a second one could open | Fixed: deferred through another thread, and guarded |
| The UI Automation step could drive a message box | Fixed: it requires the file-name box and stops on errors |
| No test that the frame cannot reach the app; the stand-in answered too fast; the live test could not tell the edit was saved | Fixed: stand-in case 9, ordered and delayed handling, and a layer count |

**Checked by the reviewer, nothing found:**

- the origin and source checks in both directions;
- templating into Photopea scripts;
- inert Tauri IPC on the local origin;
- the bridge's trust boundary and the signature check;
- COM threading and memory in `pick_files`.

**Re-review.** The same reviewer re-read the fix commits. Six of the seven findings were resolved. Two remained, both now fixed:

| Re-review finding | Outcome |
| --- | --- |
| Echoes were matched by kind only. A save that gave up after 5 minutes could have its late echo complete the next save, and its export be written to that file | Fixed: each request carries an id, and its echo must return it |
| A document with an empty source but a name could no longer be saved at all | Fixed: such a document downloads again |

The reviewer found no remaining way to write one document's bytes into another's file, or a format into a file of another.

## 6. Known limits

1. **Keyboard Ctrl+S is unverified by automation** (section 3). It is a PC check.
2. **A renamed document, or a copy Photopea made of one, downloads on Save** instead of writing the file. Safe, but surprising.
3. **Two documents with the same file tag and name are not saved;** the user is told. This can happen if Photopea duplicates without renaming, which is unmeasured.
4. **The tag check relies on a fresh document being named and sourced `file`.** One case could still be mis-tagged:
   - the user drags a file named `file.*` into Photopea in the instant one of the app's files fails to open;
   - that document is then the newest one, so it would be tagged with the app's file instead.

   This is untested.
5. **Photopea settings saved by earlier builds** do not carry over, because the shell is a different storage partition. Nothing has been released.
6. **Formats other than PSD, PNG, JPEG, and WebP are refused on save** with a message. JPEG saves re-encode at quality 0.95.

## 7. Hands-on PC checks

Queued in `docs/windows-runs/NEXT.md`:

- a real PSD the user has opens, and Ctrl+S from the keyboard saves it;
- Ctrl+O through the dialog by hand;
- the notices as they appear;
- a document made in Photopea downloads.
