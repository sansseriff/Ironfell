//! SVG in: a [`Fragment`] of new nodes, placed by the paste path.
//!
//! `usvg` reads the file: it resolves styles, `<use>`, units and arcs, and
//! hands over groups and paths. An element Iron wrote (`data-iron-type`) is
//! then read again from the raw XML by its `id`, in f64 (usvg's tree is
//! f32), so its numbers come back exactly and a rect or an ellipse comes
//! back as one rather than as a path.
//!
//! Coordinates are the SVG's user space (its `viewBox`), which for Iron's
//! own files is the composition's space.
//!
//! An anonymous group with one child (usvg makes one for every transformed
//! shape, and for opacity, filters and clips) is folded into that child; a
//! group with an `id`, or with several children, becomes a `group`.

use crate::{round4, Cut, Cuts};
use iron_document::facet::{Appearance, Arc, Contour, Corners, Fill, Handles, Name, PathShape, Radius, Shadow, Size, Stroke, Transform2d};
use iron_document::copy::Fragment;
use iron_document::shape::{self, Anchor};
use iron_document::{Actor, Affine2, Facet, LeafStruct, NodeId, Op, OrderKey, Slot, Store, TransactionInput, TypeId, Value};
use kurbo::{Affine, Point, Vec2};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use usvg::roxmltree;

/// What an import makes, and what it could not carry.
#[derive(Clone, Debug)]
pub struct Imported {
    /// The new nodes; paste it with `iron_document::copy::paste`.
    pub fragment: Fragment,
    pub cuts: Vec<Cut>,
    /// How many nodes it creates, points included.
    pub nodes: usize,
}

/// Whether pasted text is SVG rather than an Iron fragment or prose.
pub fn is_svg_text(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg") || ((t.starts_with("<?xml") || t.starts_with("<!DOCTYPE") || t.starts_with("<!--")) && t.contains("<svg"))
}

/// A node to create, before it has an id.
struct INode {
    ty: &'static str,
    comps: Vec<Facet>,
    kids: Vec<INode>,
}

impl INode {
    fn count(&self) -> usize {
        1 + self.kids.iter().map(INode::count).sum::<usize>()
    }
}

/// Parse `text` and turn what it draws into new nodes.
pub fn import(text: &str) -> Result<Imported, String> {
    let opt = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let xml = roxmltree::Document::parse_with_options(text, opt).map_err(|e| format!("not valid XML: {e}"))?;
    let svg = xml.root_element();
    if svg.tag_name().name() != "svg" {
        return Err(format!("not an SVG document (its root is <{}>)", svg.tag_name().name()));
    }
    let mut cuts = Cuts::default();
    let mut iron: HashMap<&str, roxmltree::Node> = HashMap::new();
    for n in xml.descendants().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            // usvg drops text without its font stack, so say so here.
            "text" => cuts.add("text is not imported"),
            "foreignObject" => cuts.add("foreignObject content is not imported"),
            _ => {}
        }
        if n.attribute("data-iron-type").is_some()
            && let Some(id) = n.attribute("id")
        {
            iron.insert(id, n);
        }
    }
    let tree = usvg::Tree::from_xmltree(&xml, &usvg::Options::default()).map_err(|e| format!("could not read the SVG: {e}"))?;

    // usvg maps the viewBox onto the canvas with one anonymous group at the
    // root; import in the viewBox's own units instead.
    let mut root: &usvg::Group = tree.root();
    if let Some(vb) = svg.attribute("viewBox").and_then(|v| svgtypes::ViewBox::from_str(v).ok()) {
        let size = tree.size();
        let identity = vb.x == 0.0 && vb.y == 0.0 && (vb.w - size.width() as f64).abs() < 1e-3 && (vb.h - size.height() as f64).abs() < 1e-3;
        if !identity
            && let [usvg::Node::Group(g)] = root.children()
            && g.id().is_empty()
        {
            root = g;
        }
    }

    let mut ctx = Ctx { iron, cuts };
    let mut nodes = Vec::new();
    ctx.group_children(root, Affine::IDENTITY, 1.0, &mut nodes);
    let cuts = ctx.cuts.into_lines();
    if nodes.is_empty() {
        return Err(if cuts.is_empty() { "the SVG draws nothing Iron can import".into() } else { format!("the SVG draws nothing Iron can import: {}", cuts.join("; ")) });
    }
    let count = nodes.iter().map(INode::count).sum();
    let fragment = fragment(nodes, count)?;
    Ok(Imported { fragment, cuts, nodes: count })
}

/// The nodes as a fragment: created in a scratch store, which checks them
/// as any transaction is checked, with no home of their own.
fn fragment(nodes: Vec<INode>, count: usize) -> Result<Fragment, String> {
    let mut store = Store::new();
    let mut ids = store.document().next_ids(count).into_iter();
    let mut ops = Vec::new();
    fn create(n: INode, parent: Option<NodeId>, order: OrderKey, ids: &mut impl Iterator<Item = NodeId>, ops: &mut Vec<Op>) -> NodeId {
        let id = ids.next().expect("counted");
        let ty = TypeId::lookup(n.ty).expect("registered type");
        ops.push(Op::Create { id, ty, parent, order, facets: n.comps });
        let mut k = OrderKey::first();
        for kid in n.kids {
            create(kid, Some(id), k.clone(), ids, ops);
            k = OrderKey::after(&k);
        }
        id
    }
    let mut roots = Vec::new();
    let mut homes = BTreeMap::new();
    let mut parent_world = BTreeMap::new();
    let mut k = OrderKey::first();
    for n in nodes {
        let id = create(n, None, k.clone(), &mut ids, &mut ops);
        roots.push(id);
        homes.insert(id, (None, k.clone()));
        parent_world.insert(id, Affine2::IDENTITY);
        k = OrderKey::after(&k);
    }
    store
        .apply(TransactionInput { label: "import SVG".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false })
        .map_err(|e| format!("the import did not apply: {e:?}"))?;
    Ok(Fragment { doc: store.document().clone(), roots, extras: Vec::new(), homes, parent_world, externals: BTreeMap::new(), origin: "svg".into(), dropped: Vec::new() })
}

fn affine(t: usvg::Transform) -> Affine {
    Affine::new([t.sx as f64, t.ky as f64, t.kx as f64, t.sy as f64, t.tx as f64, t.ty as f64])
}

/// The largest coefficient, for a tolerance relative to it.
fn scale_of(m: Affine) -> f64 {
    m.as_coeffs().iter().fold(1.0f64, |a, v| a.max(v.abs()))
}

fn same(a: Affine, b: Affine) -> bool {
    let d = a.as_coeffs().iter().zip(b.as_coeffs()).fold(0.0f64, |m, (x, y)| m.max((x - y).abs()));
    d <= 1e-5 * scale_of(a)
}

/// `[x, y, rot, sx, sy]` (rot in radians) if `m` is translate · rotate ·
/// scale; `None` if it skews.
fn decompose(m: Affine) -> Option<[f64; 5]> {
    let [a, b, c, d, e, f] = m.as_coeffs();
    let sx = a.hypot(b);
    if sx < 1e-12 {
        return None;
    }
    let rot = b.atan2(a);
    let sy = (a * d - b * c) / sx;
    let back = Affine::new(Affine2::from_trs(e, f, rot, sx, sy).0);
    if !same(m, back) {
        return None;
    }
    // f32 noise off: a hundredth of a thousandth of a pixel or a degree.
    Some([round4(e), round4(f), round4(rot.to_degrees()).to_radians(), round4(sx), round4(sy)])
}

fn trs_affine([x, y, rot, sx, sy]: [f64; 5]) -> Affine {
    Affine::new(Affine2::from_trs(x, y, rot, sx, sy).0)
}

/// An element's own `transform` attribute, exactly: Iron writes
/// `translate() rotate() scale()`, read as those five numbers; anything
/// else is multiplied out (f64) and decomposed.
fn raw_trs(el: roxmltree::Node) -> Option<[f64; 5]> {
    let Some(text) = el.attribute("transform") else { return Some([0.0, 0.0, 0.0, 1.0, 1.0]) };
    let mut out = [0.0, 0.0, 0.0, 1.0, 1.0];
    let mut stage = 0;
    for token in svgtypes::TransformListParser::from(text) {
        match token.ok()? {
            svgtypes::TransformListToken::Translate { tx, ty } if stage < 1 => {
                (out[0], out[1], stage) = (tx, ty, 1);
            }
            svgtypes::TransformListToken::Rotate { angle } if stage < 2 => {
                (out[2], stage) = (angle.to_radians(), 2);
            }
            svgtypes::TransformListToken::Scale { sx, sy } if stage < 3 => {
                (out[3], out[4], stage) = (sx, sy, 3);
            }
            _ => {
                let t = svgtypes::Transform::from_str(text).ok()?;
                return decompose(Affine::new([t.a, t.b, t.c, t.d, t.e, t.f]));
            }
        }
    }
    Some(out)
}

fn attr_f64(el: roxmltree::Node, name: &str) -> Option<f64> {
    el.attribute(name).and_then(|v| v.trim().parse().ok())
}

fn attr_nums(el: roxmltree::Node, name: &str) -> Option<Vec<f64>> {
    el.attribute(name).map(|v| v.split_whitespace().filter_map(|n| n.parse().ok()).collect())
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

fn transform2d([x, y, rot, sx, sy]: [f64; 5]) -> Facet {
    let mut t = Transform2d::default();
    (t.x, t.y, t.rot, t.sx, t.sy) = (n(x), n(y), n(rot), n(sx), n(sy));
    t.wrap()
}

fn name(text: &str) -> Facet {
    let mut c = Name::default();
    c.text = Slot::Const(Value::Str(text.to_owned()));
    c.wrap()
}

fn rgb(c: usvg::Color, alpha: f64) -> Value {
    Value::Color([c.red as f32 / 255.0, c.green as f32 / 255.0, c.blue as f32 / 255.0, alpha as f32])
}

/// Anchors from path segments, one run per subpath.
struct Contours {
    /// Iron's own path data: a closing line to a point on the start is a
    /// real point, not a duplicate to merge.
    iron: bool,
    done: Vec<(Vec<Anchor>, bool)>,
    cur: Vec<Anchor>,
    last_curved: bool,
}

impl Contours {
    fn new(iron: bool) -> Contours {
        Contours { iron, done: Vec::new(), cur: Vec::new(), last_curved: false }
    }

    fn flush(&mut self, closed: bool) {
        let mut run = std::mem::take(&mut self.cur);
        if closed && run.len() >= 2 {
            let (first, last) = (run[0].at, run[run.len() - 1].at);
            let tol = if self.iron { 0.0 } else { 1e-6 };
            if (first - last).hypot() <= tol && (self.last_curved || !self.iron) {
                let end = run.pop().expect("two or more");
                run[0].inn = end.inn;
            }
        }
        if run.len() >= 2 {
            self.done.push((run, closed));
        }
        self.last_curved = false;
    }

    fn move_to(&mut self, p: Point) {
        self.flush(false);
        self.cur.push(Anchor { at: p, inn: Vec2::ZERO, out: Vec2::ZERO });
    }

    fn line_to(&mut self, p: Point) {
        if self.cur.is_empty() {
            self.move_to(p);
            return;
        }
        self.cur.push(Anchor { at: p, inn: Vec2::ZERO, out: Vec2::ZERO });
        self.last_curved = false;
    }

    fn cubic_to(&mut self, c1: Point, c2: Point, p: Point) {
        let Some(last) = self.cur.last_mut() else {
            self.move_to(p);
            return;
        };
        last.out = c1 - last.at;
        self.cur.push(Anchor { at: p, inn: c2 - p, out: Vec2::ZERO });
        self.last_curved = true;
    }

    /// Raised to a cubic: the same curve.
    fn quad_to(&mut self, c: Point, p: Point) {
        let Some(from) = self.cur.last().map(|a| a.at) else {
            self.move_to(p);
            return;
        };
        self.cubic_to(from + (c - from) * (2.0 / 3.0), p + (c - p) * (2.0 / 3.0), p);
    }

    fn close(&mut self) {
        let start = self.cur.first().map(|a| a.at);
        self.flush(true);
        // A segment after a close starts where the last subpath did.
        if let Some(s) = start {
            self.cur.push(Anchor { at: s, inn: Vec2::ZERO, out: Vec2::ZERO });
        }
    }

    fn finish(mut self) -> Vec<(Vec<Anchor>, bool)> {
        self.flush(false);
        self.done
    }
}

/// Iron's own path data, in f64.
fn parse_d(d: &str) -> Vec<(Vec<Anchor>, bool)> {
    use svgtypes::SimplePathSegment as S;
    let mut c = Contours::new(true);
    for seg in svgtypes::SimplifyingPathParser::from(d) {
        let Ok(seg) = seg else { break };
        match seg {
            S::MoveTo { x, y } => c.move_to(Point::new(x, y)),
            S::LineTo { x, y } => c.line_to(Point::new(x, y)),
            S::CurveTo { x1, y1, x2, y2, x, y } => c.cubic_to(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y)),
            S::Quadratic { x1, y1, x, y } => c.quad_to(Point::new(x1, y1), Point::new(x, y)),
            S::ClosePath => c.close(),
        }
    }
    c.finish()
}

/// usvg's path data (f32), with the noise rounded off.
fn usvg_contours(p: &usvg::Path) -> Vec<(Vec<Anchor>, bool)> {
    use usvg::tiny_skia_path::PathSegment as S;
    let pt = |p: usvg::tiny_skia_path::Point| Point::new(round4(p.x as f64), round4(p.y as f64));
    let mut c = Contours::new(false);
    for seg in p.data().segments() {
        match seg {
            S::MoveTo(a) => c.move_to(pt(a)),
            S::LineTo(a) => c.line_to(pt(a)),
            S::QuadTo(a, b) => c.quad_to(pt(a), pt(b)),
            S::CubicTo(a, b, e) => c.cubic_to(pt(a), pt(b), pt(e)),
            S::Close => c.close(),
        }
    }
    c.finish()
}

fn point_node(a: &Anchor) -> INode {
    let mut t = Transform2d::default();
    (t.x, t.y) = (n(a.at.x), n(a.at.y));
    let mut h = Handles::default();
    (h.ix, h.iy, h.ox, h.oy) = (n(a.inn.x), n(a.inn.y), n(a.out.x), n(a.out.y));
    INode { ty: "point", comps: vec![t.wrap(), h.wrap()], kids: Vec::new() }
}

struct Ctx<'a, 'input> {
    iron: HashMap<&'a str, roxmltree::Node<'a, 'input>>,
    cuts: Cuts,
}

impl<'a, 'input> Ctx<'a, 'input> {
    fn iron_el(&self, id: &str) -> Option<roxmltree::Node<'a, 'input>> {
        (!id.is_empty()).then(|| self.iron.get(id).copied()).flatten()
    }

    /// Where a node sits: its transform, and what is left over for its
    /// content when `m` skews (baked into points, or into children).
    fn placement(&self, m: Affine, el: Option<roxmltree::Node>) -> ([f64; 5], Affine) {
        if let Some(el) = el
            && let Some(exact) = raw_trs(el)
            && same(m, trs_affine(exact))
        {
            return (exact, Affine::IDENTITY);
        }
        match decompose(m) {
            Some(t) => (t, Affine::IDENTITY),
            None => {
                let [.., e, f] = m.as_coeffs();
                ([e, f, 0.0, 1.0, 1.0], Affine::translate((-e, -f)) * m)
            }
        }
    }

    /// Whether every shape under `g` is one Iron wrote with a shadow: the
    /// group's filter is then Iron's own.
    fn iron_shadow_only(&self, g: &usvg::Group) -> bool {
        g.children().iter().all(|c| match c {
            usvg::Node::Group(g) => self.iron_shadow_only(g),
            usvg::Node::Path(p) => self.iron_el(p.id()).is_some_and(|el| el.attribute("data-iron-shadow").is_some()),
            _ => false,
        })
    }

    fn group_children(&mut self, g: &usvg::Group, accum: Affine, opacity: f64, out: &mut Vec<INode>) {
        for child in g.children() {
            match child {
                usvg::Node::Group(sub) => self.group(sub, accum, opacity, out),
                usvg::Node::Path(p) => self.path(p, accum, opacity, out),
                usvg::Node::Image(_) => self.cuts.add("images are not imported"),
                usvg::Node::Text(_) => self.cuts.add("text is not imported"),
            }
        }
    }

    fn group(&mut self, g: &usvg::Group, accum: Affine, opacity: f64, out: &mut Vec<INode>) {
        let m = accum * affine(g.transform());
        let el = self.iron_el(g.id());
        let iron_ty = el.and_then(|e| e.attribute("data-iron-type"));
        if g.mask().is_some() {
            self.cuts.add("masks are not imported");
        }
        if g.clip_path().is_some() && iron_ty != Some("embed") && !el.is_some_and(|e| e.attribute("data-iron-appearance").is_some()) {
            self.cuts.add("clip paths are not imported; what they hid shows");
        }
        if !g.filters().is_empty() && !self.iron_shadow_only(g) {
            self.cuts.add("filters are not imported");
        }
        if g.blend_mode() != usvg::BlendMode::Normal {
            self.cuts.add("blend modes are not imported");
        }
        let alpha = g.opacity().get() as f64;
        let opacity = opacity * alpha;
        if g.id().is_empty() && g.children().len() <= 1 {
            self.group_children(g, m, opacity, out);
            return;
        }
        match iron_ty {
            Some("embed") => self.cuts.add("an embed imports as a group of copies of what it showed"),
            Some("slider") => self.cuts.add("a slider imports as the shapes that picture it"),
            _ => {}
        }
        let (t, rest) = self.placement(m, el);
        let mut comps = vec![transform2d(t)];
        let label = match el {
            Some(el) => el.attribute("data-iron-name").map(str::to_owned),
            None => (!g.id().is_empty()).then(|| g.id().to_owned()),
        };
        comps.extend(label.as_deref().map(name));
        let extras = self.appearance(el);
        if !extras.iter().any(|c| c.kind() == iron_document::FacetKind::Appearance) && opacity != 1.0 {
            let mut appearance = Appearance::default();
            appearance.opacity = n(opacity);
            comps.push(appearance.wrap());
        }
        comps.extend(extras);
        let mut kids = Vec::new();
        self.group_children(g, rest, 1.0, &mut kids);
        out.push(INode { ty: "group", comps, kids });
    }

    fn appearance(&mut self, el: Option<roxmltree::Node<'_, '_>>) -> Vec<Facet> {
        let Some(raw) = el.and_then(|el| el.attribute("data-iron-appearance")) else { return Vec::new(); };
        match serde_json::from_str::<Vec<Facet>>(raw) {
            Ok(cs) => cs.into_iter().filter(|c| matches!(c.kind(), iron_document::FacetKind::Appearance | iron_document::FacetKind::Gradient | iron_document::FacetKind::GroupClip | iron_document::FacetKind::Fill)).collect(),
            Err(_) => { self.cuts.add("invalid Iron appearance metadata was ignored"); Vec::new() }
        }
    }

    /// Fill and stroke facets, and whether the fill is even-odd.
    fn paint(&mut self, p: &usvg::Path, opacity: f64, exact_gradient: bool) -> (Option<Facet>, Option<Facet>, bool) {
        let solid = |paint: &usvg::Paint, cuts: &mut Cuts| -> Option<(usvg::Color, f64)> {
            match paint {
                usvg::Paint::Color(c) => Some((*c, 1.0)),
                usvg::Paint::LinearGradient(g) => first_stop(g.stops(), cuts),
                usvg::Paint::RadialGradient(g) => first_stop(g.stops(), cuts),
                usvg::Paint::Pattern(_) => {
                    cuts.add("patterns are not imported");
                    None
                }
            }
        };
        let mut evenodd = false;
        let fill = p.fill().and_then(|f| {
            evenodd = f.rule() == usvg::FillRule::EvenOdd;
            let (c, a) = if exact_gradient {
                match f.paint() {
                    usvg::Paint::LinearGradient(g) => g.stops().first().map(|s| (s.color(), s.opacity().get() as f64)),
                    usvg::Paint::RadialGradient(g) => g.stops().first().map(|s| (s.color(), s.opacity().get() as f64)),
                    _ => solid(f.paint(), &mut self.cuts),
                }
            } else { solid(f.paint(), &mut self.cuts) }?;
            let mut fill = Fill::default();
            fill.color = Slot::Const(rgb(c, 1.0));
            fill.opacity = n(round4(a * f.opacity().get() as f64 * opacity));
            Some(fill.wrap())
        });
        let stroke = p.stroke().and_then(|s| {
            let (c, a) = solid(s.paint(), &mut self.cuts)?;
            let mut stroke = Stroke::default();
            stroke.color = Slot::Const(rgb(c, round4(a * s.opacity().get() as f64 * opacity)));
            stroke.width = n(round4(s.width().get() as f64));
            let word = |w: &str| Slot::Const(Value::Enum(w.to_owned()));
            stroke.join = word(match s.linejoin() {
                usvg::LineJoin::Round => "round",
                usvg::LineJoin::Bevel => "bevel",
                usvg::LineJoin::Miter => "miter",
                usvg::LineJoin::MiterClip => {
                    self.cuts.add("miter-clip joins are drawn as miter joins");
                    "miter"
                }
            });
            stroke.cap = word(match s.linecap() {
                usvg::LineCap::Round => "round",
                usvg::LineCap::Butt => "butt",
                usvg::LineCap::Square => "square",
            });
            stroke.miter = n(round4(s.miterlimit().get() as f64));
            if let Some(d) = s.dasharray() {
                stroke.dash = Slot::Const(Value::Str(d.iter().map(|v| crate::num(*v as f64)).collect::<Vec<_>>().join(" ")));
                stroke.dash_offset = n(round4(s.dashoffset() as f64));
            }
            Some(stroke.wrap())
        });
        (fill, stroke, evenodd)
    }

    fn path(&mut self, p: &usvg::Path, accum: Affine, opacity: f64, out: &mut Vec<INode>) {
        // Hidden, or with neither fill nor stroke: nothing is drawn.
        if !p.is_visible() {
            return;
        }
        let el = self.iron_el(p.id());
        let iron_ty = el.and_then(|e| e.attribute("data-iron-type"));
        // Layer opacity is separate from paint alpha. Anonymous wrappers
        // can fold into a single shape without changing isolation.
        let mut extra = self.appearance(el);
        let own_paint = extra.iter().find(|c| c.kind() == iron_document::FacetKind::Fill).cloned();
        extra.retain(|c| c.kind() != iron_document::FacetKind::Fill);
        let gradient = extra.iter().any(|c| c.kind() == iron_document::FacetKind::Gradient);
        let (mut fill, stroke, evenodd) = self.paint(p, 1.0, gradient);
        if gradient || own_paint.is_some() { fill = own_paint; }
        if !extra.iter().any(|c| c.kind() == iron_document::FacetKind::Appearance) && opacity != 1.0 {
            let mut appearance = Appearance::default();
            appearance.opacity = n(opacity);
            extra.push(appearance.wrap());
        }
        if let Some(el) = el
            && let Some(sh) = el.attribute("data-iron-shadow")
            && let [color, rest @ ..] = &sh.split_whitespace().collect::<Vec<_>>()[..]
            && let Ok(c) = svgtypes::Color::from_str(color)
        {
            let v: Vec<f64> = rest.iter().filter_map(|s| s.parse().ok()).collect();
            if let [a, dx, dy, blur] = v[..] {
                let mut s = Shadow::default();
                s.color = Slot::Const(Value::Color([c.red as f32 / 255.0, c.green as f32 / 255.0, c.blue as f32 / 255.0, a as f32]));
                (s.dx, s.dy, s.blur) = (n(dx), n(dy), n(blur));
                extra.push(s.wrap());
            }
        }
        let label = match el {
            Some(el) => el.attribute("data-iron-name").map(str::to_owned),
            None => (!p.id().is_empty()).then(|| p.id().to_owned()),
        };
        extra.extend(label.as_deref().map(name));
        let (t, rest) = self.placement(accum, el);
        let paint: Vec<Facet> = fill.iter().chain(stroke.iter()).cloned().collect();

        // Iron's rects and ellipses come back as themselves.
        if rest == Affine::IDENTITY
            && let Some(el) = el
        {
            let shape = match iron_ty {
                Some("rect") => {
                    let mut size = Size::default();
                    let w = attr_f64(el, "data-iron-w").or_else(|| attr_f64(el, "width")).unwrap_or(0.0);
                    let h = attr_f64(el, "data-iron-h").or_else(|| attr_f64(el, "height")).unwrap_or(0.0);
                    (size.w, size.h) = (n(w), n(h));
                    let radii = match attr_nums(el, "data-iron-corners").as_deref() {
                        Some(&[tl, tr, br, bl]) => [tl, tr, br, bl],
                        _ => [attr_f64(el, "rx").unwrap_or(0.0); 4],
                    };
                    let mut comps = vec![size.wrap()];
                    if radii.iter().any(|&r| r != 0.0) {
                        let mut c = Corners::default();
                        (c.tl, c.tr, c.br, c.bl) = (n(radii[0]), n(radii[1]), n(radii[2]), n(radii[3]));
                        comps.push(c.wrap());
                    }
                    Some(("rect", comps))
                }
                Some("ellipse") => {
                    let mut r = Radius::default();
                    let rx = attr_f64(el, "data-iron-rx").or_else(|| attr_f64(el, "rx")).unwrap_or(0.0);
                    let ry = attr_f64(el, "data-iron-ry").or_else(|| attr_f64(el, "ry")).unwrap_or(rx);
                    (r.rx, r.ry) = (n(rx), n(ry));
                    let mut comps = vec![r.wrap()];
                    if let Some(&[start, sweep, inner]) = attr_nums(el, "data-iron-arc").as_deref() {
                        let mut a = Arc::default();
                        (a.start, a.sweep, a.inner) = (n(start), n(sweep), n(inner));
                        comps.push(a.wrap());
                    }
                    Some(("ellipse", comps))
                }
                _ => None,
            };
            if let Some((ty, comps)) = shape {
                let mut all = vec![transform2d(t)];
                all.extend(comps);
                all.extend(paint);
                all.extend(extra);
                out.push(INode { ty, comps: all, kids: Vec::new() });
                return;
            }
        }

        // Everything else is a path, one per subpath. A rect or an ellipse
        // under a skew becomes one, from its exact outline.
        let mut contours = match (iron_ty, el) {
            (Some("path"), Some(el)) => parse_d(el.attribute("d").unwrap_or("")),
            (Some("rect" | "ellipse"), Some(el)) => {
                let outline = if iron_ty == Some("rect") {
                    let w = attr_f64(el, "data-iron-w").or_else(|| attr_f64(el, "width")).unwrap_or(0.0);
                    let h = attr_f64(el, "data-iron-h").or_else(|| attr_f64(el, "height")).unwrap_or(0.0);
                    let radii = match attr_nums(el, "data-iron-corners").as_deref() {
                        Some(&[tl, tr, br, bl]) => [tl, tr, br, bl],
                        _ => [attr_f64(el, "rx").unwrap_or(0.0); 4],
                    };
                    vec![shape::rect_outline(w, h, radii)]
                } else {
                    let rx = attr_f64(el, "data-iron-rx").or_else(|| attr_f64(el, "rx")).unwrap_or(0.0);
                    let ry = attr_f64(el, "data-iron-ry").or_else(|| attr_f64(el, "ry")).unwrap_or(rx);
                    let arc = attr_nums(el, "data-iron-arc").and_then(|v| <[f64; 3]>::try_from(v).ok());
                    shape::ellipse_outline(rx, ry, arc)
                };
                outline.into_iter().map(|c| (c, true)).collect()
            }
            _ => usvg_contours(p),
        };
        if rest != Affine::IDENTITY {
            let linear = Affine::new([rest.as_coeffs()[0], rest.as_coeffs()[1], rest.as_coeffs()[2], rest.as_coeffs()[3], 0.0, 0.0]);
            for (run, _) in &mut contours {
                let r = |p: Point| Point::new(round4(p.x), round4(p.y));
                for a in run.iter_mut() {
                    a.at = r(rest * a.at);
                    a.inn = r(linear * a.inn.to_point()).to_vec2();
                    a.out = r(linear * a.out.to_point()).to_vec2();
                }
            }
        }
        if contours.is_empty() {
            return;
        }
        // SVG fills an open subpath as if closed; Iron fills only closed
        // paths. Without a stroke, closing it looks the same.
        for (_, closed) in &mut contours {
            if !*closed && fill.is_some() {
                if stroke.is_none() {
                    *closed = true;
                } else {
                    self.cuts.add("an open path's fill is not drawn in Iron");
                }
            }
        }
        // A foreign outline that is a rect (corners rounded or not) or a full
        // ellipse, in its own axes, comes in as one: editable as Figma's are.
        if el.is_none()
            && rest == Affine::IDENTITY
            && let [(run, true)] = &contours[..]
            && let Some((ty, origin, comps)) = recognize(run)
            && let Some(placed) = decompose(trs_affine(t) * Affine::translate(origin.to_vec2()))
        {
            let mut all = vec![transform2d(placed)];
            all.extend(comps);
            all.extend(paint);
            all.extend(extra);
            out.push(INode { ty, comps: all, kids: Vec::new() });
            return;
        }
        // One path; several subpaths are its contours, holes and all.
        let mut ps = PathShape::default();
        let evenodd = match el {
            Some(el) => el.attribute("fill-rule") == Some("evenodd"),
            None => evenodd,
        };
        if evenodd && contours.len() > 1 {
            ps.fill_rule = Slot::Const(Value::Enum("evenodd".into()));
        }
        let mut comps = vec![transform2d(t), ps.wrap()];
        comps.extend(paint);
        comps.extend(extra);
        let kids = contours.iter().map(|(run, closed)| contour_node(run, *closed)).collect();
        out.push(INode { ty: "path", comps, kids });
    }
}

fn contour_node(run: &[Anchor], closed: bool) -> INode {
    let mut c = Contour::default();
    c.closed = Slot::Const(Value::Bool(closed));
    INode { ty: "contour", comps: vec![c.wrap()], kids: run.iter().map(point_node).collect() }
}

/// Whether every sampled point of `b` lies within `tol` of `a`.
fn near_all(a: &kurbo::BezPath, b: &kurbo::BezPath, tol: f64) -> bool {
    use kurbo::{ParamCurve, ParamCurveNearest};
    b.segments().all(|s| (0..=8).all(|i| a.segments().any(|q| q.nearest(s.eval(i as f64 / 8.0), 1e-9).distance_sq <= tol * tol)))
}

/// A closed outline as a rect or an ellipse, if it is one: its type, where
/// its origin goes in the outline's space (a rect's top-left corner, an
/// ellipse's centre), and its parameters. Each guess is drawn with Iron's own
/// outline and kept only if the two coincide, so a near miss stays a path.
fn recognize(run: &[Anchor]) -> Option<(&'static str, Point, Vec<Facet>)> {
    use kurbo::Shape as _;
    let given = shape::to_bez(&[run.to_vec()]);
    let bb = given.bounding_box();
    let (w, h) = (round4(bb.width()), round4(bb.height()));
    if w < 1e-3 || h < 1e-3 {
        return None;
    }
    let tol = (w.max(h) * 1e-3).max(1e-3);
    let same = |candidate: &kurbo::BezPath| near_all(&given, candidate, tol) && near_all(candidate, &given, tol);
    let (x0, y0, x1, y1) = (bb.x0, bb.y0, bb.x1, bb.y1);
    let near = |a: f64, b: f64| (a - b).abs() <= tol;

    // A full ellipse, centred in its bounds; first, so a circle is not read
    // as a rect rounded to half its size.
    if run.len() >= 4 {
        let (rx, ry) = (round4(w / 2.0), round4(h / 2.0));
        let centre = Point::new(round4((x0 + x1) / 2.0), round4((y0 + y1) / 2.0));
        let candidate = shape::to_bez(&shape::ellipse_outline(rx, ry, None));
        if same(&(Affine::translate(centre.to_vec2()) * candidate)) {
            let mut r = Radius::default();
            (r.rx, r.ry) = (n(rx), n(ry));
            return Some(("ellipse", centre, vec![r.wrap()]));
        }
    }
    // A rect: each corner's radius is how far the nearest anchor on its top
    // or bottom edge sits from the corner (zero for a sharp one).
    if (4..=8).contains(&run.len()) {
        let edge = |y: f64, from_left: bool| {
            run.iter().filter(|a| near(a.at.y, y)).map(|a| if from_left { a.at.x - x0 } else { x1 - a.at.x }).fold(f64::INFINITY, f64::min)
        };
        let radii = [edge(y0, true), edge(y0, false), edge(y1, false), edge(y1, true)].map(|r| if r < tol { 0.0 } else { round4(r) });
        if radii.iter().all(|r| r.is_finite()) {
            let candidate = shape::to_bez(&[shape::rect_outline(w, h, radii)]);
            if same(&(Affine::translate((x0, y0)) * candidate)) {
                let mut size = Size::default();
                (size.w, size.h) = (n(w), n(h));
                let mut comps = vec![size.wrap()];
                if radii.iter().any(|&r| r > 0.0) {
                    let mut c = Corners::default();
                    (c.tl, c.tr, c.br, c.bl) = (n(radii[0]), n(radii[1]), n(radii[2]), n(radii[3]));
                    comps.push(c.wrap());
                }
                return Some(("rect", Point::new(round4(x0), round4(y0)), comps));
            }
        }
    }
    None
}

fn first_stop(stops: &[usvg::Stop], cuts: &mut Cuts) -> Option<(usvg::Color, f64)> {
    cuts.add("gradients are imported as their first stop's colour");
    stops.first().map(|s| (s.color(), s.opacity().get() as f64))
}
