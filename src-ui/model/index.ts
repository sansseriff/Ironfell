/**
 * Entry points the adapters call. Runs wherever the wasm runs, so a call
 * into the store is a plain function call, never a message.
 */
import type { Glue } from '../runtime/glue';
import { ask, type AskResult } from './agent';
import { makeDocumentApi } from './api';
import { documentArguments } from '../runtime/protocol.generated';
import { applyEdit, type EditOutcome } from './edit';
import { makeModelTransport } from './transport';
import type { ModelOptions } from './runtime-options';
import type { AskLive } from './progress';

export async function modelEdit(glue: Glue, handle: bigint, xml: string, label?: string, alive = () => true): Promise<EditOutcome> {
  return applyEdit(makeDocumentApi(command => { if (!alive()) throw new Error('session disposed'); const { fn, args } = documentArguments(command); return glue.document_call(handle, fn, JSON.stringify(args)); }), xml, label);
}

export async function modelAsk(
  glue: Glue,
  handle: bigint,
  instruction: string,
  opts: ModelOptions,
  alive = () => true,
  live: AskLive = {},
): Promise<AskResult> {
  const transport = makeModelTransport(opts);
  return ask(makeDocumentApi(command => { if (!alive()) throw new Error('session disposed'); const { fn, args } = documentArguments(command); return glue.document_call(handle, fn, JSON.stringify(args)); }), transport, instruction, opts, live);
}
