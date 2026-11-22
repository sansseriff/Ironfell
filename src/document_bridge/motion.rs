//! What the views show, as the store's observers, and everything that runs
//! on time (reactive-engine.md §8.2, §10.2): one call to
//! `Store::advance` a frame brings layout glides, machines, traces and
//! accumulators, queries, items entering and leaving and the playhead to the
//! frame clock. The compositions it reports moving are live sources, so
//! their views render every frame until it rests (free-running.md §2).

use super::DocumentStore;
use super::liveness::LiveSources;
use bevy::prelude::*;
use iron_document::NodeId;
use std::collections::{BTreeMap, BTreeSet};

/// What the views show, as the store's observers (reactive-engine.md §8.2):
/// a region per composition each panel and open embed shows, kept in step
/// with the views. The whole composition counts until views report their
/// visible rectangles (phase 4, step 3). A loaded document is a new store:
/// its observers are registered again.
pub fn observe(mut store: ResMut<DocumentStore>, views: Res<super::PanelViews>, embeds: Res<super::EmbedViews>, mut held: Local<BTreeMap<String, (iron_document::ObserverId, iron_document::Interest)>>) {
    if store.0.observing_all() {
        store.0.set_observe_all(false);
        held.clear();
    }
    let everything = [f64::MIN / 4.0, f64::MIN / 4.0, f64::MAX / 2.0, f64::MAX / 2.0];
    let mut want: BTreeMap<String, iron_document::Interest> = BTreeMap::new();
    {
        let doc = store.0.tree();
        for view in &views.0 {
            for c in super::liveness::panel_compositions(doc, view) {
                want.insert(format!("{}:{c}", view.panel), iron_document::Interest::Region { composition: c, rect: everything, zoom: 1.0 });
            }
        }
        for e in embeds.0.iter().filter(|e| e.open) {
            want.insert(e.key.clone(), iron_document::Interest::Region { composition: e.source, rect: everything, zoom: 1.0 });
        }
    }
    if want.len() == held.len() && want.iter().all(|(k, i)| held.get(k).is_some_and(|(_, h)| h == i)) {
        return;
    }
    let store = store.bypass_change_detection();
    held.retain(|k, (id, _)| {
        let keep = want.contains_key(k);
        if !keep {
            store.0.unobserve(*id);
        }
        keep
    });
    for (k, i) in want {
        match held.get_mut(&k) {
            Some((id, h)) if *h != i => {
                store.0.reobserve(*id, i.clone());
                *h = i;
            }
            Some(_) => {}
            None => {
                let id = store.0.observe(i.clone());
                held.insert(k, (id, i));
            }
        }
    }
}

pub fn advance(mut store: ResMut<DocumentStore>, time: Res<Time>, events: Res<crate::host_events::HostEvents>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    // A loaded document is a new store: turn its motion on too.
    if !store.0.motion_enabled() {
        store.0.set_motion(true);
    }
    let out = {
        let _t = crate::perf::span(crate::perf::Phase::Advance);
        store.bypass_change_detection().0.advance(time.elapsed_secs_f64())
    };
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
