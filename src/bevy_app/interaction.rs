//! What the select tool's selection and highlights mean outside it: 3D
//! outlines for lit meshes, and the hover and selection the shell is told
//! about. Drags of every kind are `manipulate`'s.

use bevy::prelude::*;

use crate::bevy_app::scene3d::ActiveState;
use crate::document_bridge::Provenance;

// Reflect the highlights into ActiveState components for 3D outlines, so a
// mesh lights up when a figure of its entity is picked in any panel.
pub fn selection_reflect_system(
    highlights: Res<crate::document_bridge::Highlights>,
    store: Res<crate::document_bridge::DocumentStore>,
    mut query: Query<(&Provenance, &mut ActiveState)>,
) {
    if !highlights.is_changed() {
        return;
    }
    let doc = store.0.document();
    for (prov, mut active) in &mut query {
        (active.selected, active.hover) = highlights.covers(doc, prov.0);
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
