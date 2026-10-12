//! The drags the select tool (`select`) hands off: meshes in 3D, content on
//! planes, sliders. Selection and the 2D box live in `select`.
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
use iron_document::{DragMode, LeafPath, Value};

use crate::bevy_app::scene3d::ActiveState;
use crate::document_bridge::{Doc2d, DocumentStore, GestureRequest, GestureRequests, NodeMap, PanelViews, Provenance, Shape2d, type_of};

pub(crate) type DragCameras<'w, 's> = Query<'w, 's, (Entity, &'static Camera, &'static GlobalTransform, Option<&'static crate::bevy_app::scene3d::ScreenRect>, Option<&'static crate::bevy_app::scene3d::PointerToCamera>)>;

/// The pointer as document px in the dragged node's composition: through the
/// panel and any embeds (an affine), or through a plane in 3D (a ray from
/// the camera that showed it, meeting the plane).
pub(crate) fn drag_point(
    drag: &crate::DragState,
    views: &PanelViews,
    screen: Vec2,
    cameras: &DragCameras,
    planes: &Query<(&GlobalTransform, &crate::document_bridge::planes::EmbedPlane)>,
) -> Option<Vec2> {
    if let Some((camera, plane)) = drag.through {
        let (_, cam, cam_tf, rect, to_camera) = cameras.get(camera).ok()?;
        let local = crate::bevy_app::scene3d::to_camera_px(screen, rect, to_camera);
        let ray = crate::bevy_app::picking::camera_ray_from_window_px(cam, cam_tf, local)?;
        let (pt, p) = planes.get(plane).ok()?;
        let (_, q) = crate::bevy_app::picking::ray_plane_doc(ray, pt, p)?;
        return Some(Vec2::new(q.x as f32, q.y as f32));
    }
    let p = drag.panel.as_deref().and_then(|p| views.get(p)).and_then(|v| v.to_doc(screen))?;
    let q = drag.space * kurbo::Point::new(p.x as f64, p.y as f64);
    Some(Vec2::new(q.x as f32, q.y as f32))
}

/// How far (window px) the pointer may wander between press and release and
/// still count as a click.
const CLICK_SLOP_PX: f32 = 3.0;

/// The entity whose drag just ended, awaiting writeback.
#[derive(Resource, Default)]
pub struct PendingCommit(pub Option<Entity>);

fn leaf(s: &str) -> LeafPath {
    s.parse().expect("declared leaf")
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
    cameras: DragCameras,
    views: Res<PanelViews>,
    planes: Query<(&GlobalTransform, &crate::document_bridge::planes::EmbedPlane)>,
) {
    let Some(entity) = drag.target else { return };
    // A click (selection, shift-click to add) moves nothing: positions wait
    // until the pointer has travelled a few pixels from the press.
    let travelled = pointer.screen.distance(drag.press_screen) > CLICK_SLOP_PX;
    if !travelled && matches!(drag.kind, Some(crate::DragKind::Overlay2D | crate::DragKind::World3D)) {
        return;
    }
    // The pointer in the drag's panel's document space. Outside the panel
    // the drag holds still rather than jumping.
    let doc_point = || drag_point(&drag, &views, pointer.screen, &cameras, &planes);
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
            let Some((_, camera, cam_tf, screen, to_camera)) = drag.camera.and_then(|c| cameras.get(c).ok()) else { return };
            let local = crate::bevy_app::scene3d::to_camera_px(pointer.screen, screen, to_camera);
            let Some(ray) = crate::bevy_app::picking::camera_ray_from_window_px(camera, cam_tf, local)
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
pub(crate) fn intersect_ray_plane(ray: Ray3d, plane_point: Vec3, plane_normal: Vec3) -> Option<Vec3> {
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

// Reflect the highlights into ActiveState components for 3D outlines, so a
// mesh lights up when a figure of its entity is picked in any panel.
pub fn selection_reflect_system(
    highlights: Res<crate::document_bridge::Highlights>,
    mut query: Query<(&Provenance, &mut ActiveState)>,
) {
    if !highlights.is_changed() {
        return;
    }
    for (prov, mut active) in &mut query {
        active.selected = highlights.selected.contains(&prov.0);
        active.hover = highlights.hovered.contains(&prov.0);
    }
}

// Outbound notification systems (hover & selection) – convert sets to js_sys::Array and call externs.
pub fn outbound_hover_system(selection: Res<crate::SelectionState>) {
    if !selection.is_changed() {
        return;
    }
    let arr = js_sys::Array::new();
    for id in selection.hovered.iter() {
        arr.push(&wasm_bindgen::JsValue::from(id.to_string()));
    }
    crate::web_ffi::send_hover_from_worker(arr);
}

pub fn outbound_selection_system(selection: Res<crate::SelectionState>) {
    if !selection.is_changed() {
        return;
    }
    let arr = js_sys::Array::new();
    for id in selection.selected.iter() {
        arr.push(&wasm_bindgen::JsValue::from(id.to_string()));
    }
    crate::web_ffi::send_selection_from_worker(arr);
}
