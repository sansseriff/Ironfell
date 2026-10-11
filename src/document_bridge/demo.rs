//! The starting scene, as a transaction. Replaces the ad-hoc demo systems: a
//! draggable rect, a field of small rects, a circle, and the torus. Until a
//! file can be loaded (plan step 4) this is what the app opens with.

use super::PendingTransactions;
use bevy::prelude::*;
use iron_document::component::{Composition, Constraint, Template, Fill, Mesh, Name as DocName, ParamSpec, Radius, Size, Stroke, Transform2d, Transform3d};
use iron_document::component::{Clip, Slider, Timing};
use iron_document::definition::{self, Placement};
use iron_document::{Actor, Expr, LeafPath, LeafStruct, Op, OrderKey, RelKind, Relation, Slot, TypeId, Value};
use std::collections::BTreeMap;

pub(super) fn queue_demo_scene(store: Res<super::DocumentStore>, mut pending: ResMut<PendingTransactions>) {
    pending.0.push(demo_scene(&store.0));
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

pub fn demo_scene(store: &iron_document::Store) -> iron_document::TransactionInput {
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let mut ids = store.document().next_ids(53).into_iter();
    let rel_ids = store.document().next_relation_ids(11);
    let mut next = move || ids.next().expect("reserved");
    let mut key = OrderKey::first();
    let mut order = move || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let mut ops = Vec::new();

    // The 2D composition every 2D panel shows by default (ECM §5's
    // migration: the old root group "scene" becomes a `free` composition).
    let composition = |dims: &str| {
        let mut c = Composition::default();
        c.dims = Slot::Const(Value::Enum(dims.into()));
        c.wrap()
    };
    let scene = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("scene".into()));
    ops.push(Op::Create { id: scene, ty: ty("composition"), parent: None, order: order(), components: vec![name.wrap(), composition("2d")] });

    // A field of small rects, deterministic positions.
    let mut seed: u32 = 0x91E2_33AB;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f64) / (u32::MAX as f64)
    };
    for _ in 0..20 {
        let mut t = Transform2d::default();
        t.x = n((rnd() * 1100.0).round());
        t.y = n((rnd() * 640.0).round());
        let mut s = Size::default();
        s.w = n(30.0);
        s.h = n(30.0);
        let mut f = Fill::default();
        f.color = Slot::Const(Value::Color([rnd() as f32, rnd() as f32, rnd() as f32, 1.0]));
        ops.push(Op::Create {
            id: next(),
            ty: ty("rect"),
            parent: Some(scene),
            order: order(),
            components: vec![t.wrap(), s.wrap(), f.wrap()],
        });
    }

    // The draggable square.
    let square = next();
    let mut t = Transform2d::default();
    t.x = n(500.0);
    t.y = n(420.0);
    let mut s = Size::default();
    s.w = n(80.0);
    s.h = n(80.0);
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.2, 0.2, 0.2, 1.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("square".into()));
    ops.push(Op::Create {
        id: square,
        ty: ty("rect"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), s.wrap(), f.wrap(), name.wrap()],
    });

    // A circle with a stroke.
    let mut t = Transform2d::default();
    t.x = n(160.0);
    t.y = n(330.0);
    let mut r = Radius::default();
    r.r = n(40.0);
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.95, 0.6, 0.1, 1.0]));
    let mut st = Stroke::default();
    st.width = n(3.0);
    ops.push(Op::Create {
        id: next(),
        ty: ty("circle"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), r.wrap(), f.wrap(), st.wrap()],
    });

    // A slider; the bar that follows it is an instance of a definition,
    // created at the end once the library exists.
    let slider = next();
    let mut t = Transform2d::default();
    t.x = n(100.0);
    t.y = n(600.0);
    let mut s = Size::default();
    s.w = n(300.0);
    s.h = n(24.0);
    let mut sl = Slider::default();
    sl.value = n(0.4);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("level".into()));
    ops.push(Op::Create {
        id: slider,
        ty: ty("slider"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), s.wrap(), sl.wrap(), name.wrap()],
    });
    // The torus, in a 3D composition of its own: the second panel.
    let assembly = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("assembly".into()));
    ops.push(Op::Create { id: assembly, ty: ty("composition"), parent: None, order: order(), components: vec![name.wrap(), composition("3d")] });
    let torus = next();
    let mut t = Transform3d::default();
    t.pos = Slot::Const(Value::Vec3([0.0, 1.5, 0.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("torus".into()));
    ops.push(Op::Create {
        id: torus,
        ty: ty("mesh"),
        parent: Some(assembly),
        order: order(),
        components: vec![t.wrap(), Mesh::default().wrap(), name.wrap()],
    });

    // A timeline with two clips (doc 05: scripted time). The clip names its
    // target through an `animates` relation, not a field.
    let timeline = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("main".into()));
    ops.push(Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: order(), components: vec![name.wrap()] });
    let mut clip_order = OrderKey::first();
    let mut clip = |id, label: &str, start: f64, dur: f64, easing: &str, prop: &str, from: Value, to: Value| {
        let mut timing = Timing::default();
        timing.start = n(start);
        timing.dur = n(dur);
        timing.easing = Slot::Const(Value::Enum(easing.into()));
        let mut c = Clip::default();
        c.prop = Slot::Const(Value::Str(prop.into()));
        c.from = Slot::Const(from);
        c.to = Slot::Const(to);
        let mut name = DocName::default();
        name.text = Slot::Const(Value::Str(label.into()));
        let op = Op::Create {
            id,
            ty: ty("clip"),
            parent: Some(timeline),
            order: clip_order.clone(),
            components: vec![timing.wrap(), c.wrap(), name.wrap()],
        };
        clip_order = OrderKey::after(&clip_order);
        op
    };
    let spin = next();
    ops.push(clip(spin, "spin", 1.0, 3.0, "ease_in_out", "transform3d.rot", Value::Vec3([0.0, 0.0, 0.0]), Value::Vec3([0.0, std::f64::consts::TAU, 0.0])));
    let slide = next();
    ops.push(clip(slide, "slide", 0.5, 2.0, "linear", "transform2d.x", Value::Number(500.0), Value::Number(700.0)));
    ops.push(Op::Link { rel: Relation { id: rel_ids[0], from: spin, to: torus, rel: RelKind::Animates, data: BTreeMap::new() } });
    ops.push(Op::Link { rel: Relation { id: rel_ids[1], from: slide, to: square, rel: RelKind::Animates, data: BTreeMap::new() } });

    // A shadow of the square, bound to its position: it follows the square
    // during a drag (roadmap stage A), and dragging the shadow moves the
    // square through linear inversion of `x = #square.x + 100`.
    let shadow = next();
    let mut t = Transform2d::default();
    t.x = Slot::Bound(Expr::parse(&format!("{square}.transform2d.x + 100")).expect("demo binding"));
    t.y = Slot::Bound(Expr::parse(&format!("{square}.transform2d.y")).expect("demo binding"));
    let mut s = Size::default();
    s.w = n(80.0);
    s.h = n(80.0);
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.2, 0.2, 0.2, 0.35]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("shadow".into()));
    ops.push(Op::Create {
        id: shadow,
        ty: ty("rect"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), s.wrap(), f.wrap(), name.wrap()],
    });

    // The inverse-kinematics pendulum (constraint-system.md §14 phase 3): a
    // fixed platform, three joints and a ball chained by `distance`
    // constraints whose value a slider drives, arms drawn between the
    // joints by bindings that read the solved positions, and a bar whose
    // height follows the ball. Grab the ball and the arms follow; move the
    // `fix` to the ball and the platform drives instead.
    let pendulum = next();
    let platform = next();
    let joints = [next(), next(), next()];
    let ball = next();
    let arms = [next(), next(), next()];
    let height_bar = next();
    let length = next();
    let fix = next();
    let hinge = next();
    let links = [next(), next(), next()];
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("pendulum".into()));
    ops.push(Op::Create { id: pendulum, ty: ty("group"), parent: None, order: order(), components: vec![name.wrap(), Transform2d::default().wrap()] });
    let mut porder = OrderKey::first();
    let mut porder_next = move || {
        let k = porder.clone();
        porder = OrderKey::after(&porder);
        k
    };
    let named = |s: &str| {
        let mut n = DocName::default();
        n.text = Slot::Const(Value::Str(s.into()));
        n
    };
    let color = |c: [f32; 4]| {
        let mut f = Fill::default();
        f.color = Slot::Const(Value::Color(c));
        f
    };
    // The arms paint under the joints and the ball (so a grab at a joint
    // takes the joint, not the bound arm), which means earlier sibling keys;
    // they are still created after the joints their bindings read.
    let arm_keys = [porder_next(), porder_next(), porder_next()];
    let mut t = Transform2d::default();
    t.x = n(240.0);
    t.y = n(40.0);
    let mut s = Size::default();
    s.w = n(160.0);
    s.h = n(20.0);
    ops.push(Op::Create { id: platform, ty: ty("rect"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), s.wrap(), color([0.25, 0.25, 0.28, 1.0]).wrap(), named("platform").wrap()] });
    let chain = [joints[0], joints[1], joints[2], ball];
    for (i, (x, y)) in [(320.0, 60.0), (320.0, 160.0), (320.0, 260.0)].into_iter().enumerate() {
        let mut t = Transform2d::default();
        t.x = n(x);
        t.y = n(y);
        let mut r = Radius::default();
        r.r = n(5.0);
        ops.push(Op::Create { id: joints[i], ty: ty("circle"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), r.wrap(), color([0.15, 0.15, 0.18, 1.0]).wrap(), named(&format!("j{i}")).wrap()] });
    }
    let mut t = Transform2d::default();
    t.x = n(320.0);
    t.y = n(360.0);
    let mut r = Radius::default();
    r.r = n(14.0);
    ops.push(Op::Create { id: ball, ty: ty("circle"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), r.wrap(), color([0.9, 0.35, 0.15, 1.0]).wrap(), named("ball").wrap()] });
    for i in 0..3 {
        let (a, b) = (chain[i], chain[i + 1]);
        let mut t = Transform2d::default();
        t.rot = Slot::Bound(Expr::parse(&format!("atan2({b}.transform2d.y - {a}.transform2d.y, {b}.transform2d.x - {a}.transform2d.x) - pi / 2")).expect("demo binding"));
        t.x = Slot::Bound(Expr::parse(&format!("{a}.transform2d.x - 5 * cos({}.transform2d.rot)", arms[i])).expect("demo binding"));
        t.y = Slot::Bound(Expr::parse(&format!("{a}.transform2d.y - 5 * sin({}.transform2d.rot)", arms[i])).expect("demo binding"));
        let mut s = Size::default();
        s.w = n(10.0);
        s.h = Slot::Bound(Expr::parse(&format!("hypot({b}.transform2d.x - {a}.transform2d.x, {b}.transform2d.y - {a}.transform2d.y)")).expect("demo binding"));
        ops.push(Op::Create { id: arms[i], ty: ty("rect"), parent: Some(pendulum), order: arm_keys[i].clone(), components: vec![t.wrap(), s.wrap(), color([0.35, 0.5, 0.7, 1.0]).wrap(), named(&format!("arm{}", i + 1)).wrap()] });
    }
    // A bar whose height is bound to the ball's y.
    let mut t = Transform2d::default();
    t.x = n(700.0);
    t.y = n(60.0);
    let mut s = Size::default();
    s.w = n(24.0);
    s.h = Slot::Bound(Expr::parse(&format!("{ball}.transform2d.y - 60")).expect("demo binding"));
    ops.push(Op::Create { id: height_bar, ty: ty("rect"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), s.wrap(), color([0.3, 0.65, 0.4, 1.0]).wrap(), named("height").wrap()] });
    // The length slider: 0.5 to 1.5 times 100 px per link.
    let mut t = Transform2d::default();
    t.x = n(100.0);
    t.y = n(660.0);
    let mut s = Size::default();
    s.w = n(300.0);
    s.h = n(24.0);
    let mut sl = Slider::default();
    sl.min = n(0.5);
    sl.max = n(1.5);
    sl.value = n(1.0);
    ops.push(Op::Create { id: length, ty: ty("slider"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), s.wrap(), sl.wrap(), named("length").wrap()] });
    // Constraints: the platform is fixed, its hinge coincides with the
    // first joint, and each link keeps its distance, driven by the slider.
    let constraint = |kind: &str, name: &str| {
        let mut c = Constraint::default();
        c.kind = Slot::Const(Value::Enum(kind.into()));
        vec![c.wrap(), named(name).wrap()]
    };
    let constrains = |rel, from, to, role: &str, anchor: [f64; 2]| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        data.insert("anchor".to_owned(), Value::Vec2(anchor));
        Op::Link { rel: Relation { id: rel, from, to, rel: RelKind::Constrains, data } }
    };
    ops.push(Op::Create { id: fix, ty: ty("constraint"), parent: Some(pendulum), order: porder_next(), components: constraint("fix", "fixed platform") });
    ops.push(constrains(rel_ids[2], fix, platform, "a", [0.0, 0.0]));
    ops.push(Op::Create { id: hinge, ty: ty("constraint"), parent: Some(pendulum), order: porder_next(), components: constraint("coincident", "hinge") });
    ops.push(constrains(rel_ids[3], hinge, platform, "a", [80.0, 20.0]));
    ops.push(constrains(rel_ids[4], hinge, joints[0], "b", [0.0, 0.0]));
    for i in 0..3 {
        let mut comps = constraint("distance", &format!("link{}", i + 1));
        if let iron_document::Component::Constraint(c) = &mut comps[0] {
            c.value = Slot::Bound(Expr::parse(&format!("{length}.slider.value * 100")).expect("demo binding"));
        }
        ops.push(Op::Create { id: links[i], ty: ty("constraint"), parent: Some(pendulum), order: porder_next(), components: comps });
        ops.push(constrains(rel_ids[5 + 2 * i], links[i], chain[i], "a", [0.0, 0.0]));
        ops.push(constrains(rel_ids[6 + 2 * i], links[i], chain[i + 1], "b", [0.0, 0.0]));
    }

    // The cartoon box: a 2D picture the torus can be linked to (ECM phase
    // 2's exit), and the model tree that entities live in.
    let cartoon_box = next();
    let mut t = Transform2d::default();
    t.x = n(520.0);
    t.y = n(590.0);
    let mut s = Size::default();
    s.w = n(110.0);
    s.h = n(70.0);
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.55, 0.35, 0.75, 1.0]));
    let mut st = Stroke::default();
    st.width = n(2.0);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("box".into()));
    ops.push(Op::Create { id: cartoon_box, ty: ty("rect"), parent: Some(scene), order: order(), components: vec![t.wrap(), s.wrap(), f.wrap(), st.wrap(), name.wrap()] });
    let model = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("model".into()));
    ops.push(Op::Create { id: model, ty: ty("model"), parent: None, order: order(), components: vec![name.wrap()] });

    // The library: a `bar` definition with one parameter, `level`, and a
    // "free" template whose rect binds its top and height to the
    // parameter's default. `bar` is data, not a registry type (ECM §4.2).
    let library = next();
    let def = next();
    let level = next();
    let template = next();
    let template_rect = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("library".into()));
    ops.push(Op::Create { id: library, ty: ty("library"), parent: None, order: order(), components: vec![name.wrap()] });
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("bar".into()));
    ops.push(Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: OrderKey::first(), components: vec![name.wrap()] });
    let mut spec = ParamSpec::default();
    spec.kind = Slot::Const(Value::Enum("number".into()));
    spec.default = n(0.4);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("level".into()));
    ops.push(Op::Create { id: level, ty: ty("param_spec"), parent: Some(def), order: OrderKey::first(), components: vec![spec.wrap(), name.wrap()] });
    ops.push(Op::Create {
        id: template,
        ty: ty("template"),
        parent: Some(def),
        order: OrderKey::after(&OrderKey::first()),
        components: vec![Template::default().wrap()],
    });
    let mut t = Transform2d::default();
    t.x = n(780.0);
    t.y = Slot::Bound(Expr::parse(&format!("640 - {level}.param_spec.default * 300")).expect("demo binding"));
    let mut s = Size::default();
    s.w = n(60.0);
    s.h = Slot::Bound(Expr::parse(&format!("{level}.param_spec.default * 300")).expect("demo binding"));
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.2, 0.5, 0.9, 1.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("bar".into()));
    ops.push(Op::Create {
        id: template_rect,
        ty: ty("rect"),
        parent: Some(template),
        order: OrderKey::first(),
        components: vec![t.wrap(), s.wrap(), f.wrap(), name.wrap()],
    });

    // Instantiate it into the scene. The ops so far are applied to a scratch
    // copy so the instantiation can read the definition and allocate ids
    // past everything above; all of it stays one transaction.
    let mut scratch = iron_document::Store::with_document(store.document().clone());
    scratch
        .apply(iron_document::TransactionInput { label: "scratch".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops: ops.clone(), exact: false })
        .expect("demo scene applies");
    let last_in_scene = scratch.document().children(scene).last().and_then(|&c| scratch.document().live(c)).map(|n| n.order.clone());
    let place = Placement { entity_parent: Some(model), entity_after: None, figure_parent: Some(scene), figure_after: last_in_scene.map(|k| OrderKey::after(&k)) };
    let (entity, inst) = definition::instantiate(scratch.document(), def, "free", "bar", &place).expect("bar instantiates");
    ops.extend(inst);
    // The entity's parameter follows the slider: the exit of ECM phase 1.
    let level_path: LeafPath = "params.level".parse().expect("parameter path");
    ops.push(Op::Set { id: entity, path: level_path, slot: Slot::Bound(Expr::parse(&format!("{slider}.slider.value")).expect("demo binding")) });

    iron_document::TransactionInput { label: "open demo scene".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false }
}
