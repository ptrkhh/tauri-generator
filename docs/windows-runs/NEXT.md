# Next Windows run: D1 file plumbing and D2 vscode.dev, hands-on checks

Branch `feat/vscode-adapter`. Plans:

- `docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md` (D1, merged);
- `docs/superpowers/plans/2026-11-20-vscode-dev-adapter.md` (D2).

CI already drove both end to end on x64 and ARM64, including live vscode.dev. This run checks only what needs a person: windows coming to the front, message boxes, dragging with the mouse, and the Windows Open dialog. Nothing here needs a compiler.

**Expect this:** a file handed to Photopea does **not** appear inside Photopea yet. That arrives with the Photopea adapter (D3). For Photopea, only the window behaviour is checked.

## 1. Get the apps

1. Find the CI run:
   `gh run list --repo ptrkhh/tauri-generator --branch feat/vscode-adapter --workflow windows --status success --limit 5 --json databaseId,headSha,url`
   Use the run whose `headSha` equals `git rev-parse HEAD`. If none does, stop and report "CI has not passed for this commit yet".
2. Make a new empty folder `%TEMP%\webwrap-run-<timestamp>` and download this PC's artifact into it:
   - The artifact is `acceptance-aarch64-pc-windows-msvc` if `$env:PROCESSOR_ARCHITECTURE` is `ARM64`, otherwise `acceptance-x86_64-pc-windows-msvc`.
   - Download it with `gh run download <databaseId> --repo ptrkhh/tauri-generator -n <artifact> -D <folder>\apps`.
   - If `gh` is missing or not logged in, give me the run `url` and ask me to download and extract it there.
3. Let `<arch>` be `arm64` or `x64` to match. The folder must contain `photopea-<arch>.exe`, `twitter-<arch>.exe`, and `vscode-<arch>.exe`.
4. Copy `docs\windows-runs\fixtures\photopea-red.png` from the repo into the work folder as `red.png`.
5. Make the folder `%USERPROFILE%\Documents\webwrap-run-<timestamp>`, which the vscode.dev checks use. It must not be under `%TEMP%`, for the reason given in check K. Create these files there, as UTF-8 without BOM, each ending in CRLF:
   - `note-one.md` containing `first`;
   - `note-two.md` containing `second`;
   - `note-three.md` containing `third`.

## 2. Checks

Close every Photopea, Twitter, and VS Code app window before starting.

**D1: window behaviour**

- **A. A second launch brings the running window to the front.**
  1. Launch `photopea-<arch>.exe` and wait for Photopea to load.
  2. Open File Explorer on the apps folder so Explorer covers Photopea.
  3. Ask me to double-click `photopea-<arch>.exe` in Explorer.
  4. Ask me to confirm:
     - the existing Photopea window comes to the front;
     - no second Photopea window opens;
     - no message box appears.
  5. Also record `(Get-Process | Where-Object MainWindowTitle -eq 'Photopea').Count`. Expected: `1`.
- **B. Also when minimized.** Ask me to minimize Photopea, then double-click `photopea-<arch>.exe` again. Expected: Photopea is restored and in front, and there is still one window.
- **C. Dropping a file on the exe.** Ask me to drag `red.png` from Explorer onto `photopea-<arch>.exe`.
  - Expected: Photopea comes to the front, with no new window and no message box.
  - The image does not open inside Photopea yet; that is expected.
- **D. Dropping a file on the window.** Ask me to drag `red.png` from Explorer onto the open Photopea window. Expected: Photopea opens the image as a new document, as it would in a browser.
- **E. A missing file is reported.**
  1. With Photopea still running, run `Start-Process <folder>\apps\photopea-<arch>.exe -ArgumentList 'C:\nope\missing.psd' -PassThru -Wait`.
  2. Ask me to read out the message box text word for word, then dismiss it.
  3. Record the exit code. Expected: `0`, because the running window was still brought forward.
- **F. Pop-ups still go to the browser.** Launch `twitter-<arch>.exe`. Ask me to click a link on the page that opens in a new tab on the web, such as Terms of Service. Expected: it opens in my default browser, and no new app window appears.

Close every app before the vscode.dev checks.

**D2: vscode.dev**

- **G. Opening a file by dropping it on the exe.**
  1. Ask me to drag `note-one.md` from Explorer onto `vscode-<arch>.exe`, with no VS Code app running.
  2. Ask me to confirm:
     - the app opens vscode.dev;
     - within about 30 s, `note-one.md` opens in a tab by itself, showing `first`;
     - nothing was typed anywhere, and no message box appeared.
  3. Record how many seconds it took.
- **H. Ctrl+S saves the original.**
  1. Ask me to click into the editor, change `first` to `edited`, and press Ctrl+S.
  2. Check the file with `Get-Content -Raw <docs folder>\note-one.md`. Expected: `edited` plus CRLF.
  3. Also check that `%USERPROFILE%\Downloads\note-one.md` does not exist.
- **I. Two more files from one launch open as new tabs.**
  1. Ask me to select `note-two.md` and `note-three.md` together in Explorer and drag both onto `vscode-<arch>.exe`.
  2. Expected:
     - the running window comes to the front;
     - two new tabs open, one after the other, with the right text;
     - no second window appears.
- **J. Ctrl+O uses the Windows Open dialog, and saves back.**
  1. Ask me to press Ctrl+O inside the app. Expected: a Windows Open dialog appears.
     - If Ctrl+O does nothing, ask me to use vscode.dev's menu instead (the ☰ button, then File > Open File...), and record that Ctrl+O did nothing.
  2. Ask me to pick `note-three.md` again in the dialog. Expected: it opens, or vscode.dev switches to its tab.
  3. Ask me to change the text to `picked`, then press Ctrl+S.
  4. Check with `Get-Content -Raw`. Expected: `picked` plus CRLF.
  5. Ask me whether any permission prompt appeared, and record its text.
- **K. A file in `%TEMP%` is refused with a message.**
  1. Copy `note-one.md` into the `%TEMP%` work folder as `in-temp.md`.
  2. Run `Start-Process <folder>\apps\vscode-<arch>.exe -ArgumentList '<work folder>\in-temp.md'`.
  3. Ask me to read out the message box text word for word, then dismiss it. Expected: it names the file and says vscode.dev cannot use files in folders Windows keeps for apps.
  4. Expected: the file does not open in vscode.dev.

Close every app afterwards. Delete the Documents folder from step 5.

## 3. Extra results to record

- The CI run id and URL you used.
- For G, the seconds until the file opened.
- For J, whether Ctrl+O itself opened the dialog, and any permission prompt.
