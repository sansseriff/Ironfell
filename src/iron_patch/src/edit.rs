//! The edit operations (the note's §2.3, §5, §11.2), as lists of low-level
//! ops that each have an inverse, as `iron_document`'s transactions do.

use crate::axis::Point;
use crate::patch::{Key, Patch, PatchId};
use crate::region::Product;
use crate::world::World;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Add {
        patch: Patch,
        at: usize,
    },
    Remove(PatchId),
    /// Set (`Some`) or clear (`None`) one entry.
    Entry {
        patch: PatchId,
        key: Key,
        value: Option<i64>,
    },
    Region {
        patch: PatchId,
        region: Product,
    },
    Rank {
        patch: PatchId,
        rank: f64,
    },
}

impl World {
    /// Apply `op`; its inverse. An op on a patch that is not there does
    /// nothing, and its inverse does nothing.
    pub fn apply(&mut self, op: &Op) -> Op {
        let at = |w: &World, id: PatchId| w.patches.iter().position(|p| p.id == id);
        match op {
            Op::Add { patch, at: i } => {
                self.patches
                    .insert((*i).min(self.patches.len()), patch.clone());
                Op::Remove(patch.id)
            }
            Op::Remove(id) => match at(self, *id) {
                Some(i) => {
                    let patch = self.patches.remove(i);
                    Op::Add { patch, at: i }
                }
                None => op.clone(),
            },
            Op::Entry { patch, key, value } => match at(self, *patch) {
                Some(i) => {
                    let e = &mut self.patches[i].entries;
                    let old = match value {
                        Some(v) => e.insert(*key, *v),
                        None => e.remove(key),
                    };
                    Op::Entry {
                        patch: *patch,
                        key: *key,
                        value: old,
                    }
                }
                None => op.clone(),
            },
            Op::Region { patch, region } => match at(self, *patch) {
                Some(i) => Op::Region {
                    patch: *patch,
                    region: std::mem::replace(&mut self.patches[i].region, region.clone()),
                },
                None => op.clone(),
            },
            Op::Rank { patch, rank } => match at(self, *patch) {
                Some(i) => Op::Rank {
                    patch: *patch,
                    rank: std::mem::replace(&mut self.patches[i].rank, *rank),
                },
                None => op.clone(),
            },
        }
    }

    /// Apply `ops` in order; the ops that undo them, in the order to apply.
    pub fn apply_all(&mut self, ops: &[Op]) -> Vec<Op> {
        let mut inverse: Vec<Op> = ops.iter().map(|o| self.apply(o)).collect();
        inverse.reverse();
        inverse
    }
}

/// Edit: change a value in one patch.
pub fn edit(p: PatchId, key: Key, value: i64) -> Vec<Op> {
    vec![Op::Entry {
        patch: p,
        key,
        value: Some(value),
    }]
}

/// Widen or narrow as the note writes it: the patch's region changes, and
/// with derived strength so does its strength (§11.1's central flaw).
pub fn set_region(p: PatchId, region: Product) -> Vec<Op> {
    vec![Op::Region { patch: p, region }]
}

/// Widen by moving entries (Opus's first repair, §11.1): `keys` leave `p`
/// for the patch whose region is `region`, made if there is none.
pub fn widen_by_move(w: &World, p: PatchId, keys: &[Key], region: Product) -> Vec<Op> {
    let Some(src) = w.patch(p) else {
        return Vec::new();
    };
    let moving: BTreeMap<Key, i64> = keys
        .iter()
        .filter_map(|k| src.entries.get(k).map(|v| (*k, *v)))
        .collect();
    let mut ops: Vec<Op> = moving
        .keys()
        .map(|k| Op::Entry {
            patch: p,
            key: *k,
            value: None,
        })
        .collect();
    match w
        .patches
        .iter()
        .find(|q| q.id != p && q.writable && q.region.same(&region, &w.space))
    {
        Some(q) => ops.extend(moving.iter().map(|(k, v)| Op::Entry {
            patch: q.id,
            key: *k,
            value: Some(*v),
        })),
        None => ops.push(Op::Add {
            patch: w.make(region, moving),
            at: w.patches.len(),
        }),
    }
    ops
}

/// Split: `keys` leave `p` for a new patch on `region`. On `p`'s own region
/// the new patch sits just above `p`, so every value stays.
pub fn split(w: &World, p: PatchId, keys: &[Key], region: Product) -> Vec<Op> {
    let Some(src) = w.patch(p) else {
        return Vec::new();
    };
    let moving: BTreeMap<Key, i64> = keys
        .iter()
        .filter_map(|k| src.entries.get(k).map(|v| (*k, *v)))
        .collect();
    let mut new = w.make(region.clone(), moving.clone());
    if region.same(&src.region, &w.space) {
        new.rank = w.rank_above(src.rank);
    }
    let mut ops: Vec<Op> = moving
        .keys()
        .map(|k| Op::Entry {
            patch: p,
            key: *k,
            value: None,
        })
        .collect();
    ops.push(Op::Add {
        patch: new,
        at: w.patches.len(),
    });
    ops
}

/// Merge two patches on the same region into the stronger. Refused where a
/// third patch, between them in strength and overlapping them, sets a key
/// the weaker sets: one patch with one rank cannot keep both results
/// (Sol's counterexample, §11.2).
pub fn merge(w: &World, a: PatchId, b: PatchId) -> Result<Vec<Op>, String> {
    let (Some(pa), Some(pb)) = (w.patch(a), w.patch(b)) else {
        return Err("no such patch".into());
    };
    if !pa.region.same(&pb.region, &w.space) {
        return Err("their regions differ".into());
    }
    let (strong, weak) = if w.cmp(pa, pb) == Ordering::Greater {
        (pa, pb)
    } else {
        (pb, pa)
    };
    let moving: Vec<(Key, i64)> = weak
        .entries
        .iter()
        .filter(|(k, _)| !strong.entries.contains_key(*k))
        .map(|(k, v)| (*k, *v))
        .collect();
    for r in &w.patches {
        let between = w.cmp(r, weak) == Ordering::Greater && w.cmp(strong, r) == Ordering::Greater;
        if between
            && !r
                .region
                .intersect(&weak.region, &w.space)
                .is_empty(&w.space)
            && moving.iter().any(|(k, _)| r.entries.contains_key(k))
        {
            return Err(format!(
                "{:?} lies between them and sets what {:?} sets",
                r.id, weak.id
            ));
        }
    }
    let mut ops: Vec<Op> = moving
        .into_iter()
        .map(|(k, v)| Op::Entry {
            patch: strong.id,
            key: k,
            value: Some(v),
        })
        .collect();
    ops.push(Op::Remove(weak.id));
    Ok(ops)
}

/// Reorder: the tie-break of law 5, as a stored rank.
pub fn reorder(p: PatchId, rank: f64) -> Vec<Op> {
    vec![Op::Rank { patch: p, rank }]
}

/// A drag at `point`, in a view whose edit region is `edit`, setting `key`
/// to `value` (§11.2). The drag always shows, and never reaches outside
/// `edit`:
/// - nothing sets `key` here: the patch on `edit` takes it (made if need be);
/// - the winner lies inside `edit` and can be written: the winner takes it;
/// - otherwise a patch on `edit ∩ winner`, just above the winner, takes it.
pub fn drag(
    w: &World,
    point: &Point,
    edit: &Product,
    key: Key,
    value: i64,
) -> Result<Vec<Op>, String> {
    if !w.present(point, key.node) {
        return Err("nothing to drag here".into());
    }
    if !edit.contains(&w.space, point) {
        return Err("the point is outside the edit region".into());
    }
    if w.raw(point, key) == Some(value) {
        return Ok(Vec::new());
    }
    let entry = |p: PatchId| {
        vec![Op::Entry {
            patch: p,
            key,
            value: Some(value),
        }]
    };
    let Some(win) = w.winner(point, key) else {
        return Ok(
            match w
                .patches
                .iter()
                .find(|q| q.writable && q.region.same(edit, &w.space))
            {
                Some(q) => entry(q.id),
                None => vec![Op::Add {
                    patch: w.make(edit.clone(), BTreeMap::from([(key, value)])),
                    at: w.patches.len(),
                }],
            },
        );
    };
    if win.writable && win.region.subset(edit, &w.space) {
        return Ok(entry(win.id));
    }
    let target = edit.intersect(&win.region, &w.space);
    if let Some(q) = w.patches.iter().find(|q| {
        q.writable && q.region.same(&target, &w.space) && w.cmp(q, win) == Ordering::Greater
    }) {
        return Ok(entry(q.id));
    }
    let mut new = w.make(target, BTreeMap::from([(key, value)]));
    new.rank = w.rank_above(win.rank);
    Ok(vec![Op::Add {
        patch: new,
        at: w.patches.len(),
    }])
}
