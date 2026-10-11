use crate::bevy_app::init_app;
use crate::panels::{PanelRect, Panels};
use crate::{ActivityControl, WorkerApp, canvas_view::*};
use bevy::app::PluginsState;
use bevy::ecs::system::SystemState;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use js_sys::BigInt;
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
}

/// Build the Bevy app around the canvas it will render into.
///
/// The canvas arrives here rather than in a later call because `RenderPlugin`
/// resolves the GPU adapter while it is being added, and needs the primary
/// window to already exist. On WebGPU an adapter can be found without one, so
/// the previous split (`init_bevy_app` then `create_window_by_offscreen_canvas`)
/// worked by accident; on WebGL2 the adapter *is* the canvas's WebGL2 context,
/// so a windowless adapter request finds nothing and Bevy panics with
/// "Unable to find a GPU!".
///
/// In worker mode the canvas is a transferred `OffscreenCanvas`; in main-thread
/// mode it is the HTML canvas element, which is structurally compatible.
///
/// `variant_flags` selects a perf-grid variant (see `bevy_app::VARIANT_*`); 0 = normal app.
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

    let mut app = init_app(variant_flags, view_obj);
    app.scale_factor = scale_factor;

    let mut act = ActivityControl::new();
    act.is_in_worker = is_in_worker;
    app.insert_resource(act);

    // 包装成无生命周期的指针
    // english: Wrap it into a non-lifetime pointer
    Box::into_raw(Box::new(app)) as u64
}

/// Upsert a panel rectangle (physical px, top-left origin, window coordinates).
/// The `kind` decides what the panel drives ("composition" = one
/// composition, 2D or 3D, named by `composition`; "timeline" = the timeline
/// vello region; "ui" = a placeholder background).
#[wasm_bindgen]
pub fn set_panel_viewport(ptr: u64, id: String, kind: String, composition: String, x: f32, y: f32, w: f32, h: f32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let Some(mut panels) = app.world_mut().get_resource_mut::<Panels>() else {
        return;
    };
    panels.upsert(&id, &kind, &composition, PanelRect { x, y, w, h });

    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// Remove a panel.
#[wasm_bindgen]
pub fn despawn_panel(ptr: u64, id: String) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let Some(mut panels) = app.world_mut().get_resource_mut::<Panels>() else {
        return;
    };
    panels.remove(&id);

    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
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
    app.world_mut().write_message(cursor_move);
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
    app.world_mut().write_message(cursor_move);

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
        // bevy 0.19 added a touch phase. Per its own docs, a mouse is always Moved.
        phase: bevy::input::touch::TouchPhase::Moved,
    };
    app.world_mut().write_message(event);

    let mut active_info = app
        .world_mut()
        .get_resource_mut::<ActivityControl>()
        .unwrap();
    active_info.remaining_frames = 10;
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

/// Mouse left button down (no entity id needed; Rust picking determines target)
#[wasm_bindgen]
pub fn left_bt_down(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let event = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: app.window,
    };
    app.world_mut().write_message(event);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// 鼠标左键松开
#[wasm_bindgen]
pub fn left_bt_up(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    // Only the event crosses here. Drag end is decided by the interaction
    // systems from this event, which is what lets the release commit the
    // drag as a transaction (document_bridge); clearing `DragState` from the
    // FFI pre-empted that.
    let event = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window: app.window,
    };
    app.world_mut().write_message(event);

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
    app.world_mut().write_message(event);
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
    app.world_mut().write_message(event);
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

// ---- Document: the four-call FFI of plans/document-spine.md §9 ----------
// `apply` arrives through gestures for now; the model surface adds its own
// caller in step 7. These stay thin: parse, hand to a resource, return.

/// The canonical JSON of the current document.
#[wasm_bindgen]
pub fn document_save(ptr: u64) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    app.world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .map(|s| s.0.save())
        .unwrap_or_default()
}

/// Replace the document. Validated here, so a bad file is an error to the
/// caller and never reaches the store; applied on the next frame.
#[wasm_bindgen]
pub fn document_load(ptr: u64, text: String) -> Result<(), JsValue> {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let doc = iron_document::serial::load(&text).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let mut load = app
        .world_mut()
        .get_resource_mut::<crate::document_bridge::PendingLoad>()
        .ok_or_else(|| JsValue::from_str("document bridge not installed"))?;
    load.0 = Some(doc);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    Ok(())
}

/// Render a view. `query_json` is an `iron_document::ViewQuery`, e.g.
/// `{"scope": null, "fidelity": "summary", "depth": 2}`.
#[wasm_bindgen]
pub fn document_view(ptr: u64, query_json: String) -> Result<String, JsValue> {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let query: iron_document::ViewQuery =
        serde_json::from_str(&query_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let store = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .ok_or_else(|| JsValue::from_str("document bridge not installed"))?;
    store.0.view(&query).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Apply a transaction now and answer synchronously: the model surface needs
/// every error in one round trip (doc 04 §9.5). `expect_version`, when not
/// zero, is compare-and-swap: the transaction is refused if the document has
/// moved since the caller last read it. Returns JSON:
/// `{"ok":true,"version":N}` or `{"ok":false,"errors":[...]}`.
#[wasm_bindgen]
pub fn document_apply(ptr: u64, transaction_json: String, expect_version: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let input: iron_document::TransactionInput = match serde_json::from_str(&transaction_json) {
        Ok(t) => t,
        Err(e) => {
            return serde_json::json!({"ok": false, "errors": [{"op": null, "kind": "parse", "message": e.to_string()}]})
                .to_string();
        }
    };
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "errors": [{"op": null, "kind": "no_store", "message": "document bridge not installed"}]}).to_string();
    };
    let current = store.0.document().version().0 as u32;
    if expect_version != 0 && expect_version != current {
        return serde_json::json!({
            "ok": false,
            "version": current,
            "errors": [{"op": null, "kind": "stale", "message": format!("expected version {expect_version}, document is at {current}; re-read before retrying")}]
        })
        .to_string();
    }
    let result = match store.0.apply(input) {
        Ok(applied) => match applied.solver {
            // Causes, not effects (CS §9.2): which clusters re-solved, which
            // nodes moved, and any requested value the solver did not honour.
            Some(solver) => serde_json::json!({"ok": true, "version": applied.version.0, "solver": solver}),
            None => serde_json::json!({"ok": true, "version": applied.version.0}),
        },
        Err(errors) => serde_json::json!({"ok": false, "version": current, "errors": errors}),
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    result.to_string()
}

/// Link two pictures to one entity (ECM §7.2) and apply it now, as `actor`
/// ("human" or "model"). `keep` is "" or the entity to keep when both
/// pictures already have one. Returns JSON: `{"ok":true,"version":N,
/// "entity":"#e"}`; `{"ok":false,"question":{a,b,port_collisions}}` when
/// the survivor must be chosen; `{"ok":false,"error":"…"}` otherwise.
#[wasm_bindgen]
pub fn document_link(ptr: u64, a: String, b: String, keep: String, actor: String) -> String {
    use iron_document::composition::{self, LinkError};
    use iron_document::NodeId;
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let parse = |s: &str| s.parse::<NodeId>().map_err(|e| e.to_string());
    let (a, b) = match (parse(&a), parse(&b)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return serde_json::json!({"ok": false, "error": e}).to_string(),
    };
    let keep = if keep.is_empty() {
        None
    } else {
        match parse(&keep) {
            Ok(k) => Some(k),
            Err(e) => return serde_json::json!({"ok": false, "error": e}).to_string(),
        }
    };
    let actor = if actor == "model" { iron_document::Actor::Model } else { iron_document::Actor::Human };
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "document bridge not installed"}).to_string();
    };
    let result = match store.0.link(a, b, keep, actor) {
        Err(LinkError::Question(q)) => serde_json::json!({"ok": false, "question": q}),
        Err(e @ LinkError::AlreadyLinked(entity)) => serde_json::json!({"ok": false, "error": e.to_string(), "entity": entity.to_string()}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
        Ok(tx) => match store.0.apply(tx) {
            Ok(applied) => {
                let entity = composition::figure_of(store.0.document(), a).map(|(_, e)| e.to_string());
                serde_json::json!({"ok": true, "version": applied.version.0, "entity": entity})
            }
            Err(errors) => serde_json::json!({"ok": false, "error": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")}),
        },
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    result.to_string()
}

/// Every picture of the entity `node` stands for, with its composition: what
/// "Show in" offers (ECM §7.2). JSON `{"entity":"#e"|null,"figures":[{"node",
/// "composition","name"}]}`.
#[wasm_bindgen]
pub fn document_counterparts(ptr: u64, node: String) -> String {
    use iron_document::composition;
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"entity": null, "figures": []}).to_string();
    };
    let doc = store.0.document();
    let Some((_, entity)) = node.parse().ok().and_then(|n| composition::figure_of(doc, n)) else {
        return serde_json::json!({"entity": null, "figures": []}).to_string();
    };
    let figures: Vec<_> = composition::figures_of(doc, entity)
        .into_iter()
        .map(|f| {
            let c = composition::composition_of(doc, f);
            let name = c
                .and_then(|c| doc.get::<iron_document::component::Name>(c))
                .and_then(|n| n.text.constant())
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            serde_json::json!({"node": f.to_string(), "composition": c.map(|c| c.to_string()), "name": name})
        })
        .collect();
    serde_json::json!({"entity": entity.to_string(), "figures": figures}).to_string()
}

/// "Show in": frame `node` in every open panel that shows it, directly or
/// through embeds — a 2D panel pans it to the centre, a 3D panel's camera
/// looks at it (served next frame by `apply_focus`, which logs each panel).
/// JSON `{"ok":true}`, or `{"ok":false,"error"}` for a node outside every
/// composition.
#[wasm_bindgen]
pub fn document_focus(ptr: u64, node: String) -> String {
    use iron_document::composition;
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let Ok(id) = node.parse::<iron_document::NodeId>() else {
        return serde_json::json!({"ok": false, "error": format!("bad id {node:?}")}).to_string();
    };
    let Some(c) = app.world().get_resource::<crate::document_bridge::DocumentStore>().and_then(|s| composition::composition_of(s.0.document(), id)) else {
        return serde_json::json!({"ok": false, "error": format!("{id} is not in a composition")}).to_string();
    };
    let _ = c;
    if let Some(mut focus) = app.world_mut().get_resource_mut::<crate::document_bridge::FocusRequests>() {
        focus.0.push(id);
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    serde_json::json!({"ok": true}).to_string()
}

/// Reserve `n` node ids for a batch of creates: JSON array of id strings.
#[wasm_bindgen]
pub fn document_next_ids(ptr: u64, n: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let ids: Vec<String> = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .map(|s| s.0.document().next_ids(n as usize).iter().map(|i| i.to_string()).collect())
        .unwrap_or_default();
    serde_json::to_string(&ids).expect("strings serialise")
}

#[wasm_bindgen]
pub fn document_next_relation_ids(ptr: u64, n: u32) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let ids: Vec<String> = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .map(|s| s.0.document().next_relation_ids(n as usize).iter().map(|i| i.to_string()).collect())
        .unwrap_or_default();
    serde_json::to_string(&ids).expect("strings serialise")
}

/// `count` sibling-order keys under `parent` ("" for the roots), placed
/// after `after` ("" = at the front, "end" = after the last child). JSON
/// array of key strings, in order. Order keys are the store's business; the
/// model surface only says where.
#[wasm_bindgen]
pub fn document_place(ptr: u64, parent: String, after: String, count: u32) -> Result<String, JsValue> {
    use iron_document::OrderKey;
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    let store = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .ok_or_else(|| JsValue::from_str("document bridge not installed"))?;
    let doc = store.0.document();
    let siblings: &[iron_document::NodeId] = if parent.is_empty() {
        doc.roots()
    } else {
        let p: iron_document::NodeId = parent.parse().map_err(|e: iron_document::id::IdParseError| JsValue::from_str(&e.to_string()))?;
        doc.children(p)
    };
    let key_of = |id: iron_document::NodeId| doc.live(id).map(|n| n.order.clone());
    let (lo, hi): (Option<OrderKey>, Option<OrderKey>) = if after == "end" {
        (siblings.last().and_then(|&s| key_of(s)), None)
    } else if after.is_empty() {
        (None, siblings.first().and_then(|&s| key_of(s)))
    } else {
        let a: iron_document::NodeId = after.parse().map_err(|e: iron_document::id::IdParseError| JsValue::from_str(&e.to_string()))?;
        let pos = siblings.iter().position(|&s| s == a).ok_or_else(|| JsValue::from_str(&format!("{a} is not a child of {}", if parent.is_empty() { "the root" } else { &parent })))?;
        (key_of(a), siblings.get(pos + 1).and_then(|&s| key_of(s)))
    };
    let mut keys = Vec::new();
    let mut prev = lo;
    for _ in 0..count {
        let k = OrderKey::between(prev.as_ref(), hi.as_ref()).map_err(|e| JsValue::from_str(&e.to_string()))?;
        keys.push(k.to_string());
        prev = Some(k);
    }
    Ok(serde_json::to_string(&keys).expect("strings serialise"))
}

/// The node-type and component registry as JSON: the one source for the
/// model's manifest and the shell's editors (plans/document-spine.md §5.7).
#[wasm_bindgen]
pub fn document_schema() -> String {
    iron_document::registry::schema_json().to_string()
}

/// Editor history, requested by the shell. Which keystroke means "undo" is
/// chrome policy on the main thread; only the command crosses here.
#[wasm_bindgen]
pub fn undo(ptr: u64) {
    history_request(ptr, crate::document_bridge::HistoryOp::Undo);
}

#[wasm_bindgen]
pub fn redo(ptr: u64) {
    history_request(ptr, crate::document_bridge::HistoryOp::Redo);
}

fn history_request(ptr: u64, op: crate::document_bridge::HistoryOp) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    if let Some(mut requests) = app
        .world_mut()
        .get_resource_mut::<crate::document_bridge::HistoryRequests>()
    {
        requests.0.push(op);
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
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
        app.world_mut().write_message(event);
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
        app.world_mut().write_message(event);
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
    let app: Box<WorkerApp> = unsafe { Box::from_raw(ptr as *mut WorkerApp) };
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

/// The solver's phase 1 benchmark (plans/constraint-system.md §14) in this
/// build: the 12-variable chain at up to `iterations` iterations, solved
/// `repeats` times. JSON with the per-solve time, so the same problem can be
/// compared with `cargo run -p iron_solve --release --example bench`.
#[wasm_bindgen]
pub fn solve_bench(iterations: u32, repeats: u32) -> String {
    use bevy::platform::time::Instant;
    let repeats = repeats.max(1);
    iron_solve::bench::run(iterations, 1);
    let t0 = Instant::now();
    let s = iron_solve::bench::run(iterations, repeats);
    let elapsed = t0.elapsed().as_secs_f64();
    serde_json::json!({
        "vars": s.vars.len(),
        "iterations": s.iterations,
        "evaluations": s.evaluations,
        "repeats": repeats,
        "us_per_solve": elapsed * 1e6 / repeats as f64,
        "max_required_residual": s.max_required_residual,
        "max_strong_residual": s.max_strong_residual,
    })
    .to_string()
}
