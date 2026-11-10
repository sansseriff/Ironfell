//! Values that depend on their own past (plans/specs/reactivity.md §8): a
//! formula is pure, so these are producers the store steps once a frame on
//! presentation time (`Store::advance_steps`), publishing under
//! `ProducerId::STEPS`. Session state: never saved, never undone, and not
//! seekable (scrubbing the clock leaves them be).
//!
//! - A **trace** remembers when the value it follows (`trace.of`) changed,
//!   for `trace.delay` seconds. It publishes `out`, the value as it was
//!   that long ago; `dash`, where along a path of length 1 the value was
//!   on (truthy), newest at the start, as a `stroke.dash` pattern, so a
//!   pulse travels the path; and `ages` and `values`, the changes still in
//!   the window, newest first.
//! - An **accumulate** holds a value its own formula advances:
//!   `accumulate.next` reads `accumulate.value` and `accumulate.dt` (the
//!   seconds the last step took) and gives the next value. `reset` puts it
//!   back to `initial`.

use std::collections::{BTreeMap, VecDeque};

use crate::document::Document;
use crate::expr::Leaf;
use crate::facet::{FacetKind, LeafPath};
use crate::id::NodeId;
use crate::value::Value;

#[derive(Default)]
pub struct Steps {
    /// Each trace's changes, oldest first: when, and the value from then.
    traces: BTreeMap<NodeId, VecDeque<(f64, Value)>>,
    /// Each accumulator's value.
    values: BTreeMap<NodeId, Value>,
    last: Option<f64>,
}

/// The longest step an accumulator sees: a frame after a pause (a hidden
/// tab) does not integrate the whole pause.
const MAX_DT: f64 = 0.1;

fn leaf(node: NodeId, facet: FacetKind, field: &'static str) -> Leaf {
    Leaf { node, path: LeafPath { facet, field } }
}

/// Whether a value counts as on, for the lit runs of a trace.
pub fn on(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::Quantity { si, .. } => *si != 0.0 && !si.is_nan(),
        Value::Str(s) | Value::Enum(s) => !s.is_empty(),
        _ => true,
    }
}

/// The lit runs of `ring` at `now`, as a dash pattern over a path of
/// length 1 whose start is now and whose end is `delay` seconds ago.
/// Empty when lit throughout (a solid stroke); `0 1` when dark throughout.
fn dash(ring: &VecDeque<(f64, Value)>, now: f64, delay: f64) -> String {
    // Runs from the start of the path (the newest value) to its end.
    let mut runs: Vec<(bool, f64)> = Vec::new();
    let mut from = 0.0;
    for (t, v) in ring.iter().rev() {
        let to = if delay > 0.0 { ((now - t) / delay).clamp(0.0, 1.0) } else { 1.0 };
        if to > from {
            match runs.last_mut() {
                Some((lit, len)) if *lit == on(v) => *len += to - from,
                _ => runs.push((on(v), to - from)),
            }
            from = to;
        }
    }
    match runs.as_slice() {
        [] | [(false, _)] => return "0 1".into(),
        [(true, _)] => return String::new(),
        _ => {}
    }
    let mut out: Vec<String> = Vec::new();
    if !runs[0].0 {
        out.push("0".into());
    }
    out.extend(runs.iter().map(|(_, len)| format!("{len:.4}")));
    // A pattern has dashes and gaps in pairs, and must not repeat within
    // the path: end on a gap that covers it.
    if out.len() % 2 == 1 {
        out.push("1".into());
    } else {
        *out.last_mut().expect("two runs") = "1".into();
    }
    out.join(" ")
}

impl Steps {
    /// Start a step to `now` (seconds on the host's clock): how long it
    /// is, for each accumulator's `dt`, which the store publishes before
    /// `advance` reads their formulas.
    pub fn begin(&mut self, doc: &Document, now: f64) -> BTreeMap<Leaf, Value> {
        let dt = self.last.map_or(0.0, |l| (now - l).clamp(0.0, MAX_DT));
        self.last = Some(now);
        doc.nodes_with(FacetKind::Accumulate).filter(|&n| doc.is_live(n)).map(|n| (leaf(n, FacetKind::Accumulate, "dt"), Value::Number(dt))).collect()
    }

    /// Step every trace and accumulator to `now`. Answers what to publish
    /// beside the step lengths, and the nodes still changing.
    pub fn advance(&mut self, doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, now: f64) -> (BTreeMap<Leaf, Value>, Vec<NodeId>) {
        let mut out = BTreeMap::new();
        let mut active = Vec::new();

        let live = |kind| doc.nodes_with(kind).filter(|&n| doc.is_live(n)).collect::<Vec<_>>();
        let traces = live(FacetKind::Trace);
        self.traces.retain(|n, _| traces.contains(n));
        for n in traces {
            let f = |field| leaf(n, FacetKind::Trace, field);
            let delay = read(f("delay")).and_then(|v| v.as_f64()).filter(|d| d.is_finite()).unwrap_or(0.0).max(0.0);
            let ring = self.traces.entry(n).or_default();
            // A failed input records nothing: the trace keeps what it saw.
            if let Some(v) = read(f("of")) {
                match ring.back() {
                    // The first value has been so for as long as it matters.
                    None => ring.push_back((f64::NEG_INFINITY, v)),
                    Some((_, last)) if *last != v => ring.push_back((now, v)),
                    _ => {}
                }
            }
            // Keep one change at or before the window's far end: the value
            // in force there.
            while ring.len() >= 2 && ring[1].0 <= now - delay {
                ring.pop_front();
            }
            let Some((_, oldest)) = ring.front() else { continue };
            out.insert(f("out"), oldest.clone());
            out.insert(f("dash"), Value::Str(dash(ring, now, delay)));
            out.insert(f("ages"), Value::series(ring.iter().rev().map(|(t, _)| (now - t).min(delay)).collect::<Vec<f64>>(), crate::units::Dim::NONE));
            out.insert(f("values"), Value::list(ring.iter().rev().map(|(_, v)| v.clone()).collect::<Vec<_>>()));
            if ring.len() > 1 {
                active.push(n);
            }
        }

        let accs = live(FacetKind::Accumulate);
        self.values.retain(|n, _| accs.contains(n));
        for n in accs {
            let f = |field| leaf(n, FacetKind::Accumulate, field);
            let reset = read(f("reset")).and_then(|v| v.as_bool()).unwrap_or(false);
            let before = self.values.get(&n).cloned();
            let bound = doc.leaf(n, f("next").path).is_some_and(|s| s.expr().is_some());
            let value = match &before {
                Some(v) if !reset => if bound { read(f("next")).unwrap_or_else(|| v.clone()) } else { v.clone() },
                _ => read(f("initial")).unwrap_or(Value::Number(0.0)),
            };
            if before.as_ref() != Some(&value) {
                active.push(n);
            }
            out.insert(f("value"), value.clone());
            self.values.insert(n, value);
        }
        (out, active)
    }
}
