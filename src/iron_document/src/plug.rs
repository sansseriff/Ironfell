//! Plugging and mates (component-model.md §4.3).
//!
//! A port's anchor is where it sits on a picture and the way it faces
//! (`anchor.rs`). This module reads them back as [`PortAnchor`]s, decides
//! which ports may be plugged together, writes the ops that connect them,
//! and derives the **mates** that connections between two rigid anchors
//! imply: one `mate` constraint per such connection, held by the solver
//! (`solve.rs`) and never stored. The connection is the one authored fact.

use crate::constraint::{self, ConstraintInfo, Operand};
use crate::document::{Document, RelKind, Relation};
use crate::expr::Leaf;
use crate::facet::{Anchor, Connection, FacetKind, LeafPath, LeafStruct, Name, Port};
use crate::geom;
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::Value;
use std::collections::BTreeMap;

/// One port's anchor on one picture, as a plug gesture sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct PortAnchor {
    /// The `empty` carrying the anchor.
    pub anchor: NodeId,
    /// The port it marks.
    pub port: NodeId,
    /// The port a connection from outside reaches it through: the outermost
    /// one exposing it, or `port` itself.
    pub outer: NodeId,
    /// `outer` is inside a component that does not expose it, so nothing
    /// outside may connect to it (§8).
    pub sealed: bool,
    pub follow: bool,
    pub kind: String,
    pub dir: String,
    /// The port's name, for labels.
    pub name: String,
    pub composition: Option<NodeId>,
    /// What carries the anchor: its outermost authored ancestor below the
    /// composition. A mate holds bodies; a rigid plug moves one.
    pub body: NodeId,
    /// Where it is and the way it faces, in its composition.
    pub world: [f64; 2],
    pub facing: f64,
    /// The connection on `outer`, and the port at its other end.
    pub connection: Option<NodeId>,
    pub partner: Option<NodeId>,
}

fn anchor_leaf(id: NodeId, field: &str) -> Leaf {
    Leaf { node: id, path: LeafPath::new(FacetKind::Anchor, field).expect("declared") }
}

fn port_text(doc: &Document, port: NodeId, field: &str) -> String {
    doc.get::<Port>(port)
        .and_then(|p| if field == "kind" { p.kind.constant() } else { p.dir.constant() })
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

/// The outermost port exposing `port`.
fn outer_port(doc: &Document, mut port: NodeId) -> NodeId {
    for _ in 0..16 {
        match doc.relations_to(port).find(|r| r.rel == RelKind::Exposes && doc.is_live(r.from)) {
            Some(r) => port = r.from,
            None => break,
        }
    }
    port
}

/// The connection on `port` and the port at its other end.
pub fn connection_of(doc: &Document, port: NodeId) -> Option<(NodeId, NodeId)> {
    let c = doc.relations_to(port).find(|r| r.rel == RelKind::Connects && doc.is_live(r.from))?.from;
    let (a, b) = crate::port::ends(doc, c)?;
    Some((c, if a == port { b } else { a }))
}

/// A node's outermost authored ancestor below its composition (itself
/// when it is one).
fn body_of(doc: &Document, id: NodeId) -> NodeId {
    let mut at = id.expanded_root().unwrap_or(id);
    while let Some(p) = doc.live(at).and_then(|n| n.parent) {
        if doc.live(p).is_some_and(|n| n.ty.name() == "composition") {
            break;
        }
        at = p;
    }
    at
}

/// Every port anchor in the document, with resolved positions.
pub fn port_anchors(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>) -> Vec<PortAnchor> {
    let num = |l: Leaf| read(l).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut out = Vec::new();
    for a in doc.nodes_with(FacetKind::Anchor) {
        if !doc.live(a).is_some_and(|n| n.ty.name() == "empty") {
            continue;
        }
        let Some(facet) = doc.get::<Anchor>(a) else { continue };
        let Some(port) = facet.port.constant().and_then(|v| v.as_ref()).filter(|&p| doc.live(p).is_some_and(|n| n.ty.name() == "port")) else { continue };
        let composition = crate::composition::composition_of(doc, a);
        // An anchor inside a definition's template marks where the port
        // will sit; only placed pictures plug.
        if composition.is_none() {
            continue;
        }
        let outer = outer_port(doc, port);
        let sealed = outer.is_expanded() && outer.path().len() > 2;
        let (connection, partner) = connection_of(doc, outer).map_or((None, None), |(c, p)| (Some(c), Some(p)));
        out.push(PortAnchor {
            anchor: a,
            port,
            outer,
            sealed,
            follow: facet.follow.constant().and_then(|v| v.as_bool()) == Some(true),
            kind: port_text(doc, outer, "kind"),
            dir: port_text(doc, outer, "dir"),
            name: doc.get::<Name>(outer).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).unwrap_or("").to_owned(),
            composition,
            body: body_of(doc, a),
            world: [num(anchor_leaf(a, "wx")), num(anchor_leaf(a, "wy"))],
            facing: num(anchor_leaf(a, "wrot")),
            connection,
            partner,
        });
    }
    out
}

/// Whether the port `grabbed` is dragged from may be plugged into
/// `target`'s, and why not. `grabbed`'s own connection is taken as gone
/// (a plug drag unplugs first).
pub fn compatible(doc: &Document, grabbed: &PortAnchor, target: &PortAnchor) -> Result<(), String> {
    if target.sealed {
        return Err(format!("{} is inside a component that does not expose it", target.port));
    }
    if grabbed.body == target.body {
        return Err("both ports belong to one part".into());
    }
    if grabbed.composition != target.composition {
        return Err("the ports are in different compositions".into());
    }
    if grabbed.follow && target.follow {
        return Err("two loose ends: neither would hold the other".into());
    }
    if grabbed.kind != target.kind {
        return Err(format!("{} carries {}, {} carries {}", grabbed.name, grabbed.kind, target.name, target.kind));
    }
    let flow = crate::port::kind(&grabbed.kind).map(|k| k.flow);
    if flow == Some(crate::port::Flow::Signal) {
        let dirs = (grabbed.dir.as_str(), target.dir.as_str());
        if !matches!(dirs, ("out", "in") | ("in", "out")) {
            return Err(format!("a {} connection joins an out port to an in port", grabbed.kind));
        }
    }
    if let Some(p) = target.partner
        && p != grabbed.outer
    {
        return Err(format!("{} is already connected", target.name));
    }
    let _ = doc;
    Ok(())
}

/// The ops connecting `a` to `b` (outer ports): a `connection` beside the
/// entity of `a`, with its two ends. `id` and `rels` are fresh.
pub fn plug_ops(doc: &Document, a: NodeId, b: NodeId, id: NodeId, rels: [crate::id::RelationId; 2]) -> Result<Vec<Op>, String> {
    let entity = a.expanded_root().unwrap_or(a);
    let parent = doc.live(entity).and_then(|n| n.parent).ok_or_else(|| format!("{a} belongs to no placed entity"))?;
    let order = doc.children(parent).iter().rev().find(|c| !c.is_expanded()).and_then(|&c| doc.live(c)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    let ty = TypeId::lookup("connection").expect("registered");
    Ok(vec![
        Op::Create { id, ty, parent: Some(parent), order, facets: vec![Connection::default().wrap()] },
        Op::Link { rel: Relation { id: rels[0], from: id, to: a, rel: RelKind::Connects, data: end("a") } },
        Op::Link { rel: Relation { id: rels[1], from: id, to: b, rel: RelKind::Connects, data: end("b") } },
    ])
}

/// The mates connections imply (§4.3): for every connection between two
/// rigid anchors on different bodies in one composition, a `mate` between
/// the bodies at the anchors, named by the connection.
pub fn implied_mates(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>) -> Vec<ConstraintInfo> {
    let spec = constraint::spec("mate").expect("declared kind");
    let anchors = port_anchors(doc, read);
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for a in anchors.iter().filter(|a| !a.follow && !a.sealed) {
        let (Some(c), Some(partner)) = (a.connection, a.partner) else { continue };
        if !seen.insert((c, a.composition)) {
            continue;
        }
        let Some(b) = anchors.iter().find(|b| b.outer == partner && !b.follow && !b.sealed && b.composition == a.composition) else { continue };
        if a.body == b.body {
            continue;
        }
        let operand = |role: &'static str, x: &PortAnchor| -> Option<Operand> {
            doc.facet(x.body, FacetKind::Transform2d)?;
            // The anchor's frame in its body's space.
            let m = geom::world_affine(doc, read, x.body).inverse()?.then(geom::world_affine(doc, read, x.anchor));
            let [ma, mb, ..] = m.0;
            Some(Operand { role, node: x.body, anchor: m.apply([0.0, 0.0]), at: None, turn: mb.atan2(ma) })
        };
        let (Some(oa), Some(ob)) = (operand("a", a), operand("b", b)) else { continue };
        out.push(ConstraintInfo { id: c, spec, value: 0.0, enabled: true, operands: vec![oa, ob] });
    }
    out
}
