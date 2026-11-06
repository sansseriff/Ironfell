import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { randomBytes } from 'node:crypto';
import { createServer } from 'vite';
import { createAgentBridge } from './agent-bridge/server';
import { codexRuntime, claudeRuntime, codexModels, claudeModels, claudeAuth } from './agent-bridge/runtimes';
import { resolveClaude, resolveCodex } from './agent-bridge/resolve';

export { resolveCodex, resolveClaude } from './agent-bridge/resolve';

export async function startAgentDev(options: { root?: string; codexBin?: string; claudeBin?: string; appPort?: number; bridgePort?: number; demo?: boolean } = {}) {
  const root = options.root ?? fileURLToPath(new URL('..', import.meta.url));
  const codexBin = await resolveCodex(options.codexBin);
  // Claude Code is optional: without it, Codex still works and Claude says why it cannot.
  const claudeBin = await resolveClaude(options.claudeBin).catch(() => null);
  const auth = claudeAuth();
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
      runtimes: { codex: codexRuntime(codexBin), claude: claudeRuntime(claudeBin ?? 'claude', auth) },
      models: { codex: codexModels(codexBin), claude: claudeModels },
    });
    await new Promise<void>((resolve, reject) => {
      bridge!.server.once('error', reject);
      bridge!.server.listen(options.bridgePort ?? 4318, '127.0.0.1', resolve);
    });
    const bridgePort = (bridge.server.address() as { port: number }).port;
    return { url: `${origin}${path}`, endpoint: `http://127.0.0.1:${bridgePort}`, codexBin, claudeBin, claudeAuth: auth, vite, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  if (args.includes('--help')) {
    console.log('bun run dev:agent [--demo] [--open]\nStarts Iron + the local Codex/Claude bridge. Ctrl+C stops both.\n--demo: ephemeral demo instead of your last saved scope\n--open: open the browser after both services are ready\nIRON_CODEX_BIN, IRON_CLAUDE_BIN: optional explicit executables\nIRON_CLAUDE_AUTH=api-key: bill Claude to ANTHROPIC_API_KEY instead of your subscription');
  } else {
    try {
      if (args.some(arg => !['--demo', '--open'].includes(arg))) throw new Error('Unknown option. Run bun run dev:agent --help.');
      if (process.env.IRON_AGENT_PORT && process.env.IRON_AGENT_PORT !== '4318') throw new Error('dev:agent uses bridge port 4318 for automatic pairing. Unset IRON_AGENT_PORT, or use separate dev and agent:serve commands for custom ports.');
      const session = await startAgentDev({ demo: args.includes('--demo') });
      const claude = session.claudeBin
        ? `${session.claudeBin} (${session.claudeAuth === 'api-key' ? 'ANTHROPIC_API_KEY' : 'your Claude subscription'})`
        : 'not found: install Claude Code or set IRON_CLAUDE_BIN to use Claude';
      console.log(`\nIron + Codex + Claude ready\n  App:    ${session.url}\n  Bridge: ${session.endpoint}\n  Codex:  ${session.codexBin}\n  Claude: ${claude}\n\nChoose Codex or Claude, and a model, in Model or Lesson. Pairing is automatic; no token to paste.\nUses your local Codex and Claude Code logins. If sign-in is needed, run:\n  ${JSON.stringify(session.codexBin)} login\n  ${JSON.stringify(session.claudeBin ?? 'claude')} auth login\n${args.includes('--demo') ? 'Demo changes are temporary; export anything you want to keep.\n' : ''}Ctrl+C stops both services.\n`);
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
