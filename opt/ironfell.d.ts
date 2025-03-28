/* tslint:disable */
/* eslint-disable */
export function init_bevy_app(): bigint;
/**
 * 创建离屏窗口
 */
export function create_window_by_offscreen_canvas(ptr: bigint, canvas: OffscreenCanvas, scale_factor: number): void;
/**
 * Check if plugin initialization is completed
 * Frame rendering cannot be called before initialization is complete
 */
export function is_preparation_completed(ptr: bigint): number;
/**
 * 包装一个鼠标事件发送给 app
 */
export function mouse_move(ptr: bigint, x: number, y: number): void;
/**
 * 鼠标左键按下
 */
export function left_bt_down(ptr: bigint, obj: any, x: number, y: number): void;
/**
 * 鼠标左键松开
 */
export function left_bt_up(ptr: bigint): void;
/**
 * 设置 hover（高亮） 效果
 */
export function set_hover(ptr: bigint, arr: Array<any>): void;
/**
 * 设置 选中 效果
 */
export function set_selection(ptr: bigint, arr: Array<any>): void;
/**
 * 打开 / 关闭动画
 */
export function set_auto_animation(ptr: bigint, needs_animate: number): void;
/**
 * 帧绘制
 *
 * render 运行在 worker 中时，主线程 post 绘制 msg 时可能 render 还没有完成当前帧的更新
 *
 * TODO：需要检测帧依赖的资源是否已加载完成，否则可能提交的 update 累积会导致栈溢出
 */
export function enter_frame(ptr: bigint): void;
export function release_app(ptr: bigint): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly init_bevy_app: () => bigint;
  readonly create_window_by_offscreen_canvas: (a: bigint, b: any, c: number) => void;
  readonly is_preparation_completed: (a: bigint) => number;
  readonly mouse_move: (a: bigint, b: number, c: number) => void;
  readonly left_bt_down: (a: bigint, b: any, c: number, d: number) => void;
  readonly left_bt_up: (a: bigint) => void;
  readonly set_hover: (a: bigint, b: any) => void;
  readonly set_selection: (a: bigint, b: any) => void;
  readonly set_auto_animation: (a: bigint, b: number) => void;
  readonly enter_frame: (a: bigint) => void;
  readonly release_app: (a: bigint) => void;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_2: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_6: WebAssembly.Table;
  readonly closure5484_externref_shim: (a: number, b: number, c: any) => void;
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
