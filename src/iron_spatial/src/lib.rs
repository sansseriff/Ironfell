//! Spatial acceleration, not another document or transform hierarchy.
//!
//! [`BoundsIndex`] is one flat index over caller-owned ids in one space;
//! Understory keys remain private. [`Scene`] stacks them along the document's
//! own hierarchy: each group's contents are indexed in the group's local space
//! with one placement per group, so moving a group touches one entry (vision
//! 10 §3.1; the render handoff's item 3).

pub mod scene;

pub use scene::Scene;

use iron_document::NodeId;
use kurbo::{Affine, Point, Rect};
use std::collections::BTreeMap;
use understory_index::{Aabb2D, Index, IndexGeneric, Key, backends::RTreeF64};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub geometry: Rect,
    pub painted: Rect,
    pub interaction: Rect,
}

impl Bounds {
    /// Inflate in local space before mapping: nonuniform scale must also scale
    /// stroke/hit margins. Paint padding includes the host's join/cap policy.
    pub fn transformed(local: Rect, transform: Affine, paint_pad: f64, hit_pad: f64) -> Self {
        Self {
            geometry: transform.transform_rect_bbox(local),
            painted: transform.transform_rect_bbox(local.inflate(paint_pad, paint_pad)),
            interaction: transform.transform_rect_bbox(local.inflate(hit_pad, hit_pad)),
        }
    }
    /// From a drawable node's own extent (`iron_document::geom::LocalBounds`).
    pub fn of_local(local: &iron_document::geom::LocalBounds, transform: Affine) -> Self {
        Self::transformed(local.geometry, transform, local.paint_pad, local.hit_pad)
    }
    /// The conservative box of each bound under `transform`.
    pub fn mapped(self, transform: Affine) -> Self {
        Self {
            geometry: transform.transform_rect_bbox(self.geometry),
            painted: transform.transform_rect_bbox(self.painted),
            interaction: transform.transform_rect_bbox(self.interaction),
        }
    }
    pub fn union(self, other: Self) -> Self {
        Self { geometry: self.geometry.union(other.geometry), painted: self.painted.union(other.painted), interaction: self.interaction.union(other.interaction) }
    }
    fn indexed(self) -> Rect { self.painted.union(self.interaction) }
    fn finite(self) -> bool {
        [self.geometry, self.painted, self.interaction].iter()
            .all(|r| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) && r.x0 <= r.x1 && r.y0 <= r.y1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    pub bounds: Bounds,
    pub order: u32,
}

struct Entry { key: Option<Key>, record: Record, dirty: bool }

pub struct BoundsIndex {
    index: IndexGeneric<f64, NodeId, RTreeF64>,
    entries: BTreeMap<NodeId, Entry>,
    /// Painted damage is separate from Understory's acceleration-box damage:
    /// a style edit can damage pixels without changing an AABB.
    damage: Vec<Rect>,
    pending: Vec<NodeId>,
}

impl Default for BoundsIndex {
    fn default() -> Self {
        Self { index: Index::<f64, NodeId>::with_rtree(), entries: BTreeMap::new(), damage: Vec::new(), pending: Vec::new() }
    }
}

fn aabb(r: Rect) -> Aabb2D<f64> { Aabb2D::new(r.x0, r.y0, r.x1, r.y1) }
fn overlaps(a: Rect, b: Rect) -> bool { a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1 }

impl BoundsIndex {
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn record(&self, id: NodeId) -> Option<&Record> { self.entries.get(&id).map(|e| &e.record) }
    pub fn records(&self) -> impl Iterator<Item = (NodeId, &Record)> { self.entries.iter().map(|(id, e)| (*id, &e.record)) }
    pub fn begin_changes(&mut self) { self.damage.clear(); }
    pub fn damage(&self) -> &[Rect] { &self.damage }

    pub fn upsert(&mut self, id: NodeId, record: Record) {
        assert!(record.bounds.finite(), "non-finite spatial bounds for {id}");
        if let Some(e) = self.entries.get_mut(&id) {
            self.damage.push(e.record.bounds.painted);
            if e.record.bounds.indexed() != record.bounds.indexed() {
                if !e.dirty { e.dirty = true; self.pending.push(id); }
            }
            e.record = record;
        } else {
            self.entries.insert(id, Entry { key: None, record, dirty: true });
            self.pending.push(id);
        }
        self.damage.push(record.bounds.painted);
    }

    pub fn remove(&mut self, id: NodeId) {
        if let Some(e) = self.entries.remove(&id) {
            self.damage.push(e.record.bounds.painted);
            if let Some(key) = e.key { self.index.remove(key); }
        }
    }

    /// Queries observe a committed batch. Sparse edits update existing keys;
    /// dense motion builds a packed tree once instead of searching N old paths.
    /// No Understory key escapes this adapter, so rebuilding cannot change ids.
    pub fn commit(&mut self) {
        if self.entries.len() >= 256 && self.pending.len() > self.entries.len() / 2 {
            let items: Vec<_> = self.entries.iter().map(|(&id,e)| (aabb(e.record.bounds.indexed()), id)).collect();
            self.index = Index::<f64, NodeId>::with_rtree_bulk(&items);
            let union = aabb(self.entries.values().map(|e| e.record.bounds.indexed()).reduce(|a,b| a.union(b)).expect("nonempty dense batch"));
            let entries = &mut self.entries;
            self.index.visit_rect(union, |key,id| {
                let e = entries.get_mut(&id).expect("live bulk item");
                e.key = Some(key); e.dirty = false;
            });
            self.pending.clear();
        } else {
            for id in self.pending.drain(..) {
                let Some(e) = self.entries.get_mut(&id).filter(|e| e.dirty) else { continue };
                let box_ = aabb(e.record.bounds.indexed());
                if let Some(key) = e.key { self.index.update(key, box_); }
                else { e.key = Some(self.index.insert(box_,id)); }
                e.dirty = false;
            }
        }
        let _ = self.index.commit();
    }

    /// Candidates only. The host applies exact geometry, clips, and embed rules.
    pub fn at_point(&self, p: Point) -> Vec<NodeId> {
        let mut out: Vec<_> = self.index.query_point(p.x, p.y).map(|(_, id)| id)
            .filter(|id| overlaps(self.entries[id].record.bounds.interaction, Rect::new(p.x, p.y, p.x, p.y))).collect();
        out.sort_by_cached_key(|id| self.entries[id].record.order);
        out
    }

    pub fn in_rect(&self, rect: Rect) -> Vec<NodeId> {
        let mut out: Vec<_> = self.index.query_rect(aabb(rect)).map(|(_, id)| id)
            .filter(|id| overlaps(self.entries[id].record.bounds.painted, rect)).collect();
        out.sort_by_cached_key(|id| self.entries[id].record.order);
        out
    }

    pub fn geometry_union(&self) -> Option<Rect> {
        self.entries.values().map(|e| e.record.bounds.geometry).reduce(|a,b| a.union(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u32) -> NodeId { format!("#{n}").parse().unwrap() }
    fn record(x: f64, order: u32) -> Record {
        Record { bounds: Bounds::transformed(Rect::new(0.0,0.0,10.0,10.0), Affine::translate((x,0.0)), 2.0, 5.0), order }
    }
    #[test]
    fn moves_deletes_and_recycled_index_keys_preserve_document_identity() {
        let mut index = BoundsIndex::default();
        index.upsert(id(1), record(0.0, 1)); index.commit(); index.begin_changes();
        index.upsert(id(1), record(100.0, 1)); index.commit();
        assert!(index.at_point(Point::new(5.0,5.0)).is_empty());
        assert_eq!(index.at_point(Point::new(105.0,5.0)), vec![id(1)]);
        assert_eq!(index.damage().len(), 2);
        assert!(index.damage()[0].contains(Point::new(5.0,5.0)));
        index.begin_changes(); index.remove(id(1)); index.upsert(id(2), record(0.0, 2)); index.commit();
        assert_eq!(index.at_point(Point::new(5.0,5.0)), vec![id(2)]);
        assert!(index.record(id(1)).is_none());
    }
    #[test]
    fn paint_and_hit_bounds_are_distinct_and_results_preserve_painter_order() {
        let mut index = BoundsIndex::default();
        index.upsert(id(1), record(0.0, 20)); index.upsert(id(2), record(0.0, 10)); index.commit();
        assert_eq!(index.at_point(Point::new(-4.0,5.0)), vec![id(2),id(1)]);
        assert!(index.in_rect(Rect::new(-4.5,4.0,-3.5,6.0)).is_empty());
        index.begin_changes(); index.upsert(id(1), record(0.0, 20)); index.commit();
        assert!(!index.damage().is_empty(), "same-bounds style edits still damage paint");
    }
    #[test]
    fn nonuniform_scale_rotation_and_negative_scale_preserve_local_margins() {
        let local = Rect::new(0.0,0.0,10.0,10.0);
        let affine = Affine::rotate(0.4) * Affine::scale_non_uniform(-3.0, 0.5);
        let b = Bounds::transformed(local, affine, 8.0, 5.0);
        for p in [Point::new(-8.0,-8.0),Point::new(18.0,18.0)] {
            let p = affine * p;
            assert!(overlaps(b.painted, Rect::new(p.x,p.y,p.x,p.y)));
        }
        assert!(b.finite());
    }
    #[test]
    fn dense_rebuild_then_sparse_motion_preserves_identity_and_order() {
        let mut index = BoundsIndex::default();
        for n in 1..=1000 { index.upsert(id(n), record(n as f64 * 20.0, 1000-n)); }
        index.commit();
        for n in 1..=1000 { index.upsert(id(n), record(-(n as f64)*20.0, n)); }
        index.commit();
        index.remove(id(500));
        index.upsert(id(1), record(0.0, 20));
        index.upsert(id(2), record(0.0, 10));
        index.commit();
        assert_eq!(index.at_point(Point::new(5.0,5.0)), vec![id(2),id(1)]);
        assert!(index.record(id(500)).is_none());
        assert_eq!(index.len(),999);
    }
    #[test]
    fn indexed_queries_match_a_linear_reference_across_many_updates() {
        let mut index = BoundsIndex::default();
        for n in 1..=1000 { index.upsert(id(n), record(n as f64 * 20.0, 1000-n)); }
        index.commit();
        for n in (1..=1000).step_by(7) { index.upsert(id(n), record(-(n as f64)*20.0, 1000-n)); }
        for n in (1..=1000).step_by(11) { index.remove(id(n)); }
        index.commit();
        for x in (-1000..1000).step_by(37) {
            let r = Rect::new(x as f64, -1.0, x as f64 + 100.0, 11.0);
            let mut expected: Vec<_> = index.records().filter(|(_,v)| overlaps(v.bounds.painted,r)).collect();
            expected.sort_by_key(|(_,v)| v.order);
            assert_eq!(index.in_rect(r), expected.iter().map(|(id,_)| *id).collect::<Vec<_>>());
        }
    }
}
