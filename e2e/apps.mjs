// Launching generated apps and checking them, shared by the end-to-end
// scripts. The apps' stderr carries one `webwrap: <event>` line per native
// decision, which is how the native side is observed without test hooks.

import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { sleep, until } from './cdp.mjs';

export function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}

const children = [];
export function launch(exe, debugPort, args = []) {
  const child = spawn(exe, args, {
    env: {
      ...process.env,
      // Test only: a debugging port to drive the page, and acceptance of the
      // test server's self-signed certificate.
      WEBWRAP_TEST_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort} --ignore-certificate-errors`,
    },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  child.log = '';
  let partial = '';
  child.stderr.on('data', (chunk) => {
    child.log += chunk;
    // Print whole lines only, so two apps' output never interleaves mid-line.
    const lines = (partial + chunk).split('\n');
    partial = lines.pop();
    for (const line of lines) process.stdout.write(`  [${path.basename(exe)} ${child.pid}] ${line}\n`);
  });
  child.exitCode$ = new Promise((resolve) => child.on('exit', resolve));
  child.on('exit', (code) => process.stdout.write(`  [${path.basename(exe)} ${child.pid}] exited ${code}\n`));
  children.push(child);
  return child;
}

/** Launch an app again with files; it must hand them over and exit 0. */
export async function forward(exe, debugPort, ...files) {
  const second = launch(exe, debugPort, files);
  const code = await Promise.race([second.exitCode$, sleep(20000).then(() => 'timeout')]);
  if (code !== 0) throw new Error(`second launch of ${path.basename(exe)} ended with ${code}`);
}

export const work = fs.mkdtempSync(path.join(os.tmpdir(), 'webwrap-e2e-'));
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
export function makeFile(name, content) {
  const file = path.join(work, name);
  fs.writeFileSync(file, content);
  return file;
}

export const logged = (app, pattern, what) => until(() => pattern.test(app.log), what);

export async function check(name, body) {
  process.stdout.write(`- ${name}\n`);
  await body();
  process.stdout.write(`  PASS ${name}\n`);
}

export function assert(condition, message) {
  if (!condition) throw new Error(message);
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
    process.stdout.write(execFileSync('netstat', ['-ano']).toString().split('\n').filter((l) => /:93\d\d\b/.test(l)).join('\n') + '\n');
  } catch (error) {
    process.stdout.write(`  (diagnostics failed: ${error.message})\n`);
  }
}

/** Run the checks in `main`, report, clean up, and exit with the result. */
export async function run(main) {
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
}
