# Next Windows run: E's Open with and double-click checks, and the rest of D1-D4's

Branch `main`. Plans:

- `docs/superpowers/plans/2026-11-20-file-bridge-runtime-plumbing.md` (D1, merged);
- `docs/superpowers/plans/2026-11-20-vscode-dev-adapter.md` (D2, merged);
- `docs/superpowers/plans/2026-11-20-photopea-adapter.md` (D3, merged);
- `docs/superpowers/plans/2026-11-21-generic-sites.md` (D4, merged);
- `docs/superpowers/plans/2026-11-24-association-registration.md` (E, merged).

CI already drove all four end to end on x64 and ARM64, including live vscode.dev, live Photopea with a 150 MB PSD, and live Excalidraw. The 2026-11-21 run (`2026-11-21-results.md`) passed A-E, G-I and K, which are not repeated. This run does E's checks T-V first, then redoes F with the right link, retests J after its fix, and does L-S for the first time. CI already checked E's registry writes and the command they store; T-V check what only Explorer and a person can. It checks only what needs a person: windows coming to the front, message boxes, dragging with the mouse, the keyboard, and the Windows Open dialog. Nothing here needs a compiler.

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
5. Make the folder `%USERPROFILE%\Documents\webwrap-run-<timestamp>`, which the vscode.dev checks use. It must not be under `%TEMP%`: vscode.dev cannot use files in folders Windows keeps for apps. Create `note-three.md` there, as UTF-8 without BOM, containing `third` and CRLF, and `double.md` the same way containing `double`. Also put a PSD there as `double.psd`: a copy of your own, or, if you have none, one made in Photopea at photopea.com in a browser (File > New, then File > Save as PSD) and moved there from Downloads.

## 2. Checks

Close every Excalidraw, Photopea, Twitter, and VS Code app window before starting.

**E: Open with and double-click.** Do these first: the Photopea and VS Code apps offer to register on their first launch. In PowerShell, `& <exe> --inspect-self | ConvertFrom-Json` waits for and reads the app's report.

- **T. The first-launch offer.**
  1. Launch `photopea-<arch>.exe`. Expected: before the window, a box asking to add Photopea to "Open with" for .psd files, saying it only affects my account and changes no default app. Ask me to read it out word for word, then answer **No**. Expected: Photopea opens.
  2. Close it and launch it again. Expected: no box. Check that `%LOCALAPPDATA%\WebWrap\873756fbe97a1384\assoc-declined` exists.
  3. Close it, delete that file, and launch it again. Expected: the box again. Ask me to answer **Yes**. Then check `(& <exe> --inspect-self | ConvertFrom-Json).associations.registeredHere` is `True`.
  4. Close it. Launch `vscode-<arch>.exe`. Expected: the same box for .md files. Ask me to answer **Yes**, and check `registeredHere` the same way. Close it.
- **U. Open with lists the apps.** Ask me to right-click `double.psd` in Explorer and open **Open with** (on Windows 11, also **Choose another app** if it is not listed directly). Expected: **Photopea**, with its red icon. The same for `double.md`: **VS Code**. Record where each appeared.
- **V. Double-click opens the file, once Windows has been told to.** E never sets a default; I do, through Windows.
  1. Ask me to double-click `double.psd`. Expected: Windows' own "How do you want to open this file?" (or the current default app). Ask me to pick **Photopea** with **Always** (if the default app opened instead, use Open with > Choose another app > Photopea > Always). Expected: the Photopea app opens `double.psd`.
  2. Ask me to make a small change and press Ctrl+S. Expected: `double.psd`'s modified time changes. Close the app.
  3. Ask me to double-click `double.psd` again. Expected: the Photopea app opens it directly, with no prompt.
  4. For `double.md`, ask me to use Open with > Choose another app > **VS Code** > Always. Expected: the VS Code app opens it showing `double`. Ask me to change it to `doubled` and press Ctrl+S, allowing any prompt. Check with `Get-Content -Raw`: `doubled` plus CRLF. Close it, double-click `double.md`, and expect it to open directly.
  5. Moving the app re-offers. Copy `photopea-<arch>.exe` into a new folder `<work folder>\moved` and launch the copy. Expected: the offer box again (the registration points at the original). Ask me to answer **No**, then close it.
  6. Clean up: run `& <exe> --unregister-associations | Out-String` on the original `photopea-<arch>.exe` and `vscode-<arch>.exe`, and record the output. Then ask me to reset the .psd and .md defaults in Settings > Apps > Default apps if I want my previous ones back.

Close every app before the D1 checks.

**D1: window behaviour**

- **F. Pop-ups still go to the browser.** Launch `twitter-<arch>.exe`. Ask me to click **Terms** in the page's footer, which opens in a new tab on the web. Expected: it opens in my default browser, and no new app window appears.
  - Not the inline "Terms of Service" link above it: that one has no `target`, so it navigates the app's own window, as it would a browser tab (checked in Chromium on 2026-11-24). That is what the 2026-11-21 run clicked.

Close every app before the vscode.dev checks.

**D2: vscode.dev**

- **J. Ctrl+O uses the Windows Open dialog, and saves back.** On 2026-11-21 Ctrl+O opened vscode.dev's folder picker, because vscode.dev binds Ctrl+O to Open Folder as well as Open File. The adapter now runs Open File for Ctrl+O.
  1. Launch `vscode-<arch>.exe` and wait for vscode.dev to load.
  2. Ask me to press Ctrl+O. Expected: a Windows Open dialog for a **file**, not "Select a folder this site can view".
  3. Ask me to pick `note-three.md`. Expected: it opens in a tab showing `third`.
  4. Ask me to change the text to `picked`, then press Ctrl+S, and allow any permission prompt.
  5. Check with `Get-Content -Raw`. Expected: `picked` plus CRLF, and no `%USERPROFILE%\Downloads\note-three.md`.
  6. Record any permission prompt word for word.

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
- For T, the offer box's text, word for word.
- For U, where each app appeared in Open with.
- For V, the prompt Windows showed on the first double-click, and the unregister output.
- For J, which dialog Ctrl+O opened, and any permission prompt.
- For L, the file's size and the seconds until it opened.
- For M and N, whether the keyboard shortcuts worked or the menu was needed.
- For Q and R, any permission prompt, word for word.
- For S, which box appeared.
