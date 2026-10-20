// INPUT: Node test/assert and in-memory host streams using the actual author runtime.
// OUTPUT: Reverse-call liveness, handle expiry, cancellation/join and protocol boundary regressions.
// POS: Hermetic SDK tests complement the actual Go loader subprocess acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import { PassThrough } from 'node:stream';
import { definePlugin, run, actions, textResult, frameLimit } from '../index.mjs';

function fixture(plugin, options = {}) {
  const input = new PassThrough(), output = new PassThrough();
  const pending = new Map(), frames = [];
  let next = 0, bytes = '';
  const host = { input, output, frames, callback: () => ({}) };
  output.on('data', chunk => {
    bytes += chunk.toString();
    for (let end; (end = bytes.indexOf('\n')) >= 0;) {
      const frame = JSON.parse(bytes.slice(0, end)); bytes = bytes.slice(end + 1); frames.push(frame);
      if (frame.method) {
        Promise.resolve().then(() => host.callback(frame)).then(result => input.write(`${JSON.stringify({ jsonrpc: '2.0', id: frame.id, result })}\n`));
      } else { const resolve = pending.get(frame.id); pending.delete(frame.id); resolve?.(frame); }
    }
  });
  host.done = run(definePlugin({ name: 'fixture', version: '1', ...plugin }), { input, output, ...options });
  host.done.catch(() => {});
  host.ask = (method, params = {}) => {
    const id = `h-${++next}`;
    const response = new Promise(resolve => pending.set(id, resolve));
    input.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    return response;
  };
  host.initialize = async () => {
    const response = await host.ask('initialize', { protocolVersion: '0.1.0', hostCapabilities: { hostApi: ['log', 'state.get_metadata'], events: ['on_agent_start', 'before_tool_call'] } });
    assert.equal(response.error, undefined);
    return response;
  };
  host.shutdown = async () => { await host.ask('shutdown'); await host.done; assert.equal(output.destroyed, false); };
  return host;
}

test('reverse callbacks complete while the event waits and undeclared callbacks never send', { timeout: 3000 }, async () => {
  const f = fixture({ capabilities: ['host:state.get_metadata', 'host:log'], handlers: {
    on_agent_start: async (_, host) => {
      assert.equal((await host.getMetadata()).session_id, 'native');
      await assert.rejects(host.notify('not granted'), /unavailable/);
      await host.log('observed');
      return actions.continue();
    },
  } });
  f.callback = frame => frame.method === 'host/state.get_metadata' ? { metadata: { session_id: 'native' } } : {};
  await f.initialize();
  const response = await f.ask('extension/on_agent_start', { stateHandle: 's-1' });
  assert.deepEqual(response.result, actions.continue());
  assert.deepEqual(f.frames.filter(x => x.method).map(x => x.method), ['host/state.get_metadata', 'host/log']);
  assert.equal(f.frames.find(x => x.method === 'host/state.get_metadata').params.handle, 's-1');
  await f.shutdown();
});

test('asynchronous work cannot reuse an expired event handle', { timeout: 3000 }, async () => {
  let expired;
  const f = fixture({ capabilities: ['host:state.get_metadata'], handlers: {
    before_tool_call: (_, host) => {
      expired = new Promise(resolve => setTimeout(() => resolve(host.getMetadata().then(() => false, () => true)), 20));
      return actions.continue();
    },
  } });
  await f.initialize();
  await f.ask('extension/before_tool_call', { stateHandle: 's-1' });
  assert.equal(await expired, true);
  assert.equal(f.frames.filter(x => x.method).length, 0);
  await f.shutdown();
});

test('tool errors are classified and prototype names remain ordinary own keys', { timeout: 3000 }, async () => {
  const tools = JSON.parse('{"__proto__":{}}');
  tools.__proto__ = { description: 'own tool', inputSchema: { type: 'object' }, execute: () => textResult('own') };
  tools.bad = { description: 'failure', inputSchema: { type: 'object' }, execute: () => { throw new Error('private credential'); } };
  const f = fixture({ tools });
  const initialized = await f.initialize();
  assert.equal(initialized.result.tools.length, 2);
  assert.deepEqual((await f.ask('tools/call', { name: '__proto__' })).result, { content: [{ type: 'text', text: 'own' }], isError: false });
  const bad = await f.ask('tools/call', { name: 'bad' });
  assert.equal(bad.result.isError, true);
  assert.equal(JSON.stringify(bad).includes('private credential'), false);
  await f.shutdown();
});

test('abort joins a cooperating handler before runtime returns', { timeout: 3000 }, async () => {
  const controller = new AbortController();
  let entered, joined = false;
  const ready = new Promise(resolve => { entered = resolve; });
  const f = fixture({ handlers: { on_agent_start: async (_, host) => {
    entered();
    await new Promise(resolve => host.signal.addEventListener('abort', resolve, { once: true }));
    joined = true;
    return actions.continue();
  } } }, { signal: controller.signal });
  await f.initialize();
  void f.ask('extension/on_agent_start', { stateHandle: 's-1' });
  await ready;
  controller.abort(new Error('explicit cancel'));
  await assert.rejects(f.done, /explicit cancel/);
  assert.equal(joined, true);
});

test('incomplete EOF, invalid UTF8 and over-budget frames fail closed', { timeout: 3000 }, async () => {
  for (const bytes of [Buffer.from([0xff, 10]), Buffer.alloc(frameLimit + 1, 65), Buffer.from('{"jsonrpc":')]) {
    const f = fixture({});
    f.input.end(bytes);
    await assert.rejects(f.done);
    assert.equal(f.frames.length, 0);
  }
});

test('duplicate host identities never replay a tool', { timeout: 3000 }, async () => {
  let calls = 0;
  const f = fixture({ tools: { count: { description: 'counter', inputSchema: { type: 'object' }, execute: () => ++calls } } });
  await f.initialize();
  const frame = `${JSON.stringify({ jsonrpc: '2.0', id: 'h-duplicate', method: 'tools/call', params: { name: 'count' } })}\n`;
  f.input.write(frame + frame);
  await assert.rejects(f.done, /duplicate/);
  assert.ok(calls <= 1);
});

test('queued host requests have a combined byte budget', { timeout: 3000 }, async () => {
  const f = fixture({});
  await f.initialize();
  for (let i = 0; i < 3; i++) {
    f.input.write(`${JSON.stringify({ jsonrpc: '2.0', id: `h-large-${i}`, method: 'tools/list', params: { padding: 'x'.repeat(6 << 20) } })}\n`);
  }
  await assert.rejects(f.done, /excessive/);
});
