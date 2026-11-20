// A minimal Chrome DevTools Protocol client over Node 22's built-in WebSocket.
// It drives only the page inside a generated app, never the native side.

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Poll `probe` until it returns something truthy. Errors count as "not yet". */
export async function until(probe, what, timeoutMs = 20000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    try {
      const value = await probe();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${what}${last ? ` (last error: ${last.message})` : ''}`);
}

/** Attach to the page target on `port` whose URL starts with `urlPrefix`. */
export async function attach(port, urlPrefix) {
  const target = await until(async () => {
    const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    return list.find((t) => t.type === 'page' && t.url.startsWith(urlPrefix));
  }, `a page at ${urlPrefix} on debugging port ${port}`, 60000);
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = () => reject(new Error(`cannot open ${target.webSocketDebuggerUrl}`));
  });
  let nextId = 0;
  const pending = new Map();
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(message.error.message));
    else waiter.resolve(message.result);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++nextId;
      pending.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) {
      const { exception, text } = result.exceptionDetails;
      throw new Error(exception?.description ?? text);
    }
    return result.result.value;
  };
  /** Wait until the document at `origin` has finished loading. */
  const loaded = (origin) =>
    until(
      () => evaluate(`location.origin === ${JSON.stringify(origin)} && document.readyState === 'complete'`),
      `${origin} to load`,
      60000,
    );
  return { send, evaluate, loaded, close: () => socket.close() };
}
