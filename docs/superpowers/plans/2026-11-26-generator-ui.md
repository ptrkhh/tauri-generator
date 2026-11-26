# Generator UI plan (sub-project F)

Spec: [`../specs/2026-11-25-generator-ui-design.md`](../specs/2026-11-25-generator-ui-design.md). One phase per branch, merged after CI.

## F1 discovery: done 2026-11-26, branch `feat/generator-discovery`

- **`crates/webwrap-discover`.**
  - `client(extra_root)` builds the one client: no automatic redirects, a 10 s timeout, https only, and the platform verifier.
  - `fetch` follows redirects by hand and tracks the unbroken run of 301/308s. It enforces https, at most 5 redirects, the content type and the size cap.
  - `discover` gives the wrapped URL, app id, the slug of the typed host, the name, the adapter, the association rows (`with_presets`), and up to 12 fetched candidates. If the page cannot be read, it reports a `problem` instead of failing. It fails only for a URL that is not https or is unusable.
- **`webwrap_spec::check_assoc`** is now public.
- **Tests on Linux.** `tests/discover.rs` runs a rustls server with the fixture CA (`tests/fixtures`, valid for 100 years; the CA key was not kept). It covers:
  - name order and title trimming;
  - icon order across sources and by size, maskable icons last, SVG skipped, duplicates dropped;
  - `file_handlers` lowercasing and checking;
  - only permanent redirects moving the wrapped URL;
  - an http redirect, the wrong page type, an oversize page, a 500 and a redirect loop, each giving `problem`;
  - wrong-type and oversize icons dropped.

  Unit tests cover presets, adapters and sizes.
- **Live test:** `cargo test -p webwrap-discover -- --ignored --nocapture`, which also passes under Wine:
  - `twitter.com` gives `https://x.com/` "X" (7 icons);
  - `photopea.com` gives `https://www.photopea.com/` "Photopea", 33 rows, only `.psd` ticked;
  - `vscode.dev` gives "Code" (4 icons, through the manifest hint), with only `.md` ticked.
- **Under Wine** the local-server tests fail when building the client: E_INVALIDARG from `CertCreateCertificateChainEngine`. With extra roots, the verifier sets `hExclusiveRoot`, which Wine's `crypt32` rejects. Windows has supported it since 7, and CI runs these tests natively.

## F2-F4: done 2026-11-26, branch `feat/generator-app`

All three phases are as planned below, and CI passes on both architectures. Two things CI taught us:

- **The folder picker's "Folder:" box is control 1152,** not the Open dialog's 1148. `pickInDialog` now takes either.
- **`status` collides with `window.status`** in a classic script, so the page uses `say`.

## F2 generator app

- Create `crates/webwrap-generator`, a Tauri app with binary `tauri_generator`. It embeds both templates the way `webwrap-generate/build.rs` does.
- Add the commands `discover`, `pick_folder`, `generate` and `show_in_folder`. `discover` and `generate` each run on a `std::thread` and return through a channel. Async commands use `tauri::async_runtime::spawn_blocking`, because blocking reqwest panics inside an async task.
- Create the window: data in `%LOCALAPPDATA%\WebWrap\generator`, and the `WEBWRAP_TEST_BROWSER_ARGUMENTS` hook. `on_navigation` allows only the bundled origin, and new windows are refused.
- In CI, build both generators and add them to `acceptance-<triple>`.

## F3 UI

- Write `ui/index.html`, `ui/app.js` (`// @ts-check`) and `ui/style.css`, with the CSP from the spec.
- Add `e2e/generator-page.mjs`: headless Chrome or Edge through `e2e/cdp.mjs`, with the commands stubbed.
- Add `tsc --noEmit` in CI.

## F4 proof

- Add `e2e/generator.mjs` on both runners. It imports the test certificate into `Cert:\LocalMachine\Root` first.
- Queue PC checks W-Y in `docs/windows-runs/NEXT.md`.
