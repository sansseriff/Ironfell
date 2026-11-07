import { marked } from 'marked';
import type { AskResult } from '../model/agent';
import { boundedHistory, type ConversationMessage } from '../model/conversation';
import type { AgentEvent, AgentPhase, AskLive } from '../model/progress';

export type NarrativeMode = 'authoring' | 'hybrid' | 'exploration';
export type ContentPart = { kind: 'markdown'; text: string } | { kind: 'view'; show: string; focus: string };

export interface ChatTurn {
  id: string;
  instruction: string;
  target: string;
  status: 'running' | 'completed' | 'failed' | 'stopped';
  startedAt: number;
  completedAt?: number;
  result?: AskResult;
  error?: string;
  /** What the turn did while it ran: its steps and the reply as it formed. */
  progress?: TurnProgress;
}

/** One step of a running turn (plans/specs/agent-streaming.md §3). */
export interface WorkStep {
  phase: AgentPhase;
  attempt: number;
  startedAt: number;
  endedAt?: number;
  /** Reasoning the runtime shared during this step (summaries or thinking). */
  reasoning: string;
  /** What the model wrote during this step, once it started writing. */
  wrote?: 'prose' | 'edit';
}

export interface TurnProgress {
  steps: WorkStep[];
  /** The current model reply as it streams; a new model step starts it afresh. */
  text: string;
}

/** Fold a running turn's events into its progress, in place (it may be a
 *  Svelte state proxy), and return it. */
export function applyAgentEvents(progress: TurnProgress, events: AgentEvent[], now: number): TurnProgress {
  for (const event of events) {
    const step = progress.steps[progress.steps.length - 1];
    if (event.kind === 'phase') {
      if (step && step.endedAt === undefined) step.endedAt = now;
      progress.steps.push({ phase: event.phase, attempt: event.attempt, startedAt: now, reasoning: '' });
      if (event.phase === 'thinking') progress.text = '';
    } else if (event.kind === 'reasoning') {
      if (step) step.reasoning += event.delta;
    } else {
      progress.text += event.delta;
      if (step) step.wrote = streamView(progress.text).edit !== null ? 'edit' : progress.text.trim() ? 'prose' : step.wrote;
    }
  }
  return progress;
}

/** Close the last step when the turn ends. */
export function finishProgress(progress: TurnProgress | undefined, now: number) {
  const step = progress?.steps[progress.steps.length - 1];
  if (step && step.endedAt === undefined) step.endedAt = now;
}

/** What a step is doing (running) or did (done), in a few words. */
export function stepLabel(step: WorkStep, done: boolean): string {
  switch (step.phase) {
    case 'reading': return done ? 'Read the document' : 'Reading the document';
    case 'looking': return done ? 'Looked inside components' : 'Looking inside components';
    case 'applying': return done ? 'Applied the edit' : 'Applying the edit';
    case 'thinking': {
      const repair = step.attempt > 1;
      if (step.wrote === 'edit') return repair ? (done ? 'Rewrote the rejected edit' : 'Rewriting the rejected edit') : done ? 'Wrote the edit' : 'Writing the edit';
      if (step.wrote === 'prose') return done ? 'Replied' : 'Writing';
      return repair ? (done ? 'Reconsidered the rejected edit' : 'Reconsidering the rejected edit') : done ? 'Thought' : 'Thinking';
    }
  }
}

/** A streaming reply split for display: the prose so far (with any
 *  half-written `<edit`, `<look` or `::view` held back so markup never
 *  flashes as text), the edit markup if one has begun, and its verbs that
 *  have closed. */
export interface StreamView { prose: string; edit: string | null; verbs: string[] }

export function streamView(text: string): StreamView {
  let start = -1;
  for (const match of text.matchAll(/<(?:edit|look)[\s>/]/g)) {
    // An edit inside a fenced code block is an example, not the edit.
    if ((text.slice(0, match.index).match(/```/g)?.length ?? 0) % 2 === 0) { start = match.index!; break; }
  }
  if (start < 0) return { prose: holdBack(text), edit: null, verbs: [] };
  if (text.startsWith('<look', start)) {
    // A look asks to see inside instances; it is not shown as prose or an edit.
    const gt = text.indexOf('>', start);
    return { prose: text.slice(0, start) + (gt < 0 ? '' : holdBack(text.slice(gt + 1))), edit: null, verbs: [] };
  }
  const close = text.indexOf('</edit>', start);
  const end = close < 0 ? text.length : close + '</edit>'.length;
  const edit = text.slice(start, end);
  const after = close < 0 ? '' : holdBack(text.slice(end));
  const prose = text.slice(0, start) + after;
  return { prose: prose.trim() ? prose : '', edit, verbs: editVerbs(edit) };
}

function holdBack(text: string): string {
  const lt = text.lastIndexOf('<');
  if (lt >= 0 && !text.slice(lt).includes('>')) {
    const tail = text.slice(lt);
    if ('<edit'.startsWith(tail) || '<look'.startsWith(tail) || tail.startsWith('<edit') || tail.startsWith('<look')) text = text.slice(0, lt);
  }
  const line = text.slice(text.lastIndexOf('\n') + 1);
  if (line.startsWith(':')) text = text.slice(0, text.length - line.length);
  return text;
}

/** The top-level verbs of edit markup whose tags have closed, described. */
export function editVerbs(edit: string): string[] {
  const verbs: string[] = [];
  let depth = 0;
  for (const tag of edit.matchAll(/<(\/?)([a-zA-Z][\w-]*)((?:\s+[\w.:-]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*(\/?)>/g)) {
    const [, closing, name, attrText, selfClosing] = tag;
    if (name === 'edit') continue;
    if (closing) { depth = Math.max(0, depth - 1); continue; }
    if (depth === 0) verbs.push(describeVerb(name, attrText));
    if (!selfClosing) depth++;
  }
  return verbs;
}

function describeVerb(verb: string, attrText: string): string {
  const a: Record<string, string> = {};
  for (const m of attrText.matchAll(/([\w.:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) a[m[1]] = m[2] ?? m[3];
  const named = a.name ? ` "${a.name}"` : '';
  switch (verb) {
    case 'create': return `Create ${a.type ?? 'node'}${named}`;
    case 'set': case 'add': {
      const leaves = Object.keys(a).filter(k => !['id', 'type', 'path', 'value', 'bind'].includes(k));
      if (a.path) leaves.unshift(a.path);
      return `${verb === 'set' ? 'Set' : 'Add to'} ${a.id ?? ''}${leaves.length ? ` · ${leaves.slice(0, 4).join(', ')}${leaves.length > 4 ? ', …' : ''}` : ''}`;
    }
    case 'delete': return `Delete ${a.id ?? ''}`;
    case 'reparent': return `Move ${a.id ?? ''} into ${a.to ?? ''}`;
    case 'link': return `Link ${a.from ?? ''} ${a.rel ?? 'references'} ${a.to ?? ''}`;
    case 'unlink': return `Unlink ${a.id ?? ''}`;
    case 'place': return `Place ${a.def ?? a.entity ?? a.part ?? ''}${named}${a.in ? ` in ${a.in}` : ''}`;
    case 'make-component': return `Make component${named}`;
    case 'link-figures': return 'Link figures';
    case 'import-svg': return 'Import SVG';
    case 'table': return 'Make a table';
    case 'circuit': return 'Build a circuit';
    case 'write': case 'rewrite': return 'Write narrative';
    default: return verb;
  }
}

export interface ChatSession {
  draft: string;
  turns: ChatTurn[];
  annotations: Record<string, unknown>;
}

/** Run one turn. `live` carries the progress callback and the Stop signal. */
export type SendTurn = (text: string, history: ConversationMessage[], turn: ChatTurn, live: AskLive) => Promise<AskResult>;
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
