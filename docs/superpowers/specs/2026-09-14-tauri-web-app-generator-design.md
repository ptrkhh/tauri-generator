# Tauri Web App Generator Design

**Status:** Approved

**Date:** 2026-09-14

**Initial platforms:** Windows and Android

## Summary

Build a local Windows desktop tool, `tauri_generator.exe`, that turns a website URL into two independently installable, single-file artifacts:

- `twitter.exe`
- `twitter.apk`

Each output embeds its own URL, identity, icon, file associations, adapter selection, and DNS configuration. Generating an app requires no hosted service, account, or user-facing CLI. A small Android packaging kit is downloaded once and cached locally.

The generated applications are website wrappers, not offline mirrors. They still require network access and, on Windows, the normal WebView2 runtime used by Tauri applications.

## Product goals

- Generate self-contained Windows EXE and Android APK files from a URL.
- Discover the site name and useful icon candidates automatically.
- Allow choosing a discovered icon or uploading a custom icon.
- Let generated applications coexist independently.
- Make regeneration of the same normalized URL update the existing Android application rather than install a duplicate.
- Detect PWA-declared file handlers and allow manual association editing.
- Provide first-class file adapters for Photopea and `vscode.dev`.
- Guarantee Windows double-click open and original-file write-back for Photopea PSD files and `vscode.dev` text files before v1 ships.
- Use system DNS by default, with an optional complete HTTPS DNS-over-HTTPS endpoint embedded per application.
- Keep all generation local, with no recurring server or CI cost.

## Non-goals for v1

- A hosted multi-user generator.
- macOS or Linux output.
- Windows code signing or store submission.
- Google Play publishing or managed signing.
- Offline copies of arbitrary websites.
- Guaranteed write-back for every website.
- Guaranteed `vscode.dev` write-back on Android.
- DNS-over-TLS, DNS-over-QUIC, or raw UDP/TCP DNS configuration.
- Silently falling back from custom DNS to system DNS.

## Prior-art decision

The project will use its own small runtime and generator rather than fork an existing wrapper project.

- PakePlus is the closest existing GUI-oriented product, but its repository and product surface are much larger than this project needs.
- Pake is mature and useful as implementation reference material, but is desktop-focused and is not a suitable base for the local dual-output generator.
- eweb-tauri is small, but its implementation does not provide the icon discovery or reliable cross-platform icon conversion described by its documentation.
- PWABuilder targets installable PWAs and store packaging rather than arbitrary Tauri wrappers with native file bridges.

Useful ideas may be studied and reimplemented narrowly, but the generator will not inherit an upstream repository's unrelated UI, updater, account, or workflow machinery.

## Chosen architecture

The project contains two products built from one Rust workspace:

1. **Generator:** the local Tauri desktop UI and packager.
2. **Universal wrapper runtime:** compiled once for Windows and Android, then specialized without recompiling Rust for each website.

The generator UI uses plain HTML, CSS, and TypeScript. A frontend framework is unnecessary for the small form-and-progress workflow.

### Embedded app specification

Every generated application carries a versioned app specification containing at least:

- schema version
- stable app identity
- display name and output slug
- normalized launch URL
- trusted origin set
- selected runtime adapter
- file extensions, MIME types, and descriptions
- DNS mode and optional complete HTTPS DoH URL
- generated app version

The specification is an internal format. Users interact with fields in the generator and never need to manage JSON or project files.

### URL identity

The generator canonicalizes a URL before deriving app identity:

- add `https://` when no scheme is supplied
- lowercase the hostname
- remove the default port
- normalize an empty path to `/`
- preserve meaningful path, query, and fragment components

A stable hash of the canonical URL forms the app identity. The visible name and icon may change without changing identity. Different canonical URLs get different identities.

On Android, the identity becomes a valid unique application ID such as `app.webwrap.a<hash>`. The package namespace of the precompiled runtime remains fixed; Android's `applicationId` varies per generated app. On Windows, the same identity determines the application user model ID and persistent data directory.

## Windows artifact generation

The Windows packager performs a local specialization step:

1. Copy an unsigned universal wrapper executable into a temporary staging directory.
2. Embed the app specification as a named PE resource.
3. Replace executable icon resources.
4. Replace display-name and version resources.
5. Validate that the resources can be read back.
6. Atomically publish the final `<slug>.exe`.

The wrapper reads configuration from its own executable resources. It has no adjacent JSON, icon, or runtime-specific sidecar.

The file is portable before association registration. On first launch, it offers to register its configured types for the current user and appear in Windows **Open with**. Windows controls default-application selection; the wrapper will not attempt to bypass that policy. Moving the executable after registration may require registering it again.

One running app instance accepts subsequent OS-open requests, allowing another double-click to open a file without creating a conflicting instance.

## Android artifact generation

The Android packager uses a cached local packaging kit rather than compiling Rust for every app.

The kit contains:

- a minimal JDK
- Android SDK platform and build tools
- a pinned Gradle distribution and Android Gradle plugin dependencies
- a small Android project template
- precompiled universal wrapper native libraries for supported ABIs

For each application, the generator:

1. Creates a temporary project from the packaging template.
2. Writes the embedded app specification.
3. Sets the unique application ID, label, and monotonically increasing version code.
4. Generates icon resources and file-association intent filters.
5. Packages the precompiled wrapper code and resources.
6. Signs with the generator's persistent local key.
7. Verifies the APK signature and manifest.
8. Atomically publishes `<slug>.apk`.

Android identifies applications by embedded application ID, not filename. Therefore `twitter.apk` and `facebook.apk` coexist. Regenerating the same canonical URL uses the same application ID and signing key, so it updates only that application.

### Signing-key lifecycle

The generator creates one persistent signing key automatically and protects it in its application-data directory. The UI provides an explicit export/backup action.

Loss of the signing key means a regenerated APK cannot update an already installed copy; Android would require uninstalling the old app first. Build errors must never cause key rotation.

## Generator experience

The primary workflow is one screen with progressive disclosure:

1. Enter a website, such as `twitter.com`.
2. Fetch and normalize the URL.
3. Discover metadata and show a preview.
4. Optionally change the app name, icon, file associations, DNS, and output targets.
5. Choose an output directory.
6. Generate.
7. Show separate Windows and Android progress and result cards.

### Metadata and icon discovery

The generator checks, in order:

1. web app manifest name and icons
2. `apple-touch-icon` links
3. standard icon links and favicons
4. application-name and document-title metadata

All usable icon candidates appear in a gallery, preferring large square assets. The user may upload a replacement. If discovery fails, the generator creates a simple initials-based fallback rather than calling a paid image-search service.

Remote metadata and image downloads have content-type, size, redirect, and timeout limits. Invalid SVG or oversized images are rejected before conversion.

### File associations

The generator combines three sources:

- associations declared by a site's web app manifest
- tested built-in presets for known adapters
- user-entered extensions and MIME types

Detected associations are suggestions and remain editable. Ordinary sites receive no associations unless they declare them or the user adds them.

Photopea preselects PSD support. `vscode.dev` preselects Markdown and offers common text/code formats without automatically claiming every text file on the machine.

### DNS selection

The generator presents:

- **System DNS** — default
- **Custom DNS** — requires a complete `https://` DoH URL

There is no provider-specific parsing or NextDNS special case. A custom endpoint is parsed, queried, and TLS-validated before packaging. Invalid endpoints block generation with a clear field error.

The endpoint is embedded in the application and omitted from routine logs, but is not treated as a cryptographic secret: anyone able to inspect a client binary may recover client configuration.

## Runtime behavior

### Storage isolation

Every generated app gets its own cookies, cache, permissions, service-worker data, and local storage.

- Android isolation follows from the unique application ID.
- Windows explicitly selects a WebView2 data directory derived from the embedded app identity because all generated executables share the same compiled runtime.

Changing the display name does not move or merge application data.

### Navigation

- Normal HTTPS navigation and login redirects remain inside the app.
- External pop-up links open in the system browser.
- Certificate errors are never bypassed automatically.
- Native file capabilities are active only on the configured trusted origin.
- Navigating away disables the file bridge immediately.

The remote page receives no general Tauri IPC object. Camera, microphone, geolocation, notifications, clipboard, and related web capabilities use platform permission prompts.

### Per-app custom DNS

Custom DNS is implemented without changing device-wide settings:

1. The wrapper starts an embedded loopback HTTP CONNECT proxy on an ephemeral port.
2. The WebView is configured to use that proxy.
3. The proxy resolves target hostnames through the configured DoH endpoint.
4. HTTPS remains end-to-end between the WebView and target site; the proxy does not intercept TLS.

On Windows, the runtime uses WebView/Wry proxy configuration. On Android, it uses AndroidX WebKit's process-scoped `ProxyController` through the platform bridge.

The loopback listener accepts connections only from the local process boundary where the platform permits and never binds a LAN interface. DNS responses may be cached according to their TTL.

The system resolver may bootstrap the DoH server hostname itself, but target-site lookups never use system DNS while custom mode is active.

If the DoH endpoint cannot be reached, the app shows an error surface with:

- **Retry**
- **Use system DNS**

System fallback happens only after that explicit user action. It is never silent.

## File-open compatibility pipeline

Incoming paths or content URIs pass through a tiered pipeline:

1. tested site-specific adapter
2. PWA `file_handlers` / `launchQueue` compatibility bridge
3. Web File System Access compatibility bridge
4. file-input or drag/drop fallback
5. explicit unsupported message

The UI and documentation distinguish **verified write-back**, **verified open-only**, and **best effort**. A successful file open must not be presented as proof that saving writes to the original.

### Photopea adapter

The Photopea adapter uses Photopea's Live Messaging and custom I/O APIs:

- read bytes only from the explicitly opened file
- send the buffer into Photopea
- associate Photopea's source identifier with the native file token
- receive exported PSD bytes on save
- replace the original atomically where the platform permits

On Android, the native token is a content URI and writes occur through the content resolver. Save failure leaves the prior content intact where atomic replacement is supported and always reports the failure.

### `vscode.dev` Windows adapter

Chrome and Edge can save local files because `vscode.dev` receives a genuine writable `FileSystemFileHandle`. A Windows double-click gives Tauri a native path instead.

WebView2 provides the missing bridge: its host API can create a genuine read/write web filesystem handle from a native path and post that DOM object into the page. Tauri exposes the platform WebView through `with_webview`.

The proposed adapter flow is:

1. Windows launches the generated app with the selected file path.
2. The runtime validates the path and current `vscode.dev` origin.
3. WebView2 creates a read/write `FileSystemFileHandle` for that exact path.
4. An initialization adapter supplies the handle to `vscode.dev`'s existing open-file path.
5. `vscode.dev` registers the handle in its browser filesystem provider.
6. Ctrl+S calls the normal `createWritable()` flow and changes the original file.

Steps 4 and 5 depend on hosted `vscode.dev` behavior and therefore require a release-blocking prototype before other UI work. If the prototype fails, v1 is blocked for redesign; it must not silently substitute a Monaco editor or claim write-back support.

### PWA bridge

For websites that declare file handlers, the runtime makes incoming handles available through a compatible `launchQueue` before site code registers its consumer. This path is verified per site because manifests may claim formats the application cannot actually edit.

### Generic fallback

For unknown sites, the runtime may populate a compatible file input or simulate a drop where the platform allows. These paths are open-only until an end-to-end save test proves otherwise.

If a site saves by downloading a new file, the runtime may offer to replace the original only after explicit confirmation. It never assumes an arbitrary download corresponds to the opened document.

## Security boundaries

- Only files explicitly opened through the OS or a native picker may receive native tokens.
- A remote page cannot submit an arbitrary path for native access.
- File handles are posted only after checking the current top-level origin immediately before delivery.
- Known adapters run only on their exact configured origins.
- Navigation revokes access to pending bridge operations.
- Generated applications isolate site storage by stable app identity.
- Custom DNS changes name resolution only; it is not a VPN and does not promise to proxy non-WebView traffic.
- HTTPS certificates remain validated against the requested hostname.
- Packaging input is validated before it enters resource, manifest, command, or filename contexts.
- Packaging commands receive structured arguments rather than shell-composed strings.

## Failure handling

### Generator failures

- Metadata failure: allow manual name and icon entry.
- Icon failure: discard the candidate and retain other choices or the fallback.
- Tool download failure: preserve verified chunks and offer retry.
- Checksum mismatch: discard the affected payload and redownload.
- Insufficient disk space: fail before staging when detectable.
- Packaging failure: retain a readable diagnostic log, remove partial artifacts, and preserve the last good output.
- Signing failure: do not regenerate or replace the signing key.

### Runtime failures

- Site unavailable: show retry and connection diagnostics.
- Custom DNS unavailable: fail closed and require an explicit choice.
- File permission denied: identify the file and request the minimum necessary action.
- Save failure: report it prominently; never claim success based only on an in-memory edit.
- Unsupported adapter behavior: explain whether the file was not opened or merely cannot be written back.

## Validation strategy

### Automated checks

- URL canonicalization and stable identity vectors.
- Different URLs produce different Android application IDs.
- The same URL preserves identity across regenerations.
- App specification round-trip through PE and Android assets.
- Icon conversion and malformed-image rejection.
- File-association manifest generation.
- Custom DoH URL validation and fail-closed state transitions.
- Packaging transactions do not publish partial outputs.
- Android manifest inspection, signature verification, and version-code monotonicity.
- Origin checks deny bridge delivery after navigation.

### V1 release gates

1. `twitter.exe` and `twitter.apk` are single-file outputs with embedded configuration and icons.
2. Twitter and Facebook APKs install simultaneously and retain isolated state.
3. Regenerating Twitter updates only Twitter.
4. On Windows, a generated Photopea app appears in **Open with**, opens a PSD, edits it, and Ctrl+S changes the original file bytes.
5. On Windows, a generated `vscode.dev` app appears in **Open with**, opens an MD file, edits it, and Ctrl+S changes the original file bytes.
6. A second file opened while the Windows app is running reaches that instance.
7. File access is unavailable after navigation to an untrusted origin.
8. A generated APK passes `apksigner verify` and its manifest contains the expected application ID, label, and intent filters.
9. Twitter and Facebook use separate Windows WebView2 data directories.
10. A custom DoH test proves target lookups use the chosen endpoint.
11. An unreachable DoH endpoint does not silently use system DNS.

## Delivery sequence

The riskiest assumptions are tested before product UI construction:

1. Windows WebView2-to-`vscode.dev` writable-handle spike.
2. Photopea Windows open/save spike.
3. Windows PE embedding and icon replacement spike.
4. Android precompiled-runtime packaging, unique installation, and update spike.
5. Android Photopea content-URI open/save spike.
6. Per-app DoH proxy on Windows and Android.
7. Universal runtime integration.
8. Generator UI and metadata discovery.
9. Hardening and complete release-gate run.

This order prevents substantial UI or packaging work from being built around an unverified file-write mechanism.

## Key references

- [Tauri file associations](https://v2.tauri.app/reference/config/#fileassociation)
- [Tauri platform WebView access](https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindow.html#method.with_webview)
- [WebView2 host-created filesystem handles](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2environment.createwebfilesystemfilehandle)
- [VS Code for the Web local-file behavior](https://code.visualstudio.com/docs/remote/vscode-web)
- [VS Code browser filesystem provider](https://github.com/microsoft/vscode/blob/main/src/vs/platform/files/browser/htmlFileSystemProvider.ts)
- [Photopea Live Messaging API](https://www.photopea.com/api/live)
- [Photopea environment API](https://www.photopea.com/api/environment)
- [Android application IDs](https://developer.android.com/build/configure-app-module)
- [Android command-line APK builds](https://developer.android.com/build/building-cmdline)
- [Android APK signing](https://developer.android.com/tools/apksigner)
- [Android WebView process proxy](https://developer.android.com/reference/androidx/webkit/ProxyController)
- [Wry proxy configuration](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html#method.with_proxy_config)
