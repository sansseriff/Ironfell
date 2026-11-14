//! The demo's gallery (plans/specs/demo.md): each page is node markup with
//! `<place>` (`markup::compose`), seeded by `document_bridge::demo`. Every
//! page must build and apply in one transaction on a fresh store, and a
//! node that reads a placed plot's ports (`$plot.x_lo`) must see them.

use iron_document::facet::{Composition, Name};
use iron_document::*;

const PAGES: [(&str, &str); 9] = [
    ("oscillator", include_str!("../../document_bridge/gallery/oscillator.xml")),
    ("refraction", include_str!("../../document_bridge/gallery/refraction.xml")),
    ("interference", include_str!("../../document_bridge/gallery/interference.xml")),
    ("wave packet", include_str!("../../document_bridge/gallery/wavepacket.xml")),
    ("kepler", include_str!("../../document_bridge/gallery/kepler.xml")),
    ("lorenz", include_str!("../../document_bridge/gallery/lorenz.xml")),
    ("blackbody", include_str!("../../document_bridge/gallery/blackbody.xml")),
    ("enzyme", include_str!("../../document_bridge/gallery/enzyme.xml")),
    ("RC charging", include_str!("../../document_bridge/gallery/rc.xml")),
];

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false }
}

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn last_root(store: &Store) -> OrderKey {
    let doc = store.document();
    let last = doc.roots().iter().filter_map(|&r| doc.live(r)).map(|n| n.order.clone()).max().unwrap();
    OrderKey::after(&last)
}

fn find(store: &Store, name: &str) -> NodeId {
    let doc = store.document();
    doc.roots().iter().flat_map(|&r| doc.descendants(r)).collect::<Vec<_>>().into_iter().find(|&n| doc.is_live(n) && doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap_or_else(|| panic!("no node {name}"))
}

/// A store with a model root and the circuit the RC page embeds.
fn desk() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store
        .apply(tx(
            "desk",
            vec![
                Op::Create { id: ids[0], ty: ty("model"), parent: None, order: OrderKey::first(), facets: vec![named("model")] },
                Op::Create { id: ids[1], ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![named("circuit"), c.wrap()] },
            ],
        ))
        .unwrap();
    (store, ids[1])
}

#[test]
fn every_page_builds_in_one_transaction() {
    let (mut store, circuit) = desk();
    for (name, src) in PAGES {
        let src = src.replace("#CIRCUIT", &circuit.to_string());
        let order = last_root(&store);
        let built = markup::compose(&store, &src, None, order).unwrap_or_else(|e| panic!("{name}: {e}"));
        store.apply(tx(name, built.ops)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let doc = store.document();
        let comps = composition::compositions(doc);
        assert!(comps.iter().any(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)), "{name}: no composition");
    }
}

/// The oscillator's phase marker reads the phase-portrait plot's axis
/// ports and its place: it sits inside the plot's box.
#[test]
fn a_node_reads_a_placed_plots_ports() {
    let (mut store, _) = desk();
    let order = last_root(&store);
    let built = markup::compose(&store, PAGES[0].1, None, order).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("oscillator", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let marker = find(&store, "phase marker");
    let x = store.resolved(marker, "transform2d.x".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    let y = store.resolved(marker, "transform2d.y".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    // The phase portrait is placed at (540, 340), 460 × 320.
    assert!((540.0..1000.0).contains(&x) && (340.0..660.0).contains(&y), "marker at ({x}, {y})");
    let opacity = store.resolved(marker, "appearance.opacity".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    assert_eq!(opacity, 1.0);
    // Written after the plots, it paints over them: siblings keep markup order.
    let doc = store.document();
    let kids: Vec<NodeId> = doc.children(doc.live(marker).unwrap().parent.unwrap()).to_vec();
    let at = |id: NodeId| kids.iter().position(|&k| k == id).unwrap();
    let last_use = kids.iter().copied().filter(|&k| doc.live(k).is_some_and(|n| n.ty.name() == "use")).map(at).max().unwrap();
    assert!(at(marker) > last_use, "the marker sits above the plots");
}
