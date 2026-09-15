# Windows PE specialization spike

Date: 2026-09-15
Gate: passed for the bounded single-file PE specialization spike described below

This spike proves that `webwrap-pe-specialize.exe` can copy an immutable base
PE, embed a `WEBWRAP_APP_SPEC` `RT_RCDATA` resource plus an icon and version
strings, read all of that back from the staged file, and publish the result
atomically without ever mutating the base binary in place — and that a
runtime failure (bad input PE, missing icon) leaves any pre-existing output
byte-for-byte unchanged with no temp file left behind. See "Scope boundary"
at the end for what this does **not** prove.

## Build and environment

The implementation is split across these commits (this worktree, branch
`spike/photopea-windows-writeback`):

| Commit | Result |
| --- | --- |
| `1e0e2c6` | spike plan and task ledger |
| `271da38` | PE specialization CLI/input tests |
| `41132ef`, `8e1f059` | input validation hardening |
| `6226303` | embed PE app resources and icon |
| `e689bbd` | harden PE resource round trip |
| `2fd88ee` | publish specialized PE atomically |
| `cdf50d5` | harden PE publication transaction |
| `b4c537b` | classify spec I/O failures |
| `330b8fb` | implementation handover docs |

Repository HEAD at the time this evidence was recorded: `330b8fb818c5caeeb069708cf8a443aa647455ad`.

Toolchain and OS versions, captured by running the commands below:

- `cmd.exe /d /c "ver"` → `Microsoft Windows [Version 10.0.26200.9445]`
- WSL2 shell: `uname -a` → `Linux LENOVOnyaPATRICK 6.18.33.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun 18 21:38:49 UTC 2026 aarch64 aarch64 aarch64 GNU/Linux`
- `rustup.exe run stable-x86_64-pc-windows-msvc rustc -V` → `rustc 1.97.0 (2d8144b78 2026-07-07)`
- `rustup.exe run stable-x86_64-pc-windows-msvc cargo -V` → `cargo 1.97.0 (c980f4866 2026-06-30)`
- `rustup.exe show` (inside a `vcvars64.bat`-initialized shell) reported:
  `Default host: aarch64-pc-windows-msvc`; installed toolchains
  `stable-aarch64-pc-windows-msvc` (default) and `stable-x86_64-pc-windows-msvc`;
  active toolchain for this worktree is `stable-aarch64-pc-windows-msvc`,
  overridden per-invocation to `stable-x86_64-pc-windows-msvc` via
  `rustup run stable-x86_64-pc-windows-msvc` for every Windows-target command
  below (the worktree's `rust-toolchain.toml` pins the aarch64 host default,
  so the x64 target is always selected explicitly).

All Windows-side commands were run from WSL bash by invoking `cmd.exe`. WSL
paths under `/mnt/d/...` and `/mnt/c/...` were used directly for reading
files and computing SHA-256 hashes with `sha256sum`.

### cmd.exe pitfall discovered and worked around

`cmd.exe /d /c "cmd1 & echo EXIT=%ERRORLEVEL%"` expands `%ERRORLEVEL%` at
**parse time**, before `cmd1` runs, because the whole quoted string is one
logical line. This was caught empirically:

```
$ cmd.exe /d /c "dir /zzzinvalid & echo EXIT=%ERRORLEVEL%"
Invalid switch - "zzzinvalid".
EXIT=0
```

`dir` actually fails (exit 1) but the stale, parse-time-expanded
`%ERRORLEVEL%` printed `0`. The fix is delayed expansion:

```
$ cmd.exe /d /v:on /c "dir /zzzinvalid & echo EXIT=!ERRORLEVEL!"
Invalid switch - "zzzinvalid".
EXIT=1
```

Every exit code recorded below was captured with `cmd.exe /d /v:on /c "... & echo EXIT=!ERRORLEVEL!"`.
The first pass at Step 2/Step 3 probes (before this fix was found) used the
stale `%ERRORLEVEL%` form; those probes were re-run cleanly afterward, and
where a probe was run twice (once stale, once corrected) the tool's JSON
output and the resulting output-file SHA-256 were byte-identical both times,
which also served as an incidental determinism check.

## Build (reproduction step)

This exact command was run before this task began (per task instructions,
not re-run in this session) and produced the two binaries used below,
finishing with `Finished dev profile [unoptimized + debuginfo] target(s) in 4m 49s`:

```
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo build -p photopea-windows-writeback-spike -p windows-pe-specialization-spike --target-dir D:/git/pwa_ganerator/target/pe-specialization-x64"
```

Resulting artifacts (sizes from `ls -la` against the WSL-mapped path):

| Path | Size (bytes) |
| --- | --- |
| `D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe` | 2,856,448 |
| `D:\git\pwa_ganerator\target\pe-specialization-x64\debug\photopea-windows-writeback-spike.exe` (immutable base) | 13,456,384 |

Base binary SHA-256, confirmed identical before the first probe and after
every probe below (seven separate `sha256sum` checks across the session, all
equal):

```
689ae2aa3e75da0a561af36dd2a177418480e62a1bcb657050693d4a8cdb1884
```

## Fixtures

Spec JSON, written as BOM-free UTF-8 bytes directly from WSL bash
(`printf '%s' '...' > ...`, **not** PowerShell `Out-File`/`>`, which would add
a BOM or emit UTF-16) to `C:\Users\p\AppData\Local\Temp\webwrap-twitter-spec.json`:

```json
{"schemaVersion":1,"appId":"a123","displayName":"Twitter","slug":"twitter","launchUrl":"https://twitter.com/"}
```

Verified 110 bytes, no BOM (`xxd` shows the file starts `7b 22 73 63 68 65 6d 61 ...` = `{"schema...`).

Icon fixture (existing repo asset, read-only, never modified):
`D:\git\pwa_ganerator\.worktrees\photopea-windows-writeback\spikes\photopea-windows-writeback\icons\icon.png` (49,979 bytes).

Bad-PE-input fixture for the failure probe, written the same BOM-free way to
`C:\Users\p\AppData\Local\Temp\webwrap-bad-input.txt`, content `this is not a PE file` (21 bytes, no trailing newline).

All temporary artifacts (spec JSON, `twitter.exe`, the bad-input text file)
were placed under `C:\Users\p\AppData\Local\Temp` (`%TEMP%`), outside the
repository/worktree. Nothing generated by these probes was written inside
the worktree.

## Probe 1: specialize + inspect (brief Step 2)

Command:

```
cmd.exe /d /v:on /c "D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe specialize --input D:\git\pwa_ganerator\target\pe-specialization-x64\debug\photopea-windows-writeback-spike.exe --output C:\Users\p\AppData\Local\Temp\twitter.exe --spec C:\Users\p\AppData\Local\Temp\webwrap-twitter-spec.json --icon D:\git\pwa_ganerator\.worktrees\photopea-windows-writeback\spikes\photopea-windows-writeback\icons\icon.png --display-name Twitter --version 1.0.0 & echo EXIT=!ERRORLEVEL!"
```

Observed stdout:

```
{"fileDescription":"Twitter","iconPresent":true,"output":"C:\\Users\\p\\AppData\\Local\\Temp\\twitter.exe","productVersion":"1.0.0","specBytes":110}
EXIT=0
```

Inspect command:

```
cmd.exe /d /v:on /c "D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe inspect --input C:\Users\p\AppData\Local\Temp\twitter.exe & echo EXIT=!ERRORLEVEL!"
```

Observed stdout:

```
{"fileDescription":"Twitter","iconPresent":true,"output":"C:\\Users\\p\\AppData\\Local\\Temp\\twitter.exe","productVersion":"1.0.0","specBytes":110}
EXIT=0
```

This matches the brief's expectation exactly: `specBytes=110` (the exact
spec file size), `iconPresent=true`, `fileDescription=Twitter`,
`productVersion=1.0.0`. This resource read-back was performed by the tool's
own `inspect` subcommand reading the published file — that is the "resource
read-back" evidence for this gate.

`twitter.exe` SHA-256 after this run:

```
274d6adcacf1bf64df9f00a8f9408ad9c5446bcf7470f2f5de94443c7c501854
```

(This run was executed twice — once before the `%ERRORLEVEL%` fix was found,
once after — and produced this identical hash both times.)

## Probe 2: replacement in place (brief Step 3, part 1)

`specialize` run a second time to the **same** output path with a different
display name and version:

```
cmd.exe /d /v:on /c "D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe specialize --input D:\git\pwa_ganerator\target\pe-specialization-x64\debug\photopea-windows-writeback-spike.exe --output C:\Users\p\AppData\Local\Temp\twitter.exe --spec C:\Users\p\AppData\Local\Temp\webwrap-twitter-spec.json --icon D:\git\pwa_ganerator\.worktrees\photopea-windows-writeback\spikes\photopea-windows-writeback\icons\icon.png --display-name TwitterV2 --version 2.0.0 & echo EXIT=!ERRORLEVEL!"
```

Observed stdout:

```
{"fileDescription":"TwitterV2","iconPresent":true,"output":"C:\\Users\\p\\AppData\\Local\\Temp\\twitter.exe","productVersion":"2.0.0","specBytes":110}
EXIT=0
```

`inspect` on the replaced file:

```
cmd.exe /d /v:on /c "D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe inspect --input C:\Users\p\AppData\Local\Temp\twitter.exe & echo EXIT=!ERRORLEVEL!"
```

Observed stdout:

```
{"fileDescription":"TwitterV2","iconPresent":true,"output":"C:\\Users\\p\\AppData\\Local\\Temp\\twitter.exe","productVersion":"2.0.0","specBytes":110}
EXIT=0
```

`twitter.exe` SHA-256 after replacement:

```
1afb8a38708d008e67f0b0c38fa20c611b7d16ce2c89c52dfc48583b0305aa18
```

This differs from the Probe 1 hash (`274d6adc...` vs `1afb8a38...`), and a
directory listing of `%TEMP%` confirmed exactly one `twitter.exe` — no second
file was created by the replacement. (This run was also executed twice, once
stale-`%ERRORLEVEL%` and once corrected, with an identical resulting hash
both times.)

## Probe 3a: bad `--input` (text file), valid icon (brief Step 3, part 2)

Command:

```
cmd.exe /d /v:on /c "D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe specialize --input C:\Users\p\AppData\Local\Temp\webwrap-bad-input.txt --output C:\Users\p\AppData\Local\Temp\twitter.exe --spec C:\Users\p\AppData\Local\Temp\webwrap-twitter-spec.json --icon D:\git\pwa_ganerator\.worktrees\photopea-windows-writeback\spikes\photopea-windows-writeback\icons\icon.png --display-name BadInput --version 9.0.0 & echo EXIT=!ERRORLEVEL!"
```

Observed stdout/stderr:

```
invalid header: no dos magic
EXIT=1
```

Post-probe checks:

- `twitter.exe` SHA-256: `1afb8a38708d008e67f0b0c38fa20c611b7d16ce2c89c52dfc48583b0305aa18` — unchanged from Probe 2.
- Base binary SHA-256: `689ae2aa3e75da0a561af36dd2a177418480e62a1bcb657050693d4a8cdb1884` — unchanged.
- `find /mnt/c/Users/p/AppData/Local/Temp/ -maxdepth 1 -iname '*webwrap-pe*'` returned nothing — no `.webwrap-pe-*.tmp` file exists. (Note: a broad `-iname '*.tmp'` grep over `%TEMP%` turns up dozens of unrelated pre-existing `.tmp` files from other, unrelated Windows applications on this machine; the correct check is for the tool's own `webwrap-pe-*` prefix, which found nothing.)

## Probe 3b: valid PE base, missing `--icon` path (brief Step 3, part 2)

Confirmed first that the icon path does not exist:
`ls /mnt/c/Users/p/AppData/Local/Temp/nonexistent-icon.png` → `No such file or directory`.

Command:

```
cmd.exe /d /v:on /c "D:\git\pwa_ganerator\target\pe-specialization-x64\debug\webwrap-pe-specialize.exe specialize --input D:\git\pwa_ganerator\target\pe-specialization-x64\debug\photopea-windows-writeback-spike.exe --output C:\Users\p\AppData\Local\Temp\twitter.exe --spec C:\Users\p\AppData\Local\Temp\webwrap-twitter-spec.json --icon C:\Users\p\AppData\Local\Temp\nonexistent-icon.png --display-name MissingIcon --version 8.0.0 & echo EXIT=!ERRORLEVEL!"
```

Observed stdout/stderr:

```
io error: The system cannot find the file specified. (os error 2)
EXIT=1
```

Post-probe checks:

- `twitter.exe` SHA-256: `1afb8a38708d008e67f0b0c38fa20c611b7d16ce2c89c52dfc48583b0305aa18` — unchanged from Probe 2 and from Probe 3a.
- Base binary SHA-256: `689ae2aa3e75da0a561af36dd2a177418480e62a1bcb657050693d4a8cdb1884` — unchanged.
- `find /mnt/c/Users/p/AppData/Local/Temp/ -maxdepth 1 -iname '*webwrap-pe*'` returned nothing.

Both failure probes exited 1 (a runtime failure per the Task 3 CLI contract,
not a usage error), left the previously published `twitter.exe`
byte-for-byte unchanged, left the immutable base untouched, and left no
`webwrap-pe-*` temp file behind.

## Final repository checks (brief Step 5)

```
cmd.exe /d /v:on /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo test --workspace --target-dir D:/git/pwa_ganerator/target/pe-specialization-x64 & echo TEST_EXIT=!ERRORLEVEL!"
```

Result: all workspace tests passed — 17 (`photopea-windows-writeback-spike`) +
7 (`vscode-webview2-handle-spike`) + 3 (`windows-pe-specialization-spike` lib
unit tests) + 0 (`webwrap-pe-specialize` binary, no `#[test]`s of its own) +
5 (`pe_specialization` integration tests, including
`specializes_and_replaces_output_atomically`,
`embeds_spec_icon_and_version_metadata`,
`failed_specialization_preserves_existing_output_and_cleans_stage`,
`invalid_icon_and_same_input_output_are_rejected_without_output`,
`rejects_output_symlink_alias_of_input`) = 32 tests, 0 failed.
`TEST_EXIT=0`.

```
cargo fmt --all -- --check
```

Run directly in WSL bash (native `stable-aarch64-unknown-linux-gnu`
toolchain, `rustfmt 1.9.0-stable`). No output, exit code 0.

```
git diff --check
```

Run directly in WSL bash. No output, exit code 0. Note: this check was run
before `docs/spikes/2026-09-15-windows-pe-specialization.md` was created and
staged, so at that point it could not have scanned the new file (an
untracked file produces no `git diff` hunks). After staging the new file
with `git add`, `git diff --cached --check` was also run as a belt-and-braces
check on the new document itself; it likewise reported no output and exit
code 0.

## Scope boundary

This gate proves **single-file PE specialization only**: given an immutable
base PE, a JSON app spec, a PNG icon, and display-name/version strings, the
tool produces one output executable containing an embedded
`WEBWRAP_APP_SPEC` `RT_RCDATA` resource, a replaced icon, and updated version
strings; it publishes that output atomically (`MoveFileExW` with
`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`); a runtime failure
leaves any pre-existing output byte-for-byte unchanged and leaves no
`.webwrap-pe-*.tmp` file behind; and the immutable base is never modified in
place (verified by identical SHA-256 across every probe).

The following are explicitly **not** proven by this gate and remain future
work:

- **Runtime self-reading of the embedded resource** — nothing in this spike
  demonstrates the produced executable reading its own
  `WEBWRAP_APP_SPEC` resource at runtime to configure itself; `inspect` reads
  it from outside the process, via the same file-parsing code path used to
  write it.
- **File-association registration** (registry entries, protocol/URL
  handlers, "Open with" integration).
- **Code signing** (Authenticode or otherwise) of the produced executable.
- **APK generation** or any Android packaging path.
- **DNS enforcement** or any runtime network-policy behavior.
- **Icon search** (locating/selecting an appropriate icon from a set of
  candidates); this spike only accepts one already-chosen PNG path.
- **Generator UX** (any user-facing flow that would drive this CLI from a
  packaging wizard or web UI).

## Files changed by this task

Only this document was added:

- `docs/spikes/2026-09-15-windows-pe-specialization.md`

No generated executables, the temporary spec JSON, or anything under
`.superpowers/` (git-ignored) were committed.
