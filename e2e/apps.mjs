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
/** Decline the first-launch association offer (E) before an app starts: its
 * box would hold the window back. The offer itself is a PC check. */
function declineOffer(exe) {
  const { webview2DataDir } = JSON.parse(execFileSync(exe, ['--inspect-self'], { encoding: 'utf8' }));
  fs.mkdirSync(webview2DataDir, { recursive: true });
  fs.writeFileSync(path.join(webview2DataDir, 'assoc-declined'), '');
}

export function launch(exe, debugPort, args = []) {
  declineOffer(exe);
  return start(exe, debugPort, args);
}

/** Start any WebWrap executable with a DevTools port, its stderr captured
 * in `child.log`. `launch` is this plus the generated apps' offer decline. */
export function start(exe, debugPort, args = []) {
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

/** In %TEMP%, which on Windows is inside %LOCALAPPDATA%. */
export const work = fs.mkdtempSync(path.join(os.tmpdir(), 'webwrap-e2e-'));
/** Outside %LOCALAPPDATA%: the runner's own temporary folder. */
export const outside = fs.mkdtempSync(path.join(process.env.RUNNER_TEMP ?? os.tmpdir(), 'webwrap-e2e-'));
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
export function makeFile(name, content, dir = work) {
  const file = path.join(dir, name);
  fs.writeFileSync(file, content);
  return file;
}

export const logged = (app, pattern, what) => until(() => pattern.test(app.log), what);

/** Run one check. On failure, `explain` may print what the page looked like. */
export async function check(name, body, explain) {
  process.stdout.write(`- ${name}\n`);
  try {
    await body();
  } catch (error) {
    if (explain) {
      try {
        process.stdout.write(`  diagnosis:\n${await explain()}\n`);
      } catch (failed) {
        process.stdout.write(`  (diagnosis failed: ${failed.message})\n`);
      }
    }
    throw error;
  }
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
    for (const dir of [work, outside]) {
      try {
        fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
      } catch (error) {
        // WebView2 processes can outlive a killed app for a moment.
        process.stdout.write(`  (could not remove ${dir}: ${error.code})\n`);
      }
    }
    process.exit(failed ? 1 : 0);
  }
}

/** Choose `file` in an open dialog of the app or of any process it started,
 * as a person would. The runtime's own dialog (D3) belongs to the app; the
 * one WebView2 shows for a site's showOpenFilePicker (D4) belongs to
 * WebView2's browser process. Returns "picked", or why it could not, having
 * closed the dialog. The dialog's title goes to stderr. */
export function pickInDialog(pid, file) {
  // EnumWindows finds the dialog however UI Automation lists it. UI
  // Automation does not expose the dialog's file-name box or Open button on
  // the runners, but both are classic child windows with fixed control ids:
  // 1148 for the file name (1152, a plain Edit, in a folder picker, as CI
  // showed for the generator's) and 1 for Open.
  const script = `
    $ErrorActionPreference = 'Stop'
    Add-Type -TypeDefinition @'
      using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
      public static class OpenDialog {
        delegate bool Visit(IntPtr window, IntPtr data);
        [DllImport("user32.dll")] static extern bool EnumWindows(Visit visit, IntPtr data);
        [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, Visit visit, IntPtr data);
        [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder name, int size);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
        [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
        [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr window);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr w, string l);
        [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
        static string Class(IntPtr window) { var name = new StringBuilder(256); GetClassName(window, name, 256); return name.ToString(); }
        static string Text(IntPtr window) { var text = new StringBuilder(256); GetWindowText(window, text, 256); return text.ToString(); }
        // A dialog with a file-name or folder box, so a message box is never taken for it.
        public static IntPtr Find(uint[] pids) {
          IntPtr found = IntPtr.Zero;
          EnumWindows((window, data) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (Array.IndexOf(pids, owner) >= 0 && IsWindowVisible(window) && Class(window) == "#32770" &&
                Children(window).Exists((child) => GetDlgCtrlID(child) == 1148 || GetDlgCtrlID(child) == 1152)) found = window;
            return true;
          }, IntPtr.Zero);
          return found;
        }
        public static string Title(IntPtr window) { return Text(window); }
        // For a failure report: every visible window of these processes, with its controls.
        public static string Describe(uint[] pids) {
          var text = new StringBuilder();
          EnumWindows((window, data) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (Array.IndexOf(pids, owner) < 0 || !IsWindowVisible(window)) return true;
            text.Append(Class(window) + " '" + Text(window) + "' [");
            foreach (var child in Children(window)) if (GetDlgCtrlID(child) != 0) text.Append(Class(child) + "#" + GetDlgCtrlID(child) + " ");
            text.Append("]; ");
            return true;
          }, IntPtr.Zero);
          return text.ToString();
        }
        static List<IntPtr> Children(IntPtr parent) {
          var all = new List<IntPtr>();
          EnumChildWindows(parent, (window, data) => { all.Add(window); return true; }, IntPtr.Zero);
          return all;
        }
        public static string Pick(IntPtr dialog, string path) {
          IntPtr name = IntPtr.Zero, open = IntPtr.Zero;
          var seen = new StringBuilder();
          foreach (var child in Children(dialog)) {
            int id = GetDlgCtrlID(child);
            string cls = Class(child);
            if (id == 1148 || id == 1152 || id == 1) seen.Append(cls + "#" + id + " ");
            if ((id == 1148 || id == 1152) && cls == "Edit") name = child;
            if (id == 1148 && name == IntPtr.Zero) {
              foreach (var inner in Children(child)) if (Class(inner) == "Edit") name = inner;
            }
            if (id == 1 && cls == "Button") open = child;
          }
          if (name == IntPtr.Zero || open == IntPtr.Zero) return "no file-name box or Open button (" + seen + ")";
          SendMessage(name, 0x000C, IntPtr.Zero, path);       // WM_SETTEXT
          PostMessage(open, 0x00F5, IntPtr.Zero, IntPtr.Zero); // BM_CLICK
          return "picked";
        }
        public static void Close(IntPtr dialog) { PostMessage(dialog, 0x0010, IntPtr.Zero, IntPtr.Zero); } // WM_CLOSE
      }
'@
    $deadline = (Get-Date).AddSeconds(30)
    do {
      # The app and every process under it, looked up afresh each time.
      $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
      $pids = @([uint32]$env:APP_PID)
      do {
        $more = @($all | Where-Object { $pids -contains $_.ParentProcessId -and $pids -notcontains $_.ProcessId } | ForEach-Object { [uint32]$_.ProcessId })
        $pids += $more
      } while ($more.Count -gt 0)
      $dialog = [OpenDialog]::Find([uint32[]]$pids)
      if ($dialog -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 500 }
    } until ($dialog -ne [IntPtr]::Zero -or (Get-Date) -gt $deadline)
    if ($dialog -eq [IntPtr]::Zero) { 'no open dialog appeared (windows: ' + [OpenDialog]::Describe([uint32[]]$pids) + ')'; exit }
    [Console]::Error.WriteLine("  open dialog titled '" + [OpenDialog]::Title($dialog) + "'")
    Start-Sleep -Milliseconds 1000
    $result = [OpenDialog]::Pick($dialog, $env:PICK_PATH)
    if ($result -ne 'picked') { [OpenDialog]::Close($dialog) }
    $result`;
  return execFileSync('powershell', ['-NoProfile', '-Command', script], {
    env: { ...process.env, APP_PID: String(pid), PICK_PATH: file },
    timeout: 90000,
  }).toString().trim();
}
