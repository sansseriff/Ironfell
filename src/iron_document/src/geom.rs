//! Geometry inside the store: local and world transforms, local bounds, and
//! the revisions that say what moved (constraint-system.md §4.3; vision 10
//! §3, where Iron is the authority for world transforms and Bevy a consumer).
//!
//! `world_affine` walks a node's parent chain, reading resolved values through
//! the same reader the reactive graph uses, so previews, bindings and clips
//! are honoured. Its composition order (`T · R · S`, parent before child) is
//! Bevy's `Transform::compute_affine` and `GlobalTransform` propagation
//! exactly; a test against `glam` keeps the two from drifting.
//!
//! [`Revisions`] separates two kinds of change so a spatial index can keep a
//! group's contents in the group's own space: a **placement** change (the
//! node's own `transform2d` moved; its descendants' local-space bounds are
//! untouched) and a **local** change (the node's own geometry or paint
//! outsets moved). Rotating a group of ten thousand shapes is one placement
//! change, not ten thousand bounds. Structure (create, delete, reparent,
//! reorder) is a third, global, revision.

use crate::facet::{FacetKind, LeafPath};
use crate::document::Document;
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::op::Op;
use crate::value::Value;
use crate::Store;
use kurbo::{Rect, Shape as _};
use std::collections::{BTreeMap, BTreeSet};

/// A 2D affine map, kurbo's coefficient order: `x' = a·x + c·y + e`,
/// `y' = b·x + d·y + f`. In Bevy's `Affine3A` terms `(a, b)` is `x_axis`,
/// `(c, d)` is `y_axis`, and `(e, f)` the translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2(pub [f64; 6]);

impl Affine2 {
    pub const IDENTITY: Affine2 = Affine2([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(x: f64, y: f64) -> Affine2 {
        Affine2([1.0, 0.0, 0.0, 1.0, x, y])
    }

    /// Bevy's `from_scale_rotation_translation` restricted to the plane:
    /// scale, then rotate about z, then translate.
    pub fn from_trs(x: f64, y: f64, rot: f64, sx: f64, sy: f64) -> Affine2 {
        let (s, c) = rot.sin_cos();
        Affine2([c * sx, s * sx, -s * sy, c * sy, x, y])
    }

    /// `self ∘ other`: apply `other` first, then `self`. A parent's world
    /// affine `then` a child's local affine is the child's world affine.
    pub fn then(self, child: Affine2) -> Affine2 {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = child.0;
        Affine2([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }

    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    pub fn translation(&self) -> [f64; 2] {
        [self.0[4], self.0[5]]
    }

    pub fn determinant(&self) -> f64 {
        let [a, b, c, d, _, _] = self.0;
        a * d - b * c
    }

    /// `None` when the map is singular (a zero scale), which has no inverse.
    pub fn inverse(&self) -> Option<Affine2> {
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let [a, b, c, d, e, f] = self.0;
        let inv = 1.0 / det;
        let (ia, ib, ic, id) = (d * inv, -b * inv, -c * inv, a * inv);
        Some(Affine2([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)]))
    }

    /// Largest absolute difference between coefficients.
    pub fn max_diff(&self, other: &Affine2) -> f64 {
        self.0.iter().zip(other.0.iter()).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
    }
}

/// A node's own transform, read through `read`. A node without a
/// `transform2d` facet is the identity, as in the bridge.
pub fn local_affine(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Affine2 {
    if doc.facet(id, FacetKind::Transform2d).is_none() {
        return Affine2::IDENTITY;
    }
    let num = |field: &str, default: f64| {
        let path = LeafPath::new(FacetKind::Transform2d, field).expect("declared field");
        read(Leaf { node: id, path }).and_then(|v| v.as_f64()).unwrap_or(default)
    };
    Affine2::from_trs(num("x", 0.0), num("y", 0.0), num("rot", 0.0), num("sx", 1.0), num("sy", 1.0))
}

/// The node's local space to document 2D space: every live ancestor's local
/// affine, root first, then the node's own. Equal to Bevy's `GlobalTransform`
/// for the materialised entity.
pub fn world_affine(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Affine2 {
    let mut chain = vec![id];
    let mut cur = doc.live(id).and_then(|n| n.parent);
    while let Some(p) = cur {
        chain.push(p);
        cur = doc.live(p).and_then(|n| n.parent);
    }
    chain.iter().rev().fold(Affine2::IDENTITY, |acc, &n| acc.then(local_affine(doc, read, n)))
}

// ---------------------------------------------------------------------------
// 3D

/// A 3D affine map: a 3×3 matrix (column-major, `m[c][r]`) and a translation.
/// In Bevy's `Affine3A` terms `m[0]` is `x_axis`, `m[1]` `y_axis`, `m[2]`
/// `z_axis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3 {
    pub m: [[f64; 3]; 3],
    pub t: [f64; 3],
}

impl Affine3 {
    pub const IDENTITY: Affine3 = Affine3 { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    /// Bevy's `from_scale_rotation_translation` with the rotation given as
    /// Euler angles applied in `EulerRot::XYZ` order (`Quat::from_euler`):
    /// scale, then rotate, then translate.
    pub fn from_trs(pos: [f64; 3], rot: [f64; 3], scale: [f64; 3]) -> Affine3 {
        let (sx, cx) = rot[0].sin_cos();
        let (sy, cy) = rot[1].sin_cos();
        let (sz, cz) = rot[2].sin_cos();
        // R = Rx · Ry · Rz for EulerRot::XYZ (intrinsic), as glam composes it.
        let r = [
            [cy * cz, cx * sz + sx * sy * cz, sx * sz - cx * sy * cz],
            [-cy * sz, cx * cz - sx * sy * sz, sx * cz + cx * sy * sz],
            [sy, -sx * cy, cx * cy],
        ];
        let m = [
            [r[0][0] * scale[0], r[0][1] * scale[0], r[0][2] * scale[0]],
            [r[1][0] * scale[1], r[1][1] * scale[1], r[1][2] * scale[1]],
            [r[2][0] * scale[2], r[2][1] * scale[2], r[2][2] * scale[2]],
        ];
        Affine3 { m, t: pos }
    }

    /// `self ∘ child`: apply `child` first, then `self`.
    pub fn then(self, child: Affine3) -> Affine3 {
        let mul = |v: [f64; 3]| [
            self.m[0][0] * v[0] + self.m[1][0] * v[1] + self.m[2][0] * v[2],
            self.m[0][1] * v[0] + self.m[1][1] * v[1] + self.m[2][1] * v[2],
            self.m[0][2] * v[0] + self.m[1][2] * v[1] + self.m[2][2] * v[2],
        ];
        let t = mul(child.t);
        Affine3 { m: [mul(child.m[0]), mul(child.m[1]), mul(child.m[2])], t: [t[0] + self.t[0], t[1] + self.t[1], t[2] + self.t[2]] }
    }

    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let a = self.then(Affine3 { m: Affine3::IDENTITY.m, t: p });
        a.t
    }

    pub fn translation(&self) -> [f64; 3] {
        self.t
    }

    /// The twelve coefficients in Bevy's `Affine3A` order: the three axes
    /// then the translation.
    pub fn to_cols(&self) -> [f64; 12] {
        let m = self.m;
        [m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2], self.t[0], self.t[1], self.t[2]]
    }

    pub fn max_diff(&self, other: &Affine3) -> f64 {
        self.to_cols().iter().zip(other.to_cols().iter()).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
    }
}

/// A node's own 3D transform. A node without `transform3d` is the identity.
pub fn local_affine3(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Affine3 {
    if doc.facet(id, FacetKind::Transform3d).is_none() {
        return Affine3::IDENTITY;
    }
    let vec = |field: &str, default: [f64; 3]| {
        let path = LeafPath::new(FacetKind::Transform3d, field).expect("declared field");
        read(Leaf { node: id, path }).and_then(|v| v.as_vec3()).unwrap_or(default)
    };
    Affine3::from_trs(vec("pos", [0.0; 3]), vec("rot", [0.0; 3]), vec("scale", [1.0; 3]))
}

/// The node's local space to its composition's 3D space: every live
/// ancestor's local 3D affine, root first, then the node's own.
pub fn world_affine3(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Affine3 {
    let mut chain = vec![id];
    let mut cur = doc.live(id).and_then(|n| n.parent);
    while let Some(p) = cur {
        chain.push(p);
        cur = doc.live(p).and_then(|n| n.parent);
    }
    chain.iter().rev().fold(Affine3::IDENTITY, |acc, &n| acc.then(local_affine3(doc, read, n)))
}

// ---------------------------------------------------------------------------
// Local bounds

/// A drawable node's extent in its own local space, before its transform:
/// the geometry, and how far paint (strokes, shadows, a slider's knob) and
/// hit-testing (a path's grab margin) reach beyond it. Inflate in local space
/// before mapping, so a nonuniform scale scales the margins too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalBounds {
    pub geometry: Rect,
    pub paint_pad: f64,
    pub hit_pad: f64,
}

/// How near (local px) the pointer must come to a path's line to hit it.
pub const PATH_HIT: f64 = 5.0;

/// Conservative local paint outsets. Geometry alone trims centred strokes at
/// a frame boundary; the miter limit also covers round and bevel joins and
/// square and round caps; a shadow reaches its offset plus three standard
/// deviations (half the authored blur radius each).
pub fn paint_padding(store: &Store, id: NodeId) -> f64 {
    let read = |leaf: &str| store.resolved(id, leaf.parse().expect("declared leaf"));
    let num = |leaf: &str, default: f64| read(leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(default);
    let mut pad = if read("stroke.color").is_some() { num("stroke.width", 1.0) * 0.5 * num("stroke.miter", 4.0).max(4.0) } else { 0.0 };
    if read("shadow.color").is_some() {
        pad += num("shadow.dx", 0.0).abs().max(num("shadow.dy", 4.0).abs()) + 3.0 * (num("shadow.blur", 4.0).max(0.0) * 0.5);
    }
    if store.document().live(id).is_some_and(|n| n.ty.name() == "slider") {
        pad = pad.max((num("size.h", 24.0) * 0.5).min(12.0) + 3.0);
    }
    pad
}

/// The local bounds of a drawable 2D node (`rect`, `ellipse`, `slider`,
/// `path`, `text`), or of a 3D embed's on-screen frame. `None` for a group, an
/// embed of a 2D composition (its region is the source's content, which a
/// spatial scene knows), and anything else.
pub fn local_bounds(store: &Store, id: NodeId) -> Option<LocalBounds> {
    let doc = store.document();
    let node = doc.live(id)?;
    let num = |leaf: &str, default: f64| store.resolved(id, leaf.parse().expect("declared leaf")).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(default);
    let (geometry, hit_pad) = match node.ty.name() {
        "rect" => (Rect::new(0.0, 0.0, num("size.w", 100.0), num("size.h", 100.0)), 0.0),
        "slider" => (Rect::new(0.0, 0.0, num("size.w", 200.0), num("size.h", 24.0)), 0.0),
        "ellipse" => {
            let outline = crate::shape::outline(store, id)?;
            (crate::shape::to_bez(&outline).bounding_box(), 0.0)
        }
        "path" => (crate::contour::geometry(store, id).bounds(), PATH_HIT),
        "text" => (crate::text::measure(store, id).frame, 0.0),
        // A bare empty is its marker (the bridge draws a box with a cross);
        // one with children is a container, like a group, its bounds theirs.
        "empty" if doc.children(id).is_empty() => (Rect::new(-6.0, -6.0, 6.0, 6.0), 2.0),
        "embed" => {
            let (_, _, w, h) = crate::composition::embed_3d_extent(store, id)?;
            return Some(LocalBounds { geometry: Rect::new(0.0, 0.0, w, h), paint_pad: 2.0, hit_pad: 0.0 });
        }
        _ => return None,
    };
    let geometry = Rect::new(geometry.x0.min(geometry.x1), geometry.y0.min(geometry.y1), geometry.x0.max(geometry.x1), geometry.y0.max(geometry.y1));
    Some(LocalBounds { geometry, paint_pad: paint_padding(store, id), hit_pad })
}

// ---------------------------------------------------------------------------
// Revisions

/// What one leaf's change means for spatial bookkeeping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryEffect {
    /// The node's own placement in its parent moved.
    Placement(NodeId),
    /// The node's own local geometry or paint outset moved.
    Local(NodeId),
    /// Nothing an index cares about (a colour, a 3D transform, a name).
    None,
}

/// The `path` a `contour` or `point` belongs to, if it is inside one.
fn enclosing_path(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut at = doc.live(id)?.parent;
    while let Some(p) = at.and_then(|p| doc.live(p)) {
        if p.ty.name() == "path" {
            return Some(p.id);
        }
        at = p.parent;
    }
    None
}

/// Classify a resolved-value change. A point's position is part of its
/// path's outline, not a placement of its own.
pub fn effect_of(doc: &Document, leaf: Leaf) -> GeometryEffect {
    let Some(node) = doc.live(leaf.node) else { return GeometryEffect::None };
    let ty = node.ty.name();
    match leaf.path.facet {
        FacetKind::Transform2d if matches!(ty, "point" | "contour") => enclosing_path(doc, leaf.node).map_or(GeometryEffect::None, GeometryEffect::Local),
        FacetKind::Transform2d => GeometryEffect::Placement(leaf.node),
        FacetKind::Handles | FacetKind::Contour => enclosing_path(doc, leaf.node).map_or(GeometryEffect::None, GeometryEffect::Local),
        FacetKind::Size | FacetKind::PathShape | FacetKind::Text | FacetKind::Font | FacetKind::Radius | FacetKind::Arc | FacetKind::Stroke | FacetKind::Shadow | FacetKind::Frame | FacetKind::Embed | FacetKind::EmbedCamera => {
            GeometryEffect::Local(leaf.node)
        }
        // A camera's aspect shapes every embed that looks through it; rare,
        // and structure-free, so the embeds are found by the scene on demand.
        FacetKind::Camera => GeometryEffect::Local(leaf.node),
        _ => GeometryEffect::None,
    }
}

/// The changes since the last take, classified. `structure` means the tree's
/// shape changed and painter order, scopes and chains must be rebuilt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometryChanges {
    pub structure: bool,
    pub placement: BTreeSet<NodeId>,
    pub local: BTreeSet<NodeId>,
}

impl GeometryChanges {
    pub fn is_empty(&self) -> bool {
        !self.structure && self.placement.is_empty() && self.local.is_empty()
    }
}

/// Monotonic counters per node and for the tree, bumped as changes are
/// taken. A consumer stamps what it derived with the revisions it read and
/// re-derives when they move; a chain's placement stamp is the sum of its
/// nodes' placement revisions, so a cached world affine stays valid until
/// some ancestor moves.
#[derive(Clone, Debug, Default)]
pub struct Revisions {
    structure: u64,
    placement: BTreeMap<NodeId, u64>,
    local: BTreeMap<NodeId, u64>,
    /// Leaves set by transactions since the last take (the reactive graph
    /// reports only re-evaluated bindings and previews).
    pending_leaves: BTreeSet<Leaf>,
    pending_structure: bool,
}

impl Revisions {
    pub fn structure(&self) -> u64 {
        self.structure
    }

    pub fn placement(&self, id: NodeId) -> u64 {
        self.placement.get(&id).copied().unwrap_or(0)
    }

    pub fn local(&self, id: NodeId) -> u64 {
        self.local.get(&id).copied().unwrap_or(0)
    }

    /// The placement revisions of `id` and its live ancestors, summed: a
    /// cheap stamp for anything derived from the node's world affine.
    pub fn chain_stamp(&self, doc: &Document, id: NodeId) -> u64 {
        let mut stamp = 0u64;
        let mut at = Some(id);
        while let Some(n) = at {
            stamp = stamp.wrapping_add(self.placement(n));
            at = doc.live(n).and_then(|n| n.parent);
        }
        stamp
    }

    /// Record a transaction's ops (requested, derived, and inverse: the
    /// union names everything an undo or redo touches).
    pub(crate) fn note_ops<'a>(&mut self, ops: impl IntoIterator<Item = &'a Op>) {
        for op in ops {
            match op {
                Op::Set { id, path, .. } => {
                    self.pending_leaves.insert(Leaf { node: *id, path: *path });
                }
                Op::Create { .. } | Op::Delete { .. } | Op::Reparent { .. } | Op::Reorder { .. } | Op::AddFacet { .. } | Op::RemoveFacet { .. } => self.pending_structure = true,
                _ => {}
            }
        }
    }

    pub(crate) fn note_structure(&mut self) {
        self.pending_structure = true;
    }

    /// Classify everything pending plus `touched` from the reactive graph,
    /// bump the counters, and hand the change set to the caller.
    pub(crate) fn take(&mut self, doc: &Document, touched: BTreeSet<Leaf>) -> GeometryChanges {
        let mut out = GeometryChanges::default();
        if std::mem::take(&mut self.pending_structure) {
            self.structure += 1;
            out.structure = true;
        }
        for leaf in std::mem::take(&mut self.pending_leaves).into_iter().chain(touched) {
            match effect_of(doc, leaf) {
                GeometryEffect::Placement(n) => {
                    out.placement.insert(n);
                }
                GeometryEffect::Local(n) => {
                    out.local.insert(n);
                }
                GeometryEffect::None => {}
            }
        }
        for &n in &out.placement {
            *self.placement.entry(n).or_insert(0) += 1;
        }
        for &n in &out.local {
            *self.local.entry(n).or_insert(0) += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facet::{LeafStruct, Transform2d};
    use crate::op::{Actor, Op, TransactionInput};
    use crate::order::OrderKey;
    use crate::registry::TypeId;
    use crate::value::Slot;
    use crate::Store;

    /// Bevy's own arithmetic for the same tree: `Transform::compute_affine`
    /// is `Affine3A::from_scale_rotation_translation`, and propagation is
    /// `parent.affine() * child.affine()`.
    fn bevy_world(chain: &[(f64, f64, f64, f64, f64)]) -> Affine2 {
        let mut acc = glam::Affine3A::IDENTITY;
        for &(x, y, rot, sx, sy) in chain {
            let local = glam::Affine3A::from_scale_rotation_translation(
                glam::Vec3::new(sx as f32, sy as f32, 1.0),
                glam::Quat::from_rotation_z(rot as f32),
                glam::Vec3::new(x as f32, y as f32, 0.0),
            );
            acc = acc * local;
        }
        Affine2([
            acc.matrix3.x_axis.x as f64,
            acc.matrix3.x_axis.y as f64,
            acc.matrix3.y_axis.x as f64,
            acc.matrix3.y_axis.y as f64,
            acc.translation.x as f64,
            acc.translation.y as f64,
        ])
    }

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 11) as f64) / ((1u64 << 53) as f64)
    }

    #[test]
    fn world_affine_matches_bevy_global_transform() {
        let mut seed = 0x5EED_1234;
        for case in 0..200 {
            let depth = 1 + case % 4;
            let mut chain = Vec::new();
            for _ in 0..depth {
                let r = |s: &mut u64, lo: f64, hi: f64| lo + (hi - lo) * lcg(s);
                chain.push((
                    r(&mut seed, -800.0, 800.0),
                    r(&mut seed, -600.0, 600.0),
                    r(&mut seed, -3.5, 3.5),
                    r(&mut seed, 0.25, 3.0),
                    r(&mut seed, 0.25, 3.0),
                ));
            }
            let mut store = Store::new();
            let ids = store.document().next_ids(depth);
            let mut ops = Vec::new();
            for (i, &(x, y, rot, sx, sy)) in chain.iter().enumerate() {
                let mut t = Transform2d::default();
                t.x = Slot::Const(Value::Number(x));
                t.y = Slot::Const(Value::Number(y));
                t.rot = Slot::Const(Value::Number(rot));
                t.sx = Slot::Const(Value::Number(sx));
                t.sy = Slot::Const(Value::Number(sy));
                ops.push(Op::Create {
                    id: ids[i],
                    ty: TypeId::lookup("group").unwrap(),
                    parent: (i > 0).then(|| ids[i - 1]),
                    order: OrderKey::first(),
                    facets: vec![t.wrap()],
                });
            }
            store
                .apply(TransactionInput { label: "tree".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false })
                .unwrap();
            let ours = store.world_affine(ids[depth - 1]);
            let bevy = bevy_world(&chain);
            // Bevy works in f32 (about 7 significant digits), so agreement is
            // to f32 precision relative to the largest coefficient: far
            // inside a pixel for any document-sized transform.
            let scale = ours.0.iter().fold(1.0f64, |m, v| m.max(v.abs()));
            assert!(ours.max_diff(&bevy) <= 1e-5 * scale, "case {case}: {ours:?} vs bevy {bevy:?}");
        }
    }

    #[test]
    fn affine3_matches_bevy_euler_xyz_and_composition() {
        let mut seed = 0x3D_5EED;
        for case in 0..200 {
            let r = |s: &mut u64, lo: f64, hi: f64| lo + (hi - lo) * lcg(s);
            let depth = 1 + case % 3;
            let mut ours = Affine3::IDENTITY;
            let mut bevy = glam::Affine3A::IDENTITY;
            for _ in 0..depth {
                let pos = [r(&mut seed, -10.0, 10.0), r(&mut seed, -10.0, 10.0), r(&mut seed, -10.0, 10.0)];
                let rot = [r(&mut seed, -3.0, 3.0), r(&mut seed, -3.0, 3.0), r(&mut seed, -3.0, 3.0)];
                let scale = [r(&mut seed, 0.3, 2.0), r(&mut seed, 0.3, 2.0), r(&mut seed, 0.3, 2.0)];
                ours = ours.then(Affine3::from_trs(pos, rot, scale));
                let local = glam::Affine3A::from_scale_rotation_translation(
                    glam::Vec3::new(scale[0] as f32, scale[1] as f32, scale[2] as f32),
                    glam::Quat::from_euler(glam::EulerRot::XYZ, rot[0] as f32, rot[1] as f32, rot[2] as f32),
                    glam::Vec3::new(pos[0] as f32, pos[1] as f32, pos[2] as f32),
                );
                bevy = bevy * local;
            }
            let b = Affine3 {
                m: [bevy.matrix3.x_axis.to_array().map(f64::from), bevy.matrix3.y_axis.to_array().map(f64::from), bevy.matrix3.z_axis.to_array().map(f64::from)],
                t: bevy.translation.to_array().map(f64::from),
            };
            let scale = ours.to_cols().iter().fold(1.0f64, |m, v| m.max(v.abs()));
            assert!(ours.max_diff(&b) <= 2e-5 * scale, "case {case}: {ours:?} vs {b:?}");
            let p = [1.0, 2.0, 3.0];
            let q = ours.apply(p);
            let bq = bevy.transform_point3(glam::Vec3::new(1.0, 2.0, 3.0));
            assert!((q[0] - bq.x as f64).abs() < 2e-4 * scale && (q[2] - bq.z as f64).abs() < 2e-4 * scale, "{q:?} vs {bq:?}");
        }
    }

    #[test]
    fn affine_algebra() {
        let t = Affine2::from_trs(10.0, 20.0, std::f64::consts::FRAC_PI_2, 2.0, 1.0);
        let p = t.apply([1.0, 0.0]);
        assert!((p[0] - 10.0).abs() < 1e-12 && (p[1] - 22.0).abs() < 1e-12, "{p:?}");
        let back = t.inverse().unwrap().apply(p);
        assert!((back[0] - 1.0).abs() < 1e-12 && back[1].abs() < 1e-12, "{back:?}");
        assert_eq!(Affine2::IDENTITY.then(t), t);
        assert_eq!(t.then(Affine2::IDENTITY), t);
        assert!(Affine2::from_trs(0.0, 0.0, 0.0, 0.0, 1.0).inverse().is_none());
        let a = Affine2::translate(5.0, 0.0);
        let b = Affine2::from_trs(0.0, 0.0, std::f64::consts::PI, 1.0, 1.0);
        // a.then(b): rotate first, then translate: (1,0) -> (-1,0) -> (4,0).
        let q = a.then(b).apply([1.0, 0.0]);
        assert!((q[0] - 4.0).abs() < 1e-12 && q[1].abs() < 1e-12, "{q:?}");
    }

    /// A rect under a group under a root group; returns (root, group, rect).
    fn nested(store: &mut Store) -> [NodeId; 3] {
        use crate::facet::Size;
        let ids = store.document().next_ids(3);
        let mut t = Transform2d::default();
        t.x = Slot::Const(Value::Number(10.0));
        let mut size = Size::default();
        size.w = Slot::Const(Value::Number(20.0));
        size.h = Slot::Const(Value::Number(30.0));
        store
            .apply(TransactionInput {
                label: "tree".into(),
                actor: Actor::System,
                ts: 0,
                idempotency_key: None,
                ops: vec![
                    Op::Create { id: ids[0], ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap()] },
                    Op::Create { id: ids[1], ty: TypeId::lookup("group").unwrap(), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![t.wrap()] },
                    Op::Create { id: ids[2], ty: TypeId::lookup("rect").unwrap(), parent: Some(ids[1]), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), size.wrap()] },
                ],
                exact: false,
            })
            .unwrap();
        [ids[0], ids[1], ids[2]]
    }

    #[test]
    fn revisions_separate_placement_from_local_and_structure() {
        let mut store = Store::new();
        let [root, group, rect] = nested(&mut store);
        let first = store.take_geometry_changes();
        assert!(first.structure, "creation is structure");
        let s0 = store.revisions().structure();
        assert!(store.take_geometry_changes().is_empty(), "nothing moved since");

        // Rotating the group is one placement change; the rect's local
        // revision and the chain stamp of anything outside the group stay.
        let rect_local = store.revisions().local(rect);
        let root_stamp = store.revisions().chain_stamp(store.document(), root);
        store
            .apply(TransactionInput {
                label: "rotate".into(),
                actor: Actor::Human,
                ts: 0,
                idempotency_key: None,
                ops: vec![Op::Set { id: group, path: "transform2d.rot".parse().unwrap(), slot: Slot::Const(Value::Number(0.5)) }],
                exact: false,
            })
            .unwrap();
        let c = store.take_geometry_changes();
        assert_eq!((c.structure, c.placement.len(), c.local.len()), (false, 1, 0), "{c:?}");
        assert!(c.placement.contains(&group));
        assert_eq!(store.revisions().local(rect), rect_local);
        assert_eq!(store.revisions().chain_stamp(store.document(), root), root_stamp);
        assert_ne!(store.revisions().chain_stamp(store.document(), rect), 0, "the rect's chain includes the moved group");
        assert_eq!(store.revisions().structure(), s0);

        // A size preview is a local change of the rect, through the graph.
        store.preview(rect, "size.w".parse().unwrap(), Value::Number(99.0));
        let c = store.take_geometry_changes();
        assert_eq!((c.placement.len(), c.local.len()), (0, 1), "{c:?}");
        assert!(c.local.contains(&rect));
        assert_eq!(store.local_bounds(rect).unwrap().geometry, kurbo::Rect::new(0.0, 0.0, 99.0, 30.0));

        // Undo is structure-free but moves the group back: a placement.
        store.cancel_gesture();
        let _ = store.take_geometry_changes();
        store.undo(0).unwrap().unwrap();
        let c = store.take_geometry_changes();
        assert!(!c.structure && c.placement.contains(&group), "{c:?}");
    }

    #[test]
    fn a_points_move_is_its_paths_local_change() {
        use crate::facet::PathShape;
        let mut store = Store::new();
        let ids = store.document().next_ids(4);
        let [path, contour, point, other] = [ids[0], ids[1], ids[2], ids[3]];
        let mut at = Transform2d::default();
        at.x = Slot::Const(Value::Number(5.0));
        store
            .apply(TransactionInput {
                label: "path".into(),
                actor: Actor::System,
                ts: 0,
                idempotency_key: None,
                ops: vec![
                    Op::Create { id: path, ty: TypeId::lookup("path").unwrap(), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), PathShape::default().wrap()] },
                    Op::Create { id: contour, ty: TypeId::lookup("contour").unwrap(), parent: Some(path), order: OrderKey::first(), facets: vec![crate::facet::Contour::default().wrap()] },
                    Op::Create { id: point, ty: TypeId::lookup("point").unwrap(), parent: Some(contour), order: OrderKey::first(), facets: vec![at.clone().wrap(), crate::facet::Handles::default().wrap()] },
                    Op::Create { id: other, ty: TypeId::lookup("point").unwrap(), parent: Some(contour), order: OrderKey::after(&OrderKey::first()), facets: vec![at.wrap(), crate::facet::Handles::default().wrap()] },
                ],
                exact: false,
            })
            .unwrap();
        let _ = store.take_geometry_changes();
        store.preview(point, "transform2d.y".parse().unwrap(), Value::Number(40.0));
        let c = store.take_geometry_changes();
        assert_eq!(c.placement.len(), 0, "{c:?}");
        assert_eq!(c.local.iter().copied().collect::<Vec<_>>(), vec![path]);
        let b = store.local_bounds(path).unwrap();
        assert_eq!(b.hit_pad, PATH_HIT);
        assert_eq!(b.geometry, kurbo::Rect::new(5.0, 0.0, 5.0, 40.0));
    }

    #[test]
    fn local_bounds_follow_shape_type_and_paint() {
        use crate::facet::{Radius, Shadow, Stroke};
        let mut store = Store::new();
        let ids = store.document().next_ids(2);
        let mut r = Radius::default();
        r.rx = Slot::Const(Value::Number(10.0));
        r.ry = Slot::Const(Value::Number(4.0));
        let mut stroke = Stroke::default();
        stroke.width = Slot::Const(Value::Number(6.0));
        let mut shadow = Shadow::default();
        shadow.dx = Slot::Const(Value::Number(2.0));
        shadow.dy = Slot::Const(Value::Number(-7.0));
        shadow.blur = Slot::Const(Value::Number(4.0));
        store
            .apply(TransactionInput {
                label: "shapes".into(),
                actor: Actor::System,
                ts: 0,
                idempotency_key: None,
                ops: vec![
                    Op::Create { id: ids[0], ty: TypeId::lookup("ellipse").unwrap(), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), r.wrap(), stroke.wrap()] },
                    Op::Create { id: ids[1], ty: TypeId::lookup("rect").unwrap(), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), crate::facet::Size::default().wrap(), shadow.wrap()] },
                ],
                exact: false,
            })
            .unwrap();
        let e = store.local_bounds(ids[0]).unwrap();
        assert!((e.geometry.x0 + 10.0).abs() < 1e-9 && (e.geometry.y1 - 4.0).abs() < 1e-9, "{e:?}");
        assert_eq!(e.paint_pad, 6.0 * 0.5 * 4.0, "stroke: half width times the miter limit");
        let r = store.local_bounds(ids[1]).unwrap();
        assert_eq!(r.geometry, kurbo::Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(r.paint_pad, 7.0 + 3.0 * 2.0, "shadow: the larger offset plus three standard deviations");
        assert!(store.local_bounds(store.document().next_id()).is_none());
    }

    #[test]
    fn world_affine_reads_previews_and_skips_missing_transforms() {
        let mut store = Store::new();
        let ids = store.document().next_ids(2);
        let mut t = Transform2d::default();
        t.x = Slot::Const(Value::Number(100.0));
        store
            .apply(TransactionInput {
                label: "tree".into(),
                actor: Actor::System,
                ts: 0,
                idempotency_key: None,
                ops: vec![
                    // A group without transform2d is the identity.
                    Op::Create { id: ids[0], ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![] },
                    Op::Create {
                        id: ids[1],
                        ty: TypeId::lookup("group").unwrap(),
                        parent: Some(ids[0]),
                        order: OrderKey::first(),
                        facets: vec![t.wrap()],
                    },
                ],
                exact: false,
            })
            .unwrap();
        assert_eq!(store.world_affine(ids[1]).translation(), [100.0, 0.0]);
        store.preview(ids[1], "transform2d.x".parse().unwrap(), Value::Number(7.0));
        assert_eq!(store.world_affine(ids[1]).translation(), [7.0, 0.0], "previews are honoured");
    }
}
