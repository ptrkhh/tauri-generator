# Next Windows run: D1 file plumbing, D2 vscode.dev, D3 Photopea, D4 generic sites, hands-on checks

Branch `main`. Plans:

- `docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md` (D1, merged);
- `docs/superpowers/plans/2026-11-20-vscode-dev-adapter.md` (D2, merged);
- `docs/superpowers/plans/2026-11-20-photopea-adapter.md` (D3, merged);
- `docs/superpowers/plans/2026-11-21-generic-sites.md` (D4, merged).

CI already drove all four end to end on x64 and ARM64, including live vscode.dev, live Photopea with a 150 MB PSD, and live Excalidraw. This run checks only what needs a person: windows coming to the front, message boxes, dragging with the mouse, the keyboard, and the Windows Open dialog. Nothing here needs a compiler.

## 1. Get the apps

1. Find the CI run:
   `gh run list --repo ptrkhh/tauri-generator --branch main --workflow windows --status success --limit 5 --json databaseId,headSha,url`
   Use the run whose `headSha` equals `git rev-parse HEAD`. If none does, stop and report "CI has not passed for this commit yet".
2. Make a new empty folder `%TEMP%\webwrap-run-<timestamp>` and download this PC's artifact into it:
   - The artifact is `acceptance-aarch64-pc-windows-msvc` if `$env:PROCESSOR_ARCHITECTURE` is `ARM64`, otherwise `acceptance-x86_64-pc-windows-msvc`.
   - Download it with `gh run download <databaseId> --repo ptrkhh/tauri-generator -n <artifact> -D <folder>\apps`.
   - If `gh` is missing or not logged in, give me the run `url` and ask me to download and extract it there.
3. Let `<arch>` be `arm64` or `x64` to match. The folder must contain `excalidraw-<arch>.exe`, `photopea-<arch>.exe`, `twitter-<arch>.exe`, and `vscode-<arch>.exe`.
4. Copy `docs\windows-runs\fixtures\photopea-red.png` from the repo into the work folder as `red.png`.
5. Make the folder `%USERPROFILE%\Documents\webwrap-run-<timestamp>`, which the vscode.dev checks use. It must not be under `%TEMP%`, for the reason given in check K. Create these files there, as UTF-8 without BOM, each ending in CRLF:
   - `note-one.md` containing `first`;
   - `note-two.md` containing `second`;
   - `note-three.md` containing `third`.

## 2. Checks

Close every Excalidraw, Photopea, Twitter, and VS Code app window before starting.

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
  - The image opens in Photopea as a new document named `red`.
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

Close every app before the Photopea checks.

**D3: Photopea**

Use a real PSD of your own, preferably a large one, copied into the Documents folder from step 5 as `mine.psd`. Record its size. If you have none, use `red.png` copied there as `mine.png`, and read `.png` for `.psd` below.

- **L. A file given at launch opens.** Ask me to drag `mine.psd` onto `photopea-<arch>.exe`. Expected:
  - Photopea opens it as a document named `mine`;
  - an "Opened mine.psd" notice shows at the bottom right.

  Record how many seconds it took.
- **M. Ctrl+S from the keyboard saves the original.**
  1. Ask me to make a small visible change, then press Ctrl+S with the keyboard.
  2. Expected: "Saving mine.psd…", then "Saved mine.psd".
  3. Check that `mine.psd`'s modified time changed, and that `%USERPROFILE%\Downloads\mine.psd` does not exist.
  4. Ask me to reopen it (drag it onto the exe again) and confirm the change is there.
  5. If Ctrl+S does nothing, ask me to use File > Save, and record that the keyboard did not work.
- **N. Ctrl+O opens through the Windows dialog, and saves back.**
  1. Ask me to press Ctrl+O, or use File > Open... if Ctrl+O does nothing, and record which worked.
  2. Expected: a Windows Open dialog listing Photoshop documents.
  3. Ask me to switch the file type to "All files", then pick `red.png` from the work folder. Expected: it opens as a new document.
  4. Ask me to change it and save. Expected: "Saved red.png", and `red.png` changed.
- **O. A document made in Photopea downloads.** Ask me to use File > New, draw something, and press Ctrl+S. Expected:
  - a download of `Untitled-1.psd` (or similar) to Downloads;
  - a "Downloaded" notice.

  Delete the download afterwards.

Close every app before the generic-site checks.

**D4: generic sites**

These use the Documents folder from step 5. Make `scene.excalidraw` there, as UTF-8 without BOM, containing exactly:

`{"type":"excalidraw","version":2,"source":"webwrap-pc","elements":[{"id":"pc-box","type":"rectangle","x":100,"y":100,"width":200,"height":120}],"appState":{},"files":{}}`

- **P. Excalidraw opens a file given at launch (tier 1).** Ask me to drag `scene.excalidraw` onto `excalidraw-<arch>.exe`. Expected:
  - Excalidraw opens showing one rectangle;
  - no message box, and no notice.
- **Q. Ctrl+S in Excalidraw saves the original.** Ask me to move the rectangle, then press Ctrl+S. Expected:
  - `scene.excalidraw` changed (check its modified time and that it now contains `"pc-box"` and more than one line);
  - no `scene.excalidraw` in `%USERPROFILE%\Downloads`;
  - record any prompt that appeared, word for word.
- **R. Excalidraw's own Open, then save (tier 2).** CI could open WebView2's picker but not write through it.
  1. Copy `scene.excalidraw` to `picked.excalidraw` in the same folder.
  2. Ask me to press Ctrl+O in Excalidraw, or use its menu's Open, and pick `picked.excalidraw` in the Windows dialog. Expected: it opens.
  3. Ask me to move the rectangle and press Ctrl+S.
  4. Record word for word any permission prompt (such as "Let site edit…?"), and what I chose. Expected after allowing: `picked.excalidraw` changed, and no download.
- **S. The open-only notice (tier 3 or 4).**
  1. Launch `twitter-<arch>.exe` and wait for the page to load. Signing in is not needed.
  2. Ask me to drag `red.png` from the work folder onto `twitter-<arch>.exe` in Explorer.
  3. Wait 15 s. Ask me to read out word for word any message box, then dismiss it. Expected, either:
     - a notice that Twitter opened `red.png` as an upload, that saving downloads a copy, and that the notice won't show again; or
     - a box saying Twitter did not accept the file.
  4. Ask me to drag `red.png` onto the exe again. Expected: no notice the second time (a "did not accept" box may repeat).

Close every app afterwards. Delete the Documents folder from step 5.

## 3. Extra results to record

- The CI run id and URL you used.
- For G, the seconds until the file opened.
- For J, whether Ctrl+O itself opened the dialog, and any permission prompt.
- For L, the file's size and the seconds until it opened.
- For M and N, whether the keyboard shortcuts worked or the menu was needed.
- For Q and R, any permission prompt, word for word.
- For S, which box appeared.
