// The vscodeDev adapter, run in every document before the site's scripts.
// The runtime fills in the app's trusted origins below; it also checks
// them itself, so this check only keeps other sites untouched.
//
// vscode.dev opens files only through its "Open File..." command, which asks
// showOpenFilePicker({multiple:false}) for one file. So each file the app
// hands over is opened by running that command, one file at a time, and
// answering its picker call with the file's handle.
//
// The command is run with events dispatched on specific elements, never with
// typed input: a constructed event has no default action, so even if focus
// moves meanwhile, nothing is ever typed into an open document. (vscode.dev
// handles a constructed Ctrl+Shift+P but not a constructed Enter, so the
// palette entry is clicked instead.)
(() => {
  const trusted = __WEBWRAP_TRUSTED__;
  if (window.top !== window || !trusted.includes(location.origin)) return;
  const bridge = window.chrome?.webview;
  if (!bridge) return;
  // Page-to-app messages are JSON text posted as a string.
  const post = (message) => bridge.postMessage(JSON.stringify(message));

  /** Files handed over and not yet taken by vscode.dev, oldest first. */
  const queue = [];
  let pickerCalled;

  // While files are queued, vscode.dev's picker call gets the next one; with
  // none, the user's own Ctrl+O reaches WebView2's native picker.
  const nativePicker = window.showOpenFilePicker?.bind(window);
  window.showOpenFilePicker = async (...args) => {
    if (queue.length === 0) {
      if (nativePicker) return nativePicker(...args);
      throw new DOMException('No file was chosen', 'AbortError');
    }
    const file = queue.shift();
    pickerCalled?.(file);
    return [file.handle];
  };

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  // Timers, not requestAnimationFrame, which stops while the window is minimized.
  const waitFor = async (test, ms) => {
    const deadline = performance.now() + ms;
    for (;;) {
      const value = test();
      if (value || performance.now() >= deadline) return value;
      await sleep(50);
    }
  };

  const press = (target, key, code, keyCode, modifiers = {}) => {
    const event = new KeyboardEvent('keydown', {
      key, code, bubbles: true, cancelable: true, composed: true, ...modifiers,
    });
    // vscode.dev reads keyCode, which a constructed event leaves at 0.
    Object.defineProperty(event, 'keyCode', { get: () => keyCode });
    Object.defineProperty(event, 'which', { get: () => keyCode });
    target.dispatchEvent(event);
  };

  const focusedPalette = () => {
    const active = document.activeElement;
    return active?.tagName === 'INPUT' && active.value.startsWith('>') ? active : null;
  };

  /** Ctrl+Shift+P until the command palette has focus: early on, vscode.dev
   * may not handle keys yet. */
  async function openPalette() {
    const deadline = performance.now() + 30000;
    while (performance.now() < deadline) {
      press(document.activeElement ?? document.body, 'P', 'KeyP', 80, { ctrlKey: true, shiftKey: true });
      const palette = await waitFor(focusedPalette, 2000);
      if (palette) return palette;
    }
    return null;
  }

  /** The palette's highlighted entry, if it is Open File. */
  const openFileEntry = (palette) => {
    const id = palette.getAttribute('aria-activedescendant');
    const entry = id && document.getElementById(id);
    return entry?.textContent?.includes('Open File...') ? entry : null;
  };

  const click = (target) => {
    for (const type of ['mousedown', 'mouseup', 'click']) {
      target.dispatchEvent(new MouseEvent(type, {
        bubbles: true, cancelable: true, view: window, button: 0, buttons: type === 'mousedown' ? 1 : 0, detail: 1,
      }));
    }
  };

  /** Run "Open File..." once; true once vscode.dev took `file`. */
  async function runOpenFile(file) {
    const asked = new Promise((resolve) => (pickerCalled = resolve));
    const palette = await openPalette();
    if (!palette) return false;
    palette.value = '>Open File...';
    palette.dispatchEvent(new Event('input', { bubbles: true }));
    const entry = await waitFor(() => document.activeElement === palette && openFileEntry(palette), 5000);
    if (!entry) {
      press(palette, 'Escape', 'Escape', 27);
      return false;
    }
    click(entry);
    const taken = await Promise.race([asked, sleep(10000)]);
    pickerCalled = undefined;
    if (taken !== file) return false;
    // Let vscode.dev open and focus this file before the next run.
    await waitFor(
      () => [...document.querySelectorAll('.tab')].some((tab) => tab.textContent.includes(file.name)),
      10000,
    );
    return true;
  }

  let running = false;
  async function openQueued() {
    if (running) return;
    running = true;
    while (queue.length > 0) {
      const file = queue[0];
      if (!(await runOpenFile(file))) {
        // Not retried. The file stays queued, so vscode.dev's own
        // File > Open File... opens it.
        post({ type: 'webwrap-open-failed', token: file.token });
        break;
      }
    }
    running = false;
  }

  bridge.addEventListener('message', (event) => {
    if (event.data?.type !== 'webwrap-file-handle') return;
    const handle = event.additionalObjects?.[0];
    if (!(handle instanceof FileSystemFileHandle)) return;
    queue.push({ token: event.data.token, name: event.data.name, handle });
    openQueued();
  });

  const waiting = setInterval(() => {
    if (!document.querySelector('.monaco-workbench')) return;
    clearInterval(waiting);
    post({ type: 'webwrap-ready' });
  }, 100);
})();
