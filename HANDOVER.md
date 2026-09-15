# WebWrap handover

Updated 2026-09-15. Work is intentionally paused at the user's request after the Windows PE publication spike. A fresh Codex/Claude session should start here:

- Worktree: `/mnt/d/git/pwa_ganerator/.worktrees/photopea-windows-writeback`
- Branch: `spike/photopea-windows-writeback`
- Current code HEAD before this document: `b4c537b`
- The worktree was clean before this document was added.

Use this linked worktree, not the repository's main worktree. Delegated workers accidentally created duplicate PE commits on `main`; those commits were preserved, but `main` must not be reset or destructively rewritten. The intended implementation branch is the one above.

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

| Task | State | Commits on this branch |
| --- | --- | --- |
| 1. Scaffold/parser/validators | Complete; reviews passed | `271da38`, `41132ef`, `8e1f059` |
| 2. Spec/icon/version resources | Complete; review passed | `6226303`, `e689bbd` |
| 3. Staging/atomic publish/CLI | Implementation and fixes complete; final review was interrupted before its verdict | `2fd88ee`, `cdf50d5`, `b4c537b` |
| 4. Windows acceptance evidence | Not started | — |

Current implementation files:

- `spikes/windows-pe-specialization/src/lib.rs` — public command parser/types and unit tests.
- `spikes/windows-pe-specialization/src/pe.rs` — validation, resource editing/read-back, staging, and Windows publication.
- `spikes/windows-pe-specialization/src/main.rs` — `specialize` and `inspect` JSON CLI.
- `spikes/windows-pe-specialization/tests/pe_specialization.rs` — PE round-trip and transaction tests.

Verification already run on `b4c537b` with the native MSVC environment:

- `cargo test -p windows-pe-specialization`: 3 unit tests + 5 integration tests passed (8 total; doctests had 0 tests).
- `cargo clippy -p windows-pe-specialization --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Manual CLI specialize/inspect succeeded and emitted JSON with `specBytes`, `iconPresent`, `fileDescription`, and `productVersion`.
- Malformed spec exited 2; missing/unreadable spec exited 1 after `b4c537b`.

## Immediate next step: Task 4 acceptance evidence

First have a fresh reviewer re-check Task 3 at `b4c537b` against the plan (the last review identified the missing-spec exit-code distinction; that fix is `b4c537b`). Then run the planned Windows acceptance probe and create [`docs/spikes/2026-09-15-windows-pe-specialization.md`](docs/spikes/2026-09-15-windows-pe-specialization.md).

Use the current linked worktree and an external target directory:

```text
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo build -p photopea-windows-writeback-spike -p windows-pe-specialization-spike --target-dir D:/git/pwa_ganerator/target/pe-specialization-x64"
```

Use `D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe` as the tool and `D:\git\pwa_ganerator\target\pe-specialization-x64\debug\photopea-windows-writeback-spike.exe` as the immutable live base. The icon fixture is `D:\git\pwa_ganerator\.worktrees\photopea-windows-writeback\spikes\photopea-windows-writeback\icons\icon.png`. Write the temporary JSON spec with BOM-free UTF-8 bytes. Run `specialize` and then the tool's own `inspect`; repeat to replace the same output path; then use a text file as the base and a missing icon to prove exit code 1, unchanged output hash, and no `.webwrap-pe-*.tmp` residue.

Record exact commands, hashes, resource read-back, toolchain/OS versions, and the explicit boundary: this gate proves single-file PE specialization only. Runtime self-reading, file-association registration, signing, APK generation, DNS enforcement, icon search, and generator UX remain future work.

After evidence, run the final workspace tests/fmt/diff checks and commit only the evidence document as `docs: record PE specialization evidence`. Do not commit generated executables, temporary specs, or ignored SDD review files.

## Resume guardrails

Read the full plan before editing. Continue the subagent-driven workflow with one implementer and a review checkpoint per task. Preserve the no-sidecar and failure-preservation invariants. Do not claim the product is complete from these spikes; they are packaging and file-write proofs, not the final generator.
