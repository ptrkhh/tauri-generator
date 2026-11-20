// End-to-end checks of the D1 file bridge against real generated apps on
// real WebView2. Run by .github/workflows/windows.yml; needs:
//   WEBWRAP_E2E_APP_A  generated app for https://localhost:8443/
//   WEBWRAP_E2E_APP_B  generated app for https://localhost:8444/
//   WEBWRAP_E2E_PFX    certificate for localhost, password "webwrap"
// The apps' stderr carries one `webwrap: <event>` line per native decision,
// which is how the native side is observed without test hooks.

import { spawn, execFileSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import fs from 'node:fs';
import https from 'node:https';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { attach, sleep, until } from './cdp.mjs';

const APP_A = required('WEBWRAP_E2E_APP_A');
const APP_B = required('WEBWRAP_E2E_APP_B');
const PFX = fs.readFileSync(required('WEBWRAP_E2E_PFX'));
const HOME_A = 'https://localhost:8443';
const HOME_B = 'https://localhost:8444';
const AWAY_A = 'https://127.0.0.1:8443'; // same server, different origin
const DEBUG_A = 9301;
const DEBUG_B = 9302;
const MiB = 1024 * 1024;

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}

const PAGE = `<!doctype html><meta charset=utf-8><title>bridge test</title>
<a id=pop href="http://127.0.0.1:9/popup-clicked" target=_blank
   style="display:block;width:300px;height:120px;background:#ccc">pop-up link</a>`;
for (const port of [8443, 8444]) {
  https
    .createServer({ pfx: PFX, passphrase: 'webwrap' }, (_, response) => {
      response.writeHead(200, { 'content-type': 'text/html' });
      response.end(PAGE);
    })
    .listen(port);
}

// Installed into each document under test: the page half of the protocol.
const RECEIVER = `(() => {
  if (window.__ww) return;
  const ww = window.__ww = { files: [], results: [], pending: {} };
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
      chrome.webview.postMessage({ type: 'webwrap-save-commit', token: info.token, save: info.save });
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
    chrome.webview.postMessage({ type: 'webwrap-save', token, size: bytes.length });
  };
  ww.ready = () => chrome.webview.postMessage({ type: 'webwrap-ready' });
})()`;

const children = [];
function launch(exe, debugPort, args = []) {
  const child = spawn(exe, args, {
    env: {
      ...process.env,
      // Test only: a debugging port to drive the page, and acceptance of the
      // test server's self-signed certificate.
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort} --ignore-certificate-errors`,
    },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  child.log = '';
  child.stderr.on('data', (chunk) => {
    child.log += chunk;
    process.stdout.write(`  [${path.basename(exe)} ${child.pid}] ${chunk}`);
  });
  child.exitCode$ = new Promise((resolve) => child.on('exit', resolve));
  child.on('exit', (code) => process.stdout.write(`  [${path.basename(exe)} ${child.pid}] exited ${code}\n`));
  children.push(child);
  return child;
}

const work = fs.mkdtempSync(path.join(os.tmpdir(), 'webwrap-e2e-'));
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
function makeFile(name, content) {
  const file = path.join(work, name);
  fs.writeFileSync(file, content);
  return file;
}

/** Launch an app again with a file; it must hand it over and exit 0. */
async function forward(exe, debugPort, file) {
  const second = launch(exe, debugPort, file ? [file] : []);
  const code = await Promise.race([second.exitCode$, sleep(20000).then(() => 'timeout')]);
  if (code !== 0) throw new Error(`second launch of ${path.basename(exe)} ended with ${code}`);
}

const received = (page, name) =>
  until(() => page.evaluate(`window.__ww?.files.find((f) => f.name === ${JSON.stringify(name)})`), `${name} to arrive`);
const result = (page, token) =>
  until(() => page.evaluate(`window.__ww?.results.find((r) => r.token === ${JSON.stringify(token)})`), `save result for ${token}`);
const logged = (app, pattern, what) => until(() => pattern.test(app.log), what);

async function check(name, body) {
  process.stdout.write(`- ${name}\n`);
  await body();
  process.stdout.write(`  PASS ${name}\n`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

/** Saving must never leave its temporary or backup files behind. */
function noLeftovers() {
  const left = fs.readdirSync(work).filter((name) => name.startsWith('.webwrap-'));
  assert(left.length === 0, `left behind: ${left.join(', ')}`);
}

/** What a failure report needs: which apps still run, and WebView2's flags. */
function diagnose() {
  for (const child of children) {
    process.stdout.write(`  app ${child.pid}: exit ${child.exitCode ?? 'still running'}\n`);
  }
  try {
    const script = "Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | " +
      "Where-Object { $_.CommandLine -notmatch '--type=' } | Select-Object ProcessId, CommandLine | Format-List | Out-String -Width 4000";
    process.stdout.write(execFileSync('powershell', ['-NoProfile', '-Command', script]).toString());
    process.stdout.write(execFileSync('netstat', ['-ano']).toString().split('\n').filter((l) => /:930[12]\b/.test(l)).join('\n') + '\n');
  } catch (error) {
    process.stdout.write(`  (diagnostics failed: ${error.message})\n`);
  }
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
    await pageA.evaluate(`window.open('https://example.com/scripted')`);
    await logged(a, /popup-denied https:\/\/example\.com\/scripted/, 'denied scripted pop-up');
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
}

let failed = false;
try {
  await main();
  process.stdout.write('ALL PASS\n');
} catch (error) {
  failed = true;
  process.stdout.write(`FAIL: ${error.stack}\n`);
  diagnose();
} finally {
  for (const child of children) child.kill();
  fs.rmSync(work, { recursive: true, force: true });
  process.exit(failed ? 1 : 0);
}
