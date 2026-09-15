(() => {
  if (window.top !== window || location.origin !== "https://vscode.dev") return;
  const bridge = window.chrome?.webview;
  if (!bridge) return;

  const browserPicker = window.showOpenFilePicker?.bind(window);
  let pendingHandle;
  let readyTimer;

  window.showOpenFilePicker = async (...args) => {
    if (pendingHandle) {
      const handle = pendingHandle;
      pendingHandle = undefined;
      return [handle];
    }
    if (browserPicker) return browserPicker(...args);
    throw new DOMException("No file was supplied", "NotFoundError");
  };

  bridge.addEventListener("message", (event) => {
    if (event.data?.type !== "webwrap-file") return;
    const handle = event.additionalObjects?.[0];
    if (!(handle instanceof FileSystemFileHandle)) return;
    pendingHandle = handle;
    clearInterval(readyTimer);
    bridge.postMessage(
      JSON.stringify({ type: "webwrap-vscode-handle-ready" }),
    );
  });

  const announceWhenWorkbenchExists = () => {
    if (!document.querySelector(".monaco-workbench")) {
      requestAnimationFrame(announceWhenWorkbenchExists);
      return;
    }
    const announce = () =>
      bridge.postMessage(JSON.stringify({ type: "webwrap-vscode-ready" }));
    readyTimer = setInterval(announce, 250);
    announce();
  };
  requestAnimationFrame(announceWhenWorkbenchExists);
})();
