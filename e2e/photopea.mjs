// End-to-end checks of the photopea adapter (D3) against a local stand-in for
// Photopea, on real WebView2. Run by .github/workflows/windows.yml; needs:
//   WEBWRAP_E2E_APP_D  generated app for https://localhost:8446/, --adapter photopea
//   WEBWRAP_E2E_PFX    certificate for localhost, password "webwrap"

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';
import { attach, sleep, until } from './cdp.mjs';
import { assert, check, forward, launch, logged, makeFile, required, run, sha256, work } from './apps.mjs';
import { makePsd } from './psd.mjs';

const APP_D = required('WEBWRAP_E2E_APP_D');
const PFX = fs.readFileSync(required('WEBWRAP_E2E_PFX'));
const SHELL = 'https://tauri.localhost/photopea.html';
const STAND_IN = 'https://localhost:8446';
const AWAY = 'https://127.0.0.1:8446'; // the stand-in's server, as an untrusted top level
const DEBUG_D = 9305;

// Behaves like Photopea where the shell relies on it (checked against live
// Photopea): "done" when ready and after each message it handles, but nothing
// for a file it cannot read (here, one starting "BAD!"); a file opens, a
// moment later, as the new last and active document named and sourced
// "file"; scripts run against app; saveToOE posts the bytes of the
// *active* document, whichever document it is called on; File > Save and Open
// run the customIO scripts. Its state is reported to this test by POST
// /state; commands from the test arrive as {__test} messages from the shell's
// frame.
const STAND_IN_PAGE = `<!doctype html><meta charset=utf-8><title>photopea stand-in</title><body>stand-in<script>
  const config = JSON.parse(decodeURIComponent(location.hash.slice(1) || '{}'));
  const docs = [];
  let active = -1;
  const formats = [];
  let switchNextOpenTo = null;
  const send = (message, transfer = []) => parent.postMessage(message, '*', transfer);
  const hex = (buffer) => Array.from(new Uint8Array(buffer), (b) => b.toString(16).padStart(2, '0')).join('');
  // Reports go out in order, each a snapshot taken when it was asked for.
  let reporting = Promise.resolve();
  function report() {
    const snapshot = { active, formats: [...formats], docs: docs.map((d) => ({ ...d })) };
    reporting = reporting.then(async () => {
      const state = { active: snapshot.active, formats: snapshot.formats, docs: [] };
      for (const d of snapshot.docs) {
        state.docs.push({ name: d.name, source: d.source, size: d.bytes.byteLength, sha256: hex(await crypto.subtle.digest('SHA-256', d.bytes)) });
      }
      await fetch('/state', { method: 'POST', body: JSON.stringify(state) });
    });
  }
  const wrap = (d) => Object.defineProperty({
    get name() { return d.name; }, set name(value) { d.name = value; },
    get source() { return d.source; }, set source(value) { d.source = value; },
    saveToOE(format) {
      formats.push(format);
      const exported = docs[active];
      const bytes = (exported.export ?? exported.bytes).slice(0);
      send(bytes, [bytes]);
    },
  }, '_d', { value: d });
  const app = {
    get documents() { return docs.map(wrap); },
    get activeDocument() { return active < 0 ? null : wrap(docs[active]); },
    set activeDocument(document) { active = docs.indexOf(document._d); },
    echoToOE: (text) => send(String(text)),
  };
  // One message at a time, in order, as Photopea handles them.
  let handling = Promise.resolve();
  addEventListener('message', (event) => {
    if (event.source !== parent) return;
    handling = handling.then(() => handle(event.data));
  });
  async function handle(data) {
    if (data instanceof ArrayBuffer) {
      if (new TextDecoder().decode(data.slice(0, 4)) === 'BAD!') return void report();
      await new Promise((resolve) => setTimeout(resolve, 200)); // opening takes a moment
      docs.push({ name: 'file', source: 'file', bytes: data });
      active = docs.length - 1;
      // As if the user clicked another tab while the file was opening.
      if (switchNextOpenTo !== null) [active, switchNextOpenTo] = [switchNextOpenTo, null];
      send('done');
    } else if (typeof data === 'string') {
      new Function('app', data)(app);
      send('done');
    } else if (data?.__test === 'menu') {
      new Function('app', config.environment.customIO[data.which])(app);
    } else if (data?.__test === 'edit') {
      const bytes = new Uint8Array(docs[data.doc].bytes.slice(0));
      bytes[100] ^= 0xff;
      docs[data.doc].bytes = bytes.buffer;
    } else if (data?.__test === 'export') {
      docs[data.doc].export = new Uint8Array(data.bytes).buffer;
    } else if (data?.__test === 'activate') {
      active = data.doc;
    } else if (data?.__test === 'switch-next-open') {
      switchNextOpenTo = data.to;
    } else if (data?.__test === 'save-then-switch') {
      // Save is pressed on the active document, then another tab is clicked
      // before the shell gets to it.
      new Function('app', config.environment.customIO.save)(app);
      active = data.to;
    } else if (data?.__test === 'impersonate') {
      // Photopea's frame must never reach the app directly.
      const webview = window.chrome?.webview;
      if (webview) {
        for (const type of ['webwrap-ready', 'webwrap-pick-file']) webview.postMessage(JSON.stringify({ type }));
      }
      fetch('/impersonated', { method: 'POST', body: String(Boolean(webview)) });
    } else if (data?.__test === 'new') {
      docs.push({ name: 'Untitled-1', source: 'local,0,Untitled-1.psd', bytes: new Uint8Array(data.bytes).buffer });
      active = docs.length - 1;
    }
    report();
  }
  send('done');
  report();
</script>`;

let state = null;
let impersonated = null;
https
  .createServer({ pfx: PFX, passphrase: 'webwrap' }, (request, response) => {
    if (request.method === 'POST' && request.url === '/impersonated') {
      let body = '';
      request.on('data', (chunk) => (body += chunk));
      request.on('end', () => {
        impersonated = body;
        response.end();
      });
      return;
    }
    if (request.method === 'POST' && request.url === '/state') {
      let body = '';
      request.on('data', (chunk) => (body += chunk));
      request.on('end', () => {
        state = JSON.parse(body);
        response.end();
      });
      return;
    }
    response.writeHead(200, { 'content-type': 'text/html' });
    response.end(STAND_IN_PAGE);
  })
  .listen(8446);

const flipped = (bytes) => {
  const copy = Buffer.from(bytes);
  copy[100] ^= 0xff;
  return copy;
};
const pngBytes = (size) => Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), Buffer.alloc(size, 7)]);

/** Choose `file` in the app's open dialog with UI Automation, as a person would. */
function pickInDialog(pid, file) {
  // Windows lists an owned dialog as a top-level window of its process, so
  // EnumWindows finds it wherever UI Automation's tree places it. The app's
  // message boxes are dialogs too; the open dialog is the one with a
  // file-name box (automation id 1148).
  const script = `
    $ErrorActionPreference = 'Stop'
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
    Add-Type -TypeDefinition @'
      using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
      public static class TopWindows {
        delegate bool Visit(IntPtr window, IntPtr data);
        [DllImport("user32.dll")] static extern bool EnumWindows(Visit visit, IntPtr data);
        [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder name, int size);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
        [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
        public static List<IntPtr> Of(uint pid, out string seen) {
          var found = new List<IntPtr>(); var all = new StringBuilder();
          EnumWindows((window, data) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner != pid || !IsWindowVisible(window)) return true;
            var name = new StringBuilder(256); GetClassName(window, name, 256);
            var text = new StringBuilder(256); GetWindowText(window, text, 256);
            all.Append(name + " '" + text + "'; ");
            if (name.ToString() == "#32770") found.Add(window);
            return true;
          }, IntPtr.Zero);
          seen = all.ToString();
          return found;
        }
      }
'@
    $A = [System.Windows.Automation.AutomationElement]
    $T = [System.Windows.Automation.TreeScope]
    $edits = [System.Windows.Automation.PropertyCondition]::new($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)
    $buttons = [System.Windows.Automation.OrCondition]::new(
      [System.Windows.Automation.PropertyCondition]::new($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button),
      [System.Windows.Automation.PropertyCondition]::new($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::SplitButton))
    $describe = { param($items) ($items | ForEach-Object { "$($_.Current.ControlType.ProgrammaticName)[$($_.Current.AutomationId)] '$($_.Current.Name)'" }) -join ', ' }
    $deadline = (Get-Date).AddSeconds(30)
    $box = $null
    $seen = ''
    $inside = ''
    do {
      foreach ($window in [TopWindows]::Of([uint32]$env:APP_PID, [ref]$seen)) {
        $element = $A::FromHandle($window)
        if ($element.Current.Name -ne 'Open') { continue }
        $candidates = @($element.FindAll($T::Descendants, $edits))
        $inside = (& $describe $candidates) + ' | ' + (& $describe @($element.FindAll($T::Descendants, $buttons)))
        $found = $candidates | Where-Object { $_.Current.AutomationId -eq '1148' -or $_.Current.Name -like 'File name*' } | Select-Object -First 1
        if ($found) { $box = $found; $dialog = $element }
      }
      if (-not $box) { Start-Sleep -Milliseconds 500 }
    } until ($box -or (Get-Date) -gt $deadline)
    if (-not $box) {
      $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
      $shot = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
      [System.Drawing.Graphics]::FromImage($shot).CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
      if ($env:RUNNER_TEMP) { $shot.Save((Join-Path $env:RUNNER_TEMP 'open-dialog.png')) }
      throw "no file-name box; the app's visible windows: $seen; inside Open: $inside"
    }
    $box.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($env:PICK_PATH)
    $button = @($dialog.FindAll($T::Descendants, $buttons)) | Where-Object { $_.Current.AutomationId -eq '1' -or $_.Current.Name -eq 'Open' } | Select-Object -First 1
    if (-not $button) { throw "no Open button; inside Open: $inside" }
    $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    'picked'`;
  return execFileSync('powershell', ['-NoProfile', '-Command', script], {
    env: { ...process.env, APP_PID: String(pid), PICK_PATH: file },
    timeout: 90000,
  }).toString();
}

run(async () => {
  const big = makePsd(7480, 7480); // 160 MiB
  const bigPath = makeFile('big.psd', big);
  const d = launch(APP_D, DEBUG_D, [bigPath]);
  const shell = await attach(DEBUG_D, SHELL);
  /** Send a test command to the stand-in through the shell's frame. */
  const command = (message) =>
    shell.evaluate(`(document.querySelector('iframe').contentWindow.postMessage(${JSON.stringify(message)}, ${JSON.stringify(STAND_IN)}), 1)`);
  const lastNotice = () => shell.evaluate(`document.getElementById('status').dataset.last ?? ''`);
  const saved = (name) => until(async () => (await lastNotice()) === `Saved ${name}`, `the shell to report ${name} saved`, 180000);
  const explain = async () => `  stand-in: ${JSON.stringify(state)}\n  shell notice: ${await lastNotice()}\n${shell.problems.slice(-10).join('\n')}`;

  await check('photopea: a 160 MiB PSD given at launch reaches Photopea intact, tagged with its file', async () => {
    await until(() => state?.docs[0]?.sha256 === sha256(big) && state.docs[0].source.startsWith('webwrap:'), 'Photopea to hold big.psd, tagged', 180000);
    assert(state.docs[0].name === 'big', `name: ${JSON.stringify(state.docs[0])}`);
    await logged(d, /delivered big\.psd/, 'the delivery in the log');
  }, explain);

  await check('photopea: Save after an edit writes the edited 160 MiB, byte for byte', async () => {
    await command({ __test: 'edit', doc: 0 });
    await command({ __test: 'menu', which: 'save' });
    await saved('big.psd');
    assert(sha256(fs.readFileSync(bigPath)) === sha256(flipped(big)), 'big.psd on disk is not the edited document');
    await until(() => state.formats.at(-1) === 'psd', 'the export to be PSD');
    assert(!fs.readdirSync(work).some((name) => name.startsWith('.webwrap-')), 'a temporary file was left behind');
  }, explain);

  const png = pngBytes(5000);
  const pngPath = makeFile('image.png', png);
  await check('photopea: a PNG from a second launch is a second document; each saves only its own file', async () => {
    // As if the user clicked big.psd's tab while image.png was opening.
    await command({ __test: 'switch-next-open', to: 0 });
    await forward(APP_D, DEBUG_D, pngPath);
    await until(() => state?.docs[1]?.source.startsWith('webwrap:') && state.docs[1].name === 'image', 'image.png to open, tagged', 60000);
    assert(state.docs[0].source !== state.docs[1].source && state.docs[0].name === 'big', `tags: ${JSON.stringify(state.docs)}`);
    await command({ __test: 'activate', doc: 1 });
    await command({ __test: 'edit', doc: 1 });
    await command({ __test: 'menu', which: 'save' });
    await saved('image.png');
    assert(sha256(fs.readFileSync(pngPath)) === sha256(flipped(png)), 'image.png on disk is not the edited document');
    await until(() => state.formats.at(-1) === 'png', 'the PNG to be exported as PNG');
    assert(sha256(fs.readFileSync(bigPath)) === sha256(flipped(big)), 'saving the PNG changed big.psd');
  }, explain);

  await check('photopea: Save saves the document it was pressed on, even after a tab switch', async () => {
    const pngBefore = fs.readFileSync(pngPath);
    await command({ __test: 'activate', doc: 0 });
    await command({ __test: 'save-then-switch', to: 1 });
    await saved('big.psd');
    assert(sha256(fs.readFileSync(bigPath)) === sha256(flipped(big)), 'big.psd is not what its document holds');
    assert(fs.readFileSync(pngPath).equals(pngBefore), 'image.png was written instead');
    await command({ __test: 'activate', doc: 1 });
  }, explain);

  await check('photopea: bytes in a format the file extension does not name are refused', async () => {
    const before = fs.readFileSync(pngPath);
    await command({ __test: 'export', doc: 1, bytes: [...makePsd(4, 4)] });
    await command({ __test: 'menu', which: 'save' });
    await logged(d, /save-failed [^\n]*image\.png[\s\S]*?not a \.png file/, 'the refusal in the log');
    await until(async () => (await lastNotice()) === 'Not saved: image.png', 'the shell to report it not saved');
    assert(fs.readFileSync(pngPath).equals(before), 'image.png changed');
  }, explain);

  await check('photopea: a file Photopea cannot read is reported once; no document is retagged', async () => {
    const tags = state.docs.map((doc) => doc.source).join();
    await forward(APP_D, DEBUG_D, makeFile('bad.psd', 'BAD! not an image'));
    await logged(d, /open-failed bad\.psd: Photopea did not open it/, 'the failure in the log');
    await sleep(2000);
    assert(state.docs.length === 2 && state.docs.map((doc) => doc.source).join() === tags, `documents changed: ${JSON.stringify(state.docs)}`);
  }, explain);

  await check('photopea: Save on a document made in Photopea downloads it', async () => {
    const download = path.join(os.homedir(), 'Downloads', 'Untitled-1.psd');
    fs.rmSync(download, { force: true });
    const small = makePsd(8, 8);
    await command({ __test: 'new', bytes: [...small] });
    await command({ __test: 'menu', which: 'save' });
    try {
      await until(() => fs.existsSync(download) && fs.readFileSync(download).equals(small), `${download} to be downloaded`, 30000);
    } finally {
      fs.rmSync(download, { force: true });
    }
  }, explain);

  await check('photopea: Ctrl+O shows the native open dialog; the chosen file opens and saves back', async () => {
    const pick = makePsd(16, 16);
    const pickPath = makeFile('picked.psd', pick);
    const count = state.docs.length;
    await command({ __test: 'menu', which: 'open' });
    process.stdout.write(`  ${pickInDialog(d.pid, pickPath).trim()}\n`);
    await logged(d, /pick-file 1 chosen/, 'the choice in the log');
    await until(() => state.docs.length === count + 1 && state.docs.at(-1).source.startsWith('webwrap:'), 'picked.psd to open', 60000);
    await command({ __test: 'edit', doc: count });
    await command({ __test: 'menu', which: 'save' });
    await saved('picked.psd');
    assert(sha256(fs.readFileSync(pickPath)) === sha256(flipped(pick)), 'picked.psd on disk is not the edited document');
  }, explain);

  await check('photopea: Photopea\'s frame cannot reach the app directly', async () => {
    const picks = (d.log.match(/pick-file shown/g) ?? []).length;
    await command({ __test: 'impersonate' });
    await until(() => impersonated !== null, 'the frame to try');
    await sleep(3000);
    process.stdout.write(`  observed: chrome.webview ${impersonated === 'true' ? 'exists' : 'does not exist'} in the frame\n`);
    assert((d.log.match(/pick-file shown/g) ?? []).length === picks, 'the frame opened the dialog');
  }, explain);

  await check('gate 8, photopea: no file access from an untrusted top-level page', async () => {
    await shell.evaluate(`location.href = ${JSON.stringify(AWAY + '/')}`);
    await shell.loaded(AWAY);
    await logged(d, /revoked \d+ file tokens/, 'revocation in the log');
    await shell.evaluate(`(() => {
      window.__got = 0;
      chrome.webview.addEventListener('sharedbufferreceived', () => __got++);
      chrome.webview.addEventListener('message', () => __got++);
      for (const message of [{ type: 'webwrap-ready' }, { type: 'webwrap-pick-file' }, { type: 'webwrap-save', token: '1', size: 4 }]) {
        chrome.webview.postMessage(JSON.stringify(message));
      }
    })()`);
    await logged(d, /pick-file-refused the page is not on a trusted origin/, 'refused open dialog');
    await logged(d, /delivery-refused the page is not on a trusted origin/, 'refused readiness');
    await logged(d, /save-refused the page is not on a trusted origin/, 'refused save');
    const away = makeFile('while-away.psd', makePsd(8, 8));
    await forward(APP_D, DEBUG_D, away);
    await sleep(3000);
    assert((await shell.evaluate('__got')) === 0, 'the untrusted page was sent something');

    await shell.evaluate(`location.href = ${JSON.stringify(SHELL)}`);
    await until(() => state?.docs.some((doc) => doc.name === 'while-away' && doc.source.startsWith('webwrap:')), 'while-away.psd after returning', 60000);
  }, explain);
});
