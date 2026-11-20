# Evidence: vscode.dev adapter (sub-project D, phase D2)

Date: 2026-11-20. Branch `feat/vscode-adapter`. Spec: `docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md` (section D2). Plan: `docs/superpowers/plans/2026-11-20-vscode-dev-adapter.md`.

**What it closes:** the save half of **gate 5**. A file handed to an app generated with `--adapter vscodeDev` opens in live `vscode.dev`, and Ctrl+S changes the original file on disk, with no download. That holds for a file given at launch and for files from later launches.

**What it does not prove:**

- **Gate 5's "Open with" and double-click half** waits for registration (E).
- **Ctrl+O through the native Windows dialog** cannot be driven by CI. It is a hands-on PC check (section 8).

## 1. Environments

As D1 (D1 evidence, section 1), plus:

- **vscode.dev:** the live site, workbench build `8a14f0cc1b08d531eb83255ea3764ae22aa932b5` on 2026-11-20.
- **CI's WebView2 Runtime:** 156.0.4314.58 on x64 and 157.0.4341.60 on ARM64.
- **Before CI:** the adapter script was also tried against live vscode.dev in local headless Chromium 1255 (Linux). An origin-private file stood in for WebView2's handle.

## 2. Automated results

| Suite | Linux | x64 CI | ARM64 CI |
| --- | --- | --- | --- |
| `webwrap-bridge` (adds: open-failed message, delivered names, handle refusal) | 26 | 25 | 25 |
| `webwrap-spec` | 39 | 39 | 39 |
| `webwrap-runtime` unit (adds: the adapter script's origins) | 15 (Wine) | 15 | 15 |
| `webwrap-runtime` integration | 9 (Wine) | 9 | 9 |
| `webwrap-pack`, `webwrap-generate` | 24 | 24 + probe | 24 + probe |
| clippy `-D warnings`, all crates, both targets | pass (cross) | pass | pass |
| End to end, `e2e/bridge.mjs`: D1 (10) plus the vscodeDev stand-in (5) | n/a | 15 of 15 | 15 of 15 |
| End to end, `e2e/vscode-live.mjs`, against live vscode.dev | n/a | 3 of 3 | 3 of 3 |

**CI run:** `44778718827` at `1f6d790`, the final code commit. Every row above passed on both runners.

**Stand-in cases.** `bridge-c` is generated with `--adapter vscodeDev` for a local page. The page matches vscode.dev where the adapter relies on it:

- a constructed Ctrl+Shift+P opens a palette, which closes when it loses focus;
- its input filters 300 ms later;
- a click on "File: Open File..." calls `showOpenFilePicker({multiple:false})`, adds a tab, and 300 ms later activates it and focuses an editor;
- like vscode.dev restoring editors, it takes focus once, during the first palette.

The cases:

1. **Refused files.** A file is refused with a message box (`open-failed … vscode.dev cannot use files …`) and never reaches the page when it is:
   - in `%LOCALAPPDATA%\Temp`, `%APPDATA%`, or `%ProgramData%`;
   - on a network share (`\\localhost\D$\…`);
   - at a path over 260 characters.

   Files in Documents and in the home folder open, save through their handle, and leave no `.crswap` behind.
2. **A file given at launch** opens even though focus was taken mid-run, which proves the restart. It saves through its handle.
3. **Two files from one later launch** open in order, after the earlier ones. The editor stays empty, and the page sees no trusted key event.
4. **Gate 8 for handles.**
   - On `https://127.0.0.1:8445/`, both `webwrap-ready` and `webwrap-open-failed` are refused, and no message reaches the page.
   - A file forwarded meanwhile opens only after returning to `https://localhost:8445/`.
5. **No palette.** After 30 s of tries, the user is told once (`open-failed no-palette.md`).

**Live cases.** `vscode-live` is generated for `https://vscode.dev/` with `--adapter vscodeDev`. DevTools input drives it as a user would: Ctrl+A, typed text, Ctrl+S.

1. **A file given at launch** opens in a tab showing `before`.
2. **Ctrl+S** turns the original into exactly `after\r\n`, with no copy in Downloads.
3. **Two files from one later launch** open in two new tabs. Ctrl+S on the front one changes only that file.

## 3. What vscode.dev does, checked

- **No `launchQueue` consumer** in the workbench bundle.
- **Open File asks for one file per run.** `pickFileAndOpen` calls `showOpenFilePicker({multiple:false})`.
- **Constructed events (`isTrusted: false`).**
  - A constructed Ctrl+Shift+P opens the command palette: the keybinding service and quick input never check `isTrusted`.
  - A constructed **Enter** is ignored. The keybinding dispatcher received it, matched nothing, and left `defaultPrevented` false.
  - A constructed click on the highlighted row runs the command.
- **What the adapter waits for:**
  - the active tab is `.tab.active`, with its name in `.label-name`;
  - an opened file's editor holds focus (`.monaco-editor`).

These are DOM details of a hosted site. The weekly scheduled run guards them.

## 4. WebView2 behaviour, measured

**Where a site may use a handle.** CI run 44771559248 gave the same results on both architectures. `CreateWebFileSystemFileHandle` made a read-write handle for a file in each folder, and the page then read and wrote it:

| Folder | Read | Write |
| --- | --- | --- |
| Runner temp outside AppData, Documents, home folder | ok | ok |
| `%LOCALAPPDATA%\Temp`, `%APPDATA%`, `%ProgramData%` | `SecurityError` | ok |
| Outside AppData, 322 characters (`\\?\` path) | `SecurityError` | `InvalidStateError` |
| Network share, `\\localhost\D$\…` (run 44776082951) | `SecurityError` | not reached |

- **The AppData results are not even consistent.** An earlier run read a `%TEMP%` file and refused the write.
- **The runtime therefore refuses such files up front** for this adapter, with a message box saying why and suggesting a copy in Documents (plan clarification 6).
- **Program Files and Windows are refused too, unmeasured,** because Chromium blocks them as well.
- **The spike passed with a `%TEMP%` file** on WebView2 152 in September. Either the runtime has changed since, or the spike's `\\?\` path form slipped past the check. The 322-character case shows `\\?\` paths are refused now.

**A save through a handle does not keep the file's creation time.** This is recorded by case 2 (stand-in), on both architectures. Chromium writes a `.crswap` file and moves it over the original, unlike D1's `ReplaceFileW`. No `.crswap` was left behind in any case.

## 5. Deviations from the spec

The plan's clarifications record each with its reason. The two that change the spec's design:

1. **The page script runs Open File, not the runtime.**
   - The spec followed the spike: DevTools key events and `insertText` from the native side.
   - The first review showed those land wherever focus is when they arrive, so vscode.dev moving focus could type into the user's file.
   - Constructed events dispatched on a checked element cannot type anything. The runtime now sends no input at all, which also removed its DevTools chain and its command state.
2. **Files in AppData-type folders, on network shares, or with long paths are refused for this adapter** (section 4). The spec assumed any file could be opened.

## 6. Review

An independent reviewer read the first version and the redesign.

**First version (native DevTools chain):**

| Finding | Outcome |
| --- | --- |
| The next file's run overlaps vscode.dev opening the previous one, and can type into it | Fixed: page-side runs, constructed events only, one file at a time |
| Focus check and typing are separate DevTools calls | Fixed: the runtime sends no input |
| The command state can stick | Fixed: no host state. Page runs are bounded and reset in `finally` |
| The failure box claimed a file was waiting when it was lost, and some files were lost silently | Partly fixed: the claim is now true. A reload before vscode.dev takes a handle still loses it (known limit 3) |
| Keys can reach a focused cross-origin frame | Fixed: constructed events stay in the top-level document |
| Tests could not see the overlap; nothing tested a run stopped by navigation | Stand-in now delays, closes on blur, and takes focus once. The navigation case no longer applies |

**Redesign:**

| Finding | Outcome |
| --- | --- |
| A run fails if vscode.dev moves focus mid-run, for example at launch | Fixed: the run restarts within its 30 s. Stand-in case 2 proves it |
| The next run started on a substring tab match | Fixed: waits for the new active tab with that exact name and a focused editor |
| After a failure, later files stall silently | Fixed: every waiting file is named. The user's own Open File restarts the queue |
| A throw could leave the queue stuck | Fixed: `try`/`finally` |
| The picker handed queued files to any caller during a run | Fixed: only the run's own click gets one |
| Network shares were refused as "over 260 characters" | Changed: CI then measured that WebView2 refuses a share's handle anyway. Shares are now refused with their own message, and the length is counted without the prefix. Stand-in case 1 covers both |

The reviewer found no remaining way for a handle or input to reach an untrusted or different document.

## 7. Known limits

1. **A save through a handle does not keep the creation time.** Alternate streams and ACLs were not measured. WebView2 performs this write (section 4).
2. **Some files are refused** for this adapter, with a message suggesting a copy:
   - files in AppData, ProgramData, Program Files, or Windows;
   - files on network shares and mapped drives. This includes Documents redirected to a server, common on managed PCs. Only the runner's administrative share was measured;
   - files whose path is 260 characters or more.

   Photopea and generic sites (D3, D4) save through the runtime and are unaffected.
3. **A file is lost silently if vscode.dev reloads after the handle was posted but before Open File took it.** It is marked delivered and is not sent again. The user opens it again.
4. **The adapter depends on vscode.dev's DOM and an English command name:**
   - the `.monaco-workbench`, `.tab.active`, `.label-name`, and `.monaco-editor` classes;
   - the palette's `>` prefix and `aria-activedescendant`;
   - the text "Open File...".

   vscode.dev with another display language is untested. A change fails with a message box, never silently, and the weekly live run should catch it first.
5. **While a run waits for vscode.dev, the user's own Ctrl+O** can race it for the file in rare cases. The other caller then gets the native picker.
6. **Ctrl+O through WebView2's native picker is unverified by CI** (section 8, hands-on).

## 8. Hands-on PC checks

Queued in `docs/windows-runs/NEXT.md` with D1's:

- dropping a Markdown file on the `vscode` app opens it in vscode.dev;
- Ctrl+S saves it;
- a second file opens as a new tab;
- Ctrl+O through the native dialog opens a file, and Ctrl+S saves it;
- a file in `%TEMP%` gets the refusal message.
