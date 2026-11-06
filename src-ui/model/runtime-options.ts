import type { ConversationOptions } from './conversation';

export type AgentProvider = 'codex' | 'claude' | 'anthropic';

export interface LocalRuntimeOptions {
  provider: 'codex' | 'claude';
  endpoint: string;
  token: string;
  model?: string;
}

export interface ModelOptions extends ConversationOptions {
  apiKey: string;
  model?: string;
  runtime?: LocalRuntimeOptions;
}

/** A model a runtime offers (the bridge's `POST /models`): `id` is what
 *  `CompletionRequest.model` takes; `default` marks the runtime's own. */
export interface ModelEntry { id: string; label: string; default?: boolean }

/** The Claude API's models, for the API-key path (no bridge to ask). */
export const ANTHROPIC_API_MODELS: ModelEntry[] = [
  { id: 'claude-opus-5-5', label: 'Claude Opus 5.5' },
  { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5' },
  { id: 'claude-haiku-5-5', label: 'Claude Haiku 5.5' },
  { id: 'claude-fable-5-1', label: 'Claude Fable 5.1' },
];

export interface CompletionRequest {
  provider: 'codex' | 'claude';
  system: string;
  messages: { role: 'user' | 'assistant'; content: string }[];
  model?: string;
}

export function isLoopbackOrigin(origin: string): boolean {
  try { return localEndpoint(origin).origin === origin; }
  catch { return false; }
}

export function localEndpoint(endpoint: string): URL {
  const url = new URL(endpoint);
  if (url.protocol !== 'http:' || !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new Error('Agent endpoint must be an HTTP loopback origin');
  }
  return url;
}
