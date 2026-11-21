//! Patches, the base they apply over, and strength (laws 4 and 5).

use crate::axis::{AxisId, Kind, Space};
use crate::region::{Product, Sel};
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
    /// Stored strength (`Strength::Stored`), given from specificity when the
    /// patch is made.
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

/// One axis's key for a selection: larger is stronger, and a selection
/// strictly inside another has a larger key.
/// - ordered: a later first value, then an earlier last ("a later start
///   wins", which is "builds on");
/// - path: more trailing steps shared by every instance it selects;
/// - then, on every kind, fewer values.
pub fn axis_key(space: &Space, axis: AxisId, sel: &Sel) -> (i64, i64, i64) {
    let Some(ax) = space.axes.get(&axis) else {
        return (i64::MIN, i64::MIN, i64::MIN);
    };
    let members = sel.members(ax);
    if members.is_empty() {
        return (i64::MIN, i64::MIN, i64::MIN);
    }
    let count = -(members.len() as i64);
    match ax.kind {
        Kind::Ordered => {
            let first = ax.position(members[0]).unwrap_or(0) as i64;
            let last = ax
                .position(*members.last().expect("non-empty"))
                .unwrap_or(0) as i64;
            (first, -last, count)
        }
        Kind::Unordered => (0, 0, count),
        Kind::Path => {
            let paths: Vec<&[u32]> = members.iter().map(|v| ax.path(*v)).collect();
            let shortest = paths.iter().map(|p| p.len()).min().unwrap_or(0);
            let shared = (0..shortest)
                .take_while(|&k| {
                    paths
                        .iter()
                        .all(|p| p[p.len() - 1 - k] == paths[0][paths[0].len() - 1 - k])
                })
                .count();
            (shared as i64, 0, count)
        }
    }
}

/// Every axis's key, in deciding order.
pub fn keys(space: &Space, region: &Product) -> Vec<(i64, i64, i64)> {
    space
        .deciding()
        .into_iter()
        .map(|a| axis_key(space, a, region.sel(a)))
        .collect()
}

/// Whether `p` is stronger than `q` (`Greater`) where both apply.
pub fn compare(space: &Space, strength: Strength, p: &Patch, q: &Patch) -> Ordering {
    if p.id == q.id {
        return Ordering::Equal;
    }
    let tie = || p.rank.total_cmp(&q.rank).then(p.id.cmp(&q.id));
    match strength {
        Strength::Stored => tie(),
        Strength::AxisKeys => keys(space, &p.region)
            .cmp(&keys(space, &q.region))
            .then_with(tie),
        Strength::AsWritten => {
            let (pq, qp) = (
                p.region.subset(&q.region, space),
                q.region.subset(&p.region, space),
            );
            if pq && !qp {
                return Ordering::Greater;
            }
            if qp && !pq {
                return Ordering::Less;
            }
            for a in space.deciding() {
                let ax = &space.axes[&a];
                let (sp, sq) = (p.region.sel(a).members(ax), q.region.sel(a).members(ax));
                let inside = |x: &[crate::axis::ValueId], y: &[crate::axis::ValueId]| {
                    x.len() < y.len() && x.iter().all(|v| y.contains(v))
                };
                if inside(&sp, &sq) {
                    return Ordering::Greater;
                }
                if inside(&sq, &sp) {
                    return Ordering::Less;
                }
            }
            tie()
        }
    }
}
