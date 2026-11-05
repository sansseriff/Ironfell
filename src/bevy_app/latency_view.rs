//! The Latency view's engine tracker (plans/rendering/latency-view.md): a
//! magenta ring drawn by Vello at the engine's pointer position, over every
//! panel showing the Latency session composition. Render-only: never a
//! document node or a pick target, so it measures input → engine → Vello →
//! screen without the gesture and document path the view's yellow square
//! takes. The HTML trackers (Canvas2D, SVG) are in
//! `src-ui/views/html/LatencyView.svelte`; all of them are centred on the
//! pointer, so with the pointer still they are concentric.
use crate::document_bridge::{DocumentStore, PanelViews};
use crate::vector::{DisplayList, DisplayListRebuild, Overlay, VectorLayer, VectorView, order};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// The ring's radius and stroke, in CSS px; the view's legend says the same.
pub const RING_RADIUS: f64 = 34.0;
const RING_STROKE: f64 = 3.0;

#[derive(Component)]
pub(super) struct LatencyFollower(String);

pub(super) fn draw(
    mut commands: Commands,
    views: Res<PanelViews>,
    store: Res<DocumentStore>,
    pointer: Res<crate::PointerState>,
    dpr: Res<crate::Dpr>,
    mut followers: Query<(Entity, &LatencyFollower, &mut DisplayList, &mut VectorView)>,
) {
    let doc = store.0.document();
    let latency = doc.roots().iter().copied().find(|&r| {
        r.is_session()
            && doc.get::<iron_document::facet::Name>(r).and_then(|n| n.text.constant()).and_then(|v| v.as_str())
                == Some(iron_document::standard::LATENCY)
    });
    let panels: HashMap<&str, _> = match latency {
        Some(c) => views.showing(c).map(|v| (v.panel.as_str(), v)).collect(),
        None => HashMap::default(),
    };
    let mut drawn: Vec<&str> = Vec::new();
    for (entity, follower, mut list, mut old_view) in &mut followers {
        let Some(panel) = panels.get(follower.0.as_str()) else {
            commands.entity(entity).despawn();
            continue;
        };
        drawn.push(panel.panel.as_str());
        let view = view_of(panel);
        if *old_view != view {
            *old_view = view;
        }
        let k = dpr.0;
        list.rebuild(|b| {
            if !panel.rect.contains(pointer.screen) {
                return;
            }
            let centre = kurbo::Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
            b.clipped(kurbo::Affine::IDENTITY, panel.rect.to_kurbo(), |b| {
                b.stroke(
                    kurbo::Affine::IDENTITY,
                    kurbo::Stroke::new(RING_STROKE * k),
                    peniko::Color::from_rgba8(255, 0, 170, 255),
                    kurbo::Circle::new(centre, RING_RADIUS * k),
                );
            });
        });
    }
    for (id, panel) in panels {
        if drawn.contains(&id) {
            continue;
        }
        let entity = commands.spawn((LatencyFollower(id.to_owned()), DisplayList::default(), view_of(panel), Overlay)).id();
        commands.entity(entity).insert(VectorLayer::screen(order::LATENCY_FILM).in_view(entity));
    }
}

fn view_of(panel: &crate::document_bridge::PanelView) -> VectorView {
    VectorView {
        pos: UVec2::new(panel.rect.x.max(0.0) as u32, panel.rect.y.max(0.0) as u32),
        size: UVec2::new(panel.rect.w.max(1.0) as u32, panel.rect.h.max(1.0) as u32),
    }
}
