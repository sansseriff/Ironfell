//! Render-only pointer reference for the opt-in filming panel. Never a document
//! node or a pick target: this bypasses gestures while retaining normal rendering.
use crate::document_bridge::PanelViews;
use crate::vector::{DisplayList, DisplayListRebuild, Overlay, VectorLayer, VectorView, order};
use bevy::prelude::*;

#[derive(Component)]
pub(super) struct FilmFollower;

pub(super) fn draw(
    mut commands: Commands,
    views: Res<PanelViews>,
    pointer: Res<crate::PointerState>,
    dpr: Res<crate::Dpr>,
    mut followers: Query<(Entity, &mut DisplayList, &mut VectorView), With<FilmFollower>>,
) {
    let Some(panel) = views.get("latency-film") else {
        for (entity, ..) in &followers {
            commands.entity(entity).despawn();
        }
        return;
    };
    let view = VectorView {
        pos: UVec2::new(panel.rect.x.max(0.0) as u32, panel.rect.y.max(0.0) as u32),
        size: UVec2::new(panel.rect.w.max(1.0) as u32, panel.rect.h.max(1.0) as u32),
    };
    let Ok((_, mut list, mut old_view)) = followers.single_mut() else {
        let entity = commands
            .spawn((FilmFollower, DisplayList::default(), view, Overlay))
            .id();
        commands
            .entity(entity)
            .insert(VectorLayer::screen(order::LATENCY_FILM).in_view(entity));
        return;
    };
    if *old_view != view {
        *old_view = view;
    }
    list.rebuild(|b| {
        if !panel.rect.contains(pointer.screen) {
            return;
        }
        let k = dpr.0 as f64;
        // Same vertical coordinate as the pointer. The +96 CSS px offset avoids
        // overlap with the OS cursor, DOM ring (-96) and normal dragged shape.
        let x = pointer.screen.x as f64 + 96.0 * k;
        let y = pointer.screen.y as f64;
        b.clipped(kurbo::Affine::IDENTITY, panel.rect.to_kurbo(), |b| {
            for (color, half, thickness) in [
                (peniko::Color::from_rgba8(0, 0, 0, 255), 14.0, 5.0),
                (peniko::Color::from_rgba8(255, 0, 170, 255), 11.0, 2.5),
            ] {
                b.fill(
                    kurbo::Affine::IDENTITY,
                    color,
                    kurbo::Rect::new(
                        x - half * k,
                        y - thickness * k,
                        x + half * k,
                        y + thickness * k,
                    ),
                );
                b.fill(
                    kurbo::Affine::IDENTITY,
                    color,
                    kurbo::Rect::new(
                        x - thickness * k,
                        y - half * k,
                        x + thickness * k,
                        y + half * k,
                    ),
                );
            }
        });
    });
}
