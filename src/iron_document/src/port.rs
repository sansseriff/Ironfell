//! Ports, connections and signals (entity-composition-model.md §6, §6.1).
//!
//! A port kind declares the variables a port carries: a one-way `signal`
//! (optical power along a fiber), or an `across`/`through` pair (voltage and
//! current, temperature and heat flow). A connection joins two ports of the
//! same kind with two `connects` relations.
//!
//! One-way signals ride the reactive graph as derived bindings: an in-port's
//! `port.signal` reads its partner out-port's, built from the connection when
//! the graph is (re)built and never stored, so a file holds only the
//! connection. An out-port's signal is its definition's behaviour, an
//! ordinary binding copied at instantiation. Across/through pairs become
//! solver equations; the format carries them now, the solving comes next
//! (ECM §6.1, step 3).

use crate::Tree;
use crate::Value;
use crate::facet::{FacetKind, LeafPath, Port};
use crate::document::RelKind;
use crate::expr::{Expr, Leaf};
use crate::id::NodeId;
use std::collections::{BTreeMap, BTreeSet};

/// How a port kind's variables flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// One value, from an `out` port to an `in` port.
    Signal,
    /// An across value equal at the joint, a through value summing to zero.
    Pair,
}

/// A declared port kind: its name, flow, and the unit its values carry
/// (`None`: plain numbers).
#[derive(Clone, Copy, Debug)]
pub struct PortKind {
    pub name: &'static str,
    pub flow: Flow,
    /// The signal's unit, or a pair's across unit.
    pub unit: Option<&'static str>,
    /// A pair's through unit.
    pub through: Option<&'static str>,
}

pub static KINDS: &[PortKind] = &[
    PortKind { name: "value", flow: Flow::Signal, unit: None, through: None },
    PortKind { name: "optical_power", flow: Flow::Signal, unit: Some("W"), through: None },
    PortKind { name: "rf_voltage", flow: Flow::Signal, unit: Some("V"), through: None },
    PortKind { name: "electrical", flow: Flow::Pair, unit: Some("V"), through: Some("A") },
    PortKind { name: "heat_flow", flow: Flow::Pair, unit: Some("K"), through: Some("W") },
];

pub fn kind(name: &str) -> Option<&'static PortKind> {
    KINDS.iter().find(|k| k.name == name)
}

fn text(doc: Tree<'_>, port: NodeId, field: &str) -> Option<String> {
    let p = doc.get::<Port>(port)?;
    let slot = match field {
        "kind" => &p.kind,
        _ => &p.dir,
    };
    slot.constant().and_then(|v| v.as_str()).map(str::to_owned)
}

fn signal(port: NodeId) -> Leaf {
    Leaf { node: port, path: LeafPath::new(FacetKind::Port, "signal").expect("declared") }
}

/// A connection's two ends, `a` then `b`, if it has exactly those.
pub fn ends(doc: Tree<'_>, connection: NodeId) -> Option<(NodeId, NodeId)> {
    let mut a = None;
    let mut b = None;
    for r in doc.relations_from(connection).filter(|r| r.rel == RelKind::Connects) {
        match r.data.get("end").and_then(|v| v.as_str()) {
            Some("a") if a.is_none() => a = Some(r.to),
            Some("b") if b.is_none() => b = Some(r.to),
            _ => return None,
        }
    }
    Some((a?, b?))
}

/// For a one-way connection, which end feeds which: `(out, in)`.
fn direction(doc: Tree<'_>, a: NodeId, b: NodeId) -> Option<(NodeId, NodeId)> {
    match (text(doc, a, "dir").as_deref(), text(doc, b, "dir").as_deref()) {
        (Some("out"), Some("in")) => Some((a, b)),
        (Some("in"), Some("out")) => Some((b, a)),
        _ => None,
    }
}

/// Every in-port's signal as its partner's: one derived binding per live,
/// valid one-way connection.
pub fn derived_signals(doc: Tree<'_>) -> Vec<(Leaf, Expr)> {
    let Some(ty) = crate::registry::TypeId::lookup("connection") else { return Vec::new() };
    doc.nodes_of_type(ty)
        .filter(|&c| doc.is_live(c))
        .filter_map(|c| {
            let (a, b) = ends(doc, c)?;
            if !doc.is_live(a) || !doc.is_live(b) {
                return None;
            }
            let k = kind(&text(doc, a, "kind")?)?;
            if k.flow != Flow::Signal {
                return None;
            }
            let (from, to) = direction(doc, a, b)?;
            Some((signal(to), Expr::Ref(signal(from))))
        })
        .collect()
}

/// Check a connection: two live ports of one kind; for a one-way kind, an
/// out joined to an in that nothing else feeds; and no signal loop.
pub fn validate_connection(doc: Tree<'_>, connection: NodeId) -> Result<(), String> {
    let (a, b) = ends(doc, connection).ok_or_else(|| format!("{connection} needs exactly two connects relations, ends a and b"))?;
    for p in [a, b] {
        let ok = doc.live(p).is_some_and(|n| n.ty.name() == "port");
        if !ok {
            return Err(format!("{connection}: {p} is not a live port"));
        }
    }
    let (ka, kb) = (text(doc, a, "kind").unwrap_or_default(), text(doc, b, "kind").unwrap_or_default());
    if ka != kb {
        return Err(format!("{connection}: cannot connect {ka} ({a}) to {kb} ({b}); a connection joins ports of one kind"));
    }
    let k = kind(&ka).ok_or_else(|| format!("{connection}: unknown port kind {ka:?}"))?;
    if k.flow == Flow::Signal {
        let (from, to) = direction(doc, a, b).ok_or_else(|| format!("{connection}: a {ka} connection joins an out port to an in port"))?;
        // Declared types must agree; a loose end is always allowed, and a
        // typed input checks what arrives (types-and-units.md §3.5).
        let ty = |p: NodeId| doc.get::<Port>(p).and_then(|x| x.datatype.constant()).and_then(|v| v.as_str()).and_then(|t| crate::types::Type::parse(t).ok()).unwrap_or(crate::types::Type::ANY);
        ty(from).feeds(&ty(to)).map_err(|e| format!("{connection}: cannot connect {from} to {to}: {e}"))?;
        let ty = crate::registry::TypeId::lookup("connection").expect("registered");
        let feeds = doc
            .nodes_of_type(ty)
            .filter(|&c| c != connection && doc.is_live(c))
            .filter_map(|c| ends(doc, c))
            .filter(|&(x, y)| x == to || y == to)
            .count();
        if feeds > 0 {
            return Err(format!("{connection}: {to} already has a connection feeding it"));
        }
    }
    if let Some(at) = signal_loop(doc) {
        return Err(format!("{connection}: signals would loop through {at}"));
    }
    Ok(())
}

/// A leaf on a dependency loop through bindings and derived signals, if any.
fn signal_loop(doc: Tree<'_>) -> Option<Leaf> {
    let mut deps: BTreeMap<Leaf, BTreeSet<Leaf>> = BTreeMap::new();
    for (leaf, expr) in doc.bound_slots() {
        deps.insert(leaf, expr.deps());
    }
    for (leaf, expr) in derived_signals(doc) {
        deps.insert(leaf, expr.deps());
    }
    // Iterative three-colour DFS.
    let mut state: BTreeMap<Leaf, u8> = BTreeMap::new();
    for &start in deps.keys() {
        if state.contains_key(&start) {
            continue;
        }
        let mut stack: Vec<(Leaf, Vec<Leaf>)> = vec![(start, deps[&start].iter().copied().collect())];
        state.insert(start, 1);
        while let Some((node, rest)) = stack.last_mut() {
            match rest.pop() {
                Some(next) => match state.get(&next) {
                    Some(1) => return Some(next),
                    Some(_) => {}
                    None => {
                        state.insert(next, 1);
                        let more = deps.get(&next).map(|d| d.iter().copied().collect()).unwrap_or_default();
                        stack.push((next, more));
                    }
                },
                None => {
                    state.insert(*node, 2);
                    stack.pop();
                }
            }
        }
    }
    None
}

/// The across/through network (ECM §6.1, step 3), assembled for the solver:
/// every pair port's across and through value that is not bound is an
/// unknown, in SI units with its dimension; the equations are every
/// connection net's (across equal, through summing to zero; an unconnected
/// terminal carries no flow) and every live `equation` node's
/// `lhs - rhs`. Empty when the document has no pair ports.
pub struct Network {
    pub unknowns: Vec<(Leaf, crate::units::Dim)>,
    /// Each a residual over the unknowns: `Linear` sums `coef · unknown`,
    /// `Expr` evaluates `lhs - rhs` with the unknowns substituted.
    pub equations: Vec<Equation>,
}

/// One island of a network (reactive-engine handoff §4.10, RE-5.3): the
/// unknowns joined by any equation they share, as indices into
/// `Network::unknowns` (ascending), and the equations over them, as indices
/// into `Network::equations`. Islands are solved apart, each from its own
/// warm start.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    pub unknowns: Vec<usize>,
    pub equations: Vec<usize>,
}

impl Network {
    /// The unknowns an equation reads. An `Expr` holding a `deref` may read
    /// any of them, so it reads them all: its island is conservative.
    pub fn vars_of(&self, eq: &Equation, index: &BTreeMap<Leaf, usize>) -> Vec<usize> {
        let mut vars: Vec<usize> = match eq {
            Equation::Linear(coefs) => coefs.iter().map(|(i, _)| *i).collect(),
            Equation::Expr { lhs, rhs, .. } if has_deref(lhs) || has_deref(rhs) => (0..self.unknowns.len()).collect(),
            Equation::Expr { lhs, rhs, .. } => lhs.deps().into_iter().chain(rhs.deps()).filter_map(|l| index.get(&l).copied()).collect(),
        };
        vars.sort();
        vars.dedup();
        vars
    }

    /// The islands, by lowest unknown. An equation over no unknown is in
    /// none (it constrains nothing the network solves).
    pub fn islands(&self) -> Vec<Island> {
        let index: BTreeMap<Leaf, usize> = self.unknowns.iter().enumerate().map(|(i, (l, _))| (*l, i)).collect();
        let mut parent: Vec<usize> = (0..self.unknowns.len()).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let vars: Vec<Vec<usize>> = self.equations.iter().map(|eq| self.vars_of(eq, &index)).collect();
        for vs in &vars {
            for w in vs.windows(2) {
                let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
        let mut islands: BTreeMap<usize, Island> = BTreeMap::new();
        for i in 0..self.unknowns.len() {
            let r = root(&mut parent, i);
            islands.entry(r).or_insert_with(|| Island { unknowns: Vec::new(), equations: Vec::new() }).unknowns.push(i);
        }
        for (e, vs) in vars.iter().enumerate() {
            if let Some(&first) = vs.first() {
                let r = root(&mut parent, first);
                islands.get_mut(&r).expect("island").equations.push(e);
            }
        }
        islands.into_values().collect()
    }
}

impl Network {
    /// An island's unknowns and its equations over them, with `Linear`
    /// coefficients indexing the island's unknowns.
    pub fn localise(&self, island: &Island) -> (Vec<(Leaf, crate::units::Dim)>, Vec<Equation>) {
        let local: BTreeMap<usize, usize> = island.unknowns.iter().enumerate().map(|(pos, &i)| (i, pos)).collect();
        let unknowns = island.unknowns.iter().map(|&i| self.unknowns[i]).collect();
        let equations = island
            .equations
            .iter()
            .map(|&e| match &self.equations[e] {
                Equation::Linear(coefs) => Equation::Linear(coefs.iter().map(|(i, c)| (local[i], *c)).collect()),
                eq => eq.clone(),
            })
            .collect();
        (unknowns, equations)
    }

    /// Every island as a native cell, by its lowest unknown.
    pub(crate) fn island_natives(&self) -> Vec<(crate::graph::NativeKey, IslandNative)> {
        self.islands()
            .iter()
            .map(|island| {
                let (unknowns, equations) = self.localise(island);
                let outputs: std::collections::BTreeSet<Leaf> = unknowns.iter().map(|(l, _)| *l).collect();
                let key = crate::graph::NativeKey::Island(*outputs.first().expect("an island has an unknown"));
                (key, IslandNative { outputs, unknowns, equations })
            })
            .collect()
    }
}

/// The magnitude a solve works in: a number, or a quantity in SI.
fn magnitude(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Quantity { si, .. } => Some(*si),
        _ => None,
    }
}

/// Solve one island from `x0`, reading what is not an unknown through
/// `read`: an island native's run (handoff §4.10).
pub(crate) fn solve_island(unknowns: &[(Leaf, crate::units::Dim)], equations: &[Equation], x0: Vec<f64>, read: &dyn Fn(Leaf) -> Option<Value>) -> iron_solve::Solution {
    let index: BTreeMap<Leaf, usize> = unknowns.iter().enumerate().map(|(i, (l, _))| (*l, i)).collect();
    let mut problem = iron_solve::Problem::uniform(x0, 1.0);
    for eq in equations {
        match eq {
            Equation::Linear(coefs) => {
                let vars: Vec<usize> = coefs.iter().map(|(i, _)| *i).collect();
                let k: Vec<f64> = coefs.iter().map(|(_, c)| *c).collect();
                problem.term(iron_solve::Term::new(iron_solve::Tier::Required, vars, 1, move |v, out| {
                    out[0] = v.iter().zip(&k).map(|(x, c)| x * c).sum();
                }));
            }
            Equation::Expr { lhs, rhs, .. } => {
                // The unknowns it reads by name; a `deref`'s reads go
                // through `read`.
                let mut vars: Vec<usize> = lhs.deps().into_iter().chain(rhs.deps()).filter_map(|l| index.get(&l).copied()).collect();
                vars.sort();
                vars.dedup();
                if vars.is_empty() {
                    continue;
                }
                let index = &index;
                problem.term(iron_solve::Term::new(iron_solve::Tier::Required, vars.clone(), 1, move |v, out| {
                    let at = |l: Leaf| match index.get(&l).and_then(|i| vars.iter().position(|j| j == i)) {
                        Some(pos) => Some(Value::quantity(v[pos], unknowns[vars[pos]].1)),
                        None => read(l),
                    };
                    let side = |e: &Expr| e.eval(&at).ok().as_ref().and_then(magnitude);
                    out[0] = match (side(lhs), side(rhs)) {
                        (Some(a), Some(b)) => a - b,
                        _ => 1e6,
                    };
                }));
            }
        }
    }
    // A network is a set of equations, not a drag: nothing should stay
    // where it was at the expense of an equation (the weak rows stalled a
    // kΩ–mA circuit short of the tolerance).
    let options = iron_solve::Options { max_iterations: 100, least_change: false, ..Default::default() };
    iron_solve::solve(&problem, &options)
}

/// Why an island kept its last solution.
pub(crate) fn unsettled(residual: f64) -> String {
    format!("the across/through network does not settle (residual {residual:.3e})")
}

/// An island of the network as a native cell (handoff §4.10, RE-5.3 step
/// 4): its unknowns are its outputs, solved from where it last left them,
/// reading through the graph what is not an unknown. One that does not
/// converge keeps its last solution.
#[derive(Debug, PartialEq)]
pub(crate) struct IslandNative {
    outputs: std::collections::BTreeSet<Leaf>,
    unknowns: Vec<(Leaf, crate::units::Dim)>,
    equations: Vec<Equation>,
}

impl crate::graph::Native for IslandNative {
    fn producer(&self) -> crate::graph::ProducerId {
        crate::graph::ProducerId::NETWORK
    }
    fn outputs(&self) -> &std::collections::BTreeSet<Leaf> {
        &self.outputs
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn compute(&self, cx: &mut crate::graph::Cx<'_>) -> Result<BTreeMap<Leaf, Value>, crate::graph::Failure> {
        let x0: Vec<f64> = self.unknowns.iter().map(|(l, _)| cx.previous(*l).as_ref().and_then(magnitude).unwrap_or(0.0)).collect();
        let solution = solve_island(&self.unknowns, &self.equations, x0, &|l| cx.read(l));
        if solution.status != iron_solve::Status::Converged {
            return Err(crate::graph::Failure::KeepLast(unsettled(solution.max_required_residual)));
        }
        Ok(self.unknowns.iter().zip(&solution.vars).map(|((l, dim), x)| (*l, Value::quantity(*x, *dim))).collect())
    }
}

fn has_deref(e: &Expr) -> bool {
    let mut found = false;
    e.walk(&mut |x| found |= matches!(x, Expr::Call(name, _) if name == "deref"));
    found
}

#[derive(Clone, Debug, PartialEq)]
pub enum Equation {
    Linear(Vec<(usize, f64)>),
    Expr { node: NodeId, lhs: Expr, rhs: Expr },
}

fn dim_of(unit: Option<&str>) -> crate::units::Dim {
    unit.and_then(|u| crate::value::Value::parse_quantity(&format!("1 {u}")).ok())
        .and_then(|v| v.as_quantity())
        .map_or(crate::units::Dim::NONE, |(_, d)| d)
}

pub fn network(doc: Tree<'_>) -> Network {
    let port_ty = crate::registry::TypeId::lookup("port").expect("registered");
    let pair_ports: Vec<(NodeId, &'static PortKind)> = doc
        .nodes_of_type(port_ty)
        .filter(|&p| doc.is_live(p) && doc.live(p).and_then(|n| n.parent).is_some_and(|e| doc.is_live(e)))
        .filter_map(|p| kind(&text(doc, p, "kind")?).filter(|k| k.flow == Flow::Pair).map(|k| (p, k)))
        .collect();
    let mut unknowns = Vec::new();
    let mut index: BTreeMap<Leaf, usize> = BTreeMap::new();
    for &(p, k) in &pair_ports {
        for (field, unit) in [("across", k.unit), ("through", k.through)] {
            let leaf = Leaf { node: p, path: LeafPath::new(FacetKind::Port, field).expect("declared") };
            if doc.leaf(p, leaf.path).is_some_and(|s| s.expr().is_none()) {
                index.insert(leaf, unknowns.len());
                unknowns.push((leaf, dim_of(unit)));
            }
        }
    }
    let mut equations = Vec::new();
    if pair_ports.is_empty() {
        return Network { unknowns, equations };
    }
    // Nets: union-find over pair connections.
    let mut parent: BTreeMap<NodeId, NodeId> = pair_ports.iter().map(|&(p, _)| (p, p)).collect();
    fn root(parent: &mut BTreeMap<NodeId, NodeId>, p: NodeId) -> NodeId {
        let mut r = p;
        while parent[&r] != r {
            r = parent[&r];
        }
        parent.insert(p, r);
        r
    }
    let conn_ty = crate::registry::TypeId::lookup("connection").expect("registered");
    for c in doc.nodes_of_type(conn_ty).filter(|&c| doc.is_live(c)) {
        let Some((a, b)) = ends(doc, c) else { continue };
        if parent.contains_key(&a) && parent.contains_key(&b) {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent.insert(ra, rb);
        }
    }
    let mut nets: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for &(p, _) in &pair_ports {
        let r = root(&mut parent, p);
        nets.entry(r).or_default().push(p);
    }
    let leaf = |p: NodeId, f: &str| Leaf { node: p, path: LeafPath::new(FacetKind::Port, f).expect("declared") };
    for ports in nets.values() {
        // Across values equal at the joint (bound ones are knowns: an
        // equation against a known is an Expr row).
        for w in ports.windows(2) {
            let (a, b) = (leaf(w[0], "across"), leaf(w[1], "across"));
            match (index.get(&a), index.get(&b)) {
                (Some(&i), Some(&j)) => equations.push(Equation::Linear(vec![(i, 1.0), (j, -1.0)])),
                _ => equations.push(Equation::Expr { node: w[0], lhs: Expr::Ref(a), rhs: Expr::Ref(b) }),
            }
        }
        // Through values sum to zero; alone, a terminal carries none.
        let throughs: Vec<(usize, f64)> = ports.iter().filter_map(|&p| index.get(&leaf(p, "through")).map(|&i| (i, 1.0))).collect();
        if !throughs.is_empty() {
            equations.push(Equation::Linear(throughs));
        }
    }
    let eq_ty = crate::registry::TypeId::lookup("equation").expect("registered");
    for e in doc.nodes_of_type(eq_ty).filter(|&e| doc.is_live(e)) {
        // A definition's own equations describe the template, not a part.
        if doc.live(e).and_then(|n| n.parent).and_then(|p| doc.live(p)).is_none_or(|n| n.ty.name() != "entity") {
            continue;
        }
        let Some(eq) = doc.get::<crate::facet::Equation>(e) else { continue };
        // A constant side is read where it is, not inlined: editing it is a
        // value change the island's native follows, not a new network.
        let side = |s: &crate::value::Slot, field: &str| s.expr().cloned().or_else(|| s.constant().map(|_| Expr::Ref(Leaf { node: e, path: LeafPath::new(FacetKind::Equation, field).expect("declared") })));
        if let (Some(lhs), Some(rhs)) = (side(&eq.lhs, "lhs"), side(&eq.rhs, "rhs")) {
            equations.push(Equation::Expr { node: e, lhs, rhs });
        }
    }
    Network { unknowns, equations }
}

