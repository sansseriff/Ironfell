//! The demo's gallery (plans/specs/demo.md): each page is an edit
//! (plans/specs/markup.md), seeded by `document_bridge::demo`. Every page
//! must plan and apply in one transaction on a fresh store, and a node that
//! reads a placed plot's ports (`$plot.x_lo`) must see them.

use iron_document::facet::Name;
use iron_document::*;

const PAGES: [(&str, &str); 10] = [
    ("oscillator", include_str!("../../document_bridge/gallery/oscillator.xml")),
    ("refraction", include_str!("../../document_bridge/gallery/refraction.xml")),
    ("interference", include_str!("../../document_bridge/gallery/interference.xml")),
    ("wave packet", include_str!("../../document_bridge/gallery/wavepacket.xml")),
    ("fourier", include_str!("../../document_bridge/gallery/fourier.xml")),
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

fn find(store: &Store, name: &str) -> NodeId {
    let doc = store.document();
    doc.roots().iter().flat_map(|&r| doc.descendants(r)).collect::<Vec<_>>().into_iter().find(|&n| !n.is_expanded() && doc.is_live(n) && doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap_or_else(|| panic!("no node {name}"))
}

/// A store with a model root and a timeline for the pages' clips.
fn desk() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    store
        .apply(tx(
            "desk",
            vec![
                Op::Create { id: ids[0], ty: ty("model"), parent: None, order: OrderKey::first(), facets: vec![named("model")] },
                Op::Create { id: ids[1], ty: ty("timeline"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![named("timeline")] },
            ],
        ))
        .unwrap();
    (store, ids[1])
}

/// Plan page `src` and apply it.
fn page(store: &mut Store, name: &str, src: &str, timeline: NodeId) {
    let src = src.replace("#TIMELINE", &timeline.to_string());
    let (t, _) = edit::transaction(store, &src, name, Actor::System).unwrap_or_else(|e| panic!("{name}: {e}"));
    store.apply(t).unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

#[test]
fn every_page_builds_in_one_transaction() {
    let (mut store, timeline) = desk();
    for (name, src) in PAGES {
        page(&mut store, name, src, timeline);
        let doc = store.document();
        let comps = composition::compositions(doc);
        assert!(comps.iter().any(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)), "{name}: no composition");
    }
}

/// The oscillator's phase marker reads the phase-portrait plot's axis
/// ports and its place: it sits inside the plot's box.
#[test]
fn a_node_reads_a_placed_plots_ports() {
    let (mut store, timeline) = desk();
    page(&mut store, "oscillator", PAGES[0].1, timeline);
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

/// Past the critical angle the refraction plot's curve is NaN (the line
/// ends there), and so is its value at the cursor. The cursor's dot and
/// readout must then hide, not sit at a NaN position: a NaN transform
/// panicked the spatial index.
#[test]
fn a_plot_cursor_past_the_end_of_its_curve_stays_finite() {
    let (mut store, timeline) = desk();
    page(&mut store, "refraction", PAGES[1].1, timeline);
    let set = |id: NodeId, v: f64| Op::Set { id, path: "slider.value".parse().unwrap(), slot: Slot::Const(Value::Number(v)) };
    let (n1, n2, theta1) = (find(&store, "n1"), find(&store, "n2"), find(&store, "theta1"));
    // n₁ = 2 over n₂ = 1: the critical angle is 30°; θ₁ = 60° is past it.
    store.apply(tx("tir", vec![set(n1, 2.0), set(n2, 1.0), set(theta1, 60.0)])).unwrap();
    let doc = store.document();
    let mut checked = 0;
    for id in doc.expanded_ids().collect::<Vec<_>>() {
        for leaf in ["transform2d.x", "transform2d.y"] {
            if let Some(v) = store.resolved(id, leaf.parse().unwrap()).and_then(|v| v.as_f64()) {
                assert!(v.is_finite(), "{id} ({:?}): {leaf} = {v}", doc.get::<Name>(id).and_then(|n| n.text.constant()));
                checked += 1;
            }
        }
        if let Some(d) = store.resolved(id, "path.d".parse().unwrap()) {
            let d = format!("{d:?}");
            assert!(!d.contains("NaN"), "{id}: path {d}");
        }
    }
    assert!(checked > 0);
}

/// The wave packet's clip and the RC page's circuit are in their markup:
/// relations (`<link>`) and the circuit verb (`<circuit>`) are written in
/// an edit like nodes.
#[test]
fn a_page_writes_its_relations_and_its_circuit() {
    let (mut store, timeline) = desk();
    page(&mut store, "wave packet", PAGES[3].1, timeline);
    page(&mut store, "RC charging", PAGES[9].1, timeline);
    let doc = store.document();
    let clip = find(&store, "play t");
    let t = find(&store, "t");
    assert!(doc.relations_from(clip).any(|r| r.rel == document::RelKind::Animates && r.to == t), "the clip animates t ({t}): {:?}", doc.relations_from(clip).collect::<Vec<_>>());
    assert_eq!(doc.live(clip).and_then(|n| n.parent), Some(timeline));
    // The circuit: five parts wired, R bound to the page's slider.
    let r = doc.nodes_of_type(TypeId::lookup("entity").unwrap()).find(|&e| doc.is_live(e) && doc.get::<Name>(e).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("R")).expect("R");
    let r_slider = find(&store, "R slider");
    let arg = doc.get::<facet::Args>(r).and_then(|a| a.get("R").cloned()).expect("R's argument");
    assert!(arg.expr().is_some_and(|e| e.to_string().contains(&r_slider.to_string())), "R reads the slider: {arg:?}");
    let wires = doc.nodes_of_type(TypeId::lookup("connection").unwrap()).filter(|&c| doc.is_live(c)).count();
    assert_eq!(wires, 5);
    // Its probe reads the steady state.
    let embed = find(&store, "circuit view");
    let rc = doc.get::<facet::Embed>(embed).and_then(|e| e.source.constant().cloned());
    assert!(matches!(rc, Some(Value::Ref(_))), "the page embeds the circuit");
}
