# WebWrap handover

Updated 2026-11-20. The spikes and sub-projects A, B, and C are finished, and D's first phase (D1) is done. A fresh Codex/Claude session should start here:

- Work on `main` in the repository root. The `spike/photopea-windows-writeback` branch and its `.worktrees/photopea-windows-writeback` linked worktree were merged and removed; do not look for them.
- Current code HEAD: check `git log -1 --format=%H` rather than trusting a hash recorded here, which goes stale as soon as the next commit lands.
- Delegated workers had accidentally created duplicate PE commits on `main` on a parallel lineage. Those commits are preserved in history. The merge resolved its three add/add conflicts (`src/main.rs`, `src/pe.rs`, `tests/pe_specialization.rs`) to the reviewed branch lineage, so the merged tree is byte-identical to the branch tree. `main` was not reset or rewritten, and must not be.

## Product decisions already made

The eventual personal, non-commercial tool is a local `tauri_generator.exe`, not a hosted website or GitHub Actions service. It should accept a website such as `twitter.com` and produce self-contained `twitter-x64.exe`, `twitter-arm64.exe`, and `twitter.apk` files (no JSON/ICO sidecars). Regenerating the same URL updates that app. Since 2026-11-20, a different URL given the same file name overwrites the old app. Files the generator did not make are never overwritten. The generator asks for the output folder every time. Icon discovery is automatic with an optional gallery override.

Windows v1 must support file associations broadly, at minimum Photopea for `.psd` and vscode.dev (or a compatible alternative) for `.md`. Double-clicking a file should launch the matching generated app. Ctrl+S must write changes back to the original local file, and Ctrl+O should continue to work. Custom DNS is an optional generation setting: system DNS is the default; otherwise ask for a complete HTTPS DoH URL, never silently bypass it, and fail closed if it cannot be honored.

## Completed work

### Photopea/WebView2 write-back spike

This bounded spike is complete and documented in [`docs/spikes/2026-09-15-photopea-windows-writeback.md`](docs/spikes/2026-09-15-photopea-windows-writeback.md). It proves trusted local open/save routing, active-document source binding, atomic PSD replacement, and destination-preserving failures. Native verification covered 17 Photopea tests and 7 VS Code tests, plus a live executable/CDP save and source-mismatch probe. It does not implement the generator, packaging, associations, Android, DNS, or large-file transport.

Relevant commits: `b601cba`, `3aa5419`, `8a3526d`, `f4e8d5d`, `c15c083`, `554329e`.

### Windows PE specialization spike

Plan: [`docs/superpowers/plans/2026-09-15-windows-pe-specialization-spike.md`](docs/superpowers/plans/2026-09-15-windows-pe-specialization-spike.md). Ledger (ignored, local only): `.superpowers/sdd/2026-09-15-windows-pe-specialization-spike/progress.md`.

The spike is Windows x64 MSVC-only and uses `editpe` to copy a precompiled PE, embed a validated `WEBWRAP_APP_SPEC` `RT_RCDATA` resource, replace the main icon, update version strings, read everything back, and atomically publish one executable with `MoveFileExW`.

| Task | State | Commits |
| --- | --- | --- |
| 1. Scaffold/parser/validators | Complete; reviews passed | `271da38`, `41132ef`, `8e1f059` |
| 2. Spec/icon/version resources | Complete; review passed | `6226303`, `e689bbd` |
| 3. Staging/atomic publish/CLI | Complete; re-reviewed at `b4c537b` and passed (spec compliant, task quality approved, no Critical/Important findings) | `2fd88ee`, `cdf50d5`, `b4c537b` |
| 4. Windows acceptance evidence | Complete; acceptance probe ran on Windows and evidence is committed | `12e27b1`, `3423904` |

Current implementation files:

- `spikes/windows-pe-specialization/src/lib.rs` — public command parser/types and unit tests.
- `spikes/windows-pe-specialization/src/pe.rs` — validation, resource editing/read-back, staging, and Windows publication.
- `spikes/windows-pe-specialization/src/main.rs` — `specialize` and `inspect` JSON CLI.
- `spikes/windows-pe-specialization/tests/pe_specialization.rs` — PE round-trip and transaction tests.
- `spikes/windows-pe-specialization/tests/cli_exit_codes.rs` — CLI exit-code contract tests (usage errors exit 2, runtime failures exit 1).

Verification run on the merged result with the native MSVC environment:

- `cargo test --workspace`: 37 passed, 0 failed (17 Photopea + 7 VS Code + 3 PE unit + 4 CLI exit-code + 6 PE integration; two doctest targets have 0 tests).
- `cargo clippy -p windows-pe-specialization-spike --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- CLI specialize/inspect emit JSON with `specBytes`, `iconPresent`, `fileDescription`, and `productVersion`. Malformed spec content exits 2; a missing spec *file* exits 1 — that distinction is a fixed regression and is now pinned by `tests/cli_exit_codes.rs`.

## Where things stand

The follow-on work was brainstormed on 2026-09-16. The Windows track is split into six sub-projects, each with its own spec, plan, and implementation cycle. APK generation, custom DNS, and code signing are deferred to phase 2.

| | Sub-project | Content | Gates | State |
| --- | --- | --- | --- | --- |
| A | Core spec and identity | schema, validation, canonicalization, identity, nav policy | — | **done**, reviewed, 38 tests |
| B | Minimal wrapper runtime | resource self-read, data-dir isolation, nav policy, launch | 11, 14 | **done**, reviewed, 12 tests, Windows-verified |
| C | Generator packaging core | port `pe.rs`, embed template, specialize, atomic publish | 1 (Windows half), 3 | **done**, reviewed, Windows-verified in CI |
| D | File bridge and adapters | D1 runtime plumbing; D2 `vscode.dev`; D3 Photopea; D4 generic sites | 6, 7, 8, save half of 4 and 5 | **in progress**: D1 done (6, 7, 8), reviewed, Windows-verified in CI; D2 next |
| E | Association registration | HKCU registration, "Open with", first-launch offer | double-click half of 4 and 5 | not specced |
| F | Generator UI and discovery | Tauri shell, discovery chain and gallery, association editor | 1 | not specced |

### Sub-project B, done 2026-11-20

Plan: [`docs/superpowers/plans/2026-09-16-webwrap-spec-and-minimal-runtime.md`](docs/superpowers/plans/2026-09-16-webwrap-spec-and-minimal-runtime.md). Evidence: [`docs/evidence/2026-09-16-webwrap-spec-and-minimal-runtime.md`](docs/evidence/2026-09-16-webwrap-spec-and-minimal-runtime.md). Branch `feat/webwrap-runtime`.

`crates/webwrap-runtime` is the precompiled template. It does four things:

- reads its `WEBWRAP_APP_SPEC` resource from the loaded image and revalidates it;
- isolates WebView2 storage under `%LOCALAPPDATA%\WebWrap\<appId>` via `WebviewWindowBuilder::data_directory`;
- enforces the navigation policy and sends http/https pop-ups to the system browser;
- shows the generator-embedded `MAINICON` on its window.

Every startup failure in run mode reaches a message box. The evidence doc lists four deviations from the plan, each verified against library source. It also lists four review findings handed to D: pop-ups without a user gesture, the wry navigation hook failing open on unparseable URLs, iframes and `LaunchingExternalUriScheme` not being covered, and a silent exit on a dropped file.

### Sub-project C, done 2026-11-20

Spec: [`docs/superpowers/specs/2026-11-20-generator-packaging-core-design.md`](docs/superpowers/specs/2026-11-20-generator-packaging-core-design.md). Plan: [`docs/superpowers/plans/2026-11-20-generator-packaging-core.md`](docs/superpowers/plans/2026-11-20-generator-packaging-core.md). Evidence: [`docs/evidence/2026-11-20-generator-packaging-core.md`](docs/evidence/2026-11-20-generator-packaging-core.md). Branch `feat/generator-packaging-core`.

- **`crates/webwrap-pack`** builds and validates the `AppSpec` and derives the slug from the host. It specializes both runtime templates in memory, stages and verifies them, then publishes `<slug>-x64.exe` and `<slug>-arm64.exe`.
  - It replaces generated apps, including a different website with the same name, per the user. It refuses anything else, plus running or read-only apps.
  - It is plain Rust, so its tests run on Linux.
- **`crates/webwrap-generate`** is the CLI generator, with both templates embedded when it is built with `WEBWRAP_TEMPLATE_X64` and `WEBWRAP_TEMPLATE_ARM64`. F wraps the same library in its UI.
- **`spikes/windows-pe-specialization` is gone.** B's tests specialize through `webwrap_pack::specialize_raw`.
- **Known limits** (evidence section 5):
  - a crafted executable at an output path can exhaust memory in `editpe`'s resource parser;
  - a killed generation leaves `.webwrap-*.tmp` files;
  - a file created in the instant before publishing can still be replaced.

### Sub-project D1, done 2026-11-20

Spec: [`docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md`](docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md) (D is four phases; this is D1). Plan: [`docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md`](docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md). Evidence: [`docs/evidence/2026-11-20-file-bridge-plumbing.md`](docs/evidence/2026-11-20-file-bridge-plumbing.md). Branch `feat/file-bridge-plumbing`.

- **`crates/webwrap-bridge`** (cross-platform, tested on Linux) holds:
  - the second-instance message codec;
  - path rules;
  - the token state machine: delivery only to a ready, trusted document; revocation on an untrusted document; one save per token, with a nonce.
- **The runtime:**
  - **Files and single instance.** Every non-`--inspect-self` argument is a file. A per-user, per-app named pipe makes the app single-instance, and a later launch forwards its files and brings the window forward.
  - **Moving bytes.** Files reach the page through WebView2 shared buffers, and saves come back the same way and are written with `ReplaceFileW`.
  - **Safety handlers.** They close B's four findings.
- **The page protocol** in the plan is the contract D2-D4 build on. Page-to-host messages are JSON text **posted as a string**: wry's own handler fails on objects.
- **CI end-to-end.** `e2e/bridge.mjs` drives two generated apps through the DevTools protocol and reads their `webwrap: <event>` stderr lines. The apps get their DevTools port from the test-only `WEBWRAP_TEST_BROWSER_ARGUMENTS`, because WebView2 ignores its own `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` once wry passes arguments.
- **Known limits:** evidence section 6.

## Immediate next step: D2, the `vscode.dev` adapter

Write D2's plan from the spec's D2 section. It ports `spikes/vscode-webview2-handle` onto D1's bridge, then deletes the spike:

- create a read-write `FileSystemFileHandle` for the token's path;
- post it to the page;
- drive vscode.dev's Open File command;
- verify Ctrl+S, and Ctrl+O through WebView2's own picker.

The live-site test joins the end-to-end step, and a weekly scheduled run guards against changes to `vscode.dev`. D3 (Photopea) and D4 (generic sites) follow.

Still open from earlier sub-projects:

- C's note that both architecture builds share one app's storage folder is handled: they share the pipe too.
- Ctrl+S on real sites needs the user's PC for a hands-on run (`docs/windows-runs/`).

## How work runs now (since 2026-11-19)

The user's Windows PC is a slow ARM64 laptop (Snapdragon X Plus), so it never compiles.

- **Linux session (this container, x86_64):** writes all code and cross-compiles both Windows targets:
  - `cargo xwin build|clippy|test --target x86_64-pc-windows-msvc|aarch64-pc-windows-msvc`, with `XWIN_ACCEPT_LICENSE=1`;
  - fast local test runs under Wine 11 (WineHQ). Wine 8 cannot load current Rust binaries: there is no `ProcessPrng` in `bcryptprimitives.dll`;
  - x64 only, and Wine has no WebView2, so nothing involving the window runs there.
- **GitHub Actions (`.github/workflows/windows.yml`):** the authority for Windows behaviour.
  - Runs fmt, both suites, and clippy `-D warnings` natively on `windows-latest` (x64) and `windows-11-arm` (ARM64).
  - Builds both release templates and the generator, generates the acceptance apps from `docs/windows-runs/fixtures/` icons, and uploads them, with the generator, as `acceptance-<triple>`.
  - Probes regenerating over a running app, and smoke-launches the native pair on real WebView2.
  - Runs the file-bridge end-to-end test (`e2e/bridge.mjs`, Node 22, no dependencies) against two generated apps on a local HTTPS test page. On failure it prints the apps' state and WebView2's command lines, and uploads a desktop screenshot.
- **The user's Windows PC:** hands-on checks only.
  - Paste the prompt in [`docs/windows-runs/README.md`](docs/windows-runs/README.md).
  - Queue a run by committing `docs/windows-runs/NEXT.md`. It reads that file, downloads the CI artifacts, asks the user what is on screen, and pushes `docs/windows-runs/<date>-results.md`.
  - With no `NEXT.md` it reports "nothing queued". Delete `NEXT.md` once its results are in.
- **Cargo 1.100 layout:** test binaries live under `target/<triple>/<profile>/build/<pkg>/<hash>/out/`, with no `deps/`. Locate built bins with `env!("CARGO_BIN_EXE_<name>")`, not from the test's own path.

## Resume guardrails

Read the full plan before editing. Continue the subagent-driven workflow with one implementer and a review checkpoint per task. Preserve the no-sidecar and failure-preservation invariants. Do not claim the product is complete from these spikes; they are packaging and file-write proofs, not the final generator.

Sub-project A closes no release gate. B closed gates 11 and 14. C closed the Windows half of gate 1 (reworded to two files per app) and gate 3. D1 closed gates 6, 7, and 8. Gates 4 and 5 belong to D2, D3, and E, and must not be described as proven before then. Gate 1 also needs F and the phase-2 APK.

One lesson from A's review worth carrying into B: every defect it found was a claim asserted in prose that no test asserted — a library behaviour taken on trust. The identity vectors and the documented `url`-crate behaviour, both of which *were* pinned by tests, held up under independent recomputation. When the plan states what a dependency does, make it an `assert`, not a note.
