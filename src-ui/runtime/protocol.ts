import type { Backend } from './backend_policy';
import type { EngineRequest, EngineEvent } from './protocol.generated';
import type { EditOutcome } from '../model/edit';
import type { AskResult } from '../model/agent';
import type { ModelOptions } from '../model/runtime-options';
export type UiRequest = (Exclude<EngineRequest, { ty: 'resize' }> |
  { ty: 'resize'; width: number; height: number; dpr: number } |
  // The artifact's bytes (transferred), or the module compiled ahead of time
  // on the scopes page (`prefetchWasm`; structured-cloned, not transferred).
  { ty: 'wasmData'; backend: Backend; wasmData: ArrayBuffer | WebAssembly.Module; variantFlags: number } |
  { ty: 'init'; canvas: HTMLCanvasElement | OffscreenCanvas; devicePixelRatio: number } |
  { ty: 'modelEdit'; requestId: number; xml: string; label?: string } |
  ({ ty: 'modelAsk'; requestId: number; instruction: string } & ModelOptions)
) & { sessionId?: number };
export type ReplyType = 'documentSaved' | 'documentLoaded' | 'documentView' | 'documentCall' | 'solveBench' | 'modelEditResult' | 'modelAskResult';
type ReplyEnvelope = {
  requestId: number; sessionId?: number; version?: number; sessionVersion?: number;
  /** Queued is accepted for a later tick; applied does not promise presentation. */
  ack?: 'queued' | 'applied' | 'completed';
};
export type CommandReply = ReplyEnvelope & (
  { ty: 'documentSaved' | 'documentLoaded' | 'documentView' | 'documentCall' | 'solveBench'; ok?: true; text: string } |
  { ty: 'modelEditResult'; ok: boolean; outcome: EditOutcome } |
  { ty: 'modelAskResult'; ok: boolean; result: AskResult } |
  { ty: ReplyType; ok: false; error: string }
);

export type UiEvent = (Exclude<EngineEvent, { ty: 'convertQuestion' }> | CommandReply |
  { ty: 'convertQuestion'; question: { node: string; type: string; cuts: string[] } } |
  { ty: 'workerIsReady' } | { ty: 'enginePrepared'; pointerSource?: 'native' | 'webview' } |
  { ty: 'fontsLoaded'; families: string[] } | { ty: 'pick'; list: unknown[] } |
  { ty: 'runtimeError'; error: string } | { ty: 'probeStats'; stats: unknown }
) & { sessionId?: number };
export type ClientRequest = Extract<UiRequest, { requestId: number }>;
export type RequestPayload<T extends ClientRequest['ty']> = Omit<Extract<ClientRequest, { ty: T }>, 'ty' | 'requestId' | 'sessionId'>;
