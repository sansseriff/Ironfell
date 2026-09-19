//! The document's 2D content as one display-list layer, in painter order.
//!
//! One layer for all 2D nodes is the batched tier of
//! `plans/document-spine.md` §10: many small primitives share one DisplayList
//! entity and the layer rebuilds when any of them changed. Finer chunking is
//! the raster cache's job later (doc 07 §4.2).

use super::{Doc2d, DocumentDirty, DocumentStore, NodeMap, Shape2d};
use crate::panels::{Panels, VIEWER_PANEL, document_affine};
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};
use bevy::prelude::*;
use iron_document::NodeId;

#[derive(Component)]
pub struct DocumentLayer;

pub(super) fn setup_document_layer(mut commands: Commands) {
    commands.spawn((DisplayList::default(), VectorLayer::screen(order::DOCUMENT_2D), DocumentLayer));
}

pub(super) fn render_document_layer(
    mut layers: Query<&mut DisplayList, With<DocumentLayer>>,
    nodes: Query<(&Doc2d, &GlobalTransform)>,
    moved: Query<Entity, (With<Doc2d>, Changed<GlobalTransform>)>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    mut dirty: ResMut<DocumentDirty>,
    panels: Res<Panels>,
    selection: Res<crate::SelectionState>,
) {
    let Ok(mut list) = layers.single_mut() else { return };
    let needs_rebuild = dirty.0 || panels.is_changed() || selection.is_changed() || !moved.is_empty();
    if !needs_rebuild {
        return;
    }
    dirty.0 = false;

    let Some(panel) = panels.rect(VIEWER_PANEL) else {
        list.rebuild(|_| {});
        return;
    };
    let base = document_affine(panel);
    let doc = store.0.document();

    // Painter order is document preorder, not entity order.
    let mut ordered: Vec<NodeId> = Vec::new();
    let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        ordered.push(id);
        stack.extend(doc.children(id).iter().rev().copied());
    }

    list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, panel.to_kurbo(), |b| {
            for &id in &ordered {
                let Some(e) = map.entity(id) else { continue };
                let Ok((d, global)) = nodes.get(e) else { continue };
                let affine = base * affine_of(global);
                let shape = match d.shape {
                    Shape2d::Rect { w, h } => kurbo::Rect::new(0.0, 0.0, w as f64, h as f64).into_path(0.1),
                    Shape2d::Circle { r } => kurbo::Circle::new((0.0, 0.0), r as f64).into_path(0.1),
                };
                if let Some(c) = d.fill {
                    b.fill(affine, peniko::Color::new(c), shape.clone());
                }
                if let Some((c, w)) = d.stroke {
                    b.stroke(affine, kurbo::Stroke::new(w as f64), peniko::Color::new(c), shape.clone());
                }
                let selected = selection.selected.contains_key(&e);
                let hovered = selection.hovered.contains_key(&e);
                if selected || hovered {
                    let c = if selected { [0.1, 0.4, 1.0, 1.0] } else { [0.1, 0.4, 1.0, 0.5] };
                    b.stroke(affine, kurbo::Stroke::new(2.0), peniko::Color::new(c), shape);
                }
            }
        });
    });
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
