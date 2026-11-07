import type { UiRequest, UiEvent } from './protocol';
/** Common browser protocol adapter. Rust owns input/control dispatch; only the
 * authoring parser and remote-model operation run in the JS host. */
import type { Glue } from './glue';
import { modelAsk, modelEdit } from '../model';
import { eventBatcher } from '../model/progress';

/** Running model turns, by request id, so a cancel can stop one. */
const asks = new Map<number, AbortController>();

export async function dispatchEngineMessage(
  glue: Glue | null, handle: bigint, data: UiRequest, emit: (reply: UiEvent) => void, alive = () => true,
): Promise<void> {
  if (data.ty === 'modelAskCancel') { asks.get(data.askId)?.abort(); return; }
  try {
    if (!glue || handle === 0n) throw new Error('engine is not attached');
    if (data.ty === 'modelEdit') {
      const outcome = await modelEdit(glue, handle, data.xml, data.label, alive);
      emit({ ty: 'modelEditResult', requestId: data.requestId, ok: outcome.ok, outcome, ack: outcome.ok ? 'applied' : 'completed' });
    } else if (data.ty === 'modelAsk') {
      const controller = new AbortController();
      asks.set(data.requestId, controller);
      const progress = eventBatcher(events => emit({ ty: 'modelAskProgress', requestId: data.requestId, sessionId: data.sessionId, events }));
      try {
        const result = await modelAsk(glue, handle, data.instruction, data, alive, { onEvent: progress.push, signal: controller.signal });
        progress.flush();
        emit({ ty: 'modelAskResult', requestId: data.requestId, ok: result.ok, result, ack: 'completed' });
      } finally {
        progress.flush();
        asks.delete(data.requestId);
      }
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
