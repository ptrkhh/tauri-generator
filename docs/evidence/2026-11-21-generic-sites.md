# Evidence: generic sites (sub-project D, phase D4)

Date: 2026-11-21. Branch `feat/generic-sites`. Spec: `docs/superpowers/specs/2026-11-20-file-bridge-and-adapters-design.md` (section D4). Plan: `docs/superpowers/plans/2026-11-21-generic-sites.md`.

**What it closes:** no release gate. An app generated with no adapter now does something with a file it is given and says which tier took it. Before D4 such a file was queued and never delivered, with no message, because no real site ever said `webwrap-ready`.

**What it does not prove:**

- **Tier 2 writing.** CI drives WebView2's own picker and the site gets its handle, but a write through that handle gets no answer within 20 s on the runners, most likely a permission prompt nobody answers. That is a PC check (section 6).
- **How the notices and boxes look**, and drops on real sites other than Excalidraw.

## 1. Environments

GitHub Actions run 44963000604 at `90d42a2`, `windows-latest` (x64, WebView2 157.0.4341.46) and `windows-11-arm` (ARM64, WebView2 157.0.4341.60). Both jobs passed. Linux: `cargo test -p webwrap-bridge` (30 tests), the runtime's unit tests under Wine 11 (x64), and clippy `-D warnings` for the Windows target.

## 2. Automated results

**Local test pages** (`e2e/generic.mjs`, app `bridge-e`, `https://localhost:8447/`), identical on both runners:

| Case | Result |
| --- | --- |
| `--inspect-self` | `fileTier: "generic"` |
| 1. `/pwa`, a file outside AppData | reached the consumer as a handle; `file-tier 1`; the page's `createWritable()` changed the original |
| 2. `/pwa`, a file in `%TEMP%` | sent as a buffer instead, into the upload box; `file-tier 3`; `open-only-notice shown` once and the flag file written |
| 3. `/late`, consumer set at 12 s | arrived as an upload; the late consumer got nothing |
| 4. `/upload`, one visible and one hidden box | the visible box got the right bytes; no second notice |
| 5. `/drop`, a drop zone | got the right bytes; `file-tier 3` |
| 6. `/none` | `open-failed none.txt: Bridge E did not open it`; no tier claimed |
| 7. `/picker` | WebView2's dialog, titled "Open", was found and driven, and the page got the handle. The write did not answer in 20 s (recorded as not verified) |
| 8. gate 8, `https://127.0.0.1:8447/pwa` | no script `launchQueue`; a posted `webwrap-ready` was refused; a forwarded file was not delivered; earlier tokens revoked |

**D1-D3 unchanged:** every check in `e2e/bridge.mjs` and `e2e/photopea.mjs` passed with the generic script now also running on `bridge-a` and `bridge-b`.

**Live Excalidraw** (`e2e/generic-live.mjs`, its own step, weekly): on both runners a `.excalidraw` file given at launch went through `launchQueue` (`file-tier 1`), its rectangle reached Excalidraw's scene, and Ctrl+S from DevTools rewrote the original in Excalidraw's own serialization, with no download.

**Page script in Chromium** (headless, with a stand-in for `chrome.webview`, before CI): the visible box was chosen over a hidden one; a drop with `drop` cancelled counted; a page cancelling only `dragover`, and one with two visible boxes, both ended in tier 4; a resolving async consumer gave tier 1 and a rejecting one gave `open-failed`; `params instanceof LaunchParams` held.

## 3. Facts measured

- **Chromium drops a page-started navigation during the app's own first load.** The first CI run navigated from DevTools before `/` finished loading, and the page stayed on `/`. The test now waits for the first load, as D1's do.
- **WebView2's `showOpenFilePicker` dialog** is a classic `#32770` window titled "Open" with the same control ids as the runtime's own dialog (1148 and 1). `pickInDialog`, now in `e2e/apps.mjs`, finds it among the app's process tree.
- **Excalidraw consumes `launchQueue` in WebView2** with the script's stand-in, and saves through the handle.

## 4. Deviations from the plan

1. **The script always replaces the browser's `launchQueue`**, rather than keeping one the browser has. WebView2 has one on every page but never fills it. The gate-8 check tests for the script's version rather than for any `launchQueue`.
2. **The script also defines `LaunchParams`** if missing, and the object it passes is an instance of it, because sites test for both names.
3. **`transport()` takes `handle_allowed: bool`** instead of the refusal text, which it never used.
4. **The handle-to-buffer fallback covers only a failure to create the handle.** A failure to post it, after the token counts as delivered, still shows D1's error box (review finding 5). Known refusals never get that far: `handle_refusal` sends them as buffers first.
5. **The plan's hands-on Excalidraw check** is replaced by CI's live step. The PC instead checks tier 2 writing through Excalidraw's own Open, which CI could not. Excalidraw is added to the acceptance apps for that.

## 5. Review

An independent review of Tasks 1 and 2 found no security problem: no untrusted origin can get a file, handle, save or box, and no path reaches the page. Fixed in `503277e`:

1. An async consumer that rejected was reported as tier 1. Now the tier is posted once the consumer settles, and a rejection reports `open-failed`.
2. A buffer copy that failed was lost silently and the bytes were copied twice. Now one copy, released in `finally`, and a failure reports `open-failed`.
3. A handle arriving with no consumer was dropped silently. Now it reports `open-failed`.
4. The `want` lock was held through the delivery `match`. Now it is read first.

Recorded, not fixed: finding 5 (section 4, item 4) and finding 6 (section 6, item 3).

## 6. Known limits

1. **Tier 2 writing is unverified by automation.** PC check.
2. **Hidden upload boxes** (no client rects) are not filled, so many real sites with a styled upload button fall through to a drop or to tier 4.
3. **A trusted page can repeat `webwrap-open-failed`** for a delivered token and get a box each time. The page is the site the user chose; one box per token would be the fix.
4. **A document whose `load` never fires** never becomes ready, and its files stay queued silently, as before D4.
5. **A consumer set more than 10 s after load** is not used; the document keeps buffers.
6. **The tier is told to the user only for tiers 3 and 4** (the notice and the box). Tier 1 is only logged.

## 7. Hands-on PC checks

Queued in `docs/windows-runs/NEXT.md`:

- the open-only notice and the unsupported box as they appear;
- dropping an image on the Twitter app;
- Excalidraw: a file given at launch, then its own Open (tier 2) and Ctrl+S, recording any permission prompt.
