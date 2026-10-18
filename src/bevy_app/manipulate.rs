//! Manipulators: how a pointer becomes document values, for every gesture
//! that edits the document, in 2D and 3D (plans/specs/
//! infinite-canvas-architecture-patterns.md §4, "2D and 3D: what to share").
//!
//! The select tool runs one pointer state machine (press, slop, drag,
//! release, Esc). When a press becomes an edit it holds one [`Manip`], and
//! every manipulator has the same lifecycle:
//!
//! - it **watches** nodes: the store's gesture (`iron_document::gesture`)
//!   cancels it when an edit conflicts with them;
//! - each frame it **updates**: the pointer, mapped into the space it works
//!   in, becomes leaf values previewed through the store;
//! - on release it **commits** one labelled transaction; on Esc or a
//!   conflict nothing is written.
//!
//! What differs is only the mapping and the value semantics: a 2D move
//! drags through `Store::begin_drag` in a panel's document space, a resize
//! decomposes an affine, a 3D move meets a plane through a camera ray, the
//! gizmo hands back whole transforms.

use bevy::prelude::*;
use iron_document::copy;
use iron_document::{Actor, DragHandle, DragMode, Handle, LeafPath, NodeId, Store, TransactionInput, Value, Version};
use kurbo::{Affine, Point};

use crate::bevy_app::select::{Geo, Part, ShapeGrip, TransformKind, preview_writes, shape_grip_writes};
use crate::document_bridge::planes::EmbedPlane;
use crate::document_bridge::{PanelView, PanelViews, Shape2d};
use crate::vector::DisplayListRebuild;

pub(crate) type DragCameras<'w, 's> = Query<'w, 's, (Entity, &'static Camera, &'static GlobalTransform, Option<&'static crate::bevy_app::scene3d::ScreenRect>, Option<&'static crate::bevy_app::scene3d::PointerToCamera>)>;
pub(crate) type Planes<'w, 's> = Query<'w, 's, (&'static GlobalTransform, &'static EmbedPlane)>;

/// Window px the pointer may travel before a 3D or plane drag moves
/// anything, so a click is not a move.
const SLOP: f32 = 3.0;
/// Rotation snap with Shift.
const SNAP: f64 = std::f64::consts::PI / 12.0;

fn leaf(s: &str) -> LeafPath {
    s.parse().expect("declared leaf")
}

/// Everything a manipulator reads in a frame.
pub(crate) struct Cx<'a, 'w, 's> {
    pub store: &'a mut Store,
    pub pointer: &'a crate::PointerState,
    pub views: &'a PanelViews,
    pub drawables: &'a crate::document_bridge::Drawables,
    pub spatial: &'a crate::document_bridge::SpatialScene,
    pub cameras: &'a DragCameras<'w, 's>,
    pub planes: &'a Planes<'w, 's>,
    pub gizmo: Option<&'a mut Gizmo3d>,
}

impl Cx<'_, '_, '_> {
    fn screen(&self) -> Point {
        Point::new(self.pointer.screen.x as f64, self.pointer.screen.y as f64)
    }
}

/// How the pointer reaches the space of a node dragged outside the box:
/// through a 2D panel (and any embeds), or through a plane in 3D.
#[derive(Clone, Debug)]
pub(crate) enum Mapping {
    /// The panel's document px, then `space` into the node's composition.
    Panel { panel: String, space: Affine },
    /// The camera that showed the plane, then the plane, into its content.
    Plane { camera: Entity, plane: Entity },
}

impl Mapping {
    /// The pointer (window px) in the node's composition space; `None` off
    /// the panel or off the plane, where the drag holds still rather than
    /// jump.
    fn point(&self, screen: Vec2, views: &PanelViews, cameras: &DragCameras, planes: &Planes) -> Option<Point> {
        match self {
            Mapping::Panel { panel, space } => {
                let p = views.get(panel).and_then(|v| v.to_doc(screen))?;
                Some(*space * Point::new(p.x as f64, p.y as f64))
            }
            Mapping::Plane { camera, plane } => {
                let (_, cam, cam_tf, rect, to_camera) = cameras.get(*camera).ok()?;
                let local = crate::bevy_app::scene3d::to_camera_px(screen, rect, to_camera);
                let ray = crate::bevy_app::picking::camera_ray_from_window_px(cam, cam_tf, local)?;
                let (pt, p) = planes.get(*plane).ok()?;
                let (_, q) = crate::bevy_app::picking::ray_plane_doc(ray, pt, p)?;
                Some(Point::new(q.x as f64, q.y as f64))
            }
        }
    }
}

/// A 2D move of the selection (or of one path point): each node through
/// `Store::begin_drag`, by the pointer's travel in the panel since the press.
pub(crate) struct Drag {
    pub label: String,
    pub view: PanelView,
    pub start: Point,
    /// Each drag and the map from the panel's document px into its node's
    /// composition space.
    pub drags: Vec<(DragHandle, Affine)>,
    /// Alt duplicates (a selection move, not a path point).
    pub duplicable: bool,
    /// While Alt holds a duplicate: its transaction and the drags of the
    /// originals, to return to if Alt comes up.
    pub duplicate: Option<(Version, Vec<(DragHandle, Affine)>)>,
}

impl Drag {
    /// Duplicate the dragged nodes and drag the copies instead (Alt down),
    /// or take the copies back and drag the originals again (Alt up). The
    /// store's gesture moves with it. Returns the nodes now dragged, for the
    /// selection. The duplicate is applied at once, not queued, because the
    /// copies must exist to be dragged this frame; the bridge reconciles it
    /// from the log like any synchronous apply.
    pub fn set_duplicate(&mut self, store: &mut Store, on: bool) -> Option<Vec<NodeId>> {
        if on == self.duplicate.is_some() || !self.duplicable {
            return None;
        }
        for (h, _) in &self.drags {
            store.end_drag(h);
        }
        store.cancel_gesture();
        if on {
            let ids: Vec<NodeId> = self.drags.iter().map(|(h, _)| h.node).collect();
            let pasted = match copy::duplicate(store, &ids) {
                Ok(p) => p,
                Err(e) => {
                    info!("alt-drag: {e}");
                    return None;
                }
            };
            let label = format!("duplicate {}", self.label.trim_start_matches("move "));
            let version = match store.apply(TransactionInput { label, actor: Actor::Human, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false }) {
                Ok(a) => a.version,
                Err(errors) => {
                    for e in errors {
                        warn!("alt-drag duplicate rejected: {e}");
                    }
                    return None;
                }
            };
            let copy_of = |n: NodeId| pasted.sources.iter().position(|&s| s == n).map(|i| pasted.roots[i]);
            store.begin_gesture(&pasted.roots);
            let originals = std::mem::take(&mut self.drags);
            self.drags = originals.iter().filter_map(|(h, inv)| Some((store.begin_drag(copy_of(h.node)?, h.anchor), *inv))).collect();
            self.duplicate = Some((version, originals));
        } else {
            let (version, originals) = self.duplicate.take().expect("on a duplicate");
            if !store.retract(version, 0) {
                info!("alt-drag: the duplicate is no longer the latest edit; the copies stay");
            }
            let ids: Vec<NodeId> = originals.iter().map(|(h, _)| h.node).collect();
            store.begin_gesture(&ids);
            self.drags = originals.iter().map(|(h, inv)| (store.begin_drag(h.node, h.anchor), *inv)).collect();
        }
        Some(self.drags.iter().map(|(h, _)| h.node).collect())
    }
}

/// A resize or rotation of the selection's box (`Part::apply`).
pub(crate) struct Transform2 {
    pub label: String,
    pub view: PanelView,
    pub kind: TransformKind,
    pub frame: Affine,
    pub start: Point,
    pub parts: Vec<Part>,
    pub rot0: Option<f64>,
}

/// A 3D move in a plane through the object: camera-facing, or the ground
/// with Ctrl.
pub(crate) struct World {
    pub id: NodeId,
    pub camera: Entity,
    pub plane_origin: Vec3,
    pub plane_normal: Vec3,
    pub grab_offset: Vec3,
    pub press: Vec2,
}

/// A port dragged to plug or unplug it (component-model.md §4.3): its
/// loose end, or the instance it belongs to by the port, follows the
/// pointer and snaps onto a compatible port within `radius`.
pub(crate) struct PlugDrag {
    pub plugging: iron_document::Plugging,
    pub view: PanelView,
    /// The port's position less the pointer's at the press, in the
    /// composition (the panel's document px).
    pub offset: kurbo::Vec2,
    /// The snap distance in composition units.
    pub radius: f64,
    /// The ports it could plug into, and the one it is snapped to.
    pub targets: Vec<iron_document::plug::PortAnchor>,
    pub target: Option<iron_document::plug::PortAnchor>,
    /// Where the dragged port is now.
    pub at: Point,
}

/// Every document edit a pointer can make.
pub(crate) enum Manip {
    Drag(Drag),
    Plug(PlugDrag),
    Transform(Transform2),
    /// A path point's Bézier handle, `out` or in, mirrored unless Alt;
    /// `to_local` maps window px into the path's space.
    PathHandle { point: NodeId, out: bool, to_local: Affine, at: Point },
    /// A path's segment bent (Figma's Cmd-drag on a segment): the curve
    /// at `t` follows the pointer by moving both its inner handles, from
    /// point `a`'s out-handle to `b`'s in-handle; `curve` is the segment at
    /// the press, in the path's space, which `to_local` maps window px to.
    Bend { a: NodeId, b: NodeId, curve: kurbo::CubicBez, t: f64, to_local: Affine },
    /// A lone shape's own handle; `arc` is (start, sweep, inner) at the press.
    ShapeGrip { id: NodeId, grip: ShapeGrip, to_local: Affine, arc: [f64; 3] },
    /// Scrubbing a slider's value along its track.
    Slider { id: NodeId, mapping: Mapping },
    /// 2D content dragged on a plane in 3D, through `Store::begin_drag`.
    Plane { handle: DragHandle, mapping: Mapping, press: Vec2, ty: String },
    World(World),
    /// The 3D transform gizmo (`Gizmo3d`), on the selected meshes.
    Gizmo { targets: Vec<NodeId>, kind: Option<&'static str> },
}

impl Manip {
    /// The panel view a 2D manipulator reads the pointer through: it
    /// follows the view's pan and zoom, and ends if the view does.
    pub fn view_mut(&mut self) -> Option<&mut PanelView> {
        match self {
            Manip::Drag(d) => Some(&mut d.view),
            Manip::Plug(d) => Some(&mut d.view),
            Manip::Transform(t) => Some(&mut t.view),
            _ => None,
        }
    }

    /// What the release's transaction is called.
    pub fn label(&self, store: &Store) -> String {
        let ty = |id: NodeId| store.document().live(id).map_or("node", |n| n.ty.name());
        match self {
            Manip::Drag(d) => match &d.duplicate {
                Some(_) => format!("duplicate {}", d.label.trim_start_matches("move ")),
                None => d.label.clone(),
            },
            Manip::Plug(d) => format!("plug {}", d.plugging.from.name),
            Manip::Transform(t) => t.label.clone(),
            Manip::PathHandle { .. } => "move handle".into(),
            Manip::Bend { .. } => "bend segment".into(),
            Manip::ShapeGrip { grip, .. } => if matches!(grip, ShapeGrip::Corner(_)) { "round corners" } else { "edit arc" }.into(),
            Manip::Slider { .. } => "set slider".into(),
            Manip::Plane { ty, .. } => format!("move {ty}"),
            Manip::World(w) => format!("move {}", ty(w.id)),
            Manip::Gizmo { targets, kind } => {
                let what = if targets.len() == 1 { ty(targets[0]).to_owned() } else { format!("{} nodes", targets.len()) };
                format!("{} {what}", kind.unwrap_or("move"))
            }
        }
    }

    /// The duplicate an Alt-drag made, to merge the move into on release.
    pub fn duplicate(&self) -> Option<Version> {
        match self {
            Manip::Drag(d) => d.duplicate.as_ref().map(|(v, _)| *v),
            _ => None,
        }
    }

    /// Release the store's drag fingers (the gesture itself commits or
    /// cancels separately).
    pub fn end(&self, store: &mut Store) {
        match self {
            Manip::Drag(d) => {
                for (h, _) in &d.drags {
                    store.end_drag(h);
                }
            }
            Manip::Plane { handle, .. } => store.end_drag(handle),
            Manip::Plug(d) => store.end_drag(&d.plugging.drag),
            _ => {}
        }
    }

    /// Follow the pointer for one frame: preview what it asks for.
    pub fn update(&mut self, cx: &mut Cx) {
        let screen = cx.screen();
        let mods = &cx.pointer.modifiers;
        match self {
            Manip::Drag(d) => {
                let p = d.view.affine().inverse() * screen;
                let mut v = p - d.start;
                if mods.shift {
                    if v.x.abs() > v.y.abs() { v.y = 0.0 } else { v.x = 0.0 }
                }
                for (h, inv) in &d.drags {
                    let q = *inv * (d.start + v);
                    let t0 = bevy::platform::time::Instant::now();
                    let report = cx.store.drag_to(h, [q.x, q.y]);
                    if let DragMode::Cluster(cluster) = &h.mode {
                        // CS phase 3 exit 9: the budget is 2 ms per frame.
                        let ms = t0.elapsed().as_secs_f64() * 1e3;
                        info!(
                            "solve: cluster {cluster}, {} vars, {} iterations, {} evaluations, {ms:.2} ms{}",
                            report.vars,
                            report.iterations,
                            report.evaluations,
                            if report.converged { "" } else { "; not converged, pose kept" }
                        );
                    }
                }
            }
            Manip::Plug(d) => {
                let q = d.view.affine().inverse() * screen + d.offset;
                d.targets = cx.store.plug_targets(&d.plugging);
                d.target = d
                    .targets
                    .iter()
                    .map(|t| (Point::new(t.world[0], t.world[1]).distance(q), t))
                    .filter(|(dist, _)| *dist <= d.radius)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, t)| t.clone());
                d.at = d.target.as_ref().map_or(q, |t| Point::new(t.world[0], t.world[1]));
                cx.store.plug_drag(&d.plugging, [q.x, q.y], d.target.as_ref());
            }
            Manip::Transform(t) => {
                let p = t.view.affine().inverse() * screen;
                let a = match t.kind {
                    TransformKind::Resize { grip } => {
                        let frame = t.frame;
                        let u = frame.inverse() * p;
                        let anchor = if mods.alt { (0.5, 0.5) } else { (1.0 - grip.0, 1.0 - grip.1) };
                        // Relative to where the press landed, not the grip's
                        // exact corner, so pressing alone changes nothing.
                        let u0 = frame.inverse() * t.start;
                        let scale = |g: f64, a: f64, u: f64, u0: f64| if g == 0.5 || (u0 - a).abs() < 1e-9 { 1.0 } else { (u - a) / (u0 - a) };
                        let (mut fx, mut fy) = (scale(grip.0, anchor.0, u.x, u0.x), scale(grip.1, anchor.1, u.y, u0.y));
                        if mods.shift {
                            let m = if grip.0 == 0.5 { fy.abs() } else if grip.1 == 0.5 { fx.abs() } else { fx.abs().max(fy.abs()) };
                            fx = m * if fx < 0.0 { -1.0 } else { 1.0 };
                            fy = m * if fy < 0.0 { -1.0 } else { 1.0 };
                        }
                        // Never collapse to zero: at least a pixel.
                        let c = frame.as_coeffs();
                        let size = (c[0].hypot(c[1]), c[2].hypot(c[3]));
                        let floor = |f: f64, s: f64| if f.abs() * s < 1.0 { (1.0 / s.max(1e-9)) * if f < 0.0 { -1.0 } else { 1.0 } } else { f };
                        let (fx, fy) = (floor(fx, size.0), floor(fy, size.1));
                        frame * Affine::translate(anchor) * Affine::scale_non_uniform(fx, fy) * Affine::translate((-anchor.0, -anchor.1)) * frame.inverse()
                    }
                    TransformKind::Rotate => {
                        let c = t.frame * Point::new(0.5, 0.5);
                        let mut th = (p - c).atan2() - (t.start - c).atan2();
                        if mods.shift {
                            let base = t.rot0.unwrap_or(0.0);
                            th = ((base + th) / SNAP).round() * SNAP - base;
                        }
                        Affine::translate(c.to_vec2()) * Affine::rotate(th) * Affine::translate(-c.to_vec2())
                    }
                };
                let resize = matches!(t.kind, TransformKind::Resize { .. });
                let writes: Vec<(NodeId, &'static str, f64)> = t.parts.iter().flat_map(|part| part.apply(a, resize)).collect();
                // A text's sizing first, so its box measures the new way.
                for (id, sizing) in t.parts.iter().filter_map(|part| part.text_sizing(a, resize)) {
                    let path = "text.sizing".parse().expect("declared leaf");
                    if cx.store.resolved(id, path).and_then(|v| v.as_str().map(str::to_owned)).as_deref() != Some(sizing) {
                        cx.store.preview(id, path, iron_document::Value::Enum(sizing.into()));
                    }
                }
                preview_writes(cx.store, writes);
            }
            Manip::PathHandle { point, out, to_local, at } => {
                let v = *to_local * screen - *at;
                let (mine, other) = if *out { (("handles.ox", "handles.oy"), ("handles.ix", "handles.iy")) } else { (("handles.ix", "handles.iy"), ("handles.ox", "handles.oy")) };
                let mut writes = vec![(*point, mine.0, v.x), (*point, mine.1, v.y)];
                // Smooth unless Alt breaks the handles apart.
                if !mods.alt {
                    writes.extend([(*point, other.0, -v.x), (*point, other.1, -v.y)]);
                }
                preview_writes(cx.store, writes);
            }
            Manip::Bend { a, b, curve, t, to_local } => {
                use kurbo::ParamCurve as _;
                // Moving both inner controls by `e` moves the curve at `t` by
                // 3t(1-t)e; near the ends the weight is clamped so a grab
                // there cannot fling the handles.
                let w = 3.0 * t.clamp(0.15, 0.85) * (1.0 - t.clamp(0.15, 0.85));
                let e = (*to_local * screen - curve.eval(*t)) / w;
                let (out, inn) = (curve.p1 + e - curve.p0, curve.p2 + e - curve.p3);
                preview_writes(cx.store, vec![(*a, "handles.ox", out.x), (*a, "handles.oy", out.y), (*b, "handles.ix", inn.x), (*b, "handles.iy", inn.y)]);
            }
            Manip::ShapeGrip { id, grip, to_local, arc } => {
                let writes = {
                    let geo = Geo { store: &*cx.store, drawables: cx.drawables, spatial: cx.spatial };
                    shape_grip_writes(&geo, *id, *grip, *to_local * screen, *arc, mods.alt, mods.shift)
                };
                preview_writes(cx.store, writes);
            }
            Manip::Slider { id, mapping } => {
                // The pointer along the track, in the slider's own space,
                // becomes a value; anything bound to it follows this frame.
                let Some(p) = mapping.point(cx.pointer.screen, cx.views, cx.cameras, cx.planes) else { return };
                let Some(Shape2d::Slider { w, .. }) = cx.drawables.0.get(id).map(|d| d.shape.clone()) else { return };
                let local = crate::document_bridge::nest::world_2d(cx.store, cx.spatial, *id).inverse() * p;
                let t = (local.x / w.max(1.0) as f64).clamp(0.0, 1.0);
                let num = |s: &Store, f: &str, d: f64| s.resolved(*id, leaf(f)).and_then(|v| v.as_f64()).unwrap_or(d);
                let (min, max, step) = (num(cx.store, "slider.min", 0.0), num(cx.store, "slider.max", 1.0), num(cx.store, "slider.step", 0.0));
                let mut value = min + t * (max - min);
                if step > 0.0 {
                    value = min + ((value - min) / step).round() * step;
                }
                let value = value.clamp(min.min(max), max.max(min));
                if num(cx.store, "slider.value", f64::NAN) != value {
                    cx.store.preview(*id, leaf("slider.value"), Value::Number(value));
                }
            }
            Manip::Plane { handle, mapping, press, .. } => {
                if cx.pointer.screen.distance(*press) <= SLOP {
                    return;
                }
                let Some(p) = mapping.point(cx.pointer.screen, cx.views, cx.cameras, cx.planes) else { return };
                cx.store.drag_to(handle, [p.x, p.y]);
            }
            Manip::World(w) => {
                if cx.pointer.screen.distance(w.press) <= SLOP {
                    return;
                }
                let Some((_, camera, cam_tf, rect, to_camera)) = cx.cameras.get(w.camera).ok() else { return };
                let local = crate::bevy_app::scene3d::to_camera_px(cx.pointer.screen, rect, to_camera);
                let Some(ray) = crate::bevy_app::picking::camera_ray_from_window_px(camera, cam_tf, local) else { return };
                if let Some(hit) = intersect_ray_plane(ray, w.plane_origin, w.plane_normal) {
                    let p = hit + w.grab_offset;
                    if let Err(reason) = cx.store.preview_target(w.id, leaf("transform3d.pos"), Value::Vec3([p.x as f64, p.y as f64, p.z as f64])) {
                        // Handles are read at press, so this only happens if
                        // the document changed under the drag.
                        warn!("{}.transform3d.pos: {reason}", w.id);
                    }
                }
            }
            Manip::Gizmo { targets, kind } => {
                let Some(g) = cx.gizmo.as_deref_mut() else { return };
                let Some((result, transforms)) = g.last.take() else { return };
                *kind = Some(match result {
                    transform_gizmo::GizmoResult::Translation { .. } => "move",
                    transform_gizmo::GizmoResult::Scale { .. } => "scale",
                    _ => "rotate",
                });
                for (id, t) in targets.iter().zip(transforms) {
                    preview_transform3d(cx.store, *id, &t, *kind);
                }
            }
        }
    }
}

/// Preview the leaves of a mesh's `transform3d` a gizmo result changes.
fn preview_transform3d(store: &mut Store, id: NodeId, t: &transform_gizmo::math::Transform, kind: Option<&str>) {
    let q = Quat::from_xyzw(t.rotation.v.x as f32, t.rotation.v.y as f32, t.rotation.v.z as f32, t.rotation.s as f32);
    let (rx, ry, rz) = q.to_euler(EulerRot::XYZ);
    let writes: Vec<(&str, Value)> = match kind {
        Some("rotate") => vec![("transform3d.rot", Value::Vec3([rx as f64, ry as f64, rz as f64]))],
        Some("scale") => vec![("transform3d.scale", Value::Vec3([t.scale.x, t.scale.y, t.scale.z]))],
        _ => vec![("transform3d.pos", Value::Vec3([t.translation.x, t.translation.y, t.translation.z]))],
    };
    for (path, v) in writes {
        if let Err(reason) = store.preview_target(id, leaf(path), v) {
            debug!("{id}.{path}: {reason}");
        }
    }
}

// ---- Starting the edits the box does not run ---------------------------------

/// The manipulator for a press the select tool hands off: a slider's value,
/// 2D content on a plane in 3D, or a mesh in 3D. `None` when the node is
/// locked against the drag (the reasons are logged).
#[allow(clippy::too_many_arguments)]
pub(crate) fn hand_off(
    store: &mut Store,
    id: NodeId,
    panel: String,
    space: Affine,
    through: Option<(Entity, Entity)>,
    camera: Option<Entity>,
    slider: bool,
    pointer: &crate::PointerState,
    spatial: &crate::document_bridge::SpatialScene,
    cameras: &DragCameras,
    views: &PanelViews,
    planes: &Planes,
) -> Option<Manip> {
    store.begin_gesture(&[id]);
    let mapping = match through {
        Some((camera, plane)) => Mapping::Plane { camera, plane },
        None => Mapping::Panel { panel, space },
    };
    if slider {
        return Some(Manip::Slider { id, mapping });
    }
    let Some(camera) = camera else {
        // 2D content on a plane in 3D, grabbed where the press landed.
        let p = mapping.point(pointer.screen, views, cameras, planes).unwrap_or(Point::ORIGIN);
        let local = crate::document_bridge::nest::world_2d(store, spatial, id).inverse() * p;
        let handle = store.begin_drag(id, [local.x, local.y]);
        if let DragMode::Locked(reasons) = &handle.mode {
            for r in reasons {
                info!("{id} locked for this drag: {r}");
            }
            return None;
        }
        let ty = store.document().live(id).map_or("node", |n| n.ty.name()).to_owned();
        return Some(Manip::Plane { handle, mapping, press: pointer.screen, ty });
    };
    if let Handle::Locked(reason) = store.handle(id, leaf("transform3d.pos")) {
        info!("{id}.transform3d.pos locked for this drag: {reason}");
        return None;
    }
    let (_, cam, cam_tf, rect, to_camera) = cameras.get(camera).ok()?;
    let local = crate::bevy_app::scene3d::to_camera_px(pointer.screen, rect, to_camera);
    // A camera-facing plane through the object; Ctrl drags on the ground.
    let normal = if pointer.modifiers.ctrl { Vec3::Y } else { cam_tf.forward().as_vec3() }.normalize_or_zero();
    // The node's world position from the store, as its entity carries it.
    let [px, py, pz] = store.world_affine3(id).translation();
    let at = Vec3::new(px as f32, py as f32, pz as f32);
    let mut origin = at;
    let mut grab_offset = Vec3::ZERO;
    if let Some(ray) = crate::bevy_app::picking::camera_ray_from_window_px(cam, cam_tf, local)
        && let Some(hit) = intersect_ray_plane(ray, origin, normal)
    {
        grab_offset = at - hit;
        origin = hit;
    }
    Some(Manip::World(World { id, camera, plane_origin: origin, plane_normal: normal, grab_offset, press: pointer.screen }))
}

/// Where a ray meets a plane (a point and a normal), in front of the ray.
pub(crate) fn intersect_ray_plane(ray: Ray3d, plane_point: Vec3, plane_normal: Vec3) -> Option<Vec3> {
    let denom = ray.direction.dot(plane_normal);
    if denom.abs() < 1e-5 {
        return None;
    }
    let t = (plane_point - ray.origin).dot(plane_normal) / denom;
    (t >= 0.0).then(|| ray.origin + ray.direction * t)
}

// ---- The 3D transform gizmo --------------------------------------------------

/// The transform gizmo (`transform-gizmo`) over the selected meshes, in the
/// view they were selected in: a 3D panel, or a 3D embed inside a 2D view.
/// The crate does the picking and the math; its results preview through the
/// store like any drag (`Manip::Gizmo`), and its triangles draw in an
/// overlay vector view (`draw_gizmo_overlay`).
pub(crate) struct Gizmo3d {
    gizmo: transform_gizmo::Gizmo,
    pub panel: String,
    pub camera: Entity,
    pub targets: Vec<NodeId>,
    /// Window px → the camera's target px.
    pub to_camera: Affine,
    /// Where it can be hovered, and is drawn: window px.
    pub clip: kurbo::Rect,
    /// This frame's interaction result, for `Manip::Gizmo` to preview.
    pub last: Option<(transform_gizmo::GizmoResult, Vec<transform_gizmo::math::Transform>)>,
}

/// The gizmo's modes: translation along axes and in planes, rotation about
/// the axes and the view, scaling along the axes. Each family appears only
/// while every target's leaf for it has a handle (a clip-driven rotation
/// shows no rings).
fn modes(store: &Store, targets: &[NodeId]) -> transform_gizmo::EnumSet<transform_gizmo::GizmoMode> {
    use transform_gizmo::GizmoMode as M;
    let free = |path: &str| targets.iter().all(|&id| !matches!(store.handle(id, leaf(path)), Handle::Locked(_)));
    let mut m = transform_gizmo::EnumSet::empty();
    if free("transform3d.pos") {
        m |= M::TranslateX | M::TranslateY | M::TranslateZ | M::TranslateXY | M::TranslateXZ | M::TranslateYZ;
    }
    if free("transform3d.rot") {
        m |= M::all_rotate();
    }
    if free("transform3d.scale") {
        m |= M::ScaleX | M::ScaleY | M::ScaleZ;
    }
    m
}

/// A mesh's resolved transform, as the gizmo takes it. Meshes sit at the
/// top of their 3D composition, so local is world.
fn target_transform(store: &Store, id: NodeId) -> Option<transform_gizmo::math::Transform> {
    let v3 = |path: &str| store.resolved(id, leaf(path)).and_then(|v| match v {
        Value::Vec3(a) => Some(a),
        _ => None,
    });
    let [rx, ry, rz] = v3("transform3d.rot").unwrap_or([0.0; 3]);
    let q = Quat::from_euler(EulerRot::XYZ, rx as f32, ry as f32, rz as f32).as_dquat();
    let rotation = transform_gizmo::mint::Quaternion { v: transform_gizmo::mint::Vector3 { x: q.x, y: q.y, z: q.z }, s: q.w };
    let vec = |a: [f64; 3]| transform_gizmo::mint::Vector3 { x: a[0], y: a[1], z: a[2] };
    Some(transform_gizmo::math::Transform::from_scale_rotation_translation(vec(v3("transform3d.scale").unwrap_or([1.0; 3])), rotation, vec(v3("transform3d.pos")?)))
}

fn rows(m: bevy::math::DMat4) -> transform_gizmo::mint::RowMatrix4<f64> {
    let r = |i: usize| {
        let v = m.row(i);
        transform_gizmo::mint::Vector4 { x: v.x, y: v.y, z: v.z, w: v.w }
    };
    transform_gizmo::mint::RowMatrix4 { x: r(0), y: r(1), z: r(2), w: r(3) }
}

impl Gizmo3d {
    pub fn new(panel: String, camera: Entity, targets: Vec<NodeId>) -> Gizmo3d {
        Gizmo3d { gizmo: transform_gizmo::Gizmo::default(), panel, camera, targets, to_camera: Affine::IDENTITY, clip: kurbo::Rect::ZERO, last: None }
    }

    /// Configure for this frame's camera and targets, and feed it the
    /// pointer: `dragging` while a gizmo gesture holds the button. Leaves the
    /// result in `last`; `false` when the camera or a target is gone.
    pub fn frame(&mut self, store: &Store, pointer: &crate::PointerState, cameras: &DragCameras, clip: kurbo::Rect, dpr: f32, dragging: bool) -> bool {
        let Ok((_, camera, cam_tf, rect, to_camera)) = cameras.get(self.camera) else { return false };
        let Some(rect) = rect else { return false };
        self.to_camera = match to_camera {
            Some(p) => p.0,
            None => Affine::translate((-(rect.pos.x as f64), -(rect.pos.y as f64))),
        };
        self.clip = clip;
        let Some(transforms) = self.targets.iter().map(|&id| target_transform(store, id)).collect::<Option<Vec<_>>>() else { return false };
        let scale = (self.to_camera.determinant().abs().sqrt() as f32).max(1e-3);
        let config = transform_gizmo::GizmoConfig {
            view_matrix: rows(cam_tf.to_matrix().inverse().as_dmat4()),
            projection_matrix: rows(camera.clip_from_view().as_dmat4()),
            viewport: transform_gizmo::Rect::from_x_y_ranges(0.0..=rect.size.x as f32, 0.0..=rect.size.y as f32),
            modes: modes(store, &self.targets),
            // Its sizes are viewport (camera) px; these make them the
            // default's CSS px on screen. `pixels_per_point` only feathers.
            visuals: transform_gizmo::GizmoVisuals { gizmo_size: 75.0 * dpr * scale, stroke_width: 4.0 * dpr * scale, ..Default::default() },
            pixels_per_point: dpr * scale,
            ..Default::default()
        };
        self.gizmo.update_config(config);
        let at = self.to_camera * Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
        let over = clip.contains(Point::new(pointer.screen.x as f64, pointer.screen.y as f64));
        let interaction = transform_gizmo::GizmoInteraction {
            cursor_pos: (at.x as f32, at.y as f32),
            hovered: over || dragging,
            drag_started: pointer.just_pressed_left && over,
            dragging: pointer.buttons.left && (dragging || pointer.just_pressed_left),
        };
        self.last = self.gizmo.update(interaction, &transforms);
        true
    }

    /// Whether the pointer is over one of its handles.
    pub fn focused(&self) -> bool {
        self.gizmo.is_focused()
    }

    /// Its triangles in window px, merged into one path per colour.
    fn paths(&self) -> Vec<(peniko::Color, kurbo::BezPath)> {
        let data = self.gizmo.draw();
        let back = self.to_camera.inverse();
        let mut out: Vec<([u8; 4], kurbo::BezPath)> = Vec::new();
        for tri in data.indices.chunks_exact(3) {
            let v = |i: u32| back * Point::new(data.vertices[i as usize][0] as f64, data.vertices[i as usize][1] as f64);
            let (a, mut b, mut c) = (v(tri[0]), v(tri[1]), v(tri[2]));
            // One winding for all, so triangles that share an edge merge
            // into one area instead of leaving an antialiasing seam.
            if (b - a).cross(c - a) < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            let lin = data.colors[tri[0] as usize];
            // Linear colour to sRGB bytes.
            let srgb = |x: f32| {
                let x = x.clamp(0.0, 1.0);
                let s = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
                (s * 255.0).round() as u8
            };
            let key = [srgb(lin[0]), srgb(lin[1]), srgb(lin[2]), (lin[3].clamp(0.0, 1.0) * 255.0).round() as u8];
            let i = match out.iter().position(|(k, _)| *k == key) {
                Some(i) => i,
                None => {
                    out.push((key, kurbo::BezPath::new()));
                    out.len() - 1
                }
            };
            let path = &mut out[i].1;
            path.move_to(a);
            path.line_to(b);
            path.line_to(c);
            path.close_path();
        }
        out.into_iter().map(|(k, p)| (peniko::Color::from_rgba8(k[0], k[1], k[2], k[3]), p)).collect()
    }
}

/// The overlay view the gizmo draws in, over its panel.
#[derive(Component)]
pub(crate) struct GizmoOverlay;

/// Draw the gizmo in its own overlay vector view over its panel; gone with
/// the gizmo. The list is rebuilt every frame, and only a changed one
/// re-encodes (`DisplayListRebuild`).
pub(crate) fn draw_gizmo_overlay(
    mut commands: Commands,
    tool: Res<crate::bevy_app::select::SelectTool>,
    views: Res<PanelViews>,
    mut overlays: Query<(Entity, &mut crate::vector::DisplayList, &mut crate::vector::VectorView), With<GizmoOverlay>>,
) {
    let Some((g, view)) = tool.gizmo().and_then(|g| Some((g, views.get(&g.panel)?))) else {
        for (e, ..) in &overlays {
            commands.entity(e).despawn();
        }
        return;
    };
    let r = view.rect;
    let rect = crate::vector::VectorView { pos: UVec2::new(r.x.max(0.0) as u32, r.y.max(0.0) as u32), size: UVec2::new(r.w.max(1.0) as u32, r.h.max(1.0) as u32) };
    let Some((_, mut list, mut v)) = overlays.iter_mut().next() else {
        let e = commands.spawn((crate::vector::DisplayList::default(), GizmoOverlay, rect, crate::vector::Overlay, Name::new("gizmo overlay"))).id();
        commands.entity(e).insert(crate::vector::VectorLayer::screen(crate::vector::order::EDITOR_OVERLAY).in_view(e));
        return;
    };
    if *v != rect {
        *v = rect;
    }
    let paths = g.paths();
    list.rebuild(|b| {
        b.clipped(Affine::IDENTITY, g.clip, |b| {
            for (c, p) in paths {
                b.fill(Affine::IDENTITY, c, p);
            }
        });
    });
}
