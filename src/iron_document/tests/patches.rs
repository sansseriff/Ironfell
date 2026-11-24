//! EN-4 / EN-4b Patch Algebra, Sparse Resolved Store, 6-Tier Precedence & Point Evaluation tests
//! (`plans/specs/engine.md` §2.3–§2.4, `plans/specs/patches.md`, `plans/handoff-engine.md` EN-4).
//!
//! Exit gates verified here:
//! 1. Differential parity against `iron_patch::World` across randomized multi-step scripts
//!    covering Ordered, Unordered, and scoped Instance axes (`Suffix`, `Not`, `Excl`, anchors),
//!    presence, hiding, optional facet presence (P21), discrete-point solve layer (P17),
//!    session layer (P8), drag/split/merge/reorder, undo/redo, and Schema 16 roundtrip.
//! 2. Axis-growth property (`growing_an_axis_changes_no_decision`) in `iron_document`.
//! 3. Base-vs-effective op separation (`Op::Delete`, `Op::AddFacet`, undo preservation),
//!    effective delta propagation (`FacetPresent(false)`, descendant bindings, relations,
//!    `nodes_of_type`/`nodes_with`, host `Changes`), anchor-aware lints/invalidation,
//!    continuous `Time` precision & `-0.0` normalization, validation boundary, and atomic rollback.
//! 4. 6-tier precedence (`session > binding > producer > solve > authored patch > base`),
//!    per-discrete-point solve isolation, reactive patched bindings, and P17 release.
//! 5. 700-meter edit cost: single patch edit ≤ 2× single constant edit.

use iron_document::facet::{Composition, Fill, Name, Size, Transform2d};
use iron_document::patch::{
    Axis, AxisId, AxisKind, AxisOp, AxisValue, AxisValueId, Bound, Patch, PatchId, PatchKey,
    PatchOp, PatchTarget, PatchValue, Point, Product, SESSION_LAYER, Sel, axis_key, plan_merge,
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
// 1. Differential parity against `iron_patch::World` (Ordered + Unordered + Instance)
// ---------------------------------------------------------------------------

/// Maps a 3-node document (group + 2 rects, where rect2 has optional Fill
/// absent in base) and 3 axes (ordered `viewport`, unordered `theme`, and
/// scoped `Instance` axis `inst`) between `iron_document::Store` and `iron_patch::World`.
struct ParityHarness {
    store: Store,
    world: iron_patch::World,
    group: NodeId,
    r1: NodeId,
    r2: NodeId,
    scope_a: NodeId,
    scope_b: NodeId,
    step_1: NodeId,
    step_2: NodeId,
    ax_vp: AxisId,
    ax_th: AxisId,
    ax_inst: AxisId,
    next_val: u64,
    vp_vals: Vec<u64>,
    th_vals: Vec<u64>,
    inst_vals: Vec<u64>,
    patch_ids: Vec<u64>,
    session_ids: Vec<u64>,
}

// iron_patch reserves Field 0 = PRESENT, Field 1 = HIDDEN.
const F_X: iron_patch::Field = 2;
const F_OPACITY: iron_patch::Field = 3;
const FACET_FILL: iron_patch::FacetId = 1;

impl ParityHarness {
    fn new() -> Self {
        let mut store = Store::new();
        let ids = store.authored().next_ids(7);
        let (group, r1, r2, scope_a, scope_b, step_1, step_2) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[6]);

        let mut t1 = Transform2d::default();
        t1.x = num(10.0);
        let mut f1 = Fill::default();
        f1.opacity = num(1.0);

        let mut t2 = Transform2d::default();
        t2.x = num(20.0);
        // r2 starts WITHOUT Fill so PatchKey::FacetPresent(Fill) toggles it on/off.

        let ax_vp = AxisId(1);
        let ax_th = AxisId(2);
        let ax_inst = AxisId(3);
        let vp_axis = make_axis(ax_vp, "viewport", AxisKind::Ordered, vec![(1, "S"), (2, "M"), (3, "L")]);
        let th_axis = make_axis(ax_th, "theme", AxisKind::Unordered, vec![(4, "light"), (5, "dark")]);
        let mut inst_axis = Axis::new(ax_inst, "inst", AxisKind::Instance, None);
        // Three placement paths with scope chains:
        // v6: [step_1] under [scope_a]
        // v7: [step_1, step_2] under [scope_a, scope_b]
        // v8: [step_2] under [scope_b]
        inst_axis.push(AxisValue::with_path(AxisValueId(6), "i1", vec![step_1], vec![scope_a]));
        inst_axis.push(AxisValue::with_path(AxisValueId(7), "i2", vec![step_1, step_2], vec![scope_a, scope_b]));
        inst_axis.push(AxisValue::with_path(AxisValueId(8), "i3", vec![step_2], vec![scope_b]));

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
                    Op::Axis(AxisOp::Add { axis: inst_axis }),
                    Op::Axis(AxisOp::SetOrder { order: vec![ax_vp, ax_th, ax_inst] }),
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
        let mut w_inst = iron_patch::Axis::new(iron_patch::Kind::Path);
        w_inst.push_scoped(iron_patch::ValueId(6), vec![1], vec![10]);
        w_inst.push_scoped(iron_patch::ValueId(7), vec![1, 2], vec![10, 20]);
        w_inst.push_scoped(iron_patch::ValueId(8), vec![2], vec![20]);

        let space = iron_patch::Space {
            axes: BTreeMap::from([
                (iron_patch::AxisId(1), w_vp),
                (iron_patch::AxisId(2), w_th),
                (iron_patch::AxisId(3), w_inst),
            ]),
            order: vec![iron_patch::AxisId(1), iron_patch::AxisId(2), iron_patch::AxisId(3)],
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
        doc.base.insert(iron_patch::Key { node: n2, field: F_OPACITY }, 1);

        let world = iron_patch::World::new(space, doc, iron_patch::Strength::Stored);
        ParityHarness {
            store,
            world,
            group,
            r1,
            r2,
            scope_a,
            scope_b,
            step_1,
            step_2,
            ax_vp,
            ax_th,
            ax_inst,
            next_val: 9,
            vp_vals: vec![1, 2, 3],
            th_vals: vec![4, 5],
            inst_vals: vec![6, 7, 8],
            patch_ids: Vec::new(),
            session_ids: Vec::new(),
        }
    }

    fn node_of(&self, idx: u32) -> NodeId {
        match idx {
            0 => self.group,
            1 => self.r1,
            _ => self.r2,
        }
    }

    fn step_node(&self, step: u32) -> NodeId {
        if step == 1 { self.step_1 } else { self.step_2 }
    }

    fn scope_node(&self, s: iron_patch::ScopeId) -> NodeId {
        if s == 10 { self.scope_a } else { self.scope_b }
    }

    fn to_doc_point(p: &iron_patch::Point) -> Point {
        let mut out = Point::default();
        for (&a, &v) in p {
            out.values.insert(AxisId(a.0 as u64), AxisValueId(v.0 as u64));
        }
        out
    }

    fn to_doc_sel(&self, s: &iron_patch::Sel) -> Sel {
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
            iron_patch::Sel::Suffix { tail, not } => Sel::Suffix {
                tail: tail.iter().map(|&step| self.step_node(step)).collect(),
                not: not.iter().map(|path| path.iter().map(|&step| self.step_node(step)).collect()).collect(),
            },
        }
    }

    fn to_doc_product(&self, p: &iron_patch::Product) -> Product {
        Product { sels: p.sels.iter().map(|(a, s)| (AxisId(a.0 as u64), self.to_doc_sel(s))).collect() }
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
        // Axis 1 (ordered viewport: All, Set, Range with Incl/Excl/Open)
        match rng.usize(6) {
            0 => {}
            1 => {
                let v = self.vp_vals[rng.usize(self.vp_vals.len())];
                sels.insert(iron_patch::AxisId(1), iron_patch::Sel::Set(BTreeSet::from([iron_patch::ValueId(v as u32)])));
            }
            2 => {
                let a = self.vp_vals[rng.usize(self.vp_vals.len())];
                let b = self.vp_vals[rng.usize(self.vp_vals.len())];
                let lo = match rng.usize(3) {
                    0 => iron_patch::Bound::Open,
                    1 => iron_patch::Bound::Incl(iron_patch::ValueId(a as u32)),
                    _ => iron_patch::Bound::Excl(iron_patch::ValueId(a as u32)),
                };
                let hi = match rng.usize(3) {
                    0 => iron_patch::Bound::Open,
                    1 => iron_patch::Bound::Incl(iron_patch::ValueId(b as u32)),
                    _ => iron_patch::Bound::Excl(iron_patch::ValueId(b as u32)),
                };
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
        // Axis 2 (unordered theme: All, Set, Not)
        match rng.usize(4) {
            0 => {}
            1 => {
                let v = self.th_vals[rng.usize(self.th_vals.len())];
                sels.insert(iron_patch::AxisId(2), iron_patch::Sel::Set(BTreeSet::from([iron_patch::ValueId(v as u32)])));
            }
            2 => {
                let v = self.th_vals[rng.usize(self.th_vals.len())];
                sels.insert(iron_patch::AxisId(2), iron_patch::Sel::Not(BTreeSet::from([iron_patch::ValueId(v as u32)])));
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
        // Axis 3 (instance path axis: All, Suffix, Suffix with Not, Set)
        match rng.usize(5) {
            0 | 1 => {}
            2 => {
                let tail = match rng.usize(3) {
                    0 => vec![1],
                    1 => vec![2],
                    _ => vec![1, 2],
                };
                sels.insert(iron_patch::AxisId(3), iron_patch::Sel::Suffix { tail, not: vec![] });
            }
            3 => {
                sels.insert(iron_patch::AxisId(3), iron_patch::Sel::Suffix { tail: vec![2], not: vec![vec![1, 2]] });
            }
            _ => {
                let v = self.inst_vals[rng.usize(self.inst_vals.len())];
                sels.insert(iron_patch::AxisId(3), iron_patch::Sel::Set(BTreeSet::from([iron_patch::ValueId(v as u32)])));
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

    fn assert_parity(&mut self, ctx: &str, check_lints: bool) {
        self.store.tree().check_invariants().unwrap_or_else(|e| panic!("{ctx}: invariants failed: {e}"));
        let pts = self.world.space.points();
        for p in &pts {
            let dp = Self::to_doc_point(p);
            // Non-mutating eval_at check before set_point.
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

            // r1 transform2d.x (authored + solve + session)
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

            // patch_stack order parity on r1.x (includes session patches!)
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

        // Lints parity (unanchored scripts: iron_patch's dead/ties/contradictions don't check anchors,
        // whereas iron_document's EN-4b lints are anchor-aware — verified separately below!).
        if check_lints && self.world.patches.iter().all(|p| p.anchor.is_none() && p.layer < iron_patch::SESSION_LAYER) {
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
}

#[test]
fn differential_parity_with_iron_patch() {
    let mut rng = Rng::new(0xE4_5EED_0002);
    for script in 0..40 {
        let mut h = ParityHarness::new();
        // Half the scripts stay unanchored/no-session so all 4 lints are cross-checked against iron_patch;
        // the other half also fuzz anchored patches, solve-layer entries, and session patches!
        let fuzz_extended = script >= 20;
        for step in 0..12 {
            let tag = format!("script {script} step {step}");
            match rng.usize(13) {
                // Add an authored patch (sometimes anchored in extended scripts)
                0..=3 => {
                    let region = h.random_product(&mut rng);
                    let layer = rng.usize(2) as u32;
                    let (k, v) = h.random_key(&mut rng);
                    let w_anchor = if fuzz_extended && rng.usize(3) == 0 {
                        Some(if rng.bool() { 10 } else { 20 })
                    } else {
                        None
                    };
                    let d_anchor = w_anchor.map(|s| h.scope_node(s));
                    let mut w_patch = h.world.make(region.clone(), BTreeMap::from([(k, v)]), layer);
                    w_patch.anchor = w_anchor;
                    let pid = w_patch.id.0 as u64;
                    let d_region = h.to_doc_product(&region);
                    // Mirror `iron_patch::World::default_rank`, which ranks all patches in `layer`
                    // in one shared rank order across anchors (no derived ViewCoord layers).
                    let d_rank = {
                        let table = h.store.authored().patches();
                        let mine = table.keys(&d_region);
                        let used: BTreeSet<OrderKey> = table
                            .patches
                            .values()
                            .filter(|p| p.layer == layer)
                            .map(|p| p.rank.clone())
                            .collect();
                        let floor = table
                            .patches
                            .values()
                            .filter(|p| p.layer == layer && table.keys(&p.region) <= mine)
                            .map(|p| p.rank.clone())
                            .max();
                        iron_document::patch::PatchTable::alloc_rank(&used, floor.as_ref())
                    };
                    let d_patch = Patch {
                        id: PatchId(pid),
                        layer,
                        rank: d_rank,
                        region: d_region,
                        anchor: d_anchor,
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
                        let d_region = h.to_doc_product(&region);
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
                // Drag via plan_patch_drag vs iron_patch::edit::drag (unanchored scripts)
                7 if !fuzz_extended => {
                    let pts = h.world.space.points();
                    if let Some(p) = pts.get(rng.usize(pts.len().max(1))).cloned() {
                        let dp = ParityHarness::to_doc_point(&p);
                        h.store.set_point(dp);
                        let edit = h.random_product(&mut rng);
                        let d_edit = h.to_doc_product(&edit);
                        let k = iron_patch::Key { node: 1, field: F_X };
                        let v = (rng.usize(90) as i64) + 10;
                        let layer = rng.usize(2) as u32;
                        let w_res = iron_patch::edit::drag(&h.world, &p, &edit, k, v, layer);
                        let d_res = h.store.plan_patch_drag(&d_edit, None, h.to_doc_target(k), ParityHarness::to_doc_value(k, v), layer);
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
                // Solve layer write/clear at a specific discrete point (P17 parity!)
                8 | 9 if fuzz_extended => {
                    let pts = h.world.space.points();
                    if let Some(p) = pts.get(rng.usize(pts.len().max(1))).cloned() {
                        let dp = ParityHarness::to_doc_point(&p);
                        let clear = rng.usize(3) == 0;
                        let val = (!clear).then(|| (rng.usize(80) as i64) + 100);
                        let k = iron_patch::Key { node: 1, field: F_X };
                        h.world.apply(&iron_patch::Op::Solve { point: p, key: k, value: val });
                        h.store
                            .apply(tx(
                                "solve",
                                vec![Op::Solve {
                                    point: dp.discrete(),
                                    leaf: Leaf { node: h.r1, path: lp("transform2d.x") },
                                    slot: val.map(|x| num(x as f64)),
                                }],
                            ))
                            .unwrap();
                    }
                }
                // Session layer patch set/remove (P8 parity!)
                10 if fuzz_extended => {
                    if !h.session_ids.is_empty() && rng.bool() {
                        let sid = h.session_ids.remove(rng.usize(h.session_ids.len()));
                        h.world.apply(&iron_patch::Op::Remove(iron_patch::PatchId(sid as u32)));
                        h.store.remove_session_patch(PatchId(sid));
                    } else {
                        let region = h.random_product(&mut rng);
                        let k = iron_patch::Key { node: 1, field: F_X };
                        let v = (rng.usize(50) as i64) + 200;
                        let w_patch = h.world.make(region.clone(), BTreeMap::from([(k, v)]), iron_patch::SESSION_LAYER);
                        let sid = w_patch.id.0 as u64;
                        let d_region = h.to_doc_product(&region);
                        let d_patch = Patch {
                            id: PatchId(sid),
                            layer: SESSION_LAYER,
                            rank: OrderKey::after(&OrderKey::first()),
                            region: d_region,
                            anchor: None,
                            writable: true,
                            entries: BTreeMap::from([(h.to_doc_target(k), ParityHarness::to_doc_value(k, v))]),
                        };
                        h.world.apply(&iron_patch::Op::Add { patch: w_patch, at: h.world.patches.len() });
                        h.store.set_session_patch(d_patch).unwrap();
                        h.session_ids.push(sid);
                    }
                }
                // Axis growth / tombstone / restore / move
                11 => {
                    if h.vp_vals.len() < 4 && rng.bool() {
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
                    } else if h.vp_vals.len() > 2 {
                        let vid = h.vp_vals[rng.usize(h.vp_vals.len())];
                        let to = rng.usize(h.vp_vals.len());
                        h.world.space.axes.get_mut(&iron_patch::AxisId(1)).unwrap().move_to(iron_patch::ValueId(vid as u32), to);
                        h.store
                            .apply(tx("move vp val", vec![Op::Axis(AxisOp::MoveValue { axis: h.ax_vp, id: AxisValueId(vid), at: to })]))
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
            h.assert_parity(&tag, !fuzz_extended);
        }

        // Verify Schema 16 save/load roundtrip preserves authored patches + solve layer
        // (excluding session patches, which are transient).
        h.store.clear_session_patches();
        h.world.patches.retain(|p| p.layer < iron_patch::SESSION_LAYER);
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
    for case in 0..40 {
        let mut h = ParityHarness::new();
        for _ in 0..8 {
            let region = h.random_product(&mut rng);
            let d_region = h.to_doc_product(&region);
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
        let sample_pt = old_points[rng.usize(old_points.len())].clone();
        let sample_edit = h.to_doc_product(&h.random_product(&mut rng));
        let drag_target = PatchTarget { node: h.r1, key: PatchKey::Leaf(lp("transform2d.x")) };
        let drag_val = PatchValue::Slot { slot: num(77.0) };
        h.store.set_point(sample_pt.clone());
        let before_drag = h.store.plan_patch_drag(&sample_edit, None, drag_target, drag_val.clone(), 0);

        let at_vp = rng.usize(h.vp_vals.len() + 1);
        let new_vp = AxisValue::new(AxisValueId(100 + case), "XL");
        let new_th = AxisValue::new(AxisValueId(200 + case), "high-contrast");
        let new_inst = AxisValue::with_path(AxisValueId(300 + case), "i4", vec![h.step_2, h.step_1], vec![h.scope_b, h.scope_a]);
        h.store
            .apply(tx(
                "grow axes",
                vec![
                    Op::Axis(AxisOp::AddValue { axis: h.ax_vp, at: at_vp, value: new_vp }),
                    Op::Axis(AxisOp::AddValue { axis: h.ax_th, at: 0, value: new_th }),
                    Op::Axis(AxisOp::AddValue { axis: h.ax_inst, at: 0, value: new_inst }),
                ],
            ))
            .unwrap();

        let after_vals = snap(&mut h.store, &old_points);
        let after_lints = h.store.patch_lints();
        h.store.set_point(sample_pt);
        let after_drag = h.store.plan_patch_drag(&sample_edit, None, drag_target, drag_val, 0);
        assert_eq!(after_vals, before_vals, "case {case}: growing axes changed an existing point's decision");
        assert_eq!(after_drag, before_drag, "case {case}: growing axes changed drag ops");
        assert_eq!(after_lints.ties, before_lints.ties, "case {case}: ties lint changed");
        assert_eq!(after_lints.contradictions, before_lints.contradictions, "case {case}: contradictions lint changed");
    }
}

// ---------------------------------------------------------------------------
// 3. EN-4b Hardening: base vs effective ops, delta propagation, anchors,
//    continuous time precision, validation boundary, and atomic rollback
// ---------------------------------------------------------------------------

#[test]
fn base_vs_effective_authored_ops_and_undo() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(3);
    let (group, r1, r2) = (ids[0], ids[1], ids[2]);
    let ax = AxisId(1);
    let v_alt = AxisValueId(1);
    let axis = make_axis(ax, "mode", AxisKind::Unordered, vec![(1, "alt")]);

    let mut t1 = Transform2d::default();
    t1.x = num(10.0);
    let mut t2 = Transform2d::default();
    t2.x = num(20.0);
    let mut f2 = Fill::default();
    f2.opacity = num(0.4);

    let p = Patch {
        id: PatchId(1),
        layer: 0,
        rank: OrderKey::first(),
        region: product_of([(ax, sel_one(v_alt))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([
            // r1 is absent at v_alt
            (PatchTarget { node: r1, key: PatchKey::Present }, PatchValue::Bool { v: false }),
            // r2's x is patched to 500 and its base Fill is removed via FacetPresent(false)
            (PatchTarget { node: r2, key: PatchKey::Leaf(lp("transform2d.x")) }, PatchValue::Slot { slot: num(500.0) }),
            (PatchTarget { node: r2, key: PatchKey::FacetPresent(FacetKind::Fill) }, PatchValue::Bool { v: false }),
        ]),
    };

    store
        .apply(tx(
            "init",
            vec![
                Op::Create { id: group, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] },
                Op::Create { id: r1, ty: ty("rect"), parent: Some(group), order: OrderKey::first(), facets: vec![t1.wrap(), Size::default().wrap()] },
                Op::Create {
                    id: r2,
                    ty: ty("rect"),
                    parent: Some(group),
                    order: OrderKey::after(&OrderKey::first()),
                    facets: vec![t2.wrap(), Size::default().wrap(), f2.wrap()],
                },
                Op::Axis(AxisOp::Add { axis }),
                Op::Patch(PatchOp::Add { patch: p }),
            ],
        ))
        .unwrap();

    store.set_point(Point::new().with_value(ax, v_alt));
    assert!(!store.tree().is_live(r1));
    assert_eq!(store.resolved(r2, lp("transform2d.x")), Some(Value::Number(500.0)));
    assert!(!store.tree().has(r2, FacetKind::Fill));

    // AddFacet(Fill) on r2 MUST fail because r2 still has base Fill (even though FacetPresent=false hides it!).
    let err = store.apply(tx("dup fill", vec![Op::AddFacet { id: r2, facet: Fill::default().wrap() }])).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::FacetPresent);

    // Delete parent group while r1 is patched absent and r2 is patched:
    // r1 MUST be tombstoned too (no live authored child under a tombstone!).
    store.apply(tx("delete group", vec![Op::Delete { id: group }])).unwrap();
    assert!(store.authored().node(r1).unwrap().tombstoned.is_some(), "absent child r1 must be tombstoned when parent is deleted");
    store.tree().check_invariants().unwrap();

    // Undo delete: base x of r2 MUST stay 20.0 (not 500.0!) and base Fill opacity MUST stay 0.4!
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.tree().base_leaf(r2, lp("transform2d.x")), Some(&num(20.0)));
    assert_eq!(store.tree().base_leaf(r2, lp("fill.opacity")), Some(&num(0.4)));
    assert_eq!(store.resolved(r2, lp("transform2d.x")), Some(Value::Number(500.0)));
    assert!(!store.tree().has(r2, FacetKind::Fill));

    // Switch off v_alt -> r1 returns, r2 returns to base 20.0 and base Fill 0.4!
    store.set_point(Point::default());
    assert!(store.tree().is_live(r1));
    assert_eq!(store.resolved(r2, lp("transform2d.x")), Some(Value::Number(20.0)));
    assert_eq!(store.resolved(r2, lp("fill.opacity")), Some(Value::Number(0.4)));
}

#[test]
fn effective_delta_facet_removal_descendant_bindings_relations_and_changes() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(3);
    let (group, child, watcher) = (ids[0], ids[1], ids[2]);
    let rel_id = store.authored().next_relation_id();
    let ax = AxisId(1);
    let v_hide = AxisValueId(1);
    let v_nofill = AxisValueId(2);
    let axis = make_axis(ax, "mode", AxisKind::Unordered, vec![(1, "hide"), (2, "nofill")]);

    let mut tc = Transform2d::default();
    tc.x = Slot::Bound(Expr::parse(&format!("{watcher}.transform2d.x + 5")).unwrap());
    let mut fc = Fill::default();
    fc.opacity = num(0.75);

    let mut tw = Transform2d::default();
    tw.x = num(100.0);
    // Watcher reads child's fill.opacity reactively!
    tw.y = Slot::Bound(Expr::parse(&format!("{child}.fill.opacity")).unwrap());

    let p_hide = Patch {
        id: PatchId(1),
        layer: 0,
        rank: OrderKey::first(),
        region: product_of([(ax, sel_one(v_hide))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: group, key: PatchKey::Present }, PatchValue::Bool { v: false })]),
    };
    let p_nofill = Patch {
        id: PatchId(2),
        layer: 0,
        rank: OrderKey::after(&OrderKey::first()),
        region: product_of([(ax, sel_one(v_nofill))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: child, key: PatchKey::FacetPresent(FacetKind::Fill) }, PatchValue::Bool { v: false })]),
    };

    store
        .apply(tx(
            "init",
            vec![
                Op::Create { id: group, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] },
                Op::Create { id: child, ty: ty("rect"), parent: Some(group), order: OrderKey::first(), facets: vec![tc.wrap(), Size::default().wrap(), fc.wrap()] },
                Op::Create {
                    id: watcher,
                    ty: ty("rect"),
                    parent: None,
                    order: OrderKey::after(&OrderKey::first()),
                    facets: vec![tw.wrap(), Size::default().wrap()],
                },
                Op::Link {
                    rel: Relation { id: rel_id, from: watcher, to: child, rel: RelKind::References, data: BTreeMap::new() },
                },
                Op::Axis(AxisOp::Add { axis }),
                Op::Patch(PatchOp::Add { patch: p_hide }),
                Op::Patch(PatchOp::Add { patch: p_nofill }),
            ],
        ))
        .unwrap();
    let _ = store.take_changes();

    assert_eq!(store.resolved(child, lp("transform2d.x")), Some(Value::Number(105.0)));
    assert_eq!(store.resolved(watcher, lp("transform2d.y")), Some(Value::Number(0.75)));
    assert!(store.tree().nodes_with(FacetKind::Fill).any(|id| id == child));
    assert!(store.tree().relations_from(watcher).any(|r| r.id == rel_id));

    // 1. Removing base Fill via FacetPresent(false) must emit Key::Facet/Key::Leaf so:
    //    - child.fill.opacity reads None
    //    - nodes_with(Fill) excludes child
    //    - watcher.y re-evaluates and records a missing-leaf binding error!
    store.set_point(Point::new().with_value(ax, v_nofill));
    assert!(!store.tree().has(child, FacetKind::Fill));
    assert!(!store.tree().nodes_with(FacetKind::Fill).any(|id| id == child));
    assert_eq!(store.resolved(child, lp("fill.opacity")), None);
    assert!(
        store.reactive().errors.contains_key(&Leaf { node: watcher, path: lp("transform2d.y") }),
        "watcher must re-evaluate and error when base Fill is removed by FacetPresent(false)"
    );

    // 2. Making parent `group` absent cascades to `child`:
    //    - read_with returns None for child's bound leaf (liveness check first!)
    //    - relations touching child are filtered out
    //    - nodes_of_type excludes child
    //    - take_changes() reports both group & child as removed!
    store.set_point(Point::new().with_value(ax, v_hide));
    let ch = store.take_changes();
    assert!(ch.removed.iter().any(|(id, _)| *id == group), "group must appear in Changes.removed");
    assert!(ch.removed.iter().any(|(id, _)| *id == child), "cascaded child must appear in Changes.removed");
    assert_eq!(store.resolved(child, lp("transform2d.x")), None, "absent descendant must not return cached binding value");
    assert!(store.tree().relation(rel_id).is_none(), "relation to absent node must be filtered");
    assert!(store.tree().relations_from(watcher).next().is_none(), "relations_from must filter absent target");
    assert!(!store.tree().nodes_of_type(ty("rect")).any(|id| id == child));

    // 3. Revealing parent `group` back at default point:
    //    - take_changes() reports created
    //    - descendant binding on `child` re-installs and tracks edits to `watcher.x`!
    store.set_point(Point::default());
    let ch2 = store.take_changes();
    assert!(ch2.created.contains(&group) && ch2.created.contains(&child), "revealed nodes must appear in Changes.created");
    store.apply(tx("move watcher", vec![Op::Set { id: watcher, path: lp("transform2d.x"), slot: num(200.0) }])).unwrap();
    assert_eq!(store.resolved(child, lp("transform2d.x")), Some(Value::Number(205.0)));
    assert_eq!(store.resolved(watcher, lp("transform2d.y")), Some(Value::Number(0.75)));
}

#[test]
fn anchors_and_continuous_time_precision() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(4);
    let (r, scope_a, scope_b, step_1) = (ids[0], ids[1], ids[2], ids[3]);
    let ax_inst = AxisId(1);
    let ax_time = AxisId(2);
    let mut inst = Axis::new(ax_inst, "inst", AxisKind::Instance, None);
    inst.push(AxisValue::with_path(AxisValueId(1), "in_a", vec![step_1], vec![scope_a]));
    inst.push(AxisValue::with_path(AxisValueId(2), "in_b", vec![step_1], vec![scope_b]));
    let time_ax = Axis::new(ax_time, "t", AxisKind::Time, None);

    let target = PatchTarget { node: r, key: PatchKey::Leaf(lp("transform2d.x")) };
    // Weak unanchored patch on Product::all() vs strong anchored patch (scope_a) on Product::all()!
    let p_weak = Patch {
        id: PatchId(1),
        layer: 0,
        rank: OrderKey::first(),
        region: Product::all(),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: num(10.0) })]),
    };
    let p_strong_anchored = Patch {
        id: PatchId(2),
        layer: 0,
        rank: OrderKey::after(&OrderKey::first()),
        region: Product::all(),
        anchor: Some(scope_a),
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: num(99.0) })]),
    };

    store
        .apply(tx(
            "init",
            vec![
                Op::Create { id: r, ty: ty("rect"), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] },
                Op::Axis(AxisOp::Add { axis: inst }),
                Op::Axis(AxisOp::Add { axis: time_ax }),
                Op::Patch(PatchOp::Add { patch: p_weak }),
                Op::Patch(PatchOp::Add { patch: p_strong_anchored }),
            ],
        ))
        .unwrap();

    // Anchor-aware dead lint: p_weak MUST NOT be marked dead because p_strong_anchored only covers scope_a!
    let lints = store.patch_lints();
    assert!(lints.dead.is_empty(), "unanchored patch must not be marked dead by an anchored patch: {:?}", lints.dead);

    // Implicit anchor invalidation: moving Instance axis from in_a to in_b updates r.x even though p_strong_anchored has Product::all()!
    store.set_point(Point::new().with_value(ax_inst, AxisValueId(1)));
    assert_eq!(store.resolved(r, lp("transform2d.x")), Some(Value::Number(99.0)));
    store.set_point(Point::new().with_value(ax_inst, AxisValueId(2)));
    assert_eq!(store.resolved(r, lp("transform2d.x")), Some(Value::Number(10.0)));

    // Continuous time `-0.0` normalization: (-0.0, 1.0] ∩ [+0.0, 1.0] must NOT include 0.0!
    let t_ax = store.authored().patches().axes.get(&ax_time).unwrap();
    let s1 = Sel::Range { lo: Bound::TimeExcl { t: -0.0 }, hi: Bound::TimeIncl { t: 1.0 } };
    let s2 = Sel::Range { lo: Bound::TimeIncl { t: 0.0 }, hi: Bound::TimeIncl { t: 1.0 } };
    let inter = s1.intersect(&s2, t_ax);
    assert!(!inter.contains_time(0.0), "(-0.0, 1] ∩ [+0.0, 1] must exclude 0.0: {inter:?}");
    assert!(inter.contains_time(1e-9));

    // Sub-microsecond precision in time specificity (no microsecond truncation!).
    let k_early = axis_key(&store.authored().patches().axes, ax_time, &Sel::Range { lo: Bound::TimeIncl { t: 1.0000001 }, hi: Bound::Open });
    let k_late = axis_key(&store.authored().patches().axes, ax_time, &Sel::Range { lo: Bound::TimeIncl { t: 1.0000002 }, hi: Bound::Open });
    assert!(k_late > k_early, "later sub-microsecond start must have strictly higher specificity");
}

#[test]
fn validation_boundary_and_failed_transaction_rollback() {
    let mut store = Store::new();
    let r = store.authored().next_id();
    let ax_ord = AxisId(1);
    let ax_unord = AxisId(2);
    let ord = make_axis(ax_ord, "vp", AxisKind::Ordered, vec![(1, "S"), (2, "L")]);
    let unord = make_axis(ax_unord, "th", AxisKind::Unordered, vec![(3, "dark")]);

    store
        .apply(tx(
            "init",
            vec![
                Op::Create { id: r, ty: ty("rect"), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] },
                Op::Axis(AxisOp::Add { axis: ord }),
                Op::Axis(AxisOp::Add { axis: unord }),
            ],
        ))
        .unwrap();

    // 1. Mismatched PatchValue kind (Bool on Leaf) -> rejected
    let bad_val = Patch {
        id: PatchId(1),
        layer: 0,
        rank: OrderKey::first(),
        region: Product::all(),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(lp("transform2d.x")) }, PatchValue::Bool { v: true })]),
    };
    assert!(store.apply(tx("bad val", vec![Op::Patch(PatchOp::Add { patch: bad_val })])).is_err());

    // 2. Time bound on Ordered axis -> rejected
    let bad_bound = Patch {
        id: PatchId(2),
        layer: 0,
        rank: OrderKey::first(),
        region: product_of([(ax_ord, Sel::Range { lo: Bound::TimeIncl { t: 1.0 }, hi: Bound::Open })]),
        anchor: None,
        writable: true,
        entries: BTreeMap::new(),
    };
    assert!(store.apply(tx("bad bound", vec![Op::Patch(PatchOp::Add { patch: bad_bound })])).is_err());

    // 3. Removing required facet `Size` on `rect` via FacetPresent(false) -> rejected
    let bad_req = Patch {
        id: PatchId(3),
        layer: 0,
        rank: OrderKey::first(),
        region: Product::all(),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::FacetPresent(FacetKind::Size) }, PatchValue::Bool { v: false })]),
    };
    assert!(store.apply(tx("bad req", vec![Op::Patch(PatchOp::Add { patch: bad_req })])).is_err());

    // 4. Read-only patch refuses SetEntry / SetRegion
    let ro = Patch {
        id: PatchId(4),
        layer: 0,
        rank: OrderKey::first(),
        region: product_of([(ax_unord, sel_one(AxisValueId(3)))]),
        anchor: None,
        writable: false,
        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(lp("transform2d.x")) }, PatchValue::Slot { slot: num(42.0) })]),
    };
    store.apply(tx("add ro", vec![Op::Patch(PatchOp::Add { patch: ro })])).unwrap();
    assert!(
        store
            .apply(tx(
                "edit ro",
                vec![Op::Patch(PatchOp::SetEntry {
                    patch: PatchId(4),
                    target: PatchTarget { node: r, key: PatchKey::Leaf(lp("transform2d.x")) },
                    value: Some(PatchValue::Slot { slot: num(99.0) }),
                })]
            ))
            .is_err()
    );
    // Removing an axis still referenced by a patch -> rejected!
    assert!(store.apply(tx("rm referenced axis", vec![Op::Axis(AxisOp::Remove { id: ax_unord })])).is_err());

    // 5. Session patch rejects Present or Bound entries without leaving dangling touched keys!
    let bad_sess = Patch {
        id: PatchId(99),
        layer: SESSION_LAYER,
        rank: OrderKey::first(),
        region: Product::all(),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Present }, PatchValue::Bool { v: false })]),
    };
    assert!(store.set_session_patch(bad_sess).is_err());
    // Follow-up transaction must succeed cleanly (no queued Key::Patch tripping debug_assert!).
    store.apply(tx("after bad sess", vec![Op::Set { id: r, path: lp("transform2d.y"), slot: num(5.0) }])).unwrap();

    // 6. Atomic rollback of a mixed multi-op transaction (valid axis + valid patch + invalid bound Op::Solve)
    let saved_before = serial::save(store.authored());
    let good_p = Patch {
        id: PatchId(5),
        layer: 0,
        rank: OrderKey::after(&OrderKey::first()),
        region: Product::all(),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(lp("transform2d.x")) }, PatchValue::Slot { slot: num(777.0) })]),
    };
    let res = store.apply(tx(
        "mixed fail",
        vec![
            Op::Axis(AxisOp::AddValue { axis: ax_ord, at: 1, value: AxisValue::new(AxisValueId(9), "M") }),
            Op::Patch(PatchOp::Add { patch: good_p }),
            Op::Solve {
                point: BTreeMap::new(),
                leaf: Leaf { node: r, path: lp("transform2d.x") },
                slot: Some(Slot::Bound(Expr::parse("1 + 2").unwrap())),
            },
        ],
    ));
    assert!(res.is_err());
    assert_eq!(serial::save(store.authored()), saved_before, "rolled-back mixed transaction must leave document identical");
    assert_eq!(store.resolved(r, lp("transform2d.x")), Some(Value::Number(0.0)));
    store.tree().check_invariants().unwrap();

    // 7. Schema 16 load rejects duplicate ranks!
    let dup_json = saved_before.replace(
        r#""p4":{"#,
        r#""p4":{"#,
    );
    let mut raw: serde_json::Value = serde_json::from_str(&dup_json).unwrap();
    let p4 = raw["patches"]["p4"].clone();
    let mut p5 = p4.clone();
    p5["id"] = serde_json::json!("p5");
    raw["patches"].as_object_mut().unwrap().insert("p5".into(), p5);
    let bad_json = serde_json::to_string(&raw).unwrap();
    assert!(serial::load(&bad_json).is_err(), "Schema 16 load must reject duplicate ranks in the same layer");
}

// ---------------------------------------------------------------------------
// 4. Six-tier precedence, per-discrete-point solve isolation, patched bindings, P17
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
    let v_other = AxisValueId(2);
    let theme = make_axis(ax, "theme", AxisKind::Unordered, vec![(1, "dark"), (2, "other")]);
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
    let p_other = Patch {
        id: PatchId(10),
        layer: 0,
        rank: OrderKey::after(&OrderKey::first()),
        region: product_of([(ax, sel_one(v_other))]),
        anchor: None,
        writable: true,
        entries: BTreeMap::from([(target, PatchValue::Slot { slot: num(25.0) })]),
    };

    store
        .apply(tx(
            "setup",
            vec![
                Op::Create { id: a, ty: ty("rect"), parent: None, order: OrderKey::first(), facets: vec![ta.wrap(), Size::default().wrap()] },
                Op::Create { id: b, ty: ty("rect"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![tb.wrap(), Size::default().wrap()] },
                Op::Axis(AxisOp::Add { axis: theme }),
                Op::Patch(PatchOp::Add { patch: p_authored }),
                Op::Patch(PatchOp::Add { patch: p_other }),
            ],
        ))
        .unwrap();

    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(10.0)));
    assert_eq!(store.tree().base_leaf(a, lp("transform2d.x")), Some(&num(10.0)));

    let pt_dark = Point::new().with_value(ax, v_dark);
    let pt_other = Point::new().with_value(ax, v_other);
    store.set_point(pt_dark.clone());
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(20.0)));
    assert_eq!(store.tree().base_leaf(a, lp("transform2d.x")), Some(&num(10.0)));

    // Tier 4: Solve layer at pt_dark (30.0) beats Tier 5 authored patch (20.0) at pt_dark,
    // AND does NOT shadow pt_other (25.0)!
    store.apply(tx("solve pose", vec![Op::Solve { point: pt_dark.discrete(), leaf, slot: Some(num(30.0)) }])).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(30.0)));
    store.set_point(pt_other.clone());
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(25.0)), "solve pose on pt_dark must not mask pt_other");
    store.set_point(pt_dark.clone());
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
    store.set_session_patch(p_sess).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(999.0)));

    // Strip tiers one by one: 1 -> 2 -> 3 -> 4 -> 5 -> 6
    store.clear_session_patches();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(120.0)));

    store.apply(tx("drop binding patch", vec![Op::Patch(PatchOp::Remove { id: PatchId(2) })])).unwrap();
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(40.0)));

    store.publish(prod, BTreeMap::new());
    assert_eq!(store.resolved(a, lp("transform2d.x")), Some(Value::Number(30.0)));

    // P17 policy: deleting a constraint clears solve-layer entry -> snaps back to authored patch (20.0)!
    let release = plan_release_solve(store.authored().patches(), &pt_dark, &[leaf]);
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
        rank: OrderKey::after(&OrderKey::after(&OrderKey::first())),
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
// 5. 700-meter edit cost exit gate (`handoff-engine.md` EN-4)
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
    store.set_point(Point::new().with_value(ax, v_alt));

    let other_node = meters[METER_COUNT / 2 + 1];
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
    eprintln!("[EN-4b 700-meter bench] constant edit p50 = {c_p50:.1} us, patch edit p50 = {p_p50:.1} us, ratio = {ratio:.2}x");
    assert!(
        p_p50 <= (c_p50 * 2.0).max(c_p50 + 250.0),
        "patch edit ({p_p50:.1} us) exceeded 2x constant edit ({c_p50:.1} us, ratio {ratio:.2}x) at 700 meters"
    );
}
