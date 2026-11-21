// The generic adapter (adapter: none), run in every document before the
// site's scripts. The runtime fills in the app's trusted origins below; it
// also checks them itself, so this check only keeps other sites untouched.
//
// A file reaches the site by the strongest way it accepts, and the app is
// told which (D4 plan, clarifications 1-5):
//   1. a launchQueue consumer, given a read-write handle;
//   3. the page's one visible upload box, or else a drop, given a File;
//   4. neither: the app says the site did not accept it.
// (Tier 2, the site's own showOpenFilePicker, needs nothing from here.)
(() => {
  const trusted = __WEBWRAP_TRUSTED__;
  if (window.top !== window || !trusted.includes(location.origin)) return;
  const bridge = window.chrome?.webview;
  if (!bridge) return;
  // Page-to-app messages are JSON text posted as a string.
  const post = (message) => bridge.postMessage(JSON.stringify(message));

  let consumer = null;
  let decided = false;
  /** Say ready once, asking for handles only if the site set a consumer. */
  const ready = (want) => {
    if (decided) return;
    decided = true;
    post({ type: 'webwrap-ready', want });
  };

  // Replaces any launchQueue the browser has: WebView2 never fills its own,
  // because nothing here is an installed PWA.
  Object.defineProperty(window, 'launchQueue', {
    configurable: true,
    value: Object.freeze({
      setConsumer(fn) {
        if (typeof fn !== 'function') throw new TypeError('setConsumer needs a function');
        consumer = fn;
        ready('handle');
      },
    }),
  });

  const loaded = new Promise((resolve) => {
    if (document.readyState === 'complete') resolve();
    else window.addEventListener('load', resolve, { once: true });
  });
  // A consumer set later than this is not used: the document keeps its choice.
  loaded.then(() => setTimeout(() => ready('buffer'), 10000));

  bridge.addEventListener('message', (event) => {
    if (event.data?.type !== 'webwrap-file-handle') return;
    const handle = event.additionalObjects?.[0];
    if (!(handle instanceof FileSystemFileHandle) || !consumer) return;
    const token = event.data.token;
    try {
      consumer(Object.freeze({ files: Object.freeze([handle]), targetURL: location.href }));
      post({ type: 'webwrap-file-tier', token, tier: 1 });
    } catch (error) {
      console.error('webwrap: the launchQueue consumer failed', error);
      post({ type: 'webwrap-open-failed', tokens: [token] });
    }
  });

  const visible = (input) => !input.disabled && input.getClientRects().length > 0;

  /** Give `file` to the page's one visible upload box, or else drop it. */
  const offer = (file) => {
    const transfer = new DataTransfer();
    transfer.items.add(file);
    const boxes = [...document.querySelectorAll('input[type=file]')].filter(visible);
    if (boxes.length === 1) {
      boxes[0].files = transfer.files;
      for (const type of ['input', 'change']) boxes[0].dispatchEvent(new Event(type, { bubbles: true }));
      return true;
    }
    const [x, y] = [innerWidth / 2, innerHeight / 2];
    const target = document.elementFromPoint(x, y) ?? document.body;
    if (!target) return false;
    const fire = (type) => {
      const event = new DragEvent(type, {
        bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, dataTransfer: transfer,
      });
      target.dispatchEvent(event);
      return event.defaultPrevented;
    };
    fire('dragenter');
    fire('dragover');
    // Only a cancelled drop means the site read the file; cancelling
    // dragover alone just stops the browser opening a dropped file itself.
    return fire('drop');
  };

  bridge.addEventListener('sharedbufferreceived', async (event) => {
    const info = event.additionalData;
    if (info?.type !== 'webwrap-file') return;
    const buffer = event.getBuffer();
    const file = new File([new Uint8Array(buffer, 0, info.size).slice()], info.name);
    // Released after this dispatch, so other listeners on the page can still
    // read the buffer synchronously.
    setTimeout(() => bridge.releaseBuffer(buffer), 0);
    await loaded;
    post(offer(file)
      ? { type: 'webwrap-file-tier', token: info.token, tier: 3 }
      : { type: 'webwrap-open-failed', tokens: [info.token] });
  });
})();
