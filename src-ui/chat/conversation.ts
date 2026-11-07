import { marked } from 'marked';
import type { AskResult } from '../model/agent';
import { boundedHistory, type ConversationMessage } from '../model/conversation';

export type NarrativeMode = 'authoring' | 'hybrid' | 'exploration';
export type ContentPart = { kind: 'markdown'; text: string } | { kind: 'view'; show: string; focus: string };

export interface ChatTurn {
  id: string;
  instruction: string;
  target: string;
  status: 'running' | 'completed' | 'failed';
  startedAt: number;
  completedAt?: number;
  result?: AskResult;
  error?: string;
}

export interface ChatSession {
  draft: string;
  turns: ChatTurn[];
  annotations: Record<string, unknown>;
}

export type SendTurn = (text: string, history: ConversationMessage[], turn: ChatTurn) => Promise<AskResult>;
export interface ChatHandle { submit(text?: string, execute?: SendTurn): Promise<ChatTurn | null> }

export function narrativeMode(value: string | null): NarrativeMode {
  return value === 'hybrid' || value === 'exploration' ? value : 'authoring';
}

export function contentParts(text: string): ContentPart[] {
  const result: ContentPart[] = [];
  let prose = '';
  for (const token of marked.lexer(text)) {
    const view = token.type === 'paragraph' && token.raw.trim().match(/^::view\{\s*show=(?:"([^"\n]+)"|'([^'\n]+)'|([^\s}"']+))(?:\s+focus=(#[\w/]+))?\s*\}$/);
    if (view) {
      if (prose.trim()) result.push({ kind: 'markdown', text: prose });
      prose = '';
      result.push({ kind: 'view', show: (view[1] ?? view[2] ?? view[3]).slice(0, 200), focus: view[4] ?? '' });
    } else prose += token.raw;
  }
  if (prose.trim()) result.push({ kind: 'markdown', text: prose });
  return result;
}

/** A rejected edit's `<edit-failed>` markup as a sentence: its errors' text,
 *  unescaped; the markup itself stays in the edit details. */
export function failureSummary(failure: string, attempts = 1): string {
  const errors = [...failure.matchAll(/<error\b[^>]*>([\s\S]*?)<\/error>/g)]
    .map(m => m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&').trim())
    .filter(Boolean);
  if (!errors.length) return failure.trim() || 'The request could not be completed.';
  const tries = attempts > 1 ? ` after ${attempts} attempts` : '';
  return `The edit was rejected${tries}, so nothing changed: ${errors.join('; ')}`;
}

export function safeChatUrl(value: string, image = false): string | null {
  try {
    const url = new URL(value);
    return (image ? ['https:'] : ['https:', 'http:', 'mailto:']).includes(url.protocol) && !url.username && !url.password ? url.href : null;
  } catch { return null; }
}

export function chatHistory(turns: ChatTurn[]): ConversationMessage[] {
  return boundedHistory(turns.filter(turn => turn.status === 'completed' && turn.result?.ok).flatMap(turn => [
    { role: 'user' as const, content: turn.instruction },
    { role: 'assistant' as const, content: [turn.result?.note, turn.result?.edit].filter(Boolean).join('\n\n') || 'The requested document edit was applied.' },
  ]));
}

export function shouldSubmit(event: Pick<KeyboardEvent, 'key' | 'isComposing' | 'repeat' | 'shiftKey' | 'altKey' | 'metaKey' | 'ctrlKey'>): boolean {
  return event.key === 'Enter' && !event.isComposing && !event.repeat && !event.shiftKey && !event.altKey && (event.metaKey || event.ctrlKey);
}

export function nearBottom(top: number, height: number, viewport: number): boolean {
  return height - top - viewport < 64;
}
