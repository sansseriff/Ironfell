//! The document's 2D content as one display-list layer, in painter order.
//!
//! One layer for all 2D nodes is the batched tier of
//! `plans/document-spine.md` §10: many small primitives share one DisplayList
//! entity and the layer rebuilds when any of them changed. Finer chunking is
//! the raster cache's job later (doc 07 §4.2).
//!
//! What triggers a rebuild is the reconciler's changed-node set
//! ([`DocumentChanges`]), not a boolean: the layer knows which nodes moved,
//! keeps every node's document-space bounds ([`Bounds2d`]), and reports the
//! nodes and the damage rectangle it redrew for. That is the plumbing
//! roadmap stage H turns into culling and damage-driven invalidation; today
//! the whole layer is still re-encoded.
//!
//! Each 2D composition panel draws its own composition's nodes, offset to
//! and clipped by its rectangle (ECM §7.1); a node outside every composition
//! draws in the viewer panel. An embed of a 2D composition draws that
//! composition in place, through the embed's transform and clipped to its
//! region, recursively (ECM §7.4).

use super::{Bounds2d, Doc2d, DocumentChanges, DocumentStore, Highlights, NodeMap, PanelViews, Shape2d, nest};
use iron_document::composition::Dims;
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, VectorView, order};
use bevy::prelude::*;
use iron_document::NodeId;
use kurbo::Shape as _;

/// One document layer per 2D panel, each its own [`VectorView`]: it renders
/// into a texture the panel's size and re-renders only when its own content
/// changed (plans/view-textures.md).
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

/// How many changed node ids a redraw line names before summarising.
const LOG_IDS: usize = 12;

pub fn render_document_layer(
    mut panel_layers: Query<(&mut DisplayList, &DocumentPanelLayer), Without<super::planes::PlaneLayer>>,
    mut plane_layers: Query<(&mut DisplayList, &super::planes::PlaneLayer, &VectorView), Without<DocumentPanelLayer>>,
    new_panels: Query<(), Or<(Added<DocumentPanelLayer>, Changed<super::planes::PlaneLayer>)>>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    mut changes: ResMut<DocumentChanges>,
    mut bounds: ResMut<Bounds2d>,
    views: Res<PanelViews>,
    highlights: Res<Highlights>,
    embed_images: Res<super::EmbedImages>,
) {
    let reason = if !changes.is_empty() {
        "document"
    } else if views.is_changed() {
        "panel layout"
    } else if highlights.is_changed() {
        "selection"
    } else if !new_panels.is_empty() {
        "new view"
    } else if embed_images.is_changed() {
        "embedded view"
    } else {
        return;
    };
    let _timing = crate::perf::span(crate::perf::Phase::DisplayList);
    let changed = changes.take();
    let doc = store.0.document();

    // Damage: the old and new bounds of every changed node (what a box tree's
    // commit would report), plus the bounds of changed nodes that vanished.
    // Bounds are in each node's own composition space, so for content seen
    // through an embed the damage is approximate until stage H.
    let mut damage: Option<kurbo::Rect> = None;
    for id in &changed.nodes {
        if let Some(old) = bounds.0.get(id) {
            damage = Some(damage.map_or(*old, |d| d.union(*old)));
        }
        if doc.live(*id).is_none() {
            bounds.0.remove(id);
        }
    }

    let mut cx = Draw { store: &store.0, embed_images: &embed_images, doc, map: &map, nodes: &nodes, globals: &globals, highlights: &highlights, changed: &changed.nodes, bounds: &mut bounds.0, damage, drawn: 0 };
    // A layer per panel: a panel whose content is unchanged rebuilds to the
    // same commands, so its view does not re-render.
    for (mut panel_list, layer) in &mut panel_layers {
        let Some(view) = views.get(&layer.panel) else { continue };
        let roots = nest::panel_roots(doc, view);
        panel_list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, view.rect.to_kurbo(), |b| cx.walk(b, &roots, view.affine(), &view.panel, 0));
        });
    }
    // A plane's view: its composition's region, at its density, into its
    // own texture (`planes`).
    for (mut plane_list, layer, vview) in &mut plane_layers {
        let roots: Vec<NodeId> = doc.children(layer.source).to_vec();
        let key = format!("plane/{}", layer.embed);
        let clip = kurbo::Rect::new(0.0, 0.0, vview.size.x as f64, vview.size.y as f64);
        plane_list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, clip, |b| cx.walk(b, &roots, layer.affine(), &key, 0));
        });
    }
    let (drawn, damage) = (cx.drawn, cx.damage);
    crate::perf::set_drawn(drawn as u32);

    let mut ids: Vec<String> = changed.nodes.iter().take(LOG_IDS).map(|id| id.to_string()).collect();
    if changed.nodes.len() > LOG_IDS {
        ids.push(format!("+{} more", changed.nodes.len() - LOG_IDS));
    }
    let damage = damage.map(|r| format!("{:.0},{:.0}–{:.0},{:.0}", r.x0, r.y0, r.x1, r.y1)).unwrap_or_else(|| "none".into());
    info!(
        "redraw ({reason}{}): {} of {drawn} nodes changed [{}]; damage {damage}",
        if changed.structure { ", structure" } else { "" },
        changed.nodes.len(),
        ids.join(" "),
    );
}

/// One rebuild's walk: draws a composition's nodes in painter order and
/// recurses into 2D embeds through their placement, clipped to their region.
struct Draw<'a, 'w, 's> {
    store: &'a iron_document::Store,
    embed_images: &'a super::EmbedImages,
    doc: &'a iron_document::Document,
    map: &'a NodeMap,
    nodes: &'a Query<'w, 's, (&'static Doc2d, &'static GlobalTransform)>,
    globals: &'a Query<'w, 's, &'static GlobalTransform>,
    highlights: &'a Highlights,
    changed: &'a std::collections::BTreeSet<NodeId>,
    bounds: &'a mut bevy::platform::collections::HashMap<NodeId, kurbo::Rect>,
    damage: Option<kurbo::Rect>,
    drawn: usize,
}

impl Draw<'_, '_, '_> {
    fn walk(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, roots: &[NodeId], base: kurbo::Affine, key: &str, depth: u32) {
        for id in nest::preorder(self.doc, roots) {
            if nest::is_embed(self.doc, id) {
                if depth >= nest::MAX_DEPTH {
                    continue;
                }
                let key = format!("{key}/{id}");
                if let Some(e) = nest::embed_2d(self.doc, self.map, self.globals, self.nodes, id) {
                    let inner = base * e.placement;
                    let kids: Vec<NodeId> = self.doc.children(e.source).to_vec();
                    b.clipped(inner, e.region, |b| self.walk(b, &kids, inner, &key, depth + 1));
                } else if let Some((_, _, w, h)) = nest::embed_3d(self.store, id) {
                    // The 3D view: its camera's texture (`EmbedImages`), drawn
                    // in painter order under the embed's whole transform, so
                    // it rotates, scales and clips like the 2D around it.
                    let placement = self.map.entity(id).and_then(|e| self.globals.get(e).ok()).map(affine_of).unwrap_or(kurbo::Affine::IDENTITY);
                    if let Some(img) = self.embed_images.0.get(&key) {
                        let to_embed = kurbo::Affine::scale_non_uniform(w / img.size.x.max(1) as f64, h / img.size.y.max(1) as f64);
                        b.texture(base * placement * to_embed, img.image.clone(), img.size, img.version);
                    }
                    let frame = kurbo::Rect::new(0.0, 0.0, w, h);
                    let lit = self.highlights.selected.contains(&id) || self.highlights.hovered.contains(&id);
                    let c = if lit { [0.1, 0.4, 1.0, 1.0] } else { [0.55, 0.57, 0.62, 1.0] };
                    b.stroke(base * placement, kurbo::Stroke::new(1.0), peniko::Color::new(c), frame);
                }
                continue;
            }
            self.node(b, id, base);
        }
    }

    fn node(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, id: NodeId, base: kurbo::Affine) {
        let Some(e) = self.map.entity(id) else { return };
        let Ok((d, global)) = self.nodes.get(e) else { return };
        self.drawn += 1;
        // Node-local placement; `base` adds the panel offset and any embeds
        // only at emit time, so bounds stay in the node's composition space.
        let local = affine_of(global);
        let affine = base * local;
        let shape = nest::shape_path(&d.shape);
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
            // An open path has no inside to fill.
            let open = matches!(&d.shape, Shape2d::Path { closed: false, .. });
            if let Some(c) = d.fill.filter(|_| !open) {
                b.fill(affine, peniko::Color::new(c), shape.clone());
            }
            if let Some((c, w)) = d.stroke {
                b.stroke(affine, kurbo::Stroke::new(w as f64), peniko::Color::new(c), shape.clone());
            }
        }
        let selected = self.highlights.selected.contains(&id);
        let hovered = self.highlights.hovered.contains(&id);
        if selected || hovered {
            let c = if selected { [0.1, 0.4, 1.0, 1.0] } else { [0.1, 0.4, 1.0, 0.5] };
            b.stroke(affine, kurbo::Stroke::new(2.0), peniko::Color::new(c), shape.clone());
        }
        // Bounds in the composition's space, widened by the thickest stroke
        // that could touch this node (its own, or the 2px highlight).
        let pad = d.stroke.map_or(0.0, |(_, w)| w as f64 * 0.5).max(1.0);
        let rect = local.transform_rect_bbox(shape.bounding_box()).inflate(pad, pad);
        if self.changed.contains(&id) {
            self.damage = Some(self.damage.map_or(rect, |d| d.union(rect)));
        }
        self.bounds.insert(id, rect);
    }
}

/// The 2D part of a global transform as a kurbo affine.
pub fn affine_of(global: &GlobalTransform) -> kurbo::Affine {
    nest::affine_of(global)
}
