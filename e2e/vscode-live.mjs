// End-to-end check of the vscodeDev adapter against live https://vscode.dev,
// as a user would use it: open a file, edit it, press Ctrl+S. Run by
// .github/workflows/windows.yml on every push and weekly, to catch changes
// to vscode.dev. Needs:
//   WEBWRAP_E2E_APP_VSCODE  generated app for https://vscode.dev/, --adapter vscodeDev

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { attach, until } from './cdp.mjs';
import { assert, check, forward, launch, logged, makeFile, required, run } from './apps.mjs';

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

run(async () => {
  const one = makeFile('live-one.md', 'before\r\n');
  const app = launch(APP, DEBUG, [one]);
  const page = await attach(DEBUG, HOME);

  await check('vscode.dev opens a file given at launch', async () => {
    await showing(page, 'live-one.md', 'before');
    await logged(app, /vscode-open done/, 'a finished Open File command');
  });

  await check('Ctrl+S in vscode.dev changes the original file', async () => {
    await editAndSave(page, one, 'after');
  });

  await check('a second launch opens its file in a new vscode.dev tab, and Ctrl+S saves it', async () => {
    const two = makeFile('live-two.md', 'second\r\n');
    await forward(APP, DEBUG, two);
    await showing(page, 'live-two.md', 'second');
    const tabs = (await page.evaluate(STATE)).tabs;
    assert(tabs.some((tab) => tab.includes('live-one.md')), `the first tab is gone: ${tabs}`);
    await editAndSave(page, two, 'changed');
    assert(fs.readFileSync(one, 'utf8') === 'after\r\n', 'the first file changed too');
  });
});
