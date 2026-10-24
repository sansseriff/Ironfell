//! References in the spatial scene (instance-cache.md phase D): an import
//! with its repeated pieces as references has the same bounds as one with
//! them drawn out, group by group. Ignored for a large file (`BIG_SVG`), run
//! on a small fixture otherwise.

use iron_document::facet::{Composition, Name};
use iron_document::*;
use iron_spatial::Scene;

fn load(text: &str, share: bool) -> (Store, NodeId) {
    let imported = iron_svg::import_with(text, share).expect("imports");
    let mut store = Store::new();
    let ids = store.document().next_ids(1);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("target".into()));
    store
        .apply(TransactionInput { label: "c".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![Op::Create { id: ids[0], ty: TypeId::lookup("composition").unwrap(), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }], exact: false })
        .unwrap();
    let place = copy::Placement { parent: Some(ids[0]), ..Default::default() };
    let pasted = copy::paste(&store, &imported.fragment, &place, "import").expect("pastes");
    store.apply(TransactionInput { label: "import SVG".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false }).expect("applies");
    (store, ids[0])
}

fn compare(text: &str) {
    let (mut fs, fc) = load(text, false);
    let (mut ss, sc) = load(text, true);
    let (mut fscene, mut sscene) = (Scene::default(), Scene::default());
    fscene.update(&mut fs);
    sscene.update(&mut ss);
    let round = |r: kurbo::Rect| [r.x0, r.y0, r.x1, r.y1].map(|v| (v * 100.0).round() / 100.0);
    let (fb, sb) = (fscene.content_bounds(fc).map(round), sscene.content_bounds(sc).map(round));
    println!("content flat {fb:?} shared {sb:?}");
    // Each top-level node's content, geometry and paint.
    for (&f, &s) in fs.document().children(fc).iter().zip(ss.document().children(sc)) {
        let (a, b) = (fscene.bounds(f), sscene.bounds(s));
        let close = |x: Option<iron_spatial::Bounds>, y: Option<iron_spatial::Bounds>| match (x, y) {
            (Some(x), Some(y)) => round(x.geometry).iter().zip(round(y.geometry)).all(|(p, q)| (p - q).abs() < 0.05),
            (None, None) => true,
            _ => false,
        };
        assert!(close(a, b), "top-level node bounds: flat {:?} shared {:?}", a.map(|x| x.geometry), b.map(|x| x.geometry));
    }
    assert_eq!(fb, sb);
}

#[test]
fn a_fixture_with_nested_repeats() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../iron_svg/tests/fixtures/repeated.svg")).unwrap();
    compare(&text);
}

#[test]
#[ignore]
fn a_large_file() {
    let text = std::fs::read_to_string(std::env::var("BIG_SVG").expect("BIG_SVG")).unwrap();
    compare(&text);
}

