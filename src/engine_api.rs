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
    let doc = iron_document::serial::load(&text).map_err(|e| String::from(&e.to_string()))?;
    let mut load = app
        .world_mut()
        .get_resource_mut::<crate::document_bridge::PendingLoad>()
        .ok_or_else(|| String::from("document bridge not installed"))?;
    load.0 = Some(doc);
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    Ok(())
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

pub fn document_timings(_app: &mut WorkerApp, reset: bool) -> String {
    crate::perf::report(reset).to_string()
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
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("document bridge not installed")?;
        let doc = store.0.document();
        let (label, ops, made): (String, Vec<iron_document::Op>, Option<iron_document::NodeId>) = match verb.as_str() {
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
            other => return Err(format!("no authoring verb {other:?}")),
        };
        let applied = store.0
            .apply(iron_document::TransactionInput { label, actor: iron_document::Actor::Human, ts: crate::host_services::unix_millis(), idempotency_key: None, ops, exact: false })
            .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
        Ok(serde_json::json!({"ok": true, "version": applied.version.0, "made": made.map(|m| m.to_string())}))
    };
    let result = run();
    if let Some(mut activity) = app.world_mut().get_resource_mut::<ActivityControl>() { activity.remaining_frames = 10; }
    result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error})).to_string()
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
    let imported = match iron_svg::import(&text) {
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
    serde_json::json!({"ok": true, "cuts": imported.cuts, "nodes": imported.nodes}).to_string()
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
                        Handle::Linear { source, .. } => ("via", Some(source.to_string()), None),
                        Handle::Locked(reason) => ("locked", None, Some(reason)),
                    };
                    Some(serde_json::json!({
                        "leaf": format!("{}.{field}", kind.name()),
                        "field": field,
                        "kind": value.as_ref().map(|v| format!("{:?}", v.kind()).to_lowercase()),
                        "text": value.as_ref().map(|v| v.to_string()),
                        "bind": slot.expr().map(|e| e.to_string()),
                        "edit": edit,
                        "via": via,
                        "why": why,
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
    let main = inspect_main(&store.0, id);
    let port = inspect_port(&store.0, id);
    serde_json::json!({"node": id.to_string(), "type": n.ty.name(), "name": name, "components": components, "addable": addable, "removable": removable, "component": component, "main": main, "port": port}).to_string()
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
    serde_json::json!({"def": def.to_string(), "name": name, "ports": ports, "instances": instances})
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
                (None, Handle::Linear { source, .. }) => ("via", Some(source.to_string()), None),
                (None, Handle::Locked(reason)) => ("locked", None, Some(reason)),
            };
            Some(serde_json::json!({
                "leaf": format!("args.{}", input.name),
                "field": input.name,
                "kind": value.as_ref().map(|v| format!("{:?}", v.kind()).to_lowercase()),
                "text": value.as_ref().map(|v| v.to_string()),
                "bind": slot.expr().map(|e| e.to_string()),
                "edit": edit,
                "via": via,
                "why": why,
            }))
        })
        .collect();
    let linked = iron_document::expand::linked_definition(doc, entity).is_some();
    serde_json::json!({"entity": entity.to_string(), "name": name_of(entity), "def": name_of(def), "linked": linked, "inputs": fields})
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

fn inspector_current(store: &iron_document::Store, id: iron_document::NodeId, path: iron_document::LeafPath) -> Option<iron_document::Value> {
    store.resolved(id, path).or_else(|| {
        let node = store.document().live(id)?;
        if !node.ty.spec().allows(path.facet) || !INSPECTOR_OPTIONAL.contains(&path.facet.name()) { return None; }
        path.facet.default_facet().get(path.field).and_then(|s| s.constant()).cloned()
    })
}

fn inspector_value(current: &iron_document::Value, text: &str) -> Result<iron_document::Value, String> {
    use iron_document::Value;
    let t = text.trim();
    let nums = |n: usize| -> Option<Vec<f64>> {
        let v: Vec<f64> = t.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
        (v.len() == n && v.iter().all(|v| v.is_finite())).then_some(v)
    };
    let value = match current {
        Value::Number(_) => t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Value::Number),
        Value::Bool(_) => t.parse().ok().map(Value::Bool),
        Value::Str(_) => Some(Value::Str(t.to_owned())),
        Value::Enum(_) => Some(Value::Enum(t.to_owned())),
        Value::Vec2(_) => nums(2).map(|v| Value::Vec2([v[0], v[1]])),
        Value::Vec3(_) => nums(3).map(|v| Value::Vec3([v[0], v[1], v[2]])),
        Value::Color(c) => {
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

pub fn document_inspect_patch(app: &mut WorkerApp, node: String, patch: String) -> String {
    use iron_document::{Actor, FacetKind, Leaf, LeafPath, NodeId, Op, Slot, Value};
    use std::collections::BTreeMap;
    let mut run = || -> Result<serde_json::Value, String> {
        let id = node.parse::<NodeId>().map_err(|e| e.to_string())?;
        let patch: serde_json::Value = serde_json::from_str(&patch).map_err(|e| e.to_string())?;
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("no store")?;
        let mut writes: BTreeMap<Leaf, Value> = BTreeMap::new();
        if let Some(values) = patch.get("values").and_then(|v| v.as_object()) {
            for (leaf, text) in values {
                let path = leaf.parse::<LeafPath>().map_err(|e| e.to_string())?;
                let current = inspector_current(&store.0, id, path).ok_or_else(|| format!("no property {id}.{path}"))?;
                let value = inspector_value(&current, text.as_str().ok_or("a property value must be text")?)?;
                let (target, value) = iron_document::handle::resolve_target(store.0.document(), store.0.reactive(), Leaf { node: id, path }, value)?;
                if let Some(prior) = writes.insert(target, value.clone()) && prior != value {
                    return Err(format!("linked fields require conflicting values of {target}"));
                }
            }
        }
        let mut adds = BTreeMap::new();
        let mut ops = Vec::new();
        for (target, value) in writes {
            if store.0.document().facet(target.node, target.path.facet).is_none() {
                let comp = adds.entry((target.node, target.path.facet)).or_insert_with(|| target.path.facet.default_facet());
                comp.set(target.path.field, Slot::Const(value)).map_err(|e| e.to_string())?;
            } else if store.0.document().leaf(target.node, target.path).and_then(|s| s.constant()) != Some(&value) {
                ops.push(Op::Set { id: target.node, path: target.path, slot: Slot::Const(value) });
            }
        }
        let mut added: Vec<_> = adds.into_iter().map(|((id, _), comp)| Op::AddFacet { id, facet: comp }).collect();
        added.append(&mut ops);
        if let Some(remove) = patch.get("remove").and_then(|v| v.as_array()) {
            for kind in remove {
                let kind = kind.as_str().and_then(FacetKind::from_name).filter(|k| INSPECTOR_OPTIONAL.contains(&k.name())).ok_or("not an optional inspector component")?;
                if store.0.document().facet(id, kind).is_some() { added.push(Op::RemoveFacet { id, kind }); }
            }
        }
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
        let id = node.parse::<iron_document::NodeId>().map_err(|e| e.to_string())?;
        let options: serde_json::Value = serde_json::from_str(options.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())?;
        let leaves: Vec<String> = if let Some(leaves) = options.get("leaves") { serde_json::from_value(leaves.clone()).map_err(|e| e.to_string())? } else { vec![leaf.clone()] };
        if leaves.is_empty() { return Err("a scrub needs a property".into()); }
        let paths = leaves.iter().map(|l| l.parse::<iron_document::LeafPath>().map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let alpha = options.get("channel").and_then(|v| v.as_str()) == Some("alpha");
        let mut store = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>().ok_or("no store")?;
        if phase == "begin" {
            for &path in &paths {
                let current = inspector_current(&store.0, id, path);
                if !(matches!(current, Some(Value::Number(_))) && !alpha || matches!(current, Some(Value::Color(_))) && alpha) { return Err("this property cannot be scrubbed as a number".into()); }
                if let Handle::Locked(why) = store.0.handle(id, path) { return Err(why); }
            }
            let gesture = store.0.begin_gesture(&[id]);
            return Ok(serde_json::json!({"ok": true, "token": gesture.0}));
        }
        if store.0.active_gesture() != Some(GestureId(token as u64)) { return Err("the gesture ended or was cancelled by another edit".into()); }
        match phase.as_str() {
            "move" => {
                if !value.is_finite() { return Err("value must be finite".into()); }
                for path in paths {
                    let next = if alpha {
                        let Some(Value::Color(mut color)) = inspector_current(&store.0, id, path) else { return Err("no colour".into()); };
                        color[3] = value.clamp(0.0, 1.0) as f32;
                        Value::Color(color)
                    } else { Value::Number(value) };
                    store.0.preview_target(id, path, next)?;
                }
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
            serde_json::json!({"id": c.to_string(), "name": name, "dims": dims})
        })
        .collect();
    serde_json::Value::Array(list).to_string()
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

pub fn undo(app: &mut WorkerApp) {
    history_request(app, crate::document_bridge::HistoryOp::Undo);
}

pub fn redo(app: &mut WorkerApp) {
    history_request(app, crate::document_bridge::HistoryOp::Redo);
}

/// Undo and redo travel with the other shortcuts (`ToolKeys`), so they
/// apply in the order the keys were pressed: a tool's transaction from an
/// earlier key reaches `PendingTransactions` when the tool handles it, and
/// an undo queued straight into `HistoryRequests` would overtake it on a
/// slow frame (undoing the previous change instead).
fn history_request(app: &mut WorkerApp, op: crate::document_bridge::HistoryOp) {
    if let Some(mut keys) = app.world_mut().get_resource_mut::<crate::bevy_app::select::ToolKeys>() {
        let key = match op {
            crate::document_bridge::HistoryOp::Undo => "undo",
            crate::document_bridge::HistoryOp::Redo => "redo",
        };
        keys.0.push((key.to_owned(), 0));
    }
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
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

pub fn clipboard_paste(app: &mut WorkerApp, text: String) {
    if let Some(mut clip) = app.world_mut().get_resource_mut::<crate::bevy_app::select::Clipboard>() {
        clip.pastes.push(text);
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

const INSPECTOR_OPTIONAL: &[&str] = &["fill", "stroke", "corners", "arc", "shadow", "appearance", "gradient", "group_clip"];

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
    use iron_document::{Actor, FacetKind, LeafStruct, NodeId, Op, Slot, TransactionInput, Value};
    let fail = |e: String| serde_json::json!({"ok": false, "error": e}).to_string();
    let Ok(id) = node.parse::<NodeId>() else { return fail(format!("bad id {node:?}")) };
    let patch: iron_text::RunStyle = match serde_json::from_str(&patch) {
        Ok(p) => p,
        Err(e) => return fail(e.to_string()),
    };
    if let Some(mut active_info) = app.world_mut().get_resource_mut::<ActivityControl>() {
        active_info.remaining_frames = 10;
    }
    if let Some(mut tool) = app.world_mut().get_resource_mut::<crate::bevy_app::text_tool::TextTool>()
        && tool.editing() == Some(id)
        && tool.has_range()
    {
        tool.inputs.push(crate::bevy_app::text_tool::TextInput::Style(patch));
        return serde_json::json!({"ok": true, "range": true}).to_string();
    }
    let Some(mut store) = app.world_mut().get_resource_mut::<crate::document_bridge::DocumentStore>() else { return fail("no store".into()) };
    if store.0.document().live(id).is_none_or(|n| n.ty.name() != "text") {
        return fail(format!("{id} is not text"));
    }
    let leaf = |p: &str| p.parse::<iron_document::LeafPath>().expect("declared leaf");
    let mut ops = Vec::new();
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
