# Next Windows run: D1 file plumbing, hands-on checks

Branch `feat/file-bridge-plumbing`. Plan: `docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md`. CI already drove the file bridge end to end on x64 and ARM64. This run checks only what needs a person: windows coming to the front, message boxes, and dragging with the mouse. Nothing here needs a compiler.

**Expect this:** in this phase, a file handed to Photopea does **not** appear inside Photopea yet. That arrives with the Photopea adapter (D3). Here only the window behaviour is checked.

## 1. Get the apps

1. `gh run list --repo ptrkhh/tauri-generator --branch feat/file-bridge-plumbing --workflow windows --status success --limit 5 --json databaseId,headSha,url`. Use the run whose `headSha` equals `git rev-parse HEAD`. If none does, stop and report "CI has not passed for this commit yet".
2. In a new empty folder `%TEMP%\webwrap-run-<timestamp>`, download this PC's artifact. It is `acceptance-aarch64-pc-windows-msvc` if `$env:PROCESSOR_ARCHITECTURE` is `ARM64`, otherwise `acceptance-x86_64-pc-windows-msvc`:
   `gh run download <databaseId> --repo ptrkhh/tauri-generator -n <artifact> -D <folder>\apps`
   If `gh` is missing or not logged in, give me the run `url` and ask me to download and extract it there.
3. Let `<arch>` be `arm64` or `x64` to match. The folder must contain `photopea-<arch>.exe` and `twitter-<arch>.exe`.
4. Copy `docs\windows-runs\fixtures\photopea-red.png` from the repo into the work folder as `red.png`.

## 2. Checks

Close every Photopea and Twitter window before starting.

- **A. A second launch brings the running window to the front.**
  1. Launch `photopea-<arch>.exe` and wait for Photopea to load.
  2. Open File Explorer on the apps folder so Explorer covers Photopea.
  3. Ask me to double-click `photopea-<arch>.exe` in Explorer.
  4. Ask me to confirm: the existing Photopea window comes to the front; no second Photopea window opens; no message box appears.
  5. Also record `(Get-Process | Where-Object MainWindowTitle -eq 'Photopea').Count`. Expected: `1`.
- **B. Also when minimized.** Ask me to minimize Photopea, then double-click `photopea-<arch>.exe` again. Expected: Photopea is restored and in front; still one window.
- **C. Dropping a file on the exe.** Ask me to drag `red.png` from Explorer onto `photopea-<arch>.exe`. Expected: Photopea comes to the front; no new window; no message box. (The image does not open inside Photopea yet; that is expected.)
- **D. Dropping a file on the window.** Ask me to drag `red.png` from Explorer onto the open Photopea window. Expected: Photopea opens the image as a new document, as it would in a browser.
- **E. A missing file is reported.** With Photopea still running, run `Start-Process <folder>\apps\photopea-<arch>.exe -ArgumentList 'C:\nope\missing.psd' -PassThru -Wait`. Ask me to read out the message box text word for word, then dismiss it. Record the exit code: expected `0`, because the running window was still brought forward.
- **F. Pop-ups still go to the browser.** Launch `twitter-<arch>.exe`. Ask me to click a link on the page that opens in a new tab on the web, such as Terms of Service. Expected: it opens in my default browser, and no new app window appears.

Close every app afterwards.

## 3. Extra results to record

The CI run id and URL you used.
