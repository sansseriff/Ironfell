use crate::bevy_app::init_app;
use crate::panels::{PanelRect, Panels};
use crate::{ActivityControl, WorkerApp, canvas_view::*};
use bevy::app::PluginsState;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use wasm_bindgen::prelude::*;

// Import Bevy's input types that your FFI functions will create events for
use bevy::input::{
    ButtonState,                                            // Added ButtonState
    keyboard::{Key, KeyCode as BevyKeyCode, KeyboardInput}, // Added Key, BevyKeyCode, KeyboardInput, NativeKey
    mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel},
};
use bevy::window::CursorMoved; // CursorMoved is used in mouse_move

// pub struct MyMouseWheelEvent {
//     pub delta_x: f32,
//     pub delta_y: f32,
//     pub unit: MouseScrollUnit,
// }
// impl Event for MyMouseWheelEvent {}

#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);
    /// 在 app 初始化完成前，info! 无法使用，打印日志得用它
    #[wasm_bindgen(js_namespace = console)]
    pub(crate) fn log(s: &str);

    /// 发送 pick 列表
    ///
    /// 从 worker 环境发送
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_pick_from_worker(list: js_sys::Array);
    // New outbound helpers (implemented in JS worker) for hover & selection changes
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_hover_from_worker(list: js_sys::Array);
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_selection_from_worker(list: js_sys::Array);
    /// The document's version advanced (a transaction, undo, redo, or load
    /// applied). The shell re-reads whatever views it shows.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_document_changed_from_worker(version: u32);
    /// The canvas cursor the select tool wants: a CSS cursor name, or
    /// `rotate:<degrees>` for the shell's rotation cursor.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_cursor_from_worker(cursor: String);
    /// The active tool changed (`select`, `pen`): the shell's toolbar.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_tool_from_worker(tool: String);
    /// Editing a shape's points would cut bindings or clips
    /// (`iron_document::shape`): JSON `{node, type, cuts}` for the shell to
    /// ask about; a yes calls `document_to_path` with `force`.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_convert_from_worker(question: String);
    /// The answer to a Cmd+C or Cmd+X: the copied fragment's text
    /// (`iron_document::copy`), or empty when nothing was copied. The shell
    /// is waiting on it to finish its clipboard write.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_clipboard_from_worker(text: String);
    /// Font files to fetch, as JSON `[{family, file}]`, most needed first.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_fonts_from_worker(fonts: String);
    /// The text editor's state for the host's textarea, as JSON.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_text_edit_from_worker(state: String);
    /// Any other event, as its JSON (`ty` names it): the host posts it on.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_event_from_worker(event: String);
}

fn event_json(event: &crate::host_events::EngineEvent) -> serde_json::Value {
    serde_json::to_value(event).unwrap_or_default()
}

#[wasm_bindgen]
pub fn init_bevy_app_with_canvas(
    canvas: web_sys::OffscreenCanvas,
    scale_factor: f32,
    is_in_worker: bool,
    variant_flags: u32,
) -> u64 {
    // Without this, a panic reaches the console as a bare `RuntimeError: unreachable`
    // with no message, file or line: `wasm-release` builds with
    // `panic = "immediate-abort"` plus `-Zfmt-debug=none -Zlocation-detail=none`.
    // The hook costs nothing there (those panics are already unrecoverable aborts)
    // and makes any non-stripped build report the real panic instead.
    console_error_panic_hook::set_once();
    // info!/log crate may be unavailable in nolog/min variants; always print via console.
    log(&format!(
        "init_bevy_app_with_canvas variant_flags={variant_flags}"
    ));

    let offscreen_canvas = OffscreenCanvas::new(canvas, scale_factor, 1);
    let view_obj = ViewObj::from_offscreen_canvas(offscreen_canvas);

    let mut app = init_app(variant_flags, |app| spawn_canvas_window(app, view_obj));
    app.scale_factor = scale_factor;
    app.insert_resource(crate::Dpr(scale_factor as f64));

    let mut act = ActivityControl::new();
    act.is_in_worker = is_in_worker;
    app.insert_resource(act);

    // 包装成无生命周期的指针
    // english: Wrap it into a non-lifetime pointer
    Box::into_raw(Box::new(app)) as u64
}

#[wasm_bindgen]
pub fn is_preparation_completed(ptr: u64) -> u32 {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    // Creating device/queue is asynchronous, completion timing is uncertain
    // once the plugins are ready and loaded, then
    if app.plugins_state() == PluginsState::Ready {
        app.finish();
        app.cleanup();

        let mut windows_system_state: SystemState<Query<Entity, With<PrimaryWindow>>> =
            SystemState::from_world(app.world_mut());
        // bevy 0.19: SystemState::get returns Result, and so does Query::single —
        // with different error types, hence the two-step unwrap.
        if let Some(entity) = windows_system_state
            .get(app.world())
            .ok()
            .and_then(|windows| windows.single().ok())
        {
            app.window = entity;
            return 1;
        }
    }
    0
}

#[wasm_bindgen]
pub fn enter_frame_with_mouse(ptr: u64, mouse_x: f32, mouse_y: f32, has_mouse_update: bool) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    
    // Update mouse position first if provided
    if has_mouse_update {
        let position = app.to_physical_size(mouse_x, mouse_y);
        let cursor_move = CursorMoved {
            window: app.window,
            position,
            delta: None,
        };
        app.world_mut().write_message(cursor_move);
    }
    
    // Get a mutable borrow of the Rust object pointed to by the pointer
    {
        // Check conditions for executing frame rendering
        let mut active_info = app
            .world_mut()
            .get_resource_mut::<ActivityControl>()
            .unwrap();
        if !active_info.auto_animate && active_info.remaining_frames == 0 {
            return;
        }
        if active_info.remaining_frames > 0 {
            active_info.remaining_frames -= 1;
        }
    }

    if app.plugins_state() != PluginsState::Cleaned {
        if app.plugins_state() != PluginsState::Ready {
            // #[cfg(not(target_arch = "wasm32"))]
            // tick_global_task_pools_on_main_thread();
        } else {
            app.finish();
            app.cleanup();
        }
    } else {
        app.frame();
        flush_events(app);
    }
}

#[wasm_bindgen]
pub fn resize(ptr: u64, _width: f32, _height: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    // JS has already resized the canvas backing store; sync the Bevy window to it.
    update_canvas_window(app);

    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

#[wasm_bindgen]
pub fn enter_frame(ptr: u64) {
    // 获取到指针指代的 Rust 对象的可变借用
    // english: Get a mutable borrow of the Rust object pointed to by the pointer
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    {
        // Check conditions for executing frame rendering
        let mut active_info = app
            .world_mut()
            .get_resource_mut::<ActivityControl>()
            .unwrap();
        if !active_info.auto_animate && active_info.remaining_frames == 0 {
            return;
        }
        if active_info.remaining_frames > 0 {
            active_info.remaining_frames -= 1;
        }
    }

    if app.plugins_state() != PluginsState::Cleaned {
        if app.plugins_state() != PluginsState::Ready {
            // #[cfg(not(target_arch = "wasm32"))]
            // tick_global_task_pools_on_main_thread();
        } else {
            app.finish();
            app.cleanup();
        }
    } else {
        app.frame();
        flush_events(app);
    }
}

#[wasm_bindgen]
pub fn release_app(ptr: u64) {
    // 将指针转换为其指代的实际 Rust 对象，同时也拿回此对象的内存管理权
    let app: Box<WorkerApp> = unsafe { Box::from_raw(ptr as *mut WorkerApp) };
    crate::close_bevy_window(app);
}

#[wasm_bindgen]
pub fn set_panel_viewport(ptr: u64, id: String, kind: String, composition: String, x: f32, y: f32, w: f32, h: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_panel_viewport(app, id, kind, composition, x, y, w, h)
}

#[wasm_bindgen]
pub fn despawn_panel(ptr: u64, id: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::despawn_panel(app, id)
}

#[wasm_bindgen]
pub fn set_mouse_position(ptr: u64, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_mouse_position(app, x, y)
}

#[wasm_bindgen]
pub fn mouse_move(ptr: u64, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::mouse_move(app, x, y)
}

#[wasm_bindgen]
pub fn mouse_wheel(ptr: u64, delta_x: f32, delta_y: f32, delta_mode: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::mouse_wheel(app, delta_x, delta_y, delta_mode)
}

#[wasm_bindgen]
pub fn set_dark_mode(ptr: u64, dark: bool) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_dark_mode(app, dark)
}

#[wasm_bindgen]
pub fn set_scale_factor(ptr: u64, scale_factor: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_scale_factor(app, scale_factor)
}

#[wasm_bindgen]
pub fn left_bt_down(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::left_bt_down(app)
}

#[wasm_bindgen]
pub fn left_bt_down_at(ptr: u64, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::left_bt_down_at(app, x, y)
}

#[wasm_bindgen]
pub fn left_bt_up(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::left_bt_up(app)
}

#[wasm_bindgen]
pub fn right_bt_down(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::right_bt_down(app)
}

#[wasm_bindgen]
pub fn right_bt_up(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::right_bt_up(app)
}

#[wasm_bindgen]
pub fn middle_bt(ptr: u64, down: bool) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::middle_bt(app, down)
}

#[wasm_bindgen]
pub fn set_auto_animation(ptr: u64, needs_animate: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_auto_animation(app, needs_animate)
}

#[wasm_bindgen]
pub fn document_save(ptr: u64) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_save(app)
}

#[wasm_bindgen]
pub fn document_load(ptr: u64, text: String) -> Result<(), JsValue> {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_load(app, text).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn document_view(ptr: u64, query_json: String) -> Result<String, JsValue> {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_view(app, query_json).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn document_apply(ptr: u64, transaction_json: String, expect_version: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_apply(app, transaction_json, expect_version)
}

#[wasm_bindgen]
pub fn document_build_info(_ptr: u64) -> String {
    let app = unsafe { &mut *(_ptr as *mut WorkerApp) };
    crate::engine_api::document_build_info(app)
}

#[wasm_bindgen]
pub fn document_stress(ptr: u64, shapes: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_stress(app, shapes)
}

#[wasm_bindgen]
pub fn document_meters(ptr: u64, n: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_meters(app, n)
}

#[wasm_bindgen]
pub fn document_snapping(ptr: u64, enabled: bool) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_snapping(app, enabled)
}

#[wasm_bindgen]
pub fn document_spatial(ptr: u64, enabled: bool) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_spatial(app, enabled)
}

#[wasm_bindgen]
pub fn document_signals(ptr: u64) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_signals(app)
}

#[wasm_bindgen]
pub fn document_producer(ptr: u64, name: String, action: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_producer(app, name, action)
}

#[wasm_bindgen]
pub fn document_raster(ptr: u64, enabled: bool, budget_mb: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_raster(app, enabled, budget_mb)
}

#[wasm_bindgen]
pub fn document_timings(_ptr: u64, reset: bool) -> String {
    let app = unsafe { &mut *(_ptr as *mut WorkerApp) };
    crate::engine_api::document_timings(app, reset)
}

#[wasm_bindgen]
pub fn document_link(ptr: u64, a: String, b: String, keep: String, actor: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_link(app, a, b, keep, actor)
}

#[wasm_bindgen]
pub fn document_make_component(ptr: u64, ids: String, name: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_make_component(app, ids, name)
}

#[wasm_bindgen]
pub fn document_author(ptr: u64, verb: String, args: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_author(app, verb, args)
}

#[wasm_bindgen]
pub fn document_table(ptr: u64, spec: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_table(app, spec)
}

#[wasm_bindgen]
pub fn document_table_edit(ptr: u64, table: String, verb: String, cells: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_table_edit(app, table, verb, cells)
}

#[wasm_bindgen]
pub fn document_detach(ptr: u64, entity: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_detach(app, entity)
}

#[wasm_bindgen]
pub fn document_counterparts(ptr: u64, node: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_counterparts(app, node)
}

#[wasm_bindgen]
pub fn document_to_path(ptr: u64, node: String, force: bool) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_to_path(app, node, force)
}

#[wasm_bindgen]
pub fn document_export_svg(ptr: u64, scope: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_export_svg(app, scope)
}

#[wasm_bindgen]
pub fn document_import_svg(ptr: u64, text: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_import_svg(app, text)
}

#[wasm_bindgen]
pub fn document_import_svg_flat(ptr: u64, text: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_import_svg_flat(app, text)
}

#[wasm_bindgen]
pub fn document_import_svg_at(ptr: u64, text: String, parent: String, x: f64, y: f64, actor: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_import_svg_at(app, text, parent, x, y, actor)
}

#[wasm_bindgen]
pub fn document_focus(ptr: u64, node: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_focus(app, node)
}

#[wasm_bindgen]
pub fn document_selection(ptr: u64) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_selection(app)
}

#[wasm_bindgen]
pub fn document_inspect(ptr: u64, node: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_inspect(app, node)
}

#[wasm_bindgen]
pub fn document_component(ptr: u64, node: String, kind: String, add: bool) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_component(app, node, kind, add)
}

#[wasm_bindgen]
pub fn document_inspect_patch(ptr: u64, node: String, patch: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_inspect_patch(app, node, patch)
}

#[wasm_bindgen]
pub fn document_inspect_scrub(ptr: u64, node: String, leaf: String, phase: String, token: u32, value: f64, options: Option<String>) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_inspect_scrub(app, node, leaf, phase, token, value, options)
}

#[wasm_bindgen]
pub fn document_inspect_set(ptr: u64, node: String, leaf: String, text: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_inspect_set(app, node, leaf, text)
}

#[wasm_bindgen]
pub fn document_compositions(ptr: u64) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_compositions(app)
}

#[wasm_bindgen]
pub fn document_composition_info(ptr: u64, source: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_composition_info(app, source)
}

#[wasm_bindgen]
pub fn document_render_inspector(ptr: u64, panel: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_render_inspector(app, panel)
}

#[wasm_bindgen]
pub fn document_render_inspector_options(ptr: u64, panel: String, options: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_render_inspector_options(app, panel, options)
}

#[wasm_bindgen]
pub fn document_set_run_mode(ptr: u64, source: String, mode: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_set_run_mode(app, source, mode)
}

#[wasm_bindgen]
pub fn document_next_ids(ptr: u64, n: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_next_ids(app, n)
}

#[wasm_bindgen]
pub fn document_next_relation_ids(ptr: u64, n: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_next_relation_ids(app, n)
}

#[wasm_bindgen]
pub fn document_place(ptr: u64, parent: String, after: String, count: u32) -> Result<String, JsValue> {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_place(app, parent, after, count).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn document_inspect_bind(ptr: u64, node: String, leaf: String, expr: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_inspect_bind(app, node, leaf, expr)
}

#[wasm_bindgen]
pub fn document_text_style(ptr: u64, node: String, patch: String) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::document_text_style(app, node, patch)
}

/// A style shortcut in a text edit (`bold`, `italic`, `underline`, `strike`).
#[wasm_bindgen]
pub fn text_toggle(ptr: u64, what: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::text_toggle(app, what)
}

/// The font picker's list (`engine_api::fonts`). The handle is unused:
/// fonts are a process resource.
#[wasm_bindgen]
pub fn document_fonts(_ptr: u64) -> String {
    crate::engine_api::fonts()
}

#[wasm_bindgen]
pub fn document_font_previews(_ptr: u64, names: Vec<String>) -> String {
    crate::engine_api::font_previews(names)
}

#[wasm_bindgen]
pub fn document_schema() -> String {
    crate::engine_api::document_schema()
}

#[wasm_bindgen]
pub fn undo(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::undo(app)
}

#[wasm_bindgen]
pub fn redo(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::redo(app)
}

#[wasm_bindgen]
pub fn set_modifiers(ptr: u64, bits: u8) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_modifiers(app, bits)
}

#[wasm_bindgen]
pub fn tool_key(ptr: u64, key: String, bits: u8) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::tool_key(app, key, bits)
}

#[wasm_bindgen]
pub fn clipboard_paste(ptr: u64, text: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::clipboard_paste(app, text)
}

#[wasm_bindgen]
pub fn key_down(ptr: u64, key: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::key_down(app, key)
}

#[wasm_bindgen]
pub fn key_up(ptr: u64, key: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::key_up(app, key)
}

#[wasm_bindgen]
pub fn solve_bench(iterations: u32, repeats: u32) -> String {
    crate::engine_api::solve_bench(iterations, repeats)
}

/// A font file's bytes, fetched by the host after an `EngineEvent::Fonts`
/// (plans/specs/text.md §4). Returns the family names it registered. Fonts
/// are a process resource, so no app handle is needed.
#[wasm_bindgen]
pub fn register_font(bytes: Vec<u8>) -> String {
    iron_text::engine().register(bytes).join(", ")
}

/// The host's textarea changed: the whole draft and its selection, in
/// UTF-16 offsets (`bevy_app::text_tool`).
#[wasm_bindgen]
pub fn text_input(ptr: u64, text: String, anchor: u32, focus: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::text_input(app, text, anchor, focus)
}

/// A caret motion only the canvas layout knows (`up`, `down`, `home`, `end`).
#[wasm_bindgen]
pub fn text_motion(ptr: u64, motion: String, extend: bool) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::text_motion(app, motion, extend)
}

/// The font catalog as JSON, for the picker.
#[wasm_bindgen]
pub fn font_catalog() -> String {
    iron_text::catalog::to_json()
}

/// Deliver engine-owned Rust events through the existing wasm callbacks.
fn flush_events(app: &mut WorkerApp) {
    use crate::host_events::EngineEvent;
    let events=app.world().resource::<crate::host_events::HostEvents>().drain();
    for event in events {
        match event {
            EngineEvent::Hover { list } => send_hover_from_worker(list.into_iter().map(JsValue::from).collect()),
            EngineEvent::Selection { list } => send_selection_from_worker(list.into_iter().map(JsValue::from).collect()),
            EngineEvent::DocumentChanged { version } => send_document_changed_from_worker(version),
            EngineEvent::Cursor { cursor } => send_cursor_from_worker(cursor),
            EngineEvent::Tool { tool } => send_tool_from_worker(tool),
            EngineEvent::ConvertQuestion { question } => send_convert_from_worker(question),
            EngineEvent::Clipboard { text } => send_clipboard_from_worker(text),
            EngineEvent::TextEdit { .. } => send_text_edit_from_worker(serde_json::to_string(&event_json(&event)).unwrap_or_default()),
            EngineEvent::Fonts { fonts } => send_fonts_from_worker(serde_json::to_string(&fonts).unwrap_or_default()),
            EngineEvent::LinkDrop { .. } => send_event_from_worker(serde_json::to_string(&event_json(&event)).unwrap_or_default()),
        }
    }
}
