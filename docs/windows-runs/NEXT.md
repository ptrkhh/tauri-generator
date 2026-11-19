# Next Windows run: sub-project B hands-on acceptance

Branch `feat/webwrap-runtime`. This is Task 9 Step 2 of `docs/superpowers/plans/2026-09-16-webwrap-spec-and-minimal-runtime.md`: what automated tests cannot see. CI already ran every automated test on x64 and ARM64 Windows. Nothing here needs a compiler.

## 1. Get the apps

1. `gh run list --repo ptrkhh/tauri-generator --branch feat/webwrap-runtime --workflow windows --status success --limit 5 --json databaseId,headSha,url`. Use the run whose `headSha` equals `git rev-parse HEAD`. If none does, stop and report "CI has not passed for this commit yet".
2. Make a new empty folder `%TEMP%\webwrap-run-<timestamp>` and download both artifacts into it:
   `gh run download <databaseId> --repo ptrkhh/tauri-generator -n acceptance-x86_64-pc-windows-msvc -D <folder>\x64`
   `gh run download <databaseId> --repo ptrkhh/tauri-generator -n acceptance-aarch64-pc-windows-msvc -D <folder>\arm64`
   If `gh` is missing or not logged in, give me the run `url` and ask me to download the two artifact zips and extract them into those two folders.
   Each folder must contain `Photopea.exe`, `Twitter.exe`, and `unspecialized-template.exe`.
3. The native folder is `arm64` if `$env:PROCESSOR_ARCHITECTURE` is `ARM64`, otherwise `x64`. The full check below uses the native folder. The other folder gets the short check only if it can run here: x64 apps run on ARM64 Windows under emulation; ARM64 apps do not run on x64 Windows (record that as SKIPPED).

Before launching anything, record whether these exist and their LastWriteTime: `%LOCALAPPDATA%\WebWrap\873756fbe97a1384`, `%LOCALAPPDATA%\WebWrap\2fa5e3f40150278f`, `%LOCALAPPDATA%\dev.webwrap.runtime`.

## 2. Full check (native folder)

- **A. Photopea opens.** Launch `Photopea.exe` with no arguments. Ask me to confirm each separately: the window title is exactly `Photopea`; the Photopea editor finishes loading (a blank white window is a FAIL, so say so explicitly); the title-bar icon and the taskbar button both show a **solid red square**, not a blank, transparent, or generic program icon.
- **B. Its storage.** `%LOCALAPPDATA%\WebWrap\873756fbe97a1384\EBWebView` exists.
- **C. Twitter alongside it.** Leave Photopea open and launch `Twitter.exe`. Ask me to confirm: title `Twitter`; the X/Twitter page loads (a login page is fine); icon is a **solid blue square**. Then check `%LOCALAPPDATA%\WebWrap\2fa5e3f40150278f\EBWebView` exists and that `%LOCALAPPDATA%\dev.webwrap.runtime\EBWebView` was not created or modified by this run (compare with what you recorded).
- **D. Pop-ups go to the browser.** Ask me to click a link on the Twitter page that opens in a new tab on the web, such as Terms of Service or Privacy Policy. Expected: it opens in my default browser and no new app window appears. SKIPPED if I can't find such a link.
- **E. Separate sign-in.** Ask whether I want to sign in to X in the Twitter app. If yes: after signing in, close and reopen Twitter (still signed in = PASS for persistence), and confirm Photopea shows no sign of it. If no, SKIPPED. Never ask for or type credentials yourself.
- **F. Unspecialized template fails visibly.** Close both apps. Run `Start-Process <native>\unspecialized-template.exe -Wait -PassThru` and ask me to confirm a message box titled `WebWrap` saying the app was never specialized. After I dismiss it, record the exit code (expected `1`).
- **G. Self-report.** `Start-Process <native>\Photopea.exe -ArgumentList '--inspect-self' -Wait -NoNewWindow -PassThru -RedirectStandardOutput <folder>\inspect.json`, then record the exit code (expected `0`) and the JSON verbatim. `runtimeArch` must match the folder's architecture.

## 3. Short check (other folder, if it runs here)

Repeat A, F, and G with that folder's executables. Close every app afterwards.

## 4. Extra results to record

The CI run id and URL you used, and a final `Get-ChildItem %LOCALAPPDATA%\WebWrap` listing. Leave those folders in place.
