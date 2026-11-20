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

  // File > Save reports which document it was pressed on, so a save waiting
  // behind other work still saves that document, whatever is active by then.
  const config = {
    environment: {
      customIO: {
        open: 'app.echoToOE("Open");',
        save:
          'var d = app.activeDocument;' +
          'app.echoToOE(JSON.stringify({ webwrap: "save-pressed", source: d ? d.source : null, name: d ? d.name : null }));',
      },
    },
  };
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
  // awaits a file's own "done": each script it sends echoes a reply carrying
  // its request's id, and the "done" after that echo ends the wait. Anything
  // before the echo, including a late echo of a request that already gave
  // up, belongs to another message and is dropped. Operations run one at a
  // time.

  let pending = null;
  let lastId = 0;
  let queue = Promise.resolve();
  const serial = (operation) => (queue = queue.then(operation).catch((error) => notice(String(error))));

  /** Send `script(id)`, which must echo `{webwrap: kind, id, …}`; collect the
   * echo and what follows it until "done", or give up after 5 minutes. */
  const ask = (kind, script) =>
    new Promise((resolve) => {
      const request = {
        kind,
        id: ++lastId,
        seen: false,
        replies: [],
        finish() {
          clearTimeout(request.timer);
          if (pending === request) pending = null;
          resolve(request.replies);
        },
      };
      request.timer = setTimeout(() => request.finish(), 300000);
      pending = request;
      frame.contentWindow.postMessage(script(request.id), PHOTOPEA.origin);
    });
  const reply = (replies, kind) => replies.find((item) => item?.webwrap === kind);

  addEventListener('message', (event) => {
    if (!frame || event.source !== frame.contentWindow || event.origin !== PHOTOPEA.origin) return;
    const data = event.data;
    if (data === 'Open') return void post({ type: 'webwrap-pick-file' });
    if (data === 'done') {
      if (!ready) {
        ready = true;
        post({ type: 'webwrap-ready' });
      } else if (pending?.seen) {
        pending.finish();
      }
      return;
    }
    const item = typeof data === 'string' && data.startsWith('{"webwrap"') ? JSON.parse(data) : data;
    if (item?.webwrap === 'save-pressed') return void serial(() => save(item));
    if (!pending) return;
    if (!pending.seen && item?.webwrap === pending.kind && item.id === pending.id) pending.seen = true;
    if (pending.seen) pending.replies.push(item);
  });

  // ---- Opening -----------------------------------------------------------

  const FORMATS = { psd: 'psd', png: 'png', jpg: 'jpg:0.95', jpeg: 'jpg:0.95', webp: 'webp' };
  const formatOf = (name) => FORMATS[name.split('.').pop().toLowerCase()] ?? 'psd';
  /** token -> { name, stem, format } for each document opened from a file. */
  const files = new Map();

  async function open(token, name, bytes) {
    const before = reply(
      await ask('count', (id) => `app.echoToOE(JSON.stringify({ webwrap: "count", id: ${id}, count: app.documents.length }));`),
      'count',
    );
    if (!before) throw new Error(`Photopea did not answer, so ${name} was not opened`);
    frame.contentWindow.postMessage(bytes, PHOTOPEA.origin, [bytes]);
    // Runs once Photopea has finished with the file. Photopea appends a file
    // it opened as the last document, named and sourced "file". Only that
    // document is tagged: if Photopea could not read the file, or the user
    // switched tabs or made a document meanwhile, tagging anything else
    // would make that document's Save overwrite this file.
    const stem = name.replace(/\.[^.]*$/, '') || name;
    const tagged = reply(
      await ask('tag', (id) =>
        'var last = app.documents[app.documents.length - 1];' +
        `if (app.documents.length === ${before.count + 1} && last.source === "file" && last.name === "file") {` +
          `last.source = ${JSON.stringify(`webwrap:${token}`)};` +
          `last.name = ${JSON.stringify(stem)};` +
          `app.echoToOE(JSON.stringify({ webwrap: "tag", id: ${id}, opened: true, name: last.name }));` +
        `} else app.echoToOE(JSON.stringify({ webwrap: "tag", id: ${id}, opened: false }));`,
      ),
      'tag',
    );
    if (tagged?.opened) {
      // Photopea may shorten a name it is given, so keep the one it stored.
      files.set(token, { name, stem: tagged.name, format: formatOf(name) });
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
    new Promise((resolve) => {
      const timer = setTimeout(() => resolve({ ok: false }), 600000);
      saves.set(token, {
        bytes,
        resolve(result) {
          clearTimeout(timer);
          resolve(result);
        },
      });
      post({ type: 'webwrap-save', token, size: bytes.byteLength });
    }).finally(() => saves.delete(token));

  /** Save the document File > Save was pressed on (`pressed`). */
  async function save(pressed) {
    if (!pressed.name) return notice('Nothing to save');
    const token = pressed.source?.startsWith?.('webwrap:') ? pressed.source.slice('webwrap:'.length) : null;
    const file = token && files.get(token);
    // A tagged document writes its file only while it still has the name it
    // was opened with: a copy Photopea made of it, or a renamed one, is not
    // that file and is downloaded instead.
    const bound = file && pressed.name === file.stem ? file : null;
    const label = bound ? bound.name : `${pressed.name}.psd`;
    notice(`Saving ${label}…`);
    // Photopea exports only the active document, so the pressed one is made
    // active first. It must be the only document with that source and name.
    const replies = await ask('save', (id) =>
      '(function () {' +
        `var source = ${JSON.stringify(pressed.source)}, name = ${JSON.stringify(pressed.name)}, found = [];` +
        'for (var i = 0; i < app.documents.length; i++) {' +
          'if (app.documents[i].source === source && app.documents[i].name === name) found.push(app.documents[i]);' +
        '}' +
        `if (found.length !== 1) { app.echoToOE(JSON.stringify({ webwrap: "save", id: ${id}, found: found.length })); return; }` +
        'app.activeDocument = found[0];' +
        `app.echoToOE(JSON.stringify({ webwrap: "save", id: ${id}, found: 1 }));` +
        `app.activeDocument.saveToOE(${JSON.stringify(bound ? bound.format : 'psd')});` +
      '})();',
    );
    const head = reply(replies, 'save');
    const bytes = replies.find((item) => item instanceof ArrayBuffer);
    if (head?.found > 1) return notice(`Not saved: two documents are named ${pressed.name}`);
    if (!head?.found || !bytes) return notice(`Not saved: ${label}`);
    if (!bound) return download(bytes, label);
    const result = await write(token, bytes);
    notice(result.ok ? `Saved ${label}` : `Not saved: ${label}`);
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
