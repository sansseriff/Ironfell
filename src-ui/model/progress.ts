/**
 * What a running model turn reports while it works (plans/specs/agent-streaming.md §2):
 * the step `ask` is on, and the reply's text and reasoning as the runtime
 * produces them. Events are plain data so they cross the worker boundary.
 */

/** reading: gathering the document; thinking: waiting on the model (attempt > 1
 *  is a repair); looking: answering a `<look>`; applying: the edit's transaction. */
export type AgentPhase = 'reading' | 'thinking' | 'looking' | 'applying';

export type AgentEvent =
  | { kind: 'phase'; phase: AgentPhase; attempt: number }
  | { kind: 'reasoning'; delta: string }
  | { kind: 'text'; delta: string }
  /** The runtime's running estimate of this model call's thinking tokens
   *  (Claude Code reports one while the thinking itself stays hidden). */
  | { kind: 'thinking-tokens'; estimated: number }
  /** This model call's exact output tokens, thinking included, once it ends. */
  | { kind: 'usage'; output: number; thinking?: number };

/** What a live caller passes beside the request: where events go, and how to stop. */
export interface AskLive {
  onEvent?: (event: AgentEvent) => void;
  signal?: AbortSignal;
}

/** Merge a run of deltas of one kind into one event; of running estimates, keep the latest. */
export function mergeEvents(events: AgentEvent[]): AgentEvent[] {
  const out: AgentEvent[] = [];
  for (const event of events) {
    const last = out[out.length - 1];
    if (last && (event.kind === 'text' || event.kind === 'reasoning') && last.kind === event.kind) out[out.length - 1] = { kind: event.kind, delta: last.delta + event.delta };
    else if (last && event.kind === 'thinking-tokens' && last.kind === event.kind) out[out.length - 1] = event;
    else out.push(event);
  }
  return out;
}

/** Collect events and hand them on at most every `ms`, merged, so a fast
 *  model cannot flood `postMessage` or the UI. A phase change goes at once. */
export function eventBatcher(send: (events: AgentEvent[]) => void, ms = 50) {
  let queue: AgentEvent[] = [];
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    if (timer) { clearTimeout(timer); timer = null; }
    if (queue.length) { const events = mergeEvents(queue); queue = []; send(events); }
  };
  return {
    push(event: AgentEvent) {
      queue.push(event);
      if (event.kind === 'phase') flush();
      else timer ??= setTimeout(flush, ms);
    },
    flush,
  };
}

/** The longest a model turn may run, end to end, before its request gives
 *  up. Silence is caught sooner: the bridge's idle timeout and the transport's. */
export const MODEL_ASK_TIMEOUT_MS = 15 * 60_000;

/** Thrown, or reported, when the person stops a turn. */
export const STOPPED = 'stopped';
