//! The session clock (plans/specs/timeline.md §4.1), as the panels read and
//! set it. The player that moves the playhead while the clock plays is the
//! store's (`Store::advance`).

use iron_document::facet::{FacetKind, LeafPath};
use iron_document::{Op, Slot, Store, Value};

/// A clock field as read (a gesture's preview counts).
pub fn read(store: &Store, field: &str) -> Option<Value> {
    let clock = store.clock()?;
    let path = LeafPath::new(FacetKind::Clock, field).ok()?;
    store.resolved(clock, path)
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
