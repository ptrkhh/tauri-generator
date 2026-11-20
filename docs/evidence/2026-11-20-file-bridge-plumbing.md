# Evidence: file bridge runtime plumbing (sub-project D, phase D1)

Date: 2026-11-20. Branch `feat/file-bridge-plumbing`. Spec: `docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md` (section D1). Plan: `docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md`.

This closes:

- **gate 6:** a second file opened while the app runs reaches that instance;
- **gate 7:** with two apps running concurrently, an open request reaches only the matching app;
- **gate 8:** file access is unavailable after navigating to an untrusted origin;
- **B's four review findings** (B evidence, section 6).

**Not proven by D1:** the save halves of gates 4 and 5 (D2, D3), and their "Open with" and double-click halves (E). D1 delivers files to any page that speaks the bridge protocol. No real site does yet; the adapters in D2-D4 will.

## 1. Environments

- **Linux (x86_64):** `rustc 1.100.0`, `cargo-xwin 0.23.1`, Wine 11.0.
  - `webwrap-bridge` and `webwrap-spec` test natively.
  - The runtime's unit and integration tests run under Wine. Wine has no WebView2, so nothing involving the window runs there.
- **Windows (authoritative):** `.github/workflows/windows.yml` on `windows-latest` (x64) and `windows-11-arm` (ARM64), with WebView2 Runtime 157.0.4341.x.
  - The new end-to-end step generates two apps with the CI-built generator: `bridge-a` for `https://localhost:8443/` and `bridge-b` for `https://localhost:8444/`.
  - `e2e/bridge.mjs`, a dependency-free Node 22 script, serves a test page over HTTPS. It drives each app's page through the DevTools protocol, and reads the apps' `webwrap: <event>` stderr lines to observe native decisions.

## 2. Automated results

| Suite | Linux | x64 CI | ARM64 CI |
| --- | --- | --- | --- |
| `webwrap-bridge` (IPC, paths, session state machine) | 24 | 23 | 23 |
| `webwrap-spec` 31 unit (now with the frame verdict) + 8 vectors | 39 | 39 | 39 |
| `webwrap-runtime` unit (arguments, single instance, save, delivery read) | 14 (Wine) | 14 | 14 |
| `webwrap-runtime` integration (`inspect_self.rs`) | 9 (Wine) | 9 | 9 |
| `webwrap-pack`, `webwrap-generate` (unchanged from C) | 24 | 24 + running-app probe | 24 + running-app probe |
| clippy `-D warnings`, all five crates, both targets | pass (cross) | pass | pass |
| End-to-end, `e2e/bridge.mjs` | n/a | 10 of 10 | 10 of 10 |

On Windows the bridge has one test fewer: the symlink test is Unix-only.

**CI run:** `44716057242` at `9f2ebb6`, the final code commit. Every row above passed on both runners.

**End-to-end cases.** Each names the stderr line or page state that proves it:

1. **A file given at launch** arrives only after the page says `webwrap-ready`, with the same SHA-256. Proof: `page-ready 1 queued`, `delivered start.txt`.
2. **Gate 6:** a second launch with a file exits 0, and the running app delivers it.
3. **Gate 7:** with both apps running, B's file reaches B (`delivered for-b.txt` from bridge-b only), and A's page never lists it.
4. **Save:** saving through the protocol changes the original file on disk.
5. **Large files:** a 160 MiB file opens with a matching SHA-256, and a 150 MiB save writes back byte for byte. No `.webwrap-*` files are left.
6. **Failed save:** saving to a read-only file returns `ok:false`, with a generic error that contains no path. The file is unchanged and nothing is left behind.
7. **Pipe messages:** an oversized length, malformed JSON, a wrong `appId`, a wrong version, and a relative path are each rejected with ack `0`, each with its own `ipc-rejected` line. A normal launch afterwards still works.
8. **Gate 8:**
   - Navigating to `https://127.0.0.1:8443/` logs `revoked 5 file tokens`.
   - There, `ready` is refused, a save of an old token is refused, and a file opened meanwhile is not delivered.
   - Back on `https://localhost:8443/`, the old token's save is refused as revoked, and `ready` delivers the queued file.
9. **Pop-ups:** a `window.open` from the page's own timer is denied (`popup-denied`). A clicked `target=_blank` link opens externally (`popup-external`).
10. **Schemes:** an iframe `search-ms:` is cancelled (`frame-blocked`), and a top-level one too (`nav-blocked`); the page stays where it was.


**Mutation checks:** each key session rule was removed in turn, and a test failed each time:

- revocation on an untrusted document;
- the generation check on a finished read;
- the source check on save;
- delivering before the page is ready.

## 3. Library behaviour verified, not assumed

- **wry's message handler drops object messages.** wry's own `WebMessageReceived` handler runs first and calls `TryGetWebMessageAsString` with `?`. In CI, object messages never reached the bridge. Page-to-host messages are therefore JSON text posted as a string, as both spikes already did.
  - Tauri also receives these strings as failed IPC calls. It logs one `console.error` in the page per message (`tauri-2.11.5/src/ipc/protocol.rs:428`). That is harmless.
- **WebView2 ignores `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` when the app passes arguments.** wry always passes them, so CI's browser process started with wry's arguments only; the run logged its command line.
  - Tests now use the WebWrap-only `WEBWRAP_TEST_BROWSER_ARGUMENTS`, which the runtime appends to wry's defaults. Nothing changes when it is unset.
  - WebView2's generic name stays ignored, as the re-review advised: an account-wide setting made to debug another app would otherwise open a DevTools port on every generated app.
- **WebView2 treats DevTools-evaluated code as user-initiated.** A `window.open` evaluated through DevTools was reported as user-initiated and opened externally, much as Chrome allows console pop-ups. The pop-up test therefore uses a page-script timer, which is denied.
- **wry's navigation handler and Tauri's pop-up default.**
  - wry calls `SetCancel(!allow)`, and Tauri's hook allows an unparseable URL (`tauri-runtime-wry-2.11.4/src/lib.rs:4899`). The runtime's own handlers are added after wry's and only ever cancel.
  - With no `on_new_window`, wry sets `Handled(true)` synchronously (`wry-0.55.1/src/webview2/mod.rs:782`), so every pop-up is denied before the runtime's handler decides whether to open it externally.
- **`ReplaceFileW` without a backup name can lose the original.** On `ERROR_UNABLE_TO_MOVE_REPLACEMENT` the original no longer exists under its name. `save::replace` always passes a same-folder backup name, and restores from it on `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`. Windows CI confirmed the creation time is kept; Wine's `ReplaceFileW` does not keep it, so that one assertion is skipped under Wine.
- **Windows spells some paths two ways.** Canonicalizing expands 8.3 short names (`RUNNER~1`). `std::fs::canonicalize` returns `\\?\` paths, and `dunce` returns the plain form, keeping the prefix only over 260 characters. Two tests were wrong about this, and CI caught both.

## 4. Deviations from the spec

The plan's "Clarifications" section records each, with its reason:

1. **No mutex.** The pipe, created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, is the lock.
2. **A no-file second launch** brings the running window to the front.
3. **The acknowledgement byte** is `1` for accepted and `0` for rejected.
4. **Queued files survive an untrusted navigation**, because they were never exposed. Delivered tokens and uncommitted saves are revoked. A write the trusted page committed before leaving still completes.
5. **Frames may load `blob:` and `data:`.**
6. **`fileTier` moves to D4.**
7. **Window drops reach the site** (`disable_drag_drop_handler`).
8. **A second launch checks the pipe's owner** before sending anything.
9. **Opened paths are canonical.**
10. **Tests pass browser arguments through `WEBWRAP_TEST_BROWSER_ARGUMENTS`.**

Two more come from the protocol and the review:

- **Revocation is keyed on `ContentLoading`, not `NavigationStarting`.** A navigation that becomes a download, or is cancelled, leaves the trusted page in place.
- **`NavigationStarting` still closes the check-then-post gap.** Nothing is posted while an allowed navigation to an untrusted origin is pending.

## 5. Review

An independent review of the branch diff found:

- **one medium-high issue:** another local user could create the app's pipe first and receive its file paths;
- **two medium issues:** closing the window during a save killed the write; a failed save's error sent local paths to the page;
- **several low ones.**

| Finding | Outcome |
| --- | --- |
| Pipe squatting by another user | Fixed: owner set and verified; a foreign pipe means run standalone |
| Closing during a save kills the write | Fixed: close waits for in-flight writes |
| Save errors sent local paths to the page | Fixed: generic error to the page; the path only in the message box |
| Check-then-post race on navigation | Fixed: pending untrusted navigations block posting |
| A late commit could fill a later save | Fixed: per-save nonce |
| Pipe server stuck after a failed connect | Fixed: disconnect before retrying |
| One deadline covered both connect and ack | Fixed: separate deadlines |
| Long paths could not be saved | Fixed: canonical paths keep `\\?\` over 260 characters |
| Failure message claimed the original was untouched | Fixed: the save module states what happened |
| Paths not canonicalized (symlinks) | Fixed |
| A launch while the first window is closing | **Known limit**, below |

The same reviewer then re-reviewed the fix commits. It confirmed the fixes for the save nonce, the generic save errors, the pipe server's disconnect, and the separate deadlines, and found five more problems:

| Re-review finding | Outcome |
| --- | --- |
| A save that fails while the window waits to close was never reported: the window closed and took the message box with it | Fixed: a failed write cancels the pending close |
| A navigation that never reports its end held deliveries until the next document, and save results were dropped meanwhile | Fixed: results are gated on trust only; holds expire after 30 s and are retried by a timer |
| A pipe that denies us access failed the launch instead of running standalone | Fixed: access denied means "not ours" |
| Canonicalizing fails on some volumes (RAM disks), and saves just under 260 characters failed | Fixed: fall back to an absolute path; `ReplaceFileW` always gets `\\?\` paths |
| Honouring WebView2's generic arguments variable lets an account-wide setting open a DevTools port on every app | Fixed: a WebWrap-only test variable |
| A low-integrity process of the same user can create a pipe owned by our SID | **Known limit**, below |

The reviewer also checked that no file delivery can be posted during a pending untrusted navigation, and that the owner check works when the app is elevated.

## 6. Known limits

1. **A launch while the first window is closing can be lost.** The closing app's pipe thread may still acknowledge it, so the second launch exits 0 and the file never opens. The window is a fraction of a second at shutdown.
2. **A same-user client that connects and never finishes its message stalls the pipe thread.** Later launches then time out with a message. This is marked `shortcut:` in `single_instance.rs`.
3. **Nothing shows yet when no page accepts a file.** A file stays queued until a trusted page says `webwrap-ready`, and no real site does until D2-D4. D4's "unsupported" message covers this.
4. **Reading or writing the page's shared memory is not protected against the page changing it mid-write.** A page can only corrupt its own save this way.
5. **The `popup-external` decision trusts WebView2's `IsUserInitiated`.** Code run through DevTools counts as user-initiated (section 3); a page script does not.
6. **The pipe owner check does not look at integrity levels.** A sandboxed, low-integrity process of the same user could create the pipe first with our SID as owner. It would then receive the paths of files opened by a second launch and could take the foreground. Checking the pipe's mandatory label would close this. Other users are already kept out.
7. **Untested paths:** closing the window during a save, and a navigation that ends without a document, have no automated test. The long-path save test only bites on real Windows, because Wine does not enforce the 260-character limit.

## 7. Hands-on PC checks

Queued in `docs/windows-runs/NEXT.md`:

- a second launch, including from a minimized window and with a dropped file, brings the running window to the front;
- dropping a file on the window reaches the site;
- the missing-file message box wording;
- pop-ups still open in the browser.

The results will be recorded in `docs/windows-runs/<date>-results.md`.
