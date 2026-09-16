# WebWrap handover

Updated 2026-09-16. Both spikes below are finished, reviewed, and merged. A fresh Codex/Claude session should start here:

- Work on `main` in the repository root. The `spike/photopea-windows-writeback` branch and its `.worktrees/photopea-windows-writeback` linked worktree were merged and removed; do not look for them.
- Current code HEAD: check `git log -1 --format=%H` rather than trusting a hash recorded here, which goes stale as soon as the next commit lands.
- Delegated workers had accidentally created duplicate PE commits on `main` on a parallel lineage. Those commits are preserved in history. The merge resolved its three add/add conflicts (`src/main.rs`, `src/pe.rs`, `tests/pe_specialization.rs`) to the reviewed branch lineage, so the merged tree is byte-identical to the branch tree. `main` was not reset or rewritten, and must not be.

## Product decisions already made

The eventual personal, non-commercial tool is a local `tauri_generator.exe`, not a hosted website or GitHub Actions service. It should accept a website such as `twitter.com` and produce self-contained `twitter.exe` and `twitter.apk` files (no JSON/ICO sidecars). Different canonical URLs coexist; regenerating the same URL updates that app rather than replacing another app. Icon discovery is automatic with an optional gallery override.

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

## Immediate next step: brainstorm/plan the follow-on work

The Windows PE specialization spike is finished. Task 3 was re-reviewed at `b4c537b` and passed. Task 4 ran the planned Windows acceptance probe and its evidence is recorded in [`docs/spikes/2026-09-15-windows-pe-specialization.md`](docs/spikes/2026-09-15-windows-pe-specialization.md) (commits `12e27b1`, `3423904`). A final whole-branch review then ran over the complete spike and returned "ready to merge: with fixes"; those fixes were applied in a dedicated fix-wave pass (`cc9d3af`, `545b0b3`, `4479b3f`, merged in `c6fe72e`; fix-wave report at `.superpowers/sdd/2026-09-15-windows-pe-specialization-spike/final-fix-report.md`, git-ignored/local only) — code cleanup, a stronger post-staging cleanup test, a CLI exit-code integration test, and a new probe substantiating the temp-file-removal claim in the evidence doc.

There is no more work scoped to this spike.

## Immediate next step: implement sub-project B, on Windows

The follow-on work was brainstormed on 2026-09-16 and the Windows track is now decomposed into six sub-projects, each getting its own spec, plan, and implementation cycle. APK generation, custom DNS, and code signing are deferred to phase 2.

| | Sub-project | Content | Gates | State |
| --- | --- | --- | --- | --- |
| A | Core spec and identity | schema, validation, canonicalization, identity, nav policy | — | **done**, reviewed, 38 tests |
| B | Minimal wrapper runtime | resource self-read, data-dir isolation, nav policy, launch | 11, 14 | **next** — specced and planned |
| C | Generator packaging core | port `pe.rs`, embed template, specialize, atomic publish | 1 (exe half) | not specced |
| D | File bridge and adapters | port both WebView2 bridges, Photopea and `vscode.dev`, single-instance IPC | 6, 7, 8, save half of 4 and 5 | not specced |
| E | Association registration | HKCU registration, "Open with", first-launch offer | double-click half of 4 and 5 | not specced |
| F | Generator UI and discovery | Tauri shell, discovery chain and gallery, association editor | 1 | not specced |

**Start at Task 5 of [`docs/superpowers/plans/2026-09-16-webwrap-spec-and-minimal-runtime.md`](docs/superpowers/plans/2026-09-16-webwrap-spec-and-minimal-runtime.md).** Read that plan's Status section first — it lists how the shipped crate differs from the plan's own Tasks 1-4 code blocks after review fixes. Read the spec too: [`docs/superpowers/specs/2026-09-16-webwrap-spec-and-minimal-runtime-design.md`](docs/superpowers/specs/2026-09-16-webwrap-spec-and-minimal-runtime-design.md).

Sub-project A shipped `crates/webwrap-spec`: the `AppSpec` schema and validation, URL canonicalization, stable app identity, and navigation policy. Pure Rust, `forbid(unsafe_code)`, no Tauri or Windows dependency, so its whole suite runs anywhere. **`crates/webwrap-spec/tests/vectors.rs` is a permanent contract** — the identity of a URL decides whether regeneration updates an app or creates a second one. A failing vector means the implementation regressed, never that the vector needs updating.

Spikes are deleted by whichever sub-project ports them, not all at once. `spikes/` is untouched by A and B; C removes `windows-pe-specialization`, D removes both WebView2 spikes. **B's integration test shells out to the PE spike's `webwrap-pe-specialize` binary to build its fixture**, so that spike must stay until C replaces it — at which point `specializer_path()` in `crates/webwrap-runtime/tests/inspect_self.rs` is the single line to repoint.

## Environment notes for the implementing session

**Run sub-project B from Windows, not WSL.** It is `compile_error!` off Windows and needs the MSVC toolchain plus WebView2; none of it builds or tests under WSL, and this WSL host is `aarch64` Linux. The Windows side has both toolchains installed and the same plugin set.

- **`rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc` once**, then pass `--target` on every command. `rust-toolchain.toml` deliberately carries no `targets` key: that key only controls what rustup installs, it does not change the default build target, and it would force the WSL toolchain to download two Windows standard libraries it never uses. On this ARM64 host a naive `cargo build` silently yields an aarch64 binary while every spike's evidence was gathered on x64 MSVC. Generated executables target both architectures.
- **`D:` is 92% full — about 24 GB free, and `target/` is 22 GB of it.** A WSL build into `/mnt/d` failed with `ENOMEM`; sub-project A was built with `CARGO_TARGET_DIR=$HOME/.cache/webwrap-target` instead. Thirteen of the fourteen `target/*` directories (`photopea-x64`, `task3-review`, `merge-x64`, and similar) are stale output from earlier review checkpoints and are reclaimable. Nobody has deleted them; that is the user's call.
- **Explicit `--target` separates *compiled* artifacts into `target/<triple>/`, but not build scripts and proc macros**, which still compile for the host into plain `target/debug`. `tauri-build` is a build dependency, so Task 5 hits this. Cargo's metadata hashing should keep filenames distinct, but that is unverified — Task 5 confirms it rather than assuming it.

## Resume guardrails

Read the full plan before editing. Continue the subagent-driven workflow with one implementer and a review checkpoint per task. Preserve the no-sidecar and failure-preservation invariants. Do not claim the product is complete from these spikes; they are packaging and file-write proofs, not the final generator.

Sub-project A closes no release gate. B closes gates 11 and 14 only. Gates 1, 4, 5, 6, 7, and 8 belong to C, D, and E and must not be described as proven before then.

One lesson from A's review worth carrying into B: every defect it found was a claim asserted in prose that no test asserted — a library behaviour taken on trust. The identity vectors and the documented `url`-crate behaviour, both of which *were* pinned by tests, held up under independent recomputation. When the plan states what a dependency does, make it an `assert`, not a note.
