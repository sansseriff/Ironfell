/**
 * One request: read a view, ask the model, apply what it wrote. A rejected
 * edit gets up to MAX_REPAIRS repairs in a row (doc 04 §9.5), each a
 * <revise> of it or a new edit; an applied edit ends the turn unless the
 * reply asks for a picture or says <continue/>. Every model call counts
 * against MAX_CALLS.
 */
import type { DocumentApi } from './api';
import { applyEdit, type EditOutcome } from './edit';
import { manifest, topicsFor, topicText } from './manifest';
import { changesSince, conflicts } from './merge';
import { validImages, wireImages } from './images';
import type { ModelTransport } from './transport';
import { boundedHistory, executableReply, type ConversationOptions } from './conversation';
import { STOPPED, type AgentEvent, type AskLive } from './progress';

export interface AskResult {
  ok: boolean;
  /** What the model said outside the edit block, if anything. */
  note: string;
  edit: string | null;
  outcome: EditOutcome | null;
  attempts: number;
  transport: string;
  error?: string;
  /** Changes the person (or anyone) made while the agent worked that its
   *  applied edit was merged with: transactions between its read and its apply. */
  merged?: number;
  /** Each model reply and what became of it, in order (the agent log keeps it). */
  trace?: AskStep[];
}

/** One model reply in a turn: a look, prose, an edit (applied or rejected), or why it ended. */
export type AskStep =
  | { attempt: number; kind: 'look'; ids: string[] }
  | { attempt: number; kind: 'picture'; source: string; ok: boolean }
  | { attempt: number; kind: 'topic'; names: string[] }
  | { attempt: number; kind: 'prose' }
  | { attempt: number; kind: 'edit'; ok: boolean; failure?: string; summary?: string; version?: number; revised?: boolean; merged?: number }
  | { attempt: number; kind: 'revise'; ok: false; failure: string }
  | { attempt: number; kind: 'no-edit' | 'stale' | 'stopped' | 'error'; message?: string };

/** One request, with the turn's outcome sent to the transport's log (the
 *  bridge's agent log for local runtimes) when it ends. */
export async function ask(api: DocumentApi, transport: ModelTransport, instruction: string, options: ConversationOptions = {}, live: AskLive = {}): Promise<AskResult> {
  const started = Date.now();
  const trace: AskStep[] = [];
  let result: AskResult | undefined;
  let thrown: unknown;
  // Every ask is a turn to the bridge, so its calls share one runtime session.
  const turn = options.turnId ?? `ask-${started.toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  try {
    result = await runAsk(api, transport, instruction, { ...options, turnId: turn }, live, trace);
    result.trace = trace;
    return result;
  } catch (error) {
    thrown = error;
    throw error;
  } finally {
    void transport.end?.(turn);
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
  if (options.images !== undefined && !validImages(options.images)) throw new Error('Invalid image attachments');
  // A saved preview (chat/history.ts) is shown, never sent as the reference.
  const attached = wireImages(options.images);
  const emit = live.onEvent ?? (() => {});
  emit({ kind: 'phase', phase: 'reading', attempt: 1 });
  const snapshotVersion = await api.version();
  const schema = await api.schema();
  // The model view first (what things are and where each is drawn, ECM §11),
  // then the tree, which also holds what no composition does (the library,
  // timelines, loose roots).
  const tree = await api.view({ fidelity: 'full', omit_defaults: true });
  // Each schematic sheet's netlist too: what is wired to what, and the
  // solved voltages (plans/specs/schematic.md §5).
  const netlists = ((await api.netlists()).netlists ?? []).map((n) => `<netlist>\n${n.netlist}\n</netlist>`);
  const view = [await api.view({ fidelity: 'full', relation: 'model', omit_defaults: true }), tree, ...netlists].join('\n');
  // The prompt in topics (manifest.ts): the core, the topics this document
  // and request make relevant (recent conversation counts as the request),
  // and an index of the rest, which the model may ask for with <topic>.
  const asked = [instruction, ...(options.history ?? []).slice(-4).map((m) => m.content)].join('\n');
  const topics = topicsFor(view, asked);
  const system = manifest(schema, options.allowTextReply, topics) + (api.picture ? `\nVISION\nYou can inspect actual 2D composition pixels with <look picture="composition name or #id"/>. It captures the whole content on white, without editor overlays, at up to 1568 pixels. 3D capture is unavailable. To check a visual edit, append a look request after </edit>; the edit is applied before capture, and you may then refine it or finish with prose. At most ${MAX_PICTURES} pictures per request. Attached user images are visual references.\n` : '') + (options.allowTextReply ? '\n\nCONVERSATION\nTo show an existing Iron composition, put ::view{show="composition name" focus=#node} on its own paragraph (focus optional). Never invent visual references or embed raw HTML. Recent conversation is context only; the final document view is current.\n' : '');
  // What the person has selected, so "this" and "the selected one" resolve.
  const picked = await api.selection();
  const selection = picked.length
    ? `SELECTED\n${picked.map((p) => `${p.node} (${p.type ?? 'node'}${p.name ? ` "${p.name}"` : ''})`).join('\n')}\n\n`
    : '';
  const messages: import('./conversation').ConversationMessage[] = [
    ...boundedHistory(options.history),
    { role: 'user', content: `${view}\n\n${selection}REQUEST\n${instruction}`, ...(attached.length ? { images: attached } : {}) },
  ];
  let attempts = 0;
  /** Model calls this turn, of any kind. */
  let calls = 0;
  /** Edits rejected since the last one applied. */
  let rejections = 0;
  let lastEdit: string | null = null;
  /** The edit the store last rejected, as written: what a <revise> patches. */
  let rejected: string | null = null;
  /** The version the agent knows of: what it read, or newer once told of a conflict. */
  let since = snapshotVersion;
  let cycleVersion = snapshotVersion;
  let mergedWith = 0;
  let lastOutcome: EditOutcome | null = null;
  let note = '';
  let looks = 0;
  let pictures = 0;
  let toldNoPictures = false;
  let applied = false;
  let lastApplied: { edit: string; outcome: EditOutcome } | null = null;
  const pictureMessage = async (source: string) => {
    emit({ kind: 'phase', phase: 'capturing', attempt: attempts });
    pictures++;
    try {
      if (!api.picture) throw new Error('Composition capture is unavailable');
      const { node, ...image } = await api.picture(source, live.signal);
      trace.push({ attempt: attempts, kind: 'picture', source, ok: true });
      emit({ kind: 'picture', picture: { source, image } });
      return { node, message: { role: 'user' as const, content: `PICTURE of ${source}. Inspect it, then refine with an edit or finish with prose.`, images: wireImages([image]) } };
    } catch (error) {
      trace.push({ attempt: attempts, kind: 'picture', source, ok: false });
      emit({ kind: 'picture', picture: { source, error: error instanceof Error ? error.message : String(error) } });
      return { node: undefined, message: { role: 'user' as const, content: `Picture failed: ${String(error)}. Do not claim you saw it. Finish or continue from document data.` } };
    }
  };
  let topicAsks = 0;
  const unmerged = (edit: string): AskResult => {
    trace.push({ attempt: attempts, kind: 'stale' });
    return { ok: false, note, edit, outcome: null, attempts, transport: transport.name, error: UNMERGEABLE };
  };
  const stopped = () => {
    trace.push({ attempt: attempts, kind: 'stopped' });
    return { ok: false, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, error: STOPPED, ...(applied ? { note: 'Stopped after applying changes. ' + note } : {}) };
  };
  while (calls < MAX_CALLS) {
    calls += 1;
    attempts += 1;
    if (live.signal?.aborted) return stopped();
    emit({ kind: 'phase', phase: 'thinking', attempt: attempts });
    let text: string;
    // The reply as it streams, read by the engine for the chat.
    const streaming = live.onEvent ? streamViews(api, live.onEvent) : null;
    try {
      text = (await transport.complete(system, messages, { onEvent: streaming?.onEvent ?? live.onEvent, signal: live.signal, turn: options.turnId })).text;
      await streaming?.finish();
    } catch (e) {
      streaming?.stop();
      if (live.signal?.aborted) return stopped();
      // The message alone: a bridge error already reads "Error: …".
      const message = (e instanceof Error ? e.message : String(e)).replace(/^(Error: )+/, '');
      trace.push({ attempt: attempts, kind: 'error', message });
      return { ok: false, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, error: message };
    }
    // A reply that arrives after Stop is not applied.
    if (live.signal?.aborted) return stopped();
    // What the reply asks for, read by the engine (iron_document::reply):
    // as written, and its executable part (not examples in code).
    const read = await api.reply(text);
    const executable = await api.reply(executableReply(text));
    const picture = executable.picture;
    // A <revise> of the rejected edit is an edit too: a look beside it is taken
    // after it applies, not instead of it.
    const editing = !!read.edit || (!!rejected && !!read.revise);
    if (picture && !editing && pictures < MAX_PICTURES) {
      messages.push({ role: 'assistant', content: text });
      messages.push((await pictureMessage(picture)).message);
      attempts -= 1;
      continue;
    }
    // Out of pictures: say so rather than end the turn on a reply with no edit.
    // Free the first time (so a prose answer still counts as the first reply);
    // after that it spends an attempt, so it cannot loop.
    if (picture && !editing && !read.expand.length && !executable.note) {
      trace.push({ attempt: attempts, kind: 'picture', source: picture, ok: false });
      messages.push({ role: 'assistant', content: text });
      messages.push({ role: 'user', content: `No more pictures this request (at most ${MAX_PICTURES}). Finish with an <edit> or with prose from what you have seen.` });
      if (!toldNoPictures) { toldNoPictures = true; attempts -= 1; }
      continue;
    }
    // `<look expand="#u #e"/>` with no edit: show those instances' derived
    // insides (component-model.md §9) and ask again; not a repair attempt.
    const look = read.expand;
    if (look.length && !editing && looks < MAX_LOOKS) {
      looks += 1;
      trace.push({ attempt: attempts, kind: 'look', ids: look });
      attempts -= 1;
      messages.push({ role: 'assistant', content: text });
      emit({ kind: 'phase', phase: 'looking', attempt: attempts + 1 });
      messages.push({ role: 'user', content: await lookView(api, look) });
      continue;
    }
    // `<topic name="tables"/>` with no edit: send that part of the prompt
    // and ask again; not a repair attempt.
    const wanted = read.topics;
    if (wanted.length && !editing && topicAsks < MAX_TOPIC_ASKS) {
      topicAsks += 1;
      trace.push({ attempt: attempts, kind: 'topic', names: wanted });
      attempts -= 1;
      messages.push({ role: 'assistant', content: text });
      emit({ kind: 'phase', phase: 'looking', attempt: attempts + 1 });
      const texts = wanted.map((n) => topicText(schema, n) ?? `(no topic "${n}")`);
      messages.push({ role: 'user', content: `TOPIC\n${texts.join('\n\n')}\n\nNow answer the REQUEST: one <edit>, or prose if no change is needed.` });
      continue;
    }
    // In a chat, an edit in a code example is not applied.
    const acted = options.allowTextReply ? executable : read;
    let edit = acted.edit;
    // A <revise> patches the edit just rejected (iron_document::reply): the
    // full edit it makes is what is applied, shown and logged.
    const patch = !edit && rejected ? acted.revise : null;
    note = (edit && edit === read.edit) || (patch && !read.edit && patch === read.revise) ? read.note : read.bare;
    if (patch && rejected) {
      const revised = await api.revise(rejected, patch);
      if (!revised.ok) {
        const failure = `<revise-failed>${revised.error.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</revise-failed>`;
        trace.push({ attempt: attempts, kind: 'revise', ok: false, failure });
        lastOutcome = { ok: false, failure, errors: [`the revise could not be applied: ${revised.error}`] };
        messages.push({ role: 'assistant', content: text });
        messages.push({ role: 'user', content: `${failure}\n\nNothing was applied. ${REPAIR}` });
        continue;
      }
      edit = revised.edit;
    }
    if (!edit) {
      // After an applied step, a reply with no edit finishes the turn.
      if (applied && lastOutcome?.ok) {
        trace.push({ attempt: attempts, kind: 'prose' });
        return { ok: true, note, edit: lastApplied?.edit ?? null, outcome: lastApplied?.outcome ?? null, attempts, transport: transport.name, ...(mergedWith ? { merged: mergedWith } : {}) };
      }
      const prose = !!((options.allowTextReply || applied) && note && (attempts === 1 || (applied && lastOutcome?.ok)));
      trace.push(prose ? { attempt: attempts, kind: 'prose' } : { attempt: attempts, kind: 'no-edit' });
      if (prose) return { ok: true, note, edit: lastApplied?.edit ?? null, outcome: lastApplied?.outcome ?? null, attempts, transport: transport.name, ...(mergedWith ? { merged: mergedWith } : {}) };
      return { ok: false, note, edit: null, outcome: null, attempts, transport: transport.name, error: 'the reply had no <edit> block' };
    }
    lastEdit = edit;
    emit({ kind: 'phase', phase: 'applying', attempt: attempts });
    // The document may have changed while the agent worked: merge (merge.ts).
    let merged = 0;
    let unmergeable = false;
    let plannedAt: number | undefined;
    const mergingApi = {
      ...api,
      // A node the edit names that the person deleted meanwhile: say so,
      // not only that it is missing.
      plan: async (xml: string) => {
        const planned = await api.plan(xml);
        plannedAt = planned.version;
        if (planned.ok || !planned.errors?.some((e) => e.kind === 'unknown_node') || (await api.version()) === since) return planned;
        const changes = await changesSince(api, since);
        const errors = planned.errors.map((e) => {
          const id = e.kind === 'unknown_node' ? e.message.match(/#[\w/~]+/)?.[0] : undefined;
          return id && changes.deleted.has(id) ? { ...e, message: `the person deleted ${id} while you worked` } : e;
        });
        return { ...planned, errors };
      },
      // Applied at the version it was planned against (applyEdit plans
      // again when the document moved in between, so its ids, order keys and
      // compound verbs see the person's change).
      apply: async (transaction: object) => {
        const current = await api.version();
        if (plannedAt !== undefined && current !== plannedAt) return { ok: false, version: current, errors: [{ op: null, kind: 'stale', message: 'the document changed after this edit was planned' }] };
        if (current !== since) {
          const changes = await changesSince(api, since);
          if (changes.gap) { unmergeable = true; return { ok: false, version: current, errors: [{ op: null, kind: 'stale', message: UNMERGEABLE }] }; }
          const found = conflicts((transaction as { ops: object[] }).ops, changes);
          // Told now; a resend is judged against this version.
          if (found.length) { since = current; return { ok: false, version: current, errors: found }; }
        }
        const result = await api.apply(transaction, current);
        if (result.ok) merged = current - cycleVersion;
        return result;
      },
    };
    lastOutcome = await applyEdit(mergingApi, edit, `model: ${instruction.slice(0, 60)}`);
    if (unmergeable) return unmerged(edit);
    trace.push({ attempt: attempts, kind: 'edit', ok: lastOutcome.ok, ...(lastOutcome.failure ? { failure: lastOutcome.failure } : {}), ...(lastOutcome.summary ? { summary: lastOutcome.summary } : {}), ...(lastOutcome.version !== undefined ? { version: lastOutcome.version } : {}), ...(patch ? { revised: true } : {}), ...(merged > 0 ? { merged } : {}) });
    if (lastOutcome.ok) {
      if (merged > 0) mergedWith += merged;
      applied = true;
      lastApplied = { edit, outcome: lastOutcome };
      since = lastOutcome.version ?? await api.version();
      cycleVersion = since;
      rejected = null;
      rejections = 0;
      if (picture && pictures < MAX_PICTURES && calls < MAX_CALLS) {
        messages.push({ role: 'assistant', content: text });
        // The captured composition as it now is, not the whole document again:
        // each refinement would otherwise add another copy of the full tree.
        const shot = await pictureMessage(picture);
        const current = shot.node
          ? await api.view({ fidelity: 'full', scope: shot.node, omit_defaults: true })
          : (await api.version()) === since ? '(unchanged since your edit)' : await api.view({ fidelity: 'full', omit_defaults: true });
        messages.push({ ...shot.message, content: `Edit applied successfully. ${shot.node ? `Current ${shot.node}` : 'Current document'}:\n${current}\n\n${shot.message.content}` });
        continue;
      }
      // <continue/>: the reply is one step of several. Say what it made (the
      // ids its aliases became) and ask for the next.
      if (executable.continue && calls < MAX_CALLS) {
        messages.push({ role: 'assistant', content: text });
        messages.push({ role: 'user', content: `${appliedMarkup(lastOutcome)}\n\nContinue: the next <edit> (with <continue/> after it if more follows), or finish with prose.` });
        continue;
      }
      break;
    }
    rejected = edit;
    rejections += 1;
    if (rejections > MAX_REPAIRS) break;
    messages.push({ role: 'assistant', content: text });
    // A revised edit is shown whole, so a second revise's op numbers and the
    // errors' match what the model sees.
    const shown = patch ? `Your revise made this edit, which was rejected:\n${edit}\n\n` : '';
    messages.push({ role: 'user', content: `${shown}${lastOutcome.failure}\n\n${REPAIR}` });
  }
  const ok = !!lastOutcome?.ok;
  // Out of calls, or stuck on one step: the steps before it stay applied.
  const error = ok ? undefined : rejections > MAX_REPAIRS
    ? `the store rejected ${rejections} edits in a row${applied ? '; the steps before them are applied' : ''}`
    : calls >= MAX_CALLS ? `used all ${MAX_CALLS} model calls${applied ? '; the steps so far are applied' : ''}` : undefined;
  // Applied, but the reply asked to continue: it did not finish.
  if (ok && calls >= MAX_CALLS) note = `Stopped at the limit of ${MAX_CALLS} model calls; more steps were planned. ${note}`.trim();
  return { ok, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, ...(error ? { error } : {}), ...(mergedWith ? { merged: mergedWith } : {}) };
}

/** An applied edit for the model: its summary, which names the ids its aliases became. */
function appliedMarkup(outcome: EditOutcome): string {
  return outcome.summary ?? `<applied version="${outcome.version}">\n</applied>`;
}

/** Events with each reply's stream view after the text it read: the
 *  engine reads the reply as it streams (iron_document::reply::stream), one
 *  read at a time, the latest text next. */
function streamViews(api: DocumentApi, emit: (event: AgentEvent) => void) {
  let text = '';
  let dirty = false;
  let stopped = false;
  let running: Promise<void> | null = null;
  const run = async () => {
    while (dirty && !stopped) {
      dirty = false;
      const view = await Promise.resolve(api.stream(text)).catch(() => null);
      if (view && !stopped) emit({ kind: 'stream', view });
    }
    running = null;
  };
  return {
    onEvent(event: AgentEvent) {
      emit(event);
      if (event.kind !== 'text') return;
      text += event.delta;
      dirty = true;
      running ??= run();
    },
    /** The last view sent before the turn moves on. */
    async finish() { await running; stopped = true; },
    stop() { stopped = true; },
  };
}

const UNMERGEABLE = 'the document changed while the agent worked, and too much of that is gone from history to merge its edit; ask again with the current document';

/** Model calls one request may make, of every kind: steps, repairs, looks,
 *  topics and refinements after a picture. Each re-sends the conversation. */
const MAX_CALLS = 20;
/** Rejected edits in a row before the turn ends: an edit and its repairs
 *  that keep failing are stuck, and more tries only spend calls. */
const MAX_REPAIRS = 3;

/** What a rejected edit's reply asks for. */
const REPAIR = 'Fix what it names with a <revise> of that edit (only the ops that change), or send one new <edit> if most of it must change.';

/** Looks one request may take. */
const MAX_LOOKS = 4;
/** `<look picture>` captures a turn may take, refining an edit or not. Each
 *  is an image every later call in the turn carries. */
const MAX_PICTURES = 6;

/** Topic requests one request may make before it must answer. */
const MAX_TOPIC_ASKS = 2;

/** The view a look asks for: each named instance's subtree with what it
 *  derives shown, marked `derived`. */
async function lookView(api: DocumentApi, ids: string[]): Promise<string> {
  const parts = await Promise.all(ids.map(async (id) => {
    try {
      return await api.view({ fidelity: 'full', scope: id, expand: [id], omit_defaults: true });
    } catch (e) {
      return `<look-failed id="${id}">${String(e).replace(/[<>&]/g, '')}</look-failed>`;
    }
  }));
  return `LOOK\n${parts.join('\n')}\n\nNow answer the REQUEST: one <edit>, or prose if no change is needed.`;
}
