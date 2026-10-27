import type { EngineEvent } from './protocol.generated';
import type { UiEvent } from './protocol';
import type { FontRequest } from './font_loader';
/** One outbound path for both browser hosts. Native uses the same serialized
 * Rust events on its channel; font byte acquisition remains host-specific. */
export function installWasmEvents(scope: object, emit: (event: UiEvent) => void, fonts: (requests: FontRequest[]) => void) {
  Object.assign(scope, { rustBridge: {
    send_event_from_worker(json: string) {
      const event: EngineEvent = JSON.parse(json);
      if (event.ty === 'fonts') fonts(event.fonts);
      else emit(event as UiEvent);
    },
  } });
}
