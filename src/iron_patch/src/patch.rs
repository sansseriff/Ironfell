//! Patches, the base they apply over, and strength (laws 4 and 5).

use crate::axis::{AxisId, Kind, Space};
use crate::region::{Bound, Product, Sel};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub type Node = u32;
pub type Field = u16;

/// Whether a node exists at a point: a field like any other, resolved
/// before the rest (structure in patches, note §11.1). A node exists where
/// it is `1` and its parent exists; a patch that adds a node sets it to `1`.
pub const PRESENT: Field = 0;
/// Hidden, cascading: a node is shown where it exists, it is not `1`, and
/// its parent is shown. A stronger patch can set it back to `0`, but cannot
/// show what a hidden ancestor hides.
pub const HIDDEN: Field = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    pub node: Node,
    pub field: Field,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatchId(pub u32);

/// A partial set of overrides and the region where they apply.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub id: PatchId,
    pub region: Product,
    /// Where the patch is written, outermost largest: a definition's own
    /// patches are layer 0, an enclosing definition's 1, the document's
    /// above. Every patch of an outer layer is stronger than every patch of
    /// an inner one (component-model.md §5.3's outermost-wins, by
    /// structure), and ranks compare only within a layer, so an imported
    /// definition's ranks never interleave with a document's.
    pub layer: u32,
    /// Stored strength within the layer (`Strength::Stored`), given from
    /// specificity when the patch is made. A model artefact: `f64` halves
    /// gaps, where the spec's rank is an order key that never runs out.
    pub rank: f64,
    pub entries: BTreeMap<Key, i64>,
    /// A recorded move's patch, or an imported definition's: never written
    /// in place (note §2.4, §11.2).
    pub writable: bool,
}

/// The base: a fixed tree of nodes (a patch adds a node by making it
/// present, never by inventing an id), and the values under every patch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub parent: BTreeMap<Node, Option<Node>>,
    pub base: BTreeMap<Key, i64>,
}

/// How strength is decided where two patches set one field at one point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    /// The note's laws 4 and 5 as written (§2.2): the narrower region by
    /// inclusion; where neither contains the other, the first axis in the
    /// declared order on which one's selection is strictly inside the
    /// other's; then rank. Not transitive (note §11.1).
    AsWritten,
    /// Derived from the regions: per axis a key that extends inclusion,
    /// compared in the declared axis order, then rank (Opus's repair, note
    /// §11.1). A strict total order that extends inclusion; a region edit
    /// changes it.
    AxisKeys,
    /// Stored: rank, given from `AxisKeys` when the patch is made. A region
    /// edit leaves it alone, and it may then contradict inclusion (a lint).
    Stored,
}

/// One axis's key for a selection: larger is more specific.
pub type AxisKey = (i64, i64, Vec<i64>);

/// One axis's key for a selection, from the selection as written (so it
/// does not change as the axis grows): larger is more specific, and a
/// selection strictly within another (`Sel::within`) has a larger key.
/// - ordered: a later lower end, then an earlier upper end, then a finite
///   set before an interval (an interval of a dense order holds values yet
///   to be inserted), fewer values first;
/// - unordered: a finite set before a cofinite one; fewer values in a set,
///   more excluded from a cofinite one;
/// - instance: more steps fixed; then the exclusions that remove something,
///   reduced (one that extends another removes nothing more), counted by
///   length, shortest first: a shorter exclusion removes more, so counting
///   them alone could put a wider pattern above a narrower one (round 2).
///   There are no classes: a variant is a layer (patches.md §6.1).
pub fn axis_key(space: &Space, axis: AxisId, sel: &Sel) -> AxisKey {
    let Some(ax) = space.axes.get(&axis) else {
        return (i64::MIN, i64::MIN, vec![i64::MIN]);
    };
    if sel.void(ax) {
        return (i64::MAX, i64::MAX, vec![i64::MAX]);
    }
    let cut = |v: crate::axis::ValueId| 2 * ax.position(v).map_or(i64::MAX / 4, |p| p as i64);
    match (ax.kind, sel) {
        (Kind::Ordered, Sel::All) => (i64::MIN, -i64::MAX, vec![i64::MIN / 2]),
        (Kind::Ordered, Sel::Range(lo, hi)) => {
            let lo = match lo {
                Bound::Open => i64::MIN,
                Bound::Incl(v) => cut(*v),
                Bound::Excl(v) => cut(*v) + 1,
            };
            let hi = match hi {
                Bound::Open => i64::MAX,
                Bound::Incl(v) => cut(*v),
                Bound::Excl(v) => cut(*v) - 1,
            };
            (lo, -hi, vec![if lo == hi { -1 } else { i64::MIN / 2 }])
        }
        (Kind::Ordered, Sel::Set(s)) => {
            let cuts: Vec<i64> = s.iter().map(|v| cut(*v)).collect();
            (
                cuts.iter().copied().min().unwrap_or(0),
                -cuts.iter().copied().max().unwrap_or(0),
                vec![-(s.len() as i64)],
            )
        }
        (Kind::Unordered, Sel::Set(s)) => (1, -(s.len() as i64), Vec::new()),
        (Kind::Unordered, Sel::Not(t)) => (0, t.len() as i64, Vec::new()),
        (Kind::Unordered, Sel::All) => (0, 0, Vec::new()),
        (Kind::Path, Sel::Suffix { tail, not }) => (tail.len() as i64, 0, exclusions(tail, not)),
        (Kind::Path, Sel::All) => (0, 0, Vec::new()),
        // Forms an axis of this kind does not take.
        _ => (i64::MIN, i64::MIN, vec![i64::MIN]),
    }
}

/// A suffix pattern's exclusions as a key: of those that extend `tail`,
/// the ones no other extends, counted per length from `tail`'s length + 1,
/// trailing zeros dropped. If one pattern is strictly within another with
/// the same tail, at the shortest length where their reduced exclusions
/// differ it has every one the other has and more.
fn exclusions(tail: &[u32], not: &[Vec<u32>]) -> Vec<i64> {
    let ends = |p: &[u32], t: &[u32]| p.len() >= t.len() && p[p.len() - t.len()..] == *t;
    let mut ext: Vec<&Vec<u32>> = not
        .iter()
        .filter(|n| n.len() > tail.len() && ends(n, tail))
        .collect();
    ext.sort();
    ext.dedup();
    let reduced: Vec<&Vec<u32>> = ext
        .iter()
        .copied()
        .filter(|n| !ext.iter().any(|m| m.len() < n.len() && ends(n, m)))
        .collect();
    let mut out = Vec::new();
    for n in reduced {
        let i = n.len() - tail.len() - 1;
        if out.len() <= i {
            out.resize(i + 1, 0);
        }
        out[i] += 1;
    }
    out
}

/// Every axis's key, in deciding order.
pub fn keys(space: &Space, region: &Product) -> Vec<AxisKey> {
    space
        .deciding()
        .into_iter()
        .map(|a| axis_key(space, a, region.sel(a)))
        .collect()
}

/// Whether `p` is stronger than `q` (`Greater`) where both apply. An outer
/// layer first, always; then the rule; then, for equal ranks, the later id.
pub fn compare(space: &Space, strength: Strength, p: &Patch, q: &Patch) -> Ordering {
    if p.id == q.id {
        return Ordering::Equal;
    }
    let tie = || p.rank.total_cmp(&q.rank).then(p.id.cmp(&q.id));
    p.layer.cmp(&q.layer).then_with(|| match strength {
        Strength::Stored => tie(),
        Strength::AxisKeys => keys(space, &p.region)
            .cmp(&keys(space, &q.region))
            .then_with(tie),
        Strength::AsWritten => {
            let (pq, qp) = (
                p.region.within(&q.region, space),
                q.region.within(&p.region, space),
            );
            if pq && !qp {
                return Ordering::Greater;
            }
            if qp && !pq {
                return Ordering::Less;
            }
            for a in space.deciding() {
                let ax = &space.axes[&a];
                let (sp, sq) = (p.region.sel(a), q.region.sel(a));
                let (inp, inq) = (sp.within(sq, ax), sq.within(sp, ax));
                if inp && !inq {
                    return Ordering::Greater;
                }
                if inq && !inp {
                    return Ordering::Less;
                }
            }
            tie()
        }
    })
}
