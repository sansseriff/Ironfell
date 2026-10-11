//! Selection, hover, and drag.
//!
//! A drag is an intent (plans/document-spine.md §10) carried by the store's
//! gesture overlay: every frame the pointer's position is previewed through
//! `Store::preview_target`, the bridge reconciles the affected nodes before
//! transforms propagate (`document_bridge::reconcile_gesture`), and one
//! labelled transaction closes the gesture on release. Nothing here writes a
//! Bevy `Transform`; everything the drag moves, including nodes bound to the
//! dragged one, moves through the store in the same frame (roadmap stage A,
//! constraint-system.md §8.2).
//!
//! The store decides what a 2D drag may do (`Store::begin_drag`): a node in
//! a constraint cluster is dragged by a finger and the cluster re-solves
//! every frame; otherwise each axis previews through its own handle (an
//! authored constant directly, a linearly bound leaf through its source),
//! and a locked axis holds still for the whole gesture rather than moving
//! and snapping back. The reasons are logged at press.

use bevy::prelude::*;
use iron_document::{DragMode, Handle, LeafPath, Value};

use crate::bevy_app::picking::panel_camera;
use crate::bevy_app::scene3d::{ActiveState, PanelCamera};
use crate::document_bridge::{Doc2d, DocumentStore, GestureRequest, GestureRequests, NodeMap, PanelViews, Provenance, Shape2d, type_of};
use crate::panels::document_from_screen;

/// The entity whose drag just ended, awaiting writeback.
#[derive(Resource, Default)]
pub struct PendingCommit(pub Option<Entity>);

fn leaf(s: &str) -> LeafPath {
    s.parse().expect("declared leaf")
}

/// Whether the leaf has a handle now; the reason is logged when it does not,
/// which is the "why can't I move this" answer until the inspector shows it.
fn locked(store: &DocumentStore, id: iron_document::NodeId, path: LeafPath) -> bool {
    match store.0.handle(id, path) {
        Handle::Locked(reason) => {
            info!("{id}.{path} locked for this drag: {reason}");
            true
        }
        _ => false,
    }
}

// Decide drag start/stop and update selection based on pointer hits.
pub fn interaction_decide_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    mut drag: ResMut<crate::DragState>,
    mut selection: ResMut<crate::SelectionState>,
    mut commit: ResMut<PendingCommit>,
    cameras: Query<(&Camera, &GlobalTransform, &PanelCamera)>,
    transforms: Query<&GlobalTransform>,
    doc2d: Query<&Doc2d>,
    provenance: Query<&Provenance>,
    mut store: ResMut<DocumentStore>,
    views: Res<PanelViews>,
) {
    // Hover follows the primary hit. Written only on change so downstream
    // change detection (and the outbound FFI) stays quiet while hovering.
    let hovered_now: Option<Entity> = hits.primary;
    let hovered_before = selection.hovered.keys().next().copied();
    if hovered_now != hovered_before {
        selection.hovered.clear();
        if let Some(e) = hovered_now {
            selection.hovered.insert(e, ());
        }
    }

    // Drag end: the finger lifts now; the commit follows next sync.
    if pointer.just_released_left && drag.target.is_some() {
        commit.0 = drag.target;
        drag.target = None;
        drag.kind = None;
        if let Some(h) = drag.handle.take() {
            store.0.end_drag(&h);
        }
    }

    // Drag begin or click selection start
    if pointer.just_pressed_left {
        if let Some(primary) = hits.primary {
            selection.selected.clear();
            selection.selected.insert(primary, ());
            selection.last_primary = Some(primary);
            drag.target = Some(primary);
            drag.handle = None;
            drag.panel = hits.panel.clone();
            let Ok(prov) = provenance.get(primary) else {
                drag.target = None;
                return;
            };
            let id = prov.0;

            if doc2d.get(primary).is_ok_and(|d| matches!(d.shape, Shape2d::Slider { .. })) {
                // Scrubbing the value, not moving the control. The first
                // preview happens in drag_apply this same frame.
                drag.kind = Some(crate::DragKind::SliderValue);
            } else if doc2d.contains(primary) {
                // The grab point in the node's local space is the anchor the
                // store drags by (a finger on it, for a constrained node).
                let p = drag
                    .panel
                    .as_deref()
                    .and_then(|panel| views.get(panel))
                    .and_then(|v| document_from_screen(v.rect, pointer.screen))
                    .unwrap_or(Vec2::ZERO);
                let local = transforms
                    .get(primary)
                    .map(|g| g.affine().inverse().transform_point3(p.extend(0.0)))
                    .unwrap_or(Vec3::ZERO);
                let h = store.0.begin_drag(id, [local.x as f64, local.y as f64]);
                match &h.mode {
                    DragMode::Locked(reasons) => {
                        for r in reasons {
                            info!("{id} locked for this drag: {r}");
                        }
                        drag.kind = None;
                        drag.target = None;
                    }
                    DragMode::Axes { locked, .. } => {
                        for r in locked {
                            info!("{id} locked for this drag: {r}");
                        }
                        drag.kind = Some(crate::DragKind::Overlay2D);
                        drag.handle = Some(h);
                    }
                    DragMode::Cluster(cluster) => {
                        info!("{id} dragged through cluster {cluster}");
                        drag.kind = Some(crate::DragKind::Overlay2D);
                        drag.handle = Some(h);
                    }
                }
            } else if locked(&store, id, leaf("transform3d.pos")) {
                drag.kind = None;
                drag.target = None;
            } else {
                drag.kind = Some(crate::DragKind::World3D);
                // Establish drag plane for 3D: if ctrl held -> fixed XZ plane (normal Y).
                // Otherwise plane passes through object and is camera-facing (normal = camera forward).
                if let Some((camera, cam_tf)) = panel_camera(&cameras, drag.panel.as_deref()) {
                    let cam_forward = cam_tf.forward().as_vec3();
                    let plane_normal = if pointer.modifiers.ctrl { Vec3::Y } else { cam_forward };
                    drag.plane_normal = plane_normal.normalize_or_zero();
                    if let Ok(ent_tf) = transforms.get(primary) {
                        drag.plane_origin = ent_tf.translation();
                    } else {
                        drag.plane_origin = cam_tf.translation();
                    }
                    if let Some(ray) =
                        crate::bevy_app::picking::camera_ray_from_window_px(camera, cam_tf, pointer.screen)
                    {
                        if let Some(hit_pos) = intersect_ray_plane(ray, drag.plane_origin, drag.plane_normal) {
                            if let Ok(ent_tf) = transforms.get(primary) {
                                drag.grab_offset_world = ent_tf.translation() - hit_pos;
                            } else {
                                drag.grab_offset_world = Vec3::ZERO;
                            }
                            drag.plane_origin = hit_pos; // better stability when camera plane
                        } else {
                            drag.grab_offset_world = Vec3::ZERO;
                        }
                    }
                }
            }
        } else {
            selection.selected.clear();
            selection.last_primary = None;
        }
    }
}

/// Follow the pointer by previewing the dragged node's leaves in the store.
/// The gesture reconcile pass that runs right after this rewrites the
/// entity (and everything bound to it) before transforms propagate, so the
/// view keeps up with the pointer with no frame of lag.
pub fn drag_apply_system(
    pointer: Res<crate::PointerState>,
    drag: Res<crate::DragState>,
    globals: Query<&GlobalTransform>,
    nodes: Query<(&Provenance, Option<&Doc2d>)>,
    mut store: ResMut<DocumentStore>,
    cameras: Query<(&Camera, &GlobalTransform, &PanelCamera)>,
    views: Res<PanelViews>,
) {
    let Some(entity) = drag.target else { return };
    // The pointer in the drag's panel's document space. Outside the panel
    // the drag holds still rather than jumping.
    let doc_point = || drag.panel.as_deref().and_then(|p| views.get(p)).and_then(|v| document_from_screen(v.rect, pointer.screen));
    let Ok((prov, doc2d)) = nodes.get(entity) else { return };
    let id = prov.0;
    let target = |store: &mut DocumentStore, path: &str, value: Value| {
        if let Err(reason) = store.0.preview_target(id, leaf(path), value) {
            // Handles are read at press time, so this only happens if the
            // document changed under the drag (a model edit mid-gesture).
            warn!("{id}.{path}: {reason}");
        }
    };
    match drag.kind {
        Some(crate::DragKind::SliderValue) => {
            // Pointer x along the track, in the slider's own space, becomes a
            // value previewed through the store; anything bound to it follows
            // in the same frame, and nothing is written until release.
            let Some(p) = doc_point() else {
                return;
            };
            let Some(Shape2d::Slider { w, .. }) = doc2d.map(|d| d.shape) else { return };
            let Ok(global) = globals.get(entity) else { return };
            let local = global.affine().inverse().transform_point3(p.extend(0.0));
            let t = (local.x / w.max(1.0)).clamp(0.0, 1.0) as f64;
            let num = |s: &str, d: f64| store.0.resolved(id, leaf(s)).and_then(|v| v.as_f64()).unwrap_or(d);
            let (min, max, step) = (num("slider.min", 0.0), num("slider.max", 1.0), num("slider.step", 0.0));
            let mut value = min + t * (max - min);
            if step > 0.0 {
                value = min + ((value - min) / step).round() * step;
            }
            let value = value.clamp(min.min(max), max.max(min));
            if num("slider.value", f64::NAN) != value {
                store.0.preview(id, leaf("slider.value"), Value::Number(value));
            }
        }
        Some(crate::DragKind::Overlay2D) => {
            let Some(p) = doc_point() else {
                return;
            };
            let Some(h) = &drag.handle else { return };
            // The store puts the grab point under the pointer, through the
            // cluster solve or the axis handles; everything moved is
            // previewed and reconciled this frame.
            let t0 = bevy::platform::time::Instant::now();
            let report = store.0.drag_to(h, [p.x as f64, p.y as f64]);
            if let DragMode::Cluster(cluster) = &h.mode {
                // CS phase 3 exit 9: the budget is 2 ms per frame.
                let ms = t0.elapsed().as_secs_f64() * 1e3;
                info!(
                    "solve: cluster {cluster}, {} vars, {} iterations, {} evaluations, {ms:.2} ms{}",
                    report.vars,
                    report.iterations,
                    report.evaluations,
                    if report.converged { "" } else { "; not converged, pose kept" }
                );
            }
            let _ = target;
        }
        Some(crate::DragKind::World3D) => {
            let Some((camera, cam_tf)) = panel_camera(&cameras, drag.panel.as_deref()) else { return };
            let Some(ray) = crate::bevy_app::picking::camera_ray_from_window_px(camera, cam_tf, pointer.screen)
            else {
                return;
            };
            if let Some(hit_pos) = intersect_ray_plane(ray, drag.plane_origin, drag.plane_normal) {
                let p = hit_pos + drag.grab_offset_world;
                target(&mut store, "transform3d.pos", Value::Vec3([p.x as f64, p.y as f64, p.z as f64]));
            }
        }
        _ => {}
    }
}

/// Close the gesture: the store turns its previews into one transaction
/// (or none, when nothing moved, so a click is not a "move"). The label is
/// in the user's vocabulary: "move rect", "set slider".
pub fn drag_commit_system(
    mut commit: ResMut<PendingCommit>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    mut gestures: ResMut<GestureRequests>,
    query: Query<Option<&Doc2d>>,
) {
    let Some(entity) = commit.0.take() else { return };
    let Ok(doc2d) = query.get(entity) else { return };
    let Some(ty) = type_of(&store.0, &map, entity) else { return };
    let verb = if doc2d.is_some_and(|d| matches!(d.shape, Shape2d::Slider { .. })) { "set" } else { "move" };
    gestures.0.push(GestureRequest::Commit(format!("{verb} {ty}")));
}

// Utility: ray-plane intersection (plane defined by point & normal). Returns world hit.
fn intersect_ray_plane(ray: Ray3d, plane_point: Vec3, plane_normal: Vec3) -> Option<Vec3> {
    let denom = ray.direction.dot(plane_normal);
    if denom.abs() < 1e-5 {
        return None;
    }
    let t = (plane_point - ray.origin).dot(plane_normal) / denom;
    if t < 0.0 {
        return None;
    }
    Some(ray.origin + ray.direction * t)
}

// Reflect selection & hover state into ActiveState components for rendering outlines.
pub fn selection_reflect_system(
    selection: Res<crate::SelectionState>,
    mut query: Query<(Entity, &mut ActiveState)>,
) {
    if !selection.is_changed() {
        return;
    }
    for (entity, mut active) in &mut query {
        active.selected = selection.selected.contains_key(&entity);
        active.hover = selection.hovered.contains_key(&entity);
    }
}

// Outbound notification systems (hover & selection) – convert sets to js_sys::Array and call externs.
pub fn outbound_hover_system(selection: Res<crate::SelectionState>) {
    if !selection.is_changed() {
        return;
    }
    let arr = js_sys::Array::new();
    for (entity, _) in selection.hovered.iter() {
        arr.push(&wasm_bindgen::JsValue::from(entity.to_bits()));
    }
    crate::web_ffi::send_hover_from_worker(arr);
}

pub fn outbound_selection_system(selection: Res<crate::SelectionState>) {
    if !selection.is_changed() {
        return;
    }
    let arr = js_sys::Array::new();
    for (entity, _) in selection.selected.iter() {
        arr.push(&wasm_bindgen::JsValue::from(entity.to_bits()));
    }
    crate::web_ffi::send_selection_from_worker(arr);
}
