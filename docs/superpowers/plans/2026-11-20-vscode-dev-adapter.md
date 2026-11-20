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

Read from the live workbench bundle (`workbench.web.main.internal.js`, build `8a14f0cc1b08d531eb83255ea3764ae22aa932b5`, 2026-11-20):

- **It has no `launchQueue` consumer.** The string does not occur, so D4's PWA tier cannot open files in it.
- **"File: Open File..." (`pickFileAndOpen`) calls `showOpenFilePicker({multiple:false})`.** It then opens the returned handle with `registerFileHandle`. So the command opens exactly one handle per run.

That leaves the spike's route: hand the page a real handle, and run the Open File command so that vscode.dev asks for it.

## Clarifications of the spec

1. **One Open File command per file, in turn.**
   - The adapter script keeps a queue of handles. Each `showOpenFilePicker` call takes one, and the script asks for another command while handles remain.
   - The runtime runs one command at a time. A request that arrives while one runs is remembered and run next, never dropped and never run concurrently.
2. **The adapter script reads the app's trusted origins from the spec** instead of hard-coding `https://vscode.dev`. This lets CI run the same script against a local test page (Task 4). The runtime still enforces origins natively; the script's check only keeps it from altering other sites.
3. **Handles reuse D1's token model.**
   - A handle is posted only through `Session::ready`, then `Session::delivery_read`, exactly like a shared buffer. So gate 8 and the pending-navigation hold apply unchanged.
   - There is no shared buffer and no `webwrap-save*` message for this adapter. Saving is vscode.dev's own `createWritable()` on the handle, which WebView2 carries out.
   - Chromium writes a `.crswap` file next to the original and swaps it in with `ReplaceFileW` (`base::ReplaceFile`). So the original keeps its creation time, like D1's own save. Task 4 asserts both points, the creation time and the swap file being gone, rather than trusting them.
4. **The Open File command is driven only into the document that asked.**
   - Before each DevTools step, the runtime checks two things: the live document is still the one the request came from (`Session::document`), and it may still be posted to.
   - Otherwise it stops. The check is the spike's, made per document instead of per URL.
5. **A failed command shows a message box and is not retried.** The page keeps the handle, and the message says vscode.dev's own File > Open File... will open it. Retrying automatically could type into the page repeatedly.
6. **Ctrl+O needs no code.** With no handle waiting, the script passes `showOpenFilePicker` through to WebView2's native picker. The hands-on PC run checks that it saves back. CI cannot drive the Windows dialog reliably.
7. **The live test runs on every push and weekly.**
   - A separate CI step runs it, so a vscode.dev outage reads as that step failing, not the bridge.
   - The workflow gains a weekly `schedule`.

## Page protocol additions

These extend D1's table and keep its rules.

| Direction | Message | Meaning |
| --- | --- | --- |
| host → page | message `{"type":"webwrap-file-handle","token","name"}` with one `FileSystemFileHandle` in `additionalObjects` | One opened file, read-write. Only for the `vscodeDev` adapter. |
| page → host | `{"type":"webwrap-vscode-open"}` | The page holds a handle. Run vscode.dev's Open File command once. Honoured only for the `vscodeDev` adapter, from a trusted, ready document. |

## Task 1: `webwrap-bridge` session additions

- `PageMessage::VscodeOpen` (`"webwrap-vscode-open"`).
- `Session::document() -> u64`: the current document's generation, which `document_changed` already increments.
- `Session::open_command(source, live) -> Result<u64, Refusal>`:
  - refuses an untrusted source or live document, or one that is not ready;
  - refuses while an untrusted navigation is pending;
  - otherwise returns `document()`.
- `Session::may_drive(document, live) -> bool`: `document == self.document() && self.may_post(live)`.
- Tests:
  - the message parses;
  - an untrusted source, an unready document, and a pending navigation are each refused;
  - a new document makes an earlier `document()` stale for `may_drive`.

## Task 2: runtime

**`main.rs`**

- Passes `spec.adapter` to `Bridge::install`.
- For `Adapter::VscodeDev`, adds `vscode::script(&trusted_origins)` as an initialization script.

**`src/vscode.js`** (ported from the spike)

- Runs only in the top-level document of a trusted origin; the origins are JSON substituted by `vscode::script`.
- Replaces `showOpenFilePicker`:
  - with a handle queued, it returns `[handle]`, and posts `webwrap-vscode-open` if more remain;
  - otherwise it calls the original picker.
- Posts `webwrap-ready` once `.monaco-workbench` exists.
- On `webwrap-file-handle`, it queues the handle and posts `webwrap-vscode-open`.

**`src/vscode.rs`**

- `script(origins) -> String`.
- `post_handle(environment, webview, path, info)`: `CreateWebFileSystemFileHandle` (read-write) plus `PostWebMessageAsJsonWithAdditionalObjects`, from the spike.
- `run_open_command(...)`: the spike's six-step DevTools chain:
  1. Ctrl+Shift+P down;
  2. Ctrl+Shift+P up;
  3. `insertText "Open File..."`;
  4. a bounded 5 s readiness check that the selected entry is Open File;
  5. Enter down;
  6. Enter up.

  It checks `may_drive` before each step. It calls back with `Ok` or `Err(reason)` on the UI thread.

**`bridge.rs`**

- `Bridge` gains `adapter` and two flags: `opening` and `open_again`.
- `start` posts handles for `vscodeDev` (`post_handle_delivery`: `delivery_read`, then `Post` or `Retry`, as `finish_read` does). It reads into buffers otherwise.
- `on_message(VscodeOpen)`:
  - refused unless the adapter is `vscodeDev` and `open_command` passes (log `open-refused`);
  - if a command is running, sets `open_again`;
  - otherwise runs one.
- When a command finishes:
  - success logs `vscode-open done`, and if `open_again` is set it clears it and runs the next;
  - failure logs `vscode-open-failed <reason>`, clears `open_again`, and shows the message box.

## Task 3: delete the spike

- Remove `spikes/vscode-webview2-handle`.
- Its plan and result report (`docs/spikes/2026-09-14-vscode-webview2-handle.md`) stay as history.

## Task 4: end to end in CI

**Local, deterministic** (in `e2e/bridge.mjs`):

- A third app, `bridge-c`, uses `--adapter vscodeDev` for `https://localhost:8445/`.
- That port serves a stand-in for vscode.dev:
  - a `.monaco-workbench` element;
  - a palette input that Ctrl+Shift+P shows and focuses, whose `aria-activedescendant` entry echoes the typed text;
  - Enter calls `showOpenFilePicker({multiple:false})` and records the file's name and text.
- Cases:
  1. **Launch with a file:** it opens through the command (`vscode-open done`). The page writes new text through the handle's `createWritable()`. The file on disk changes, its creation time is unchanged, and no `.crswap` file is left.
  2. **Two files from a second launch:** both open, in order, through two commands.
  3. **Gate 8 for handles and keys.** On `https://127.0.0.1:8445/`, `webwrap-ready` and `webwrap-vscode-open` are refused, and no key event reaches the page. A file forwarded meanwhile is delivered only after returning to `https://localhost:8445/`.
  4. **A palette that never appears:** the `?nopalette` page has none. The command fails (`vscode-open-failed`) and is not retried. This case runs last, and the app is then killed with its message box.

**Live** (`e2e/vscode-live.mjs`, its own CI step):

- An app generated for `https://vscode.dev/` with `--adapter vscodeDev` is launched with `live-one.md` (`before\r\n`).
- The test waits for a tab named `live-one.md` whose editor shows `before`.
- It sends Ctrl+A, inserts `after\r\n`, and sends Ctrl+S, as a user would.
- The file must become exactly `after\r\n`, with no copy in Downloads.
- A second launch with `live-two.md` must open a second tab, and Ctrl+S must change that file too. This is gate 6 on the real site.

**Workflow**

- Generates `bridge-c` and the live app.
- Adds the `vscode-dev` acceptance app (`--url vscode.dev --adapter vscodeDev --assoc .md text/markdown "Markdown document"`) for the PC run.
- Adds `schedule: cron: "17 5 * * 1"`.

## Task 5: evidence, review, handover

- `docs/evidence/2026-11-20-vscode-dev-adapter.md`.
- An independent review of the branch diff, with the findings fixed or recorded.
- Update `HANDOVER.md` and the spec status.
- Append the D2 hands-on checks to `docs/windows-runs/NEXT.md`:
  - double-click-style open, by dropping on the exe;
  - Ctrl+O through the native dialog, then Ctrl+S;
  - a second file opening as a new tab.
