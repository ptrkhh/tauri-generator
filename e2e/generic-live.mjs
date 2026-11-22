// End-to-end check of a generic site (D4, adapter: none) against live
// https://excalidraw.com, which consumes launchQueue: a file given at launch
// should reach it as a handle (tier 1), and its Ctrl+S should write the
// original. Run by .github/workflows/windows.yml on every push and weekly.
// Needs:
//   WEBWRAP_E2E_APP_EXCALIDRAW  generated app for https://excalidraw.com/, no adapter

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { attach, until } from './cdp.mjs';
import { assert, check, launch, makeFile, outside, required, run } from './apps.mjs';

const APP = required('WEBWRAP_E2E_APP_EXCALIDRAW');
const HOME = 'https://excalidraw.com';
const DEBUG = 9307;
const NAME = 'live.excalidraw';

// One rectangle with an id this test can find; Excalidraw fills in the rest.
const SCENE = JSON.stringify({
  type: 'excalidraw', version: 2, source: 'webwrap-e2e',
  elements: [{ id: 'webwrap-e2e', type: 'rectangle', x: 100, y: 100, width: 200, height: 120 }],
  appState: {}, files: {},
});

const explain = (page) => async () => {
  if (process.env.RUNNER_TEMP) {
    const shot = await page.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(path.join(process.env.RUNNER_TEMP, 'excalidraw-live.png'), Buffer.from(shot.data, 'base64'));
  }
  return page.problems.slice(-20).join('\n');
};

run(async () => {
  // Outside %LOCALAPPDATA%, where WebView2 gives a site a handle (D2 evidence).
  const file = makeFile(NAME, SCENE, outside);
  const app = launch(APP, DEBUG, [file]);
  const page = await attach(DEBUG, HOME);
  let tier1 = false;

  await check('Excalidraw takes a file given at launch through launchQueue', async () => {
    const outcome = await until(
      () => app.log.match(/file-tier \d live\.excalidraw|open-failed live\.excalidraw/)?.[0],
      'a tier for the file', 60000,
    );
    if (outcome !== `file-tier 1 ${NAME}`) {
      // Evidence, not a failure: tier 1 then rests on the local test.
      process.stdout.write(`  EXCALIDRAW DID NOT USE launchQueue: ${outcome}\n`);
      return;
    }
    tier1 = true;
    // Excalidraw keeps its scene in localStorage once it has loaded one.
    await until(() => page.evaluate(`(localStorage.getItem('excalidraw') ?? '').includes('webwrap-e2e')`),
      'the rectangle in the scene', 30000);
  }, explain(page));

  if (!tier1) return;
  await check('Ctrl+S in Excalidraw writes the original file', async () => {
    for (const type of ['rawKeyDown', 'keyUp']) {
      await page.send('Input.dispatchKeyEvent', {
        type, modifiers: 2, key: 's', code: 'KeyS', windowsVirtualKeyCode: 83, nativeVirtualKeyCode: 83,
      });
    }
    // Excalidraw writes its own serialization, so the bytes change.
    await until(() => {
      const now = fs.readFileSync(file, 'utf8');
      return now !== SCENE && now.includes('webwrap-e2e');
    }, `${NAME} to be saved`, 30000);
    const download = path.join(os.homedir(), 'Downloads', NAME);
    assert(!fs.existsSync(download), `Ctrl+S created a download, ${download}`);
  }, explain(page));
});
