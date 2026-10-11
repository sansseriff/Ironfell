//! World transforms inside the store (constraint-system.md §4.3).
//!
//! Bevy computes `GlobalTransform` for rendering; the solver and the gesture
//! code need the same world positions without a round trip through the ECS.
//! `world_affine` walks a node's parent chain, reading resolved values through
//! the same reader the reactive graph uses, so previews, bindings and clips
//! are honoured. Its composition order (`T · R · S`, parent before child) is
//! Bevy's `Transform::compute_affine` and `GlobalTransform` propagation
//! exactly; a test against `glam` keeps the two from drifting.

use crate::component::{ComponentKind, LeafPath};
use crate::document::Document;
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::value::Value;

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
/// `transform2d` component is the identity, as in the bridge.
pub fn local_affine(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Affine2 {
    if doc.component(id, ComponentKind::Transform2d).is_none() {
        return Affine2::IDENTITY;
    }
    let num = |field: &str, default: f64| {
        let path = LeafPath::new(ComponentKind::Transform2d, field).expect("declared field");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{LeafStruct, Transform2d};
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
                    components: vec![t.wrap()],
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
                    Op::Create { id: ids[0], ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), components: vec![] },
                    Op::Create {
                        id: ids[1],
                        ty: TypeId::lookup("group").unwrap(),
                        parent: Some(ids[0]),
                        order: OrderKey::first(),
                        components: vec![t.wrap()],
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
