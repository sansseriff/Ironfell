//! Compositions, the model tree, and the `figures` relation between them
//! (entity-composition-model.md §5, §7).
//!
//! ```text
//! model                      composition "scene" (2d)      composition "assembly" (3d)
//!   entity "thing"  ◀──figures── rect "box"                    mesh "torus"
//!                   ◀──figures──────────────────────────────── ┘
//! ```
//!
//! A composition is a root whose subtree one panel shows. A figure is any
//! node with a `figures` relation to an entity; it is a role, not a type.
//! Everything here is a read over the document: which composition a node
//! is drawn in, which entity a node pictures, and every picture of an
//! entity. Nothing is stored that these functions can compute.

use crate::component::Composition;
use crate::document::{Document, RelKind};
use crate::id::NodeId;
use crate::registry::TypeId;

/// Whether a composition is drawn as 2D vector content or as a 3D scene.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dims {
    Two,
    Three,
}

/// The composition `id` is drawn in: itself or its nearest composition
/// ancestor. `None` for nodes outside every composition (the model tree,
/// the library, the timeline, and loose roots from older files).
pub fn composition_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    let mut cur = Some(id);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if n.ty == composition {
            return Some(n.id);
        }
        cur = n.parent;
    }
    None
}

/// Every live composition, in root order.
pub fn compositions(doc: &Document) -> Vec<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| n.ty == composition)).collect()
}

pub fn dims(doc: &Document, composition: NodeId) -> Option<Dims> {
    let c = doc.get::<Composition>(composition)?;
    Some(match c.dims.constant().and_then(|v| v.as_str()) {
        Some("3d") => Dims::Three,
        _ => Dims::Two,
    })
}

pub fn notation(doc: &Document, composition: NodeId) -> Option<String> {
    doc.get::<Composition>(composition)?.notation.constant()?.as_str().map(str::to_owned)
}

/// The entity `node` pictures directly: the target of its `figures`
/// relation, if that entity is live.
pub fn figured_by(doc: &Document, node: NodeId) -> Option<NodeId> {
    doc.relations_from(node).find(|r| r.rel == RelKind::Figures && doc.is_live(r.to)).map(|r| r.to)
}

/// The figure `node` belongs to and the entity it pictures: `node` itself
/// or its nearest ancestor with a `figures` relation, stopping at the
/// composition root. This is what hover and selection lift through, so
/// grabbing a part of a grouped figure selects the whole thing.
pub fn figure_of(doc: &Document, node: NodeId) -> Option<(NodeId, NodeId)> {
    let mut cur = Some(node);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if let Some(entity) = figured_by(doc, n.id) {
            return Some((n.id, entity));
        }
        cur = n.parent;
    }
    None
}

/// Every live figure of `entity`, in relation order.
pub fn figures_of(doc: &Document, entity: NodeId) -> Vec<NodeId> {
    doc.relations_to(entity).filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from)).map(|r| r.from).collect()
}

/// The `model` root, if the document has one.
pub fn model_root(doc: &Document) -> Option<NodeId> {
    let model = TypeId::lookup("model").expect("registered");
    doc.roots().iter().copied().find(|&r| doc.live(r).is_some_and(|n| n.ty == model))
}
