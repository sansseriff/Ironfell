//! Gesture ownership (plans/specs/infinite-canvas-architecture-patterns.md §4).
//!
//! A gesture is the store's previews between a press and a release: a drag,
//! a resize, a slider scrub, a 3D move. It has an identity and a set of
//! *watched* nodes, the ones whose pose it interprets the pointer against.
//! While it runs, other transactions keep arriving: a model edit, an
//! inspector field, undo. Each is classified against the gesture:
//!
//! - **Compatible**: it touches nothing the gesture depends on (a colour, a
//!   caption, another node). The gesture carries on. The transaction is
//!   applied and solved as if the gesture were not there, then the previews
//!   are put back on top.
//! - **Conflicting**: it deletes or reparents a watched node or an ancestor,
//!   sets one of their geometry leaves or binds any of their leaves, sets a
//!   leaf the gesture drives, links a constraint or a clip to a watched node,
//!   or makes the solver move a watched node. The gesture is cancelled: its
//!   previews are dropped and the document, with the edit, shows. Its release
//!   commits nothing, so a stale pose never overwrites newer intent.
//!
//! Undo and redo cancel a gesture outright. The gesture's own commit never
//! conflicts with itself.
//!
//! The lifecycle is the same for 2D and 3D, for box gestures, point handles,
//! sliders and drags through a plane: the tools differ only in how a pointer
//! becomes leaf values.

use crate::facet::FacetKind;
use crate::document::{Document, RelKind};
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::op::Op;
use std::collections::BTreeSet;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GestureId(pub u64);

impl std::fmt::Display for GestureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "g{}", self.0)
    }
}

/// Where a gesture stands.
#[derive(Clone, Debug, PartialEq)]
pub enum GestureStatus {
    Active,
    /// Released; its previews became a transaction (or nothing had moved).
    Committed,
    /// Dropped with no transaction: Esc, a conflicting edit, undo, or a
    /// newer gesture. Says why.
    Cancelled(String),
    /// Not a gesture this store knows: an older one, or one from before the
    /// document was replaced. Treat it as over.
    Unknown,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveGesture {
    pub id: GestureId,
    pub nodes: BTreeSet<NodeId>,
}

/// The previews of a gesture set aside while another transaction applies.
pub(crate) struct Suspended {
    pub values: Vec<(Leaf, crate::value::Value)>,
    pub drivers: BTreeSet<Leaf>,
    pub fingers: Vec<crate::solve::Finger>,
}

/// Facets that place or shape a node: what a gesture reads to turn the
/// pointer into values. A change to one of these under a gesture is a
/// conflict; a colour or a name is not.
const GEOMETRY: &[FacetKind] = &[
    FacetKind::Transform2d,
    FacetKind::Transform3d,
    FacetKind::Size,
    FacetKind::Radius,
    FacetKind::Corners,
    FacetKind::Arc,
    FacetKind::Handles,
    FacetKind::PathShape,
    FacetKind::Contour,
    FacetKind::Frame,
    FacetKind::GroupClip,
    FacetKind::Slider,
];

fn watched(doc: &Document, g: &ActiveGesture, n: NodeId) -> bool {
    g.nodes.contains(&n) || g.nodes.iter().any(|&w| doc.is_ancestor(n, w))
}

/// Why `ops`, about to apply to `doc`, conflict with the gesture; `None`
/// when they are compatible.
pub(crate) fn conflict(doc: &Document, ops: &[Op], g: &ActiveGesture, drivers: &BTreeSet<Leaf>) -> Option<String> {
    for op in ops {
        let reason = match op {
            Op::Delete { id } if watched(doc, g, *id) => format!("{id} was deleted"),
            Op::Reparent { id, .. } if watched(doc, g, *id) => format!("{id} moved to another parent"),
            Op::Create { id, .. } if g.nodes.contains(id) => format!("{id} was replaced"),
            Op::Set { id, path, .. } if drivers.contains(&Leaf { node: *id, path: *path }) => format!("{id}.{path}, which the gesture drives, was set"),
            Op::Set { id, path, slot } if watched(doc, g, *id) && (GEOMETRY.contains(&path.facet) || slot.expr().is_some()) => format!("{id}.{path} was set"),
            Op::AddFacet { id, facet } if watched(doc, g, *id) && GEOMETRY.contains(&facet.kind()) => format!("{id} gained {}", facet.kind().name()),
            Op::RemoveFacet { id, kind } if watched(doc, g, *id) && GEOMETRY.contains(kind) => format!("{id} lost {}", kind.name()),
            Op::Link { rel } if matches!(rel.rel, RelKind::Constrains | RelKind::Animates) && g.nodes.contains(&rel.to) => {
                format!("{} now {} {}", rel.from, rel.rel.name(), rel.to)
            }
            Op::Unlink { id } => match doc.relation(*id) {
                Some(r) if matches!(r.rel, RelKind::Constrains | RelKind::Animates) && g.nodes.contains(&r.to) => format!("{} no longer {} {}", r.from, r.rel.name(), r.to),
                _ => continue,
            },
            _ => continue,
        };
        return Some(reason);
    }
    None
}

/// Why the solver's writes for another transaction conflict: they moved a
/// watched node.
pub(crate) fn moved(derived: &[Op], g: &ActiveGesture) -> Option<String> {
    derived.iter().find_map(|op| match op {
        Op::Set { id, path, .. } if g.nodes.contains(id) => Some(format!("the solver moved {id}.{path}")),
        _ => None,
    })
}
