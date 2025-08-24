use crate::bevy_app::init_app;
use crate::{
    ActivityControl, DragState, WorkerApp, canvas_view::*, create_canvas_window,
    update_canvas_windows,
};
use bevy::app::PluginsState;
use bevy::ecs::system::SystemState;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, Window};
use js_sys::BigInt;
use wasm_bindgen::prelude::*;

// Import Bevy's input types that your FFI functions will create events for
use bevy::input::{
    ButtonState,                                            // Added ButtonState
    keyboard::{Key, KeyCode as BevyKeyCode, KeyboardInput}, // Added Key, BevyKeyCode, KeyboardInput, NativeKey
    mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel},
};
use bevy::window::CursorMoved; // CursorMoved is used in mouse_move. Removed WindowResized

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

    // Inspector streaming callbacks
    pub(crate) fn send_inspector_update_from_worker(update_json: &str);
    /// 从主线程环境发送
    // pub(crate) fn send_pick_from_rust(list: js_sys::Array);

    /// 执行阻塞
    /// 由于 wasm 环境不支持 std::thread, 交由 js 环境代为执行
    ///
    /// 在 worker 环境执行
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn block_from_worker();
    /// 在主线程环境执行
    /// english: Execute blocking in the main thread environment
    pub(crate) fn block_from_rust();
}

#[wasm_bindgen]
pub fn init_bevy_app() -> u64 {
    let mut app = init_app();
    // 添加自定义的 canvas 窗口插件
    app.add_plugins(CanvasViewPlugin);

    info!("init_bevy_app");

    // 包装成无生命周期的指针
    // english: Wrap it into a non-lifetime pointer
    Box::into_raw(Box::new(app)) as u64
}

// // 创建 Canvas 窗口
// #[wasm_bindgen]
// pub fn create_window_by_canvas(ptr: u64, canvas_id: &str, scale_factor: f32) {
//     let app = unsafe { &mut *(ptr as *mut WorkerApp) };
//     app.scale_factor = scale_factor;

//     // 完成自定义 canvas 窗口的创建
//     let canvas = Canvas::new(canvas_id, 1);
//     let view_obj = ViewObj::from_canvas(canvas);

//     create_window(app, view_obj, false);
// }

/// 创建离屏窗口（带 canvas_id 与 window_kind，用于稳定匹配）
#[wasm_bindgen]
pub fn create_window_by_offscreen_canvas(
    ptr: u64,
    canvas: web_sys::OffscreenCanvas,
    scale_factor: f32,
) {
    // Backward-compatible API without IDs; infer kind by order (1st viewer, 2nd timeline)
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    app.scale_factor = scale_factor;

    // Use a small resource to count windows
    #[derive(Resource, Default)]
    struct InitOrderCounter { n: u32 }
    if !app.world().contains_resource::<InitOrderCounter>() {
        app.insert_resource(InitOrderCounter { n: 0 });
    }
    let order = {
        let mut c = app.world_mut().resource_mut::<InitOrderCounter>();
        c.n += 1;
        c.n
    };

    let (canvas_id, kind) = if order == 1 { ("viewer-canvas".to_string(), "viewer".to_string()) } else { ("timeline-canvas".to_string(), "timeline".to_string()) };

    let offscreen_canvas = OffscreenCanvas::new(canvas, scale_factor, 1);
    let view_obj = ViewObj::from_offscreen_canvas(offscreen_canvas);
    info!("create_window_by_offscreen_canvas[compat]: {} ({})", canvas_id, kind);

    create_window(app, view_obj, true, canvas_id, kind);
}

/// 创建离屏窗口（带 canvas_id 与 window_kind，用于稳定匹配）
#[wasm_bindgen]
pub fn create_window_by_offscreen_canvas_with_id(
    ptr: u64,
    canvas: web_sys::OffscreenCanvas,
    scale_factor: f32,
    canvas_id: String,
    window_kind: String,
) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    app.scale_factor = scale_factor;

    let offscreen_canvas = OffscreenCanvas::new(canvas, scale_factor, 1);
    let view_obj = ViewObj::from_offscreen_canvas(offscreen_canvas);
    info!("create_window_by_offscreen_canvas_with_id: {} ({})", canvas_id, window_kind);

    create_window(app, view_obj, true, canvas_id, window_kind);
}

fn create_window(
    app: &mut WorkerApp,
    view_obj: ViewObj,
    is_in_worker: bool,
    canvas_id: String,
    window_kind: String,
) {
    use crate::canvas_view::CanvasName;

    // Spawn a Window entity first so the CanvasViewPlugin can associate the ViewObj
    let title = match window_kind.as_str() {
        "timeline" => "Timeline".to_owned(),
        "viewer" => "Viewer".to_owned(),
        other => other.to_owned(),
    };
    let is_viewer = window_kind == "viewer";
    // If this should be the primary window, first remove old PrimaryWindow markers
    if is_viewer {
        let mut remove_state: SystemState<Query<Entity, With<PrimaryWindow>>> =
            SystemState::from_world(app.world_mut());
        // Collect entities while holding the borrow once
        let existing: Vec<Entity> = {
            let mut world = app.world_mut();
            let q = remove_state.get_mut(&mut world);
            q.iter().collect()
        };
        {
            let mut world = app.world_mut();
            for e in existing {
                world.entity_mut(e).remove::<PrimaryWindow>();
            }
            remove_state.apply(&mut world);
        }
    }

    // Now spawn the window and optionally tag it as primary
    let entity = {
        let mut world = app.world_mut();
        let mut ecmd = world.spawn(Window { title, ..default() });
        if is_viewer {
            ecmd.insert(PrimaryWindow);
        }
        ecmd.id()
    };
    {
        let mut world = app.world_mut();
        world.entity_mut(entity).insert(CanvasName(canvas_id.clone()));
    }

    // Provide the ViewObj for this Added<Window>
    app.insert_non_send_resource(view_obj);
    create_canvas_window(app);

    // Activity control
    let mut act = ActivityControl::new();
    act.is_in_worker = is_in_worker;
    app.insert_resource(act);
}

/// Helper: tag the most recently created window with CanvasName and set a predictable title
fn tag_last_created_window(_app: &mut WorkerApp, _canvas_id: &str, _window_kind: &str) {}

/// Check if plugin initialization is completed
/// Frame rendering cannot be called before initialization is complete
#[wasm_bindgen]
pub fn is_preparation_completed(ptr: u64) -> u32 {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    // Creating device/queue is asynchronous, completion timing is uncertain
    // once the plugins are ready and loaded, then
    if app.plugins_state() == PluginsState::Ready {
        app.finish();
        app.cleanup();

        // Choose a default window deterministically. Prefer a window tagged with CanvasName("viewer-canvas").
        let mut windows_system_state: SystemState<Query<(Entity, Option<&crate::canvas_view::CanvasName>, &Window)>> =
            SystemState::from_world(app.world_mut());
        let mut q = windows_system_state.get_mut(app.world_mut());
        let mut chosen: Option<Entity> = None;
        for (entity, name, _win) in q.iter_mut() {
            if let Some(n) = name {
                if n.0 == "viewer-canvas" {
                    chosen = Some(entity);
                    break;
                }
            }
            if chosen.is_none() {
                chosen = Some(entity);
            }
        }
        if let Some(entity) = chosen {
            app.window = entity;
            return 1;
        }
    }
    0
}

/// Set mouse position without triggering activity (for batched updates)
#[wasm_bindgen]
pub fn set_mouse_position(ptr: u64, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let position = app.to_physical_size(x, y);
    let cursor_move = CursorMoved {
        window: app.window,
        position,
        delta: None,
    };
    app.world_mut().send_event(cursor_move);
    // Note: No activity trigger - this will be handled by enter_frame
}

/// 包装一个鼠标事件发送给 app
#[wasm_bindgen]
pub fn mouse_move(ptr: u64, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    // 提前将逻辑像转换成物理像素
    let position = app.to_physical_size(x, y);
    let cursor_move = CursorMoved {
        window: app.window,
        position,
        delta: None,
    };
    app.world_mut().send_event(cursor_move);

    let mut active_info = app
        .world_mut()
        .get_resource_mut::<ActivityControl>()
        .unwrap();
    active_info.remaining_frames = 10;
}

/// Frame rendering with optional mouse position update
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
        app.world_mut().send_event(cursor_move);
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
        // 模拟阻塞
        let active_info = app.world().get_resource::<ActivityControl>().unwrap();
        if active_info.is_in_worker {
            block_from_worker();
        } else {
            block_from_rust();
        }

        app.update();
    }
}

/// 鼠标滚轮事件处理
///
/// # Parameters
///
/// - `ptr`: WorkerApp 的指针
/// - `delta_x`: X 轴滚动增量
/// - `delta_y`: Y 轴滚动增量
/// - `delta_mode`: 滚动单位模式
#[wasm_bindgen]
pub fn mouse_wheel(ptr: u64, delta_x: f32, delta_y: f32, delta_mode: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let unit = match delta_mode {
        0 => MouseScrollUnit::Pixel, // DOM_DELTA_PIXEL
        1 => MouseScrollUnit::Line,  // DOM_DELTA_LINE
        2 => MouseScrollUnit::Line,  // DOM_DELTA_PAGE (treat as lines for simplicity)
        _ => MouseScrollUnit::Line,
    };

    let event = MouseWheel {
        // This event is read by Bevy's accumulate_mouse_scroll_system
        unit,
        x: delta_x,
        y: delta_y,
        window: app.window,
    };
    app.world_mut().send_event(event);

    let mut active_info = app
        .world_mut()
        .get_resource_mut::<ActivityControl>()
        .unwrap();
    active_info.remaining_frames = 10;
}

#[wasm_bindgen]
pub fn resize(ptr: u64, width: f32, height: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    // Directly modify the window resolution
    update_canvas_windows(app, width, height);
}

/// Mouse left button down (no entity id needed; Rust picking determines target)
#[wasm_bindgen]
pub fn left_bt_down(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let event = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: app.window,
    };
    app.world_mut().send_event(event);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// 鼠标左键松开
#[wasm_bindgen]
pub fn left_bt_up(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    if let Some(mut drag_state) = app.world_mut().get_resource_mut::<DragState>() {
        drag_state.target = None;
        drag_state.kind = None;
    }

    // Send Bevy MouseButtonInput event
    let event = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window: app.window,
    };
    app.world_mut().send_event(event);

    // If you need to modify active_info again, get it again
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// 鼠标右键按下
#[wasm_bindgen]
pub fn right_bt_down(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let event = MouseButtonInput {
        button: MouseButton::Right,
        state: ButtonState::Pressed,
        window: app.window,
    };
    app.world_mut().send_event(event);
    let mut active_info = app
        .world_mut()
        .get_resource_mut::<ActivityControl>()
        .unwrap();
    active_info.remaining_frames = 10;
}

/// 鼠标右键松开
#[wasm_bindgen]
pub fn right_bt_up(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let event = MouseButtonInput {
        button: MouseButton::Right,
        state: ButtonState::Released,
        window: app.window,
    };
    app.world_mut().send_event(event);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

// Inbound hover/selection setters removed; Rust is authoritative now. Keep optional FFI if UI wants to force selection later.

/// 打开 / 关闭动画
#[wasm_bindgen]
pub fn set_auto_animation(ptr: u64, needs_animate: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let mut active_info = app
        .world_mut()
        .get_resource_mut::<ActivityControl>()
        .unwrap();
    active_info.auto_animate = needs_animate > 0;
}

fn map_key_str_to_bevy_key(key_str: &str) -> Option<(BevyKeyCode, Key)> {
    // This is a simplified mapping. A more comprehensive one might be needed.
    // The `Key` (logical key) part can be more complex depending on desired behavior.
    match key_str.to_lowercase().as_str() {
        "w" => Some((BevyKeyCode::KeyW, Key::Character("w".into()))),
        "a" => Some((BevyKeyCode::KeyA, Key::Character("a".into()))),
        "s" => Some((BevyKeyCode::KeyS, Key::Character("s".into()))),
        "d" => Some((BevyKeyCode::KeyD, Key::Character("d".into()))),
        "g" => Some((BevyKeyCode::KeyG, Key::Character("g".into()))),
        "f" => Some((BevyKeyCode::KeyF, Key::Character("f".into()))),
        " " | "space" => Some((BevyKeyCode::Space, Key::Space)),
        "shift" | "shiftleft" => Some((BevyKeyCode::ShiftLeft, Key::Shift)), // Assuming ShiftLeft
        "control" | "controlleft" => Some((BevyKeyCode::ControlLeft, Key::Control)), // Assuming ControlLeft
        // Add more mappings as needed
        _ => None,
    }
}

/// Handle key down event
#[wasm_bindgen]
pub fn key_down(ptr: u64, key: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    if let Some((bevy_key_code, logical_key)) = map_key_str_to_bevy_key(&key) {
        let event = KeyboardInput {
            key_code: bevy_key_code,
            logical_key,
            text: None,
            state: ButtonState::Pressed,
            window: app.window,
            repeat: false,
        };

        // info!("sending key event: {:?}", event);
        app.world_mut().send_event(event);
    }

    // Original ActiveInfo update (can be removed if camera controller fully relies on ButtonInput)
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// Handle key up event
#[wasm_bindgen]
pub fn key_up(ptr: u64, key: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    if let Some((bevy_key_code, logical_key)) = map_key_str_to_bevy_key(&key) {
        let event = KeyboardInput {
            key_code: bevy_key_code,
            logical_key,
            state: ButtonState::Released,
            window: app.window,
            text: None,
            repeat: false,
        };
        app.world_mut().send_event(event);
    }

    // Original ActiveInfo update (can be removed if camera controller fully relies on ButtonInput)
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// Frame rendering
///
/// When render is running in a worker, the main thread may post a rendering message
/// before the render has finished updating the current frame
///
/// TODO: Need to check if the resources required for the frame have been fully loaded,
/// otherwise accumulated updates might cause stack overflow
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
        // 模拟阻塞
        let active_info = app.world().get_resource::<ActivityControl>().unwrap();
        if active_info.is_in_worker {
            block_from_worker();
        } else {
            block_from_rust();
        }

        app.update();
    }
}

// TODO
// #[wasm_bindgen]
// process_reflection_command(command_json: &str)
// to be written
// should tke in a BrpRequest
// process it to get the command

// execute the command

// 释放 engine 实例
#[wasm_bindgen]
pub fn release_app(ptr: u64) {
    // 将指针转换为其指代的实际 Rust 对象，同时也拿回此对象的内存管理权
    let app: Box<App> = unsafe { Box::from_raw(ptr as *mut _) };
    crate::close_bevy_window(app);
}

/// 将 js 数组转换为 rust HashMap
fn to_map(arr: js_sys::Array) -> HashMap<Entity, u64> {
    let mut map: HashMap<Entity, u64> = HashMap::default();
    let length = arr.length();
    for i in 0..length {
        let value = bigint_to_u64(arr.get(i));
        if let Ok(v) = value {
            let entity = Entity::from_bits(v);
            map.insert(entity, v);
        }
    }
    map
}

/// 将 js BigInt 转换成 rust u64
/// 测试了几种方式，只有下边的能方式转换成功
fn bigint_to_u64(value: JsValue) -> Result<u64, JsValue> {
    if let Ok(big_int) = BigInt::new(&value) {
        // 转换为字符串，基数为10
        let big_int_str = big_int.to_string(10).unwrap().as_string();
        let big_int_u64: Result<u64, _> = big_int_str.unwrap().parse::<u64>();
        if let Ok(number) = big_int_u64 {
            return Ok(number);
        }
    }
    Err(JsValue::from_str("Value is not a valid u64"))
}
