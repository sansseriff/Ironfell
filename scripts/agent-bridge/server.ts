import { createServer, type IncomingMessage } from 'node:http';
import { timingSafeEqual, randomBytes } from 'node:crypto';
import { codexRuntime, claudeRuntime, codexModels, claudeModels, claudeAuth, type ModelEntry, type ModelLister, type Runtime } from './runtimes';
import { resolveClaude, resolveCodex } from './resolve';
import type { CompletionRequest } from '../../src-ui/model/runtime-options';
import { isLoopbackOrigin } from '../../src-ui/model/runtime-options';

export function validateRequest(value: unknown): CompletionRequest {
  const request = value as CompletionRequest;
  if (!request || !['codex', 'claude'].includes(request.provider) || typeof request.system !== 'string' || !request.system.trim() ||
    !Array.isArray(request.messages) || !request.messages.length || request.messages.length > 16 ||
    request.messages.some(message => !message || !['user', 'assistant'].includes(message.role) || typeof message.content !== 'string') ||
    (request.model !== undefined && (typeof request.model !== 'string' || request.model.length > 200))) {
    throw new Error('Invalid completion request');
  }
  return request;
}

async function readBody(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > 1_000_000) throw new Error('Request exceeds 1 MB limit');
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}

export function createAgentBridge(options: {
  token: string;
  origins: string[];
  runtimes?: Record<'codex' | 'claude', Runtime>;
  /** What each runtime offers, for the model picker (`POST /models`). */
  models?: Partial<Record<'codex' | 'claude', ModelLister>>;
  timeoutMs?: number;
  allowPairing?: boolean;
}) {
  if (options.token.length < 32) throw new Error('Bridge token must be at least 32 characters');
  const runtimes = options.runtimes ?? { codex: codexRuntime(), claude: claudeRuntime() };
  const models = options.models ?? (options.runtimes ? {} : { codex: codexModels(), claude: claudeModels });
  // A login's model list changes rarely: asked once a few minutes.
  const listed = new Map<string, { at: number; models: ModelEntry[] }>();
  let busy = false;
  const active = new Set<AbortController>();
  const server = createServer(async (request, response) => {
    response.setHeader('Cache-Control', 'no-store');
    const send = (status: number, value: object) => {
      if (!response.destroyed) response.writeHead(status, { 'Content-Type': 'application/json' }).end(JSON.stringify(value));
    };
    const host = request.headers.host ?? '';
    if (!/^(127\.0\.0\.1|localhost|\[::1\]):\d+$/.test(host)) return send(403, { error: 'Invalid Host' });
    const origin = request.headers.origin;
    if (origin && !options.origins.includes(origin)) return send(403, { error: 'Origin not allowed' });
    if (origin) {
      response.setHeader('Access-Control-Allow-Origin', origin);
      response.setHeader('Vary', 'Origin');
    }
    if (request.method === 'OPTIONS') {
      response.setHeader('Access-Control-Allow-Methods', 'POST');
      response.setHeader('Access-Control-Allow-Headers', 'Authorization, Content-Type, X-Iron-Local-Pairing');
      response.writeHead(204).end();
      return;
    }
    if (request.url === '/pair' && options.allowPairing) {
      if (request.method !== 'POST' || !origin || !isLoopbackOrigin(origin) || request.headers['x-iron-local-pairing'] !== '1') return send(403, { error: 'Local pairing requires a trusted loopback app origin and pairing header' });
      if (request.headers['content-type'] !== 'application/json') return send(415, { error: 'Expected application/json' });
      request.resume();
      return send(200, { token: options.token });
    }
    const actual = Buffer.from(request.headers.authorization ?? '');
    const expected = Buffer.from(`Bearer ${options.token}`);
    if (actual.length !== expected.length || !timingSafeEqual(actual, expected)) return send(401, { error: 'Invalid bridge token' });
    if (request.url === '/models' && request.method === 'POST') {
      if (request.headers['content-type'] !== 'application/json') return send(415, { error: 'Expected application/json' });
      try {
        const { provider } = await readBody(request);
        if (provider !== 'codex' && provider !== 'claude') throw new Error('Unknown provider');
        const cached = listed.get(provider);
        if (cached && Date.now() - cached.at < 300_000) return send(200, { models: cached.models });
        const lister = models[provider as 'codex' | 'claude'];
        const found = lister ? await lister(AbortSignal.timeout(15000)) : [];
        listed.set(provider, { at: Date.now(), models: found });
        return send(200, { models: found });
      } catch (error) {
        return send(400, { error: String(error) });
      }
    }
    if (request.url !== '/complete' || request.method !== 'POST') return send(404, { error: 'Not found' });
    if (request.headers['content-type'] !== 'application/json') return send(415, { error: 'Expected application/json' });
    if (busy) return send(429, { error: 'Agent bridge is busy' });
    busy = true;
    const controller = new AbortController();
    active.add(controller);
    let timedOut = false;
    const timeout = setTimeout(() => { timedOut = true; controller.abort(); }, options.timeoutMs ?? 45000);
    const disconnect = () => { if (!response.writableEnded) controller.abort(); };
    response.on('close', disconnect);
    try {
      const payload = validateRequest(await readBody(request));
      const result = await runtimes[payload.provider](payload, controller.signal);
      if (typeof result.text !== 'string' || !result.text.trim()) throw new Error('Runtime returned no text');
      send(200, result);
    } catch (error) {
      send(timedOut ? 504 : 400, { error: timedOut ? 'Agent runtime timed out' : String(error) });
    } finally {
      clearTimeout(timeout);
      response.off('close', disconnect);
      active.delete(controller);
      busy = false;
    }
  });
  server.requestTimeout = 10000;
  server.headersTimeout = 10000;
  return { server, stop: () => { for (const controller of active) controller.abort(); server.close(); server.closeAllConnections(); } };
}

if (import.meta.main) {
  const token = process.env.IRON_AGENT_TOKEN ?? randomBytes(24).toString('hex');
  const port = Number(process.env.IRON_AGENT_PORT ?? 4318);
  const origins = (process.env.IRON_AGENT_ORIGINS ?? 'http://localhost:5173,http://127.0.0.1:5173,tauri://localhost,http://tauri.localhost').split(',');
  // Each CLI is optional: a missing one fails only when it is asked.
  const codexBin = await resolveCodex().catch(() => process.env.IRON_CODEX_BIN ?? 'codex');
  const claudeBin = await resolveClaude().catch(() => process.env.IRON_CLAUDE_BIN ?? 'claude');
  const auth = claudeAuth();
  const bridge = createAgentBridge({
    token, origins, allowPairing: true,
    runtimes: { codex: codexRuntime(codexBin), claude: claudeRuntime(claudeBin, auth) },
    models: { codex: codexModels(codexBin), claude: claudeModels },
  });
  bridge.server.listen(port, '127.0.0.1', () => {
    console.log(`Iron agent bridge: http://127.0.0.1:${port}\nSession token: ${token}\nCodex:  ${codexBin}\nClaude: ${claudeBin} (${auth === 'api-key' ? 'ANTHROPIC_API_KEY' : 'your Claude subscription'})\nTrusted local Iron dev pages pair automatically. Manual token entry remains available. Provider credentials stay local.`);
  });
  for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, () => bridge.stop());
}
