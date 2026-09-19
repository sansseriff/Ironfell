use bevy::math::bounding::RayCast3d;
use bevy::prelude::*;

use crate::bevy_app::scene3d::{CurrentVolume, MainCamera3D};
use crate::document_bridge::{Doc2d, NodeMap, Provenance};
use crate::panels::{Panels, VIEWER_PANEL, document_from_screen};

/// Build a world ray from a window-space cursor position (physical px).
/// The camera renders into a viewport sub-rect, so the position is translated to
/// viewport-local coordinates first; returns None when the cursor is outside the
/// viewport (e.g. over the timeline or an HTML panel).
pub fn camera_ray_from_window_px(
    camera: &Camera,
    cam_transform: &GlobalTransform,
    screen: Vec2,
) -> Option<Ray3d> {
    // Reject clicks outside the camera's viewport sub-rect (e.g. over the timeline
    // or an HTML panel). `screen` and the viewport are both window/canvas-relative
    // physical px.
    if let Some(vp) = &camera.viewport {
        let origin = vp.physical_position.as_vec2();
        let size = vp.physical_size.as_vec2();
        let p = screen - origin;
        if p.x < 0.0 || p.y < 0.0 || p.x > size.x || p.y > size.y {
            return None;
        }
    }
    // NOTE: `viewport_to_world` expects the position in WINDOW space, not
    // viewport-local space — it subtracts the viewport offset itself via
    // `logical_viewport_rect().min`. So pass the full-window `screen`, not
    // `screen - viewport.origin`; pre-subtracting double-counts the offset and
    // throws the ray off by the viewport's top-left (the left panel width). The
    // window scale factor is forced to 1.0, so physical `screen` == logical.
    camera
        .viewport_to_world(cam_transform, screen)
        .ok()
        .map(Ray3d::from)
}

/// Hit-test the document's 2D nodes. The cursor is mapped into document space,
/// then into each node's local space through the inverse of its global
/// transform, so rotated and scaled shapes pick correctly. Depth is the
/// node's painter index: later in preorder means on top.
pub fn pick_document_2d_system(
    pointer: Res<crate::PointerState>,
    panels: Res<Panels>,
    map: Res<NodeMap>,
    nodes: Query<(Entity, &Provenance, &Doc2d, &GlobalTransform)>,
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.overlay.clear();
    let Some(rect) = panels.rect(VIEWER_PANEL) else { return };
    let Some(p) = document_from_screen(rect, pointer.screen) else { return };
    for (entity, prov, d, global) in &nodes {
        let local = global.affine().inverse().transform_point3(p.extend(0.0));
        if d.shape.contains_local(local.truncate()) {
            let z = map.paint_index(prov.0).unwrap_or(0) as f32;
            hits.overlay.push(crate::Hit2D { entity, z });
        }
    }
    hits.overlay.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));
}

// 3D picking using AABB intersection along view ray.
pub fn pick_world_3d_system(
    pointer: Res<crate::PointerState>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera3D>>,
    query: Query<(Entity, &CurrentVolume)>,
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.world3d.clear();
    let Ok((camera, cam_transform)) = cameras.single() else {
        return;
    };
    // Build ray from pointer screen pos (viewport-aware)
    let Some(ray) = camera_ray_from_window_px(camera, cam_transform, pointer.screen) else {
        return;
    };
    let ray_cast = RayCast3d::from_ray(ray, 10_000.0);
    for (entity, vol) in query.iter() {
        if let Some(dist) = ray_cast.aabb_intersection_at(&vol.0) {
            // using underlying Aabb3d
            hits.world3d.push(crate::Hit3D {
                entity,
                distance: dist,
            });
        }
    }
    hits.world3d.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// The 2D layer composites over the 3D viewport, so a 2D hit wins.
pub fn resolve_primary_hit_system(mut hits: ResMut<crate::PointerHits>) {
    hits.primary = hits
        .overlay
        .first()
        .map(|h| h.entity)
        .or_else(|| hits.world3d.first().map(|h| h.entity));
}
