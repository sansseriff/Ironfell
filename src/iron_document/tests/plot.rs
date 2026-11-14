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

/// The texts of the copies of the plot's tick label `name` (a repeat's
/// copies, `#use/labels/~k/label`), in order.
fn labels(store: &Store, u: NodeId, name: &str) -> Vec<String> {
    let doc = store.document();
    let mut copies: Vec<(String, NodeId)> = doc
        .descendants(u)
        .into_iter()
        .filter(|&n| doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name))
        .filter_map(|n| n.to_string().split("/~").nth(1).map(|k| (k.to_owned(), n)))
        .collect();
    copies.sort_by_key(|(k, _)| k.split('/').next().and_then(|k| k.parse::<usize>().ok()));
    copies.into_iter().map(|(_, n)| text(store, n)).collect()
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
    // One label per tick, from the repeat: no empty slots.
    let xs = labels(&store, u, "x tick");
    assert_eq!(xs.first().map(String::as_str), Some("0"));
    assert!(xs.len() >= 2 && xs.iter().all(|t| !t.is_empty()), "{xs:?}");
    assert_eq!(store.resolved(e, path("args.style")), Some(Value::Enum("line".into())));
}

#[test]
fn units_title_the_axes_and_a_resize_reflows() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"w="480" h="300" title="Intensity" xs="{= range(400 nm, 700 nm, 61)}" ys="{= pow(sin(pi * ($self.xs - 400 nm) / 100 nm), 2) * 1 mW}" style="area""#);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    assert_eq!(text(&store, part(&store, u, "x title")), "nm");
    assert_eq!(text(&store, part(&store, u, "y title")), "mW");
    assert_eq!(labels(&store, u, "x tick").first().map(String::as_str), Some("400"));
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

/// The plot's vis machine (plans/specs/machines.md): disappear takes it
/// away; appear draws it on in order, axes before the curve.
#[test]
fn the_plot_appears_and_disappears_in_order() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, "");
    let at = |s: &Store, name: &str, leaf: &str| s.resolved(part(s, u, name), path(leaf)).unwrap().as_f64().unwrap();
    store.advance_machines(0.0);
    assert_eq!(at(&store, "axis lines", "trim.end"), 1.0);
    // As an edit writes it: text, for an enum input.
    let vis = |s: &mut Store, v: &str| s.apply(tx("vis", vec![Op::Set { id: e, path: path("args.vis"), slot: Slot::Const(Value::Str(v.into())) }])).unwrap();
    vis(&mut store, "hidden");
    store.advance_machines(1.0);
    store.advance_machines(1.5);
    assert_eq!(at(&store, "axis lines", "trim.end"), 0.0);
    assert_eq!(at(&store, "series 1", "trim.end"), 0.0);
    assert_eq!(at(&store, "frame", "appearance.opacity"), 0.0);
    // Appear, 0.4 s in: the frame is in, the axes half drawn (Easy Ease is
    // symmetric), the curve not started.
    vis(&mut store, "shown");
    store.advance_machines(2.0);
    store.advance_machines(2.4);
    assert_eq!(at(&store, "frame", "appearance.opacity"), 1.0);
    assert!((at(&store, "axis lines", "trim.end") - 0.5).abs() < 1e-6, "{}", at(&store, "axis lines", "trim.end"));
    assert_eq!(at(&store, "series 1", "trim.end"), 0.0);
    store.advance_machines(3.5);
    assert_eq!(at(&store, "series 1", "trim.end"), 1.0);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
}

/// A placed plot's unset inputs (NaN until `auto`, types-and-units.md)
/// survive a save and load.
#[test]
fn a_document_with_a_plot_saves_and_loads() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, "");
    let text = store.save();
    let mut again = Store::load(&text).unwrap_or_else(|err| panic!("{err:?}"));
    assert_eq!(again.save(), text);
    assert!(errors(&again, e, u).is_empty(), "{:#?}", errors(&again, e, u));
    assert!(again.resolved(e, path("args.cursor")).unwrap().as_f64().unwrap().is_nan());
    again.advance_machines(0.0);
}

/// The model's view leaves out leaves at their default, and keeps bound
/// ones.
#[test]
fn the_model_view_omits_defaults() {
    let (mut store, bench) = desk();
    let (_, u) = place(&mut store, r#"title="Intensity""#);
    let q = |omit: bool| ViewQuery { omit_defaults: omit, relation: Default::default(), scope: Some(bench), fidelity: Fidelity::Full, depth: None, expand: vec![u] };
    let (full, brief) = (store.view(&q(false)).unwrap(), store.view(&q(true)).unwrap());
    assert!(full.contains(r#" rot="0""#) && !brief.contains(r#" rot="0""#), "{brief}");
    assert!(brief.contains("Intensity") && brief.contains(".bind="));
    assert!(brief.len() * 10 < full.len() * 9, "{} vs {}", brief.len(), full.len());
}

/// Council findings: every tick gets a label on a wide plot, bars stand on
/// zero, and empty data draws an empty frame without errors.
#[test]
fn wide_plots_label_every_tick_bars_stand_on_zero_and_empty_data_is_fine() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"w="900""#);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    let marks = anchors(&store, part(&store, u, "tick marks"));
    let (labelled, y_labelled) = (labels(&store, u, "x tick"), labels(&store, u, "y tick"));
    assert!(labelled.iter().chain(&y_labelled).all(|t| !t.is_empty()));
    assert_eq!(marks, 2 * (labelled.len() + y_labelled.len()), "a label for every tick mark");

    let (e, u) = place(&mut store, r#"xs="{= series(1, 2, 3)}" ys="{= series(-1, 2, -3)}" style="bars""#);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    let g = contour::geometry(&store, part(&store, u, "series 1"));
    assert_eq!(g.contours().len(), 3, "three bars");
    let zero_line = |s: &Store| {
        // Every bar shares one edge: the y = 0 line, inside the area.
        let ys: Vec<f64> = contour::geometry(s, part(s, u, "series 1")).contours().iter().flat_map(|c| c.anchors.iter().map(|a| a.at.y)).collect();
        ys
    };
    let ys = zero_line(&store);
    let common = ys.iter().filter(|y| ys.iter().filter(|z| (*z - *y).abs() < 1e-6).count() >= 6).count();
    assert!(common >= 6, "bars share a baseline: {ys:?}");
    assert!(g.contours().iter().all(|c| { let h: Vec<f64> = c.anchors.iter().map(|a| a.at.y).collect(); h.iter().cloned().fold(f64::MIN, f64::max) - h.iter().cloned().fold(f64::MAX, f64::min) > 1.0 }), "no zero-height bar");

    let (e, u) = place(&mut store, r#"xs="{= series()}" ys="{= series()}""#);
    let errs = errors(&store, e, u);
    assert!(errs.is_empty(), "{errs:#?}");
}

/// The plot's colours are theme tokens (palette.rs): placing one brings the
/// theme, and its frame and series follow light and dark; a series may have
/// its own style.
#[test]
fn the_plot_follows_the_theme_and_styles_each_series() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"ys2="{= range(0, 1, 41)}" style2="scatter""#);
    assert!(errors(&store, e, u).is_empty(), "{:#?}", errors(&store, e, u));
    assert!(palette::tokens(store.document()).contains_key("series-1"));
    let colour = |s: &Store, part_name: &str, leaf: &str| s.resolved(part(s, u, part_name), path(leaf));
    let light = (colour(&store, "frame", "fill.color"), colour(&store, "series 1", "stroke.color"));
    store.set_theme(true);
    let dark = (colour(&store, "frame", "fill.color"), colour(&store, "series 1", "stroke.color"));
    assert_ne!(light.0, dark.0, "the frame's surface");
    assert_ne!(light.1, dark.1, "the series' colour");
    assert!(errors(&store, e, u).is_empty());
    // Series 2 is scatter: dots, no line.
    assert_eq!(colour(&store, "series 2", "stroke.color"), Some(Value::Color([0.0, 0.0, 0.0, 0.0])));
    assert_ne!(colour(&store, "series 1", "stroke.color"), Some(Value::Color([0.0, 0.0, 0.0, 0.0])));
}

/// Marks in the data's coordinates (plot.md §4.2): points with labels and
/// dashed reference lines, drawn in the clipped area and counted in the
/// auto range; a series may be dashed.
#[test]
fn marks_label_points_and_reference_lines_in_data_coordinates() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"xs="0 1 2 3 4" ys="0 1 4 9 16" style="dashed" marks_x="{= series(2, 3)}" marks_y="{= series(4, 9)}" marks_label="{= list('peak', 'next')}" hlines="20" vlines="{= series(1)}""#);
    assert!(errors(&store, e, u).is_empty(), "{:?}", errors(&store, e, u));
    // Path data resolves to geometry: count its contours.
    let contours = |name: &str| format!("{:?}", store.resolved(part(&store, u, name), path("path.d")));
    assert!(contours("marks").contains("(2 contours"), "two marks: {}", contours("marks"));
    assert!(contours("reference lines").contains("(2 contours"), "one h, one v: {}", contours("reference lines"));
    let dash = store.resolved(part(&store, u, "series 1"), path("stroke.dash")).unwrap();
    assert_eq!(dash.as_str(), Some("7 5"));
    // The reference line at 20 is in view: the y range reaches it.
    let y_hi = store.resolved(store.document().children(e).iter().copied().find(|&p| store.document().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("y_hi")).unwrap(), path("port.signal")).unwrap();
    assert!(y_hi.as_f64().unwrap() >= 20.0, "{y_hi:?}");
    assert_eq!(labels(&store, u, "mark label"), vec!["peak", "next"]);
}
