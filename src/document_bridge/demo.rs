//! The starting scene, as a transaction. Replaces the ad-hoc demo systems: a
//! draggable rect, a field of small rects, a circle, and the torus. Until a
//! file can be loaded (plan step 4) this is what the app opens with.

use super::PendingTransactions;
use bevy::prelude::*;
use iron_document::component::{Composition, Constraint, Embed, Template, Fill, Mesh, Name as DocName, ParamSpec, Radius, Size, Stroke, Transform2d, Transform3d};
use iron_document::component::{Clip, Slider, Timing};
use iron_document::definition::{self, Placement};
use iron_document::{Actor, Expr, LeafPath, LeafStruct, NodeId, Op, OrderKey, RelKind, Relation, Slot, TypeId, Value};
use std::collections::BTreeMap;

pub(super) fn queue_demo_scene(store: Res<super::DocumentStore>, mut pending: ResMut<PendingTransactions>) {
    pending.0.push(demo_scene(&store.0));
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

pub fn demo_scene(store: &iron_document::Store) -> iron_document::TransactionInput {
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let mut ids = store.document().next_ids(61).into_iter();
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
        // Left of x≈790, so the main layout's 3D view at the right is clear.
        t.x = n((rnd() * 760.0).round());
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
    let radius = n(40.0);
    r.rx = radius.clone();
    r.ry = radius;
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.95, 0.6, 0.1, 1.0]));
    let mut st = Stroke::default();
    st.width = n(3.0);
    ops.push(Op::Create {
        id: next(),
        ty: ty("ellipse"),
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
        let radius = n(5.0);
        r.rx = radius.clone();
        r.ry = radius;
        ops.push(Op::Create { id: joints[i], ty: ty("ellipse"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), r.wrap(), color([0.15, 0.15, 0.18, 1.0]).wrap(), named(&format!("j{i}")).wrap()] });
    }
    let mut t = Transform2d::default();
    t.x = n(320.0);
    t.y = n(360.0);
    let mut r = Radius::default();
    let radius = n(14.0);
    r.rx = radius.clone();
    r.ry = radius;
    ops.push(Op::Create { id: ball, ty: ty("ellipse"), parent: Some(pendulum), order: porder_next(), components: vec![t.wrap(), r.wrap(), color([0.9, 0.35, 0.15, 1.0]).wrap(), named("ball").wrap()] });
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

    // The layout the viewer shows (ECM §7.4): `main` embeds the 2D scene
    // where it sits (so document coordinates read the same in the viewer)
    // and the 3D assembly as a camera view at the right, through the
    // derived default view that frames the torus.
    let main = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("main".into()));
    ops.push(Op::Create { id: main, ty: ty("composition"), parent: None, order: order(), components: vec![name.wrap(), composition("2d")] });
    let embed = |source: NodeId| {
        let mut e = Embed::default();
        e.source = Slot::Const(Value::Ref(source));
        e.wrap()
    };
    let scene_embed = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("scene view".into()));
    ops.push(Op::Create { id: scene_embed, ty: ty("embed"), parent: Some(main), order: OrderKey::first(), components: vec![embed(scene), Transform2d::default().wrap(), name.wrap()] });
    let assembly_embed = next();
    let mut t = Transform2d::default();
    t.x = n(815.0);
    t.y = n(10.0);
    let mut s = Size::default();
    s.w = n(200.0);
    s.h = n(350.0);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("assembly view".into()));
    ops.push(Op::Create { id: assembly_embed, ty: ty("embed"), parent: Some(main), order: OrderKey::after(&OrderKey::first()), components: vec![embed(assembly), t.wrap(), s.wrap(), name.wrap()] });

    // A 2D composition shown in the 3D assembly as a plane (ECM §7.4): a
    // small card beside the torus, 160×100 px, so 1.6×1 world units.
    let card = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("card".into()));
    ops.push(Op::Create { id: card, ty: ty("composition"), parent: None, order: order(), components: vec![name.wrap(), composition("2d")] });
    let card_shape = |id, ty_name: &str, x: f64, y: f64, w: f64, h: f64, c: [f32; 4], label: &str, key: OrderKey| {
        let mut t = Transform2d::default();
        t.x = n(x);
        t.y = n(y);
        let mut f = Fill::default();
        f.color = Slot::Const(Value::Color(c));
        let mut name = DocName::default();
        name.text = Slot::Const(Value::Str(label.into()));
        let geometry = if ty_name == "ellipse" {
            let mut r = Radius::default();
            let radius = n(w);
            r.rx = radius.clone();
            r.ry = radius;
            r.wrap()
        } else {
            let mut sz = Size::default();
            sz.w = n(w);
            sz.h = n(h);
            sz.wrap()
        };
        Op::Create { id, ty: ty(ty_name), parent: Some(card), order: key, components: vec![t.wrap(), geometry, f.wrap(), name.wrap()] }
    };
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    ops.push(card_shape(next(), "rect", 0.0, 0.0, 160.0, 100.0, [0.16, 0.22, 0.42, 1.0], "card panel", k0));
    ops.push(card_shape(next(), "ellipse", 40.0, 50.0, 22.0, 0.0, [0.98, 0.78, 0.2, 1.0], "card dot", k1));
    ops.push(card_shape(next(), "rect", 75.0, 42.0, 70.0, 16.0, [0.92, 0.94, 0.98, 1.0], "card bar", k2));
    let card_plane = next();
    let mut t = Transform3d::default();
    t.pos = Slot::Const(Value::Vec3([1.3, 2.6, -0.6]));
    t.rot = Slot::Const(Value::Vec3([0.0, -0.5, 0.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("card plane".into()));
    ops.push(Op::Create { id: card_plane, ty: ty("embed"), parent: Some(assembly), order: order(), components: vec![embed(card), t.wrap(), name.wrap()] });

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
    scratch
        .apply(iron_document::TransactionInput { label: "scratch".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops: inst.clone(), exact: false })
        .expect("instantiation applies");
    ops.extend(inst);

    // Semantic tabs (plans/specs/panels-and-session.md): the pendulum and the
    // level slider get compositions of their own, each a view in its own
    // tab, and `main` embeds them where they sat, so the everything-in-one
    // layout still reads as before. Their ids come after everything above,
    // so every earlier id stays put.
    let late = scratch.document().next_ids(4);
    let (pendulum_comp, controls_comp, pendulum_embed, controls_embed) = (late[0], late[1], late[2], late[3]);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("pendulum".into()));
    ops.push(Op::Create { id: pendulum_comp, ty: ty("composition"), parent: None, order: order(), components: vec![name.wrap(), composition("2d")] });
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("controls".into()));
    ops.push(Op::Create { id: controls_comp, ty: ty("composition"), parent: None, order: order(), components: vec![name.wrap(), composition("2d")] });
    ops.push(Op::Reparent { id: pendulum, parent: Some(pendulum_comp), order: OrderKey::first() });
    ops.push(Op::Reparent { id: slider, parent: Some(controls_comp), order: OrderKey::first() });
    let k_assembly = OrderKey::after(&OrderKey::first());
    let k_pendulum = OrderKey::after(&k_assembly);
    let k_controls = OrderKey::after(&k_pendulum);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("pendulum view".into()));
    ops.push(Op::Create { id: pendulum_embed, ty: ty("embed"), parent: Some(main), order: k_pendulum, components: vec![embed(pendulum_comp), Transform2d::default().wrap(), name.wrap()] });
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("controls view".into()));
    ops.push(Op::Create { id: controls_embed, ty: ty("embed"), parent: Some(main), order: k_controls, components: vec![embed(controls_comp), Transform2d::default().wrap(), name.wrap()] });
    // The entity's parameter follows the slider: the exit of ECM phase 1.
    let level_path: LeafPath = "params.level".parse().expect("parameter path");
    ops.push(Op::Set { id: entity, path: level_path, slot: Slot::Bound(Expr::parse(&format!("{slider}.slider.value")).expect("demo binding")) });

    // Stage F: laser → fiber → power meter, each a definition with ports,
    // drawn as a cartoon and as a schematic (ECM phase 3). Built last, from
    // a scratch store holding everything above, so every earlier id stays.
    ops.extend(optics(store, &ops, library, model));
    // Stage F step 3: a heater, a thermal link and an ambient sink, solved
    // as an across/through network (ECM §6.1).
    ops.extend(thermal(store, &ops, library, model));

    iron_document::TransactionInput { label: "open demo scene".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false }
}

/// The optics chain (ECM §6.1, phase 3's exit): definitions `laser`, `fiber`
/// and `meter` with `optical_power` ports and behaviours; one entity each;
/// figures in an `optics` (cartoon) and a `schematic` composition; two
/// connections. A slider sets the laser's power and one the fiber's length;
/// the meter's gauge in both compositions reads its in-port, and dragging a
/// gauge's edge sets the power slider back through the chain.
fn optics(base: &iron_document::Store, before: &[Op], library: NodeId, model: NodeId) -> Vec<Op> {
    use iron_document::component::{Connection, Handles, PathShape, Port};
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let mut scratch = iron_document::Store::with_document(base.document().clone());
    scratch
        .apply(iron_document::TransactionInput { label: "scratch".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops: before.to_vec(), exact: false })
        .expect("demo scene applies");
    let mut out: Vec<Op> = Vec::new();
    // Apply a phase to the scratch store and keep its ops.
    let mut phase = |scratch: &mut iron_document::Store, ops: Vec<Op>| {
        scratch
            .apply(iron_document::TransactionInput { label: "scratch".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops: ops.clone(), exact: false })
            .expect("optics demo applies");
        out.extend(ops);
    };
    let last = |s: &iron_document::Store, parent: Option<NodeId>| {
        let doc = s.document();
        let kids = match parent {
            Some(p) => doc.children(p).to_vec(),
            None => doc.roots().to_vec(),
        };
        kids.last().and_then(|&k| doc.live(k)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order))
    };
    let named = |s: &str| {
        let mut n = DocName::default();
        n.text = Slot::Const(Value::Str(s.into()));
        n.wrap()
    };
    let at = |x: f64, y: f64| {
        let mut t = Transform2d::default();
        t.x = n(x);
        t.y = n(y);
        t.wrap()
    };
    let size = |w: f64, h: f64| {
        let mut s = Size::default();
        s.w = n(w);
        s.h = n(h);
        s.wrap()
    };
    let fill = |c: [f32; 4]| {
        let mut f = Fill::default();
        f.color = Slot::Const(Value::Color(c));
        f.wrap()
    };
    let stroke = |c: [f32; 4], w: f64| {
        let mut s = Stroke::default();
        s.color = Slot::Const(Value::Color(c));
        s.width = n(w);
        s.wrap()
    };
    let port = |kind: &str, dir: &str, signal: Option<String>| {
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str(kind.into()));
        p.dir = Slot::Const(Value::Enum(dir.into()));
        // In the kind's unit, so the template's own bindings evaluate.
        p.signal = match signal {
            Some(e) => Slot::Bound(Expr::parse(&e).expect("demo behaviour")),
            None => Slot::Const(Value::parse_quantity("0 W").expect("quantity")),
        };
        p.wrap()
    };
    let template = |notation: &str| {
        let mut t = Template::default();
        t.notation = Slot::Const(Value::Str(notation.into()));
        t.wrap()
    };
    let composition = |dims: &str| {
        let mut c = Composition::default();
        c.dims = Slot::Const(Value::Enum(dims.into()));
        c.wrap()
    };
    let key = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let rect_op = |id: NodeId, parent: NodeId, order: OrderKey, comps: Vec<iron_document::Component>| Op::Create { id, ty: ty("rect"), parent: Some(parent), order, components: comps };

    // The two compositions.
    let ids = scratch.document().next_ids(2);
    let (cartoon, schematic) = (ids[0], ids[1]);
    let k0 = last(&scratch, None);
    let k1 = OrderKey::after(&k0);
    phase(&mut scratch, vec![
        Op::Create { id: cartoon, ty: ty("composition"), parent: None, order: k0, components: vec![named("optics"), composition("2d")] },
        Op::Create { id: schematic, ty: ty("composition"), parent: None, order: k1, components: vec![named("schematic"), composition("2d")] },
    ]);

    // Definitions. Each template draws its part of the picture where it sits
    // in the layout, cartoon and schematic alike.
    let gauge = |id: NodeId, parent: NodeId, order: OrderKey, input: NodeId, x: f64, y: f64, scale: f64, c: [f32; 4]| {
        let mut s = Size::default();
        s.w = Slot::Bound(Expr::parse(&format!("{input}.port.signal / 1 mW * {scale}")).expect("demo binding"));
        s.h = n(14.0);
        rect_op(id, parent, order, vec![at(x, y), s.wrap(), fill(c), named("reading")])
    };
    let ids = scratch.document().next_ids(28);
    let mut it = ids.into_iter();
    let mut next = move || it.next().expect("reserved");
    let (laser, power, laser_out, laser_cartoon, laser_schematic) = (next(), next(), next(), next(), next());
    let (fiber, loss, length, fiber_in, fiber_out, fiber_cartoon, fiber_schematic) = (next(), next(), next(), next(), next(), next(), next());
    let (meter, meter_in, meter_cartoon, meter_schematic) = (next(), next(), next(), next());
    // In the order they are created below: ids must ascend.
    let (laser_body, laser_beam) = (next(), next());
    let (fiber_path, fiber_a, fiber_b) = (next(), next(), next());
    let (dial, track, reading) = (next(), next(), next());
    let (laser_block, fiber_block, meter_block, schematic_reading) = (next(), next(), next(), next());
    let def_key = last(&scratch, Some(library));
    let spec = |id: NodeId, parent: NodeId, order: OrderKey, name: &str, default: Value| {
        let mut p = ParamSpec::default();
        p.kind = Slot::Const(Value::Enum(if matches!(default, Value::Quantity { .. }) { "quantity".into() } else { "number".into() }));
        p.default = Slot::Const(default);
        Op::Create { id, ty: ty("param_spec"), parent: Some(parent), order, components: vec![p.wrap(), named(name)] }
    };
    let mut ops = vec![
        Op::Create { id: laser, ty: ty("definition"), parent: Some(library), order: def_key.clone(), components: vec![named("laser")] },
        spec(power, laser, key(0), "power", Value::parse_quantity("10 mW").expect("quantity")),
        Op::Create { id: laser_out, ty: ty("port_spec"), parent: Some(laser), order: key(1), components: vec![named("out"), port("optical_power", "out", Some(format!("{power}.param_spec.default")))] },
        Op::Create { id: laser_cartoon, ty: ty("template"), parent: Some(laser), order: key(2), components: vec![template("cartoon")] },
        Op::Create { id: laser_schematic, ty: ty("template"), parent: Some(laser), order: key(3), components: vec![template("schematic")] },
        Op::Create { id: fiber, ty: ty("definition"), parent: Some(library), order: OrderKey::after(&def_key), components: vec![named("fiber")] },
        spec(loss, fiber, key(0), "loss_db_per_km", Value::Number(0.5)),
        spec(length, fiber, key(1), "length_km", Value::Number(2.0)),
        Op::Create { id: fiber_in, ty: ty("port_spec"), parent: Some(fiber), order: key(2), components: vec![named("in"), port("optical_power", "in", None)] },
        Op::Create {
            id: fiber_out,
            ty: ty("port_spec"),
            parent: Some(fiber),
            order: key(3),
            components: vec![named("out"), port("optical_power", "out", Some(format!("{fiber_in}.port.signal * pow(10, -{loss}.param_spec.default * {length}.param_spec.default / 10)")))],
        },
        Op::Create { id: fiber_cartoon, ty: ty("template"), parent: Some(fiber), order: key(4), components: vec![template("cartoon")] },
        Op::Create { id: fiber_schematic, ty: ty("template"), parent: Some(fiber), order: key(5), components: vec![template("schematic")] },
        Op::Create { id: meter, ty: ty("definition"), parent: Some(library), order: OrderKey::after(&OrderKey::after(&def_key)), components: vec![named("meter")] },
        Op::Create { id: meter_in, ty: ty("port_spec"), parent: Some(meter), order: key(0), components: vec![named("in"), port("optical_power", "in", None)] },
        Op::Create { id: meter_cartoon, ty: ty("template"), parent: Some(meter), order: key(1), components: vec![template("cartoon")] },
        Op::Create { id: meter_schematic, ty: ty("template"), parent: Some(meter), order: key(2), components: vec![template("schematic")] },
    ];
    // Cartoon: the laser's body and a beam whose opacity follows the power.
    ops.push(rect_op(laser_body, laser_cartoon, key(0), vec![at(60.0, 140.0), size(130.0, 70.0), fill([0.18, 0.2, 0.26, 1.0]), named("body")]));
    let mut beam = Fill::default();
    beam.color = Slot::Const(Value::Color([0.95, 0.15, 0.1, 1.0]));
    beam.opacity = Slot::Bound(Expr::parse(&format!("clamp({power}.param_spec.default / 20 mW, 0.15, 1)")).expect("demo binding"));
    ops.push(rect_op(laser_beam, laser_cartoon, key(1), vec![at(190.0, 168.0), size(50.0, 14.0), beam.wrap(), named("beam")]));
    // The fiber: a curve from the laser to the meter.
    let mut shape = PathShape::default();
    shape.closed = Slot::Const(Value::Bool(false));
    ops.push(Op::Create { id: fiber_path, ty: ty("path"), parent: Some(fiber_cartoon), order: key(0), components: vec![at(240.0, 175.0), shape.wrap(), stroke([0.96, 0.7, 0.15, 1.0], 6.0), named("fiber")] });
    let point = |id: NodeId, order: OrderKey, x: f64, y: f64, ox: f64, oy: f64| {
        let mut h = Handles::default();
        h.ix = n(-ox);
        h.iy = n(-oy);
        h.ox = n(ox);
        h.oy = n(oy);
        Op::Create { id, ty: ty("point"), parent: Some(fiber_path), order, components: vec![at(x, y), h.wrap()] }
    };
    ops.push(point(fiber_a, key(0), 0.0, 0.0, 90.0, 120.0));
    ops.push(point(fiber_b, key(1), 270.0, 0.0, 90.0, -120.0));
    // The meter: a dial, a gauge track, and the reading.
    let mut dial_r = Radius::default();
    let radius = n(55.0);
    dial_r.rx = radius.clone();
    dial_r.ry = radius;
    ops.push(Op::Create { id: dial, ty: ty("ellipse"), parent: Some(meter_cartoon), order: key(0), components: vec![at(600.0, 175.0), dial_r.wrap(), fill([0.88, 0.9, 0.94, 1.0]), stroke([0.3, 0.32, 0.38, 1.0], 3.0), named("dial")] });
    ops.push(rect_op(track, meter_cartoon, key(1), vec![at(540.0, 260.0), size(240.0, 14.0), fill([0.85, 0.86, 0.9, 1.0]), named("track")]));
    ops.push(gauge(reading, meter_cartoon, key(2), meter_in, 540.0, 260.0, 24.0, [0.15, 0.7, 0.35, 1.0]));
    // Schematic: blocks, and the meter's reading as a bar under its block.
    let block = |id: NodeId, parent: NodeId, x: f64, w: f64| rect_op(id, parent, key(0), vec![at(x, 100.0), size(w, 50.0), fill([1.0, 1.0, 1.0, 1.0]), stroke([0.1, 0.1, 0.12, 1.0], 2.0), named("block")]);
    ops.push(block(laser_block, laser_schematic, 60.0, 90.0));
    ops.push(block(fiber_block, fiber_schematic, 240.0, 120.0));
    ops.push(block(meter_block, meter_schematic, 450.0, 90.0));
    ops.push(gauge(schematic_reading, meter_schematic, key(1), meter_in, 450.0, 170.0, 9.0, [0.1, 0.45, 0.9, 1.0]));
    phase(&mut scratch, ops);

    // Entities: each instantiated in the cartoon, then drawn again in the
    // schematic.
    let mut entities = Vec::new();
    for (def, name) in [(laser, "laser"), (fiber, "fiber"), (meter, "meter")] {
        let doc = scratch.document();
        let place = Placement {
            entity_parent: Some(model),
            entity_after: doc.children(model).last().and_then(|&c| doc.live(c)).map(|c| c.order.clone()),
            figure_parent: Some(cartoon),
            figure_after: doc.children(cartoon).last().and_then(|&c| doc.live(c)).map(|c| c.order.clone()),
        };
        let (e, ops) = definition::instantiate(scratch.document(), def, "cartoon", name, &place).expect("optics instantiates");
        phase(&mut scratch, ops);
        let after = scratch.document().children(schematic).last().and_then(|&c| scratch.document().live(c)).map(|c| c.order.clone());
        let ops = definition::place_figures(scratch.document(), e, "schematic", Some(schematic), after).expect("schematic figures");
        phase(&mut scratch, ops);
        entities.push(e);
    }
    let port_of = |s: &iron_document::Store, e: NodeId, name: &str| {
        let doc = s.document();
        doc.children(e).iter().copied().find(|&p| doc.get::<DocName>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).expect("port")
    };

    // The connections, and their wires in the schematic.
    for (i, (from, to)) in [((entities[0], "out"), (entities[1], "in")), ((entities[1], "out"), (entities[2], "in"))].into_iter().enumerate() {
        let (a, b) = (port_of(&scratch, from.0, from.1), port_of(&scratch, to.0, to.1));
        let id = scratch.document().next_ids(2);
        let rels = scratch.document().next_relation_ids(3);
        let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
        let (x0, x1) = if i == 0 { (150.0, 240.0) } else { (360.0, 450.0) };
        let mut shape = PathShape::default();
        shape.closed = Slot::Const(Value::Bool(false));
        let wire_key = last(&scratch, Some(schematic));
        let model_key = last(&scratch, Some(model));
        let mut c = Connection::default();
        c.domain = Slot::Const(Value::Str("optical".into()));
        phase(&mut scratch, vec![
            Op::Create { id: id[0], ty: ty("connection"), parent: Some(model), order: model_key, components: vec![c.wrap()] },
            Op::Link { rel: Relation { id: rels[0], from: id[0], to: a, rel: RelKind::Connects, data: end("a") } },
            Op::Link { rel: Relation { id: rels[1], from: id[0], to: b, rel: RelKind::Connects, data: end("b") } },
            Op::Create { id: id[1], ty: ty("path"), parent: Some(schematic), order: wire_key, components: vec![at(x0, 125.0), shape.wrap(), stroke([0.1, 0.1, 0.12, 1.0], 2.0), named("wire")] },
            Op::Link { rel: Relation { id: rels[2], from: id[1], to: id[0], rel: RelKind::Figures, data: BTreeMap::new() } },
        ]);
        let pts = scratch.document().next_ids(2);
        phase(&mut scratch, vec![
            Op::Create { id: pts[0], ty: ty("point"), parent: Some(id[1]), order: key(0), components: vec![at(0.0, 0.0), Handles::default().wrap()] },
            Op::Create { id: pts[1], ty: ty("point"), parent: Some(id[1]), order: key(1), components: vec![at(x1 - x0, 0.0), Handles::default().wrap()] },
        ]);
    }

    // The controls, in the cartoon: the laser's power (0–20 mW) and the
    // fiber's length (0–20 km) follow two sliders.
    let ids = scratch.document().next_ids(2);
    let slider_op = |id: NodeId, y: f64, value: f64, name: &str, order: OrderKey| {
        let mut sl = Slider::default();
        sl.min = n(0.0);
        sl.max = n(20.0);
        sl.step = n(0.1);
        sl.value = n(value);
        Op::Create { id, ty: ty("slider"), parent: Some(cartoon), order, components: vec![at(60.0, y), size(300.0, 24.0), sl.wrap(), named(name)] }
    };
    let k = last(&scratch, Some(cartoon));
    phase(&mut scratch, vec![slider_op(ids[0], 340.0, 10.0, "laser power", k.clone()), slider_op(ids[1], 390.0, 2.0, "fiber length", OrderKey::after(&k))]);
    phase(&mut scratch, vec![
        Op::Set { id: entities[0], path: "params.power".parse().expect("parameter"), slot: Slot::Bound(Expr::parse(&format!("{}.slider.value * 1 mW", ids[0])).expect("demo binding")) },
        Op::Set { id: entities[1], path: "params.length_km".parse().expect("parameter"), slot: Slot::Bound(Expr::parse(&format!("{}.slider.value", ids[1])).expect("demo binding")) },
    ]);
    out
}

/// A thermal network (ECM §6.1 step 3): a heater (a heat source), a thermal
/// link (ΔT = R·Q) and an ambient sink (a fixed temperature), each a
/// definition with `heat_flow` ports and behaviour equations, connected and
/// solved together. Sliders set the heater's power and the link's
/// resistance; gauges show the heater's temperature rise and the heat flow.
fn thermal(base: &iron_document::Store, before: &[Op], library: NodeId, model: NodeId) -> Vec<Op> {
    use iron_document::component::{Equation, Port};
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let mut scratch = iron_document::Store::with_document(base.document().clone());
    scratch
        .apply(iron_document::TransactionInput { label: "scratch".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops: before.to_vec(), exact: false })
        .expect("demo scene applies");
    let mut out: Vec<Op> = Vec::new();
    let mut phase = |scratch: &mut iron_document::Store, ops: Vec<Op>| {
        scratch
            .apply(iron_document::TransactionInput { label: "scratch".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops: ops.clone(), exact: false })
            .expect("thermal demo applies");
        out.extend(ops);
    };
    let after = |s: &iron_document::Store, parent: Option<NodeId>| {
        let doc = s.document();
        let kids = match parent {
            Some(p) => doc.children(p).to_vec(),
            None => doc.roots().to_vec(),
        };
        kids.last().and_then(|&k| doc.live(k)).map(|n| n.order.clone())
    };
    let next_key = |k: Option<OrderKey>| k.map_or_else(OrderKey::first, |k| OrderKey::after(&k));
    let key = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let named = |s: &str| {
        let mut n = DocName::default();
        n.text = Slot::Const(Value::Str(s.into()));
        n.wrap()
    };
    let rect = |id: NodeId, parent: NodeId, order: OrderKey, x: f64, y: f64, w: Slot, h: f64, c: [f32; 4], name: &str| {
        let mut t = Transform2d::default();
        t.x = n(x);
        t.y = n(y);
        let mut sz = Size::default();
        sz.w = w;
        sz.h = n(h);
        let mut f = Fill::default();
        f.color = Slot::Const(Value::Color(c));
        Op::Create { id, ty: ty("rect"), parent: Some(parent), order, components: vec![t.wrap(), sz.wrap(), f.wrap(), named(name)] }
    };
    let bound = |e: String| Slot::Bound(Expr::parse(&e).expect("demo binding"));
    let port = |name: &str| {
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str("heat_flow".into()));
        p.dir = Slot::Const(Value::Enum("both".into()));
        // In the kind's units: the template's gauges read these, and an
        // instance's are solved (the physics layer reads over them).
        p.across = Slot::Const(Value::parse_quantity("293.15 K").expect("quantity"));
        p.through = Slot::Const(Value::parse_quantity("0 W").expect("quantity"));
        vec![named(name), p.wrap()]
    };
    let equation = |lhs: String, rhs: String| {
        let mut e = Equation::default();
        e.lhs = Slot::Bound(Expr::parse(&lhs).expect("demo equation"));
        e.rhs = Slot::Bound(Expr::parse(&rhs).expect("demo equation"));
        e.wrap()
    };
    let spec = |id: NodeId, parent: NodeId, order: OrderKey, name: &str, default: &str| {
        let mut p = ParamSpec::default();
        p.kind = Slot::Const(Value::Enum("quantity".into()));
        p.default = Slot::Const(Value::parse_quantity(default).expect("quantity"));
        Op::Create { id, ty: ty("param_spec"), parent: Some(parent), order, components: vec![p.wrap(), named(name)] }
    };
    let template = |id: NodeId, parent: NodeId, order: OrderKey| {
        let mut t = Template::default();
        t.notation = Slot::Const(Value::Str("cartoon".into()));
        Op::Create { id, ty: ty("template"), parent: Some(parent), order, components: vec![t.wrap()] }
    };

    // The composition.
    let comp = scratch.document().next_ids(1)[0];
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let root_key = next_key(after(&scratch, None));
    phase(&mut scratch, vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: root_key, components: vec![named("thermal"), c.wrap()] }]);

    // Definitions, in creation order (ids ascend).
    let ids = scratch.document().next_ids(23);
    let mut it = ids.into_iter();
    let mut next = move || it.next().expect("reserved");
    let (heater, power, h_out, h_eq, h_t, h_body, h_track, h_rise, h_flow) = (next(), next(), next(), next(), next(), next(), next(), next(), next());
    let (link, r, l_a, l_b, l_eq1, l_eq2, l_t, l_body) = (next(), next(), next(), next(), next(), next(), next(), next());
    let (ambient, t, a_p, a_eq, a_t, a_body) = (next(), next(), next(), next(), next(), next());
    let k0 = next_key(after(&scratch, Some(library)));
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    phase(&mut scratch, vec![
        Op::Create { id: heater, ty: ty("definition"), parent: Some(library), order: k0, components: vec![named("heater")] },
        spec(power, heater, key(0), "power", "5 W"),
        Op::Create { id: h_out, ty: ty("port_spec"), parent: Some(heater), order: key(1), components: port("out") },
        Op::Create { id: h_eq, ty: ty("equation"), parent: Some(heater), order: key(2), components: vec![equation(format!("{h_out}.port.through"), format!("-{power}.param_spec.default"))] },
        template(h_t, heater, key(3)),
        rect(h_body, h_t, key(0), 60.0, 110.0, n(120.0), 90.0, [0.86, 0.3, 0.15, 1.0], "heater"),
        rect(h_track, h_t, key(1), 60.0, 240.0, n(300.0), 14.0, [0.85, 0.86, 0.9, 1.0], "track"),
        // The temperature rise above 20 °C: 6 px per kelvin.
        rect(h_rise, h_t, key(2), 60.0, 240.0, bound(format!("clamp(({h_out}.port.across - 293.15 K) / 1 K * 6, 0, 300)")), 14.0, [0.9, 0.2, 0.1, 1.0], "temperature"),
        // The heat leaving the heater: 12 px per watt.
        rect(h_flow, h_t, key(3), 60.0, 266.0, bound(format!("clamp(-{h_out}.port.through / 1 W * 12, 0, 300)")), 10.0, [0.95, 0.65, 0.15, 1.0], "heat flow"),
        Op::Create { id: link, ty: ty("definition"), parent: Some(library), order: k1, components: vec![named("thermal link")] },
        spec(r, link, key(0), "r", "2 K/W"),
        Op::Create { id: l_a, ty: ty("port_spec"), parent: Some(link), order: key(1), components: port("a") },
        Op::Create { id: l_b, ty: ty("port_spec"), parent: Some(link), order: key(2), components: port("b") },
        Op::Create { id: l_eq1, ty: ty("equation"), parent: Some(link), order: key(3), components: vec![equation(format!("{l_a}.port.through + {l_b}.port.through"), "0 W".into())] },
        Op::Create { id: l_eq2, ty: ty("equation"), parent: Some(link), order: key(4), components: vec![equation(format!("{l_a}.port.across - {l_b}.port.across"), format!("{r}.param_spec.default * {l_a}.port.through"))] },
        template(l_t, link, key(5)),
        rect(l_body, l_t, key(0), 180.0, 140.0, n(150.0), 30.0, [0.55, 0.57, 0.62, 1.0], "link"),
        Op::Create { id: ambient, ty: ty("definition"), parent: Some(library), order: k2, components: vec![named("ambient")] },
        spec(t, ambient, key(0), "t", "293.15 K"),
        Op::Create { id: a_p, ty: ty("port_spec"), parent: Some(ambient), order: key(1), components: port("p") },
        Op::Create { id: a_eq, ty: ty("equation"), parent: Some(ambient), order: key(2), components: vec![equation(format!("{a_p}.port.across"), format!("{t}.param_spec.default"))] },
        template(a_t, ambient, key(3)),
        rect(a_body, a_t, key(0), 330.0, 95.0, n(140.0), 120.0, [0.55, 0.75, 0.95, 1.0], "sink"),
    ]);

    let mut entities = Vec::new();
    for (def, name) in [(heater, "heater"), (link, "thermal link"), (ambient, "ambient")] {
        let place = Placement { entity_parent: Some(model), entity_after: after(&scratch, Some(model)), figure_parent: Some(comp), figure_after: after(&scratch, Some(comp)) };
        let (e, ops) = definition::instantiate(scratch.document(), def, "cartoon", name, &place).expect("thermal instantiates");
        phase(&mut scratch, ops);
        entities.push(e);
    }
    let port_of = |s: &iron_document::Store, e: NodeId, name: &str| {
        let doc = s.document();
        doc.children(e).iter().copied().find(|&p| doc.get::<DocName>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).expect("port")
    };
    for (from, to) in [((entities[0], "out"), (entities[1], "a")), ((entities[1], "b"), (entities[2], "p"))] {
        let (a, b) = (port_of(&scratch, from.0, from.1), port_of(&scratch, to.0, to.1));
        let id = scratch.document().next_ids(1)[0];
        let rels = scratch.document().next_relation_ids(2);
        let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
        let order = next_key(after(&scratch, Some(model)));
        let mut c = iron_document::component::Connection::default();
        c.domain = Slot::Const(Value::Str("thermal".into()));
        phase(&mut scratch, vec![
            Op::Create { id, ty: ty("connection"), parent: Some(model), order, components: vec![c.wrap()] },
            Op::Link { rel: Relation { id: rels[0], from: id, to: a, rel: RelKind::Connects, data: end("a") } },
            Op::Link { rel: Relation { id: rels[1], from: id, to: b, rel: RelKind::Connects, data: end("b") } },
        ]);
    }
    // The controls: heater power 0–20 W, link resistance 0.5–5 K/W.
    let ids = scratch.document().next_ids(2);
    let slider = |id: NodeId, y: f64, min: f64, max: f64, value: f64, name: &str, order: OrderKey| {
        let mut sl = Slider::default();
        sl.min = n(min);
        sl.max = n(max);
        sl.step = n(0.1);
        sl.value = n(value);
        let mut tr = Transform2d::default();
        tr.x = n(60.0);
        tr.y = n(y);
        let mut sz = Size::default();
        sz.w = n(300.0);
        sz.h = n(24.0);
        Op::Create { id, ty: ty("slider"), parent: Some(comp), order, components: vec![tr.wrap(), sz.wrap(), sl.wrap(), named(name)] }
    };
    let k = next_key(after(&scratch, Some(comp)));
    phase(&mut scratch, vec![slider(ids[0], 320.0, 0.0, 20.0, 5.0, "heater power", k.clone()), slider(ids[1], 370.0, 0.5, 5.0, 2.0, "link resistance", OrderKey::after(&k))]);
    phase(&mut scratch, vec![
        Op::Set { id: entities[0], path: "params.power".parse().expect("parameter"), slot: Slot::Bound(Expr::parse(&format!("{}.slider.value * 1 W", ids[0])).expect("demo binding")) },
        Op::Set { id: entities[1], path: "params.r".parse().expect("parameter"), slot: Slot::Bound(Expr::parse(&format!("{}.slider.value * 1 K/W", ids[1])).expect("demo binding")) },
    ]);
    out
}

