//! The document's 2D content as a tree of indexes: one [`BoundsIndex`] per
//! scope, a scope being the children of one node in that node's local space
//! (`None`: loose roots, in document space). A group with content is an
//! entry in its parent's scope whose bounds are its own subtree's union
//! under its local affine. Queries descend from a composition's scope through
//! group entries, mapping the query through each group's inverse affine, and
//! return leaves in painter order.
//!
//! What the store reports (`Store::take_geometry_changes`) is what moves:
//! a **placement** change rewrites one entry and the unions above it; a
//! **local** change recomputes one leaf; **structure** rebuilds. Nothing here
//! walks a moved group's descendants, which is the point.
//!
//! Hosts read painter-ordered candidates in composition space and keep the
//! exact tests (shape containment, clips, embed regions) to themselves.

use crate::{Bounds, BoundsIndex, Record};
use iron_document::composition::{self, Dims};
use iron_document::{Document, NodeId, Store, TypeId};
use kurbo::{Affine, Point, Rect};
use std::collections::{BTreeMap, BTreeSet};

/// Deepest nesting of 2D embeds a region lookup follows; the hosts' limit.
const MAX_EMBED_DEPTH: u32 = 8;

/// Paint outset of an embed's frame, as the hosts stroke it.
const EMBED_PAD: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A drawable node, or a 3D embed's frame: its own local bounds.
    Leaf,
    /// An embed of a 2D composition: its region (frame, or the source's
    /// content), which follows the source's content.
    Embed2d { source: NodeId, framed: bool },
    /// A group with content below it.
    Group,
}

#[derive(Clone, Debug)]
struct Entry {
    /// The scope that holds this entry: the node's parent.
    scope: Option<NodeId>,
    kind: Kind,
    /// The node's own transform in the scope's space.
    affine: Affine,
    /// The entry's bounds in the scope's space, as last indexed.
    bounds: Bounds,
    order: u32,
}

#[derive(Default)]
struct Scope {
    index: BoundsIndex,
    /// The union of the scope's entries, in its space.
    union: Option<Bounds>,
    depth: u32,
}

#[derive(Default)]
pub struct Scene {
    scopes: BTreeMap<Option<NodeId>, Scope>,
    entries: BTreeMap<NodeId, Entry>,
    /// Every indexed node's ancestors inside its composition, outermost
    /// first, ending with the node: the chain a hit reports.
    chains: BTreeMap<NodeId, Vec<NodeId>>,
    /// Every live embed in indexed content.
    embeds: Vec<NodeId>,
    /// 2D embeds by the composition they show, for region updates.
    shown_by: BTreeMap<NodeId, Vec<NodeId>>,
    /// 3D embeds by the camera they look through, for aspect changes.
    camera_embeds: BTreeMap<NodeId, Vec<NodeId>>,
    built: bool,
    /// Bumped by every update that changed anything.
    pub generation: u64,
    /// Old and new painted bounds of what the last update moved, in each
    /// node's composition space. Diagnostic: not yet a raster command.
    pub damage: Vec<Rect>,
    /// Entries rewritten by the last update.
    pub updated: u32,
}

/// The scope key a node's children live in.
fn scope_of(node: NodeId) -> Option<NodeId> {
    Some(node)
}

fn lineage(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    let mut out = vec![id];
    let mut cur = doc.live(id).and_then(|n| n.parent);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if n.ty == composition {
            break;
        }
        out.push(n.id);
        cur = n.parent;
    }
    out.reverse();
    out
}

fn is_type(doc: &Document, id: NodeId, ty: &str) -> bool {
    doc.live(id).is_some_and(|n| n.ty.name() == ty)
}

fn affine_of(a: iron_document::Affine2) -> Affine {
    Affine::new(a.0)
}

impl Scene {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every embed in indexed content, as of the last structural change.
    pub fn embeds(&self) -> &[NodeId] {
        &self.embeds
    }

    /// `id`'s ancestors inside its composition and itself, outermost first.
    pub fn chain(&self, id: NodeId) -> &[NodeId] {
        self.chains.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// An indexed node's affine from its local space to its composition's:
    /// its parent scope's placement then its own transform. `None` for a
    /// node the scene does not hold (an empty group, a 3D node, a point).
    pub fn world_affine(&self, id: NodeId) -> Option<Affine> {
        let e = self.entries.get(&id)?;
        Some(self.placement(e.scope) * e.affine)
    }

    /// An indexed node's own transform in its parent's space.
    pub fn local_affine(&self, id: NodeId) -> Option<Affine> {
        self.entries.get(&id).map(|e| e.affine)
    }

    /// An entry's bounds in its own composition's space.
    pub fn bounds(&self, id: NodeId) -> Option<Bounds> {
        let e = self.entries.get(&id)?;
        Some(e.bounds.mapped(self.placement(e.scope)))
    }

    /// An entry's painted bounds in composition space. A group's covers its
    /// whole subtree.
    pub fn painted(&self, id: NodeId) -> Option<Rect> {
        self.bounds(id).map(|b| b.painted)
    }

    /// The painted union of a composition's content, in its space.
    pub fn content_bounds(&self, composition: NodeId) -> Option<Rect> {
        self.scopes.get(&scope_of(composition))?.union.map(|b| b.painted)
    }

    pub fn record(&self, id: NodeId) -> Option<Record> {
        let e = self.entries.get(&id)?;
        Some(Record { bounds: self.bounds(id)?, order: e.order })
    }

    /// The affine from a scope's space to its composition's: the product of
    /// the group affines above it. `O(depth)`, no walk of any content.
    fn placement(&self, scope: Option<NodeId>) -> Affine {
        let mut out = Affine::IDENTITY;
        let mut at = scope;
        while let Some(g) = at {
            let Some(e) = self.entries.get(&g) else { break };
            out = e.affine * out;
            at = e.scope;
        }
        out
    }

    /// Leaves under `roots` whose bounds meet `point` (interaction) or
    /// `rect` (painted), in painter order. `roots` are siblings or loose
    /// roots: what one panel walks.
    pub fn candidates(&self, roots: &[NodeId], point: Option<Point>, rect: Rect) -> Vec<NodeId> {
        let root_set: BTreeSet<NodeId> = roots.iter().copied().collect();
        let scopes: BTreeSet<Option<NodeId>> = roots.iter().filter_map(|r| self.entries.get(r).map(|e| e.scope)).collect();
        let mut out = Vec::new();
        for scope in scopes {
            self.query(scope, Some(&root_set), point, rect, &mut out);
        }
        out.sort_by_cached_key(|id| self.entries[id].order);
        out
    }

    fn query(&self, scope: Option<NodeId>, only: Option<&BTreeSet<NodeId>>, point: Option<Point>, rect: Rect, out: &mut Vec<NodeId>) {
        let Some(s) = self.scopes.get(&scope) else { return };
        let hits = match point {
            Some(p) => s.index.at_point(p),
            None => s.index.in_rect(rect),
        };
        for id in hits {
            if only.is_some_and(|set| !set.contains(&id)) {
                continue;
            }
            let e = &self.entries[&id];
            match e.kind {
                Kind::Leaf | Kind::Embed2d { .. } => out.push(id),
                Kind::Group => {
                    // Into the group's space; a singular affine shows nothing.
                    if e.affine.determinant().abs() < 1e-12 {
                        continue;
                    }
                    let inv = e.affine.inverse();
                    self.query(scope_of(id), None, point.map(|p| inv * p), inv.transform_rect_bbox(rect), out);
                }
            }
        }
    }

    // ---- Updates ----------------------------------------------------------

    /// Bring the scene up to date with everything the store reports moved.
    pub fn update(&mut self, store: &mut Store) {
        let changes = store.take_geometry_changes();
        if changes.structure || !self.built {
            self.rebuild(store);
            return;
        }
        if changes.is_empty() {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.damage.clear();
        self.updated = 0;
        for s in self.scopes.values_mut() {
            s.index.begin_changes();
        }
        let mut dirty: BTreeSet<Option<NodeId>> = BTreeSet::new();
        let mut local: BTreeSet<NodeId> = changes.local;
        // A camera's aspect shapes the embeds that look through it.
        for cam in local.clone() {
            if let Some(embeds) = self.camera_embeds.get(&cam) {
                local.extend(embeds.iter().copied());
            }
        }
        for id in local {
            let Some((kind, affine, scope)) = self.entries.get(&id).map(|e| (e.kind, e.affine, e.scope)) else { continue };
            match kind {
                Kind::Leaf => {
                    match store.local_bounds(id) {
                        Some(lb) => self.upsert(id, scope, Bounds::of_local(&lb, affine), affine),
                        None => self.remove(id),
                    }
                    dirty.insert(scope);
                }
                Kind::Embed2d { .. } => {
                    self.refresh_embed(store, id);
                    dirty.insert(scope);
                }
                Kind::Group => {}
            }
        }
        for id in changes.placement {
            let Some((kind, scope)) = self.entries.get(&id).map(|e| (e.kind, e.scope)) else { continue };
            let affine = affine_of(store.local_affine(id));
            match kind {
                Kind::Leaf => {
                    if let Some(lb) = store.local_bounds(id) {
                        self.upsert(id, scope, Bounds::of_local(&lb, affine), affine);
                    }
                }
                Kind::Embed2d { .. } => {
                    self.entries.get_mut(&id).expect("entry").affine = affine;
                    self.refresh_embed(store, id);
                }
                Kind::Group => {
                    // One entry: the subtree's union under the new affine.
                    let union = self.scopes.get(&scope_of(id)).and_then(|s| s.union);
                    match union {
                        Some(u) => self.upsert(id, scope, u.mapped(affine), affine),
                        None => self.remove(id),
                    }
                }
            }
            dirty.insert(scope);
        }
        self.settle(store, dirty);
    }

    /// Commit every dirty scope, deepest first, and carry changed unions up
    /// through group entries and across to the embeds that show them.
    fn settle(&mut self, store: &Store, mut dirty: BTreeSet<Option<NodeId>>) {
        while let Some(&scope) = dirty.iter().max_by_key(|s| self.scopes.get(s).map_or(0, |x| x.depth)) {
            dirty.remove(&scope);
            let Some(s) = self.scopes.get_mut(&scope) else { continue };
            s.index.commit();
            let union = s.index.records().map(|(_, r)| r.bounds).reduce(Bounds::union);
            let changed = s.union != union;
            s.union = union;
            // Damage in composition space: the scope's own damage, placed.
            let placement = self.placement(scope);
            let rects: Vec<Rect> = self.scopes[&scope].index.damage().iter().map(|r| placement.transform_rect_bbox(*r)).collect();
            self.damage.extend(rects);
            if !changed {
                continue;
            }
            let Some(g) = scope else { continue };
            if is_type(store.document(), g, "composition") {
                // Content changed: the embeds showing it follow.
                for embed in self.shown_by.get(&g).cloned().unwrap_or_default() {
                    let Some(es) = self.entries.get(&embed).filter(|e| matches!(e.kind, Kind::Embed2d { framed: false, .. })).map(|e| e.scope) else { continue };
                    self.refresh_embed(store, embed);
                    dirty.insert(es);
                }
            } else if let Some((parent, affine)) = self.entries.get(&g).map(|e| (e.scope, e.affine)) {
                match union {
                    Some(u) => self.upsert(g, parent, u.mapped(affine), affine),
                    None => self.remove(g),
                }
                dirty.insert(parent);
            } else if let Some(u) = union {
                // A group that just gained its first content without a
                // structural change: a leaf whose bounds appeared.
                let doc = store.document();
                let parent = doc.live(g).and_then(|n| n.parent);
                let affine = affine_of(store.local_affine(g));
                let order = self.entries.values().map(|e| e.order).max().unwrap_or(0);
                self.entries.insert(g, Entry { scope: parent, kind: Kind::Group, affine, bounds: u.mapped(affine), order });
                self.chains.insert(g, lineage(doc, g));
                self.scopes.entry(parent).or_default().index.upsert(g, Record { bounds: u.mapped(affine), order });
                self.updated += 1;
                dirty.insert(parent);
            }
        }
        for s in self.scopes.values_mut() {
            s.index.commit();
        }
    }

    fn upsert(&mut self, id: NodeId, scope: Option<NodeId>, bounds: Bounds, affine: Affine) {
        let e = self.entries.get_mut(&id).expect("entry");
        e.bounds = bounds;
        e.affine = affine;
        let order = e.order;
        self.scopes.entry(scope).or_default().index.upsert(id, Record { bounds, order });
        self.updated += 1;
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(e) = self.entries.remove(&id)
            && let Some(s) = self.scopes.get_mut(&e.scope)
        {
            s.index.remove(id);
            self.updated += 1;
        }
    }

    /// A 2D embed's region: its frame, or the source's content bounds.
    fn embed_region(&self, store: &Store, embed: NodeId, source: NodeId) -> Option<(Rect, bool)> {
        let doc = store.document();
        if let Some(f) = doc.get::<iron_document::component::Frame>(embed) {
            let n = |s: &iron_document::Slot| s.constant().and_then(|v| v.as_f64()).unwrap_or(0.0);
            return Some((Rect::new(n(&f.x0), n(&f.y0), n(&f.x1), n(&f.y1)), true));
        }
        self.content_bounds(source).map(|r| (r, false))
    }

    fn refresh_embed(&mut self, store: &Store, embed: NodeId) {
        let Some(e) = self.entries.get(&embed) else { return };
        let (scope, affine) = (e.scope, e.affine);
        let Kind::Embed2d { source, .. } = e.kind else { return };
        match self.embed_region(store, embed, source) {
            Some((region, framed)) => {
                self.entries.get_mut(&embed).expect("entry").kind = Kind::Embed2d { source, framed };
                self.upsert(embed, scope, Bounds::transformed(region, affine, EMBED_PAD, 0.0), affine);
            }
            None => self.remove(embed),
        }
    }

    /// Everything from scratch: scopes, entries, chains, painter order.
    fn rebuild(&mut self, store: &Store) {
        let doc = store.document();
        let old_damage: Vec<Rect> = self.entries.keys().filter_map(|id| self.painted(*id)).collect();
        *self = Scene { generation: self.generation.wrapping_add(1), built: true, ..Default::default() };
        self.damage = old_damage;
        // Painter order is the document's preorder, as the reconciler's.
        let mut order: BTreeMap<NodeId, u32> = BTreeMap::new();
        let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
        let mut i = 0u32;
        while let Some(id) = stack.pop() {
            order.insert(id, i);
            i += 1;
            stack.extend(doc.children(id).iter().rev().copied());
        }
        let structural = ["composition", "model", "library", "timeline", "session"].map(|t| TypeId::lookup(t).expect("registered"));
        // 2D compositions' children, then loose 2D roots.
        let mut top: Vec<(Option<NodeId>, Vec<NodeId>)> = Vec::new();
        for c in composition::compositions(doc) {
            if composition::dims(doc, c) == Some(Dims::Two) {
                top.push((scope_of(c), doc.children(c).to_vec()));
            }
        }
        let loose: Vec<NodeId> = doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| !structural.contains(&n.ty))).collect();
        top.push((None, loose));
        for (scope, roots) in top {
            let mut s = Scope::default();
            s.depth = 0;
            self.scopes.insert(scope, s);
            for r in roots {
                self.build_node(store, r, scope, 1, &order);
            }
        }
        // Every scope commits, deepest first, so group entries get their
        // unions. Embed regions that follow content can only be known once
        // every composition's union exists, so they refresh after.
        let all: BTreeSet<Option<NodeId>> = self.scopes.keys().copied().collect();
        self.settle(store, all);
        let embeds: Vec<NodeId> = self.entries.iter().filter(|(_, e)| matches!(e.kind, Kind::Embed2d { .. })).map(|(id, _)| *id).collect();
        // Nested embeds resolve in a few passes (a frame-less embed of a
        // composition that itself embeds another); bounded by the depth hosts draw.
        for _ in 0..MAX_EMBED_DEPTH {
            let mut dirty = BTreeSet::new();
            for &embed in &embeds {
                let Some(e) = self.entries.get(&embed) else { continue };
                if matches!(e.kind, Kind::Embed2d { framed: false, .. }) {
                    let scope = e.scope;
                    let before = e.bounds;
                    self.refresh_embed(store, embed);
                    if self.entries.get(&embed).map(|e| e.bounds) != Some(before) {
                        dirty.insert(scope);
                    }
                }
            }
            if dirty.is_empty() {
                break;
            }
            self.settle(store, dirty);
        }
        self.embeds = self.entries.keys().copied().filter(|&id| is_type(doc, id, "embed")).collect();
        self.updated = self.entries.len() as u32;
        self.damage.extend(self.entries.keys().filter_map(|id| self.painted(*id)).collect::<Vec<_>>());
    }

    /// Index `id` into `scope`; recurse into its children as a scope of
    /// its own. Returns whether anything below `id` (or `id`) was indexed.
    fn build_node(&mut self, store: &Store, id: NodeId, scope: Option<NodeId>, depth: u32, order: &BTreeMap<NodeId, u32>) -> bool {
        let doc = store.document();
        let Some(node) = doc.live(id) else { return false };
        let ord = order.get(&id).copied().unwrap_or(0);
        let affine = affine_of(store.local_affine(id));
        let ty = node.ty.name();
        if ty == "embed" {
            let kind = match composition::embedded(doc, id) {
                Some(source) if composition::dims(doc, source) == Some(Dims::Two) => {
                    self.shown_by.entry(source).or_default().push(id);
                    Some((Kind::Embed2d { source, framed: doc.get::<iron_document::component::Frame>(id).is_some() }, Bounds::transformed(Rect::ZERO, affine, EMBED_PAD, 0.0)))
                }
                _ => store.local_bounds(id).map(|lb| {
                    if let Some(cam) = composition::embed_camera(doc, id) {
                        self.camera_embeds.entry(cam).or_default().push(id);
                    }
                    (Kind::Leaf, Bounds::of_local(&lb, affine))
                }),
            };
            let Some((kind, bounds)) = kind else { return false };
            self.insert(id, scope, kind, affine, bounds, ord, doc);
            if let Kind::Embed2d { .. } = kind {
                // Region filled in by `rebuild` once content unions exist.
                if let Some((region, framed)) = self.embed_region(store, id, composition::embedded(doc, id).expect("checked")) {
                    self.entries.get_mut(&id).expect("entry").kind = Kind::Embed2d { source: composition::embedded(doc, id).expect("checked"), framed };
                    self.upsert(id, scope, Bounds::transformed(region, affine, EMBED_PAD, 0.0), affine);
                }
            }
            return true;
        }
        if let Some(lb) = store.local_bounds(id) {
            self.insert(id, scope, Kind::Leaf, affine, Bounds::of_local(&lb, affine), ord, doc);
            // A path's points are its outline, not content of their own.
            return true;
        }
        // Anything else with children is a scope: a group, or a node of a
        // type that happens to hold 2D content.
        let children = doc.children(id).to_vec();
        if children.is_empty() {
            return false;
        }
        let mut s = Scope::default();
        s.depth = depth;
        self.scopes.insert(scope_of(id), s);
        let mut any = false;
        for c in children {
            any |= self.build_node(store, c, scope_of(id), depth + 1, order);
        }
        if !any {
            self.scopes.remove(&scope_of(id));
            return false;
        }
        // Its entry's bounds come from `settle`, once the scope commits.
        self.insert(id, scope, Kind::Group, affine, Bounds::transformed(Rect::ZERO, Affine::IDENTITY, 0.0, 0.0), ord, doc);
        true
    }

    fn insert(&mut self, id: NodeId, scope: Option<NodeId>, kind: Kind, affine: Affine, bounds: Bounds, order: u32, doc: &Document) {
        self.entries.insert(id, Entry { scope, kind, affine, bounds, order });
        self.chains.insert(id, lineage(doc, id));
        if let Kind::Group = kind {
            // Indexed by `settle` from its union; a placeholder box here
            // would be wrong until then.
            return;
        }
        self.scopes.entry(scope).or_default().index.upsert(id, Record { bounds, order });
    }
}
