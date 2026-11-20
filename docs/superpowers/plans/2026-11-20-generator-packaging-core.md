# Generator Packaging Core Implementation Plan (sub-project C)

Spec: [`docs/superpowers/specs/2026-11-20-generator-packaging-core-design.md`](../specs/2026-11-20-generator-packaging-core-design.md). Branch `feat/generator-packaging-core`.

## Status

Tasks 1-6 are pending. Each is committed on its own and pushed, and CI must be green before the next push that depends on Windows.

## Global constraints

- **Where code runs.** Code is written and built on Linux with `cargo xwin`. Windows behaviour is decided by `.github/workflows/windows.yml` on x64 and ARM64. The user's Windows PC never compiles. See `HANDOVER.md`, "How work runs now".
- **No new dependency trees.** Every crate below is already in `Cargo.lock`. Pin with `=`:
  - `editpe =0.2.4`;
  - `image =0.25.10`, with `default-features = false, features = ["png"]`. editpe enables only `ico`, so PNG decoding must be switched on here, not inherited by accident;
  - `serde_json =1.0.151`;
  - `url =2.5.8`;
  - `webwrap-spec` by path.
- **`webwrap-pack` is `#![forbid(unsafe_code)]`** and builds on every platform. Its tests run on Linux.
- **Templates for tests.** Template-dependent tests read `WEBWRAP_TEMPLATE_X64` and `WEBWRAP_TEMPLATE_ARM64`. If either is unset, the test **fails** with instructions. It never skips: a skipped test would let CI pass without testing anything.
  - Linux: `cargo xwin build -p webwrap-runtime --target x86_64-pc-windows-msvc` and `--target aarch64-pc-windows-msvc`, then point the variables at `target/<triple>/debug/webwrap-runtime.exe`.
- **Exit codes:** `0` success, `2` usage or invalid input, `1` runtime failure.
- **The identity vectors in `crates/webwrap-spec/tests/vectors.rs` are a permanent contract.** C calls `app_identity` and never re-derives it.

## Task 1: `webwrap-pack` PE primitives

Port of `spikes/windows-pe-specialization/src/pe.rs`, working in memory.

`crates/webwrap-pack/src/pe.rs` provides:

- `Arch { X64, Arm64 }`, with `suffix()` (`x64`/`arm64`), `machine()` (`0x8664`/`0xAA64`), and `runtime_arch()` (`x86_64`/`aarch64`).
- `machine_of(bytes)`. Reads the COFF machine field via `e_lfanew` and checks the `PE\0\0` signature.
- `specialize_raw(template, spec_bytes, icon, display_name, version) -> Vec<u8>`. The unchecked primitive: embeds `RT_RCDATA`/`WEBWRAP_APP_SPEC`/`0x0409`, the `MAINICON`, and the four version strings.
- `inspect(bytes) -> Inspection`, with:
  - `spec: Option<Vec<u8>>`;
  - `icon_present`;
  - `file_description`;
  - `product_version`;
  - `arch`.

Tests (real templates):

- Round trip on both architectures.
- The machine type is preserved.
- `inspect` on a template reports `spec: None`.
- A non-PE input is an error, not a panic.

## Task 2: spec construction and slug derivation

`crates/webwrap-pack/src/spec.rs` provides:

- `derive_slug(canonical_url)`. Implements the spec's slug rules exactly.
- `build_spec(request) -> (AppSpec, Vec<u8>)`. Canonicalizes, rejects non-https, and fills the fixed fields. The serialized bytes go back through `AppSpec::from_slice`.
- `check_icon(png)`. PNG only, square, at most 4096 px a side and 10 MiB. Dimensions are read before decoding.

Tests (no templates):

- Slug examples from the spec, the reserved-name and empty fallbacks, and truncation.
- The `appId` of `twitter.com` equals the pinned vector.
- `http://` is rejected.
- Bad icons are rejected: non-square, too large, not a PNG.

## Task 3: the `generate` pipeline and overwrite rules

`crates/webwrap-pack/src/generate.rs` provides `generate(request, Templates { x64, arm64 }) -> Generated { app_id, slug, outputs[{ arch, path, replaced }] }`. It follows the spec's pipeline:

1. Validate the inputs and both targets.
2. Check each template's machine type and that it has no spec.
3. Specialize both in memory.
4. Stage, then `fsync`.
5. Read back from disk and verify.
6. Rename x64, then arm64.
7. Clean up the staging files on every error path.

If the second rename fails, the error is `PartialPublish`, naming the updated file.

Tests (real templates), one per spec row:

- **Empty folder:** exactly two files result.
- **Same app:** both files report `sameApp`. A neighbouring Facebook pair stays byte-identical (gate 3).
- **Other app:** reports `otherApp`.
- **Foreign file:** generation is refused. Both targets are untouched and no staging files remain.
- **Directory at the target:** refused.
- **Failures:** a bad icon, a bad name, a non-https URL, and swapped templates each leave the folder byte-identical.

## Task 4: the `webwrap-generate` CLI and embedded templates

- `crates/webwrap-generate/build.rs`. If `WEBWRAP_TEMPLATE_X64`/`WEBWRAP_TEMPLATE_ARM64` are set, copies them into `OUT_DIR`; otherwise writes empty files. Emits `rerun-if-env-changed` and `rerun-if-changed`.
- `src/main.rs`:
  - provides `generate` and `inspect` with the spec's flags, emitting JSON;
  - embeds the templates with `include_bytes!`;
  - an empty embedded template plus no `--template-*` flag means exit 2 with an explanation.

Tests: CLI exit codes for an unknown flag, a missing value, missing templates, and an invalid URL. A JSON shape test runs a generation through the binary.

## Task 5: repoint sub-project B, delete the spike, and update CI

- `crates/webwrap-runtime/tests/inspect_self.rs`:
  - `webwrap-pack` becomes a dev-dependency;
  - `specialize_fixture` calls `specialize_raw`, and `inspect_with_specializer` calls `inspect`;
  - `specializer_path()` goes;
  - all 9 integration tests must still pass.
- Delete `spikes/windows-pe-specialization` and `docs/windows-runs/fixtures/*.json`.
- `.github/workflows/windows.yml`, on each runner:
  1. Build the runtime for both targets.
  2. Export the template variables.
  3. Test `webwrap-pack`, `webwrap-runtime`, and `webwrap-spec`.
  4. Build the release generator with templates embedded.
  5. Generate the Photopea and Twitter pairs with it.
  6. Smoke-launch the runner's native pair, using the existing folder checks.
  7. Upload the generator and the apps.
- **Running-app probe.** A Windows-only `#[ignore]` test is run explicitly in CI. It launches a generated app, regenerates over it while it runs, and records the outcome. Then the behaviour is pinned:
  - if replacement is refused, map the error to a clear "close `<app>` and retry" `PackError::InUse`, and assert that;
  - if it succeeds, assert the running process is unaffected and that the new file is valid.

## Task 6: evidence and handover

- `docs/evidence/2026-11-20-generator-packaging-core.md`. Records test counts per environment, the running-app finding, and the deviations.
- Update the spec's Status line, this plan's Status, and the `HANDOVER.md` table and next step (D).
- An independent review of the branch diff before merging.
