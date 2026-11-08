//! `std:plot` (plans/specs/plot.md §4): placed by the model's `<place>`,
//! drawn from series or a function, labelled by its data's units, and
//! reflowing when its box is resized.

use iron_document::facet::{Composition, Name};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false }
}

/// A 2D composition and a model root.
fn desk() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store
        .apply(tx(
            "desk",
            vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
            ],
        ))
        .unwrap();
    (store, ids[0])
}

/// Place a plot; answer (entity, use).
fn place(store: &mut Store, attrs: &str) -> (NodeId, NodeId) {
    let markup = format!(r#"<place def="std:plot" in="bench" x="20" y="20" {attrs}/>"#);
    let (ops, summary) = compose::compose_ops(store, &markup).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("place", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let get = |a: &str| -> NodeId {
        let start = line.find(&format!("{a}=\"")).unwrap() + a.len() + 2;
        line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
    };
    (get("entity"), get("use"))
}

/// The plot's part named `name`.
fn part(store: &Store, u: NodeId, name: &str) -> NodeId {
    let doc = store.document();
    doc.descendants(u).into_iter().find(|&n| doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap_or_else(|| panic!("no part {name}"))
}

/// Every binding error inside the plot (its picture or its entity's ports).
fn errors(store: &Store, e: NodeId, u: NodeId) -> Vec<String> {
    store.reactive().errors.iter().filter(|(l, _)| l.node == u || l.node == e || l.node.expanded_parts().is_some_and(|(o, _)| o == u || o == e)).map(|(l, m)| format!("{l}: {m}")).collect()
}

fn text(store: &Store, id: NodeId) -> String {
    store.resolved(id, path("text.content")).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn anchors(store: &Store, id: NodeId) -> usize {
    contour::geometry(store, id).anchor_count()
}

#[test]
fn a_default_plot_draws_its_sample_with_ticks_and_no_errors() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, "");
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    assert_eq!(anchors(&store, part(&store, u, "series 1")), 41);
    assert_eq!(anchors(&store, part(&store, u, "series 2")), 0, "an empty series draws nothing");
    assert!(anchors(&store, part(&store, u, "grid")) > 4);
    assert_eq!(text(&store, part(&store, u, "x tick 0")), "0");
    assert_eq!(text(&store, part(&store, u, "x tick 7")), "", "unused slots are empty");
    assert_eq!(store.resolved(e, path("args.style")), Some(Value::Enum("line".into())));
}

#[test]
fn units_title_the_axes_and_a_resize_reflows() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"w="480" h="300" title="Intensity" xs="{= range(400 nm, 700 nm, 61)}" ys="{= pow(sin(pi * ($self.xs - 400 nm) / 100 nm), 2) * 1 mW}" style="area""#);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    assert_eq!(text(&store, part(&store, u, "x title")), "nm");
    assert_eq!(text(&store, part(&store, u, "y title")), "mW");
    assert_eq!(text(&store, part(&store, u, "x tick 0")), "400");
    let width = |s: &Store| contour::geometry(s, part(s, u, "series 1")).bounds().width();
    let before = width(&store);
    store.apply(tx("wider", vec![Op::Set { id: u, path: path("size.w"), slot: Slot::Const(Value::Number(680.0)) }])).unwrap();
    assert!((width(&store) - before - 200.0).abs() < 0.5, "{before} → {}", width(&store));
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
}

#[test]
fn a_function_is_sampled_at_the_plot_s_width_and_read_at_the_cursor() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"f="{= x => x * x}" x_min="-2" x_max="2" cursor="1.5" style="smooth""#);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    // 360 px wide, less the margins, at one sample per 2 px.
    assert_eq!(anchors(&store, part(&store, u, "series 1")), 145);
    let port = |name: &str| {
        let doc = store.document();
        let p = doc.children(e).iter().copied().find(|&c| doc.get::<Name>(c).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap();
        store.resolved(p, path("port.signal")).unwrap()
    };
    assert_eq!(port("y_at_cursor"), Value::Number(2.25));
    assert_eq!(port("y_lo"), Value::Number(0.0));
    assert_eq!(port("y_hi"), Value::Number(4.0));
    assert_eq!(text(&store, part(&store, u, "cursor value")), "2.25");
    // Widening resamples.
    store.apply(tx("wider", vec![Op::Set { id: u, path: path("size.w"), slot: Slot::Const(Value::Number(560.0)) }])).unwrap();
    assert_eq!(anchors(&store, part(&store, u, "series 1")), 245);
}
