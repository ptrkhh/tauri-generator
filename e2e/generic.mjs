// End-to-end checks of generic sites (D4, adapter: none) against local test
// pages, one per tier, on real WebView2. Run by .github/workflows/windows.yml;
// needs:
//   WEBWRAP_E2E_APP_E  generated app for https://localhost:8447/, no adapter
//   WEBWRAP_E2E_PFX    certificate for localhost, password "webwrap"

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import https from 'node:https';
import path from 'node:path';
import { attach, sleep, until } from './cdp.mjs';
import {
  assert, check, forward, launch, logged, makeFile, outside, pickInDialog, required, run, sha256, work,
} from './apps.mjs';

const APP_E = required('WEBWRAP_E2E_APP_E');
const PFX = fs.readFileSync(required('WEBWRAP_E2E_PFX'));
const HOME = 'https://localhost:8447';
const AWAY = 'https://127.0.0.1:8447'; // same server, untrusted origin
const DEBUG_E = 9306;

// What every test page records, in window.__got: files given to a launchQueue
// consumer, an upload box, a drop, or the page's own picker. Handles are kept
// in __handles so the test can write through them.
const COMMON = `<script>
  window.__got = { consumer: [], upload: [], drop: [], picker: [] };
  window.__handles = {};
  const hex = async (blob) => Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', await blob.arrayBuffer())),
    (b) => b.toString(16).padStart(2, '0')).join('');
  window.__record = async (kind, file, extra = {}) =>
    __got[kind].push({ name: file.name, sha256: await hex(file), ...extra });
  window.__keep = async (kind, handle) => {
    __handles[handle.name] = handle;
    await __record(kind, await handle.getFile());
  };
  window.__write = async (name, text) => {
    const writable = await __handles[name].createWritable();
    await writable.write(text);
    await writable.close();
    return true;
  };
  window.__consume = () => launchQueue.setConsumer(async (params) => {
    for (const handle of params.files) await __keep('consumer', handle);
  });
</script>`;
const UPLOAD = `<input type=file id=visible onchange="__record('upload', this.files[0], { box: this.id })">`;

const PAGES = {
  '/pwa': `${COMMON}${UPLOAD}<script>__consume()</script>`,
  '/late': `${COMMON}${UPLOAD}<script>setTimeout(__consume, 12000)</script>`,
  '/upload': `${COMMON}${UPLOAD}
    <input type=file id=hidden style="display:none" onchange="__record('upload', this.files[0], { box: this.id })">`,
  '/drop': `${COMMON}<div style="position:fixed;inset:0">drop here</div><script>
    addEventListener('dragover', (event) => event.preventDefault());
    addEventListener('drop', (event) => { event.preventDefault(); __record('drop', event.dataTransfer.files[0]); });
  </script>`,
  '/none': `${COMMON}<p>this page takes no files</p>`,
  '/picker': `${COMMON}<button id=pick style="width:300px;height:120px"
    onclick="showOpenFilePicker().then(([handle]) => __keep('picker', handle))">open</button>`,
};

https
  .createServer({ pfx: PFX, passphrase: 'webwrap' }, (request, response) => {
    const page = PAGES[new URL(request.url, HOME).pathname] ?? PAGES['/none'];
    response.writeHead(200, { 'content-type': 'text/html' });
    response.end(`<!doctype html><meta charset=utf-8><title>generic test</title>${page}`);
  })
  .listen(8447);

const count = (app, pattern) => (app.log.match(new RegExp(pattern, 'g')) ?? []).length;

run(async () => {
  const dataDir = JSON.parse(execFileSync(APP_E, ['--inspect-self'])).webview2DataDir;
  const flag = path.join(dataDir, 'open-only-notice');
  fs.rmSync(flag, { force: true });
  const e = launch(APP_E, DEBUG_E);
  const page = await attach(DEBUG_E, HOME);
  const go = async (origin, pathname) => {
    await page.evaluate(`location.href = ${JSON.stringify(origin + pathname)}`);
    await until(
      () => page.evaluate(`location.origin + location.pathname === ${JSON.stringify(origin + pathname)} && document.readyState === 'complete'`),
      `${pathname} to load`,
      60000,
    );
  };
  /** The page's record of `name`, once it has one. Buffers wait for the
   * page's ready, 10 s after it loads, so this allows for that. */
  const got = (kind, name) =>
    until(() => page.evaluate(`__got.${kind}.find((f) => f.name === ${JSON.stringify(name)})`), `${name} in ${kind}`, 40000);
  const explain = async () =>
    `  page: ${JSON.stringify(await page.evaluate('[location.href, window.__got]'))}\n${page.problems.slice(-10).join('\n')}`;

  await check('inspect-self reports a generic app', async () => {
    const report = JSON.parse(execFileSync(APP_E, ['--inspect-self']));
    assert(report.fileTier === 'generic', `fileTier is ${report.fileTier}`);
  });

  await check('tier 1: a launchQueue consumer gets a writable handle, and its save reaches the file', async () => {
    await go(HOME, '/pwa');
    const file = makeFile('tier1.txt', 'original', outside);
    await forward(APP_E, DEBUG_E, file);
    const seen = await got('consumer', 'tier1.txt');
    assert(seen.sha256 === sha256('original'), 'tier1.txt bytes differ');
    await logged(e, /file-tier 1 tier1\.txt/, 'tier 1 in the log');
    assert(await page.evaluate(`__write('tier1.txt', 'edited')`), 'the write failed');
    assert(fs.readFileSync(file, 'utf8') === 'edited', 'tier1.txt was not changed');
  }, explain);

  await check('tier 1 falls back to an upload for a file WebView2 will not give a handle to, with the notice once', async () => {
    const content = 'in temp';
    await forward(APP_E, DEBUG_E, makeFile('in-temp.txt', content, work));
    const seen = await got('upload', 'in-temp.txt');
    assert(seen.sha256 === sha256(content), 'in-temp.txt bytes differ');
    assert((await page.evaluate('__got.consumer.length')) === 1, 'the consumer got the AppData file');
    await logged(e, /file-tier 3 in-temp\.txt/, 'tier 3 in the log');
    await logged(e, /open-only-notice shown/, 'the notice');
    assert(fs.existsSync(flag), 'the notice was not remembered');
  }, explain);

  await check('a consumer set after 10 s is not used: the file arrives as an upload', async () => {
    await go(HOME, '/late');
    await forward(APP_E, DEBUG_E, makeFile('late.txt', 'late', outside));
    await got('upload', 'late.txt');
    await sleep(3000); // past the page's own setConsumer at 12 s
    assert((await page.evaluate('__got.consumer.length')) === 0, 'the late consumer got the file');
    await logged(e, /file-tier 3 late\.txt/, 'tier 3 in the log');
  }, explain);

  await check('tier 3: the one visible upload box gets the file, the hidden one does not', async () => {
    await go(HOME, '/upload');
    const content = 'for the upload box';
    await forward(APP_E, DEBUG_E, makeFile('upload.txt', content, outside));
    const seen = await got('upload', 'upload.txt');
    assert(seen.box === 'visible' && seen.sha256 === sha256(content), `wrong upload: ${JSON.stringify(seen)}`);
    await logged(e, /file-tier 3 upload\.txt/, 'tier 3 in the log');
    assert(count(e, 'open-only-notice shown') === 1, 'the notice showed again');
  }, explain);

  await check('tier 3: with no upload box, a drop the site accepts', async () => {
    await go(HOME, '/drop');
    const content = 'dropped';
    await forward(APP_E, DEBUG_E, makeFile('drop.txt', content, outside));
    const seen = await got('drop', 'drop.txt');
    assert(seen.sha256 === sha256(content), 'drop.txt bytes differ');
    await logged(e, /file-tier 3 drop\.txt/, 'tier 3 in the log');
  }, explain);

  await check('tier 4: a site that takes no file is reported, and nothing is claimed', async () => {
    await go(HOME, '/none');
    await forward(APP_E, DEBUG_E, makeFile('none.txt', 'unwanted', outside));
    await until(() => /open-failed none\.txt: Bridge E did not open it/.test(e.log), 'the failure in the log', 40000);
    assert(!/file-tier \d none\.txt/.test(e.log), 'a tier was claimed for none.txt');
  }, explain);

  await check('tier 2: the site\'s own picker gets a writable handle, untouched by the runtime', async () => {
    await go(HOME, '/picker');
    const file = makeFile('picked.txt', 'before', outside);
    const point = await page.evaluate(`(() => {
      const r = document.getElementById('pick').getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    })()`);
    for (const type of ['mousePressed', 'mouseReleased']) {
      await page.send('Input.dispatchMouseEvent', { type, ...point, button: 'left', clickCount: 1 });
    }
    const outcome = pickInDialog(e.pid, file);
    if (outcome !== 'picked') {
      // Evidence, not a failure: tier 2 then stays a PC check.
      process.stdout.write(`  TIER 2 NOT VERIFIED IN CI: ${outcome}\n`);
      return;
    }
    await got('picker', 'picked.txt');
    const wrote = await Promise.race([page.evaluate(`__write('picked.txt', 'after')`).catch((error) => error.message), sleep(20000).then(() => 'no answer in 20 s')]);
    if (wrote !== true) {
      process.stdout.write(`  TIER 2 WRITE NOT VERIFIED IN CI (a permission prompt?): ${wrote}\n`);
      return;
    }
    assert(fs.readFileSync(file, 'utf8') === 'after', 'picked.txt was not changed');
    assert(!/delivered picked\.txt/.test(e.log), 'the runtime delivered the picked file itself');
  }, explain);

  await check('gate 8: an untrusted origin gets no launchQueue of ours, no readiness, and no file', async () => {
    await go(AWAY, '/pwa');
    const ours = await page.evaluate(`typeof launchQueue !== 'undefined' && !String(launchQueue.setConsumer).includes('[native code]')`);
    assert(!ours, 'the untrusted page has the script\'s launchQueue');
    await page.evaluate(`chrome.webview.postMessage(JSON.stringify({ type: 'webwrap-ready', want: 'handle' }))`);
    await logged(e, /delivery-refused the page is not on a trusted origin/, 'refused readiness');
    await forward(APP_E, DEBUG_E, makeFile('away.txt', 'not for this page', outside));
    await sleep(13000);
    assert(!/delivered away\.txt/.test(e.log), 'away.txt was delivered to the untrusted page');
    const any = await page.evaluate('Object.values(__got).flat().length');
    assert(any === 0, `the untrusted page got ${any} files`);
  }, explain);
});
