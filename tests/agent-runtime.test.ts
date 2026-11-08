import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { resolve } from 'node:path';
import { createAgentBridge, validateRequest } from '../scripts/agent-bridge/server';
import { agentLog, agentLogDir } from '../scripts/agent-bridge/log';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { codexRuntime, claudeRuntime, codexModels, claudeModels, claudeAuth, type Runtime } from '../scripts/agent-bridge/runtimes';
import { LocalRuntimeTransport } from '../src-ui/model/transport';
import { localEndpoint, type CompletionRequest } from '../src-ui/model/runtime-options';
import { ask } from '../src-ui/model/agent';
import { discoverLocalAgent } from '../src-ui/model/local-pairing';
import type { DocumentApi } from '../src-ui/model/api';

const fixture = resolve(import.meta.dir, 'fixtures/fake-agent-runtime.ts');
const token = 'iron-test-token-not-a-provider-key-12345';
const payload: CompletionRequest = { provider: 'codex', system: 'Iron manifest', messages: [{ role: 'user', content: 'add a shape' }] };

async function bridgeTest(runtime: Runtime, run: (endpoint: string) => Promise<void>, timeoutMs = 500) {
  const bridge = createAgentBridge({ token, origins: ['http://localhost:5173'], runtimes: { codex: runtime, claude: runtime }, timeoutMs });
  bridge.server.listen(0, '127.0.0.1');
  await once(bridge.server, 'listening');
  const address = bridge.server.address() as { port: number };
  try { await run(`http://127.0.0.1:${address.port}`); }
  finally { bridge.stop(); }
}

test('bridge validates loopback URLs and request envelopes', () => {
  for (const endpoint of ['https://127.0.0.1', 'http://example.com', 'http://127.0.0.1.evil', 'http://user@localhost', 'http://localhost/path']) assert.throws(() => localEndpoint(endpoint));
  assert.equal(localEndpoint('http://127.0.0.1:4318').hostname, '127.0.0.1');
  assert.deepEqual(validateRequest(payload), payload);
  for (const value of [null, {}, { ...payload, provider: 'shell' }, { ...payload, messages: [{ role: 'system', content: '' }] }, { ...payload, effort: '--model' }, { ...payload, effort: 3 }]) assert.throws(() => validateRequest(value));
  assert.equal(validateRequest({ ...payload, effort: 'xhigh' }).effort, 'xhigh');
});

test('the chosen effort reaches Codex (turn/start) and Claude Code (--effort); none leaves the default', async () => {
  const request = { ...payload, messages: [{ role: 'user' as const, content: 'effort-test' }] };
  assert.equal((await codexRuntime(fixture)({ ...request, effort: 'ultra' }, AbortSignal.timeout(3000))).text, 'effort=ultra');
  assert.equal((await codexRuntime(fixture)(request, AbortSignal.timeout(3000))).text, 'effort=none');
  const saved = process.env.ANTHROPIC_API_KEY;
  delete process.env.ANTHROPIC_API_KEY;
  try {
    assert.equal((await claudeRuntime(fixture)({ ...request, provider: 'claude', effort: 'max' }, AbortSignal.timeout(3000))).text, 'effort=max');
    assert.equal((await claudeRuntime(fixture)({ ...request, provider: 'claude' }, AbortSignal.timeout(3000))).text, 'effort=none');
  } finally { if (saved !== undefined) process.env.ANTHROPIC_API_KEY = saved; }
});

test('local pairing requires opt-in, exact trusted loopback Origin and preflight header', async () => {
  const origin = 'http://localhost:5173';
  const bridge = createAgentBridge({ token, origins: [origin, 'https://example.com'], allowPairing: true });
  bridge.server.listen(0, '127.0.0.1');
  await once(bridge.server, 'listening');
  const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
  const headers = { Origin: origin, 'Content-Type': 'application/json', 'X-Iron-Local-Pairing': '1' };
  try {
    const response = await fetch(`${endpoint}/pair`, { method: 'POST', headers });
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('Cache-Control'), 'no-store');
    assert.equal(response.headers.get('Access-Control-Allow-Origin'), origin);
    assert.deepEqual(await response.json(), { token });
    for (const originValue of ['', 'null', 'http://localhost:9999', 'https://example.com', 'https://evil.test']) {
      assert.equal((await fetch(`${endpoint}/pair`, { method: 'POST', headers: { ...headers, Origin: originValue } })).status, 403);
    }
    assert.equal((await fetch(`${endpoint}/pair`, { headers })).status, 403);
    assert.equal((await fetch(`${endpoint}/pair`, { method: 'POST', headers: { ...headers, 'X-Iron-Local-Pairing': '' } })).status, 403);
    assert.equal((await fetch(`${endpoint}/pair`, { method: 'POST', headers: { ...headers, 'Content-Type': 'text/plain' } })).status, 415);
    assert.equal((await fetch(`${endpoint}/complete`, { method: 'POST', headers, body: JSON.stringify(payload) })).status, 401);
    const preflight = await fetch(`${endpoint}/pair`, { method: 'OPTIONS', headers: { Origin: origin } });
    assert.match(preflight.headers.get('Access-Control-Allow-Headers')!, /X-Iron-Local-Pairing/);
    const fetcher = ((url, options) => fetch(url, { ...options, headers: { ...options?.headers, Origin: origin } })) as typeof fetch;
    assert.equal(await discoverLocalAgent(endpoint, fetcher), token);
  } finally { bridge.stop(); }
  await bridgeTest(async () => ({ text: 'unused' }), async endpoint => {
    assert.equal((await fetch(`${endpoint}/pair`, { method: 'POST', headers })).status, 401);
  });
});

test('pairing discovery is memory-only, loopback-only and fails gracefully', async () => {
  let calls = 0;
  const fetcher = (async (url, options) => {
    calls++;
    assert.equal(String(url), 'http://127.0.0.1:4318/pair');
    assert.equal(options?.method, 'POST');
    assert.equal(options?.cache, 'no-store');
    assert.equal(options?.credentials, 'omit');
    assert.equal(options?.redirect, 'error');
    assert.equal(options?.referrerPolicy, 'no-referrer');
    assert.equal(new Headers(options?.headers).get('X-Iron-Local-Pairing'), '1');
    return Response.json({ token });
  }) as typeof fetch;
  assert.equal(await discoverLocalAgent('http://127.0.0.1:4318', fetcher), token);
  for (const endpoint of ['https://localhost', 'http://evil.test', 'http://localhost/?token=secret']) {
    assert.equal(await discoverLocalAgent(endpoint, fetcher), null);
  }
  assert.equal(calls, 1);
  for (const value of [null, {}, { token: 'short' }, { token: 123 }, { token: 'x'.repeat(1025) }]) {
    assert.equal(await discoverLocalAgent('http://127.0.0.1:4318', (async () => Response.json(value)) as typeof fetch), null);
  }
  assert.equal(await discoverLocalAgent('http://127.0.0.1:4318', (async () => new Response('', { status: 403 })) as typeof fetch), null);
  assert.equal(await discoverLocalAgent('http://127.0.0.1:4318', (async () => { throw new Error('offline'); }) as typeof fetch), null);
});

test('authenticated transport preserves prompts, model and provider without API credentials', async () => {
  await bridgeTest(async request => {
    assert.deepEqual(request, { ...payload, model: 'chosen-model' });
    assert.equal('apiKey' in request, false);
    return { text: '<edit/>', usage: { tokens: 4 } };
  }, async endpoint => {
    const transport = new LocalRuntimeTransport({ provider: 'codex', endpoint, token, model: 'chosen-model' });
    assert.deepEqual(await transport.complete(payload.system, payload.messages), { text: '<edit/>', usage: { tokens: 4 } });
    await assert.rejects(() => new LocalRuntimeTransport({ provider: 'codex', endpoint, token: 'bad' }).complete(payload.system, payload.messages), /token/);
  });
});

test('bridge rejects untrusted origins, Host rebinding, missing tokens and oversized bodies', async () => {
  let calls = 0;
  await bridgeTest(async () => { calls++; return { text: '<edit/>' }; }, async endpoint => {
    const post = (headers = {}, body = JSON.stringify(payload)) => fetch(`${endpoint}/complete`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}`, ...headers }, body });
    assert.equal((await post({ Origin: 'https://evil.test' })).status, 403);
    assert.equal((await post({ Host: 'evil.test:4318' })).status, 403);
    assert.equal((await post({ Authorization: '' })).status, 401);
    assert.equal((await post({ 'Content-Type': 'text/plain' })).status, 415);
    assert.equal((await post({}, 'x'.repeat(1_000_001))).status, 400);
    assert.equal(calls, 0);
    const preflight = await fetch(`${endpoint}/complete`, { method: 'OPTIONS', headers: { Origin: 'http://localhost:5173' } });
    assert.equal(preflight.headers.get('Access-Control-Allow-Origin'), 'http://localhost:5173');
  });
});

test('bridge bounds concurrency and aborts timed-out runtimes', async () => {
  let entered!: () => void;
  const started = new Promise<void>(resolve => { entered = resolve; });
  await bridgeTest(async (_request, signal) => {
    entered();
    return await new Promise((_, reject) => signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true }));
  }, async endpoint => {
    const post = () => fetch(`${endpoint}/complete`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` }, body: JSON.stringify(payload) });
    const first = post();
    await started;
    assert.equal((await post()).status, 429);
    assert.equal((await first).status, 504);
  }, 100);
});

test('disconnecting a browser request cancels its runtime', async () => {
  let entered!: () => void;
  let aborted!: () => void;
  const started = new Promise<void>(resolve => { entered = resolve; });
  const cancelled = new Promise<void>(resolve => { aborted = resolve; });
  await bridgeTest(async (_request, signal) => {
    entered();
    return await new Promise((_, reject) => signal.addEventListener('abort', () => { aborted(); reject(new Error('disconnected')); }, { once: true }));
  }, async endpoint => {
    const controller = new AbortController();
    const request = fetch(`${endpoint}/complete`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` }, body: JSON.stringify(payload), signal: controller.signal });
    const rejected = assert.rejects(() => request, /abort/i);
    await started;
    controller.abort();
    await rejected;
    await cancelled;
  });
});

test('Codex app-server handshake, final items, failures, approvals, exit and cancellation', async () => {
  try {
    const runtime = codexRuntime(fixture);
    const result = await runtime(payload, AbortSignal.timeout(3000));
    assert.match(result.text, /<create/);
    await assert.rejects(() => codexRuntime('/missing-iron-codex')(payload, AbortSignal.timeout(3000)), /ENOENT/);
    for (const [mode, expected] of [['failed', /fixture turn failed/], ['approval', /Unexpected runtime request/], ['exit', /Runtime exited/], ['malformed', /JSON|Unexpected/], ['timeout', /abort/i]] as const) {
      process.env.IRON_TEST_RUNTIME_MODE = mode;
      await assert.rejects(() => runtime(payload, AbortSignal.timeout(mode === 'timeout' ? 100 : 3000)), expected);
    }
  } finally { delete process.env.IRON_TEST_RUNTIME_MODE; }
});

test('Claude adapter is tool-free, uses the subscription login by default and an API key only when asked', async () => {
  const saved = { key: process.env.ANTHROPIC_API_KEY, auth: process.env.IRON_CLAUDE_AUTH, session: process.env.CLAUDE_CODE_SESSION_ID, code: process.env.CLAUDECODE };
  const restore = (k: string, v: string | undefined) => { if (v === undefined) delete process.env[k]; else process.env[k] = v; };
  try {
    delete process.env.IRON_CLAUDE_AUTH;
    assert.equal(claudeAuth(), 'subscription');
    assert.throws(() => claudeAuth('keychain'), /subscription|api-key/);
    // Subscription: safe mode, and neither a key nor a surrounding Claude
    // Code session's variables reach the child (the fixture exits 4 if so).
    process.env.ANTHROPIC_API_KEY = 'fixture-not-a-real-key';
    process.env.CLAUDE_CODE_SESSION_ID = 'outer-session';
    process.env.CLAUDECODE = '1';
    assert.equal((await claudeRuntime(fixture)(payload, AbortSignal.timeout(3000))).text, '<edit/>');
    // API key: bare, and only with a key.
    assert.equal((await claudeRuntime(fixture, 'api-key')(payload, AbortSignal.timeout(3000))).text, '<edit/>');
    delete process.env.ANTHROPIC_API_KEY;
    await assert.rejects(() => claudeRuntime(fixture, 'api-key')(payload, AbortSignal.timeout(3000)), /ANTHROPIC_API_KEY/);
  } finally {
    restore('ANTHROPIC_API_KEY', saved.key); restore('IRON_CLAUDE_AUTH', saved.auth);
    restore('CLAUDE_CODE_SESSION_ID', saved.session); restore('CLAUDECODE', saved.code);
  }
});

test('bridge lists each runtime\'s models behind the token, Codex from its app-server', async () => {
  assert.deepEqual(await codexModels(fixture)(AbortSignal.timeout(3000)), [
    { id: 'fixture-default', label: 'Fixture Default', default: true, efforts: ['low', 'high', 'ultra'] },
    { id: 'fixture-other', label: 'Fixture Other' },
  ]);
  assert.deepEqual((await claudeModels(AbortSignal.timeout(1000))).map(m => m.id), ['opus', 'sonnet', 'haiku', 'fable']);
  const token = 'm'.repeat(32);
  const bridge = createAgentBridge({ token, origins: ['http://localhost:5173'], runtimes: { codex: codexRuntime(fixture), claude: claudeRuntime(fixture) }, models: { codex: codexModels(fixture), claude: claudeModels } });
  bridge.server.listen(0, '127.0.0.1');
  await once(bridge.server, 'listening');
  const url = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}/models`;
  const ask = (provider: string, auth = `Bearer ${token}`) => fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: auth }, body: JSON.stringify({ provider }) });
  try {
    assert.equal((await ask('codex', 'Bearer wrong')).status, 401);
    assert.deepEqual((await (await ask('codex')).json()).models.map((m: { id: string }) => m.id), ['fixture-default', 'fixture-other']);
    assert.equal((await (await ask('claude')).json()).models[0].id, 'opus');
    assert.equal((await ask('gemini')).status, 400);
  } finally { bridge.stop(); }
});

function documentFixture() {
  let version = 1;
  let applyCalls = 0;
  const nodes: Record<string, { type: string; name: string; x: number; color: string }> = {};
  const api = {
    version: () => version,
    schema: () => ({ facets: { transform2d: { x: 'number', y: 'number' }, size: { w: 'number', h: 'number' }, fill: { color: 'color' } }, types: { rect: { facets: [{ kind: 'transform2d', required: true }, { kind: 'size', required: true }, { kind: 'fill', required: true }], default_host: '2d', children: 'none' } } }),
    view: () => `<document version="${version}"><composition id="#1" name="test" notation="2d">${Object.entries(nodes).map(([id, node]) => `<rect id="${id}" name="${node.name}" x="${node.x}"/>`).join('')}</composition></document>`,
    selection: () => [], nextIds: () => ['#fixture'], nextRelationIds: () => [], place: () => ['a0'],
    apply: (transaction: any, expected: number) => {
      applyCalls++;
      assert.equal(transaction.actor, 'model');
      assert.equal(expected, version);
      for (const operation of transaction.ops) {
        if (operation.t === 'create') nodes[operation.id] = { type: operation.ty, name: 'agent-test-shape', x: 100, color: '#ff2200' };
        if (operation.t === 'set' && operation.path === 'transform2d.x') nodes[operation.id].x = operation.slot;
      }
      version++;
      return { ok: true, version };
    },
  } as unknown as DocumentApi;
  return { api, nodes, changed: () => version++, calls: () => applyCalls };
}

test('agent edit pipeline sends model-authored shape creation through the transaction API', async () => {
  const fixtureDoc = documentFixture();
  await bridgeTest(codexRuntime(fixture), async endpoint => {
    const transport = new LocalRuntimeTransport({ provider: 'codex', endpoint, token });
    const result = await ask(fixtureDoc.api, transport, 'add a rectangle');
    assert.equal(result.ok, true, JSON.stringify(result));
    assert.equal(fixtureDoc.calls(), 1);
    assert.equal(result.outcome?.aliases?.$shape, '#fixture');
    assert.equal(fixtureDoc.nodes['#fixture'].type, 'rect');
    const edited = await ask(fixtureDoc.api, transport, 'edit-test-shape');
    assert.equal(edited.ok, true, JSON.stringify(edited));
    assert.equal(fixtureDoc.nodes['#fixture'].x, 240);
    assert.equal(fixtureDoc.calls(), 2);
  }, 3000);
});

test('late replies cannot edit a document changed during generation', async () => {
  const fixtureDoc = documentFixture();
  const result = await ask(fixtureDoc.api, { name: 'fixture', complete: async () => { fixtureDoc.changed(); return { text: '<edit><create type="rect" parent="#1"/></edit>' }; } }, 'add a rectangle');
  assert.equal(result.ok, false);
  assert.match(result.error!, /document changed/);
  assert.equal(fixtureDoc.calls(), 0);
});

test('invalid model edits get one correction attempt and no partial transaction', async () => {
  const fixtureDoc = documentFixture();
  let calls = 0;
  const result = await ask(fixtureDoc.api, { name: 'fixture', complete: async (_system, messages) => {
    calls++;
    if (calls === 1) return { text: '<edit><create type="not-a-type" parent="#1"/></edit>' };
    assert.match(messages.at(-1)!.content, /edit-failed/);
    return { text: '<edit><create id="$shape" type="rect" parent="#1" x="20"/></edit>' };
  } }, 'add a rectangle');
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.equal(result.attempts, 2);
  assert.equal(fixtureDoc.calls(), 1);
});

test('missing edit blocks and runtime errors do not mutate or silently fall back', async () => {
  for (const complete of [async () => ({ text: 'I cannot do that' }), async () => { throw new Error('runtime unavailable'); }]) {
    const fixtureDoc = documentFixture();
    const result = await ask(fixtureDoc.api, { name: 'fixture', complete }, 'add a rectangle');
    assert.equal(result.ok, false);
    assert.equal(result.attempts, 1);
    assert.equal(fixtureDoc.calls(), 0);
  }
});

test('chat explicitly accepts prose-only replies with bounded previous conversation and no mutation', async () => {
  const fixtureDoc = documentFixture();
  const history = [{ role: 'user' as const, content: 'What is the rectangle?' }, { role: 'assistant' as const, content: 'A shape in the composition.' }];
  const result = await ask(fixtureDoc.api, { name: 'fixture', complete: async (system, messages) => {
    assert.match(system, /CONVERSATION/);
    assert.deepEqual(messages.slice(0, 2), history);
    assert.match(messages.at(-1)!.content, /REQUEST\nExplain it/);
    return { text: '## The rectangle\n\nA **2D** shape.\n\n::view{show=test}' };
  } }, 'Explain it', { history, allowTextReply: true });
  assert.equal(result.ok, true);
  assert.equal(result.edit, null);
  assert.equal(fixtureDoc.calls(), 0);
  assert.match(result.note, /2D/);
});

test('chat renders edit examples as prose without applying fenced or inline code', async () => {
  const fixtureDoc = documentFixture();
  for (const text of ['Here is an example:\n\n```xml\n<edit><create type="rect" parent="#1"/></edit>\n```', 'Example: `<edit><create type="rect" parent="#1"/></edit>`', 'Example:\n\n    <edit><create type="rect" parent="#1"/></edit>', '> <edit><create type="rect" parent="#1"/></edit>']) {
    const result = await ask(fixtureDoc.api, { name: 'fixture', complete: async () => ({ text }) }, 'Explain edit syntax', { allowTextReply: true });
    assert.equal(result.ok, true);
    assert.equal(result.edit, null);
    assert.equal(fixtureDoc.calls(), 0);
  }
});

test('a prose-only correction cannot turn a rejected edit into a reported success', async () => {
  const fixtureDoc = documentFixture();
  let calls = 0;
  const result = await ask(fixtureDoc.api, { name: 'fixture', complete: async () => ({ text: ++calls === 1 ? '<edit><create type="not-a-type"/></edit>' : 'Done.' }) }, 'Make a shape', { allowTextReply: true });
  assert.equal(result.ok, false);
  assert.equal(result.attempts, 2);
  assert.equal(fixtureDoc.calls(), 0);
});

// ---- Streaming (plans/specs/agent-streaming.md) --------------------------------

test('Codex and Claude adapters stream reply and reasoning deltas that add up to the reply', async () => {
  const codexEvents: any[] = [];
  const codex = await codexRuntime(fixture)(payload, AbortSignal.timeout(3000), event => codexEvents.push(event));
  assert.equal(codexEvents.filter(e => e.type === 'text').map(e => e.delta).join(''), codex.text);
  assert.deepEqual(codexEvents.filter(e => e.type === 'reasoning').map(e => e.delta), ['Reading the request.']);
  assert.deepEqual(codexEvents.filter(e => e.type === 'usage'), [{ type: 'usage', output: 42, thinking: 30 }]);
  const saved = process.env.ANTHROPIC_API_KEY;
  delete process.env.ANTHROPIC_API_KEY;
  try {
    const claudeEvents: any[] = [];
    const claude = await claudeRuntime(fixture)(payload, AbortSignal.timeout(3000), event => claudeEvents.push(event));
    assert.equal(claude.text, '<edit/>');
    assert.equal(claudeEvents.filter(e => e.type === 'text').map(e => e.delta).join(''), '<edit/>');
    assert.deepEqual(claudeEvents.filter(e => e.type === 'reasoning').map(e => e.delta), ['Nothing to change.']);
    // Claude Code's running estimate of hidden thinking, then the exact count.
    assert.deepEqual(claudeEvents.filter(e => e.type === 'thinking-tokens' || e.type === 'usage'), [
      { type: 'thinking-tokens', estimated: 50 }, { type: 'thinking-tokens', estimated: 120 }, { type: 'usage', output: 130, thinking: 125 },
    ]);
  } finally { if (saved !== undefined) process.env.ANTHROPIC_API_KEY = saved; }
});

test('the bridge streams NDJSON deltas to the transport, and output keeps a long turn alive', async () => {
  // Idle limit 100 ms, but a delta every 40 ms for 400 ms: the turn completes.
  await bridgeTest(async (_request, signal, onEvent) => {
    for (let i = 0; i < 10; i++) {
      if (signal.aborted) throw new Error('aborted');
      onEvent?.({ type: i === 0 ? 'reasoning' : 'text', delta: i === 0 ? 'plan' : `${i}` });
      await new Promise(resolve => setTimeout(resolve, 40));
    }
    onEvent?.({ type: 'thinking-tokens', estimated: 7 });
    onEvent?.({ type: 'usage', output: 12, thinking: 3 });
    return { text: '123456789' };
  }, async endpoint => {
    const events: any[] = [];
    const transport = new LocalRuntimeTransport({ provider: 'codex', endpoint, token });
    const result = await transport.complete(payload.system, payload.messages, { onEvent: event => events.push(event) });
    assert.equal(result.text, '123456789');
    assert.deepEqual(events[0], { kind: 'reasoning', delta: 'plan' });
    assert.equal(events.filter(e => e.kind === 'text').map(e => e.delta).join(''), '123456789');
    assert.deepEqual(events.slice(-2), [{ kind: 'thinking-tokens', estimated: 7 }, { kind: 'usage', output: 12, thinking: 3 }]);
  }, 100);
});

test('a streamed runtime that goes quiet times out with an error line; Stop aborts the runtime', async () => {
  await bridgeTest(async (_request, signal) => new Promise((_, reject) => signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true })), async endpoint => {
    const transport = new LocalRuntimeTransport({ provider: 'codex', endpoint, token });
    await assert.rejects(() => transport.complete(payload.system, payload.messages, { onEvent: () => {} }), /timed out/);
  }, 100);
  let aborted!: () => void;
  const cancelled = new Promise<void>(resolve => { aborted = resolve; });
  await bridgeTest(async (_request, signal, onEvent) => {
    onEvent?.({ type: 'text', delta: 'Hel' });
    return await new Promise((_, reject) => signal.addEventListener('abort', () => { aborted(); reject(new Error('aborted')); }, { once: true }));
  }, async endpoint => {
    const controller = new AbortController();
    const transport = new LocalRuntimeTransport({ provider: 'codex', endpoint, token });
    const running = transport.complete(payload.system, payload.messages, { signal: controller.signal, onEvent: () => controller.abort() });
    await assert.rejects(() => running, /abort/i);
    await cancelled;
  }, 3000);
});

test('ask reports its phases and passes deltas on; a stopped turn applies nothing', async () => {
  const fixtureDoc = documentFixture();
  const events: any[] = [];
  const streaming = { name: 'fixture', complete: async (_s: string, _m: unknown, live: any = {}) => {
    live.onEvent?.({ kind: 'text', delta: '<edit><create id="$shape" type="rect" parent="#1"/></edit>' });
    return { text: '<edit><create id="$shape" type="rect" parent="#1"/></edit>' };
  } };
  const result = await ask(fixtureDoc.api, streaming, 'add a rectangle', {}, { onEvent: event => events.push(event) });
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.deepEqual(events.filter(e => e.kind === 'phase').map(e => `${e.phase}:${e.attempt}`), ['reading:1', 'thinking:1', 'applying:1']);
  assert.equal(events.filter(e => e.kind === 'text').length, 1);
  // Stop while the model is answering: the reply that still arrives is not applied.
  const controller = new AbortController();
  const stopped = await ask(fixtureDoc.api, { name: 'fixture', complete: async () => { controller.abort(); return { text: '<edit><create id="$s" type="rect" parent="#1"/></edit>' }; } }, 'add another', {}, { signal: controller.signal });
  assert.equal(stopped.ok, false);
  assert.equal(stopped.error, 'stopped');
  assert.equal(fixtureDoc.calls(), 1);
});

// ---- Agent log (scripts/agent-bridge/log.ts) ---------------------------------------

test('the agent log keeps each call (prompt once, reply, timing, usage) and each turn outcome', async () => {
  assert.equal(agentLogDir('/repo', {}), '/repo/.agents/agent-logs');
  assert.equal(agentLogDir('/repo', { IRON_AGENT_LOG: '0' }), null);
  assert.equal(agentLogDir('/repo', { IRON_AGENT_LOG: '/elsewhere' }), '/elsewhere');
  const dir = await mkdtemp(join(tmpdir(), 'iron-agent-log-'));
  const log = agentLog(dir);
  const bridge = createAgentBridge({ token, origins: ['http://localhost:5173'], log, runtimes: {
    codex: async (_request, _signal, onEvent) => { onEvent?.({ type: 'reasoning', delta: 'Weighing it.' }); onEvent?.({ type: 'usage', output: 9, thinking: 4 }); return { text: '<edit/>' }; },
    claude: async () => { throw new Error('claude is down'); },
  } });
  bridge.server.listen(0, '127.0.0.1');
  await once(bridge.server, 'listening');
  const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
  try {
    const transport = new LocalRuntimeTransport({ provider: 'codex', endpoint, token });
    await transport.complete(payload.system, payload.messages, { turn: 'turn-1', onEvent: () => {} });
    await transport.complete(payload.system, payload.messages, { turn: 'turn-1' });
    await assert.rejects(() => new LocalRuntimeTransport({ provider: 'claude', endpoint, token }).complete(payload.system, payload.messages, { turn: 'turn-2' }), /claude is down/);
    await transport.log({ turn: 'turn-1', ok: true, trace: [{ attempt: 1, kind: 'edit', ok: true }] });
    const bad = await fetch(`${endpoint}/log`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` }, body: JSON.stringify({ turn: '../etc' }) });
    assert.equal(bad.status, 400);
    const unauthorised = await fetch(`${endpoint}/log`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ turn: 'x' }) });
    assert.equal(unauthorised.status, 401);
    await log.flushed();
    const [file] = await readdir(dir);
    const records = (await readFile(join(dir, file), 'utf8')).trim().split('\n').map(line => JSON.parse(line));
    // The system prompt once, three calls naming it, the page's turn record.
    assert.deepEqual(records.map(r => `${r.kind}:${r.turn ?? ''}`), ['system:', 'call:turn-1', 'call:turn-1', 'call:turn-2', 'turn:turn-1']);
    const [system, first, , failed, turn] = records;
    assert.equal(system.text, payload.system);
    assert.equal(first.system, system.hash);
    assert.deepEqual([first.reply, first.reasoning, first.usage, first.messages], ['<edit/>', 'Weighing it.', { output: 9, thinking: 4 }, payload.messages]);
    assert.equal(typeof first.ms, 'number');
    assert.match(failed.error, /claude is down/);
    assert.deepEqual(turn.trace, [{ attempt: 1, kind: 'edit', ok: true }]);
  } finally { bridge.stop(); await rm(dir, { recursive: true, force: true }); }
});

test('ask sends the turn id with each call and logs each reply\'s fate once the turn ends', async () => {
  const fixtureDoc = documentFixture();
  const turns: string[] = [];
  const records: any[] = [];
  let calls = 0;
  const transport = {
    name: 'fixture',
    complete: async (_s: string, _m: unknown, live: any = {}) => {
      turns.push(live.turn);
      calls++;
      return { text: calls === 1 ? '<edit><create type="not-a-type" parent="#1"/></edit>' : '<edit><create id="$shape" type="rect" parent="#1" x="20"/></edit>' };
    },
    log: async (record: any) => { records.push(record); },
  };
  const result = await ask(fixtureDoc.api, transport, 'add a rectangle', { turnId: 'turn-7' });
  assert.equal(result.ok, true);
  assert.deepEqual(turns, ['turn-7', 'turn-7']);
  assert.deepEqual(result.trace?.map(step => `${step.kind}:${'ok' in step ? step.ok : ''}`), ['edit:false', 'edit:true']);
  assert.equal(records.length, 1);
  assert.equal(records[0].turn, 'turn-7');
  assert.equal(records[0].attempts, 2);
  assert.match(records[0].trace[0].failure, /edit-failed/);
  // No turn id (a script's __iron.ask): nothing is logged.
  calls = 1;
  await ask(fixtureDoc.api, transport, 'add another');
  assert.equal(records.length, 1);
});
