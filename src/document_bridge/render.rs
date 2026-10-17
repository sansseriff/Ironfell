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

use super::{SpatialScene, Doc2d, DocumentChanges, DocumentStore, Highlights, NodeMap, PanelViews, Shape2d, nest};
use iron_document::composition::Dims;
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, VectorView, order};
use bevy::prelude::*;
use iron_document::NodeId;

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

/// How many changed node ids a redraw line names before summarising.
const LOG_IDS: usize = 12;

pub fn render_document_layer(
    mut panel_layers: Query<(&mut DisplayList, &DocumentPanelLayer), Without<super::planes::PlaneLayer>>,
    mut plane_layers: Query<(&mut DisplayList, &super::planes::PlaneLayer, &VectorView), Without<DocumentPanelLayer>>,
    new_panels: Query<(), Or<(Added<DocumentPanelLayer>, Changed<super::planes::PlaneLayer>)>>,
    nodes: Query<&'static Doc2d>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
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
    } else if views.is_changed() {
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
    let mut cx = Draw { store: &store.0, embed_images: &embed_images, doc, map: &map, nodes: &nodes, highlights: None, spatial: &spatial, drawn: 0 };
    // A layer per panel: a panel whose content is unchanged rebuilds to the
    // same commands, so its view does not re-render.
    for (mut panel_list, layer) in panel_layers.iter_mut().filter(|_| !planes_only) {
        let Some(view) = views.get(&layer.panel) else { continue };
        let roots = nest::panel_roots(doc, view);
        panel_list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, view.rect.to_kurbo(), |b| cx.walk(b, &roots, view.affine(), view.rect.to_kurbo(), &view.panel, 0));
        });
    }
    // A plane's view: its composition's region, at its density, into its
    // own texture (`planes`), lit nodes outlined.
    cx.highlights = Some(&highlights);
    for (mut plane_list, layer, vview) in &mut plane_layers {
        let roots: Vec<NodeId> = doc.children(layer.source).to_vec();
        let key = format!("plane/{}", layer.embed);
        let clip = kurbo::Rect::new(0.0, 0.0, vview.size.x as f64, vview.size.y as f64);
        plane_list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, clip, |b| cx.walk(b, &roots, layer.affine(), clip, &key, 0));
        });
    }
    let drawn = cx.drawn;
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
    nodes: &'a Query<'w, 's, &'static Doc2d>,
    /// Set for plane views only; panels leave feedback to the overlay.
    highlights: Option<&'a Highlights>,
    spatial: &'a SpatialScene,
    drawn: usize,
}

impl Draw<'_, '_, '_> {
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
        self.branches(b, roots, base, clip, key, depth, &allowed);
    }

    fn number(&self, id: NodeId, leaf: &str, fallback: f64) -> f64 {
        self.store.resolved(id, leaf.parse().expect("declared leaf")).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(fallback)
    }

    fn branches(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, roots: &[NodeId], base: kurbo::Affine, clip: kurbo::Rect, key: &str, depth: u32, allowed: &std::collections::BTreeSet<NodeId>) {
        for &id in roots {
            if !allowed.contains(&id) { continue; }
            let opacity = self.number(id, "appearance.opacity", 1.0).clamp(0.0, 1.0) as f32;
            let frame = iron_document::appearance::ClipFrame::read(self.store, id);
            b.opacity(opacity, clip, |b| {
                if let Some(frame) = frame {
                    let (w, h, r) = (frame.w, frame.h, frame.radius);
                    let rect = kurbo::Rect::new(0.0, 0.0, w, h);
                    let shape = if frame.ellipse {
                        use kurbo::Shape;
                        crate::vector::Shape::Path(kurbo::Ellipse::new((w/2.0, h/2.0), (w/2.0, h/2.0), 0.0).to_path(0.1))
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

    fn content(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, id: NodeId, base: kurbo::Affine, clip: kurbo::Rect, key: &str, depth: u32, allowed: &std::collections::BTreeSet<NodeId>) {
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

    /// A node's placement in its composition's space (`nest::world_2d`).
    fn world(&self, id: NodeId) -> kurbo::Affine {
        nest::world_2d(self.store, self.spatial, id)
    }

    fn node(&mut self, b: &mut crate::vector::DisplayListBuilder<'_>, id: NodeId, base: kurbo::Affine) {
        let Some(e) = self.map.entity(id) else { return };
        let Ok(d) = self.nodes.get(e) else { return };
        self.drawn += 1;
        // Node-local placement; `base` adds the panel offset and any embeds
        // only at emit time, so bounds stay in the node's composition space.
        let local = self.world(id);
        let affine = base * local;
        let shape = nest::draw_shape(&d.shape);
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
                if let Some(brush) = self.fill_brush(id, d, &shape) {
                    match &d.shape {
                        Shape2d::Path { fill: Some(f), even_odd, .. } => {
                            let rule = if *even_odd { peniko::Fill::EvenOdd } else { peniko::Fill::NonZero };
                            b.fill_with_rule(affine, brush, rule, crate::vector::Shape::Path((**f).clone()));
                        }
                        Shape2d::Path { fill: None, .. } => {}
                        _ => b.fill(affine, brush, shape.clone()),
                    }
                }
                if let Some((c, style)) = &d.stroke {
                    b.stroke(affine, style.clone(), peniko::Color::new(*c), shape.clone());
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
                    let analytic = match &shape {
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
                            let own = in_parent.transform_rect_bbox(shape.bounding_box().inflate(pad, pad));
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

/// Outline colours of selected and hovered leaves, shared with the overlay.
pub const LIT_SELECTED: [f32; 4] = [0.1, 0.4, 1.0, 1.0];
pub const LIT_HOVERED: [f32; 4] = [0.1, 0.4, 1.0, 0.5];


