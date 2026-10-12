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

use crate::component::{ComponentKind, LeafPath, Port};
use crate::document::{Document, RelKind};
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
    pub unit: Option<&'static str>,
}

pub static KINDS: &[PortKind] = &[
    PortKind { name: "value", flow: Flow::Signal, unit: None },
    PortKind { name: "optical_power", flow: Flow::Signal, unit: Some("W") },
    PortKind { name: "rf_voltage", flow: Flow::Signal, unit: Some("V") },
    PortKind { name: "electrical", flow: Flow::Pair, unit: Some("V") },
    PortKind { name: "heat_flow", flow: Flow::Pair, unit: Some("K") },
];

pub fn kind(name: &str) -> Option<&'static PortKind> {
    KINDS.iter().find(|k| k.name == name)
}

fn text(doc: &Document, port: NodeId, field: &str) -> Option<String> {
    let p = doc.get::<Port>(port)?;
    let slot = match field {
        "kind" => &p.kind,
        _ => &p.dir,
    };
    slot.constant().and_then(|v| v.as_str()).map(str::to_owned)
}

fn signal(port: NodeId) -> Leaf {
    Leaf { node: port, path: LeafPath::new(ComponentKind::Port, "signal").expect("declared") }
}

/// A connection's two ends, `a` then `b`, if it has exactly those.
pub fn ends(doc: &Document, connection: NodeId) -> Option<(NodeId, NodeId)> {
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
fn direction(doc: &Document, a: NodeId, b: NodeId) -> Option<(NodeId, NodeId)> {
    match (text(doc, a, "dir").as_deref(), text(doc, b, "dir").as_deref()) {
        (Some("out"), Some("in")) => Some((a, b)),
        (Some("in"), Some("out")) => Some((b, a)),
        _ => None,
    }
}

/// Every in-port's signal as its partner's: one derived binding per live,
/// valid one-way connection.
pub fn derived_signals(doc: &Document) -> Vec<(Leaf, Expr)> {
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
pub fn validate_connection(doc: &Document, connection: NodeId) -> Result<(), String> {
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
        let (_, to) = direction(doc, a, b).ok_or_else(|| format!("{connection}: a {ka} connection joins an out port to an in port"))?;
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
fn signal_loop(doc: &Document) -> Option<Leaf> {
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
