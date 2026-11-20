// End-to-end checks of the file bridge against real generated apps on real
// WebView2. Run by .github/workflows/windows.yml; needs:
//   WEBWRAP_E2E_APP_A  generated app for https://localhost:8443/
//   WEBWRAP_E2E_APP_B  generated app for https://localhost:8444/
//   WEBWRAP_E2E_APP_C  generated app for https://localhost:8445/, --adapter vscodeDev
//   WEBWRAP_E2E_PFX    certificate for localhost, password "webwrap"

import { execFileSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import fs from 'node:fs';
import https from 'node:https';
import net from 'node:net';
import path from 'node:path';
import { attach, sleep, until } from './cdp.mjs';
import { assert, check, forward, launch, logged, makeFile, required, run, sha256, work } from './apps.mjs';

const APP_A = required('WEBWRAP_E2E_APP_A');
const APP_B = required('WEBWRAP_E2E_APP_B');
const APP_C = required('WEBWRAP_E2E_APP_C');
const PFX = fs.readFileSync(required('WEBWRAP_E2E_PFX'));
const HOME_A = 'https://localhost:8443';
const HOME_B = 'https://localhost:8444';
const HOME_C = 'https://localhost:8445';
const AWAY_A = 'https://127.0.0.1:8443'; // same server, different origin
const AWAY_C = 'https://127.0.0.1:8445';
const DEBUG_A = 9301;
const DEBUG_B = 9302;
const DEBUG_C = 9303;
const MiB = 1024 * 1024;

const PAGE = `<!doctype html><meta charset=utf-8><title>bridge test</title>
<a id=pop href="http://127.0.0.1:9/popup-clicked" target=_blank
   style="display:block;width:300px;height:120px;background:#ccc">pop-up link</a>
<script>
  // A pop-up opened by the page's own script, with no click: B's finding 1.
  if (location.search === '?autopopup') setTimeout(() => window.open('https://example.com/scripted'), 300);
</script>`;

// A stand-in for vscode.dev, enough for the vscodeDev adapter: Ctrl+Shift+P
// opens a command palette whose selected entry echoes what is typed, and
// Enter on "Open File..." asks showOpenFilePicker for one file, as
// vscode.dev's pickFileAndOpen does. `?nopalette` has no palette at all.
const VSCODE_STAND_IN = `<!doctype html><meta charset=utf-8><title>vscode stand-in</title>
<div class=monaco-workbench>
  <input id=palette hidden aria-activedescendant=pick><div id=pick role=option></div>
</div>
<script>
  window.__opened = [];
  window.__keys = 0;
  const palette = document.getElementById('palette');
  const pick = document.getElementById('pick');
  addEventListener('keydown', (event) => {
    __keys++;
    if (location.search !== '?nopalette' && event.ctrlKey && event.shiftKey && event.code === 'KeyP') {
      event.preventDefault();
      palette.hidden = false;
      palette.value = '>';
      pick.textContent = '';
      palette.focus();
    } else if (event.key === 'Enter' && document.activeElement === palette) {
      const command = pick.textContent;
      palette.hidden = true;
      palette.blur();
      if (command === 'Open File...') openFile();
    }
  }, true);
  palette.addEventListener('input', () => { pick.textContent = palette.value.slice(1); });
  async function openFile() {
    const [handle] = await showOpenFilePicker({ multiple: false });
    const file = await handle.getFile();
    __opened.push({ name: file.name, text: await file.text(), handle });
  }
</script>`;

for (const [port, page] of [[8443, PAGE], [8444, PAGE], [8445, VSCODE_STAND_IN]]) {
  https
    .createServer({ pfx: PFX, passphrase: 'webwrap' }, (_, response) => {
      response.writeHead(200, { 'content-type': 'text/html' });
      response.end(page);
    })
    .listen(port);
}

// Installed into each document under test: the page half of the protocol.
const RECEIVER = `(() => {
  if (window.__ww) return;
  const ww = window.__ww = { files: [], results: [], pending: {} };
  // Page-to-app messages are JSON text posted as a string.
  const post = (message) => chrome.webview.postMessage(JSON.stringify(message));
  const hex = async (view) => Array.from(
    new Uint8Array(await crypto.subtle.digest('SHA-256', view.slice())),
    (b) => b.toString(16).padStart(2, '0')).join('');
  chrome.webview.addEventListener('sharedbufferreceived', async (event) => {
    const info = event.additionalData;
    const buffer = event.getBuffer();
    if (info.type === 'webwrap-file') {
      const sha256 = await hex(new Uint8Array(buffer, 0, info.size));
      ww.files.push({ token: info.token, name: info.name, size: info.size, sha256 });
    } else if (info.type === 'webwrap-save-buffer') {
      new Uint8Array(buffer).set(ww.pending[info.token]);
      post({ type: 'webwrap-save-commit', token: info.token, save: info.save });
    }
    chrome.webview.releaseBuffer(buffer);
  });
  chrome.webview.addEventListener('message', (event) => ww.results.push(event.data));
  ww.save = (token, content) => {
    let bytes;
    if (typeof content === 'string') bytes = new TextEncoder().encode(content);
    else {
      bytes = new Uint8Array(content.pattern);
      for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 31 + 7) & 255;
    }
    ww.pending[token] = bytes;
    post({ type: 'webwrap-save', token, size: bytes.length });
  };
  ww.ready = () => post({ type: 'webwrap-ready' });
})()`;

const received = (page, name) =>
  until(() => page.evaluate(`window.__ww?.files.find((f) => f.name === ${JSON.stringify(name)})`), `${name} to arrive`);
const result = (page, token) =>
  until(() => page.evaluate(`window.__ww?.results.find((r) => r.token === ${JSON.stringify(token)})`), `save result for ${token}`);
/** The text of a file the vscode stand-in opened. */
const opened = (page, name) =>
  until(() => page.evaluate(`window.__opened?.find((f) => f.name === ${JSON.stringify(name)})?.text`), `${name} to open`);
const count = (app, pattern) => (app.log.match(new RegExp(pattern, 'g')) ?? []).length;

/** Saving must never leave its temporary, backup, or swap files behind. */
function noLeftovers() {
  const left = fs.readdirSync(work).filter((name) => name.startsWith('.webwrap-') || name.endsWith('.crswap'));
  assert(left.length === 0, `left behind: ${left.join(', ')}`);
}

async function main() {
  const startContent = 'opened at startup';
  const a = launch(APP_A, DEBUG_A, [makeFile('start.txt', startContent)]);
  const pageA = await attach(DEBUG_A, HOME_A);
  await pageA.loaded(HOME_A);
  await pageA.evaluate(RECEIVER);
  await pageA.evaluate('__ww.ready()');

  await check('a file given at launch arrives after the page is ready, intact', async () => {
    const file = await received(pageA, 'start.txt');
    assert(file.sha256 === sha256(startContent), 'start.txt bytes differ');
  });

  let forwarded;
  await check('gate 6: a second launch hands its file to the running app', async () => {
    await forward(APP_A, DEBUG_A, makeFile('forwarded.txt', 'sent by a second launch'));
    forwarded = await received(pageA, 'forwarded.txt');
    assert(forwarded.sha256 === sha256('sent by a second launch'), 'forwarded.txt bytes differ');
  });

  await check('gate 7: with two apps running, a file reaches only its own app', async () => {
    launch(APP_B, DEBUG_B);
    const pageB = await attach(DEBUG_B, HOME_B);
    await pageB.loaded(HOME_B);
    await pageB.evaluate(RECEIVER);
    await pageB.evaluate('__ww.ready()');
    await forward(APP_B, DEBUG_B, makeFile('for-b.txt', 'only for B'));
    await received(pageB, 'for-b.txt');
    await sleep(2000);
    assert(!(await pageA.evaluate(`__ww.files.some((f) => f.name === 'for-b.txt')`)), 'A received B\'s file');
  });

  await check('saving writes the new bytes to the original file', async () => {
    await pageA.evaluate(`__ww.save(${JSON.stringify(forwarded.token)}, 'edited in the page')`);
    const reply = await result(pageA, forwarded.token);
    assert(reply.ok === true, `save failed: ${JSON.stringify(reply)}`);
    assert(fs.readFileSync(path.join(work, 'forwarded.txt'), 'utf8') === 'edited in the page', 'file not changed');
    noLeftovers();
  });

  await check('a 160 MiB file opens and a 150 MiB save writes back, byte for byte', async () => {
    const big = randomBytes(160 * MiB);
    const bigPath = makeFile('big.bin', big);
    await forward(APP_A, DEBUG_A, bigPath);
    const file = await until(() => pageA.evaluate(`__ww.files.find((f) => f.name === 'big.bin')`), 'big.bin', 120000);
    assert(file.size === big.length && file.sha256 === sha256(big), 'big.bin bytes differ');
    const pattern = Buffer.alloc(150 * MiB);
    for (let i = 0; i < pattern.length; i++) pattern[i] = (i * 31 + 7) & 255;
    await pageA.evaluate(`__ww.save(${JSON.stringify(file.token)}, { pattern: ${pattern.length} })`);
    const reply = await until(() => pageA.evaluate(`__ww.results.find((r) => r.token === ${JSON.stringify(file.token)})`), 'big save', 120000);
    assert(reply.ok === true, `big save failed: ${JSON.stringify(reply)}`);
    assert(sha256(fs.readFileSync(bigPath)) === sha256(pattern), 'big.bin on disk is not the saved content');
    noLeftovers();
  });

  await check('a save that cannot be written reports failure and leaves the file alone', async () => {
    const lockedPath = makeFile('locked.txt', 'original');
    await forward(APP_A, DEBUG_A, lockedPath);
    const file = await received(pageA, 'locked.txt');
    fs.chmodSync(lockedPath, 0o444);
    await pageA.evaluate(`__ww.save(${JSON.stringify(file.token)}, 'must not land')`);
    const reply = await result(pageA, file.token);
    fs.chmodSync(lockedPath, 0o666);
    assert(reply.ok === false, 'a read-only save reported success');
    assert(fs.readFileSync(lockedPath, 'utf8') === 'original', 'the read-only file changed');
    assert(!/[A-Za-z]:\\/.test(reply.error), `the page was told a local path: ${reply.error}`);
    await logged(a, /save-failed/, 'save-failed in the log');
    noLeftovers();
  });

  await check('malformed pipe messages are rejected and the app keeps serving', async () => {
    const appId = JSON.parse(execFileSync(APP_A, ['--inspect-self'])).appId;
    const sid = execFileSync('whoami', ['/user', '/fo', 'csv', '/nh']).toString().trim().split(',')[1].replaceAll('"', '');
    const pipe = `\\\\.\\pipe\\WebWrap.${appId}.${sid}`;
    const frame = (json) => {
      const body = Buffer.from(json);
      const length = Buffer.alloc(4);
      length.writeUInt32LE(body.length);
      return Buffer.concat([length, body]);
    };
    const cases = {
      'oversized length': Buffer.from([0xff, 0xff, 0xff, 0xff]),
      'not JSON': frame('nonsense'),
      'wrong appId': frame(JSON.stringify({ v: 1, appId: 'ffffffffffffffff', files: [] })),
      'wrong version': frame(JSON.stringify({ v: 2, appId, files: [] })),
      'relative path': frame(JSON.stringify({ v: 1, appId, files: ['relative.txt'] })),
    };
    for (const [label, bytes] of Object.entries(cases)) {
      const ack = await new Promise((resolve, reject) => {
        const socket = net.connect(pipe);
        socket.on('error', reject);
        socket.on('connect', () => socket.write(bytes));
        socket.on('data', (data) => {
          resolve(data[0]);
          socket.destroy();
        });
        socket.on('end', () => resolve(null));
      });
      assert(ack === 0, `${label}: expected a rejection, got ${ack}`);
    }
    await logged(a, /ipc-rejected/, 'ipc-rejected in the log');
    await forward(APP_A, DEBUG_A, makeFile('after-garbage.txt', 'still works'));
    await received(pageA, 'after-garbage.txt');
  });

  await check('gate 8: no file access after navigating to an untrusted origin', async () => {
    await pageA.evaluate(`location.href = ${JSON.stringify(AWAY_A + '/')}`);
    await pageA.loaded(AWAY_A);
    await logged(a, /revoked \d+ file tokens/, 'revocation in the log');
    await pageA.evaluate(RECEIVER);
    await pageA.evaluate('__ww.ready()');
    await logged(a, /delivery-refused the page is not on a trusted origin/, 'refused readiness');
    await pageA.evaluate(`__ww.save(${JSON.stringify(forwarded.token)}, 'from an untrusted page')`);
    await logged(a, /save-refused the page is not on a trusted origin/, 'refused save');
    await forward(APP_A, DEBUG_A, makeFile('while-away.txt', 'opened while away'));
    await sleep(3000);
    const seen = await pageA.evaluate('[__ww.files.length, __ww.results.length]');
    assert(seen[0] === 0 && seen[1] === 0, `the untrusted page got ${seen[0]} files and ${seen[1]} replies`);
    assert(fs.readFileSync(path.join(work, 'forwarded.txt'), 'utf8') === 'edited in the page', 'file changed');

    await pageA.evaluate(`location.href = ${JSON.stringify(HOME_A + '/')}`);
    await pageA.loaded(HOME_A);
    await pageA.evaluate(RECEIVER);
    await pageA.evaluate(`__ww.save(${JSON.stringify(forwarded.token)}, 'with a revoked token')`);
    const reply = await result(pageA, forwarded.token);
    assert(reply.ok === false && /revoked/.test(reply.error), `revoked token was usable: ${JSON.stringify(reply)}`);
    await pageA.evaluate('__ww.ready()');
    await received(pageA, 'while-away.txt');
  });

  await check('pop-ups: a script pop-up is denied, a clicked link opens externally', async () => {
    // Driven by the page's own script: code evaluated over DevTools counts as
    // user-initiated in WebView2, as console pop-ups do in Chrome.
    await pageA.evaluate(`location.href = ${JSON.stringify(HOME_A + '/?autopopup')}`);
    await pageA.loaded(HOME_A);
    await logged(a, /popup-denied https:\/\/example\.com\/scripted/, 'denied scripted pop-up');
    assert(!/popup-external https:\/\/example\.com\/scripted/.test(a.log), 'the scripted pop-up opened externally');
    const point = await pageA.evaluate(`(() => {
      const r = document.getElementById('pop').getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    })()`);
    for (const type of ['mousePressed', 'mouseReleased']) {
      await pageA.send('Input.dispatchMouseEvent', { type, ...point, button: 'left', clickCount: 1 });
    }
    await logged(a, /popup-external http:\/\/127\.0\.0\.1:9\/popup-clicked/, 'clicked pop-up opened externally');
  });

  await check('external schemes: an iframe and a top-level search-ms: are both stopped', async () => {
    await pageA.evaluate(`document.body.insertAdjacentHTML('beforeend', '<iframe src="search-ms:query=frame"></iframe>')`);
    await logged(a, /(frame|scheme)-blocked search-ms:query=frame/, 'blocked iframe search-ms');
    await pageA.evaluate(`location.href = 'search-ms:query=top'`);
    await logged(a, /(nav|scheme)-blocked search-ms:query=top/, 'blocked top-level search-ms');
    await sleep(1000);
    assert((await pageA.evaluate('location.origin')) === HOME_A, 'the page navigated away');
  });

  await checkVscodeAdapter();
}

/** D2: the vscodeDev adapter against the local stand-in. */
async function checkVscodeAdapter() {
  const onePath = makeFile('one.md', 'first file');
  const c = launch(APP_C, DEBUG_C, [onePath]);
  const pageC = await attach(DEBUG_C, HOME_C);
  await pageC.loaded(HOME_C);

  await check('vscodeDev: a file given at launch opens through Open File and saves through its handle', async () => {
    assert((await opened(pageC, 'one.md')) === 'first file', 'one.md opened with the wrong text');
    await logged(c, /vscode-open done/, 'a finished Open File command');
    const created = fs.statSync(onePath).birthtimeMs;
    await pageC.evaluate(`(async () => {
      const writable = await __opened.find((f) => f.name === 'one.md').handle.createWritable();
      await writable.write('saved through the handle');
      await writable.close();
      return true;
    })()`);
    assert(fs.readFileSync(onePath, 'utf8') === 'saved through the handle', 'one.md not changed');
    noLeftovers();
    // Recorded, not asserted: WebView2, not the runtime, performs this write.
    const kept = fs.statSync(onePath).birthtimeMs === created;
    process.stdout.write(`  observed: a handle save ${kept ? 'keeps' : 'does not keep'} the creation time\n`);
  });

  await check('vscodeDev: two files from a second launch open one after the other', async () => {
    await forward(APP_C, DEBUG_C, makeFile('two.md', 'second file'), makeFile('three.md', 'third file'));
    assert((await opened(pageC, 'two.md')) === 'second file', 'two.md opened with the wrong text');
    assert((await opened(pageC, 'three.md')) === 'third file', 'three.md opened with the wrong text');
    const names = await pageC.evaluate('__opened.map((f) => f.name)');
    assert(names.join() === 'one.md,two.md,three.md', `opened ${names}`);
    await until(() => count(c, 'vscode-open done') === 3, 'one Open File command per file');
  });

  await check('gate 8, vscodeDev: no handle and no keys after navigating to an untrusted origin', async () => {
    await pageC.evaluate(`location.href = ${JSON.stringify(AWAY_C + '/')}`);
    await pageC.loaded(AWAY_C);
    await logged(c, /revoked 3 file tokens/, 'revocation in the log');
    await pageC.evaluate(`(() => {
      window.__messages = 0;
      chrome.webview.addEventListener('message', () => __messages++);
      for (const type of ['webwrap-ready', 'webwrap-vscode-open']) chrome.webview.postMessage(JSON.stringify({ type }));
    })()`);
    await logged(c, /delivery-refused the page is not on a trusted origin/, 'refused readiness');
    await logged(c, /open-refused the page is not on a trusted origin/, 'refused Open File command');
    await forward(APP_C, DEBUG_C, makeFile('while-away.md', 'opened after returning'));
    await sleep(3000);
    const seen = await pageC.evaluate('[__messages, __keys, __opened.length]');
    assert(seen.join() === '0,0,0', `the untrusted page got messages, keys, files: ${seen}`);

    await pageC.evaluate(`location.href = ${JSON.stringify(HOME_C + '/')}`);
    await pageC.loaded(HOME_C);
    assert((await opened(pageC, 'while-away.md')) === 'opened after returning', 'while-away.md text');
  });

  await check('vscodeDev: an Open File command that cannot run fails once, without retrying', async () => {
    await pageC.evaluate(`location.href = ${JSON.stringify(HOME_C + '/?nopalette')}`);
    await pageC.loaded(HOME_C);
    await forward(APP_C, DEBUG_C, makeFile('no-palette.md', 'never opened'));
    await logged(c, /vscode-open-failed the command palette did not open/, 'the failure in the log');
    await sleep(7000); // longer than one run's 5 s wait
    assert(count(c, 'vscode-open-failed') === 1, 'the failed command was retried');
    assert((await pageC.evaluate('__keys')) === 1, 'more keys than one Ctrl+Shift+P reached the page');
  });
}

run(main);
