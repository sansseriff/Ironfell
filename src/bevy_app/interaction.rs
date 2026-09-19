//! Selection, hover, and drag.
//!
//! A drag is an intent (plans/document-spine.md §10): the entity's `Transform`
//! moves every frame so the view keeps up with the pointer, and one labelled
//! transaction is queued on release. The reconciler then rewrites the entity
//! from the store, which lands on the same value.

use bevy::prelude::*;
use iron_document::{Actor, LeafPath, Op, Slot, Value};

use crate::bevy_app::scene3d::ActiveState;
use crate::document_bridge::{
    Doc2d, DocumentStore, GestureRequest, GestureRequests, NodeMap, PendingTransactions, Provenance, Shape2d, type_of,
};
use crate::panels::{Panels, VIEWER_PANEL, document_from_screen};

/// The entity whose drag just ended, awaiting writeback.
#[derive(Resource, Default)]
pub struct PendingCommit(pub Option<Entity>);

// Decide drag start/stop and update selection based on pointer hits.
pub fn interaction_decide_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    mut drag: ResMut<crate::DragState>,
    mut selection: ResMut<crate::SelectionState>,
    mut commit: ResMut<PendingCommit>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::bevy_app::scene3d::MainCamera3D>>,
    transforms: Query<&GlobalTransform>,
    doc2d: Query<&Doc2d>,
    panels: Res<Panels>,
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

    // Drag end
    if pointer.just_released_left && drag.target.is_some() {
        commit.0 = drag.target;
        drag.target = None;
        drag.kind = None;
    }

    // Drag begin or click selection start
    if pointer.just_pressed_left {
        if let Some(primary) = hits.primary {
            selection.selected.clear();
            selection.selected.insert(primary, ());
            selection.last_primary = Some(primary);
            drag.target = Some(primary);

            if doc2d.get(primary).is_ok_and(|d| matches!(d.shape, Shape2d::Slider { .. })) {
                // Scrubbing the value, not moving the control. The first
                // preview happens in drag_apply this same frame.
                drag.kind = Some(crate::DragKind::SliderValue);
            } else if doc2d.contains(primary) {
                drag.kind = Some(crate::DragKind::Overlay2D);
                // Offset from the pointer (document space) to the node's
                // global origin, so the grab point stays under the cursor.
                let p = panels
                    .rect(VIEWER_PANEL)
                    .and_then(|r| document_from_screen(r, pointer.screen))
                    .unwrap_or(Vec2::ZERO);
                let origin = transforms.get(primary).map(|g| g.translation().truncate()).unwrap_or(Vec2::ZERO);
                drag.grab_offset_2d = origin - p;
            } else {
                drag.kind = Some(crate::DragKind::World3D);
                // Establish drag plane for 3D: if ctrl held -> fixed XZ plane (normal Y).
                // Otherwise plane passes through object and is camera-facing (normal = camera forward).
                if let Ok((camera, cam_tf)) = cameras.single() {
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

/// Move the dragged entity to follow the pointer. Optimistic: this writes
/// the entity, not the store.
pub fn drag_apply_system(
    pointer: Res<crate::PointerState>,
    drag: Res<crate::DragState>,
    mut query: Query<(&mut Transform, Option<&ChildOf>)>,
    globals: Query<&GlobalTransform>,
    sliders: Query<(&Provenance, &Doc2d)>,
    mut store: ResMut<DocumentStore>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::bevy_app::scene3d::MainCamera3D>>,
    panels: Res<Panels>,
) {
    let Some(entity) = drag.target else { return };
    match drag.kind {
        Some(crate::DragKind::SliderValue) => {
            // Pointer x along the track, in the slider's own space, becomes a
            // value previewed through the store; anything bound to it follows
            // in the same frame, and nothing is written until release.
            let Some(p) = panels.rect(VIEWER_PANEL).and_then(|r| document_from_screen(r, pointer.screen)) else {
                return;
            };
            let Ok((prov, d)) = sliders.get(entity) else { return };
            let Shape2d::Slider { w, .. } = d.shape else { return };
            let Ok(global) = globals.get(entity) else { return };
            let local = global.affine().inverse().transform_point3(p.extend(0.0));
            let t = (local.x / w.max(1.0)).clamp(0.0, 1.0) as f64;
            let id = prov.0;
            let leaf = |s: &str| -> LeafPath { s.parse().expect("declared leaf") };
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
            let Some(p) = panels.rect(VIEWER_PANEL).and_then(|r| document_from_screen(r, pointer.screen)) else {
                return;
            };
            let target_global = p + drag.grab_offset_2d;
            let Ok((mut transform, parent)) = query.get_mut(entity) else { return };
            // The transform is local to the parent; map the global target back.
            let local = match parent.and_then(|c| globals.get(c.parent()).ok()) {
                Some(pg) => pg.affine().inverse().transform_point3(target_global.extend(0.0)),
                None => target_global.extend(0.0),
            };
            if transform.translation.truncate() != local.truncate() {
                transform.translation.x = local.x;
                transform.translation.y = local.y;
            }
        }
        Some(crate::DragKind::World3D) => {
            let Ok((camera, cam_tf)) = cameras.single() else { return };
            let Some(ray) = crate::bevy_app::picking::camera_ray_from_window_px(camera, cam_tf, pointer.screen)
            else {
                return;
            };
            if let Some(hit_pos) = intersect_ray_plane(ray, drag.plane_origin, drag.plane_normal)
                && let Ok((mut transform, _)) = query.get_mut(entity)
            {
                transform.translation = hit_pos + drag.grab_offset_world;
            }
        }
        _ => {}
    }
}

/// Write the released entity's position home as one transaction. Skipped
/// when nothing moved, so a click is not a "move".
pub fn drag_commit_system(
    mut commit: ResMut<PendingCommit>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    mut pending: ResMut<PendingTransactions>,
    mut gestures: ResMut<GestureRequests>,
    query: Query<(&Provenance, &Transform, Option<&Doc2d>)>,
) {
    let Some(entity) = commit.0.take() else { return };
    let Ok((prov, transform, doc2d)) = query.get(entity) else { return };
    let Some(ty) = type_of(&store.0, &map, entity) else { return };
    let doc = store.0.document();
    let id = prov.0;
    let path = |s: &str| -> LeafPath { s.parse().expect("declared leaf") };
    if doc2d.is_some_and(|d| matches!(d.shape, Shape2d::Slider { .. })) {
        // The store holds the previewed value; one transaction closes it.
        gestures.0.push(GestureRequest::Commit(format!("set {ty}")));
        return;
    }
    let mut ops = Vec::new();
    if doc2d.is_some() {
        let t = transform.translation;
        for (leaf, value) in [("transform2d.x", t.x), ("transform2d.y", t.y)] {
            let p = path(leaf);
            let Some(slot) = doc.leaf(id, p) else { continue };
            if slot.expr().is_some() {
                // A bound leaf has no handle without a disambiguation policy
                // (doc 02 §8.2); the reconciler snaps the entity back.
                info!("{id}.{p} is bound; drag not written");
                continue;
            }
            let current = slot.constant().and_then(|v| v.as_f64());
            if current != Some(value as f64) {
                ops.push(Op::Set { id, path: p, slot: Slot::Const(Value::Number(value as f64)) });
            }
        }
    } else {
        let p = path("transform3d.pos");
        let t = transform.translation;
        let value = Value::Vec3([t.x as f64, t.y as f64, t.z as f64]);
        let current = doc.leaf(id, p).and_then(|s| s.constant());
        if current != Some(&value) {
            ops.push(Op::Set { id, path: p, slot: Slot::Const(value) });
        }
    }
    if !ops.is_empty() {
        pending.push(format!("move {ty}"), Actor::Human, ops);
    }
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
