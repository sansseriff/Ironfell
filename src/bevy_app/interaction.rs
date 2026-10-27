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

/// Nodes' interaction state (affordances.md §8.1): which are hovered,
/// selected, pressed or dragged, told to the store for rules that match
/// `state:`, and published onto the `interaction` facet of the nodes that
/// carry one, for their bindings. Only on change, and the publish only where
/// some node opts in.
pub fn publish_interaction_state(mut store: ResMut<crate::document_bridge::DocumentStore>, selection: Res<crate::SelectionState>, tool: Res<crate::bevy_app::select::SelectTool>) {
    use iron_document::{FacetKind, Leaf, NodeId, Value};
    use std::collections::BTreeMap;
    let mut states: BTreeMap<NodeId, Vec<&'static str>> = BTreeMap::new();
    for &n in &selection.selected {
        let s = states.entry(n).or_default();
        s.push("selected");
        if tool.pressing() {
            s.push("pressed");
        }
        if tool.dragging() {
            s.push("dragged");
        }
    }
    if let Some(h) = selection.hovered {
        states.entry(h).or_default().push("hovered");
    }
    if !store.bypass_change_detection().0.set_interaction(states.clone()) {
        return;
    }
    let doc = store.0.document();
    let nodes: Vec<NodeId> = doc.nodes_with(FacetKind::Interaction).filter(|&n| doc.is_live(n)).collect();
    let producer = store.0.producer("interaction");
    if nodes.is_empty() && producer.is_none() {
        return;
    }
    let mut values = BTreeMap::new();
    for n in nodes {
        for field in ["hovered", "selected", "pressed", "dragged"] {
            let on = states.get(&n).is_some_and(|s| s.contains(&field));
            values.insert(Leaf { node: n, path: format!("interaction.{field}").parse().expect("declared leaf") }, Value::Bool(on));
        }
    }
    let producer = producer.unwrap_or_else(|| store.0.register_producer("interaction"));
    store.0.publish(producer, values);
}
