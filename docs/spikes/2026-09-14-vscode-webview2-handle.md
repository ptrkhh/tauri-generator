# vscode.dev writable-handle spike

Date: 2026-09-14

## Result

**PASS — gate satisfied.** The exact tested executable automatically opened the supplied local Markdown file in `vscode.dev`; one trusted Ctrl+S changed the original file bytes, and the hostile-origin check delivered neither a native handle nor input events.

This is a measured feasibility spike only. It does not implement file associations, packaging, Photopea, Android, custom DNS, or the generator UI.

## Exact environment

- Git commit tested: `34e5f6aa0165f4ed722a391fa320414ed148df10`
- Rust: `rustc 1.97.0 (2d8144b78 2026-07-07)`
- Rust host/target: `x86_64-pc-windows-msvc`
- Rust LLVM: `22.1.6`
- Windows: `Microsoft Windows NT 10.0.26200.0`
- Windows architecture: `ARM64`
- WebView2 runtime: `152.0.4191.66`
- Build output: `target/task4-final-x64/debug/vscode-webview2-handle-spike.exe`

Rust and the executable were built with the native x64 MSVC toolchain on the ARM64 Windows host. WSL only invoked commands and inspected results; it was not used as the GUI build/runtime.

The commit identity was collected with `git rev-parse HEAD` (output:
`34e5f6aa0165f4ed722a391fa320414ed148df10`). The toolchain identity was
collected in the same native developer prompt with:

```text
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc rustc -Vv"
```

The relevant `rustc -Vv` output was:

```text
rustc 1.97.0 (2d8144b78 2026-07-07)
binary: rustc
commit-hash: 2d8144b7880597b6e6d3dfd63a9a9efae3f533d3
commit-date: 2026-07-07
host: x86_64-pc-windows-msvc
release: 1.97.0
LLVM version: 22.1.6
```

The Windows and WebView2 values were collected with:

```text
$programFilesX86 = [Environment]::GetFolderPath("ProgramFilesX86"); "OS=" + [Environment]::OSVersion.VersionString; "PROCESSOR_ARCHITECTURE=" + $env:PROCESSOR_ARCHITECTURE; $version = Get-ChildItem "$programFilesX86\Microsoft\EdgeWebView\Application" -Directory | Where-Object { $_.Name -match "^\d+(\.\d+){3}$" } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1 -ExpandProperty Name; "WEBVIEW2_RUNTIME=$version"
```

```text
OS=Microsoft Windows NT 10.0.26200.0
PROCESSOR_ARCHITECTURE=ARM64
WEBVIEW2_RUNTIME=152.0.4191.66
```

## Native probe

The exact commit was built with the native x64 MSVC toolchain:

```text
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/vscode-webview2-handle && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo build -p vscode-webview2-handle-spike --target-dir D:/git/pwa_ganerator/.worktrees/vscode-webview2-handle/target/task4-final-x64"
```

The probe file was created, run, and verified by this PowerShell command:

```text
$probe = Join-Path $env:TEMP 'webwrap-vscode-task4-final-probe.txt'; Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue; [IO.File]::WriteAllText($probe, 'before', [Text.UTF8Encoding]::new($false)); $exe = 'D:\git\pwa_ganerator\.worktrees\vscode-webview2-handle\target\task4-final-x64\debug\vscode-webview2-handle-spike.exe'; $process = Start-Process -FilePath $exe -ArgumentList @('--probe', $probe) -PassThru; if (-not $process.WaitForExit(30000)) { Stop-Process -Id $process.Id -Force; throw 'probe did not complete' }; $hex = [BitConverter]::ToString([IO.File]::ReadAllBytes($probe)); $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $probe).Hash; "PASS probe bytes=$hex hash=$hash exit=$($process.ExitCode) path=$probe"; Remove-Item -LiteralPath $probe -Force
```

The `--probe` executable run returned exit code `0` and wrote the exact expected bytes:

```text
PASS bytes=77-65-62-77-72-61-70-2D-70-72-6F-62-65-2D-6F-6B-0A hash=C875C7515E18F31EDFDE5BC46ABEAA4D94FED7748B344AF6D3797EFF2561100F exit=0
```

Native probe: **PASS**.

## Automatic open and Ctrl+S write-back

Fresh proof file:

- Path: `%TEMP%\webwrap-vscode-task4-final-writeback.md`
- Contents: UTF-8 without BOM, `before` plus Windows CRLF
- Before hash: `A6456ED3BDE94789512C42C563637B929DA22B9B5611962BD2C8FF63343D8195`
- Before bytes: `62-65-66-6F-72-65-0D-0A`

The proof file was prepared with Windows PowerShell (`UTF8Encoding($false)` prevents a BOM):

```text
$p = Join-Path $env:TEMP 'webwrap-vscode-task4-final-writeback.md'
[IO.File]::WriteAllText($p, "before`r`n", [Text.UTF8Encoding]::new($false))
```

The exact executable was launched with a deleted-then-fresh WebView2 profile and a local CDP endpoint:

```text
$port = 9306; $profile = Join-Path $env:TEMP 'webwrap-vscode-task4-final-profile'; Remove-Item -LiteralPath $profile -Recurse -Force -ErrorAction SilentlyContinue; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"; $env:WEBVIEW2_USER_DATA_FOLDER = $profile; $process = Start-Process -FilePath 'D:\git\pwa_ganerator\.worktrees\vscode-webview2-handle\target\task4-final-x64\debug\vscode-webview2-handle-spike.exe' -ArgumentList @($p) -PassThru -WorkingDirectory 'D:\git\pwa_ganerator\.worktrees\vscode-webview2-handle\target\task4-final-x64\debug'
```

Phase 1 was observation only: the CDP script polled the visible tab/editor state until the supplied file appeared. It did not invoke `showOpenFilePicker`, press Ctrl+O, click a picker, change focus, or inject a second open action.

Automatic-open observation:

```json
{
  "href": "https://vscode.dev/",
  "tab": "webwrap-vscode-task4-final-writeback.md",
  "lines": ["before", ""]
}
```

Phase 2 used the same CDP connection while the already-open editor remained focused, replaced its contents with `after` plus CRLF, verified the original file was still unchanged immediately before saving, and sent exactly one trusted Ctrl+S key-down/key-up pair. A condition-based wait then polled the editor and filesystem until the original file contained the expected bytes. No dirty marker was exposed by this VS Code build before or after the edit, so this run does not claim a dirty-marker transition.

The harness sent these CDP calls for the edit and save (with `Runtime.evaluate` state reads between phases):

```text
Input.dispatchKeyEvent({type: 'rawKeyDown', modifiers: 2, windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65, key: 'a', code: 'KeyA'})
Input.dispatchKeyEvent({type: 'keyUp', modifiers: 2, windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65, key: 'a', code: 'KeyA'})
Input.insertText({text: 'after\r\n'})
Runtime.evaluate({expression: STATE_EXPRESSION, returnByValue: true})
Input.dispatchKeyEvent({type: 'rawKeyDown', modifiers: 2, windowsVirtualKeyCode: 83, nativeVirtualKeyCode: 83, key: 's', code: 'KeyS'})
Input.dispatchKeyEvent({type: 'keyUp', modifiers: 2, windowsVirtualKeyCode: 83, nativeVirtualKeyCode: 83, key: 's', code: 'KeyS'})
Runtime.evaluate({expression: STATE_EXPRESSION, returnByValue: true})
```

`STATE_EXPRESSION` was this read-only expression (the actual proof filename was substituted before evaluation):

```javascript
(()=>{const proofName='webwrap-vscode-task4-final-writeback.md';const tabs=[...document.querySelectorAll('.tab')];const tab=tabs.find(n=>n.textContent.includes(proofName));const dirtyNodes=[...document.querySelectorAll('*')].filter(n=>String(n.className).toLowerCase().includes('dirty')||String(n.getAttribute('aria-label')||'').toLowerCase().includes('dirty')||String(n.getAttribute('title')||'').toLowerCase().includes('dirty'));const dirty=!!tab&&(tab.classList.contains('dirty')||!!tab.querySelector('.codicon-close-dirty')||!!tab.querySelector('[aria-label*="dirty" i]')||dirtyNodes.some(n=>tab.contains(n))||document.title.startsWith('*'));return{title:document.title,href:location.href,focused:document.hasFocus(),lines:[...document.querySelectorAll('.view-lines .view-line')].map(n=>n.textContent),tab:tab?{className:tab.className,aria:tab.getAttribute('aria-label')}:null,dirty,dirtyNodes:dirtyNodes.slice(0,20).map(n=>({tag:n.tagName,className:String(n.className),aria:n.getAttribute('aria-label'),title:n.getAttribute('title')}))};})()
```

It performed no click, picker, or keyboard action. The initial observation reported `focused: true`, so the harness sent no separate focus click before the key events.

The current VS Code WebView exposed no dirty class, dirty close icon, dirty ARIA label, or title marker either before or after the edit; this absence is recorded rather than inferred as a transition.

Automatic open: **PASS**. Ctrl+S original-file write-back: **PASS**.

- Before-save hash: `A6456ED3BDE94789512C42C563637B929DA22B9B5611962BD2C8FF63343D8195` (unchanged)
- After hash: `1235F8F435B1D16F7AAC442351F2E29FF1BD2B74652392C4ACCB5EC5565973DA`
- After bytes: `61-66-74-65-72-0D-0A`
- After text: `after<CR><LF>`
- Hash and bytes were identical after the wrapper process was closed.
- Wrapper process count after close: `0`.
- `%USERPROFILE%\Downloads\webwrap-vscode-task4-final-writeback.md`: did not exist.

The post-close assertion was run with:

```text
$p = Join-Path $env:TEMP 'webwrap-vscode-task4-final-writeback.md'
@{ exists = Test-Path $p; hash = (Get-FileHash $p -Algorithm SHA256).Hash; bytes = (([IO.File]::ReadAllBytes($p) | ForEach-Object { $_.ToString('X2') }) -join '-'); text = [IO.File]::ReadAllText($p); downloads = @(Get-ChildItem "$env:USERPROFILE\Downloads" -Filter 'webwrap-vscode-task4-final-writeback.md' -ErrorAction SilentlyContinue).Count } | ConvertTo-Json -Compress
```

An independent exact-head rerun used `%TEMP%\webwrap-vscode-task4-root2.md`
with a fresh WebView2 profile. It automatically opened the proof tab, changed
`before` to `after`, sent one trusted Ctrl+S key-down/key-up pair, and observed
the saved state. The original-file result was:

```text
before bytes=62-65-66-6F-72-65-0D-0A
after bytes=61-66-74-65-72-0D-0A
after hash=1235F8F435B1D16F7AAC442351F2E29FF1BD2B74652392C4ACCB5EC5565973DA
Downloads match count=0
```

## Wrong-origin fail-closed check

To make the race-free check observable, a separate deleted-then-fresh profile used the proof file `%TEMP%\webwrap-vscode-task4-final-origin.md` and started with `vscode.dev` mapped to `127.0.0.1`; the initial top-level URL remained `https://vscode.dev/` but no hosted workbench readiness message could fire. DevTools then navigated the top-level page to this data document:

```text
$port = 9313; $proof = Join-Path $env:TEMP 'webwrap-vscode-task4-final-origin.md'; $profile = Join-Path $env:TEMP 'webwrap-vscode-task4-final-origin-profile-7'; Remove-Item -LiteralPath $proof -Force -ErrorAction SilentlyContinue; Remove-Item -LiteralPath $profile -Recurse -Force -ErrorAction SilentlyContinue; [IO.File]::WriteAllText($proof, ('before' + [Environment]::NewLine), [Text.UTF8Encoding]::new($false)); $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port --host-resolver-rules=`"MAP vscode.dev 127.0.0.1`""; $env:WEBVIEW2_USER_DATA_FOLDER = $profile; $process = Start-Process -FilePath 'D:\git\pwa_ganerator\.worktrees\vscode-webview2-handle\target\task4-final-x64\debug\vscode-webview2-handle-spike.exe' -ArgumentList @($proof) -PassThru -WorkingDirectory 'D:\git\pwa_ganerator\.worktrees\vscode-webview2-handle\target\task4-final-x64\debug'

data:text/html,<title>Wrong%20Origin%20Probe</title><p>wrong-origin</p>
```

The page reported `origin: "null"` and `chrome.webview: true`. From that document's console context, the test emitted every recognized bridge message as a JSON-string post:

```text
webwrap-vscode-ready
webwrap-vscode-handle-ready
webwrap-probe-ready
webwrap-probe-complete
```

After five seconds, page-side evidence was:

```json
{
  "messages": [],
  "keydowns": [],
  "bridge": true
}
```

The exact hostile-origin CDP calls, including listener setup before any message emission, were:

```text
Page.navigate({url: 'data:text/html,<title>Wrong%20Origin%20Probe</title><p>wrong-origin</p>'})
Runtime.evaluate({expression: `(()=>{const bridge=window.chrome?.webview;window.__webwrapEvidence={messages:[],keydowns:[],bridge:!!bridge};window.addEventListener('keydown',event=>window.__webwrapEvidence.keydowns.push({key:event.key,code:event.code,ctrl:event.ctrlKey,shift:event.shiftKey,trusted:event.isTrusted}),true);if(bridge)bridge.addEventListener('message',event=>window.__webwrapEvidence.messages.push({data:event.data,objects:event.additionalObjects?.length||0}));return{bridge:!!bridge,href:location.href,origin:location.origin};})()`,returnByValue:true})
Runtime.evaluate({expression: `["webwrap-vscode-ready","webwrap-vscode-handle-ready","webwrap-probe-ready","webwrap-probe-complete"].forEach(type => chrome.webview.postMessage(JSON.stringify({type})))`})
// wait five seconds
await sleep(5000)
Runtime.evaluate({expression: 'window.__webwrapEvidence',returnByValue:true})
```

The observer sent no keyboard input during the five-second wait. Wrong-origin denial: **PASS**. No additional-object message (therefore no `FileSystemFileHandle`) and no trusted keyboard event, including Ctrl+O, was observed. The separate origin proof file remained `62-65-66-6F-72-65-0D-0A` with hash `A6456ED3BDE94789512C42C563637B929DA22B9B5611962BD2C8FF63343D8195`.

An earlier exploratory hostile-origin run showed that synchronously changing the Tauri title from the WebView2 message callback stalled CDP. Commit `34e5f6a` removes that callback-side UI mutation: the disallowed source is logged and returned before any handle or input state is touched. The final hostile-origin run completed without that stall or any COM error.

## Static verification

Native x64 MSVC checks for the exact source returned:

```text
cargo test -p vscode-webview2-handle-spike
test result: ok. 7 passed; 0 failed

cargo fmt --all -- --check
cargo clippy -p vscode-webview2-handle-spike --all-targets -- -D warnings
node --check spikes/vscode-webview2-handle/src/vscode.js
git diff --check
```

Formatting, Clippy, JavaScript syntax, and diff checks exited `0` with no
diagnostic output. The final workspace run after adding this report used an
isolated x64 target directory:

```text
cargo test --workspace --target-dir target/final-x64
test result: ok. 7 passed; 0 failed
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --target-dir target/final-x64 -- -D warnings
Finished `dev` profile [unoptimized + debuginfo]
node --check spikes/vscode-webview2-handle/src/vscode.js
git diff --check
```

The final clean-tree check after committing this report was:

```text
git status --short
(no output)
```

## Conclusion

The vscode.dev feasibility gate is satisfied for this measured WebView2 runtime: a genuine writable handle reaches the web app, the file opens automatically, and Ctrl+S writes the original local file without a Downloads replacement. The implementation remains a spike; product/generator work should use a separate plan.
