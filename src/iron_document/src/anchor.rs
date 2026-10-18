//! Anchors (component-model.md, "anchors"): where ports sit on pictures,
//! derived into the reactive graph.
//!
//! An `empty` with an `anchor` facet marks a port on a picture. The graph
//! derives two things from anchors, alongside `port::derived_signals`:
//!
//! - every anchor's position in its composition, `anchor.wx`/`anchor.wy`,
//!   as an expression over its own and its ancestors' transform leaves
//!   (`translate · rotate · scale`, as `geom::local_affine`), so readers
//!   bind to a port's position and it follows any move of what holds it;
//!   and the direction it faces there, `anchor.wrot`: where its local +x
//!   points (component-model.md §4.3);
//! - for a `follow` anchor whose port is connected, its own
//!   `transform2d.x`/`y`: the partner's anchor position, taken back into
//!   the follower's parent space. A fiber's ends sit on what they plug into.
//!
//! Both are ordinary expressions with static dependencies, so they update
//! through the graph's usual invalidation, and a drag on a follower inverts
//! (`handle`) onto whatever placed the partner: dragging a fiber's end moves
//! the laser it is plugged into. A follower whose partner also follows is
//! left where its template put it, so anchors never loop.

use crate::composition;
use crate::document::{Document, RelKind};
use crate::expr::{BinOp, Expr, Leaf};
use crate::facet::{Anchor, FacetKind, LeafPath};
use crate::id::NodeId;
use crate::value::Value;

fn leaf(node: NodeId, facet: FacetKind, field: &str) -> Expr {
    Expr::Ref(Leaf { node, path: LeafPath::new(facet, field).expect("declared field") })
}
fn t(node: NodeId, field: &str) -> Expr {
    leaf(node, FacetKind::Transform2d, field)
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn call(name: &str, a: Expr) -> Expr {
    Expr::Call(name.to_owned(), vec![a])
}
fn num(n: f64) -> Expr {
    Expr::Lit(Value::Number(n))
}

/// Live ancestors of `id` with a 2D transform, nearest first, then `id`
/// itself first when `with_self`.
fn transformed_chain(doc: &Document, id: NodeId, with_self: bool) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut cur = if with_self { Some(id) } else { doc.live(id).and_then(|n| n.parent) };
    while let Some(n) = cur {
        if doc.facet(n, FacetKind::Transform2d).is_some() {
            out.push(n);
        }
        cur = doc.live(n).and_then(|x| x.parent);
    }
    out
}

/// The point `(px, py)` of `node`'s parent space (or own, `with_self`)
/// carried out to composition space.
fn outward(doc: &Document, node: NodeId, with_self: bool, mut px: Expr, mut py: Expr) -> (Expr, Expr) {
    for n in transformed_chain(doc, node, with_self) {
        let (c, s) = (call("cos", t(n, "rot")), call("sin", t(n, "rot")));
        let (ax, ay) = (bin(BinOp::Mul, t(n, "sx"), px), bin(BinOp::Mul, t(n, "sy"), py));
        px = bin(BinOp::Add, bin(BinOp::Sub, bin(BinOp::Mul, c.clone(), ax.clone()), bin(BinOp::Mul, s.clone(), ay.clone())), t(n, "x"));
        py = bin(BinOp::Add, bin(BinOp::Add, bin(BinOp::Mul, s, ax), bin(BinOp::Mul, c, ay)), t(n, "y"));
    }
    (px, py)
}

/// A composition-space point carried into `node`'s parent space.
fn inward(doc: &Document, node: NodeId, mut qx: Expr, mut qy: Expr) -> (Expr, Expr) {
    let mut chain = transformed_chain(doc, node, false);
    chain.reverse();
    for n in chain {
        let (c, s) = (call("cos", t(n, "rot")), call("sin", t(n, "rot")));
        let (dx, dy) = (bin(BinOp::Sub, qx, t(n, "x")), bin(BinOp::Sub, qy, t(n, "y")));
        qx = bin(BinOp::Div, bin(BinOp::Add, bin(BinOp::Mul, c.clone(), dx.clone()), bin(BinOp::Mul, s.clone(), dy.clone())), t(n, "sx"));
        qy = bin(BinOp::Div, bin(BinOp::Sub, bin(BinOp::Mul, c, dy), bin(BinOp::Mul, s, dx)), t(n, "sy"));
    }
    (qx, qy)
}

fn port_of(doc: &Document, anchor: NodeId) -> Option<NodeId> {
    doc.get::<Anchor>(anchor)?.port.constant()?.as_ref().filter(|&p| doc.is_live(p))
}

fn follows(doc: &Document, anchor: NodeId) -> bool {
    doc.get::<Anchor>(anchor).and_then(|a| a.follow.constant()).and_then(|v| v.as_bool()) == Some(true)
}

/// The port at the other end of `port`'s connection, and the connection. A
/// port inside a component has none of its own when the component exposes
/// it: its partner is the exposing port's (component-model.md §5.5).
fn partner(doc: &Document, port: NodeId) -> Option<(NodeId, NodeId)> {
    for _ in 0..16 {
        if let Some(connection) = doc.relations_to(port).find(|r| r.rel == RelKind::Connects && doc.is_live(r.from)).map(|r| r.from) {
            let (a, b) = crate::port::ends(doc, connection)?;
            return Some((if a == port { b } else { a }, connection));
        }
        let outer = doc.relations_to(port).find(|r| r.rel == RelKind::Exposes && doc.is_live(r.from))?.from;
        return partner(doc, outer);
    }
    None
}

/// The ports whose anchors stand for `port`: itself, and, when it is a
/// component's port, the inner port it exposes (whose picture is drawn).
fn standing_for(doc: &Document, port: NodeId) -> Vec<NodeId> {
    let mut out = vec![port];
    let mut at = port;
    for _ in 0..16 {
        match doc.relations_from(at).find(|r| r.rel == RelKind::Exposes) {
            Some(r) => {
                out.push(r.to);
                at = r.to;
            }
            None => break,
        }
    }
    out
}

/// Every derived anchor binding: positions, and followers' placements.
pub fn derived(doc: &Document) -> Vec<(Leaf, Expr)> {
    let anchors: Vec<NodeId> = doc.nodes_with(FacetKind::Anchor).filter(|&a| doc.is_live(a) && doc.live(a).is_some_and(|n| n.ty.name() == "empty")).collect();
    let mut out = Vec::new();
    for &a in &anchors {
        let (wx, wy) = outward(doc, a, true, num(0.0), num(0.0));
        let (fx, fy) = outward(doc, a, true, num(1.0), num(0.0));
        let facing = Expr::Call("atan2".to_owned(), vec![bin(BinOp::Sub, fy, wy.clone()), bin(BinOp::Sub, fx, wx.clone())]);
        out.push((Leaf { node: a, path: LeafPath::new(FacetKind::Anchor, "wx").expect("declared") }, wx));
        out.push((Leaf { node: a, path: LeafPath::new(FacetKind::Anchor, "wy").expect("declared") }, wy));
        out.push((Leaf { node: a, path: LeafPath::new(FacetKind::Anchor, "wrot").expect("declared") }, facing));
    }
    for &a in anchors.iter().filter(|&&a| follows(doc, a)) {
        let Some((other, connection)) = port_of(doc, a).and_then(|p| partner(doc, p)) else { continue };
        let here = composition::composition_of(doc, a);
        let ports = standing_for(doc, other);
        // A joint of two loose ends: the held one stays, the other follows
        // it (component-authoring.md §5).
        let held = crate::plug::held_end(doc, connection);
        let holds = |b: NodeId| !follows(doc, b) || held.is_some_and(|h| standing_for(doc, h).contains(&port_of(doc, b).unwrap_or(b)));
        let Some(target) = anchors.iter().copied().find(|&b| b != a && holds(b) && port_of(doc, b).is_some_and(|p| ports.contains(&p)) && composition::composition_of(doc, b) == here) else { continue };
        let (x, y) = inward(doc, a, leaf(target, FacetKind::Anchor, "wx"), leaf(target, FacetKind::Anchor, "wy"));
        out.push((Leaf { node: a, path: LeafPath::new(FacetKind::Transform2d, "x").expect("declared") }, x));
        out.push((Leaf { node: a, path: LeafPath::new(FacetKind::Transform2d, "y").expect("declared") }, y));
    }
    out
}
