//! What a cell reads of the document's structure and wiring, and what a write
//! touched (plans/specs/engine.md §3.2).
//!
//! A cell reads through `Cx` and a recording [`crate::Tree`], which note a
//! [`Key`] for each read; the document's and graph's mutation choke points
//! note the keys each write touched in a [`Touched`] log. The graph takes the
//! log at pass start and marks stale the cells that recorded a touched key.

use crate::expr::Leaf;
use crate::facet::FacetKind;
use crate::id::{NodeId, RelationId};
use crate::registry::TypeId;

/// One thing a read depends on (engine.md §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Key {
    /// Whether `n` exists and is live, its type, its parent and its order.
    Node(NodeId),
    /// `n`'s child list, authored and copies merged.
    Children(NodeId),
    /// Which facets `n` carries.
    FacetSet(NodeId),
    /// Whether `n` carries the facet, and what it holds.
    Facet(NodeId, FacetKind),
    /// The authored/stored value of `leaf`.
    Leaf(Leaf),
    /// Live nodes of `ty` under top-level root `root`.
    In(NodeId, TypeId),
    /// Live nodes carrying `kind` under top-level root `root`.
    With(NodeId, FacetKind),
    /// Relations leaving or arriving at `n`.
    Relations(NodeId),
    /// One relation by id.
    Relation(RelationId),
    /// The document's top-level roots list.
    Roots,
    /// Graph wiring on `leaf`: whether it is bound, whether a producer drives
    /// it, or its binding/tracked dependency edges.
    Wiring(Leaf),
}

/// Where a recording tree notes what it reads.
pub(crate) type Recorder = std::cell::RefCell<Vec<Key>>;

/// The keys a document's writes touched since they were last taken. Kept
/// only in a store's documents (`on`), so a scratch document or a fragment
/// collects nothing. Outside a document's equality; a clone keeps `on` and
/// starts empty, as the journal does.
#[derive(Default)]
pub(crate) struct Touched {
    pub(crate) on: bool,
    pub(crate) keys: Vec<Key>,
}

impl Touched {
    pub(crate) fn note(&mut self, k: Key) {
        if self.on {
            self.keys.push(k);
        }
    }
}

impl Clone for Touched {
    fn clone(&self) -> Self {
        Touched { on: self.on, keys: Vec::new() }
    }
}

impl PartialEq for Touched {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Touched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} touched", self.keys.len())
    }
}

#[cfg(any(test, feature = "structure-oracle"))]
thread_local! {
    static SILENCED: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// The canary (engine.md §6): silence one mutation choke point on this thread
/// (e.g. `"set_order"`, `"set_leaf"`, `"insert_node"`, `"insert_relation"`),
/// so what it writes touches no key. Tests assert that silencing a choke
/// point causes its verification test to fail.
#[cfg(any(test, feature = "structure-oracle"))]
pub fn silence_for_canary(site: Option<&'static str>) {
    SILENCED.with(|s| s.set(site));
}

/// Whether the canary has silenced choke point `site`.
pub(crate) fn silenced(site: &str) -> bool {
    #[cfg(any(test, feature = "structure-oracle"))]
    return SILENCED.with(|s| s.get() == Some(site));
    #[cfg(not(any(test, feature = "structure-oracle")))]
    {
        let _ = site;
        false
    }
}
