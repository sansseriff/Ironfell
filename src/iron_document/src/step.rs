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
//!   back to `initial`. While its formula leaves it where it is, the step
//!   length is not republished, so nothing changes and no frame is drawn.

use crate::Tree;
use crate::clocks::Clocks;
use std::collections::{BTreeMap, VecDeque};

use crate::expr::Leaf;
use crate::facet::{FacetKind, LeafPath};
use crate::id::NodeId;
use crate::value::Value;

#[derive(Clone, Default)]
pub struct Steps {
    /// Each trace's changes, oldest first: when, and the value from then.
    traces: crate::work::Counted<BTreeMap<NodeId, VecDeque<(f64, Value)>>, { crate::work::STEPS }>,
    /// Each accumulator's value.
    values: crate::work::Counted<BTreeMap<NodeId, Value>, { crate::work::STEPS }>,
    /// What the steppers last published, by leaf: what a step compares with
    /// so it publishes only what changed.
    shown: BTreeMap<Leaf, Value>,
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
    /// Forget having shown `leaves`, which the graph refused (bound, or held
    /// by another producer): a later visit publishes them again, so one let
    /// go is driven (council review of DS-2).
    pub(crate) fn unshow(&mut self, leaves: &[Leaf]) {
        for l in leaves {
            self.shown.remove(l);
        }
    }

    /// Start a step to `now` (seconds on the host's clock): how long it is.
    pub fn tick(&mut self, now: f64) -> f64 {
        let dt = self.last.map_or(0.0, |l| (now - l).clamp(0.0, MAX_DT));
        self.last = Some(now);
        dt
    }

    /// Whether any of `running` would move this step, read with the step
    /// length now published (any length above 0 answers the same for a
    /// formula at rest). The store publishes the real length only then, so
    /// an accumulator at rest changes nothing from frame to frame.
    pub(crate) fn restless(&self, doc: Tree<'_>, read: &dyn Fn(Leaf) -> Option<Value>, running: &[NodeId]) -> bool {
        running.iter().any(|&n| {
            let f = |field| leaf(n, FacetKind::Accumulate, field);
            let Some(v) = self.values.get(&n) else { return true };
            if read(f("reset")).and_then(|v| v.as_bool()).unwrap_or(false) {
                return read(f("initial")).unwrap_or(Value::Number(0.0)) != *v;
            }
            doc.leaf(n, f("next").path).is_some_and(|s| s.expr().is_some()) && read(f("next")).is_some_and(|next| next != *v)
        })
    }

    /// The `dt` leaf of accumulator `n`.
    pub fn dt_leaf(n: NodeId) -> Leaf {
        leaf(n, FacetKind::Accumulate, "dt")
    }

    /// The leaves a held trace wakes on: what it records, and its delay.
    pub fn input_leaves(n: NodeId) -> [Leaf; 2] {
        [leaf(n, FacetKind::Trace, "of"), leaf(n, FacetKind::Trace, "delay")]
    }

    /// Forget the steppers no longer live (`live` holds every one): what
    /// they published, to withdraw.
    pub(crate) fn prune(&mut self, live: &std::collections::BTreeSet<NodeId>) -> Vec<Leaf> {
        let mut gone = Vec::new();
        let traces: Vec<NodeId> = self.traces.keys().copied().filter(|n| !live.contains(n)).collect();
        for n in traces {
            self.traces.remove(&n);
            gone.extend(["out", "dash", "ages", "values"].map(|f| leaf(n, FacetKind::Trace, f)));
        }
        let accs: Vec<NodeId> = self.values.keys().copied().filter(|n| !live.contains(n)).collect();
        for n in accs {
            self.values.remove(&n);
            gone.extend(["value", "dt"].map(|f| leaf(n, FacetKind::Accumulate, f)));
        }
        gone.retain(|l| self.shown.remove(l).is_some());
        gone
    }

    /// Step the traces and accumulators given to `now` (presentation time),
    /// each on its own clock (clocks.rs): the running ones (`traces`,
    /// `accs`) and the held traces whose input changed (`woken`), which
    /// record it at their frozen time. A held stepper is not visited
    /// otherwise: its value and its trace's changes in flight stand, as
    /// published (DS-2: nothing visits a paused page each frame). Answers
    /// what changed, to publish, and the nodes still changing.
    pub(crate) fn advance(&mut self, doc: Tree<'_>, read: &dyn Fn(Leaf) -> Option<Value>, now: f64, clocks: &Clocks, traces: &[NodeId], accs: &[NodeId], woken: &[NodeId]) -> (BTreeMap<Leaf, Value>, Vec<NodeId>) {
        let presentation = now;
        let mut out = BTreeMap::new();
        let mut active = Vec::new();
        let mut put = |shown: &mut BTreeMap<Leaf, Value>, l: Leaf, v: Value| {
            if shown.get(&l) != Some(&v) {
                shown.insert(l, v.clone());
                out.insert(l, v);
            }
        };
        for &n in traces.iter().chain(woken) {
            if !doc.is_live(n) {
                continue;
            }
            let now = clocks.time(doc, n, presentation);
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
            let (oldest, dash, ages, values) = (
                oldest.clone(),
                Value::Str(dash(ring, now, delay)),
                Value::series(ring.iter().rev().map(|(t, _)| (now - t).min(delay)).collect::<Vec<f64>>(), crate::units::Dim::NONE),
                Value::list(ring.iter().rev().map(|(_, v)| v.clone()).collect::<Vec<_>>()),
            );
            let busy = ring.len() > 1;
            put(&mut self.shown, f("out"), oldest);
            put(&mut self.shown, f("dash"), dash);
            put(&mut self.shown, f("ages"), ages);
            put(&mut self.shown, f("values"), values);
            if busy && !clocks.held(doc, n) {
                active.push(n);
            }
        }
        for &n in accs {
            if !doc.is_live(n) {
                continue;
            }
            let f = |field| leaf(n, FacetKind::Accumulate, field);
            let reset = read(f("reset")).and_then(|v| v.as_bool()).unwrap_or(false);
            let before = self.values.get(&n).cloned();
            let bound = doc.leaf(n, f("next").path).is_some_and(|s| s.expr().is_some());
            let value = match &before {
                Some(v) if clocks.held(doc, n) => v.clone(),
                Some(v) if !reset => if bound { read(f("next")).unwrap_or_else(|| v.clone()) } else { v.clone() },
                _ => read(f("initial")).unwrap_or(Value::Number(0.0)),
            };
            if before.as_ref() != Some(&value) {
                active.push(n);
            }
            put(&mut self.shown, f("value"), value.clone());
            self.values.insert(n, value);
        }
        (out, active)
    }

    /// Note a step length published for accumulator `n`, so `prune`
    /// withdraws it with the rest.
    pub(crate) fn shown_dt(&mut self, n: NodeId, dt: Value) {
        self.shown.insert(leaf(n, FacetKind::Accumulate, "dt"), dt);
    }

    /// The step length last published for accumulator `n`.
    pub(crate) fn dt_of(&self, n: NodeId) -> Option<f64> {
        self.shown.get(&leaf(n, FacetKind::Accumulate, "dt")).and_then(Value::as_f64)
    }
}
