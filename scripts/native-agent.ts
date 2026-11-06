import { spawn } from 'node:child_process';
import { stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';
import { once } from 'node:events';
import { createAgentBridge } from './agent-bridge/server';
import { claudeRuntime, claudeModels, codexRuntime, codexModels } from './agent-bridge/runtimes';
import { resolveClaude, resolveCodex } from './agent-bridge/resolve';

export function nativeAgentEnvironment(base: NodeJS.ProcessEnv = process.env, home = homedir()): NodeJS.ProcessEnv & { CLAUDE_CONFIG_DIR: string; IRON_CLAUDE_AUTH: 'subscription' } {
  return { ...base, CLAUDE_CONFIG_DIR: resolve(base.IRON_CLAUDE_CONFIG_DIR ?? join(home, '.claude_2nd')), IRON_CLAUDE_AUTH: 'subscription' };
}

export async function startNativeAgent(options: {
  env?: NodeJS.ProcessEnv; claudeBin?: string; codexBin?: string;
  bridgePort?: number; command?: string[];
} = {}) {
  const env = nativeAgentEnvironment(options.env);
  if (!(await stat(env.CLAUDE_CONFIG_DIR)).isDirectory()) throw new Error(`Claude profile is not a directory: ${env.CLAUDE_CONFIG_DIR}`);
  const claudeBin = await resolveClaude(options.claudeBin);
  const codexBin = await resolveCodex(options.codexBin).catch(() => 'codex');
  const token = randomBytes(24).toString('hex');
  const bridge = createAgentBridge({
    token, origins: ['tauri://localhost', 'http://tauri.localhost', 'https://tauri.localhost'],
    runtimes: { claude: claudeRuntime(claudeBin, 'subscription', env), codex: codexRuntime(codexBin) },
    models: { claude: claudeModels, codex: codexModels(codexBin) },
  });
  try {
    await new Promise<void>((resolve, reject) => {
      bridge.server.once('error', reject);
      bridge.server.listen(options.bridgePort ?? 4318, '127.0.0.1', resolve);
    });
  } catch (error) { bridge.stop(); throw error; }
  const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
  const command = options.command ?? [process.execPath, 'run', 'native:run'];
  const child = spawn(command[0], command.slice(1), {
    cwd: fileURLToPath(new URL('..', import.meta.url)), stdio: 'inherit', detached: true,
    env: { ...env, IRON_NATIVE_AGENT_TOKEN: token, IRON_NATIVE_AGENT_ENDPOINT: endpoint, IRON_NATIVE_AGENT_PROVIDER: 'claude' },
  });
  const exited = new Promise<number>((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', code => resolve(code ?? 1));
  });
  let stopping: Promise<void> | undefined;
  const stop = () => stopping ??= (async () => {
    const closed = bridge.server.listening ? once(bridge.server, 'close') : Promise.resolve();
    bridge.stop();
    if (child.pid) {
      const kill = (signal: NodeJS.Signals) => { try { process.kill(-child.pid!, signal); } catch {} };
      kill('SIGTERM');
      if (child.exitCode === null && child.signalCode === null) {
        const force = setTimeout(() => kill('SIGKILL'), 2000);
        try { await exited.catch(() => {}); } finally { clearTimeout(force); }
      }
    }
    await closed;
  })();
  const done = exited.finally(stop);
  void done.catch(() => {});
  return { endpoint, profile: env.CLAUDE_CONFIG_DIR, claudeBin, done, stop };
}

if (import.meta.main) {
  if (process.argv.includes('--help')) {
    console.log('bun run native:agent\nStarts the local bridge and builds/runs Tauri. Ctrl+C or closing Tauri stops both.\nClaude uses ~/.claude_2nd and subscription auth by default.\nIRON_CLAUDE_CONFIG_DIR: explicit alternate profile directory\nIRON_CLAUDE_BIN: optional Claude executable override');
  } else {
    try {
      if (process.argv.length > 2) throw new Error('Unknown option. Run bun run native:agent --help.');
      const session = await startNativeAgent();
      console.log(`Native agent bridge ready: ${session.endpoint}\nClaude profile: ${session.profile}\nClaude executable: ${session.claudeBin}\nTauri connects automatically and starts with Claude selected.\nIf needed, sign in with: CLAUDE_CONFIG_DIR=${JSON.stringify(session.profile)} ${JSON.stringify(session.claudeBin)} auth login\nBuilding/running Tauri. Ctrl+C or closing its window stops both services.`);
      let interrupted = false;
      for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, () => { interrupted = true; void session.stop(); });
      process.exitCode = await session.done;
      if (interrupted) process.exitCode = 0;
    } catch (error) {
      console.error(`Could not start native agents: ${error instanceof Error ? error.message : String(error)}\nExisting servers were not stopped.`);
      process.exitCode = 1;
    }
  }
}
