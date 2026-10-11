use bevy::math::bounding::RayCast3d;
use bevy::prelude::*;

use crate::bevy_app::scene3d::{CurrentVolume, EmbedCam, PanelCamera};
use crate::document_bridge::{Doc2d, DocumentStore, EmbedViews, InComposition, NodeMap, PanelViews, nest};
use iron_document::NodeId;
use iron_document::composition::Dims;

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

/// Which composition panel the pointer is over (ECM §7.1: picking maps the
/// pointer to a panel first). Both pickers hit only what that panel shows.
pub fn pointer_panel_system(pointer: Res<crate::PointerState>, views: Res<PanelViews>, mut hits: ResMut<crate::PointerHits>) {
    let now = views.at(pointer.screen).map(|v| v.panel.as_str());
    if hits.panel.as_deref() != now {
        hits.panel = now.map(str::to_owned);
    }
}

/// Hit-test the document's 2D nodes in the 2D panel under the pointer. The
/// cursor is mapped into document space, then through any embeds into the
/// embedded composition's space (inside its region only), then into each
/// node's local space through the inverse of its global transform, so
/// rotated, scaled and embedded shapes all pick. Depth is painter order of
/// the walk: later means on top.
pub fn pick_document_2d_system(
    pointer: Res<crate::PointerState>,
    views: Res<PanelViews>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.overlay.clear();
    let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two) else { return };
    let Some(p) = view.to_doc(pointer.screen) else { return };
    let doc = store.0.document();
    let roots = nest::panel_roots(doc, view);
    let mut walk = Pick { doc, map: &map, nodes: &nodes, globals: &globals, z: 0, out: Vec::new() };
    walk.walk(&roots, kurbo::Point::new(p.x as f64, p.y as f64), kurbo::Affine::IDENTITY, 0);
    let mut out = walk.out;
    out.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));
    hits.overlay = out;
}

struct Pick<'a, 'w, 's> {
    doc: &'a iron_document::Document,
    map: &'a NodeMap,
    nodes: &'a Query<'w, 's, (&'static Doc2d, &'static GlobalTransform)>,
    globals: &'a Query<'w, 's, &'static GlobalTransform>,
    z: u32,
    out: Vec<crate::Hit2D>,
}

impl Pick<'_, '_, '_> {
    /// `p` is in the current composition's space; `space` maps the panel's
    /// document space into it.
    fn walk(&mut self, roots: &[NodeId], p: kurbo::Point, space: kurbo::Affine, depth: u32) {
        for id in nest::preorder(self.doc, roots) {
            self.z += 1;
            if nest::is_embed(self.doc, id) {
                if depth < nest::MAX_DEPTH
                    && let Some(e) = nest::embed_2d(self.doc, self.map, self.globals, self.nodes, id)
                {
                    let inv = e.placement.inverse();
                    let q = inv * p;
                    if e.region.contains(q) {
                        let kids: Vec<NodeId> = self.doc.children(e.source).to_vec();
                        self.walk(&kids, q, inv * space, depth + 1);
                    }
                }
                continue;
            }
            let Some(entity) = self.map.entity(id) else { continue };
            let Ok((d, global)) = self.nodes.get(entity) else { continue };
            let local = global.affine().inverse().transform_point3(Vec3::new(p.x as f32, p.y as f32, 0.0));
            if d.shape.contains_local(local.truncate()) {
                self.out.push(crate::Hit2D { entity, z: self.z as f32, space });
            }
        }
    }
}

// 3D picking: AABB intersection along a ray from the camera that shows the
// pointer's spot — a 3D panel's own camera, or, in a 2D panel, the camera
// of the 3D embed under the pointer (ECM §7.4) — against that view's
// composition (and, in a panel, loose 3D nodes).
pub fn pick_world_3d_system(
    pointer: Res<crate::PointerState>,
    views: Res<PanelViews>,
    embeds: Res<EmbedViews>,
    cameras: Query<(Entity, &Camera, &GlobalTransform, Option<&PanelCamera>, Option<&EmbedCam>)>,
    query: Query<(Entity, &CurrentVolume, &InComposition)>,
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.world3d.clear();
    let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)) else { return };
    let (camera_entity, composition, loose) = match view.dims {
        Dims::Three => {
            let Some((e, ..)) = cameras.iter().find(|(_, _, _, pc, _)| pc.is_some_and(|pc| pc.panel == view.panel)) else { return };
            (e, view.composition, true)
        }
        Dims::Two => {
            let p = kurbo::Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
            // The last one listed is drawn last, so it is on top.
            let Some(ev) = embeds.0.iter().rev().find(|v| v.panel == view.panel && v.rect.contains(p)) else { return };
            let Some((e, ..)) = cameras.iter().find(|(_, _, _, _, ec)| ec.is_some_and(|ec| ec.key == ev.key)) else { return };
            (e, Some(ev.source), false)
        }
    };
    let Ok((_, camera, cam_transform, ..)) = cameras.get(camera_entity) else { return };
    let Some(ray) = camera_ray_from_window_px(camera, cam_transform, pointer.screen) else {
        return;
    };
    let ray_cast = RayCast3d::from_ray(ray, 10_000.0);
    for (entity, vol, within) in query.iter() {
        let shown = match within.0 {
            Some(c) => Some(c) == composition,
            None => loose,
        };
        if !shown {
            continue;
        }
        if let Some(dist) = ray_cast.aabb_intersection_at(&vol.0) {
            hits.world3d.push(crate::Hit3D { entity, distance: dist, camera: camera_entity });
        }
    }
    hits.world3d.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// One panel is under the pointer, so at most one of the two pickers hit.
pub fn resolve_primary_hit_system(mut hits: ResMut<crate::PointerHits>) {
    hits.primary = hits
        .overlay
        .first()
        .map(|h| h.entity)
        .or_else(|| hits.world3d.first().map(|h| h.entity));
}
