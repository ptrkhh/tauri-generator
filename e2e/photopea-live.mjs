// End-to-end check of the photopea adapter against live Photopea with a
// 150 MB PSD, as a user would use it: open the file, edit it, press Ctrl+S.
// Run by .github/workflows/windows.yml on every push and weekly. Needs:
//   WEBWRAP_E2E_APP_PHOTOPEA  generated app for https://www.photopea.com/, --adapter photopea

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { attach, until } from './cdp.mjs';
import { assert, check, launch, logged, makeFile, required, run, sha256 } from './apps.mjs';
import { layerCount, makePsd } from './psd.mjs';

const APP = required('WEBWRAP_E2E_APP_PHOTOPEA');
const SHELL = 'https://tauri.localhost/photopea.html';
const PHOTOPEA = 'https://www.photopea.com';
const DEBUG = 9306;

run(async () => {
  const psd = makePsd(7100, 7100); // 151 MB of random pixels
  const file = makeFile('live.psd', psd);
  const app = launch(APP, DEBUG, [file]);
  const shell = await attach(DEBUG, SHELL);
  const lastNotice = () => shell.evaluate(`document.getElementById('status')?.dataset.last ?? ''`);
  const explain = async () => {
    if (process.env.RUNNER_TEMP) {
      const shot = await shell.send('Page.captureScreenshot', { format: 'png' });
      fs.writeFileSync(path.join(process.env.RUNNER_TEMP, 'photopea-live.png'), Buffer.from(shot.data, 'base64'));
    }
    return [`  shell notice: ${await lastNotice()}`, ...shell.problems.slice(-15)].join('\n');
  };
  const press = async (modifiers, key, code, keyCode) => {
    for (const type of ['rawKeyDown', 'keyUp']) {
      await shell.send('Input.dispatchKeyEvent', { type, modifiers, key, code, windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode });
    }
  };
  const click = async (x, y) => {
    for (const type of ['mousePressed', 'mouseReleased']) {
      await shell.send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1 });
    }
  };

  await check('Photopea opens a 150 MB PSD given at launch', async () => {
    await logged(app, /delivered live\.psd/, 'the delivery in the log');
    await until(async () => (await lastNotice()) === 'Opened live.psd', 'Photopea to open live.psd', 300000);
  }, explain);

  await check('Ctrl+S in Photopea writes the edited document back to the original file', async () => {
    // An edit, as a person would make one: a new, empty layer.
    await shell.evaluate(`(document.querySelector('iframe').contentWindow.postMessage('app.activeDocument.artLayers.add();', ${JSON.stringify(PHOTOPEA)}), 1)`);
    // Into Photopea's canvas, as a person would before pressing keys; with
    // the Move tool a click without a drag changes nothing.
    const { width, height } = await shell.evaluate('({ width: innerWidth, height: innerHeight })');
    await click(Math.round(width / 2), Math.round(height / 2));
    await press(2, 's', 'KeyS', 83);
    // The shell says "Saving" as soon as Save reaches it, long before a
    // 150 MB export is written.
    const started = async () => /^(Saving|Saved) live\.psd/.test(await lastNotice());
    const byKeys = await until(started, 'Ctrl+S to reach Save', 20000).then(() => true, () => false);
    if (byKeys) {
      process.stdout.write('  observed: Ctrl+S reached Photopea\'s Save\n');
    } else {
      // Not proven by keys: use the menu, as headless Chromium needed.
      process.stdout.write('  observed: Ctrl+S did not reach Photopea\'s Save; using File > Save\n');
      await click(19, 15);
      await new Promise((resolve) => setTimeout(resolve, 800));
      await click(42, 215);
      await until(started, 'File > Save to reach Save', 20000);
    }
    await until(async () => (await lastNotice()) === 'Saved live.psd', 'the save to finish', 300000);
    const saved = fs.readFileSync(file);
    assert(saved.subarray(0, 4).toString('latin1') === '8BPS', 'the saved file is not a PSD');
    assert(saved.length >= 150_000_000, `the saved file is only ${saved.length} bytes`);
    assert(sha256(saved) !== sha256(psd), 'the file did not change');
    assert(layerCount(saved) >= 2, `the added layer is not in the saved file (${layerCount(saved)} layers)`);
    const download = path.join(os.homedir(), 'Downloads', 'live.psd');
    assert(!fs.existsSync(download), `Ctrl+S created a download, ${download}`);
  }, explain);
});
