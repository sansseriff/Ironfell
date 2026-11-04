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
