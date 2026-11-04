//! Shared geometric controls. A point keeps its contour parent; ordinary
//! bindings carry a control-local offset into that contour's coordinate frame.
use crate::{Document, Expr, FacetKind, Leaf, LeafPath, LeafStruct, NodeId, Op, Slot, Store, Value};
use crate::facet::Attachment;

fn path(s: &str) -> LeafPath { s.parse().expect("declared leaf") }
fn r(id: NodeId, s: &str) -> Expr { Expr::Ref(Leaf { node: id, path: path(s) }) }

/// Authoring targets are plain geometry or one level inside a main, never
/// an instance override or an unexposed nested component's internals.
pub fn authored(doc: &Document, id: NodeId) -> Result<NodeId, String> {
    if !doc.is_live(id) { return Err(format!("{id} no longer exists")); }
    if !id.is_expanded() { return Ok(id); }
    let p = id.path();
    if p.len() == 2 && crate::expand::main_definition(doc, p[0]).is_some() { Ok(p[1]) }
    else { Err("Edit the component's main to attach geometry".into()) }
}

fn scope(doc: &Document, mut id: NodeId) -> Option<NodeId> {
    while let Some(n) = doc.live(id) {
        if matches!(n.ty.name(), "template" | "composition") { return Some(id); }
        id = n.parent?;
    }
    None
}

fn pair(doc: &Document, point: NodeId, control: NodeId) -> Result<(), String> {
    if doc.live(point).map(|n| n.ty.name()) != Some("point") { return Err(format!("{point} is not a path point")); }
    if doc.live(control).map(|n| n.ty.name()) != Some("empty") { return Err(format!("Control {control} is missing; detach its points before deleting it")); }
    if scope(doc, point).is_none() || scope(doc, point) != scope(doc, control) {
        return Err("Points and control must belong to the same composition or component template".into());
    }
    Ok(())
}

pub fn bindings(doc: &Document, point: NodeId, control: NodeId) -> [Expr; 2] {
    let (x, y) = crate::anchor::outward(doc, control, true, r(point, "attachment.dx"), r(point, "attachment.dy"));
    let (x, y) = crate::anchor::inward(doc, point, x, y);
    [x, y]
}

/// Attach atomically, keeping placement. Existing drivers must be detached
/// explicitly; they are never silently overwritten.
pub fn attach(store: &Store, control: NodeId, points: &[NodeId]) -> Result<Vec<Op>, String> {
    let doc = store.document();
    let control = authored(doc, control)?;
    let inv = store.world_affine(control).inverse().ok_or("The control has a zero scale")?;
    if points.is_empty() { return Err("Pick at least one path point".into()); }
    let mut ops = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for &shown in points {
        let p = authored(doc, shown)?;
        if !seen.insert(p) { continue; }
        pair(doc, p, control)?;
        if doc.facet(p, FacetKind::Attachment).is_some() { return Err(format!("{p} is already attached; detach it first")); }
        for f in ["transform2d.x", "transform2d.y"] {
            if doc.leaf(p, path(f)).and_then(Slot::constant).is_none() || !matches!(store.handle(p, path(f)), crate::Handle::Direct) {
                return Err(format!("{p}.{f} already has a driver; remove that driver first"));
            }
        }
        let parent = doc.live(p).and_then(|n| n.parent).ok_or("Point has no contour")?;
        if store.world_affine(parent).inverse().is_none() { return Err("The path has a zero scale".into()); }
        let offset = inv.apply(store.world_affine(p).translation());
        if !offset.iter().all(|v| v.is_finite()) { return Err("Attachment offset is not finite".into()); }
        let mut a = Attachment::default();
        a.control = Slot::Const(Value::Ref(control));
        a.dx = Slot::Const(Value::Number(offset[0]));
        a.dy = Slot::Const(Value::Number(offset[1]));
        ops.push(Op::AddFacet { id: p, facet: a.wrap() });
        for (f, expr) in ["transform2d.x", "transform2d.y"].into_iter().zip(bindings(doc, p, control)) {
            ops.push(Op::Set { id: p, path: path(f), slot: Slot::Bound(expr) });
        }
    }
    Ok(ops)
}

pub fn detach(store: &Store, points: &[NodeId]) -> Result<Vec<Op>, String> {
    let mut ops = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for &shown in points {
        let p = authored(store.document(), shown)?;
        if !seen.insert(p) { continue; }
        if store.document().get::<Attachment>(p).is_none() { return Err(format!("{p} is not attached")); }
        for f in ["transform2d.x", "transform2d.y"] {
            let v = store.resolved(p, path(f)).ok_or("Cannot resolve point position")?;
            ops.push(Op::Set { id: p, path: path(f), slot: Slot::Const(v) });
        }
        ops.push(Op::RemoveFacet { id: p, kind: FacetKind::Attachment });
    }
    if ops.is_empty() { return Err("No attached points selected".into()); }
    Ok(ops)
}

/// Keep metadata and bindings in agreement through every entry point. A
/// reparent of attached geometry must first detach it, preserving placement.
pub fn validate(previous: &Document, doc: &Document) -> Result<(), String> {
    for point in previous.nodes_with(FacetKind::Attachment).filter(|point| !point.is_expanded() && doc.is_live(*point) && doc.get::<Attachment>(*point).is_none()) {
        if ["transform2d.x", "transform2d.y"].into_iter().any(|field| doc.leaf(point, path(field)).and_then(Slot::constant).is_none()) {
            return Err(format!("{point}: detach before removing attachment metadata"));
        }
    }
    for p in doc.nodes_with(FacetKind::Attachment).filter(|p| doc.is_live(*p) && !p.is_expanded()) {
        let a = doc.get::<Attachment>(p).expect("indexed");
        let control = a.control.constant().and_then(Value::as_ref).ok_or("An attachment needs a constant control reference")?;
        pair(doc, p, control)?;
        for (f, want) in ["transform2d.x", "transform2d.y"].into_iter().zip(bindings(doc, p, control)) {
            if doc.leaf(p, path(f)).and_then(Slot::expr) != Some(&want) {
                return Err(format!("{p}: detach before replacing its position bindings or reparenting its geometry"));
            }
        }
    }
    Ok(())
}
