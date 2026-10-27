use crate::bevy_app::init_app;
use crate::{ActivityControl, WorkerApp, canvas_view::*};
use bevy::app::PluginsState;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use wasm_bindgen::prelude::*;

use bevy::window::CursorMoved;

#[wasm_bindgen]
extern "C" {
    /// 在 app 初始化完成前，info! 无法使用，打印日志得用它
    #[wasm_bindgen(js_namespace = console)]
    pub(crate) fn log(s: &str);

    /// Any other event, as its JSON (`ty` names it): the host posts it on.
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_event_from_worker(event: String);
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
pub fn mouse_move(ptr: u64, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::mouse_move(app, x, y)
}

#[wasm_bindgen]
pub fn set_scale_factor(ptr: u64, scale_factor: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_api::set_scale_factor(app, scale_factor)
}

/// Shared semantic input/control routing. Surface and frame ownership stay in the host.
#[wasm_bindgen]
pub fn engine_message(ptr: u64, message_json: String) -> Result<String, JsValue> {
    if ptr == 0 { return Err(JsValue::from_str("engine is not attached")); }
    let request = serde_json::from_str(&message_json)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let (reply, _) = crate::engine_protocol::dispatch(app, request);
    serde_json::to_string(&reply).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Document operations share the same dispatcher as the native host.
#[wasm_bindgen]
pub fn document_call(ptr: u64, name: String, args_json: String) -> Result<String, JsValue> {
    let args: Vec<serde_json::Value> = serde_json::from_str(&args_json)
        .map_err(|e| JsValue::from_str(&format!("invalid document arguments: {e}")))?;
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    crate::engine_dispatch::call(app, &name, &args).map_err(|e| JsValue::from_str(&e))
}

/// A font file's bytes, fetched by the host after an `EngineEvent::Fonts`
/// (plans/specs/text.md §4). Returns the family names it registered. Fonts
/// are a process resource, so no app handle is needed.
#[wasm_bindgen]
pub fn register_font(bytes: Vec<u8>) -> String {
    iron_text::engine().register(bytes).join(", ")
}

/// Deliver engine-owned Rust events through the existing wasm callbacks.
fn flush_events(app: &mut WorkerApp) {
    for event in app.world().resource::<crate::host_events::HostEvents>().drain() {
        send_event_from_worker(serde_json::to_string(&event).expect("engine event is serializable"));
    }
}
