import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { CompletionRequest } from '../../src-ui/model/runtime-options';

export interface RuntimeResult { text: string; usage?: unknown }
export type Runtime = (request: CompletionRequest, signal: AbortSignal) => Promise<RuntimeResult>;

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
  return async (request, signal) => {
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
    rpc.onFailure = error => fail?.(error);
    rpc.onNotification = message => {
      if (message.method === 'item/completed' && message.params.item.type === 'agentMessage') {
        messages.set(message.params.item.id, message.params.item.text);
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
      await rpc.request('turn/start', { threadId: thread.id, input: [{ type: 'text', text: transcript(request) }] });
      return await completed;
    } finally {
      await rpc.close();
      await rm(cwd, { recursive: true, force: true });
    }
  };
}

export function claudeRuntime(command = process.env.IRON_CLAUDE_BIN ?? 'claude'): Runtime {
  return async (request, signal) => {
    if (!process.env.ANTHROPIC_API_KEY) throw new Error('Claude runtime requires ANTHROPIC_API_KEY on the bridge; subscription login is not assumed');
    const cwd = await mkdtemp(join(tmpdir(), 'iron-agent-'));
    const child = spawn(command, ['-p', '--bare', '--output-format', 'json', '--tools', '',
      '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--setting-sources', '',
      '--no-session-persistence', '--system-prompt', request.system,
      ...(request.model ? ['--model', request.model] : [])], { cwd, stdio: 'pipe', signal });
    child.stderr.resume();
    try {
      return await new Promise<RuntimeResult>((resolve, reject) => {
        child.stdin.on('error', reject);
        let output = '';
        child.stdout.on('data', chunk => {
          output += chunk.toString();
          if (output.length > 2_000_000) { child.kill(); reject(new Error('Claude response exceeds limit')); }
        });
        child.on('error', reject);
        child.on('close', code => {
          try {
            if (code !== 0) throw new Error(`Claude exited (${code}); check local runtime authentication`);
            const result = JSON.parse(output);
            if (result.is_error || typeof result.result !== 'string') throw new Error('Claude did not return a successful text result');
            resolve({ text: result.result, usage: result.usage });
          } catch (error) { reject(error); }
        });
        child.stdin.end(transcript(request));
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
