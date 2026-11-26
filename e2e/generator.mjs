// End-to-end check of the generator (F4) on real WebView2: find a local
// HTTPS site, choose a folder in the Windows folder picker, generate, and
// inspect what was written. Run by .github/workflows/windows.yml; needs:
//   WEBWRAP_E2E_GENERATOR  tauri_generator.exe for this runner
//   WEBWRAP_E2E_INSPECT    webwrap-generate.exe, for `inspect`
//   WEBWRAP_E2E_PFX        certificate for localhost, password "webwrap",
//                          trusted in Cert:\LocalMachine\Root: discovery
//                          verifies against the Windows store
//   WEBWRAP_E2E_NATIVE     x64 or arm64, the outputs this runner can run

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import https from 'node:https';
import path from 'node:path';
import { attach, until } from './cdp.mjs';
import { assert, check, outside, pickInDialog, required, run, start } from './apps.mjs';

const GENERATOR = required('WEBWRAP_E2E_GENERATOR');
const INSPECT = required('WEBWRAP_E2E_INSPECT');
const PFX = fs.readFileSync(required('WEBWRAP_E2E_PFX'));
const NATIVE = required('WEBWRAP_E2E_NATIVE');
const SITE = 'https://localhost:8448/';
const DEBUG = 9311;
const RED = fs.readFileSync(path.join(import.meta.dirname, '..', 'docs', 'windows-runs', 'fixtures', 'photopea-red.png'));

const ROUTES = {
  '/': ['text/html', '<title>Ignored | Site</title><link rel="manifest" href="/app.webmanifest"><link rel="icon" href="/red.png">'],
  '/app.webmanifest': ['application/manifest+json', JSON.stringify({
    short_name: 'Gen Test',
    icons: [{ src: '/red.png', sizes: '256x256', type: 'image/png' }],
    file_handlers: [{ name: 'Gen test file', accept: { 'application/x-gentest': ['.GENTEST'] } }],
  })],
  '/red.png': ['image/png', RED],
};
const server = https.createServer({ pfx: PFX, passphrase: 'webwrap' }, (request, response) => {
  const route = ROUTES[new URL(request.url, SITE).pathname];
  if (!route) return response.writeHead(404).end();
  response.writeHead(200, { 'Content-Type': route[0] }).end(route[1]);
});
await new Promise((resolve) => server.listen(8448, 'localhost', resolve));

run(async () => {
  const folder = fs.mkdtempSync(path.join(outside, 'generated-'));
  const generator = start(GENERATOR, DEBUG);
  const page = await attach(DEBUG, 'https://tauri.localhost');
  await page.loaded('https://tauri.localhost');
  const explain = async () => {
    const state = await page.evaluate(`({ status: document.getElementById('status').textContent,
      tiles: [...document.querySelectorAll('#gallery small')].map((s) => s.textContent),
      outputs: document.getElementById('outputs').textContent })`).catch((error) => error.message);
    return `  page: ${JSON.stringify(state)}\n${page.problems.slice(-10).join('\n')}`;
  };

  /** Press Generate, choose `folder` in the Windows picker, wait for the result. */
  const generate = async () => {
    await page.evaluate(`document.getElementById('result').hidden = true; document.getElementById('generate').click(); 1`);
    const outcome = pickInDialog(generator.pid, folder);
    assert(outcome === 'picked', `the folder picker: ${outcome}`);
    await until(() => page.evaluate(`!document.getElementById('result').hidden`), 'the result card', 60000);
    return page.evaluate(`document.getElementById('outputs').textContent`);
  };

  await check('the generator finds a site: name, wrapped URL, icon and a declared association', async () => {
    await page.evaluate(`document.getElementById('url').value = ${JSON.stringify(SITE)}; document.getElementById('find-button').click(); 1`);
    await until(() => page.evaluate(`!document.getElementById('details').hidden`), 'the details', 60000);
    const state = await page.evaluate(`({
      name: document.getElementById('name').value,
      slug: document.getElementById('slug').value,
      wrapped: document.getElementById('wrapped').textContent,
      tiles: [...document.querySelectorAll('#gallery small')].map((s) => s.textContent),
      assocs: document.getElementById('assocs').textContent,
      status: document.getElementById('status').textContent,
    })`);
    assert(state.name === 'Gen Test' && state.slug === 'localhost' && state.wrapped === SITE, JSON.stringify(state));
    assert(state.tiles[0] === 'manifest', `tiles: ${state.tiles}`);
    assert(state.assocs.includes('.gentest'), `associations: ${state.assocs}`);
  }, explain);

  await check('Generate asks for a folder in the Windows picker and writes both apps', async () => {
    const shown = await generate();
    assert((shown.match(/created/g) ?? []).length === 2, `result: ${shown}`);
    for (const arch of ['x64', 'arm64']) {
      const file = path.join(folder, `localhost-${arch}.exe`);
      const report = JSON.parse(execFileSync(INSPECT, ['inspect', '--input', file], { encoding: 'utf8' }));
      assert(report.iconPresent === true && report.fileDescription === 'Gen Test', `${arch}: ${JSON.stringify(report)}`);
    }
    const self = JSON.parse(execFileSync(path.join(folder, `localhost-${NATIVE}.exe`), ['--inspect-self'], { encoding: 'utf8' }));
    assert(self.displayName === 'Gen Test' && self.launchUrl === SITE && self.slug === 'localhost', JSON.stringify(self));
    assert(JSON.stringify(self.fileAssociations.map((a) => a.extension)) === '[".gentest"]', `associations: ${JSON.stringify(self.fileAssociations)}`);
  }, explain);

  await check('generating again into the same folder updates both apps', async () => {
    const shown = await generate();
    assert((shown.match(/updated/g) ?? []).length === 2, `result: ${shown}`);
  }, explain);

  server.close();
});
