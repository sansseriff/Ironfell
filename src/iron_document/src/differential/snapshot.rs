//! The snapshot: what "the same" means for two stores (reactive-engine
//! handoff §4.1, task 3; spec §16.1). It is written out there field by
//! field; do not weaken it to make something pass.
//!
//! It reads without draining: where the store has only a draining accessor,
//! this looks at the buffer behind it.

use crate::*;
use std::collections::BTreeMap;
use std::fmt::Debug;

/// Everything the differential harness compares, after every call.
pub struct Snapshot {
    /// `serial`'s saved text: authored nodes, tombstones and the facets they
    /// keep, counters, the version.
    pub saved: String,
    /// Every node the documents hold (live and tombstoned; authored, derived
    /// and session) with its type, parent, order and every facet, except
    /// derived tombstones.
    pub nodes: Vec<(Node, Vec<Facet>)>,
    pub relations: Vec<Relation>,
    /// For every declared field of every facet of every live node, and
    /// every leaf with an overlay, runtime, published or session entry and
    /// no stored slot: the value as a view reads it, and its error.
    pub values: Vec<(Leaf, Option<Value>, Option<String>)>,
    pub signals: Vec<(Leaf, Signal)>,
    pub signal_errors: Vec<(Leaf, String)>,
    pub problems: Vec<expand::Problem>,
    /// The whole document's view at full fidelity, every node expanded.
    pub view: Result<String, ViewError>,
    /// The undo and redo stacks and the log, `expanded` left out.
    pub history: (Vec<Transaction>, Vec<Transaction>, Vec<Transaction>),
    pub other: Other,
    pub pending: Pending,
}

/// What the public `&self` methods answer beyond the document and values.
/// Methods left out, and why, are listed in `NOT_COMPARED`.
#[derive(PartialEq, Debug)]
pub struct Other {
    pub time: f64,
    pub time_errors: BTreeMap<NodeId, String>,
    pub clock: Option<NodeId>,
    pub producers: Vec<(ProducerId, String)>,
    pub clusters: solve::Clusters,
    pub port_anchors: Vec<plug::PortAnchor>,
    pub gesture_in_progress: bool,
    pub active_gesture: Option<GestureId>,
    /// `gesture_status` of the active and the last ended gesture.
    pub gesture_status: Vec<(GestureId, GestureStatus)>,
    pub has_actions: bool,
    pub problem_notes: BTreeMap<NodeId, Vec<String>>,
    pub interaction: BTreeMap<NodeId, Vec<&'static str>>,
    pub motion_enabled: bool,
    pub pinned_state: Option<machine::Pinned>,
    pub shared_edits: u64,
    pub inside_open: std::collections::BTreeSet<NodeId>,
    pub running_machines: Vec<machine::Running>,
    pub pending_queries: Vec<NodeId>,
    pub dark: bool,
    pub layout_preview: Option<layout::Preview>,
    pub contexts: contexts::Library,
    /// `table_grid` for every table that has one.
    pub tables: BTreeMap<NodeId, table::Grid>,
    pub stale_overrides: Vec<(NodeId, String)>,
    pub expansion_problems: Vec<expand::Problem>,
    pub physics_error: Option<String>,
    pub session_version: u64,
    pub selection: Vec<(NodeId, BTreeMap<String, Value>)>,
    pub scope: Option<ScopeId>,
    pub client: u32,
    pub horizon: usize,
    /// Per live node: `world_affine` and `local_bounds`, what the host
    /// places and indexes by.
    pub geometry: Vec<(NodeId, Affine2, Option<geom::LocalBounds>)>,
    /// `context_reads` and `hover_listeners` of every live node that has any.
    pub context_reads: Vec<(NodeId, Vec<(String, Option<NodeId>, Option<Value>)>)>,
    pub hover_listeners: Vec<(NodeId, Vec<NodeId>)>,
    /// Session state cells that differ from their initial value.
    pub session_state: BTreeMap<Leaf, Value>,
}

/// What `take_changes` would return now, and what is buffered for the
/// host.
#[derive(PartialEq, Debug)]
pub struct Pending {
    /// `take_changes`, unless it is taken (and `changes_pending`).
    pub changes: Changes,
    /// The revision counters `take_changes` bumps (`revisions`).
    pub revisions: geom::Revisions,
    /// `take_emitted`.
    pub emitted: Vec<(NodeId, String)>,
}

/// Public `&self` methods of `StoreCore` the snapshot does not call, and
/// why. Every other one is in `Snapshot`.
pub const NOT_COMPARED: &[(&str, &str)] = &[
    ("document", "the document is compared in full: saved text, nodes, relations"),
    ("history", "compared in full as `history`"),
    ("save", "compared as `saved`"),
    ("resolved", "compared for every leaf as `values`"),
    ("signals, signal_errors, problems, view", "compared as their own fields"),
    ("changes_pending", "a function of `pending.changes`"),
    ("revisions", "compared as `pending.revisions`"),
    ("reactive", "the graph's internals (heights, evaluation counts) are not compared (§16.1); its values, errors, signals and pending changes are"),
    ("expansion_problems", "compared in `other`"),
    ("world_affine3, local_affine, extent", "functions of the document and of values already compared, with no state of their own; `world_affine` and `local_bounds` are compared for every node"),
    ("handle", "a function of the document and values, per leaf; compared indirectly by the calls that use it (`set_through_handle`)"),
    ("is_session_state", "a function of the document"),
    ("handlers, listens_for", "functions of the document, per node and event name"),
    ("plug_targets", "takes a `Plugging`, which only a call returns; a function of the document and `port_anchors`"),
    ("producer, producer_name", "functions of `other.producers`"),
    ("resolver, contexts", "`contexts` is compared; the resolver borrows it"),
    ("make_component, link", "functions of the document and a caller's selection; they build a transaction and change nothing"),
    ("log_since", "a window of the log, which is compared in full"),
    ("gesture_status", "compared for the active and the last ended gesture; any other id answers `Unknown` from the same state"),
];

pub fn snapshot(s: &StoreCore) -> Snapshot {
    let doc = s.tree();
    // Virtual derived nodes leave no tombstones (`EN-13`).
    let mut nodes: Vec<(Node, Vec<Facet>)> = doc.nodes().filter(|n| n.is_live() || !n.id.is_expanded()).map(|n| { let id = n.id; (n.into_owned(), doc.facets_of(id).cloned().collect()) }).collect();
    nodes.sort_by_key(|(n, _)| n.id);
    let relations: Vec<Relation> = doc.relations().cloned().collect();

    let mut leaves: std::collections::BTreeSet<Leaf> = Default::default();
    for n in doc.live_nodes() {
        for f in doc.facets_of(n.id) {
            for field in f.field_names() {
                if let Ok(path) = LeafPath::new(f.kind(), field) {
                    leaves.insert(Leaf { node: n.id, path });
                }
            }
        }
    }
    leaves.extend(s.reactive().gesture_preview().keys().copied());
    leaves.extend(s.reactive().live_solve_preview().keys().copied());
    leaves.extend(s.authored().solved().keys().copied());
    leaves.extend(s.signals().keys().copied());
    leaves.extend(s.session_state.keys().copied());
    leaves.extend(s.reactive().errors.keys().copied());
    let values = leaves.into_iter().map(|l| (l, s.resolved(l.node, l.path), s.reactive().errors.get(&l).cloned())).collect();

    let live: Vec<NodeId> = doc.live_nodes().map(|n| n.id).collect();
    let view = s.view(&ViewQuery { relation: ViewRelation::Tree, scope: None, fidelity: Fidelity::Full, depth: None, expand: live.clone(), omit_defaults: false });

    let h = s.history();
    let history = (h.past().to_vec(), h.future().to_vec(), h.log.iter().cloned().collect());

    let mut gesture_status = Vec::new();
    if let Some(g) = s.active_gesture() {
        gesture_status.push((g, s.gesture_status(g)));
    }
    if let Some((g, _)) = &s.ended {
        gesture_status.push((*g, s.gesture_status(*g)));
    }
    let table_ty = TypeId::lookup("table");
    let tables = live.iter().filter(|&&n| Some(doc.node(n).map(|x| x.ty)) == Some(table_ty)).filter_map(|&n| Some((n, s.table_grid(n)?.clone()))).collect();
    let other = Other {
        time: s.time(),
        time_errors: s.time_errors().clone(),
        clock: s.clock(),
        producers: s.producers().map(|(p, n)| (p, n.to_owned())).collect(),
        clusters: s.clusters().clone(),
        port_anchors: s.port_anchors(),
        gesture_in_progress: s.gesture_in_progress(),
        active_gesture: s.active_gesture(),
        gesture_status,
        has_actions: s.has_actions(),
        problem_notes: s.problem_notes(),
        interaction: s.interaction().clone(),
        motion_enabled: s.motion_enabled(),
        pinned_state: s.pinned_state().cloned(),
        shared_edits: s.shared_edits(),
        inside_open: s.inside_open().clone(),
        running_machines: s.running_machines().to_vec(),
        pending_queries: s.pending_queries(),
        dark: s.dark(),
        layout_preview: s.layout_preview().cloned(),
        contexts: s.contexts().clone(),
        tables,
        stale_overrides: s.stale_overrides(),
        expansion_problems: s.expansion_problems().to_vec(),
        physics_error: s.physics_error().map(str::to_owned),
        session_version: s.session_version(),
        selection: s.selection(),
        scope: s.scope(),
        client: s.client(),
        horizon: h.horizon,
        geometry: live.iter().map(|&n| (n, s.world_affine(n), s.local_bounds(n))).collect(),
        context_reads: live.iter().map(|&n| (n, s.context_reads(n))).filter(|(_, r)| !r.is_empty()).collect(),
        hover_listeners: live.iter().map(|&n| (n, s.hover_listeners(n))).filter(|(_, r)| !r.is_empty()).collect(),
        session_state: s.session_state.clone(),
    };
    let pending = Pending { changes: s.peek_changes(), revisions: s.revisions().clone(), emitted: s.emitted.clone() };
    Snapshot {
        saved: s.save(),
        nodes,
        relations,
        values,
        signals: s.signals().iter().map(|(l, v)| (*l, v.clone())).collect(),
        signal_errors: s.signal_errors().iter().map(|(l, v)| (*l, v.clone())).collect(),
        problems: s.problems(),
        view,
        history,
        other,
        pending,
    }
}

/// The first element at which two lists differ.
fn list<T: PartialEq + Debug>(name: &str, a: &[T], b: &[T]) -> Option<String> {
    if a == b {
        return None;
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y {
            return Some(format!("{name}[{i}]:\n  subject:   {x:?}\n  reference: {y:?}"));
        }
    }
    let (longer, which) = if a.len() > b.len() { (a, "subject") } else { (b, "reference") };
    Some(format!("{name}: lengths {} and {}; only the {which} has {:?}", a.len(), b.len(), longer[a.len().min(b.len())]))
}

/// The first line at which two texts differ.
fn text(name: &str, a: &str, b: &str) -> Option<String> {
    if a == b {
        return None;
    }
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    list(name, &la, &lb).or_else(|| Some(format!("{name}: the texts differ in line endings")))
}

fn field<T: PartialEq + Debug>(name: &str, a: &T, b: &T) -> Option<String> {
    (a != b).then(|| format!("{name}:\n  subject:   {a:?}\n  reference: {b:?}"))
}

macro_rules! fields {
    ($group:literal, $a:expr, $b:expr, [$($f:ident),* $(,)?]) => {
        None$(.or_else(|| field(concat!($group, ".", stringify!($f)), &$a.$f, &$b.$f)))*
    };
}

impl Snapshot {
    /// `None` when equal; otherwise the first difference, named, subject
    /// first.
    pub fn diff(&self, r: &Snapshot) -> Option<String> {
        let (a, b) = (&self.other, &r.other);
        let o = fields!("other", a, b, [
            time, time_errors, clock, producers, clusters, port_anchors, gesture_in_progress, active_gesture, gesture_status,
            has_actions, problem_notes, interaction, motion_enabled, pinned_state, shared_edits, inside_open, running_machines,
            pending_queries, dark, layout_preview, contexts, tables, stale_overrides, expansion_problems, physics_error,
            session_version, selection, scope, client, horizon, session_state,
        ]);
        let (pa, pb) = (&self.pending, &r.pending);
        let p = fields!("pending", pa, pb, [changes, revisions, emitted]);
        text("saved", &self.saved, &r.saved)
            .or_else(|| list("nodes", &self.nodes, &r.nodes))
            .or_else(|| list("relations", &self.relations, &r.relations))
            .or_else(|| list("values", &self.values, &r.values))
            .or_else(|| list("signals", &self.signals, &r.signals))
            .or_else(|| list("signal_errors", &self.signal_errors, &r.signal_errors))
            .or_else(|| list("problems", &self.problems, &r.problems))
            .or_else(|| match (&self.view, &r.view) {
                (Ok(x), Ok(y)) => text("view", x, y),
                (x, y) => field("view", x, y),
            })
            .or_else(|| list("history.past", &self.history.0, &r.history.0))
            .or_else(|| list("history.future", &self.history.1, &r.history.1))
            .or_else(|| list("history.log", &self.history.2, &r.history.2))
            .or(o)
            .or_else(|| list("other.geometry", &a.geometry, &b.geometry))
            .or_else(|| list("other.context_reads", &a.context_reads, &b.context_reads))
            .or_else(|| list("other.hover_listeners", &a.hover_listeners, &b.hover_listeners))
            .or(p)
    }
}
