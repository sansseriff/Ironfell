//! Text nodes (plans/specs/text.md): bounds from layout, the three box
//! sizings, content bound to values through `fmt`, a runtime signal
//! driving content, and re-measuring when a font arrives.

use iron_document::facet::{Font, Size, Slider, Text, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;
use std::sync::OnceLock;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn inter() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/fonts/Inter.ttf")).unwrap();
        iron_text::engine().register(bytes);
    });
}

fn text(store: &mut Store, content: Slot, sizing: &str) -> NodeId {
    let id = store.document().next_ids(1)[0];
    let mut t = Text::default();
    t.content = content;
    t.sizing = Slot::Const(Value::Enum(sizing.into()));
    let mut s = Size::default();
    s.w = Slot::Const(Value::Number(120.0));
    s.h = Slot::Const(Value::Number(200.0));
    store
        .apply(tx("text", vec![Op::Create { id, ty: ty("text"), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), t.wrap(), Font::default().wrap(), s.wrap()] }]))
        .unwrap();
    id
}

fn str_slot(s: &str) -> Slot {
    Slot::Const(Value::Str(s.into()))
}

#[test]
fn auto_width_bounds_fit_the_words() {
    inter();
    let mut store = Store::new();
    let short = text(&mut store, str_slot("Hi"), "auto_width");
    let long = text(&mut store, str_slot("Hello, wide world"), "auto_width");
    let a = store.local_bounds(short).unwrap().geometry;
    let b = store.local_bounds(long).unwrap().geometry;
    assert!(a.width() > 10.0 && b.width() > a.width() * 3.0, "{a:?} {b:?}");
    assert!((a.height() - b.height()).abs() < 0.01);
}

#[test]
fn sizing_modes() {
    inter();
    let mut store = Store::new();
    let words = "one two three four five six";
    let auto_h = text(&mut store, str_slot(words), "auto_height");
    let fixed = text(&mut store, str_slot(words), "fixed");
    let h = store.local_bounds(auto_h).unwrap().geometry;
    assert_eq!(h.width(), 120.0);
    assert!(h.height() > 50.0, "wrapped onto several lines: {h:?}");
    assert_eq!(store.local_bounds(fixed).unwrap().geometry.size(), kurbo::Size::new(120.0, 200.0));
}

#[test]
fn content_binds_through_fmt() {
    inter();
    let mut store = Store::new();
    let gauge = store.document().next_ids(1)[0];
    let mut sl = Slider::default();
    sl.value = Slot::Const(Value::Number(0.123456));
    store.apply(tx("gauge", vec![Op::Create { id: gauge, ty: ty("slider"), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), sl.wrap()] }])).unwrap();
    let readout = text(&mut store, Slot::Bound(Expr::parse(&format!("\"v = \" + fmt({gauge}.slider.value, 2)")).unwrap()), "auto_width");
    assert_eq!(iron_document::text::content(&store, readout), "v = 0.12");
    store.apply(tx("set", vec![Op::Set { id: gauge, path: path("slider.value"), slot: Slot::Const(Value::Number(0.987)) }])).unwrap();
    assert_eq!(iron_document::text::content(&store, readout), "v = 0.99");
}

#[test]
fn fmt_with_units() {
    let eval = |src: &str| Expr::parse(src).unwrap().eval(&|_| None).unwrap();
    assert_eq!(eval("fmt(1500 Pa, \"kPa\", 1)"), Value::Str("1.5 kPa".into()));
    assert_eq!(eval("fmt(0.5 kg)"), Value::Str("500 g".into()));
    assert_eq!(eval("fmt(3.14159)"), Value::Str("3.14159".into()));
    assert_eq!(eval("sci(0.000123, 1)"), Value::Str("1.2e-4".into()));
    assert_eq!(eval("str(true)"), Value::Str("true".into()));
    assert!(Expr::parse("fmt(2 m, \"kPa\")").unwrap().eval(&|_| None).is_err());
}

#[test]
fn a_signal_drives_content() {
    inter();
    let mut store = Store::new();
    let id = text(&mut store, str_slot("idle"), "auto_width");
    let before = store.local_bounds(id).unwrap().geometry.width();
    let p = store.register_producer("instrument");
    store.publish(p, BTreeMap::from([(Leaf { node: id, path: path("text.content") }, Value::Str("reading: 42.000 mV".into()))]));
    assert_eq!(iron_document::text::content(&store, id), "reading: 42.000 mV");
    assert!(store.local_bounds(id).unwrap().geometry.width() > before);
    // A number published onto text shows as a number.
    store.publish(p, BTreeMap::from([(Leaf { node: id, path: path("text.content") }, Value::Number(2.5))]));
    assert_eq!(iron_document::text::content(&store, id), "2.5");
}

#[test]
fn font_arrival_remeasures_text() {
    inter();
    let mut store = Store::new();
    let id = text(&mut store, str_slot("abc"), "auto_width");
    store.drain_changes();
    let _ = store.take_geometry_changes();
    assert!(!store.refresh_text(), "no font change yet");
    iron_text::engine().register(std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/fonts/SpaceGrotesk.ttf")).unwrap());
    assert!(store.refresh_text());
    assert!(store.drain_changes().iter().any(|c| c.leaf.node == id));
    assert!(store.take_geometry_changes().local.contains(&id));
}
