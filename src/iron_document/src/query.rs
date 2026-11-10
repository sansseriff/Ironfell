//! Queries (plans/specs/reactivity.md §8): a `query` node asks a Model,
//! something slow or remote, for a value, and the picture binds to how the
//! asking stands. Tier 1 of plans/specs/model-interop.md: a job from a
//! snapshot of inputs to one result.
//!
//! The store never calls out. Each frame `Store::advance_queries` says
//! which requests are due; the host carries them out however it can and
//! answers with `Store::resolve_query`. A query:
//!
//! - asks only while something reads it (a binding on `value`, `status`,
//!   `error` or `stale`);
//! - waits until its input has been still for `debounce` seconds, so a
//!   drag asks once it rests, not on every frame (the first request is
//!   immediate);
//! - keeps its last good value while a newer request is pending or has
//!   failed, with `stale` true while the value answers older input;
//! - drops an answer to anything but its newest request.
//!
//! Session state under `ProducerId::QUERIES`: never saved, never undone.
//! `query.initial` (a constant or a formula) is what readers see before the
//! first answer.

use std::collections::BTreeMap;

use crate::document::Document;
use crate::expr::Leaf;
use crate::facet::{FacetKind, LeafPath};
use crate::id::NodeId;
use crate::value::Value;

/// A request the host is to carry out.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub node: NodeId,
    /// Names this request among the node's: answer with it.
    pub seq: u64,
    pub model: String,
    pub input: Value,
}

#[derive(Default)]
struct State {
    /// The model and input last seen, and when they last changed.
    seen: Option<(String, Value)>,
    changed_at: f64,
    /// What the newest request asked.
    asked: Option<(String, Value)>,
    pending: Option<u64>,
    value: Option<Value>,
    /// What `value` answers.
    answers: Option<(String, Value)>,
    error: Option<String>,
    /// A request is out, or one will go once the input rests.
    waiting: bool,
}

#[derive(Default)]
pub struct Queries {
    states: BTreeMap<NodeId, State>,
    seq: u64,
}

fn leaf(node: NodeId, field: &'static str) -> Leaf {
    Leaf { node, path: LeafPath { facet: FacetKind::Query, field } }
}

/// The leaves a reader of a query binds to.
pub const OUTPUTS: [&str; 4] = ["value", "status", "error", "stale"];

impl Queries {
    /// What to publish at `now` (seconds on the host's clock), and the
    /// requests that became due. `wanted` says whether anything reads a
    /// query.
    pub fn advance(&mut self, doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, wanted: &dyn Fn(NodeId) -> bool, now: f64) -> (BTreeMap<Leaf, Value>, Vec<Request>) {
        let nodes: Vec<NodeId> = doc.nodes_with(FacetKind::Query).filter(|&n| doc.is_live(n)).collect();
        self.states.retain(|n, _| nodes.contains(n));
        let mut out = BTreeMap::new();
        let mut due = Vec::new();
        for n in nodes {
            let st = self.states.entry(n).or_default();
            let model = read(leaf(n, "model")).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
            // A failed input asks nothing: the query keeps what it has.
            if let Some(input) = read(leaf(n, "input")) {
                let now_seen = Some((model, input));
                if st.seen != now_seen {
                    st.seen = now_seen;
                    st.changed_at = now;
                }
            }
            let debounce = read(leaf(n, "debounce")).and_then(|v| v.as_f64()).filter(|d| d.is_finite()).unwrap_or(0.0).max(0.0);
            let rested = st.asked.is_none() || now - st.changed_at >= debounce;
            if let Some((model, input)) = &st.seen
                && !model.is_empty()
                && st.asked != st.seen
                && rested
                && wanted(n)
            {
                self.seq += 1;
                st.pending = Some(self.seq);
                st.asked = st.seen.clone();
                due.push(Request { node: n, seq: self.seq, model: model.clone(), input: input.clone() });
            }
            st.waiting = st.pending.is_some() || (st.asked != st.seen && st.seen.as_ref().is_some_and(|(m, _)| !m.is_empty()) && wanted(n));
            let status = match (&st.pending, &st.error, &st.value) {
                (Some(_), _, _) => "pending",
                (None, Some(_), _) => "error",
                (None, None, Some(_)) => "ready",
                _ => "idle",
            };
            out.insert(leaf(n, "status"), Value::Enum(status.into()));
            out.insert(leaf(n, "error"), Value::Str(st.error.clone().unwrap_or_default()));
            out.insert(leaf(n, "stale"), Value::Bool(st.value.is_some() && st.answers != st.seen));
            // Before the first answer, the placeholder: readers always
            // have a value of the answer's shape.
            if let Some(v) = st.value.clone().or_else(|| read(leaf(n, "initial"))) {
                out.insert(leaf(n, "value"), v);
            }
        }
        (out, due)
    }

    /// The host's answer to request `seq` of `node`. False when it was not
    /// the node's newest request, and so is dropped.
    pub fn resolve(&mut self, node: NodeId, seq: u64, result: Result<Value, String>) -> bool {
        let Some(st) = self.states.get_mut(&node).filter(|s| s.pending == Some(seq)) else { return false };
        st.pending = None;
        match result {
            Ok(v) => {
                st.value = Some(v);
                st.answers = st.asked.clone();
                st.error = None;
            }
            Err(e) => st.error = Some(e),
        }
        true
    }

    /// Whether any request is unanswered: the host keeps frames coming.
    pub fn pending(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.states.iter().filter(|(_, s)| s.waiting).map(|(n, _)| *n)
    }
}

/// A value as plain JSON, for a Model that knows nothing of Iron: a
/// number (a quantity in SI units), text, a bool, an array (a series or a
/// list), an object (a record). Anything else is null.
pub fn to_plain(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    let num = |x: f64| serde_json::Number::from_f64(x).map_or(J::Null, J::Number);
    match v {
        Value::Number(x) => num(*x),
        Value::Quantity { si, .. } => num(*si),
        Value::Bool(b) => J::Bool(*b),
        Value::Str(s) | Value::Enum(s) => J::String(s.clone()),
        Value::Ref(id) => J::String(id.to_string()),
        Value::Color(c) => J::Array(c.iter().map(|x| num(*x as f64)).collect()),
        Value::Vec2(a) => J::Array(a.iter().map(|x| num(*x)).collect()),
        Value::Vec3(a) => J::Array(a.iter().map(|x| num(*x)).collect()),
        Value::Series { data, .. } => J::Array(data.iter().map(|x| num(*x)).collect()),
        Value::List(items) => J::Array(items.iter().map(to_plain).collect()),
        Value::Record { fields, .. } => J::Object(fields.iter().map(|(k, v)| ((*k).to_owned(), to_plain(v))).collect()),
        Value::Geometry(_) | Value::Fn(_) => J::Null,
    }
}

/// A Model's plain JSON as a value: an array of numbers is a series, any
/// other array a list, an object a record, null not a number.
pub fn from_plain(j: &serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Null => Value::Number(f64::NAN),
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        J::String(s) => Value::Str(s.clone()),
        J::Array(items) if !items.is_empty() && items.iter().all(|i| i.is_number()) => {
            Value::series(items.iter().map(|i| i.as_f64().unwrap_or(f64::NAN)).collect::<Vec<f64>>(), crate::units::Dim::NONE)
        }
        J::Array(items) => Value::list(items.iter().map(from_plain).collect::<Vec<_>>()),
        J::Object(fields) => Value::record("record", fields.iter().map(|(k, v)| (k.as_str(), from_plain(v)))),
    }
}
