# Evidence: webwrap spec and minimal runtime (sub-projects A and B)

Date: 2026-11-19. Branch `feat/webwrap-runtime`. This covers sub-projects A and B only. It closes release gates 11 and 14 and nothing else: **gates 1, 4, 5, 6, 7, and 8 are not proven by this work.**

Status: complete. Automated checks ran on x64 and ARM64 Windows in CI. The hands-on check ran on an ARM64 laptop for both architectures (`docs/windows-runs/2026-11-20-results.md`).

## 1. How this was built and tested

Code was written and cross-compiled on Linux, and run on real Windows in GitHub Actions. No local Windows build was used.

- Linux host (x86_64): `rustc 1.100.0 (0ef1bc386 2026-11-09)`, LLVM 23.1.5, `cargo-xwin 0.23.1` (MSVC CRT/SDK via xwin), Wine 11.0 for fast local test runs. Wine has no WebView2, so it never exercised the window path.
- Windows, authoritative: `.github/workflows/windows.yml` on `windows-latest` (`x86_64-pc-windows-msvc`) and `windows-11-arm` (`aarch64-pc-windows-msvc`), each running natively. The exact image versions are in each run's "Set up job" log.

## 2. Automated results

| Suite | x64 (CI) | ARM64 (CI) | x64 (Wine) |
| --- | --- | --- | --- |
| `webwrap-spec` unit + vectors | 30 + 8 pass | 30 + 8 pass | n/a (Linux: 30 + 8 pass) |
| `webwrap-runtime` unit | 3 pass | 3 pass | 3 pass |
| `webwrap-runtime` integration (`tests/inspect_self.rs`) | 9 pass | 9 pass | 9 pass |
| `cargo clippy -p webwrap-spec -p webwrap-runtime --all-targets -- -D warnings` | pass | pass | pass (cross) |
| `cargo fmt --all -- --check` | pass | pass | pass |

CI runs: `44580831965` (Task 5), `44581797255` (Tasks 6-8), `44583078554` (final code commit `acaf4da`, which also smoke-launches both specialized release apps, section 4). Counts above are read from the CI logs.

## 3. Library behaviour verified against source, not assumed

- **WebView2 data folder (Task 7).** `tauri` 2.11.5 has `WebviewWindowBuilder::data_directory` (`src/webview/webview_window.rs:1024`). `wry` 0.55.1 passes it as `userDataFolder` to `CreateCoreWebView2EnvironmentWithOptions` (`src/webview2/mod.rs:287-345`). Used it rather than `WEBVIEW2_USER_DATA_FOLDER`, which child processes would inherit. Without it, Tauri forces `%LOCALAPPDATA%\<tauri.conf identifier>` on Windows (`src/manager/webview.rs:534-545`). Every generated app shares the identifier `dev.webwrap.runtime`, so they would all share one storage folder.
- **New-window hook (Task 8).** `tauri` 2.11.5 has `on_new_window` (`src/webview/webview_window.rs:315`), so it is wired up now rather than deferred to D. An http/https pop-up goes to the system browser via `ShellExecuteW`. Every pop-up is denied inside the app. Without a handler, Tauri would open pop-ups as in-app windows that bypass `on_navigation`.
- **Window icon (deviation from plan).** `tauri-codegen` always compiles a default window icon (`src/context.rs:211-243`), and Tauri applies it to every window. tao's window class carries no icon. So a generated app would have shown the template's icon, not its own. editpe writes the replacement icon group under the name `MAINICON` and removes the template's group 32512 (`editpe-0.2.4/src/resource.rs:343`). The runtime therefore loads `MAINICON` from its own image and sends `WM_SETICON` (big and small). The compiled-in default is the same 1x1 transparent placeholder (`crates/webwrap-runtime/icons/icon.ico`), so a missing `MAINICON` shows as a blank icon in the GUI check.
- **`windows` 0.61.3 call shapes used.**
  - `GetModuleHandleW(PCWSTR::null()) -> Result<HMODULE>`.
  - `FindResourceExW(Option<HMODULE>, type, name, u16) -> HRSRC`, checked with `HRSRC::is_invalid()`.
  - `SizeofResource(Option<HMODULE>, HRSRC) -> u32`.
  - `LoadResource(Option<HMODULE>, HRSRC) -> Result<HGLOBAL>`.
  - `LockResource(HGLOBAL) -> *mut c_void`.
  - `MessageBoxW(Option<HWND>, …)`.
  - `ShellExecuteW(Option<HWND>, …, SHOW_WINDOW_CMD) -> HINSTANCE`.
  - `LoadImageW(Option<HINSTANCE>, …) -> Result<HANDLE>`.
  - `SendMessageW(HWND, u32, Option<WPARAM>, Option<LPARAM>)`.

  The plan's `Some(module.into())` is a no-op conversion (the parameter is already `Option<HMODULE>`). It became `Some(module)`.
- **Cargo 1.100 build-dir layout (deviation from plan).** Test binaries now live in `target/<triple>/<profile>/build/<pkg>/<hash>/out/`, with no `deps/`. The plan's `build_dir()` would have looked there. It now uses `env!("CARGO_BIN_EXE_webwrap-runtime")`, which a probe showed still points at the uplifted `target/<triple>/<profile>/` directory.
- **Version strings.** `fileDescription` reads back `Photopea`, not `WebWrap Runtime`, on a real Tauri-built PE. So the `strings[0]`-only limitation in `pe.rs` did not bite here.

## 4. Gate observations

- **Gate 11 (separate WebView2 data folders).** `gate_11_two_apps_use_separate_webview2_data_directories` shows that Photopea and Twitter resolve to `%LOCALAPPDATA%\WebWrap\873756fbe97a1384` and `…\2fa5e3f40150278f`. The CI smoke step launches both release apps on real WebView2 and checks:
  - each `…\<appId>\EBWebView` was created;
  - `%LOCALAPPDATA%\dev.webwrap.runtime\EBWebView`, the shared fallback, was not.
- **Gate 14 (reject non-HTTPS).** `AppSpec` validation rejects a non-https `launchUrl` (covered by `webwrap-spec`). `gate_14_navigation_policy_is_enforced_by_the_real_binary` shows the shipped binary's policy decisions: https allowed; http, `file:`, `javascript:`, and garbage blocked.
  - **Limit, stated plainly:** that test exercises the policy through `--inspect-self`, not through the live window. Removing the `.on_navigation(...)` or `.on_new_window(...)` wiring would leave every automated test green. The wiring is covered by code review and hands-on check D only.

## 5. Hands-on Windows check

Full record: `docs/windows-runs/2026-11-20-results.md`.

- Machine: Windows `10.0.26300.9711` on a Snapdragon X Plus (ARM64). WebView2 Runtime `157.0.4341.46`.
- Apps: release builds from CI run `44583078554` at `acaf4da`. Checked natively (`aarch64`) and under x64 emulation (`x86_64`).

| Check | ARM64 | x64 (emulated) |
| --- | --- | --- |
| Photopea: title `Photopea`, editor loads, title-bar and taskbar icon is the supplied red square | PASS | PASS |
| `WebWrap\873756fbe97a1384\EBWebView` created | PASS | — |
| Twitter alongside: title `Twitter`, page loads, blue icon, its own `EBWebView`; `dev.webwrap.runtime` never created | PASS | — |
| A new-tab link opens in the default browser, no app window | PASS | — |
| Signing in to one app does not affect the other | SKIPPED (no account used) | — |
| Unspecialized template: `WebWrap` message box "…never specialized by the generator", exit 1 | PASS | PASS |
| `--inspect-self` exits 0, `runtimeArch` matches the build | PASS | PASS |

- **Step 5a (`about:blank`).** Photopea and Twitter both load, so blocking `about:blank` in `on_navigation` does not break startup. No exception was added.
- **Sign-in check skipped.** Storage separation is still shown at the folder level by the hands-on check and by the CI smoke step.

## 6. Known gaps handed to sub-project D

These came from an independent review of this change. Each needs raw WebView2 access (`with_webview`), which D already plans for the file bridge.

1. **Pop-ups without a user gesture.** wry 0.55.1 does not pass WebView2's `IsUserInitiated` to `on_new_window`. So any reachable https page can open system-browser tabs without a click. Fix in D: read `IsUserInitiated` from `ICoreWebView2NewWindowRequestedEventArgs` and drop the pop-up when it is false.
2. **Unparseable URLs are allowed.** `tauri-runtime-wry` 2.11.4 allows a navigation whose URL the `url` crate cannot parse (`unwrap_or(true)`, `src/lib.rs:4899-4905`). No bypass URL was found; this is a suspicion only.
3. **Iframes are not covered.** Iframe navigations and `LaunchingExternalUriScheme` are not covered by `on_navigation`. WebView2's own prompt is the only guard against `search-ms:`-style external schemes. Fix in D: handle `LaunchingExternalUriScheme` and cancel it.
4. **Dragged files fail silently.** A file dragged onto the app, or any other positional argument, exits 2 with nothing on screen. D adds file arguments.
