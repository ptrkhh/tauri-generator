# Generator UI and Discovery Design (sub-project F)

Date: 2026-11-25. Status: approved by the user 2026-11-26. Plan: [`2026-11-26-generator-ui.md`](../plans/2026-11-26-generator-ui.md).

## Summary

F is `tauri_generator.exe`, the local window that turns a website into its two Windows apps. You type a website. It finds the site's name and icons and suggests file associations. You adjust them, pick an output folder, and it writes `<slug>-x64.exe` and `<slug>-arm64.exe` through `webwrap_pack::generate`, the same library the CI generator uses.

F is Windows-only for now. APK output and custom DNS are phase 2, and the runtime has no DNS support, so F shows neither the Android card nor the DNS field. Each lands together with the phase-2 work that makes it real.

## Decisions

| Question | Decision | Source |
| --- | --- | --- |
| Scope | Windows outputs only. No DNS field and no Android card until phase 2 | HANDOVER (phase 2); runtime has no DNS |
| UI language | Plain HTML, CSS and JavaScript with `// @ts-check`, type-checked by `tsc --noEmit` in CI. No build step, no framework | User, 2026-11-25 (replaces "TypeScript" in the base design) |
| Builds | `tauri_generator-x64.exe` and `tauri_generator-arm64.exe`, each embedding both runtime templates, so either one generates both app files | User, 2026-11-25 |
| SVG icons | Skipped. A site with only SVG icons gets the other candidates, the initials icon, or an upload | User, 2026-11-25 |
| Registration | The generator does nothing beyond E's first-launch offer | User, 2026-11-25 |
| Where fetching runs | In Rust: the page's own fetch would be blocked by CORS. The host is HTTPS only, with at most 5 redirects, all HTTPS. There is a 10 s timeout per request, 2 MB for HTML or a manifest, and 2 MB per image. At most 12 images are fetched, and only for content types the generator can decode | Base design ("content-type, size, redirect, and timeout limits"); gate 14 |
| Where image decoding runs | In the generator's own WebView2. Rust hands over the fetched bytes as a `blob:` URL, an `<img>` decodes them, and a `<canvas>` letterboxes the image to a transparent 256x256 square and exports PNG. Rust then runs pack's `check_icon` on the result as before. Chromium's decoders handle PNG, ICO, JPEG and WebP with no new dependency. The canvas is never tainted, because the bytes are local | This design |
| Initials icon | Drawn on the same canvas: one or two letters on a colour derived from the app id. Used when no candidate decodes | Base design ("initials-based fallback") |
| Upload | A file input reads a local image, which takes the same canvas path | Base design |
| Output folder | Asked every time with the Windows folder picker: `IFileOpenDialog` with `FOS_PICKFOLDERS`, the same COM dialog the runtime uses for Photopea's Ctrl+O. It opens at the last folder used, remembered in the generator's data folder | HANDOVER ("asks for the output folder every time") |
| Adapter | Chosen from the canonical host and shown, not edited: `www.photopea.com` and `photopea.com` get `photopea`, `vscode.dev` gets `vscodeDev`, and everything else gets `none` | D (adapters exist only for these) |
| Replacing | Pack's rules and API are unchanged: it replaces this app or another generated app with the same file name, and refuses anything else. The result card says which happened for each file: "updated" (`sameApp`) or "replaced another generated app" (`otherApp`; pack does not say which site that was) | C; user, 2026-11-20 |

## Discovery

A new plain-Rust crate, **`crates/webwrap-discover`**, so it is tested on Linux. `discover(url) -> Discovery` works like this:

1. **Canonicalize** with `webwrap_spec::canonicalize_url`. A non-HTTPS URL is refused here (gate 14).
2. **Fetch the page.** Metadata is read from the page at the end of the redirects. The wrapped URL follows only the unbroken run of permanent redirects (301, 308) from the start. The first temporary redirect (302, 303, 307) ends that run, so a login or locale bounce is never wrapped. This matters because identity, the data folder and the registration are all keyed on the wrapped URL (E: "F must keep URL canonicalization stable"). `photopea.com` becomes `https://www.photopea.com/`, the origin Photopea's shell frames. `twitter.com` becomes `https://x.com/`. The wrapped URL replaces the typed one in the form, so the user sees it before generating. The default slug comes from the host the user typed, so `twitter.com` still gives `twitter-*.exe`.
3. **Name**, first found wins:
   1. the manifest's `short_name`;
   2. the manifest's `name`;
   3. `<meta name="application-name">`;
   4. `<title>`, cut at the first ` | `, ` - ` or ` – `;
   5. the host.
4. **Icon candidates**, in this order:
   1. manifest `icons`, then `<link rel="apple-touch-icon">`, then `<link rel="icon">` and `shortcut icon`, then `/favicon.ico`;
   2. duplicates by URL are dropped;
   3. `image/svg+xml`, and anything ending `.svg`, is skipped;
   4. the order within a source puts declared square sizes first, largest first.

   The first 12 are fetched, each with its source label. A manifest's icon `src` resolves against the manifest's URL, and a link's `href` against the final page URL.
   - **Maskable icons go last.** Manifest icons whose `purpose` lacks `any` are designed to be cropped, so they look padded as desktop icons and sort after the others.
   - **vscode.dev links its manifest from a script.** That script adds `<link rel="manifest">`, `apple-touch-icon` and `icon` at run time. So for an adapter site whose page links no manifest, discovery uses the adapter's known manifest path, `/static/stable/code.webmanifest`, which gives "Code" and its PNG icons. The weekly live run notices if that path moves (added in F1 after the live check, 2026-11-26).
5. **File associations from the manifest.** These are the manifest's `file_handlers[].accept` entries, a MIME type mapped to extensions, one row per extension. Discovery lowercases each extension and drops any that the schema's `check_assoc` rejects. That function was made public for this in F1 rather than duplicated. The description is the handler's `name`, or else `<EXT> file`.
6. **Presets**, added by adapter. They are merged by extension, and a preset replaces a manifest row for the same extension, because the schema refuses an extension claimed twice:
   - Photopea: `.psd` ticked.
   - vscode.dev: `.md` ticked. `.txt`, `.json`, `.js`, `.ts`, `.css`, `.html`, `.py`, `.yaml` and `.xml` are offered unticked.

   Manifest rows are ticked for a site with no adapter (base design: such sites receive the associations they declare). For an adapter site they are offered unticked, so Photopea's manifest, which declares more than 30 types including `.jpg`, `.png` and `.pdf`, still preselects only `.psd`.

The HTML is parsed with `dom_query` 0.27, which is already in the tree through `tauri-utils`. The HTTP client is `reqwest` 0.13, already in the tree through `tauri`, as a direct dependency with `default-features = false` and `blocking` and `rustls`. That `rustls` uses the platform verifier, which is the Windows certificate store on Windows. Version 0.13 has no `rustls-tls` feature. On 2026-11-26 this set built on Linux, cross-compiled to `aarch64-pc-windows-msvc` with `cargo xwin`, and fetched a live page.

`webwrap_discover::client(extra_root)` builds the one client discovery uses, carrying every limit above. The generator passes no extra root. The F1 tests pass their test certificate, so they exercise the real limits rather than a client of their own.

## The generator app

A new crate, **`crates/webwrap-generator`**: a Tauri app whose binary is `tauri_generator`.

- **Embedded templates.** It embeds both templates the way `webwrap-generate`'s `build.rs` does.
- **Tauri commands:**
  - `discover(url)` returns:
    - the wrapped URL, its app id (for the initials colour), the default slug, the name and the adapter;
    - the associations, each with its tick;
    - the candidates, as bytes plus a label. The page makes them into `blob:` URLs.
  - `pick_folder()` returns a path, or null on cancel.
  - `generate(input, icon_png, out_dir)` runs `webwrap_pack::generate` and returns the outputs and how each was replaced.
  - `discover` and `generate` run on a worker thread, never the main thread or an async task. Tauri runs a non-async command on the main thread, which would freeze the window for up to 13 timeouts. `reqwest::blocking` panics inside an async runtime.
  - `show_in_folder(path)` runs `explorer /select,<path>`.
- **Window.**
  - A URL field and a **Find** button.
  - Then a preview: the name (editable), the slug (editable; pack's `derive_slug` of the typed host, always passed to `generate`, because pack would otherwise derive it from the wrapped URL), the adapter (shown), and the icon gallery. A tile is selected, and the last tiles are **Initials** and **Upload…**.
  - Then file associations (checkbox rows, plus an "add" row taking extension, MIME type and description) and **Generate…**, which asks for the folder.
  - Then the result card.
  - Discovery failing leaves the form usable with the initials icon and the host as the name (base design, "Metadata failure").
- **Its own WebView2 data** lives under `%LOCALAPPDATA%\WebWrap\generator`, which is not an app id.
- **Navigation and untrusted text.** The generator's window never leaves its bundled page. Its `on_navigation` allows only the bundled origin, and new-window requests are refused. `nav_verdict` cannot do this: it allows every `https` top-level navigation. The page holds commands that write executables, so everything a fetched site supplies is set only through `textContent` or `value`: the name, the title, the labels and the association descriptions. The CSP is `default-src 'self'; img-src 'self' blob: data:`.
- **Test hook.** Like the runtime, the generator appends `WEBWRAP_TEST_BROWSER_ARGUMENTS` to its WebView2 arguments, so `e2e/apps.mjs` `launch` can give it a DevTools port.

`webwrap-generate`, the CLI, stays as it is: CI and the tests use it.

## Phases

| Phase | Content |
| --- | --- |
| **F1** Discovery | `webwrap-discover`: fetch limits, name, candidates, `file_handlers`, presets. Linux tests against a local HTTPS server with fixture pages, its certificate given to `client`. They cover: <br>- limits, redirects off HTTPS, an oversize body and a wrong content type; <br>- a permanent then a temporary redirect, where only the first is wrapped; <br>- SVG skipped, name order and title trimming; <br>- the `file_handlers` mapping: lowercasing, a preset replacing a manifest row, and ticks with and without an adapter |
| **F2** Generator app | `webwrap-generator` with the four commands, the embedded templates, and the folder picker. CI builds and uploads both generators in `acceptance-<triple>` |
| **F3** UI | `index.html`, `app.js`, `style.css`: form, gallery, canvas conversion, initials, upload, association editor, result card. Tested by `e2e/generator-page.mjs` with the Tauri commands stubbed. It runs headless Chrome or Edge, driven through `e2e/cdp.mjs` `attach`, so it adds no dependency and runs both locally and in the Windows CI job. It covers: <br>- conversion of PNG, ICO, JPEG and WebP fixtures to a 256x256 PNG; <br>- the initials icon; <br>- a rejected, undecodable candidate dropped from the gallery; <br>- a name containing markup shown as text |
| **F4** Proof | **CI e2e `e2e/generator.mjs`:** <br>1. launch the native generator with a DevTools port; <br>2. drive the form against the local HTTPS test server. Its certificate is first imported into `Cert:\LocalMachine\Root`, because discovery trusts the Windows store, not WebView2's `--ignore-certificate-errors`; <br>3. pick the folder with `pickInDialog`; <br>4. generate; <br>5. assert both outputs with `webwrap-generate inspect` (name, slug, associations, icon present). <br>**PC checks W-Y** |

**PC checks**, the next letters after V:

- **W:** Generate `twitter.com`, `photopea.com` and `vscode.dev` from the ARM64 generator.
  - The form shows `https://x.com/` and `https://www.photopea.com/`, and the files are `twitter-*.exe` and `photopea-*.exe`.
  - The gallery shows real icons, the names are sensible, and only the presets are ticked.
- **X:** Each generated app runs with the chosen icon on its window and taskbar.
- **Y:** Regenerating `twitter.com` into the same folder says "updated". A different site given the file name `twitter` says "replaced another generated app".

## Release gates

F completes gate 1's Windows side end to end: a generator a person uses, not only the CLI. Gate 1 as a whole still needs the phase-2 APK. F closes no other gate. Gate 14 (generator rejects non-HTTPS) is reasserted through discovery.

## Accepted risks

- **Content type.** An icon served with a content type outside the decodable list, such as `application/octet-stream`, is dropped, and the other candidates or the initials icon remain.
- **Candidate bytes.** They cross the IPC as JSON number arrays, up to 12 x 2 MB in the worst case. Real icons are kilobytes. If this is ever slow, a per-candidate `tauri::ipc::Response` is the fix.
- **One site under two names.** Typing `twitter.com` and then `x.com` gives one app, with one identity and one data folder, under two file names. Each file works. Re-registering from the other file re-offers the association.
- **Small favicons.** One upscaled to 256x256 looks soft. The gallery shows it, and the user chooses.
- ~~**The `rustls` crypto provider (`aws-lc-sys`)** had not been built natively on `windows-11-arm`.~~ Retired: F1's CI run built it and passed the discovery tests natively on both runners (2026-11-26).

## Open items for later

- **Phase 2:** the DNS field (System / Custom DoH, validated before packaging) and the Android card.
- **SVG:** the canvas path would render SVG with no new dependency, because an SVG in `<img>` runs no script and loads nothing external. It was skipped by choice and can be revisited.
- **A list of apps already generated in a folder:** not asked for.
