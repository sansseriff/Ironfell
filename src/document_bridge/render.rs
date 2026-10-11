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
//! draws in the viewer panel.

use super::{Bounds2d, Doc2d, DocumentChanges, DocumentStore, InComposition, NodeMap, PanelViews, Shape2d};
use crate::panels::document_affine;
use iron_document::composition::Dims;
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};
use bevy::prelude::*;
use iron_document::NodeId;

#[derive(Component)]
pub struct DocumentLayer;

pub(super) fn setup_document_layer(mut commands: Commands) {
    commands.spawn((DisplayList::default(), VectorLayer::screen(order::DOCUMENT_2D), DocumentLayer));
}

/// How many changed node ids a redraw line names before summarising.
const LOG_IDS: usize = 12;

pub(super) fn render_document_layer(
    mut layers: Query<&mut DisplayList, With<DocumentLayer>>,
    nodes: Query<(&Doc2d, &GlobalTransform, &InComposition)>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    mut changes: ResMut<DocumentChanges>,
    mut bounds: ResMut<Bounds2d>,
    views: Res<PanelViews>,
    selection: Res<crate::SelectionState>,
) {
    let Ok(mut list) = layers.single_mut() else { return };
    let reason = if !changes.is_empty() {
        "document"
    } else if views.is_changed() {
        "panel layout"
    } else if selection.is_changed() {
        "selection"
    } else {
        return;
    };
    let changed = changes.take();

    let panels: Vec<_> = views.0.iter().filter(|v| v.dims == Dims::Two).collect();
    let doc = store.0.document();

    // Painter order is document preorder, not entity order.
    let mut ordered: Vec<NodeId> = Vec::new();
    let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        ordered.push(id);
        stack.extend(doc.children(id).iter().rev().copied());
    }

    // Damage: the old and new bounds of every changed node (what a box tree's
    // commit would report), plus the bounds of changed nodes that vanished.
    let mut damage: Option<kurbo::Rect> = None;
    let mut grow = |r: kurbo::Rect| damage = Some(damage.map_or(r, |d| d.union(r)));
    for id in &changed.nodes {
        if let Some(old) = bounds.0.get(id) {
            grow(*old);
        }
        if doc.live(*id).is_none() {
            bounds.0.remove(id);
        }
    }

    let mut drawn = 0usize;
    list.rebuild(|b| {
        for view in &panels {
            let base = document_affine(view.rect);
            b.clipped(kurbo::Affine::IDENTITY, view.rect.to_kurbo(), |b| {
                for &id in &ordered {
                    let Some(e) = map.entity(id) else { continue };
                    let Ok((d, global, within)) = nodes.get(e) else { continue };
                    if !view.shows(within.0) {
                        continue;
                    }
                    drawn += 1;
                    // Node-local placement; `base` adds the panel offset only at
                    // emit time, so bounds stay in document space.
                    let local = affine_of(global);
                    let affine = base * local;
                    let shape = match d.shape {
                        Shape2d::Rect { w, h } => kurbo::Rect::new(0.0, 0.0, w as f64, h as f64).into_path(0.1),
                        Shape2d::Circle { r } => kurbo::Circle::new((0.0, 0.0), r as f64).into_path(0.1),
                        Shape2d::Slider { w, h, t } => {
                            // Track, filled part, knob. The hit shape stays the
                            // whole w×h box so the knob is easy to grab.
                            let (w, h, t) = (w as f64, h as f64, t as f64);
                            let mid = h * 0.5;
                            let track = kurbo::RoundedRect::new(0.0, mid - 3.0, w, mid + 3.0, 3.0);
                            b.fill(affine, peniko::Color::new([0.75, 0.76, 0.8, 1.0]), track);
                            let filled = kurbo::RoundedRect::new(0.0, mid - 3.0, (w * t).max(6.0), mid + 3.0, 3.0);
                            b.fill(affine, peniko::Color::new(d.fill.unwrap_or([0.2, 0.5, 0.9, 1.0])), filled);
                            let knob = kurbo::Circle::new((w * t, mid), mid.min(12.0));
                            b.fill(affine, peniko::Color::new([1.0, 1.0, 1.0, 1.0]), knob);
                            b.stroke(affine, kurbo::Stroke::new(1.5), peniko::Color::new([0.3, 0.3, 0.35, 1.0]), knob);
                            kurbo::Rect::new(0.0, 0.0, w, h).into_path(0.1)
                        }
                    };
                    if !matches!(d.shape, Shape2d::Slider { .. }) {
                        if let Some(c) = d.fill {
                            b.fill(affine, peniko::Color::new(c), shape.clone());
                        }
                        if let Some((c, w)) = d.stroke {
                            b.stroke(affine, kurbo::Stroke::new(w as f64), peniko::Color::new(c), shape.clone());
                        }
                    }
                    let selected = selection.selected.contains_key(&e);
                    let hovered = selection.hovered.contains_key(&e);
                    if selected || hovered {
                        let c = if selected { [0.1, 0.4, 1.0, 1.0] } else { [0.1, 0.4, 1.0, 0.5] };
                        b.stroke(affine, kurbo::Stroke::new(2.0), peniko::Color::new(c), shape.clone());
                    }
                    // Document-space bounds, widened by the thickest stroke that
                    // could touch this node (its own, or the 2px highlight).
                    let pad = d.stroke.map_or(0.0, |(_, w)| w as f64 * 0.5).max(1.0);
                    let rect = local.transform_rect_bbox(shape.bounding_box()).inflate(pad, pad);
                    if changed.nodes.contains(&id) {
                        grow(rect);
                    }
                    bounds.0.insert(id, rect);
                }
            });
        }
    });

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

use kurbo::Shape as _;
