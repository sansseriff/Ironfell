/**
 * One request, one edit: read a view, ask the model, apply what it wrote,
 * and give it one chance to fix a rejected edit (doc 04 §9.5).
 */
import type { DocumentApi } from './api';
import { applyEdit, extractEdit, type EditOutcome } from './edit';
import { manifest } from './manifest';
import type { ModelTransport } from './transport';

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

export async function ask(api: DocumentApi, transport: ModelTransport, instruction: string): Promise<AskResult> {
  const system = manifest(api.schema());
  // The model view first (what things are and where each is drawn, ECM §11),
  // then the tree, which also holds what no composition does (the library,
  // timelines, loose roots).
  const view = `${api.view({ fidelity: 'full', relation: 'model' })}\n${api.view({ fidelity: 'full' })}`;
  const messages: { role: 'user' | 'assistant'; content: string }[] = [
    { role: 'user', content: `${view}\n\nREQUEST\n${instruction}` },
  ];
  let attempts = 0;
  let lastEdit: string | null = null;
  let lastOutcome: EditOutcome | null = null;
  let note = '';
  while (attempts < 2) {
    attempts += 1;
    let text: string;
    try {
      text = (await transport.complete(system, messages)).text;
    } catch (e) {
      return { ok: false, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name, error: String(e) };
    }
    const edit = extractEdit(text);
    note = text.replace(edit ?? '', '').trim();
    if (!edit) {
      return { ok: false, note, edit: null, outcome: null, attempts, transport: transport.name, error: 'the reply had no <edit> block' };
    }
    lastEdit = edit;
    lastOutcome = applyEdit(api, edit, `model: ${instruction.slice(0, 60)}`);
    if (lastOutcome.ok) break;
    messages.push({ role: 'assistant', content: text });
    messages.push({ role: 'user', content: `${lastOutcome.failure}\n\nSend one corrected <edit>.` });
  }
  return { ok: !!lastOutcome?.ok, note, edit: lastEdit, outcome: lastOutcome, attempts, transport: transport.name };
}
