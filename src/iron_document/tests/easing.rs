//! Easing as a function and Trim Paths (plans/specs/wires-and-formulas.md
//! §5): one curve vocabulary for keys and formulas; `ease`, `window`;
//! a shape's outline cut by arc length.

use iron_document::facet::{Clip, Key, Timing, Transform2d};
use iron_document::geometry::{trim, TrimMode};
use iron_document::time::{curve, Curve};
use iron_document::*;
use kurbo::{BezPath, ParamCurveArclen, Shape};

fn eval(text: &str) -> Result<Value, String> {
    Expr::parse(text).map_err(|e| e.to_string())?.eval(&|_| None).map_err(|e| e.to_string())
}

fn num(text: &str) -> f64 {
    eval(text).unwrap_or_else(|e| panic!("{text}: {e}")).as_f64().unwrap()
}

fn length(p: &BezPath) -> f64 {
    p.segments().map(|s| s.arclen(1e-6)).sum()
}

#[test]
fn curves_by_name_and_cubic_bezier() {
    for name in ["linear", "ease_in", "ease_out", "ease_in_out", "ease", "easy_ease", "hold"] {
        let c = curve(name).unwrap();
        assert_eq!((c.at(0.0), c.at(1.0)), (0.0, 1.0), "{name} ends");
        let mut last = 0.0;
        for i in 0..=100 {
            let y = c.at(i as f64 / 100.0);
            assert!(y >= last - 1e-9, "{name} is monotonic");
            last = y;
        }
    }
    // Easy Ease is symmetric: halfway in time is halfway in value, slow at the ends.
    let e = curve("easy_ease").unwrap();
    assert!((e.at(0.5) - 0.5).abs() < 1e-6);
    assert!(e.at(0.1) < 0.05 && e.at(0.9) > 0.95);
    // CSS's linear bezier is linear.
    let lin = curve("cubic-bezier(0, 0, 1, 1)").unwrap();
    assert!((lin.at(0.3) - 0.3).abs() < 1e-6);
    // An overshoot is allowed in y, not in x.
    assert!(matches!(curve("cubic-bezier(0.3, -0.5, 0.7, 1.5)"), Ok(Curve::Bezier(_))));
    assert!(curve("cubic-bezier(1.3, 0, 0.7, 1)").unwrap_err().contains("x1 and x2"));
    assert!(curve("bouncy").unwrap_err().contains("easy_ease"));
}

#[test]
fn ease_and_window_in_formulas() {
    assert_eq!(num("ease('easy_ease', 0.5)"), 0.5);
    assert!((num("ease('easy_ease', 0.25)") - curve("easy_ease").unwrap().at(0.25)).abs() < 1e-12);
    assert_eq!(num("ease(\"cubic-bezier(0, 0, 1, 1)\", 2)"), 1.0, "progress is clamped");
    assert!((num("window(0.3, 0.2, 0.6)") - 0.25).abs() < 1e-12);
    assert_eq!(num("window(0.1, 0.2, 0.6)"), 0.0);
    assert_eq!(num("window(0.9, 0.2, 0.6)"), 1.0);
    // The note's staggered draw-on: at progress 0.45 the axes are done, the
    // ticks half-way in time, the curve not started.
    let at = |a: f64, b: f64| num(&format!("ease('easy_ease', window(0.45, {a}, {b}))"));
    assert_eq!(at(0.0, 0.4), 1.0);
    assert!((at(0.3, 0.6) - 0.5).abs() < 1e-6);
    assert_eq!(at(0.5, 1.0), 0.0);
    // Per element over a series.
    assert_eq!(eval("ease('linear', series(0, 0.5, 2))").unwrap().as_series().unwrap().0.to_vec(), vec![0.0, 0.5, 1.0]);
    assert!(eval("ease('easy_ease', 1 mm)").unwrap_err().contains("plain number"));
}

#[test]
fn trim_cuts_an_outline_by_length() {
    let mut line = BezPath::new();
    line.move_to((0.0, 0.0));
    line.line_to((100.0, 0.0));
    line.line_to((100.0, 100.0));
    assert!((length(&trim(&line, 0.0, 0.25, 0.0, I)) - 50.0).abs() < 1e-6);
    let mid = trim(&line, 0.25, 0.75, 0.0, I);
    assert!((length(&mid) - 100.0).abs() < 1e-6);
    // One run across the corner, not two.
    assert_eq!(mid.elements().iter().filter(|e| matches!(e, kurbo::PathEl::MoveTo(_))).count(), 1);
    assert!(trim(&line, 0.5, 0.5, 0.0, I).elements().is_empty());
    assert_eq!(trim(&line, 0.0, 1.0, 0.3, I), line, "the whole outline, whatever the offset");
    // An offset wraps the kept span around the end.
    let wrapped = trim(&line, 0.0, 0.25, 0.9, I);
    assert!((length(&wrapped) - 50.0).abs() < 1e-6);
    assert_eq!(wrapped.elements().iter().filter(|e| matches!(e, kurbo::PathEl::MoveTo(_))).count(), 2);
    // A closed shape's outline, as a rect is drawn.
    let rect = kurbo::Rect::new(0.0, 0.0, 40.0, 10.0).to_path(0.1);
    assert!((length(&trim(&rect, 0.0, 0.5, 0.0, I)) - 50.0).abs() < 1e-6);
    // Wrapping over a closed shape's start joins there: one run, no notch.
    let over = trim(&rect, 0.0, 0.5, 0.875, I);
    assert!((length(&over) - 50.0).abs() < 1e-6);
    assert_eq!(moves(&over), 1, "{over:?}");
}

const I: TrimMode = TrimMode::Individually;

fn moves(p: &BezPath) -> usize {
    p.elements().iter().filter(|e| matches!(e, kurbo::PathEl::MoveTo(_))).count()
}

/// Several pieces (tick marks): simultaneously trims each by the same
/// fractions; individually runs through them one after another.
#[test]
fn trim_mode_treats_pieces_together_or_in_turn() {
    let mut ticks = BezPath::new();
    for x in [0.0, 10.0, 20.0, 30.0] {
        ticks.move_to((x, 0.0));
        ticks.line_to((x, 10.0));
    }
    let each = trim(&ticks, 0.0, 0.5, 0.0, TrimMode::Simultaneously);
    assert_eq!(moves(&each), 4);
    assert!((length(&each) - 20.0).abs() < 1e-6, "half of every tick");
    let along = trim(&ticks, 0.0, 0.5, 0.0, I);
    assert_eq!(moves(&along), 2, "the first two ticks whole");
    assert!((length(&along) - 20.0).abs() < 1e-6);
    // A closed piece kept whole stays closed.
    let mut two = kurbo::Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
    two.extend(kurbo::Rect::new(20.0, 0.0, 30.0, 10.0).to_path(0.1));
    let first = trim(&two, 0.0, 0.5, 0.0, I);
    assert!(first.elements().iter().any(|e| matches!(e, kurbo::PathEl::ClosePath)), "{first:?}");
}

/// A clip key takes the new curves by name.
#[test]
fn a_key_eases_with_easy_ease() {
    let mut store = Store::new();
    let ids = store.document().next_ids(5);
    let (dot, timeline, clip, k0, k1) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    let key = |t: f64, v: f64, easing: &str| {
        let mut k = Key::default();
        k.t = Slot::Const(Value::Number(t));
        k.value = Slot::Const(Value::Number(v));
        k.easing = Slot::Const(Value::Enum(easing.into()));
        k.wrap()
    };
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    let first = OrderKey::first();
    let ops = vec![
        Op::Create { id: dot, ty: TypeId::lookup("rect").unwrap(), parent: None, order: first.clone(), facets: vec![Transform2d::default().wrap(), facet::Size::default().wrap()] },
        Op::Create { id: timeline, ty: TypeId::lookup("timeline").unwrap(), parent: None, order: OrderKey::after(&first), facets: vec![] },
        Op::Create { id: clip, ty: TypeId::lookup("clip").unwrap(), parent: Some(timeline), order: first.clone(), facets: vec![Timing::default().wrap(), c.wrap()] },
        Op::Create { id: k0, ty: TypeId::lookup("key").unwrap(), parent: Some(clip), order: first.clone(), facets: vec![key(0.0, 0.0, "easy_ease")] },
        Op::Create { id: k1, ty: TypeId::lookup("key").unwrap(), parent: Some(clip), order: OrderKey::after(&first), facets: vec![key(1.0, 100.0, "linear")] },
        Op::Link { rel: Relation { id: store.document().next_relation_id(), from: clip, to: dot, rel: RelKind::Animates, data: Default::default() } },
    ];
    store.apply(TransactionInput { label: "clip".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
    store.set_time(0.25);
    let x = store.resolved(dot, "transform2d.x".parse().unwrap()).unwrap().as_f64().unwrap();
    assert!((x - 100.0 * curve("easy_ease").unwrap().at(0.25)).abs() < 1e-6, "{x}");
}
