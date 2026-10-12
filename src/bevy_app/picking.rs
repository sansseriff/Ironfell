use bevy::math::bounding::RayCast3d;
use bevy::prelude::*;

use crate::bevy_app::scene3d::{CurrentVolume, EmbedCam, PanelCamera, PointerToCamera, ScreenRect, to_camera_px};
use crate::document_bridge::planes::{EmbedPlane, PX_PER_UNIT};
use crate::document_bridge::{Doc2d, DocumentStore, EmbedViews, InComposition, NodeMap, PanelViews, Provenance, nest};
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
/// the walk: later means on top. A 3D embed is opaque: 2D content under it
/// does not pick where it shows, and the 3D picker takes over there.
pub fn pick_document_2d_system(
    pointer: Res<crate::PointerState>,
    views: Res<PanelViews>,
    embeds: Res<EmbedViews>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.overlay.clear();
    hits.embed = None;
    hits.embed_chain.clear();
    let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two) else { return };
    let Some(p) = view.to_doc(pointer.screen) else { return };
    let doc = store.0.document();
    let roots = nest::panel_roots(doc, view);
    let screen = kurbo::Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let mut walk = Pick { doc, map: &map, nodes: &nodes, globals: &globals, embeds: &embeds, screen, z: 0, out: Vec::new(), embed: None };
    walk.walk(&roots, &[], kurbo::Point::new(p.x as f64, p.y as f64), kurbo::Affine::IDENTITY, &view.panel, 0);
    let mut out = walk.out;
    // Whatever a 3D embed covers is not hit; whatever is drawn over it is.
    if let Some((_, z, _)) = &walk.embed {
        out.retain(|h| h.z > *z);
    }
    sort_hits(&mut out);
    if let Some((k, _, chain)) = walk.embed {
        hits.embed = Some(k);
        hits.embed_chain = chain;
    }
    hits.overlay = out;
}

/// Topmost first: shapes by painter depth, then embed backgrounds by depth.
fn sort_hits(hits: &mut [crate::Hit2D]) {
    hits.sort_by(|a, b| a.background.cmp(&b.background).then(b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal)));
}

pub(crate) struct Pick<'a, 'w, 's> {
    pub doc: &'a iron_document::Document,
    pub map: &'a NodeMap,
    pub nodes: &'a Query<'w, 's, (&'static Doc2d, &'static GlobalTransform)>,
    pub globals: &'a Query<'w, 's, &'static GlobalTransform>,
    pub embeds: &'a EmbedViews,
    /// The pointer in window px, for 3D embeds (`EmbedView::texture_point`).
    pub screen: kurbo::Point,
    pub z: u32,
    pub out: Vec<crate::Hit2D>,
    /// The topmost 3D embed under the pointer, its painter depth and chain.
    pub embed: Option<(String, f32, Vec<NodeId>)>,
}

impl Pick<'_, '_, '_> {
    /// `p` is in the current composition's space; `space` maps the panel's
    /// document space into it; `prefix` is the chain down to (and including)
    /// the enclosing embed. Painter order: each node, then its children.
    pub fn walk(&mut self, roots: &[NodeId], prefix: &[NodeId], p: kurbo::Point, space: kurbo::Affine, key: &str, depth: u32) {
        for &id in roots {
            self.z += 1;
            let mut chain = prefix.to_vec();
            chain.push(id);
            if nest::is_embed(self.doc, id) {
                if depth >= nest::MAX_DEPTH {
                    continue;
                }
                let key = format!("{key}/{id}");
                if let Some(e) = nest::embed_2d(self.doc, self.map, self.globals, self.nodes, id) {
                    let inv = e.placement.inverse();
                    let q = inv * p;
                    if e.region.contains(q) {
                        // The embed's own area is a hit below its content:
                        // what a press on its background selects.
                        if let Some(entity) = self.map.entity(id) {
                            self.out.push(crate::Hit2D { entity, chain: chain.clone(), background: true, z: self.z as f32, space, through: None });
                        }
                        let kids: Vec<NodeId> = self.doc.children(e.source).to_vec();
                        self.walk(&kids, &chain, q, inv * space, &key, depth + 1);
                    }
                } else if self.embeds.0.iter().any(|v| v.key == key && v.texture_point(self.screen).is_some()) {
                    self.embed = Some((key, self.z as f32, chain));
                }
                continue;
            }
            if let Some(entity) = self.map.entity(id)
                && let Ok((d, global)) = self.nodes.get(entity)
            {
                let local = global.affine().inverse().transform_point3(Vec3::new(p.x as f32, p.y as f32, 0.0));
                if d.shape.contains_local(local.truncate()) {
                    self.out.push(crate::Hit2D { entity, chain: chain.clone(), background: false, z: self.z as f32, space, through: None });
                }
            }
            let kids: Vec<NodeId> = self.doc.children(id).to_vec();
            self.walk(&kids, &chain, p, space, key, depth);
        }
    }
}

// 3D picking: AABB intersection along a ray from the camera that shows the
// pointer's spot — a 3D panel's own camera, or, in a 2D panel, the camera
// of the 3D embed under the pointer (ECM §7.4) — against that view's
// composition (and, in a panel, loose 3D nodes).
#[allow(clippy::too_many_arguments)]
pub fn pick_world_3d_system(
    pointer: Res<crate::PointerState>,
    views: Res<PanelViews>,
    embeds: Res<EmbedViews>,
    cameras: Query<(Entity, &Camera, &GlobalTransform, Option<&PanelCamera>, Option<&EmbedCam>, Option<&ScreenRect>, Option<&PointerToCamera>)>,
    query: Query<(Entity, &CurrentVolume, &InComposition, &Provenance)>,
    planes: Query<(Entity, &GlobalTransform, &EmbedPlane, &InComposition, &Provenance)>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    mut hits: ResMut<crate::PointerHits>,
) {
    hits.world3d.clear();
    let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)) else { return };
    let doc = store.0.document();
    // The chain down to the 3D view's content: nothing in a 3D panel, the
    // embed's own chain when looking into a 3D embed.
    let prefix: Vec<NodeId> = if view.dims == Dims::Two { hits.embed_chain.clone() } else { Vec::new() };
    let (camera_entity, composition, loose) = match view.dims {
        Dims::Three => {
            let Some((e, ..)) = cameras.iter().find(|(_, _, _, pc, ..)| pc.is_some_and(|pc| pc.panel == view.panel)) else { return };
            (e, view.composition, true)
        }
        Dims::Two => {
            // The 3D embed the 2D picker found on top under the pointer.
            let Some(ev) = hits.embed.as_deref().and_then(|k| embeds.0.iter().find(|v| v.key == k)) else { return };
            let Some((e, ..)) = cameras.iter().find(|(_, _, _, _, ec, ..)| ec.is_some_and(|ec| ec.key == ev.key)) else { return };
            (e, Some(ev.source), false)
        }
    };
    let Ok((_, camera, cam_transform, _, _, screen, to_camera)) = cameras.get(camera_entity) else { return };
    // Window px → the camera's own texture px.
    let Some(ray) = camera_ray_from_window_px(camera, cam_transform, to_camera_px(pointer.screen, screen, to_camera)) else {
        return;
    };
    let shown = |within: &InComposition| match within.0 {
        Some(c) => Some(c) == composition,
        None => loose,
    };
    let ray_cast = RayCast3d::from_ray(ray, 10_000.0);
    for (entity, vol, within, prov) in query.iter() {
        if !shown(within) {
            continue;
        }
        if let Some(dist) = ray_cast.aabb_intersection_at(&vol.0) {
            let mut chain = prefix.clone();
            chain.extend(nest::lineage(doc, prov.0));
            hits.world3d.push(crate::Hit3D { entity, chain, distance: dist, camera: camera_entity });
        }
    }
    hits.world3d.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 2D compositions on planes (ECM §7.4): the nearest plane in front of
    // every mesh hit hands the pointer, as document px, to the 2D picker.
    let nearest_mesh = hits.world3d.first().map(|h| h.distance).unwrap_or(f32::INFINITY);
    let mut best: Option<(f32, Entity, kurbo::Point, iron_document::NodeId, NodeId)> = None;
    for (entity, gt, plane, within, prov) in &planes {
        if !shown(within) {
            continue;
        }
        let Some((dist, p)) = ray_plane_doc(ray, gt, plane) else { continue };
        if dist < nearest_mesh && plane.region.contains(p) && best.as_ref().is_none_or(|b| dist < b.0) {
            best = Some((dist, entity, p, plane.source, prov.0));
        }
    }
    if let Some((_, plane_entity, p, source, plane_node)) = best {
        let kids: Vec<iron_document::NodeId> = doc.children(source).to_vec();
        let mut walk = Pick { doc, map: &map, nodes: &nodes, globals: &globals, embeds: &embeds, screen: kurbo::Point::ZERO, z: 0, out: Vec::new(), embed: None };
        let mut chain = prefix.clone();
        chain.extend(nest::lineage(doc, plane_node));
        walk.walk(&kids, &chain, p, kurbo::Affine::IDENTITY, "plane", 0);
        let mut out = walk.out;
        sort_hits(&mut out);
        for h in &mut out {
            h.through = Some((camera_entity, plane_entity));
        }
        // Whatever is on the plane is in front of what is behind it.
        hits.world3d.clear();
        hits.overlay = out;
    }
}

/// Where a ray meets a plane: the distance along the ray and the point as
/// document px in the plane's composition (not limited to its region).
pub fn ray_plane_doc(ray: Ray3d, plane_transform: &GlobalTransform, plane: &EmbedPlane) -> Option<(f32, kurbo::Point)> {
    let inv = plane_transform.affine().inverse();
    let o = inv.transform_point3(ray.origin);
    let d = inv.transform_vector3(*ray.direction);
    if d.z.abs() < 1e-6 {
        return None;
    }
    let t = -o.z / d.z;
    if t <= 0.0 {
        return None;
    }
    let local = o + d * t;
    let p = kurbo::Point::new(plane.region.x0 + local.x as f64 * PX_PER_UNIT, plane.region.y0 - local.y as f64 * PX_PER_UNIT);
    let world = plane_transform.transform_point(local);
    Some((world.distance(ray.origin), p))
}

/// One panel is under the pointer, so at most one of the two pickers hit.
pub fn resolve_primary_hit_system(mut hits: ResMut<crate::PointerHits>) {
    hits.primary = hits
        .overlay
        .first()
        .map(|h| h.entity)
        .or_else(|| hits.world3d.first().map(|h| h.entity));
}
