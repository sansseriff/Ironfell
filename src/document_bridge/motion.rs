//! Declared transitions on the frame clock (plans/specs/affordances.md §8.2).
//! The store eases what layout moves where a context says so
//! (`motion.placement`); this keeps its clock and keeps the views of what is
//! moving rendering until it rests, as a running producer does
//! (`LiveSources`).

use super::DocumentStore;
use super::liveness::LiveSources;
use bevy::prelude::*;
use iron_document::NodeId;
use std::collections::BTreeSet;

pub fn advance(mut store: ResMut<DocumentStore>, time: Res<Time>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    // A loaded document is a new store: turn its motion on too.
    if !store.0.motion_enabled() {
        store.0.set_motion(true);
    }
    let moving = store.bypass_change_detection().0.advance_motion(time.elapsed_secs_f64());
    if moving.is_empty() && marked.is_empty() {
        return;
    }
    if !moving.is_empty() {
        store.set_changed();
    }
    let doc = store.0.document();
    let comps: BTreeSet<NodeId> = moving.iter().filter_map(|&n| iron_document::composition::composition_of(doc, n)).collect();
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}

/// Components' state machines on the frame clock (plans/specs/machines.md):
/// the store starts a transition where an instance's state changed and
/// moves the running ones; this keeps the compositions picturing them
/// rendering until they rest.
pub fn advance_machines(mut store: ResMut<DocumentStore>, time: Res<Time>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    let running = store.bypass_change_detection().0.advance_machines(time.elapsed_secs_f64());
    if running.is_empty() && marked.is_empty() {
        return;
    }
    if !running.is_empty() {
        store.set_changed();
    }
    let doc = store.0.document();
    let figures = |e: NodeId| doc.relations_to(e).filter(|r| r.rel == iron_document::RelKind::Figures).map(|r| r.from).collect::<Vec<_>>();
    let comps: BTreeSet<NodeId> = running.iter().flat_map(|r| figures(r.entity)).filter_map(|u| iron_document::composition::composition_of(doc, u)).collect();
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}

