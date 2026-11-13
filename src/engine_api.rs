//! Shared engine operations. Hosts decode arguments and borrow the engine only
//! on its owner thread; neither wasm pointers nor transport types enter here.
use crate::{ActivityControl, WorkerApp};
use crate::panels::{PanelRect, Panels};
use bevy::prelude::*;
use bevy::input::{ButtonState, keyboard::{Key, KeyCode as BevyKeyCode, KeyboardInput}, mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel}};
use bevy::window::CursorMoved;

/// A replacement host session keeps authored state but loses its old viewports
/// and input ownership. Cancellation still runs through the ordinary tool system.
pub fn reset_host_session(app: &mut WorkerApp) {
    if let Some(mut changes) = app.world_mut().get_resource_mut::<crate::document_bridge::ui::UiChanges>() { changes.reset_reader(); }
    if let Some(mut interests) = app.world_mut().get_resource_mut::<crate::document_bridge::ui::UiInterests>() { *interests = Default::default(); }
    let panels = app.world().resource::<Panels>().iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    for id in panels { despawn_panel(app, id); }
    tool_key(app, "escape".into(), 0);
    left_bt_up(app);
    middle_bt(app, false);
    set_modifiers(app, 0);
    app.world_mut().resource_mut::<ButtonInput<BevyKeyCode>>().release_all();
    app.world().resource::<crate::host_events::HostEvents>().drain();
}

pub fn set_panel_viewport(app: &mut WorkerApp, id: String, kind: String, composition: String, x: f32, y: f32, w: f32, h: f32) {
    let Some(mut panels) = app.world_mut().get_resource_mut::<Panels>() else {
        return;
    };
    panels.upsert(&id, &kind, &composition, PanelRect { x, y, w, h });

    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

pub fn despawn_panel(app: &mut WorkerApp, id: String) {
    let Some(mut panels) = app.world_mut().get_resource_mut::<Panels>() else {
        return;
    };
    panels.remove(&id);

    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

pub fn set_mouse_position(app: &mut WorkerApp, x: f32, y: f32) {
    let position = app.to_physical_size(x, y);
    let cursor_move = CursorMoved {
        window: app.window,
        position,
        delta: None,
    };
    app.world_mut().write_message(cursor_move);
    // Note: No activity trigger - this will be handled by enter_frame
}

pub fn mouse_move(app: &mut WorkerApp, x: f32, y: f32) {
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

pub fn mouse_wheel(app: &mut WorkerApp, delta_x: f32, delta_y: f32, delta_mode: u32) {

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

pub fn set_dark_mode(app: &mut WorkerApp, dark: bool) {
    let colour = if dark { crate::bevy_app::CANVAS_DARK } else { crate::bevy_app::CANVAS_LIGHT };
    app.world_mut().insert_resource(ClearColor(colour));
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

pub fn set_scale_factor(app: &mut WorkerApp, scale_factor: f32) {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        app.scale_factor = scale_factor;
        app.insert_resource(crate::Dpr(scale_factor as f64));
    }
}

pub fn left_bt_down(app: &mut WorkerApp) {
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

pub fn left_bt_down_at(app: &mut WorkerApp, x: f32, y: f32) {
    let position = app.to_physical_size(x, y);
    if let Some(mut press) = app.world_mut().get_resource_mut::<crate::bevy_app::PressPosition>() {
        press.0 = Some(position);
    }
    left_bt_down(app);
}

pub fn left_bt_up(app: &mut WorkerApp) {
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

pub fn right_bt_down(app: &mut WorkerApp) {
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

pub fn right_bt_up(app: &mut WorkerApp) {
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

pub fn middle_bt(app: &mut WorkerApp, down: bool) {
    let state = if down { ButtonState::Pressed } else { ButtonState::Released };
    let window = app.window;
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Middle, state, window });
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

pub fn set_auto_animation(app: &mut WorkerApp, needs_animate: u32) {
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

pub fn document_save(app: &mut WorkerApp) -> String {
    app.world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .map(|s| s.0.save())
        .unwrap_or_default()
}

pub fn document_load(app: &mut WorkerApp, text: String) -> Result<(), String> {
    let store = iron_document::Store::load(&text).map_err(|e| String::from(&e.to_string()))?;
    install(app, store)
}

/// Queue `store` to replace the current one at the next sync.
fn install(app: &mut WorkerApp, store: iron_document::Store) -> Result<(), String> {
    let mut load = app
        .world_mut()
        .get_resource_mut::<crate::document_bridge::PendingLoad>()
        .ok_or_else(|| String::from("document bridge not installed"))?;
    load.0 = Some(store);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    Ok(())
}

fn scope_id(id: &str) -> Result<iron_document::ScopeId, String> {
    id.parse().map_err(|e: iron_document::id::IdParseError| e.to_string())
}

/// Open a scope (plans/specs/scopes.md §4.3): load its snapshot, replay the
/// logged transactions newer than it in order, name it `id`, and start with
/// empty history. A transaction that fails stops the replay and keeps what
/// loaded; the reply says where, so the host can set the rest aside.
/// `{"version", "replayed", "skipped", "failed": null | {"index", "message"}}`,
/// `index` counting `log` lines.
pub fn scope_open(app: &mut WorkerApp, id: String, snapshot: String, log: Vec<String>) -> Result<String, String> {
    let id = scope_id(&id)?;
    let mut store = iron_document::Store::load(&snapshot).map_err(|e| e.to_string())?;
    let (mut replayed, mut skipped, mut failed) = (0usize, 0usize, serde_json::Value::Null);
    for (index, line) in log.iter().enumerate() {
        let tx: iron_document::Transaction = match serde_json::from_str(line) {
            Ok(t) => t,
            Err(e) => {
                failed = serde_json::json!({"index": index, "message": format!("unreadable: {e}")});
                break;
            }
        };
        let current = store.document().version();
        if tx.version <= current {
            skipped += 1;
            continue;
        }
        if tx.version != current.next() {
            failed = serde_json::json!({"index": index, "message": format!("version {} follows {}", tx.version.0, current.0)});
            break;
        }
        if let Err(errors) = store.replay(&tx) {
            let message = errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
            failed = serde_json::json!({"index": index, "message": message});
            break;
        }
        replayed += 1;
    }
    store.clear_history();
    store.set_scope(Some(id));
    let version = store.document().version().0;
    install(app, store)?;
    Ok(serde_json::json!({"version": version, "replayed": replayed, "skipped": skipped, "failed": failed}).to_string())
}

/// A new scope's starting document (the scope menu's New, scopes.md §6), as
/// snapshot text, built in a fresh store so the open document is untouched:
/// `"empty"` holds a `main` 2D composition, a library and a model; `"demo"`
/// is the demo scene with the contexts showcase. `{"snapshot"}`.
pub fn scope_template(_app: &mut WorkerApp, kind: String) -> Result<String, String> {
    let snapshot = crate::document_bridge::template_snapshot(&kind)?;
    Ok(serde_json::json!({"snapshot": snapshot}).to_string())
}

/// Name the current document's scope (`""`: none), as creating a scope from
/// what is open does. Applies to a load still waiting for the next sync,
/// if there is one, since that is what will be open.
pub fn scope_assign(app: &mut WorkerApp, id: String) -> Result<String, String> {
    let scope = if id.is_empty() { None } else { Some(scope_id(&id)?) };
    let reply = serde_json::json!({"scope": scope.map(|s| s.to_string())}).to_string();
    let world = app.world_mut();
    if let Some(mut load) = world.get_resource_mut::<crate::document_bridge::PendingLoad>()
        && let Some(store) = load.0.as_mut()
    {
        store.set_scope(scope);
        return Ok(reply);
    }
    let mut store = world
        .get_resource_mut::<crate::document_bridge::DocumentStore>()
        .ok_or_else(|| String::from("document bridge not installed"))?;
    store.0.set_scope(scope);
    if let Some(mut origin) = world.get_resource_mut::<crate::document_bridge::DocumentOrigin>() {
        *origin = crate::document_bridge::DocumentOrigin::for_scope(scope);
    }
    Ok(reply)
}

/// The durable log's feed (scopes.md §4.2): the open scope's committed
/// transactions after `since`, one JSON line each.
/// `{"scope", "version", "pending", "gap", "ndjson", "count"}`. `pending`: a
/// load has not yet been installed, so ask again after `documentChanged`.
/// `gap`: the history horizon dropped some, so write a snapshot instead.
pub fn scope_log(app: &mut WorkerApp, since: u64) -> Result<String, String> {
    let world = app.world();
    if world
        .get_resource::<crate::document_bridge::PendingLoad>()
        .is_some_and(|l| l.0.is_some())
    {
        return Ok(serde_json::json!({"pending": true}).to_string());
    }
    let store = &world
        .get_resource::<crate::document_bridge::DocumentStore>()
        .ok_or_else(|| String::from("document bridge not installed"))?
        .0;
    let scope = store.scope().map(|s| s.to_string());
    let version = store.document().version().0;
    let (gap, ndjson, count) = match store.log_since(iron_document::Version(since)) {
        None => (true, String::new(), 0),
        Some(txs) => {
            let mut out = String::new();
            for t in &txs {
                out.push_str(&serde_json::to_string(t).map_err(|e| e.to_string())?);
                out.push('\n');
            }
            (false, out, txs.len())
        }
    };
    Ok(serde_json::json!({"pending": false, "scope": scope, "version": version, "gap": gap, "ndjson": ndjson, "count": count}).to_string())
}

pub fn document_view(app: &mut WorkerApp, query_json: String) -> Result<String, String> {
    let query: iron_document::ViewQuery =
        serde_json::from_str(&query_json).map_err(|e| String::from(&e.to_string()))?;
    let store = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .ok_or_else(|| String::from("document bridge not installed"))?;
    store.0.view(&query).map_err(|e| String::from(&e.to_string()))
}

pub fn document_apply(app: &mut WorkerApp, transaction_json: String, expect_version: u32) -> String {
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

pub fn document_build_info(_app: &mut WorkerApp) -> String {
    let (profile, opt, simd, fast) = crate::document_bridge::stats::build_profile();
    serde_json::json!({
        "profile": profile,
        "opt": opt,
        "simd": simd,
        "fast": fast,
        "gfx": crate::host_services::graphics_backend(),
    })
    .to_string()
}

/// The FPS readout's data (a composition view's settings menu, "Show FPS"):
/// every panel's render count so far (`view_rates`; the shell divides two
/// readings' difference by their `t`s), the app's tick rate, and what draws.
pub fn document_frame_stats(app: &mut WorkerApp) -> String {
    use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
    let world = app.world_mut();
    let t = world.get_resource::<Time<Real>>().map_or(0.0, |t| t.elapsed_secs_f64());
    let tick = world.get_resource::<DiagnosticsStore>().and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FPS)).and_then(|d| d.smoothed());
    let canvas = world.query_filtered::<&bevy::window::Window, With<bevy::window::PrimaryWindow>>().iter(world).next().map(|w| [w.physical_size().x, w.physical_size().y]);
    let panel_of: Vec<(Entity, String)> = world.query::<(Entity, &crate::document_bridge::render::DocumentPanelLayer)>().iter(world).map(|(e, l)| (e, l.panel.clone())).collect();
    let (profile, opt, simd, fast) = crate::document_bridge::stats::build_profile();
    serde_json::json!({
        "t": t,
        "views": crate::view_rates::counts(&panel_of),
        "tick_fps": tick,
        "canvas": canvas,
        "build": {"profile": profile, "opt": opt, "simd": simd, "fast": fast, "gfx": crate::host_services::graphics_backend()},
    })
    .to_string()
}

/// The instance-cache benchmark's `meters` composition (`?meters=N`,
/// `__iron.meters(n)`): `n` dial meters, the first driven by a slider.
pub fn document_meters(app: &mut WorkerApp, n: u32) -> String {
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "document bridge not installed"}).to_string();
    };
    let out = crate::document_bridge::stress::meters_scene(&mut store.0, n as usize).unwrap_or_else(|e| serde_json::json!({"ok": false, "error": e}));
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    out.to_string()
}

pub fn document_stress(app: &mut WorkerApp, shapes: u32) -> String {
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "document bridge not installed"}).to_string();
    };
    let (input, ids) = crate::document_bridge::stress::stress_scene(&store.0, shapes as usize);
    let result = match store.0.apply(input) {
        Ok(applied) => serde_json::json!({
            "ok": true,
            "version": applied.version.0,
            "composition": ids.composition.to_string(),
            "slider": ids.slider.to_string(),
            "field": ids.field.to_string(),
            "sample": ids.sample.to_string(),
            "sample_at": [ids.sample_at.0, ids.sample_at.1],
            "nodes": ids.nodes,
            "width": ids.width,
            "height": ids.height,
        }),
        Err(errors) => serde_json::json!({"ok": false, "errors": errors}),
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    result.to_string()
}

/// Object snapping on or off for the select tool (plans/specs/snapping.md),
/// and the guides of the move in progress.
pub fn document_snapping(app: &mut WorkerApp, enabled: bool) -> String {
    let mut tool = app.world_mut().resource_mut::<crate::bevy_app::select::SelectTool>();
    tool.snap_off = !enabled;
    // The current move's guides, `x=…` or `y=…` with their marks' count.
    let guides: Vec<String> = tool.snap_guides().iter().map(|g| format!("{}={} ({})", if g.vertical { "x" } else { "y" }, g.at, g.marks.len())).collect();
    serde_json::json!({"enabled": enabled, "guides": guides}).to_string()
}

pub fn document_spatial(app: &mut WorkerApp, enabled: bool) -> String {
    let mut scene = app.world_mut().resource_mut::<crate::document_bridge::SpatialScene>();
    if scene.enabled != enabled { scene.enabled = enabled; }
    let (enabled, generation, nodes) = (scene.enabled, scene.generation, scene.len());
    let entities = app.world().resource::<crate::document_bridge::NodeMap>().len();
    let live = app.world().resource::<crate::document_bridge::Modes>().live.len();
    let hits = app.world().resource::<crate::PointerHits>();
    let hits = serde_json::json!({
        "overlay": hits.overlay.iter().map(|h| serde_json::json!({
            "chain": h.chain.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "background": h.background, "through_plane": h.through.is_some()
        })).collect::<Vec<_>>(),
        "embed": hits.embed, "embed_chain": hits.embed_chain.iter().map(ToString::to_string).collect::<Vec<_>>()
    });
    let result = serde_json::json!({"enabled": enabled, "generation": generation, "nodes": nodes, "entities": entities, "live": live, "hits": hits});
    if let Some(mut active) = app.world_mut().get_resource_mut::<ActivityControl>() { active.remaining_frames = 10; }
    result.to_string()
}

/// The runtime signal layer (vision 10 §4): registered producers, every
/// published signal with its producer and stamp, and conflicts.
pub fn document_signals(app: &mut WorkerApp) -> String {
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "document bridge not installed"}).to_string();
    };
    let gas = app.world().resource::<crate::document_bridge::gas::GasSim>();
    let producers: Vec<_> = [(iron_document::ProducerId::CLIPS, "clips".to_owned()), (iron_document::ProducerId::NETWORK, "network".to_owned())]
        .into_iter()
        .chain(store.0.producers().map(|(p, n)| (p, n.to_owned())))
        .map(|(p, name)| {
            let running = if name == crate::document_bridge::gas::PRODUCER { Some(gas.running) } else { None };
            serde_json::json!({"id": p.0, "name": name, "running": running, "held": store.0.signals().values().filter(|s| s.producer == p).count()})
        })
        .collect();
    let signals: Vec<_> = store
        .0
        .signals()
        .iter()
        .map(|(leaf, s)| serde_json::json!({"leaf": leaf.to_string(), "value": s.value, "producer": store.0.producer_name(s.producer), "stamp": s.stamp}))
        .collect();
    let errors: Vec<_> = store.0.signal_errors().iter().map(|(leaf, e)| serde_json::json!({"leaf": leaf.to_string(), "message": e})).collect();
    let result = serde_json::json!({"ok": true, "producers": producers, "signals": signals, "errors": errors, "gas": {"frames": gas.frames, "pressure": gas.pressure}});
    if let Some(mut active) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active.remaining_frames = 10;
    }
    result.to_string()
}

/// Control a host producer: `freeze` commits what it publishes as one
/// transaction and stops it, `stop` only stops it, `run` starts it again.
pub fn document_producer(app: &mut WorkerApp, name: String, action: String) -> String {
    use crate::document_bridge::gas;
    if name != gas::PRODUCER {
        return serde_json::json!({"ok": false, "error": format!("unknown producer {name:?}; only {:?} exists", gas::PRODUCER)}).to_string();
    }
    let now = app.world().resource::<Time>().elapsed().as_millis() as u64;
    let world = app.world_mut();
    let mut sim = world.resource_mut::<gas::GasSim>();
    let result = match action.as_str() {
        "run" => {
            sim.running = true;
            serde_json::json!({"ok": true, "running": true})
        }
        "stop" => {
            sim.running = false;
            serde_json::json!({"ok": true, "running": false})
        }
        "freeze" => {
            sim.running = false;
            let mut store = world.resource_mut::<crate::document_bridge::DocumentStore>();
            let producer = gas::producer_id(&mut store.0);
            match store.0.freeze(producer, format!("freeze {name}"), iron_document::Actor::Human, now) {
                None => serde_json::json!({"ok": true, "running": false, "frozen": 0}),
                Some(Ok(applied)) => serde_json::json!({"ok": true, "running": false, "frozen": 1, "version": applied.version.0}),
                Some(Err(errors)) => serde_json::json!({"ok": false, "errors": errors}),
            }
        }
        other => serde_json::json!({"ok": false, "error": format!("unknown action {other:?}; use run, stop or freeze")}),
    };
    if let Some(mut active) = world.get_resource_mut::<ActivityControl>() {
        active.remaining_frames = 10;
    }
    result.to_string()
}

/// Turn raster layers on or off (`vector::raster`); off draws every view
/// live. `budget_mb` sets the atlas budget, `0` the default. Returns the
/// raster report.
pub fn document_raster(app: &mut WorkerApp, enabled: bool, budget_mb: u32) -> String {
    crate::vector::raster::set_enabled(enabled, budget_mb as u64);
    if let Some(mut active) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active.remaining_frames = 10;
    }
    crate::vector::raster::report(false).to_string()
}

/// The frame-phase timings, with the raster cache's state and counters
/// under `raster` (`vector::raster::report`).
pub fn document_timings(_app: &mut WorkerApp, reset: bool) -> String {
    let mut out = crate::perf::report(reset);
    out["raster"] = crate::vector::raster::report(reset);
    out.to_string()
}

/// Make a component of the selected pictures (component-model.md §5.5):
/// `ids` is a JSON array of node ids. One transaction.
pub fn document_make_component(app: &mut WorkerApp, ids: String, name: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let ids: Vec<String> = serde_json::from_str(&ids).map_err(|e| e.to_string())?;
        let ids = ids.iter().map(|s| s.parse::<iron_document::NodeId>().map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let name = if name.trim().is_empty() { "component".to_owned() } else { name };
        let (made, mut input) = store.0.make_component(&ids, &name, iron_document::Actor::Human)?;
        input.ts = crate::host_services::unix_millis();
        let applied = store.0.apply(input).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        // What was made is selected, in place of what went into it.
        if let Some(mut sel) = app.world_mut().get_resource_mut::<crate::SelectionState>() {
            sel.selected = vec![made.picture];
        }
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "entity": made.entity.to_string(), "picture": made.picture.to_string(), "definition": made.definition.to_string()}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Component authoring from the UI (component-authoring.md): one entry for
/// its verbs, each one transaction. `args` is a JSON object:
///
/// - `add_port {main, x, y, dir?, follow?}`: a port on a main at (x, y) in
///   the main's space;
/// - `add_input {def, name, default?}`: a setting;
/// - `edit_port {spec, patch}`: `author::PortPatch`;
/// - `wrap {ids}`: wrap siblings in an empty;
/// - `connect {a, b}`: two ports, by a line;
/// - `add_empty {parent, x, y}`: an empty in `parent` (a main: its definition).
pub fn document_author(app: &mut WorkerApp, verb: String, args: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        use iron_document::author;
        let a: serde_json::Value = serde_json::from_str(&args).map_err(|e| e.to_string())?;
        let id = |k: &str| -> Result<iron_document::NodeId, String> { a.get(k).and_then(|v| v.as_str()).ok_or_else(|| format!("{k} missing"))?.parse().map_err(|e| format!("{k}: {e}")) };
        let num = |k: &str| a.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let picked_point = app.world().get_resource::<crate::bevy_app::select::PathEdit>().and_then(|e| e.lone_point()).map(|p| p.to_string());
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let doc = store.0.document();
        if verb == "attachment_info" {
            let target = id("target")?;
            let main = target.path()[0];
            let template = iron_document::expand::main_template(doc, main).ok_or("Select a main component or one of its parts")?;
            let name = |n| doc.get::<iron_document::facet::Name>(n).and_then(|v| v.text.constant()).and_then(|v| v.as_str()).unwrap_or("").to_owned();
            let shown = |n| iron_document::NodeId::expanded(main, n).unwrap().to_string();
            let controls: Vec<_> = doc.descendants(template).into_iter().filter(|&n| !n.is_expanded() && doc.live(n).is_some_and(|v| v.ty.name() == "empty")).map(|n| serde_json::json!({"id": shown(n), "name": name(n), "port": doc.get::<iron_document::facet::Anchor>(n).is_some()})).collect();
            let paths: Vec<_> = doc.descendants(template).into_iter().filter(|&n| !n.is_expanded() && doc.live(n).is_some_and(|v| v.ty.name() == "path")).map(|n| {
                let points: Vec<_> = iron_document::contour::contours(&store.0, n).into_iter().flat_map(|c| c.points).enumerate().map(|(i, (p, _))| {
                    let attachment = doc.get::<iron_document::facet::Attachment>(p).map(|v| serde_json::json!({"control": v.control.constant().and_then(|v| v.as_ref()).map(shown), "dx": store.0.resolved(p, "attachment.dx".parse().unwrap()).and_then(|v| v.as_f64()), "dy": store.0.resolved(p, "attachment.dy".parse().unwrap()).and_then(|v| v.as_f64())}));
                    serde_json::json!({"id": shown(p), "index": i + 1, "attachment": attachment})
                }).collect();
                serde_json::json!({"id": shown(n), "name": name(n), "packed": iron_document::contour::packed(doc, n).is_some(), "points": points})
            }).collect();
            return Ok(serde_json::json!({"ok": true, "main": main.to_string(), "name": name(main), "controls": controls, "paths": paths, "picked_point": picked_point}));
        }
        // Pin a state on its main (session only, machines.md §6): no
        // transaction.
        // `scrub: {from, to, progress}` shows a moment of a transition on
        // the main instead (the machine panel's transition playhead).
        // The slices the timeline's selected bars and diamonds stand for
        // (machines.md §6): the editor's easing and stagger act on them.
        if verb == "selected_slices" {
            let nodes: Vec<iron_document::NodeId> = a.get("nodes").and_then(|v| v.as_array()).map(|xs| xs.iter().filter_map(|x| x.as_str()?.parse().ok()).collect()).unwrap_or_default();
            let mut out: Vec<serde_json::Value> = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for n in nodes {
                // A slice's bar or diamond stands for its tween; a part's bar
                // for all of the part's (its `with`).
                for w in iron_document::timeline_view::projected_tweens(doc, n) {
                    if seen.insert(w) {
                        let t = doc.get::<iron_document::facet::Tween>(w);
                        out.push(serde_json::json!({"tween": w.to_string(), "channel": t.and_then(|t| t.channel.constant()).and_then(|v| v.as_str())}));
                    }
                }
            }
            return Ok(serde_json::json!({"ok": true, "slices": out}));
        }
        if verb == "pin_state" {
            let scrub = a.get("scrub").filter(|v| v.is_object()).map(|v| {
                let t = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
                (t("from"), t("to"), v.get("progress").and_then(|x| x.as_f64()).unwrap_or(0.0))
            });
            let pin = match a.get("state").and_then(|v| v.as_str()) {
                Some(state) => Some(iron_document::machine::Pinned { def: id("def")?, machine: id("machine")?, state: state.to_owned(), scrub }),
                None => None,
            };
            store.0.pin_state(pin);
            return Ok(serde_json::json!({"ok": true}));
        }
        if verb == "add_state" {
            let def = id("def")?;
            let word = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::to_owned);
            let (ops, name) = iron_document::machine::add_named_state_ops(doc, def, word("first").as_deref(), word("name").as_deref())?;
            let label = format!("add state {name}");
            store.0.apply(iron_document::TransactionInput { label, actor: iron_document::Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false }).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
            // The new state is pinned, so the next edits shape it.
            let doc = store.0.document();
            if let Some(&(machine, _)) = iron_document::machine::machines(doc, def).first() {
                store.0.pin_state(Some(iron_document::machine::Pinned::state(def, machine, name.clone())));
            }
            return Ok(serde_json::json!({"ok": true, "state": name}));
        }
        // A pin that a rename moves, set once the rename has applied.
        let mut repin: Option<iron_document::machine::Pinned> = None;
        let (label, ops, made): (String, Vec<iron_document::Op>, Option<iron_document::NodeId>) = match verb.as_str() {
            "rename_state" => {
                let state = id("state")?;
                let ops = iron_document::machine::rename_state_ops(doc, state, a.get("name").and_then(|v| v.as_str()).unwrap_or(""))?;
                // A pin on the old name follows the rename.
                let old = doc.get::<iron_document::facet::Name>(state).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
                let new = ops.iter().find_map(|o| match o {
                    iron_document::Op::Set { id, slot, .. } if *id == state => slot.constant().and_then(|v| v.as_str()).map(str::to_owned),
                    _ => None,
                });
                // A pin on the old name (of this machine) follows the rename,
                // the scrubbed transition's ends too.
                let machine = doc.live(state).and_then(|n| n.parent);
                if let (Some(old), Some(new), Some(mut pin)) = (old, new, store.0.pinned_state().cloned())
                    && Some(pin.machine) == machine
                {
                    let swap = |s: &mut String| if *s == old { *s = new.clone() };
                    swap(&mut pin.state);
                    if let Some((f, t, _)) = &mut pin.scrub {
                        swap(f);
                        swap(t);
                    }
                    repin = Some(pin);
                }
                ("rename state".into(), ops, None)
            }
            // The machine panel (machines.md §6): one pair of states its own
            // timing; a channel its own slice of a transition, or not.
            "add_transition" => {
                let read = |l: iron_document::Leaf| store.0.resolved(l.node, l.path);
                let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
                let (ops, t) = iron_document::machine::add_transition_ops(doc, &read, id("def")?, id("machine")?, &s("from"), &s("to"))?;
                ("add transition".into(), ops, Some(t))
            }
            // The state editor (machines.md §6): a pair's timing, edited as
            // one step (its own transition made first when it used the
            // default), or copied from another, mirrored for a reverse leg.
            "edit_pair" | "copy_timing" => {
                use iron_document::machine;
                let read = |l: iron_document::Leaf| store.0.resolved(l.node, l.path);
                let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
                let edit = if verb == "copy_timing" {
                    machine::copied_timing(doc, &read, id("source")?, a.get("mirrored").and_then(|v| v.as_bool()).unwrap_or(false))?
                } else {
                    let f = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_f64());
                    let t = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
                    machine::PairEdit {
                        ms: f(&a, "ms"),
                        ease: t(&a, "ease"),
                        name: t(&a, "name"),
                        slices: a.get("slices").and_then(|v| v.as_array()).map(|xs| xs.iter().map(|x| machine::Slice { channel: t(x, "channel").unwrap_or_default(), start: f(x, "start"), end: f(x, "end"), ease: t(x, "ease") }).collect()).unwrap_or_default(),
                        follow: a.get("follow").and_then(|v| v.as_array()).map(|xs| xs.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
                        only: false,
                    }
                };
                let (ops, t) = machine::edit_pair_ops(doc, &read, id("def")?, id("machine")?, &s("from"), &s("to"), &edit)?;
                (a.get("label").and_then(|v| v.as_str()).map_or_else(|| verb.replace('_', " "), str::to_owned), ops, Some(t))
            }
            "copy_pose" => ("copy look".into(), iron_document::machine::copy_pose_ops(doc, id("state")?, id("from")?)?, None),
            "reset_value" => ("reset to the drawing".into(), iron_document::machine::reset_value_ops(doc, id("state")?, a.get("channel").and_then(|v| v.as_str()).unwrap_or(""))?, None),
            "delete_state" => ("delete state".into(), iron_document::machine::delete_state_ops(doc, id("state")?)?, None),
            "set_default_state" => ("set default state".into(), iron_document::machine::set_default_state_ops(doc, id("state")?)?, None),
            "add_tween" => {
                let (ops, w) = iron_document::machine::add_tween_ops(doc, id("transition")?, a.get("channel").and_then(|v| v.as_str()).unwrap_or(""))?;
                ("add tween".into(), ops, Some(w))
            }
            "remove_node" => {
                let n = id("node")?;
                if !matches!(doc.live(n).map(|x| x.ty.name()), Some("tween" | "transition")) {
                    return Err("remove_node takes a tween or a transition".into());
                }
                (format!("remove {}", doc.live(n).map_or("", |x| x.ty.name())), vec![iron_document::Op::Delete { id: n }], None)
            }
            "attach_points" | "detach_points" => {
                let points: Vec<iron_document::NodeId> = serde_json::from_value(a.get("points").cloned().ok_or("points missing")?).map_err(|e| e.to_string())?;
                let ops = if verb == "attach_points" { iron_document::attachment::attach(&store.0, id("control")?, &points)? }
                    else { iron_document::attachment::detach(&store.0, &points)? };
                (verb.replace('_', " "), ops, None)
            }
            "expose_control" => ("expose control as port".into(), author::expose_control(doc, id("control")?, a.get("dir").and_then(|v| v.as_str()).unwrap_or("in"), true)?, None),
            "expand_path_points" => {
                let path = iron_document::attachment::authored(doc, id("path")?)?;
                ("expand path points".into(), iron_document::contour::expand_points(&store.0, path), None)
            }
            "add_port" => {
                let main = id("main")?;
                let dir = a.get("dir").and_then(|v| v.as_str()).unwrap_or("in");
                let follow = a.get("follow").and_then(|v| v.as_bool()).unwrap_or(false);
                let (ops, _, anchor) = author::add_port(doc, main, [num("x"), num("y")], dir, follow)?;
                ("add port".into(), ops, iron_document::NodeId::expanded(main, anchor))
            }
            "add_input" => {
                let def = id("def")?;
                let default = match a.get("default").and_then(|v| v.as_str()) {
                    Some(t) => t.trim().parse::<f64>().map(iron_document::Value::Number).or_else(|_| iron_document::Value::parse_quantity(t)).map_err(|_| format!("{t:?} is not a number or quantity"))?,
                    None => iron_document::Value::Number(0.0),
                };
                let (ops, spec) = author::add_input(doc, def, a.get("name").and_then(|v| v.as_str()).unwrap_or("input"), default)?;
                ("add input".into(), ops, Some(spec))
            }
            "edit_port" => {
                let patch: author::PortPatch = serde_json::from_value(a.get("patch").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
                ("edit port".into(), author::edit_port(doc, id("spec")?, &patch)?, None)
            }
            "wrap" => {
                let ids: Vec<iron_document::NodeId> = a.get("ids").and_then(|v| v.as_array()).ok_or("ids missing")?.iter().filter_map(|v| v.as_str()?.parse().ok()).collect();
                let extent = |n: iron_document::NodeId| store.0.extent(n);
                let (ops, empty) = author::wrap_in_empty(store.0.document(), &extent, &ids)?;
                // Inside a main the empty lands in the definition: select its picture there.
                let parent = ids.first().and_then(|&n| store.0.document().live(n)).and_then(|n| n.parent);
                let shown = parent.and_then(|p| p.path().first().copied()).filter(|&m| iron_document::expand::main_definition(store.0.document(), m).is_some()).and_then(|m| iron_document::NodeId::expanded(m, empty));
                ("wrap in empty".into(), ops, Some(shown.unwrap_or(empty)))
            }
            "resizable" => {
                let def = id("def")?;
                let on = a.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
                let extent = |n: iron_document::NodeId| store.0.extent(n);
                (if on { "make resizable" } else { "make fixed size" }.into(), author::resizable(store.0.document(), def, on, &extent)?, None)
            }
            "add_token" => {
                // A new colour token (palette.rs) in the document's palette,
                // made if there is none: light and dark as hex.
                let name: String = a.get("name").and_then(|v| v.as_str()).map(|n| n.trim().trim_start_matches('@').to_owned()).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).ok_or("a token's name is letters, digits, - and _ (series-9)")?;
                if iron_document::palette::tokens(doc).contains_key(&name) {
                    return Err(format!("@{name} already exists"));
                }
                let colour = |k: &str| a.get(k).and_then(|v| v.as_str()).map(|h| iron_document::palette::hex(h)).ok_or(format!("{k} missing"));
                (format!("add token @{name}"), iron_document::palette::add_ops(doc, &name, colour("light")?, colour("dark")?), None)
            }
            "rename" => {
                let node = id("node")?;
                let name = a.get("name").and_then(|v| v.as_str()).map(str::trim).filter(|n| !n.is_empty()).ok_or("a name is needed")?;
                ("rename".into(), vec![iron_document::Op::Set { id: node, path: "name.text".parse().expect("declared leaf"), slot: iron_document::Slot::Const(iron_document::Value::Str(name.to_owned())) }], None)
            }
            "connect" => {
                let read = |l: iron_document::Leaf| store.0.resolved(l.node, l.path);
                ("connect".into(), author::connect(store.0.document(), &read, id("a")?, id("b")?)?, None)
            }
            "add_empty" => {
                let parent = id("parent")?;
                let e = doc.next_id();
                let mut t = iron_document::facet::Transform2d::default();
                t.x = iron_document::Slot::Const(iron_document::Value::Number(num("x")));
                t.y = iron_document::Slot::Const(iron_document::Value::Number(num("y")));
                let mut name = iron_document::facet::Name::default();
                name.text = iron_document::Slot::Const(iron_document::Value::Str("empty".into()));
                use iron_document::LeafStruct;
                let order = doc.children(parent).iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(iron_document::OrderKey::first, |k| iron_document::OrderKey::after(&k));
                let shown = if iron_document::expand::main_definition(doc, parent).is_some() { iron_document::NodeId::expanded(parent, e) } else { None };
                ("add empty".into(), vec![iron_document::Op::Create { id: e, ty: iron_document::TypeId::lookup("empty").expect("registered"), parent: Some(parent), order, facets: vec![t.wrap(), name.wrap()] }], Some(shown.unwrap_or(e)))
            }
            "new_composition" => {
                // A root composition (ECM §5), made from the "+" menu's New
                // composition: a name no other composition has, 2D or 3D, and
                // its notation (free or schematic).
                let name = a.get("name").and_then(|v| v.as_str()).map(str::trim).filter(|n| !n.is_empty()).ok_or("a name is needed")?;
                let name_of = |c: iron_document::NodeId| doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
                if iron_document::composition::compositions(doc).into_iter().any(|c| name_of(c).as_deref() == Some(name)) {
                    return Err(format!("a composition named {name:?} already exists"));
                }
                let dims = a.get("dims").and_then(|v| v.as_str()).unwrap_or("2d");
                if !matches!(dims, "2d" | "3d") {
                    return Err(format!("dims {dims:?}: 2d or 3d"));
                }
                let notation = a.get("notation").and_then(|v| v.as_str()).unwrap_or("free");
                if !matches!(notation, "free" | "schematic") || (notation == "schematic" && dims == "3d") {
                    return Err(format!("notation {notation:?}: free, or schematic in 2D"));
                }
                use iron_document::LeafStruct;
                let mut c = iron_document::facet::Composition::default();
                c.dims = iron_document::Slot::Const(iron_document::Value::Enum(dims.into()));
                c.notation = iron_document::Slot::Const(iron_document::Value::Str(notation.into()));
                let mut n = iron_document::facet::Name::default();
                n.text = iron_document::Slot::Const(iron_document::Value::Str(name.into()));
                let id = doc.next_id();
                let order = doc.roots().iter().filter(|r| !r.is_session()).filter_map(|&r| doc.live(r)).map(|r| r.order.clone()).max().map_or_else(iron_document::OrderKey::first, |k| iron_document::OrderKey::after(&k));
                ("new composition".into(), vec![iron_document::Op::Create { id, ty: iron_document::TypeId::lookup("composition").expect("registered"), parent: None, order, facets: vec![n.wrap(), c.wrap()] }], Some(id))
            }
            "delete_composition" => {
                // The composition and everything in it, as one undoable entry.
                let node = id("node")?;
                if node.is_session() || !iron_document::composition::compositions(doc).contains(&node) {
                    return Err(format!("{node} is not a composition of the document"));
                }
                ("delete composition".into(), vec![iron_document::Op::Delete { id: node }], None)
            }
            other => return Err(format!("no authoring verb {other:?}")),
        };
        let applied = store.0
            .apply(iron_document::TransactionInput { label, actor: iron_document::Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        if repin.is_some() {
            store.0.pin_state(repin);
        }
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "made": made.map(|m| m.to_string())}))
    };
    let result = run();
    if verb != "attachment_info" {
        if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Make or rewrite a table (layout-and-tables.md §5.2): `spec` is JSON,
/// `{parent, x, y, rows, preset?, header?, label_col?, emphasis?, align?,
/// caption?, actor?}` to make one, where `rows` is a list of rows of cells
/// or text with one row per line and cells split by `|`, and `emphasis` is
/// 0-based. With `id`, the table's words are replaced (`table::replace_ops`),
/// and a `header` or a different `preset` is applied in the same history
/// entry. A `caption` puts the caption and the table in a vertical stack.
/// Returns the table's id.
pub fn document_table(app: &mut WorkerApp, spec: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        use iron_document::{table, Actor, LeafStruct, NodeId, Op, OrderKey, Slot, TransactionInput, TypeId, Value};
        let v: serde_json::Value = serde_json::from_str(&spec).map_err(|e| e.to_string())?;
        let id_of = |k: &str| v.get(k).and_then(|p| p.as_str()).map(|p| p.parse::<NodeId>().map_err(|e| format!("{k}: {e}"))).transpose();
        let rows: Vec<Vec<String>> = match v.get("rows") {
            Some(serde_json::Value::String(text)) => table::parse_rows(text),
            Some(serde_json::Value::Array(rows)) => rows.iter().map(|r| r.as_array().map(|r| r.iter().map(|c| c.as_str().map(str::to_owned).unwrap_or_else(|| c.to_string())).collect()).unwrap_or_default()).collect(),
            _ => return Err("rows: a list of rows, or text with one row per line".into()),
        };
        let actor = if v.get("actor").and_then(|a| a.as_str()) == Some("model") { Actor::Model } else { Actor::Human };
        let tx = |label: &str, ops| TransactionInput { label: label.into(), actor, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false };
        let err = |es: Vec<iron_document::op::ApplyError>| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        if let Some(t) = id_of("id")? {
            let mut ops = table::replace_ops(&store.0, t, &rows)?;
            if let Some(h) = v.get("header").and_then(|h| h.as_u64()) {
                ops.push(Op::Set { id: t, path: "table.header".parse().expect("declared leaf"), slot: Slot::Const(Value::Number(h as f64)) });
            }
            let first = store.0.apply(tx("replace table", ops)).map_err(err)?.version;
            let mut version = first;
            let now = store.0.resolved(t, "table.preset".parse().expect("declared leaf")).and_then(|p| p.as_str().map(str::to_owned));
            if let Some(p) = v.get("preset").and_then(|p| p.as_str()).filter(|p| Some(*p) != now.as_deref()) {
                let (ops, _) = table::edit_ops(&store.0, t, &format!("preset:{p}"), &[])?;
                version = store.0.apply(tx("replace table", ops)).map_err(err)?.version;
                store.0.merge_with_previous(first, "replace table");
            }
            return Ok(serde_json::json!({"ok": true, "version": version.0, "table": t.to_string()}));
        }
        let parent = id_of("parent")?;
        let spec = table::Spec {
            rows,
            preset: table::Preset::parse(v.get("preset").and_then(|p| p.as_str()).unwrap_or("light")),
            header: v.get("header").and_then(|h| h.as_u64()).unwrap_or(1) as usize,
            label_col: v.get("label_col").and_then(|b| b.as_bool()).unwrap_or(false),
            emphasis: v.get("emphasis").and_then(|e| e.as_u64()).map(|e| e as usize),
            align: v.get("align").and_then(|a| a.as_str()).map(|a| a.split_whitespace().map(str::to_owned).collect()).unwrap_or_default(),
        };
        let at = [v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0), v.get("y").and_then(|y| y.as_f64()).unwrap_or(0.0)];
        let doc = store.0.document();
        // Last among its siblings: drawn on top.
        let siblings = match parent {
            Some(p) => doc.children(p).to_vec(),
            None => doc.roots().to_vec(),
        };
        let order = siblings.iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |k| OrderKey::after(&k));
        let caption = v.get("caption").and_then(|c| c.as_str()).filter(|c| !c.trim().is_empty());
        let (ops, made) = match caption {
            None => table::create_ops(doc, parent, order, at, &spec)?,
            Some(words) => {
                // A stack: the caption above the table (layout-and-tables.md
                // §3.3), small, spaced capitals in the muted ink.
                let ids = doc.next_ids(2);
                let mut stack = iron_document::facet::Layout::default();
                stack.mode = Slot::Const(Value::Enum("stack_y".into()));
                stack.gap = Slot::Const(Value::Number(10.0));
                let mut place = iron_document::facet::Transform2d::default();
                place.x = Slot::Const(Value::Number(at[0]));
                place.y = Slot::Const(Value::Number(at[1]));
                let mut name = iron_document::facet::Name::default();
                name.text = Slot::Const(Value::Str("captioned table".into()));
                let mut text = iron_document::facet::Text::default();
                text.content = Slot::Const(Value::Str(words.to_uppercase()));
                let mut font = iron_document::facet::Font::default();
                font.size = Slot::Const(Value::Number(12.0));
                font.weight = Slot::Const(Value::Number(500.0));
                font.letter_spacing = Slot::Const(Value::Number(12.0));
                let mut fill = iron_document::facet::Fill::default();
                fill.color = Slot::Const(Value::Color([0.45, 0.48, 0.55, 1.0]));
                let mut ops = vec![
                    Op::Create { id: ids[0], ty: TypeId::lookup("group").expect("registered"), parent, order, facets: vec![place.wrap(), stack.wrap(), name.wrap()] },
                    Op::Create { id: ids[1], ty: TypeId::lookup("text").expect("registered"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![iron_document::facet::Transform2d::default().wrap(), text.wrap(), font.wrap(), fill.wrap()] },
                ];
                // The table's ids follow the two just taken.
                let (more, t) = table::create_ops_after(doc, 2, Some(ids[0]), OrderKey::after(&OrderKey::first()), [0.0, 0.0], &spec)?;
                ops.extend(more);
                (ops, t)
            }
        };
        let applied = store.0.apply(tx("add table", ops)).map_err(err)?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "table": made.to_string()}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Edit a table's structure (layout-and-tables.md §5.1): `verb` is one of
/// `table::edit_ops`'s, `cells` a JSON list of the selected cells (for merge
/// and split). One transaction.
pub fn document_table_edit(app: &mut WorkerApp, table: String, verb: String, cells: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let table = table.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
        let cells: Vec<String> = serde_json::from_str(&cells).map_err(|e| e.to_string())?;
        let cells = cells.iter().map(|s| s.parse::<iron_document::NodeId>().map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let (ops, label) = iron_document::table::edit_ops(&store.0, table, &verb, &cells)?;
        let applied = store.0
            .apply(iron_document::TransactionInput { label, actor: iron_document::Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Write one context into the document (plans/specs/contexts.md §2.2; the
/// model's `<context name="…">` verb): its markup replaces a document
/// context of the same name, or a new `context` node holds it. One
/// transaction.
pub fn document_context(app: &mut WorkerApp, markup: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let (ops, node) = iron_document::contexts::put_ops(store.0.document(), &markup)?;
        let applied = store.0
            .apply(iron_document::TransactionInput { label: "context".into(), actor: iron_document::Actor::Model, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "context": node.to_string()}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Place and wire parts from a model's `<circuit>` (schematic.md §5), as
/// one history entry (`parts::circuit_ops`).
pub fn document_circuit(app: &mut WorkerApp, markup: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let (ops, summary) = iron_document::parts::circuit_ops(&store.0, &markup)?;
        let applied = store.0
            .apply(iron_document::TransactionInput { label: "circuit".into(), actor: iron_document::Actor::Model, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "summary": summary}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// The model's component verbs (`compose::compose_after`): `<place>` and
/// `<make-component>`, after the edit's other ops (`ops_json`, a JSON array,
/// often empty), as one history entry. The summary names what each verb
/// made. A failing op answers `errors` indexed into `ops_json`, as `apply`
/// does; a failing verb answers `error`.
pub fn document_compose(app: &mut WorkerApp, markup: String, ops_json: String) -> String {
    use iron_document::compose::ComposeError;
    let mut run = || -> Result<serde_json::Value, serde_json::Value> {
        let verb = |error: String| serde_json::json!({"ok": false, "error": error});
        let before: Vec<iron_document::Op> = serde_json::from_str(&ops_json)
            .map_err(|e| serde_json::json!({"ok": false, "errors": [{"op": null, "kind": "parse", "message": e.to_string()}]}))?;
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or_else(|| verb("document bridge not installed".into()))?;
        let current = store.0.document().version().0;
        let (ops, summary) = iron_document::compose::compose_after(&store.0, before, &markup).map_err(|e| match e {
            ComposeError::Ops(errors) => serde_json::json!({"ok": false, "version": current, "errors": errors}),
            ComposeError::Verb(message) => verb(message),
        })?;
        let applied = store.0
            .apply(iron_document::TransactionInput { label: "model: components".into(), actor: iron_document::Actor::Model, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| verb(es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "summary": summary}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| error).to_string()
}

/// A sheet's netlist, SPICE-like, with each net's solved voltage
/// (schematic.md §5; `parts::netlist`). `sheet` is a composition's id or
/// name.
pub fn document_netlist(app: &mut WorkerApp, sheet: String) -> String {
    let run = || -> Result<serde_json::Value, String> {
        let store = app.world().get_resource::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let doc = store.0.document();
        let id = match sheet.parse::<iron_document::NodeId>() {
            Ok(id) if sheet.starts_with('#') => id,
            _ => doc
                .nodes_of_type(iron_document::TypeId::lookup("composition").expect("registered"))
                .find(|&c| doc.is_live(c) && doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(sheet.as_str()))
                .ok_or_else(|| format!("no composition named {sheet}"))?,
        };
        Ok(serde_json::json!({"ok": true, "netlist": iron_document::parts::netlist(&store.0, id)}))
    };
    run().unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// What the contexts resolve for a node, each value with the rule that set
/// it (contexts.md §2.6), through the node's own containers.
pub fn document_computed(app: &mut WorkerApp, node: String) -> String {
    let run = || -> Result<serde_json::Value, String> {
        let id = node.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
        let store = app.world().get_resource::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let doc = store.0.document();
        if !doc.is_live(id) {
            return Err(format!("{id} is not a live node"));
        }
        let chain = iron_document::contexts::chain_of(doc, id);
        let r = iron_document::contexts::Resolver::new(doc, store.0.contexts());
        let i = chain.len() - 1;
        let values: Vec<serde_json::Value> = r.computed(&chain, i).into_iter().map(|(f, v, by)| serde_json::json!({"field": f, "value": v, "by": by})).collect();
        Ok(serde_json::json!({"ok": true, "values": values, "text": r.computed_markup(&chain, i)}))
    };
    run().unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// What a view offers and why (contexts.md §2.7): its focus (world, open
/// scope, states) and each view field's value with the rule that set it.
pub fn document_view_context(app: &mut WorkerApp, panel: String) -> String {
    let run = || -> Result<serde_json::Value, String> {
        let store = app.world().get_resource::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let focus = app.world().get_resource::<crate::bevy_app::view_focus::ViewFocus>().ok_or("no view focus")?;
        let (_, f, offer) = focus.views.iter().find(|(p, ..)| *p == panel).ok_or_else(|| format!("no view {panel}"))?;
        let r = iron_document::contexts::Resolver::new(store.0.document(), store.0.contexts());
        Ok(serde_json::json!({
            "ok": true,
            "world": f.world.map(|w| w.to_string()),
            "scope": f.scope.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "states": f.states,
            "palette": offer.palette,
            "keys": offer.keys,
            "text": r.computed_view_markup(&panel, f),
        }))
    };
    run().unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Fill the open document with the demo scene (`document_bridge::
/// demo_scene`), one transaction: what a page showing the demo (`?demo`, a
/// benchmark, the native build) opens with. The engine starts empty; a
/// stored scope is never preceded by the demo. A document that already
/// holds anything is left alone (`already`): the native engine keeps its
/// document across webview reloads, and the demo never lands on top of one.
pub fn document_demo(app: &mut WorkerApp) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        if store.0.document().live_nodes().any(|n| !n.id.is_session()) {
            return Ok(serde_json::json!({"ok": true, "already": true}));
        }
        let tx = crate::document_bridge::demo_scene(&store.0);
        let applied = store.0.apply(tx).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// Add the contexts showcase (`document_bridge::demo::showcase`): the
/// outline, pitch and dials compositions, one transaction.
pub fn document_showcase(app: &mut WorkerApp) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let doc = store.0.document();
        if iron_document::composition::compositions(doc).iter().any(|&c| doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned))).as_deref() == Some("dials")) {
            return Ok(serde_json::json!({"ok": true, "already": true}));
        }
        let tx = crate::document_bridge::showcase(&store.0)?;
        let applied = store.0.apply(tx).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// The built-in contexts (`std:*`) in their markup: how things behave
/// before a document says otherwise.
pub fn document_contexts(_app: &mut WorkerApp) -> String {
    serde_json::json!({"ok": true, "markup": iron_document::contexts::Library::builtin().print()}).to_string()
}

/// Detach a linked instance (component-model.md §5.2): what it derives
/// becomes authored, one transaction.
pub fn document_detach(app: &mut WorkerApp, entity: String) -> String {
    let mut run = || -> Result<serde_json::Value, String> {
        let id = entity.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let ops = iron_document::definition::detach_ops(store.0.document(), id)?;
        let applied = store.0
            .apply(iron_document::TransactionInput { label: "detach instance".into(), actor: iron_document::Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

pub fn document_link(app: &mut WorkerApp, a: String, b: String, keep: String, actor: String) -> String {
    use iron_document::composition::{self, LinkError};
    use iron_document::NodeId;
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

pub fn document_counterparts(app: &mut WorkerApp, node: String) -> String {
    use iron_document::composition;
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
                .and_then(|c| doc.get::<iron_document::facet::Name>(c))
                .and_then(|n| n.text.constant())
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            serde_json::json!({"node": f.to_string(), "composition": c.map(|c| c.to_string()), "name": name})
        })
        .collect();
    serde_json::json!({"entity": entity.to_string(), "figures": figures}).to_string()
}

pub fn document_to_path(app: &mut WorkerApp, node: String, force: bool) -> String {
    let Ok(id) = node.parse::<iron_document::NodeId>() else {
        return serde_json::json!({"ok": false, "error": format!("bad id {node:?}")}).to_string();
    };
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "no document"}).to_string();
    };
    let ty = store.0.document().live(id).map(|n| n.ty.name()).unwrap_or("");
    let conversion = match iron_document::shape::to_path(&store.0, id) {
        Ok(c) => c,
        Err(e) => return serde_json::json!({"ok": false, "error": e}).to_string(),
    };
    if !force && !conversion.cuts.is_empty() {
        return serde_json::json!({"ok": false, "cuts": conversion.cuts}).to_string();
    }
    let label = format!("convert {ty} to path");
    if let Some(mut pending) = app.world_mut().get_resource_mut::<crate::document_bridge::PendingTransactions>() {
        pending.0.push(iron_document::TransactionInput { label, actor: iron_document::Actor::Human, ts: 0, idempotency_key: None, ops: conversion.ops, exact: false });
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    serde_json::json!({"ok": true, "cuts": conversion.cuts}).to_string()
}

pub fn document_export_svg(app: &mut WorkerApp, scope: String) -> String {
    use iron_document::composition;
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "no document"}).to_string();
    };
    let doc = store.0.document();
    let name_of = |c: iron_document::NodeId| doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
    let scope = if scope == "selection" {
        iron_svg::Scope::Nodes(store.0.selection().into_iter().map(|(id, _)| id).collect())
    } else if scope.starts_with('#')
        && let Ok(id) = scope.parse::<iron_document::NodeId>()
    {
        match doc.live(id).map(|n| n.ty.name()) {
            Some("composition") => iron_svg::Scope::Composition(id),
            Some(_) => iron_svg::Scope::Nodes(vec![id]),
            None => return serde_json::json!({"ok": false, "error": format!("no node {id}")}).to_string(),
        }
    } else {
        match composition::compositions(doc).into_iter().find(|&c| name_of(c).is_some_and(|n| n.eq_ignore_ascii_case(&scope))) {
            Some(c) => iron_svg::Scope::Composition(c),
            None => return serde_json::json!({"ok": false, "error": format!("no composition named {scope:?}")}).to_string(),
        }
    };
    match iron_svg::export(&store.0, &scope) {
        Ok(out) => serde_json::json!({"ok": true, "svg": out.svg, "cuts": out.cuts}).to_string(),
        Err(e) => serde_json::json!({"ok": false, "error": e}).to_string(),
    }
}

pub fn document_import_svg(app: &mut WorkerApp, text: String) -> String {
    import_svg(app, text, true)
}

/// [`document_import_svg`] with every repeated piece drawn out in full, no
/// references: what the references are checked against.
pub fn document_import_svg_flat(app: &mut WorkerApp, text: String) -> String {
    import_svg(app, text, false)
}

fn import_svg(app: &mut WorkerApp, text: String, share: bool) -> String {
    let imported = match iron_svg::import_with(&text, share) {
        Ok(i) => i,
        Err(e) => return serde_json::json!({"ok": false, "error": e}).to_string(),
    };
    for cut in &imported.cuts {
        info!("import SVG: {cut}");
    }
    let Some(mut clip) = app.world_mut().get_resource_mut::<crate::bevy_app::select::Clipboard>() else {
        return serde_json::json!({"ok": false, "error": "no select tool"}).to_string();
    };
    clip.imports.push(imported.fragment);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    serde_json::json!({"ok": true, "cuts": imported.cuts, "nodes": imported.nodes, "anchors": imported.anchors, "definitions": imported.definitions, "references": imported.references}).to_string()
}

pub fn document_import_svg_at(app: &mut WorkerApp, text: String, parent: String, x: f64, y: f64, actor: String) -> String {
    let Ok(parent) = parent.parse::<iron_document::NodeId>() else {
        return serde_json::json!({"ok": false, "error": format!("bad id {parent:?}")}).to_string();
    };
    let imported = match iron_svg::import(&text) {
        Ok(i) => i,
        Err(e) => return serde_json::json!({"ok": false, "error": e}).to_string(),
    };
    let actor = if actor == "model" { iron_document::Actor::Model } else { iron_document::Actor::Human };
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else {
        return serde_json::json!({"ok": false, "error": "no document"}).to_string();
    };
    if !store.0.document().is_live(parent) {
        return serde_json::json!({"ok": false, "error": format!("no node {parent}")}).to_string();
    }
    let place = iron_document::copy::Placement { parent: Some(parent), beside: false, offset: [x, y], share_entities: false };
    let result = match iron_document::copy::paste(&store.0, &imported.fragment, &place, "import") {
        Err(e) => serde_json::json!({"ok": false, "error": e}),
        Ok(pasted) => match store.0.apply(iron_document::TransactionInput { label: "import SVG".into(), actor, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false }) {
            Ok(applied) => serde_json::json!({
                "ok": true,
                "version": applied.version.0,
                "roots": pasted.roots.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
                "cuts": imported.cuts,
            }),
            Err(errors) => serde_json::json!({"ok": false, "error": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")}),
        },
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    result.to_string()
}

pub fn document_focus(app: &mut WorkerApp, node: String) -> String {
    use iron_document::composition;
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

pub fn document_selection(app: &mut WorkerApp) -> String {
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return "[]".into();
    };
    let doc = store.0.document();
    let list: Vec<serde_json::Value> = store
        .0
        .selection()
        .into_iter()
        .map(|(id, data)| {
            let name = doc.get::<iron_document::facet::Name>(id).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
            let s = |k: &str| data.get(k).and_then(|v| v.as_str()).map(str::to_owned);
            serde_json::json!({
                "node": id.to_string(),
                "type": doc.live(id).map(|n| n.ty.name()),
                "name": name,
                "panel": s("panel"),
                "chain": s("chain"),
            })
        })
        .collect();
    serde_json::Value::Array(list).to_string()
}

pub fn document_inspect(app: &mut WorkerApp, node: String) -> String {
    use iron_document::Handle;
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return "null".into();
    };
    let Ok(id) = node.parse::<iron_document::NodeId>() else { return "null".into() };
    let doc = store.0.document();
    // A timeline's key diamond or clip bar (session glyphs) inspects the key
    // or clip it projects: a selected key shows its time, value and easing.
    let id = if id.is_session() {
        doc.relations_from(id)
            .find(|r| r.rel == iron_document::RelKind::Projects && doc.live(r.to).is_some_and(|t| matches!(t.ty.name(), "key" | "clip")))
            .map_or(id, |r| r.to)
    } else {
        id
    };
    let Some(n) = doc.live(id) else { return "null".into() };
    let name = doc.get::<iron_document::facet::Name>(id).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
    let components: Vec<serde_json::Value> = doc
        .facets_of(id)
        .filter(|c| c.kind().name() != "name")
        .map(|c| {
            let kind = c.kind();
            let fields: Vec<serde_json::Value> = c
                .field_names()
                .into_iter()
                .filter_map(|field| {
                    let path = iron_document::LeafPath::new(kind, field).ok()?;
                    let slot = c.get(field)?;
                    let value = store.0.resolved(id, path);
                    let (edit, via, why) = match store.0.handle(id, path) {
                        Handle::Direct => ("direct", None, None),
                        Handle::Linear { source, .. } | Handle::Alias { source } => ("via", Some(source.to_string()), None),
                        Handle::Locked(reason) => ("locked", None, Some(reason)),
                    };
                    Some(serde_json::json!({
                        "leaf": format!("{}.{field}", kind.name()),
                        "field": field,
                        "kind": value.as_ref().map(|v| format!("{:?}", v.kind()).to_lowercase()),
                        "text": value.as_ref().map(|v| v.to_string()),
                        "full": value.as_ref().map(|v| v.edit_text()),
                        "bind": slot.expr().map(|e| e.to_string()),
                        "edit": edit,
                        "via": via,
                        "why": why,
                        "options": kind.options(field),
                        "key": key_state(&store.0, iron_document::Leaf { node: id, path }),
                        // A field whose default is NaN reads "auto" (a key's span).
                        "auto": kind.default_facet().get(field).and_then(|s| s.constant()).is_some_and(|v| matches!(v, iron_document::Value::Number(x) if x.is_nan())),
                        "error": store.0.reactive().errors.get(&iron_document::Leaf { node: id, path }),
                    }))
                })
                .collect();
            serde_json::json!({"kind": kind.name(), "fields": fields})
        })
        .collect();
    // Optional components a person adds and removes from the inspector.
    let optional = |k: iron_document::FacetKind| n.ty.spec().allows(k) && !n.ty.spec().requires(k) && INSPECTOR_OPTIONAL.contains(&k.name());
    let addable: Vec<&str> = iron_document::FacetKind::ALL.iter().copied().filter(|&k| optional(k) && doc.facet(id, k).is_none()).map(|k| k.name()).collect();
    let removable: Vec<&str> = doc.facets_of(id).map(|c| c.kind()).filter(|&k| optional(k)).map(|k| k.name()).collect();
    let component = inspect_component(&store.0, id);
    let constraints = inspect_constraints(&store.0, id);
    let main = inspect_main(&store.0, id);
    let port = inspect_port(&store.0, id);
    // The table this node is, or is a cell of (layout-and-tables.md §5.1).
    let table = iron_document::table::table_of(doc, id).map(|t| t.to_string());
    // The inspector's sections, as the view holding the selection says
    // (contexts.md §2.7): that view's focus, or the node's own world.
    let focus = app
        .world()
        .get_resource::<crate::bevy_app::select::SelectTool>()
        .and_then(|t| t.reach.get(&id))
        .and_then(|r| app.world().get_resource::<crate::bevy_app::view_focus::ViewFocus>()?.views.iter().find(|(p, ..)| *p == r.panel).map(|(_, f, _)| f.clone()))
        .unwrap_or_else(|| iron_document::contexts::Focus { world: iron_document::composition::composition_of(doc, id), ..Default::default() });
    let sections = iron_document::contexts::Resolver::new(doc, store.0.contexts()).view_values(&focus).get(iron_document::contexts::Field::SECTIONS).to_owned();
    serde_json::json!({"node": id.to_string(), "type": n.ty.name(), "name": name, "components": components, "addable": addable, "removable": removable, "component": component, "main": main, "port": port, "table": table, "constraints": constraints, "sections": sections.split_whitespace().collect::<Vec<_>>()}).to_string()
}

/// A part of a resizable component's picture, selected on its main
/// (frame-constraints.md §4): the template part its constraints are set on,
/// and their values. Null for anything else.
fn inspect_constraints(store: &iron_document::Store, id: iron_document::NodeId) -> serde_json::Value {
    use iron_document::{expand, FacetKind};
    let doc = store.document();
    let part = match id.expanded_parts() {
        Some((owner, local)) if expand::main_definition(doc, owner).is_some() => local,
        Some(_) => return serde_json::Value::Null,
        None => id,
    };
    let Some(parent) = doc.live(part).and_then(|n| n.parent) else { return serde_json::Value::Null };
    if !doc.live(parent).is_some_and(|n| n.ty.name() == "template") || doc.facet(parent, FacetKind::Size).is_none() || !doc.live(part).is_some_and(|n| n.ty.spec().allows(FacetKind::Constraints)) {
        return serde_json::Value::Null;
    }
    let c = doc.get::<iron_document::facet::Constraints>(part);
    let word = |s: Option<&iron_document::Slot>, d: &str| s.and_then(|s| s.constant()).and_then(|v| v.as_str()).unwrap_or(d).to_owned();
    serde_json::json!({
        "node": part.to_string(),
        "horizontal": word(c.map(|c| &c.horizontal), "left"),
        "vertical": word(c.map(|c| &c.vertical), "top"),
    })
}

/// A main (component-authoring.md §1): its definition and the definition's
/// ports, for the inspector's Main section.
fn inspect_main(store: &iron_document::Store, id: iron_document::NodeId) -> serde_json::Value {
    let doc = store.document();
    let Some(def) = iron_document::expand::main_definition(doc, id) else { return serde_json::Value::Null };
    let name = doc.get::<iron_document::facet::Name>(def).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
    let ports: Vec<serde_json::Value> = iron_document::author::ports_of(doc, def)
        .into_iter()
        .map(|(spec, name, dir, placed)| serde_json::json!({"spec": spec.to_string(), "name": name, "dir": dir, "placed": placed, "port": port_json(store, spec)}))
        .collect();
    let instances = iron_document::author::entities_of(doc, def).len();
    let resizable = doc.children(def).iter().any(|&c| doc.live(c).is_some_and(|n| n.ty.name() == "template") && doc.facet(c, iron_document::FacetKind::Size).is_some());
    // Its state machines (machines.md §6): states, transitions, which state
    // is pinned for editing.
    // A scrubbed transition is a moment between states, not a state: the
    // inspector shows no state as previewed then (the panel says what it is).
    let pinned = store.pinned_state().filter(|p| p.def == def && p.scrub.is_none());
    let name_of = |n: iron_document::NodeId| doc.get::<iron_document::facet::Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
    let kids = |n: iron_document::NodeId, t: &str| doc.children(n).iter().copied().filter(|c| !c.is_expanded() && doc.live(*c).is_some_and(|x| x.ty.name() == t)).collect::<Vec<_>>();
    let text = |n: iron_document::NodeId, p: &str| store.resolved(n, p.parse().expect("declared")).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()));
    let machines: Vec<serde_json::Value> = iron_document::machine::machines(doc, def)
        .into_iter()
        .map(|(m, port)| {
            let states: Vec<_> = kids(m, "state").into_iter().map(|s| serde_json::json!({"id": s.to_string(), "name": name_of(s)})).collect();
            let transitions: Vec<_> = kids(m, "transition").into_iter().map(|t| serde_json::json!({"id": t.to_string(), "name": name_of(t), "from": text(t, "transition.from"), "to": text(t, "transition.to"), "ms": text(t, "transition.ms"), "ease": text(t, "transition.ease")})).collect();
            serde_json::json!({"id": m.to_string(), "port": port, "states": states, "transitions": transitions, "pinned": pinned.filter(|p| p.machine == m).map(|p| p.state.clone())})
        })
        .collect();
    serde_json::json!({"def": def.to_string(), "name": name, "ports": ports, "instances": instances, "resizable": resizable, "machines": machines})
}

fn port_json(store: &iron_document::Store, spec: iron_document::NodeId) -> serde_json::Value {
    let doc = store.document();
    let Some(p) = doc.get::<iron_document::facet::Port>(spec) else { return serde_json::Value::Null };
    let text = |s: &iron_document::Slot| s.constant().and_then(|v| v.as_str()).map(str::to_owned);
    let name = doc.get::<iron_document::facet::Name>(spec).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
    let follow = doc.nodes_with(iron_document::FacetKind::Anchor).filter(|a| !a.is_expanded()).find(|&a| doc.get::<iron_document::facet::Anchor>(a).and_then(|x| x.port.constant()).and_then(|v| v.as_ref()) == Some(spec)).and_then(|a| doc.get::<iron_document::facet::Anchor>(a)).and_then(|x| x.follow.constant()).and_then(|v| v.as_bool());
    serde_json::json!({
        "spec": spec.to_string(),
        "name": name,
        "kind": text(&p.kind),
        "dir": text(&p.dir),
        "connector": text(&p.connector),
        "gender": text(&p.gender),
        "follow": follow,
        "signal": p.signal.constant().map(|v| v.to_string()),
        "bind": p.signal.expr().map(|e| e.to_string()),
        "kinds": iron_document::port::KINDS.iter().map(|k| k.name).collect::<Vec<_>>(),
        "datatype": p.datatype.constant().and_then(|v| v.as_str()).map(str::to_owned),
        "show": text(&p.show),
        "group": text(&p.group),
        "suggest": iron_document::author::type_suggestions(doc, spec),
    })
}

/// A port's anchor (an empty marking where a port sits), for the
/// inspector's Port section: the spec it marks, editable when the anchor is
/// in a main or a definition's own template.
fn inspect_port(store: &iron_document::Store, id: iron_document::NodeId) -> serde_json::Value {
    let doc = store.document();
    let Some(a) = doc.get::<iron_document::facet::Anchor>(id) else { return serde_json::Value::Null };
    let Some(port) = a.port.constant().and_then(|v| v.as_ref()) else { return serde_json::Value::Null };
    let spec = if doc.live(port).is_some_and(|n| n.ty.name() == "port_spec") { port } else { port.expanded_parts().map(|(_, s)| s).unwrap_or(port) };
    let editable = id.path().first().is_some_and(|&root| iron_document::expand::main_definition(doc, root).is_some()) || !id.is_expanded();
    let mut j = port_json(store, spec);
    if let Some(o) = j.as_object_mut() {
        o.insert("editable".into(), serde_json::Value::Bool(editable));
    }
    j
}

/// The component instance a node is (an entity) or pictures (a figure), with
/// its inputs as inspector fields on the entity's `args`
/// (component-model.md §4): an unwired input edits its argument; a wired one
/// is locked and names its connection.
fn inspect_component(store: &iron_document::Store, id: iron_document::NodeId) -> serde_json::Value {
    use iron_document::{definition, Handle, LeafPath, FacetKind};
    let doc = store.document();
    let is_entity = doc.live(id).is_some_and(|n| n.ty.name() == "entity");
    let Some(entity) = (if is_entity { Some(id) } else { iron_document::composition::figured_by(doc, id) }) else { return serde_json::Value::Null };
    let name_of = |n: iron_document::NodeId| doc.get::<iron_document::facet::Name>(n).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
    let Some(def) = doc.get::<iron_document::facet::Instance>(entity).and_then(|i| i.def.constant()).and_then(|v| v.as_ref()) else { return serde_json::Value::Null };
    let Ok(inputs) = definition::inputs_of(doc, def) else { return serde_json::Value::Null };
    let wired = |input: &str| -> Option<String> {
        let port = doc.children(entity).iter().copied().find(|&p| doc.live(p).is_some_and(|n| n.ty.name() == "port") && name_of(p).as_deref() == Some(input))?;
        let leaf = iron_document::Leaf { node: port, path: "port.signal".parse().expect("declared leaf") };
        let fed = store.reactive().expr_of(leaf).is_some_and(|e| doc.leaf(port, leaf.path).and_then(|s| s.expr()) != Some(e));
        fed.then(|| store.reactive().expr_of(leaf).map(|e| e.to_string()).unwrap_or_default())
    };
    let fields: Vec<serde_json::Value> = inputs
        .iter()
        .filter_map(|input| {
            let path = LeafPath::new(FacetKind::Args, input.name).ok()?;
            let slot = doc.leaf(entity, path)?;
            let value = store.resolved(entity, path);
            let from = wired(input.name);
            let (edit, via, why) = match (&from, store.handle(entity, path)) {
                (Some(src), _) => ("locked", None, Some(format!("driven by its connection ({src})"))),
                (None, Handle::Direct) => ("direct", None, None),
                (None, Handle::Linear { source, .. } | Handle::Alias { source }) => ("via", Some(source.to_string()), None),
                (None, Handle::Locked(reason)) => ("locked", None, Some(reason)),
            };
            Some(serde_json::json!({
                "leaf": format!("args.{}", input.name),
                "field": input.name,
                "kind": value.as_ref().map(|v| format!("{:?}", v.kind()).to_lowercase()),
                "text": value.as_ref().map(|v| v.to_string()),
                "full": value.as_ref().map(|v| v.edit_text()),
                "bind": slot.expr().map(|e| e.to_string()),
                "edit": edit,
                "via": via,
                "why": why,
                "error": store.reactive().errors.get(&iron_document::Leaf { node: entity, path }),
                "key": key_state(store, iron_document::Leaf { node: entity, path }),
                // Its declared type (types-and-units.md §3.1): the widget.
                "datatype": (!input.ty.is_loose()).then(|| input.ty.to_string()),
                "options": input.ty.options(),
                "auto": input.ty.auto,
                "unit": input_unit(store, entity, &input.ty),
                "show": port_text(doc, input.spec, "show"),
                "group": port_text(doc, input.spec, "group"),
            }))
        })
        .collect();
    let linked = iron_document::expand::linked_definition(doc, entity).is_some();
    // What this picture reads from where it is placed (reactivity.md §6):
    // the selected picture's, or the entity's first.
    let figure = if is_entity { iron_document::composition::figures_of(doc, entity).first().copied() } else { Some(id) };
    let context: Vec<serde_json::Value> = figure
        .map(|f| store.context_reads(f))
        .unwrap_or_default()
        .into_iter()
        .map(|(name, provider, value)| {
            let from = provider.map(|p| {
                let holder = doc.live(p).and_then(|n| n.parent);
                holder.and_then(name_of).or_else(|| holder.map(|h| h.to_string())).unwrap_or_else(|| "the document".into())
            });
            serde_json::json!({"name": name, "from": from, "provider": provider.map(|p| p.to_string()), "text": value.map(|v| v.to_string())})
        })
        .collect();
    serde_json::json!({"entity": entity.to_string(), "name": name_of(entity), "def": name_of(def), "linked": linked, "inputs": fields, "context": context})
}

pub fn document_component(app: &mut WorkerApp, node: String, kind: String, add: bool) -> String {
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let Ok(id) = node.parse::<iron_document::NodeId>() else { return fail(format!("bad id {node:?}")) };
    let Some(k) = iron_document::FacetKind::from_name(&kind).filter(|k| INSPECTOR_OPTIONAL.contains(&k.name())) else { return fail(format!("no optional component {kind:?}")) };
    let op = if add { iron_document::Op::AddFacet { id, facet: k.default_facet() } } else { iron_document::Op::RemoveFacet { id, kind: k } };
    let label = format!("{} {kind}", if add { "add" } else { "remove" });
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let result = store.0.apply(iron_document::TransactionInput { label, actor: iron_document::Actor::Human, ts: 0, idempotency_key: None, ops: vec![op], exact: false });
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    match result {
        Ok(_) => serde_json::json!({"ok": true}).to_string(),
        Err(errors) => fail(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")),
    }
}

/// The unit an input is typed in: its type's display unit, or, for
/// `like(other)`, the unit the other input's data shows in.
fn input_unit(store: &iron_document::Store, entity: iron_document::NodeId, ty: &iron_document::types::Type) -> Option<String> {
    use iron_document::types::Base;
    match &ty.base {
        Base::Quantity { unit: Some(u), .. } => Some(u.clone()),
        Base::Like(other) => {
            let v = store.resolved(entity, iron_document::LeafPath::new(iron_document::FacetKind::Args, other).ok()?)?;
            let (data, dim) = v.as_series().map(|(d, dim)| (d.iter().copied().filter(|x| x.is_finite()).fold(0.0f64, |m, x| m.max(x.abs())), dim)).or_else(|| v.as_quantity())?;
            (!dim.is_none()).then(|| iron_document::units::display_unit(data, dim).0)
        }
        _ => None,
    }
}

fn port_text(doc: &iron_document::Document, spec: iron_document::NodeId, field: &str) -> Option<String> {
    let p = doc.get::<iron_document::facet::Port>(spec)?;
    let slot = match field { "show" => &p.show, _ => &p.group };
    slot.constant().and_then(|v| v.as_str()).filter(|t| !t.is_empty()).map(str::to_owned)
}

/// A property's animation, for the inspector's ◇: `"none"`, `"animated"`
/// (a clip drives it) or `"on"` (a key sits at the playhead).
fn key_state(store: &iron_document::Store, leaf: iron_document::Leaf) -> &'static str {
    use iron_document::timeline_view as tv;
    match tv::clip_of(store.document(), leaf) {
        None => "none",
        Some((clip, offset)) => {
            let u = store.time() - iron_document::time::start(store.document(), clip) - offset;
            if iron_document::time::keys(store.document(), clip).iter().any(|k| (k.t - u).abs() <= tv::AT_KEY) { "on" } else { "animated" }
        }
    }
}

fn inspector_current(store: &iron_document::Store, id: iron_document::NodeId, path: iron_document::LeafPath) -> Option<iron_document::Value> {
    store.resolved(id, path).or_else(|| {
        let node = store.document().live(id)?;
        if !node.ty.spec().allows(path.facet) || !INSPECTOR_OPTIONAL.contains(&path.facet.name()) { return None; }
        path.facet.default_facet().get(path.field).and_then(|s| s.constant()).cloned()
    })
}

/// The formula in a field's text, if it is one: `=expr` or `{= expr}`.
fn inspector_formula(text: &str) -> Option<String> {
    let t = text.trim();
    if let Some(inner) = t.strip_prefix("{=").and_then(|r| r.strip_suffix('}')) {
        return Some(inner.trim().to_owned());
    }
    t.strip_prefix('=').filter(|r| !r.trim().is_empty()).map(|r| r.trim().to_owned())
}

fn inspector_value(current: &iron_document::Value, text: &str) -> Result<iron_document::Value, String> {
    use iron_document::Value;
    let t = text.trim();
    let nums = |n: usize| -> Option<Vec<f64>> {
        let v: Vec<f64> = t.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
        (v.len() == n && v.iter().all(|v| v.is_finite())).then_some(v)
    };
    if t == "auto" && matches!(current, Value::Number(_) | Value::Quantity { .. }) {
        return Ok(Value::Number(f64::NAN));
    }
    let value = match current {
        // A number, or a quantity where the input takes one (an `auto`
        // range takes `400 nm`; the store checks the kind).
        Value::Number(_) => t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Value::Number).or_else(|| Value::parse_quantity(t).ok()),
        Value::Series { .. } => Value::parse_series(t).and_then(Result::ok),
        Value::Bool(_) => t.parse().ok().map(Value::Bool),
        Value::Str(_) => Some(Value::Str(t.to_owned())),
        Value::Enum(_) => Some(Value::Enum(t.to_owned())),
        Value::Vec2(_) => nums(2).map(|v| Value::Vec2([v[0], v[1]])),
        Value::Vec3(_) => nums(3).map(|v| Value::Vec3([v[0], v[1], v[2]])),
        Value::Color(c) => {
            if let Some(alpha) = t.strip_prefix("alpha:") {
                let alpha = alpha.parse::<f32>().map_err(|e| e.to_string())?;
                if !alpha.is_finite() { return Err("opacity must be finite".into()); }
                return Ok(Value::Color([c[0], c[1], c[2], alpha.clamp(0.0, 1.0)]));
            }
            let h = t.trim_start_matches('#');
            let ch = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|b| b as f32 / 255.0);
            match h.len() {
                6 | 8 => (|| Some(Value::Color([ch(0)?, ch(2)?, ch(4)?, if h.len() == 8 { ch(6)? } else { c[3] }])))(),
                _ => None,
            }
        }
        Value::Quantity { .. } => Value::parse_quantity(t).ok(),
        _ => None,
    };
    value.ok_or_else(|| format!("{t:?} is not a {:?}", current.kind()))
}

// Explicit targets keep an in-flight inspector edit attached to the selection
// that began it, even if the session selection changes before it arrives.
fn inspector_targets(node: &str, options: &serde_json::Value) -> Result<Vec<iron_document::NodeId>, String> {
    let names: Vec<String> = match options.get("nodes") {
        Some(nodes) => serde_json::from_value(nodes.clone()).map_err(|e| e.to_string())?,
        None => vec![node.to_owned()],
    };
    let mut ids = Vec::new();
    for name in names {
        let id = name.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
        if !ids.contains(&id) { ids.push(id); }
    }
    if ids.is_empty() { return Err("an inspector edit needs a target".into()); }
    Ok(ids)
}

fn inspector_patch_ops(store: &iron_document::Store, id: iron_document::NodeId, patch: &serde_json::Value) -> Result<Vec<iron_document::Op>, String> {
    use iron_document::{FacetKind, Leaf, LeafPath, Op, Slot, Value};
    use std::collections::BTreeMap;
    let mut writes: BTreeMap<Leaf, Value> = BTreeMap::new();
    // Formulas (`=expr` or `{= expr}`, typed in any field) set the leaf's
    // binding itself; `$self.k` is this entity's argument `k`.
    let mut bindings: Vec<Op> = Vec::new();
    if let Some(values) = patch.get("values").and_then(|v| v.as_object()) {
        for (leaf, text) in values {
            let path = leaf.parse::<LeafPath>().map_err(|e| e.to_string())?;
            if let Some(formula) = text.as_str().and_then(inspector_formula) {
                let formula = formula.replace("$self.", &format!("#{}.args.", id.to_string().trim_start_matches('#')));
                let expr = iron_document::Expr::parse(&formula).map_err(|e| format!("formula: {e}"))?;
                bindings.push(Op::Set { id, path, slot: Slot::Bound(expr) });
                continue;
            }
            let current = inspector_current(store, id, path).ok_or_else(|| format!("no property {id}.{path}"))?;
            let value = inspector_value(&current, text.as_str().ok_or("a property value must be text")?)?;
            // An animated leaf keys the playhead (After Effects' stopwatch).
            if let Some(keyed) = iron_document::timeline_view::set_animated_ops(store, iron_document::Leaf { node: id, path }, value.clone()) {
                bindings.extend(keyed);
                continue;
            }
            // `replace` (pasted data): a constant replaces the formula
            // instead of editing through it.
            if patch.get("replace").and_then(|v| v.as_bool()) == Some(true) && store.document().leaf(id, path).is_some_and(|s| s.expr().is_some()) {
                bindings.push(Op::Set { id, path, slot: Slot::Const(value) });
                continue;
            }
            let (target, value) = iron_document::handle::resolve_target(store.document(), store.reactive(), Leaf { node: id, path }, value)?;
            if let Some(prior) = writes.insert(target, value.clone()) && prior != value {
                return Err(format!("linked fields require conflicting values of {target}"));
            }
        }
    }
    let mut adds = BTreeMap::new();
    let mut ops = Vec::new();
    for (target, value) in writes {
        if store.document().facet(target.node, target.path.facet).is_none() {
            let comp = adds.entry((target.node, target.path.facet)).or_insert_with(|| target.path.facet.default_facet());
            comp.set(target.path.field, Slot::Const(value)).map_err(|e| e.to_string())?;
        } else {
            ops.push(Op::Set { id: target.node, path: target.path, slot: Slot::Const(value) });
        }
    }
    // Remove a formula: the leaf keeps the value it shows now.
    if let Some(unbind) = patch.get("unbind").and_then(|v| v.as_array()) {
        for leaf in unbind {
            let path = leaf.as_str().ok_or("unbind takes leaf paths")?.parse::<LeafPath>().map_err(|e| e.to_string())?;
            let value = store.resolved(id, path).ok_or_else(|| format!("{id}.{path} has no value to keep"))?;
            if matches!(value, Value::Fn(_)) {
                return Err("a function has no constant form; edit its formula instead".into());
            }
            ops.push(Op::Set { id, path, slot: Slot::Const(value) });
        }
    }
    let mut added: Vec<_> = adds.into_iter().map(|((id, _), comp)| Op::AddFacet { id, facet: comp }).collect();
    added.append(&mut ops);
    for op in bindings {
        // A binding on an optional component not yet added adds it.
        if let Op::Set { id, path, slot } = &op
            && store.document().facet(*id, path.facet).is_none()
        {
            let mut comp = path.facet.default_facet();
            comp.set(path.field, slot.clone()).map_err(|e| e.to_string())?;
            added.push(Op::AddFacet { id: *id, facet: comp });
        } else {
            added.push(op);
        }
    }
    if let Some(kinds) = patch.get("add").and_then(|v| v.as_array()) {
        for kind in kinds {
            let kind = kind.as_str().and_then(FacetKind::from_name).filter(|k| INSPECTOR_OPTIONAL.contains(&k.name())).ok_or("not an optional inspector component")?;
            if store.document().facet(id, kind).is_none() { added.push(Op::AddFacet { id, facet: kind.default_facet() }); }
        }
    }
    if let Some(remove) = patch.get("remove").and_then(|v| v.as_array()) {
        for kind in remove {
            let kind = kind.as_str().and_then(FacetKind::from_name).filter(|k| INSPECTOR_OPTIONAL.contains(&k.name())).ok_or("not an optional inspector component")?;
            if store.document().facet(id, kind).is_some() { added.push(Op::RemoveFacet { id, kind }); }
        }
    }
    Ok(added)
}

pub fn document_inspect_patch(app: &mut WorkerApp, node: String, patch: String) -> String {
    use iron_document::{Actor, Op};
    let mut run = || -> Result<serde_json::Value, String> {
        let patch: serde_json::Value = serde_json::from_str(&patch).map_err(|e| e.to_string())?;
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("no store")?;
        let ids = inspector_targets(&node, &patch)?;
        let mut added = Vec::new();
        for id in ids {
            for op in inspector_patch_ops(&store.0, id, &patch)? {
                if let Op::Set { id, path, slot } = &op {
                    if let Some(Op::Set { slot: prior, .. }) = added.iter().find(|o| matches!(o, Op::Set { id: other, path: p, .. } if other == id && p == path)) {
                        if prior != slot { return Err(format!("linked fields require conflicting values of {id}.{path}")); }
                        continue;
                    }
                }
                added.push(op);
            }
        }
        added.retain(|op| match op {
            Op::Set { id, path, slot } => store.0.document().leaf(*id, *path) != Some(slot),
            _ => true,
        });
        if added.is_empty() { return Ok(serde_json::json!({"ok": true})); }
        let label = patch.get("label").and_then(|v| v.as_str()).unwrap_or("edit appearance").to_owned();
        store.0.apply(iron_document::TransactionInput { label, actor: Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops: added, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

pub fn document_inspect_scrub(app: &mut WorkerApp, node: String, leaf: String, phase: String, token: u32, value: f64, options: Option<String>) -> String {
    use iron_document::{Actor, GestureId, Handle, Value};
    let mut run = || -> Result<serde_json::Value, String> {
        let options: serde_json::Value = serde_json::from_str(options.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())?;
        let ids = inspector_targets(&node, &options)?;
        let leaves: Vec<String> = if let Some(leaves) = options.get("leaves") { serde_json::from_value(leaves.clone()).map_err(|e| e.to_string())? } else { vec![leaf.clone()] };
        if leaves.is_empty() { return Err("a scrub needs a property".into()); }
        let paths = leaves.iter().map(|l| l.parse::<iron_document::LeafPath>().map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let channel = options.get("channel").and_then(|v| v.as_str());
        let alpha = channel == Some("alpha");
        // A colour picker's drag: `value` carries the whole colour packed as
        // 0xRRGGBBAA (exact in an f64), one preview per pointer move.
        let rgba = channel == Some("color");
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("no store")?;
        if phase == "begin" {
            for &id in &ids {
                for &path in &paths {
                    let current = inspector_current(&store.0, id, path);
                    if !(matches!(current, Some(Value::Number(_))) && !alpha && !rgba || matches!(current, Some(Value::Color(_))) && (alpha || rgba)) { return Err("this property cannot be scrubbed as a number".into()); }
                    if let Handle::Locked(why) = store.0.handle(id, path) { return Err(why); }
                }
            }
            let gesture = store.0.begin_gesture(&ids);
            return Ok(serde_json::json!({"ok": true, "token": gesture.0}));
        }
        if store.0.active_gesture() != Some(GestureId(token as u64)) { return Err("the gesture ended or was cancelled by another edit".into()); }
        match phase.as_str() {
            "move" => {
                if !value.is_finite() { return Err("value must be finite".into()); }
                let mut writes = std::collections::BTreeMap::new();
                for &id in &ids {
                    for &path in &paths {
                        let next = if rgba {
                            let packed = value.clamp(0.0, u32::MAX as f64) as u32;
                            Value::Color(packed.to_be_bytes().map(|c| c as f32 / 255.0))
                        } else if alpha {
                            let Some(Value::Color(mut color)) = inspector_current(&store.0, id, path) else { store.0.cancel_gesture(); return Err("no colour".into()); };
                            color[3] = value.clamp(0.0, 1.0) as f32;
                            Value::Color(color)
                        } else { Value::Number(value) };
                        let resolved = iron_document::handle::resolve_target(store.0.document(), store.0.reactive(), iron_document::Leaf { node: id, path }, next);
                        let (target, next) = match resolved { Ok(v) => v, Err(error) => { store.0.cancel_gesture(); return Err(error); } };
                        if let Some(prior) = writes.insert(target, next.clone()) && prior != next {
                            store.0.cancel_gesture();
                            return Err(format!("linked fields require conflicting values of {target}"));
                        }
                    }
                }
                for (target, next) in writes { store.0.preview(target.node, target.path, next); }
            }
            "commit" => {
                if let Some(r) = store.0.commit_gesture(format!("set {leaf}"), Actor::Human, crate::host_services::unix_millis()) {
                    r.map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
                }
            }
            "cancel" => store.0.cancel_gesture(),
            _ => return Err("unknown scrub phase".into()),
        }
        Ok(serde_json::json!({"ok": true}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// The colours used in `node`'s composition, most used first (ties in tree
/// order): the colour picker's "On this page" swatches.
/// The colour picker's tokens (palette.rs): the document's, then the
/// built-in ones it does not have yet (`id` null: using one adds the
/// theme). Each with its light and dark value as hex, and which is shown.
pub fn document_tokens(app: &mut WorkerApp) -> String {
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else { return "{}".into() };
    let doc = store.0.document();
    let hex = |v: Option<&iron_document::Value>| match v {
        Some(iron_document::Value::Color(c)) => format!("#{}", c.iter().map(|x| format!("{:02x}", (x.clamp(0.0, 1.0) * 255.0).round() as u8)).collect::<String>()),
        _ => "#000000ff".into(),
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    let have = iron_document::palette::tokens(doc);
    for (name, &t) in &have {
        let tok = doc.get::<iron_document::facet::Token>(t);
        out.push(serde_json::json!({"id": t.to_string(), "name": name, "light": hex(tok.and_then(|x| x.light.constant())), "dark": hex(tok.and_then(|x| x.dark.constant()))}));
    }
    // In the built-in order, the document's own after.
    let order = |n: &str| iron_document::palette::BUILT_IN.iter().position(|(b, _, _)| *b == n).unwrap_or(usize::MAX);
    out.sort_by_key(|t| order(t["name"].as_str().unwrap_or("")));
    for (name, light, dark) in iron_document::palette::BUILT_IN {
        if !have.contains_key(*name) {
            out.push(serde_json::json!({"id": null, "name": name, "light": format!("{light}ff"), "dark": format!("{dark}ff")}));
        }
    }
    serde_json::json!({"dark": store.0.dark(), "tokens": out}).to_string()
}

pub fn document_colors(app: &mut WorkerApp, node: String) -> String {
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else { return "[]".into() };
    let Ok(id) = node.parse::<iron_document::NodeId>() else { return "[]".into() };
    let doc = store.0.document();
    let Some(root) = iron_document::composition::composition_of(doc, id) else { return "[]".into() };
    let mut seen: Vec<(String, usize)> = Vec::new();
    for n in doc.descendants(root) {
        for facet in doc.facets_of(n) {
            let kind = facet.kind();
            for field in facet.field_names() {
                let Ok(path) = iron_document::LeafPath::new(kind, field) else { continue };
                let Some(iron_document::Value::Color(c)) = store.0.resolved(n, path) else { continue };
                let hex = iron_document::Value::Color(c).to_string();
                match seen.iter_mut().find(|(h, _)| *h == hex) { Some(entry) => entry.1 += 1, None => seen.push((hex, 1)) }
            }
        }
    }
    seen.sort_by(|a, b| b.1.cmp(&a.1));
    serde_json::to_string(&seen.into_iter().take(40).map(|(h, _)| h).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".into())
}

pub fn document_inspect_set(app: &mut WorkerApp, node: String, leaf: String, text: String) -> String {
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let Ok(id) = node.parse::<iron_document::NodeId>() else { return fail(format!("bad id {node:?}")) };
    let Ok(path) = leaf.parse::<iron_document::LeafPath>() else { return fail(format!("no leaf {leaf:?}")) };
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let Some(current) = store.0.resolved(id, path) else { return fail(format!("{id} has no {leaf}")) };
    let value = match inspector_value(&current, &text) { Ok(v) => v, Err(e) => return fail(e) };
    // One transaction through the leaf's handle, outside the gesture
    // overlay: a drag in progress is left alone unless this conflicts with it.
    let now = crate::host_services::unix_millis();
    let result = match store.0.set_through_handle(id, path, value, format!("set {leaf}"), iron_document::Actor::Human, now) {
        Ok(None) => serde_json::json!({"ok": true, "version": store.0.document().version().0}),
        Ok(Some(applied)) => serde_json::json!({"ok": true, "version": applied.version.0}),
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    result.to_string()
}

pub fn document_compositions(app: &mut WorkerApp) -> String {
    use iron_document::composition::{self, Dims};
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else {
        return "[]".into();
    };
    let doc = store.0.document();
    let list: Vec<serde_json::Value> = composition::compositions(doc)
        .into_iter()
        .map(|c| {
            let name = doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
            let dims = match composition::dims(doc, c) {
                Some(Dims::Three) => "3d",
                _ => "2d",
            };
            serde_json::json!({"id": c.to_string(), "name": name, "dims": dims, "session": c.is_session()})
        })
        .collect();
    serde_json::Value::Array(list).to_string()
}

/// The layers panel's rows (plans/specs/layers-panel.md §7.1): the visible
/// rows of the composition `source` (an id or name), descending into each
/// row whose key is in `open_json` (a JSON array of keys). `{"ok", "composition",
/// "dims", "rows", "total"}`.
pub fn document_layers(app: &mut WorkerApp, source: String, open_json: String) -> String {
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let doc = store.0.document();
    let Some(c) = find_composition(doc, &source) else { return fail(format!("no composition {source:?}")) };
    let open: std::collections::BTreeSet<String> = match serde_json::from_str(&open_json) {
        Ok(o) => o,
        Err(e) => return fail(format!("open: {e}")),
    };
    let mut rows = iron_document::layers::rows(doc, c, &open);
    let notes = store.0.problem_notes();
    for row in &mut rows {
        row.problem = row.id.parse().ok().and_then(|id| notes.get(&id)).map(|n| n.join("\n"));
    }
    let dims = match iron_document::composition::dims(doc, c) {
        Some(iron_document::composition::Dims::Three) => "3d",
        _ => "2d",
    };
    serde_json::json!({"ok": true, "composition": c.to_string(), "dims": dims, "total": rows.len(), "rows": rows}).to_string()
}

/// Set the selection from HTML (layers-panel.md §7.2): `items_json` is
/// `[{node, panel, chain?}]`, a chain being the select tool's (ids joined by
/// spaces, the node last); one left out is the node's lineage in its own
/// composition. `mode` is `replace`, `add` or `toggle`. One session change,
/// which the select tool reads back.
pub fn document_select(app: &mut WorkerApp, items_json: String, mode: String) -> String {
    use iron_document::{NodeId, Value};
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    #[derive(serde::Deserialize)]
    struct Item {
        node: String,
        #[serde(default)]
        panel: Option<String>,
        #[serde(default)]
        chain: Option<String>,
    }
    let items: Vec<Item> = match serde_json::from_str(&items_json) {
        Ok(i) => i,
        Err(e) => return fail(format!("items: {e}")),
    };
    if !matches!(mode.as_str(), "replace" | "add" | "toggle") {
        return fail(format!("unknown mode {mode:?}; use replace, add or toggle"));
    }
    if let Some(mut active) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active.remaining_frames = 10;
    }
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let doc = store.0.document();
    let mut picked = Vec::new();
    for item in items {
        let Some(id) = item.node.parse::<NodeId>().ok().filter(|&id| doc.is_live(id)) else { return fail(format!("no node {}", item.node)) };
        let chain = match item.chain.filter(|c| !c.is_empty()) {
            Some(c) => c,
            None => iron_document::layers::key(&iron_document::layers::lineage(doc, id)),
        };
        if chain.split(' ').next_back() != Some(item.node.as_str()) && chain.split(' ').next_back() != Some(id.to_string().as_str()) {
            return fail(format!("chain {chain:?} does not end at {}", item.node));
        }
        let mut data = std::collections::BTreeMap::new();
        if let Some(panel) = item.panel.filter(|p| !p.is_empty()) {
            data.insert("panel".to_owned(), Value::Str(panel));
        }
        data.insert("chain".to_owned(), Value::Str(chain));
        picked.push((id, data));
    }
    let mut next = if mode == "replace" { Vec::new() } else { store.0.selection() };
    for (id, data) in picked {
        match next.iter().position(|(n, _)| *n == id) {
            Some(i) if mode == "toggle" => {
                next.remove(i);
            }
            Some(i) => next[i] = (id, data),
            None => next.push((id, data)),
        }
    }
    match store.0.set_selection(&next) {
        Ok(()) => serde_json::json!({"ok": true, "selected": next.iter().map(|(n, _)| n.to_string()).collect::<Vec<_>>()}).to_string(),
        Err(e) => fail(e.to_string()),
    }
}

/// A composition named by id or name (as a view's `source` names it).
fn find_composition(doc: &iron_document::Document, source: &str) -> Option<iron_document::NodeId> {
    use iron_document::composition;
    let name_of = |c: iron_document::NodeId| doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
    composition::compositions(doc).into_iter().find(|&c| c.to_string() == source || name_of(c).as_deref() == Some(source))
}

/// The composition settings menu's state (free-running.md §3): the stored
/// run mode, whether its own view would be free running now, and the live
/// sources that make it so, by name.
pub fn document_composition_info(app: &mut WorkerApp, source: String) -> String {
    use iron_document::composition;
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let world = app.world();
    let (Some(store), Some(sources)) = (world.get_resource::<crate::document_bridge::DocumentStore>(), world.get_resource::<crate::document_bridge::liveness::LiveSources>()) else {
        return fail("no store".into());
    };
    let doc = store.0.document();
    let Some(c) = find_composition(doc, &source) else { return fail(format!("no composition {source:?}")) };
    let reach = composition::reach(doc, &[c], &sources.0);
    let name_of = |c: iron_document::NodeId| doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned))).unwrap_or_else(|| c.to_string());
    serde_json::json!({
        "ok": true,
        "id": c.to_string(),
        "run": composition::run_mode(doc, c).as_str(),
        "live": reach.live,
        "running": reach.running.iter().map(|&s| name_of(s)).collect::<Vec<_>>(),
    })
    .to_string()
}

/// Set a composition's run mode: one transaction, or a session op for a
/// session composition (the Library, Stats), which history never sees.
pub fn document_set_run_mode(app: &mut WorkerApp, source: String, mode: String) -> String {
    use iron_document::{Actor, Op, Slot, TransactionInput, Value};
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    if !matches!(mode.as_str(), "auto" | "always" | "on_demand") {
        return fail(format!("unknown run mode {mode:?}; use auto, always or on_demand"));
    }
    if let Some(mut active) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active.remaining_frames = 10;
    }
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let Some(c) = find_composition(store.0.document(), &source) else { return fail(format!("no composition {source:?}")) };
    let op = Op::Set { id: c, path: "composition.run".parse().expect("declared leaf"), slot: Slot::Const(Value::Enum(mode)) };
    if c.is_session() {
        return match store.0.apply_session(vec![op]) {
            Ok(()) => serde_json::json!({"ok": true}).to_string(),
            Err(e) => fail(e.to_string()),
        };
    }
    let input = TransactionInput { label: "set run mode".into(), actor: Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops: vec![op], exact: false };
    match store.0.apply(input) {
        Ok(a) => serde_json::json!({"ok": true, "version": a.version.0}).to_string(),
        Err(es) => fail(es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")),
    }
}

pub fn document_next_ids(app: &mut WorkerApp, n: u32) -> String {
    let ids: Vec<String> = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .map(|s| s.0.document().next_ids(n as usize).iter().map(|i| i.to_string()).collect())
        .unwrap_or_default();
    serde_json::to_string(&ids).expect("strings serialise")
}

pub fn document_next_relation_ids(app: &mut WorkerApp, n: u32) -> String {
    let ids: Vec<String> = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .map(|s| s.0.document().next_relation_ids(n as usize).iter().map(|i| i.to_string()).collect())
        .unwrap_or_default();
    serde_json::to_string(&ids).expect("strings serialise")
}

pub fn document_place(app: &mut WorkerApp, parent: String, after: String, count: u32) -> Result<String, String> {
    use iron_document::OrderKey;
    let store = app
        .world()
        .get_resource::<crate::document_bridge::DocumentStore>()
        .ok_or_else(|| String::from("document bridge not installed"))?;
    let doc = store.0.document();
    let siblings: &[iron_document::NodeId] = if parent.is_empty() {
        doc.roots()
    } else {
        let p: iron_document::NodeId = parent.parse().map_err(|e: iron_document::id::IdParseError| String::from(&e.to_string()))?;
        doc.children(p)
    };
    let key_of = |id: iron_document::NodeId| doc.live(id).map(|n| n.order.clone());
    let (lo, hi): (Option<OrderKey>, Option<OrderKey>) = if after == "end" {
        (siblings.last().and_then(|&s| key_of(s)), None)
    } else if after.is_empty() {
        (None, siblings.first().and_then(|&s| key_of(s)))
    } else {
        let a: iron_document::NodeId = after.parse().map_err(|e: iron_document::id::IdParseError| String::from(&e.to_string()))?;
        let pos = siblings.iter().position(|&s| s == a).ok_or_else(|| String::from(&format!("{a} is not a child of {}", if parent.is_empty() { "the root" } else { &parent })))?;
        (key_of(a), siblings.get(pos + 1).and_then(|&s| key_of(s)))
    };
    let mut keys = Vec::new();
    let mut prev = lo;
    for _ in 0..count {
        let k = OrderKey::between(prev.as_ref(), hi.as_ref()).map_err(|e| String::from(&e.to_string()))?;
        keys.push(k.to_string());
        prev = Some(k);
    }
    Ok(serde_json::to_string(&keys).expect("strings serialise"))
}

pub fn document_schema() -> String {
    iron_document::registry::schema_json().to_string()
}

/// Run a command (iron_document `commands.rs`) by id. Engine commands
/// travel with the keys (`ToolKeys`), so they apply in the order they were
/// asked for: a tool's transaction from an earlier key reaches
/// `PendingTransactions` when the tool handles it, and an undo queued
/// straight into `HistoryRequests` would overtake it on a slow frame
/// (undoing the previous change instead). A shell command, or an unknown
/// id, is refused here: the shell runs its own.
pub fn command(app: &mut WorkerApp, id: String) {
    match iron_document::commands::find(&id) {
        Some(c) if c.runs == iron_document::commands::Runs::Engine => {
            if let Some(mut keys) = app.world_mut().get_resource_mut::<crate::bevy_app::select::ToolKeys>() {
                keys.0.push((c.id.to_owned(), 0));
            }
        }
        Some(_) => warn!("command {id}: the shell runs it"),
        None => warn!("unknown command {id}"),
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// The command registry (`commands.rs`) as JSON: id, label, icon, whether
/// it needs a selection, its clipboard use and where it runs.
pub fn commands() -> String {
    iron_document::commands::registry_json().to_string()
}

pub fn set_modifiers(app: &mut WorkerApp, bits: u8) {
    if let Some(mut m) = app.world_mut().get_resource_mut::<crate::bevy_app::select::ShellModifiers>()
        && m.0 != bits
    {
        m.0 = bits;
    }
}

pub fn tool_key(app: &mut WorkerApp, key: String, bits: u8) {
    if let Some(mut keys) = app.world_mut().get_resource_mut::<crate::bevy_app::select::ToolKeys>() {
        keys.0.push((key.to_lowercase(), bits));
    }
}

/// `at` is a canvas point in CSS px (Paste here); the engine places it in
/// window px, as a pointer move is.
pub fn clipboard_paste(app: &mut WorkerApp, text: String, at: Option<(f32, f32)>) {
    let at = at.map(|(x, y)| app.to_physical_size(x, y));
    if let Some(mut clip) = app.world_mut().get_resource_mut::<crate::bevy_app::select::Clipboard>() {
        clip.pastes.push((text, at));
    }
}

pub fn key_down(app: &mut WorkerApp, key: String) {

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

pub fn key_up(app: &mut WorkerApp, key: String) {

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

const INSPECTOR_OPTIONAL: &[&str] = &["fill", "stroke", "corners", "arc", "shadow", "appearance", "gradient", "group_clip", "layout", "layout_item", "rules", "trim", "constraints"];

/// A host's answer to an `EngineEvent::Queries` request (iron_document
/// `query.rs`): the Model's plain JSON when `ok`, else what went wrong.
/// Answers whether it was taken (a newer request drops it).
pub fn query_result(app: &mut WorkerApp, node: String, seq: u64, ok: bool, payload: serde_json::Value) -> Result<String, String> {
    let node = node.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
    let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
    let result = if ok { Ok(iron_document::query::from_plain(&payload)) } else { Err(payload.as_str().map_or_else(|| payload.to_string(), str::to_owned)) };
    let taken = store.bypass_change_detection().0.resolve_query(node, seq, result);
    Ok(serde_json::json!({"taken": taken}).to_string())
}

/// The font picker's list (plans/specs/text.md §4): the catalog, the
/// installed families on a native host, and which are loaded now.
pub fn fonts() -> String {
    let e = iron_text::engine();
    let mut v: serde_json::Value = serde_json::from_str(&iron_text::catalog::to_json()).expect("catalog JSON");
    v["system"] = serde_json::json!(e.system_families());
    v["loaded"] = serde_json::json!(iron_text::catalog::FAMILIES.iter().filter(|f| e.has_family(f.name)).map(|f| f.name).collect::<Vec<_>>());
    v.to_string()
}

/// Live previews for families the build step could not draw (installed
/// fonts), as `previews.json` holds them.
pub fn font_previews(names: Vec<String>) -> String {
    iron_text::previews_json(&names, 20.0).to_string()
}

/// The host's textarea changed (`bevy_app::text_tool::TextInput::Replace`).
pub fn text_input(app: &mut WorkerApp, text: String, anchor: u32, focus: u32) {
    if let Some(mut t) = app.world_mut().get_resource_mut::<crate::bevy_app::text_tool::TextTool>() {
        t.inputs.push(crate::bevy_app::text_tool::TextInput::Replace { text, anchor, focus });
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// A caret motion by the canvas layout: `up`, `down`, `home`, `end`.
pub fn text_motion(app: &mut WorkerApp, motion: String, extend: bool) {
    use iron_text::Motion;
    let motion = match motion.as_str() {
        "up" => Motion::Up,
        "down" => Motion::Down,
        "home" => Motion::LineStart,
        "end" => Motion::LineEnd,
        "left" => Motion::Left,
        "right" => Motion::Right,
        _ => return,
    };
    if let Some(mut t) = app.world_mut().get_resource_mut::<crate::bevy_app::text_tool::TextTool>() {
        t.inputs.push(crate::bevy_app::text_tool::TextInput::Motion { motion, extend });
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// Bind a leaf to an expression, or with `expr` empty, unbind it, keeping
/// its current value (plans/specs/text.md §6: a readout's content). One
/// transaction either way.
pub fn document_inspect_bind(app: &mut WorkerApp, node: String, leaf: String, expr: String) -> String {
    use iron_document::{Actor, Expr, LeafPath, NodeId, Op, Slot, TransactionInput};
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let Ok(id) = node.parse::<NodeId>() else { return fail(format!("bad id {node:?}")) };
    let Ok(path) = leaf.parse::<LeafPath>() else { return fail(format!("no leaf {leaf:?}")) };
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let slot = if expr.trim().is_empty() {
        match store.0.resolved(id, path) {
            Some(v) => Slot::Const(v),
            None => return fail(format!("{id} has no {leaf}")),
        }
    } else {
        match Expr::parse(expr.trim()) {
            Ok(e) => Slot::Bound(e),
            Err(e) => return fail(e.to_string()),
        }
    };
    let label = if matches!(slot, Slot::Bound(_)) { format!("bind {leaf}") } else { format!("unbind {leaf}") };
    let input = TransactionInput { label, actor: Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops: vec![Op::Set { id, path, slot }], exact: false };
    let result = match store.0.apply(input) {
        Ok(a) => serde_json::json!({"ok": true, "version": a.version.0}),
        Err(es) => serde_json::json!({"ok": false, "error": es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")}),
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    result.to_string()
}

/// Style text (plans/specs/text.md §2.1): `patch` is a `RunStyle` as JSON.
/// While `node` is being edited with a range selected, the range takes it
/// (previewed with the edit, committed with it); otherwise the whole node
/// does, in one transaction, and its runs lose the properties it sets, as
/// in Figma.
pub fn document_text_style(app: &mut WorkerApp, node: String, patch: String) -> String {
    use iron_document::facet::Fill;
    use iron_document::{Actor, FacetKind, LeafStruct, Op, Slot, TransactionInput, Value};
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let options: serde_json::Value = match serde_json::from_str(&patch) { Ok(v) => v, Err(e) => return fail(e.to_string()) };
    let ids = match inspector_targets(&node, &options) { Ok(ids) => ids, Err(e) => return fail(e) };
    let id = ids[0];
    let patch: iron_text::RunStyle = match serde_json::from_str(&patch) {
        Ok(p) => p,
        Err(e) => return fail(e.to_string()),
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    if let Some(mut tool) = app.world_mut().get_resource_mut::<crate::bevy_app::text_tool::TextTool>()
        && ids.len() == 1
        && tool.editing() == Some(id)
        && tool.has_range()
    {
        tool.inputs.push(crate::bevy_app::text_tool::TextInput::Style(patch));
        return serde_json::json!({"ok": true, "range": true}).to_string();
    }
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let mut ops = Vec::new();
    for id in ids {
        if store.0.document().live(id).is_none_or(|n| n.ty.name() != "text") {
            return fail(format!("{id} is not text"));
        }
        let leaf = |p: &str| p.parse::<iron_document::LeafPath>().expect("declared leaf");
        let mut set = |path: &str, v: Value| ops.push(Op::Set { id, path: leaf(path), slot: Slot::Const(v) });
        if let Some(f) = &patch.family { set("font.family", Value::Str(f.clone())); }
        if let Some(v) = patch.size { set("font.size", Value::Number(v as f64)); }
        if let Some(v) = patch.weight { set("font.weight", Value::Number(v as f64)); }
        if let Some(v) = patch.italic { set("font.italic", Value::Bool(v)); }
        if let Some(v) = patch.underline { set("font.underline", Value::Bool(v)); }
        if let Some(v) = patch.strike { set("font.strike", Value::Bool(v)); }
        if let Some(c) = patch.color {
            if store.0.document().facet(id, FacetKind::Fill).is_some() {
                set("fill.color", Value::Color(c));
            } else {
                let mut fill = Fill::default();
                fill.color = Slot::Const(Value::Color(c));
                ops.push(Op::AddFacet { id, facet: fill.wrap() });
            }
        }
        let text = iron_document::text::content(&store.0, id);
        let runs = iron_document::text::runs(&store.0, id);
        let cleared = iron_text::runs::clear(&text, &runs, &patch);
        if cleared != runs {
            ops.push(Op::Set { id, path: leaf("text.runs"), slot: Slot::Const(Value::Str(iron_text::runs::to_json(&cleared))) });
        }
    }
    if ops.is_empty() {
        return serde_json::json!({"ok": true}).to_string();
    }
    let input = TransactionInput { label: "style text".into(), actor: Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false };
    match store.0.apply(input) {
        Ok(a) => serde_json::json!({"ok": true, "version": a.version.0}).to_string(),
        Err(es) => fail(es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")),
    }
}

/// A style shortcut inside a text edit: `bold`, `italic`, `underline`,
/// `strike` (`bevy_app::text_tool::TextInput::Toggle`).
pub fn text_toggle(app: &mut WorkerApp, what: String) {
    if let Some(mut t) = app.world_mut().get_resource_mut::<crate::bevy_app::text_tool::TextTool>() {
        t.inputs.push(crate::bevy_app::text_tool::TextInput::Toggle(what));
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
}

/// A render inspector's legend (plans/specs/render-inspector.md §2): the
/// watched panel's chunks and how its view culls them.
pub fn document_render_inspector(app: &mut WorkerApp, panel: String) -> String {
    crate::document_bridge::inspector::info(app.world(), &panel).to_string()
}

/// Set what a render inspector draws: `{"tint", "dim", "outlines"}`, each
/// a bool, missing ones left as they are.
pub fn document_render_inspector_options(app: &mut WorkerApp, panel: String, options: String) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&options) else {
        return serde_json::json!({"ok": false, "error": "options must be JSON"}).to_string();
    };
    let Some(mut all) = app.world_mut().get_resource_mut::<crate::document_bridge::inspector::InspectorOptions>() else {
        return serde_json::json!({"ok": false, "error": "no inspector"}).to_string();
    };
    let o = all.0.entry(panel).or_default();
    let flag = |k: &str, old: bool| v.get(k).and_then(|b| b.as_bool()).unwrap_or(old);
    *o = crate::document_bridge::inspector::Options { tint: flag("tint", o.tint), dim: flag("dim", o.dim), outlines: flag("outlines", o.outlines), tiles: flag("tiles", o.tiles) };
    let o = *o;
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    serde_json::json!({"ok": true, "tint": o.tint, "dim": o.dim, "outlines": o.outlines, "tiles": o.tiles}).to_string()
}


// ---- narratives (plans/specs/narrative-panel.md) ----------------------------

fn narrative_run(app: &mut WorkerApp, run: impl FnOnce(&mut iron_document::Store) -> Result<serde_json::Value, String>) -> String {
    let result = match app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() {
        Some(mut store) => run(&mut store.0),
        None => Err("document bridge not installed".into()),
    };
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() {
        activity.remaining_frames = 10;
    }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

fn narrative_actor(actor: &str) -> iron_document::Actor {
    if actor == "model" { iron_document::Actor::Model } else { iron_document::Actor::Human }
}

fn narrative_apply(store: &mut iron_document::Store, label: &str, actor: &str, ops: Vec<iron_document::Op>) -> Result<u64, String> {
    if ops.is_empty() {
        return Ok(store.document().version().0);
    }
    let applied = store
        .apply(iron_document::TransactionInput { label: label.into(), actor: narrative_actor(actor), ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
        .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
    Ok(applied.version.0)
}

fn narrative_source(store: &iron_document::Store, source: &str) -> Result<iron_document::NodeId, String> {
    iron_document::narrative::find(store.document(), source).ok_or_else(|| format!("no narrative {source:?}"))
}

/// The narratives in the document: `{"ok", "narratives": [{"id", "name", "blocks"}]}`.
pub fn narrative_list(app: &mut WorkerApp) -> String {
    narrative_run(app, |store| {
        let doc = store.document();
        let list: Vec<_> = iron_document::narrative::narratives(doc)
            .into_iter()
            .map(|n| serde_json::json!({"id": n.to_string(), "name": iron_document::narrative::name(doc, n), "blocks": doc.children(n).len()}))
            .collect();
        Ok(serde_json::json!({"ok": true, "narratives": list}))
    })
}

/// One narrative's blocks for the panel: `{"ok", "id", "name", "version",
/// "blocks": [{"id", "kind", "level", "show", "focus", "text", "marks"}]}`.
/// Each mention and value mark also carries its target's `label` (name,
/// else type) and `stale` (the target is gone).
pub fn narrative_read(app: &mut WorkerApp, source: String) -> String {
    narrative_run(app, |store| {
        let n = narrative_source(store, &source)?;
        let doc = store.document();
        let label = |t: iron_document::NodeId| -> String {
            match doc.live(t) {
                None => String::new(),
                Some(node) => doc
                    .get::<iron_document::facet::Name>(t)
                    .and_then(|nm| nm.text.constant().and_then(|v| v.as_str().map(str::to_owned)))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| node.ty.name().to_owned()),
            }
        };
        let blocks: Vec<serde_json::Value> = iron_document::narrative::blocks(doc, n)
            .into_iter()
            .map(|(id, c)| {
                let mut v = serde_json::to_value(&c).expect("block serialises");
                v["id"] = serde_json::json!(id.to_string());
                if let Some(marks) = v["marks"].as_array_mut() {
                    for m in marks.iter_mut() {
                        if let Some(t) = m["target"].as_str().and_then(iron_document::narrative::target_node) {
                            m["label"] = serde_json::json!(label(t));
                            m["stale"] = serde_json::json!(!doc.is_live(t));
                        }
                    }
                }
                v
            })
            .collect();
        Ok(serde_json::json!({"ok": true, "id": n.to_string(), "name": iron_document::narrative::name(doc, n), "version": doc.version().0, "blocks": blocks}))
    })
}

/// Apply block edits (`narrative::Edit`, JSON list) as one transaction:
/// `{"ok", "version", "minted": {key: id}}`.
pub fn narrative_edit(app: &mut WorkerApp, source: String, edits: String, label: String, actor: String) -> String {
    narrative_run(app, |store| {
        let n = narrative_source(store, &source)?;
        let edits: Vec<iron_document::narrative::Edit> = serde_json::from_str(&edits).map_err(|e| format!("edits: {e}"))?;
        let planned = iron_document::narrative::plan(store.document(), n, &edits)?;
        let label = if label.is_empty() { "edit narrative".to_owned() } else { label };
        let version = narrative_apply(store, &label, &actor, planned.ops)?;
        let minted: serde_json::Map<String, serde_json::Value> = planned.minted.into_iter().map(|(k, v)| (k, serde_json::json!(v.to_string()))).collect();
        Ok(serde_json::json!({"ok": true, "version": version, "minted": minted}))
    })
}

/// A new narrative named `name` holding `markdown`'s blocks: `{"ok", "id", "version"}`.
pub fn narrative_create(app: &mut WorkerApp, name: String, markdown: String, actor: String) -> String {
    narrative_run(app, |store| {
        let mut blocks = iron_document::narrative::parse_markdown(&markdown);
        if blocks.is_empty() {
            blocks.push(iron_document::narrative::BlockContent::default());
        }
        let (id, ops) = iron_document::narrative::create_ops(store.document(), &name, &blocks);
        let version = narrative_apply(store, &format!("add narrative {name}"), &actor, ops)?;
        Ok(serde_json::json!({"ok": true, "id": id.to_string(), "version": version}))
    })
}

/// A narrative as markdown (export): `{"ok", "name", "markdown"}`.
pub fn narrative_markdown(app: &mut WorkerApp, source: String) -> String {
    narrative_run(app, |store| {
        let n = narrative_source(store, &source)?;
        Ok(serde_json::json!({"ok": true, "name": iron_document::narrative::name(store.document(), n), "markdown": iron_document::narrative::to_markdown(store.document(), n, false)}))
    })
}

/// Insert `markdown`'s blocks after `after` (a block id; empty: at the
/// start; `"end"`: at the end), one transaction: the model's `<write>`.
/// `{"ok", "version", "blocks": [ids]}`.
pub fn narrative_write(app: &mut WorkerApp, source: String, after: String, markdown: String, actor: String) -> String {
    narrative_run(app, |store| {
        use iron_document::narrative::{self, Edit};
        let n = narrative_source(store, &source)?;
        let mut prev = match after.as_str() {
            "" => None,
            "end" => store.document().children(n).last().map(|b| b.to_string()),
            a => Some(a.to_owned()),
        };
        let mut edits = Vec::new();
        for (i, block) in narrative::parse_markdown(&markdown).into_iter().enumerate() {
            let key = format!("w{i}");
            edits.push(Edit::Insert { key: key.clone(), after: prev.clone(), block });
            prev = Some(key);
        }
        if edits.is_empty() {
            return Err("nothing to write: the markdown has no blocks".into());
        }
        let planned = narrative::plan(store.document(), n, &edits)?;
        let ids: Vec<String> = (0..edits.len()).map(|i| planned.minted[&format!("w{i}")].to_string()).collect();
        let version = narrative_apply(store, if actor == "model" { "model: write" } else { "write" }, &actor, planned.ops)?;
        Ok(serde_json::json!({"ok": true, "version": version, "blocks": ids}))
    })
}

/// Replace block `block` with `markdown`'s blocks (none: delete it), one
/// transaction: the model's `<rewrite>`. `{"ok", "version", "blocks": [ids]}`.
pub fn narrative_rewrite(app: &mut WorkerApp, block: String, markdown: String, actor: String) -> String {
    narrative_run(app, |store| {
        use iron_document::narrative::{self, Edit};
        let id = block.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
        let n = store.document().live(id).and_then(|b| b.parent).filter(|p| narrative::narratives(store.document()).contains(p)).ok_or_else(|| format!("{block} is not a narrative block"))?;
        let parsed = narrative::parse_markdown(&markdown);
        let mut edits = Vec::new();
        let mut ids = Vec::new();
        let mut prev = block.clone();
        for (i, b) in parsed.into_iter().enumerate() {
            if i == 0 {
                edits.push(Edit::Set { id: block.clone(), block: b });
                ids.push(block.clone());
            } else {
                let key = format!("r{i}");
                edits.push(Edit::Insert { key: key.clone(), after: Some(prev.clone()), block: b });
                prev = key.clone();
                ids.push(key);
            }
        }
        if edits.is_empty() {
            edits.push(Edit::Delete { id: block.clone() });
        }
        let planned = narrative::plan(store.document(), n, &edits)?;
        let ids: Vec<String> = ids.into_iter().map(|k| planned.minted.get(&k).map_or(k, |v| v.to_string())).collect();
        let version = narrative_apply(store, if actor == "model" { "model: rewrite" } else { "rewrite" }, &actor, planned.ops)?;
        Ok(serde_json::json!({"ok": true, "version": version, "blocks": ids}))
    })
}

/// Every live value in a narrative, formatted, keyed by the expression it
/// shows (`narrative::value_expr`): `{"ok", "values": {expr: text}, "errors": {expr: message}}`.
pub fn narrative_values(app: &mut WorkerApp, source: String) -> String {
    narrative_run(app, |store| {
        use iron_document::narrative;
        let n = narrative_source(store, &source)?;
        let mut values = serde_json::Map::new();
        let mut errors = serde_json::Map::new();
        for (b, c) in narrative::blocks(store.document(), n) {
            let _ = b;
            for m in c.marks.iter().filter(|m| m.kind == "value") {
                let Some(key) = narrative::value_expr(m) else { continue };
                if values.contains_key(&key) || errors.contains_key(&key) {
                    continue;
                }
                let result = Some(key.clone())
                    .ok_or_else(|| "no target".to_owned())
                    .and_then(|e| iron_document::Expr::parse(&e).map_err(|e| e.to_string()))
                    .and_then(|e| e.eval(&|l| store.resolved(l.node, l.path)).map_err(|e| e.to_string()));
                match result {
                    Ok(v) => {
                        values.insert(key, serde_json::json!(v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())));
                    }
                    Err(e) => {
                        errors.insert(key, serde_json::json!(e));
                    }
                }
            }
        }
        Ok(serde_json::json!({"ok": true, "values": values, "errors": errors}))
    })
}

/// Cross-probe from HTML (a narrative's mention): `action` `hover` lights
/// `node` and its counterparts in every composition, `unhover` clears it,
/// `select` selects it (ECM §7.2). `{"ok"}`.
pub fn narrative_probe(app: &mut WorkerApp, node: String, action: String) -> String {
    let id = if node.is_empty() { None } else { node.parse::<iron_document::NodeId>().ok() };
    let live = id.filter(|&id| app.world().get_resource::<crate::document_bridge::DocumentStore>().is_some_and(|s| s.0.document().is_live(id)));
    if let Some(mut sel) = app.world_mut().get_resource_mut::<crate::SelectionState>() {
        match action.as_str() {
            "hover" => {
                sel.probe = live;
                sel.hovered = live;
            }
            "unhover" => {
                sel.probe = None;
                sel.hovered = None;
            }
            "select" => sel.selected = live.into_iter().collect(),
            _ => return serde_json::json!({"ok": false, "error": format!("unknown probe action {action:?}")}).to_string(),
        }
    }
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() {
        activity.remaining_frames = 10;
    }
    serde_json::json!({"ok": live.is_some() || action == "unhover"}).to_string()
}

/// The playhead (timeline.md §4.1): `{"ok", "id", "t", "playing", "rate",
/// "loop_in", "loop_out", "end"}`, `end` where the last clip ends. `id` is
/// empty until something creates the clock.
pub fn clock_read(app: &mut WorkerApp) -> String {
    narrative_run(app, |store| {
        use crate::document_bridge::clock;
        let n = |f: &str| clock::read(store, f).and_then(|v| v.as_f64()).unwrap_or(0.0);
        Ok(serde_json::json!({
            "ok": true,
            "id": store.clock().map(|c| c.to_string()).unwrap_or_default(),
            "t": store.time(),
            "playing": clock::playing(store),
            "autokey": iron_document::timeline_view::autokey(store),
            "rate": if store.clock().is_some() { n("rate") } else { 1.0 },
            "loop_in": n("loop_in"),
            "loop_out": n("loop_out"),
            "end": clock::end_of_clips(store),
        }))
    })
}

/// Set clock fields from JSON (`{"t": 1.5, "playing": true}`): session
/// ops, never saved or undone. The clock is created when missing.
pub fn clock_set(app: &mut WorkerApp, fields: String) -> String {
    narrative_run(app, |store| {
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&fields).map_err(|e| format!("clock_set: {e}"))?;
        let mut out: Vec<(&str, iron_document::Value)> = Vec::new();
        for (k, v) in &map {
            let value = match (k.as_str(), v) {
                ("playing" | "autokey", serde_json::Value::Bool(b)) => iron_document::Value::Bool(*b),
                ("t" | "rate" | "loop_in" | "loop_out", serde_json::Value::Number(x)) => iron_document::Value::Number(x.as_f64().unwrap_or(0.0)),
                _ => return Err(format!("clock_set: bad field {k} = {v}")),
            };
            out.push((k.as_str(), value));
        }
        crate::document_bridge::clock::set(store, &out)?;
        Ok(serde_json::json!({"ok": true, "t": store.time()}))
    })
}

/// Navigate the timeline's time axis (timeline.md §6), as session ops:
/// `{"fit": width}` fits every clip into `width` document px;
/// `{"pan": dx, "zoom": z, "at": x}` pans by `dx` px and zooms by `z` (log,
/// positive out) about document x `at`; `{"scroll": dy}` scrolls the rows
/// by `dy` document px (`axis.y0`). `{"ok", "t0", "pps", "y0"}` after.
pub fn timeline_axis(app: &mut WorkerApp, fields: String) -> String {
    narrative_run(app, |store| {
        use iron_document::timeline_view as tv;
        let v: serde_json::Value = serde_json::from_str(&fields).map_err(|e| format!("timeline_axis: {e}"))?;
        let n = |k: &str| v.get(k).and_then(|x| x.as_f64());
        if let Some(part) = v.get("open").and_then(|x| x.as_str()).and_then(|x| x.parse::<iron_document::NodeId>().ok()) {
            // A part's rows inside a transition, open or closed (session).
            store.set_inside_open(part, v.get("on").and_then(|x| x.as_bool()).unwrap_or(true));
        } else if let (Some(t0), Some(pps)) = (n("t0"), n("pps")) {
            // Put the axis back where it was (the state editor, leaving a
            // transition, restores the document's view).
            let comp = tv::find(store).ok_or("no timeline composition")?;
            let set = |f: &str, v: f64| iron_document::Op::Set { id: comp, path: iron_document::facet::LeafPath::new(iron_document::facet::FacetKind::Axis, f).expect("axis field"), slot: iron_document::Slot::Const(iron_document::Value::Number(v)) };
            store.apply_session(vec![set("t0", t0.max(0.0)), set("pps", pps.clamp(tv::PPS.0, tv::PPS.1)), set("y0", n("y0").unwrap_or(0.0).max(0.0))]).map_err(|e| e.message)?;
        } else if let Some(t) = n("cursor") {
            // The playhead inside a transition (its preview), seconds.
            let comp = tv::find(store).ok_or("no timeline composition")?;
            store.apply_session(vec![iron_document::Op::Set { id: comp, path: iron_document::facet::LeafPath::new(iron_document::facet::FacetKind::Axis, "cursor").expect("axis field"), slot: iron_document::Slot::Const(iron_document::Value::Number(t.max(0.0))) }]).map_err(|e| e.message)?;
        } else if let Some(w) = n("fit") {
            tv::fit(store, w)?;
        } else if let Some(dy) = n("scroll") {
            tv::scroll(store, dy)?;
        } else {
            tv::navigate(store, n("pan").unwrap_or(0.0), n("zoom").unwrap_or(0.0), n("at").unwrap_or(0.0))?;
        }
        let comp = tv::find(store).ok_or("no timeline composition")?;
        let read = |f: &str| iron_document::facet::LeafPath::new(iron_document::facet::FacetKind::Axis, f).ok().and_then(|p| store.resolved(comp, p)).and_then(|v| v.as_f64());
        Ok(serde_json::json!({"ok": true, "t0": read("t0"), "pps": read("pps"), "y0": read("y0")}))
    })
}

/// The timeline's rows for its DOM column (timeline.md §12.3): `{"ok",
/// "rows": [{"kind", "label", "y", "target", "prop", "clip", "error",
/// "keyed"}], "y0", "zoom", "label_w", "ruler_h", "row_h"}`, `y` in
/// document px; `zoom` is CSS px per document px, so a row sits at
/// `(y − y0)·zoom` CSS px from the view's top, whatever the pixel ratio.
/// `keyed`: whether a key of the row's clip is at the playhead.
pub fn timeline_rows(app: &mut WorkerApp, panel: String) -> String {
    let camera = app.world().get_resource::<crate::document_bridge::ViewCameras>().and_then(|c| c.0.get(&panel).copied()).unwrap_or_default();
    narrative_run(app, |store| {
        use iron_document::timeline_view as tv;
        let clock = store.clock().ok_or("no clock yet")?;
        let comp = tv::find(store);
        let plan = tv::plan(store, clock, comp);
        let doc = store.document();
        let now = store.time();
        let pieces = comp.map(|c| tv::pieces(store, c)).unwrap_or_default();
        let rows: Vec<serde_json::Value> = tv::rows(&plan)
            .into_iter()
            .map(|r| {
                let keyed = r.clip.is_some_and(|c| {
                    let base = iron_document::time::start(doc, c) + doc.relations_from(c).find(|x| x.rel == iron_document::RelKind::Animates).and_then(|x| x.data.get("offset").and_then(|v| v.as_f64())).unwrap_or(0.0);
                    iron_document::time::keys(doc, c).iter().any(|k| (base + k.t - now).abs() <= tv::AT_KEY)
                });
                // A state lane says what state it is in at the playhead, and
                // the transition it is going through, if any.
                let mut state = serde_json::Value::Null;
                if r.lane
                    && let Some(port) = r.prop.as_deref().and_then(|p| p.strip_prefix("args."))
                    && let Ok(path) = iron_document::facet::LeafPath::new(iron_document::FacetKind::Args, port)
                {
                    let read = |l: iron_document::Leaf| store.resolved(l.node, l.path);
                    let leaf = iron_document::Leaf { node: r.target, path };
                    let now_state = read(leaf).and_then(|v| v.as_str().map(str::to_owned));
                    let run = iron_document::expand::linked_definition(doc, r.target).and_then(|def| {
                        let (m, _) = iron_document::machine::machines(doc, def).into_iter().find(|(_, p)| p == port)?;
                        let (t, p, from, to) = machine_running(store, &read, def, m, r.target, leaf, now)?;
                        let name = doc.get::<iron_document::facet::Name>(t).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
                        Some(serde_json::json!({"name": name, "progress": p, "from": from, "to": to}))
                    });
                    state = serde_json::json!({"state": now_state, "running": run});
                }
                // Inside a transition: the channel's slice, if it has one.
                let tween = plan.objects.iter().flat_map(|o| &o.props).find(|p| Some(&p.prop) == r.prop.as_ref() && p.slice.is_some()).and_then(|p| p.slice.as_ref()).and_then(|s| s.tween).map(|w| w.to_string());
                // Its bars (an object row: all its properties'), for the
                // column to select.
                let slice_bar = |p: &tv::Prop| p.slice.as_ref().and_then(|s| s.tween).and_then(|w| pieces.get(&format!("slice {w}"))).map(|n| n.to_string());
                let bars: Vec<String> = match (&plan.inside, r.kind) {
                    (Some(i), "object") if !i.open.contains(&r.target) => pieces.get(&format!("part {}", r.target)).map(|n| vec![n.to_string()]).unwrap_or_default(),
                    (Some(_), "object") => plan.objects.iter().find(|o| o.target == r.target).into_iter().flat_map(|o| &o.props).filter_map(slice_bar).collect(),
                    (Some(_), _) => plan.objects.iter().find(|o| o.target == r.target).into_iter().flat_map(|o| &o.props).filter(|p| Some(&p.prop) == r.prop.as_ref()).filter_map(slice_bar).collect(),
                    _ => Vec::new(),
                };
                serde_json::json!({"kind": r.kind, "label": r.label, "y": r.y, "target": r.target.to_string(), "prop": r.prop, "clip": r.clip.map(|c| c.to_string()), "error": r.error, "keyed": keyed, "lane": r.lane, "state": state, "tween": tween, "bars": bars, "open": plan.inside.as_ref().map(|i| i.open.contains(&r.target))})
            })
            .collect();
        let axis = |f: &str, d: f64| comp.and_then(|c| iron_document::facet::LeafPath::new(iron_document::facet::FacetKind::Axis, f).ok().and_then(|p| store.resolved(c, p))).and_then(|v| v.as_f64()).unwrap_or(d);
        let label_w = axis("label_w", 160.0);
        let inside = plan.inside.as_ref().map(|i| serde_json::json!({"from": i.from, "to": i.to, "own": i.own, "secs": i.secs, "transition": i.transition.map(|t| t.to_string()), "cursor": axis("cursor", 0.0)}));
        Ok(serde_json::json!({"ok": true, "rows": rows, "y0": axis("y0", 0.0), "zoom": camera.zoom, "label_w": label_w, "ruler_h": plan.ruler_h(), "row_h": tv::ROW_H, "top": plan.top(), "inset": plan.inset(), "inside": inside}))
    })
}

/// A timeline row's key buttons (timeline.md §12.3): `prev` and `next` move
/// the playhead to the clip's previous or next key (session ops), `toggle`
/// adds a key at the playhead holding the value shown, or removes the key
/// there (a transaction). `{"ok", "t"}` or `{"ok", "version"}`.
pub fn timeline_key(app: &mut WorkerApp, action: String, clip: String) -> String {
    narrative_run(app, |store| {
        use iron_document::timeline_view as tv;
        let clip: iron_document::NodeId = clip.parse().map_err(|_| format!("not a node id: {clip}"))?;
        match action.as_str() {
            "prev" | "next" => {
                let Some(t) = tv::step_key(store, clip, if action == "prev" { -1 } else { 1 }) else { return Ok(serde_json::json!({"ok": true, "t": store.time()})) };
                crate::document_bridge::clock::set(store, &[("playing", iron_document::Value::Bool(false)), ("t", iron_document::Value::Number(t))])?;
                Ok(serde_json::json!({"ok": true, "t": t}))
            }
            "toggle" => {
                let ops = tv::toggle_key_ops(store, clip)?;
                let label = if ops.iter().any(|o| matches!(o, iron_document::Op::Delete { .. })) { "remove key" } else { "add key" };
                let version = narrative_apply(store, label, "human", ops)?;
                Ok(serde_json::json!({"ok": true, "version": version}))
            }
            _ => Err(format!("timeline_key: unknown action {action:?} (prev, next, toggle)")),
        }
    })
}

/// The inspector's animation actions on one property (timeline.md §12.3):
/// `key` is the ◇ beside a field (toggle a key at the playhead, or start an
/// animation there); `draw_on` keys `trim.end` 0 → 1 over a second with Easy
/// Ease (`leaf` is ignored). One transaction each.
pub fn inspect_animate(app: &mut WorkerApp, node: String, leaf: String, action: String) -> String {
    narrative_run(app, |store| {
        use iron_document::timeline_view as tv;
        let id: iron_document::NodeId = node.parse().map_err(|_| format!("not a node id: {node}"))?;
        let (ops, label) = match action.as_str() {
            "key" => {
                let path: iron_document::LeafPath = leaf.parse().map_err(|e| format!("{e}"))?;
                let ops = tv::key_leaf_ops(store, iron_document::Leaf { node: id, path })?;
                let label = if ops.iter().any(|o| matches!(o, iron_document::Op::Delete { .. })) { "remove key" } else { "add key" };
                (ops, label)
            }
            "draw_on" => (tv::draw_on_ops(store, id, 1.0)?, "draw on"),
            _ => return Err(format!("inspect_animate: unknown action {action:?} (key, draw_on)")),
        };
        let version = narrative_apply(store, label, "human", ops)?;
        Ok(serde_json::json!({"ok": true, "version": version}))
    })
}

/// Nodes a mention can name (the `#` menu): live, named, not session or
/// derived, whose name contains `query` (case-insensitive), at most 20,
/// best first. `{"ok", "targets": [{"id", "name", "type", "composition"}]}`.
pub fn narrative_targets(app: &mut WorkerApp, query: String) -> String {
    narrative_run(app, |store| {
        let doc = store.document();
        let q = query.to_lowercase();
        let skip = ["block", "narrative", "session", "selection", "port_spec", "template", "override", "constraint", "contour", "point"];
        let mut hits: Vec<(usize, String, iron_document::NodeId)> = Vec::new();
        for node in doc.nodes() {
            let id = node.id;
            if id.is_session() || id.is_expanded() || node.tombstoned.is_some() || skip.contains(&node.ty.name()) {
                continue;
            }
            let Some(name) = doc.get::<iron_document::facet::Name>(id).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned))).filter(|n| !n.is_empty()) else { continue };
            let lower = name.to_lowercase();
            let Some(at) = lower.find(&q) else { continue };
            hits.push((at * 1000 + name.len(), name, id));
        }
        hits.sort();
        let targets: Vec<_> = hits
            .into_iter()
            .take(20)
            .map(|(_, name, id)| {
                let comp = iron_document::composition::composition_of(doc, id).and_then(|c| doc.get::<iron_document::facet::Name>(c).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned))));
                serde_json::json!({"id": id.to_string(), "name": name, "type": doc.live(id).map(|n| n.ty.name()), "composition": comp})
            })
            .collect();
        Ok(serde_json::json!({"ok": true, "targets": targets}))
    })
}

// ---- the Latency view (plans/rendering/latency-view.md) ---------------------

/// The Latency view's composition and square, for panel `panel`. Builds the
/// session composition on first use (`document_bridge::LatencyRequested`).
/// `fields` may hold `{"place": [x, y]}`: put the square's centre at that
/// panel-local CSS point (a session op, never in history). Answers
/// `{"ok", "ready", "composition", "target", "centre": [x, y]}`, the centre
/// in panel-local CSS px, or `ready: false` until the composition and the
/// panel exist.
pub fn document_latency(app: &mut WorkerApp, panel: String, fields: String) -> String {
    use iron_document::{Op, Slot, Value, facet::{Name, Transform2d}};
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let v: serde_json::Value = if fields.trim().is_empty() { serde_json::json!({}) } else {
        match serde_json::from_str(&fields) { Ok(v) => v, Err(e) => return fail(format!("latency: {e}")) }
    };
    if let Some(mut active) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active.remaining_frames = 10;
    }
    let Some(mut wanted) = app.world_mut().get_resource_mut::<crate::document_bridge::LatencyRequested>() else { return fail("no document bridge".into()) };
    wanted.0 = true;
    let dpr = app.world().get_resource::<crate::Dpr>().map_or(1.0, |d| d.0);
    let view = app.world().get_resource::<crate::document_bridge::PanelViews>().and_then(|v| v.get(&panel).cloned());
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    let doc = store.0.document();
    let Some(comp) = find_composition(doc, iron_document::standard::LATENCY).filter(|c| c.is_session()) else {
        return serde_json::json!({"ok": true, "ready": false}).to_string();
    };
    let Some(target) = doc.children(comp).iter().copied().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(iron_document::standard::LATENCY_TARGET)) else {
        return fail("latency composition has no target".into());
    };
    let Some(view) = view.filter(|v| v.composition == Some(comp)) else {
        return serde_json::json!({"ok": true, "ready": false, "composition": comp.to_string(), "target": target.to_string()}).to_string();
    };
    let half = iron_document::standard::LATENCY_TARGET_SIZE / 2.0;
    // Window px to the view's document px, and back (`PanelView::affine`).
    let affine = view.affine();
    if let Some(p) = v.get("place").and_then(|p| p.as_array()) {
        let (Some(x), Some(y)) = (p.first().and_then(|x| x.as_f64()), p.get(1).and_then(|y| y.as_f64())) else { return fail("place: [x, y]".into()) };
        let window = kurbo::Point::new(view.rect.x as f64 + x * dpr, view.rect.y as f64 + y * dpr);
        let d = affine.inverse() * window;
        let set = |path: &str, n: f64| Op::Set { id: target, path: path.parse().expect("declared leaf"), slot: Slot::Const(Value::Number(n)) };
        let ops = vec![set("transform2d.x", d.x - half), set("transform2d.y", d.y - half), set("transform2d.rot", 0.0)];
        if let Err(e) = store.0.apply_session(ops) {
            return fail(e.to_string());
        }
    }
    let doc = store.0.document();
    let t = doc.get::<Transform2d>(target);
    let leaf = |s: Option<&Slot>| s.and_then(|s| s.constant()).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let (tx, ty) = (leaf(t.map(|t| &t.x)), leaf(t.map(|t| &t.y)));
    let w = affine * kurbo::Point::new(tx + half, ty + half);
    let centre = [(w.x - view.rect.x as f64) / dpr, (w.y - view.rect.y as f64) / dpr];
    serde_json::json!({"ok": true, "ready": true, "composition": comp.to_string(), "target": target.to_string(), "centre": centre}).to_string()
}

/// The machine panel's data (machines.md §6): for the main `node` is in
/// (the main or one of its parts), or the instance `node` shows, the
/// component's machines: states, transitions with their tweens, which
/// transition each pair of states runs, the channels, strip or graph
/// layout. For an instance, also the state it is in now and the transition
/// it is in, if any. And the pin, wherever it is (for the canvas chip).
/// `{"ok", "view": {"def", "name", "main", "entity", "machines"} | null,
/// "pinned": {"def", "name", "machine", "state", "scrub"} | null}`.
/// A channel's value as the state editor shows it: a number, a colour as
/// `#rrggbbaa`, or words.
fn shown_value(v: &iron_document::Value) -> serde_json::Value {
    match v {
        iron_document::Value::Number(x) => serde_json::json!(x),
        iron_document::Value::Str(t) => serde_json::json!(t),
        other => serde_json::json!(other.to_string()),
    }
}

pub fn machine_view(app: &mut WorkerApp, node: String) -> String {
    // A read polled by the shell: no frames asked for (narrative_run asks).
    let run = |store: &iron_document::Store| -> Result<serde_json::Value, String> {
        use iron_document::{expand, machine, NodeId};
        let doc = store.document();
        let name_of = |n: NodeId| doc.get::<iron_document::facet::Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
        let read = |l: iron_document::Leaf| store.resolved(l.node, l.path);
        let text = |n: NodeId, p: &str| read(iron_document::Leaf { node: n, path: p.parse().expect("declared") }).and_then(|v| v.as_str().map(str::to_owned));
        let num = |n: NodeId, p: &str| read(iron_document::Leaf { node: n, path: p.parse().expect("declared") }).and_then(|v| v.as_f64());
        let kids = |n: NodeId, t: &str| doc.children(n).iter().copied().filter(|c| !c.is_expanded() && doc.live(*c).is_some_and(|x| x.ty.name() == t)).collect::<Vec<_>>();
        let pin = store.pinned_state().cloned();
        let pinned = pin.as_ref().map(|p| serde_json::json!({"def": p.def.to_string(), "name": name_of(p.def), "machine": p.machine.to_string(), "state": p.state, "scrub": p.scrub.as_ref().map(|(f, t, x)| serde_json::json!({"from": f, "to": t, "progress": x}))}));
        // Which component: the main the node is in, else the instance it shows.
        let node: Option<NodeId> = node.parse().ok().filter(|&n| doc.is_live(n));
        let main = node.map(|n| n.path()[0]).filter(|&m| expand::main_definition(doc, m).is_some());
        let entity = if main.is_some() { None } else {
            node.and_then(|n| if doc.live(n).is_some_and(|x| x.ty.name() == "entity") { Some(n) } else { iron_document::composition::figured_by(doc, n) })
        };
        // A definition named directly: the editor opened on a component.
        let named_def = node.filter(|&n| doc.live(n).is_some_and(|x| x.ty.name() == "definition"));
        let entity = if named_def.is_some() { None } else { entity };
        let def = named_def.or_else(|| main.and_then(|m| expand::main_definition(doc, m))).or_else(|| entity.and_then(|e| expand::linked_definition(doc, e)));
        let Some(def) = def else { return Ok(serde_json::json!({"ok": true, "view": null, "pinned": pinned, "shared": store.shared_edits()})) };
        let now = store.time();
        let machines: Vec<serde_json::Value> = machine::machines(doc, def)
            .into_iter()
            .map(|(m, port)| {
                let states: Vec<String> = machine::state_names(doc, m);
                let transitions: Vec<_> = kids(m, "transition").into_iter().map(|t| {
                    let tweens: Vec<_> = kids(t, "tween").into_iter().map(|w| serde_json::json!({
                        "id": w.to_string(), "channel": text(w, "tween.channel"), "start": num(w, "tween.start"), "end": num(w, "tween.end"), "ease": text(w, "tween.ease"),
                        "shaped": matches!(read(iron_document::Leaf { node: w, path: "tween.shape".parse().expect("declared") }), Some(iron_document::Value::Fn(_))),
                    })).collect();
                    serde_json::json!({"id": t.to_string(), "name": name_of(t), "from": text(t, "transition.from"), "to": text(t, "transition.to"), "ms": num(t, "transition.ms"), "ease": text(t, "transition.ease"), "tweens": tweens,
                        "ms_formula": doc.leaf(t, "transition.ms".parse().expect("declared")).is_some_and(|s| s.expr().is_some())})
                }).collect();
                let mc = machine::copy_of(doc, entity, m);
                let mut pairs = Vec::new();
                for a in &states {
                    for b in states.iter().filter(|b| *b != a) {
                        let p = machine::plan(doc, &read, def, mc, a, b);
                        pairs.push(serde_json::json!({"from": a, "to": b, "transition": p.as_ref().map(|p| machine::copy_source(p.transition).to_string()), "ms": p.as_ref().map(|p| p.ms)}));
                    }
                }
                // Each state's pose: a value per channel, and which it sets
                // itself (the rest are the channel's rest value).
                let state_ids: Vec<_> = kids(m, "state").into_iter().map(|s| {
                    let name = name_of(s).unwrap_or_default();
                    let pose = machine::pose(doc, &read, def, m, &name).unwrap_or_default();
                    let own: Vec<String> = doc.get::<iron_document::facet::Args>(s).map(|a| a.slots.keys().map(|k| k.to_string()).collect()).unwrap_or_default();
                    let values: serde_json::Map<String, serde_json::Value> = machine::channels(doc, def).into_iter().map(|(c, n, _)| (n, pose.get(&c).map_or(serde_json::Value::Null, shown_value))).collect();
                    serde_json::json!({"id": s.to_string(), "name": name_of(s), "pose": values, "own": own})
                }).collect();
                let current = entity.map(|e| {
                    let leaf = iron_document::Leaf { node: e, path: iron_document::facet::LeafPath::new(iron_document::FacetKind::Args, &port).expect("an argument path") };
                    let state = read(leaf).and_then(|v| v.as_str().map(str::to_owned));
                    let run = machine_running(store, &read, def, m, e, leaf, now);
                    serde_json::json!({"state": state, "transition": run.as_ref().map(|r| r.0.to_string()), "progress": run.as_ref().map(|r| r.1), "from": run.as_ref().map(|r| r.2.clone()), "to": run.as_ref().map(|r| r.3.clone())})
                });
                serde_json::json!({
                    "id": m.to_string(), "port": port, "states": state_ids, "transitions": transitions, "pairs": pairs,
                    "layout": if machine::is_strip(doc, &read, m) { "strip" } else { "graph" },
                    "order": machine::strip_order(doc, &read, m),
                    "pinned": pin.as_ref().filter(|p| p.machine == m).map(|p| p.state.clone()),
                    "default": doc.children(def).iter().copied().find(|&c| doc.get::<iron_document::facet::Port>(c).is_some() && name_of(c).as_deref() == Some(port.as_str())).and_then(|c| doc.get::<iron_document::facet::Port>(c).and_then(|p| p.signal.constant()).and_then(|v| v.as_str()).map(str::to_owned)),
                    "current": current,
                })
            })
            .collect();
        // The component's main, for "Edit main" from an instance.
        let main_of = doc.nodes_of_type(iron_document::TypeId::lookup("use").expect("registered")).find(|&u| !u.is_expanded() && doc.is_live(u) && expand::main_definition(doc, u) == Some(def));
        // Each channel with the parts that read it (the editor's rows are
        // parts and their properties): the part as the main shows it.
        let readers = machine::readers(doc, def);
        let channels: Vec<_> = machine::channels(doc, def).into_iter().map(|(c, n, rest)| {
            let kind = match rest { iron_document::Value::Color(_) => "color", iron_document::Value::Str(_) => "text", _ => "number" };
            let parts: Vec<_> = readers.get(&c).into_iter().flatten().map(|(p, path)| serde_json::json!({
                "node": p.to_string(), "shown": main_of.and_then(|u| NodeId::expanded(u, *p)).map(|s| s.to_string()), "name": name_of(*p).unwrap_or_else(|| doc.live(*p).map_or("part", |x| x.ty.name()).to_owned()), "path": path.to_string(),
            })).collect();
            serde_json::json!({"id": c.to_string(), "name": n, "rest": shown_value(&rest), "kind": kind, "parts": parts})
        }).collect();
        Ok(serde_json::json!({"ok": true, "pinned": pinned, "shared": store.shared_edits(), "view": {
            "def": def.to_string(), "name": name_of(def), "main": main.map(|m| m.to_string()), "entity": entity.map(|e| e.to_string()), "machines": machines, "channels": channels, "main_of": main_of.map(|u| u.to_string()),
        }}))
    };
    let result = match app.world().get_resource::<crate::document_bridge::DocumentStore>() {
        Some(store) => run(&store.0),
        None => Err("document bridge not installed".into()),
    };
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// The transition an instance's machine `m` (a definition's) is going
/// through now: the keyed one the clock drives, else one an event started
/// (the player's last advance). (the definition's transition, progress,
/// from, to).
fn machine_running(store: &iron_document::Store, read: &dyn Fn(iron_document::Leaf) -> Option<iron_document::Value>, def: iron_document::NodeId, m: iron_document::NodeId, entity: iron_document::NodeId, leaf: iron_document::Leaf, now: f64) -> Option<(iron_document::NodeId, f64, String, String)> {
    use iron_document::machine;
    let doc = store.document();
    if let Some((t, p, f, g)) = machine::progress_at(doc, read, def, machine::copy_of(doc, Some(entity), m), leaf, now) {
        return Some((machine::copy_source(t), p, f, g));
    }
    store.running_machines().iter().find_map(|r| {
        let t = machine::copy_source(r.transition);
        (r.entity == entity && doc.live(t).and_then(|n| n.parent) == Some(m)).then(|| (t, r.progress, r.from.clone(), r.to.clone()))
    })
}

/// What a key on a state lane stands for (the timeline's double-click,
/// machines.md §6): the component, machine and instance, the state it
/// keys and the one before it. `{"ok", "def", "machine", "entity", "state",
/// "from"}`, or `{"ok": false}` when it keys no machine's state.
pub fn state_key(app: &mut WorkerApp, key: String) -> String {
    let run = |store: &iron_document::Store| -> Result<serde_json::Value, String> {
        use iron_document::{machine, NodeId};
        let doc = store.document();
        let key: NodeId = key.parse().map_err(|_| format!("not a node id: {key}"))?;
        let clip = doc.live(key).and_then(|n| n.parent).ok_or("not a key")?;
        let prop = doc.get::<iron_document::facet::Clip>(clip).and_then(|c| c.prop.constant()).and_then(|v| v.as_str()).map(str::to_owned).ok_or("not a clip's key")?;
        let port = prop.strip_prefix("args.").ok_or("not a state key")?.to_owned();
        let entity = doc.relations_from(clip).find(|r| r.rel == iron_document::RelKind::Animates).map(|r| r.to).ok_or("animates nothing")?;
        let def = iron_document::expand::linked_definition(doc, entity).ok_or("not an instance")?;
        let (m, _) = machine::machines(doc, def).into_iter().find(|(_, p)| *p == port).ok_or("no machine follows it")?;
        let keys = iron_document::time::keys(doc, clip);
        let i = keys.iter().position(|k| k.id == key).ok_or("not in its clip")?;
        let word = |v: &iron_document::Value| v.as_str().map(str::to_owned);
        let state = word(&keys[i].value);
        let from = if i > 0 { word(&keys[i - 1].value) } else { doc.leaf(entity, iron_document::facet::LeafPath::new(iron_document::FacetKind::Args, &port).map_err(|e| e.to_string())?).and_then(|s| s.constant()).and_then(word) };
        let entity_name = doc.get::<iron_document::facet::Name>(entity).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
        Ok(serde_json::json!({"ok": true, "def": def.to_string(), "machine": m.to_string(), "entity": entity.to_string(), "entity_name": entity_name, "state": state, "from": from.filter(|f| Some(f) != state.as_ref())}))
    };
    let result = match app.world().get_resource::<crate::document_bridge::DocumentStore>() {
        Some(store) => run(&store.0),
        None => Err("document bridge not installed".into()),
    };
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
}

/// The workspace's view selector strip (panel-primitives.md): the views
/// `std:workspace` (or a document's override of it) offers, in order.
/// `{"ok", "strip": ["layers", "model", "composition:library", …]}`.
pub fn workspace_strip(app: &mut WorkerApp) -> String {
    let result = match app.world().get_resource::<crate::document_bridge::DocumentStore>() {
        Some(store) => {
            let r = iron_document::contexts::Resolver::new(store.0.document(), store.0.contexts());
            let strip = r.workspace_values().get(iron_document::contexts::Field::STRIP).to_owned();
            serde_json::json!({"ok": true, "strip": strip.split_whitespace().collect::<Vec<_>>()})
        }
        None => serde_json::json!({"ok": false, "error": "document bridge not installed"}),
    };
    result.to_string()
}

/// Request/poll a content-only GPU picture. These only mutate session resources.
pub fn document_picture(app: &mut WorkerApp, source: String) -> String {
    match crate::document_bridge::capture::start(app.world_mut(), &source) {
        Ok(ticket) => serde_json::json!({"ok": true, "ticket": ticket}),
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    }.to_string()
}
pub fn document_picture_poll(app: &mut WorkerApp, ticket: u64, cancel: bool) -> String {
    crate::document_bridge::capture::poll(app.world_mut(), ticket, cancel).to_string()
}
