# Evidence: generator packaging core (sub-project C)

Date: 2026-11-20. Branch `feat/generator-packaging-core`. Spec: `docs/superpowers/specs/2026-11-20-generator-packaging-core-design.md`.

This covers sub-project C only. It closes:

- **the Windows half of gate 1**, reworded by the user's decision: `twitter-x64.exe` and `twitter-arm64.exe` are single-file outputs with embedded configuration and icons;
- **gate 3**: regenerating Twitter updates only Twitter.

**Gates 4, 5, 6, 7, and 8 are not proven by this work.** Gate 1 also lists F (UI and discovery) and the APK half (phase 2).

## 1. Environments

- **Linux (x86_64).** `rustc 1.100.0`, `cargo-xwin 0.23.1`, Wine 11.0.
  - `webwrap-pack`, `webwrap-generate`, and `webwrap-spec` build and test natively. They have no Windows API calls.
  - Templates come from `cargo xwin build -p webwrap-runtime` for both targets.
- **Windows (authoritative).** `.github/workflows/windows.yml` on `windows-latest` (x64) and `windows-11-arm` (ARM64).
  - Each runner builds **both** release templates. Building the other architecture worked on both runner images.
  - Each runner runs every suite natively, then builds the generator with both templates embedded.
  - It then generates the Photopea and Twitter pairs with that generator and smoke-launches the native pair on real WebView2.

## 2. Automated results

| Suite | Linux | x64 CI | ARM64 CI |
| --- | --- | --- | --- |
| `webwrap-pack` unit (PE header, spec, slug, icon) | 7 | 7 | 7 |
| `webwrap-pack` `tests/pe.rs` (real templates) | 4 | 4 | 4 |
| `webwrap-pack` `tests/generate.rs` (real templates) | 9 | 9 | 9 |
| `webwrap-pack` `tests/icon_memory.rs` | 1 | 1 | 1 |
| `webwrap-pack` `tests/running_app.rs` (Windows only, run explicitly) | n/a | 1 | 1 |
| `webwrap-generate` `tests/cli.rs` | 3 | 3 | 3 |
| `webwrap-runtime` 3 unit + 9 integration (now specialized by `webwrap-pack`) | 12 (Wine) | 12 | 12 |
| `webwrap-spec` 30 unit + 8 vectors | 38 | 38 | 38 |
| clippy `-D warnings`, all four crates, both Windows targets | pass (cross) | pass | pass |

CI runs:

- `44658859525`, the first full run. It also yielded the running-app observation in section 3.
- `44660537408` at `5427379`, the final code commit. Every row above passed on both runners, including the pinned running-app test (refused as `InUse`, folder unchanged), followed by the smoke launch.

Template-dependent tests **fail**, not skip, when `WEBWRAP_TEMPLATE_X64` and `WEBWRAP_TEMPLATE_ARM64` are unset. Each new data-safety test was mutation-checked, meaning it was confirmed to fail with its fix removed:

- deleting the pre-publish re-check fails the race test (3 of 3 runs);
- deleting the read-only refusal fails its test;
- lifting the icon allocation bound fails the memory test.

## 3. Library behaviour verified, not assumed

- **Replacing a running app.** CI run `44658859525` probed it on both runners. Windows refuses to rename over a running executable with `Access is denied (os error 5)`, even through the standard library's fallback.
  - On ARM64 that left `example-x64.exe` updated and the running `example-arm64.exe` not: a partial publish.
  - The generator now opens each target for write, without truncating, before publishing anything. A running app fails that check, so it is refused as `PackError::InUse` with the folder unchanged. `tests/running_app.rs` pins exactly this.
- **`std::fs::rename` on Windows.** It calls `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. On `ERROR_ACCESS_DENIED` it falls back to `SetFileInformationByHandle` with `FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS` (rust 1.100, `library/std/src/sys/fs/windows.rs:1306`). This replaces the spike's direct `MoveFileExW` and its `unsafe` block. There is no `MOVEFILE_WRITE_THROUGH`; the staged file is `fsync`ed first.
- **PNG decoding is not inherited.** `editpe` 0.2.4 depends on `image` with only the `ico` feature. PNG worked in the spike only because another workspace member enabled `png`. `webwrap-pack` now enables it itself.
- **ICC profiles.** `png` 0.18.1 inflates an `iCCP` profile up to the decoder's byte limit before any pixels, and silently drops a larger one (`parse_iccp`, `src/decoder/stream.rs:1696`, `:1726`). `image` 0.25.10 passes `Limits::max_alloc` into that limit (`PngDecoder::with_limits`, `src/codecs/png.rs:50`).
  - The dimension check is bounded at 8 MiB. Measured largest allocation with a 64 MiB profile: 8.6 MB bounded, 69 MB unbounded.
  - Full decoding is bounded at 80 MiB, enough for a 4096 px RGBA icon.
- **editpe in memory.** `Image::parse` and `write_writer` let both apps be built and checked before anything touches disk. The replacement icon group is named `MAINICON` (B's evidence), which the runtime loads for its window.

## 4. Deviations from the spec and plan

- **Empty derived slug** becomes `app`, because `-app` alone would start with a hyphen. The spec was updated.
- **CLI adapter names** are the spec's JSON names: `none`, `photopea`, `vscodeDev`.
- **Running apps** are refused before publishing (section 3). The spec had left the outcome to the probe.
- **A read-only generated app** is refused up front. Windows would otherwise refuse it only after the other architecture was published.
- **Both targets are re-checked immediately before publishing.**
  - *Known limit:* a file created in the instant between that check and the rename would still be replaced. Closing it needs a no-replace rename, which std lacks. It is marked `shortcut:` in `generate.rs`.
- **CI tests use the release templates.** The acceptance artifact now also contains the generator itself.

## 5. Review findings

An independent review of the branch found the following.

**Fixed:**

| Finding | Fix |
| --- | --- |
| A file appearing mid-generation was overwritten | Re-check before publish |
| A running or read-only second target was partially published | Refused up front |
| The ICC profile was inflated to 512 MiB during the icon check | Bounded |
| No cleanup on panic | A drop guard removes staging files |
| The CLI read an unbounded icon file | Reads at most 10 MiB + 1 byte |
| Repeated CLI flags were silently accepted | Rejected |

**Known and not fixed:**

1. **A deliberately crafted executable can exhaust memory.** Placed at an output path, it can make `editpe`'s resource parser run out of memory. Tables that share subtables are re-parsed for every path; a 5 KB file aborts under a 4 GB limit. Triggering it requires write access to the folder the user chose. The fix belongs in `editpe`. F should know that the generator process, its UI included, can be killed this way.
2. **A killed process leaves staging files.** A process killed mid-generation leaves `.webwrap-<pid>-<n>.tmp` (about 10 MB each). The drop guard covers errors and panics, not a kill.
3. **CLI values cannot start with `--`**, so a display name like `--x` cannot be passed on the command line. F's UI is unaffected.
4. **Cleanup after a verify failure or a failed second rename has no test.** Neither can be induced deterministically. The code path is the same drop guard the tested paths use.
