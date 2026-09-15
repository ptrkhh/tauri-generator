(() => {
  "use strict";

  const PHOTOPEA_ORIGIN = "https://www.photopea.com";
  const SHELL_READY = JSON.stringify({ type: "webwrap-photopea-shell-ready" });
  const bridge = window.chrome?.webview;
  const app = document.querySelector("#app");
  const status = document.querySelector("#status");
  const config = {
    environment: {
      customIO: {
        open: 'app.echoToOE("Open");',
        save: 'app.echoToOE("Save");',
      },
      localsave: false,
    },
  };

  const frame = document.createElement("iframe");
  frame.title = "Photopea";
  frame.src = `${PHOTOPEA_ORIGIN}/#${encodeURIComponent(JSON.stringify(config))}`;
  app.append(frame);

  bridge?.addEventListener("message", () => {
    // Task 2 adds the native handle protocol.
  });

  window.addEventListener("message", (event) => {
    if (event.source !== frame.contentWindow || event.origin !== PHOTOPEA_ORIGIN) return;
    // Task 2 adds Photopea Live Messaging handling.
  });

  frame.addEventListener("load", () => {
    status.textContent = "Photopea ready";
    bridge?.postMessage(SHELL_READY);
  });
})();
