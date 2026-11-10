import type { ConversationOptions } from './conversation';

export type AgentProvider = 'codex' | 'claude' | 'anthropic';

export interface LocalRuntimeOptions {
  provider: 'codex' | 'claude';
  endpoint: string;
  token: string;
  model?: string;
  /** Reasoning effort (`low` … `max`, and Codex's `ultra`); unset: the runtime's default. */
  effort?: string;
  /** A speed tier the model lists (Codex's `priority`: Fast); unset: the runtime's default. */
  serviceTier?: string;
}

export interface ModelOptions extends ConversationOptions {
  apiKey: string;
  model?: string;
  /** The API path's `output_config.effort`; unset: the model's default. */
  effort?: string;
  runtime?: LocalRuntimeOptions;
}

/** A model a runtime offers (the bridge's `POST /models`): `id` is what
 *  `CompletionRequest.model` takes; `default` marks the runtime's own;
 *  `efforts` are the reasoning levels it accepts, lowest first. */
export interface ModelEntry { id: string; label: string; default?: boolean; efforts?: string[]; tiers?: ServiceTier[] }

/** A speed tier a model offers (Codex's `serviceTiers`): `priority` is Fast. */
export interface ServiceTier { id: string; name: string; description?: string }

/** The effort levels Claude takes, through Claude Code (`--effort`) and the
 *  API (`output_config.effort`). */
export const CLAUDE_EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'];

/** An effort level's name: low … max, and Codex's `ultra`. */
export function isEffort(value: unknown): value is string {
  return typeof value === 'string' && /^[a-z]{2,12}$/.test(value);
}

/** A service tier's id (`priority`, `flex`, …). */
export function isServiceTier(value: unknown): value is string {
  return typeof value === 'string' && /^[a-z][a-z0-9_-]{0,31}$/.test(value);
}

/** The Claude API's models, for the API-key path (no bridge to ask). */
export const ANTHROPIC_API_MODELS: ModelEntry[] = [
  { id: 'claude-opus-5-5', label: 'Claude Opus 5.5', efforts: CLAUDE_EFFORTS },
  { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5', efforts: CLAUDE_EFFORTS },
  { id: 'claude-haiku-5-5', label: 'Claude Haiku 5.5', efforts: CLAUDE_EFFORTS },
  { id: 'claude-fable-5-1', label: 'Claude Fable 5.1', efforts: CLAUDE_EFFORTS },
];

export interface CompletionRequest {
  provider: 'codex' | 'claude';
  system: string;
  messages: import('./conversation').ConversationMessage[];
  model?: string;
  effort?: string;
  serviceTier?: string;
  /** The chat turn this call belongs to, so the agent log can group them. */
  turn?: string;
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
