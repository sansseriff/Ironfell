//! A world: the axes, the base, the patches, and the value at a point
//! (`value(x) = base ⊕ p₁ ⊕ p₂ ⊕ …`, the note's §2.1).

use crate::axis::{Point, ScopeId, Space};
use crate::patch::{
    Doc, FacetId, Field, HIDDEN, Key, Node, PRESENT, Patch, PatchId, SESSION_LAYER, Strength,
    compare, facet_present, is_facet_present, keys,
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
    /// Dedicated system-owned solve layer (engine.md §2.4, patches.md P17):
    /// saved, exact-point entries for held constraint poses, sitting below
    /// bindings and producers and above authored patches.
    pub solve: BTreeMap<(Point, Key), i64>,
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
            solve: BTreeMap::new(),
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
                && p.region.contains_anchored(&self.space, point, p.anchor)
                && best.is_none_or(|b| self.cmp(p, b) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// The strongest authored patch (`layer < SESSION_LAYER`) setting `key` at
    /// `point`.
    pub fn winner_authored(&self, point: &Point, key: Key) -> Option<&Patch> {
        let mut best: Option<&Patch> = None;
        for p in &self.patches {
            if p.layer < SESSION_LAYER
                && p.entries.contains_key(&key)
                && p.region.contains_anchored(&self.space, point, p.anchor)
                && best.is_none_or(|b| self.cmp(p, b) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// The strongest session patch (`layer == SESSION_LAYER`) setting `key` at
    /// `point`.
    pub fn winner_session(&self, point: &Point, key: Key) -> Option<&Patch> {
        let mut best: Option<&Patch> = None;
        for p in &self.patches {
            if p.layer == SESSION_LAYER
                && p.entries.contains_key(&key)
                && p.region.contains_anchored(&self.space, point, p.anchor)
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

    /// Whether the view at `point` itself exists (P18): if any path coordinate
    /// is placed by an enclosing `use` node, that `use` node must be present
    /// at its enclosing view's coordinate (never at the placed view's own
    /// coordinate, so presence on a `use` is never self-referential).
    pub fn view_present(&self, point: &Point) -> bool {
        for (&axis, &val) in point {
            if let Some(&(use_node, enclosing_val)) = self.doc.enclosing_use.get(&(axis, val)) {
                let mut parent_point = point.clone();
                parent_point.insert(axis, enclosing_val);
                if !self.present(&parent_point, use_node) {
                    return false;
                }
            }
        }
        true
    }

    /// Whether `node` exists at `point`: view placement and presence are
    /// resolved before any other field (P6, P18).
    pub fn present(&self, point: &Point, node: Node) -> bool {
        if !self.view_present(point) {
            return false;
        }
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

    /// Whether optional facet `facet` is present on `node` at `point` (P21).
    pub fn facet_present(&self, point: &Point, node: Node, facet: FacetId) -> bool {
        self.present(point, node)
            && self.raw(
                point,
                Key {
                    node,
                    field: facet_present(facet),
                },
            ) == Some(1)
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

    /// `key` at `point` (authored/session patch algebra): `None` where its
    /// node or optional facet does not exist, so an entry on an absent node or
    /// facet does nothing (law 6, P21).
    pub fn value(&self, point: &Point, key: Key) -> Option<i64> {
        if !self.present(point, key.node) {
            return None;
        }
        if let Some(&facet) = self.doc.field_facet.get(&key.field)
            && !self.facet_present(point, key.node, facet)
        {
            return None;
        }
        match key.field {
            PRESENT => Some(1),
            HIDDEN => Some(if self.shown(point, key.node) { 0 } else { 1 }),
            f if is_facet_present(f) => Some(if self.raw(point, key) == Some(1) { 1 } else { 0 }),
            _ => self.raw(point, key),
        }
    }

    /// Full 6-tier effective value precedence at `point` (engine.md §2.4, P17):
    /// 1. Session patch layer (`SESSION_LAYER`)
    /// 2. Binding value (`binding`)
    /// 3. Producer signal (`producer`)
    /// 4. Solve layer (`self.solve`)
    /// 5. Winning authored patch (`layer < SESSION_LAYER`)
    /// 6. Base constant
    /// All gated on Stage 1 node + optional-facet presence.
    pub fn effective(
        &self,
        point: &Point,
        key: Key,
        binding: Option<i64>,
        producer: Option<i64>,
    ) -> Option<i64> {
        if !self.present(point, key.node) {
            return None;
        }
        if let Some(&facet) = self.doc.field_facet.get(&key.field)
            && !self.facet_present(point, key.node, facet)
        {
            return None;
        }
        if let Some(p) = self.winner_session(point, key) {
            return p.entries.get(&key).copied();
        }
        if let Some(b) = binding {
            return Some(b);
        }
        if let Some(pr) = producer {
            return Some(pr);
        }
        if let Some(&s) = self.solve.get(&(point.clone(), key)) {
            return Some(s);
        }
        match self.winner_authored(point, key) {
            Some(p) => p.entries.get(&key).copied(),
            None => self.doc.base.get(&key).copied(),
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

    /// The entries that do nothing anywhere they would win: their node or
    /// optional facet is absent at every point of a non-empty winning region
    /// (patches.md §5.3, P21). A removal (`PRESENT` 0 or `facet_present` 0) is
    /// never stale: its node or facet is absent because it worked.
    pub fn stale(&self) -> Vec<(PatchId, Key)> {
        let mut out = Vec::new();
        for p in &self.patches {
            for (&key, &v) in &p.entries {
                if (key.field == PRESENT || is_facet_present(key.field)) && v == 0 {
                    continue;
                }
                let points: Vec<Point> = if p.anchor.is_some() {
                    self.space
                        .points()
                        .into_iter()
                        .filter(|x| self.winner(x, key).is_some_and(|w| w.id == p.id))
                        .collect()
                } else {
                    self.winning(p.id, key).points(&self.space)
                };
                let absent_at = |x: &Point| {
                    if !self.present(x, key.node) {
                        return true;
                    }
                    if let Some(&facet) = self.doc.field_facet.get(&key.field) {
                        return !self.facet_present(x, key.node, facet);
                    }
                    false
                };
                if !points.is_empty() && points.iter().all(absent_at) {
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
        self.make_anchored(region, entries, layer, None)
    }

    /// A new patch in `layer` anchored at `anchor` (patches.md §4, P18).
    pub fn make_anchored(
        &self,
        region: Product,
        entries: BTreeMap<Key, i64>,
        layer: u32,
        anchor: Option<ScopeId>,
    ) -> Patch {
        Patch {
            id: self.fresh_id(),
            layer,
            rank: self.default_rank(&region, layer),
            region,
            entries,
            writable: true,
            anchor,
        }
    }
}

/// The fields the properties edit; `PRESENT`, `HIDDEN`, and optional
/// facet-presence fields are structure.
pub fn is_structure(field: Field) -> bool {
    field == PRESENT || field == HIDDEN || is_facet_present(field)
}
