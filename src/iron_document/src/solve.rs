//! The adapter between constraints in the document and `iron_solve`
//! (constraint-system.md §3, §5–8).
//!
//! Roles are assigned per solve: a leaf of an operand node is a *variable*
//! unless its node carries a `fix` constraint, its slot is bound, or a clip
//! drives it now; everything else the residuals read is a *known*, taken
//! through the same reader the reactive graph uses. Constraints connected
//! through shared operand nodes form a *cluster*, solved as one problem.
//! Drivers (a transaction's explicit `Set`, or a finger during a drag) are
//! strong terms; the implicit least-change rows keep the rest still.

use crate::Tree;
use crate::facet::{FacetKind, LeafPath};
use crate::constraint::{self, ConstraintInfo};
use crate::expr::Leaf;
use crate::geom::{self, Affine2};
use crate::id::NodeId;
use crate::value::Value;
use iron_solve::{Options, Problem, Status, Term, Tier};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Constraints that share operand nodes, and the leaves they may move.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    /// What views and notifications call the cluster: the parent group of
    /// its lowest constraint, or that constraint when it is a root.
    pub id: NodeId,
    pub constraints: Vec<ConstraintInfo>,
    pub nodes: BTreeSet<NodeId>,
    /// Free leaves, in order: the solver's variables.
    pub vars: Vec<Leaf>,
    /// Leaves the residuals read but may not move.
    pub inputs: BTreeSet<Leaf>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clusters {
    /// In solve order: a cluster whose inputs depend on another's variables
    /// comes after it.
    pub list: crate::work::Counted<Vec<Cluster>, { crate::work::STORE_CLUSTERS }>,
    /// Variable leaf → cluster id, for `.by` markers.
    pub var_cluster: BTreeMap<Leaf, NodeId>,
    /// Each cluster's place in `list`, by key, and the cluster each node
    /// is in: so a solve of what moved finds it without a pass over all
    /// (plans/handoff-scaling.md DS-3a).
    pub at: BTreeMap<NodeId, usize>,
    pub by_node: BTreeMap<NodeId, NodeId>,
    /// The copies some cluster holds (`copies`).
    copies: BTreeSet<NodeId>,
    /// Every node some cluster names: its nodes, and its variables' and
    /// inputs' nodes. What the owners clusters hold in demand depend on
    /// (`EN-9`).
    named: BTreeSet<NodeId>,
    /// The clusters, by key, in a cycle through bindings: formed, never
    /// solved (DS-7: a cycle is a problem, not a refusal).
    pub failed: BTreeSet<NodeId>,
    /// What grouping could not use or solve, by constraint: a malformed
    /// constraint, the second of a duplicate pair, a cluster in a cycle.
    pub problems: Vec<(NodeId, String)>,
}

impl Clusters {
    /// `list` and its indexes.
    pub fn new(list: Vec<Cluster>, var_cluster: BTreeMap<Leaf, NodeId>) -> Clusters {
        let at = list.iter().enumerate().map(|(i, c)| (c.key(), i)).collect();
        let by_node = list.iter().flat_map(|c| c.nodes.iter().map(move |&n| (n, c.key()))).collect();
        let copies = list.iter().flat_map(|c| c.nodes.iter().copied()).filter(|n| n.is_expanded()).collect();
        let named = list.iter().flat_map(|c| c.nodes.iter().copied().chain(c.vars.iter().chain(&c.inputs).map(|l| l.node))).collect();
        Clusters { list: list.into(), var_cluster, at, by_node, copies, named, failed: BTreeSet::new(), problems: Vec::new() }
    }

    /// Whether some cluster names `node`.
    pub(crate) fn names(&self, node: NodeId) -> bool {
        self.named.contains(&node)
    }

    /// The cluster with key `key`.
    pub fn by_key(&self, key: NodeId) -> Option<&Cluster> {
        self.at.get(&key).and_then(|&i| self.list.get(i))
    }

    pub fn containing(&self, node: NodeId) -> Option<&Cluster> {
        self.by_node.get(&node).and_then(|&k| self.by_key(k))
    }

    /// The copies some cluster holds: their transform is the solver's,
    /// as a variable or as a fixed or driven input, until no cluster
    /// holds them.
    pub(crate) fn copies(&self) -> &BTreeSet<NodeId> {
        &self.copies
    }
}

/// What a cluster's solve could read, for telling whether solving it again
/// could move anything (reactive-engine.md §9.4, A6): the cluster as grouped
/// (its constraints, their values, operands and anchors, its roles), the
/// operand nodes' transform, size and radius leaves as they read (the
/// variables, and what scales a rotation), and its other inputs. What
/// `build_problem` reads; equal before and after means a solve from the
/// converged pose, with no driver and no finger, returns that pose.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Posed {
    cluster: Cluster,
    leaves: Vec<(Leaf, Option<Value>)>,
    /// Each operand's ancestors: the same ancestor leaves can belong to
    /// other operands after a reparent.
    ancestry: Vec<Vec<NodeId>>,
}

impl Cluster {
    /// The cluster's key: its lowest constraint, which no other cluster has
    /// (`id`, its parent, can be shared).
    pub(crate) fn key(&self) -> NodeId {
        self.constraints.iter().map(|c| c.id).min().unwrap_or(self.id)
    }

    /// What its solve could read now (`Posed`).
    pub(crate) fn posed(&self, doc: Tree<'_>, read: &dyn Fn(Leaf) -> Option<Value>) -> Posed {
        let mut leaves = Vec::new();
        for &n in &self.nodes {
            for kind in [FacetKind::Transform2d, FacetKind::Size, FacetKind::Radius] {
                if let Some(f) = doc.facet(n, kind) {
                    for field in f.field_names() {
                        let Ok(path) = crate::LeafPath::new(f.kind(), field) else { continue };
                        let leaf = Leaf { node: n, path };
                        leaves.push((leaf, read(leaf)));
                    }
                }
            }
        }
        leaves.extend(self.inputs.iter().map(|&l| (l, read(l))));
        let ancestry = self
            .nodes
            .iter()
            .map(|&n| std::iter::successors(doc.node(n).and_then(|x| x.parent), |&p| doc.node(p).and_then(|x| x.parent)).collect())
            .collect();
        Posed { cluster: self.clone(), leaves, ancestry }
    }
}

/// A leaf a transaction asked for by name: pulled toward `requested` at
/// strong priority.
#[derive(Clone, Debug, PartialEq)]
pub struct Driver {
    pub leaf: Leaf,
    pub requested: f64,
}

/// The driver of a drag: "this anchor on this node is at the cursor".
#[derive(Clone, Debug, PartialEq)]
pub struct Finger {
    pub node: NodeId,
    pub anchor: [f64; 2],
    pub target: [f64; 2],
    /// Hold the node's rotation here too: a part mated to others is moved
    /// whole, not swung about the grab (component-model.md §4.3).
    pub hold_rot: Option<f64>,
}

/// A requested value the solver could not honour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unmet {
    pub leaf: String,
    pub requested: f64,
    pub solved: f64,
    pub reason: String,
}

/// What the model is told about a transaction's solve (§9): causes, not
/// every inferred value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SolverSummary {
    pub clusters: Vec<NodeId>,
    pub moved: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmet: Vec<Unmet>,
    /// What the transaction left that constraints can't satisfy or use: a
    /// cluster whose solve failed, a malformed or duplicate constraint, a
    /// cycle (DS-7: applied and reported, not refused).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<SolverProblem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolverProblem {
    pub node: NodeId,
    pub message: String,
}

impl SolverSummary {
    pub fn is_empty(&self) -> bool {
        self.clusters.is_empty() && self.moved.is_empty() && self.unmet.is_empty() && self.problems.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClusterResult {
    pub cluster: NodeId,
    /// Every variable with its solved value.
    pub values: Vec<(Leaf, f64)>,
    pub status: Status,
    pub iterations: u32,
    pub evaluations: u64,
    pub max_required_residual: f64,
    pub unmet: Vec<Unmet>,
    /// Constraints that cannot all hold, when `status` is `NotConverged`.
    pub culprits: Vec<NodeId>,
}

pub(crate) const TRANSFORM_LEAVES: [&str; 3] = ["x", "y", "rot"];

fn t2(field: &str) -> LeafPath {
    LeafPath::new(FacetKind::Transform2d, field).expect("declared")
}

fn num(read: &dyn Fn(Leaf) -> Option<Value>, node: NodeId, path: LeafPath, default: f64) -> f64 {
    read(Leaf { node, path }).and_then(|v| v.as_f64()).unwrap_or(default)
}

/// Whether a transform leaf may move: its facet is present, not bound, not
/// clip- or producer-driven. Uses `doc.has` (`Key::FacetSet`) + `Wiring` so
/// constant value edits to `transform2d` do not rerun constraint grouping.
fn is_free(doc: Tree<'_>, wiring: &dyn crate::graph::Wiring, leaf: Leaf) -> bool {
    doc.has(leaf.node, leaf.path.facet) && !wiring.is_bound(leaf) && !wiring.is_driven(leaf)
}

/// Partition effective constraints `all` into `(Cluster, dep_leaves)` pairs,
/// where `dep_leaves` maps each upstream leaf to the cluster input leaf that
/// reached it through `wiring`.
fn group_effective(doc: Tree<'_>, wiring: &dyn crate::graph::Wiring, all: Vec<ConstraintInfo>) -> Vec<(Cluster, BTreeMap<Leaf, Leaf>)> {
    if all.is_empty() {
        return Vec::new();
    }
    let fixed: BTreeSet<NodeId> = all.iter().filter(|c| c.spec.kind == "fix").flat_map(|c| c.nodes()).collect();

    // Union-find over operand nodes.
    let nodes: Vec<NodeId> = all.iter().flat_map(|c| c.nodes()).collect::<BTreeSet<_>>().into_iter().collect();
    let index: BTreeMap<NodeId, usize> = nodes.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for c in &all {
        let ids: Vec<usize> = c.nodes().map(|n| index[&n]).collect();
        for &i in &ids[1..] {
            let (a, b) = (find(&mut parent, ids[0]), find(&mut parent, i));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<ConstraintInfo>> = BTreeMap::new();
    for c in &all {
        let root = find(&mut parent, index[&c.nodes().next().expect("validated operand")]);
        groups.entry(root).or_default().push(c.clone());
    }

    let mut out = Vec::with_capacity(groups.len());
    for (_, constraints) in groups {
        let lowest = constraints.iter().map(|c| c.id).min().expect("non-empty");
        let id = doc.live(lowest).and_then(|n| n.parent).unwrap_or(lowest);
        let nodes: BTreeSet<NodeId> = constraints.iter().flat_map(|c| c.nodes()).collect();
        let mut vars = Vec::new();
        let mut inputs = BTreeSet::new();
        for &n in &nodes {
            for f in TRANSFORM_LEAVES {
                let leaf = Leaf { node: n, path: t2(f) };
                if !fixed.contains(&n) && is_free(doc, wiring, leaf) {
                    vars.push(leaf);
                } else {
                    inputs.insert(leaf);
                }
            }
            for f in ["sx", "sy"] {
                inputs.insert(Leaf { node: n, path: t2(f) });
            }
            let mut cur = doc.live(n).and_then(|x| x.parent);
            while let Some(p) = cur {
                if doc.has(p, FacetKind::Transform2d) {
                    for f in ["x", "y", "rot", "sx", "sy"] {
                        inputs.insert(Leaf { node: p, path: t2(f) });
                    }
                }
                cur = doc.live(p).and_then(|x| x.parent);
            }
        }
        let mut dep_roots: BTreeSet<Leaf> = BTreeSet::new();
        for c in &constraints {
            let v = constraint::value_leaf(c.id);
            inputs.insert(v);
            dep_roots.insert(v);
            if c.spec.kind != "mate" {
                dep_roots.insert(constraint::kind_leaf(c.id));
                dep_roots.insert(constraint::enabled_leaf(c.id));
            }
        }
        dep_roots.extend(inputs.iter().copied());
        let mut dep_leaves: BTreeMap<Leaf, Leaf> = BTreeMap::new();
        for input in dep_roots {
            for dep in wiring.transitive_deps(input).into_iter().chain(std::iter::once(input)) {
                dep_leaves.entry(dep).or_insert(input);
            }
        }
        out.push((Cluster { id, constraints, nodes, vars, inputs }, dep_leaves));
    }
    out
}

/// Assemble effective clusters (already grouped per candidate component with
/// their precomputed upstream dependency leaves) into solve order via Tarjan
/// SCC, marking self- and multi-cluster cycles as failed problems.
pub(crate) fn assemble_clusters(mut entries: Vec<(Cluster, BTreeMap<Leaf, Leaf>)>, mut problems: Vec<(NodeId, String)>) -> Clusters {
    if entries.is_empty() {
        problems.sort_by_key(|(id, _)| *id);
        return Clusters { problems, ..Clusters::default() };
    }
    // Canonical order by lowest operand NodeId (matches whole-document union-find).
    entries.sort_by_key(|(c, _)| c.nodes.iter().next().copied());
    let list: Vec<Cluster> = entries.iter().map(|(c, _)| c.clone()).collect();
    let var_owner: BTreeMap<Leaf, usize> = list.iter().enumerate().flat_map(|(i, c)| c.vars.iter().map(move |v| (*v, i))).collect();
    let mut edges: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); list.len()];
    let mut own: Vec<Option<String>> = vec![None; list.len()];
    for (i, (c, dep_leaves)) in entries.iter().enumerate() {
        for (&dep, &via) in dep_leaves {
            if let Some(&j) = var_owner.get(&dep) {
                if j == i {
                    own[i].get_or_insert_with(|| format!("binding {via} reads {dep}, a variable of cluster {}, and feeds the same cluster", c.id));
                } else {
                    edges[i].insert(j);
                }
            }
        }
    }
    let comps = strongly_connected(&edges);
    let mut order = Vec::with_capacity(list.len());
    let mut failed = BTreeSet::new();
    for comp in comps {
        let ring = comp.len() > 1;
        for &i in &comp {
            let why = if ring {
                let names: Vec<String> = comp.iter().map(|&k| list[k].key().to_string()).collect();
                Some(format!("the clusters of constraints {} depend on each other through bindings", names.join(", ")))
            } else {
                own[i].clone()
            };
            if let Some(why) = why {
                failed.insert(list[i].key());
                problems.extend(list[i].constraints.iter().map(|c| (c.id, why.clone())));
            }
            order.push(i);
        }
    }
    let ordered: Vec<Cluster> = order.into_iter().map(|i| list[i].clone()).collect();
    let var_cluster = ordered.iter().flat_map(|c| c.vars.iter().map(move |v| (*v, c.id))).collect();
    problems.sort_by_key(|(id, _)| *id);
    Clusters { failed, problems, ..Clusters::new(ordered, var_cluster) }
}

/// Group the document's constraints into clusters from scratch. Total (DS-7):
/// a constraint that can't be used, and clusters that bindings join in a cycle,
/// are problems in the result. Used by the from-scratch oracle check.
pub fn clusters(doc: Tree<'_>, reactive: &crate::graph::Reactive, lib: &crate::contexts::Library) -> Clusters {
    clusters_with(doc, reactive, lib, &|l| reactive.read(doc, l))
}

/// `clusters`, reading values through `read` and graph wiring through `wiring`.
pub(crate) fn clusters_with(doc: Tree<'_>, wiring: &dyn crate::graph::Wiring, lib: &crate::contexts::Library, read: &dyn Fn(Leaf) -> Option<Value>) -> Clusters {
    let (usable, problems) = constraint::usable(doc, read);
    let all: Vec<ConstraintInfo> = usable.into_iter().chain(crate::plug::implied_mates(doc, read, lib)).filter(|c| c.enabled && c.nodes().all(|n| doc.is_live(n))).collect();
    let entries = group_effective(doc, wiring, all);
    assemble_clusters(entries, problems)
}

/// Tarjan's strongly connected components of `edges` (`i` reads `j`),
/// each emitted after every component it reaches, members in index order.
fn strongly_connected(edges: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    struct S<'a> {
        edges: &'a [BTreeSet<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    fn visit(s: &mut S<'_>, v: usize) {
        s.index[v] = Some(s.next);
        s.low[v] = s.next;
        s.next += 1;
        s.stack.push(v);
        s.on[v] = true;
        for &w in s.edges[v].iter() {
            match s.index[w] {
                None => {
                    visit(s, w);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(iw) if s.on[w] => s.low[v] = s.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(s.low[v]) == s.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = s.stack.pop() {
                s.on[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            comp.sort_unstable();
            s.out.push(comp);
        }
    }
    let n = edges.len();
    let mut s = S { edges, index: vec![None; n], low: vec![0; n], on: vec![false; n], stack: Vec::new(), next: 0, out: Vec::new() };
    for v in 0..n {
        if s.index[v].is_none() {
            visit(&mut s, v);
        }
    }
    s.out
}

/// Where a residual gets a transform leaf from: the solver's variable
/// vector, or a value read before the solve.
#[derive(Clone, Copy, Debug)]
enum Src {
    Var(usize),
    Known(f64),
}

impl Src {
    fn get(self, v: &[f64]) -> f64 {
        match self {
            Src::Var(i) => v[i],
            Src::Known(k) => k,
        }
    }
}

/// One operand node's placement as the residuals see it.
#[derive(Clone, Copy, Debug)]
struct Pose {
    x: Src,
    y: Src,
    rot: Src,
    sx: f64,
    sy: f64,
    parent: Affine2,
}

impl Pose {
    fn world(&self, v: &[f64]) -> Affine2 {
        self.parent.then(Affine2::from_trs(self.x.get(v), self.y.get(v), self.rot.get(v), self.sx, self.sy))
    }

    fn point(&self, v: &[f64], anchor: [f64; 2]) -> [f64; 2] {
        self.world(v).apply(anchor)
    }

    fn rotation(&self, v: &[f64]) -> f64 {
        let [a, b, ..] = self.parent.0;
        b.atan2(a) + self.rot.get(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tag {
    Constraint(NodeId),
    Driver(Leaf),
    Finger(NodeId),
}

/// Builds terms whose closures index the term's own variable list.
struct Builder<'d> {
    doc: Tree<'d>,
    read: &'d dyn Fn(Leaf) -> Option<Value>,
    var_index: BTreeMap<Leaf, usize>,
}

impl Builder<'_> {
    /// A pose over the term-local variable list `locals` (extended as needed).
    fn pose(&self, node: NodeId, locals: &mut Vec<usize>) -> Pose {
        let mut src = |field: &str| {
            let leaf = Leaf { node, path: t2(field) };
            match self.var_index.get(&leaf) {
                Some(&g) => {
                    let l = match locals.iter().position(|&x| x == g) {
                        Some(p) => p,
                        None => {
                            locals.push(g);
                            locals.len() - 1
                        }
                    };
                    Src::Var(l)
                }
                None => Src::Known(num(self.read, node, t2(field), if field == "rot" { 0.0 } else { 0.0 })),
            }
        };
        let x = src("x");
        let y = src("y");
        let rot = src("rot");
        let parent = match self.doc.live(node).and_then(|n| n.parent) {
            Some(p) => geom::world_affine(self.doc, self.read, p),
            None => Affine2::IDENTITY,
        };
        Pose { x, y, rot, sx: num(self.read, node, t2("sx"), 1.0), sy: num(self.read, node, t2("sy"), 1.0), parent }
    }
}

fn wrap_angle(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// Half the diagonal of a node's shape, for a rotation's scale.
fn half_diagonal(read: &dyn Fn(Leaf) -> Option<Value>, node: NodeId) -> f64 {
    let size = |c: FacetKind, f: &str| read(Leaf { node, path: LeafPath::new(c, f).expect("declared") }).and_then(|v| v.as_f64());
    if let Some(rx) = size(FacetKind::Radius, "rx") {
        return rx.max(size(FacetKind::Radius, "ry").unwrap_or(rx)).max(1.0);
    }
    let w = size(FacetKind::Size, "w").unwrap_or(100.0);
    let h = size(FacetKind::Size, "h").unwrap_or(100.0);
    (w.hypot(h) * 0.5).max(1.0)
}

/// The solver's problem for one cluster, with a tag per term. `exclude`
/// leaves one constraint out, for the culprit search.
fn build_problem<'a>(
    doc: Tree<'a>,
    read: &'a dyn Fn(Leaf) -> Option<Value>,
    cluster: &Cluster,
    drivers: &[Driver],
    fingers: &[Finger],
    exclude: Option<NodeId>,
) -> (Problem<'a>, Vec<Tag>) {
    let var_index: BTreeMap<Leaf, usize> = cluster.vars.iter().enumerate().map(|(i, l)| (*l, i)).collect();
    let vars: Vec<f64> = cluster.vars.iter().map(|l| read(*l).and_then(|v| v.as_f64()).unwrap_or(0.0)).collect();
    let scale: Vec<f64> = cluster
        .vars
        .iter()
        .map(|l| if l.path.field == "rot" { 1.0 / half_diagonal(read, l.node) } else { 1.0 })
        .collect();
    let mut problem = Problem::new(vars, scale);
    let mut tags = Vec::new();
    let b = Builder { doc, read, var_index };

    for c in &cluster.constraints {
        let kind = c.spec.kind;
        if kind == "fix" || Some(c.id) == exclude {
            continue;
        }
        let mut locals = Vec::new();
        let poses: Vec<(Pose, [f64; 2])> = c.operands.iter().map(|o| (b.pose(o.node, &mut locals), o.anchor)).collect();
        // Read now, not when the cluster was built: a driving dimension bound
        // to a slider changes between solves.
        let value = read(constraint::value_leaf(c.id)).and_then(|v| v.as_f64()).unwrap_or(c.value);
        let at = c.operands[0].at;
        let turns = [c.operands[0].turn, c.operands.get(1).map_or(0.0, |o| o.turn)];
        let term = match kind {
            "coincident" => Term::new(Tier::Required, locals, 2, move |v, r| {
                let p = poses[0].0.point(v, poses[0].1);
                let q = poses[1].0.point(v, poses[1].1);
                r[0] = p[0] - q[0];
                r[1] = p[1] - q[1];
            }),
            "distance" => Term::new(Tier::Required, locals, 1, move |v, r| {
                let p = poses[0].0.point(v, poses[0].1);
                let q = poses[1].0.point(v, poses[1].1);
                r[0] = (p[0] - q[0]).hypot(p[1] - q[1]) - value;
            }),
            "pin" => {
                let at = at.unwrap_or([0.0, 0.0]);
                Term::new(Tier::Required, locals, 2, move |v, r| {
                    let p = poses[0].0.point(v, poses[0].1);
                    r[0] = p[0] - at[0];
                    r[1] = p[1] - at[1];
                })
            }
            "angle" => Term::new(Tier::Required, locals, 1, move |v, r| {
                r[0] = wrap_angle(poses[1].0.rotation(v) - poses[0].0.rotation(v) - value);
            }),
            "mate" => Term::new(Tier::Required, locals, 3, move |v, r| {
                let p = poses[0].0.point(v, poses[0].1);
                let q = poses[1].0.point(v, poses[1].1);
                r[0] = p[0] - q[0];
                r[1] = p[1] - q[1];
                // Facings opposed; a radian weighs like a unit here, as for
                // `angle`.
                r[2] = wrap_angle(poses[1].0.rotation(v) + turns[1] - poses[0].0.rotation(v) - turns[0] - std::f64::consts::PI);
            }),
            "horizontal" => Term::new(Tier::Required, locals, 1, move |v, r| {
                r[0] = poses[0].0.point(v, poses[0].1)[1] - poses[1].0.point(v, poses[1].1)[1];
            }),
            "vertical" => Term::new(Tier::Required, locals, 1, move |v, r| {
                r[0] = poses[0].0.point(v, poses[0].1)[0] - poses[1].0.point(v, poses[1].1)[0];
            }),
            _ => continue,
        };
        problem.term(term);
        tags.push(Tag::Constraint(c.id));
    }
    for d in drivers {
        if let Some(&i) = b.var_index.get(&d.leaf) {
            let requested = d.requested;
            problem.term(Term::new(Tier::Strong, vec![i], 1, move |v, r| r[0] = v[0] - requested));
            tags.push(Tag::Driver(d.leaf));
        }
    }
    for f in fingers {
        if !cluster.nodes.contains(&f.node) {
            continue;
        }
        let mut locals = Vec::new();
        let pose = b.pose(f.node, &mut locals);
        if locals.is_empty() {
            continue;
        }
        let (anchor, target) = (f.anchor, f.target);
        problem.term(Term::new(Tier::Strong, locals, 2, move |v, r| {
            let p = pose.point(v, anchor);
            r[0] = p[0] - target[0];
            r[1] = p[1] - target[1];
        }));
        tags.push(Tag::Finger(f.node));
        if let Some(rot) = f.hold_rot
            && let Some(&i) = b.var_index.get(&Leaf { node: f.node, path: t2("rot") })
        {
            problem.term(Term::new(Tier::Strong, vec![i], 1, move |v, r| r[0] = v[0] - rot));
            tags.push(Tag::Finger(f.node));
        }
    }
    (problem, tags)
}

const OPTIONS: Options = Options { max_iterations: 50, tolerance: 1e-9, weights: [1e2, 1.0, 1e-3], least_change: true, step_limit: None, project: true };

/// Solve one cluster from the current resolved values. Drivers whose leaf
/// is not one of the cluster's variables are ignored; fingers on nodes
/// outside it too.
pub fn solve_cluster(
    doc: Tree<'_>,
    read: &dyn Fn(Leaf) -> Option<Value>,
    cluster: &Cluster,
    drivers: &[Driver],
    fingers: &[Finger],
) -> ClusterResult {
    let (problem, tags) = build_problem(doc, read, cluster, drivers, fingers, None);
    let s = iron_solve::solve(&problem, &OPTIONS);
    // Snap to a nanometre of a pixel (or of a radian): far inside the
    // tolerance, and it keeps a point that is coincident with a fixed one
    // at exactly its coordinate in files and views.
    let snap = |v: f64| (v * 1e9).round() / 1e9;
    let snapped: Vec<f64> = s.vars.iter().map(|v| snap(*v)).collect();
    // Unless snapping undoes a hold that solving reached (a parent scaled
    // a millionfold turns a nanometre into a millimetre): the watchers
    // would report what was just written (review of DS-7, Sol).
    let keep = if problem.max_residual(&snapped, iron_solve::Tier::Required) <= HOLDS || problem.max_residual(&s.vars, iron_solve::Tier::Required) > HOLDS { snapped } else { s.vars.clone() };
    let values: Vec<(Leaf, f64)> = cluster.vars.iter().copied().zip(keep).collect();
    let mut unmet = Vec::new();
    for d in drivers {
        if let Some(i) = cluster.vars.iter().position(|l| *l == d.leaf)
            && (s.vars[i] - d.requested).abs() > 1e-6
        {
            unmet.push(Unmet {
                leaf: d.leaf.to_string(),
                requested: d.requested,
                solved: s.vars[i],
                reason: format!("constrained by cluster {}: the constraints allow {} at most", cluster.id, crate::value::fmt_num(s.vars[i])),
            });
        }
    }
    let mut culprits = Vec::new();
    if s.status == Status::NotConverged {
        // Causal test: a constraint whose removal makes the cluster solvable
        // is part of the conflict. Clusters are small and this runs only on
        // failure. Falls back to the rank analysis plus the worst residuals
        // when no single removal helps (two independent conflicts).
        for c in &cluster.constraints {
            if c.spec.kind == "fix" {
                continue;
            }
            let (reduced, _) = build_problem(doc, read, cluster, &[], &[], Some(c.id));
            if iron_solve::solve(&reduced, &OPTIONS).status == Status::Converged {
                culprits.push(c.id);
            }
        }
        if culprits.is_empty() {
            let a = iron_solve::analysis::analyze(&problem, &s.vars, 1e-6);
            let mut buf = Vec::new();
            let mut residuals: Vec<(usize, f64)> = Vec::new();
            for (i, t) in problem.terms.iter().enumerate() {
                if !matches!(tags[i], Tag::Constraint(_)) {
                    continue;
                }
                buf.clear();
                buf.resize(t.len, 0.0);
                t.eval(&s.vars, &mut buf);
                residuals.push((i, buf.iter().fold(0.0f64, |m, r| m.max(r.abs()))));
            }
            let worst = residuals.iter().fold(0.0f64, |m, (_, r)| m.max(*r));
            for (i, r) in residuals {
                let Tag::Constraint(id) = tags[i] else { continue };
                if (a.conflicting.contains(&i) || (r > OPTIONS.tolerance && r > 1e-3 * worst)) && !culprits.contains(&id) {
                    culprits.push(id);
                }
            }
        }
    }
    ClusterResult {
        cluster: cluster.id,
        values,
        status: s.status,
        iterations: s.iterations,
        evaluations: s.evaluations,
        max_required_residual: s.max_required_residual,
        unmet,
        culprits,
    }
}

/// The largest required residual at which constraints hold: what a solve
/// must reach to be written, and what the watchers report past (review of
/// DS-7: the solver's own tolerance and the watchers' differed, so a
/// failed solve could leave no problem).
pub(crate) const HOLDS: f64 = 1e-6;

/// Whether a solve's result holds: converged, or within `HOLDS`.
pub(crate) fn holds(result: &ClusterResult) -> bool {
    result.status == Status::Converged || result.max_required_residual <= HOLDS
}

/// The constraints of `cluster` that don't hold at the values `read`
/// gives now, each with its largest residual: what a cluster whose solve
/// failed shows, read from the pose and not remembered from the solve
/// (DS-7), so undo, redo and replay show the right problem without one.
pub(crate) fn violated(doc: Tree<'_>, read: &dyn Fn(Leaf) -> Option<Value>, cluster: &Cluster) -> Vec<(NodeId, f64)> {
    let (problem, tags) = build_problem(doc, read, cluster, &[], &[], None);
    let mut out: Vec<(NodeId, f64)> = Vec::new();
    let mut buf = Vec::new();
    for (i, t) in problem.terms.iter().enumerate() {
        let Tag::Constraint(id) = tags[i] else { continue };
        buf.clear();
        buf.resize(t.len, 0.0);
        t.eval(&problem.vars, &mut buf);
        // NaN anywhere doesn't hold (`f64::max` would drop it).
        let r = buf.iter().fold(0.0f64, |m, x| if x.is_nan() { f64::INFINITY } else { m.max(x.abs()) });
        if r > HOLDS {
            match out.iter_mut().find(|(c, _)| *c == id) {
                Some((_, m)) => *m = m.max(r),
                None => out.push((id, r)),
            }
        }
    }
    out
}

/// Structural constraint candidates discovered for one top-level root by the
/// Stage 2 partition maker (engine.md §3.6, EN-2b).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RootCandidates {
    /// Explicit constraint nodes under this root and their `Constrains` targets.
    pub(crate) explicit: Vec<(NodeId, Vec<NodeId>)>,
    /// Candidate implied mates under this root (per-composition attachment).
    pub(crate) mates: Vec<crate::plug::CandidateMate>,
}

/// Stage 2 maker for one top-level root: reads only through `MakerCx`'s
/// recording `Tree` and scoped indexes (`In(root, constraint)`,
/// `With(root, Anchor)`).
pub(crate) fn root_candidates(cx: &crate::MakerCx<'_>, root: NodeId) -> RootCandidates {
    let doc = cx.tree();
    let mut explicit = Vec::new();
    for c in doc.nodes_of_type_in(root, constraint::constraint_type()) {
        let targets: Vec<NodeId> = doc
            .relations_from(c)
            .filter(|r| r.rel == crate::document::RelKind::Constrains)
            .map(|r| r.to)
            .collect();
        explicit.push((c, targets));
    }
    let mates = crate::plug::candidate_mates_in(doc, root);
    RootCandidates { explicit, mates }
}

/// Partition structural candidates across all roots into connected
/// `ComponentNative`s keyed by each component's lowest constraint/connection id.
pub(crate) fn partition_candidates<'a>(roots: impl IntoIterator<Item = &'a RootCandidates>) -> BTreeMap<NodeId, ComponentNative> {
    let mut explicit: Vec<(NodeId, &[NodeId])> = Vec::new();
    let mut mates: Vec<crate::plug::CandidateMate> = Vec::new();
    let mut nodes_set: BTreeSet<NodeId> = BTreeSet::new();
    for rc in roots {
        for (c, targets) in &rc.explicit {
            explicit.push((*c, targets.as_slice()));
            nodes_set.extend(targets.iter().copied());
        }
        for m in &rc.mates {
            mates.push(*m);
            nodes_set.insert(m.body_a);
            nodes_set.insert(m.body_b);
        }
    }
    if explicit.is_empty() && mates.is_empty() {
        return BTreeMap::new();
    }
    let nodes: Vec<NodeId> = nodes_set.into_iter().collect();
    let index: BTreeMap<NodeId, usize> = nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let union = |p: &mut [usize], a: usize, b: usize| {
        let (ra, rb) = (find(p, a), find(p, b));
        if ra != rb {
            p[ra.max(rb)] = ra.min(rb);
        }
    };
    for (_, targets) in &explicit {
        if let Some((&first, rest)) = targets.split_first() {
            let a = index[&first];
            for &t in rest {
                union(&mut parent, a, index[&t]);
            }
        }
    }
    for m in &mates {
        union(&mut parent, index[&m.body_a], index[&m.body_b]);
    }
    let mut by_root: BTreeMap<usize, (Vec<NodeId>, Vec<crate::plug::CandidateMate>)> = BTreeMap::new();
    let mut singletons: Vec<NodeId> = Vec::new();
    for (c, targets) in explicit {
        if let Some(&first) = targets.first() {
            let r = find(&mut parent, index[&first]);
            by_root.entry(r).or_default().0.push(c);
        } else {
            singletons.push(c);
        }
    }
    for m in mates {
        let r = find(&mut parent, index[&m.body_a]);
        by_root.entry(r).or_default().1.push(m);
    }
    let mut out = BTreeMap::new();
    for c in singletons {
        out.insert(c, ComponentNative::new(vec![c], Vec::new()));
    }
    for (_, (mut cs, mut ms)) in by_root {
        cs.sort();
        cs.dedup();
        ms.sort();
        ms.dedup();
        let key = cs.iter().copied().chain(ms.iter().map(|m| m.connection)).min().expect("non-empty component");
        out.insert(key, ComponentNative::new(cs, ms));
    }
    out
}

/// Output of one candidate component's Stage 3 evaluation (`ComponentNative`).
#[derive(Clone, Debug)]
pub(crate) struct ComponentOut {
    pub(crate) clusters: Vec<(Cluster, BTreeMap<Leaf, Leaf>)>,
    pub(crate) problems: Vec<(NodeId, String)>,
}

/// One structural candidate component of constraints and implied mates as a
/// keyed Stage 3 native cell (engine.md §3.6, EN-2b): evaluates `kind`,
/// `enabled`, `value`, mate eligibility (`plug.mate`), free leaves (`Wiring`),
/// and upstream transitive dependency leaves. Claims nothing; its output lives
/// in `extra` as [`ComponentOut`].
#[derive(Debug, PartialEq)]
pub(crate) struct ComponentNative {
    constraints: Vec<NodeId>,
    mates: Vec<crate::plug::CandidateMate>,
    outputs: BTreeSet<Leaf>,
}

impl ComponentNative {
    pub(crate) fn new(constraints: Vec<NodeId>, mates: Vec<crate::plug::CandidateMate>) -> ComponentNative {
        ComponentNative { constraints, mates, outputs: BTreeSet::new() }
    }
}

impl crate::graph::Native for ComponentNative {
    fn producer(&self) -> crate::graph::ProducerId {
        crate::graph::ProducerId::NETWORK
    }
    fn outputs(&self) -> &BTreeSet<Leaf> {
        &self.outputs
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn keyed(&self) -> bool {
        true
    }
    fn compute(&self, cx: &mut crate::graph::Cx<'_>) -> Result<BTreeMap<Leaf, Value>, crate::graph::Failure> {
        let (usable, problems) = constraint::usable_in(cx.tree(), &|l| cx.read(l), &self.constraints);
        let mut all = usable;
        if !self.mates.is_empty() {
            let lib = cx.input::<crate::contexts::Library>(crate::graph::InputKey::Contexts).unwrap_or_else(|| crate::contexts::Library::builtin());
            all.extend(crate::plug::eval_mates(cx.tree(), &|l| cx.read(l), lib, &self.mates));
        }
        all.retain(|c| c.enabled && c.nodes().all(|n| cx.tree().is_live(n)));
        let clusters = group_effective(cx.tree(), cx, all);
        cx.set_extra(std::sync::Arc::new(ComponentOut { clusters, problems }));
        Ok(BTreeMap::new())
    }
}

/// One cluster as a keyed native cell that only watches (EN-2b): it reads what
/// a solve of the cluster reads (including operand ancestry through the
/// recording `Tree`), so that it ran says something of it changed since the
/// cluster was last solved and committed.
#[derive(Debug, PartialEq)]
pub(crate) struct ClusterNative {
    cluster: Cluster,
    outputs: BTreeSet<Leaf>,
}

/// A cluster's constraints that don't hold at its pose, as its watching
/// native last read them (`violated`).
pub(crate) struct Violated(pub(crate) Vec<(NodeId, f64)>);

impl ClusterNative {
    pub(crate) fn new(cluster: Cluster) -> ClusterNative {
        ClusterNative { cluster, outputs: BTreeSet::new() }
    }
}

impl crate::graph::Native for ClusterNative {
    fn producer(&self) -> crate::graph::ProducerId {
        crate::graph::ProducerId::NETWORK
    }
    fn outputs(&self) -> &BTreeSet<Leaf> {
        &self.outputs
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn keyed(&self) -> bool {
        true
    }
    fn compute(&self, cx: &mut crate::graph::Cx<'_>) -> Result<BTreeMap<Leaf, Value>, crate::graph::Failure> {
        let _ = self.cluster.posed(cx.tree(), &|l| cx.read(l));
        // What scales a rotation, whether or not its facet is there yet.
        for &n in &self.cluster.nodes {
            for p in ["size.w", "size.h", "radius.rx", "radius.ry"] {
                let _ = cx.read(Leaf { node: n, path: p.parse().expect("declared leaf") });
            }
        }
        // Whether its constraints hold where it is: its problem (DS-7). No
        // solve here: which are at fault is the failed solve's to tell, in
        // the transaction's reply (review of DS-7: a solve per watcher run
        // doubled a drag's solves).
        cx.set_extra(std::sync::Arc::new(Violated(violated(cx.tree(), &|l| cx.settled(l), &self.cluster))));
        Ok(BTreeMap::new())
    }
}
