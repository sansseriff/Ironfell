import { access, stat } from 'node:fs/promises';
import { constants } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { randomBytes } from 'node:crypto';
import { createServer } from 'vite';
import { createAgentBridge } from './agent-bridge/server';
import { codexRuntime, claudeRuntime } from './agent-bridge/runtimes';

export async function resolveCodex(explicit = process.env.IRON_CODEX_BIN, searchPath = process.env.PATH ?? '', home = homedir()): Promise<string> {
  const onPath = (name: string) => searchPath.split(delimiter).filter(Boolean).map(directory => resolve(directory, name));
  const candidates = explicit
    ? (isAbsolute(explicit) || explicit.includes('/') ? [resolve(explicit)] : onPath(explicit))
    : [...onPath('codex'), join(home, '.codex/plugins/.plugin-appserver/codex-cli/bin/codex')];
  for (const candidate of candidates) {
    try {
      await access(candidate, constants.X_OK);
      if ((await stat(candidate)).isFile()) return candidate;
    } catch {}
  }
  throw new Error(explicit
    ? `Cannot execute IRON_CODEX_BIN=${explicit}. Point it to your Codex CLI executable.`
    : 'Codex CLI not found. Install it or set IRON_CODEX_BIN to its executable, then run this command again.');
}

export async function startAgentDev(options: { root?: string; codexBin?: string; appPort?: number; bridgePort?: number; demo?: boolean } = {}) {
  const root = options.root ?? fileURLToPath(new URL('..', import.meta.url));
  const codexBin = await resolveCodex(options.codexBin);
  const path = `/Ironfell/${options.demo ? '?demo' : '?scope=last'}`;
  const vite = await createServer({ root, clearScreen: false, server: { host: '127.0.0.1', port: options.appPort ?? 5175, strictPort: true, open: false } });
  let bridge: ReturnType<typeof createAgentBridge> | undefined;
  let stopping: Promise<void> | undefined;
  const stop = () => stopping ??= (async () => {
    if (bridge) {
      const closed = bridge.server.listening ? once(bridge.server, 'close') : Promise.resolve();
      bridge.stop();
      await closed;
    }
    await vite.close();
  })();
  try {
    await vite.listen();
    const appPort = (vite.httpServer!.address() as { port: number }).port;
    const origin = `http://127.0.0.1:${appPort}`;
    bridge = createAgentBridge({
      token: randomBytes(24).toString('hex'),
      origins: [origin, `http://localhost:${appPort}`],
      allowPairing: true,
      runtimes: { codex: codexRuntime(codexBin), claude: claudeRuntime() },
    });
    await new Promise<void>((resolve, reject) => {
      bridge!.server.once('error', reject);
      bridge!.server.listen(options.bridgePort ?? 4318, '127.0.0.1', resolve);
    });
    const bridgePort = (bridge.server.address() as { port: number }).port;
    return { url: `${origin}${path}`, endpoint: `http://127.0.0.1:${bridgePort}`, codexBin, vite, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  if (args.includes('--help')) {
    console.log('bun run dev:agent [--demo] [--open]\nStarts Iron + the local Codex bridge. Ctrl+C stops both.\n--demo: ephemeral demo instead of your last saved scope\n--open: open the browser after both services are ready\nIRON_CODEX_BIN: optional explicit Codex executable');
  } else {
    try {
      if (args.some(arg => !['--demo', '--open'].includes(arg))) throw new Error('Unknown option. Run bun run dev:agent --help.');
      if (process.env.IRON_AGENT_PORT && process.env.IRON_AGENT_PORT !== '4318') throw new Error('dev:agent uses bridge port 4318 for automatic pairing. Unset IRON_AGENT_PORT, or use separate dev and agent:serve commands for custom ports.');
      const session = await startAgentDev({ demo: args.includes('--demo') });
      console.log(`\nIron + Codex ready\n  App:    ${session.url}\n  Bridge: ${session.endpoint}\n  Codex:  ${session.codexBin}\n\nChoose Codex in Model or Lesson. Pairing is automatic; no token to paste.\nUses your local Codex login. If sign-in is needed, run:\n  ${JSON.stringify(session.codexBin)} login\n${args.includes('--demo') ? 'Demo changes are temporary; export anything you want to keep.\n' : ''}Ctrl+C stops both services.\n`);
      for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, () => { void session.stop().catch(error => { console.error(error); process.exitCode = 1; }); });
      if (args.includes('--open')) {
        session.vite.config.server.open = new URL(session.url).pathname + new URL(session.url).search;
        session.vite.openBrowser();
      }
    } catch (error) {
      console.error(`Could not start Iron + Codex: ${error instanceof Error ? error.message : String(error)}\nIf a port is occupied, stop that server in its terminal first. No existing server was stopped.`);
      process.exitCode = 1;
    }
  }
}
