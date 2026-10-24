//! Embedded compositions in the bridge (ECM §7.4): where an embed puts the
//! composition it shows, and which part of it shows.
//!
//! Every 2D node's world affine is in its own composition's space
//! (compositions are never placed, so their children are roots); the store
//! computes it and the spatial scene caches it (`world_2d`). An embed adds
//! one more affine: from the embedded composition's space into its parent's.
//! Drawing and picking walk the tree of a panel's composition and, at each
//! embed, recurse into the embedded one through that affine, clipped to its
//! region.

use super::{Doc2d, PanelView, Shape2d};
use bevy::prelude::*;
use iron_document::facet::Frame;
use iron_document::composition::{self, Dims};
use iron_document::{Document, NodeId, TypeId};

/// Deepest nesting drawn or picked. Loops are refused by the store; this
/// only bounds the cost of a legitimately deep stack.
pub const MAX_DEPTH: u32 = 8;

/// A 2D composition shown by an embed: `placement` maps its space into the
/// parent's, and `region` (in its own space) is what shows, clipped.
#[derive(Clone, Copy, Debug)]
pub struct Embed2d {
    pub source: NodeId,
    pub placement: kurbo::Affine,
    pub region: kurbo::Rect,
}

/// A node's shape as a path in its own local space.
pub fn shape_path(shape: &Shape2d) -> kurbo::BezPath {
    match draw_shape(shape) {
        crate::vector::Shape::Path(p) => (*p.0).clone(),
        other => other.to_path(),
    }
}

/// A rect's rounded outline, its radii clamped as Figma draws them.
pub fn rounded_rect(w: f32, h: f32, radii: [f32; 4]) -> kurbo::RoundedRect {
    let [tl, tr, br, bl] = iron_document::shape::clamp_radii(w as f64, h as f64, radii.map(f64::from));
    kurbo::RoundedRect::from_rect(kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), kurbo::RoundedRectRadii::new(tl, tr, br, bl))
}

/// A node's shape as the display list's typed shape, so a backend can take
/// its fast path for a plain or rounded rect.
pub fn draw_shape(shape: &Shape2d) -> crate::vector::Shape {
    use crate::vector::Shape;
    match shape {
        &Shape2d::Rect { w, h, radii } if radii.iter().any(|&r| r > 0.0) => Shape::RoundedRect(rounded_rect(w, h, radii)),
        &Shape2d::Rect { w, h, .. } | &Shape2d::Slider { w, h, .. } | &Shape2d::Text { w, h, .. } => Shape::Rect(kurbo::Rect::new(0.0, 0.0, w as f64, h as f64)),
        &Shape2d::Ellipse { rx, ry, arc } => {
            let arc = arc.map(|a| a.map(f64::from));
            iron_document::shape::to_bez(&iron_document::shape::ellipse_outline(rx as f64, ry as f64, arc)).into()
        }
        // The packed path's cached outline itself (packed-paths.md §2.1).
        Shape2d::Path { outline, .. } => outline.clone().into(),
    }
}

/// The roots one composition panel walks: the composition's children, and
/// for the viewer, 2D nodes outside every composition.
pub fn panel_roots(doc: &Document, view: &PanelView) -> Vec<NodeId> {
    let mut roots: Vec<NodeId> = view.composition.map(|c| doc.children(c).to_vec()).unwrap_or_default();
    if view.loose {
        roots.extend(loose_roots(doc));
    }
    roots
}

/// 2D nodes outside every composition: what the viewer draws besides its
/// composition.
pub fn loose_roots(doc: &Document) -> Vec<NodeId> {
    let structural = ["composition", "model", "library", "timeline", "session"].map(|t| TypeId::lookup(t).expect("registered"));
    doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| !structural.contains(&n.ty))).collect()
}

/// Where a changed node draws, for deciding which panels rebuild
/// (plans/specs/render-cache.md §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Home {
    /// Inside this composition.
    Composition(NodeId),
    /// A 2D node outside every composition: the viewer draws it.
    Loose,
    /// No record of it: every panel rebuilds.
    Unknown,
}

/// The composition `id` belongs to, deleted nodes included (a tombstone
/// keeps its parent, so a deletion still finds the panels that drew it).
pub fn home(doc: &Document, id: NodeId) -> Home {
    let composition = TypeId::lookup("composition").expect("registered");
    let mut cur = doc.node(id);
    if cur.is_none() {
        return Home::Unknown;
    }
    while let Some(n) = cur {
        if n.ty == composition {
            return Home::Composition(n.id);
        }
        cur = n.parent.and_then(|p| doc.node(p));
    }
    Home::Loose
}

/// The compositions a panel showing `root` draws: `root` and whatever its
/// embeds show, through embeds inside those, to any depth.
pub fn shown_compositions(doc: &Document, root: Option<NodeId>) -> std::collections::BTreeSet<NodeId> {
    let embed = TypeId::lookup("embed").expect("registered");
    let edges: Vec<(NodeId, NodeId)> = doc
        .nodes_of_type(embed)
        .filter(|&e| doc.is_live(e))
        .filter_map(|e| Some((composition::composition_of(doc, e)?, composition::embedded(doc, e)?)))
        .collect();
    let mut shown = std::collections::BTreeSet::new();
    let mut stack: Vec<NodeId> = root.into_iter().collect();
    while let Some(c) = stack.pop() {
        if shown.insert(c) {
            stack.extend(edges.iter().filter(|(host, _)| *host == c).map(|(_, source)| *source));
        }
    }
    shown
}

/// The embeds at or under `roots`, in painter order. Found from the
/// document's type index, not by walking the content: this runs every
/// frame, and the walk cost ~8 ms a frame on a 42k-node import
/// (render-cache.md §2).
pub fn embeds_under(doc: &Document, roots: &[NodeId]) -> Vec<NodeId> {
    let embed = TypeId::lookup("embed").expect("registered");
    let under = |e: NodeId| {
        let mut at = doc.live(e);
        while let Some(n) = at {
            if let Some(i) = roots.iter().position(|&r| r == n.id) {
                return Some(i);
            }
            at = n.parent.and_then(|p| doc.live(p));
        }
        None
    };
    // In `roots`' order, then preorder within each.
    let mut found: Vec<(usize, Vec<&iron_document::OrderKey>, NodeId)> = doc.nodes_of_type(embed).filter_map(|e| Some((under(e)?, doc.order_path(e), e))).collect();
    found.sort();
    found.into_iter().map(|(.., e)| e).collect()
}

/// Preorder of `roots` and their descendants: painter order.
pub fn preorder(doc: &Document, roots: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = roots.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        out.push(id);
        stack.extend(doc.children(id).iter().rev().copied());
    }
    out
}

/// Conservative local paint outsets, shared by spatial indexing and automatic
/// embed frames. Geometry alone trims centred strokes at the frame boundary.
pub fn paint_padding(d: &Doc2d) -> f64 {
    // The miter limit also covers round/bevel joins and square/round caps.
    let mut pad = d.stroke.as_ref().map_or(0.0, |(_, st)| st.width * 0.5 * st.miter_limit.max(4.0));
    if let Some(s) = d.shadow {
        pad += (s.dx.abs().max(s.dy.abs()) + 3.0 * s.std_dev) as f64;
    }
    if let Shape2d::Slider { h, .. } = d.shape {
        pad = pad.max((h as f64 * 0.5).min(12.0) + 3.0);
    }
    pad
}

/// A 2D node's local space to its composition's: the spatial scene's cached
/// placement for anything it indexes (every drawable node, group with
/// content and embed), the store's walk for the rest. Both read resolved
/// values, so previews, bindings and clips are honoured.
pub fn world_2d(store: &iron_document::Store, spatial: &super::SpatialScene, id: NodeId) -> kurbo::Affine {
    spatial.world_affine(id).unwrap_or_else(|| kurbo::Affine::new(store.world_affine(id).0))
}

/// The union of the painted bounds of a composition's content, in its space
/// (the scene's, kept current even when no view draws the composition, so an
/// embed shows on its first frame).
pub fn content_bounds(spatial: &super::SpatialScene, composition: NodeId) -> Option<kurbo::Rect> {
    spatial.content_bounds(composition)
}

/// What a 2D embed shows, if it embeds a live 2D composition: placement
/// from the embed's transform, region from its `frame` or the content.
pub fn embed_2d(store: &iron_document::Store, spatial: &super::SpatialScene, embed: NodeId) -> Option<Embed2d> {
    let doc = store.document();
    let source = composition::embedded(doc, embed)?;
    if composition::dims(doc, source) != Some(Dims::Two) {
        return None;
    }
    let placement = world_2d(store, spatial, embed);
    let region = match doc.get::<Frame>(embed) {
        Some(f) => {
            let n = |s: &iron_document::Slot| s.constant().and_then(|v| v.as_f64()).unwrap_or(0.0);
            kurbo::Rect::new(n(&f.x0), n(&f.y0), n(&f.x1), n(&f.y1))
        }
        None => content_bounds(spatial, source)?,
    };
    Some(Embed2d { source, placement, region })
}

/// `id` and its ancestors inside its composition, outermost first: the part
/// of a selection chain (`bevy_app::select`) one composition contributes.
/// Stops below the composition root, or at a loose root.
pub fn lineage(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    let template = TypeId::lookup("template").expect("registered");
    let mut out = vec![id];
    let mut cur = doc.live(id).and_then(|n| n.parent);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        // A definition's template is a root like a composition: what a
        // reference shows (instance-cache.md phase D).
        if n.ty == composition || n.ty == template {
            break;
        }
        out.push(n.id);
        cur = n.parent;
    }
    out.reverse();
    out
}

/// The template a reference shows (`iron_document::expand::reference_template`):
/// a repeated piece of an imported drawing, drawn from its definition.
pub fn reference(doc: &Document, id: NodeId) -> Option<NodeId> {
    iron_document::expand::reference_template(doc, id)
}

pub fn is_embed(doc: &Document, id: NodeId) -> bool {
    doc.live(id).is_some_and(|n| n.ty.name() == "embed")
}

pub use iron_document::composition::EMBED_3D_SIZE;

/// A 3D composition seen from a 2D one (ECM §7.4), resolved for one frame:
/// which composition, through which camera node (none: the derived default
/// view), at what texture size, and how window pixels map onto it. The
/// camera renders the embed's whole shape into its texture; the 2D view
/// draws that texture under the embed's transform, clipped like any other
/// content (plans/specs/view-textures.md, migration step 3).
#[derive(Clone, Debug, PartialEq)]
pub struct EmbedView {
    /// Panel id plus the chain of embeds that reaches it: stable while the
    /// layout is, so its Bevy camera survives from frame to frame.
    pub key: String,
    pub panel: String,
    /// Texture px: the embed's size at its on-screen scale.
    pub size: UVec2,
    /// Window px → texture px (through the panel, enclosing embeds, and this
    /// embed's own transform, which may rotate or scale).
    pub to_texture: kurbo::Affine,
    /// The visible part, window px: the embed's bounds clipped to the panel
    /// and to enclosing embeds.
    pub clip: kurbo::Rect,
    pub source: NodeId,
    pub camera: Option<NodeId>,
    pub layer: usize,
    /// No `on_demand` composition between the panel and this embed's
    /// source, inclusive (free-running.md §2): it may free-run.
    pub open: bool,
}

impl EmbedView {
    /// A window point on this view, in its texture's px, if it shows there.
    pub fn texture_point(&self, screen: kurbo::Point) -> Option<kurbo::Point> {
        if !self.clip.contains(screen) {
            return None;
        }
        let q = self.to_texture * screen;
        (q.x >= 0.0 && q.y >= 0.0 && q.x <= self.size.x as f64 && q.y <= self.size.y as f64).then_some(q)
    }
}

/// Every 3D embed reachable from the 2D composition panels, recomputed each
/// frame after transforms propagate. Their cameras are kept by
/// `bevy_app::scene3d::sync_embed_cameras`; the 2D layer draws their
/// textures (`EmbedImages`).
#[derive(Resource, Default, Debug, PartialEq)]
pub struct EmbedViews(pub Vec<EmbedView>);

/// The on-screen size of a 3D embed in its parent's space, and the camera
/// it names (`composition::embed_3d_extent`).
pub fn embed_3d(store: &iron_document::Store, embed: NodeId) -> Option<(NodeId, Option<NodeId>, f64, f64)> {
    composition::embed_3d_extent(store, embed)
}

pub fn collect_embed_views(
    store: Res<super::DocumentStore>,
    views: Res<super::PanelViews>,
    spatial: Res<super::SpatialScene>,
    mut layers: ResMut<super::CompositionLayers>,
    mut out: ResMut<EmbedViews>,
) {
    let doc = store.0.document();
    let mut found = Vec::new();
    for view in views.0.iter().filter(|v| v.dims == Dims::Two) {
        let roots = panel_roots(doc, view);
        let open = view.composition.is_none_or(|c| composition::run_mode(doc, c) != composition::RunMode::OnDemand);
        collect(&store.0, &spatial, &mut layers, view, &roots, view.affine(), view.rect.to_kurbo(), &view.panel, 0, open, &mut found);
    }
    if out.0 != found {
        out.0 = found;
    }
}

#[allow(clippy::too_many_arguments)]
fn collect(
    store: &iron_document::Store,
    spatial: &super::SpatialScene,
    layers: &mut super::CompositionLayers,
    view: &PanelView,
    roots: &[NodeId],
    base: kurbo::Affine,
    clip: kurbo::Rect,
    key: &str,
    depth: u32,
    open: bool,
    found: &mut Vec<EmbedView>,
) {
    let doc = store.document();
    let open_through = |c: NodeId| open && composition::run_mode(doc, c) != composition::RunMode::OnDemand;
    for id in embeds_under(doc, roots) {
        if depth >= MAX_DEPTH {
            continue;
        }
        let placement = world_2d(store, spatial, id);
        let key = format!("{key}/{id}");
        if let Some(e) = embed_2d(store, spatial, id) {
            let inner = base * e.placement;
            let clip = clip.intersect(inner.transform_rect_bbox(e.region));
            let kids: Vec<NodeId> = doc.children(e.source).to_vec();
            collect(store, spatial, layers, view, &kids, inner, clip, &key, depth + 1, open_through(e.source), found);
        } else if let Some((source, camera, w, h)) = embed_3d(store, id) {
            let to_window = base * placement;
            let visible = to_window.transform_rect_bbox(kurbo::Rect::new(0.0, 0.0, w, h)).intersect(clip);
            if visible.width() < 1.0 || visible.height() < 1.0 {
                continue;
            }
            // Texture px follow the embed's on-screen scale, so it stays
            // sharp at any zoom; the whole embed renders, the clip crops.
            let s = to_window.determinant().abs().sqrt().max(1e-6);
            let size = UVec2::new(((w * s).ceil() as u32).clamp(1, 4096), ((h * s).ceil() as u32).clamp(1, 4096));
            let to_texture = kurbo::Affine::scale_non_uniform(size.x as f64 / w, size.y as f64 / h) * to_window.inverse();
            found.push(EmbedView { key, panel: view.panel.clone(), size, to_texture, clip: visible, source, camera, layer: layers.of(Some(source)), open: open_through(source) });
        }
    }
}

/// Where `node` shows in a 2D panel, in the panel's document space: its
/// bounds if it is drawn there directly, or through embeds (a 2D embed maps
/// its bounds out; a 3D embed contributes its whole rectangle). The first
/// appearance in painter order wins. For "Show in" (ECM §7.2).
#[allow(clippy::too_many_arguments)]
pub fn locate(store: &iron_document::Store, spatial: &super::SpatialScene, view: &PanelView, node: NodeId) -> Option<kurbo::Rect> {
    let doc = store.document();
    let target = composition::composition_of(doc, node);
    // A group's entry is already its subtree's union.
    let own = |n: NodeId| -> Option<kurbo::Rect> { spatial.painted(n) };
    if target == view.composition || (target.is_none() && view.loose) {
        return own(node);
    }
    fn walk(
        store: &iron_document::Store,
        spatial: &super::SpatialScene,
        roots: &[NodeId],
        base: kurbo::Affine,
        target: NodeId,
        own: &dyn Fn(NodeId) -> Option<kurbo::Rect>,
        node: NodeId,
        depth: u32,
    ) -> Option<kurbo::Rect> {
        let doc = store.document();
        for id in preorder(doc, roots) {
            if !is_embed(doc, id) || depth >= MAX_DEPTH {
                continue;
            }
            if let Some(e) = embed_2d(store, spatial, id) {
                let inner = base * e.placement;
                if e.source == target {
                    return own(node).map(|r| inner.transform_rect_bbox(r));
                }
                let kids: Vec<NodeId> = doc.children(e.source).to_vec();
                if let Some(r) = walk(store, spatial, &kids, inner, target, own, node, depth + 1) {
                    return Some(r);
                }
            } else if let Some((source, _, w, h)) = embed_3d(store, id)
                && source == target
            {
                let placement = world_2d(store, spatial, id);
                return Some((base * placement).transform_rect_bbox(kurbo::Rect::new(0.0, 0.0, w, h)));
            }
        }
        None
    }
    let roots = panel_roots(doc, view);
    walk(store, spatial, &roots, kurbo::Affine::IDENTITY, target?, &own, node, 0)
}

/// One 3D embed's rendered texture, as the 2D layer draws it: the image, its
/// size in texels, and a version that changes whenever the image's content
/// does (so the 2D view that draws it re-renders).
#[derive(Clone, Debug, PartialEq)]
pub struct EmbedImage {
    pub image: Handle<Image>,
    pub size: UVec2,
    pub version: u32,
}

/// Every 3D embed's texture, by `EmbedView::key`. Written by
/// `bevy_app::scene3d::sync_view_images`, read by the 2D layer.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct EmbedImages(pub std::collections::BTreeMap<String, EmbedImage>);
