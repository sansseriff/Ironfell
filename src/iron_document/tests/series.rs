//! Series values, lambdas and paths drawn from SVG path data
//! (plans/specs/plot.md §1–§2).

use iron_document::facet::{PathShape, Slider, Transform2d};
use iron_document::geometry::Geometry;
use iron_document::units::Dim;
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

/// Evaluate `text` with no document.
fn eval(text: &str) -> Result<Value, String> {
    Expr::parse(text).map_err(|e| e.to_string())?.eval(&|_| None).map_err(|e| e.to_string())
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_series().unwrap_or_else(|| panic!("{v:?} is not a series")).0.to_vec()
}

#[test]
fn series_are_made_and_broadcast_with_units() {
    assert_eq!(nums(&eval("range(0, 1, 5)").unwrap()), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    assert_eq!(nums(&eval("step(0, 1, 0.5)").unwrap()), vec![0.0, 0.5, 1.0]);
    assert_eq!(nums(&eval("series(1, 2, 3) * 2 + 1").unwrap()), vec![3.0, 5.0, 7.0]);
    assert_eq!(nums(&eval("series(1, 2) + series(10, 20)").unwrap()), vec![11.0, 22.0]);
    assert_eq!(nums(&eval("-series(1, 2)").unwrap()), vec![-1.0, -2.0]);
    // Floor functions apply per element.
    let sq = nums(&eval("pow(series(1, 2, 3), 2)").unwrap());
    assert_eq!(sq, vec![1.0, 4.0, 9.0]);
    let s = nums(&eval("sinc(series(0, pi))").unwrap());
    assert!((s[0] - 1.0).abs() < 1e-12 && s[1].abs() < 1e-12);
    // Units ride along; dividing by one gives plain numbers.
    let nm = eval("range(400 nm, 700 nm, 4)").unwrap();
    assert_eq!(nm.as_series().unwrap().1, Dim::LENGTH);
    assert_eq!(nm.to_string(), "400 500 600 700 nm");
    assert_eq!(eval("range(400 nm, 700 nm, 4) / 1 nm").unwrap().as_series().unwrap().1, Dim::NONE);
    assert!(eval("range(400 nm, 700 nm, 4) + 1 mW").unwrap_err().contains("element by element"));
    assert!(eval("series(1, 2) + series(1, 2, 3)").unwrap_err().contains("different lengths"));
    // Reductions and reading.
    assert_eq!(eval("len(range(0, 1, 7))").unwrap(), Value::Number(7.0));
    assert_eq!(eval("max(series(3, 9, 4))").unwrap(), Value::Number(9.0));
    assert_eq!(eval("min(2, 5)").unwrap(), Value::Number(2.0), "two arguments still compare");
    assert_eq!(eval("sum(series(1, 2, 3))").unwrap(), Value::Number(6.0));
    assert_eq!(eval("at(series(5, 6, 7), 1)").unwrap(), Value::Number(6.0));
    assert_eq!(eval("at(series(5, 6, 7), 99)").unwrap(), Value::Number(7.0));
    assert_eq!(eval("interp(series(0, 10), series(0, 100), 2.5)").unwrap(), Value::Number(25.0));
    assert_eq!(nums(&eval("where(series(1, 2, 3) > 1.5, 1, 0)").unwrap()), vec![0.0, 1.0, 1.0]);
    // Nice ticks.
    assert_eq!(nums(&eval("ticks(0, 10, 5)").unwrap()), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
    assert_eq!(nums(&eval("ticks(-0.13, 0.92, 5)").unwrap()), vec![0.0, 0.2, 0.4, 0.6, 0.8]);
    assert_eq!(eval("nice_hi(-0.13, 0.92, 5)").unwrap(), Value::Number(1.0));
    assert_eq!(eval("nice_lo(-0.13, 0.92, 5)").unwrap(), Value::Number(-0.2));
    // A series condition is not one bool.
    assert!(eval("series(1, 2) > 1 ? 1 : 0").unwrap_err().contains("where"));
    // Literal round trip: a frozen series prints as an expression that parses back.
    let e = Expr::Lit(eval("range(1 mm, 3 mm, 3)").unwrap());
    assert_eq!(Expr::parse(&e.to_string()).unwrap().eval(&|_| None).unwrap(), eval("range(1 mm, 3 mm, 3)").unwrap());
}

#[test]
fn lambdas_parse_print_and_close_over_their_references() {
    // Parameters are locals; unknown bare names are still refused.
    assert!(Expr::parse("tau * 2").is_err());
    for text in ["x => x * 2", "(x, y) => hypot(x, y)", "map(series(1, 2), x => x + 1)", "f => f(2)"] {
        let e = Expr::parse(text).unwrap();
        assert_eq!(Expr::parse(&e.to_string()).unwrap(), e, "{text} → {e}");
    }
    assert_eq!(nums(&eval("map(series(1, 2, 3), x => x * x)").unwrap()), vec![1.0, 4.0, 9.0]);
    assert_eq!(nums(&eval("sample(x => 2 * x, 0, 1, 3)").unwrap()), vec![0.0, 1.0, 2.0]);
    assert_eq!(eval("((x, y) => x - y)(5, 2)").unwrap(), Value::Number(3.0));
    assert!(eval("map(series(1), (a, b) => a)").unwrap_err().contains("takes 2 arguments"));
    // A reference inside a lambda is a dependency of the binding that holds it.
    let slider: NodeId = "#3".parse().unwrap();
    let e = Expr::parse("x => x * #3.slider.value").unwrap();
    assert!(e.deps().contains(&expr::Leaf { node: slider, path: path("slider.value") }));
    // Evaluating closes it over the value then: a new value, a new function.
    let at = |v: f64| e.eval(&|_| Some(Value::Number(v))).unwrap();
    assert_ne!(at(2.0), at(3.0));
    let Value::Fn(f) = at(2.0) else { panic!("a function") };
    assert_eq!(f.apply(&[Value::Number(5.0)]).unwrap(), Value::Number(10.0));
    assert_eq!(f.to_string(), "x => x * 2");
    // An unresolved reference fails the lambda, not later calls.
    assert!(e.eval(&|_| None).is_err());
    // A function cannot be loaded as a constant.
    assert!(Value::from_json(&serde_json::json!({"fn": "x => x"})).is_err());
}

#[test]
fn path_data_writers_and_reading() {
    assert_eq!(eval("'M ' + points(series(0, 10, 20), series(0, 5, 0))").unwrap(), Value::Str("M 0,0 10,5 20,0".into()));
    // A non-finite sample breaks the line.
    assert_eq!(eval("'M ' + points(series(0, 1, 2, 3), series(0, 1 / 0, 2, 3))").unwrap(), Value::Str("M 0,0 M 2,2 3,3".into()));
    assert_eq!(eval("segments(series(0, 10), 0, series(0, 10), 4)").unwrap(), Value::Str("M 0,0 L 0,4 M 10,0 L 10,4".into()));
    assert!(eval("points(range(1 mm, 2 mm, 2), series(0, 1))").unwrap_err().contains("plain numbers"));
    let g = Geometry::from_svg_path("M 0,0 10,5 20,0").unwrap();
    assert_eq!(g.contours().len(), 1);
    assert_eq!(g.anchor_count(), 3);
    assert!(!g.contours()[0].closed);
    let circle = Geometry::from_svg_path(&eval("circles(series(5), series(5), 5)").unwrap().as_str().unwrap().to_owned()).unwrap();
    assert!(circle.contours()[0].closed);
    let b = circle.bounds();
    assert!((b.x0 - 0.0).abs() < 1e-6 && (b.x1 - 10.0).abs() < 1e-6 && (b.y1 - 10.0).abs() < 1e-6, "{b:?}");
    let area = Geometry::from_svg_path(&eval("area(series(0, 10), series(2, 4), 10)").unwrap().as_str().unwrap().to_owned()).unwrap();
    assert_eq!((area.contours().len(), area.anchor_count(), area.contours()[0].closed), (1, 4, true));
    assert!(Geometry::from_svg_path("M 0 0 Q").is_err());
}

#[test]
fn colours_from_expressions() {
    assert_eq!(eval("rgb(255, 0, 0)").unwrap(), Value::Color([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(eval("hsl(120, 100, 50)").unwrap(), Value::Color([0.0, 1.0, 0.0, 1.0]));
    assert_eq!(eval("mix(rgb(0, 0, 0), rgb(255, 255, 255), 0.5)").unwrap(), Value::Color([0.5, 0.5, 0.5, 1.0]));
    let Value::Color(c) = eval("spectrum(650 nm)").unwrap() else { panic!() };
    assert!(c[0] > 0.9 && c[1] < 0.1 && c[2] < 0.1, "{c:?}");
    // A quoted word in either style joins as text.
    assert_eq!(eval("'λ = ' + fmt(550 nm, 'nm', 0)").unwrap(), Value::Str("λ = 550 nm".into()));
}

/// A slider, and a path whose `d` is bound to a curve over it: moving the
/// slider redraws the path; a constant `d` written as path data parses; a
/// bound path refuses contour children.
#[test]
fn a_path_bound_to_path_data_follows_its_inputs() {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (slider, curve, fixed, contour) = (ids[0], ids[1], ids[2], ids[3]);
    let mut sl = Slider::default();
    sl.value = Slot::Const(Value::Number(1.0));
    let mut d = PathShape::default();
    d.d = Slot::Bound(Expr::parse(&format!("'M ' + points(range(0, 100, 11), {slider}.slider.value * 10 * sin(range(0, 100, 11) / 10))")).unwrap());
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    store
        .apply(tx(
            "draw",
            vec![
                Op::Create { id: slider, ty: ty("slider"), parent: None, order: k0, facets: vec![Transform2d::default().wrap(), sl.wrap()] },
                Op::Create { id: curve, ty: ty("path"), parent: None, order: k1.clone(), facets: vec![Transform2d::default().wrap(), d.wrap()] },
            ],
        ))
        .unwrap();
    assert!(contour::bound(store.document(), curve));
    let height = |store: &Store| contour::geometry(store, curve).bounds().height();
    let h1 = height(&store);
    assert_eq!(contour::geometry(&store, curve).anchor_count(), 11);
    store.apply(tx("taller", vec![Op::Set { id: slider, path: path("slider.value"), slot: Slot::Const(Value::Number(2.0)) }])).unwrap();
    let h2 = height(&store);
    assert!((h2 - 2.0 * h1).abs() < 1e-3, "{h1} → {h2}");
    // Path data that does not parse is a binding error, not a crash.
    store.apply(tx("bad", vec![Op::Set { id: curve, path: path("path.d"), slot: Slot::Bound(Expr::parse("'M 0 0 Q'").unwrap()) }])).unwrap();
    assert!(store.reactive().errors.keys().any(|l| l.node == curve), "{:?}", store.reactive().errors);
    // A constant `d` written as text is parsed into geometry.
    let mut text = serde_json::Map::new();
    text.insert("d".into(), serde_json::json!("M 0 0 L 30 0 L 30 30 Z"));
    let facet = Facet::from_json(FacetKind::PathShape, &serde_json::Value::Object(text)).unwrap();
    store.apply(tx("fixed", vec![Op::Create { id: fixed, ty: ty("path"), parent: None, order: OrderKey::after(&k1), facets: vec![Transform2d::default().wrap(), facet] }])).unwrap();
    let g = contour::packed(store.document(), fixed).unwrap();
    assert_eq!((g.anchor_count(), g.contours()[0].closed), (3, true));
    // A bound path cannot also hold contour children.
    let err = store.apply(tx("mixed", vec![Op::Create { id: contour, ty: ty("contour"), parent: Some(curve), order: OrderKey::first(), facets: vec![] }]));
    assert!(err.is_err());
}
