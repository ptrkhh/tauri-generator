# Windows PE Specialization Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove that a precompiled Windows wrapper can be copied, specialized with an app specification, icon, display metadata, and version, then published as one self-contained `<slug>.exe` with no sidecar files.

**Architecture:** Add a Windows-only Rust spike that treats an existing PE executable as an immutable base, edits its resource directory with `editpe`, validates a complete round trip by parsing the staged output, and atomically replaces the requested output path. The spike exposes a small structured-argument CLI for repeatable evidence; generator UI, runtime resource loading, file associations, and signing remain separate work.

**Tech Stack:** Rust workspace, `editpe` 0.2.4 with its `images` feature, `serde_json` for the embedded spec, and the Windows `MoveFileExW` API for atomic publication. `editpe`'s `Image`, `ResourceDirectory`, `ResourceTable`, `ResourceEntry`, `ResourceEntryName`, `ResourceData`, and `VersionInfo` APIs are the implementation surface ([Image](https://docs.rs/editpe/0.2.4/editpe/struct.Image.html), [ResourceDirectory](https://docs.rs/editpe/0.2.4/editpe/struct.ResourceDirectory.html), [ResourceEntryName](https://docs.rs/editpe/0.2.4/editpe/enum.ResourceEntryName.html)).

**Spec:** `docs/superpowers/specs/2026-09-14-tauri-web-app-generator-design.md` (Windows artifact generation and embedded app specification sections).

## Global Constraints

- Build and run this spike on Windows x64 with the MSVC Rust target; non-Windows builds fail clearly at compile time.
- The base executable is never modified in place; the final output is published only after all resource edits and read-back validation pass.
- The embedded resource name is `WEBWRAP_APP_SPEC`, type `RT_RCDATA`, language `LANGUAGE_ID_EN_US`, and its bytes are a UTF-8 JSON object containing at least `schemaVersion`, `appId`, `displayName`, `slug`, and `launchUrl`.
- Icon input is a local PNG/ICO path; invalid or oversized input fails before publication. The spike reuses the existing `spikes/photopea-windows-writeback/icons/icon.png` fixture for deterministic tests and does not add a second binary fixture.
- Display name, version, resource names, and paths are validated before entering PE resource or filesystem contexts; no shell-composed packaging commands are used.
- A failed specialization leaves an existing output byte-for-byte unchanged and removes its temporary staging file.
- This spike validates PE resource round-trip and single-file output only. Runtime self-reading, Windows association registration, signing, and generator UX are later plans.

---

## File map

- Modify: `Cargo.toml` — add the spike crate to the workspace.
- Modify: `Cargo.lock` — lock `editpe` and its image-conversion dependencies.
- Create: `spikes/windows-pe-specialization/Cargo.toml` — Windows-only spike package and pinned dependencies.
- Create: `spikes/windows-pe-specialization/src/lib.rs` — expose the resource-specialization module to the CLI and integration tests.
- Create: `spikes/windows-pe-specialization/src/main.rs` — structured CLI (`specialize` and `inspect`) and argument validation.
- Create: `spikes/windows-pe-specialization/src/pe.rs` — resource editing, round-trip inspection, staging, and atomic publication.
- Create: `spikes/windows-pe-specialization/tests/pe_specialization.rs` — Windows integration tests using the current test PE as an immutable base.
- Create: `docs/spikes/2026-09-15-windows-pe-specialization.md` — exact build, specialization, inspection, hash, and failure evidence.

The crate deliberately has no Tauri window or frontend. It is the smallest executable proof of the packaging boundary that the eventual generator will call.

### Task 1: Scaffold the Windows-only package and validate structured inputs

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `spikes/windows-pe-specialization/Cargo.toml`
- Create: `spikes/windows-pe-specialization/src/lib.rs`
- Create: `spikes/windows-pe-specialization/src/main.rs`
- Create: `spikes/windows-pe-specialization/src/pe.rs`
- Test: `spikes/windows-pe-specialization/src/main.rs` (unit tests for argument validation)

**Interfaces:**
- Produces `SpecializeArgs { input: PathBuf, output: PathBuf, spec: PathBuf, icon: PathBuf, display_name: String, version: String }` for the `specialize` command.
- Produces `InspectArgs { input: PathBuf }` for the `inspect` command.
- Produces `pe::validate_spec(&[u8]) -> Result<Vec<u8>, PeError>` and `pe::validate_text(&str, field: &str) -> Result<(), PeError>` for later resource work.
- `main` must pass parsed paths and byte buffers as structured values; no command string is built or re-parsed.

- [ ] **Step 1: Write the failing input tests.** Cover zero arguments, unknown subcommands, missing required flags, `input == output`, empty/control-character display names, and specs that are not a UTF-8 JSON object. Use assertions such as:

```rust
assert!(parse_args(["specialize", "--input", "a.exe"]).is_err());
assert!(validate_text("", "display-name").is_err());
assert!(validate_spec(br#"[1, 2, 3]"#).is_err());
```

- [ ] **Step 2: Run the focused tests and confirm they fail because the parser and validators do not exist.**

Run:

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test -p windows-pe-specialization-spike --lib
```

Expected: compilation failure naming the missing parser/validator symbols.

- [ ] **Step 3: Add the minimum package and parser.** Use this dependency shape in `spikes/windows-pe-specialization/Cargo.toml`:

```toml
[package]
name = "windows-pe-specialization-spike"
version = "0.1.0"
edition = "2021"

[dependencies]
editpe = { version = "0.2.4", features = ["images"] }
serde_json = "1"
windows = { version = "0.61", features = ["Win32_Storage_FileSystem"] }

[lib]
path = "src/lib.rs"

[[bin]]
name = "webwrap-pe-specialize"
path = "src/main.rs"
```

Add `#[cfg(not(windows))] compile_error!("the Windows PE specialization spike must be built and run on Windows");` to `src/lib.rs`, and re-export `pub mod pe;`. Parse only the explicit `specialize`/`inspect` subcommands and their named flags. `validate_spec` must parse a JSON object, require integer `schemaVersion` and string `appId`, `displayName`, `slug`, and `launchUrl` members, then return the original UTF-8 bytes. `validate_text` rejects empty values, NUL/control characters, and values longer than 255 UTF-8 bytes.

- [ ] **Step 4: Run the focused tests and formatting checks.**

Run:

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test -p windows-pe-specialization-spike --lib
cargo fmt --all -- --check
```

Expected: all parser/validator tests pass and rustfmt reports no changes.

- [ ] **Step 5: Commit the scaffold.**

```bash
git add Cargo.toml Cargo.lock spikes/windows-pe-specialization
git commit -m "test(spike): define PE specialization inputs"
```

### Task 2: Embed the app specification, icon, and Windows version metadata

**Files:**
- Modify: `spikes/windows-pe-specialization/src/pe.rs`
- Create: `spikes/windows-pe-specialization/tests/pe_specialization.rs`

**Interfaces:**
- Consumes `validate_spec`, `validate_text`, and `SpecializeArgs` from Task 1.
- Produces `pe::edit_resources(image: &mut editpe::Image<'static>, spec: &[u8], icon_path: &Path, display_name: &str, version: &str) -> Result<(), PeError>`.
- Produces `pe::read_embedded_spec(path: &Path) -> Result<Vec<u8>, PeError>` and `pe::read_metadata(path: &Path) -> Result<Metadata, PeError>`; later publication code calls these against its staged file. `Metadata` has public fields `spec: Vec<u8>`, `icon_present: bool`, `file_description: String`, and `product_version: String`.

- [ ] **Step 1: Write the round-trip tests before resource editing.** Use `std::env::current_exe()` as the immutable PE input so tests do not commit a generated executable. Copy the output into a unique temp directory. Reuse `PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../photopea-windows-writeback/icons/icon.png")` as the icon fixture. Assert that the output's custom resource bytes equal the JSON input, `get_main_icon()` returns non-empty bytes, and version strings contain `FileDescription`, `ProductName`, `FileVersion`, and `ProductVersion` with the requested values.

Define the test-only `temp_dir() -> PathBuf` helper with `std::env::temp_dir()`, the process ID, an atomic counter, `create_dir`, and cleanup at the end of each test; do not use a fixed shared path.

```rust
let spec = br#"{"schemaVersion":1,"appId":"a123","displayName":"Photopea","slug":"photopea","launchUrl":"https://www.photopea.com/"}"#;
let input = std::env::current_exe().unwrap();
let icon = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../photopea-windows-writeback/icons/icon.png");
let mut image = editpe::Image::parse_file(&input).unwrap();
edit_resources(&mut image, spec, &icon, "Photopea", "1.2.3").unwrap();
let output = temp_dir().join("specialized.exe");
image.write_file(&output).unwrap();
let metadata = read_metadata(&output).unwrap();
assert_eq!(metadata.spec, spec);
assert!(metadata.icon_present);
assert_eq!(metadata.file_description, "Photopea");
assert_eq!(metadata.product_version, "1.2.3");
```

- [ ] **Step 2: Run the new tests and confirm they fail because no resource tree is written.**

Run:

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test -p windows-pe-specialization-spike --test pe_specialization
```

Expected: the round-trip assertions fail or return a missing-resource error.

- [ ] **Step 3: Build the resource tree with `editpe`.** Keep existing resources by cloning `image.resource_directory().cloned().unwrap_or_default()`. Add the spec under the exact three-level PE path using `ResourceTable::insert`, `ResourceEntry::Table`, and `ResourceEntry::Data`:

```rust
let mut data = ResourceData::default();
data.set_data(spec.to_vec());
data.set_codepage(CODE_PAGE_ID_EN_US);

let mut languages = ResourceTable::default();
languages.insert(
    ResourceEntryName::ID(LANGUAGE_ID_EN_US as u32),
    ResourceEntry::Data(data),
);

let mut names = ResourceTable::default();
names.insert(
    ResourceEntryName::from_string("WEBWRAP_APP_SPEC"),
    ResourceEntry::Table(languages),
);

resources.root_mut().insert(
    ResourceEntryName::ID(RT_RCDATA as u32),
    ResourceEntry::Table(names),
);
```

Before `set_resource_directory`, call `remove_main_icon()` and `set_main_icon_file(icon_path.to_str().ok_or(PeError::InvalidPath)?)`. Then clone or default the existing `VersionInfo`, ensure it has one `VersionStringTable`, and update only these string keys: `FileDescription`, `ProductName`, `FileVersion`, and `ProductVersion`. Preserve the existing fixed-file info when present. Call `resources.set_version_info(&version_info)` and finally `image.set_resource_directory(resources)`.

- [ ] **Step 4: Implement read-back helpers and make the tests pass.** `read_embedded_spec` must traverse `root().get(ID(RT_RCDATA)) -> as_table() -> get(Name("WEBWRAP_APP_SPEC")) -> as_table() -> get(ID(LANGUAGE_ID_EN_US)) -> as_data() -> data()`, then re-run `validate_spec` before returning. `read_metadata` must use `get_main_icon()` and `get_version_info()`; a missing icon, version table, or custom resource is an error, not a warning.

- [ ] **Step 5: Run focused tests, Clippy, and formatting.**

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test -p windows-pe-specialization-spike --test pe_specialization
rustup run stable-x86_64-pc-windows-msvc cargo clippy -p windows-pe-specialization-spike --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: the resource round-trip and metadata tests pass with no warnings.

- [ ] **Step 6: Commit the resource implementation.**

```bash
git add spikes/windows-pe-specialization/src/pe.rs spikes/windows-pe-specialization/tests/pe_specialization.rs
git commit -m "feat(spike): embed PE app resources and icon"
```

### Task 3: Stage and atomically publish the single-file executable

**Files:**
- Modify: `spikes/windows-pe-specialization/src/pe.rs`
- Modify: `spikes/windows-pe-specialization/src/main.rs`
- Modify: `spikes/windows-pe-specialization/tests/pe_specialization.rs`

**Interfaces:**
- Consumes `edit_resources`, `read_embedded_spec`, and `read_metadata` from Task 2.
- Produces `pe::specialize(input: &Path, output: &Path, spec: &[u8], icon: &Path, display_name: &str, version: &str) -> Result<Metadata, PeError>`; it returns the read-back metadata only after the final staged image is valid.
- The CLI `specialize` subcommand calls this function and prints one JSON result; `inspect` calls the read-back helpers and prints the same metadata shape.

- [ ] **Step 1: Add failing transaction tests.** Cover:
  - a successful replacement of an existing output and exact custom-resource round trip;
  - an invalid PE input that leaves a pre-existing output byte-for-byte unchanged;
  - an invalid icon path that leaves no output when none existed;
  - a failed validation that leaves no `.webwrap-pe-*.tmp` residue in the output directory;
  - rejection when input and output resolve to the same file.

```rust
let temp = temp_dir();
let bad_input = temp.join("not-a-pe.exe");
let output = temp.join("twitter.exe");
let icon = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../photopea-windows-writeback/icons/icon.png");
let spec = br#"{"schemaVersion":1,"appId":"a123","displayName":"Photopea","slug":"photopea","launchUrl":"https://www.photopea.com/"}"#;
std::fs::write(&output, b"previous output").unwrap();
std::fs::write(&bad_input, b"not a PE").unwrap();
let before = std::fs::read(&output).unwrap();
assert!(specialize(&bad_input, &output, spec, &icon, "Photopea", "1.2.3").is_err());
assert_eq!(std::fs::read(&output).unwrap(), before);
assert!(!std::fs::read_dir(output.parent().unwrap()).unwrap()
    .map(Result::unwrap)
    .any(|entry| entry.file_name().to_string_lossy().starts_with(".webwrap-pe-")));
```

- [ ] **Step 2: Run the transaction tests and confirm they fail before staging/publish exists.**

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test -p windows-pe-specialization-spike --test pe_specialization
```

Expected: the transaction tests fail with missing `specialize` behavior.

- [ ] **Step 3: Implement staging and publication.** Validate/canonicalize the input and output parent, reject identical canonical paths, parse the input with `Image::parse_file`, edit resources, and write to a `create_new` temp file in the output directory. Reopen the temp with `Image::parse_file` and require `read_embedded_spec(temp) == spec` plus successful `read_metadata(temp)` before publishing. Flush the staged file if the writer exposes a handle; then call `MoveFileExW(temp, output, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`. Remove the temp on every error path. Never delete the destination before `MoveFileExW`.

- [ ] **Step 4: Implement the two structured CLI commands.** Accept only these forms:

```text
webwrap-pe-specialize.exe specialize --input BASE.exe --output twitter.exe --spec app.json --icon icon.png --display-name Twitter --version 1.0.0
webwrap-pe-specialize.exe inspect --input twitter.exe
```

Read `--spec` and validate its bytes, validate the display name/version, and print JSON containing `output`, `specBytes`, `iconPresent`, `fileDescription`, and `productVersion`. `inspect` must never modify its input. Invalid flags, missing values, and extra positional arguments exit with code 2; specialization failures exit with code 1.

- [ ] **Step 5: Run all crate tests and static checks.**

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test -p windows-pe-specialization-spike
rustup run stable-x86_64-pc-windows-msvc cargo clippy -p windows-pe-specialization-spike --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all transaction, round-trip, and CLI tests pass with no warnings.

- [ ] **Step 6: Commit the atomic publisher.**

```bash
git add spikes/windows-pe-specialization/src/main.rs spikes/windows-pe-specialization/src/pe.rs spikes/windows-pe-specialization/tests/pe_specialization.rs
git commit -m "feat(spike): publish specialized PE atomically"
```

### Task 4: Run the Windows acceptance probe and record the packaging gate

**Files:**
- Create: `docs/spikes/2026-09-15-windows-pe-specialization.md`

**Interfaces:**
- Consumes the `webwrap-pe-specialize.exe` binary and CLI from Task 3.
- Produces a reproducible evidence record proving the base PE remains usable, the output contains the JSON/icon/version resources, and failed runs do not publish partial outputs.

- [ ] **Step 1: Build a clean x64 binary and create a temporary app spec.** Use the repository's MSVC environment and a target directory outside the source tree's generated files:

```powershell
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo build -p photopea-windows-writeback-spike -p windows-pe-specialization-spike --target-dir D:/git/pwa_ganerator/target/pe-specialization-x64"
$spec = Join-Path $env:TEMP 'webwrap-twitter-spec.json'
[IO.File]::WriteAllBytes($spec, [Text.Encoding]::UTF8.GetBytes('{"schemaVersion":1,"appId":"a123","displayName":"Twitter","slug":"twitter","launchUrl":"https://twitter.com/"}'))
```

- [ ] **Step 2: Specialize the built PE and inspect it with the executable itself.** Use the built spike executable as the immutable base and the existing PNG fixture as the icon input:

```powershell
$tool = 'D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe'
$base = 'D:\git\pwa_ganerator\target\pe-specialization-x64\debug\photopea-windows-writeback-spike.exe'
$out = Join-Path $env:TEMP 'twitter.exe'
$icon = 'D:\git\pwa_ganerator\.worktrees\photopea-windows-writeback\spikes\photopea-windows-writeback\icons\icon.png'
& $tool specialize --input $base --output $out --spec $spec --icon $icon --display-name Twitter --version 1.0.0
& $tool inspect --input $out
```

Expected: inspect reports the exact spec, `iconPresent=true`, `fileDescription=Twitter`, and `productVersion=1.0.0`; only `twitter.exe` is created next to the temporary spec/icon inputs.

- [ ] **Step 3: Prove replacement and failure preservation.** Run specialization twice with a changed display name/version and confirm the same output path is replaced. Then pass a text file as `--input` and a missing icon; capture exit code 1 and verify the previous output hash is unchanged and no `.webwrap-pe-*.tmp` file remains.

- [ ] **Step 4: Record exact output hashes, resource read-back, environment versions, and unsupported scope** in `docs/spikes/2026-09-15-windows-pe-specialization.md`. State explicitly that this gate proves single-file PE specialization, not runtime resource consumption or file association registration.

- [ ] **Step 5: Run final repository checks and commit the evidence.**

```bash
rustup run stable-x86_64-pc-windows-msvc cargo test --workspace --target-dir D:/git/pwa_ganerator/target/pe-specialization-x64
cargo fmt --all -- --check
git diff --check
git add docs/spikes/2026-09-15-windows-pe-specialization.md
git commit -m "docs: record PE specialization evidence"
```

## Self-review checklist

- Spec coverage: the plan implements the Windows packager's copy/edit/read-back/atomic-publish sequence and the single-file/no-sidecar constraint. Runtime self-reading, associations, signing, Android, DNS, and generator UX remain explicitly outside this spike as required by the delivery sequence.
- Placeholder scan: no step depends on a future decision or an unspecified helper; every named interface and resource path is defined above.
- Type consistency: `specialize` produces `Metadata`; `inspect` consumes the same read-back helpers; the CLI passes validated byte buffers and paths without shell composition.

Plan complete and saved to `docs/superpowers/plans/2026-09-15-windows-pe-specialization-spike.md`. Because the project already selected subagent-driven execution, implementation should dispatch one fresh worker per task with a review checkpoint after each task.
