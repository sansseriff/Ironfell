import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, mkdir, writeFile, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { resolveCodex, startAgentDev } from '../scripts/dev-agent';

const fixture = resolve(import.meta.dir, 'fixtures/fake-agent-runtime.ts');

async function appFixture() {
  const root = await mkdtemp(join(tmpdir(), 'iron-dev-app-'));
  await writeFile(join(root, 'index.html'), '<html><body>Iron launcher fixture</body></html>');
  await writeFile(join(root, 'vite.config.ts'), "export default { base: '/Ironfell/', optimizeDeps: { noDiscovery: true, include: [] } };");
  return root;
}

test('launcher resolves explicit, PATH and bundled Codex without silently replacing an invalid override', async () => {
  const home = await mkdtemp(join(tmpdir(), 'iron-launcher-'));
  try {
    const bundled = join(home, '.codex/plugins/.plugin-appserver/codex-cli/bin/codex');
    await mkdir(join(bundled, '..'), { recursive: true });
    await writeFile(bundled, '#!/bin/sh\nexit 0\n');
    await chmod(bundled, 0o700);
    assert.equal(await resolveCodex('', '', home), bundled);
    assert.equal(await resolveCodex(fixture, '', home), fixture);
    assert.equal(await resolveCodex('fake-agent-runtime.ts', join(fixture, '..'), home), fixture);
    await assert.rejects(resolveCodex(join(home, 'missing'), '', home), /IRON_CODEX_BIN/);
    await chmod(bundled, 0o600);
    await assert.rejects(resolveCodex('', '', home), /Codex CLI not found/);
  } finally { await rm(home, { recursive: true, force: true }); }
});

test('one launcher serves Iron and a trusted-origin paired bridge, then closes both', async () => {
  const root = await appFixture();
  const session = await startAgentDev({ root, codexBin: fixture, appPort: 0, bridgePort: 0, demo: true });
  try {
    assert.equal(new URL(session.url).search, '?demo');
    assert.equal((await fetch(session.url)).status, 200);
    const paired = await fetch(`${session.endpoint}/pair`, {
      method: 'POST', headers: { Origin: new URL(session.url).origin, 'Content-Type': 'application/json', 'X-Iron-Local-Pairing': '1' },
    });
    assert.equal(paired.status, 200);
    assert.ok((await paired.json()).token.length >= 32);
    assert.equal((await fetch(`${session.endpoint}/pair`, {
      method: 'POST', headers: { Origin: 'http://localhost:9999', 'Content-Type': 'application/json', 'X-Iron-Local-Pairing': '1' },
    })).status, 403);
  } finally { await session.stop(); await rm(root, { recursive: true, force: true }); }
  await session.stop();
  await assert.rejects(fetch(session.url));
  await assert.rejects(fetch(`${session.endpoint}/pair`));
});

test('occupied bridge port fails startup and releases the newly started app without killing its owner', async () => {
  const root = await appFixture();
  const occupied = createServer();
  occupied.listen(0, '127.0.0.1');
  await once(occupied, 'listening');
  const port = (occupied.address() as { port: number }).port;
  const appProbe = createServer();
  appProbe.listen(0, '127.0.0.1');
  await once(appProbe, 'listening');
  const appPort = (appProbe.address() as { port: number }).port;
  const released = once(appProbe, 'close');
  appProbe.close();
  await released;
  try {
    await assert.rejects(startAgentDev({ root, codexBin: fixture, appPort, bridgePort: port }), (error: any) => error.code === 'EADDRINUSE');
    assert.equal(occupied.listening, true);
    appProbe.listen(appPort, '127.0.0.1');
    await once(appProbe, 'listening');
  } finally { appProbe.close(); occupied.close(); await rm(root, { recursive: true, force: true }); }
});
