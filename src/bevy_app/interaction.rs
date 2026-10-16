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

// Outbound payloads remain Rust data until a host adapter drains the app outbox.
pub fn outbound_hover_system(selection: Res<crate::SelectionState>, events: Res<crate::host_events::HostEvents>) {
    if selection.is_changed() {
        events.push(crate::host_events::EngineEvent::Hover { list:selection.hovered.iter().map(ToString::to_string).collect() });
    }
}

pub fn outbound_selection_system(selection: Res<crate::SelectionState>, events: Res<crate::host_events::HostEvents>) {
    if selection.is_changed() {
        events.push(crate::host_events::EngineEvent::Selection { list:selection.selected.iter().map(ToString::to_string).collect() });
    }
}
