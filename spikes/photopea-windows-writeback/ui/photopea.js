(() => {
  "use strict";

  const PHOTOPEA_ORIGIN = "https://www.photopea.com";
  const MAX_PSD_BYTES = 32 * 1024 * 1024;
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
  const messages = Object.freeze({
    shellReady: JSON.stringify({ type: "webwrap-photopea-shell-ready" }),
    handleReceived: JSON.stringify({
      type: "webwrap-photopea-handle-received",
    }),
    opened: JSON.stringify({ type: "webwrap-photopea-opened" }),
    saveRequested: JSON.stringify({
      type: "webwrap-photopea-save-requested",
    }),
    saveComplete: JSON.stringify({ type: "webwrap-photopea-save-complete" }),
    error: JSON.stringify({ type: "webwrap-error" }),
  });

  const sourceIdFor = (fileName, token) => {
    const basename = String(fileName)
      .replaceAll("\\", "/")
      .split("/")
      .pop();
    const safeName = (basename || "file")
      .replace(/[^A-Za-z0-9._-]/g, "-")
      .replace(/-+/g, "-")
      .replace(/^[.-]+|[.-]+$/g, "")
      .slice(0, 80);
    const safeToken = String(token)
      .replace(/[^A-Za-z0-9_-]/g, "")
      .slice(0, 64);
    if (!safeToken) throw new Error("source token is empty");
    return `webwrap-local:${safeName || "file"}:${safeToken}`;
  };

  if (
    sourceIdFor("My file.psd", "test-token") !==
    "webwrap-local:My-file.psd:test-token"
  ) {
    throw new Error("source ID self-check failed");
  }

  const frame = document.createElement("iframe");
  frame.title = "Photopea";
  frame.src = `${PHOTOPEA_ORIGIN}/#${encodeURIComponent(JSON.stringify(config))}`;
  app.append(frame);

  let fileName;
  let fileBytes;
  let sourceId;
  let fileAssociation;
  let fileState = "idle";
  let openState = "waiting-photopea";
  let saveState = "idle";
  let photopeaReady = false;

  const setStatus = (text) => {
    status.textContent = text;
  };

  const reportError = (reason, notifyNative = true) => {
    setStatus(`FAIL: ${reason}`);
    if (notifyNative) {
      try {
        bridge?.postMessage(messages.error);
      } catch {
        // The status remains visible when the native channel itself failed.
      }
    }
  };

  const base64From = (buffer) => {
    const bytes = new Uint8Array(buffer);
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
    }
    return btoa(binary);
  };

  const requestSave = () => {
    if (openState !== "opened" || saveState !== "idle" || !sourceId) {
      reportError("Photopea save requested out of order");
      return;
    }
    const script = [
      "(function(){",
      "var source=app.activeDocument?app.activeDocument.source:null;",
      'app.echoToOE(JSON.stringify({kind:"save-source",source:source}));',
      `if(source===${JSON.stringify(sourceId)}){app.activeDocument.saveToOE("psd");}`,
      "})();",
    ].join("");
    saveState = "checking-source";
    try {
      if (!bridge) throw new Error("native bridge is unavailable");
      bridge.postMessage(messages.saveRequested);
      frame.contentWindow.postMessage(script, PHOTOPEA_ORIGIN);
    } catch (error) {
      saveState = "idle";
      reportError(`Photopea save request failed: ${error?.message || "unknown error"}`);
    }
  };

  const handleExport = (buffer) => {
    if (saveState !== "awaiting-export" || !sourceId || fileAssociation?.sourceId !== sourceId) {
      saveState = "idle";
      reportError("Photopea export arrived out of order");
      return;
    }
    if (buffer.byteLength > MAX_PSD_BYTES) {
      saveState = "idle";
      reportError("PSD export exceeds the 32 MiB spike limit");
      return;
    }
    let encoded;
    try {
      encoded = base64From(buffer);
    } catch (error) {
      saveState = "idle";
      reportError(`PSD export encoding failed: ${error?.message || "unknown error"}`);
      return;
    }
    saveState = "awaiting-native";
    try {
      if (!bridge) throw new Error("native bridge is unavailable");
      bridge.postMessage(JSON.stringify({
        type: "webwrap-photopea-save",
        source: sourceId,
        data: encoded,
      }));
    } catch (error) {
      saveState = "idle";
      reportError(`PSD export delivery failed: ${error?.message || "unknown error"}`);
    }
  };

  const reportOpened = () => {
    if (
      openState !== "waiting-opened" ||
      fileAssociation?.sourceId !== sourceId ||
      !fileAssociation?.handle
    ) {
      reportError("Photopea opened out of order");
      return;
    }
    openState = "opened";
    status.dataset.sourceId = sourceId;
    setStatus(`PASS: Opened ${fileName}`);
    bridge?.postMessage(messages.opened);
  };

  const sendSourceAssignment = () => {
    if (!frame.contentWindow || !sourceId || fileAssociation?.sourceId !== sourceId) {
      reportError("Photopea frame is unavailable");
      return;
    }
    const openedPayload = JSON.stringify({ kind: "opened", source: sourceId });
    const script = [
      `app.activeDocument.source=${JSON.stringify(sourceId)};`,
      `app.echoToOE(${JSON.stringify(openedPayload)});`,
    ].join("");
    openState = "waiting-opened";
    try {
      frame.contentWindow.postMessage(script, PHOTOPEA_ORIGIN);
    } catch (error) {
      reportError(`Photopea source assignment failed: ${error?.message || "unknown error"}`);
    }
  };

  const startOpen = () => {
    if (!photopeaReady || !fileBytes || openState !== "waiting-photopea") return;
    if (!frame.contentWindow || !sourceId || fileAssociation?.sourceId !== sourceId) {
      reportError("Photopea frame is unavailable");
      return;
    }
    const bytes = fileBytes;
    fileBytes = undefined;
    openState = "waiting-file-done";
    try {
      frame.contentWindow.postMessage(bytes, PHOTOPEA_ORIGIN, [bytes]);
    } catch (error) {
      reportError(`Photopea open failed: ${error?.message || "unknown error"}`);
    }
  };

  const readHandle = async (handle, nativeSourceId) => {
    if (fileState !== "idle") {
      reportError("duplicate file handle");
      return;
    }
    if (
      typeof FileSystemFileHandle === "undefined" ||
      !(handle instanceof FileSystemFileHandle)
    ) {
      reportError("native message did not include a file handle");
      return;
    }
    fileState = "reading";
    try {
      const file = await handle.getFile();
      const bytes = await file.arrayBuffer();
      if (bytes.byteLength > MAX_PSD_BYTES) {
        reportError("PSD exceeds the 32 MiB spike limit");
        return;
      }
      const view = new Uint8Array(bytes, 0, 4);
      if (view.length !== 4 || String.fromCharCode(...view) !== "8BPS") {
        reportError("selected file is not a PSD");
        return;
      }
      fileName = file.name || "file";
      if (
        typeof nativeSourceId !== "string" ||
        !nativeSourceId.startsWith("webwrap-local:") ||
        nativeSourceId.length > 256
      ) {
        reportError("native source ID is invalid");
        return;
      }
      sourceId = nativeSourceId;
      fileAssociation = { sourceId, handle };
      fileBytes = bytes;
      fileState = "ready";
      status.dataset.sourceId = sourceId;
      bridge?.postMessage(messages.handleReceived);
      startOpen();
    } catch (error) {
      reportError(`file read failed: ${error?.message || "unknown error"}`);
    }
  };

  bridge?.addEventListener("message", (event) => {
    if (event.data?.type === "webwrap-photopea-save-complete") {
      if (saveState !== "awaiting-native") {
        reportError("native save completed out of order", false);
        return;
      }
      saveState = "idle";
      setStatus(`PASS: Saved ${fileName}`);
      return;
    }
    if (event.data?.type === "webwrap-error") {
      saveState = "idle";
      reportError("native bridge failed", false);
      return;
    }
    if (event.data?.type !== "webwrap-file") return;
    void readHandle(event.additionalObjects?.[0], event.data.source);
  });

  window.addEventListener("message", (event) => {
    if (event.source !== frame.contentWindow || event.origin !== PHOTOPEA_ORIGIN) return;

    if (event.data instanceof ArrayBuffer) {
      handleExport(event.data);
      return;
    }

    if (event.data === "done") {
      if (!photopeaReady) {
        photopeaReady = true;
        startOpen();
      } else if (openState === "waiting-file-done") {
        sendSourceAssignment();
      }
      return;
    }

    if (event.data === "Save") {
      requestSave();
      return;
    }

    if (typeof event.data !== "string") return;
    let payload;
    try {
      payload = JSON.parse(event.data);
    } catch {
      return;
    }
    if (payload?.kind === "save-source") {
      if (
        saveState !== "checking-source" ||
        typeof payload.source !== "string" ||
        payload.source !== sourceId ||
        fileAssociation?.sourceId !== sourceId
      ) {
        saveState = "idle";
        reportError("Photopea active document does not match the local file");
        return;
      }
      saveState = "awaiting-export";
      return;
    }
    if (
      payload?.kind === "opened" &&
      typeof payload.source === "string" &&
      payload.source === sourceId
    ) {
      reportOpened();
    }
  });

  frame.addEventListener("load", () => {
    setStatus("Photopea loaded; waiting for its ready message");
    bridge?.postMessage(messages.shellReady);
  });
})();
