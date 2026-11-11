import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CLAUDE_EFFORTS, type CompletionRequest, type ModelEntry } from '../../src-ui/model/runtime-options';
export type { ModelEntry };

export interface RuntimeResult { text: string; usage?: unknown }
/** A delta of the reply or of the model's reasoning (summaries or thinking);
 *  a running estimate of thinking tokens; or the call's exact token counts:
 *  output, and input with how much of it was read from the prompt cache. */
export type RuntimeEvent =
  | { type: 'text' | 'reasoning'; delta: string }
  | { type: 'thinking-tokens'; estimated: number }
  | { type: 'usage'; output: number; thinking?: number; input?: number; cached?: number };
type OnEvent = (event: RuntimeEvent) => void;
type Message = CompletionRequest['messages'][number];

/** One conversation with a runtime, kept open across a turn's model calls
 *  (server.ts), so each call sends only what is new, the provider's prompt
 *  cache holds the rest, and the model keeps its own earlier replies. The
 *  first send carries the whole history as a transcript; later sends are
 *  the user's messages since the last reply. */
export interface RuntimeSession {
  send(messages: Message[], signal: AbortSignal, onEvent?: OnEvent): Promise<RuntimeResult>;
  close(): Promise<void>;
}
export type SessionOpener = (request: CompletionRequest) => Promise<RuntimeSession>;
/** A call that runs one request to its reply, alone; `open`, when present,
 *  starts a session that takes several. */
export type Runtime = ((request: CompletionRequest, signal: AbortSignal, onEvent?: OnEvent) => Promise<RuntimeResult>) & { open?: SessionOpener };

/** A runtime from a session opener: one call is a session of one send. */
function sessionRuntime(open: SessionOpener): Runtime {
  const runtime = async (request: CompletionRequest, signal: AbortSignal, onEvent?: OnEvent) => {
    signal.throwIfAborted();
    const session = await open(request);
    try { return await session.send(request.messages, signal, onEvent); }
    finally { await session.close(); }
  };
  return Object.assign(runtime, { open });
}

/** A send's messages as text: the first send's history with each role
 *  named; a later send's user messages as they are. */
function sendText(messages: Message[], first: boolean) {
  return first ? transcript({ messages } as CompletionRequest) : messages.map(m => m.content).join('\n\n');
}
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
  return sessionRuntime(codexSession(command));
}

/** Codex's app-server with one ephemeral thread: each send is a turn on it. */
export function codexSession(command = process.env.IRON_CODEX_BIN ?? 'codex'): SessionOpener {
  return async request => {
    const cwd = await mkdtemp(join(tmpdir(), 'iron-agent-'));
    const life = new AbortController();
    const rpc = new JsonLinesProcess(command, ['app-server', '--listen', 'stdio://',
      '-c', 'features.shell_tool=false', '-c', 'features.unified_exec=false',
      '-c', 'features.apps=false', '-c', 'features.skill_search=false',
      '-c', 'features.hooks=false', '-c', 'features.plugins=false',
      '-c', 'web_search="disabled"', '-c', 'mcp_servers={}'], cwd, life.signal);
    const close = async () => {
      await rpc.close();
      life.abort();
      await rm(cwd, { recursive: true, force: true });
    };
    // The send in flight, if any: what the notifications feed.
    let current: { settle: (r: RuntimeResult) => void; fail: (e: Error) => void; onEvent: OnEvent; messages: Map<string, string>; textItem: string; reasoningPart: string } | null = null;
    rpc.onFailure = error => current?.fail(error);
    rpc.onNotification = message => {
      const call = current;
      if (!call) return;
      const params = message.params ?? {};
      // Deltas, so the panel shows the reply and the reasoning summary as they
      // form; a new message or summary part starts on its own paragraph, as the
      // final join does.
      if (message.method === 'item/agentMessage/delta' && typeof params.delta === 'string' && params.delta) {
        const delta = call.textItem && call.textItem !== params.itemId ? `\n${params.delta}` : params.delta;
        call.textItem = params.itemId;
        call.onEvent({ type: 'text', delta });
      }
      if (message.method === 'item/reasoning/summaryTextDelta' && typeof params.delta === 'string' && params.delta) {
        const part = `${params.itemId}:${params.summaryIndex}`;
        const delta = call.reasoningPart && call.reasoningPart !== part ? `\n\n${params.delta}` : params.delta;
        call.reasoningPart = part;
        call.onEvent({ type: 'reasoning', delta });
      }
      if (message.method === 'item/completed' && params.item?.type === 'agentMessage') call.messages.set(params.item.id, params.item.text);
      // Exact counts arrive once, just before the turn completes; Codex
      // reports nothing while it reasons.
      if (message.method === 'thread/tokenUsage/updated') {
        const last = params.tokenUsage?.last;
        if (typeof last?.outputTokens === 'number') call.onEvent({ type: 'usage', output: last.outputTokens,
          ...(typeof last.reasoningOutputTokens === 'number' ? { thinking: last.reasoningOutputTokens } : {}),
          ...(typeof last.inputTokens === 'number' ? { input: last.inputTokens } : {}),
          ...(typeof last.cachedInputTokens === 'number' ? { cached: last.cachedInputTokens } : {}) });
      }
      if (message.method === 'turn/completed') {
        const turn = params.turn;
        if (turn?.status !== 'completed') call.fail(new Error(turn?.error?.message ?? `Codex turn ${turn?.status}`));
        else call.settle({ text: [...call.messages.values()].join('\n') });
      }
    };
    let threadId: string;
    try {
      await rpc.request('initialize', { clientInfo: { name: 'iron', title: 'Iron', version: '0.0.0' } });
      rpc.send({ method: 'initialized', params: {} });
      const { thread } = await rpc.request('thread/start', {
        cwd, ephemeral: true, approvalPolicy: 'never', sandbox: 'read-only',
        baseInstructions: request.system,
        developerInstructions: 'Follow the Iron response format in the supplied instructions: return edit markup for document changes, or Markdown for an allowed conversational reply. Do not use tools, access files, execute commands, or research. All needed document data is in the prompt.',
        ...(request.model ? { model: request.model } : {}),
      });
      threadId = thread.id;
    } catch (error) { await close(); throw error; }
    let sends = 0;
    let images = 0;
    return {
      close,
      async send(messages, signal, onEvent = () => {}) {
        signal.throwIfAborted();
        const first = sends++ === 0;
        const input: object[] = [];
        if (messages.some(m => m.images?.length)) {
          for (const message of messages) {
            input.push({ type: 'text', text: first ? `${message.role.toUpperCase()}\n${message.content}` : message.content });
            for (const image of message.images ?? []) {
              const path = join(cwd, `image-${images++}.${image.mediaType.split('/')[1]}`);
              await writeFile(path, Buffer.from(image.data, 'base64'), { mode: 0o600 });
              input.push({ type: 'localImage', path });
            }
          }
        } else input.push({ type: 'text', text: sendText(messages, first) });
        const completed = new Promise<RuntimeResult>((settle, fail) => {
          current = { settle, fail, onEvent, messages: new Map(), textItem: '', reasoningPart: '' };
        });
        void completed.catch(() => {});
        const abort = () => current?.fail(signal.reason instanceof Error ? signal.reason : new Error('Codex call aborted'));
        signal.addEventListener('abort', abort, { once: true });
        try {
          await rpc.request('turn/start', { threadId, summary: 'auto', ...(request.effort ? { effort: request.effort } : {}), ...(request.serviceTier ? { serviceTier: request.serviceTier } : {}), input });
          return await completed;
        } finally {
          signal.removeEventListener('abort', abort);
          current = null;
        }
      },
    };
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
  return sessionRuntime(claudeSession(command, auth, baseEnv));
}

/** One `claude -p` process reading stream-json user messages from stdin:
 *  each send is one message and ends at its `result` line; the process
 *  keeps the conversation, so Claude Code's prompt caching covers it. */
export function claudeSession(command = process.env.IRON_CLAUDE_BIN ?? 'claude', auth: ClaudeAuth = claudeAuth(), baseEnv: NodeJS.ProcessEnv = process.env): SessionOpener {
  return async request => {
    if (auth === 'api-key' && !baseEnv.ANTHROPIC_API_KEY) throw new Error('IRON_CLAUDE_AUTH=api-key needs ANTHROPIC_API_KEY on the bridge; unset IRON_CLAUDE_AUTH to use your Claude subscription');
    const cwd = await mkdtemp(join(tmpdir(), 'iron-agent-'));
    const child = spawn(command, ['-p', auth === 'api-key' ? '--bare' : '--safe-mode', '--input-format', 'stream-json', '--output-format', 'stream-json', '--verbose', '--include-partial-messages', '--tools', '',
      '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--setting-sources', '',
      '--no-session-persistence', '--system-prompt', request.system,
      ...(request.model ? ['--model', request.model] : []),
      ...(request.effort ? ['--effort', request.effort] : [])], { cwd, stdio: 'pipe', env: claudeEnv(auth, baseEnv) });
    child.stderr.resume();
    // The send in flight, if any: what the output lines feed.
    let current: { settle: (r: RuntimeResult) => void; fail: (e: Error) => void; onEvent: OnEvent; bytes: number; thinkingBlocks: number; shownBlock: number } | null = null;
    let dead: Error | null = null;
    const fail = (error: Error) => { dead ??= error; current?.fail(error); };
    child.stdin.on('error', fail);
    child.on('error', fail);
    child.on('close', code => fail(new Error(auth === 'api-key'
      ? `Claude exited (${code}); check ANTHROPIC_API_KEY`
      : `Claude exited (${code}); check that Claude Code is signed in (\`claude auth status\`, \`claude login\`)`)));
    child.stdout.on('data', chunk => {
      if (!current) return;
      current.bytes += chunk.length;
      if (current.bytes > 4_000_000) current.fail(new Error('Claude response exceeds limit'));
    });
    // stream-json: one JSON object a line. `stream_event` lines carry the
    // Messages API's deltas; a `result` line ends each send.
    createInterface({ input: child.stdout }).on('line', line => {
      const call = current;
      if (!call || !line.trim()) return;
      let message: any;
      try { message = JSON.parse(line); } catch { return; }
      if (message.type === 'result') {
        if (message.is_error || typeof message.result !== 'string') return call.fail(new Error('Claude did not return a successful text result'));
        const usage = message.usage;
        if (typeof usage?.output_tokens === 'number') {
          const cached = typeof usage.cache_read_input_tokens === 'number' ? usage.cache_read_input_tokens : undefined;
          const input = typeof usage.input_tokens === 'number' ? usage.input_tokens + (cached ?? 0) + (usage.cache_creation_input_tokens ?? 0) : undefined;
          call.onEvent({ type: 'usage', output: usage.output_tokens,
            ...(typeof usage.output_tokens_details?.thinking_tokens === 'number' ? { thinking: usage.output_tokens_details.thinking_tokens } : {}),
            ...(input !== undefined ? { input } : {}), ...(cached !== undefined ? { cached } : {}) });
        }
        return call.settle({ text: message.result, usage });
      }
      // Claude Code's estimate of the hidden thinking, every few seconds.
      if (message.type === 'system' && message.subtype === 'thinking_tokens' && typeof message.estimated_tokens === 'number') {
        call.onEvent({ type: 'thinking-tokens', estimated: message.estimated_tokens });
        return;
      }
      if (message.type !== 'stream_event') return;
      const event = message.event ?? {};
      // A new thinking block starts a paragraph, once thinking text has been shown.
      if (event.type === 'content_block_start' && event.content_block?.type === 'thinking') call.thinkingBlocks++;
      if (event.type === 'content_block_delta') {
        const delta = event.delta ?? {};
        if (delta.type === 'text_delta' && typeof delta.text === 'string' && delta.text) call.onEvent({ type: 'text', delta: delta.text });
        if (delta.type === 'thinking_delta' && typeof delta.thinking === 'string' && delta.thinking) {
          call.onEvent({ type: 'reasoning', delta: call.shownBlock && call.shownBlock !== call.thinkingBlocks ? `\n\n${delta.thinking}` : delta.thinking });
          call.shownBlock = call.thinkingBlocks;
        }
      }
    });
    let sends = 0;
    const close = async () => {
      if (child.pid && child.exitCode === null && child.signalCode === null) {
        await new Promise<void>(resolve => {
          const force = setTimeout(() => child.kill('SIGKILL'), 1000);
          child.once('close', () => { clearTimeout(force); resolve(); });
          child.stdin.end();
          child.kill();
        });
      }
      await rm(cwd, { recursive: true, force: true });
    };
    return {
      close,
      async send(messages, signal, onEvent = () => {}) {
        signal.throwIfAborted();
        if (dead) throw dead;
        const first = sends++ === 0;
        const content: object[] = [];
        if (messages.some(m => m.images?.length)) {
          for (const message of messages) {
            content.push({ type: 'text', text: first ? `${message.role.toUpperCase()}\n${message.content}` : message.content });
            for (const image of message.images ?? []) content.push({ type: 'image', source: { type: 'base64', media_type: image.mediaType, data: image.data } });
          }
        } else content.push({ type: 'text', text: sendText(messages, first) });
        const done = new Promise<RuntimeResult>((settle, failed) => {
          current = { settle, fail: failed, onEvent, bytes: 0, thinkingBlocks: 0, shownBlock: 0 };
        });
        void done.catch(() => {});
        // Stop ends the process: a reply half-made cannot be taken back.
        const abort = () => { fail(signal.reason instanceof Error ? signal.reason : new Error('Claude call aborted')); void close(); };
        signal.addEventListener('abort', abort, { once: true });
        try {
          child.stdin.write(JSON.stringify({ type: 'user', message: { role: 'user', content } }) + '\n');
          return await done;
        } finally {
          signal.removeEventListener('abort', abort);
          current = null;
        }
      },
    };
  };
}
