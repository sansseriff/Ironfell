/**
 * How a prompt reaches a model. The agent only needs `complete`; where the
 * tokens come from is a deployment choice:
 *
 * - `AnthropicTransport`: the Claude API from the browser, with a key the
 *   user supplies. Simplest; the key lives in this browser's localStorage.
 * - a local sidecar (not yet written) that shells out to `claude -p` or
 *   exposes the document as MCP tools, for driving from Claude Code.
 */
import Anthropic from '@anthropic-ai/sdk';

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
