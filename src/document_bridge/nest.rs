//! Embedded compositions in the bridge (ECM §7.4): where an embed puts the
//! composition it shows, and which part of it shows.
//!
//! Every materialised 2D node's `GlobalTransform` is in its own
//! composition's space (compositions are never materialised, so their
//! children are roots). An embed adds one more affine: from the embedded
//! composition's space into its parent's. Drawing and picking walk the tree
//! of a panel's composition and, at each embed, recurse into the embedded
//! one through that affine, clipped to its region.

use super::{Doc2d, NodeMap, PanelView, Shape2d};
use bevy::prelude::*;
use iron_document::component::Frame;
use iron_document::composition::{self, Dims};
use iron_document::{Document, NodeId, TypeId};
use kurbo::Shape as _;

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
    match *shape {
        Shape2d::Rect { w, h } | Shape2d::Slider { w, h, .. } => kurbo::Rect::new(0.0, 0.0, w as f64, h as f64).into_path(0.1),
        Shape2d::Circle { r } => kurbo::Circle::new((0.0, 0.0), r as f64).into_path(0.1),
    }
}

/// The 2D part of a global transform as a kurbo affine.
pub fn affine_of(global: &GlobalTransform) -> kurbo::Affine {
    let a = global.affine();
    kurbo::Affine::new([
        a.matrix3.x_axis.x as f64,
        a.matrix3.x_axis.y as f64,
        a.matrix3.y_axis.x as f64,
        a.matrix3.y_axis.y as f64,
        a.translation.x as f64,
        a.translation.y as f64,
    ])
}

/// The roots one composition panel walks: the composition's children, and
/// for the viewer, 2D nodes outside every composition.
pub fn panel_roots(doc: &Document, view: &PanelView) -> Vec<NodeId> {
    let mut roots: Vec<NodeId> = view.composition.map(|c| doc.children(c).to_vec()).unwrap_or_default();
    if view.loose {
        let structural = ["composition", "model", "library", "timeline"].map(|t| TypeId::lookup(t).expect("registered"));
        roots.extend(doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| !structural.contains(&n.ty))));
    }
    roots
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

/// The union of the drawn bounds of a composition's content, in its space,
/// computed from shapes (not from what was last drawn, so an embed shows on
/// its first frame).
pub fn content_bounds(doc: &Document, map: &NodeMap, nodes: &Query<(&Doc2d, &GlobalTransform)>, composition: NodeId) -> Option<kurbo::Rect> {
    preorder(doc, doc.children(composition))
        .into_iter()
        .filter_map(|id| map.entity(id).and_then(|e| nodes.get(e).ok()))
        .map(|(d, g)| affine_of(g).transform_rect_bbox(shape_path(&d.shape).bounding_box()))
        .reduce(|a, b| a.union(b))
}

/// What a 2D embed shows, if it embeds a live 2D composition: placement
/// from the embed's transform, region from its `frame` or the content.
pub fn embed_2d(
    doc: &Document,
    map: &NodeMap,
    globals: &Query<&GlobalTransform>,
    nodes: &Query<(&Doc2d, &GlobalTransform)>,
    embed: NodeId,
) -> Option<Embed2d> {
    let source = composition::embedded(doc, embed)?;
    if composition::dims(doc, source) != Some(Dims::Two) {
        return None;
    }
    let placement = map.entity(embed).and_then(|e| globals.get(e).ok()).map(affine_of).unwrap_or(kurbo::Affine::IDENTITY);
    let region = match doc.get::<Frame>(embed) {
        Some(f) => {
            let n = |s: &iron_document::Slot| s.constant().and_then(|v| v.as_f64()).unwrap_or(0.0);
            kurbo::Rect::new(n(&f.x), n(&f.y), n(&f.x) + n(&f.w), n(&f.y) + n(&f.h))
        }
        None => content_bounds(doc, map, nodes, source)?,
    };
    Some(Embed2d { source, placement, region })
}

pub fn is_embed(doc: &Document, id: NodeId) -> bool {
    doc.live(id).is_some_and(|n| n.ty.name() == "embed")
}
