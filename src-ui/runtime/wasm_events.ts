import type { EngineEvent } from './protocol.generated';
import type { UiEvent } from './protocol';
import type { FontRequest } from './font_loader';
import type { QueryRequest } from './model_loader';
/** One outbound path for both browser hosts. Native uses the same serialized
 * Rust events on its channel; font byte acquisition remains host-specific. */
export function installWasmEvents(scope: object, emit: (event: UiEvent) => void, fonts: (requests: FontRequest[]) => void, queries: (requests: QueryRequest[]) => void) {
  Object.assign(scope, { rustBridge: {
    send_event_from_worker(json: string) {
      const event: EngineEvent = JSON.parse(json);
      if (event.ty === 'fonts') fonts(event.fonts);
      else if (event.ty === 'queries') queries(event.requests);
      else emit(event as UiEvent);
    },
  } });
}
