// End-to-end check of the vscodeDev adapter against live https://vscode.dev,
// as a user would use it: open a file, edit it, press Ctrl+S. Run by
// .github/workflows/windows.yml on every push and weekly, to catch changes
// to vscode.dev. Needs:
//   WEBWRAP_E2E_APP_VSCODE  generated app for https://vscode.dev/, --adapter vscodeDev

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { attach, until } from './cdp.mjs';
import { assert, check, forward, launch, logged, makeFile, outside, required, run } from './apps.mjs';

const APP = required('WEBWRAP_E2E_APP_VSCODE');
const HOME = 'https://vscode.dev';
const DEBUG = 9304;

/** The open tabs, and the lines of the editor in front. Read-only. */
const STATE = `(() => ({
  tabs: [...document.querySelectorAll('.tab')].map((tab) => tab.textContent),
  lines: [...document.querySelectorAll('.view-lines .view-line')].map((line) => line.textContent),
}))()`;

async function key(page, modifiers, key, code, keyCode) {
  for (const type of ['rawKeyDown', 'keyUp']) {
    await page.send('Input.dispatchKeyEvent', {
      type, modifiers, key, code, windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode,
    });
  }
}

/** Wait until `name` is the file in front, showing `firstLine`. */
async function showing(page, name, firstLine) {
  await until(async () => {
    const state = await page.evaluate(STATE);
    return state.tabs.some((tab) => tab.includes(name)) && state.lines[0] === firstLine;
  }, `${name} open in vscode.dev showing "${firstLine}"`, 120000);
}

/** Replace the text of the file in front, then save it with Ctrl+S. */
async function editAndSave(page, file, text) {
  const name = path.basename(file);
  const before = fs.readFileSync(file, 'utf8');
  await key(page, 2, 'a', 'KeyA', 65);
  await page.send('Input.insertText', { text: `${text}\r\n` });
  await showing(page, name, text);
  assert(fs.readFileSync(file, 'utf8') === before, `${name} changed before Ctrl+S`);
  await key(page, 2, 's', 'KeyS', 83);
  await until(() => fs.readFileSync(file, 'utf8') === `${text}\r\n`, `${name} to be saved`, 30000);
  const download = path.join(os.homedir(), 'Downloads', name);
  assert(!fs.existsSync(download), `Ctrl+S created a download, ${download}`);
}

/** What the page looked like, for a failure report, with a screenshot. */
const explain = (page) => async () => {
  if (process.env.RUNNER_TEMP) {
    const shot = await page.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(path.join(process.env.RUNNER_TEMP, 'vscode-live.png'), Buffer.from(shot.data, 'base64'));
  }
  const state = await page.evaluate(`({
    ...${STATE},
    active: document.activeElement?.tagName + '.' + document.activeElement?.className,
    palette: document.querySelector('.quick-input-widget')?.style.display,
    paletteValue: document.querySelector('.quick-input-box input')?.value,
    notifications: [...document.querySelectorAll('.notification-toast')].map((n) => n.textContent),
    dialog: document.querySelector('.monaco-dialog-box')?.textContent,
  })`);
  return [JSON.stringify(state, null, 1), ...page.problems.slice(-20)].join('\n');
};

run(async () => {
  // Outside %LOCALAPPDATA%, where WebView2 lets a handle save (D2 evidence).
  const one = makeFile('live-one.md', 'before\r\n', outside);
  const app = launch(APP, DEBUG, [one]);
  const page = await attach(DEBUG, HOME);

  await check('vscode.dev opens a file given at launch', async () => {
    await logged(app, /delivered live-one\.md/, 'the handle delivery in the log');
    await showing(page, 'live-one.md', 'before');
  }, explain(page));

  await check('Ctrl+S in vscode.dev changes the original file', async () => {
    await editAndSave(page, one, 'after');
  }, explain(page));

  await check('two files from one later launch open in two new tabs, and Ctrl+S saves', async () => {
    const two = makeFile('live-two.md', 'second\r\n', outside);
    const three = makeFile('live-three.md', 'third\r\n', outside);
    await forward(APP, DEBUG, two, three);
    await showing(page, 'live-three.md', 'third');
    const tabs = (await page.evaluate(STATE)).tabs;
    for (const name of ['live-one.md', 'live-two.md']) {
      assert(tabs.some((tab) => tab.includes(name)), `no tab for ${name}: ${tabs}`);
    }
    await editAndSave(page, three, 'changed');
    assert(fs.readFileSync(one, 'utf8') === 'after\r\n', 'live-one.md changed too');
    assert(fs.readFileSync(two, 'utf8') === 'second\r\n', 'live-two.md changed too');
  }, explain(page));
});
