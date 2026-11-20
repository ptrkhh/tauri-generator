# vscode.dev Adapter Implementation Plan (sub-project D, phase D2)

Spec: [`docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md`](../specs/2026-11-20-file-bridge-and-adapters-design.md), section D2. Builds on D1's bridge (plan `2026-11-20-file-bridge-runtime-plumbing.md`). Branch `feat/vscode-adapter`.

## Goal

An app generated with `--adapter vscodeDev` opens a file it is given, at launch or from a later launch, as a tab in `vscode.dev`. Ctrl+S then changes the original file. This is the save half of gate 5; its "Open with" and double-click half stays with E.

## Global constraints

Same as D1:

- code is written and cross-built on Linux, and Windows CI on x64 and ARM64 decides;
- no new dependencies: `CreateWebFileSystemFileHandle`, `PostWebMessageAsJsonWithAdditionalObjects`, and `CallDevToolsProtocolMethod` are all in `webview2-com =0.38.2`, as the spike used them;
- the runtime's handlers only ever veto;
- every decision prints one `webwrap: <event>` line to stderr.

## What vscode.dev does, checked

Read from the live workbench bundle (`workbench.web.main.internal.js`, build `8a14f0cc1b08d531eb83255ea3764ae22aa932b5`, 2026-11-20), then tried against live vscode.dev in headless Chromium, with an origin-private file standing in for WebView2's handle:

- **No `launchQueue` consumer.** The string does not occur, so D4's PWA tier cannot open files in it.
- **"File: Open File..." (`pickFileAndOpen`) calls `showOpenFilePicker({multiple:false})`** and opens the returned handle with `registerFileHandle`: exactly one file per run.
- **Constructed events (`isTrusted: false`) drive the command palette.**
  - A constructed Ctrl+Shift+P keydown opens it, since the keybinding service and the quick input never check `isTrusted`.
  - Setting the input's value and firing `input` filters it to "File: Open File...".
  - A constructed click on that highlighted row runs the command.
  - A constructed Enter is ignored. vscode.dev's dispatcher receives it and matches nothing.
- **End to end in Chromium:**
  - three handles opened as three tabs, in order, in 1.3 s;
  - Ctrl+S through DevTools input wrote the edited file through its handle;
  - the other files were unchanged.

## Clarifications of the spec

1. **The page script runs the Open File command; the runtime sends no input.**
   - The spec followed the spike, which drove the command with DevTools key events and `Input.insertText`. Those go to whatever has focus when they arrive. The first review found that vscode.dev moving focus into an editor between the runtime's check and its keys would type "Open File..." into the user's file, which a later Ctrl+S saves.
   - Constructed events dispatched on a checked element cannot do that: they have no default action, so nothing is ever typed.
   - Doing it in the page also removes the runtime's DevTools chain, its command state, and keys reaching a focused cross-origin frame.
2. **One file at a time.** The script keeps a queue. It runs Open File for the oldest file, answers vscode.dev's picker call with that file's handle, and waits for the file's tab before starting the next.
3. **The adapter script reads the app's trusted origins from the spec** instead of hard-coding `https://vscode.dev`. This lets CI run the same script against a local stand-in (Task 4). The runtime still enforces origins natively; the script's check only keeps it from altering other sites.
4. **Handles reuse D1's token model.**
   - A handle is posted only through `Session::ready`, then `Session::delivery_read`, exactly like a shared buffer. So gate 8 and the pending-navigation hold apply unchanged.
   - There is no shared buffer and no `webwrap-save*` message for this adapter. Saving is vscode.dev's own `createWritable()` on the handle, which WebView2 carries out.
   - Chromium writes a `.crswap` file next to the original and moves it over the original on `close()`. Task 4 asserts that no swap file is left. Whether the original keeps its creation time is WebView2's behaviour, not the runtime's, so Task 4 records it rather than asserting it.
5. **If the command cannot run, the user is told once.**
   - The script tries Ctrl+Shift+P for up to 30 s, because early on vscode.dev may not handle keys yet.
   - It then posts `webwrap-open-failed`. The runtime shows a message box naming the file, saying that vscode.dev's own File > Open File... will open it.
   - The file stays queued in the page, so that command does open it. Nothing retries automatically.
6. **Ctrl+O needs no code.** With nothing queued, the script passes `showOpenFilePicker` through to WebView2's native picker. The hands-on PC run checks that it saves back. CI cannot drive the Windows dialog reliably.
7. **The live test runs on every push and weekly.**
   - A separate CI step runs it, so a vscode.dev outage reads as that step failing, not the bridge.
   - The workflow gains a weekly `schedule`.

## Page protocol additions

These extend D1's table and keep its rules.

| Direction | Message | Meaning |
| --- | --- | --- |
| host → page | message `{"type":"webwrap-file-handle","token","name"}` with one `FileSystemFileHandle` in `additionalObjects` | One opened file, read-write. Only for the `vscodeDev` adapter. |
| page → host | `{"type":"webwrap-open-failed","token"}` | vscode.dev did not open this delivered file. Honoured only for the `vscodeDev` adapter, from a trusted page, for a token delivered to it; the runtime shows a message box naming the file. |

## Task 1: `webwrap-bridge` session additions

- `PageMessage::OpenFailed { token }` (`"webwrap-open-failed"`).
- `Session::delivered_name(source, live, token) -> Result<&str, Refusal>`: the name of a delivered file, only for a trusted source and live document.
- Tests:
  - the message parses;
  - an untrusted source or live document, a queued, unknown, or differently spelled token, and a token revoked by an untrusted document are each refused.

## Task 2: runtime

**`main.rs`**

- Passes `spec.adapter` to `Bridge::install`.
- For `Adapter::VscodeDev`, adds `vscode::script(&trusted_origins)` as an initialization script.

**`src/vscode.js`**

- Runs only in the top-level document of a trusted origin; the origins are JSON substituted by `vscode::script`.
- Posts `webwrap-ready` once `.monaco-workbench` exists.
- On `webwrap-file-handle`, it queues the handle and runs the queue (clarifications 1, 2, and 5). `showOpenFilePicker` answers from the queue, or calls the native picker when it is empty.

**`src/vscode.rs`**

- `script(origins) -> String`.
- `post_handle(environment, webview, path, message)`: `CreateWebFileSystemFileHandle` (read-write) plus `PostWebMessageAsJsonWithAdditionalObjects`, from the spike.

**`bridge.rs`**

- `Bridge` gains `adapter`.
- `start` posts handles for `vscodeDev` (`post_handle`: `delivery_read`, then `Post` or `Retry`, as `finish_read` does). It reads into buffers otherwise.
- `on_message(OpenFailed)`:
  - logs `open-failed <name>` and shows the message box;
  - is refused (`open-failed-refused`) for another adapter, or when `delivered_name` refuses.

## Task 3: delete the spike

- Remove `spikes/vscode-webview2-handle`.
- Its plan and result report (`docs/spikes/2026-09-14-vscode-webview2-handle.md`) stay as history.

## Task 4: end to end in CI

**Local, deterministic** (in `e2e/bridge.mjs`):

- A third app, `bridge-c`, uses `--adapter vscodeDev` for `https://localhost:8445/`.
- That port serves a stand-in that matches vscode.dev where the adapter relies on it:
  - a constructed Ctrl+Shift+P opens a palette;
  - its input filters to a highlighted entry;
  - clicking "File: Open File..." calls `showOpenFilePicker({multiple:false})`. Then, 300 ms later, it opens a tab and focuses a `<textarea>` editor.
- Cases:
  1. **Launch with a file:** it opens. The page writes new text through the handle's `createWritable()`. The file on disk changes and no `.crswap` file is left; whether its creation time is kept is recorded.
  2. **Two files from a second launch:** both open, in order. The editor stays empty and the page sees no trusted key event.
  3. **Gate 8 for handles.**
     - On `https://127.0.0.1:8445/`, `webwrap-ready` and `webwrap-open-failed` are refused, and no message reaches the page.
     - A file forwarded meanwhile is delivered only after returning to `https://localhost:8445/`.
  4. **No palette (`?nopalette`):** after the script's 30 s of tries, the user is told once (`open-failed no-palette.md`).

**Live** (`e2e/vscode-live.mjs`, its own CI step):

- An app generated for `https://vscode.dev/` with `--adapter vscodeDev` is launched with `live-one.md` (`before\r\n`).
- The test waits for a tab named `live-one.md` whose editor shows `before`.
- It sends Ctrl+A, inserts `after\r\n`, and sends Ctrl+S, as a user would.
- The file must become exactly `after\r\n`, with no copy in Downloads.
- A second launch with `live-two.md` must open a second tab, and Ctrl+S must change that file too. This is gate 6 on the real site.

**Workflow**

- Generates `bridge-c` and the live app.
- Adds the `vscode` acceptance app (`--url vscode.dev --adapter vscodeDev --assoc .md text/markdown "Markdown document"`) for the PC run.
- Adds `schedule: cron: "17 5 * * 1"`.

## Task 5: evidence, review, handover

- `docs/evidence/2026-11-20-vscode-dev-adapter.md`.
- An independent review of the branch diff, with the findings fixed or recorded.
- Update `HANDOVER.md` and the spec status.
- Append the D2 hands-on checks to `docs/windows-runs/NEXT.md`:
  - opening by dropping a file on the exe;
  - Ctrl+O through the native dialog, then Ctrl+S;
  - a second file opening as a new tab.
