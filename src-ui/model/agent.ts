/**
 * One request, one edit: read a view, ask the model, apply what it wrote,
 * and give it one chance to fix a rejected edit (doc 04 §9.5).
 */
import type { DocumentApi } from './api';
import { applyEdit, extractEdit, type EditOutcome } from './edit';
import { manifest } from './manifest';
import type { ModelTransport } from './transport';
import { boundedHistory, executableReply, type ConversationOptions } from './conversation';

export interface AskResult {
  ok: boolean;
  /** What the model said outside the edit block, if anything. */
  note: string;
  edit: string | null;
  outcome: EditOutcome | null;
  attempts: number;
  transport: string;
  error?: string;
}

export async function ask(api: DocumentApi, transport: ModelTransport, instruction: string, options: ConversationOptions = {}): Promise<AskResult> {
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
  let lastOutcome: EditOutcome | null = null;
  let note = '';
  let looks = 0;
  while (attempts < 2) {
    attempts += 1;
    let text: string;
    try {
      text = (await transport.complete(system, messages)).text;
    } catch (e) {
      return { ok: false, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, error: String(e) };
    }
    // `<look expand="#u #e"/>` with no edit: show those instances' derived
    // insides (component-model.md §9) and ask again; not a repair attempt.
    const look = lookRequest(text);
    if (look && !extractEdit(text) && looks < MAX_LOOKS) {
      looks += 1;
      attempts -= 1;
      messages.push({ role: 'assistant', content: text });
      messages.push({ role: 'user', content: await lookView(api, look) });
      continue;
    }
    const edit = extractEdit(options.allowTextReply ? executableReply(text) : text);
    note = text.replace(edit ?? '', '').trim();
    if (!edit) {
      if (options.allowTextReply && note && attempts === 1) return { ok: true, note, edit: null, outcome: null, attempts, transport: transport.name };
      return { ok: false, note, edit: null, outcome: null, attempts, transport: transport.name, error: 'the reply had no <edit> block' };
    }
    lastEdit = edit;
    if (await api.version() !== snapshotVersion) {
      return { ok: false, note, edit, outcome: null, attempts, transport: transport.name, error: 'document changed while the agent was thinking; ask again with the current document' };
    }
    const guardedApi = { ...api, apply: (transaction: object) => api.apply(transaction, snapshotVersion) };
    lastOutcome = await applyEdit(guardedApi, edit, `model: ${instruction.slice(0, 60)}`);
    if (lastOutcome.ok) break;
    messages.push({ role: 'assistant', content: text });
    messages.push({ role: 'user', content: `${lastOutcome.failure}\n\nSend one corrected <edit>.` });
  }
  return { ok: !!lastOutcome?.ok, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name };
}

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
