# Photopea Windows open/save spike

Date: 2026-09-15  
Gate: passed for the bounded Windows x64 spike

This spike proves that the local Tauri/WebView2 Outer Environment can open a
PSD in Photopea without a picker and route a trusted Ctrl+S export back to the
original file. It does not claim that the generator, packaging, Android,
associations, DNS, or large-file transport are complete.

## Build and environment

The implementation is split across these commits:

| Commit | Result |
| --- | --- |
| `b601cba` | spike plan and task ledger |
| `3aa5419` | local OE scaffold and request tests |
| `8a3526d` | WebView2 handle delivery and Photopea open |
| `f4e8d5d` | PSD export, validation, and atomic write-back |
| `c15c083` | Active-document source binding, malformed-save routing, and read-only-destination regression |

The acceptance run used:

- Windows `10.0.26200.0`, WebView2 `152.0.4191.66`, x64 MSVC target.
- WSL2 `2.7.12.0`, kernel `6.18.33.2-microsoft-standard-WSL2`.
- `rustc 1.97.0 (2d8144b78 2026-07-07)`, host `x86_64-pc-windows-msvc`.
- Tauri `2.11.5`, `webview2-com` `0.38.2`, `serde_json` `1.0.151`,
  `base64` `0.22.1`.

The exact native verification commands were:

```powershell
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo test --workspace --target-dir D:/git/pwa_ganerator/target/photopea-review-x64"
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo clippy --workspace --all-targets --target-dir D:/git/pwa_ganerator/target/photopea-review-x64 -- -D warnings"
cargo fmt --all -- --check
node spikes/photopea-windows-writeback/tests/photopea.test.js
node --check spikes/photopea-windows-writeback/ui/photopea.js
git diff --check
```

The native suite reported 17/17 Photopea tests and 7/7 VS Code tests. The tests cover the exact
JSON-string envelope, origin and live-source gates, open-state sequencing,
source binding, malformed/wrong-type/missing/base64/non-PSD/over-cap save
payload routing, atomic replacement, and destination-preserving failures. The
Node VM check covers matching and repeat saves plus rejection when Photopea's
active document reports a different source.

## Fixture and launch

The fixture is a 43-byte 1x1 RGB PSD. The command below recreates it and prints
the starting hash; it writes only a temporary test copy.

```powershell
$fixture = Join-Path $env:TEMP 'webwrap-photopea-final.psd'
$bytes = [Convert]::FromBase64String('OEJQUwABAAAAAAAAAAMAAAABAAAAAQAIAAMAAAAAAAAAAAAAAAAAAP8AAA==')
[IO.File]::WriteAllBytes($fixture, $bytes)
Get-Item -LiteralPath $fixture | Select-Object Length,FullName
(Get-FileHash -Algorithm SHA256 -LiteralPath $fixture).Hash
```

Observed before the edit:

```text
Length: 43
SHA-256: 3702889B012DDBB7186ABB22C68D1B4D69AAA2D411D6FB686EAFD42C9597C145
```

The executable was built with:

```powershell
cmd.exe /d /c "call C:/Progra~2/MICROS~2/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat && cd /d D:/git/pwa_ganerator/.worktrees/photopea-windows-writeback && C:/Users/p/.cargo/bin/rustup.exe run stable-x86_64-pc-windows-msvc cargo build -p photopea-windows-writeback-spike --target-dir D:/git/pwa_ganerator/target/photopea-review-x64"
```

The live run used a fresh WebView2 profile and CDP port:

```powershell
$port = 9360
$profile = Join-Path $env:TEMP 'webwrap-photopea-final-profile'
Remove-Item -LiteralPath $profile -Recurse -Force -ErrorAction SilentlyContinue
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"
$env:WEBVIEW2_USER_DATA_FOLDER = $profile
$exe = 'D:\git\pwa_ganerator\target\photopea-review-x64\debug\photopea-windows-writeback-spike.exe'
$fixture = Join-Path $env:TEMP 'webwrap-photopea-final.psd'
$process = Start-Process -FilePath $exe -ArgumentList @($fixture) -PassThru -WorkingDirectory (Split-Path $exe)
```

The OE generated this Photopea URL (the path is not in the URL):

```text
https://www.photopea.com/#%7B%22environment%22%3A%7B%22customIO%22%3A%7B%22open%22%3A%22app.echoToOE(%5C%22Open%5C%22)%3B%22%2C%22save%22%3A%22app.echoToOE(%5C%22Save%5C%22)%3B%22%7D%2C%22localsave%22%3Afalse%7D%7D
```

Decoded configuration:

```json
{"environment":{"customIO":{"open":"app.echoToOE(\"Open\");","save":"app.echoToOE(\"Save\");"},"localsave":false}}
```

## Live transcript and result

The native/page transcript was:

```text
webwrap-photopea-shell-ready
webwrap-file additional object (read/write handle), source=webwrap-local:webwrap-photopea-final.psd:32576-0
Photopea done
ArrayBuffer:43 (open)
{"kind":"edit","width":2,"height":2}
Save
{"kind":"save-source","source":"webwrap-local:webwrap-photopea-final.psd:32576-0"}
ArrayBuffer:1602 (saveToOE("psd"))
webwrap-photopea-save-complete
```

The shell displayed `PASS: Opened webwrap-photopea-final.psd`; the
Photopea frame exposed its normal editor UI, a PSD document, and two canvases.
After the resize edit, a trusted Playwright `Control+S` produced
`PASS: Saved webwrap-photopea-final.psd`. The resulting file was
1,602 bytes, began with `8BPS`, and had SHA-256
`C915BE23D167571F81C8B5C93FEFD3A04E88C85D761201B187A2DC765DF4DB06`.
`Downloads\webwrap-photopea-final.psd` did not exist. A second
trusted Ctrl+S also completed with `PASS: Saved ...` and another
`ArrayBuffer:1602`, proving the save state resets for repeat saves.

The native log records only source ID, byte count, and elapsed time; the
canonical native path is never sent to page JavaScript, Photopea, or status
messages.

The live edit/save interaction was driven with this CDP/Playwright command
(run from `D:\git` so the repository's Playwright dependency is resolved):

```powershell
@'
const {chromium}=require('playwright');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
  const browser=await chromium.connectOverCDP('http://127.0.0.1:9360');
  const page=browser.contexts()[0].pages().find(p=>p.url().includes('tauri.localhost'));
  const wait=async predicate=>{for(let i=0;i<240;i++){const value=await predicate();if(value?.ok)return value.value;await sleep(250)}throw new Error('acceptance timeout')};
  const opened=await wait(async()=>{const value=await page.evaluate(()=>({status:document.querySelector('#status')?.textContent,source:document.querySelector('#status')?.dataset.sourceId}));return {ok:value.status?.startsWith('PASS: Opened'),value}});
  await page.evaluate(()=>{window.__trace=[];const frame=document.querySelector('iframe');window.addEventListener('message',event=>{if(event.source===frame.contentWindow&&event.origin==='https://www.photopea.com')window.__trace.push(event.data instanceof ArrayBuffer?`ArrayBuffer:${event.data.byteLength}`:String(event.data))})});
  await page.evaluate(()=>document.querySelector('iframe').contentWindow.postMessage('app.activeDocument.resizeImage(2,2);app.echoToOE(JSON.stringify({kind:"edit",width:app.activeDocument.width,height:app.activeDocument.height}));','https://www.photopea.com'));
  const edited=await wait(async()=>{const value=await page.evaluate(()=>({trace:window.__trace,status:document.querySelector('#status')?.textContent}));return {ok:value.trace?.some(item=>item.includes('"kind":"edit"')),value}});
  await page.locator('iframe').click({position:{x:200,y:200}}); await page.keyboard.press('Control+S');
  const first=await wait(async()=>{const value=await page.evaluate(()=>({status:document.querySelector('#status')?.textContent,trace:window.__trace}));return {ok:value.status?.startsWith('PASS: Saved'),value}});
  await page.evaluate(()=>window.__trace=[]); await page.locator('iframe').click({position:{x:200,y:200}}); await page.keyboard.press('Control+S');
  const repeat=await wait(async()=>{const value=await page.evaluate(()=>({status:document.querySelector('#status')?.textContent,trace:window.__trace}));return {ok:value.status?.startsWith('PASS: Saved'),value}});
  console.log(JSON.stringify({opened,edited,first,repeat},null,2));
  process.exit(0);
})().catch(error=>{console.error(error);process.exit(1)});
'@ | & 'C:\Program Files\nodejs\node.exe'
```

The post-save check was:

```powershell
$fixture = Join-Path $env:TEMP 'webwrap-photopea-final.psd'
"length=" + (Get-Item -LiteralPath $fixture).Length
"sha256=" + (Get-FileHash -Algorithm SHA256 -LiteralPath $fixture).Hash
"downloads=" + (Test-Path -LiteralPath (Join-Path $env:USERPROFILE 'Downloads\webwrap-photopea-final.psd'))
```

The source-binding rejection was also exercised against the running app. A CDP
script changed `app.activeDocument.source` to
`webwrap-local:other.psd:wrong`, then issued trusted Ctrl+S. Photopea echoed the
mismatched source, no `ArrayBuffer` was emitted, and the shell reported
`FAIL: Photopea active document does not match the local file`. The original
file remained `length=1602` with SHA-256
`C915BE23D167571F81C8B5C93FEFD3A04E88C85D761201B187A2DC765DF4DB06`.

The rejection command was:

```javascript
const frame = document.querySelector('iframe');
frame.contentWindow.postMessage(
  'app.activeDocument.source="webwrap-local:other.psd:wrong";app.echoToOE(JSON.stringify({kind:"source-changed",source:app.activeDocument.source}));',
  'https://www.photopea.com',
);
// after the source-changed echo:
await page.locator('iframe').click({position:{x:200,y:200}});
await page.keyboard.press('Control+S');
```

## Rejection checks

### Hostile origin

With a fresh profile, immediately after the WebView2 page became available and
before the Photopea iframe could finish loading, the OE was navigated through
CDP to a `data:text/html` page which posted the exact shell-ready message. The
page reported `title=untrusted`, its WebView2 message listener received `[]`,
and no handle was delivered. The native origin gate requires both the event
source and the live WebView2 source to be `http://tauri.localhost` (known port
80). The hostile run's fixture remained 43 bytes with SHA-256
`3702889B012DDBB7186ABB22C68D1B4D69AAA2D411D6FB686EAFD42C9597C145`.

The hostile-origin probe was:

```javascript
const html = '<title>untrusted</title><script>' +
  'window.received=[];' +
  'window.chrome.webview.addEventListener("message",e=>window.received.push(e.data));' +
  'setTimeout(()=>window.chrome.webview.postMessage(JSON.stringify({type:"webwrap-photopea-shell-ready"})),0);' +
  '</script>';
await page.goto('data:text/html,' + encodeURIComponent(html));
await new Promise(resolve => setTimeout(resolve, 2000));
console.log(await page.evaluate(() => ({title: document.title, received: window.received})));
// observed: { title: "untrusted", received: [] }
```

### Malformed and over-cap save messages

The native tests exercise the real decoder and recognized-save routing with an
exact outer JSON-string envelope containing invalid base64, missing fields,
non-PSD bytes, and a base64 field of `ceil(32 MiB / 3) * 4 + 1` bytes. These
inputs are rejected before any temporary file is created. The invalid-PSD and
read-only-destination-file tests confirm that existing destination bytes remain
untouched. The 32 MiB decoded cap is a spike transport ceiling, not a
production large-file design.

## Scope boundary

Verified: Windows x64 WebView2 handle delivery, Photopea Live Messaging open,
Photopea custom-I/O PSD export, trusted Ctrl+S, repeat save, and atomic
write-back to the original file.

Not verified here: generator UX, single-file PE specialization, icon/resource
replacement, Windows file-association registration and single-instance
routing, Android packaging/content-URI handling, custom DoH, and files larger
than 32 MiB.
