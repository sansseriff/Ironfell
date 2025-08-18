use bevy::math::bounding::RayCast3d;
use bevy::prelude::*;

use crate::bevy_app::overlay2d::DraggableSquare;
use crate::bevy_app::scene3d::{CurrentVolume, MainCamera3D};

// Overlay 2D placeholder: treat draggable square as a hit if pointer over its AABB.
pub fn pick_overlay_2d_system(
    pointer: Res<crate::PointerState>,
    square: Option<Res<DraggableSquare>>, // legacy structure
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.overlay.clear();
    let Some(square) = square else {
        return;
    };
    let half = square.size * 0.5;
    let pos = square.position;
    let p = pointer.screen; // screen -> we don't yet compute overlay_world; fallback AABB in overlay coords if available
    // Without overlay_world mapping yet, skip unless we later map pointer.overlay_world.
    if let Some(world_pos) = pointer.overlay_world {
        // once implemented
        if world_pos.x >= pos.x - half.x
            && world_pos.x <= pos.x + half.x
            && world_pos.y >= pos.y - half.y
            && world_pos.y <= pos.y + half.y
        {
            // No entity ID for square yet; will become component later; using placeholder None.
        }
    }
    let _ = p; // suppress unused for now
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
    // Build ray from pointer screen pos
    let Some(ray) = camera
        .viewport_to_world(cam_transform, pointer.screen)
        .ok()
        .map(Ray3d::from)
    else {
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

// Determine primary entity hit (currently prefer 3D first; adjust when UI/overlay implemented)
pub fn resolve_primary_hit_system(mut hits: ResMut<crate::PointerHits>) {
    hits.primary = hits.world3d.first().map(|h| h.entity);
}
