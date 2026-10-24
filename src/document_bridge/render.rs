//! The document's 2D content as one display-list layer, in painter order.
//!
//! One layer for all 2D nodes is the batched tier of
//! `plans/specs/document-spine.md` §10: many small primitives share one DisplayList
//! entity and the layer rebuilds when any of them changed. Finer chunking is
//! the raster cache's job later (doc 07 §4.2).
//!
//! What triggers a rebuild is the reconciler's changed-node set
//! ([`DocumentChanges`]), not a boolean: the layer knows which nodes moved,
//! consumes independently prepared bounds/index ([`SpatialScene`]), and
//! emits only visible candidates in painter order. Each affected view still
//! rebuilds its visible display list; partial encoding/rasterization is later.
//!
//! Each 2D composition panel draws its own composition's nodes, offset to
//! and clipped by its rectangle (ECM §7.1); a node outside every composition
//! draws in the viewer panel. An embed of a 2D composition draws that
//! composition in place, through the embed's transform and clipped to its
//! region, recursively (ECM §7.4).
//!
//! Panels draw content only: hover and selection feedback is the editor
//! overlay's, in its own view (`bevy_app::select::draw_select_overlay`), so
//! a feedback change leaves every panel's content list, and its encoding,
//! untouched. A plane's view still draws lit nodes outlined, since it has
//! no overlay of its own.

use super::{SpatialScene, Doc2d, DocumentChanges, DocumentStore, Highlights, PanelViews, Shape2d, nest};
use super::chunks::{ChunkLayer, Chunks, Place, panel_homes};
use iron_document::composition::Dims;
use crate::vector::{DisplayList, DisplayListRebuild, Recorded, VectorLayer, VectorView, order};
use bevy::prelude::*;
use iron_document::NodeId;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// The clip a chunk records under: none. A chunk is recorded once for
/// every panel, so it cannot be cut to any one view; the backend culls
/// per layer and per command instead.
const UNCLIPPED: kurbo::Rect = kurbo::Rect::new(-1e9, -1e9, 1e9, 1e9);

/// One document layer per 2D panel, each its own [`VectorView`]: it renders
/// into a texture the panel's size and re-renders only when its own content
/// changed (plans/specs/view-textures.md).
#[derive(Component, Debug)]
pub struct DocumentPanelLayer {
    pub panel: String,
}

/// Keep one panel layer per 2D composition panel, at the panel's rectangle.
pub(super) fn sync_document_views(
    mut commands: Commands,
    views: Res<PanelViews>,
    mut layers: Query<(Entity, &DocumentPanelLayer, &mut VectorView)>,
) {
    let wanted: Vec<_> = views.0.iter().filter(|v| v.dims == Dims::Two).collect();
    for (e, layer, _) in &layers {
        if !wanted.iter().any(|v| v.panel == layer.panel) {
            commands.entity(e).despawn();
        }
    }
    for view in wanted {
        let r = view.rect;
        let next = VectorView { pos: UVec2::new(r.x.max(0.0) as u32, r.y.max(0.0) as u32), size: UVec2::new(r.w.max(1.0) as u32, r.h.max(1.0) as u32) };
        match layers.iter_mut().find(|(_, l, _)| l.panel == view.panel) {
            Some((_, _, mut v)) => {
                if *v != next {
                    *v = next;
                }
            }
            None => {
                let e = commands.spawn((DisplayList::default(), next, DocumentPanelLayer { panel: view.panel.clone() })).id();
                commands.entity(e).insert(VectorLayer::screen(order::DOCUMENT_2D).in_view(e));
            }
        }
    }
}

/// A chunk's key for the backend's raster cache (`VectorLayer::cache`): its
/// home and first root, which stay while a re-cut keeps it, and whether it
/// is a render inspector's tinted copy, which must not share its tiles.
fn cache_key(home: nest::Home, first: NodeId, tinted: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (home, first, tinted).hash(&mut h);
    h.finish()
}

/// The definition template `n` belongs to, if it is inside one.
fn template_of(doc: &iron_document::Document, n: NodeId) -> Option<NodeId> {
    let mut at = doc.node(n).and_then(|x| x.parent);
    while let Some(p) = at {
        let node = doc.node(p)?;
        match node.ty.name() {
            "template" => return Some(p),
            "composition" => return None,
            _ => at = node.parent,
        }
    }
    None
}

/// Two keys made one.
fn mix(a: u64, b: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (a, b).hash(&mut h);
    h.finish()
}

/// How many changed node ids a redraw line names before summarising.
const LOG_IDS: usize = 12;

#[allow(clippy::too_many_arguments)]
pub fn render_document_layer(
    mut commands: Commands,
    mut panel_layers: Query<(Entity, &mut DisplayList, &DocumentPanelLayer), (Without<super::planes::PlaneLayer>, Without<ChunkLayer>)>,
    mut plane_layers: Query<(&mut DisplayList, &super::planes::PlaneLayer, &VectorView), (Without<DocumentPanelLayer>, Without<ChunkLayer>)>,
    mut chunk_layers: Query<(Entity, &ChunkLayer, &mut DisplayList, &mut VectorLayer), (Without<DocumentPanelLayer>, Without<super::planes::PlaneLayer>)>,
    mut chunks: ResMut<Chunks>,
    time: Res<Time>,
    inspector: Res<super::inspector::InspectorOptions>,
    new_panels: Query<(), Or<(Added<DocumentPanelLayer>, Changed<super::planes::PlaneLayer>)>>,
    drawables: Res<super::Drawables>,
    store: Res<DocumentStore>,
    mut changes: ResMut<DocumentChanges>,
    spatial: Res<SpatialScene>,
    views: Res<PanelViews>,
    highlights: Res<Highlights>,
    embed_images: Res<super::EmbedImages>,
) {
    let reason = if !changes.is_empty() {
        "document"
    } else if spatial.is_changed() {
        "spatial projection"
    } else if views.is_changed() || inspector.is_changed() {
        "panel layout"
    } else if !new_panels.is_empty() {
        "new view"
    } else if embed_images.is_changed() {
        "embedded view"
    } else if highlights.is_changed() {
        // Only planes draw highlights; panels' lists would rebuild equal.
        "selection"
    } else {
        return;
    };
    let planes_only = reason == "selection";
    let _timing = crate::perf::span(crate::perf::Phase::DisplayList);
    let changed = changes.take();
    let doc = store.0.document();

    // Damage belongs to the independently prepared canonical compositions.
    // It is diagnostic here; per-view damage mapping/partial rasterization is later.
    let damage = spatial.damage.iter().copied().reduce(|a,b| a.union(b));
    let mut cx = Draw { store: &store.0, embed_images: &embed_images, doc, drawables: &drawables, highlights: None, spatial: &spatial, drawn: 0, relative: None };
    // Where the changed nodes draw, when only the document changed: a
    // panel rebuilds only for changes it shows (render-cache.md §4.3), so a
    // Stats tick does not rebuild a 29k-node drawing beside it. Any other
    // reason, or a structure change with no nodes named, rebuilds every
    // panel. A panel that does rebuild unchanged gets equal commands, so its
    // view does not re-render.
    // An edit inside a definition's template shows wherever it is
    // referenced: every panel rebuilds.
    let in_templates = changed.nodes.iter().any(|&n| template_of(doc, n).is_some());
    let homes: Option<std::collections::BTreeSet<nest::Home>> = (reason == "document" && !changed.nodes.is_empty() && !in_templates)
        .then(|| changed.nodes.iter().map(|&n| nest::home(doc, n)).collect())
        .filter(|h: &std::collections::BTreeSet<nest::Home>| !h.contains(&nest::Home::Unknown));
    let shows = |composition: Option<NodeId>, loose: bool| {
        homes.as_ref().is_none_or(|homes| {
            let shown = nest::shown_compositions(doc, composition);
            homes.iter().any(|h| match h {
                nest::Home::Composition(c) => shown.contains(c),
                nest::Home::Loose => loose,
                nest::Home::Unknown => true,
            })
        })
    };
    let mut skipped = 0_usize;
    if !planes_only {
        // Panels draw chunks (render-cache.md §4): cut what they show,
        // re-record only the chunks whose nodes changed, and give each panel
        // a layer per chunk under its own pan and zoom.
        let wanted: BTreeSet<nest::Home> = views.0.iter().filter(|v| v.dims == Dims::Two).flat_map(panel_homes).collect();
        // A spatial or embedded-view change can move what any chunk draws
        // (an embed's region, a 3D texture) without naming its nodes.
        let everything = !matches!(reason, "document" | "panel layout" | "new view");
        let mut recut: BTreeSet<nest::Home> = wanted.iter().copied().filter(|h| changed.structure || !chunks.homes.contains_key(h)).collect();
        let mut dirty: BTreeMap<nest::Home, BTreeSet<usize>> = BTreeMap::new();
        // Chunks whose frame may have moved, with nothing they draw changed.
        let mut reframe: BTreeMap<nest::Home, BTreeSet<usize>> = BTreeMap::new();
        for &n in &changed.nodes {
            // Inside a definition's template: every chunk that shows it
            // through a reference (instance-cache.md phase D).
            if let Some(t) = template_of(doc, n) {
                for (home, part) in &chunks.homes {
                    if let Some(shown) = part.shows_template.get(&t) {
                        dirty.entry(*home).or_default().extend(shown.iter().copied());
                    }
                }
                continue;
            }
            let home = nest::home(doc, n);
            if !wanted.contains(&home) {
                continue;
            }
            match chunks.chunk_of(home, n) {
                // An instance moved as a whole: its runs are recorded
                // relative to it, so only its frame changes.
                Some(Place::Chunk(i)) if chunks.homes.get(&home).and_then(|p| p.chunks.get(i)).is_some_and(|c| !c.runs.is_empty() && c.roots[0] == n) => {
                    reframe.entry(home).or_default().insert(i);
                }
                Some(Place::Chunk(i)) => {
                    dirty.entry(home).or_default().insert(i);
                }
                // A split group changed: it draws nothing itself, so only
                // its placement can matter, which its chunks' frames carry
                // (instance-cache.md §2). No re-recording, no cutting again.
                Some(Place::Split(range)) if !Chunks::must_cut_again(&store.0, &drawables, n) => {
                    reframe.entry(home).or_default().extend(range);
                }
                // A split group that must now be drawn whole, or a node no
                // chunk holds (the composition itself): cut again.
                _ => {
                    recut.insert(home);
                }
            }
        }
        for &home in &recut {
            chunks.partition(&store.0, &drawables, home);
        }
        chunks.homes.retain(|h, _| wanted.contains(h));
        let Chunks { homes: chunk_homes, shared, .. } = &mut *chunks;
        let now = time.elapsed_secs_f64();
        for (home, part) in chunk_homes.iter_mut() {
            for (i, chunk) in part.chunks.iter_mut().enumerate() {
                let mut record = chunk.fresh() || everything || dirty.get(home).is_some_and(|d| d.contains(&i));
                if !chunk.runs.is_empty() {
                    // An instance: recorded relative to its own position.
                    if record || reframe.get(home).is_some_and(|d| d.contains(&i)) {
                        let [a, b, c, d, e, f] = cx.world(chunk.roots[0]).as_coeffs();
                        let basis = kurbo::Affine::new([a, b, c, d, 0.0, 0.0]);
                        record |= basis != chunk.basis;
                        chunk.basis = basis;
                        chunk.frame = kurbo::Affine::translate((e, f));
                    }
                    if record && cx.record_instance(chunk, *home, shared) {
                        chunk.recorded_at = now;
                    }
                    continue;
                }
                // The frame is the parent's position only: a chunk is
                // recorded with its parent's turn and scale applied, so its
                // coordinates stay composition-sized and a large import's
                // raw ones never meet a large opposite translation in f32.
                // A parent that turned or scaled is recorded again.
                if !chunk.embeds && (record || reframe.get(home).is_some_and(|d| d.contains(&i))) {
                    let w = cx.parent_world(chunk.roots[0]);
                    let [a, b, c, d, e, f] = w.as_coeffs();
                    let basis = kurbo::Affine::new([a, b, c, d, 0.0, 0.0]);
                    record |= basis != chunk.basis;
                    chunk.basis = basis;
                    chunk.frame = kurbo::Affine::translate((e, f));
                }
                if chunk.embeds || !record {
                    continue;
                }
                let into = chunk.frame.inverse();
                let rec = Recorded::record(|b| cx.branches(b, &chunk.roots, into, UNCLIPPED, "", 0, None));
                if chunk.recorded.as_deref() != Some(&rec) {
                    chunk.recorded = Some(Arc::new(rec));
                    chunk.recorded_at = now;
                }
            }
        }
        shared.collect();
        // Each panel's layers: its homes' chunks in painter order. An embed
        // chunk is recorded per panel, and only when the panel shows the
        // change.
        let mut want: HashMap<(String, nest::Home, NodeId, u32), (Arc<Recorded>, u32, kurbo::Affine, Entity, Option<u64>)> = HashMap::new();
        let mut per_panel = std::mem::take(&mut chunks.per_panel);
        let mut kept: HashMap<(String, NodeId), Arc<Recorded>> = HashMap::new();
        let mut tinted = HashMap::new();
        let mut inspected: Vec<(Arc<Recorded>, NodeId, (String, nest::Home, NodeId, u32))> = Vec::new();
        for (entity, mut panel_list, layer) in &mut panel_layers {
            let Some(view) = views.get(&layer.panel) else { continue };
            // The panel's own list stays empty: its chunks draw.
            panel_list.rebuild(|_| {});
            let panel_shows = shows(view.composition, view.loose);
            if !panel_shows {
                skipped += 1;
            }
            let mut sub = 0_u32;
            for home in panel_homes(view) {
                let Some(part) = chunks.homes.get(&home) else { continue };
                for chunk in &part.chunks {
                    let first = chunk.roots[0];
                    let tint = view.observes.is_some() && inspector.0.get(&view.panel).is_none_or(|o| o.tint);
                    // An instance: a layer per run, a static or keyed one
                    // cached under the run's key, which equal instances
                    // share.
                    if !chunk.runs.is_empty() {
                        for (r, run) in chunk.runs.iter().enumerate() {
                            let Some(rec) = run.recorded.clone() else { continue };
                            let key = (view.panel.clone(), home, first, r as u32);
                            if tint {
                                inspected.push((rec.clone(), first, key.clone()));
                            }
                            // A live run is known to change: drawn live,
                            // never grouped with what is baked around it.
                            let cache = run.share.map(|_| if tint { mix(run.key, 1) } else { run.key });
                            want.insert(key, (rec, sub, view.affine() * chunk.frame, entity, cache));
                            sub += 1;
                        }
                        continue;
                    }
                    // A chunk that draws an embed changes with it (a 3D
                    // view's texture), so only the others may be cached.
                    let cache = (!chunk.embeds).then(|| cache_key(home, first, tint));
                    let rec = if chunk.embeds {
                        let key = (view.panel.clone(), first);
                        let old = per_panel.remove(&key);
                        let rec = match old {
                            Some(old) if !panel_shows && !everything => old,
                            old => {
                                let into = chunk.frame.inverse();
                                let rec = Recorded::record(|b| cx.branches(b, &chunk.roots, into, UNCLIPPED, &view.panel, 0, None));
                                match old {
                                    Some(old) if *old == rec => old,
                                    _ => Arc::new(rec),
                                }
                            }
                        };
                        kept.insert(key, rec.clone());
                        rec
                    } else {
                        let Some(rec) = chunk.recorded.clone() else { continue };
                        rec
                    };
                    // A render inspector shows each chunk in its own colour.
                    if tint {
                        inspected.push((rec.clone(), first, (view.panel.clone(), home, first, 0)));
                    }
                    want.insert((view.panel.clone(), home, first, 0), (rec, sub, view.affine() * chunk.frame, entity, cache));
                    sub += 1;
                }
            }
        }
        chunks.per_panel = kept;
        for (rec, first, key) in inspected {
            let t = chunks.tinted(&rec, first, &mut tinted);
            if let Some(w) = want.get_mut(&key) {
                w.0 = t;
            }
        }
        chunks.keep_tinted(tinted);
        for (e, layer, mut list, mut vl) in &mut chunk_layers {
            let Some((rec, sub, affine, view, cache)) = want.remove(&(layer.panel.clone(), layer.home, layer.first, layer.run)) else {
                commands.entity(e).despawn();
                continue;
            };
            list.set(rec);
            if vl.sub != sub || vl.affine != affine || vl.view != Some(view) || vl.cache != cache {
                vl.sub = sub;
                vl.affine = affine;
                vl.view = Some(view);
                vl.cache = cache;
            }
        }
        for ((panel, home, first, run), (rec, sub, affine, view, cache)) in want {
            let mut vl = VectorLayer::screen(order::DOCUMENT_2D).in_view(view);
            vl.sub = sub;
            vl.affine = affine;
            vl.cache = cache;
            commands.spawn((DisplayList::from(rec), vl, ChunkLayer { panel, home, first, run }));
        }
    }
    // A plane's view: its composition's region, at its density, into its
    // own texture (`planes`), lit nodes outlined.
    cx.highlights = Some(&highlights);
    for (mut plane_list, layer, vview) in &mut plane_layers {
        if !planes_only && !shows(Some(layer.source), false) {
            skipped += 1;
            continue;
        }
        let roots: Vec<NodeId> = doc.children(layer.source).to_vec();
        let key = format!("plane/{}", layer.embed);
        let clip = kurbo::Rect::new(0.0, 0.0, vview.size.x as f64, vview.size.y as f64);
        plane_list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, clip, |b| cx.walk(b, &roots, layer.affine(), clip, &key, 0));
        });
    }
    let drawn = cx.drawn;
    // A frame that rebuilt nothing leaves the last count standing.
    if drawn > 0 || skipped == 0 {
        crate::perf::set_drawn(drawn as u32);
    }

    let mut ids: Vec<String> = changed.nodes.iter().take(LOG_IDS).map(|id| id.to_string()).collect();
    if changed.nodes.len() > LOG_IDS {
        ids.push(format!("+{} more", changed.nodes.len() - LOG_IDS));
    }
    let damage = damage.map(|r| format!("{:.0},{:.0}–{:.0},{:.0}", r.x0, r.y0, r.x1, r.y1)).unwrap_or_else(|| "none".into());
    if drawn == 0 && skipped > 0 {
        // Only views that show none of it: nothing rebuilt, nothing to say.
        return;
    }
    info!(
        "redraw ({reason}{}): {} of {drawn} nodes changed [{}]; damage {damage}",
        if changed.structure { ", structure" } else { "" },
        changed.nodes.len(),
        ids.join(" "),
    );
}

/// One rebuild's walk: draws a composition's nodes in painter order and
/// recurses into 2D embeds through their placement, clipped to their region.
struct Draw<'a> {
    store: &'a iron_document::Store,
    embed_images: &'a super::EmbedImages,
    doc: &'a iron_document::Document,
    drawables: &'a super::Drawables,
    /// Set for plane views only; panels leave feedback to the overlay.
    highlights: Option<&'a Highlights>,
    spatial: &'a SpatialScene,
    drawn: usize,
    /// While an instance's runs record: the instance and its turn and scale.
    /// Its parts are then placed relative to it, from the template's own
    /// local transforms, so equal instances record equal lists wherever they
    /// are.
    relative: Option<(NodeId, kurbo::Affine)>,
}

impl Draw<'_> {
    fn walk(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, roots: &[NodeId], base: kurbo::Affine, clip: kurbo::Rect, key: &str, depth: u32) {
        // Expand in device space for antialiasing, then conservatively map the
        // clip AABB back through the view/occurrence affine. Exact clips remain
        // in the display list, so rotation cannot expose extra pixels.
        let visible = base.inverse().transform_rect_bbox(clip.inflate(2.0, 2.0));
        let candidates = if self.spatial.enabled {
            self.spatial.candidates(roots, None, visible)
        } else { nest::preorder(self.doc, roots) };
        // Preserve the spatial query, then restore ancestor scopes around its
        // painter-ordered leaves. A group composites once, not once per leaf.
        let mut allowed = std::collections::BTreeSet::new();
        for id in candidates {
            let mut at = Some(id);
            while let Some(n) = at {
                if !allowed.insert(n) { break; }
                at = self.doc.live(n).and_then(|n| n.parent);
            }
        }
        self.branches(b, roots, base, clip, key, depth, Some(&allowed));
    }

    fn number(&self, id: NodeId, leaf: &str, fallback: f64) -> f64 {
        self.store.resolved(id, leaf.parse().expect("declared leaf")).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(fallback)
    }

    /// Draw `roots` with their subtrees in painter order: every node, or
    /// only those in `allowed` (a view's culled set).
    fn branches(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, roots: &[NodeId], base: kurbo::Affine, clip: kurbo::Rect, key: &str, depth: u32, allowed: Option<&BTreeSet<NodeId>>) {
        for &id in roots {
            if allowed.is_some_and(|a| !a.contains(&id)) { continue; }
            let opacity = self.number(id, "appearance.opacity", 1.0).clamp(0.0, 1.0) as f32;
            let frame = iron_document::appearance::ClipFrame::read(self.store, id);
            b.opacity(opacity, clip, |b| {
                if let Some(frame) = frame {
                    let (w, h, r) = (frame.w, frame.h, frame.radius);
                    let rect = kurbo::Rect::new(0.0, 0.0, w, h);
                    let shape = if frame.ellipse {
                        use kurbo::Shape;
                        kurbo::Ellipse::new((w/2.0, h/2.0), (w/2.0, h/2.0), 0.0).to_path(0.1).into()
                    } else { crate::vector::Shape::RoundedRect(rect.to_rounded_rect(r)) };
                    let placement = self.world(id);
                    let affine = base * placement;
                    let inner_clip = clip.intersect(affine.transform_rect_bbox(rect));
                    b.clipped(affine, shape, |b| self.content(b, id, base, inner_clip, key, depth, allowed));
                } else {
                    self.content(b, id, base, clip, key, depth, allowed);
                }
            });
        }
    }

    fn content(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, id: NodeId, base: kurbo::Affine, clip: kurbo::Rect, key: &str, depth: u32, allowed: Option<&BTreeSet<NodeId>>) {
            // A reference: its definition's template, in place, placed
            // through it (instance-cache.md phase D).
            if let Some(template) = nest::reference(self.doc, id) {
                if depth >= nest::MAX_DEPTH {
                    return;
                }
                let outer = self.relative;
                self.relative = Some((template, self.world(id)));
                let kids: Vec<NodeId> = self.doc.children(template).to_vec();
                self.branches(b, &kids, base, clip, key, depth + 1, None);
                self.relative = outer;
                return;
            }
            if nest::is_embed(self.doc, id) {
                if depth >= nest::MAX_DEPTH {
                    return;
                }
                let key = format!("{key}/{id}");
                if let Some(e) = nest::embed_2d(self.store, self.spatial, id) {
                    let inner = base * e.placement;
                    let kids: Vec<NodeId> = self.doc.children(e.source).to_vec();
                    let inner_clip = clip.intersect(inner.transform_rect_bbox(e.region));
                    b.clipped(inner, e.region, |b| self.walk(b, &kids, inner, inner_clip, &key, depth + 1));
                } else if let Some((_, _, w, h)) = nest::embed_3d(self.store, id) {
                    // The 3D view: its camera's texture (`EmbedImages`), drawn
                    // in painter order under the embed's whole transform, so
                    // it rotates, scales and clips like the 2D around it.
                    let placement = self.world(id);
                    if let Some(img) = self.embed_images.0.get(&key) {
                        let to_embed = kurbo::Affine::scale_non_uniform(w / img.size.x.max(1) as f64, h / img.size.y.max(1) as f64);
                        b.texture(base * placement * to_embed, img.image.clone(), img.size, img.version);
                    }
                    let frame = kurbo::Rect::new(0.0, 0.0, w, h);
                    let lit = self.lit(id).is_some();
                    let c = if lit { [0.1, 0.4, 1.0, 1.0] } else { [0.55, 0.57, 0.62, 1.0] };
                    b.stroke(base * placement, kurbo::Stroke::new(1.0), peniko::Color::new(c), frame);
                }
                return;
            }
            self.node(b, id, base);
            self.branches(b, self.doc.children(id), base, clip, key, depth, allowed);
    }

    /// A node's placement in its composition's space (`nest::world_2d`), or
    /// inside an instance being recorded, relative to the instance.
    fn world(&self, id: NodeId) -> kurbo::Affine {
        if let Some((root, basis)) = self.relative {
            let read = |l: iron_document::Leaf| self.store.resolved(l.node, l.path);
            let mut chain = Vec::new();
            let mut at = Some(id);
            while let Some(n) = at.filter(|&n| n != root) {
                chain.push(n);
                at = self.doc.live(n).and_then(|x| x.parent);
            }
            if at == Some(root) {
                return chain.iter().rev().fold(basis, |acc, &n| acc * kurbo::Affine::new(iron_document::geom::local_affine(self.doc, &read, n).0));
            }
        }
        nest::world_2d(self.store, self.spatial, id)
    }

    /// Record an instance chunk's runs, relative to it. A static or keyed
    /// run shares the recording of every equal instance: its analysis key
    /// and the instance's turn and scale say which instances may be equal,
    /// and among those, each distinct recording is a variant with a layer
    /// key of its own ([`super::chunks::Shared`]), so one key always means
    /// one content, whatever the analysis missed (a font that loaded since).
    /// Returns whether any run's recording changed.
    fn record_instance(&mut self, chunk: &mut super::chunks::Chunk, home: nest::Home, shared: &mut super::chunks::Shared) -> bool {
        // The classes can change without a re-cut (an input wired or
        // unwired), so the runs are worked out again.
        if let Some(runs) = super::chunks::instance_runs(self.store, chunk.roots[0]) {
            let same = runs.len() == chunk.runs.len() && runs.iter().zip(&chunk.runs).all(|(a, b)| a.parts == b.parts && a.share == b.share);
            if !same {
                chunk.runs = runs;
            }
        }
        let into = kurbo::Affine::IDENTITY;
        let [a, b, c, d, ..] = chunk.basis.as_coeffs();
        let basis = mix(mix(a.to_bits(), b.to_bits()), mix(c.to_bits(), d.to_bits()));
        let mut changed = false;
        for (r, run) in chunk.runs.iter_mut().enumerate() {
            self.relative = Some((run.root, chunk.basis));
            let rec = Recorded::record(|bl| {
                for (&p, &whole) in run.parts.iter().zip(&run.whole) {
                    if whole {
                        self.branches(bl, &[p], into, UNCLIPPED, "", 0, None);
                    } else {
                        self.node(bl, p, into);
                    }
                }
            });
            let own = mix(cache_key(home, chunk.roots[0], false), r as u64);
            let (key, arc) = match run.share.map(|s| mix(s, basis)) {
                Some(k) => shared.find_or_add(k, rec),
                None => match &run.recorded {
                    Some(have) if run.key == own && **have == rec => (own, have.clone()),
                    _ => (own, Arc::new(rec)),
                },
            };
            if run.key != key || run.recorded.as_ref().is_none_or(|have| !Arc::ptr_eq(have, &arc)) {
                changed = true;
            }
            run.key = key;
            run.recorded = Some(arc);
        }
        self.relative = None;
        changed
    }

    /// The placement of the parent a chunk starting at `root` hangs from,
    /// in its composition: identity for a composition's own children.
    fn parent_world(&self, root: NodeId) -> kurbo::Affine {
        let Some(parent) = self.doc.live(root).and_then(|n| n.parent) else { return kurbo::Affine::IDENTITY };
        if self.doc.live(parent).is_none_or(|p| p.ty.name() == "composition") {
            return kurbo::Affine::IDENTITY;
        }
        let w = self.world(parent);
        if w.as_coeffs().iter().all(|c| c.is_finite()) { w } else { kurbo::Affine::IDENTITY }
    }

    fn node(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, id: NodeId, base: kurbo::Affine) {
        let Some(d) = self.drawables.0.get(&id) else { return };
        self.drawn += 1;
        // Node-local placement; `base` adds the panel offset and any embeds
        // only at emit time, so bounds stay in the node's composition space.
        let local = self.world(id);
        let affine = base * local;
        let shape = nest::draw_shape(&d.shape);
        // What fill, stroke and shadow paint: the shape, or a text's glyphs
        // (its `shape` is its box, which only picking and outlines use).
        let painted = match &d.shape {
            Shape2d::Text { glyphs, .. } => glyphs.clone().into(),
            _ => shape.clone(),
        };
        if let Shape2d::Slider { w, h, t } = d.shape.clone() {
            // Track, filled part, knob. The hit shape stays the whole w×h
            // box so the knob is easy to grab.
            let (w, h, t) = (w as f64, h as f64, t as f64);
            let mid = h * 0.5;
            let track = kurbo::RoundedRect::new(0.0, mid - 3.0, w, mid + 3.0, 3.0);
            b.fill(affine, peniko::Color::new([0.75, 0.76, 0.8, 1.0]), track);
            let filled = kurbo::RoundedRect::new(0.0, mid - 3.0, (w * t).max(6.0), mid + 3.0, 3.0);
            b.fill(affine, peniko::Color::new(d.fill.unwrap_or([0.2, 0.5, 0.9, 1.0])), filled);
            let knob = kurbo::Circle::new((w * t, mid), mid.min(12.0));
            b.fill(affine, peniko::Color::new([1.0, 1.0, 1.0, 1.0]), knob);
            b.stroke(affine, kurbo::Stroke::new(1.5), peniko::Color::new([0.3, 0.3, 0.35, 1.0]), knob);
        } else {
            // A path fills its closed contours by its rule; an open contour
            // has no inside.
            let paint = |b: &mut crate::vector::DisplayListBuilder<'_>| {
                // Text paints its own pieces: outline glyphs in the node's
                // fill or a run's colour (and the node's stroke), colour
                // glyphs in their own colours.
                if let Shape2d::Text { paints, offset, .. } = &d.shape {
                    let at = affine * kurbo::Affine::translate((offset[0] as f64, offset[1] as f64));
                    let node_brush = self.fill_brush(id, d, &shape);
                    let node_color = d.fill.unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    for p in paints.iter() {
                        match p {
                            iron_text::TextPaint::Fill { color, path } => {
                                let path: crate::vector::Shape = path.clone().into();
                                let brush = match color {
                                    Some(c) => Some(peniko::Color::new(*c).into()),
                                    None => node_brush.clone(),
                                };
                                if let Some(brush) = brush {
                                    b.fill(at, brush, path.clone());
                                }
                                if let Some((c, style)) = &d.stroke {
                                    b.stroke(at, style.clone(), peniko::Color::new(*c), path);
                                }
                            }
                            iron_text::TextPaint::Color { transform, ops, foreground } => {
                                color_glyph(b, at * *transform, &mut ops.iter(), foreground.unwrap_or(node_color));
                            }
                        }
                    }
                    return;
                }
                if let Some(brush) = self.fill_brush(id, d, &shape) {
                    match &d.shape {
                        Shape2d::Path { fill: Some(f), even_odd, .. } => {
                            let rule = if *even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero };
                            b.fill_with_rule(affine, brush, rule, f.clone());
                        }
                        Shape2d::Path { fill: None, .. } => {}
                        _ => b.fill(affine, brush, painted.clone()),
                    }
                }
                if let Some((c, style)) = &d.stroke {
                    b.stroke(affine, style.clone(), peniko::Color::new(*c), painted.clone());
                }
            };
            match d.shadow.filter(|s| s.color[3] > 0.0) {
                None => paint(b),
                Some(s) => {
                    // The offset is in the parent's space, so the shadow
                    // stays below a rotated shape, as in Figma.
                    let parent = self.doc.live(id).and_then(|n| n.parent).map(|p| self.world(p)).unwrap_or(kurbo::Affine::IDENTITY);
                    let offset = kurbo::Vec2::new(s.dx as f64, s.dy as f64);
                    let color = peniko::Color::new(s.color);
                    let std_dev = s.std_dev as f64;
                    // A plain or evenly rounded rect: Vello's analytic blur,
                    // no offscreen pass. Anything else: a filter layer.
                    let analytic = match &painted {
                        crate::vector::Shape::Rect(r) => Some((*r, 0.0)),
                        crate::vector::Shape::RoundedRect(r) => r.radii().as_single_radius().map(|radius| (r.rect(), radius)),
                        _ => None,
                    };
                    match analytic {
                        Some((rect, radius)) => {
                            let shifted = base * parent * kurbo::Affine::translate(offset) * parent.inverse() * local;
                            b.blurred_rect(shifted, color, rect, radius, std_dev);
                            paint(b);
                        }
                        None => {
                            let in_parent = parent.inverse() * local;
                            let pad = d.stroke.as_ref().map_or(0.0, |(_, st)| st.width);
                            let own = in_parent.transform_rect_bbox(painted.bounding_box().union(shape.bounding_box()).inflate(pad, pad));
                            let blur = 3.0 * std_dev;
                            let bounds = own.union((own + offset).inflate(blur, blur));
                            b.shadowed(base * parent, color, offset, std_dev, bounds, paint);
                        }
                    }
                }
            }
        }
        if let Some(c) = self.lit(id) {
            b.stroke(affine, kurbo::Stroke::new(2.0), peniko::Color::new(c), shape.clone());
        }
    }

    fn fill_brush(&self, id: NodeId, d: &Doc2d, shape: &crate::vector::Shape) -> Option<crate::vector::Brush> {
        let color = |leaf: &str| self.store.resolved(id, leaf.parse().unwrap()).and_then(|v| v.as_color());
        let Some(mut start) = color("gradient.start") else { return d.fill.map(|c| peniko::Color::new(c).into()); };
        let mut end = color("gradient.end").unwrap_or(start);
        let opacity = self.number(id, "fill.opacity", 1.0).clamp(0.0, 1.0) as f32;
        start[3] *= opacity;
        end[3] *= opacity;
        let bounds = shape.bounding_box();
        let point = |x, y| kurbo::Point::new(bounds.x0 + bounds.width() * x, bounds.y0 + bounds.height() * y);
        let p0 = point(self.number(id, "gradient.x0", 0.0), self.number(id, "gradient.y0", 0.0));
        let p1 = point(self.number(id, "gradient.x1", 1.0), self.number(id, "gradient.y1", 1.0));
        let radial = self.store.resolved(id, "gradient.kind".parse().unwrap()).and_then(|v| v.as_str().map(str::to_owned)).as_deref() == Some("radial");
        let gradient = if radial { peniko::Gradient::new_radial(p0, p0.distance(p1).max(0.001) as f32) }
            else { peniko::Gradient::new_linear(p0, if p0 == p1 { p1 + kurbo::Vec2::new(0.001, 0.0) } else { p1 }) };
        Some(gradient.with_stops([peniko::Color::new(start), peniko::Color::new(end)]).into())
    }

    /// The outline colour of a lit node on a plane, if it is lit there.
    fn lit(&self, id: NodeId) -> Option<[f32; 4]> {
        match self.highlights?.covers(self.doc, id) {
            (true, _) => Some(LIT_SELECTED),
            (_, true) => Some(LIT_HOVERED),
            _ => None,
        }
    }
}

/// Replay a colour glyph's ops (`iron_text::ColorOp`), placed by `at`,
/// until the op that closes the current scope: a clip nests what follows
/// up to its `PopClip`, a layer up to its `PopLayer`.
fn color_glyph<'a>(b: &mut crate::vector::DisplayListBuilder<'_>, at: kurbo::Affine, ops: &mut impl Iterator<Item = &'a iron_text::ColorOp>, foreground: [f32; 4]) {
    use iron_text::{ColorFill, ColorOp};
    while let Some(op) = ops.next() {
        match op {
            ColorOp::PushClip { transform, path } => {
                b.clipped(at * *transform, path.clone(), |b| color_glyph(b, at, ops, foreground));
            }
            ColorOp::PushLayer(mode) => b.blended(*mode, |b| color_glyph(b, at, ops, foreground)),
            ColorOp::PopClip | ColorOp::PopLayer => return,
            ColorOp::Fill { transform, path, fill } => {
                let brush: crate::vector::Brush = match fill {
                    ColorFill::Solid(c) => peniko::Color::new(*c).into(),
                    ColorFill::Foreground(a) => peniko::Color::new([foreground[0], foreground[1], foreground[2], foreground[3] * a]).into(),
                    ColorFill::Gradient(g) => g.clone().into(),
                };
                b.fill(at * *transform, brush, path.clone());
            }
        }
    }
}

/// Outline colours of selected and hovered leaves, shared with the overlay.
pub const LIT_SELECTED: [f32; 4] = [0.1, 0.4, 1.0, 1.0];
pub const LIT_HOVERED: [f32; 4] = [0.1, 0.4, 1.0, 0.5];


