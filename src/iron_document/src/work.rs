//! Work counted where whole-document passes would happen
//! (plans/handoff-scaling.md §5, DS-1): every node, relation, binding and
//! map entry a pass visits adds to a counter for its site. The scaling test
//! runs one operation in documents of several sizes and requires the same
//! counts: a pass over what the operation does not touch or show grows with
//! the document, and fails it.
//!
//! Counted only with the `work` feature (tests); otherwise every call is
//! empty and compiled away. Counters are per thread, so tests running in
//! parallel do not mix, and counting never allocates, so an allocation
//! counter beside it reads true.

// A tripwire for whole-document walks inside a transaction or a frame
// (`EN-9`): in debug builds (every test run), `Document::nodes` and
// `live_nodes`, which always visit every node, panic while a transaction
// applies or a frame advances, unless the caller holds `allow_scan`. The
// scaling test measures what grows; this catches a new full walk in any
// test that reaches it.
#[cfg(debug_assertions)]
thread_local! {
    static HOT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static ALLOWED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Held while a transaction applies or a frame advances.
pub(crate) struct Hot;

impl Drop for Hot {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        HOT.with(|h| h.set(h.get() - 1));
    }
}

pub(crate) fn hot() -> Hot {
    #[cfg(debug_assertions)]
    HOT.with(|h| h.set(h.get() + 1));
    Hot
}

/// Held by a caller that walks the whole document on purpose (export, a
/// save, a rebuild), naming why.
pub struct AllowScan;

impl Drop for AllowScan {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        ALLOWED.with(|a| a.set(a.get() - 1));
    }
}

pub fn allow_scan(_why: &'static str) -> AllowScan {
    #[cfg(debug_assertions)]
    ALLOWED.with(|a| a.set(a.get() + 1));
    AllowScan
}

/// A walk of every node is starting: panics inside a transaction or a frame
/// without `allow_scan` (debug builds).
#[inline]
pub(crate) fn whole_document(what: &'static str) {
    #[cfg(debug_assertions)]
    if HOT.with(|h| h.get()) > 0 && ALLOWED.with(|a| a.get()) == 0 {
        panic!("{what} walks the whole document inside a transaction or a frame (EN-9): scope the walk, or hold work::allow_scan(\"why\")");
    }
    #[cfg(not(debug_assertions))]
    let _ = what;
}

/// Where work is counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Site {
    /// `Document::nodes` and `live_nodes`: every node.
    Nodes,
    /// `Document::roots`: every root.
    Roots,
    /// `Document::descendants` and `descendants_within`.
    Descendants,
    /// `Document::nodes_of_type`.
    NodesOfType,
    /// `Document::nodes_with`.
    NodesWith,
    /// `Document::bound_slots`: every facet of every node.
    BoundSlots,
    /// `Document::relations`: every relation.
    Relations,
    /// A large map's entries iterated (`Counted`).
    Map,
}

impl Site {
    pub const ALL: [Site; 8] = [Site::Nodes, Site::Roots, Site::Descendants, Site::NodesOfType, Site::NodesWith, Site::BoundSlots, Site::Relations, Site::Map];

    pub fn name(self) -> &'static str {
        match self {
            Site::Nodes => "nodes",
            Site::Roots => "roots",
            Site::Descendants => "descendants",
            Site::NodesOfType => "nodes_of_type",
            Site::NodesWith => "nodes_with",
            Site::BoundSlots => "bound_slots",
            Site::Relations => "relations",
            Site::Map => "maps",
        }
    }
}

#[cfg(feature = "work")]
thread_local! {
    static COUNTS: [std::cell::Cell<u64>; 8] = const { [const { std::cell::Cell::new(0) }; 8] };
    /// Map entries by the map's name, for naming a pass (`Counted`).
    static MAPS: [std::cell::Cell<u64>; MAP_NAMES.len()] = const { [const { std::cell::Cell::new(0) }; MAP_NAMES.len()] };
}

/// The large maps `Counted` wraps, by the name a report gives them.
pub const MAP_NAMES: [&str; 23] = [
    "graph.nodes",
    "graph.dependents",
    "graph.natives",
    "graph.unneeded",
    "graph.runtime",
    "graph.held",
    "graph.owned",
    "graph.claims",
    "graph.base_readers",
    "graph.derived",
    "graph.resolved",
    "store.presence",
    "store.item_memo",
    "store.repeats",
    "store.observers",
    "store.clusters",
    "machines",
    "steps",
    "queries",
    "clocks",
    "other",
    "graph.readers",
    "producer.published",
];

/// Indexes into `MAP_NAMES`, as `Counted`'s parameter.
pub const GRAPH_NODES: usize = 0;
pub const GRAPH_DEPENDENTS: usize = 1;
pub const GRAPH_NATIVES: usize = 2;
pub const GRAPH_UNNEEDED: usize = 3;
pub const GRAPH_RUNTIME: usize = 4;
pub const GRAPH_HELD: usize = 5;
pub const GRAPH_OWNED: usize = 6;
pub const GRAPH_CLAIMS: usize = 7;
pub const GRAPH_BASE_READERS: usize = 8;
pub const GRAPH_DERIVED: usize = 9;
pub const GRAPH_RESOLVED: usize = 10;
pub const STORE_PRESENCE: usize = 11;
pub const STORE_ITEM_MEMO: usize = 12;
pub const STORE_REPEATS: usize = 13;
pub const STORE_OBSERVERS: usize = 14;
pub const STORE_CLUSTERS: usize = 15;
pub const MACHINES: usize = 16;
pub const STEPS: usize = 17;
pub const QUERIES: usize = 18;
pub const CLOCKS: usize = 19;
/// The readers of one leaf or native, as a pass visits them (`Readers`).
pub const GRAPH_READERS: usize = 21;
pub const PRODUCER_PUBLISHED: usize = 22;

/// Count `n` units of work at `site`.
#[inline(always)]
pub fn visit(site: Site, n: usize) {
    #[cfg(feature = "work")]
    {
        COUNTS.with(|c| {
            let c = &c[site as usize];
            c.set(c.get() + n as u64);
        });
        trace_hit(site.name(), n);
    }
    #[cfg(not(feature = "work"))]
    let _ = (site, n);
}

#[cfg(feature = "work")]
thread_local! {
    /// The count being traced (`trace`), and the stacks that hit it, by
    /// the store functions on them.
    static TRACING: std::cell::RefCell<Option<(&'static str, BTreeMap<String, u64>)>> = const { std::cell::RefCell::new(None) };
}

/// Record, on this thread, the call stacks that add to the count `name`
/// (a site or a map), until `take_trace`: which pass a count comes from.
/// Diagnostic only: it allocates.
#[cfg(feature = "work")]
pub fn trace(name: &'static str) {
    TRACING.with(|t| *t.borrow_mut() = Some((name, BTreeMap::new())));
}

/// The stacks recorded since `trace`, each with the work it added.
#[cfg(feature = "work")]
pub fn take_trace() -> BTreeMap<String, u64> {
    TRACING.with(|t| t.borrow_mut().take().map(|(_, m)| m).unwrap_or_default())
}

#[cfg(feature = "work")]
fn trace_hit(name: &'static str, n: usize) {
    let on = TRACING.with(|t| t.try_borrow().ok().and_then(|t| t.as_ref().map(|(k, _)| *k == name)).unwrap_or(false));
    if !on || n == 0 {
        return;
    }
    let bt = std::backtrace::Backtrace::force_capture().to_string();
    let frames: Vec<String> = bt
        .lines()
        .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f.to_owned()))
        .filter(|f| f.contains("iron_document") && !f.contains("work::") && !f.starts_with("core::") && !f.starts_with("alloc::") && !f.starts_with("<core::") && !f.starts_with("<alloc::") && !f.starts_with("<&mut") && !f.contains("{closure") && !f.contains("inspect"))
        .map(|f| f.replace("iron_document::", "").replace("<StoreCore>::", "Store::").replace("<graph::Reactive>::", "Reactive::"))
        .map(|f| f.rsplit_once("::h").map_or(f.clone(), |(a, _)| a.to_owned()))
        .take(6)
        .collect();
    let key = frames.join(" < ");
    TRACING.with(|t| {
        if let Ok(mut t) = t.try_borrow_mut()
            && let Some((_, m)) = t.as_mut()
        {
            *m.entry(key).or_default() += n as u64;
        }
    });
}

/// Count `n` entries of the map named `MAP_NAMES[map]`.
#[inline(always)]
pub fn visit_map(map: usize, n: usize) {
    #[cfg(feature = "work")]
    {
        visit(Site::Map, n);
        MAPS.with(|c| {
            let c = &c[map.min(MAP_NAMES.len() - 1)];
            c.set(c.get() + n as u64);
        });
        trace_hit(MAP_NAMES[map.min(MAP_NAMES.len() - 1)], n);
    }
    #[cfg(not(feature = "work"))]
    let _ = (map, n);
}

/// The counts so far on this thread, by site, then by map; and reset them.
#[cfg(feature = "work")]
pub fn take() -> (Vec<(&'static str, u64)>, Vec<(&'static str, u64)>) {
    let sites = COUNTS.with(|c| Site::ALL.iter().map(|s| (s.name(), c[*s as usize].replace(0))).collect());
    let maps = MAPS.with(|c| MAP_NAMES.iter().enumerate().map(|(i, n)| (*n, c[i].replace(0))).collect());
    (sites, maps)
}

/// A map or set whose iteration is counted (`MAP_NAMES[N]`): its own
/// `iter`, `keys`, `values`, `range` and `&`-iteration count each entry
/// they yield; everything else reaches the inner map through `Deref`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Counted<M, const N: usize>(pub M);

impl<M: std::fmt::Debug, const N: usize> std::fmt::Debug for Counted<M, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<M, const N: usize> std::ops::Deref for Counted<M, N> {
    type Target = M;
    fn deref(&self) -> &M {
        &self.0
    }
}

impl<M, const N: usize> std::ops::DerefMut for Counted<M, N> {
    fn deref_mut(&mut self) -> &mut M {
        &mut self.0
    }
}

impl<M, const N: usize> From<M> for Counted<M, N> {
    fn from(m: M) -> Self {
        Counted(m)
    }
}

/// Counts each item as it is yielded.
pub struct Counting<I, const N: usize>(I);

impl<I: Iterator, const N: usize> Iterator for Counting<I, N> {
    type Item = I::Item;
    #[inline]
    fn next(&mut self) -> Option<I::Item> {
        let x = self.0.next();
        if x.is_some() {
            visit_map(N, 1);
        }
        x
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<I: ExactSizeIterator, const N: usize> ExactSizeIterator for Counting<I, N> {}

impl<I: std::iter::FusedIterator, const N: usize> std::iter::FusedIterator for Counting<I, N> {}

impl<I: DoubleEndedIterator, const N: usize> DoubleEndedIterator for Counting<I, N> {
    fn next_back(&mut self) -> Option<I::Item> {
        let x = self.0.next_back();
        if x.is_some() {
            visit_map(N, 1);
        }
        x
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeBounds;

impl<K: Ord, V, const N: usize> Counted<BTreeMap<K, V>, N> {
    pub fn iter(&self) -> Counting<std::collections::btree_map::Iter<'_, K, V>, N> {
        Counting(self.0.iter())
    }
    pub fn iter_mut(&mut self) -> Counting<std::collections::btree_map::IterMut<'_, K, V>, N> {
        Counting(self.0.iter_mut())
    }
    pub fn keys(&self) -> Counting<std::collections::btree_map::Keys<'_, K, V>, N> {
        Counting(self.0.keys())
    }
    pub fn values(&self) -> Counting<std::collections::btree_map::Values<'_, K, V>, N> {
        Counting(self.0.values())
    }
    pub fn values_mut(&mut self) -> Counting<std::collections::btree_map::ValuesMut<'_, K, V>, N> {
        Counting(self.0.values_mut())
    }
    pub fn range<T: Ord + ?Sized, R: RangeBounds<T>>(&self, range: R) -> Counting<std::collections::btree_map::Range<'_, K, V>, N>
    where
        K: std::borrow::Borrow<T>,
    {
        Counting(self.0.range(range))
    }
    pub fn retain<F: FnMut(&K, &mut V) -> bool>(&mut self, f: F) {
        visit_map(N, self.0.len());
        self.0.retain(f);
    }
}

impl<'a, K: Ord, V, const N: usize> IntoIterator for &'a Counted<BTreeMap<K, V>, N> {
    type Item = (&'a K, &'a V);
    type IntoIter = Counting<std::collections::btree_map::Iter<'a, K, V>, N>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, K: Ord, V, const N: usize> IntoIterator for &'a mut Counted<BTreeMap<K, V>, N> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = Counting<std::collections::btree_map::IterMut<'a, K, V>, N>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<K: Ord, V, const N: usize> IntoIterator for Counted<BTreeMap<K, V>, N> {
    type Item = (K, V);
    type IntoIter = Counting<std::collections::btree_map::IntoIter<K, V>, N>;
    fn into_iter(self) -> Self::IntoIter {
        Counting(self.0.into_iter())
    }
}

impl<K: Ord, V, const N: usize> FromIterator<(K, V)> for Counted<BTreeMap<K, V>, N> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Counted(iter.into_iter().collect())
    }
}

impl<K: Ord, V, const N: usize> Extend<(K, V)> for Counted<BTreeMap<K, V>, N> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<T: Ord, const N: usize> Counted<BTreeSet<T>, N> {
    pub fn iter(&self) -> Counting<std::collections::btree_set::Iter<'_, T>, N> {
        Counting(self.0.iter())
    }
    pub fn range<K: Ord + ?Sized, R: RangeBounds<K>>(&self, range: R) -> Counting<std::collections::btree_set::Range<'_, T>, N>
    where
        T: std::borrow::Borrow<K>,
    {
        Counting(self.0.range(range))
    }
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, f: F) {
        visit_map(N, self.0.len());
        self.0.retain(f);
    }
    pub fn difference<'a>(&'a self, other: &'a BTreeSet<T>) -> Counting<std::collections::btree_set::Difference<'a, T>, N> {
        Counting(self.0.difference(other))
    }
    pub fn is_subset(&self, other: &BTreeSet<T>) -> bool {
        visit_map(N, self.0.len());
        self.0.is_subset(other)
    }
}

impl<'a, T: Ord, const N: usize> IntoIterator for &'a Counted<BTreeSet<T>, N> {
    type Item = &'a T;
    type IntoIter = Counting<std::collections::btree_set::Iter<'a, T>, N>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Ord, const N: usize> IntoIterator for Counted<BTreeSet<T>, N> {
    type Item = T;
    type IntoIter = Counting<std::collections::btree_set::IntoIter<T>, N>;
    fn into_iter(self) -> Self::IntoIter {
        Counting(self.0.into_iter())
    }
}

impl<T: Ord, const N: usize> FromIterator<T> for Counted<BTreeSet<T>, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Counted(iter.into_iter().collect())
    }
}

impl<T: Ord, const N: usize> Extend<T> for Counted<BTreeSet<T>, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<T, const N: usize> Counted<Vec<T>, N> {
    pub fn iter(&self) -> Counting<std::slice::Iter<'_, T>, N> {
        Counting(self.0.iter())
    }
    pub fn iter_mut(&mut self) -> Counting<std::slice::IterMut<'_, T>, N> {
        Counting(self.0.iter_mut())
    }
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, f: F) {
        visit_map(N, self.0.len());
        self.0.retain(f);
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a Counted<Vec<T>, N> {
    type Item = &'a T;
    type IntoIter = Counting<std::slice::Iter<'a, T>, N>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut Counted<Vec<T>, N> {
    type Item = &'a mut T;
    type IntoIter = Counting<std::slice::IterMut<'a, T>, N>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T, const N: usize> FromIterator<T> for Counted<Vec<T>, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Counted(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    /// Every map name has a constant, and every constant a name.
    #[test]
    fn every_map_is_named() {
        assert_eq!(super::MAP_NAMES.len(), super::PRODUCER_PUBLISHED + 1);
    }
}

#[cfg(all(test, debug_assertions))]
mod tripwire {
    #[test]
    #[should_panic(expected = "walks the whole document")]
    fn a_whole_document_walk_in_a_transaction_panics() {
        let _hot = super::hot();
        let _ = crate::Document::new().live_nodes().count();
    }

    #[test]
    fn one_allowed_by_name_does_not() {
        let _hot = super::hot();
        let _allow = super::allow_scan("test");
        let _ = crate::Document::new().live_nodes().count();
    }
}
