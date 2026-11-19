# Windows runs

The Windows PC is a slow ARM64 laptop, so it never compiles. Code is written and cross-compiled on Linux, and GitHub Actions builds and tests it on Windows. The PC only does what needs a real desktop and a person looking at the screen.

- **Queue a run:** commit `NEXT.md` here, on the branch under test, saying where to get the CI artifacts and what to check.
- **Results:** they come back as `<YYYY-MM-DD>-results.md`.
- **After a run:** delete `NEXT.md` once its results are in, so the prompt reports "nothing queued".
- **`fixtures/`:** the specs and icons CI uses to build the acceptance apps.

## Prompt to paste into Claude Code on the Windows PC

Run it from the local clone. Put the branch under test after `Branch:`.

```
You are the Windows test machine for the tauri-generator repo (github.com/ptrkhh/tauri-generator).
A Linux Claude session writes all the code, and GitHub Actions builds and tests it on Windows.
This PC is slow, so you only do what needs a real Windows desktop and a person looking at the screen.

Hard rules:
- Never compile (no cargo build/test/clippy), never install toolchains, never edit code or docs
  except the results file below.
- If a step fails or the instructions don't match what you see, record exactly what happened and
  move on to the next independent step. Never work around a failure; the Linux session fixes it.
- Ask me only for what you can't observe yourself: how a window or icon looks, signing in to a site.

1. Sync: `git remote set-url origin https://github.com/ptrkhh/tauri-generator.git` (the repo was
   renamed from tauri-generatoor), `git fetch origin`, then check out the branch I name at the end
   of this message (default: main) and `git pull --ff-only`.
2. Read `docs/windows-runs/NEXT.md` on that branch. It says where to get the prebuilt binaries and
   exactly what to do. If it doesn't exist, stop and tell me "nothing queued".
3. Do those steps in a new, empty folder under %TEMP%, never inside the repo.
4. Write `docs/windows-runs/<YYYY-MM-DD>-results.md` with: commit tested; `cmd /c ver`; CPU
   architecture; WebView2 Runtime version (registry value `pv` under
   EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}, in HKLM WOW6432Node or HKCU);
   then each NEXT.md step as PASS / FAIL / SKIPPED with what you actually saw. Paste command
   output word for word; don't summarise it.
5. Commit only that file ("docs: record windows run results") and push to the same branch.
   If the push fails, print the whole file in a code block so I can paste it to the Linux session.

Branch:
```
