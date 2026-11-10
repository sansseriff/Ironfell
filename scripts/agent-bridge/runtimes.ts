import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CLAUDE_EFFORTS, type CompletionRequest, type ModelEntry } from '../../src-ui/model/runtime-options';
export type { ModelEntry };

export interface RuntimeResult { text: string; usage?: unknown }
/** A delta of the reply or of the model's reasoning (summaries or thinking);
 *  a running estimate of thinking tokens; or the call's exact output tokens. */
export type RuntimeEvent =
  | { type: 'text' | 'reasoning'; delta: string }
  | { type: 'thinking-tokens'; estimated: number }
  | { type: 'usage'; output: number; thinking?: number };
export type Runtime = (request: CompletionRequest, signal: AbortSignal, onEvent?: (event: RuntimeEvent) => void) => Promise<RuntimeResult>;
export type ModelLister = (signal: AbortSignal) => Promise<ModelEntry[]>;

export class JsonLinesProcess {
  private nextId = 0;
  private pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void }>();
  onNotification: (message: any) => void = () => {};
  onFailure: (error: Error) => void = () => {};
  private failure: Error | null = null;
  private closed = false;
  readonly child: ChildProcessWithoutNullStreams;

  constructor(command: string, args: string[], cwd: string, signal: AbortSignal) {
    this.child = spawn(command, args, { cwd, stdio: 'pipe', signal });
    this.child.stdin.on('error', error => this.fail(error));
    this.child.stderr.resume();
    let bytes = 0;
    this.child.stdout.on('data', chunk => {
      bytes += chunk.length;
      if (bytes > 4_000_000) this.fail(new Error('Runtime output exceeds limit'));
    });
    const lines = createInterface({ input: this.child.stdout });
    lines.on('line', line => {
      try {
        if (line.length > 2_000_000) throw new Error('Runtime message exceeds limit');
        const message = JSON.parse(line);
        if (message.method && message.id !== undefined) {
          this.send({ id: message.id, error: { code: -32601, message: 'Iron does not grant runtime tool approvals' } });
          this.fail(new Error(`Unexpected runtime request: ${message.method}`));
        } else if (message.id !== undefined) {
          const entry = this.pending.get(message.id);
          if (!entry) return;
          this.pending.delete(message.id);
          if (message.error) entry.reject(new Error(message.error.message));
          else entry.resolve(message.result);
        } else this.onNotification(message);
      } catch (error) { this.fail(error instanceof Error ? error : new Error(String(error))); }
    });
    this.child.on('error', error => this.fail(error));
    this.child.once('close', () => { this.closed = true; });
    this.child.on('exit', code => this.fail(new Error(`Runtime exited (${code ?? 'signal'})`)));
  }

  send(message: object) { this.child.stdin.write(`${JSON.stringify(message)}\n`); }

  request(method: string, params: object): Promise<any> {
    if (this.failure) return Promise.reject(this.failure);
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.send({ id, method, params });
    });
  }

  private fail(error: Error) {
    if (this.failure) return;
    this.failure = error;
    for (const entry of this.pending.values()) entry.reject(error);
    this.pending.clear();
    this.onFailure(error);
  }

  async close() {
    if (this.closed || !this.child.pid || this.child.exitCode !== null || this.child.signalCode !== null) return;
    await new Promise<void>(resolve => {
      const force = setTimeout(() => this.child.kill('SIGKILL'), 1000);
      this.child.once('close', () => { clearTimeout(force); resolve(); });
      this.child.kill('SIGTERM');
    });
  }
}

function transcript(request: CompletionRequest) {
  return request.messages.map(message => `${message.role.toUpperCase()}\n${message.content}`).join('\n\n');
}

export function codexRuntime(command = process.env.IRON_CODEX_BIN ?? 'codex'): Runtime {
  return async (request, signal, onEvent = () => {}) => {
    const cwd = await mkdtemp(join(tmpdir(), 'iron-agent-'));
    const rpc = new JsonLinesProcess(command, ['app-server', '--listen', 'stdio://',
      '-c', 'features.shell_tool=false', '-c', 'features.unified_exec=false',
      '-c', 'features.apps=false', '-c', 'features.skill_search=false',
      '-c', 'features.hooks=false', '-c', 'features.plugins=false',
      '-c', 'web_search="disabled"', '-c', 'mcp_servers={}'], cwd, signal);
    let settle: ((result: RuntimeResult) => void) | undefined;
    let fail: ((error: Error) => void) | undefined;
    const completed = new Promise<RuntimeResult>((resolve, reject) => { settle = resolve; fail = reject; });
    void completed.catch(() => {});
    const messages = new Map<string, string>();
    // Deltas, so the panel shows the reply and the reasoning summary as they
    // form; a new message or summary part starts on its own paragraph, as the
    // final join does.
    let textItem = '';
    let reasoningPart = '';
    rpc.onFailure = error => fail?.(error);
    rpc.onNotification = message => {
      const params = message.params ?? {};
      if (message.method === 'item/agentMessage/delta' && typeof params.delta === 'string' && params.delta) {
        const delta = textItem && textItem !== params.itemId ? `\n${params.delta}` : params.delta;
        textItem = params.itemId;
        onEvent({ type: 'text', delta });
      }
      if (message.method === 'item/reasoning/summaryTextDelta' && typeof params.delta === 'string' && params.delta) {
        const part = `${params.itemId}:${params.summaryIndex}`;
        const delta = reasoningPart && reasoningPart !== part ? `\n\n${params.delta}` : params.delta;
        reasoningPart = part;
        onEvent({ type: 'reasoning', delta });
      }
      if (message.method === 'item/completed' && message.params.item.type === 'agentMessage') {
        messages.set(message.params.item.id, message.params.item.text);
      }
      // Exact counts arrive once, just before the turn completes; Codex
      // reports nothing while it reasons.
      if (message.method === 'thread/tokenUsage/updated') {
        const last = params.tokenUsage?.last;
        if (typeof last?.outputTokens === 'number') onEvent({ type: 'usage', output: last.outputTokens, ...(typeof last.reasoningOutputTokens === 'number' ? { thinking: last.reasoningOutputTokens } : {}) });
      }
      if (message.method === 'turn/completed') {
        const turn = message.params.turn;
        if (turn.status !== 'completed') fail?.(new Error(turn.error?.message ?? `Codex turn ${turn.status}`));
        else settle?.({ text: [...messages.values()].join('\n') });
      }
    };
    try {
      await rpc.request('initialize', { clientInfo: { name: 'iron', title: 'Iron', version: '0.0.0' } });
      rpc.send({ method: 'initialized', params: {} });
      const { thread } = await rpc.request('thread/start', {
        cwd, ephemeral: true, approvalPolicy: 'never', sandbox: 'read-only',
        baseInstructions: request.system,
        developerInstructions: 'Follow the Iron response format in the supplied instructions: return edit markup for document changes, or Markdown for an allowed conversational reply. Do not use tools, access files, execute commands, or research. All needed document data is in the prompt.',
        ...(request.model ? { model: request.model } : {}),
      });
      const input: object[] = [];
      for (const [index, message] of request.messages.entries()) {
        input.push({ type: 'text', text: `${message.role.toUpperCase()}\n${message.content}` });
        for (const [imageIndex, image] of (message.images ?? []).entries()) {
          const path = join(cwd, `image-${index}-${imageIndex}.${image.mediaType.split('/')[1]}`);
          await writeFile(path, Buffer.from(image.data, 'base64'), { mode: 0o600 });
          input.push({ type: 'localImage', path });
        }
      }
      await rpc.request('turn/start', { threadId: thread.id, summary: 'auto', ...(request.effort ? { effort: request.effort } : {}), ...(request.serviceTier ? { serviceTier: request.serviceTier } : {}), input: request.messages.some(m => m.images?.length) ? input : [{ type: 'text', text: transcript(request) }] });
      return await completed;
    } finally {
      await rpc.close();
      await rm(cwd, { recursive: true, force: true });
    }
  };
}

/** The models Codex offers this login, from its app-server (`model/list`). */
export function codexModels(command = process.env.IRON_CODEX_BIN ?? 'codex'): ModelLister {
  return async signal => {
    const cwd = await mkdtemp(join(tmpdir(), 'iron-agent-'));
    const rpc = new JsonLinesProcess(command, ['app-server', '--listen', 'stdio://'], cwd, signal);
    try {
      await rpc.request('initialize', { clientInfo: { name: 'iron', title: 'Iron', version: '0.0.0' } });
      rpc.send({ method: 'initialized', params: {} });
      const result = await rpc.request('model/list', {});
      const data: any[] = Array.isArray(result?.data) ? result.data : [];
      return data.filter(m => !m.hidden && typeof (m.model ?? m.id) === 'string')
        .map(m => {
          const efforts = Array.isArray(m.supportedReasoningEfforts) ? m.supportedReasoningEfforts.map((e: any) => e?.reasoningEffort).filter((e: unknown) => typeof e === 'string') : [];
          const tiers = Array.isArray(m.serviceTiers) ? m.serviceTiers.filter((t: any) => typeof t?.id === 'string' && typeof t?.name === 'string')
            .map((t: any) => ({ id: t.id, name: t.name, ...(typeof t.description === 'string' && t.description ? { description: t.description } : {}) })) : [];
          return { id: m.model ?? m.id, label: m.displayName ?? m.model ?? m.id, ...(m.isDefault ? { default: true } : {}), ...(efforts.length ? { efforts } : {}), ...(tiers.length ? { tiers } : {}) };
        });
    } finally {
      await rpc.close();
      await rm(cwd, { recursive: true, force: true });
    }
  };
}

/** Claude Code's aliases, each the latest of its family (`claude --model`). */
export const claudeModels: ModelLister = async () => [
  { id: 'opus', label: 'Opus', efforts: CLAUDE_EFFORTS },
  { id: 'sonnet', label: 'Sonnet', efforts: CLAUDE_EFFORTS },
  { id: 'haiku', label: 'Haiku', efforts: CLAUDE_EFFORTS },
  { id: 'fable', label: 'Fable', efforts: CLAUDE_EFFORTS },
];

/** How Claude Code is paid for: the signed-in Claude subscription (the
 *  default, what `claude login` set up), or `ANTHROPIC_API_KEY` with
 *  `IRON_CLAUDE_AUTH=api-key`. */
export type ClaudeAuth = 'subscription' | 'api-key';

export function claudeAuth(value = process.env.IRON_CLAUDE_AUTH): ClaudeAuth {
  if (!value || value === 'subscription') return 'subscription';
  if (value === 'api-key') return 'api-key';
  throw new Error(`IRON_CLAUDE_AUTH=${value}: use subscription (the default) or api-key`);
}

/** Claude Code's environment: the bridge's, without what a surrounding
 *  Claude Code session sets (`CLAUDECODE`, `CLAUDE_CODE_*`, …: the child
 *  would join that session and see its context), and on a subscription
 *  without API credentials, so the login is what pays. */
export function claudeEnv(auth: ClaudeAuth, base: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  const env = { ...base };
  for (const key of Object.keys(env)) {
    if (key === 'CLAUDECODE' || key === 'CLAUDE_PID' || key === 'CLAUDE_EFFORT' || key.startsWith('CLAUDE_CODE_') || key.startsWith('CLAUDE_AGENT_SDK')) delete env[key];
  }
  // A base URL routes to whoever holds that token (a host's proxy, say);
  // without the token it only fails, so the subscription drops both.
  if (auth === 'subscription') { delete env.ANTHROPIC_API_KEY; delete env.ANTHROPIC_AUTH_TOKEN; delete env.ANTHROPIC_BASE_URL; }
  return env;
}

/** Claude Code, headless, with no tools, MCP servers or settings. On a
 *  subscription `--safe-mode` turns off every customization (CLAUDE.md,
 *  skills, plugins, hooks, agents) and keeps the login; with an API key
 *  `--bare` does the same and reads only the key. */
export function claudeRuntime(command = process.env.IRON_CLAUDE_BIN ?? 'claude', auth: ClaudeAuth = claudeAuth(), baseEnv: NodeJS.ProcessEnv = process.env): Runtime {
  return async (request, signal, onEvent = () => {}) => {
    if (auth === 'api-key' && !baseEnv.ANTHROPIC_API_KEY) throw new Error('IRON_CLAUDE_AUTH=api-key needs ANTHROPIC_API_KEY on the bridge; unset IRON_CLAUDE_AUTH to use your Claude subscription');
    const cwd = await mkdtemp(join(tmpdir(), 'iron-agent-'));
    const child = spawn(command, ['-p', auth === 'api-key' ? '--bare' : '--safe-mode', '--output-format', 'stream-json', ...(request.messages.some(m => m.images?.length) ? ['--input-format', 'stream-json'] : []), '--verbose', '--include-partial-messages', '--tools', '',
      '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--setting-sources', '',
      '--no-session-persistence', '--system-prompt', request.system,
      ...(request.model ? ['--model', request.model] : []),
      ...(request.effort ? ['--effort', request.effort] : [])], { cwd, stdio: 'pipe', signal, env: claudeEnv(auth, baseEnv) });
    child.stderr.resume();
    try {
      return await new Promise<RuntimeResult>((resolve, reject) => {
        child.stdin.on('error', reject);
        // stream-json: one JSON object a line. `stream_event` lines carry the
        // Messages API's deltas; the last line is the `result`.
        let bytes = 0;
        let result: any = null;
        let thinkingBlocks = 0;
        let shownBlock = 0;
        const lines = createInterface({ input: child.stdout });
        child.stdout.on('data', chunk => {
          bytes += chunk.length;
          if (bytes > 4_000_000) { child.kill(); reject(new Error('Claude response exceeds limit')); }
        });
        lines.on('line', line => {
          if (!line.trim()) return;
          let message: any;
          try { message = JSON.parse(line); } catch { return; }
          if (message.type === 'result') { result = message; return; }
          // Claude Code's estimate of the hidden thinking, every few seconds.
          if (message.type === 'system' && message.subtype === 'thinking_tokens' && typeof message.estimated_tokens === 'number') {
            onEvent({ type: 'thinking-tokens', estimated: message.estimated_tokens });
            return;
          }
          if (message.type !== 'stream_event') return;
          const event = message.event ?? {};
          // A new thinking block starts a paragraph, once thinking text has been shown.
          if (event.type === 'content_block_start' && event.content_block?.type === 'thinking') thinkingBlocks++;
          if (event.type === 'content_block_delta') {
            const delta = event.delta ?? {};
            if (delta.type === 'text_delta' && typeof delta.text === 'string' && delta.text) onEvent({ type: 'text', delta: delta.text });
            if (delta.type === 'thinking_delta' && typeof delta.thinking === 'string' && delta.thinking) {
              onEvent({ type: 'reasoning', delta: shownBlock && shownBlock !== thinkingBlocks ? `\n\n${delta.thinking}` : delta.thinking });
              shownBlock = thinkingBlocks;
            }
          }
        });
        child.on('error', reject);
        // Settle once the process has exited and its last line is read: the
        // result line can be delivered after the child's own close.
        const exited = new Promise<number | null>(done => child.on('close', code => done(code)));
        const drained = new Promise<void>(done => lines.on('close', () => done()));
        void Promise.all([exited, drained]).then(([code]) => {
          try {
            if (code !== 0 && !result) throw new Error(auth === 'api-key'
              ? `Claude exited (${code}); check ANTHROPIC_API_KEY`
              : `Claude exited (${code}); check that Claude Code is signed in (\`claude auth status\`, \`claude login\`)`);
            if (!result || result.is_error || typeof result.result !== 'string') throw new Error('Claude did not return a successful text result');
            const usage = result.usage;
            if (typeof usage?.output_tokens === 'number') onEvent({ type: 'usage', output: usage.output_tokens, ...(typeof usage.output_tokens_details?.thinking_tokens === 'number' ? { thinking: usage.output_tokens_details.thinking_tokens } : {}) });
            resolve({ text: result.result, usage });
          } catch (error) { reject(error); }
        });
        if (request.messages.some(m => m.images?.length)) {
          const content: object[] = [];
          for (const message of request.messages) {
            content.push({ type: 'text', text: `${message.role.toUpperCase()}\n${message.content}` });
            for (const image of message.images ?? []) content.push({ type: 'image', source: { type: 'base64', media_type: image.mediaType, data: image.data } });
          }
          child.stdin.end(JSON.stringify({ type: 'user', message: { role: 'user', content } }) + '\n');
        } else child.stdin.end(transcript(request));
      });
    } finally {
      if (child.pid && child.exitCode === null && child.signalCode === null) {
        await new Promise<void>(resolve => {
          const force = setTimeout(() => child.kill('SIGKILL'), 1000);
          child.once('close', () => { clearTimeout(force); resolve(); });
          child.kill();
        });
      }
      await rm(cwd, { recursive: true, force: true });
    }
  };
}
