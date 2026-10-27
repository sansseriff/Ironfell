import type { UiRequest, UiEvent } from './protocol';
/** Common browser protocol adapter. Rust owns input/control dispatch; only the
 * authoring parser and remote-model operation run in the JS host. */
import type { Glue } from './glue';
import { modelAsk, modelEdit } from '../model';

export async function dispatchEngineMessage(
  glue: Glue, handle: bigint, data: UiRequest, emit: (reply: UiEvent) => void, alive = () => true,
): Promise<void> {
  try {
    if (handle === 0n) throw new Error('engine is not attached');
    if (data.ty === 'modelEdit') {
      const outcome = await modelEdit(glue, handle, data.xml, data.label, alive);
      emit({ ty: 'modelEditResult', requestId: data.requestId, ok: outcome.ok, outcome, ack: outcome.ok ? 'applied' : 'completed' });
    } else if (data.ty === 'modelAsk') {
      const result = await modelAsk(glue, handle, data.instruction, { apiKey: data.apiKey, model: data.model }, alive);
      emit({ ty: 'modelAskResult', requestId: data.requestId, ok: result.ok, result, ack: 'completed' });
    } else {
      const reply = JSON.parse(glue.engine_message(handle, JSON.stringify(data)));
      if (reply) emit(reply);
    }
  } catch (e) {
    const ty = ({documentSave:'documentSaved', documentLoad:'documentLoaded', modelEdit:'modelEditResult', modelAsk:'modelAskResult'} as Record<string,string>)[data.ty] ?? data.ty;
    if ('requestId' in data) emit({ ty, requestId: data.requestId, ok: false, error: String(e) } as UiEvent);
    else console.error('Engine message failed', data.ty, e);
  }
}
