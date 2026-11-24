//! A world: the axes, the base, the patches, and the value at a point
//! (`value(x) = base ⊕ p₁ ⊕ p₂ ⊕ …`, the note's §2.1).

use crate::axis::{Point, Space};
use crate::patch::{
    Doc, Field, HIDDEN, Key, Node, PRESENT, Patch, PatchId, Strength, compare, keys,
};
use crate::region::{Product, Region};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub space: Space,
    pub doc: Doc,
    /// In storage order, which decides nothing (determinism, §11.5 item 2).
    pub patches: Vec<Patch>,
    pub strength: Strength,
}

/// Everything at one point: each key's value, `None` where its node does
/// not exist. `PRESENT` reads 1 where the node exists and `HIDDEN` 1 where
/// it is not shown, cascades applied.
pub type State = BTreeMap<Key, Option<i64>>;

impl World {
    pub fn new(space: Space, doc: Doc, strength: Strength) -> Self {
        World {
            space,
            doc,
            patches: Vec::new(),
            strength,
        }
    }

    pub fn patch(&self, id: PatchId) -> Option<&Patch> {
        self.patches.iter().find(|p| p.id == id)
    }

    pub fn cmp(&self, p: &Patch, q: &Patch) -> Ordering {
        compare(&self.space, self.strength, p, q)
    }

    /// The strongest patch that sets `key` at `point`.
    pub fn winner(&self, point: &Point, key: Key) -> Option<&Patch> {
        let mut best: Option<&Patch> = None;
        for p in &self.patches {
            if p.entries.contains_key(&key)
                && p.region.contains(&self.space, point)
                && best.is_none_or(|b| self.cmp(p, b) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// `key` at `point` before structure: the winner's entry, else the base.
    pub fn raw(&self, point: &Point, key: Key) -> Option<i64> {
        match self.winner(point, key) {
            Some(p) => p.entries.get(&key).copied(),
            None => self.doc.base.get(&key).copied(),
        }
    }

    /// Whether `node` exists at `point`: presence is resolved before any
    /// other field (§11.1).
    pub fn present(&self, point: &Point, node: Node) -> bool {
        self.raw(
            point,
            Key {
                node,
                field: PRESENT,
            },
        ) == Some(1)
            && self
                .doc
                .parent
                .get(&node)
                .copied()
                .flatten()
                .is_none_or(|p| self.present(point, p))
    }

    pub fn shown(&self, point: &Point, node: Node) -> bool {
        self.present(point, node)
            && self.raw(
                point,
                Key {
                    node,
                    field: HIDDEN,
                },
            ) != Some(1)
            && self
                .doc
                .parent
                .get(&node)
                .copied()
                .flatten()
                .is_none_or(|p| self.shown(point, p))
    }

    /// `key` at `point`: `None` where its node does not exist, so an entry
    /// on an absent node does nothing (law 6).
    pub fn value(&self, point: &Point, key: Key) -> Option<i64> {
        if !self.present(point, key.node) {
            return None;
        }
        match key.field {
            PRESENT => Some(1),
            HIDDEN => Some(if self.shown(point, key.node) { 0 } else { 1 }),
            _ => self.raw(point, key),
        }
    }

    /// Every key any patch or the base names, and every node's presence and
    /// hiding.
    pub fn keys(&self) -> BTreeSet<Key> {
        let mut out: BTreeSet<Key> = self.doc.base.keys().copied().collect();
        out.extend(self.patches.iter().flat_map(|p| p.entries.keys().copied()));
        for &node in self.doc.parent.keys() {
            out.insert(Key {
                node,
                field: PRESENT,
            });
            out.insert(Key {
                node,
                field: HIDDEN,
            });
        }
        out
    }

    pub fn state(&self, point: &Point) -> State {
        self.keys()
            .into_iter()
            .map(|k| (k, self.value(point, k)))
            .collect()
    }

    /// Every point's state: what the properties compare.
    pub fn snapshot(&self) -> Vec<(Point, State)> {
        self.space
            .points()
            .into_iter()
            .map(|p| {
                let s = self.state(&p);
                (p, s)
            })
            .collect()
    }

    /// Where patch `id`'s entry for `key` is the one that applies: its
    /// region minus every stronger patch's that sets `key` (the note's §4.2,
    /// and law 3's real blast radius, §11.1). Symbolic.
    pub fn winning(&self, id: PatchId, key: Key) -> Region {
        let Some(p) = self.patch(id).filter(|p| p.entries.contains_key(&key)) else {
            return Region::default();
        };
        let mut out = Region(vec![p.region.clone()]);
        for q in &self.patches {
            if q.id != p.id && q.entries.contains_key(&key) && self.cmp(q, p) == Ordering::Greater {
                out = out.minus(&q.region, &self.space);
            }
        }
        out
    }

    /// The entries that do nothing anywhere they would win: their node is
    /// absent at every point of a non-empty winning region (§11.1: one
    /// absent at some points only is normal, and not listed). A removal
    /// (`PRESENT` 0) is never stale: its node is absent because it worked.
    pub fn stale(&self) -> Vec<(PatchId, Key)> {
        let mut out = Vec::new();
        for p in &self.patches {
            for (&key, &v) in &p.entries {
                if key.field == PRESENT && v == 0 {
                    continue;
                }
                let points = self.winning(p.id, key).points(&self.space);
                if !points.is_empty() && points.iter().all(|x| !self.present(x, key.node)) {
                    out.push((p.id, key));
                }
            }
        }
        out
    }

    /// The entries that win nowhere over the points that exist: stronger
    /// patches *of their own layer* set the field everywhere they apply. A
    /// rank that contradicts inclusion shows up here, as the inner patch's
    /// dead entries, and both fixes (reorder, drop) are the layer's own. An
    /// outer layer covering an entry is not listed: that is outermost-wins
    /// working, and an inner layer may not be writable (round 2).
    pub fn dead(&self) -> Vec<(PatchId, Key)> {
        let mut out = Vec::new();
        for p in &self.patches {
            if p.region.is_empty(&self.space) {
                continue;
            }
            for &key in p.entries.keys() {
                let mut left = Region(vec![p.region.clone()]);
                for q in &self.patches {
                    if q.id != p.id
                        && q.layer == p.layer
                        && q.entries.contains_key(&key)
                        && self.cmp(q, p) == Ordering::Greater
                    {
                        left = left.minus(&q.region, &self.space);
                    }
                }
                if left.is_empty(&self.space) {
                    out.push((p.id, key));
                }
            }
        }
        out
    }

    /// Pairs whose order the rank alone decided where specificity tied
    /// (law 5's flag): one layer, equal keys, regions that differ and
    /// meet, and a field both set. Two patches on the same region are a
    /// deliberate order, and not listed.
    pub fn ties(&self) -> Vec<(PatchId, PatchId)> {
        let mut out = Vec::new();
        for (i, p) in self.patches.iter().enumerate() {
            for q in &self.patches[i + 1..] {
                if p.layer == q.layer
                    && keys(&self.space, &p.region) == keys(&self.space, &q.region)
                    && !p.region.same(&q.region, &self.space)
                    && p.region.meets(&q.region, &self.space)
                    && p.entries.keys().any(|k| q.entries.contains_key(k))
                {
                    out.push((p.id.min(q.id), p.id.max(q.id)));
                }
            }
        }
        out
    }

    /// Pairs where a patch whose region is strictly within another's, in
    /// its layer, is the weaker: possible after a region edit or a reorder.
    pub fn contradictions(&self) -> Vec<(PatchId, PatchId)> {
        let mut out = Vec::new();
        for p in &self.patches {
            for q in &self.patches {
                let inside = p.region.within(&q.region, &self.space)
                    && !q.region.within(&p.region, &self.space);
                if p.id != q.id
                    && p.layer == q.layer
                    && inside
                    && !p.region.void(&self.space)
                    && self.cmp(p, q) == Ordering::Less
                {
                    out.push((p.id, q.id));
                }
            }
        }
        out
    }

    /// An id no patch has.
    pub fn fresh_id(&self) -> PatchId {
        PatchId(self.patches.iter().map(|p| p.id.0 + 1).max().unwrap_or(0))
    }

    /// A rank for a new patch on `region` in `layer`: just above every patch
    /// of the layer whose region is no more specific (the axis keys), below
    /// the next rank above those. Where earlier edits put ranks out of key
    /// order, no slot is below every more specific patch: above wins, and
    /// the inner patch's entries show as dead (`dead`). A newer patch on an
    /// equal region is stronger.
    pub fn default_rank(&self, region: &Product, layer: u32) -> f64 {
        let mine = keys(&self.space, region);
        let below = self
            .patches
            .iter()
            .filter(|p| p.layer == layer && keys(&self.space, &p.region) <= mine)
            .map(|p| p.rank)
            .fold(f64::NEG_INFINITY, f64::max);
        if below == f64::NEG_INFINITY {
            return self
                .patches
                .iter()
                .filter(|p| p.layer == layer)
                .map(|p| p.rank)
                .fold(f64::INFINITY, f64::min)
                .min(1.0)
                - 1.0;
        }
        self.rank_above(layer, below)
    }

    /// A rank in `layer` between `r` and the next rank above it.
    pub fn rank_above(&self, layer: u32, r: f64) -> f64 {
        let next = self
            .patches
            .iter()
            .filter(|p| p.layer == layer)
            .map(|p| p.rank)
            .filter(|&x| x > r)
            .fold(f64::INFINITY, f64::min);
        if next == f64::INFINITY {
            r + 1.0
        } else {
            (r + next) / 2.0
        }
    }

    /// A new patch in `layer`, not yet added: `Op::Add` adds it.
    pub fn make(&self, region: Product, entries: BTreeMap<Key, i64>, layer: u32) -> Patch {
        Patch {
            id: self.fresh_id(),
            layer,
            rank: self.default_rank(&region, layer),
            region,
            entries,
            writable: true,
        }
    }
}

/// The fields the properties edit; `PRESENT` and `HIDDEN` are structure.
pub fn is_structure(field: Field) -> bool {
    field == PRESENT || field == HIDDEN
}
