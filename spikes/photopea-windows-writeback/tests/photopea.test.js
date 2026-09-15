"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");

const listeners = new Map();
const bridgeListeners = new Map();
const nativeMessages = [];
const sourceId = "webwrap-local:image.psd:test-token";
const status = { dataset: {}, textContent: "" };
const photopea = {
  activeDocument: {
    source: "",
    saveToOE() {
      dispatchPhotopea(new Uint8Array([56, 66, 80, 83]).buffer);
    },
  },
  echoToOE(message) {
    dispatchPhotopea(message);
  },
};
const frame = {
  contentWindow: {
    postMessage(message) {
      if (message instanceof ArrayBuffer) {
        dispatchPhotopea("done");
      } else {
        vm.runInNewContext(message, { app: photopea, JSON });
      }
    },
  },
  addEventListener(type, listener) {
    listeners.set(`frame:${type}`, listener);
  },
};

function dispatchPhotopea(data) {
  listeners.get("window:message")?.({
    data,
    origin: "https://www.photopea.com",
    source: frame.contentWindow,
  });
}

class FileSystemFileHandle {
  async getFile() {
    return {
      name: "image.psd",
      async arrayBuffer() {
        return new Uint8Array([56, 66, 80, 83, 0]).buffer;
      },
    };
  }
}

const bridge = {
  addEventListener(type, listener) {
    bridgeListeners.set(type, listener);
  },
  postMessage(message) {
    nativeMessages.push(message);
  },
};

const context = {
  ArrayBuffer,
  FileSystemFileHandle,
  JSON,
  String,
  Uint8Array,
  btoa,
  document: {
    createElement() {
      return frame;
    },
    querySelector(selector) {
      return selector === "#app" ? { append() {} } : status;
    },
  },
  encodeURIComponent,
  window: {
    chrome: { webview: bridge },
    addEventListener(type, listener) {
      listeners.set(`window:${type}`, listener);
    },
  },
};

const script = fs.readFileSync(`${__dirname}/../ui/photopea.js`, "utf8");
vm.runInNewContext(script, context, { filename: "photopea.js" });
listeners.get("frame:load")();
dispatchPhotopea("done");
bridgeListeners.get("message")({
  additionalObjects: [new FileSystemFileHandle()],
  data: { type: "webwrap-file", source: sourceId },
});

setImmediate(() => {
  assert.equal(status.textContent, "PASS: Opened image.psd");
  nativeMessages.length = 0;
  photopea.activeDocument.source = "webwrap-local:other.psd:test-token";

  dispatchPhotopea("Save");

  const payloads = nativeMessages
    .map((message) => {
      try {
        return JSON.parse(message);
      } catch {
        return null;
      }
    })
    .filter((message) => message?.type === "webwrap-photopea-save");
  assert.deepEqual(payloads, [], "a different active document must never be written");

  photopea.activeDocument.source = sourceId;
  dispatchPhotopea("Save");
  bridgeListeners.get("message")({ data: { type: "webwrap-photopea-save-complete" } });
  dispatchPhotopea("Save");

  const repeatedPayloads = nativeMessages
    .map((message) => {
      try {
        return JSON.parse(message);
      } catch {
        return null;
      }
    })
    .filter((message) => message?.type === "webwrap-photopea-save");
  assert.equal(repeatedPayloads.length, 2, "the associated document must support repeat saves");
});
