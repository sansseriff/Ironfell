/**
 * Entry points the adapters call. Runs wherever the wasm runs, so a call
 * into the store is a plain function call, never a message.
 */
import type { Glue } from '../runtime/glue';
import { ask, type AskResult } from './agent';
import { makeDocumentApi } from './api';
import { applyEdit, type EditOutcome } from './edit';
import { AnthropicTransport } from './transport';

export async function modelEdit(glue: Glue, handle: bigint, xml: string, label?: string): Promise<EditOutcome> {
  return applyEdit(makeDocumentApi(glue, handle), xml, label);
}

export async function modelAsk(
  glue: Glue,
  handle: bigint,
  instruction: string,
  opts: { apiKey: string; model?: string },
): Promise<AskResult> {
  const transport = new AnthropicTransport(opts.apiKey, opts.model);
  return ask(makeDocumentApi(glue, handle), transport, instruction);
}
