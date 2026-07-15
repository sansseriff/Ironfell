/* tslint:disable */
/* eslint-disable */
export function start_winit_main(canvas: HTMLCanvasElement, options: any, on_frame: Function): WinitMainHandle;
export function start_bevy_main(canvas_id: string, options: any, on_frame: Function): BevyMainHandle;
export function start_wgpu_worker(canvas: OffscreenCanvas, options: any, on_frame: Function): Promise<WgpuWorkerHandle>;
export function start_bevy_worker(_canvas: OffscreenCanvas, _options: any, _on_frame: Function): Promise<any>;
export function start_bevy_vello_worker(_canvas: OffscreenCanvas, _options: any, _on_frame: Function): Promise<any>;
export class BevyMainHandle {
  private constructor();
  free(): void;
  start(pointer: any, rendered: any): void;
}
export class WgpuWorkerHandle {
  private constructor();
  free(): void;
  start(pointer: any, rendered: any): void;
  pointer_down(point: any): void;
  pointer_move(point: any): void;
  pointer_up(point: any): void;
  stop(): void;
  dispose(): void;
}
export class WinitMainHandle {
  private constructor();
  free(): void;
  start(pointer: any, rendered: any): void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly start_winit_main: (a: any, b: any, c: any) => [number, number, number];
  readonly __wbg_winitmainhandle_free: (a: number, b: number) => void;
  readonly winitmainhandle_start: (a: number, b: any, c: any) => [number, number];
  readonly start_bevy_main: (a: number, b: number, c: any, d: any) => [number, number, number];
  readonly __wbg_bevymainhandle_free: (a: number, b: number) => void;
  readonly bevymainhandle_start: (a: number, b: any, c: any) => [number, number];
  readonly start_wgpu_worker: (a: any, b: any, c: any) => any;
  readonly __wbg_wgpuworkerhandle_free: (a: number, b: number) => void;
  readonly wgpuworkerhandle_start: (a: number, b: any, c: any) => [number, number];
  readonly wgpuworkerhandle_pointer_down: (a: number, b: any) => [number, number];
  readonly wgpuworkerhandle_pointer_move: (a: number, b: any) => [number, number];
  readonly wgpuworkerhandle_pointer_up: (a: number, b: any) => [number, number];
  readonly wgpuworkerhandle_stop: (a: number) => void;
  readonly wgpuworkerhandle_dispose: (a: number) => void;
  readonly start_bevy_worker: (a: any, b: any, c: any) => any;
  readonly start_bevy_vello_worker: (a: any, b: any, c: any) => any;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_1: WebAssembly.Table;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_export_6: WebAssembly.Table;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__he8af2f093d91a0ee: (a: number, b: number, c: number) => void;
  readonly closure67410_externref_shim: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h38142521103eab74: (a: number, b: number) => void;
  readonly closure67411_externref_shim: (a: number, b: number, c: any, d: any) => void;
  readonly closure67595_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure67621_externref_shim: (a: number, b: number, c: any, d: any) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
