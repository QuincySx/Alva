// INPUT: Explicit plugin definition, Node streams and AEP v0.1 JSON-RPC frames.
// OUTPUT: definePlugin, run, scoped host callbacks and typed action/message helpers.
// POS: Plugin-side author SDK; native launch, grants and durable authority stay with the Go host.
import { AsyncLocalStorage } from 'node:async_hooks';
import { randomUUID } from 'node:crypto';

export const protocolVersion = '0.1.0';
export const frameLimit = 16 << 20;
const events = new Set(['before_tool_call', 'after_tool_call', 'on_llm_call_start', 'on_llm_call_end', 'on_user_message', 'on_agent_start', 'on_agent_end']);
const object = x => x !== null && typeof x === 'object' && !Array.isArray(x);
const bounded = (x, n) => typeof x === 'string' && x.length > 0 && Buffer.byteLength(x) <= n && !/[\u0000-\u001f\u007f]/u.test(x);
const clone = x => JSON.parse(JSON.stringify(x));
export const message = (role, text) => ({ id: randomUUID(), role, content: [{ type: 'text', text }], timestamp: Date.now() });
export const textResult = (text, isError = false) => ({ content: [{ type: 'text', text }], is_error: isError });
const toolOutput = value => {
  const { is_error, ...result } = object(value) && Array.isArray(value.content) ? value : textResult(typeof value === 'string' ? value : JSON.stringify(value));
  return { ...result, isError: result.isError ?? is_error ?? false };
};
export const actions = Object.freeze({
  continue: () => ({ action: 'continue' }),
  block: reason => ({ action: 'block', reason }),
  modifyArgs: modified_arguments => ({ action: 'modify', modified_arguments }),
  replaceResult: result => ({ action: 'replace_result', result }),
  modifyResult: result => ({ action: 'modify_result', result }),
  modifyMessages: messages => ({ action: 'modify_messages', messages }),
  modifyResponse: response => ({ action: 'modify_response', response }),
});

export function definePlugin(definition) {
  if (!object(definition) || !/^[\w.-]{1,64}$/u.test(definition.name ?? '') || !bounded(definition.version, 64)) throw new TypeError('invalid plugin identity');
  const capabilities = [...(definition.capabilities ?? [])];
  const handlers = { ...(definition.handlers ?? {}) };
  const tools = { ...(definition.tools ?? {}) };
  if (capabilities.length > 32 || new Set(capabilities).size !== capabilities.length || capabilities.some(x => !bounded(x, 128) || !/^host:[a-zA-Z0-9_.:-]+$/u.test(x))) throw new TypeError('invalid capabilities');
  if (Object.keys(handlers).some(x => !events.has(x) || typeof handlers[x] !== 'function')) throw new TypeError('invalid event handler');
  if (Object.keys(tools).length > 1024) throw new TypeError('tool catalog too large');
  const frozenTools = Object.create(null);
  let catalogBytes = 0;
  for (const [name, tool] of Object.entries(tools)) {
    if (!bounded(name, 256) || !object(tool) || typeof tool.execute !== 'function' || !object(tool.inputSchema) || !bounded(tool.description, 16 << 10) || Buffer.byteLength(JSON.stringify(tool.inputSchema)) > 64 << 10) throw new TypeError('invalid tool');
    frozenTools[name] = Object.freeze({ ...tool, inputSchema: clone(tool.inputSchema) });
    catalogBytes += Buffer.byteLength(name) + Buffer.byteLength(tool.description) + Buffer.byteLength(JSON.stringify(tool.inputSchema));
    if (catalogBytes > 8 << 20) throw new TypeError('tool catalog metadata too large');
  }
  return Object.freeze({ ...definition, capabilities: Object.freeze(capabilities), handlers: Object.freeze(handlers), tools: Object.freeze(frozenTools) });
}

// run borrows streams while active. Completion removes listeners and pauses
// input, never closes caller-owned output. No discovery, installation or env IO.
export async function run(definition, { input = process.stdin, output = process.stdout, signal, callbackTimeout = 5000 } = {}) {
  const plugin = definePlugin(definition);
  if (!Number.isInteger(callbackTimeout) || callbackTimeout < 1 || callbackTimeout > 60000) throw new TypeError('invalid callback deadline');
  if (signal?.aborted) throw signal.reason ?? new Error('plugin aborted');
  const scope = new AsyncLocalStorage();
  const lifetime = new AbortController();
  const pending = new Map();
  const seen = new Set();
  const grants = new Set(plugin.capabilities.map(x => `host/${x.slice(5)}`));
  let buffer = Buffer.alloc(0), serial = 0, queued = 0, queuedBytes = 0, callbacks = 0;
  let initialized = false, closing = false, stopped = false, active = Promise.resolve(), writes = Promise.resolve();
  let settle, reject;
  const finished = new Promise((resolve, fail) => { settle = resolve; reject = fail; });
  const fail = error => {
    if (stopped) return;
    stopped = true;
    lifetime.abort(error);
    for (const call of pending.values()) { clearTimeout(call.timer); call.reject(error); }
    pending.clear();
    reject(error);
  };
  const send = value => {
    const bytes = Buffer.from(`${JSON.stringify(value)}\n`);
    if (bytes.length - 1 > frameLimit) return Promise.reject(new Error('AEP frame limit exceeded'));
    writes = writes.then(() => new Promise((resolve, failWrite) => {
      if (stopped) { failWrite(new Error('AEP stopped')); return; }
      output.write(bytes, error => error ? failWrite(error) : resolve());
    }));
    // A failed write also terminates pending reverse calls and the runtime.
    writes.catch(fail);
    return writes;
  };
  const call = async (method, params) => {
    if (stopped || !initialized || !grants.has(method)) throw new Error('host capability unavailable');
    if (pending.size >= 128 || ++callbacks > 4096 || Buffer.byteLength(JSON.stringify(params)) > 1 << 20) throw new Error('host callback budget exceeded');
    const id = `p-${++serial}`;
    const result = new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('host callback deadline exceeded')); }, callbackTimeout);
      pending.set(id, { resolve, reject, timer });
    });
    // Attach before awaiting the write; a fast host can reject immediately.
    result.catch(() => {});
    try { await send({ jsonrpc: '2.0', id, method, params }); return await result; }
    finally { const entry = pending.get(id); if (entry) clearTimeout(entry.timer); pending.delete(id); }
  };
  const stateCall = (method, extra = {}) => {
    const current = scope.getStore();
    if (!current?.active || !current.handle) return Promise.reject(new Error('state handle expired or unavailable'));
    return call(`host/state.${method}`, { ...extra, handle: current.handle });
  };
  const host = Object.freeze({
    signal: lifetime.signal,
    log: (message, level = 'info', fields = {}) => call('host/log', { message, level, fields }),
    notify: (message, level = 'info') => call('host/notify', { message, level }),
    emitMetric: (name, value, labels = {}) => call('host/emit_metric', { name, value, labels }),
    getMessages: (limit = 10, offset = 0) => stateCall('get_messages', { limit, offset }).then(x => x.messages),
    getMetadata: () => stateCall('get_metadata').then(x => x.metadata),
    countTokens: () => stateCall('count_tokens').then(x => x.tokens),
    readReference: (uri, offset = 0, limit = 2048) => stateCall('read_reference', { uri, offset, limit }),
    searchReference: (uri, query, limit = 5) => stateCall('search_reference', { uri, query, limit }),
  });
  const catalog = Object.entries(plugin.tools).map(([name, tool]) => ({ name, description: tool.description, inputSchema: tool.inputSchema }));
  const dispatch = async request => {
    const { method, id, params = {} } = request;
    const respond = result => id === undefined ? Promise.resolve() : send({ jsonrpc: '2.0', id, result });
    const remoteError = (code, message) => id === undefined ? Promise.resolve() : send({ jsonrpc: '2.0', id, error: { code, message } });
    if (method === 'initialize') {
      if (initialized || params.protocolVersion !== protocolVersion || !object(params.hostCapabilities)) return remoteError(-32602, 'invalid initialization');
      const offered = params.hostCapabilities;
      if (plugin.capabilities.some(x => !offered.hostApi?.includes(x.slice(5))) || Object.keys(plugin.handlers).some(x => !offered.events?.includes(x))) return remoteError(-32602, 'unavailable host capability');
      initialized = true;
      return respond({ protocolVersion, plugin: { name: plugin.name, version: plugin.version, description: plugin.description ?? '' }, tools: catalog, eventSubscriptions: Object.keys(plugin.handlers), requestedCapabilities: plugin.capabilities });
    }
    if (!initialized) return remoteError(-32000, 'initialize first');
    if (method === 'initialized') { if (plugin.onInit) await plugin.onInit(host); return; }
    if (method === 'shutdown') {
      closing = true;
      if (plugin.onShutdown) await plugin.onShutdown(host);
      await respond({});
      stopped = true;
      settle();
      return;
    }
    if (method === 'tools/list') return respond({ tools: catalog });
    const current = { active: true, handle: params.stateHandle };
    try {
      return await scope.run(current, async () => {
        if (method === 'tools/call') {
          const tool = Object.hasOwn(plugin.tools, params.name) && plugin.tools[params.name];
          if (!tool) return remoteError(-32602, 'unknown tool');
          try {
            const value = await tool.execute(params.arguments ?? {}, host);
            return respond(toolOutput(value));
          } catch { return respond(toolOutput(textResult('Plugin tool failed; inspect plugin diagnostics', true))); }
        }
        if (method.startsWith('extension/')) {
          const name = method.slice(10);
          const handler = Object.hasOwn(plugin.handlers, name) && plugin.handlers[name];
          return respond(handler ? (await handler(params, host) ?? actions.continue()) : actions.continue());
        }
        return remoteError(-32601, 'method not found');
      });
    } catch { return remoteError(-32603, 'Plugin handler failed'); }
    finally { current.active = false; }
  };
  const accept = (request, size) => {
    if (!object(request) || request.jsonrpc !== '2.0') throw new Error('invalid AEP envelope');
    if (request.method === undefined) {
      const entry = pending.get(request.id);
      if (!entry || (Object.hasOwn(request, 'result') === Object.hasOwn(request, 'error'))) throw new Error('unexpected AEP response');
      clearTimeout(entry.timer); pending.delete(request.id);
      if (Object.hasOwn(request, 'error')) entry.reject(new Error('host callback rejected'));
      else entry.resolve(request.result);
      return;
    }
    if (!bounded(request.method, 256) || request.id !== undefined && !bounded(request.id, 128) || closing || ++queued > 128 || (queuedBytes += size) > frameLimit) throw new Error('invalid or excessive AEP requests');
    if (request.id !== undefined) {
      if (seen.has(request.id) || seen.size >= 4096) throw new Error('duplicate or excessive AEP request identities');
      seen.add(request.id);
    }
    active = active.then(() => { if (!stopped) return dispatch(request); }).finally(() => { queued--; queuedBytes -= size; });
    active.catch(fail);
  };
  const onData = chunk => {
    try {
      // Iterate over a chunk rather than accumulating an unbounded stream.
      let start = 0;
      for (let i = 0; i < chunk.length; i++) {
        if (chunk[i] !== 10) continue;
        if (buffer.length + i - start > frameLimit) throw new Error('AEP frame limit exceeded');
        const bytes = Buffer.concat([buffer, chunk.subarray(start, i)]);
        const line = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
        buffer = Buffer.alloc(0); start = i + 1;
        accept(JSON.parse(line), bytes.length);
      }
      if (buffer.length + chunk.length - start > frameLimit) throw new Error('AEP frame limit exceeded');
      buffer = Buffer.concat([buffer, chunk.subarray(start)]);
    } catch (error) { fail(error); }
  };
  const onEnd = () => { if (!stopped) fail(new Error('AEP input ended before shutdown')); };
  const onAbort = () => fail(signal.reason ?? new Error('plugin aborted'));
  input.on('data', onData); input.on('end', onEnd); input.on('error', fail); output.on('error', fail);
  signal?.addEventListener('abort', onAbort, { once: true });
  try { await finished; }
  finally {
    input.off('data', onData); input.off('end', onEnd); input.off('error', fail); output.off('error', fail); input.pause();
    signal?.removeEventListener('abort', onAbort);
    for (const entry of pending.values()) { clearTimeout(entry.timer); entry.reject(new Error('AEP stopped')); }
    pending.clear();
    lifetime.abort(new Error('AEP stopped'));
    // Cooperating handlers observe host.signal. Uncooperative native author
    // code remains owned by the Go subprocess supervisor, which kills/joins it.
    await active.catch(() => {});
    await writes.catch(() => {});
  }
}
