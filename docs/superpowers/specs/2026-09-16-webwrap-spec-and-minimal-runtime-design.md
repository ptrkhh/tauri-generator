# WebWrap App Spec and Minimal Runtime Design

**Status:** Approved

**Date:** 2026-09-16

**Sub-project:** A + B of the Windows phase-1 track

**Parent design:** [`2026-09-14-tauri-web-app-generator-design.md`](2026-09-14-tauri-web-app-generator-design.md)

## Summary

Build two crates:

- `webwrap-spec` — the versioned app specification, its validation, URL canonicalization, app identity derivation, and navigation policy. Pure Rust, cross-platform, no `unsafe`.
- `webwrap-runtime` — the universal Windows wrapper binary. It reads its own embedded `WEBWRAP_APP_SPEC` resource, derives an isolated WebView2 data directory from the embedded identity, opens the embedded launch URL, and enforces navigation policy.

When this sub-project lands, a specialized executable opens its own configured website under its own isolated storage, and release gates 11 and 14 are provable. It is also the template binary that the generator embeds and specializes in sub-project C.

## Scope

### In scope

- The complete `AppSpec` schema, all field validation, and two structural invariants.
- URL canonicalization and stable app identity derivation with permanent test vectors.
- Navigation policy as a pure function, including the trusted-origin predicate consumed later by the file bridge.
- Windows resource self-read from the loaded image.
- Per-identity WebView2 data directory isolation.
- A visible fatal-error surface for a GUI-subsystem binary.
- A `--inspect-self` JSON mode that makes the runtime's own configuration assertable by automated tests.
- A placeholder icon and version resources so sub-project C has something to replace. The icon is the load-bearing one: the spike's specializer calls `remove_main_icon` before `set_main_icon_file`, so a template with no main icon cannot be specialized. Version strings are created if absent, so `VERSIONINFO` is shipped for the sake of the unspecialized template not being anonymous rather than because specialization requires it.

### Out of scope

Deferred to later sub-projects, listed with their owner:

| Deferred | Owner |
| --- | --- |
| PE specialization, atomic publish, generator packaging | C |
| WebView2 file bridge, Photopea and `vscode.dev` adapters, single-instance IPC | D |
| File-association registry registration and the first-launch offer | E |
| Generator UI, metadata and icon discovery | F |
| Custom DNS over HTTPS, loopback CONNECT proxy | Phase 2 |
| Android APK generation | Phase 2 |
| Code signing | Phase 2 |

## Phase-1 context

The parent design's delivery sequence reached step 3. Steps 1-3 are complete as merged spikes; step 4 onward is unbuilt. The Windows track is decomposed into six sub-projects with one-way dependencies, each receiving its own spec, plan, and implementation cycle:

| | Sub-project | Content | Gates |
| --- | --- | --- | --- |
| A | Core spec and identity | schema, validation, canonicalization, identity, policy | — |
| B | Minimal wrapper runtime | resource self-read, data-dir isolation, nav policy, launch | 11, 14 |
| C | Generator packaging core | port `pe.rs`, embed template, specialize, atomic publish | 1 (exe half) |
| D | File bridge and adapters | port both WebView2 bridges, Photopea and `vscode.dev`, single-instance IPC | 6, 7, 8, save half of 4 and 5 |
| E | Association registration | HKCU registration, **Open with**, first-launch offer | double-click half of 4 and 5 |
| F | Generator UI and discovery | Tauri shell, discovery chain and gallery, association editor | 1 |

This document covers A and B together, so that A's schema has a live consumer rather than being designed in a vacuum.

### Decisions carried into this design

- **Custom DNS is deferred to phase 2.** The `dnsMode` field exists and is validated, but phase 1 accepts only `system`. The runtime rejects any other value rather than silently using system DNS, so phase 2 cannot introduce a silent-fallback regression.
- **The generator embeds the wrapper.** `tauri_generator.exe` carries the wrapper template inside itself via `include_bytes!` and ships as a single file. This is C's mechanism, but it is why B must produce a clean, resource-complete template.
- **Metadata and icon discovery ships in full** in sub-project F: manifest, `apple-touch-icon`, icon links and favicons, then application-name and title, with a gallery, upload, and initials fallback.
- **Spikes are deleted by whichever sub-project ports them,** not all at once. `spikes/` is untouched by A and B. C removes `windows-pe-specialization` when `pe.rs` lands in the generator; D removes both WebView2 spikes when the bridges land. Nothing is deleted before its replacement exists, and B's integration test depends on this (see Verification).
- **Generated executables target both `x86_64` and `aarch64`.** Windows-on-ARM emulates x64, so a single ARM64 Windows host can build and test both targets.

## Crate layout

```
Cargo.toml                 [workspace] members = ["crates/*", "spikes/*"]
crates/
  webwrap-spec/
    Cargo.toml             serde, serde_json, url, sha2
    src/lib.rs             #![forbid(unsafe_code)]
    src/schema.rs          AppSpec, field validation, invariants
    src/identity.rs        canonicalize_url, app_identity
    src/policy.rs          navigation verdicts, trusted-origin predicate
    tests/vectors.rs       canonicalization and identity contract vectors
  webwrap-runtime/
    Cargo.toml             tauri, webwrap-spec, windows
    build.rs               tauri-build
    src/main.rs            argument parsing, startup, window
    src/embedded_spec.rs   Win32 resource read
    src/fatal.rs           MessageBoxW error surface
    src/inspect.rs         --inspect-self JSON
    tauri.conf.json        placeholder icon and VERSIONINFO
    tests/inspect_self.rs  native integration test
spikes/                    untouched by A and B
```

`webwrap-spec` has no Tauri and no Windows dependency, so its entire test suite runs under WSL. `webwrap-runtime` is Windows-only and contains only Win32 and Tauri glue; `unsafe` is confined to `embedded_spec.rs` and `fatal.rs`.

Navigation policy lives in `webwrap-spec`, not in the runtime. The verdict is a pure function of the spec and a URL, and `webwrap-runtime` refuses to compile off Windows, so policy placed there would be untestable under WSL — which would defeat the purpose of the `--inspect-self` verification approach.

`sha2`, `url`, `serde`, and `serde_json` are already present in `Cargo.lock` transitively. A and B introduce no new dependency tree.

## The app specification

JSON, `#[serde(deny_unknown_fields)]`, `#[serde(rename_all = "camelCase")]`, stored as an `RT_RCDATA` resource named `WEBWRAP_APP_SPEC`, capped at 64 KiB.

| Field | Type | Validation |
| --- | --- | --- |
| `schemaVersion` | `u32` | must equal `1` exactly |
| `appId` | `String` | exactly 16 lowercase hex characters |
| `displayName` | `String` | 1-255 characters, no control characters, no NUL |
| `slug` | `String` | `[a-z0-9-]`, 1-64 characters, no leading or trailing `-`, not a Windows reserved device name |
| `launchUrl` | `String` | canonical `https` URL |
| `trustedOrigins` | `Vec<String>` | non-empty, each `https://host[:port]` with no path, query, or fragment, no duplicates |
| `adapter` | enum | `none`, `photopea`, or `vscodeDev` |
| `fileAssociations` | `Vec<Assoc>` | may be empty; no duplicate extensions |
| `dnsMode` | enum | `system` only in phase 1 |
| `appVersion` | `String` | `major.minor.patch`, each component `u16` |

`Assoc` fields:

| Field | Type | Validation |
| --- | --- | --- |
| `extension` | `String` | leading `.` then 1-16 characters of `[a-z0-9]` |
| `mimeType` | `String` | `type/subtype`, both non-empty, no whitespace |
| `description` | `String` | 1-255 characters, no control characters, no NUL |

Field-level notes that are requirements, not preferences:

- **`slug` becomes a filename.** Rejecting the Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, case-insensitive, with or without extension) prevents the generator from being asked to publish `nul.exe`.
- **`appVersion` components are `u16`** because the PE `VERSIONINFO` resource stores four 16-bit fields. The specification carries three components; the fourth is written as `0` during specialization.
- **`trustedOrigins` entries carry no trailing slash; `launchUrl` does.** An origin is `https://www.photopea.com` while the canonical launch URL for the same site is `https://www.photopea.com/`, because canonicalization normalizes an empty path to `/` and an origin has no path at all. This asymmetry is deliberate, and comparing the two as strings is a defect — derive the origin from the URL before comparing.
- **`deny_unknown_fields` is safe here.** A generated executable only ever reads the specification embedded into it at generation time, so generator and runtime can never be at different versions. An unknown field therefore always means a bug, never an older reader. The same property makes the strict `schemaVersion == 1` equality check correct without any forward-compatibility machinery.

### Structural invariants

Two checks beyond per-field validation, both cheap, both catching generator defects the runtime would otherwise inherit silently:

1. **`launchUrl` is canonical.** Re-canonicalizing it must be a no-op. A failure means the specification was produced by a path that skipped canonicalization.
2. **`appId == app_identity(launchUrl)`.** The runtime verifies its own identity against its own URL. This catches a mis-generated or hand-edited specification and guarantees the WebView2 data directory cannot drift away from the site it belongs to.

The specification is an internal format. Users interact with generator fields and never edit JSON.

## URL canonicalization and identity

Canonicalization, in order:

1. If the input has no scheme, prepend `https://`.
2. Parse with the `url` crate. Reject anything that fails to parse.
3. Reject any scheme other than `https`.
4. Reject a URL with no host, or with embedded credentials.
5. Lowercase the host and apply IDN punycode encoding (the `url` crate does both).
6. Remove the port if it is the `https` default of 443.
7. Normalize an empty path to `/`.
8. Preserve path, query, and fragment as meaningful components.
9. Serialize back to a string. That string is the canonical URL.

Identity is `sha256(canonical_url_bytes)`, hex-encoded, truncated to the first 16 lowercase characters — 64 bits, which puts birthday collision well beyond any plausible number of generated apps for a personal tool.

The hash must be a cryptographic digest, not `std::collections::hash_map::DefaultHasher`. `DefaultHasher`'s output is explicitly not stable across Rust releases, and identity must remain stable forever: it determines whether regenerating a URL updates the existing app or creates a second one, and on Android it becomes part of the application ID.

`tests/vectors.rs` pins expected canonical strings and expected identity hashes as hardcoded literals. These vectors are a permanent contract. Changing one is a breaking change to every app already generated, not a test fix.

## Navigation policy

`trustedOrigins` gates the **file bridge**, not navigation. The parent design states that native file capabilities are active only on the configured trusted origin and that navigating away disables the bridge immediately; it separately states that non-HTTPS top-level navigation is blocked and that external pop-up links open in the system browser.

Conflating the two would break authentication: an OAuth redirect to an identity provider is a legitimate top-level navigation to an origin that is deliberately not trusted for file access.

| Event | Verdict | Reason |
| --- | --- | --- |
| top-level navigation, `https` | Allow | stays in the app; login redirects work |
| top-level navigation, any other scheme | Block | release gate 14 |
| new window or `target="_blank"` | Open in system browser | parent design's pop-up rule |
| file-bridge delivery (consumed by D) | Allow only if origin is in `trustedOrigins` | native capability boundary |

`policy.rs` exposes two independent functions:

- `nav_verdict(url, event) -> Verdict`, where `event` is `TopLevel` or `NewWindow`. It deliberately does **not** take the specification: once trusted origins are known to gate the bridge rather than navigation, the navigation verdict depends only on the scheme and the event kind. Passing a specification it cannot use would imply navigation is origin-restricted, which is exactly the misreading that breaks login.
- `is_trusted_origin(trusted_origins: &[String], url: &str) -> bool`, the bridge predicate. It takes the slice rather than the whole specification so that `policy` has no dependency on `schema` at all, which keeps the two modules independently implementable and reviewable. It compares serialized origins, never string prefixes — a prefix comparison would accept `https://www.photopea.com.evil.test`.

Both are defined and tested in A even though the bridge predicate is first used in D, so that `trustedOrigins` has enforced semantics from the moment the field exists rather than sitting unvalidated for two sub-projects.

Certificate errors are never bypassed. The runtime adds no override of WebView2's default certificate handling.

## Runtime startup

```
main()
 1. parse arguments
 2. bytes = embedded_spec::read()      FindResourceW(WEBWRAP_APP_SPEC, RT_RCDATA)
                                       on GetModuleHandleW(None)
 3. spec  = AppSpec::validate(bytes)   strict; any failure is fatal
 4. if --inspect-self                  inspect::run(&spec); exit 0
 5. data_dir = %LOCALAPPDATA%\WebWrap\<appId>\
 6. tauri::Builder
      title = spec.displayName
      WebviewUrl::External(spec.launchUrl)
      on_navigation(|url| policy::nav_verdict(url, TopLevel).is_allow())
```

Arguments are parsed before the specification is read, so that a specification failure in `--inspect-self` mode reports on stdout and stderr instead of opening a message box in a non-interactive context.

### Reading the embedded specification

The resource is read from the **loaded image** via `GetModuleHandleW(None)`, then `FindResourceW`, `SizeofResource`, `LoadResource`, and `LockResource`. It is not read by re-opening and re-parsing the executable file from disk.

This matters practically: the produced executable is portable and users rename and move it. Reading the loaded image keeps renaming, moving, and file locking harmless, and needs no file I/O at startup.

The spike proved only external read-back through its `inspect` command. Self-reading is new work here.

### WebView2 data directory

`%LOCALAPPDATA%\WebWrap\<appId>\`, read with `std::env::var("LOCALAPPDATA")`. No directory-discovery dependency is needed for a single well-known Windows environment variable.

Every generated executable shares one compiled runtime, so Windows isolation must be explicit — unlike Android, where a unique application ID provides it. Deriving the path from `appId` is what makes gate 11 true, and the `appId == app_identity(launchUrl)` invariant is what keeps it honest.

**One API item the implementation plan must verify against the pinned Tauri version rather than assume:** whether Tauri 2.11 exposes an explicit WebView2 data-directory builder method, or whether this must go through the `WEBVIEW2_USER_DATA_FOLDER` environment variable read by the WebView2 loader. Prefer an explicit builder API if one exists, because an environment variable is inherited by child processes. Record which mechanism was used and why.

### Fatal errors

Every specification failure is fatal, and must be visible. The spikes print to stderr and exit; in a release `windows_subsystem = "windows"` binary that shows the user nothing at all. Fatal errors therefore go through `MessageBoxW`, which needs no console.

Fatal conditions:

| Condition | Meaning to the user |
| --- | --- |
| resource absent | this executable was never specialized |
| resource larger than 64 KiB | rejected before parsing |
| invalid UTF-8 or invalid JSON | corrupt configuration |
| any field or invariant rejection | invalid configuration, naming the field |
| `LOCALAPPDATA` unset | the environment cannot provide isolated storage |

Exit codes follow the contract the PE spike already pinned: `0` success, `2` usage error, `1` runtime failure.

### `--inspect-self`

Prints one JSON object to stdout and exits without creating a window:

```json
{
  "schemaVersion": 1,
  "appId": "0123456789abcdef",
  "displayName": "Photopea",
  "slug": "photopea",
  "launchUrl": "https://www.photopea.com/",
  "trustedOrigins": ["https://www.photopea.com"],
  "adapter": "photopea",
  "fileAssociations": [
    { "extension": ".psd", "mimeType": "image/vnd.adobe.photoshop", "description": "Photoshop document" }
  ],
  "dnsMode": "system",
  "appVersion": "1.0.0",
  "runtimeArch": "x86_64",
  "webview2DataDir": "C:\\Users\\p\\AppData\\Local\\WebWrap\\0123456789abcdef",
  "navVerdicts": []
}
```

`runtimeArch` is `std::env::consts::ARCH`, derived from the running binary rather than stored in the specification, so it cannot disagree with the actual executable.

`--check-url <url>` is repeatable and appends `{ "url": ..., "verdict": ... }` entries to `navVerdicts`, which is how navigation policy becomes automatically asserted against a real specialized binary instead of eyeballed. Each entry is evaluated as a **top-level navigation**; the new-window verdict is constant — every pop-up opens in the system browser — so it needs no per-URL probe and is covered by the `webwrap-spec` unit tests instead.

Phase 1 exposes nothing sensitive this way: DoH is deferred, and the parent design already treats an embedded endpoint as recoverable client configuration rather than a secret.

## Architecture targets

**Every build and test command passes `--target` explicitly.** Both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`.

`rust-toolchain.toml` pins `channel = "stable"` with no target list, so on the ARM64 Windows host a naive `cargo build` produces an **aarch64** binary while every existing spike acceptance result was gathered on **x64 MSVC**. B produces the template C specializes, so the architecture cannot be left to whatever the host happens to be.

Adding `targets = [...]` to `rust-toolchain.toml` was considered and rejected. That key controls only which targets rustup *installs*; it does not change the default build target, so it would not prevent the wrong-arch build it appears to address. It would also make the WSL toolchain download two Windows standard libraries it never uses. The Windows session instead runs `rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc` once, and every command thereafter names its target.

Explicit `--target` is load-bearing beyond correctness: it routes artifacts into `target/<triple>/`, whereas `rustup run <toolchain> cargo build` on a matching host writes to plain `target/debug` — which is precisely how WSL and Windows artifacts came to clobber each other.

This also clears an existing problem: the repository accumulated fourteen ad-hoc target directories totalling 22 GB because each task invented its own `CARGO_TARGET_DIR` to stop WSL and Windows artifacts colliding. Explicit `--target` removes the need for any of them.

Windows-on-ARM emulates x64, so one ARM64 Windows host builds and tests both targets. The matrix doubles in wall-clock time, not in machines.

## Verification

| Suite | Runs on | Covers |
| --- | --- | --- |
| `cargo test -p webwrap-spec` | WSL, no target flag | canonicalization vectors, identity contract vectors, validation accept and reject table, navigation verdict table, trusted-origin predicate, both structural invariants |
| `cargo test -p webwrap-runtime --target x86_64-pc-windows-msvc` | Windows | `inspect_self.rs` |
| `cargo test -p webwrap-runtime --target aarch64-pc-windows-msvc` | Windows | `inspect_self.rs` |

`inspect_self.rs` specializes a fixture executable, spawns it with `--inspect-self` and several `--check-url` arguments, and asserts the emitted specification, `runtimeArch`, `webview2DataDir`, and navigation verdicts. It also asserts that two specifications with different launch URLs produce different data directories, which is gate 11, and that a non-HTTPS URL is blocked, which is gate 14.

### The test fixture

B's integration test needs a genuinely specialized executable, but the specializer does not land until C. The test therefore invokes the spike's existing `webwrap-pe-specialize` binary, which is still in the workspace precisely because spike deletion is staged per porter.

The rejected alternative was a debug-only `WEBWRAP_SPEC_FILE` environment variable that loads a specification from a file when no resource is found. That would leave B's real Win32 read path untested by the only test that exercises it. Using the spike binary tests the genuine path, and C's arrival is a one-line change in a test helper.

### Quality gates

`cargo clippy --all-targets -- -D warnings` for both crates on both Windows targets and for `webwrap-spec` under WSL, plus `cargo fmt --all -- --check` and `git diff --check`, matching the standard the spikes already held.

### Release gates closed

- **Gate 11** — two generated apps use separate Windows WebView2 data directories.
- **Gate 14** — the runtime rejects non-HTTPS URLs, both at specification validation and at navigation time.

Gates 1, 4, 5, 6, 7, and 8 remain open and belong to C, D, and E. This sub-project must not be described as proving them.

## Evidence

Acceptance evidence goes to `docs/evidence/2026-09-16-webwrap-spec-and-minimal-runtime.md`. `docs/spikes/` is reserved for the feasibility spikes; this is product code.

The evidence document records the exact toolchain and Windows versions, both target triples, the resolved Tauri data-directory mechanism, the test counts per suite, and the gate 11 and gate 14 observations.

## Open items for later sub-projects

- **C:** whether generation emits one executable per architecture or one executable with an architecture chosen per generation. This changes gate 1's stated `twitter.exe` output name and the generator's UI, so it is decided in C's design rather than assumed here.
- **C:** the generator's own architecture, and whether it embeds one wrapper template or both.
- **D:** single-instance discovery must use an identity-derived mutex and IPC endpoint, not Tauri's compile-time bundle identifier, because every generated executable shares one compiled runtime.

## Key references

- [Win32 FindResourceW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-findresourcew)
- [Win32 LockResource](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-lockresource)
- [WebView2 user data folder](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)
- [Tauri platform WebView access](https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindow.html#method.with_webview)
- [PE VERSIONINFO resource](https://learn.microsoft.com/en-us/windows/win32/menurc/vs-versioninfo)
- [Windows reserved file names](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)
- [WHATWG URL Standard](https://url.spec.whatwg.org/)
