// The vscodeDev adapter, run in every document before the site's scripts.
// The runtime replaces __WEBWRAP_TRUSTED__ with the app's trusted origins; it
// also checks them itself, so this check only keeps other sites untouched.
(() => {
  const trusted = __WEBWRAP_TRUSTED__;
  if (window.top !== window || !trusted.includes(location.origin)) return;
  const bridge = window.chrome?.webview;
  if (!bridge) return;
  // Page-to-app messages are JSON text posted as a string.
  const post = (type) => bridge.postMessage(JSON.stringify({ type }));

  // vscode.dev's "Open File..." asks showOpenFilePicker for one file. While
  // the app has handed over files, each run gets the next one; otherwise the
  // user's own Ctrl+O reaches WebView2's native picker.
  const nativePicker = window.showOpenFilePicker?.bind(window);
  const held = [];
  window.showOpenFilePicker = async (...args) => {
    if (held.length === 0) {
      if (nativePicker) return nativePicker(...args);
      throw new DOMException('No file was chosen', 'AbortError');
    }
    const handle = held.shift();
    if (held.length > 0) post('webwrap-vscode-open');
    return [handle];
  };

  bridge.addEventListener('message', (event) => {
    if (event.data?.type !== 'webwrap-file-handle') return;
    const handle = event.additionalObjects?.[0];
    if (!(handle instanceof FileSystemFileHandle)) return;
    held.push(handle);
    post('webwrap-vscode-open');
  });

  // Ready once the workbench exists. A timer, not requestAnimationFrame,
  // which stops while the window is minimized.
  const waiting = setInterval(() => {
    if (!document.querySelector('.monaco-workbench')) return;
    clearInterval(waiting);
    post('webwrap-ready');
  }, 100);
})();
