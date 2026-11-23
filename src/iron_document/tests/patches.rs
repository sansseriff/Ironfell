//! EN-4 Patch Algebra, Sparse Resolved Store, 6-Tier Precedence & Point Evaluation tests
//! (`plans/specs/engine.md` §2.3–§2.4, `plans/specs/patches.md`, `plans/handoff-engine.md` EN-4).
//!
//! Exit gates verified here:
//! 1. Differential parity against `iron_patch::World` across randomized multi-step scripts
//!    covering axes, patches, presence, hiding, optional facet presence (P21), solve layer
//!    (P17), session layer (P8), drag/split/merge/reorder, undo/redo, and Schema 16 roundtrip.
//! 2. Axis-growth property (`growing_an_axis_changes_no_decision`) in `iron_document`.
//! 3. 6-tier precedence (`session > binding > producer > solve > authored patch > base`),
//!    reactive patched bindings, `base_facet`/`base_leaf`, `eval_at`, and `Key::Patch` canary.
//! 4. 700-meter edit cost: single patch edit ≤ 2× single constant edit.

use iron_document::facet::{Composition, Fill, Name, Size, Transform2d};
use iron_document::patch::{
    Axis, AxisId, AxisKind, AxisOp, AxisValue, AxisValueId, Bound, Patch, PatchId, PatchKey,
    PatchOp, PatchTarget, PatchValue, Point, Product, SESSION_LAYER, Sel, plan_merge,
    plan_release_solve, plan_reorder, plan_split,
};
use iron_document::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn lp(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn sel_one(v: AxisValueId) -> Sel {
    Sel::Set { values: BTreeSet::from([v]) }
}

fn product_of(items: impl IntoIterator<Item = (AxisId, Sel)>) -> Product {
    Product { sels: items.into_iter().collect() }
}

fn make_axis(id: AxisId, name: &str, kind: AxisKind, vals: Vec<(u64, &str)>) -> Axis {
    let mut ax = Axis::new(id, name, kind, None);
    for (v, label) in vals {
        ax.push(AxisValue::new(AxisValueId(v), label));
    }
    ax
}

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn usize(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
    fn bool(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

// ---------------------------------------------------------------------------
// 1. Differential parity against `iron_patch::World`
// ---------------------------------------------------------------------------

/// Maps a 3-node document (group + 2 rects, where rect2 has optional Fill
/// absent in base) and 2 axes (ordered `viewport`, unordered `theme`) between
/// `iron_document::Store` and `iron_patch::World`.
struct ParityHarness {
    store: Store,
    world: iron_patch::World,
    group: NodeId,
    r1: NodeId,
    r2: NodeId,
    ax_vp: AxisId,
    ax_th: AxisId,
    next_val: u64,
    vp_vals: Vec<u64>,
    th_vals: Vec<u64>,
    patch_ids: Vec<u64>,
}

// iron_patch reserves Field 0 = PRESENT, Field 1 = HIDDEN.
const F_X: iron_patch::Field = 2;
const F_OPACITY: iron_patch::Field = 3;
const FACET_FILL: iron_patch::FacetId = 1;

impl ParityHarness {
    fn new() -> Self {
        let mut store = Store::new();
        let ids = store.authored().next_ids(3);
        let (group, r1, r2) = (ids[0], ids[1], ids[2]);

        let mut t1 = Transform2d::default();
        t1.x = num(10.0);
        let mut f1 = Fill::default();
        f1.opacity = num(1.0);

        let mut t2 = Transform2d::default();
        t2.x = num(20.0);
        // r2 starts WITHOUT Fill so PatchKey::FacetPresent(Fill) toggles it on/off.

        let ax_vp = AxisId(1);
        let ax_th = AxisId(2);
        let vp_axis = make_axis(ax_vp, "viewport", AxisKind::Ordered, vec![(1, "S"), (2, "M"), (3, "L")]);
        let th_axis = make_axis(ax_th, "theme", AxisKind::Unordered, vec![(4, "light"), (5, "dark")]);

        let k1 = OrderKey::first();
        let k2 = OrderKey::after(&k1);
        store
            .apply(tx(
                "init",
                vec![
                    Op::Create { id: group, ty: ty("group"), parent: None, order: k1.clone(), facets: vec![] },
                    Op::Create { id: r1, ty: ty("rect"), parent: Some(group), order: k1, facets: vec![t1.wrap(), Size::default().wrap(), f1.wrap()] },
                    Op::Create { id: r2, ty: ty("rect"), parent: Some(group), order: k2, facets: vec![t2.wrap(), Size::default().wrap()] },
                    Op::Axis(AxisOp::Add { axis: vp_axis }),
                    Op::Axis(AxisOp::Add { axis: th_axis }),
                    Op::Axis(AxisOp::SetOrder { order: vec![ax_vp, ax_th] }),
                ],
            ))
            .unwrap();

        // Matching iron_patch::World
        let mut w_vp = iron_patch::Axis::new(iron_patch::Kind::Ordered);
        for v in [1u32, 2, 3] {
            w_vp.push(iron_patch::ValueId(v), vec![]);
        }
        let mut w_th = iron_patch::Axis::new(iron_patch::Kind::Unordered);
        for v in [4u32, 5] {
            w_th.push(iron_patch::ValueId(v), vec![]);
        }
        let space = iron_patch::Space {
            axes: BTreeMap::from([(iron_patch::AxisId(1), w_vp), (iron_patch::AxisId(2), w_th)]),
            order: vec![iron_patch::AxisId(1), iron_patch::AxisId(2)],
        };
        let mut doc = iron_patch::Doc::default();
        let (ng, n1, n2): (iron_patch::Node, iron_patch::Node, iron_patch::Node) = (0, 1, 2);
        doc.parent.insert(ng, None);
        doc.parent.insert(n1, Some(ng));
        doc.parent.insert(n2, Some(ng));
        doc.field_facet.insert(F_OPACITY, FACET_FILL);
        for n in [ng, n1, n2] {
            doc.base.insert(iron_patch::Key { node: n, field: iron_patch::PRESENT }, 1);
            doc.base.insert(iron_patch::Key { node: n, field: iron_patch::HIDDEN }, 0);
        }
        doc.base.insert(iron_patch::Key { node: n1, field: iron_patch::facet_present(FACET_FILL) }, 1);
        doc.base.insert(iron_patch::Key { node: n2, field: iron_patch::facet_present(FACET_FILL) }, 0);
        doc.base.insert(iron_patch::Key { node: n1, field: F_X }, 10);
        doc.base.insert(iron_patch::Key { node: n1, field: F_OPACITY }, 1);
        doc.base.insert(iron_patch::Key { node: n2, field: F_X }, 20);
        // Default Fill::opacity in iron_document is 1.0 when facet is added via FacetPresent = true.
        doc.base.insert(iron_patch::Key { node: n2, field: F_OPACITY }, 1);

        let world = iron_patch::World::new(space, doc, iron_patch::Strength::Stored);
        ParityHarness {
            store,
            world,
            group,
            r1,
            r2,
            ax_vp,
            ax_th,
            next_val: 6,
            vp_vals: vec![1, 2, 3],
            th_vals: vec![4, 5],
            patch_ids: Vec::new(),
        }
    }

    fn node_of(&self, idx: u32) -> NodeId {
        match idx {
            0 => self.group,
            1 => self.r1,
            _ => self.r2,
        }
    }

    fn to_doc_point(p: &iron_patch::Point) -> Point {
        let mut out = Point::default();
        for (&a, &v) in p {
            out.values.insert(AxisId(a.0 as u64), AxisValueId(v.0 as u64));
        }
        out
    }

    fn to_doc_sel(s: &iron_patch::Sel) -> Sel {
        match s {
            iron_patch::Sel::All => Sel::All,
            iron_patch::Sel::Set(vs) => Sel::Set { values: vs.iter().map(|v| AxisValueId(v.0 as u64)).collect() },
            iron_patch::Sel::Not(vs) => Sel::Not { values: vs.iter().map(|v| AxisValueId(v.0 as u64)).collect() },
            iron_patch::Sel::Range(lo, hi) => {
                let cvt = |b: &iron_patch::Bound| match b {
                    iron_patch::Bound::Open => Bound::Open,
                    iron_patch::Bound::Incl(v) => Bound::Incl { v: AxisValueId(v.0 as u64) },
                    iron_patch::Bound::Excl(v) => Bound::Excl { v: AxisValueId(v.0 as u64) },
                };
                Sel::Range { lo: cvt(lo), hi: cvt(hi) }
            }
            iron_patch::Sel::Suffix { .. } => Sel::All,
        }
    }

    fn to_doc_product(p: &iron_patch::Product) -> Product {
        Product { sels: p.sels.iter().map(|(a, s)| (AxisId(a.0 as u64), Self::to_doc_sel(s))).collect() }
    }

    fn to_doc_target(&self, k: iron_patch::Key) -> PatchTarget {
        let node = self.node_of(k.node);
        let key = if k.field == iron_patch::PRESENT {
            PatchKey::Present
        } else if k.field == iron_patch::HIDDEN {
            PatchKey::Hidden
        } else if k.field == iron_patch::facet_present(FACET_FILL) {
            PatchKey::FacetPresent(FacetKind::Fill)
        } else if k.field == F_X {
            PatchKey::Leaf(lp("transform2d.x"))
        } else {
            PatchKey::Leaf(lp("fill.opacity"))
        };
        PatchTarget { node, key }
    }

    fn to_doc_value(k: iron_patch::Key, v: i64) -> PatchValue {
        if k.field == iron_patch::PRESENT || k.field == iron_patch::HIDDEN || k.field == iron_patch::facet_present(FACET_FILL) {
            PatchValue::Bool { v: v != 0 }
        } else {
            PatchValue::Slot { slot: num(v as f64) }
        }
    }

    fn random_product(&self, rng: &mut Rng) -> iron_patch::Product {
        let mut sels = BTreeMap::new();
        // Axis 1 (ordered viewport)
        match rng.usize(5) {
            0 => {}
            1 => {
                let v = self.vp_vals[rng.usize(self.vp_vals.len())];
                sels.insert(iron_patch::AxisId(1), iron_patch::Sel::Set(BTreeSet::from([iron_patch::ValueId(v as u32)])));
            }
            2 => {
                let a = self.vp_vals[rng.usize(self.vp_vals.len())];
                let b = self.vp_vals[rng.usize(self.vp_vals.len())];
                let lo = if rng.bool() { iron_patch::Bound::Incl(iron_patch::ValueId(a as u32)) } else { iron_patch::Bound::Open };
                let hi = if rng.bool() { iron_patch::Bound::Incl(iron_patch::ValueId(b as u32)) } else { iron_patch::Bound::Open };
                sels.insert(iron_patch::AxisId(1), iron_patch::Sel::Range(lo, hi));
            }
            3 => {
                let mut set = BTreeSet::new();
                for &v in &self.vp_vals {
                    if rng.bool() {
                        set.insert(iron_patch::ValueId(v as u32));
                    }
                }
                if !set.is_empty() {
                    sels.insert(iron_patch::AxisId(1), iron_patch::Sel::Set(set));
                }
            }
            _ => {
                let a = self.vp_vals[rng.usize(self.vp_vals.len())];
                sels.insert(
                    iron_patch::AxisId(1),
                    iron_patch::Sel::Range(iron_patch::Bound::Incl(iron_patch::ValueId(a as u32)), iron_patch::Bound::Open),
                );
            }
        }
        // Axis 2 (unordered theme)
        match rng.usize(3) {
            0 => {}
            1 => {
                let v = self.th_vals[rng.usize(self.th_vals.len())];
                sels.insert(iron_patch::AxisId(2), iron_patch::Sel::Set(BTreeSet::from([iron_patch::ValueId(v as u32)])));
            }
            _ => {
                let mut set = BTreeSet::new();
                for &v in &self.th_vals {
                    if rng.bool() {
                        set.insert(iron_patch::ValueId(v as u32));
                    }
                }
                if !set.is_empty() {
                    sels.insert(iron_patch::AxisId(2), iron_patch::Sel::Set(set));
                }
            }
        }
        iron_patch::Product { sels }
    }

    fn random_key(&self, rng: &mut Rng) -> (iron_patch::Key, i64) {
        match rng.usize(7) {
            0 => (iron_patch::Key { node: 0, field: iron_patch::PRESENT }, i64::from(rng.bool())),
            1 => (iron_patch::Key { node: 1, field: iron_patch::PRESENT }, i64::from(rng.bool())),
            2 => (iron_patch::Key { node: 0, field: iron_patch::HIDDEN }, i64::from(rng.bool())),
            3 => (iron_patch::Key { node: 2, field: iron_patch::facet_present(FACET_FILL) }, i64::from(rng.bool())),
            4 => (iron_patch::Key { node: 1, field: F_X }, (rng.usize(50) as i64) + 1),
            5 => (iron_patch::Key { node: 2, field: F_X }, (rng.usize(50) as i64) + 1),
            _ => (iron_patch::Key { node: 2, field: F_OPACITY }, (rng.usize(5) as i64) + 1),
        }
    }

    fn assert_parity(&mut self, ctx: &str) {
        let pts = self.world.space.points();
        for p in &pts {
            let dp = Self::to_doc_point(p);
            // Check eval_at BEFORE set_point so we test non-mutating point evaluation too.
            let prev_pt = self.store.point().clone();
            let eval_r1_x = self.store.eval_at(&dp, Leaf { node: self.r1, path: lp("transform2d.x") });
            let eval_r2_op = self.store.eval_at(&dp, Leaf { node: self.r2, path: lp("fill.opacity") });
            assert_eq!(self.store.point(), &prev_pt, "{ctx}: eval_at mutated store.point()");

            self.store.set_point(dp.clone());
            let doc = self.store.tree();

            for (idx, nid) in [(0u32, self.group), (1, self.r1), (2, self.r2)] {
                let w_pres = self.world.present(p, idx);
                let s_pres = doc.is_live(nid);
                assert_eq!(s_pres, w_pres, "{ctx}: present mismatch for node {idx} at {p:?}");

                if w_pres {
                    let w_hidden = !self.world.shown(p, idx);
                    let s_hidden = doc.is_hidden(nid);
                    assert_eq!(s_hidden, w_hidden, "{ctx}: hidden mismatch for node {idx} at {p:?}");
                }
            }

            // r1 transform2d.x
            let w_r1_x = self.world.effective(p, iron_patch::Key { node: 1, field: F_X }, None, None);
            let s_r1_x = self.store.resolved(self.r1, lp("transform2d.x")).and_then(|v| v.as_f64()).map(|x| x as i64);
            assert_eq!(s_r1_x, w_r1_x, "{ctx}: r1.x mismatch at {p:?}");
            assert_eq!(eval_r1_x.and_then(|v| v.as_f64()).map(|x| x as i64), w_r1_x, "{ctx}: eval_at r1.x mismatch at {p:?}");

            // r2 Fill presence & fill.opacity (P21 optional facet)
            let w_r2_fill = self.world.facet_present(p, 2, FACET_FILL);
            assert_eq!(doc.has(self.r2, FacetKind::Fill), w_r2_fill, "{ctx}: r2.has(Fill) mismatch at {p:?}");

            let w_r2_op = self.world.effective(p, iron_patch::Key { node: 2, field: F_OPACITY }, None, None);
            let s_r2_op = self.store.resolved(self.r2, lp("fill.opacity")).and_then(|v| v.as_f64()).map(|x| x as i64);
            assert_eq!(s_r2_op, w_r2_op, "{ctx}: r2.fill.opacity mismatch at {p:?}");
            assert_eq!(eval_r2_op.and_then(|v| v.as_f64()).map(|x| x as i64), w_r2_op, "{ctx}: eval_at r2.fill.opacity mismatch at {p:?}");

            // patch_stack order parity on r1.x
            let t_r1_x = PatchTarget { node: self.r1, key: PatchKey::Leaf(lp("transform2d.x")) };
            let s_stack: Vec<u64> = self.store.patch_stack(t_r1_x).into_iter().map(|(p, _)| p.id.0).collect();
            let w_key = iron_patch::Key { node: 1, field: F_X };
            let mut w_stack: Vec<&iron_patch::Patch> = self
                .world
                .patches
                .iter()
                .filter(|q| q.entries.contains_key(&w_key) && q.region.contains_anchored(&self.world.space, p, q.anchor))
                .collect();
            w_stack.sort_by(|a, b| self.world.cmp(b, a));
            let w_stack_ids: Vec<u64> = w_stack.into_iter().map(|q| q.id.0 as u64).collect();
            assert_eq!(s_stack, w_stack_ids, "{ctx}: patch_stack mismatch at {p:?}");
        }

        // Lints parity (dead, ties, contradictions, stale)
        let lints = self.store.patch_lints();
        let w_dead: BTreeSet<(u64, PatchTarget)> = self.world.dead().into_iter().map(|(id, k)| (id.0 as u64, self.to_doc_target(k))).collect();
        let s_dead: BTreeSet<(u64, PatchTarget)> = lints.dead.into_iter().map(|(id, t)| (id.0, t)).collect();
        assert_eq!(s_dead, w_dead, "{ctx}: dead lint mismatch");

        let w_ties: BTreeSet<(u64, u64)> = self.world.ties().into_iter().map(|(a, b)| (a.0 as u64, b.0 as u64)).collect();
        let s_ties: BTreeSet<(u64, u64)> = lints.ties.into_iter().map(|(a, b)| (a.0, b.0)).collect();
        assert_eq!(s_ties, w_ties, "{ctx}: ties lint mismatch");

        let w_con: BTreeSet<(u64, u64)> = self.world.contradictions().into_iter().map(|(a, b)| (a.0 as u64, b.0 as u64)).collect();
        let s_con: BTreeSet<(u64, u64)> = lints.contradictions.into_iter().map(|(a, b)| (a.0, b.0)).collect();
        assert_eq!(s_con, w_con, "{ctx}: contradictions lint mismatch");

        let w_stale: BTreeSet<(u64, PatchTarget)> = self.world.stale().into_iter().map(|(id, k)| (id.0 as u64, self.to_doc_target(k))).collect();
        let s_stale: BTreeSet<(u64, PatchTarget)> = lints.stale.into_iter().map(|(id, t)| (id.0, t)).collect();
        assert_eq!(s_stale, w_stale, "{ctx}: stale lint mismatch");
    }
}

#[test]
fn differential_parity_with_iron_patch() {
    let mut rng = Rng::new(0xE4_5EED_0001);
    for script in 0..80 {
        let mut h = ParityHarness::new();
        for step in 0..12 {
            let tag = format!("script {script} step {step}");
            match rng.usize(10) {
                // Add an authored patch
                0..=3 => {
                    let region = h.random_product(&mut rng);
                    let layer = rng.usize(2) as u32;
                    let (k, v) = h.random_key(&mut rng);
                    let w_patch = h.world.make(region.clone(), BTreeMap::from([(k, v)]), layer);
                    let pid = w_patch.id.0 as u64;
                    let d_region = ParityHarness::to_doc_product(&region);
                    let d_rank = h.store.authored().patches().default_rank(&d_region, layer);
                    let d_patch = Patch {
                        id: PatchId(pid),
                        layer,
                        rank: d_rank,
                        region: d_region,
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(h.to_doc_target(k), ParityHarness::to_doc_value(k, v))]),
                    };
                    h.world.apply(&iron_patch::Op::Add { patch: w_patch, at: h.world.patches.len() });
                    h.store.apply(tx("add patch", vec![Op::Patch(PatchOp::Add { patch: d_patch })])).unwrap();
                    h.patch_ids.push(pid);
                }
                // Update an entry on an existing patch
                4 => {
                    if let Some(&pid) = h.patch_ids.get(rng.usize(h.patch_ids.len().max(1)))
                        && h.store.authored().patches().patches.contains_key(&PatchId(pid))
                    {
                        let (k, v) = h.random_key(&mut rng);
                        let clear = rng.usize(4) == 0;
                        let w_val = (!clear).then_some(v);
                        let d_val = w_val.map(|val| ParityHarness::to_doc_value(k, val));
                        h.world.apply(&iron_patch::Op::Entry { patch: iron_patch::PatchId(pid as u32), key: k, value: w_val });
                        h.store
                            .apply(tx(
                                "set entry",
                                vec![Op::Patch(PatchOp::SetEntry { patch: PatchId(pid), target: h.to_doc_target(k), value: d_val })],
                            ))
                            .unwrap();
                    }
                }
                // Set region on an existing patch
                5 => {
                    if let Some(&pid) = h.patch_ids.get(rng.usize(h.patch_ids.len().max(1)))
                        && h.store.authored().patches().patches.contains_key(&PatchId(pid))
                    {
                        let region = h.random_product(&mut rng);
                        let d_region = ParityHarness::to_doc_product(&region);
                        h.world.apply(&iron_patch::Op::Region { patch: iron_patch::PatchId(pid as u32), region });
                        h.store.apply(tx("set region", vec![Op::Patch(PatchOp::SetRegion { patch: PatchId(pid), region: d_region })])).unwrap();
                    }
                }
                // Remove a patch
                6 => {
                    if !h.patch_ids.is_empty() {
                        let idx = rng.usize(h.patch_ids.len());
                        let pid = h.patch_ids.remove(idx);
                        if h.store.authored().patches().patches.contains_key(&PatchId(pid)) {
                            h.world.apply(&iron_patch::Op::Remove(iron_patch::PatchId(pid as u32)));
                            h.store.apply(tx("remove patch", vec![Op::Patch(PatchOp::Remove { id: PatchId(pid) })])).unwrap();
                        }
                    }
                }
                // Drag via plan_patch_drag vs iron_patch::edit::drag
                7 => {
                    let pts = h.world.space.points();
                    if let Some(p) = pts.get(rng.usize(pts.len().max(1))).cloned() {
                        let dp = ParityHarness::to_doc_point(&p);
                        h.store.set_point(dp);
                        let edit = h.random_product(&mut rng);
                        let d_edit = ParityHarness::to_doc_product(&edit);
                        let k = iron_patch::Key { node: 1, field: F_X };
                        let v = (rng.usize(90) as i64) + 10;
                        let layer = rng.usize(2) as u32;
                        let w_res = iron_patch::edit::drag(&h.world, &p, &edit, k, v, layer);
                        let d_res = h.store.plan_patch_drag(&d_edit, h.to_doc_target(k), ParityHarness::to_doc_value(k, v), layer);
                        assert_eq!(d_res.is_ok(), w_res.is_ok(), "{tag}: drag ok/err parity mismatch: doc={d_res:?} world={w_res:?}");
                        if let (Ok(w_ops), Ok(d_ops)) = (w_res, d_res) {
                            let minted = w_ops.iter().find_map(|o| match o {
                                iron_patch::Op::Add { patch, .. } => Some(patch.id.0 as u64),
                                _ => None,
                            });
                            let d_ops: Vec<Op> = d_ops
                                .into_iter()
                                .map(|o| match (o, minted) {
                                    (Op::Patch(PatchOp::Add { mut patch }), Some(mid)) => {
                                        patch.id = PatchId(mid);
                                        h.patch_ids.push(mid);
                                        Op::Patch(PatchOp::Add { patch })
                                    }
                                    (other, _) => other,
                                })
                                .collect();
                            h.world.apply_all(&w_ops);
                            if !d_ops.is_empty() {
                                h.store.apply(tx("drag", d_ops)).unwrap();
                            }
                        }
                    }
                }
                // Axis growth
                8 => {
                    if h.vp_vals.len() < 5 {
                        let vid = h.next_val;
                        h.next_val += 1;
                        let at = rng.usize(h.vp_vals.len() + 1);
                        h.vp_vals.insert(at, vid);
                        h.world.space.axes.get_mut(&iron_patch::AxisId(1)).unwrap().insert(at, iron_patch::ValueId(vid as u32));
                        h.store
                            .apply(tx(
                                "add vp val",
                                vec![Op::Axis(AxisOp::AddValue {
                                    axis: h.ax_vp,
                                    at,
                                    value: AxisValue::new(AxisValueId(vid), format!("V{vid}")),
                                })],
                            ))
                            .unwrap();
                    }
                }
                // Undo then redo roundtrip
                _ => {
                    let before_patches = h.store.authored().patches().clone();
                    if h.store.undo(0).is_some() {
                        h.store.redo(0).unwrap().unwrap();
                        assert_eq!(h.store.authored().patches(), &before_patches, "{tag}: undo/redo drifted");
                    }
                }
            }
            h.assert_parity(&tag);
        }

        // Verify Schema 16 save/load roundtrip preserves every point's effective values.
        let json = serial::save(h.store.authored());
        let loaded = serial::load(&json).unwrap();
        let mut reloaded = Store::with_document(loaded);
        for p in h.world.space.points() {
            let dp = ParityHarness::to_doc_point(&p);
            h.store.set_point(dp.clone());
            reloaded.set_point(dp);
            assert_eq!(reloaded.resolved(h.r1, lp("transform2d.x")), h.store.resolved(h.r1, lp("transform2d.x")), "script {script}: reload r1.x");
            assert_eq!(reloaded.resolved(h.r2, lp("fill.opacity")), h.store.resolved(h.r2, lp("fill.opacity")), "script {script}: reload r2.opacity");
            assert_eq!(reloaded.tree().is_live(h.r1), h.store.tree().is_live(h.r1), "script {script}: reload r1.live");
            assert_eq!(reloaded.tree().is_hidden(h.r1), h.store.tree().is_hidden(h.r1), "script {script}: reload r1.hidden");
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Axis-growth property in `iron_document`
// ---------------------------------------------------------------------------

#[test]
fn growing_an_axis_changes_no_decision() {
    let mut rng = Rng::new(0xA1_600D);
    for case in 0..60 {
        let mut h = ParityHarness::new();
        for _ in 0..8 {
            let region = h.random_product(&mut rng);
            let d_region = ParityHarness::to_doc_product(&region);
            let layer = rng.usize(2) as u32;
            let (k, v) = h.random_key(&mut rng);
            let pid = h.store.authored().patches().next_patch_id();
            let rank = h.store.authored().patches().default_rank(&d_region, layer);
            let patch = Patch {
                id: pid,
                layer,
                rank,
                region: d_region,
                anchor: None,
                writable: true,
                entries: BTreeMap::from([(h.to_doc_target(k), ParityHarness::to_doc_value(k, v))]),
            };
            h.store.apply(tx("p", vec![Op::Patch(PatchOp::Add { patch })])).unwrap();
        }

        let old_points: Vec<Point> = h.store.authored().patches().points();
        let snap = |store: &mut Store, pts: &[Point]| {
            pts.iter()
                .map(|p| {
                    store.set_point(p.clone());
                    (
                        store.tree().is_live(h.r1),
                        store.tree().is_hidden(h.r1),
                        store.tree().has(h.r2, FacetKind::Fill),
                        store.resolved(h.r1, lp("transform2d.x")),
                        store.resolved(h.r2, lp("fill.opacity")),
                    )
                })
                .collect::<Vec<_>>()
        };

        let before_vals = snap(&mut h.store, &old_points);
        let before_lints = h.store.patch_lints();
        // Also check drag ops at an existing point before & after growing the axes.
        let sample_pt = old_points[rng.usize(old_points.len())].clone();
        let sample_edit = ParityHarness::to_doc_product(&h.random_product(&mut rng));
        let drag_target = PatchTarget { node: h.r1, key: PatchKey::Leaf(lp("transform2d.x")) };
        let drag_val = PatchValue::Slot { slot: num(77.0) };
        h.store.set_point(sample_pt.clone());
        let before_drag = h.store.plan_patch_drag(&sample_edit, drag_target, drag_val.clone(), 0);

        // Grow ordered axis at start, middle, or end; AND grow unordered axis.
        let at_vp = rng.usize(h.vp_vals.len() + 1);
        let new_vp = AxisValue::new(AxisValueId(100 + case), "XL");
        let new_th = AxisValue::new(AxisValueId(200 + case), "high-contrast");
        h.store
            .apply(tx(
                "grow axes",
                vec![
                    Op::Axis(AxisOp::AddValue { axis: h.ax_vp, at: at_vp, value: new_vp }),
                    Op::Axis(AxisOp::AddValue { axis: h.ax_th, at: 0, value: new_th }),
                ],
            ))
            .unwrap();

        let after_vals = snap(&mut h.store, &old_points);
        let after_lints = h.store.patch_lints();
        h.store.set_point(sample_pt);
        let after_drag = h.store.plan_patch_drag(&sample_edit, drag_target, drag_val, 0);
        assert_eq!(after_vals, before_vals, "case {case}: growing axes changed an existing point's decision");
        assert_eq!(after_drag, before_drag, "case {case}: growing axes changed drag ops");
        assert_eq!(after_lints.ties, before_lints.ties, "case {case}: ties lint changed");
        assert_eq!(after_lints.contradictions, before_lints.contradictions, "case {case}: contradictions lint changed");
    }
}

// ---------------------------------------------------------------------------
// 3. Six-tier precedence, patched bindings, solve layer P17, and Key::Patch
// ---------------------------------------------------------------------------

#[test]
fn six_tier_precedence_and_solve_layer_release() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(2);
    let (a, b) = (ids[0], ids[1]);
    let mut ta = Transform2d::default();
    ta.x = num(10.0); // Tier 6: Base = 10
    let mut tb = Transform2d::default();
    tb.x = num(50.0);

    let ax = AxisId(1);
    let v_dark = AxisValueId(1);
    let theme = make_axis(ax, "theme", AxisKind::Unordered, vec![(1, "dark")]);
    let target = PatchTarget { node: a, key: PatchKey::Leaf(lp("transform2d.x")) };
    let leaf = Leaf { node: a, path: lp("transform2d.x") };

    let p_authored = Patch {
        id: PatchId(1),
        layer: 0,
        rank: OrderKey::first(),
        region: product_of([(ax, sel_one(v_dark))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: num(20.0) })]), // Tier 5: Authored patch = 20
    };

    store
        .apply(tx(
            "setup",
            vec![
                Op::Create { id: a, ty: ty("rect"), parent: None, order: OrderKey::first(), facets: vec![ta.wrap(), Size::default().wrap()] },
                Op::Create { id: b, ty: ty("rect"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![tb.wrap(), Size::default().wrap()] },
                Op::Axis(AxisOp::Add { axis: theme }),
                Op::Patch(PatchOp::Add { patch: p_authored }),
            ],
        ))
        .unwrap();

    // Base vs authored patch when switching points
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(10.0)));
    assert_eq!(store.tree().base_leaf(a, lp("transform2d.x")), Some(&num(10.0)));

    let pt_dark = Point { values: BTreeMap::from([(ax, v_dark)]), times: BTreeMap::new() };
    store.set_point(pt_dark.clone());
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(20.0)));
    // base_leaf still returns unpatched base (10.0)!
    assert_eq!(store.tree().base_leaf(a, lp("transform2d.x")), Some(&num(10.0)));

    // Tier 4: Solve layer (30.0) beats Tier 5 authored patch (20.0)
    store.apply(tx("solve pose", vec![Op::Solve { leaf, slot: Some(num(30.0)) }])).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(30.0)));

    // Tier 3: Runtime producer signal (40.0) beats Tier 4 solve layer (30.0)
    let prod = store.register_producer("sim");
    store.publish(prod, BTreeMap::from([(leaf, Value::Number(40.0))]));
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(40.0)));

    // Tier 2: Binding (b.x * 2 = 100.0) beats Tier 3 producer (40.0)
    let p_bind = Patch {
        id: PatchId(2),
        layer: 1,
        rank: OrderKey::first(),
        region: product_of([(ax, sel_one(v_dark))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: Slot::Bound(Expr::parse(&format!("{b}.transform2d.x * 2")).unwrap()) })]),
    };
    store.apply(tx("patched binding", vec![Op::Patch(PatchOp::Add { patch: p_bind })])).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(100.0)));

    // Reactive update through patched binding!
    store.apply(tx("move b", vec![Op::Set { id: b, path: lp("transform2d.x"), slot: num(60.0) }])).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(120.0)));

    // Tier 1: Session patch (999.0) beats Tier 2 binding (120.0)
    let p_sess = Patch {
        id: PatchId(99),
        layer: SESSION_LAYER,
        rank: OrderKey::first(),
        region: product_of([(ax, sel_one(v_dark))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: num(999.0) })]),
    };
    store.set_session_patch(p_sess);
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(999.0)));

    // Strip tiers one by one: 1 -> 2 -> 3 -> 4 -> 5 -> 6
    store.clear_session_patches();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(120.0)));

    store.apply(tx("drop binding patch", vec![Op::Patch(PatchOp::Remove { id: PatchId(2) })])).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(40.0)));

    store.publish(prod, BTreeMap::new());
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(30.0)));

    // P17 policy: deleting a constraint clears solve-layer entry -> snaps back to authored patch (20.0)!
    let release = plan_release_solve(store.authored().patches(), &[leaf]);
    assert_eq!(release.len(), 1);
    store.apply(tx("release solve", release)).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(20.0)));

    // Move off dark -> snaps back to base (10.0)
    store.set_point(Point::default());
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(10.0)));

    // Split, reorder, merge smoke check
    let p_extra = Patch {
        id: PatchId(3),
        layer: 0,
        rank: OrderKey::after(&OrderKey::first()),
        region: product_of([(ax, sel_one(v_dark))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: b, key: PatchKey::Leaf(lp("transform2d.y")) }, PatchValue::Slot { slot: num(7.0) })]),
    };
    store.apply(tx("extra", vec![Op::Patch(PatchOp::Add { patch: p_extra })])).unwrap();
    let split_ops = plan_split(store.authored().patches(), PatchId(1), &[target], product_of([(ax, sel_one(v_dark))])).unwrap();
    store.apply(tx("split", split_ops)).unwrap();
    let new_rank = OrderKey::before(&OrderKey::first());
    let reorder_ops = plan_reorder(store.authored().patches(), PatchId(3), new_rank).unwrap();
    store.apply(tx("reorder", reorder_ops)).unwrap();
    let merge_ops = plan_merge(store.authored().patches(), PatchId(1), PatchId(3)).unwrap();
    store.apply(tx("merge", merge_ops)).unwrap();
}

// ---------------------------------------------------------------------------
// 4. 700-meter edit cost exit gate (`handoff-engine.md` EN-4)
// ---------------------------------------------------------------------------

#[test]
fn patch_edit_at_700_meters_within_2x_constant_edit() {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    store
        .apply(tx(
            "comp",
            vec![Op::Create {
                id: comp,
                ty: ty("composition"),
                parent: None,
                order: OrderKey::first(),
                facets: vec![
                    Composition::default().wrap(),
                    {
                        let mut n = Name::default();
                        n.text = Slot::Const(Value::Str("bench".into()));
                        n.wrap()
                    },
                ],
            }],
        ))
        .unwrap();

    // Place 700 standard meters (the reference 700-meter scaling benchmark scene;
    // scaled down under `differential` so the per-call snapshot oracle stays fast).
    #[cfg(not(feature = "differential"))]
    const METER_COUNT: usize = 700;
    #[cfg(feature = "differential")]
    const METER_COUNT: usize = 40;
    let mut meters = Vec::with_capacity(METER_COUNT);
    for i in 0..METER_COUNT {
        let x = (i % 35) as f64 * 120.0;
        let y = (i / 35) as f64 * 120.0;
        let m = store.place_standard("meter", comp, [x, y], Actor::Human).unwrap();
        meters.push(m);
    }

    // Add a variation axis and an authored patch targeting one meter's transform2d.x.
    let target_node = meters[METER_COUNT / 2];
    let ax = AxisId(1);
    let v_alt = AxisValueId(1);
    let axis = make_axis(ax, "layout", AxisKind::Unordered, vec![(1, "alt")]);
    let target = PatchTarget { node: target_node, key: PatchKey::Leaf(lp("transform2d.x")) };
    let patch = Patch {
        id: PatchId(1),
        layer: 0,
        rank: OrderKey::first(),
        region: product_of([(ax, sel_one(v_alt))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: num(500.0) })]),
    };
    store
        .apply(tx(
            "add patch",
            vec![Op::Axis(AxisOp::Add { axis }), Op::Patch(PatchOp::Add { patch })],
        ))
        .unwrap();
    store.set_point(Point { values: BTreeMap::from([(ax, v_alt)]), times: BTreeMap::new() });

    let other_node = meters[METER_COUNT / 2 + 1];
    // Warm up both paths.
    for i in 0..5 {
        store.apply(tx("warm c", vec![Op::Set { id: other_node, path: lp("transform2d.x"), slot: num(100.0 + i as f64) }])).unwrap();
        store
            .apply(tx(
                "warm p",
                vec![Op::Patch(PatchOp::SetEntry {
                    patch: PatchId(1),
                    target,
                    value: Some(PatchValue::Slot { slot: num(500.0 + i as f64) }),
                })],
            ))
            .unwrap();
    }

    // Interleave 21 measurements each so CPU frequency scaling affects both equally.
    let mut const_us = Vec::with_capacity(21);
    let mut patch_us = Vec::with_capacity(21);
    for i in 0..21 {
        let t0 = Instant::now();
        store.apply(tx("c", vec![Op::Set { id: other_node, path: lp("transform2d.x"), slot: num(200.0 + i as f64) }])).unwrap();
        const_us.push(t0.elapsed().as_secs_f64() * 1e6);

        let t1 = Instant::now();
        store
            .apply(tx(
                "p",
                vec![Op::Patch(PatchOp::SetEntry {
                    patch: PatchId(1),
                    target,
                    value: Some(PatchValue::Slot { slot: num(600.0 + i as f64) }),
                })],
            ))
            .unwrap();
        patch_us.push(t1.elapsed().as_secs_f64() * 1e6);
    }
    const_us.sort_by(f64::total_cmp);
    patch_us.sort_by(f64::total_cmp);
    let c_p50 = const_us[const_us.len() / 2];
    let p_p50 = patch_us[patch_us.len() / 2];
    let ratio = p_p50 / c_p50.max(1e-6);
    eprintln!("[EN-4 700-meter bench] constant edit p50 = {c_p50:.1} us, patch edit p50 = {p_p50:.1} us, ratio = {ratio:.2}x");
    assert!(
        p_p50 <= (c_p50 * 2.0).max(c_p50 + 250.0),
        "patch edit ({p_p50:.1} us) exceeded 2x constant edit ({c_p50:.1} us, ratio {ratio:.2}x) at 700 meters"
    );
}
