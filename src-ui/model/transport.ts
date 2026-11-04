/**
 * How a prompt reaches a model. The agent only needs `complete`; where the
 * tokens come from is a deployment choice:
 *
 * - `AnthropicTransport`: the Claude API from the browser, with a key the
 *   user supplies. Simplest; the key lives in this browser's localStorage.
 * - `LocalRuntimeTransport`: a loopback bridge to Codex or Claude runtimes.
 */
import Anthropic from '@anthropic-ai/sdk';
import { localEndpoint, type ModelOptions, type LocalRuntimeOptions } from './runtime-options';

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

  async complete(system: string, messages: { role: 'user' | 'assistant'; content: string }[]) {
    const response = await fetch(new URL('/complete', this.endpoint), {
      method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${this.options.token}` },
      body: JSON.stringify({ provider: this.options.provider, system, messages, ...(this.options.model ? { model: this.options.model } : {}) }),
      signal: AbortSignal.timeout(50000),
    });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error ?? `Agent bridge returned ${response.status}`);
    if (typeof result.text !== 'string') throw new Error('Invalid agent bridge response');
    return { text: result.text, usage: result.usage };
  }
}

export interface ModelTransport {
  readonly name: string;
  complete(system: string, messages: { role: 'user' | 'assistant'; content: string }[]): Promise<{ text: string; usage?: unknown }>;
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

  async complete(system: string, messages: { role: 'user' | 'assistant'; content: string }[]) {
    // Streaming so a large view or a slow turn cannot hit a request timeout.
    const stream = this.client.messages.stream({
      model: this.model,
      max_tokens: 16000,
      thinking: { type: 'adaptive' },
      system: [{ type: 'text', text: system, cache_control: { type: 'ephemeral' } }],
      messages,
    });
    const final = await stream.finalMessage();
    const text = final.content
      .filter((b): b is Anthropic.TextBlock => b.type === 'text')
      .map((b) => b.text)
      .join('\n');
    return { text, usage: final.usage };
  }
}
