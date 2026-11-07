import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { nativeAgentEnvironment, startNativeAgent } from '../scripts/native-agent';
import { claudeEnv } from '../scripts/agent-bridge/runtimes';
import { discoverNativeAgent } from '../src-ui/model/native-pairing';

const fixture = fileURLToPath(new URL('fixtures/native-agent-child.ts', import.meta.url));

test('native launcher passes explicit frame-mode overrides through while keeping the second Claude profile', () => {
  const env = nativeAgentEnvironment({ IRON_NATIVE_RENDER_MODE: 'sync', IRON_NATIVE_PACING: 'adaptive', IRON_NATIVE_FRAME_LATENCY: '2' }, '/home/test');
  assert.equal(env.IRON_NATIVE_RENDER_MODE, 'sync');
  assert.equal(env.IRON_NATIVE_PACING, 'adaptive');
  assert.equal(env.IRON_NATIVE_FRAME_LATENCY, '2');
  assert.equal(env.CLAUDE_CONFIG_DIR, '/home/test/.claude_2nd');
});

test('native launcher selects the second Claude profile, regardless of a parent default profile or API billing', () => {
  const env = nativeAgentEnvironment({ CLAUDE_CONFIG_DIR: '/first', IRON_CLAUDE_AUTH: 'api-key', ANTHROPIC_API_KEY: 'not-a-real-key' }, '/home/test');
  assert.equal(env.CLAUDE_CONFIG_DIR, '/home/test/.claude_2nd');
  assert.equal(env.IRON_CLAUDE_AUTH, 'subscription');
  assert.equal(claudeEnv('subscription', env).ANTHROPIC_API_KEY, undefined);
  assert.equal(nativeAgentEnvironment({ IRON_CLAUDE_CONFIG_DIR: '/chosen' }).CLAUDE_CONFIG_DIR, '/chosen');
});

test('native pairing only accepts a validated IPC connection, with safe manual fallback', async () => {
  const connection = { endpoint: 'http://127.0.0.1:4318', token: 'x'.repeat(48), provider: 'claude' };
  assert.deepEqual(await discoverNativeAgent(async command => { assert.equal(command, 'agent_connection'); return connection; }), connection);
  for (const value of [null, {}, { ...connection, endpoint: 'https://example.com' }, { ...connection, token: 'short' }, { ...connection, provider: 'shell' }]) {
    assert.equal(await discoverNativeAgent(async () => value), null);
  }
  assert.equal(await discoverNativeAgent(async () => { throw new Error('older native build'); }), null);
});

test('native launcher passes the profile and private connection, authenticates Claude requests and closes the bridge on app exit', async () => {
  const profile = await mkdtemp(join(tmpdir(), 'iron-native-agent-'));
  const session = await startNativeAgent({
    env: { ...process.env, IRON_CLAUDE_CONFIG_DIR: profile, IRON_TEST_PROFILE: profile, ANTHROPIC_API_KEY: 'must-not-reach-claude', IRON_TEST_ADAPTIVE: '1', IRON_NATIVE_RENDER_MODE: 'sync', IRON_NATIVE_PACING: 'adaptive', IRON_NATIVE_FRAME_LATENCY: '2' },
    claudeBin: fixture, codexBin: fixture, bridgePort: 0, command: [process.execPath, fixture],
  });
  try {
    assert.equal(await session.done, 0);
    await assert.rejects(fetch(`${session.endpoint}/models`));
  } finally { await session.stop(); await rm(profile, { recursive: true, force: true }); }
});

test('native launcher stops a running child and bridge together', async () => {
  const profile = await mkdtemp(join(tmpdir(), 'iron-native-agent-'));
  const session = await startNativeAgent({ env: { ...process.env, IRON_CLAUDE_CONFIG_DIR: profile }, claudeBin: fixture, codexBin: fixture, bridgePort: 0, command: [process.execPath, '-e', 'setInterval(() => {}, 1000)'] });
  try { await session.stop(); await session.done; await assert.rejects(fetch(session.endpoint)); }
  finally { await rm(profile, { recursive: true, force: true }); }
});

test('native launcher rejects missing profiles and occupied ports without launching an app or killing the owner', async () => {
  const profile = await mkdtemp(join(tmpdir(), 'iron-native-agent-'));
  const owner = createServer();
  owner.listen(0, '127.0.0.1');
  await once(owner, 'listening');
  try {
    await assert.rejects(startNativeAgent({ env: { IRON_CLAUDE_CONFIG_DIR: join(profile, 'missing') } }), /ENOENT/);
    await assert.rejects(startNativeAgent({ env: { IRON_CLAUDE_CONFIG_DIR: profile }, claudeBin: fixture, codexBin: fixture, bridgePort: (owner.address() as { port: number }).port, command: ['/must-not-launch'] }), (error: any) => error.code === 'EADDRINUSE');
    assert.equal(owner.listening, true);
  } finally { owner.close(); await rm(profile, { recursive: true, force: true }); }
});
