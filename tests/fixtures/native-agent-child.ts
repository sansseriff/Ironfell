#!/usr/bin/env bun
import assert from 'node:assert/strict';

assert.equal(process.env.CLAUDE_CONFIG_DIR, process.env.IRON_TEST_PROFILE);
if (process.argv.includes('--safe-mode')) {
  assert.equal(process.env.ANTHROPIC_API_KEY, undefined);
  assert.equal(process.env.ANTHROPIC_AUTH_TOKEN, undefined);
  for await (const chunk of process.stdin) { void chunk; }
  process.stdout.write(JSON.stringify({ result: 'alternate-profile-confirmed', is_error: false }));
} else {
  assert.equal(process.env.IRON_NATIVE_AGENT_PROVIDER, 'claude');
  if (process.env.IRON_TEST_ADAPTIVE === '1') {
    assert.equal(process.env.IRON_NATIVE_RENDER_MODE, 'sync');
    assert.equal(process.env.IRON_NATIVE_PACING, 'adaptive');
    assert.equal(process.env.IRON_NATIVE_FRAME_LATENCY, '2');
  }
  const endpoint = process.env.IRON_NATIVE_AGENT_ENDPOINT!;
  const headers = { Origin: 'tauri://localhost', 'Content-Type': 'application/json', Authorization: `Bearer ${process.env.IRON_NATIVE_AGENT_TOKEN}` };
  const response = await fetch(`${endpoint}/complete`, { method: 'POST', headers, body: JSON.stringify({ provider: 'claude', system: 'Fixture', messages: [{ role: 'user', content: 'Verify profile' }] }) });
  assert.equal(response.status, 200);
  assert.equal((await response.json()).text, 'alternate-profile-confirmed');
  const browserPair = await fetch(`${endpoint}/pair`, { method: 'POST', headers: { Origin: 'http://localhost:5175', 'X-Iron-Local-Pairing': '1' } });
  assert.equal(browserPair.status, 403);
}
