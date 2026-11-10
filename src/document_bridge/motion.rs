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
    // Including the frame a transition ends on: its final pose publishes
    // with nothing left running.
    store.set_changed();
    let doc = store.0.document();
    let figures = |e: NodeId| doc.relations_to(e).filter(|r| r.rel == iron_document::RelKind::Figures).map(|r| r.from).collect::<Vec<_>>();
    let comps: BTreeSet<NodeId> = running.iter().flat_map(|r| figures(r.entity)).filter_map(|u| iron_document::composition::composition_of(doc, u)).collect();
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}


/// Traces and accumulators on the frame clock (iron_document `step.rs`):
/// the store steps them, and the compositions holding the ones still
/// changing (a pulse travelling a fiber) keep rendering until they rest.
pub fn advance_steps(mut store: ResMut<DocumentStore>, time: Res<Time>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    let active = store.bypass_change_detection().0.advance_steps(time.elapsed_secs_f64());
    if active.is_empty() && marked.is_empty() {
        return;
    }
    // Including the frame the last one rests on: its final value publishes.
    store.set_changed();
    let doc = store.0.document();
    let comps: BTreeSet<NodeId> = active.iter().filter_map(|&n| iron_document::composition::composition_of(doc, n)).collect();
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}

/// Queries to Models (iron_document `query.rs`): the store says which
/// requests are due and the host carries them out (`EngineEvent::Queries`,
/// answered by the `query_result` command). The compositions holding a
/// waiting query keep rendering, so its pending state shows and its answer
/// is picked up.
pub fn advance_queries(mut store: ResMut<DocumentStore>, time: Res<Time>, events: Res<crate::host_events::HostEvents>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>, mut was: Local<Vec<NodeId>>) {
    let due = store.bypass_change_detection().0.advance_queries(time.elapsed_secs_f64());
    if !due.is_empty() {
        let requests = due.into_iter().map(|r| crate::host_events::QueryRequest { node: r.node.to_string(), seq: r.seq, model: r.model, input: iron_document::query::to_plain(&r.input) }).collect();
        events.push(crate::host_events::EngineEvent::Queries { requests });
    }
    let waiting = store.0.pending_queries();
    if waiting.is_empty() && marked.is_empty() && was.is_empty() {
        return;
    }
    // Including the frame the last answer lands on.
    if waiting != *was {
        store.set_changed();
    }
    let doc = store.0.document();
    let comps: BTreeSet<NodeId> = waiting.iter().filter_map(|&n| iron_document::composition::composition_of(doc, n)).collect();
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
    *was = waiting;
}

/// Items entering and leaving a repeat or a conditional
/// (`Store::advance_presence`): the compositions holding one on its way
/// keep rendering until it has arrived or gone.
pub fn advance_presence(mut store: ResMut<DocumentStore>, time: Res<Time>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    let active = store.bypass_change_detection().0.advance_presence(time.elapsed_secs_f64());
    if active.is_empty() && marked.is_empty() {
        return;
    }
    store.set_changed();
    let doc = store.0.document();
    let comps: BTreeSet<NodeId> = active.iter().filter_map(|&n| iron_document::composition::composition_of(doc, n)).collect();
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}
