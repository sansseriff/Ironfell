//! The player (plans/specs/timeline.md §4.1). The playhead is the session
//! clock's `t`; while the clock is `playing` this advances it by wall time
//! times `rate`, wrapping between `loop_in` and `loop_out` when they differ
//! and stopping at the end of the last clip otherwise. While it plays, the
//! compositions of what the clips animate are live sources, so their views
//! render every frame (free-running.md §2).

use super::DocumentStore;
use super::liveness::LiveSources;
use bevy::prelude::*;
use iron_document::facet::{FacetKind, LeafPath};
use iron_document::{NodeId, Op, RelKind, Slot, Store, Value};
use std::collections::BTreeSet;

/// A clock field as read (a gesture's preview counts).
pub fn read(store: &Store, field: &str) -> Option<Value> {
    let clock = store.clock()?;
    let path = LeafPath::new(FacetKind::Clock, field).ok()?;
    store.resolved(clock, path)
}

fn number(store: &Store, field: &str) -> f64 {
    read(store, field).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

pub fn playing(store: &Store) -> bool {
    read(store, "playing") == Some(Value::Bool(true))
}

/// Where the last clip ends: the default stop for a clock with no loop.
pub fn end_of_clips(store: &Store) -> f64 {
    // Keyed states run their transitions past their keys.
    iron_document::timeline_view::clips_end(store)
}

/// Set clock fields as one session op batch (creating the clock if needed).
pub fn set(store: &mut Store, fields: &[(&str, Value)]) -> Result<(), String> {
    let clock = store.ensure_clock().map_err(|e| e.message)?;
    // `t` goes through `set_time`, which resamples once.
    let mut ops = Vec::new();
    for (field, value) in fields {
        if *field == "t" {
            continue;
        }
        let path = LeafPath::new(FacetKind::Clock, field).map_err(|e| e.to_string())?;
        ops.push(Op::Set { id: clock, path, slot: Slot::Const(value.clone()) });
    }
    if !ops.is_empty() {
        store.apply_session(ops).map_err(|e| e.message)?;
    }
    if let Some((_, v)) = fields.iter().find(|(f, _)| *f == "t") {
        // Time starts at 0.
        let t = v.as_f64().ok_or("clock.t is a number")?.max(0.0);
        store.set_time(t);
    }
    Ok(())
}

pub fn advance(mut store: ResMut<DocumentStore>, time: Res<Time>, mut sources: ResMut<LiveSources>, mut marked: Local<BTreeSet<NodeId>>) {
    let running = playing(&store.0);
    if running {
        let dt = time.delta_secs_f64() * number(&store.0, "rate");
        let (lo, hi) = (number(&store.0, "loop_in"), number(&store.0, "loop_out"));
        let mut t = store.0.time() + dt;
        let mut stop = false;
        if hi > lo {
            if t >= hi {
                t = lo + (t - hi) % (hi - lo);
            }
        } else {
            let end = end_of_clips(&store.0);
            if t >= end {
                t = end;
                stop = true;
            }
        }
        store.0.set_time(t);
        if stop {
            let _ = set(&mut store.0, &[("playing", Value::Bool(false))]);
        }
    }
    // What the clips animate renders every frame while the clock plays.
    let comps: BTreeSet<NodeId> = if running {
        let doc = store.0.document();
        doc.relations()
            .filter(|r| r.rel == RelKind::Animates && doc.is_live(r.to))
            .filter_map(|r| iron_document::composition::composition_of(doc, r.to))
            .collect()
    } else {
        BTreeSet::new()
    };
    if comps == *marked {
        return;
    }
    for c in marked.difference(&comps) {
        sources.0.remove(c);
    }
    sources.0.extend(comps.iter().copied());
    *marked = comps;
}
