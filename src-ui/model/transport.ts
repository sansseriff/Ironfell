/**
 * How a prompt reaches a model. The agent only needs `complete`; where the
 * tokens come from is a deployment choice:
 *
 * - `AnthropicTransport`: the Claude API from the browser, with a key the
 *   user supplies. Simplest; the key lives in this browser's localStorage.
 * - `LocalRuntimeTransport`: a loopback bridge to Codex or Claude runtimes.
 */
import Anthropic from '@anthropic-ai/sdk';
import { localEndpoint, type ModelEntry, type ModelOptions, type LocalRuntimeOptions } from './runtime-options';
import type { AgentEvent } from './progress';

type Message = { role: 'user' | 'assistant'; content: string };
/** Where a streaming completion sends its deltas, and how it is stopped. */
export interface CompleteLive { onEvent?: (event: AgentEvent) => void; signal?: AbortSignal }

/** No bytes from the bridge for this long means it is gone; it pings every
 *  10 s while a runtime works (scripts/agent-bridge/server.ts). */
const BRIDGE_SILENCE_MS = 30000;

/** The models a local runtime offers, from the bridge (`POST /models`). */
export async function listLocalModels(endpoint: string, token: string, provider: 'codex' | 'claude'): Promise<ModelEntry[]> {
  const response = await fetch(new URL('/models', localEndpoint(endpoint)), {
    method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
    body: JSON.stringify({ provider }), signal: AbortSignal.timeout(20000),
  });
  const result = await response.json();
  if (!response.ok) throw new Error(result.error ?? `Agent bridge returned ${response.status}`);
  return Array.isArray(result.models) ? result.models : [];
}

export function makeModelTransport(options: ModelOptions): ModelTransport {
  return options.runtime ? new LocalRuntimeTransport(options.runtime) : new AnthropicTransport(options.apiKey, options.model);
}

export class LocalRuntimeTransport implements ModelTransport {
  readonly name: string;
  private endpoint: URL;

  constructor(private options: LocalRuntimeOptions) {
    this.endpoint = localEndpoint(options.endpoint);
    this.name = `${options.provider}:local${options.model ? `:${options.model}` : ''}`;
  }

  async complete(system: string, messages: Message[], live: CompleteLive = {}) {
    // Silence, not length, ends a turn: a long component build may stream
    // for minutes, but a bridge that stops answering is dead.
    const controller = new AbortController();
    let silent = false;
    let timer = setTimeout(() => { silent = true; controller.abort(); }, BRIDGE_SILENCE_MS);
    const heard = () => { clearTimeout(timer); timer = setTimeout(() => { silent = true; controller.abort(); }, BRIDGE_SILENCE_MS); };
    const signal = live.signal ? AbortSignal.any([live.signal, controller.signal]) : controller.signal;
    try {
      const response = await fetch(new URL('/complete', this.endpoint), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/x-ndjson', Authorization: `Bearer ${this.options.token}` },
        body: JSON.stringify({ provider: this.options.provider, system, messages, ...(this.options.model ? { model: this.options.model } : {}) }),
        signal,
      });
      heard();
      if (!response.headers.get('content-type')?.includes('application/x-ndjson') || !response.body) {
        // An error, or a bridge from before streaming: one JSON object.
        const result = await response.json();
        if (!response.ok) throw new Error(result.error ?? `Agent bridge returned ${response.status}`);
        if (typeof result.text !== 'string') throw new Error('Invalid agent bridge response');
        return { text: result.text, usage: result.usage };
      }
      return await readStream(response.body, live.onEvent, heard);
    } catch (error) {
      if (silent) throw new Error('Agent bridge stopped responding');
      throw error;
    } finally {
      clearTimeout(timer);
    }
  }
}

/** The bridge's NDJSON: `text` and `reasoning` deltas and `ping`s, then one
 *  `done` (the whole reply) or `error`. */
export async function readStream(body: ReadableStream<Uint8Array>, onEvent: CompleteLive['onEvent'], heard: () => void = () => {}) {
  const reader = body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = '';
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    heard();
    buffer += value;
    let newline: number;
    while ((newline = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (!line) continue;
      const message = JSON.parse(line);
      if (message.type === 'text' || message.type === 'reasoning') {
        if (typeof message.delta === 'string') onEvent?.({ kind: message.type, delta: message.delta });
      } else if (message.type === 'thinking-tokens') {
        if (typeof message.estimated === 'number') onEvent?.({ kind: 'thinking-tokens', estimated: message.estimated });
      } else if (message.type === 'usage') {
        if (typeof message.output === 'number') onEvent?.({ kind: 'usage', output: message.output, ...(typeof message.thinking === 'number' ? { thinking: message.thinking } : {}) });
      } else if (message.type === 'done') {
        if (typeof message.text !== 'string') throw new Error('Invalid agent bridge response');
        return { text: message.text as string, usage: message.usage };
      } else if (message.type === 'error') {
        throw new Error(message.error ?? 'Agent runtime failed');
      }
    }
  }
  throw new Error('Agent bridge closed the stream without a reply');
}

export interface ModelTransport {
  readonly name: string;
  /** The whole reply; with `live.onEvent`, its text and reasoning as they form. */
  complete(system: string, messages: Message[], live?: CompleteLive): Promise<{ text: string; usage?: unknown }>;
}

export class AnthropicTransport implements ModelTransport {
  readonly name: string;
  private client: Anthropic;

  constructor(
    apiKey: string,
    private model = 'claude-opus-5',
  ) {
    this.name = `anthropic:${model}`;
    // The key is the user's own and stays in their browser.
    this.client = new Anthropic({ apiKey, dangerouslyAllowBrowser: true });
  }

  async complete(system: string, messages: Message[], live: CompleteLive = {}) {
    // Streaming so a large view or a slow turn cannot hit a request timeout.
    const stream = this.client.messages.stream({
      model: this.model,
      max_tokens: 16000,
      thinking: { type: 'adaptive', display: 'summarized' },
      system: [{ type: 'text', text: system, cache_control: { type: 'ephemeral' } }],
      messages,
    }, { signal: live.signal });
    if (live.onEvent) {
      const onEvent = live.onEvent;
      stream.on('text', delta => onEvent({ kind: 'text', delta }));
      stream.on('thinking', delta => onEvent({ kind: 'reasoning', delta }));
    }
    const final = await stream.finalMessage();
    live.onEvent?.({ kind: 'usage', output: final.usage.output_tokens });
    const text = final.content
      .filter((b): b is Anthropic.TextBlock => b.type === 'text')
      .map((b) => b.text)
      .join('\n');
    return { text, usage: final.usage };
  }
}
