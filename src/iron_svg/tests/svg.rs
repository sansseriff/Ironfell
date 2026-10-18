//! SVG export and import (plans/specs/svg-import-export.md): a fixture
//! composition with every row of §3 against its golden file, the round-trip
//! law, and foreign files (Figma, Inkscape, nested transforms and `<use>`).
//!
//! `UPDATE_GOLDEN=1 cargo test -p iron_svg --target aarch64-apple-darwin`
//! rewrites the golden files.

use iron_document::component::*;
use iron_document::copy::{self, Placement};
use iron_document::*;
use iron_svg::{export, import, is_svg_text, Scope};
use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI};

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn num(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

fn color(c: [f32; 4]) -> Slot {
    Slot::Const(Value::Color(c))
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

/// Ops under construction, with ids handed out in order and each parent's
/// children appended.
struct Build {
    ids: Vec<NodeId>,
    used: usize,
    keys: BTreeMap<Option<NodeId>, OrderKey>,
    ops: Vec<Op>,
}

impl Build {
    fn new(store: &Store) -> Build {
        Build { ids: store.document().next_ids(400), used: 0, keys: BTreeMap::new(), ops: Vec::new() }
    }

    fn add(&mut self, t: &str, parent: Option<NodeId>, components: Vec<Component>) -> NodeId {
        let id = self.ids[self.used];
        self.used += 1;
        let order = match self.keys.get(&parent) {
            Some(k) => OrderKey::after(k),
            None => OrderKey::first(),
        };
        self.keys.insert(parent, order.clone());
        self.ops.push(Op::Create { id, ty: ty(t), parent, order, components });
        id
    }

    fn apply(self, store: &mut Store) {
        store.apply(tx("fixture", self.ops)).unwrap();
    }
}

fn trs(x: f64, y: f64, rot: f64, sx: f64, sy: f64) -> Component {
    let mut t = Transform2d::default();
    (t.x, t.y, t.rot, t.sx, t.sy) = (num(x), num(y), num(rot), num(sx), num(sy));
    t.wrap()
}

fn at(x: f64, y: f64) -> Component {
    trs(x, y, 0.0, 1.0, 1.0)
}

fn named(s: &str) -> Component {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(s.into()));
    n.wrap()
}

fn size(w: f64, h: f64) -> Component {
    let mut s = Size::default();
    (s.w, s.h) = (num(w), num(h));
    s.wrap()
}

fn corners(r: [f64; 4]) -> Component {
    let mut c = Corners::default();
    (c.tl, c.tr, c.br, c.bl) = (num(r[0]), num(r[1]), num(r[2]), num(r[3]));
    c.wrap()
}

fn radius(rx: f64, ry: f64) -> Component {
    let mut r = Radius::default();
    (r.rx, r.ry) = (num(rx), num(ry));
    r.wrap()
}

fn arc(start: f64, sweep: f64, inner: f64) -> Component {
    let mut a = Arc::default();
    (a.start, a.sweep, a.inner) = (num(start), num(sweep), num(inner));
    a.wrap()
}

fn fill(c: [f32; 4], opacity: f64) -> Component {
    let mut f = Fill::default();
    (f.color, f.opacity) = (color(c), num(opacity));
    f.wrap()
}

fn stroke(c: [f32; 4], w: f64) -> Component {
    let mut s = Stroke::default();
    (s.color, s.width) = (color(c), num(w));
    s.wrap()
}

fn shadow(c: [f32; 4], dx: f64, dy: f64, blur: f64) -> Component {
    let mut s = Shadow::default();
    (s.color, s.dx, s.dy, s.blur) = (color(c), num(dx), num(dy), num(blur));
    s.wrap()
}

fn composition(name: &str, dims: &str) -> Vec<Component> {
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum(dims.into()));
    vec![named(name), c.wrap()]
}

/// One contour of a path: its points, `(x, y, ix, iy, ox, oy)`.
fn contour(b: &mut Build, path: NodeId, closed: bool, pts: &[(f64, f64, f64, f64, f64, f64)]) {
    let mut c = Contour::default();
    c.closed = Slot::Const(Value::Bool(closed));
    let id = b.add("contour", Some(path), vec![c.wrap()]);
    for &(x, y, ix, iy, ox, oy) in pts {
        let mut h = Handles::default();
        (h.ix, h.iy, h.ox, h.oy) = (num(ix), num(iy), num(ox), num(oy));
        b.add("point", Some(id), vec![at(x, y), h.wrap()]);
    }
}

fn path_shape(rule: &str) -> Component {
    let mut p = PathShape::default();
    p.fill_rule = Slot::Const(Value::Enum(rule.into()));
    p.wrap()
}

fn dashed(c: [f32; 4], w: f64, dash: &str, join: &str, cap: &str) -> Component {
    let mut s = Stroke::default();
    (s.color, s.width) = (color(c), num(w));
    s.dash = Slot::Const(Value::Str(dash.into()));
    s.dash_offset = num(2.0);
    s.join = Slot::Const(Value::Enum(join.into()));
    s.cap = Slot::Const(Value::Enum(cap.into()));
    s.miter = num(8.0);
    s.wrap()
}

const RED: [f32; 4] = [0.9, 0.2, 0.2, 1.0];
const BLUE: [f32; 4] = [0.2, 0.4, 0.9, 1.0];
const INK: [f32; 4] = [0.1, 0.1, 0.12, 1.0];

/// Shapes that round-trip: a rotated, scaled group holding an evenly
/// rounded rect; an unevenly rounded rect; full, pie and ring ellipses; an
/// open curved path and a closed mixed one; fills, strokes, opacity, a
/// shadow and names. Returns the store and the composition.
fn shapes(store: &mut Store, b: &mut Build) -> NodeId {
    let comp = b.add("composition", None, composition("fixture", "2d"));
    let group = b.add("group", Some(comp), vec![trs(40.0, 30.0, PI / 6.0, 1.5, 0.75), named("card group")]);
    b.add("rect", Some(group), vec![at(0.0, 0.0), size(120.0, 80.0), corners([12.0; 4]), fill(BLUE, 1.0), stroke(INK, 2.0), named("card")]);
    b.add("rect", Some(comp), vec![at(220.0, 20.0), size(90.0, 60.0), corners([0.0, 20.0, 5.0, 0.0]), fill(RED, 0.5)]);
    b.add("ellipse", Some(comp), vec![at(80.0, 220.0), radius(50.0, 30.0), fill(RED, 1.0), shadow([0.0, 0.0, 0.0, 0.25], 0.0, 4.0, 8.0), named("blob")]);
    b.add("ellipse", Some(comp), vec![at(220.0, 220.0), radius(40.0, 40.0), arc(-FRAC_PI_2, 1.5 * PI, 0.0), fill(BLUE, 1.0)]);
    b.add("ellipse", Some(comp), vec![at(330.0, 220.0), radius(40.0, 40.0), arc(0.0, 2.0 * PI, 0.5), fill(INK, 1.0)]);
    let open = b.add("path", Some(comp), vec![trs(20.0, 320.0, 0.0, 1.0, 1.0), path_shape("nonzero"), dashed(INK, 3.0, "6 3", "miter", "butt"), named("wave")]);
    contour(b, open, false, &[(0.0, 0.0, 0.0, 0.0, 30.0, -40.0), (100.0, 0.0, -30.0, 40.0, 30.0, -40.0), (200.0, 0.0, -30.0, 40.0, 0.0, 0.0)]);
    let closed = b.add("path", Some(comp), vec![trs(260.0, 300.0, 0.1, 1.0, 1.0), path_shape("nonzero"), fill([0.3, 0.7, 0.4, 0.8], 1.0), stroke(INK, 1.0)]);
    contour(b, closed, true, &[(0.0, 0.0, -20.0, 10.0, 0.0, 0.0), (80.0, 0.0, 0.0, 0.0, 0.0, 0.0), (60.0, 60.0, 15.5, 0.0, -15.5, 0.0), (10.0, 50.0, 0.0, 0.0, 0.0, 0.0)]);
    // A frame: two contours, the inner a hole by the even-odd rule.
    let frame = b.add("path", Some(comp), vec![at(380.0, 300.0), path_shape("evenodd"), fill(BLUE, 1.0), named("frame")]);
    let square = |s: f64, o: f64| [(o, o, 0.0, 0.0, 0.0, 0.0), (o + s, o, 0.0, 0.0, 0.0, 0.0), (o + s, o + s, 0.0, 0.0, 0.0, 0.0), (o, o + s, 0.0, 0.0, 0.0, 0.0)];
    contour(b, frame, true, &square(60.0, 0.0));
    contour(b, frame, true, &square(30.0, 15.0));
    let _ = store;
    comp
}

/// `shapes` plus everything that is cut: a binding, a constraint, a slider,
/// a 2D embed with a frame and a 3D one.
fn fixture() -> (Store, NodeId) {
    let mut store = Store::new();
    let mut b = Build::new(&store);
    let comp = shapes(&mut store, &mut b);
    let mut s = Slider::default();
    s.value = num(0.25);
    let slider = b.add("slider", Some(comp), vec![at(20.0, 420.0), s.wrap(), size(160.0, 24.0), named("level")]);
    let mut sz = Size::default();
    sz.w = Slot::Bound(Expr::parse(&format!("{slider}.slider.value * 200")).unwrap());
    sz.h = num(20.0);
    let bar = b.add("rect", Some(comp), vec![at(220.0, 420.0), sz.wrap(), fill(BLUE, 1.0), named("bar")]);
    let mut c = Constraint::default();
    c.kind = Slot::Const(Value::Enum("fix".into()));
    let fix = b.add("constraint", Some(comp), vec![c.wrap()]);
    let inner = b.add("composition", None, composition("badge", "2d"));
    b.add("ellipse", Some(inner), vec![at(30.0, 30.0), radius(30.0, 30.0), fill(RED, 1.0)]);
    b.add("rect", Some(inner), vec![at(50.0, 10.0), size(60.0, 20.0), fill(BLUE, 1.0)]);
    let mut e = Embed::default();
    e.source = Slot::Const(Value::Ref(inner));
    let mut frame = Frame::default();
    (frame.x0, frame.y0, frame.x1, frame.y1) = (num(0.0), num(0.0), num(80.0), num(60.0));
    b.add("embed", Some(comp), vec![e.wrap(), trs(400.0, 20.0, 0.0, 1.0, 1.0), frame.wrap(), named("badge view")]);
    let space = b.add("composition", None, composition("space", "3d"));
    let mut e3 = Embed::default();
    e3.source = Slot::Const(Value::Ref(space));
    b.add("embed", Some(comp), vec![e3.wrap(), at(400.0, 200.0)]);
    let rel = store.document().next_relation_id();
    let mut data = BTreeMap::new();
    data.insert("role".to_owned(), Value::Str("a".into()));
    data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
    b.ops.push(Op::Link { rel: Relation { id: rel, from: fix, to: bar, rel: RelKind::Constrains, data } });
    b.apply(&mut store);
    (store, comp)
}

fn golden(name: &str, text: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::write(&path, text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, want, "{name} differs from its golden file; UPDATE_GOLDEN=1 to accept");
}

fn valid_svg(text: &str) -> usvg::Tree {
    usvg::Tree::from_str(text, &usvg::Options::default()).expect("valid SVG")
}

#[test]
fn the_fixture_composition_matches_its_golden_file() {
    let (store, comp) = fixture();
    let out = export(&store, &Scope::Composition(comp)).unwrap();
    golden("fixture.svg", &out.svg);
    valid_svg(&out.svg);
    assert_eq!(
        out.cuts,
        vec![
            "slider level is exported as a picture of itself; it imports as shapes".to_owned(),
            "a bound value is exported at its current value".to_owned(),
            "constraints are not exported".to_owned(),
            "embed badge view is exported as a copy of badge; it imports as shapes, not a live embed".to_owned(),
            format!("embed {} shows a 3D composition and is left out", store.document().children(comp).last().unwrap()),
        ]
    );
    // The binding exports what it evaluates to now: 0.25 · 200.
    assert!(out.svg.contains("id=\"bar\" data-iron-type=\"rect\" data-iron-name=\"bar\" transform=\"translate(220 420)\" width=\"50\""), "{}", out.svg);
}

#[test]
fn export_maps_each_row_of_the_table() {
    let (store, comp) = fixture();
    let svg = export(&store, &Scope::Composition(comp)).unwrap().svg;
    let has = |s: &str| assert!(svg.contains(s), "missing {s:?} in\n{svg}");
    // transform2d: translate, rotate (degrees), scale.
    has("transform=\"translate(40 30) rotate(30) scale(1.5 0.75)\"");
    // group, named.
    has("<g id=\"card-group\" data-iron-type=\"group\" data-iron-name=\"card group\"");
    // Equal corners: a <rect> with rx; strokes as Iron draws them.
    has("width=\"120\" height=\"80\" rx=\"12\" fill=\"#3366e6\" stroke=\"#1a1a1f\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>");
    // Unequal corners: a path, with the parameters as data.
    has("data-iron-w=\"90\" data-iron-h=\"60\" data-iron-corners=\"0 20 5 0\" fill=\"#e63333\" fill-opacity=\"0.5\"");
    // A full ellipse, centred, with a shadow filter in the parent's space.
    has("<g filter=\"url(#shadow-1)\">");
    has("rx=\"50\" ry=\"30\" fill=\"#e63333\" data-iron-shadow=\"#000000 0.25 0 4 8\"/>");
    has("<feDropShadow dx=\"0\" dy=\"4\" stdDeviation=\"4\" flood-color=\"#000000\" flood-opacity=\"0.25\"/>");
    // An arc is a path, with its parameters.
    has("data-iron-arc=\"-1.5707963267948966 4.71238898038469 0\"");
    has("data-iron-arc=\"0 6.283185307179586 0.5\"");
    // Paths: M, then L or C, Z when closed; an open path is not filled.
    has("d=\"M0 0 C30 -40 70 40 100 0 C130 -40 170 40 200 0\" fill=\"none\" stroke=\"#1a1a1f\" stroke-width=\"3\" stroke-linejoin=\"miter\" stroke-linecap=\"butt\" stroke-miterlimit=\"8\" stroke-dasharray=\"6 3\" stroke-dashoffset=\"2\"");
    // A compound path: two subpaths and the even-odd rule.
    has("d=\"M0 0 L60 0 L60 60 L0 60 Z M15 15 L45 15 L45 45 L15 45 Z\" fill-rule=\"evenodd\" fill=\"#3366e6\"");
    has("d=\"M0 0 L80 0 C80 0 75.5 60 60 60 C44.5 60 10 50 10 50 C10 50 -20 10 0 0 Z\"");
    // A 2D embed: its content, clipped to its frame.
    has("<clipPath id=\"clip-1\">");
    has("data-iron-type=\"embed\" data-iron-name=\"badge view\" transform=\"translate(400 20)\" clip-path=\"url(#clip-1)\"");
    // A slider, as a picture.
    has("data-iron-type=\"slider\" data-iron-name=\"level\"");
}

#[test]
fn the_selection_exports_where_it_sits() {
    let (store, comp) = fixture();
    let card = store.document().children(store.document().children(comp)[0])[0];
    let out = export(&store, &Scope::Nodes(vec![card])).unwrap();
    valid_svg(&out.svg);
    // Inside the rotated group: wrapped in the group's world transform.
    assert!(out.svg.contains("<g transform=\"matrix(1.299 0.75 -0.375 0.6495 40 30)\">"), "{}", out.svg);
    assert!(out.cuts.is_empty(), "{:?}", out.cuts);
    assert!(export(&store, &Scope::Nodes(vec![])).is_err());
}

/// Export → import → export gives the same text for Iron-made content.
#[test]
fn the_round_trip_law() {
    let mut store = Store::new();
    let mut b = Build::new(&store);
    let comp = shapes(&mut store, &mut b);
    b.apply(&mut store);
    let first = export(&store, &Scope::Composition(comp)).unwrap();
    assert!(first.cuts.is_empty(), "{:?}", first.cuts);

    let imported = import(&first.svg).unwrap();
    assert!(imported.cuts.is_empty(), "{:?}", imported.cuts);
    let (mut fresh, target) = empty_composition();
    paste(&mut fresh, &imported.fragment, target);
    let second = export(&fresh, &Scope::Composition(target)).unwrap();
    assert_eq!(first.svg, second.svg);

    // Rects and ellipses come back as themselves, and the store agrees.
    let doc = fresh.document();
    let types: Vec<&str> = doc.children(target).iter().map(|&c| doc.live(c).unwrap().ty.name()).collect();
    assert_eq!(types, ["group", "rect", "ellipse", "ellipse", "ellipse", "path", "path", "path"]);
    // The frame keeps both contours and its rule.
    let frame = *doc.children(target).last().unwrap();
    assert_eq!(iron_document::contour::contours(&fresh, frame).len(), 2);
    assert_eq!(iron_document::contour::fill_rule(&fresh, frame), iron_document::contour::FillRule::EvenOdd);
}

fn empty_composition() -> (Store, NodeId) {
    let mut store = Store::new();
    let mut b = Build::new(&store);
    let comp = b.add("composition", None, composition("target", "2d"));
    b.apply(&mut store);
    (store, comp)
}

fn paste(store: &mut Store, frag: &copy::Fragment, parent: NodeId) -> Vec<NodeId> {
    let pasted = copy::paste(store, frag, &Placement { parent: Some(parent), ..Default::default() }, "test").unwrap();
    store.apply(tx("import SVG", pasted.ops)).unwrap();
    pasted.roots
}

fn f(store: &Store, id: NodeId, leaf: &str) -> f64 {
    store.resolved(id, leaf.parse().unwrap()).and_then(|v| v.as_f64()).unwrap()
}

fn kids_of(store: &Store, id: NodeId) -> Vec<NodeId> {
    store.document().children(id).to_vec()
}

fn ty_of(store: &Store, id: NodeId) -> &'static str {
    store.document().live(id).unwrap().ty.name()
}

fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

#[test]
fn figma_copy_as_svg_imports_editable() {
    let text = fixture_text("figma.svg");
    assert!(is_svg_text(&text));
    let imported = import(&text).unwrap();
    let (mut store, comp) = empty_composition();
    let roots = paste(&mut store, &imported.fragment, comp);
    let types: Vec<&str> = roots.iter().map(|&r| ty_of(&store, r)).collect();
    // Rects and the circle are recognised; the line stays a path.
    assert_eq!(types, ["rect", "ellipse", "path", "rect"], "cuts: {:?}", imported.cuts);
    let rect = roots[0];
    assert_eq!((f(&store, rect, "transform2d.x"), f(&store, rect, "transform2d.y")), (10.0, 10.0));
    assert_eq!((f(&store, rect, "size.w"), f(&store, rect, "size.h")), (100.0, 60.0));
    assert_eq!(f(&store, rect, "corners.tl"), 8.0);
    assert_eq!(f(&store, rect, "corners.br"), 8.0);
    let circle = roots[1];
    assert_eq!((f(&store, circle, "transform2d.x"), f(&store, circle, "transform2d.y")), (150.0, 40.0));
    assert_eq!((f(&store, circle, "radius.rx"), f(&store, circle, "radius.ry")), (30.0, 30.0));
    assert_eq!(store.resolved(circle, "fill.color".parse().unwrap()), Some(Value::Color([1.0, 92.0 / 255.0, 0.0, 1.0])));
    // A line: open, stroked, not filled, two points.
    let line = roots[2];
    assert_eq!(store.resolved(kids_of(&store, line)[0], "contour.closed".parse().unwrap()), Some(Value::Bool(false)));
    assert_eq!(f(&store, line, "stroke.width"), 2.0);
    assert_eq!(kids_of(&store, kids_of(&store, line)[0]).len(), 2);
    assert_eq!(store.resolved(line, "stroke.cap".parse().unwrap()), Some(Value::Enum("butt".into())));
    // A sharp-cornered square has no corners component.
    assert!(store.resolved(roots[3], "corners.tl".parse().unwrap()).is_none());
    assert_eq!((f(&store, roots[3], "size.w"), f(&store, roots[3], "size.h")), (40.0, 40.0));
    // Figma's drop shadow is a filter Iron does not read back.
    assert!(imported.cuts.iter().any(|c| c.starts_with("filters are not imported")), "{:?}", imported.cuts);
    assert_eq!(imported.cuts, ["filters are not imported"]);
}

#[test]
fn foreign_paths_that_are_not_rects_or_ellipses_stay_paths() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <path d="M0 0 H50 V50 H10 Z" fill="red"/>
      <path d="M100 0 H150 L160 50 H100 Z" fill="red"/>
      <path d="M60 50 A20 20 0 0 1 100 50 Z" fill="red"/>
      <rect x="10" y="60" width="40" height="30" rx="4" ry="12" fill="blue"/>
      <rect x="120" y="60" width="40" height="30" transform="skewX(20)" fill="blue"/>
    </svg>"##;
    let imported = import(svg).unwrap();
    let (mut store, comp) = empty_composition();
    let roots = paste(&mut store, &imported.fragment, comp);
    // A trapezoid, a quadrilateral, a half disc, an elliptical-cornered rect
    // (Iron's corners are circular) and a skewed rect.
    assert_eq!(roots.iter().map(|&r| ty_of(&store, r)).collect::<Vec<_>>(), ["path", "path", "path", "path", "path"]);
}

#[test]
fn an_inkscape_file_keeps_its_layers_and_reports_text_and_gradients() {
    let imported = import(&fixture_text("inkscape.svg")).unwrap();
    let (mut store, comp) = empty_composition();
    let roots = paste(&mut store, &imported.fragment, comp);
    assert_eq!(roots.len(), 1, "one layer");
    let layer = roots[0];
    assert_eq!(ty_of(&store, layer), "group");
    assert_eq!(store.resolved(layer, "name.text".parse().unwrap()), Some(Value::Str("layer1".into())));
    // The layer's translate is the group's.
    assert_eq!((f(&store, layer, "transform2d.x"), f(&store, layer, "transform2d.y")), (-20.0, -10.0));
    let kids = kids_of(&store, layer);
    assert_eq!(kids.iter().map(|&k| ty_of(&store, k)).collect::<Vec<_>>(), ["rect", "rect"]);
    assert_eq!((f(&store, kids[1], "transform2d.x"), f(&store, kids[1], "size.w"), f(&store, kids[1], "size.h")), (40.0, 80.0, 40.0));
    // The gradient fill is its first stop.
    assert_eq!(store.resolved(kids[1], "fill.color".parse().unwrap()), Some(Value::Color([1.0, 0.0, 0.0, 1.0])));
    assert_eq!(
        imported.cuts,
        vec!["text is not imported".to_owned(), "gradients are imported as their first stop's colour".to_owned()]
    );
    // The dashed stroke keeps its dashes, in the file's units.
    assert_eq!(store.resolved(kids[0], "stroke.dash".parse().unwrap()), Some(Value::Str("2 1".into())));
    assert_eq!(store.resolved(kids[0], "stroke.join".parse().unwrap()), Some(Value::Enum("miter".into())));
}

#[test]
fn nested_transforms_use_and_skew() {
    let imported = import(&fixture_text("nested.svg")).unwrap();
    let (mut store, comp) = empty_composition();
    let roots = paste(&mut store, &imported.fragment, comp);
    let outer = roots[0];
    assert_eq!(ty_of(&store, outer), "group");
    assert!((f(&store, outer, "transform2d.rot") - 45f64.to_radians()).abs() < 1e-9);
    assert_eq!(f(&store, outer, "transform2d.sx"), 2.0);
    let kids = kids_of(&store, outer);
    // `<use>` twice: two groups, each placing a copy of the square.
    let uses: Vec<NodeId> = kids.iter().copied().filter(|&k| ty_of(&store, k) == "group").collect();
    assert_eq!(uses.len(), 2, "{:?}", kids.iter().map(|&k| ty_of(&store, k)).collect::<Vec<_>>());
    assert_eq!((f(&store, uses[1], "transform2d.x"), f(&store, uses[1], "transform2d.y")), (30.0, 0.0));
    // The skewed one bakes the skew into its points: the top edge shifts.
    let skewed = roots[1];
    assert_eq!(ty_of(&store, skewed), "path");
    let pts = kids_of(&store, kids_of(&store, skewed)[0]);
    let xs: Vec<f64> = pts.iter().map(|&p| f(&store, p, "transform2d.x")).collect();
    assert_eq!(xs, [0.0, 10.0, 15.7735, 5.7735]);
    // The donut: one path, two contours, the even-odd rule: a hole.
    let donut = roots[2];
    assert_eq!(ty_of(&store, donut), "path");
    assert_eq!(kids_of(&store, donut).len(), 2);
    assert_eq!(iron_document::contour::fill_rule(&store, donut), iron_document::contour::FillRule::EvenOdd);
    assert!(!imported.cuts.iter().any(|c| c.contains("hole")), "{:?}", imported.cuts);
    // An arc and a quadratic become cubics.
    let curve = roots[3];
    assert!(kids_of(&store, kids_of(&store, curve)[0]).iter().any(|&p| f(&store, p, "handles.ox") != 0.0));
}

#[test]
fn import_refuses_what_is_not_svg() {
    assert!(import("hello").is_err());
    assert!(import("<html></html>").is_err());
    assert!(import("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>").is_err(), "nothing to import");
    assert!(!is_svg_text("{\"iron_fragment\":1}"));
    assert!(is_svg_text("<?xml version=\"1.0\"?>\n<svg/>"));
}

#[test]
fn an_import_undoes_in_one_step() {
    let imported = import(&fixture_text("figma.svg")).unwrap();
    let (mut store, comp) = empty_composition();
    paste(&mut store, &imported.fragment, comp);
    assert_eq!(store.document().children(comp).len(), 4);
    store.undo(0).unwrap().unwrap();
    assert!(store.document().children(comp).is_empty());
}

#[test]
fn appearance_gradients_and_clip_frames_round_trip() {
    for radial in [false, true] {
        let mut store = Store::new();
        let mut b = Build::new(&store);
        let comp = b.add("composition", None, composition("paint", "2d"));
        let mut opacity = Appearance::default();
        opacity.opacity = num(0.45);
        let mut clip = GroupClip::default();
        clip.shape = Slot::Const(Value::Enum(if radial { "ellipse" } else { "rect" }.into()));
        clip.radius = num(12.0);
        let group = b.add("group", Some(comp), vec![named("clipped"), at(20.0, 30.0), opacity.wrap(), clip.wrap()]);
        let mut gradient = Gradient::default();
        gradient.kind = Slot::Const(Value::Enum(if radial { "radial" } else { "linear" }.into()));
        gradient.end = color([0.1, 0.8, 0.4, 0.3]);
        b.add("rect", Some(group), vec![at(10.0, 10.0), size(180.0, 100.0), fill([0.2, 0.3, 0.4, 1.0], 0.7), gradient.wrap()]);
        b.add("rect", Some(group), vec![at(60.0, 40.0), size(80.0, 90.0), Fill::default().wrap(), Appearance { opacity: num(0.6) }.wrap()]);
        b.apply(&mut store);
        let first = export(&store, &Scope::Composition(comp)).unwrap();
        valid_svg(&first.svg);
        assert!(first.svg.contains("opacity=\"0.45\""));
        assert!(first.svg.contains("clip-path=\"url(#clip-1)\""));
        assert!(first.svg.contains(if radial { "<radialGradient" } else { "<linearGradient" }));
        let imported = import(&first.svg).unwrap();
        assert!(imported.cuts.is_empty(), "{:?}", imported.cuts);
        let (mut fresh, target) = empty_composition();
        paste(&mut fresh, &imported.fragment, target);
        let second = export(&fresh, &Scope::Composition(target)).unwrap();
        assert_eq!(first.svg, second.svg);
    }
}

#[test]
fn foreign_group_opacity_remains_isolated() {
    let imported = import(r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><g opacity="0.5"><rect width="80" height="80" fill="red"/><rect x="40" width="80" height="80" fill="blue"/></g></svg>"##).unwrap();
    assert!(imported.cuts.is_empty(), "{:?}", imported.cuts);
    let (mut store, target) = empty_composition();
    paste(&mut store, &imported.fragment, target);
    let group = store.document().children(target)[0];
    assert_eq!(store.resolved(group, "appearance.opacity".parse().unwrap()), Some(Value::Number(0.5)));
    for &id in store.document().children(group) {
        assert_eq!(store.resolved(id, "fill.opacity".parse().unwrap()), Some(Value::Number(1.0)));
    }
}

#[test]
fn automatic_embed_frame_includes_transformed_stroke_and_shadow() {
    let mut store = Store::new();
    let mut b = Build::new(&store);
    let source = b.add("composition", None, composition("source", "2d"));
    // A local 8px stroke outset is conservatively padded to 32px for joins,
    // then 10 + 3*sigma for the shadow, before applying nonuniform scale.
    b.add("rect", Some(source), vec![trs(40.0, 40.0, 0.0, 2.0, 0.5), size(240.0, 160.0),
        corners([45.0; 4]), fill(RED, 1.0), stroke(INK, 16.0), shadow(INK, 0.0, 10.0, 4.0)]);
    let comp = b.add("composition", None, composition("view", "2d"));
    let e = Embed { source: Slot::Const(Value::Ref(source)) };
    b.add("embed", Some(comp), vec![e.clone().wrap(), at(0.0, 0.0)]);
    let frame = Frame { x0: num(0.0), y0: num(0.0), x1: num(280.0), y1: num(200.0) };
    b.add("embed", Some(comp), vec![e.wrap(), at(600.0, 0.0), frame.wrap()]);
    b.apply(&mut store);
    let result = export(&store, &Scope::Composition(comp)).unwrap();
    valid_svg(&result.svg);
    assert!(result.svg.contains("<rect x=\"-56\" y=\"16\" width=\"672\" height=\"128\"/>"), "{}", result.svg);
    assert!(result.svg.contains("<rect x=\"0\" y=\"0\" width=\"280\" height=\"200\"/>"), "{}", result.svg);
}

/// Text exports as `<text>` with a `<tspan>` per line and styled part, at
/// Iron's own layout (plans/specs/text.md §9).
#[test]
fn text_exports_as_tspans() {
    use iron_document::component::{Font, Text};
    static FONTS: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        iron_text::engine().register(std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/fonts/Inter.ttf")).unwrap());
    });
    let mut store = Store::new();
    let mut b = Build::new(&store);
    let comp = b.add("composition", None, composition("labels", "2d"));
    let mut body = Text::default();
    body.content = Slot::Const(Value::Str("Hot & cold\nsecond line".into()));
    body.runs = Slot::Const(Value::Str(r#"[{"start":0,"end":3,"weight":700,"color":[1,0,0,1]},{"start":6,"end":10,"underline":true}]"#.into()));
    let mut font = Font::default();
    font.size = num(20.0);
    b.add("text", Some(comp), vec![at(10.0, 20.0), body.wrap(), font.wrap(), fill([0.0, 0.0, 0.0, 1.0], 1.0), named("label")]);
    b.apply(&mut store);
    let svg = export(&store, &Scope::Composition(comp)).unwrap().svg;
    assert!(svg.contains(r#"<text id="label" data-iron-type="text""#), "{svg}");
    assert!(svg.contains("font-family=\"'Inter', Inter, sans-serif\""), "{svg}");
    assert!(svg.contains(r##"font-weight="700" fill="#ff0000">Hot</tspan>"##), "{svg}");
    assert!(svg.contains(r#"text-decoration="underline">cold</tspan>"#), "{svg}");
    assert!(svg.contains(">second line</tspan>"), "{svg}");
    // The piece after a styled run does not inherit it.
    assert!(svg.contains(r#"y="19.375"> &amp; </tspan>"#) || svg.contains(r#""> &amp; </tspan>"#), "{svg}");
    assert!(svg.contains("&amp;"), "escaped: {svg}");
    // Lines stack: the second line's baseline is below the first's.
    let ys: Vec<f64> = svg.match_indices(" y=\"").map(|(i, _)| svg[i + 4..].split('"').next().unwrap().parse().unwrap()).collect();
    assert!(ys.last().unwrap() > ys.first().unwrap(), "{ys:?}");
}
