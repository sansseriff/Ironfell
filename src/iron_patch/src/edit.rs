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
/// for the patch of its layer whose region is `region`, made if there is
/// none.
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
    let target = w
        .patches
        .iter()
        .filter(|q| {
            q.id != p && q.layer == src.layer && q.writable && q.region.same(&region, &w.space)
        })
        .min_by(|a, b| w.cmp(a, b));
    match target {
        Some(q) => ops.extend(moving.iter().map(|(k, v)| Op::Entry {
            patch: q.id,
            key: *k,
            value: Some(*v),
        })),
        None => ops.push(Op::Add {
            patch: w.make(region, moving, src.layer),
            at: w.patches.len(),
        }),
    }
    ops
}

/// Split: `keys` leave `p` for a new patch on `region`, in `p`'s layer. On
/// `p`'s own region the new patch sits just above `p`, so every value stays.
/// Refused for a patch that may not be written, since it clears entries.
pub fn split(w: &World, p: PatchId, keys: &[Key], region: Product) -> Result<Vec<Op>, String> {
    let Some(src) = w.patch(p) else {
        return Err("no such patch".into());
    };
    if !src.writable {
        return Err("a patch that may not be written".into());
    }
    let moving: BTreeMap<Key, i64> = keys
        .iter()
        .filter_map(|k| src.entries.get(k).map(|v| (*k, *v)))
        .collect();
    let mut new = w.make(region.clone(), moving.clone(), src.layer);
    if region.same(&src.region, &w.space) {
        new.rank = w.rank_above(src.layer, src.rank);
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
    Ok(ops)
}

/// Merge two patches of one layer on the same region (as written) into the
/// stronger. Refused for a patch with itself, a patch that may not be
/// written, and where a third patch, between them in strength and meeting
/// them, sets a field the weaker sets and the stronger does not: one rank
/// cannot keep both results (Sol's counterexample, §11.2). The refusal is
/// conservative: it does not look at whether the third patch's value is the
/// same, or whether a fourth hides the overlap.
pub fn merge(w: &World, a: PatchId, b: PatchId) -> Result<Vec<Op>, String> {
    if a == b {
        return Err("a patch with itself".into());
    }
    let (Some(pa), Some(pb)) = (w.patch(a), w.patch(b)) else {
        return Err("no such patch".into());
    };
    if !pa.writable || !pb.writable {
        return Err("a patch that may not be written".into());
    }
    if pa.layer != pb.layer || !pa.region.same(&pb.region, &w.space) {
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
            && r.region.meets(&weak.region, &w.space)
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

/// Reorder: change a rank within its layer. Ranks are distinct within a
/// layer (patches.md §6.1), so a rank another patch of the layer has is
/// refused: with equal ranks, inserting "just above" one of them could land
/// above the other, and a split would change a value (round 2).
pub fn reorder(w: &World, p: PatchId, rank: f64) -> Result<Vec<Op>, String> {
    let Some(src) = w.patch(p) else {
        return Err("no such patch".into());
    };
    if w
        .patches
        .iter()
        .any(|q| q.id != p && q.layer == src.layer && q.rank == rank)
    {
        return Err("another patch of the layer has that rank".into());
    }
    Ok(vec![Op::Rank { patch: p, rank }])
}

/// A drag at `point`, from a view writing in `layer` whose edit region is
/// `edit`, setting `key` to `value` (§11.2, patches.md §7.2). It shows what it
/// set, never reaches outside `edit` (as written: not on values the axes do
/// not have yet), and never writes a patch that may not be written:
/// - the value is already shown: nothing;
/// - the winner is of an outer layer: refused, the view cannot override it;
/// - nothing sets `key` here: the weakest writable patch of `layer` on
///   exactly `edit` takes it, made at the default rank if there is none;
/// - the winner is of `layer`, may be written and lies within `edit`: the
///   winner takes it;
/// - otherwise a patch of `layer` on `edit ∩ winner` takes it: the weakest
///   one stronger than the winner with no patch between them setting `key`
///   where it applies, else a new one just above the winner (or at the
///   default rank, when the winner is of an inner layer).
pub fn drag(
    w: &World,
    point: &Point,
    edit: &Product,
    key: Key,
    value: i64,
    layer: u32,
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
    let add = |region: Product, rank: Option<f64>| {
        let mut new = w.make(region, BTreeMap::from([(key, value)]), layer);
        if let Some(r) = rank {
            new.rank = r;
        }
        vec![Op::Add {
            patch: new,
            at: w.patches.len(),
        }]
    };
    let Some(win) = w.winner(point, key) else {
        let q = w
            .patches
            .iter()
            .filter(|q| q.layer == layer && q.writable && q.region.same(edit, &w.space))
            .min_by(|a, b| w.cmp(a, b));
        return Ok(match q {
            Some(q) => entry(q.id),
            None => add(edit.clone(), None),
        });
    };
    if win.layer > layer {
        return Err("an outer scope sets it here".into());
    }
    if win.layer == layer && win.writable && win.region.within(edit, &w.space) {
        return Ok(entry(win.id));
    }
    let target = edit.intersect(&win.region, &w.space);
    let clear = |q: &Patch| {
        !w.patches.iter().any(|r| {
            r.id != q.id
                && r.id != win.id
                && r.entries.contains_key(&key)
                && w.cmp(r, win) == Ordering::Greater
                && w.cmp(q, r) == Ordering::Greater
                && r.region.meets(&target, &w.space)
        })
    };
    let reuse = w
        .patches
        .iter()
        .filter(|q| {
            q.layer == layer
                && q.writable
                && q.region.same(&target, &w.space)
                && w.cmp(q, win) == Ordering::Greater
                && clear(q)
        })
        .min_by(|a, b| w.cmp(a, b));
    Ok(match reuse {
        Some(q) => entry(q.id),
        None if win.layer == layer => add(target, Some(w.rank_above(layer, win.rank))),
        None => add(target, None),
    })
}
