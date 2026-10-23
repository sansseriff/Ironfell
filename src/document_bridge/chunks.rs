//! Chunks (plans/specs/render-cache.md §4): each composition's 2D content
//! cut into painter-contiguous pieces, each recorded once in its
//! composition's space and shared by every panel that shows it.
//!
//! A panel draws its chunks as layers of their own, each carrying the
//! panel's pan and zoom (`VectorLayer::affine`). So a pan re-records and
//! re-extracts nothing, and an edit re-records only the chunks it touched.
//! The backend still encodes every visible layer when the view changes; the
//! raster layers of the spec's phase 2 are what remove that.
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
    /// The shared recording; `None` for an embed chunk, or before the first.
    pub recorded: Option<Arc<Recorded>>,
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
}

/// Where a split group's nodes point: a change to one means the cut may be
/// wrong now (an appearance added), so its home is cut again.
const SPLIT: usize = usize::MAX;

#[derive(Resource, Default)]
pub struct Chunks {
    pub homes: BTreeMap<Home, Partition>,
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
        let mut cut = Cut { store, doc, drawables, cost: HashMap::new(), embeds: HashMap::new(), out: Partition::default() };
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

    /// Which chunk of its home `node` is drawn in, if any: `Err(())` when it
    /// is a split group, whose change means cutting the home again.
    pub fn chunk_of(&self, home: Home, node: NodeId) -> Option<Result<usize, ()>> {
        let i = *self.homes.get(&home)?.of_node.get(&node)?;
        Some(if i == SPLIT { Err(()) } else { Ok(i) })
    }
}

struct Cut<'a> {
    store: &'a Store,
    doc: &'a Document,
    drawables: &'a Drawables,
    cost: HashMap<NodeId, (usize, usize)>,
    embeds: HashMap<NodeId, bool>,
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

    /// Cost and node count of `id`'s subtree.
    fn subtree(&mut self, id: NodeId) -> (usize, usize) {
        if let Some(&c) = self.cost.get(&id) {
            return c;
        }
        let mut c = (self.own(id), 1);
        for &k in self.doc.children(id) {
            let (kc, kn) = self.subtree(k);
            c = (c.0 + kc, c.1 + kn);
        }
        self.cost.insert(id, c);
        c
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
            let (cost, nodes) = self.subtree(id);
            let embeds = self.has_embed(id);
            let whole = self.whole(id);
            if (cost > CHUNK_MAX || embeds) && !whole {
                self.flush(&mut run, &mut run_cost);
                self.out.of_node.insert(id, SPLIT);
                let kids = self.doc.children(id).to_vec();
                self.pack(&kids);
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
        self.out.chunks.push(Chunk { roots, cost: cost.0, nodes: cost.1, embeds, recorded: None, recorded_at: f64::NEG_INFINITY });
        *cost = (0, 0);
    }
}

/// One panel's layer for one chunk.
#[derive(Component, Debug)]
pub struct ChunkLayer {
    pub panel: String,
    pub home: Home,
    /// The chunk's first root: its identity across re-cuts that keep it.
    pub first: NodeId,
}
