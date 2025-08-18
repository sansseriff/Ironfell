use bevy::prelude::*;

use crate::bevy_app::scene3d::ActiveState;

// Decide drag start/stop and update selection based on pointer hits.
pub fn interaction_decide_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    mut drag: ResMut<crate::DragState>,
    mut selection: ResMut<crate::SelectionState>,
) {
    // Drag end
    if pointer.just_released_left {
        drag.target = None;
        drag.kind = None;
    }

    // Drag begin or click selection start
    if pointer.just_pressed_left {
        if let Some(primary) = hits.primary {
            // Selection handling (simple: single select)
            selection.selected.clear();
            selection.selected.insert(primary, ());
            selection.last_primary = Some(primary);

            // Start drag (3D for now)
            drag.target = Some(primary);
            drag.kind = Some(crate::DragKind::World3D);
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
) {
    let Some(entity) = drag.target else {
        return;
    };
    match drag.kind {
        Some(crate::DragKind::World3D) => {}
        _ => return,
    }
    if pointer.delta == Vec2::ZERO {
        return;
    }
    if let Ok(mut transform) = query.get_mut(entity) {
        // Simple screen space to world scaling heuristic
        let scale = 0.02; // TODO: derive from camera distance / projection
        transform.translation.x += pointer.delta.x * scale;
        transform.translation.y -= pointer.delta.y * scale; // invert Y screen delta
    }
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
