//! What a cell reads of the document's structure, and what a write touched
//! (plans/specs/structure-in-the-graph.md §3).
//!
//! A cell reads through a recording [`crate::Tree`], which notes a [`Key`]
//! for each read; the document's mutation choke points note the keys each
//! write touched in a [`Touched`] log. The store hands the log to the graph
//! at the points where it primes cells (§3.3), and the graph marks stale
//! the cells that recorded a touched key. Nothing decides by hand what an
//! edit affects.

use crate::facet::FacetKind;
use crate::id::NodeId;

/// One thing a read depends on (§3.1). DS-9 has the four its slice, the
/// unpinned layouts, reads; a later phase adds the keys its cells need.
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

#[cfg(feature = "structure-oracle")]
thread_local! {
    static SILENCED: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// The canary (§3.5): silence one of the document's choke points on this
/// thread (`"set_order"`), so what it writes touches no key. The oracle must
/// then fail; if it does not, it is not checking. A reorder is the write
/// only keys reach in DS-9: a create, delete, move or facet change also
/// reaches layouts through `sync_layout` or `restructure`'s leaf priming.
#[cfg(feature = "structure-oracle")]
pub fn silence_for_canary(site: Option<&'static str>) {
    SILENCED.with(|s| s.set(site));
}

/// Whether the canary has silenced choke point `site`.
pub(crate) fn silenced(site: &str) -> bool {
    #[cfg(feature = "structure-oracle")]
    return SILENCED.with(|s| s.get() == Some(site));
    #[cfg(not(feature = "structure-oracle"))]
    {
        let _ = site;
        false
    }
}
