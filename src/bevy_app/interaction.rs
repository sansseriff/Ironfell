use bevy::prelude::*;

use crate::bevy_app::scene3d::ActiveState;

// Decide drag start/stop and update selection based on pointer hits.
pub fn interaction_decide_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    mut drag: ResMut<crate::DragState>,
    mut selection: ResMut<crate::SelectionState>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::bevy_app::scene3d::MainCamera3D>>,
    transforms: Query<&GlobalTransform>,
) {
    // Drag end
    if pointer.just_released_left {
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
            drag.kind = Some(crate::DragKind::World3D);

            // Establish drag plane for 3D: if ctrl held -> fixed XZ plane (normal Y).
            // Otherwise plane passes through object and is camera-facing (normal = camera forward).
            if let Ok((camera, cam_tf)) = cameras.single() {
                let cam_forward = cam_tf.forward().as_vec3();
                let plane_normal = if pointer.modifiers.ctrl {
                    Vec3::Y
                } else {
                    // Use camera forward but ensure it's not degenerate (avoid too small Y when close to parallel with Y?)
                    cam_forward
                };
                drag.plane_normal = plane_normal.normalize_or_zero();
                // Plane origin: entity position if available, else ray-plane intersection at click.
                if let Ok(ent_tf) = transforms.get(primary) {
                    drag.plane_origin = ent_tf.translation();
                } else {
                    drag.plane_origin = cam_tf.translation();
                }
                // Compute grab offset: intersection point - entity translation
                if let Some(ray) = camera
                    .viewport_to_world(cam_tf, pointer.screen)
                    .ok()
                    .map(Ray3d::from)
                {
                    if let Some(hit_pos) =
                        intersect_ray_plane(ray, drag.plane_origin, drag.plane_normal)
                    {
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
        } else {
            selection.selected.clear();
            selection.last_primary = None;
        }
    }
}

// Apply drag translation for 3D entities (simple XY plane move by screen delta * scalar)
pub fn drag_apply_system(
    pointer: Res<crate::PointerState>,
    drag: Res<crate::DragState>,
    mut query: Query<&mut Transform>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::bevy_app::scene3d::MainCamera3D>>,
) {
    let Some(entity) = drag.target else {
        return;
    };
    match drag.kind {
        Some(crate::DragKind::World3D) => {}
        _ => return,
    }
    // Build new world point from current ray-plane intersection
    let Ok((camera, cam_tf)) = cameras.single() else {
        return;
    };
    let Some(ray) = camera
        .viewport_to_world(cam_tf, pointer.screen)
        .ok()
        .map(Ray3d::from)
    else {
        return;
    };
    if let Some(hit_pos) = intersect_ray_plane(ray, drag.plane_origin, drag.plane_normal) {
        if let Ok(mut transform) = query.get_mut(entity) {
            transform.translation = hit_pos + drag.grab_offset_world;
        }
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
    // Build array from hovered keys
    let arr = js_sys::Array::new();
    for (entity, _) in selection.hovered.iter() {
        arr.push(&wasm_bindgen::JsValue::from(entity.to_bits()));
    }
    // SAFETY: extern provided by web_ffi registration
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
