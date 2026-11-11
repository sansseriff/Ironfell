//! The standard components (component-authoring.md §6): each places as a
//! linked instance, its definition copied in once; the slider drives a
//! laser by a line; the Library tab's session composition bakes every
//! preview with no bindings left.

use iron_document::facet::{Composition, Fill, Name};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn desk() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str("bench".into()));
    store
        .apply(TransactionInput { label: "desk".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![
            Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![n.wrap(), c.wrap()] },
            Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
        ], exact: false })
        .unwrap();
    (store, ids[0])
}

#[test]
fn standard_components_place_once_and_a_slider_drives_a_laser() {
    let (mut store, comp) = desk();
    for e in standard::ENTRIES {
        let u = store.place_standard(e.key, comp, [100.0, 100.0], Actor::Human).unwrap_or_else(|err| panic!("{}: {err}", e.key));
        assert!(expand::use_entity(store.document(), u).is_some(), "{}", e.key);
        assert_eq!(store.resolved(u, "transform2d.x".parse().unwrap()), Some(Value::Number(100.0)));
    }
    // One history entry per placement; the definition comes in once.
    let entries = store.history().past().len();
    let second = store.place_standard("laser", comp, [400.0, 100.0], Actor::Human).unwrap();
    assert_eq!(store.history().past().len(), entries + 1);
    let lasers = store.document().nodes_of_type(ty("definition")).filter(|&d| store.document().is_live(d)).filter(|&d| standard::find(store.document(), "laser") == Some(d)).count();
    assert_eq!(lasers, 1);
    // The slider's value (10 × 1 mW) drives the second laser's power by a
    // line; a readout of plain numbers takes it too.
    let doc = store.document();
    let port = |u: NodeId, name: &str| {
        let e = expand::use_entity(doc, u).unwrap();
        doc.children(e).iter().copied().find(|&p| doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    };
    let slider = doc.children(comp).iter().copied().find(|&u| doc.get::<Name>(u).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("slider")).unwrap();
    let (from, to) = (port(slider, "value"), port(second, "power"));
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.document(), &read, from, to).unwrap();
    store.apply(TransactionInput { label: "connect".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
    assert_eq!(store.resolved(to, "port.signal".parse().unwrap()), Some(Value::parse_quantity("10 mW").unwrap()));
}

#[test]
fn the_library_composition_bakes_every_preview() {
    let mut store = Store::new();
    let first = store.document().next_session_ids(1)[0];
    let ops = standard::library_ops(first, OrderKey::first());
    assert!(ops.iter().all(|op| match op {
        Op::Create { facets, .. } => facets.iter().all(|f| f.field_names().into_iter().all(|n| f.get(n).is_none_or(|s| s.expr().is_none()))),
        _ => true,
    }));
    store.apply_session(ops).unwrap();
    let doc = store.document();
    let lib = composition::compositions(doc).into_iter().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(standard::LIBRARY)).unwrap();
    assert!(lib.is_session());
    assert_eq!(doc.children(lib).len(), standard::ENTRIES.len());
    assert!(!store.save().contains("Dial meter"), "never saved");
}

#[test]
fn the_library_recolours_its_cards_for_the_theme() {
    let mut store = Store::new();
    let first = store.document().next_session_ids(1)[0];
    store.apply_session(standard::library_ops(first, OrderKey::first())).unwrap();
    assert!(standard::library_theme_ops(store.document(), true).is_empty(), "built in the light theme's colours");
    let dark = standard::library_theme_ops(store.document(), false);
    // Each card's fill and edge, each label's ink.
    assert_eq!(dark.len(), standard::ENTRIES.len() * 3);
    store.apply_session(dark).unwrap();
    assert!(standard::library_theme_ops(store.document(), false).is_empty());
}

/// WCAG contrast between two sRGB colours.
fn contrast(a: [f32; 4], b: [f32; 4]) -> f32 {
    let lum = |c: [f32; 4]| {
        let ch = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn the_library_previews_read_in_both_themes() {
    let mut store = Store::new();
    let first = store.document().next_session_ids(1)[0];
    store.apply_session(standard::library_ops(first, OrderKey::first())).unwrap();
    for light in [false, true] {
        store.apply_session(standard::library_theme_ops(store.document(), light)).unwrap();
        let doc = store.document();
        let lib = composition::compositions(doc).into_iter().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(standard::LIBRARY)).unwrap();
        let named = |id: NodeId| doc.get::<Name>(id).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
        let colour = |id: NodeId| match doc.get::<Fill>(id).and_then(|f| f.color.constant().cloned()) {
            Some(Value::Color(c)) => Some(c),
            _ => None,
        };
        let mut texts = 0;
        // The previews whose ink sits on the card itself (the Plot draws
        // on its own surface); the Laser's body is a shape, held to 3:1.
        for &cell in doc.children(lib).iter().filter(|&&c| matches!(named(c).as_deref(), Some("readout" | "meter" | "laser"))) {
            let card = doc.children(cell).iter().copied().find(|&c| named(c).as_deref() == Some("card")).unwrap();
            let paper = colour(card).unwrap();
            let Some(preview) = doc.children(cell).iter().copied().find(|&c| named(c).as_deref() == Some("preview")) else { continue };
            let mut stack = vec![preview];
            while let Some(id) = stack.pop() {
                stack.extend(doc.children(id).iter().copied());
                if named(id).as_deref() == Some("body")
                    && let Some(body) = colour(id)
                {
                    assert!(contrast(body, paper) >= 3.0, "the laser's body reads at {:.2}:1", contrast(body, paper));
                }
                if doc.live(id).is_some_and(|n| n.ty.name() == "text")
                    && let Some(ink) = colour(id)
                {
                    texts += 1;
                    // The Readout's figure and the Dial meter's reading among them.
                    assert!(contrast(ink, paper) >= 4.5, "{:?} in {:?} reads at {:.2}:1 ({})", named(id), named(cell), contrast(ink, paper), if light { "light" } else { "dark" });
                }
            }
        }
        assert_eq!(texts, 2, "the Readout's figure and the Dial meter's reading");
    }
}
