// The photopea adapter's shell, run in every document before its scripts.
// It acts only in the shell page the runtime serves, which embeds Photopea
// and moves files between it and the app. The runtime fills in Photopea's
// address below; the shell talks to that origin's frame and nothing else.
(() => {
  if (window.top !== window || location.origin !== 'https://tauri.localhost' || location.pathname !== '/photopea.html') return;
  const PHOTOPEA = __WEBWRAP_PHOTOPEA__;
  const bridge = window.chrome?.webview;
  if (!bridge) return;
  // Page-to-app messages are JSON text posted as a string.
  const post = (message) => bridge.postMessage(JSON.stringify(message));
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  const config = { environment: { customIO: { open: 'app.echoToOE("Open");', save: 'app.echoToOE("Save");' } } };
  let frame;
  let status;
  let ready = false;

  const notice = (text) => {
    status.textContent = text;
    // Kept after the notice fades, for tests and for anyone inspecting.
    status.dataset.last = text;
    clearTimeout(notice.timer);
    notice.timer = setTimeout(() => (status.textContent = ''), 4000);
  };

  // ---- Talking to Photopea ----------------------------------------------
  // Photopea handles messages in order and answers each with "done", except
  // a file it cannot read, which gets no answer at all. So the shell never
  // awaits a file's own "done": each script it sends echoes a reply tagged
  // with a kind, and the "done" after that reply ends the wait. Operations run
  // one at a time.

  let pending = null;
  let queue = Promise.resolve();
  const serial = (operation) => (queue = queue.then(operation).catch((error) => notice(String(error))));

  /** Send a script that echoes `{webwrap: kind, …}`; collect the replies until
   * the "done" after that echo, or for up to 5 minutes. */
  const ask = (script, kind) =>
    Promise.race([
      new Promise((resolve) => {
        pending = { kind, seen: false, replies: [], resolve };
        frame.contentWindow.postMessage(script, PHOTOPEA.origin);
      }),
      sleep(300000).then(() => {
        const replies = pending?.replies ?? [];
        pending = null;
        return replies;
      }),
    ]);
  const reply = (replies, kind) => replies.find((item) => item?.webwrap === kind);

  addEventListener('message', (event) => {
    if (!frame || event.source !== frame.contentWindow || event.origin !== PHOTOPEA.origin) return;
    const data = event.data;
    // The user's File > Save and File > Open, through customIO.
    if (data === 'Save') return void serial(save);
    if (data === 'Open') return void post({ type: 'webwrap-pick-file' });
    if (data === 'done') {
      if (!ready) {
        ready = true;
        post({ type: 'webwrap-ready' });
      } else if (pending?.seen) {
        const { replies, resolve } = pending;
        pending = null;
        resolve(replies);
      }
      return;
    }
    if (!pending) return;
    const item = typeof data === 'string' && data.startsWith('{"webwrap"') ? JSON.parse(data) : data;
    if (item?.webwrap === pending.kind) pending.seen = true;
    pending.replies.push(item);
  });

  // ---- Opening -----------------------------------------------------------

  const FORMATS = { psd: 'psd', png: 'png', jpg: 'jpg:0.95', jpeg: 'jpg:0.95', webp: 'webp' };
  const formatOf = (name) => FORMATS[name.split('.').pop().toLowerCase()] ?? 'psd';
  /** token -> { name, format } for each document opened from a file. */
  const files = new Map();

  async function open(token, name, bytes) {
    const before = reply(
      await ask('app.echoToOE(JSON.stringify({ webwrap: "count", count: app.documents.length }));', 'count'),
      'count',
    );
    if (!before) throw new Error(`Photopea did not answer, so ${name} was not opened`);
    frame.contentWindow.postMessage(bytes, PHOTOPEA.origin, [bytes]);
    // Runs once Photopea has finished with the file. The document is tagged
    // only if one was added: if Photopea could not read the file, the active
    // document is an earlier one, and tagging it would make its Save
    // overwrite this file.
    const stem = name.replace(/\.[^.]*$/, '') || name;
    const tagged = reply(
      await ask(
        `if (app.documents.length === ${before.count + 1}) {` +
          `app.activeDocument.source = ${JSON.stringify(`webwrap:${token}`)};` +
          `app.activeDocument.name = ${JSON.stringify(stem)};` +
          'app.echoToOE(JSON.stringify({ webwrap: "tag", opened: true }));' +
        '} else app.echoToOE(JSON.stringify({ webwrap: "tag", opened: false }));',
        'tag',
      ),
      'tag',
    );
    if (tagged?.opened) {
      files.set(token, { name, format: formatOf(name) });
      notice(`Opened ${name}`);
    } else {
      post({ type: 'webwrap-open-failed', tokens: [token] });
      notice(`Photopea could not open ${name}`);
    }
  }

  // ---- Saving ------------------------------------------------------------

  /** token -> the save waiting for its buffer or its result. */
  const saves = new Map();

  /** Hand `bytes` to the app to write to the token's file. */
  const write = (token, bytes) =>
    Promise.race([
      new Promise((resolve) => {
        saves.set(token, { bytes, resolve });
        post({ type: 'webwrap-save', token, size: bytes.byteLength });
      }),
      sleep(600000).then(() => ({ ok: false, error: 'no answer' })),
    ]).finally(() => saves.delete(token));

  async function save() {
    // One Photopea turn reads the active document and exports it, so the
    // export always belongs to the document that was checked.
    const bound = Object.fromEntries([...files].map(([token, file]) => [`webwrap:${token}`, file.format]));
    const replies = await ask(
      '(function () {' +
        `var bound = ${JSON.stringify(bound)};` +
        'var d = app.activeDocument;' +
        'if (!d) { app.echoToOE(JSON.stringify({ webwrap: "save", state: "none" })); return; }' +
        'var format = bound[d.source];' +
        'app.echoToOE(JSON.stringify({ webwrap: "save", state: format ? "bound" : "unbound", source: d.source, name: d.name }));' +
        'd.saveToOE(format || "psd");' +
      '})();',
      'save',
    );
    const head = reply(replies, 'save');
    const bytes = replies.find((item) => item instanceof ArrayBuffer);
    if (!head || !bytes) return notice('Nothing to save');
    if (head.state === 'unbound') return download(bytes, `${head.name}.psd`);
    const token = head.source.slice('webwrap:'.length);
    const name = files.get(token)?.name ?? head.name;
    const result = await write(token, bytes);
    notice(result.ok ? `Saved ${name}` : `Not saved: ${name}`);
  }

  /** A document made inside Photopea has no file: download it, as Photopea does in a browser. */
  function download(bytes, name) {
    const url = URL.createObjectURL(new Blob([bytes]));
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 60000);
    notice(`Downloaded ${name}`);
  }

  // ---- The app's side ----------------------------------------------------

  bridge.addEventListener('sharedbufferreceived', (event) => {
    const info = event.additionalData;
    const shared = event.getBuffer();
    if (info?.type === 'webwrap-file') {
      // An ordinary copy, which can be handed to Photopea's frame.
      const bytes = shared.slice(0, info.size);
      bridge.releaseBuffer(shared);
      serial(() => open(info.token, info.name, bytes));
    } else if (info?.type === 'webwrap-save-buffer' && saves.has(info.token)) {
      new Uint8Array(shared).set(new Uint8Array(saves.get(info.token).bytes));
      bridge.releaseBuffer(shared);
      post({ type: 'webwrap-save-commit', token: info.token, save: info.save });
    } else {
      bridge.releaseBuffer(shared);
    }
  });

  bridge.addEventListener('message', (event) => {
    const data = event.data;
    if (data?.type === 'webwrap-save-result') saves.get(data.token)?.resolve(data);
  });

  document.addEventListener('DOMContentLoaded', () => {
    status = document.getElementById('status');
    frame = document.createElement('iframe');
    frame.title = 'Photopea';
    frame.src = `${PHOTOPEA.url}#${encodeURIComponent(JSON.stringify(config))}`;
    document.getElementById('app').append(frame);
  });
})();
