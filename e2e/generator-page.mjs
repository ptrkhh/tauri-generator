// The generator's page (F3) in headless Chrome or Edge, with the Tauri
// commands stubbed: icon conversion, the initials icon, dropping what does
// not decode, and site text shown as text. Runs on Linux and in Windows CI.
//   WEBWRAP_E2E_BROWSER  a Chrome or Edge executable (found if unset)

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { attach, until } from './cdp.mjs';
import { assert, check, run } from './apps.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const UI = path.join(here, '..', 'crates', 'webwrap-generator', 'ui');
const FIXTURES = path.join(here, 'fixtures', 'generator');
const DEBUG = 9310;

function browser() {
  if (process.env.WEBWRAP_E2E_BROWSER) return process.env.WEBWRAP_E2E_BROWSER;
  const candidates = [
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
  ];
  const playwright = path.join(os.homedir(), '.cache', 'ms-playwright');
  if (fs.existsSync(playwright)) {
    for (const dir of fs.readdirSync(playwright).filter((d) => d.startsWith('chromium-')).sort().reverse()) {
      candidates.push(path.join(playwright, dir, 'chrome-linux64', 'chrome'));
    }
  }
  const found = candidates.find((file) => fs.existsSync(file));
  if (!found) throw new Error('no Chrome or Edge found; set WEBWRAP_E2E_BROWSER');
  return found;
}

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const server = http.createServer((request, response) => {
  const file = path.join(UI, path.basename(new URL(request.url, 'http://x').pathname) || 'index.html');
  if (!fs.existsSync(file)) return response.writeHead(404).end();
  response.writeHead(200, { 'Content-Type': TYPES[path.extname(file)] ?? 'application/octet-stream' });
  response.end(fs.readFileSync(file));
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const PAGE = `http://127.0.0.1:${server.address().port}/index.html`;

const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'webwrap-page-'));
const chrome = spawn(browser(), [
  '--headless=new', `--remote-debugging-port=${DEBUG}`, `--user-data-dir=${profile}`,
  '--no-first-run', '--no-default-browser-check', 'about:blank',
], { stdio: 'ignore' });

const bytes = (name) => [...fs.readFileSync(path.join(FIXTURES, name))];
const candidate = (label, name, contentType) => ({ label, url: `https://site.test/${name}`, contentType, bytes: bytes(name) });

/** A discovery as the Rust side returns it. */
const discovery = (overrides) => ({
  wrappedUrl: 'https://site.test/', appId: '0123456789abcdef', slug: 'site', name: 'Test Site', adapter: 'none',
  associations: [
    { assoc: { extension: '.foo', mimeType: 'application/x-foo', description: 'Foo file' }, ticked: true },
    { assoc: { extension: '.bar', mimeType: 'application/x-bar', description: 'Bar file' }, ticked: false },
  ],
  candidates: [], problem: null, ...overrides,
});

// Stands in for Tauri: records each call and answers from window.__answers.
const STUB = `window.__calls = []; window.__answers = {};
  window.__TAURI__ = { core: { invoke: async (command, args) => {
    window.__calls.push({ command, args });
    const answer = window.__answers[command];
    if (answer instanceof Error) throw answer.message;
    return answer;
  } } };`;

/** Width, height, and the alpha at the top-middle and centre, of PNG bytes. */
const INSPECT = `async (png) => {
  const bitmap = await createImageBitmap(new Blob([new Uint8Array(png)], { type: 'image/png' }));
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext('2d');
  context.drawImage(bitmap, 0, 0);
  const alpha = (x, y) => context.getImageData(x, y, 1, 1).data[3];
  const signature = png.slice(0, 8).join() === '137,80,78,71,13,10,26,10';
  return { width: bitmap.width, height: bitmap.height, top: alpha(128, 2), centre: alpha(128, 128), signature };
}`;

run(async () => {
  try {
    const page = await attach(DEBUG, 'about:blank');
    await page.send('Page.enable');
    await page.send('Page.addScriptToEvaluateOnNewDocument', { source: STUB });

    /** Load the page fresh, answer discover with `answer`, and press Find. */
    const findWith = async (answer) => {
      await page.send('Page.navigate', { url: PAGE });
      await page.loaded(new URL(PAGE).origin);
      await page.evaluate(`window.__answers.discover = ${JSON.stringify(answer)}; 1`);
      await page.evaluate(`document.getElementById('url').value = 'site.test'; document.getElementById('find-button').click(); 1`);
      await until(() => page.evaluate(`!document.getElementById('details').hidden && document.querySelectorAll('#gallery button').length > 1`), 'the gallery');
    };
    const tiles = () => page.evaluate(`[...document.querySelectorAll('#gallery small')].map((s) => s.textContent)`);
    /** Press Generate with a folder chosen, and return the request sent. */
    const generated = async () => {
      await page.evaluate(`window.__answers.pick_folder = 'C:\\\\out';
        window.__answers.generate = [{ path: 'C:\\\\out\\\\site-x64.exe', replaced: 'sameApp' }, { path: 'C:\\\\out\\\\site-arm64.exe', replaced: 'otherApp' }];
        document.getElementById('result').hidden = true;
        document.getElementById('generate').click(); 1`);
      await until(() => page.evaluate(`!document.getElementById('result').hidden`), 'the result card');
      return page.evaluate(`window.__calls.find((call) => call.command === 'generate').args.request`);
    };

    await check('PNG, ICO, JPEG and WebP become 256x256 PNGs; what does not decode is dropped', async () => {
      await findWith(discovery({
        candidates: [
          candidate('manifest', 'broken.png', 'image/png'),
          candidate('manifest', 'green.ico', 'image/x-icon'),
          candidate('icon', 'wide.jpg', 'image/jpeg'),
          candidate('icon', 'purple.webp', 'image/webp'),
          { label: 'favicon', url: 'https://site.test/red.png', contentType: 'image/png', bytes: [...fs.readFileSync(path.join(here, '..', 'docs', 'windows-runs', 'fixtures', 'photopea-red.png'))] },
        ],
      }));
      const labels = await tiles();
      assert(JSON.stringify(labels) === JSON.stringify(['manifest', 'icon', 'icon', 'favicon', 'Initials', 'Upload…']), `tiles: ${labels}`);
      const sizes = await page.evaluate(`Promise.all([...document.querySelectorAll('#gallery img')].map((i) => i.decode().then(() => i.naturalWidth + 'x' + i.naturalHeight)))`);
      assert(sizes.every((size) => size === '256x256'), `sizes: ${sizes}`);
      // The first decodable one is chosen; the wide JPEG is letterboxed.
      const request = await generated();
      const ico = await page.evaluate(`(${INSPECT})(${JSON.stringify(request.iconPng)})`);
      assert(ico.signature && ico.width === 256 && ico.height === 256 && ico.top === 255, `ico: ${JSON.stringify(ico)}`);
      await page.evaluate(`document.querySelectorAll('#gallery button')[1].click(); 1`);
      await page.evaluate(`window.__calls.length = 0; 1`);
      const wide = await page.evaluate(`(${INSPECT})(${JSON.stringify((await generated()).iconPng)})`);
      assert(wide.top === 0 && wide.centre === 255, `jpeg letterbox: ${JSON.stringify(wide)}`);
    });

    await check('the request carries the wrapped URL, the slug, and only ticked associations', async () => {
      await findWith(discovery({ candidates: [candidate('icon', 'green.ico', 'image/x-icon')] }));
      await page.evaluate(`document.getElementById('slug').value = 'mine'; 1`);
      const request = await generated();
      assert(request.url === 'https://site.test/' && request.slug === 'mine' && request.outDir === 'C:\\out', JSON.stringify(request).slice(0, 200));
      assert(JSON.stringify(request.associations.map((a) => a.extension)) === '[".foo"]', `associations: ${JSON.stringify(request.associations)}`);
      const shown = await page.evaluate(`document.getElementById('outputs').textContent`);
      assert(shown.includes('updated') && shown.includes('replaced another generated app'), `result: ${shown}`);
    });

    await check('with no usable icon, the initials icon is chosen', async () => {
      await findWith(discovery({ name: 'Mail Box', candidates: [candidate('manifest', 'broken.png', 'image/png')] }));
      const labels = await tiles();
      assert(JSON.stringify(labels) === JSON.stringify(['Initials', 'Upload…']), `tiles: ${labels}`);
      const icon = await page.evaluate(`(${INSPECT})(${JSON.stringify((await generated()).iconPng)})`);
      assert(icon.signature && icon.width === 256 && icon.centre === 255 && icon.top === 0, `initials: ${JSON.stringify(icon)}`);
    });

    await check("a site's markup is shown as text, never run or parsed", async () => {
      const name = '<b id="bold">Bold</b><img id="evil" src="x" onerror="window.__ran = 1">';
      await findWith(discovery({
        name,
        associations: [{ assoc: { extension: '.x', mimeType: 'application/x', description: '<i id="ital">i</i>' }, ticked: true }],
        problem: '<u id="under">u</u>',
      }));
      const state = await page.evaluate(`({
        value: document.getElementById('name').value,
        parsed: ['bold', 'evil', 'ital', 'under'].filter((id) => document.getElementById(id)),
        ran: window.__ran ?? 0,
        status: document.getElementById('status').textContent,
      })`);
      assert(state.value === name, `name: ${state.value}`);
      assert(state.parsed.length === 0 && state.ran === 0, `parsed: ${state.parsed}, ran: ${state.ran}`);
      assert(state.status.includes('<u id="under">'), `status: ${state.status}`);
    });

    assert(page.problems.length === 0, `page problems:\n${page.problems.join('\n')}`);
    page.close();
  } finally {
    chrome.kill();
    server.close();
  }
});
