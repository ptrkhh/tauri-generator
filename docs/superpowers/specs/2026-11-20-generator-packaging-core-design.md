# Generator Packaging Core Design (sub-project C)

Date: 2026-11-20. Status: implemented on `feat/generator-packaging-core`. Deviations and the running-app finding are in `docs/evidence/2026-11-20-generator-packaging-core.md`.

## Summary

C turns user inputs into finished Windows apps. The user gives a website, a name, a square PNG icon, and an output folder. C produces `<slug>-x64.exe` and `<slug>-arm64.exe` there. Each is the sub-project B runtime template with the app specification, icon, and version strings embedded. Each is verified by reading it back, then published by an atomic rename.

The `.exe`-editing code moves out of `spikes/windows-pe-specialization` into a library. The spike is deleted.

Everything in C except the in-use file case is plain Rust with no Windows API calls. So its tests run on Linux as well as in Windows CI.

## Decisions

| Question | Decision | Source |
| --- | --- | --- |
| Which processor architecture do generated apps target? | Both, every time, as two files: `<slug>-x64.exe` and `<slug>-arm64.exe` | User, 2026-11-20 |
| Default output folder | None: the generator asks every time, starting at the last folder used. C takes the folder as an input; the picker and its memory belong to F | User, 2026-11-20 |
| A different website produces an existing file name | Overwrite it. This supersedes "different canonical URLs coexist" for same-name clashes | User, 2026-11-20 |
| An existing file with that name was not made by the generator | Refuse, name the file, write nothing. The overwrite decision covers generated apps only; a stray `twitter.exe` installer must never be destroyed | This design (safety limit on the above) |
| Generator's own packaging | A single file that embeds both release templates. A sidecar template could drift from the generator's version | This design |
| Generator's own architecture | Built for both; either build embeds both templates | Follows from row 1 |
| Default file name | Derived from the website address, so `twitter.com` gives `twitter-x64.exe` (see Slug derivation) | This design; keeps gate 1's naming |
| App version | Always `1.0.0` in C. Nothing on Windows needs it to change. Monotonic versions arrive with Android (phase 2, gate 10) | This design |

## Scope

### In scope

- `crates/webwrap-pack`, a library with `forbid(unsafe_code)`. It contains the ported resource editing, spec construction, slug derivation, the packaging pipeline, overwrite rules, and inspection.
- `crates/webwrap-generate`, a CLI binary with `generate` and `inspect` subcommands. It is the generator until F wraps the library in its Tauri UI. CI acceptance builds use it.
- Embedding both runtime templates into the generator.
- Deleting `spikes/windows-pe-specialization`, and repointing B's tests and the CI workflow.

### Out of scope

- Anything visual: the folder picker, the overwrite notice, icon discovery and the initials fallback, the association editor. All of these are F.
- Converting non-PNG icons, and padding non-square ones. Both are F.
- Registering associations and creating Start-menu shortcuts (E).
- Adapter presets beyond passing the chosen adapter and associations through (D and F).
- Android, custom DNS, and signing (phase 2).
- A diagnostic log file (F). C returns errors that already name the file and the step.
- A free-disk-space precheck. A full disk fails during staging, which already leaves existing outputs untouched.

## Inputs and the app specification

| Input | Rule |
| --- | --- |
| Website | Free text. `webwrap_spec::canonicalize_url` normalises it. Anything that is not https is rejected, which is the generator half of gate 14 |
| Display name | Required, and validated exactly as `AppSpec` validates `displayName` |
| Slug | Optional. Derived when absent, validated as `AppSpec.slug` |
| Icon | PNG bytes. Must decode, be square, and be at most 4096 px a side and 10 MiB. Checked before anything is staged |
| Adapter and file associations | Passed through. Defaults: `none` and empty |
| Output folder | Must exist and be a directory |

C builds the `AppSpec` itself:

- `appId` = `app_identity(canonical)`;
- `trustedOrigins` = the launch URL's origin;
- `dnsMode` = `system`;
- `appVersion` = `1.0.0`;
- `schemaVersion` = 1.

It serialises the spec and parses it back with `AppSpec::from_slice` before embedding. So the generator can never embed a spec the runtime would reject. This closes the gap the spike's weaker `validate_spec` left open.

### Slug derivation

From the canonical URL's host:

1. Drop a leading `www.`.
2. Drop the last label (the top-level domain).
3. Join the remaining labels with `-`.
4. Replace any character outside `[a-z0-9-]` with `-`, collapse repeats, and trim hyphens at the ends.
5. Truncate to 64 characters.
6. If the result is empty, use `app`. If it is a reserved device name, append `-app`. (Appending `-app` to an empty result would start the name with a hyphen.)

Examples:

| Host | Slug |
| --- | --- |
| `twitter.com` | `twitter` |
| `www.photopea.com` | `photopea` |
| `vscode.dev` | `vscode` |
| `mail.google.com` | `mail-google` |
| `bbc.co.uk` | `bbc-co` |

`bbc-co` is the known cost of having no public-suffix list. The slug is user-overridable, and a new dependency is not worth it here.

## Packaging pipeline

1. **Validate** every input and both output targets (see Overwrite rules) before writing anything.
2. **For each architecture:**
   - parse the template;
   - check it is an unspecialized runtime of that architecture (PE machine `0x8664` or `0xAA64`, no `WEBWRAP_APP_SPEC`);
   - embed the spec (`RT_RCDATA`, `WEBWRAP_APP_SPEC`, language `0x0409`), the icon (as `MAINICON`, via editpe), and the four version strings;
   - write the result to a uniquely named staging file in the output folder (same volume), then `fsync`.
3. **Read both staged files back** and require:
   - the embedded spec bytes are equal to what was written;
   - the icon is present;
   - `FileDescription` is the display name and `ProductVersion` is the version;
   - the machine type is still the template's.
4. **Publish** each staged file over its target with `std::fs::rename`, an atomic replace on the same volume on both Linux and Windows. The spike's direct `MoveFileExW` call, and its `unsafe` block, go away.
   - On Windows the standard library calls `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. On `ERROR_ACCESS_DENIED` it falls back to `SetFileInformationByHandle` with `FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS` (rust 1.100 `library/std/src/sys/fs/windows.rs:1306`). Checked against the source.
   - Unlike the spike, there is no `MOVEFILE_WRITE_THROUGH`. The staged file is already `fsync`ed, so after a power loss the target is either the old app or the new one, and both are valid.
5. **On any failure:** delete the staging files. Existing outputs are untouched.

The two publishes are not one atomic unit. If the second rename fails after the first succeeded, the error says which file was updated. Both files are complete, valid apps either way, and the next generation repairs the mismatch.

The version-string limitation the spike recorded, where only `strings[0]` is updated, carries over with its comment. B's evidence shows the runtime template has a single table.

## Overwrite rules

| Existing target | Outcome |
| --- | --- |
| Nothing | Write it |
| A generated app with the same `appId` | Replace it. Gate 3: regenerating Twitter updates Twitter |
| A generated app with a different `appId` | Replace it, per the user's decision. The old app's storage folder stays in `%LOCALAPPDATA%\WebWrap\<oldId>`; nothing deletes it |
| Anything else: not a PE, a PE without `WEBWRAP_APP_SPEC`, a directory | Refuse. Name the file. Write nothing, including the other architecture's file |
| An app that is currently running (Windows) | Refused before anything is published: `PackError::InUse`, naming the app to close. CI showed Windows denies the rename (os error 5), even through the POSIX-semantics fallback; see the evidence. |

The result reports, per file, whether it was new, replaced the same app, or replaced a different app. That lets F show what happened without asking first.

## Templates

- The CI or release pipeline builds `webwrap-runtime` in release mode for both targets. It then builds `webwrap-generate` with `WEBWRAP_TEMPLATE_X64` and `WEBWRAP_TEMPLATE_ARM64` pointing at them. A build script copies them into `OUT_DIR`, and the binary embeds them with `include_bytes!`.
- Ordinary `cargo build` and `cargo test` runs set neither variable. The binary is then built without templates and requires `--template-x64` and `--template-arm64`. A generator with neither embedded templates nor flags exits 2 with a message saying so.
- Library tests take template paths from those same two environment variables. The plan decides how each environment produces them: `cargo xwin` on Linux, native builds in CI.

## CLI

```
webwrap-generate generate --url <text> --name <display name> --icon <png> --out <folder>
                          [--slug <slug>] [--adapter none|photopea|vscodeDev]
                          [--assoc <.ext> <mime> <description>]...
                          [--template-x64 <exe> --template-arm64 <exe>]
webwrap-generate inspect --input <exe>
```

- `generate` prints one JSON object: `appId`, `slug`, and `outputs[]`. Each output has `arch`, `path`, and `replaced`: `none`, `sameApp`, or `otherApp`.
- `inspect` prints the embedded spec's `appId`, `specBytes`, `iconPresent`, `fileDescription`, `productVersion`, and `arch`.
- Exit codes match the rest of the project: `0` success, `2` usage error, `1` runtime failure.

## Changes to sub-project B and CI

- `crates/webwrap-runtime/tests/inspect_self.rs` stops shelling out to the spike binary. Instead it adds `webwrap-pack` as a dev-dependency:
  - `specialize_fixture` calls `webwrap_pack::specialize_raw`, the low-level primitive the pipeline itself uses. It takes spec bytes, icon, name, and version, with no `AppSpec` validation, so B's mismatched-identity test can still build an invalid app;
  - `inspect_with_specializer` calls `webwrap_pack::inspect`.

  `specialize_raw` is documented as the unchecked primitive. Only `generate` is the product path.
- `.github/workflows/windows.yml`:
  - drops `cargo build -p windows-pe-specialization-spike`;
  - builds the acceptance apps with `webwrap-generate generate` from the fixtures, so the output names become `photopea-x64.exe`, `photopea-arm64.exe`, and the Twitter pair;
  - smoke-launches the build matching the runner's architecture;
  - uploads the generator itself, with templates embedded, as an artifact.
- `docs/windows-runs/fixtures/*.json` are no longer needed by CI, because `generate` builds the spec. The icons remain.

## Verification

All of these run on Linux (`cargo test -p webwrap-pack`, with templates built by `cargo xwin`) and in both Windows CI jobs.

- **Gate 1 (exe half).**
  - Generating Twitter into an empty folder leaves exactly `twitter-x64.exe` and `twitter-arm64.exe` and nothing else: no staging leftovers, no sidecars.
  - Each file carries the spec, `MAINICON`, the version strings, and the right PE machine type.
  - The x64 file was built from the x64 template, not swapped.
- **Gate 3.**
  - Regenerating Twitter beside a generated Facebook pair leaves both Facebook files byte-identical.
  - Regenerating Twitter replaces both Twitter files and reports `sameApp`.
- **Overwrite rules.** Each row of the table, including the requirement that a foreign file blocks both architectures.
- **Failure preservation.** A bad icon, an invalid name, a non-https URL, and a template of the wrong architecture each leave the output folder byte-identical to before.
- **Spec construction.**
  - The embedded spec round-trips through `AppSpec::from_slice`.
  - `appId` matches the identity vectors already pinned in `webwrap-spec`.
  - Slug derivation is checked against the examples above.
- **CLI.** Exit codes and JSON shapes.
- **Windows only.** The running-app case (see Overwrite rules), plus B's full integration suite running against fixtures specialized by `webwrap-pack`.

No hands-on PC run is planned for C. CI smoke-launches the generated apps on real WebView2, and B already confirmed the window and icon by hand. A hands-on run is added only if the running-app probe needs eyes.

## Release gates

- **Gate 1, Windows half, reworded:** `twitter-x64.exe` and `twitter-arm64.exe` are single-file outputs with embedded configuration and icons. The APK half stays open until phase 2. Gate 1 also lists F, which supplies discovery and UI around the same outputs.
- **Gate 3:** regenerating Twitter updates only Twitter. This still holds for distinct names. A *different* website deliberately given the same name now replaces the old app, by the user's decision.

C proves neither gate 4, 5, 6, 7, nor 8.

## Open items for later sub-projects

- **D.** Both architecture builds of one app share an identity, so they share `%LOCALAPPDATA%\WebWrap\<appId>`. D's identity-derived single-instance mutex must make the second one, of either architecture, forward to the first rather than open the same WebView2 folder concurrently.
- **E.** Associations are registered per executable path. Decide whether registering one architecture's build should claim the types for both, or only for itself.
- **F.**
  - The folder picker, which remembers the last folder.
  - Showing `replaced: otherApp` in the result card.
  - PNG conversion and square padding for discovered icons.
  - Adapter and association presets.
