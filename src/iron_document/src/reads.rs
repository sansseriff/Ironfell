//! What layout, and the bounds and text it measures, read (reactive-engine
//! handoff §4.10, RE-5.4): the tree and resolved values. The store reads them
//! as anything does; a layout native reads them through its `Cx`, which
//! tracks every value it reads, so one body of code serves both.

use crate::facet::LeafPath;
use crate::graph::ProducerId;
use crate::{Leaf, NodeId, StoreCore, Tree, Value};

pub trait Reads {
    fn tree(&self) -> Tree<'_>;
    fn resolved(&self, id: NodeId, path: LeafPath) -> Option<Value>;
    /// A leaf as layout must read it: beneath any layout claim (the
    /// authored constant, or the facet's default when the node lacks it),
    /// so its own output never feeds back into it.
    fn layout_base(&self, id: NodeId, path: LeafPath) -> Option<Value>;
    fn layout_preview(&self) -> Option<&crate::layout::Preview>;
}

/// Beneath every claim and binding: the authored constant, or the facet's
/// default.
pub(crate) fn constant_or_default(doc: Tree<'_>, id: NodeId, path: LeafPath) -> Option<Value> {
    match doc.leaf(id, path) {
        Some(slot) => slot.constant().cloned(),
        None => path.facet.default_facet().get(path.field).and_then(|s| s.constant().cloned()),
    }
}

impl Reads for StoreCore {
    fn tree(&self) -> Tree<'_> {
        StoreCore::tree(self)
    }
    fn resolved(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        StoreCore::resolved(self, id, path)
    }
    fn layout_base(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        match self.reactive.signal(Leaf { node: id, path }) {
            Some(s) if s.producer == ProducerId::LAYOUT => constant_or_default(StoreCore::tree(self), id, path),
            _ => StoreCore::resolved(self, id, path),
        }
    }
    fn layout_preview(&self) -> Option<&crate::layout::Preview> {
        StoreCore::layout_preview(self)
    }
}
