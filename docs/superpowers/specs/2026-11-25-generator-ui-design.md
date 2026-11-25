# Generator UI and Discovery Design (sub-project F)

Date: 2026-11-25. Status: draft, awaiting user approval. Plan: not yet written.

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
| Replacing | Pack's rules are unchanged: it replaces this app or another generated app, and refuses anything else. The result card says which happened for each file ("updated", or "replaced the app for <other site>") | C; user, 2026-11-20 |

## Discovery

A new plain-Rust crate, **`crates/webwrap-discover`**, so it is tested on Linux. `discover(url) -> Discovery` works like this:

1. **Canonicalize** with `webwrap_spec::canonicalize_url`. A non-HTTPS URL is refused here (gate 14).
2. **Fetch the page.** The final URL after redirects is the one that is wrapped. If a redirect leaves the origin, the user sees that before generating, because the final URL replaces the typed one in the form.
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

   The first 12 are fetched, each with its source label.
5. **File associations from the manifest.** These are the manifest's `file_handlers[].accept` entries: MIME type mapped to extensions. Only extensions that pass the schema's `check_assoc` are offered, so `.PSD` becomes `.psd` and anything else is dropped.
6. **Presets**, added by adapter:
   - Photopea: `.psd` ticked.
   - vscode.dev: `.md` ticked. `.txt`, `.json`, `.js`, `.ts`, `.css`, `.html`, `.py`, `.yaml` and `.xml` are offered unticked.

The HTML is parsed with `dom_query`, which is already in the tree through `tauri-utils`. The HTTP client is `reqwest` with only `blocking` and `rustls-tls`, a new direct dependency.

## The generator app

A new crate, **`crates/webwrap-generator`**: a Tauri app whose binary is `tauri_generator`.

- **Embedded templates.** It embeds both templates the way `webwrap-generate`'s `build.rs` does.
- **Tauri commands:**
  - `discover(url)`. The candidates come back as bytes plus a label; the page makes them into `blob:` URLs.
  - `pick_folder()` returns a path, or null on cancel.
  - `generate(input, icon_png, out_dir)` runs `webwrap_pack::generate` on a worker thread and returns the outputs and how each was replaced.
  - `show_in_folder(path)` runs `explorer /select,<path>`.
- **Window.**
  - A URL field and a **Find** button.
  - Then a preview: the name (editable), the slug (editable, derived by pack), the adapter (shown), and the icon gallery. A tile is selected, and the last tiles are **Initials** and **Upload…**.
  - Then file associations (checkbox rows, plus an "add" row taking extension, MIME type and description) and **Generate…**, which asks for the folder.
  - Then the result card.
  - Discovery failing leaves the form usable with the initials icon and the host as the name (base design, "Metadata failure").
- **Its own WebView2 data** lives under `%LOCALAPPDATA%\WebWrap\generator`, which is not an app id.
- **Navigation.** The generator's window never leaves its bundled page: it uses the runtime's `nav_verdict` policy, but with only `tauri.localhost` allowed.

`webwrap-generate`, the CLI, stays as it is: CI and the tests use it.

## Phases

| Phase | Content |
| --- | --- |
| **F1** Discovery | `webwrap-discover`: fetch limits, name, candidates, `file_handlers`, presets. Linux tests against a local HTTPS server with fixture pages: limits, redirects off HTTPS, an oversize body, a wrong content type, SVG skipped, name order, title trimming, and the `file_handlers` mapping |
| **F2** Generator app | `webwrap-generator` with the four commands, the embedded templates, and the folder picker. CI builds and uploads both generators in `acceptance-<triple>` |
| **F3** UI | `index.html`, `app.js`, `style.css`: form, gallery, canvas conversion, initials, upload, association editor, result card. Tested in Chromium (Playwright, on Linux) with the Tauri commands stubbed: conversion of PNG, ICO, JPEG and WebP fixtures to a 256x256 PNG; the initials icon; and a rejected, undecodable candidate dropped from the gallery |
| **F4** Proof | **CI e2e `e2e/generator.mjs`:** <br>1. launch the native generator with a DevTools port; <br>2. drive the form against the local HTTPS test server; <br>3. pick the folder with `pickInDialog`; <br>4. generate; <br>5. assert both outputs with `webwrap-generate inspect` (name, slug, associations, icon present). <br>**PC checks W-Y** |

**PC checks**, the next letters after V:

- **W:** Generate Twitter, Photopea and vscode.dev from the ARM64 generator. The gallery shows real icons, the names are sensible, and the presets are ticked.
- **X:** Each generated app runs with the chosen icon on its window and taskbar.
- **Y:** Regenerating Twitter into the same folder says "updated". A different site given the name Twitter says "replaced the app for twitter.com".

## Release gates

F completes gate 1's Windows side end to end: a generator a person uses, not only the CLI. Gate 1 as a whole still needs the phase-2 APK. F closes no other gate. Gate 14 (generator rejects non-HTTPS) is reasserted through discovery.

## Open items for later

- **Phase 2:** the DNS field (System / Custom DoH, validated before packaging) and the Android card.
- **SVG:** the canvas path would render SVG with no new dependency, because an SVG in `<img>` runs no script and loads nothing external. It was skipped by choice and can be revisited.
- **A list of apps already generated in a folder:** not asked for.
