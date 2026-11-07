/**
 * One request, one edit: read a view, ask the model, apply what it wrote,
 * and give it two chances to fix a rejected edit (doc 04 §9.5), each a
 * <revise> of the rejected edit or a new one.
 */
import type { DocumentApi } from './api';
import { applyEdit, extractEdit, type EditOutcome } from './edit';
import { manifest } from './manifest';
import { extractRevise, revise } from './revise';
import type { ModelTransport } from './transport';
import { boundedHistory, executableReply, type ConversationOptions } from './conversation';
import { STOPPED, type AskLive } from './progress';

export interface AskResult {
  ok: boolean;
  /** What the model said outside the edit block, if anything. */
  note: string;
  edit: string | null;
  outcome: EditOutcome | null;
  attempts: number;
  transport: string;
  error?: string;
  /** Each model reply and what became of it, in order (the agent log keeps it). */
  trace?: AskStep[];
}

/** One model reply in a turn: a look, prose, an edit (applied or rejected), or why it ended. */
export type AskStep =
  | { attempt: number; kind: 'look'; ids: string[] }
  | { attempt: number; kind: 'prose' }
  | { attempt: number; kind: 'edit'; ok: boolean; failure?: string; summary?: string; version?: number; revised?: boolean }
  | { attempt: number; kind: 'revise'; ok: false; failure: string }
  | { attempt: number; kind: 'no-edit' | 'stale' | 'stopped' | 'error'; message?: string };

/** One request, with the turn's outcome sent to the transport's log (the
 *  bridge's agent log for local runtimes) when it ends. */
export async function ask(api: DocumentApi, transport: ModelTransport, instruction: string, options: ConversationOptions = {}, live: AskLive = {}): Promise<AskResult> {
  const started = Date.now();
  const trace: AskStep[] = [];
  let result: AskResult | undefined;
  let thrown: unknown;
  try {
    result = await runAsk(api, transport, instruction, options, live, trace);
    result.trace = trace;
    return result;
  } catch (error) {
    thrown = error;
    throw error;
  } finally {
    if (options.turnId && transport.log) {
      void transport.log({
        turn: options.turnId, instruction, transport: transport.name, ms: Date.now() - started,
        ok: result?.ok ?? false, attempts: result?.attempts ?? 0,
        error: result?.error ?? (thrown === undefined ? undefined : String(thrown)),
        note: result?.note || undefined, edit: result?.edit ?? undefined,
        outcome: result?.outcome ? { ok: result.outcome.ok, version: result.outcome.version, failure: result.outcome.failure, summary: result.outcome.summary } : undefined,
        trace, historyMessages: options.history?.length ?? 0,
      });
    }
  }
}

async function runAsk(api: DocumentApi, transport: ModelTransport, instruction: string, options: ConversationOptions, live: AskLive, trace: AskStep[]): Promise<AskResult> {
  const emit = live.onEvent ?? (() => {});
  emit({ kind: 'phase', phase: 'reading', attempt: 1 });
  const snapshotVersion = await api.version();
  const system = manifest(await api.schema(), options.allowTextReply) + (options.allowTextReply ? '\n\nCONVERSATION\nTo show an existing Iron composition, put ::view{show="composition name" focus=#node} on its own paragraph (focus optional). Never invent visual references or embed raw HTML. Recent conversation is context only; the final document view is current.\n' : '');
  // The model view first (what things are and where each is drawn, ECM §11),
  // then the tree, which also holds what no composition does (the library,
  // timelines, loose roots).
  const tree = await api.view({ fidelity: 'full' });
  // Each schematic sheet's netlist too: what is wired to what, and the
  // solved voltages (plans/specs/schematic.md §5).
  const sheets = [...tree.matchAll(/<composition id="[^"]+" name="([^"]+)" notation="schematic"/g)].map((m) => m[1]);
  const netlists = (await Promise.all(sheets.map((s) => api.netlist(s)))).filter((n) => n.ok).map((n) => `<netlist>\n${n.netlist}\n</netlist>`);
  const view = [await api.view({ fidelity: 'full', relation: 'model' }), tree, ...netlists].join('\n');
  // What the person has selected, so "this" and "the selected one" resolve.
  const picked = await api.selection();
  const selection = picked.length
    ? `SELECTED\n${picked.map((p) => `${p.node} (${p.type ?? 'node'}${p.name ? ` "${p.name}"` : ''})`).join('\n')}\n\n`
    : '';
  const messages: { role: 'user' | 'assistant'; content: string }[] = [
    ...boundedHistory(options.history),
    { role: 'user', content: `${view}\n\n${selection}REQUEST\n${instruction}` },
  ];
  let attempts = 0;
  let lastEdit: string | null = null;
  /** The edit the store last rejected, as written: what a <revise> patches. */
  let rejected: string | null = null;
  let lastOutcome: EditOutcome | null = null;
  let note = '';
  let looks = 0;
  const stopped = () => {
    trace.push({ attempt: attempts, kind: 'stopped' });
    return { ok: false, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, error: STOPPED };
  };
  while (attempts < MAX_ATTEMPTS) {
    attempts += 1;
    if (live.signal?.aborted) return stopped();
    emit({ kind: 'phase', phase: 'thinking', attempt: attempts });
    let text: string;
    try {
      text = (await transport.complete(system, messages, { onEvent: live.onEvent, signal: live.signal, turn: options.turnId })).text;
    } catch (e) {
      if (live.signal?.aborted) return stopped();
      // The message alone: a bridge error already reads "Error: …".
      const message = (e instanceof Error ? e.message : String(e)).replace(/^(Error: )+/, '');
      trace.push({ attempt: attempts, kind: 'error', message });
      return { ok: false, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, error: message };
    }
    // A reply that arrives after Stop is not applied.
    if (live.signal?.aborted) return stopped();
    // `<look expand="#u #e"/>` with no edit: show those instances' derived
    // insides (component-model.md §9) and ask again; not a repair attempt.
    const look = lookRequest(text);
    if (look && !extractEdit(text) && looks < MAX_LOOKS) {
      looks += 1;
      trace.push({ attempt: attempts, kind: 'look', ids: look });
      attempts -= 1;
      messages.push({ role: 'assistant', content: text });
      emit({ kind: 'phase', phase: 'looking', attempt: attempts + 1 });
      messages.push({ role: 'user', content: await lookView(api, look) });
      continue;
    }
    const executable = options.allowTextReply ? executableReply(text) : text;
    let edit = extractEdit(executable);
    // A <revise> patches the edit just rejected (revise.ts): the full edit it
    // makes is what is applied, shown and logged.
    const patch = !edit && rejected ? extractRevise(executable) : null;
    note = text.replace(edit ?? patch ?? '', '').trim();
    if (patch && rejected) {
      const revised = revise(rejected, patch);
      if (!revised.ok) {
        const failure = `<revise-failed>${revised.error.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</revise-failed>`;
        trace.push({ attempt: attempts, kind: 'revise', ok: false, failure });
        lastOutcome = { ok: false, failure };
        messages.push({ role: 'assistant', content: text });
        messages.push({ role: 'user', content: `${failure}\n\nNothing was applied. ${REPAIR}` });
        continue;
      }
      edit = revised.edit;
    }
    if (!edit) {
      const prose = !!(options.allowTextReply && note && attempts === 1);
      trace.push(prose ? { attempt: attempts, kind: 'prose' } : { attempt: attempts, kind: 'no-edit' });
      if (prose) return { ok: true, note, edit: null, outcome: null, attempts, transport: transport.name };
      return { ok: false, note, edit: null, outcome: null, attempts, transport: transport.name, error: 'the reply had no <edit> block' };
    }
    lastEdit = edit;
    if (await api.version() !== snapshotVersion) {
      trace.push({ attempt: attempts, kind: 'stale' });
      return { ok: false, note, edit, outcome: null, attempts, transport: transport.name, error: 'document changed while the agent was thinking; ask again with the current document' };
    }
    emit({ kind: 'phase', phase: 'applying', attempt: attempts });
    const guardedApi = { ...api, apply: (transaction: object) => api.apply(transaction, snapshotVersion) };
    lastOutcome = await applyEdit(guardedApi, edit, `model: ${instruction.slice(0, 60)}`);
    trace.push({ attempt: attempts, kind: 'edit', ok: lastOutcome.ok, ...(lastOutcome.failure ? { failure: lastOutcome.failure } : {}), ...(lastOutcome.summary ? { summary: lastOutcome.summary } : {}), ...(lastOutcome.version !== undefined ? { version: lastOutcome.version } : {}), ...(patch ? { revised: true } : {}) });
    if (lastOutcome.ok) break;
    rejected = edit;
    messages.push({ role: 'assistant', content: text });
    // A revised edit is shown whole, so a second revise's op numbers and the
    // errors' match what the model sees.
    const shown = patch ? `Your revise made this edit, which was rejected:\n${edit}\n\n` : '';
    messages.push({ role: 'user', content: `${shown}${lastOutcome.failure}\n\n${REPAIR}` });
  }
  return { ok: !!lastOutcome?.ok, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name };
}

/** Model replies one request may take: the edit and two repairs. A repair
 *  is usually a short <revise>, so a second costs little. */
const MAX_ATTEMPTS = 3;

/** What a rejected edit's reply asks for. */
const REPAIR = 'Fix what it names with a <revise> of that edit (only the ops that change), or send one new <edit> if most of it must change.';

/** Looks one request may take before it must answer. */
const MAX_LOOKS = 2;

/** The ids a reply's `<look expand="…"/>` names, if it has one. */
export function lookRequest(text: string): string[] | null {
  const m = text.match(/<look\s[^>]*?expand="([^"]*)"[^>]*\/?>/);
  if (!m) return null;
  const ids = m[1].split(/\s+/).filter(Boolean).map((id) => (id.startsWith('#') ? id : `#${id}`));
  return ids.length ? ids : null;
}

/** The view a look asks for: each named instance's subtree with what it
 *  derives shown, marked `derived`. */
async function lookView(api: DocumentApi, ids: string[]): Promise<string> {
  const parts = await Promise.all(ids.map(async (id) => {
    try {
      return await api.view({ fidelity: 'full', scope: id, expand: [id] });
    } catch (e) {
      return `<look-failed id="${id}">${String(e).replace(/[<>&]/g, '')}</look-failed>`;
    }
  }));
  return `LOOK\n${parts.join('\n')}\n\nNow answer the REQUEST: one <edit>, or prose if no change is needed.`;
}
