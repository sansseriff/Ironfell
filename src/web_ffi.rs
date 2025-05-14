use crate::bevy_app::init_app;
use crate::{ActiveInfo, WorkerApp, canvas_view::*, create_canvas_window, update_canvas_windows};
use bevy::app::PluginsState;
use bevy::ecs::system::SystemState;
use bevy::platform_support::collections::HashMap;
use bevy::prelude::*;
use js_sys::BigInt;
use wasm_bindgen::prelude::*;

// Import Bevy's input types that your FFI functions will create events for
use bevy::input::{
    ButtonState,                                                       // Added ButtonState
    keyboard::{Key, KeyCode as BevyKeyCode, KeyboardInput, NativeKey}, // Added Key, BevyKeyCode, KeyboardInput, NativeKey
    mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel},
};
use bevy::window::{CursorMoved, WindowResized}; // CursorMoved is used in mouse_move

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
    /// 从主线程环境发送
    // pub(crate) fn send_pick_from_rust(list: js_sys::Array);

    /// 执行阻塞
    /// 由于 wasm 环境不支持 std::thread, 交由 js 环境代为执行
    ///
    /// 在 worker 环境执行
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn block_from_worker();
    /// 在主线程环境执行
    pub(crate) fn block_from_rust();
}

#[wasm_bindgen]
pub fn init_bevy_app() -> u64 {
    let mut app = init_app();
    // 添加自定义的 canvas 窗口插件
    app.add_plugins(CanvasViewPlugin);

    info!("init_bevy_app");

    // 包装成无生命周期的指针
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

/// 创建离屏窗口
#[wasm_bindgen]
pub fn create_window_by_offscreen_canvas(
    ptr: u64,
    canvas: web_sys::OffscreenCanvas,
    scale_factor: f32,
) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    app.scale_factor = scale_factor;

    let offscreen_canvas = OffscreenCanvas::new(canvas, scale_factor, 1);
    let view_obj = ViewObj::from_offscreen_canvas(offscreen_canvas);

    create_window(app, view_obj, true);
}

fn create_window(app: &mut WorkerApp, view_obj: ViewObj, is_in_worker: bool) {
    app.insert_non_send_resource(view_obj);

    let mut info = ActiveInfo::new();
    info.is_in_worker = is_in_worker;
    // 选中/高亮 资源
    app.insert_resource(info);

    create_canvas_window(app);
}

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

        // Store the window object directly on the app to avoid subsequent queries
        let mut windows_system_state: SystemState<Query<(Entity, &Window)>> =
            SystemState::from_world(app.world_mut());
        if let Ok((entity, _)) = windows_system_state.get(app.world_mut()).single() {
            app.window = entity;
            return 1;
        }
    }
    0
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

    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    active_info.remaining_frames = 10;
}

/// Receives mouse wheel events from JavaScript and sends Bevy's MouseWheel event.
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

    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    active_info.remaining_frames = 10;
}

#[wasm_bindgen]
pub fn resize(ptr: u64, width: f32, height: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    // Directly modify the window resolution
    update_canvas_windows(app, width, height);
}

/// 鼠标左键按下
#[wasm_bindgen]
pub fn left_bt_down(ptr: u64, obj: JsValue, x: f32, y: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let position = app.to_physical_size(x, y);
    let window_entity = app.window; // Read app.window before active_info gets its borrow

    // Scope for the first set of operations involving active_info
    {
        let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
        let value = bigint_to_u64(obj);
        if let Ok(v) = value {
            let entity = Entity::from_bits(v);
            active_info.drag = entity;
            active_info.last_drag_pos = position;
            // 当前要 drap 的对象同时也是 selection 对象
            let mut map: HashMap<Entity, u64> = HashMap::default();
            map.insert(entity, 0);
            active_info.selection = map;
        }
    } // active_info goes out of scope here, releasing its mutable borrow of app.world

    // Send Bevy MouseButtonInput event
    let event = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: window_entity, // Use the stored window_entity
    };
    app.world_mut().send_event(event); // This is a new mutable borrow of app.world, which is fine now

    // If you need to modify active_info again, get it again
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    active_info.remaining_frames = 10;
}

/// 鼠标左键松开
#[wasm_bindgen]
pub fn left_bt_up(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    {
        let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
        active_info.drag = Entity::PLACEHOLDER;
    }

    // Send Bevy MouseButtonInput event
    let event = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window: app.window,
    };
    app.world_mut().send_event(event);

    // If you need to modify active_info again, get it again
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    active_info.remaining_frames = 10;
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
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
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
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    active_info.remaining_frames = 10;
}

/// 设置 hover（高亮） 效果
#[wasm_bindgen]
pub fn set_hover(ptr: u64, arr: js_sys::Array) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();

    // 将 js hover 列表转换为 rust 对象
    let hover = to_map(arr);
    // 更新 hover 数据
    active_info.hover = hover;

    active_info.remaining_frames = 10;
}

/// 设置 选中 效果
#[wasm_bindgen]
pub fn set_selection(ptr: u64, arr: js_sys::Array) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();

    // 将 js selection 列表转换为 rust 对象
    let selection = to_map(arr);
    // 更新 hover 数据
    active_info.selection = selection;

    active_info.remaining_frames = 10;
}

/// 打开 / 关闭动画
#[wasm_bindgen]
pub fn set_auto_animation(ptr: u64, needs_animate: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
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

        info!("sending key event: {:?}", event);
        app.world_mut().send_event(event);
    }

    // Original ActiveInfo update (can be removed if camera controller fully relies on ButtonInput)
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    match key.as_str() {
        "w" => active_info.w_pressed = true,
        "a" => active_info.a_pressed = true,
        "s" => active_info.s_pressed = true,
        "d" => active_info.d_pressed = true,
        _ => {}
    }
    active_info.remaining_frames = 10; // Ensure re-render
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
    let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
    match key.as_str() {
        "w" => active_info.w_pressed = false,
        "a" => active_info.a_pressed = false,
        "s" => active_info.s_pressed = false,
        "d" => active_info.d_pressed = false,
        _ => {}
    }
    active_info.remaining_frames = 10; // Ensure re-render
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
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    {
        // Check conditions for executing frame rendering
        let mut active_info = app.world_mut().get_resource_mut::<ActiveInfo>().unwrap();
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
        let active_info = app.world().get_resource::<ActiveInfo>().unwrap();
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
