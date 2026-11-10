import type { ModelImage } from '../model/images';
import { marked } from 'marked';
import type { AskResult } from '../model/agent';
import { boundedHistory, type ConversationMessage } from '../model/conversation';
import type { AgentEvent, AgentPhase, AskLive } from '../model/progress';

export type NarrativeMode = 'authoring' | 'hybrid' | 'exploration';
export type ContentPart = { kind: 'markdown'; text: string } | { kind: 'view'; show: string; focus: string };

export interface ChatTurn {
  id: string;
  instruction: string;
  images?: ModelImage[];
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
  wrote?: 'prose' | 'edit' | 'revise';
  /** Characters of reply and of shared reasoning this step, for estimates. */
  textChars?: number;
  reasoningChars?: number;
  /** The runtime's own running estimate of hidden thinking tokens. */
  thinkingTokens?: number;
  /** When the first sign of output arrived (any token, estimate or delta). */
  firstTokenAt?: number;
  /** Exact output tokens (thinking included) once the model call ended. */
  usage?: { output: number; thinking?: number };
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
    } else if (event.kind === 'usage') {
      if (step) {
        step.usage = { output: event.output, ...(event.thinking !== undefined ? { thinking: event.thinking } : {}) };
        const replyTokens = event.output - (event.thinking ?? 0);
        if ((step.textChars ?? 0) >= 200 && replyTokens > 0) charsPerToken = Math.min(6, Math.max(1.5, 0.5 * charsPerToken + 0.5 * (step.textChars! / replyTokens)));
      }
    } else if (event.kind === 'thinking-tokens') {
      if (step) { step.thinkingTokens = event.estimated; step.firstTokenAt ??= now; }
    } else if (event.kind === 'reasoning') {
      if (step) { step.reasoning += event.delta; step.reasoningChars = (step.reasoningChars ?? 0) + event.delta.length; step.firstTokenAt ??= now; }
    } else {
      progress.text += event.delta;
      if (step) { step.textChars = (step.textChars ?? 0) + event.delta.length; step.firstTokenAt ??= now; }
      if (step) {
        const formed = streamView(progress.text).edit;
        step.wrote = formed !== null ? (formed.startsWith('<revise') ? 'revise' : 'edit') : progress.text.trim() ? 'prose' : step.wrote;
      }
    }
  }
  return progress;
}

/** Close the last step when the turn ends. */
export function finishProgress(progress: TurnProgress | undefined, now: number) {
  const step = progress?.steps[progress.steps.length - 1];
  if (step && step.endedAt === undefined) step.endedAt = now;
}

/** Characters a reply token, for the live estimate. It starts at 3 and is
 *  recalibrated from each exact count: measured 2026-11-06, Codex wrote ~3.4
 *  and Claude ~2 (units and symbols such as Ω cost more). */
let charsPerToken = 3;

/** A step's output tokens: exact once its model call ended, else estimated
 *  from the runtime's thinking estimate (or shared reasoning) and the reply. */
export function stepTokens(step: WorkStep): { tokens: number; thinking: number; exact: boolean } {
  if (step.usage) return { tokens: step.usage.output, thinking: step.usage.thinking ?? 0, exact: true };
  const thinking = step.thinkingTokens ?? Math.ceil((step.reasoningChars ?? 0) / charsPerToken);
  return { tokens: thinking + Math.ceil((step.textChars ?? 0) / charsPerToken), thinking, exact: false };
}

/** A turn's output tokens over its model calls, and their rate: per second of
 *  generating (from the first token) while running, per second of model time once done. */
export function turnTokens(progress: TurnProgress, now: number, running: boolean): { tokens: number; thinking: number; exact: boolean; perSecond: number | null } {
  let tokens = 0, thinking = 0, exact = true, seconds = 0;
  for (const step of progress.steps) {
    if (step.phase !== 'thinking') continue;
    const counted = stepTokens(step);
    tokens += counted.tokens;
    thinking += counted.thinking;
    exact &&= counted.exact;
    const from = running ? step.firstTokenAt : step.startedAt;
    if (from !== undefined) seconds += ((step.endedAt ?? now) - from) / 1000;
  }
  return { tokens, thinking, exact: exact && tokens > 0, perSecond: seconds >= 1 && tokens > 0 ? tokens / seconds : null };
}

/** "1,234" or "12.3k". */
export function formatTokens(n: number): string {
  return n < 10000 ? n.toLocaleString('en-US') : `${(n / 1000).toFixed(1)}k`;
}

/** What a step is doing (running) or did (done), in a few words. */
export function stepLabel(step: WorkStep, done: boolean): string {
  switch (step.phase) {
    case 'reading': return done ? 'Read the document' : 'Reading the document';
    case 'looking': return done ? 'Looked inside components' : 'Looking inside components';
    case 'applying': return done ? 'Applied the edit' : 'Applying the edit';
    case 'thinking': {
      const repair = step.attempt > 1;
      if (step.wrote === 'revise') return done ? 'Revised the rejected edit' : 'Revising the rejected edit';
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
  for (const match of text.matchAll(/<(?:edit|look|revise)[\s>/]/g)) {
    // An edit inside a fenced code block is an example, not the edit.
    if ((text.slice(0, match.index).match(/```/g)?.length ?? 0) % 2 === 0) { start = match.index!; break; }
  }
  if (start < 0) return { prose: holdBack(text), edit: null, verbs: [] };
  if (text.startsWith('<look', start)) {
    // A look asks to see inside instances; it is not shown as prose or an edit.
    const gt = text.indexOf('>', start);
    return { prose: text.slice(0, start) + (gt < 0 ? '' : holdBack(text.slice(gt + 1))), edit: null, verbs: [] };
  }
  // A <revise> patches the rejected edit; it forms like an edit.
  const closeTag = text.startsWith('<revise', start) ? '</revise>' : '</edit>';
  const close = text.indexOf(closeTag, start);
  const end = close < 0 ? text.length : close + closeTag.length;
  const edit = text.slice(start, end);
  const after = close < 0 ? '' : holdBack(text.slice(end));
  const prose = text.slice(0, start) + after;
  return { prose: prose.trim() ? prose : '', edit, verbs: editVerbs(edit) };
}

function holdBack(text: string): string {
  const lt = text.lastIndexOf('<');
  if (lt >= 0 && !text.slice(lt).includes('>')) {
    const tail = text.slice(lt);
    if (['<edit', '<look', '<revise'].some(tag => tag.startsWith(tail) || tail.startsWith(tag))) text = text.slice(0, lt);
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
    if (name === 'edit' || name === 'revise') continue;
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
    case 'change': return `Change ${a.op ?? ''}${Object.keys(a).filter(k => k !== 'op').length ? ` · ${Object.keys(a).filter(k => k !== 'op').slice(0, 4).join(', ')}` : ''}`;
    case 'replace': return `Replace ${a.op ?? ''}`;
    case 'drop': return `Drop ${a.op ?? ''}`;
    case 'insert': return `Insert ${a.after !== undefined ? `after ${a.after}` : `before ${a.before ?? ''}`}`;
    case 'text': return `Replace text "${(a.old ?? '').slice(0, 30)}"${/^(true|yes|1)$/i.test(a.all ?? '') ? ' everywhere' : ''}`;
    default: return verb;
  }
}

export interface ChatSession {
  draft: string;
  draftImages?: ModelImage[];
  turns: ChatTurn[];
  annotations: Record<string, unknown>;
}

/** Set a conversation's unsent draft. The conversation belongs to its
 *  panel's history (sessions.svelte.ts), not to the component showing it. */
export function setDraft(session: ChatSession, draft: string) { session.draft = draft; }

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

/** What "Copy turn log" puts on the clipboard: the turn as the chat saw it
 *  (steps, tokens, each reply's fate, the final edit). The prompts and raw
 *  replies are in the bridge's agent log, found by `turn`. */
export function turnLog(turn: ChatTurn, now = Date.now()) {
  const day = new Date(turn.startedAt);
  const file = `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, '0')}-${String(day.getDate()).padStart(2, '0')}.jsonl`;
  const progress = turn.progress;
  return {
    turn: turn.id,
    agentLog: `.agents/agent-logs/${file} (records with this turn)`,
    instruction: turn.instruction,
    target: turn.target,
    status: turn.status,
    startedAt: new Date(turn.startedAt).toISOString(),
    seconds: Number((((turn.completedAt ?? now) - turn.startedAt) / 1000).toFixed(1)),
    transport: turn.result?.transport ?? null,
    error: turn.error || turn.result?.error || null,
    attempts: turn.result?.attempts ?? null,
    tokens: progress ? turnTokens(progress, turn.completedAt ?? now, turn.status === 'running') : null,
    steps: progress?.steps.map(step => ({
      step: stepLabel(step, step.endedAt !== undefined),
      attempt: step.attempt,
      seconds: Number((((step.endedAt ?? now) - step.startedAt) / 1000).toFixed(1)),
      ...(step.phase === 'thinking' ? { tokens: stepTokens(step) } : {}),
      ...(step.reasoning ? { reasoning: step.reasoning } : {}),
    })) ?? [],
    trace: turn.result?.trace ?? [],
    note: turn.result?.note || null,
    edit: turn.result?.edit ?? null,
    failure: turn.result?.outcome?.failure ?? null,
    // A turn still running, or stopped: what had streamed so far.
    ...(turn.status === 'running' || turn.status === 'stopped' ? { streamed: progress?.text ?? '' } : {}),
  };
}

/** A rejected edit's `<edit-failed>` markup as a sentence: its errors' text,
 *  unescaped; the markup itself stays in the edit details. */
/** The edits a turn applied before it ended, whatever its status: a stopped
 *  or failed turn can have applied earlier ones (a picture refinement). */
export function appliedEdits(turn: Pick<ChatTurn, 'result'>): { summary?: string }[] {
  return (turn.result?.trace ?? []).filter(step => step.kind === 'edit' && step.ok) as { summary?: string }[];
}

/** `applied`: earlier edits in the turn went in, so only the last was rejected. */
export function failureSummary(failure: string, attempts = 1, applied = false): string {
  const errors = [...failure.matchAll(/<error\b[^>]*>([\s\S]*?)<\/error>/g)]
    .map(m => m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&').trim())
    .filter(Boolean);
  const revise = failure.match(/<revise-failed>([\s\S]*?)<\/revise-failed>/);
  if (!errors.length && revise) errors.push(`the revise could not be applied: ${revise[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&').trim()}`);
  if (!errors.length) return failure.trim() || 'The request could not be completed.';
  const tries = attempts > 1 ? ` after ${attempts} attempts` : '';
  return applied
    ? `Earlier edits in this turn remain applied; the next edit was rejected${tries}, so that edit was not applied: ${errors.join('; ')}`
    : `The edit was rejected${tries}, so nothing changed: ${errors.join('; ')}`;
}

export function safeChatUrl(value: string, image = false): string | null {
  try {
    const url = new URL(value);
    return (image ? ['https:'] : ['https:', 'http:', 'mailto:']).includes(url.protocol) && !url.username && !url.password ? url.href : null;
  } catch { return null; }
}

export function chatHistory(turns: ChatTurn[]): ConversationMessage[] {
  // A turn that changed the document belongs in the history even if it was
  // stopped or failed later on, so the next request knows those edits happened.
  return boundedHistory(turns.filter(turn => (turn.status === 'completed' && turn.result?.ok) || appliedEdits(turn).length).flatMap(turn => [
    { role: 'user' as const, content: turn.instruction, ...(turn.images?.length ? { images: turn.images } : {}) },
    { role: 'assistant' as const, content: turn.status === 'completed' && turn.result?.ok
      ? [turn.result?.note, turn.result?.edit].filter(Boolean).join('\n\n') || 'The requested document edit was applied.'
      : interruptedSummary(turn) },
  ]));
}

function interruptedSummary(turn: ChatTurn): string {
  const edits = appliedEdits(turn);
  const summaries = edits.map(edit => edit.summary).filter(Boolean);
  return `[This request ${turn.status === 'stopped' ? 'was stopped' : 'failed'} after ${edits.length} applied edit${edits.length === 1 ? '' : 's'}, which remain in the document${summaries.length ? `: ${summaries.join('; ')}` : ''}.]`;
}

/** Enter sends, as T3 Code's composer (Cmd/Ctrl+Enter too); Shift+Enter or
 *  Alt+Enter is a new line. Never while an IME is composing: WebKit reports
 *  the Enter that ends a composition with keyCode 229 and isComposing false. */
export function shouldSubmit(event: Pick<KeyboardEvent, 'key' | 'isComposing' | 'repeat' | 'shiftKey' | 'altKey'> & { keyCode?: number }): boolean {
  return event.key === 'Enter' && !event.isComposing && event.keyCode !== 229 && !event.repeat && !event.shiftKey && !event.altKey;
}

export function nearBottom(top: number, height: number, viewport: number): boolean {
  return height - top - viewport < 64;
}
