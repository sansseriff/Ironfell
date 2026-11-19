//! Everything that runs on time (reactive-engine.md §10.2): one call to
//! `Store::advance` a frame brings layout glides, machines, traces and
//! accumulators, queries, items entering and leaving and the playhead to the
//! frame clock. The compositions it reports moving are live sources, so
//! their views render every frame until it rests (free-running.md §2).

use super::DocumentStore;
use super::liveness::LiveSources;
use bevy::prelude::*;
use iron_document::NodeId;
use std::collections::BTreeSet;

pub fn advance(mut store: ResMut<DocumentStore>, time: Res<Time>, views: Res<super::PanelViews>, embeds: Res<super::EmbedViews>, events: Res<crate::host_events::HostEvents>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    // A loaded document is a new store: turn its motion on too.
    if !store.0.motion_enabled() {
        store.0.set_motion(true);
    }
    // An accumulator steps only while some view shows its composition (a
    // simulation in a hidden tab holds still and costs nothing).
    let paused = {
        let doc = store.0.tree();
        let holding: BTreeSet<NodeId> = doc.nodes_with(iron_document::FacetKind::Accumulate).filter(|&n| doc.is_live(n)).filter_map(|n| iron_document::composition::composition_of(doc, n)).collect();
        if holding.is_empty() {
            holding
        } else {
            let shown = super::liveness::reached(doc, &views, &embeds, &holding);
            holding.difference(&shown).copied().collect()
        }
    };
    let out = store.bypass_change_detection().0.advance(time.elapsed_secs_f64(), &paused);
    if !out.queries.is_empty() {
        let requests = out.queries.into_iter().map(|r| crate::host_events::QueryRequest { node: r.node.to_string(), seq: r.seq, model: r.model, input: iron_document::query::to_plain(&r.input) }).collect();
        events.push(crate::host_events::EngineEvent::Queries { requests });
    }
    if out.changed {
        store.set_changed();
    }
    // Re-added every frame: another producer (the gas) may have taken a
    // composition this one still keeps live out of the shared set.
    let comps = out.wake.live;
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}
