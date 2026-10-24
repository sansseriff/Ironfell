//! Chunks (plans/specs/render-cache.md §4): each composition's 2D content
//! cut into painter-contiguous pieces, each recorded once in its
//! composition's space and shared by every panel that shows it.
//!
//! A panel draws its chunks as layers of their own, each carrying the
//! panel's pan and zoom (`VectorLayer::affine`). So a pan re-records and
//! re-extracts nothing, and an edit re-records only the chunks it touched.
//! Each layer carries its chunk's key (`VectorLayer::cache`), so the backend
//! can draw cold chunks from raster tiles instead of encoding them again
//! (`vector::raster`, the spec's §5).
//!
//! A chunk is recorded relative to where its roots' parent sits, and drawn
//! through that position, its *frame*. Moving a group split into chunks (a
//! drag of most of a large import) then changes their frames only: no
//! re-recording, and the backend moves their tiles rigidly
//! (instance-cache.md §2). Turning or scaling the group records them again.
//!
//! A chunk is a run of consecutive siblings, each with its whole subtree.
//! A subtree too large for one chunk is split into its children's chunks,
//! unless splitting would change what it draws:
//! - a group with an `appearance` (isolated opacity) or a clip frame
//!   composites its children together, so it stays whole;
//! - an embed is a chunk of its own, recorded per panel, because what it
//!   shows depends on the occurrence (a 3D view's texture is per panel);
//! - a node that draws and has children stays whole, since it draws before
//!   them.
//!
//! A linked component instance is a chunk of its own, drawn as one layer per
//! *run* of its picture (instance-cache.md phase C, `iron_document::analysis`):
//! painter-contiguous parts of one class. A static or keyed run is recorded
//! relative to the instance and keyed by what it depends on, so every
//! instance with equal values shares one recording, and the backend bakes it
//! once and draws its tiles at each placement. A live run (a needle driven by
//! a slider) is drawn live, uncached: it is known to change, so it never
//! joins a baked group and its changes regroup nothing.

use super::{Drawables, Shape2d, nest::{self, Home}};
use crate::vector::Recorded;
use bevy::prelude::*;
use iron_document::{Document, FacetKind, NodeId, Store};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Most cost one chunk holds, in commands (an anchor-heavy path counts
/// its segments in eighths): about 8 ms of encode at the measured 4 µs a
/// command (render-cache.md §2).
pub const CHUNK_MAX: usize = 2_000;

/// The least cost, in commands, for which a reference is drawn as a layer of
/// its own (instance-cache.md phase D).
const REFERENCE_LAYER: usize = 256;

/// One run of an instance's picture: its parts' copies in painter order,
/// and how it may be shared.
#[derive(Debug)]
pub struct InstanceRun {
    /// What the parts are placed relative to: the instance's `use`, or for
    /// a reference its definition's template.
    pub root: NodeId,
    pub parts: Vec<NodeId>,
    /// For each part, whether it draws with its subtree (a path's points, a
    /// nested picture) or alone (its children are parts of their own).
    pub whole: Vec<bool>,
    /// For a static or keyed run, the analysis's key: equal for instances
    /// whose run depends on equal values. `None` for a live run.
    pub share: Option<u64>,
    /// The key its layer caches under, and its recording, once recorded.
    pub key: u64,
    pub recorded: Option<Arc<Recorded>>,
}

/// One piece of a composition's content.
#[derive(Debug)]
pub struct Chunk {
    /// Consecutive siblings, each drawn with its subtree, in painter order.
    pub roots: Vec<NodeId>,
    pub cost: usize,
    /// Nodes in it: roots and their descendants.
    pub nodes: usize,
    /// It draws an embed, so each panel records it ([`Chunks::per_panel`]).
    pub embeds: bool,
    /// The shared recording, in its frame's space; `None` for an embed
    /// chunk, or before the first.
    pub recorded: Option<Arc<Recorded>>,
    /// Where its roots' parent sits in the composition, as a translation
    /// (identity for a composition's own children, and for an embed chunk):
    /// the recording is made relative to it and drawn through it, so moving
    /// a group split into chunks changes their frames, not their recordings
    /// (instance-cache.md §2).
    pub frame: kurbo::Affine,
    /// The parent's turn and scale the recording was made with: when it
    /// changes, the chunk is recorded again.
    pub basis: kurbo::Affine,
    /// For a component instance, its runs, each a layer of its own; its
    /// frame is then the instance's own position. Empty otherwise.
    pub runs: Vec<InstanceRun>,
    /// When it was last re-recorded (app seconds): the render inspector
    /// shows recent ones.
    pub recorded_at: f64,
}

/// A home's chunks, and which chunk each of its nodes is drawn in.
#[derive(Debug, Default)]
pub struct Partition {
    pub chunks: Vec<Chunk>,
    /// Every node in a chunk, to that chunk; a split group, which draws
    /// nothing itself, to [`SPLIT`].
    of_node: HashMap<NodeId, usize>,
    /// Each split group's chunks: its subtree packs into consecutive ones.
    split: HashMap<NodeId, std::ops::Range<usize>>,
    /// The chunks that draw each definition template through references,
    /// directly or nested: what an edit to the template re-records.
    pub shows_template: HashMap<NodeId, std::collections::BTreeSet<usize>>,
}

/// Where a split group's nodes point (see [`Chunks::chunk_of`]).
const SPLIT: usize = usize::MAX;

/// Where a changed node is drawn.
pub enum Place {
    /// In one chunk.
    Chunk(usize),
    /// Across these chunks: a group too large for one, split into its
    /// children's chunks. A change to it (a move) changes all of them,
    /// unless it now has to be drawn whole ([`Chunks::must_cut_again`]).
    Split(std::ops::Range<usize>),
}

#[derive(Resource, Default)]
pub struct Chunks {
    pub homes: BTreeMap<Home, Partition>,
    /// Instance runs' shared recordings (instance-cache.md phase C).
    pub shared: Shared,
    /// Embed chunks' recordings, by panel and the chunk's first root.
    pub per_panel: HashMap<(String, NodeId), Arc<Recorded>>,
    /// Render inspectors' tinted copies, by the recording they tint (its
    /// address, kept alive alongside).
    tinted: HashMap<usize, (Arc<Recorded>, Arc<Recorded>)>,
}

/// How much of its hue an inspector mixes into a chunk's colours.
const TINT: f32 = 0.35;

/// A chunk's colour in the render inspector: a golden-ratio step round the
/// hue circle from a hash of its first root's id, so neighbours differ and a
/// chunk keeps its colour (render-inspector.md §2).
pub fn chunk_hue(first: NodeId) -> [f32; 3] {
    use std::hash::{Hash, Hasher};
    // `DefaultHasher::new` has fixed keys, so the colour is stable.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    first.hash(&mut hasher);
    let h = ((hasher.finish() % 1_000_003) as f64 * 0.618_033_988_75).fract() as f32 * 6.0;
    let (s, v) = (0.75, 0.95);
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// What a panel draws, home by home: its composition, then for the viewer
/// the loose 2D nodes.
pub fn panel_homes(view: &super::PanelView) -> Vec<Home> {
    let mut out: Vec<Home> = view.composition.map(Home::Composition).into_iter().collect();
    if view.loose {
        out.push(Home::Loose);
    }
    out
}

/// The roots a home's content hangs from: a composition's children, or the
/// loose 2D roots.
pub fn home_roots(doc: &Document, home: Home) -> Vec<NodeId> {
    match home {
        Home::Composition(c) => doc.children(c).to_vec(),
        Home::Loose => nest::loose_roots(doc),
        Home::Unknown => Vec::new(),
    }
}

impl Chunks {
    /// Cut `home` into chunks again, dropping its recordings.
    pub fn partition(&mut self, store: &Store, drawables: &Drawables, home: Home) {
        let doc = store.document();
        let mut cut = Cut { store, doc, drawables, cost: HashMap::new(), embeds: HashMap::new(), templates: HashMap::new(), out: Partition::default() };
        let roots = home_roots(doc, home);
        cut.pack(&roots);
        self.homes.insert(home, cut.out);
    }

    /// `rec` tinted for a render inspector: made once per recording.
    pub fn tinted(&mut self, rec: &Arc<Recorded>, first: NodeId, used: &mut HashMap<usize, (Arc<Recorded>, Arc<Recorded>)>) -> Arc<Recorded> {
        let key = Arc::as_ptr(rec) as usize;
        let entry = self.tinted.remove(&key).unwrap_or_else(|| (rec.clone(), Arc::new(rec.tinted(chunk_hue(first), TINT))));
        let out = entry.1.clone();
        used.insert(key, entry);
        out
    }

    /// Keep only the tinted copies this frame used.
    pub fn keep_tinted(&mut self, used: HashMap<usize, (Arc<Recorded>, Arc<Recorded>)>) {
        self.tinted = used;
    }

    /// Which chunks of its home `node` is drawn in, if any.
    pub fn chunk_of(&self, home: Home, node: NodeId) -> Option<Place> {
        let part = self.homes.get(&home)?;
        let i = *part.of_node.get(&node)?;
        Some(if i == SPLIT { Place::Split(part.split.get(&node)?.clone()) } else { Place::Chunk(i) })
    }

    /// Whether a change to split group `node` means cutting its home again:
    /// it now draws, holds an embed, or gained an appearance or a clip
    /// frame, so it has to be drawn whole. A move, the common change, does
    /// not.
    pub fn must_cut_again(store: &Store, drawables: &Drawables, node: NodeId) -> bool {
        let doc = store.document();
        let cut = Cut { store, doc, drawables, cost: HashMap::new(), embeds: HashMap::new(), templates: HashMap::new(), out: Partition::default() };
        cut.whole(node)
    }
}

struct Cut<'a> {
    store: &'a Store,
    doc: &'a Document,
    drawables: &'a Drawables,
    cost: HashMap<NodeId, (usize, usize)>,
    embeds: HashMap<NodeId, bool>,
    /// The templates each subtree draws through references, transitively.
    templates: HashMap<NodeId, std::collections::BTreeSet<NodeId>>,
    out: Partition,
}

impl Cut<'_> {
    /// A node's own cost: what it emits, roughly in commands.
    fn own(&self, id: NodeId) -> usize {
        match self.drawables.0.get(&id).map(|d| &d.shape) {
            Some(Shape2d::Path { outline, .. }) => 2 + outline.elements().len() / 8,
            Some(Shape2d::Text { paints, .. }) => 2 + paints.len(),
            Some(_) => 2,
            None => usize::from(nest::is_embed(self.doc, id)) * 2,
        }
    }

    /// Cost and node count of `id`'s subtree. A reference costs what its
    /// template draws, though its nodes are the template's.
    fn subtree(&mut self, id: NodeId) -> (usize, usize) {
        if let Some(&c) = self.cost.get(&id) {
            return c;
        }
        // A definition that pictures itself draws nothing the second time.
        self.cost.insert(id, (0, 1));
        let mut c = (self.own(id), 1);
        for &k in self.doc.children(id) {
            let (kc, kn) = self.subtree(k);
            c = (c.0 + kc, c.1 + kn);
        }
        if let Some(t) = iron_document::expand::reference_template(self.doc, id) {
            c.0 += self.subtree(t).0;
        }
        self.cost.insert(id, c);
        c
    }

    /// The templates `id`'s subtree draws through references, nested ones
    /// included.
    fn shown_templates(&mut self, id: NodeId) -> std::collections::BTreeSet<NodeId> {
        if let Some(t) = self.templates.get(&id) {
            return t.clone();
        }
        self.templates.insert(id, Default::default());
        let mut out = std::collections::BTreeSet::new();
        for &k in self.doc.children(id).to_vec().iter() {
            out.extend(self.shown_templates(k));
        }
        if let Some(t) = iron_document::expand::reference_template(self.doc, id) {
            out.insert(t);
            out.extend(self.shown_templates(t));
        }
        self.templates.insert(id, out.clone());
        out
    }

    fn has_embed(&mut self, id: NodeId) -> bool {
        if let Some(&e) = self.embeds.get(&id) {
            return e;
        }
        let e = nest::is_embed(self.doc, id) || self.doc.children(id).to_vec().into_iter().any(|k| self.has_embed(k));
        self.embeds.insert(id, e);
        e
    }

    /// Whether splitting `id` into its children's chunks would change what
    /// it draws (see the module comment).
    fn whole(&self, id: NodeId) -> bool {
        let doc = self.doc;
        doc.children(id).is_empty()
            || nest::is_embed(doc, id)
            || self.drawables.0.contains_key(&id)
            || doc.facet(id, FacetKind::Appearance).is_some()
            || iron_document::appearance::ClipFrame::read(self.store, id).is_some()
    }

    /// Chunks for consecutive siblings `ids`: runs packed up to
    /// [`CHUNK_MAX`], a larger subtree split into its children, and anything
    /// that draws an embed on its own, so the rest stays shared.
    fn pack(&mut self, ids: &[NodeId]) {
        let mut run: Vec<NodeId> = Vec::new();
        let mut run_cost = (0, 0);
        for &id in ids {
            // A reference is a layer of its own only when it is heavy: it
            // sits wherever its piece did, so its tiles are seldom another
            // reference's (they share by sub-pixel phase), and a light one
            // costs less drawn in an ordinary chunk.
            let light_reference = iron_document::expand::reference_template(self.doc, id).is_some() && self.subtree(id).0 < REFERENCE_LAYER;
            if let Some(runs) = instance_runs(self.store, id).filter(|_| !self.has_embed(id) && !light_reference) {
                self.flush(&mut run, &mut run_cost);
                run.push(id);
                run_cost = self.subtree(id);
                self.flush(&mut run, &mut run_cost);
                self.out.chunks.last_mut().expect("flushed").runs = runs;
                continue;
            }
            let (cost, nodes) = self.subtree(id);
            let embeds = self.has_embed(id);
            let whole = self.whole(id);
            if (cost > CHUNK_MAX || embeds) && !whole {
                self.flush(&mut run, &mut run_cost);
                self.out.of_node.insert(id, SPLIT);
                let kids = self.doc.children(id).to_vec();
                let start = self.out.chunks.len();
                self.pack(&kids);
                self.out.split.insert(id, start..self.out.chunks.len());
                continue;
            }
            if embeds {
                self.flush(&mut run, &mut run_cost);
                run.push(id);
                run_cost = (cost, nodes);
                self.flush(&mut run, &mut run_cost);
                continue;
            }
            if !run.is_empty() && run_cost.0 + cost > CHUNK_MAX {
                self.flush(&mut run, &mut run_cost);
            }
            run.push(id);
            run_cost = (run_cost.0 + cost, run_cost.1 + nodes);
        }
        self.flush(&mut run, &mut run_cost);
    }

    fn flush(&mut self, run: &mut Vec<NodeId>, cost: &mut (usize, usize)) {
        if run.is_empty() {
            return;
        }
        let i = self.out.chunks.len();
        let roots = std::mem::take(run);
        for &r in &roots {
            self.out.of_node.insert(r, i);
            for n in self.doc.descendants(r) {
                self.out.of_node.insert(n, i);
            }
        }
        let embeds = roots.iter().any(|&r| self.embeds.get(&r).copied().unwrap_or(false));
        for &r in &roots {
            for t in self.shown_templates(r) {
                self.out.shows_template.entry(t).or_default().insert(i);
            }
        }
        self.out.chunks.push(Chunk { roots, cost: cost.0, nodes: cost.1, embeds, recorded: None, frame: kurbo::Affine::IDENTITY, basis: kurbo::Affine::IDENTITY, runs: Vec::new(), recorded_at: f64::NEG_INFINITY });
        *cost = (0, 0);
    }
}

/// Instance runs' shared recordings: for each analysis key (which instances
/// may draw a run alike), the distinct recordings seen under it, each with a
/// layer key of its own. Equal content always gets the same layer key, and a
/// layer key always means one content, which the backend's tile cache
/// relies on.
#[derive(Debug, Default)]
pub struct Shared {
    variants: HashMap<u64, Vec<(u64, Arc<Recorded>)>>,
    next: u64,
}

impl Shared {
    /// The layer key and shared recording for `rec` under analysis key `k`.
    pub fn find_or_add(&mut self, k: u64, rec: Recorded) -> (u64, Arc<Recorded>) {
        let list = self.variants.entry(k).or_default();
        if let Some((key, have)) = list.iter().find(|(_, have)| **have == rec) {
            return (*key, have.clone());
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (k, self.next).hash(&mut h);
        self.next += 1;
        let entry = (h.finish(), Arc::new(rec));
        list.push(entry.clone());
        entry
    }

    /// Drop the variants no instance holds any more.
    pub fn collect(&mut self) {
        for list in self.variants.values_mut() {
            list.retain(|(_, r)| Arc::strong_count(r) > 1);
        }
        self.variants.retain(|_, l| !l.is_empty());
    }
}

impl Chunk {
    /// Whether it has never been recorded.
    pub fn fresh(&self) -> bool {
        if self.runs.is_empty() { self.recorded.is_none() } else { self.runs.iter().any(|r| r.recorded.is_none()) }
    }

    /// What it draws, in its composition's space.
    pub fn bounds(&self) -> Option<kurbo::Rect> {
        let b = if self.runs.is_empty() {
            self.recorded.as_ref().and_then(|r| r.bounds())
        } else {
            self.runs.iter().filter_map(|r| r.recorded.as_ref()?.bounds()).reduce(|a, b| a.union(b))
        };
        b.map(|b| self.frame.transform_rect_bbox(b))
    }
}

/// The runs a linked instance's picture is drawn in, when `id` is one that
/// can be: a `use` of a linked instance whose picture composites nothing as
/// a whole (no opacity or clip on it or on a container inside it). `None`
/// for anything else, which chunks as before.
pub fn instance_runs(store: &Store, id: NodeId) -> Option<Vec<InstanceRun>> {
    use iron_document::analysis;
    use std::hash::{Hash, Hasher};
    let doc = store.document();
    // A reference: its template drawn whole, one static run every
    // reference to it with the same turn and scale shares.
    if let Some(template) = iron_document::expand::reference_template(doc, id) {
        if doc.facet(id, FacetKind::Appearance).is_some() {
            return None;
        }
        let parts: Vec<NodeId> = doc.children(template).iter().copied().filter(|&c| doc.is_live(c)).collect();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ("reference", template).hash(&mut h);
        return (!parts.is_empty()).then(|| vec![InstanceRun { root: template, whole: vec![true; parts.len()], parts, share: Some(h.finish()), key: 0, recorded: None }]);
    }
    if id.is_expanded() || doc.live(id)?.ty.name() != "use" {
        return None;
    }
    iron_document::expand::linked_definition(doc, iron_document::expand::use_entity(doc, id)?)?;
    let composites = |n: NodeId| doc.facet(n, FacetKind::Appearance).is_some() || iron_document::appearance::ClipFrame::read(store, n).is_some();
    if composites(id) {
        return None;
    }
    let a = analysis::classify(doc, store.reactive(), id)?;
    if a.parts.iter().any(|p| !p.draws && composites(p.copy)) {
        return None;
    }
    let mut out = Vec::new();
    for (i, run) in analysis::runs(&a).into_iter().enumerate() {
        let parts: Vec<&analysis::PartClass> = run.parts.iter().filter_map(|n| a.parts.iter().find(|p| p.node == *n)).collect();
        let whole = parts.iter().map(|p| doc.live(p.copy).is_some_and(|n| matches!(n.ty.name(), "path" | "use"))).collect();
        let share = (run.class != analysis::Class::Live).then(|| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (a.definition, &a.notation, i).hash(&mut h);
            for p in &parts {
                for (k, v) in &p.key {
                    (k, format!("{v:?}")).hash(&mut h);
                }
            }
            h.finish()
        });
        out.push(InstanceRun { root: id, parts: parts.iter().map(|p| p.copy).collect(), whole, share, key: 0, recorded: None });
    }
    (!out.is_empty()).then_some(out)
}

/// One panel's layer for one chunk, or for one run of an instance chunk.
#[derive(Component, Debug)]
pub struct ChunkLayer {
    pub panel: String,
    pub home: Home,
    /// The chunk's first root: its identity across re-cuts that keep it.
    pub first: NodeId,
    /// Which of an instance chunk's runs; 0 for any other chunk.
    pub run: u32,
}
